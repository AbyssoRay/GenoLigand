from __future__ import annotations

import csv
import json
import math
import os
import subprocess
import threading
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

from genoligand_workers.common import WorkerExecution, WorkerOutcome
from genoligand_workers.worker_io import parse_worker_args, read_job, write_result


MAX_PEPTIDE_RESIDUES = 80
PEPTIDE_CONFORMER_COUNT = 12
SIMULATION_SEED = 20260629
STANDARD_AMINO_ACIDS = set("ACDEFGHIKLMNPQRSTVWY")
DEFAULT_EQUILIBRATION_STEPS = 10000
DEFAULT_PRODUCTION_SEGMENTS = 50
DEFAULT_PRODUCTION_STEPS_PER_SEGMENT = 1000
DEFAULT_MINIMIZATION_ITERATIONS = 200
DEFAULT_TIMESTEP_PS = 0.002
DEFAULT_GPU_TELEMETRY_POLL_SECONDS = 1.0
GAS_CONSTANT_KJ_PER_MOL_K = 0.00831446261815324
MINIMUM_MD_PRODUCTION_SAMPLES = 10
RMSD_SCORE_SCALE_ANGSTROM = 4.0
LIGAND_DRIFT_SCORE_SCALE_ANGSTROM = 3.0
CONTACT_CUTOFF_ANGSTROM = 4.5
HBOND_DISTANCE_CUTOFF_NM = 0.25
HBOND_ANGLE_CUTOFF_DEGREES = 120.0


@dataclass
class ReceptorCoordinate:
    path: Path
    topology: Any
    atoms: list[dict[str, str]]
    positions: Any
    positions_angstrom: Any
    preparation: str
    risk_flags: list[str]


@dataclass
class GpuTelemetry:
    rows: list[dict[str, str]]
    available: bool = False
    failure_reason: str = ""
    lock: Any = field(default_factory=threading.Lock, repr=False)


def main() -> int:
    args = parse_worker_args()
    job = read_job(args.job)
    try:
        execution = run_md_summary_job(job)
    except Exception as exc:  # pragma: no cover - depends on local scientific runtime
        reason = safe_note(str(exc)) or "md_summary_worker_failed"
        execution = md_execution("failed", reason, f"MD summary worker failed: {reason}", True)
    write_result(
        args.out,
        job,
        execution.result_status,
        execution.engine_version,
        execution_status=execution.status,
        reason=execution.reason,
        outcomes=execution.outcome_dicts(),
    )
    return 0 if execution.status in {"completed", "not_applicable"} else 2


def md_execution(status: str, reason: str, version: str, required: bool, row_count: int = 0) -> WorkerExecution:
    return WorkerExecution(
        status,
        reason,
        version,
        (WorkerOutcome(status, reason, required, "md_summary", row_count),),
    )


def run_md_summary_job(job: dict[str, Any]) -> WorkerExecution:
    input_manifest = runtime_path(str(job["input_manifest"]))
    run_dir = runtime_path(str(job["run_dir"]))
    science_store = runtime_path(str(job.get("science_store", "")))
    smiles, input_id = read_input_conversion_manifest(run_dir, "small_molecule")
    if not smiles:
        smiles, input_id = read_small_molecule_input(input_manifest)
    sequence = ""
    if not smiles:
        sequence, input_id = read_input_conversion_manifest(run_dir, "peptide")
        if not sequence:
            sequence, input_id = read_peptide_sequence_input(input_manifest)
        if not sequence:
            reason = "no_applicable_md_input"
            write_md_not_ready(run_dir, reason, [])
            return md_execution("not_applicable", reason, "MD summary not executed", False)

    receptor_path = find_receptor_coordinate(run_dir) if smiles else None
    if smiles and receptor_path is None:
        reason = "missing_receptor_coordinate"
        write_md_not_ready(run_dir, reason, [reason])
        return md_execution("not_ready", reason, "OpenMM not executed", True)

    import numpy as np
    import openmm
    from openmm import unit
    from rdkit import Chem, rdBase
    from rdkit.Chem import AllChem

    if sequence:
        try:
            mol, frames, parameterization, parameter_flags = prepare_peptide_conformer_ensemble(
                sequence,
                Chem,
                AllChem,
                np,
            )
        except Exception as exc:  # noqa: BLE001 - not-ready artifact is the contract for unusable sequence inputs
            reason = "peptide_conformer_calculation_failed"
            write_md_not_ready(
                run_dir,
                f"peptide conformer calculation failed: {safe_note(str(exc))}",
                [reason],
            )
            return md_execution("not_ready", reason, openmm_version_text(openmm, "not_ready", reason), True)
        candidate_id = top_candidate_id(run_dir) or input_id
        risk_flags = [
            *parameter_flags,
            "no_receptor_contact_model",
            "rdkit_peptide_conformer_ensemble",
        ]
        metrics = summarize_peptide_conformer_frames(mol, frames, np)
        write_peptide_conformer_artifacts(
            run_dir,
            candidate_id,
            input_id,
            sequence,
            metrics,
            frames,
            mol,
            parameterization,
            risk_flags,
            rdBase.rdkitVersion,
        )
        return md_execution(
            "completed",
            "peptide_conformer_completed",
            f"RDKit {rdBase.rdkitVersion}; method=rdkit_peptide_conformer_ensemble; input_kind=peptide",
            True,
            1,
        )

    try:
        assert receptor_path is not None
        receptor = load_receptor_coordinate(receptor_path, unit)
    except Exception as exc:  # noqa: BLE001 - not-ready artifact is the contract for unusable coordinates
        reason = "receptor_coordinate_preparation_failed"
        write_md_not_ready(
            run_dir,
            f"receptor coordinate preparation failed: {safe_note(str(exc))}",
            [reason],
        )
        return md_execution("not_ready", reason, openmm_version_text(openmm, "not_ready", reason), True)
    if not receptor.atoms:
        reason = "receptor_coordinate_parse_failed"
        write_md_not_ready(run_dir, reason, [reason])
        return md_execution("not_ready", reason, openmm_version_text(openmm, "not_ready", reason), True)

    mol = read_docking_pose_ligand(run_dir, Chem)
    if mol is None:
        reason = "missing_docking_pose_sdf"
        write_md_not_ready(
            run_dir,
            "missing AutoDock Vina docking pose SDF for complex MD",
            [reason],
        )
        return md_execution("not_ready", reason, openmm_version_text(openmm, "not_ready", reason), True)

    timestep_ps = env_float("GENOLIGAND_MD_TIMESTEP_PS", DEFAULT_TIMESTEP_PS)
    equilibration_steps = env_int("GENOLIGAND_MD_EQUILIBRATION_STEPS", DEFAULT_EQUILIBRATION_STEPS)
    production_segments = env_int("GENOLIGAND_MD_PRODUCTION_SEGMENTS", DEFAULT_PRODUCTION_SEGMENTS)
    production_steps_per_segment = env_int(
        "GENOLIGAND_MD_PRODUCTION_STEPS_PER_SEGMENT",
        DEFAULT_PRODUCTION_STEPS_PER_SEGMENT,
    )
    telemetry_poll_seconds = env_float("GENOLIGAND_GPU_TELEMETRY_POLL_SECONDS", DEFAULT_GPU_TELEMETRY_POLL_SECONDS)
    production_steps_total = production_segments * production_steps_per_segment
    sampling_reason = md_sampling_rejection_reason(production_steps_total, production_segments + 1)
    if sampling_reason:
        write_md_not_ready(run_dir, sampling_reason, [sampling_reason])
        return md_execution(
            "not_ready",
            sampling_reason,
            openmm_version_text(openmm, "not_ready", sampling_reason),
            True,
        )

    conformer = mol.GetConformer()
    initial_angstrom = np.array(
        [
            [
                conformer.GetAtomPosition(index).x,
                conformer.GetAtomPosition(index).y,
                conformer.GetAtomPosition(index).z,
            ]
            for index in range(mol.GetNumAtoms())
        ],
        dtype=float,
    )
    ligand_positions = initial_angstrom
    off_mol, parameterization, parameter_flags = prepare_openff_ligand(mol)
    platform_name = select_openmm_platform(openmm)
    platform = openmm.Platform.getPlatformByName(platform_name)
    platform_properties = openmm_platform_properties(platform_name, platform)
    gpu_telemetry = GpuTelemetry(rows=[])
    sample_gpu_telemetry(gpu_telemetry, "baseline_before_openmm_context")
    system, role_ranges, context_positions, model_metadata = build_complex_system(
        openmm,
        unit,
        mol,
        off_mol,
        receptor,
        ligand_positions,
        science_store,
    )
    integrator = openmm.LangevinMiddleIntegrator(
        300 * unit.kelvin,
        1 / unit.picosecond,
        timestep_ps * unit.picoseconds,
    )
    integrator.setRandomNumberSeed(SIMULATION_SEED)
    if platform_properties:
        context = openmm.Context(system, integrator, platform, platform_properties)
    else:
        context = openmm.Context(system, integrator, platform)
    sample_gpu_telemetry(gpu_telemetry, "after_openmm_context")
    context.setPositions(context_positions)
    sample_gpu_telemetry(gpu_telemetry, "after_positions_set")
    pre_minimization_energy = potential_energy_kj_mol(context, unit)
    openmm.LocalEnergyMinimizer.minimize(
        context,
        tolerance=10 * unit.kilojoule_per_mole / unit.nanometer,
        maxIterations=env_int("GENOLIGAND_MD_MINIMIZATION_ITERATIONS", DEFAULT_MINIMIZATION_ITERATIONS),
    )
    post_minimization_energy = potential_energy_kj_mol(context, unit)
    sample_gpu_telemetry(gpu_telemetry, "after_minimization")
    context.setVelocitiesToTemperature(300 * unit.kelvin, SIMULATION_SEED)
    total_step = 0
    candidate_id = top_candidate_id(run_dir) or input_id
    if equilibration_steps > 0:
        with GpuTelemetryMonitor(
            gpu_telemetry,
            enabled=platform_name == "CUDA",
            label_prefix="equilibration_poll",
            interval_seconds=telemetry_poll_seconds,
        ):
            integrator.step(equilibration_steps)
        total_step += equilibration_steps
    sample_gpu_telemetry(gpu_telemetry, "after_equilibration")

    frames: list[Any] = []
    box_vectors: list[Any | None] = []
    state_rows: list[list[str]] = []
    frame, state_row, periodic_box = current_state_snapshot(
        context,
        unit,
        candidate_id=candidate_id,
        input_id=input_id,
        frame_index=0,
        step=total_step,
        time_ps=total_step * timestep_ps,
        particle_count=system.getNumParticles(),
        stage="post_equilibration",
        periodic=model_metadata["nonbonded_method"] == "PME",
    )
    frames.append(frame)
    box_vectors.append(periodic_box)
    state_rows.append(state_row)
    with GpuTelemetryMonitor(
        gpu_telemetry,
        enabled=platform_name == "CUDA" and production_steps_total > 0,
        label_prefix="production_poll",
        interval_seconds=telemetry_poll_seconds,
    ):
        for segment in range(production_segments):
            integrator.step(production_steps_per_segment)
            total_step += production_steps_per_segment
            frame, state_row, periodic_box = current_state_snapshot(
                context,
                unit,
                candidate_id=candidate_id,
                input_id=input_id,
                frame_index=segment + 1,
                step=total_step,
                time_ps=total_step * timestep_ps,
                particle_count=system.getNumParticles(),
                stage=f"production_segment_{segment + 1}",
                periodic=model_metadata["nonbonded_method"] == "PME",
            )
            frames.append(frame)
            box_vectors.append(periodic_box)
            state_rows.append(state_row)
            sample_gpu_telemetry(gpu_telemetry, f"production_segment_{segment + 1}")
    sample_gpu_telemetry(gpu_telemetry, "after_production")
    del context
    del integrator

    gpu_status = summarize_gpu_telemetry(platform_name, gpu_telemetry)
    protocol_rows = build_md_protocol_rows(
        candidate_id,
        input_id,
        platform_name,
        platform_properties,
        receptor,
        mol,
        parameterization,
        "autodock_vina_pose_sdf",
        pre_minimization_energy,
        post_minimization_energy,
        equilibration_steps,
        production_segments,
        production_steps_per_segment,
        timestep_ps,
        len(frames),
        len(state_rows),
        gpu_status,
        model_metadata,
    )
    risk_flags = [
        *receptor.risk_flags,
        *parameter_flags,
        "ligand_placement_from_autodock_vina_pose",
        "standard_openmm_forcefield_complex",
    ]
    metrics = summarize_complex_frames(
        mol,
        receptor,
        frames,
        role_ranges,
        box_vectors=box_vectors,
        production_steps_total=production_steps_total,
    )
    write_md_artifacts(
        run_dir,
        candidate_id,
        input_id,
        smiles,
        platform_name,
        metrics,
        frames,
        state_rows,
        mol,
        ligand_positions,
        receptor,
        role_ranges,
        parameterization,
        risk_flags,
        "autodock_vina_pose_sdf",
        platform_properties,
        protocol_rows,
        gpu_telemetry,
        gpu_status,
        model_metadata,
    )
    return md_execution(
        "completed",
        "complex_md_completed",
        f"OpenMM {openmm.version.full_version}; platform={platform_name}; "
        "method=openmm_receptor_ligand_complex_dynamics; "
        f"gpu_observed={gpu_status['gpu_observed']}; receptor={receptor.path.name}",
        True,
        1,
    )


