from __future__ import annotations

import csv
import json
import math
import os
import subprocess
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from genoligand_workers.worker_io import parse_worker_args, read_job, write_result


MAX_PEPTIDE_RESIDUES = 80
PEPTIDE_CONFORMER_COUNT = 12
SIMULATION_SEED = 20260629
STANDARD_AMINO_ACIDS = set("ACDEFGHIKLMNPQRSTVWY")
DEFAULT_EQUILIBRATION_STEPS = 1000
DEFAULT_PRODUCTION_SEGMENTS = 20
DEFAULT_PRODUCTION_STEPS_PER_SEGMENT = 250
DEFAULT_MINIMIZATION_ITERATIONS = 200


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


def main() -> int:
    args = parse_worker_args()
    job = read_job(args.job)
    try:
        version = run_md_summary_job(job)
        status = "PASS"
    except Exception as exc:  # pragma: no cover - depends on local scientific runtime
        version = f"MD summary worker failed: {exc}"
        status = "FAIL"
    write_result(args.out, job, status, version)
    return 0 if status == "PASS" else 2


def run_md_summary_job(job: dict[str, Any]) -> str:
    import numpy as np
    import openmm
    from openmm import unit
    from rdkit import Chem, rdBase
    from rdkit.Chem import AllChem

    input_manifest = runtime_path(str(job["input_manifest"]))
    run_dir = runtime_path(str(job["run_dir"]))
    science_store = runtime_path(str(job.get("science_store", "")))
    smiles, input_id = read_input_conversion_manifest(run_dir, "small_molecule")
    if not smiles:
        smiles, input_id = read_small_molecule_input(input_manifest)
    if not smiles:
        sequence, input_id = read_input_conversion_manifest(run_dir, "peptide")
        if not sequence:
            sequence, input_id = read_peptide_sequence_input(input_manifest)
        if sequence:
            try:
                mol, frames, parameterization, parameter_flags = prepare_peptide_conformer_ensemble(
                    sequence,
                    Chem,
                    AllChem,
                    np,
                )
            except Exception as exc:  # noqa: BLE001 - not-ready artifact is the contract for unusable sequence inputs
                write_md_not_ready(
                    run_dir,
                    f"peptide conformer calculation failed: {safe_note(str(exc))}",
                    ["peptide_conformer_calculation_failed"],
                )
                return openmm_version_text(openmm, "not_ready", "peptide_conformer_calculation_failed")
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
            return (
                f"RDKit {rdBase.rdkitVersion}; "
                "method=rdkit_peptide_conformer_ensemble; input_kind=peptide"
            )
        write_md_not_ready(run_dir, "no small_molecule input available for MD summary", [])
        return openmm_version_text(openmm, "not_ready", "missing_small_molecule")

    receptor_path = find_receptor_coordinate(run_dir)
    if receptor_path is None:
        write_md_not_ready(run_dir, "missing receptor coordinate artifact for complex MD", ["missing_receptor_coordinate"])
        return openmm_version_text(openmm, "not_ready", "missing_receptor_coordinate")

    try:
        receptor = load_receptor_coordinate(receptor_path, unit)
    except Exception as exc:  # noqa: BLE001 - not-ready artifact is the contract for unusable coordinates
        write_md_not_ready(
            run_dir,
            f"receptor coordinate preparation failed: {safe_note(str(exc))}",
            ["receptor_coordinate_preparation_failed"],
        )
        return openmm_version_text(openmm, "not_ready", "receptor_coordinate_preparation_failed")
    if not receptor.atoms:
        write_md_not_ready(run_dir, "receptor coordinate file did not contain usable heavy atoms", ["receptor_coordinate_parse_failed"])
        return openmm_version_text(openmm, "not_ready", "receptor_coordinate_parse_failed")

    mol = read_docking_pose_ligand(run_dir, Chem)
    if mol is None:
        write_md_not_ready(
            run_dir,
            "missing AutoDock Vina docking pose SDF for complex MD",
            ["missing_docking_pose_sdf"],
        )
        return openmm_version_text(openmm, "not_ready", "missing_docking_pose_sdf")

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
        0.001 * unit.picoseconds,
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
    equilibration_steps = env_int("GENOLIGAND_MD_EQUILIBRATION_STEPS", DEFAULT_EQUILIBRATION_STEPS)
    production_segments = env_int("GENOLIGAND_MD_PRODUCTION_SEGMENTS", DEFAULT_PRODUCTION_SEGMENTS)
    production_steps_per_segment = env_int(
        "GENOLIGAND_MD_PRODUCTION_STEPS_PER_SEGMENT",
        DEFAULT_PRODUCTION_STEPS_PER_SEGMENT,
    )
    if equilibration_steps > 0:
        integrator.step(equilibration_steps)
    sample_gpu_telemetry(gpu_telemetry, "after_equilibration")

    frames = [current_positions(context, unit)]
    for segment in range(production_segments):
        integrator.step(production_steps_per_segment)
        frames.append(current_positions(context, unit))
        sample_gpu_telemetry(gpu_telemetry, f"production_segment_{segment + 1}")
    sample_gpu_telemetry(gpu_telemetry, "after_production")
    del context
    del integrator

    candidate_id = top_candidate_id(run_dir) or input_id
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
        len(frames),
        gpu_status,
        model_metadata,
    )
    risk_flags = [
        *receptor.risk_flags,
        *parameter_flags,
        "ligand_placement_from_autodock_vina_pose",
        "standard_openmm_forcefield_complex",
    ]
    metrics = summarize_complex_frames(mol, receptor, frames, role_ranges)
    write_md_artifacts(
        run_dir,
        candidate_id,
        input_id,
        smiles,
        platform_name,
        metrics,
        frames,
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
    return (
        f"OpenMM {openmm.version.full_version}; platform={platform_name}; "
        "method=openmm_receptor_ligand_complex_dynamics; "
        f"gpu_observed={gpu_status['gpu_observed']}; receptor={receptor.path.name}"
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


def current_positions(context: Any, unit: Any) -> Any:
    return context.getState(getPositions=True).getPositions(asNumpy=True).value_in_unit(
        unit.angstrom
    )


def potential_energy_kj_mol(context: Any, unit: Any) -> float:
    state = context.getState(getEnergy=True)
    return float(state.getPotentialEnergy().value_in_unit(unit.kilojoule_per_mole))


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
        telemetry.failure_reason = append_reason(telemetry.failure_reason, "nvidia-smi unavailable")
        return
    telemetry.available = True
    process_rows = parse_compute_process_rows(process_result or "")
    current_pid = str(os.getpid())
    current_process = any(row.get("pid") == current_pid for row in process_rows)
    process_memory = sum(parse_float(row.get("used_memory_mb", "")) for row in process_rows)
    compute_process_count = len(process_rows)
    for row in parse_gpu_rows(gpu_result):
        row.update(
            {
                "sample_label": label,
                "current_pid_observed": bool_text(current_process),
                "compute_process_count": str(compute_process_count),
                "compute_process_memory_used_mb": fmt(process_memory),
                "failure_reason": telemetry.failure_reason,
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
    process_observed = False
    for row in telemetry.rows:
        gpu_index = row.get("gpu_index", "")
        memory_used = parse_float(row.get("memory_used_mb", ""))
        utilization = parse_float(row.get("utilization_gpu_percent", ""))
        if row.get("sample_label") == "baseline_before_openmm_context":
            baseline_by_gpu.setdefault(gpu_index, memory_used)
        baseline = baseline_by_gpu.get(gpu_index, memory_used)
        max_memory_delta = max(max_memory_delta, memory_used - baseline)
        max_utilization = max(max_utilization, utilization)
        process_observed = process_observed or row.get("current_pid_observed") == "true"
    gpu_observed = platform_name == "CUDA" and telemetry.available and (
        process_observed or (max_memory_delta >= 8.0 and max_utilization > 0.0)
    )
    return {
        "cuda_platform": platform_name,
        "nvidia_smi_available": telemetry.available,
        "gpu_observed": gpu_observed,
        "max_utilization_gpu_percent": max_utilization,
        "max_memory_delta_mb": max_memory_delta,
        "process_observed": process_observed,
        "sample_count": len(telemetry.rows),
        "failure_reason": telemetry.failure_reason,
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
    frame_count: int,
    gpu_status: dict[str, Any],
    model_metadata: dict[str, Any],
) -> list[list[str]]:
    properties = ",".join(f"{key}={value}" for key, value in sorted(platform_properties.items()))
    production_steps = production_segments * production_steps_per_segment
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
        [candidate_id, input_id, "minimization", "max_iterations", str(env_int("GENOLIGAND_MD_MINIMIZATION_ITERATIONS", DEFAULT_MINIMIZATION_ITERATIONS)), "steps", "OpenMM"],
        [candidate_id, input_id, "minimization", "pre_minimization_energy", fmt(pre_minimization_energy), "kJ/mol", "OpenMM"],
        [candidate_id, input_id, "minimization", "post_minimization_energy", fmt(post_minimization_energy), "kJ/mol", "OpenMM"],
        [candidate_id, input_id, "equilibration", "steps", str(equilibration_steps), "steps", "OpenMM"],
        [candidate_id, input_id, "production", "segments", str(production_segments), "segments", "OpenMM"],
        [candidate_id, input_id, "production", "steps_per_segment", str(production_steps_per_segment), "steps", "OpenMM"],
        [candidate_id, input_id, "production", "total_steps", str(production_steps), "steps", "OpenMM"],
        [candidate_id, input_id, "output", "frame_count", str(frame_count), "frames", "md_trajectory.tsv"],
        [candidate_id, input_id, "cuda", "nvidia_smi_available", bool_text(bool(gpu_status["nvidia_smi_available"])), "", "nvidia-smi"],
        [candidate_id, input_id, "cuda", "gpu_observed", bool_text(bool(gpu_status["gpu_observed"])), "", "nvidia-smi"],
        [candidate_id, input_id, "cuda", "max_utilization_gpu_percent", fmt(float(gpu_status["max_utilization_gpu_percent"])), "%", "nvidia-smi"],
        [candidate_id, input_id, "cuda", "max_memory_delta_mb", fmt(float(gpu_status["max_memory_delta_mb"])), "MB", "nvidia-smi"],
        [candidate_id, input_id, "cuda", "process_observed", bool_text(bool(gpu_status["process_observed"])), "", "nvidia-smi"],
    ]


def summarize_complex_frames(
    mol: Any,
    receptor: ReceptorCoordinate,
    frames: list[Any],
    role_ranges: dict[str, range],
) -> dict[str, float]:
    import numpy as np

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
    receptor_heavy = [receptor_indices[index] for index in receptor_heavy_local]
    ligand_heavy = [ligand_indices[index] for index in ligand_heavy_local]

    receptor_frames = [np.asarray(frame, dtype=float)[receptor_heavy, :] for frame in frames]
    ligand_frames = [np.asarray(frame, dtype=float)[ligand_heavy, :] for frame in frames]
    receptor_reference = receptor_frames[0]
    rmsd_values = [kabsch_rmsd(frame, receptor_reference) for frame in receptor_frames[1:]]
    stack = np.stack([np.vstack([r_frame, l_frame]) for r_frame, l_frame in zip(receptor_frames, ligand_frames)], axis=0)
    mean_by_atom = stack.mean(axis=0)
    rmsf_by_atom = np.sqrt(((stack - mean_by_atom) ** 2).sum(axis=2).mean(axis=0))
    centers = np.array([frame.mean(axis=0) for frame in ligand_frames])
    drift_values = np.sqrt(((centers - centers[0]) ** 2).sum(axis=1))[1:]
    contact = receptor_ligand_contact_occupancy(receptor_frames, ligand_frames)
    hbond = receptor_ligand_hbond_occupancy(
        receptor,
        mol,
        receptor_frames,
        ligand_frames,
        ligand_heavy_local,
        receptor_heavy_local,
    )
    rmsd = float(np.mean(rmsd_values)) if rmsd_values else 0.0
    rmsf = float(np.mean(rmsf_by_atom)) if len(rmsf_by_atom) else 0.0
    drift = float(np.mean(drift_values)) if len(drift_values) else 0.0
    persistence = max(contact, hbond)
    stability = max(0.0, min(1.0, 1.0 - (rmsd / 5.0 + drift / 5.0) / 2.0))
    return {
        "rmsd": rmsd,
        "rmsf": rmsf,
        "ligand_drift": drift,
        "contact_occupancy": contact,
        "hydrogen_bond_occupancy": hbond,
        "interaction_persistence": persistence,
        "md_stability_score": stability,
    }


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

    occupied = 0
    total = 0
    for receptor_frame, ligand_frame in zip(receptor_frames, ligand_frames):
        distances = np.sqrt(((receptor_frame[:, None, :] - ligand_frame[None, :, :]) ** 2).sum(axis=2))
        occupied += int((distances <= 4.5).sum())
        total += int(distances.size)
    return occupied / total if total else 0.0


def receptor_ligand_hbond_occupancy(
    receptor: ReceptorCoordinate,
    mol: Any,
    receptor_frames: list[Any],
    ligand_frames: list[Any],
    ligand_heavy_local: list[int],
    receptor_heavy_local: list[int],
) -> float:
    import numpy as np

    receptor_polar = [
        out_index
        for out_index, atom_index in enumerate(receptor_heavy_local)
        if receptor.atoms[atom_index]["element"].upper() in {"N", "O", "S"}
    ]
    ligand_polar = [
        out_index
        for out_index, atom_index in enumerate(ligand_heavy_local)
        if mol.GetAtomWithIdx(atom_index).GetAtomicNum() in {7, 8, 16}
    ]
    if not receptor_polar or not ligand_polar:
        return 0.0
    occupied = 0
    total = 0
    for receptor_frame, ligand_frame in zip(receptor_frames, ligand_frames):
        distances = np.sqrt(
            ((receptor_frame[receptor_polar, None, :] - ligand_frame[None, ligand_polar, :]) ** 2).sum(axis=2)
        )
        occupied += int((distances <= 3.5).sum())
        total += int(distances.size)
    return occupied / total if total else 0.0


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
                "receptor_coordinate": str(receptor.path),
                "receptor_preparation": receptor.preparation,
                "ligand_coordinate": "tool_outputs/ligand_placement.tsv",
                "ligand_parameterization": parameterization,
                "ligand_placement": placement_method,
                "protocol_artifact": "tool_outputs/md_protocol.tsv",
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
    rows = [[row.get(column, "") for column in header] for row in telemetry.rows]
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