def runtime_path(value: str) -> Path:
    path = Path(value)
    if path.exists():
        return path
    if len(value) >= 3 and value[1] == ":" and value[2] in "\\/":
        drive = value[0].lower()
        rest = value[3:].replace("\\", "/")
        return Path(f"/mnt/{drive}/{rest}")
    return path


def read_small_molecule_input(path: Path) -> tuple[str, str]:
    with path.open("r", encoding="utf-8-sig") as handle:
        payload = json.load(handle)
    for index, row in enumerate(payload.get("inputs", []), start=1):
        field = str(row.get("field", "")).lower()
        label = str(row.get("label", "")).lower()
        value = str(row.get("value", "")).strip()
        explicit_small_molecule = field_mentions_small_molecule(field, label)
        if not value or not explicit_small_molecule:
            continue
        if looks_like_inchi(value) or looks_like_smiles_text(value):
            return value, f"input_{index}"
        if normalize_peptide_sequence(value):
            continue
        return value, f"input_{index}"
    return "", "input_1"


def read_input_conversion_manifest(run_dir: Path, canonical_kind: str) -> tuple[str, str]:
    path = run_dir / "input_conversion_manifest.tsv"
    if not path.exists():
        return "", "input_1"
    with path.open("r", encoding="utf-8-sig", newline="") as handle:
        reader = csv.DictReader(handle, delimiter="\t")
        for row in reader:
            if str(row.get("conversion_status", "")).strip() != "converted":
                continue
            if str(row.get("canonical_kind", "")).strip() != canonical_kind:
                continue
            value = decode_tsv_cell(str(row.get("canonical_value", ""))).strip()
            input_id = str(row.get("input_id", "input_1")).strip() or "input_1"
            if value:
                return value, input_id
    return "", "input_1"


def read_peptide_sequence_input(path: Path) -> tuple[str, str]:
    with path.open("r", encoding="utf-8-sig") as handle:
        payload = json.load(handle)
    for index, row in enumerate(payload.get("inputs", []), start=1):
        field = str(row.get("field", "")).lower()
        label = str(row.get("label", "")).lower()
        raw_value = str(row.get("value", ""))
        value = normalize_peptide_sequence(raw_value)
        explicit_small_molecule = field_mentions_small_molecule(field, label)
        if value and explicit_small_molecule and (looks_like_inchi(raw_value) or looks_like_smiles_text(raw_value)):
            continue
        if value:
            return value, f"input_{index}"
    return "", "input_1"


def decode_tsv_cell(value: str) -> str:
    out = []
    index = 0
    while index < len(value):
        ch = value[index]
        if ch != "\\" or index + 1 >= len(value):
            out.append(ch)
            index += 1
            continue
        marker = value[index + 1]
        if marker == "n":
            out.append("\n")
        elif marker == "r":
            out.append("\r")
        elif marker == "t":
            out.append("\t")
        elif marker == "\\":
            out.append("\\")
        else:
            out.append(ch)
            out.append(marker)
        index += 2
    return "".join(out)


def normalize_peptide_sequence(value: str) -> str:
    lines = []
    for raw_line in value.replace("\r", "\n").split("\n"):
        line = raw_line.strip()
        if line and not line.startswith(">"):
            lines.append(line)
    sequence = "".join(lines).replace(" ", "").replace("-", "").upper()
    if not sequence or len(sequence) > MAX_PEPTIDE_RESIDUES:
        return ""
    if any(ch not in STANDARD_AMINO_ACIDS for ch in sequence):
        return ""
    return sequence


def looks_like_inchi(value: str) -> bool:
    return value.strip().startswith("InChI=")


def field_mentions_small_molecule(field: str, label: str) -> bool:
    text = f"{field} {label}"
    return "small_molecule" in text or "smiles" in text or ("ligand" in text and "protein_ligand" not in field)


def looks_like_smiles_text(value: str) -> bool:
    text = value.strip()
    if not text or any(ch.isspace() for ch in text):
        return False
    allowed_symbols = set("[]()=#@+-\\/%.:")
    if any(not (ch.isascii() and ch.isalnum()) and ch not in allowed_symbols for ch in text):
        return False

    saw_atom = False
    index = 0
    while index < len(text):
        ch = text[index]
        if ch.isupper():
            symbol = ch
            if index + 1 < len(text) and text[index + 1].islower():
                symbol += text[index + 1]
                if valid_element_symbol(symbol):
                    saw_atom = True
                    index += 2
                    continue
                return False
            if valid_element_symbol(symbol):
                saw_atom = True
                index += 1
                continue
            return False
        if ch in "bcnops":
            saw_atom = True
            index += 1
            continue
        if ch.islower():
            return False
        index += 1
    return saw_atom


def valid_element_symbol(symbol: str) -> bool:
    return symbol in {
        "B",
        "C",
        "N",
        "O",
        "P",
        "S",
        "F",
        "I",
        "H",
        "Cl",
        "Br",
        "Na",
        "K",
        "Li",
        "Mg",
        "Ca",
        "Zn",
        "Fe",
    }


def top_candidate_id(run_dir: Path) -> str:
    selected = run_dir / "complex_candidate_selection.tsv"
    if selected.exists():
        with selected.open("r", encoding="utf-8-sig", newline="") as handle:
            reader = csv.DictReader(handle, delimiter="\t")
            for row in reader:
                value = str(row.get("candidate_id", "")).strip()
                if value:
                    return value
    path = run_dir / "target_candidate_ranking.tsv"
    if not path.exists():
        return ""
    with path.open("r", encoding="utf-8-sig", newline="") as handle:
        reader = csv.DictReader(handle, delimiter="\t")
        for row in reader:
            value = str(row.get("candidate_id", "")).strip()
            if value:
                return value
    return ""


def read_docking_pose_ligand(run_dir: Path, Chem: Any) -> Any | None:
    pose_path = run_dir / "tool_outputs" / "docking_pose.sdf"
    ligand_path = run_dir / "tool_outputs" / "docking_ligand.sdf"
    ligand_pdbqt = run_dir / "tool_outputs" / "docking_ligand.pdbqt"
    pose_pdbqt = run_dir / "tool_outputs" / "docking_pose.pdbqt"
    if not pose_path.exists() or pose_path.stat().st_size == 0:
        return None
    ligand_mol = None
    if ligand_path.exists() and ligand_path.stat().st_size > 0:
        ligand_supplier = Chem.SDMolSupplier(str(ligand_path), removeHs=False)
        ligand_mol = next((item for item in ligand_supplier if item is not None), None)
        if ligand_mol is not None:
            placed = ligand_with_vina_pose_coordinates(ligand_mol, ligand_pdbqt, pose_pdbqt, Chem)
            if placed is not None:
                return placed
    pose_supplier = Chem.SDMolSupplier(str(pose_path), sanitize=False, removeHs=False)
    pose_mol = next((item for item in pose_supplier if item is not None), None)
    if pose_mol is None or pose_mol.GetNumConformers() == 0:
        return None
    if ligand_mol is not None and ligand_mol.GetNumAtoms() == pose_mol.GetNumAtoms():
        conformer = Chem.Conformer(ligand_mol.GetNumAtoms())
        pose_conformer = pose_mol.GetConformer()
        for atom_index in range(ligand_mol.GetNumAtoms()):
            conformer.SetAtomPosition(atom_index, pose_conformer.GetAtomPosition(atom_index))
        ligand_mol.RemoveAllConformers()
        ligand_mol.AddConformer(conformer, assignId=True)
        return ligand_mol
    sanitized = Chem.Mol(pose_mol)
    try:
        Chem.SanitizeMol(sanitized)
    except Exception:
        return None
    return sanitized


def ligand_with_vina_pose_coordinates(ligand_mol: Any, ligand_pdbqt: Path, pose_pdbqt: Path, Chem: Any) -> Any | None:
    if not ligand_pdbqt.exists() or not pose_pdbqt.exists():
        return None
    original_atoms = read_pdbqt_atoms(ligand_pdbqt, first_model_only=False)
    pose_atoms = read_pdbqt_atoms(pose_pdbqt, first_model_only=True)
    if not original_atoms or len(original_atoms) != len(pose_atoms):
        return None
    template = Chem.RemoveHs(Chem.Mol(ligand_mol))
    if template.GetNumConformers() == 0:
        return None
    template_conf = template.GetConformer()
    assignments: dict[int, tuple[float, float, float]] = {}
    used: set[int] = set()
    for original, pose in zip(original_atoms, pose_atoms, strict=False):
        symbol = str(original["symbol"]).upper()
        if symbol == "H" or symbol != str(pose["symbol"]).upper():
            continue
        atom_index = nearest_template_atom(template, template_conf, symbol, original["coord"], used)
        if atom_index is None:
            return None
        used.add(atom_index)
        assignments[atom_index] = pose["coord"]
    if len(assignments) != template.GetNumAtoms():
        return None
    conformer = Chem.Conformer(template.GetNumAtoms())
    for atom_index in range(template.GetNumAtoms()):
        coord = assignments[atom_index]
        conformer.SetAtomPosition(atom_index, coord)
    template.RemoveAllConformers()
    template.AddConformer(conformer, assignId=True)
    placed = Chem.AddHs(template, addCoords=True)
    Chem.SanitizeMol(placed)
    return placed


def nearest_template_atom(template: Any, conformer: Any, symbol: str, coord: tuple[float, float, float], used: set[int]) -> int | None:
    best_index = None
    best_distance = 0.25
    for atom in template.GetAtoms():
        index = atom.GetIdx()
        if index in used or atom.GetSymbol().upper() != symbol:
            continue
        pos = conformer.GetAtomPosition(index)
        distance = ((pos.x - coord[0]) ** 2 + (pos.y - coord[1]) ** 2 + (pos.z - coord[2]) ** 2) ** 0.5
        if distance < best_distance:
            best_distance = distance
            best_index = index
    return best_index


def read_pdbqt_atoms(path: Path, first_model_only: bool) -> list[dict[str, Any]]:
    atoms: list[dict[str, Any]] = []
    in_model = not first_model_only
    seen_model = False
    with path.open("r", encoding="utf-8", errors="replace") as handle:
        for line in handle:
            record = line[:6].strip()
            if first_model_only and record == "MODEL":
                if seen_model:
                    break
                seen_model = True
                in_model = True
                continue
            if first_model_only and record == "ENDMDL" and seen_model:
                break
            if record not in {"ATOM", "HETATM"} or not in_model:
                continue
            try:
                atom_name = line[12:16].strip()
                symbol = "".join(ch for ch in atom_name if ch.isalpha())[:1].upper()
                coord = (float(line[30:38]), float(line[38:46]), float(line[46:54]))
            except ValueError:
                continue
            if symbol:
                atoms.append({"symbol": symbol, "coord": coord})
    return atoms


def prepare_peptide_conformer_ensemble(sequence: str, Chem: Any, AllChem: Any, np: Any) -> tuple[Any, list[Any], str, list[str]]:
    base_mol = Chem.MolFromFASTA(sequence)
    if base_mol is None:
        base_mol = Chem.MolFromSequence(sequence)
    if base_mol is None:
        raise ValueError("RDKit peptide conversion failed")
    mol = Chem.AddHs(base_mol)
    params = AllChem.ETKDGv3()
    params.randomSeed = SIMULATION_SEED
    params.pruneRmsThresh = 0.15
    conformer_ids = list(
        AllChem.EmbedMultipleConfs(
            mol,
            numConfs=max(3, min(PEPTIDE_CONFORMER_COUNT, len(sequence) * 3)),
            params=params,
        )
    )
    if not conformer_ids:
        if AllChem.EmbedMolecule(mol, params) != 0:
            raise ValueError("RDKit peptide conformer embedding failed")
        conformer_ids = [mol.GetConformer().GetId()]

    flags = []
    if AllChem.MMFFHasAllMoleculeParams(mol):
        parameterization = "rdkit_mmff94"
        try:
            results = AllChem.MMFFOptimizeMoleculeConfs(mol, numThreads=0, maxIters=300)
        except Exception as exc:  # noqa: BLE001 - UFF fallback keeps calculation available
            flags.append(f"mmff_optimization_failed:{safe_note(str(exc))}")
            parameterization = "rdkit_uff"
            results = AllChem.UFFOptimizeMoleculeConfs(mol, numThreads=0, maxIters=300)
    else:
        parameterization = "rdkit_uff"
        flags.append("mmff_parameters_unavailable")
        results = AllChem.UFFOptimizeMoleculeConfs(mol, numThreads=0, maxIters=300)

    energies = [float(result[1]) for result in results] if results else [0.0 for _ in conformer_ids]
    ordered = sorted(zip(conformer_ids, energies), key=lambda item: item[1])
    frames = [conformer_positions(mol, conf_id, np) for conf_id, _ in ordered]
    if not frames:
        raise ValueError("RDKit peptide conformer coordinates missing")
    return mol, frames, parameterization, flags


def conformer_positions(mol: Any, conformer_id: int, np: Any) -> Any:
    conformer = mol.GetConformer(conformer_id)
    return np.array(
        [
            [
                conformer.GetAtomPosition(index).x,
                conformer.GetAtomPosition(index).y,
                conformer.GetAtomPosition(index).z,
            ]
            for index in range(mol.GetNumAtoms())
        ],
        dtype=float,
    )


def find_receptor_coordinate(run_dir: Path) -> Path | None:
    for rel in [
        "structure_viewer.pdb",
        "structure_viewer.cif",
        "structure_viewer.mmcif",
        "tool_outputs/structure_viewer.pdb",
        "tool_outputs/structure_viewer.cif",
        "tool_outputs/structure_viewer.mmcif",
    ]:
        path = run_dir / rel
        if path.exists() and path.stat().st_size > 0:
            return path
    return None


def load_receptor_coordinate(path: Path, unit: Any) -> ReceptorCoordinate:
    try:
        return load_receptor_with_pdbfixer(path, unit)
    except Exception as pdbfixer_error:  # noqa: BLE001 - fallback keeps real coordinate provenance
        receptor = load_receptor_with_openmm_app(path, unit)
        receptor.risk_flags.append(f"pdbfixer_failed:{safe_note(str(pdbfixer_error))}")
        return receptor


def load_receptor_with_pdbfixer(path: Path, unit: Any) -> ReceptorCoordinate:
    import numpy as np
    from openmm import app
    from pdbfixer import PDBFixer

    fixer = PDBFixer(filename=str(path))
    fixer.findMissingResidues()
    fixer.findNonstandardResidues()
    fixer.replaceNonstandardResidues()
    fixer.removeHeterogens(keepWater=False)
    fixer.findMissingAtoms()
    fixer.addMissingAtoms()
    fixer.addMissingHydrogens(7.0)
    atoms, positions = atoms_from_topology(fixer.topology, fixer.positions, unit)
    prepared = path.parent / "tool_outputs" / "receptor_prepared.pdb"
    prepared.parent.mkdir(parents=True, exist_ok=True)
    with prepared.open("w", encoding="utf-8") as handle:
        app.PDBFile.writeFile(fixer.topology, fixer.positions, handle)
    return ReceptorCoordinate(
        path=path,
        topology=fixer.topology,
        atoms=atoms,
        positions=fixer.positions,
        positions_angstrom=np.asarray(positions, dtype=float),
        preparation="pdbfixer_prepared_receptor",
        risk_flags=[],
    )


def load_receptor_with_openmm_app(path: Path, unit: Any) -> ReceptorCoordinate:
    import numpy as np
    from openmm import app

    lower = path.name.lower()
    if lower.endswith(".pdb") or lower.endswith(".ent"):
        structure = app.PDBFile(str(path))
    else:
        structure = app.PDBxFile(str(path))
    modeller = app.Modeller(structure.topology, structure.positions)
    waters = [residue for residue in modeller.topology.residues() if residue.name.upper() in {"HOH", "WAT", "DOD"}]
    if waters:
        modeller.delete(waters)
    atoms, positions = atoms_from_topology(modeller.topology, modeller.positions, unit)
    return ReceptorCoordinate(
        path=path,
        topology=modeller.topology,
        atoms=atoms,
        positions=modeller.positions,
        positions_angstrom=np.asarray(positions, dtype=float),
        preparation="openmm_coordinate_parse",
        risk_flags=["pdbfixer_not_applied"],
    )


def atoms_from_topology(topology: Any, positions: Any, unit: Any) -> tuple[list[dict[str, str]], list[list[float]]]:
    coords = positions.value_in_unit(unit.angstrom)
    atoms: list[dict[str, str]] = []
    out_positions: list[list[float]] = []
    for index, atom in enumerate(topology.atoms()):
        element = getattr(getattr(atom, "element", None), "symbol", "") or "C"
        residue = getattr(atom.residue, "name", "")
        if residue.upper() in {"HOH", "WAT", "DOD"}:
            continue
        chain = getattr(getattr(atom, "residue", None), "chain", None)
        atoms.append(
            {
                "element": element,
                "name": getattr(atom, "name", f"atom_{index}"),
                "residue": residue,
                "residue_id": str(getattr(atom.residue, "id", "")),
                "chain": str(getattr(chain, "id", "")),
            }
        )
        pos = coords[index]
        out_positions.append([float(pos[0]), float(pos[1]), float(pos[2])])
    return atoms, out_positions


def prepare_openff_ligand(mol: Any) -> tuple[Any, str, list[str]]:
    try:
        from openff.toolkit import Molecule

        off_mol = Molecule.from_rdkit(
            mol,
            allow_undefined_stereo=True,
            hydrogens_are_explicit=True,
        )
        off_mol.name = "LIG"
        if not off_mol.n_atoms:
            raise ValueError("OpenFF molecule has zero atoms")
        try:
            off_mol.assign_partial_charges(partial_charge_method="mmff94")
            charge_method = "rdkit_mmff94_partial_charges"
            flags: list[str] = []
        except Exception as exc:  # noqa: BLE001 - Gasteiger is the installed-toolkit fallback
            off_mol.assign_partial_charges(partial_charge_method="gasteiger")
            charge_method = "rdkit_gasteiger_partial_charges"
            flags = [f"mmff94_partial_charge_failed:{safe_note(str(exc))}"]
        return off_mol, f"openff_smirnoff_template;charge_method={charge_method}", flags
    except Exception as exc:  # noqa: BLE001 - not-ready artifact reports exact parameterization boundary
        raise ValueError(f"OpenFF ligand parameterization failed: {safe_note(str(exc))}") from exc


def select_openmm_platform(openmm: Any) -> str:
    names = [
        openmm.Platform.getPlatform(index).getName()
        for index in range(openmm.Platform.getNumPlatforms())
    ]
    for preferred in ["CUDA", "CPU", "Reference"]:
        if preferred in names:
            return preferred
    if not names:
        raise ValueError("OpenMM has no available platform")
    return names[0]


def openmm_platform_properties(platform_name: str, platform: Any) -> dict[str, str]:
    if hasattr(platform, "getPropertyNames"):
        property_names = set(platform.getPropertyNames())
    else:
        property_names = {
            platform.getPropertyName(index)
            for index in range(platform.getNumPropertyNames())
        }
    properties: dict[str, str] = {}
    if platform_name == "CUDA":
        precision = os.environ.get("GENOLIGAND_OPENMM_PRECISION", "mixed").strip() or "mixed"
        if "Precision" in property_names:
            properties["Precision"] = precision
        elif "CudaPrecision" in property_names:
            properties["CudaPrecision"] = precision
        device_index = os.environ.get("GENOLIGAND_OPENMM_DEVICE_INDEX", "").strip()
        if device_index:
            if "DeviceIndex" in property_names:
                properties["DeviceIndex"] = device_index
            elif "CudaDeviceIndex" in property_names:
                properties["CudaDeviceIndex"] = device_index
    return properties


def env_int(name: str, default: int) -> int:
    raw = os.environ.get(name, "").strip()
    if not raw:
        return default
    try:
        value = int(raw)
    except ValueError:
        return default
    return max(0, value)


def env_float(name: str, default: float) -> float:
    raw = os.environ.get(name, "").strip()
    if not raw:
        return default
    try:
        value = float(raw)
    except ValueError:
        return default
    return max(0.0, value)


def build_complex_system(
    openmm: Any,
    unit: Any,
    mol: Any,
    off_mol: Any,
    receptor: ReceptorCoordinate,
    ligand_positions: Any,
    science_store: Path,
) -> tuple[Any, dict[str, range], Any, dict[str, Any]]:
    import numpy as np
    from openmm import Vec3, app
    from openmmforcefields.generators import SMIRNOFFTemplateGenerator

    ligand_positions_nm = unit.Quantity(
        [Vec3(float(coord[0]) / 10.0, float(coord[1]) / 10.0, float(coord[2]) / 10.0) for coord in ligand_positions],
        unit.nanometer,
    )
    forcefield_name = openff_forcefield_source(science_store)
    protein_forcefield = os.environ.get("GENOLIGAND_OPENMM_PROTEIN_FORCEFIELD", "amber14/protein.ff14SB.xml").strip()
    water_forcefield = os.environ.get("GENOLIGAND_OPENMM_WATER_FORCEFIELD", "amber14/tip3p.xml").strip()
    forcefield = app.ForceField(protein_forcefield, water_forcefield)
    template = SMIRNOFFTemplateGenerator(molecules=[off_mol], forcefield=forcefield_name)
    forcefield.registerTemplateGenerator(template.generator)
    modeller = app.Modeller(receptor.topology, receptor.positions)
    ligand_topology = off_mol.to_topology().to_openmm()
    modeller.add(ligand_topology, ligand_positions_nm)

    receptor_range = range(0, len(receptor.atoms))
    ligand_range = range(len(receptor.atoms), len(receptor.atoms) + mol.GetNumAtoms())
    solvent_mode = os.environ.get("GENOLIGAND_MD_SOLVENT_MODE", "explicit").strip().lower() or "explicit"
    nonbonded_method = app.NoCutoff
    solvent_atom_count = 0
    if solvent_mode == "explicit":
        padding_nm = env_float("GENOLIGAND_MD_SOLVENT_PADDING_NM", 0.9)
        ionic_strength = env_float("GENOLIGAND_MD_IONIC_STRENGTH_MOLAR", 0.15)
        modeller.addSolvent(
            forcefield,
            model="tip3p",
            padding=padding_nm * unit.nanometer,
            ionicStrength=ionic_strength * unit.molar,
        )
        nonbonded_method = app.PME
        solvent_atom_count = max(0, len(list(modeller.topology.atoms())) - len(receptor.atoms) - mol.GetNumAtoms())
    system = forcefield.createSystem(
        modeller.topology,
        nonbondedMethod=nonbonded_method,
        constraints=app.HBonds,
        rigidWater=True,
        removeCMMotion=True,
    )
    metadata = {
        "protein_forcefield": protein_forcefield,
        "water_forcefield": water_forcefield,
        "small_molecule_forcefield": forcefield_name,
        "solvent_mode": solvent_mode,
        "solvent_atom_count": solvent_atom_count,
        "total_atom_count": system.getNumParticles(),
        "nonbonded_method": "PME" if nonbonded_method == app.PME else "NoCutoff",
        "ligand_atom_count": mol.GetNumAtoms(),
        "receptor_atom_count": len(receptor.atoms),
    }
    if system.getNumParticles() != len(list(modeller.topology.atoms())):
        raise ValueError("OpenMM system particle count does not match topology atom count")
    return system, {"receptor": receptor_range, "ligand": ligand_range}, modeller.positions, metadata


def openff_forcefield_source(science_store: Path) -> str:
    explicit = os.environ.get("GENOLIGAND_OPENFF_FORCEFIELD", "").strip()
    if explicit:
        return explicit
    local = science_store / "indexes" / "forcefields" / "openff-2.2.1.offxml"
    if local.exists() and local.stat().st_size > 0:
        return str(local)
    return "openff-2.2.1"


def current_state_snapshot(
    context: Any,
    unit: Any,
    candidate_id: str,
    input_id: str,
    frame_index: int,
    step: int,
    time_ps: float,
    particle_count: int,
    stage: str,
    periodic: bool,
) -> tuple[Any, list[str], Any | None]:
    state = context.getState(getPositions=True, getEnergy=True, enforcePeriodicBox=periodic)
    positions = state.getPositions(asNumpy=True).value_in_unit(unit.angstrom)
    periodic_box = (
        state.getPeriodicBoxVectors(asNumpy=True).value_in_unit(unit.angstrom)
        if periodic
        else None
    )
    potential = float(state.getPotentialEnergy().value_in_unit(unit.kilojoule_per_mole))
    kinetic = float(state.getKineticEnergy().value_in_unit(unit.kilojoule_per_mole))
    total = potential + kinetic
    temperature = temperature_estimate_kelvin(kinetic, particle_count)
    return (
        positions,
        [
            candidate_id,
            input_id,
            str(frame_index),
            str(step),
            fmt(time_ps),
            stage,
            fmt(potential),
            fmt(kinetic),
            fmt(total),
            fmt(temperature),
            "OpenMM Context state",
        ],
        periodic_box,
    )


def temperature_estimate_kelvin(kinetic_energy_kj_mol: float, particle_count: int) -> float:
    dof = max(1, 3 * int(particle_count) - 6)
    return (2.0 * kinetic_energy_kj_mol) / (dof * GAS_CONSTANT_KJ_PER_MOL_K)


def potential_energy_kj_mol(context: Any, unit: Any) -> float:
    state = context.getState(getEnergy=True)
    return float(state.getPotentialEnergy().value_in_unit(unit.kilojoule_per_mole))


class GpuTelemetryMonitor:
    def __init__(
        self,
        telemetry: GpuTelemetry,
        enabled: bool,
        label_prefix: str,
        interval_seconds: float,
    ) -> None:
        self.telemetry = telemetry
        self.enabled = enabled
        self.label_prefix = label_prefix
        self.interval_seconds = max(0.05, float(interval_seconds))
        self.stop_event = threading.Event()
        self.thread: threading.Thread | None = None
        self.count = 0

    def __enter__(self) -> GpuTelemetryMonitor:
        if not self.enabled:
            return self
        self.thread = threading.Thread(target=self._run, name=f"genoligand-{self.label_prefix}", daemon=True)
        self.thread.start()
        return self

    def __exit__(self, exc_type: Any, exc: Any, traceback: Any) -> None:
        if not self.enabled:
            return
        self.stop_event.set()
        if self.thread is not None:
            self.thread.join(timeout=max(1.0, self.interval_seconds * 2.0))
        sample_gpu_telemetry(self.telemetry, f"{self.label_prefix}_stop")

    def _run(self) -> None:
        sample_gpu_telemetry(self.telemetry, f"{self.label_prefix}_start")
        while not self.stop_event.wait(self.interval_seconds):
            self.count += 1
            sample_gpu_telemetry(self.telemetry, f"{self.label_prefix}_{self.count}")


def sample_gpu_telemetry(telemetry: GpuTelemetry, label: str) -> None:
    gpu_result = run_nvidia_smi(
        [
            "--query-gpu=timestamp,index,name,utilization.gpu,memory.used,memory.total,power.draw",
            "--format=csv,noheader,nounits",
        ]
    )
    process_result = run_nvidia_smi(
        [
            "--query-compute-apps=pid,process_name,used_memory",
            "--format=csv,noheader,nounits",
        ]
    )
    if gpu_result is None:
        with telemetry.lock:
            telemetry.failure_reason = append_reason(telemetry.failure_reason, "nvidia-smi unavailable")
        return
    process_rows = parse_compute_process_rows(process_result or "")
    current_pid = str(os.getpid())
    current_process = any(row.get("pid") == current_pid for row in process_rows)
    process_memory = sum(parse_float(row.get("used_memory_mb", "")) for row in process_rows)
    compute_process_count = len(process_rows)
    parsed_rows = parse_gpu_rows(gpu_result)
    with telemetry.lock:
        telemetry.available = True
        failure_reason = telemetry.failure_reason
        for row in parsed_rows:
            row.update(
                {
                    "sample_label": label,
                    "current_pid_observed": bool_text(current_process),
                    "compute_process_count": str(compute_process_count),
                    "compute_process_memory_used_mb": fmt(process_memory),
                    "failure_reason": failure_reason,
                }
            )
            telemetry.rows.append(row)


def run_nvidia_smi(args: list[str]) -> str | None:
    try:
        result = subprocess.run(
            ["nvidia-smi", *args],
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=8,
            check=False,
        )
    except (FileNotFoundError, subprocess.SubprocessError):
        return None
    if result.returncode != 0:
        return None
    return result.stdout


def parse_gpu_rows(text: str) -> list[dict[str, str]]:
    rows: list[dict[str, str]] = []
    reader = csv.reader(text.splitlines())
    for cols in reader:
        if len(cols) < 7:
            continue
        rows.append(
            {
                "timestamp": cols[0].strip(),
                "gpu_index": cols[1].strip(),
                "name": cols[2].strip(),
                "utilization_gpu_percent": fmt(parse_float(cols[3])),
                "memory_used_mb": fmt(parse_float(cols[4])),
                "memory_total_mb": fmt(parse_float(cols[5])),
                "power_draw_w": fmt(parse_float(cols[6])),
            }
        )
    return rows


def parse_compute_process_rows(text: str) -> list[dict[str, str]]:
    rows: list[dict[str, str]] = []
    reader = csv.reader(text.splitlines())
    for cols in reader:
        if len(cols) < 3:
            continue
        pid = cols[0].strip()
        if not pid or pid.upper() in {"N/A", "[N/A]"}:
            continue
        rows.append(
            {
                "pid": pid,
                "process_name": cols[1].strip(),
                "used_memory_mb": fmt(parse_float(cols[2])),
            }
        )
    return rows


def summarize_gpu_telemetry(platform_name: str, telemetry: GpuTelemetry) -> dict[str, Any]:
    baseline_by_gpu: dict[str, float] = {}
    max_memory_delta = 0.0
    max_utilization = 0.0
    production_max_memory_delta = 0.0
    production_max_utilization = 0.0
    process_observed = False
    production_sample_count = 0
    with telemetry.lock:
        rows = list(telemetry.rows)
        available = telemetry.available
        failure_reason = telemetry.failure_reason
    for row in rows:
        gpu_index = row.get("gpu_index", "")
        memory_used = parse_float(row.get("memory_used_mb", ""))
        utilization = parse_float(row.get("utilization_gpu_percent", ""))
        if row.get("sample_label") == "baseline_before_openmm_context":
            baseline_by_gpu.setdefault(gpu_index, memory_used)
        baseline = baseline_by_gpu.get(gpu_index, memory_used)
        memory_delta = memory_used - baseline
        max_memory_delta = max(max_memory_delta, memory_delta)
        max_utilization = max(max_utilization, utilization)
        process_observed = process_observed or row.get("current_pid_observed") == "true"
        if row.get("sample_label", "").startswith("production_poll_"):
            production_sample_count += 1
            production_max_memory_delta = max(production_max_memory_delta, memory_delta)
            production_max_utilization = max(production_max_utilization, utilization)
    if process_observed and production_sample_count > 0:
        observation_basis = "current_pid"
    elif production_max_memory_delta >= 8.0 and production_max_utilization > 0.0:
        observation_basis = "memory_delta_and_utilization"
    else:
        observation_basis = "not_observed"
    gpu_observed = platform_name == "CUDA" and available and production_sample_count > 0 and observation_basis != "not_observed"
    return {
        "cuda_platform": platform_name,
        "nvidia_smi_available": available,
        "gpu_observed": gpu_observed,
        "max_utilization_gpu_percent": max_utilization,
        "max_memory_delta_mb": max_memory_delta,
        "production_max_utilization_gpu_percent": production_max_utilization,
        "production_max_memory_delta_mb": production_max_memory_delta,
        "process_observed": process_observed,
        "sample_count": len(rows),
        "production_sample_count": production_sample_count,
        "monitoring_mode": "nvidia-smi_polling" if production_sample_count > 0 else "stage_samples",
        "observation_basis": observation_basis,
        "failure_reason": failure_reason,
    }


def append_reason(existing: str, reason: str) -> str:
    if not existing:
        return reason
    if reason in existing.split(";"):
        return existing
    return f"{existing};{reason}"


def parse_float(value: str) -> float:
    text = str(value).strip().replace("MiB", "").replace("W", "").replace("%", "").strip()
    if not text or text.upper() in {"N/A", "[N/A]", "NOT SUPPORTED", "[NOT SUPPORTED]"}:
        return 0.0
    try:
        return float(text)
    except ValueError:
        return 0.0


def bool_text(value: bool) -> str:
    return "true" if value else "false"


def build_md_protocol_rows(
    candidate_id: str,
    input_id: str,
    platform_name: str,
    platform_properties: dict[str, str],
    receptor: ReceptorCoordinate,
    mol: Any,
    parameterization: str,
    placement_method: str,
    pre_minimization_energy: float,
    post_minimization_energy: float,
    equilibration_steps: int,
    production_segments: int,
    production_steps_per_segment: int,
    timestep_ps: float,
    frame_count: int,
    state_row_count: int,
    gpu_status: dict[str, Any],
    model_metadata: dict[str, Any],
) -> list[list[str]]:
    properties = ",".join(f"{key}={value}" for key, value in sorted(platform_properties.items()))
    production_steps = production_segments * production_steps_per_segment
    equilibration_time_ps = equilibration_steps * timestep_ps
    production_time_ps = production_steps * timestep_ps
    return [
        [candidate_id, input_id, "input", "receptor_coordinate", str(receptor.path), "", "structure_viewer"],
        [candidate_id, input_id, "input", "ligand_placement", placement_method, "", "docking_pose_sdf"],
        [candidate_id, input_id, "input", "ligand_atom_count", str(mol.GetNumAtoms()), "atoms", "rdkit"],
        [candidate_id, input_id, "input", "receptor_atom_count", str(len(receptor.atoms)), "atoms", "pdbfixer_or_coordinate_parser"],
        [candidate_id, input_id, "parameterization", "ligand_parameterization", parameterization, "", "OpenFF/RDKit"],
        [candidate_id, input_id, "parameterization", "protein_forcefield", str(model_metadata.get("protein_forcefield", "")), "", "OpenMM"],
        [candidate_id, input_id, "parameterization", "small_molecule_forcefield", str(model_metadata.get("small_molecule_forcefield", "")), "", "OpenFF"],
        [candidate_id, input_id, "system", "solvent_mode", str(model_metadata.get("solvent_mode", "")), "", "OpenMM Modeller"],
        [candidate_id, input_id, "system", "solvent_atom_count", str(model_metadata.get("solvent_atom_count", 0)), "atoms", "OpenMM Modeller"],
        [candidate_id, input_id, "system", "total_atom_count", str(model_metadata.get("total_atom_count", 0)), "atoms", "OpenMM"],
        [candidate_id, input_id, "system", "nonbonded_method", str(model_metadata.get("nonbonded_method", "")), "", "OpenMM"],
        [candidate_id, input_id, "platform", "openmm_platform", platform_name, "", "OpenMM"],
        [candidate_id, input_id, "platform", "openmm_platform_properties", properties, "", "OpenMM"],
        [candidate_id, input_id, "integrator", "integrator", "LangevinMiddleIntegrator", "", "OpenMM"],
        [candidate_id, input_id, "integrator", "temperature", "300", "K", "OpenMM"],
        [candidate_id, input_id, "integrator", "friction", "1", "1/ps", "OpenMM"],
        [candidate_id, input_id, "integrator", "time_step", fmt(timestep_ps), "ps", "OpenMM"],
        [candidate_id, input_id, "minimization", "max_iterations", str(env_int("GENOLIGAND_MD_MINIMIZATION_ITERATIONS", DEFAULT_MINIMIZATION_ITERATIONS)), "steps", "OpenMM"],
        [candidate_id, input_id, "minimization", "pre_minimization_energy", fmt(pre_minimization_energy), "kJ/mol", "OpenMM"],
        [candidate_id, input_id, "minimization", "post_minimization_energy", fmt(post_minimization_energy), "kJ/mol", "OpenMM"],
        [candidate_id, input_id, "equilibration", "steps", str(equilibration_steps), "steps", "OpenMM"],
        [candidate_id, input_id, "equilibration", "time", fmt(equilibration_time_ps), "ps", "OpenMM"],
        [candidate_id, input_id, "production", "segments", str(production_segments), "segments", "OpenMM"],
        [candidate_id, input_id, "production", "steps_per_segment", str(production_steps_per_segment), "steps", "OpenMM"],
        [candidate_id, input_id, "production", "total_steps", str(production_steps), "steps", "OpenMM"],
        [candidate_id, input_id, "production", "total_time", fmt(production_time_ps), "ps", "OpenMM"],
        [candidate_id, input_id, "output", "frame_count", str(frame_count), "frames", "md_trajectory.tsv"],
        [candidate_id, input_id, "output", "state_rows", str(state_row_count), "rows", "md_state.tsv"],
        [candidate_id, input_id, "cuda", "nvidia_smi_available", bool_text(bool(gpu_status["nvidia_smi_available"])), "", "nvidia-smi"],
        [candidate_id, input_id, "cuda", "gpu_observed", bool_text(bool(gpu_status["gpu_observed"])), "", "nvidia-smi"],
        [candidate_id, input_id, "cuda", "max_utilization_gpu_percent", fmt(float(gpu_status["max_utilization_gpu_percent"])), "%", "nvidia-smi"],
        [candidate_id, input_id, "cuda", "max_memory_delta_mb", fmt(float(gpu_status["max_memory_delta_mb"])), "MB", "nvidia-smi"],
        [candidate_id, input_id, "cuda", "production_sample_count", str(gpu_status["production_sample_count"]), "samples", "nvidia-smi"],
        [candidate_id, input_id, "cuda", "observation_basis", str(gpu_status["observation_basis"]), "", "nvidia-smi"],
        [candidate_id, input_id, "cuda", "process_observed", bool_text(bool(gpu_status["process_observed"])), "", "nvidia-smi"],
    ]


def summarize_complex_frames(
    mol: Any,
    receptor: ReceptorCoordinate,
    frames: list[Any],
    role_ranges: dict[str, range],
    box_vectors: list[Any | None] | None = None,
    production_steps_total: int | None = None,
) -> dict[str, float]:
    import mdtraj
    import numpy as np

    sampling_reason = md_sampling_rejection_reason(
        1 if production_steps_total is None else production_steps_total,
        len(frames),
    )
    if sampling_reason:
        raise ValueError(sampling_reason)

    receptor_indices = list(role_ranges["receptor"])
    ligand_indices = list(role_ranges["ligand"])
    receptor_heavy_local = [
        index for index, atom in enumerate(receptor.atoms) if atom["element"].upper() != "H"
    ]
    ligand_heavy_local = [atom.GetIdx() for atom in mol.GetAtoms() if atom.GetAtomicNum() > 1]
    if not ligand_heavy_local:
        ligand_heavy_local = list(range(mol.GetNumAtoms()))
    if not receptor_heavy_local:
        receptor_heavy_local = list(range(len(receptor.atoms)))

    topology = complex_mdtraj_topology(receptor, mol, mdtraj)
    complex_frames = np.stack(
        [
            np.vstack(
                [
                    np.asarray(frame, dtype=float)[receptor_indices, :],
                    np.asarray(frame, dtype=float)[ligand_indices, :],
                ]
            )
            for frame in frames
        ],
        axis=0,
    )
    trajectory = mdtraj.Trajectory(complex_frames / 10.0, topology)
    if box_vectors is not None and len(box_vectors) > 0 and all(box is not None for box in box_vectors):
        periodic_boxes = np.asarray(box_vectors, dtype=float)
        if periodic_boxes.shape != (len(frames), 3, 3):
            raise ValueError("invalid_periodic_box_vectors")
        trajectory.unitcell_vectors = periodic_boxes / 10.0
        receptor_atoms = set(range(len(receptor_indices)))
        anchor_molecules = [
            molecule
            for molecule in topology.find_molecules()
            if molecule and all(atom.index in receptor_atoms for atom in molecule)
        ]
        if anchor_molecules:
            trajectory = trajectory.image_molecules(anchor_molecules=anchor_molecules)

    trajectory.superpose(
        trajectory,
        frame=0,
        atom_indices=np.asarray(receptor_heavy_local, dtype=int),
    )
    aligned = np.asarray(trajectory.xyz, dtype=float) * 10.0
    receptor_heavy = receptor_heavy_local
    ligand_heavy = [len(receptor_indices) + index for index in ligand_heavy_local]
    receptor_frames = aligned[1:, receptor_heavy, :]
    ligand_frames = aligned[1:, ligand_heavy, :]
    receptor_reference = aligned[0, receptor_heavy, :]
    ligand_reference = aligned[0, ligand_heavy, :]

    receptor_delta = receptor_frames - receptor_reference
    rmsd_values = np.sqrt((receptor_delta**2).sum(axis=2).mean(axis=1))
    mean_by_atom = receptor_frames.mean(axis=0)
    rmsf_by_atom = np.sqrt(((receptor_frames - mean_by_atom) ** 2).sum(axis=2).mean(axis=0))
    drift_values = np.sqrt(((ligand_frames.mean(axis=1) - ligand_reference.mean(axis=0)) ** 2).sum(axis=1))
    contact = receptor_ligand_contact_occupancy(receptor_frames, ligand_frames)
    hbond = receptor_ligand_hbond_occupancy(
        trajectory[1:],
        range(len(receptor_indices)),
        range(len(receptor_indices), len(receptor_indices) + len(ligand_indices)),
        mdtraj,
        np,
    )
    rmsd = float(np.mean(rmsd_values))
    rmsf = float(np.mean(rmsf_by_atom)) if len(rmsf_by_atom) else 0.0
    drift = float(np.mean(drift_values))
    persistence = max(contact, hbond)
    stability = md_stability_score(rmsd, drift, contact, hbond, persistence)
    return {
        "rmsd": rmsd,
        "rmsf": rmsf,
        "ligand_drift": drift,
        "contact_occupancy": contact,
        "hydrogen_bond_occupancy": hbond,
        "interaction_persistence": persistence,
        "md_stability_score": stability,
    }


def complex_mdtraj_topology(receptor: ReceptorCoordinate, mol: Any, mdtraj: Any) -> Any:
    topology = mdtraj.Topology.from_openmm(receptor.topology)
    ligand_chain = topology.add_chain()
    ligand_residue = topology.add_residue("LIG", ligand_chain)
    ligand_atoms = []
    for atom in mol.GetAtoms():
        symbol = atom.GetSymbol()
        ligand_atoms.append(
            topology.add_atom(
                f"{symbol}{atom.GetIdx() + 1}",
                mdtraj.element.get_by_symbol(symbol),
                ligand_residue,
            )
        )
    for bond in mol.GetBonds():
        topology.add_bond(
            ligand_atoms[bond.GetBeginAtomIdx()],
            ligand_atoms[bond.GetEndAtomIdx()],
        )
    return topology


def md_sampling_rejection_reason(production_steps_total: int, frame_count: int) -> str:
    if production_steps_total <= 0:
        return "zero_production_steps"
    if frame_count <= 1:
        return "single_frame_trajectory"
    if frame_count - 1 < MINIMUM_MD_PRODUCTION_SAMPLES:
        return "insufficient_production_samples"
    return ""


def md_stability_score(
    rmsd_angstrom: float,
    ligand_drift_angstrom: float,
    contact_occupancy: float,
    hydrogen_bond_occupancy: float,
    interaction_persistence: float,
) -> float:
    rmsd_component = clamp_unit(1.0 - float(rmsd_angstrom) / RMSD_SCORE_SCALE_ANGSTROM)
    drift_component = clamp_unit(1.0 - float(ligand_drift_angstrom) / LIGAND_DRIFT_SCORE_SCALE_ANGSTROM)
    score = (
        rmsd_component * 0.30
        + drift_component * 0.25
        + clamp_unit(contact_occupancy) * 0.20
        + clamp_unit(hydrogen_bond_occupancy) * 0.10
        + clamp_unit(interaction_persistence) * 0.15
    )
    return clamp_unit(score)


def clamp_unit(value: float) -> float:
    value = float(value)
    if not math.isfinite(value):
        return 0.0
    return max(0.0, min(1.0, value))


def summarize_peptide_conformer_frames(mol: Any, frames: list[Any], np: Any) -> dict[str, float]:
    heavy_indices = [atom.GetIdx() for atom in mol.GetAtoms() if atom.GetAtomicNum() > 1]
    if not heavy_indices:
        heavy_indices = list(range(mol.GetNumAtoms()))
    peptide_frames = [np.asarray(frame, dtype=float)[heavy_indices, :] for frame in frames]
    reference = peptide_frames[0]
    rmsd_values = [kabsch_rmsd(frame, reference) for frame in peptide_frames[1:]]
    stack = np.stack(peptide_frames, axis=0)
    mean_by_atom = stack.mean(axis=0)
    rmsf_by_atom = np.sqrt(((stack - mean_by_atom) ** 2).sum(axis=2).mean(axis=0))
    centers = np.array([frame.mean(axis=0) for frame in peptide_frames])
    drift_values = np.sqrt(((centers - centers[0]) ** 2).sum(axis=1))[1:]
    rmsd = float(np.mean(rmsd_values)) if rmsd_values else 0.0
    rmsf = float(np.mean(rmsf_by_atom)) if len(rmsf_by_atom) else 0.0
    drift = float(np.mean(drift_values)) if len(drift_values) else 0.0
    stability = max(0.0, min(1.0, 1.0 - (rmsd / 5.0 + rmsf / 5.0 + drift / 5.0) / 3.0))
    return {
        "rmsd": rmsd,
        "rmsf": rmsf,
        "ligand_drift": drift,
        "contact_occupancy": 0.0,
        "hydrogen_bond_occupancy": 0.0,
        "interaction_persistence": 0.0,
        "md_stability_score": stability,
    }


def kabsch_rmsd(frame: Any, reference: Any) -> float:
    import numpy as np

    left = np.asarray(frame, dtype=float) - np.asarray(frame, dtype=float).mean(axis=0)
    right = np.asarray(reference, dtype=float) - np.asarray(reference, dtype=float).mean(axis=0)
    covariance = left.T @ right
    v, _, wt = np.linalg.svd(covariance)
    sign = np.sign(np.linalg.det(v @ wt))
    correction = np.diag([1.0, 1.0, sign])
    rotation = v @ correction @ wt
    aligned = left @ rotation
    return float(np.sqrt(((aligned - right) ** 2).sum(axis=1).mean()))


def receptor_ligand_contact_occupancy(receptor_frames: list[Any], ligand_frames: list[Any]) -> float:
    import numpy as np

    if len(receptor_frames) != len(ligand_frames):
        raise ValueError("receptor_ligand_frame_count_mismatch")
    occupied_frames = 0
    for receptor_frame, ligand_frame in zip(receptor_frames, ligand_frames):
        distances = np.sqrt(((receptor_frame[:, None, :] - ligand_frame[None, :, :]) ** 2).sum(axis=2))
        occupied_frames += int(bool(np.any(distances <= CONTACT_CUTOFF_ANGSTROM)))
    return occupied_frames / len(receptor_frames) if len(receptor_frames) else 0.0


def receptor_ligand_hbond_occupancy(
    trajectory: Any,
    receptor_indices: range,
    ligand_indices: range,
    mdtraj: Any,
    np: Any,
) -> float:
    if trajectory.n_frames == 0:
        return 0.0
    triplets = mdtraj.baker_hubbard(
        trajectory,
        freq=0.0,
        exclude_water=True,
        periodic=False,
        distance_cutoff=HBOND_DISTANCE_CUTOFF_NM,
        angle_cutoff=HBOND_ANGLE_CUTOFF_DEGREES,
    )
    receptor_set = set(receptor_indices)
    ligand_set = set(ligand_indices)
    interface_triplets = [
        tuple(int(index) for index in triplet)
        for triplet in triplets
        if (int(triplet[0]) in receptor_set and int(triplet[2]) in ligand_set)
        or (int(triplet[0]) in ligand_set and int(triplet[2]) in receptor_set)
    ]
    if not interface_triplets:
        return 0.0
    distances = mdtraj.compute_distances(
        trajectory,
        np.asarray([(hydrogen, acceptor) for _, hydrogen, acceptor in interface_triplets], dtype=int),
        periodic=False,
    )
    angles = mdtraj.compute_angles(
        trajectory,
        np.asarray(interface_triplets, dtype=int),
        periodic=False,
    )
    present = (distances <= HBOND_DISTANCE_CUTOFF_NM) & (
        angles >= np.deg2rad(HBOND_ANGLE_CUTOFF_DEGREES)
    )
    return float(np.any(present, axis=1).mean())


def distance(left: Any, right: Any) -> float:
    return math.sqrt(sum((float(a) - float(b)) ** 2 for a, b in zip(left, right)))


def write_md_not_ready(run_dir: Path, reason: str, risk_flags: list[str]) -> None:
    tool_outputs = run_dir / "tool_outputs"
    tool_outputs.mkdir(parents=True, exist_ok=True)
    write_tsv(
        tool_outputs / "md_not_ready.tsv",
        ["reason", "risk_flags"],
        [[reason, ";".join(risk_flags)]],
    )
    with (tool_outputs / "md_run_manifest.json").open("w", encoding="utf-8") as handle:
        json.dump(
            {
                "method": "not_ready",
                "reason": reason,
                "risk_flags": risk_flags,
                "metric_artifacts": [],
                "ligand_coordinate": "",
                "ligand_placement_manifest": "",
                "complex_manifest": "",
                "trajectory": "",
            },
            handle,
            indent=2,
            sort_keys=True,
        )
        handle.write("\n")


def write_md_artifacts(
    run_dir: Path,
    candidate_id: str,
    input_id: str,
    smiles: str,
    platform_name: str,
    metrics: dict[str, float],
    frames: list[Any],
    state_rows: list[list[str]],
    mol: Any,
    ligand_positions: Any,
    receptor: ReceptorCoordinate,
    role_ranges: dict[str, range],
    parameterization: str,
    risk_flags: list[str],
    placement_method: str,
    platform_properties: dict[str, str],
    protocol_rows: list[list[str]],
    gpu_telemetry: GpuTelemetry,
    gpu_status: dict[str, Any],
    model_metadata: dict[str, Any],
) -> None:
    tool_outputs = run_dir / "tool_outputs"
    tool_outputs.mkdir(parents=True, exist_ok=True)
    score_note = "score_decomposition.tsv records Rust-owned candidate scores"
    risk_text = ";".join(risk_flags)
    summary_header = [
        "candidate_id",
        "rmsd",
        "rmsf",
        "ligand_drift",
        "hydrogen_bond_occupancy",
        "contact_occupancy",
        "interaction_persistence",
        "md_risk_flags",
        "md_score_decomposition",
        "md_stability_score",
    ]
    write_tsv(
        tool_outputs / "md_summary.tsv",
        summary_header,
        [
            [
                candidate_id,
                fmt(metrics["rmsd"]),
                fmt(metrics["rmsf"]),
                fmt(metrics["ligand_drift"]),
                fmt(metrics["hydrogen_bond_occupancy"]),
                fmt(metrics["contact_occupancy"]),
                fmt(metrics["interaction_persistence"]),
                risk_text,
                score_note,
                fmt(metrics["md_stability_score"]),
            ]
        ],
    )
    metric_specs = [
        ("md_rmsd.tsv", "rmsd"),
        ("md_rmsf.tsv", "rmsf"),
        ("md_ligand_drift.tsv", "ligand_drift"),
        ("md_contacts.tsv", "contact_occupancy"),
        ("md_hbonds.tsv", "hydrogen_bond_occupancy"),
    ]
    for file_name, metric in metric_specs:
        write_tsv(
            tool_outputs / file_name,
            ["candidate_id", "input_id", "metric", "value", "source"],
            [[candidate_id, input_id, metric, fmt(metrics[metric]), "openmm_receptor_ligand_complex_dynamics"]],
        )
    metric_artifacts = [
        "tool_outputs/md_summary.tsv",
        "tool_outputs/md_rmsd.tsv",
        "tool_outputs/md_rmsf.tsv",
        "tool_outputs/md_ligand_drift.tsv",
        "tool_outputs/md_contacts.tsv",
        "tool_outputs/md_hbonds.tsv",
    ]
    write_ligand_placement(
        tool_outputs / "ligand_placement.tsv",
        candidate_id,
        input_id,
        mol,
        ligand_positions,
        placement_method,
    )
    write_trajectory(tool_outputs / "md_trajectory.tsv", candidate_id, frames, mol, receptor, role_ranges)
    write_md_state(tool_outputs / "md_state.tsv", state_rows)
    write_tsv(
        tool_outputs / "md_protocol.tsv",
        ["candidate_id", "input_id", "stage", "parameter", "value", "unit", "source"],
        protocol_rows,
    )
    write_gpu_telemetry(tool_outputs / "gpu_telemetry.tsv", gpu_telemetry)
    write_gpu_telemetry_status(tool_outputs / "gpu_telemetry_status.tsv", gpu_status)
    write_complex_md_manifest(
        tool_outputs / "complex_md_manifest.json",
        candidate_id,
        input_id,
        platform_name,
        receptor,
        parameterization,
        risk_flags,
        metric_artifacts,
        placement_method,
        platform_properties,
        gpu_status,
        model_metadata,
    )
    with (tool_outputs / "md_run_manifest.json").open("w", encoding="utf-8") as handle:
        json.dump(
            {
                "candidate_id": candidate_id,
                "input_id": input_id,
                "input_smiles": smiles,
                "method": "openmm_receptor_ligand_complex_dynamics",
                "platform": platform_name,
                "platform_properties": platform_properties,
                "cuda_ready": platform_name == "CUDA" and bool(gpu_status["gpu_observed"]),
                "gpu_observed": bool(gpu_status["gpu_observed"]),
                "gpu_telemetry_artifact": "tool_outputs/gpu_telemetry.tsv",
                "gpu_telemetry_status_artifact": "tool_outputs/gpu_telemetry_status.tsv",
                "gpu_telemetry": gpu_status,
                "system": model_metadata,
                "receptor_coordinate": str(receptor.path),
                "receptor_preparation": receptor.preparation,
                "receptor_atom_count": len(receptor.atoms),
                "ligand_parameterization": parameterization,
                "ligand_coordinate": "tool_outputs/ligand_placement.tsv",
                "ligand_coordinate_format": "tsv_angstrom",
                "ligand_atom_count": mol.GetNumAtoms(),
                "ligand_placement": placement_method,
                "ligand_placement_manifest": "tool_outputs/ligand_placement.tsv",
                "complex_manifest": "tool_outputs/complex_md_manifest.json",
                "protocol_artifact": "tool_outputs/md_protocol.tsv",
                "frame_count": len(frames),
                "state_artifact": "tool_outputs/md_state.tsv",
                "state_row_count": len(state_rows),
                "trajectory": "tool_outputs/md_trajectory.tsv",
                "metric_artifacts": metric_artifacts,
                "risk_flags": risk_flags,
                "score_decomposition": "score_decomposition.tsv",
            },
            handle,
            indent=2,
            sort_keys=True,
        )
        handle.write("\n")


def write_peptide_conformer_artifacts(
    run_dir: Path,
    candidate_id: str,
    input_id: str,
    sequence: str,
    metrics: dict[str, float],
    frames: list[Any],
    mol: Any,
    parameterization: str,
    risk_flags: list[str],
    rdkit_version: str,
) -> None:
    tool_outputs = run_dir / "tool_outputs"
    tool_outputs.mkdir(parents=True, exist_ok=True)
    score_note = "score_decomposition.tsv records Rust-owned candidate scores"
    risk_text = ";".join(risk_flags)
    summary_header = [
        "candidate_id",
        "rmsd",
        "rmsf",
        "ligand_drift",
        "hydrogen_bond_occupancy",
        "contact_occupancy",
        "interaction_persistence",
        "md_risk_flags",
        "md_score_decomposition",
        "md_stability_score",
    ]
    write_tsv(
        tool_outputs / "md_summary.tsv",
        summary_header,
        [
            [
                candidate_id,
                fmt(metrics["rmsd"]),
                fmt(metrics["rmsf"]),
                fmt(metrics["ligand_drift"]),
                fmt(metrics["hydrogen_bond_occupancy"]),
                fmt(metrics["contact_occupancy"]),
                fmt(metrics["interaction_persistence"]),
                risk_text,
                score_note,
                fmt(metrics["md_stability_score"]),
            ]
        ],
    )
    metric_specs = [
        ("md_rmsd.tsv", "rmsd"),
        ("md_rmsf.tsv", "rmsf"),
        ("md_ligand_drift.tsv", "ligand_drift"),
        ("md_contacts.tsv", "contact_occupancy"),
        ("md_hbonds.tsv", "hydrogen_bond_occupancy"),
    ]
    for file_name, metric in metric_specs:
        write_tsv(
            tool_outputs / file_name,
            ["candidate_id", "input_id", "metric", "value", "source"],
            [[candidate_id, input_id, metric, fmt(metrics[metric]), "rdkit_peptide_conformer_ensemble"]],
        )
    metric_artifacts = [
        "tool_outputs/md_summary.tsv",
        "tool_outputs/md_rmsd.tsv",
        "tool_outputs/md_rmsf.tsv",
        "tool_outputs/md_ligand_drift.tsv",
        "tool_outputs/md_contacts.tsv",
        "tool_outputs/md_hbonds.tsv",
    ]
    write_ligand_placement(
        tool_outputs / "ligand_placement.tsv",
        candidate_id,
        input_id,
        mol,
        frames[0],
        "rdkit_peptide_lowest_energy_conformer",
    )
    write_peptide_trajectory(tool_outputs / "md_trajectory.tsv", candidate_id, frames, mol)
    with (tool_outputs / "md_run_manifest.json").open("w", encoding="utf-8") as handle:
        json.dump(
            {
                "candidate_id": candidate_id,
                "input_id": input_id,
                "input_kind": "peptide",
                "input_sequence": sequence,
                "method": "rdkit_peptide_conformer_ensemble",
                "platform": "RDKit",
                "cuda_ready": False,
                "rdkit_version": rdkit_version,
                "sequence_length": len(sequence),
                "ligand_parameterization": parameterization,
                "ligand_coordinate": "tool_outputs/ligand_placement.tsv",
                "ligand_coordinate_format": "tsv_angstrom",
                "ligand_atom_count": mol.GetNumAtoms(),
                "ligand_heavy_atom_count": sum(1 for atom in mol.GetAtoms() if atom.GetAtomicNum() > 1),
                "ligand_placement": "rdkit_peptide_lowest_energy_conformer",
                "ligand_placement_manifest": "tool_outputs/ligand_placement.tsv",
                "complex_manifest": "",
                "frame_count": len(frames),
                "trajectory": "tool_outputs/md_trajectory.tsv",
                "metric_artifacts": metric_artifacts,
                "risk_flags": risk_flags,
                "score_decomposition": "score_decomposition.tsv",
            },
            handle,
            indent=2,
            sort_keys=True,
        )
        handle.write("\n")


def write_ligand_placement(
    path: Path,
    candidate_id: str,
    input_id: str,
    mol: Any,
    ligand_positions: Any,
    placement_method: str = "rdkit_conformer_translated_near_receptor_centroid",
) -> None:
    rows = []
    for atom_index, atom in enumerate(mol.GetAtoms()):
        coord = ligand_positions[atom_index]
        rows.append(
            [
                candidate_id,
                input_id,
                str(atom_index),
                atom.GetSymbol(),
                fmt(coord[0]),
                fmt(coord[1]),
                fmt(coord[2]),
                "angstrom",
                placement_method,
            ]
        )
    write_tsv(
        path,
        [
            "candidate_id",
            "input_id",
            "atom_index",
            "element",
            "x",
            "y",
            "z",
            "unit",
            "placement_method",
        ],
        rows,
    )


def write_peptide_trajectory(path: Path, candidate_id: str, frames: list[Any], mol: Any) -> None:
    rows = []
    elements = [atom.GetSymbol() for atom in mol.GetAtoms()]
    residues = []
    for atom in mol.GetAtoms():
        info = atom.GetPDBResidueInfo()
        residues.append(info.GetResidueName().strip() if info else "PEP")
    for frame_index, frame in enumerate(frames):
        for atom_index, coord in enumerate(frame):
            rows.append(
                [
                    candidate_id,
                    str(frame_index),
                    str(atom_index),
                    "peptide",
                    elements[atom_index],
                    residues[atom_index],
                    fmt(coord[0]),
                    fmt(coord[1]),
                    fmt(coord[2]),
                ]
            )
    write_tsv(path, ["candidate_id", "frame", "atom_index", "role", "element", "residue", "x", "y", "z"], rows)


def write_complex_md_manifest(
    path: Path,
    candidate_id: str,
    input_id: str,
    platform_name: str,
    receptor: ReceptorCoordinate,
    parameterization: str,
    risk_flags: list[str],
    metric_artifacts: list[str],
    placement_method: str,
    platform_properties: dict[str, str],
    gpu_status: dict[str, Any],
    model_metadata: dict[str, Any],
) -> None:
    with path.open("w", encoding="utf-8") as handle:
        json.dump(
            {
                "candidate_id": candidate_id,
                "input_id": input_id,
                "method": "openmm_receptor_ligand_complex_dynamics",
                "platform": platform_name,
                "platform_properties": platform_properties,
                "system": model_metadata,
                "cuda_ready": platform_name == "CUDA" and bool(gpu_status["gpu_observed"]),
                "gpu_observed": bool(gpu_status["gpu_observed"]),
                "gpu_telemetry_artifact": "tool_outputs/gpu_telemetry.tsv",
                "gpu_telemetry_status_artifact": "tool_outputs/gpu_telemetry_status.tsv",
                "gpu_telemetry": gpu_status,
                "receptor_coordinate": str(receptor.path),
                "receptor_preparation": receptor.preparation,
                "ligand_coordinate": "tool_outputs/ligand_placement.tsv",
                "ligand_parameterization": parameterization,
                "ligand_placement": placement_method,
                "protocol_artifact": "tool_outputs/md_protocol.tsv",
                "state_artifact": "tool_outputs/md_state.tsv",
                "trajectory": "tool_outputs/md_trajectory.tsv",
                "metric_artifacts": metric_artifacts,
                "risk_flags": risk_flags,
            },
            handle,
            indent=2,
            sort_keys=True,
        )
        handle.write("\n")


def write_trajectory(
    path: Path,
    candidate_id: str,
    frames: list[Any],
    mol: Any,
    receptor: ReceptorCoordinate,
    role_ranges: dict[str, range],
) -> None:
    rows = []
    ligand_elements = [atom.GetSymbol() for atom in mol.GetAtoms()]
    receptor_indices = list(role_ranges["receptor"])
    ligand_indices = list(role_ranges["ligand"])
    for frame_index, frame in enumerate(frames):
        for local_index, global_index in enumerate(receptor_indices):
            coord = frame[global_index]
            atom = receptor.atoms[local_index]
            rows.append(
                [
                    candidate_id,
                    str(frame_index),
                    str(global_index),
                    "receptor",
                    atom["element"],
                    atom["residue"],
                    fmt(coord[0]),
                    fmt(coord[1]),
                    fmt(coord[2]),
                ]
            )
        for local_index, global_index in enumerate(ligand_indices):
            coord = frame[global_index]
            rows.append(
                [
                    candidate_id,
                    str(frame_index),
                    str(global_index),
                    "ligand",
                    ligand_elements[local_index],
                    "LIG",
                    fmt(coord[0]),
                    fmt(coord[1]),
                    fmt(coord[2]),
                ]
            )
    write_tsv(path, ["candidate_id", "frame", "atom_index", "role", "element", "residue", "x", "y", "z"], rows)


def write_md_state(path: Path, rows: list[list[str]]) -> None:
    write_tsv(
        path,
        [
            "candidate_id",
            "input_id",
            "frame",
            "step",
            "time_ps",
            "stage",
            "potential_energy_kj_mol",
            "kinetic_energy_kj_mol",
            "total_energy_kj_mol",
            "temperature_estimate_K",
            "source",
        ],
        rows,
    )


def write_gpu_telemetry(path: Path, telemetry: GpuTelemetry) -> None:
    header = [
        "sample_label",
        "timestamp",
        "gpu_index",
        "name",
        "utilization_gpu_percent",
        "memory_used_mb",
        "memory_total_mb",
        "power_draw_w",
        "current_pid_observed",
        "compute_process_count",
        "compute_process_memory_used_mb",
        "failure_reason",
    ]
    with telemetry.lock:
        telemetry_rows = list(telemetry.rows)
    rows = [[row.get(column, "") for column in header] for row in telemetry_rows]
    write_tsv(path, header, rows)


def write_gpu_telemetry_status(path: Path, gpu_status: dict[str, Any]) -> None:
    write_tsv(
        path,
        [
            "cuda_platform",
            "nvidia_smi_available",
            "gpu_observed",
            "max_utilization_gpu_percent",
            "max_memory_delta_mb",
            "process_observed",
            "sample_count",
            "production_sample_count",
            "production_max_utilization_gpu_percent",
            "production_max_memory_delta_mb",
            "monitoring_mode",
            "observation_basis",
            "failure_reason",
        ],
        [
            [
                str(gpu_status["cuda_platform"]),
                bool_text(bool(gpu_status["nvidia_smi_available"])),
                bool_text(bool(gpu_status["gpu_observed"])),
                fmt(float(gpu_status["max_utilization_gpu_percent"])),
                fmt(float(gpu_status["max_memory_delta_mb"])),
                bool_text(bool(gpu_status["process_observed"])),
                str(gpu_status["sample_count"]),
                str(gpu_status["production_sample_count"]),
                fmt(float(gpu_status["production_max_utilization_gpu_percent"])),
                fmt(float(gpu_status["production_max_memory_delta_mb"])),
                str(gpu_status["monitoring_mode"]),
                str(gpu_status["observation_basis"]),
                str(gpu_status["failure_reason"]),
            ]
        ],
    )


def write_tsv(path: Path, header: list[str], rows: list[list[str]]) -> None:
    with path.open("w", encoding="utf-8", newline="") as handle:
        writer = csv.writer(handle, delimiter="\t", lineterminator="\n")
        writer.writerow(header)
        writer.writerows(rows)


def fmt(value: float) -> str:
    return f"{float(value):.4f}"


def safe_note(value: str) -> str:
    return value.replace("\t", " ").replace("\n", " ")[:160]


def openmm_version_text(openmm: Any, state: str, reason: str) -> str:
    return f"OpenMM {openmm.version.full_version}; md_summary={state}; reason={reason}"


if __name__ == "__main__":
    raise SystemExit(main())
