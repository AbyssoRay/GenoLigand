from __future__ import annotations

import csv
import hashlib
import json
import math
import os
import subprocess
import threading
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

from genoligand_workers.common import WorkerExecution, WorkerOutcome
from genoligand_workers.receptor_contract import (
    BIOLOGICAL_METALS,
    RECEPTOR_CHAIN_CONTACT_CUTOFF_ANGSTROM,
    STANDARD_PROTEIN_RESIDUES,
    chain_signature,
)
from genoligand_workers.worker_io import (
    candidate_selection_digest,
    parse_worker_args,
    read_job,
    runtime_artifact_path,
    write_result,
)


MAX_PEPTIDE_RESIDUES = 80
PEPTIDE_CONFORMER_COUNT = 12
SIMULATION_SEED = 20260629
STANDARD_AMINO_ACIDS = set("ACDEFGHIKLMNPQRSTVWY")
DEFAULT_EQUILIBRATION_STEPS = 20000
DEFAULT_PRODUCTION_SEGMENTS = 50
DEFAULT_PRODUCTION_STEPS_PER_SEGMENT = 2000
DEFAULT_MINIMIZATION_ITERATIONS = 2000
DEFAULT_TIMESTEP_PS = 0.001
SHORT_MD_POLICY_ID = "short_complex_md_screen_v1"
DEFAULT_SOLVENT_BOX_SHAPE = "dodecahedron"
DEFAULT_GPU_TELEMETRY_POLL_SECONDS = 1.0
GAS_CONSTANT_KJ_PER_MOL_K = 0.00831446261815324
MINIMUM_MD_PRODUCTION_SAMPLES = 10
COMPLEX_MD_METRIC_UNITS = {
    "rmsd": "angstrom",
    "rmsf": "angstrom",
    "ligand_drift": "angstrom",
    "contact_occupancy": "fraction",
    "hydrogen_bond_occupancy": "fraction",
}
RMSD_SCORE_SCALE_ANGSTROM = 4.0
LIGAND_DRIFT_SCORE_SCALE_ANGSTROM = 3.0
CONTACT_CUTOFF_ANGSTROM = 4.5
RECEPTOR_CHAIN_SELECTION_VERSION = "pose_heavy_atom_contact_v2"
RECEPTOR_SELECTION_FIELDS = (
    "receptor_chain_selection_algorithm",
    "receptor_chain_atom_policy",
    "receptor_chain_contact_cutoff_angstrom",
    "original_protein_chain_count",
    "retained_protein_chain_count",
    "selected_chain_id",
    "selected_chain_ordinal",
    "selected_chain_signature_sha256",
    "selected_chain_coordinate_sha256",
    "selected_chain_minimum_distance_angstrom",
    "source_receptor_coordinate",
    "source_receptor_sha256",
    "docking_pose_sha256",
    "receptor_chain_selection_digest",
    "prepared_receptor_sha256",
)
RECEPTOR_SELECTION_DIGEST_FIELDS = (
    "source_receptor_sha256",
    "docking_pose_sha256",
    "receptor_chain_selection_algorithm",
    "receptor_chain_atom_policy",
    "receptor_chain_contact_cutoff_angstrom",
    "selected_chain_id",
    "selected_chain_signature_sha256",
    "selected_chain_coordinate_sha256",
)
RECEPTOR_SELECTION_PROTOCOL_FIELDS = (
    ("receptor_chain_selection_algorithm", "receptor_chain_selection_algorithm", ""),
    ("receptor_chain_atom_policy", "receptor_chain_atom_policy", ""),
    ("receptor_chain_contact_cutoff", "receptor_chain_contact_cutoff_angstrom", "angstrom"),
    ("original_protein_chain_count", "original_protein_chain_count", "chains"),
    ("retained_protein_chain_count", "retained_protein_chain_count", "chains"),
    ("selected_chain_id", "selected_chain_id", ""),
    ("selected_chain_ordinal", "selected_chain_ordinal", ""),
    ("selected_chain_signature_sha256", "selected_chain_signature_sha256", ""),
    ("selected_chain_coordinate_sha256", "selected_chain_coordinate_sha256", ""),
    ("selected_chain_minimum_distance", "selected_chain_minimum_distance_angstrom", "angstrom"),
    ("receptor_chain_selection_digest", "receptor_chain_selection_digest", ""),
)
UNSUPPORTED_METAL_ELEMENTS = BIOLOGICAL_METALS
HBOND_DISTANCE_CUTOFF_NM = 0.25
HBOND_ANGLE_CUTOFF_DEGREES = 120.0
MD_JOB_ID = "md_summary_ligand_dynamics"
MD_PROVENANCE_SCHEMA_VERSION = "genoligand_md_artifact_provenance_v1"
MD_PROVENANCE_ARTIFACT = "md_artifact_provenance.json"
MD_OWNED_ARTIFACTS = (
    "complex_md_manifest.json",
    "gpu_telemetry.tsv",
    "gpu_telemetry_status.tsv",
    "ligand_placement.tsv",
    MD_PROVENANCE_ARTIFACT,
    "md_contacts.tsv",
    "md_hbonds.tsv",
    "md_ligand_drift.tsv",
    "md_not_ready.tsv",
    "md_protocol.tsv",
    "md_rmsd.tsv",
    "md_rmsf.tsv",
    "md_run_manifest.json",
    "md_state.tsv",
    "md_summary.tsv",
    "md_trajectory.tsv",
    "peptide_conformer_coordinates.tsv",
    "peptide_conformer_ensemble.tsv",
    "peptide_conformer_metrics.tsv",
    "peptide_conformer_summary.tsv",
    "receptor_prepared.pdb",
)
DOCKING_ARTIFACT_FIELDS = (
    "receptor_coordinate",
    "receptor_pdb",
    "receptor_pdbqt",
    "ligand_sdf",
    "ligand_pdbqt",
    "pose",
    "pose_sdf",
    "log",
)


@dataclass
class ReceptorCoordinate:
    path: Path
    topology: Any
    atoms: list[dict[str, str]]
    positions: Any
    positions_angstrom: Any
    preparation: str
    risk_flags: list[str]
    model_metadata: dict[str, Any] = field(default_factory=dict)


@dataclass
class GpuTelemetry:
    rows: list[dict[str, str]]
    available: bool = False
    failure_reason: str = ""
    lock: Any = field(default_factory=threading.Lock, repr=False)


class MdProvenanceError(ValueError):
    pass


@dataclass(frozen=True)
class MdInvocationBinding:
    run_dir: Path
    run_id: str
    job_id: str
    invocation_id: str
    candidate_id: str
    candidate_digest: str
    input_id: str
    record_id: str
    input_digest: str
    source_digest: str
    input_kind: str
    input_value: str
    receptor_coordinate: Path | None
    docking_ligand_sdf: Path | None
    docking_ligand_pdbqt: Path | None
    docking_pose_pdbqt: Path | None
    docking_pose_sdf: Path | None
    output_dir: Path
    docking_receptor_components: tuple[dict[str, Any], ...] = ()
    docking_receptor_contact_chains: tuple[dict[str, Any], ...] = ()


def main() -> int:
    args = parse_worker_args()
    job = read_job(args.job)
    try:
        result_path = resolve_worker_output_path(job, args.out)
    except MdProvenanceError:
        return 2
    if result_path.exists():
        if not result_path.is_file():
            return 2
        result_path.unlink()
    try:
        execution = run_md_summary_job(job)
    except Exception as exc:  # pragma: no cover - depends on local scientific runtime
        reason = safe_note(str(exc)) or "md_summary_worker_failed"
        execution = md_execution("failed", reason, f"MD summary worker failed: {reason}", True)
    write_result(
        result_path,
        job,
        execution.result_status,
        execution.engine_version,
        execution_status=execution.status,
        reason=execution.reason,
        outcomes=execution.outcome_dicts(),
    )
    try:
        bind_worker_result(result_path, job)
    except MdProvenanceError:
        result_path.unlink(missing_ok=True)
        return 2
    return 0 if execution.status in {"completed", "not_applicable"} else 2


def md_execution(status: str, reason: str, version: str, required: bool, row_count: int = 0) -> WorkerExecution:
    return WorkerExecution(
        status,
        reason,
        version,
        (WorkerOutcome(status, reason, required, "md_summary", row_count),),
    )


def run_md_summary_job(job: dict[str, Any]) -> WorkerExecution:
    try:
        run_dir = resolve_md_run_dir(job)
        validate_md_job_identity(job)
        candidate_id = str(job.get("candidate_id", "")).strip()
        if candidate_id:
            clear_previous_md_outputs(run_dir, candidate_id)
        if not has_applicable_md_input(job, run_dir):
            return md_execution(
                "not_applicable",
                "no_applicable_md_input",
                "MD summary not executed",
                False,
            )
        if not candidate_id:
            raise MdProvenanceError("candidate_id_missing")
        binding = resolve_md_invocation(job)
    except MdProvenanceError as exc:
        reason = safe_note(str(exc)) or "md_provenance_validation_failed"
        try:
            record_failed_md_index(job, reason)
        except MdProvenanceError:
            pass
        return md_execution(
            "failed",
            reason,
            f"MD summary provenance validation failed: {reason}",
            True,
        )
    science_store = runtime_path(str(job.get("science_store", "")))
    candidate_id = binding.candidate_id
    input_id = binding.input_id
    smiles = binding.input_value if binding.input_kind == "small_molecule" else ""
    sequence = binding.input_value if binding.input_kind == "peptide" else ""
    receptor_path = binding.receptor_coordinate

    import numpy as np
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
                binding=binding,
            )
            return md_execution(
                "not_ready",
                reason,
                f"RDKit {rdBase.rdkitVersion}; method=rdkit_peptide_conformer_ensemble; status=not_ready",
                True,
            )
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
            binding,
        )
        return md_execution(
            "completed",
            "peptide_conformer_completed",
            f"RDKit {rdBase.rdkitVersion}; method=rdkit_peptide_conformer_ensemble; input_kind=peptide",
            True,
            1,
        )

    import openmm
    from openmm import unit

    assert binding.docking_pose_sdf is not None
    assert binding.docking_ligand_sdf is not None
    assert binding.docking_ligand_pdbqt is not None
    assert binding.docking_pose_pdbqt is not None
    mol = read_docking_pose_ligand(
        binding.docking_pose_sdf,
        binding.docking_ligand_sdf,
        binding.docking_ligand_pdbqt,
        binding.docking_pose_pdbqt,
        Chem,
    )
    if mol is None:
        reason = "missing_docking_pose_sdf"
        write_md_not_ready(
            run_dir,
            "missing AutoDock Vina docking pose SDF for complex MD",
            [reason],
            binding=binding,
        )
        return md_execution("not_ready", reason, openmm_version_text(openmm, "not_ready", reason), True)

    try:
        assert receptor_path is not None
        metal_components = receptor_metal_components(receptor_path, unit)
        if metal_components:
            reason = "unsupported_metal_parameterization"
            details = {
                "metal_components": metal_components,
                "source_receptor_coordinate": relative_run_artifact(binding, receptor_path),
                "source_receptor_sha256": sha256_file(receptor_path),
            }
            write_md_not_ready(
                run_dir,
                reason,
                [reason, *(f"metal_component:{row['identity']}" for row in metal_components)],
                details=details,
                binding=binding,
            )
            return md_execution("not_ready", reason, openmm_version_text(openmm, "not_ready", reason), True)
        chain_selection = select_receptor_chain_from_pose(
            receptor_path,
            mol,
            unit,
            binding.docking_receptor_contact_chains,
        )
        chain_selection["source_receptor_sha256"] = sha256_file(receptor_path)
        chain_selection["docking_pose_sha256"] = sha256_file(binding.docking_pose_sdf)
        selection_binding = {
            key: chain_selection[key]
            for key in RECEPTOR_SELECTION_DIGEST_FIELDS
        }
        chain_selection["receptor_chain_selection_digest"] = hashlib.sha256(
            json.dumps(selection_binding, sort_keys=True, separators=(",", ":")).encode("utf-8")
        ).hexdigest()
        receptor = load_receptor_coordinate(
            receptor_path,
            unit,
            selected_chain_ordinal=int(chain_selection["selected_chain_ordinal"]),
        )
        receptor.model_metadata.update(chain_selection)
        receptor.model_metadata["source_receptor_coordinate"] = relative_run_artifact(binding, receptor_path)
        receptor.model_metadata["source_receptor_sha256"] = sha256_file(receptor_path)
        receptor = write_invocation_prepared_receptor(receptor, binding.output_dir / "receptor_prepared.pdb")
    except Exception as exc:  # noqa: BLE001 - not-ready artifact is the contract for unusable coordinates
        message = safe_note(str(exc))
        stable_reasons = (
            "no_receptor_chain_within_pose_contact_cutoff",
            "ambiguous_receptor_chain_contacts",
            "docking_pose_heavy_atom_coordinates_invalid",
            "docking_receptor_contact_chain_mismatch",
            "selected_receptor_chain_ordinal_invalid",
        )
        reason = next((code for code in stable_reasons if message.startswith(code)), "receptor_coordinate_preparation_failed")
        write_md_not_ready(
            run_dir,
            reason,
            [reason],
            details={"error": message},
            binding=binding,
        )
        return md_execution("not_ready", reason, openmm_version_text(openmm, "not_ready", reason), True)
    if not receptor.atoms:
        reason = "receptor_coordinate_parse_failed"
        write_md_not_ready(run_dir, reason, [reason], binding=binding)
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
        write_md_not_ready(run_dir, sampling_reason, [sampling_reason], binding=binding)
        return md_execution(
            "not_ready",
            sampling_reason,
            openmm_version_text(openmm, "not_ready", sampling_reason),
            True,
        )
    sampling_policy = build_md_sampling_policy(
        timestep_ps,
        equilibration_steps,
        production_segments,
        production_steps_per_segment,
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
    platform_policy = md_platform_policy(job)
    gpu_telemetry = GpuTelemetry(rows=[])
    system, role_ranges, context_positions, model_metadata = build_complex_system(
        openmm,
        unit,
        mol,
        off_mol,
        receptor,
        ligand_positions,
        science_store,
    )
    model_metadata["sampling_policy"] = sampling_policy
    integrator = openmm.LangevinMiddleIntegrator(
        300 * unit.kelvin,
        1 / unit.picosecond,
        timestep_ps * unit.picoseconds,
    )
    integrator.setRandomNumberSeed(SIMULATION_SEED)
    sample_gpu_telemetry(gpu_telemetry, "baseline_before_openmm_context")
    try:
        context, platform_name, platform_properties, platform_failures = create_openmm_context(
            openmm,
            system,
            integrator,
            platform_policy,
        )
    except ValueError as exc:
        reason = safe_note(str(exc)) or "openmm_platform_unavailable"
        write_md_not_ready(run_dir, reason, [reason], binding=binding)
        return md_execution("not_ready", reason, openmm_version_text(openmm, "not_ready", reason), True)
    sample_gpu_telemetry(gpu_telemetry, "after_openmm_context")
    device_evidence = openmm_platform_execution_evidence(
        platform_policy,
        platform_name,
        available_openmm_platforms(openmm),
        platform_properties,
        platform_failures,
    )
    model_metadata["execution_device"] = device_evidence
    context.setPositions(context_positions)
    assert_openmm_state_finite(context, unit, "set_positions")
    sample_gpu_telemetry(gpu_telemetry, "after_positions_set")
    pre_minimization_energy = potential_energy_kj_mol(context, unit)
    minimize_openmm(
        openmm,
        context,
        unit,
        env_int("GENOLIGAND_MD_MINIMIZATION_ITERATIONS", DEFAULT_MINIMIZATION_ITERATIONS),
    )
    assert_openmm_state_finite(context, unit, "post_minimization")
    post_minimization_energy = potential_energy_kj_mol(context, unit)
    sample_gpu_telemetry(gpu_telemetry, "after_minimization")
    context.setVelocitiesToTemperature(300 * unit.kelvin, SIMULATION_SEED)
    total_step = 0
    if equilibration_steps > 0:
        with GpuTelemetryMonitor(
            gpu_telemetry,
            enabled=platform_name == "CUDA",
            label_prefix="equilibration_poll",
            interval_seconds=telemetry_poll_seconds,
        ):
            integrate_openmm(integrator, equilibration_steps, "equilibration")
        assert_openmm_state_finite(context, unit, "equilibration")
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
            integrate_openmm(integrator, production_steps_per_segment, f"production_segment_{segment + 1}")
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
    device_evidence["nvidia_smi_available"] = bool(gpu_status["nvidia_smi_available"])
    device_evidence["gpu_observed"] = bool(gpu_status["gpu_observed"])
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
        *(
            [f"platform_fallback:{device_evidence['fallback_reason']}"]
            if device_evidence["fallback_reason"]
            else []
        ),
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
        binding,
        openmm.version.full_version,
        platform_policy,
        platform_failures,
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
    if os.name != "nt" and len(value) >= 3 and value[1] == ":" and value[2] in "\\/":
        drive = value[0].lower()
        rest = value[3:].replace("\\", "/")
        return Path(f"/mnt/{drive}/{rest}")
    return path


def candidate_artifact_key(candidate_id: str) -> str:
    cleaned = "".join(ch if ch.isalnum() or ch in "-_" else "_" for ch in candidate_id).strip("_")
    digest = hashlib.sha256(candidate_id.encode("utf-8")).hexdigest()[:12]
    return f"{cleaned[:64] or 'candidate'}-{digest}"


def md_candidate_dir(run_dir: Path, candidate_id: str) -> Path:
    return run_dir / "tool_outputs" / "md" / candidate_artifact_key(candidate_id)


def md_output_dir(run_dir: Path, candidate_id: str, invocation_id: str) -> Path:
    return md_candidate_dir(run_dir, candidate_id) / candidate_artifact_key(invocation_id)


def md_artifact_path(
    binding: MdInvocationBinding,
    name: str,
    *,
    must_exist: bool = True,
) -> Path:
    return resolve_run_artifact(binding.run_dir, binding.output_dir / name, must_exist=must_exist)


def md_artifact_relative(
    binding: MdInvocationBinding,
    name: str,
    *,
    must_exist: bool = True,
) -> str:
    return md_artifact_path(binding, name, must_exist=must_exist).relative_to(binding.run_dir).as_posix()


def atomic_write_json(path: Path, value: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(f".{path.name}.{os.getpid()}.tmp")
    temporary.write_text(
        json.dumps(value, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    temporary.replace(path)


def upsert_md_manifest_index(
    binding: MdInvocationBinding,
    *,
    status: str,
    reason: str,
    manifest: str,
    artifact_provenance: str,
) -> None:
    path = binding.run_dir / "tool_outputs" / "md_manifest_index.json"
    current: dict[str, Any] = {}
    if path.is_file():
        try:
            current = json.loads(path.read_text(encoding="utf-8-sig"))
        except (OSError, json.JSONDecodeError) as exc:
            raise MdProvenanceError("md_manifest_index_invalid") from exc
    if current and (
        current.get("schema_version") != "genoligand_md_manifest_index_v1"
        or current.get("run_id") != binding.run_id
        or not isinstance(current.get("entries"), list)
    ):
        raise MdProvenanceError("md_manifest_index_invalid")
    entries = {
        str(row.get("candidate_id", "")): row
        for row in current.get("entries", [])
        if isinstance(row, dict) and str(row.get("candidate_id", ""))
    }
    entries[binding.candidate_id] = {
        "candidate_id": binding.candidate_id,
        "input_id": binding.input_id,
        "record_id": binding.record_id,
        "candidate_digest": binding.candidate_digest,
        "input_digest": binding.input_digest,
        "source_digest": binding.source_digest,
        "job_id": binding.job_id,
        "invocation_id": binding.invocation_id,
        "status": status,
        "reason": reason,
        "manifest": manifest,
        "artifact_provenance": artifact_provenance,
    }
    atomic_write_json(
        path,
        {
            "schema_version": "genoligand_md_manifest_index_v1",
            "run_id": binding.run_id,
            "entries": [entries[key] for key in sorted(entries)],
        },
    )


def upsert_md_summary(run_dir: Path, row: list[str]) -> None:
    path = run_dir / "tool_outputs" / "md_summary.tsv"
    header = [
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
    rows: dict[str, list[str]] = {}
    if path.is_file():
        with path.open("r", encoding="utf-8-sig", newline="") as handle:
            reader = csv.DictReader(handle, delimiter="\t")
            if reader.fieldnames != header:
                raise MdProvenanceError("md_summary_schema_invalid")
            for current in reader:
                candidate_id = str(current.get("candidate_id", "")).strip()
                if candidate_id:
                    rows[candidate_id] = [str(current.get(name, "")) for name in header]
    rows[row[0]] = row
    temporary = path.with_name(f".{path.name}.{os.getpid()}.tmp")
    write_tsv(temporary, header, [rows[key] for key in sorted(rows)])
    temporary.replace(path)


def failed_md_binding(job: dict[str, Any]) -> MdInvocationBinding:
    run_dir = resolve_md_run_dir(job)
    fields = {
        name: str(job.get(name, "")).strip()
        for name in (
            "run_id",
            "job_id",
            "invocation_id",
            "candidate_id",
            "candidate_digest",
            "input_id",
            "record_id",
            "input_digest",
            "source_digest",
        )
    }
    missing = [name for name, value in fields.items() if not value]
    if missing:
        raise MdProvenanceError(f"md_failure_binding_missing:{','.join(missing)}")
    return MdInvocationBinding(
        run_dir=run_dir,
        run_id=fields["run_id"],
        job_id=fields["job_id"],
        invocation_id=fields["invocation_id"],
        candidate_id=fields["candidate_id"],
        candidate_digest=fields["candidate_digest"],
        input_id=fields["input_id"],
        record_id=fields["record_id"],
        input_digest=fields["input_digest"],
        source_digest=fields["source_digest"],
        input_kind="",
        input_value="",
        receptor_coordinate=None,
        docking_ligand_sdf=None,
        docking_ligand_pdbqt=None,
        docking_pose_pdbqt=None,
        docking_pose_sdf=None,
        output_dir=md_output_dir(
            run_dir,
            fields["candidate_id"],
            fields["invocation_id"],
        ).resolve(),
    )


def record_failed_md_index(job: dict[str, Any], reason: str) -> None:
    upsert_md_manifest_index(
        failed_md_binding(job),
        status="failed",
        reason=reason,
        manifest="",
        artifact_provenance="",
    )


def canonical_digest(value: Any) -> str:
    payload = json.dumps(
        value,
        ensure_ascii=True,
        sort_keys=True,
        separators=(",", ":"),
    ).encode("utf-8")
    return hashlib.sha256(payload).hexdigest()


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def valid_sha256(value: str) -> bool:
    return len(value) == 64 and all(ch in "0123456789abcdef" for ch in value)


def resolve_md_run_dir(job: dict[str, Any]) -> Path:
    value = str(job.get("run_dir", "")).strip()
    if not value:
        raise MdProvenanceError("run_dir_missing")
    try:
        run_dir = runtime_path(value).resolve(strict=True)
    except OSError as exc:
        raise MdProvenanceError("run_dir_unavailable") from exc
    if not run_dir.is_dir():
        raise MdProvenanceError("run_dir_not_directory")
    return run_dir


def resolve_run_artifact(
    run_dir: Path,
    artifact: str | Path,
    *,
    must_exist: bool = True,
) -> Path:
    try:
        root = run_dir.resolve(strict=True)
    except OSError as exc:
        raise MdProvenanceError("run_dir_unavailable") from exc
    value = str(artifact).strip()
    if not value:
        raise MdProvenanceError("artifact_path_missing")
    candidate = runtime_artifact_path(artifact)
    if not candidate.is_absolute():
        candidate = root / candidate
    try:
        candidate.relative_to(root)
    except ValueError as exc:
        raise MdProvenanceError(f"artifact_outside_current_run:{value}") from exc
    try:
        resolved = candidate.resolve(strict=must_exist)
    except OSError as exc:
        raise MdProvenanceError(f"artifact_unavailable:{value}") from exc
    try:
        resolved.relative_to(root)
    except ValueError as exc:
        raise MdProvenanceError(f"artifact_outside_current_run:{value}") from exc
    if must_exist and not resolved.is_file():
        raise MdProvenanceError(f"artifact_not_file:{value}")
    return resolved


def resolve_worker_output_path(job: dict[str, Any], output: str | Path) -> Path:
    return resolve_run_artifact(resolve_md_run_dir(job), output, must_exist=False)


def read_tsv_dicts(path: Path) -> list[dict[str, str]]:
    try:
        with path.open("r", encoding="utf-8-sig", newline="") as handle:
            return [
                {str(key): str(value or "").strip() for key, value in row.items()}
                for row in csv.DictReader(handle, delimiter="\t")
            ]
    except (OSError, csv.Error) as exc:
        raise MdProvenanceError(f"artifact_parse_failed:{path.name}") from exc


def read_json_object(path: Path) -> dict[str, Any]:
    try:
        with path.open("r", encoding="utf-8-sig") as handle:
            value = json.load(handle)
    except (OSError, json.JSONDecodeError) as exc:
        raise MdProvenanceError(f"artifact_parse_failed:{path.name}") from exc
    if not isinstance(value, dict):
        raise MdProvenanceError(f"artifact_schema_invalid:{path.name}")
    return value


def selected_md_candidate(
    run_dir: Path,
    candidate_id: str = "",
    input_id: str = "",
    record_id: str = "",
) -> tuple[dict[str, str], str]:
    path = resolve_run_artifact(run_dir, "complex_candidate_selection.tsv")
    rows = read_tsv_dicts(path)
    if candidate_id:
        rows = [row for row in rows if row.get("candidate_id") == candidate_id]
    if input_id:
        rows = [row for row in rows if row.get("input_id") == input_id]
    if record_id:
        rows = [row for row in rows if row.get("record_id") == record_id]
    if len(rows) != 1:
        raise MdProvenanceError("selected_md_candidate_must_be_unique")
    row = rows[0]
    if (
        not row.get("candidate_id")
        or not row.get("input_id")
        or not row.get("record_id")
    ):
        raise MdProvenanceError("selected_md_candidate_binding_incomplete")
    return row, candidate_selection_digest(row)


def selected_md_input(
    run_dir: Path,
    input_id: str,
    record_id: str,
) -> tuple[dict[str, str], str, str, str]:
    path = resolve_run_artifact(run_dir, "input_conversion_manifest.tsv")
    rows = [
        row
        for row in read_tsv_dicts(path)
        if row.get("input_id") == input_id
        and row.get("record_id") == record_id
        and row.get("conversion_status") == "converted"
        and row.get("canonical_kind") in {"small_molecule", "peptide"}
    ]
    if len(rows) != 1:
        raise MdProvenanceError("selected_md_input_must_be_unique")
    row = rows[0]
    input_value = decode_tsv_cell(row.get("canonical_value", "")).strip()
    if not input_value:
        raise MdProvenanceError("selected_md_input_value_missing")
    source_digest = row.get("content_sha256", "").lower()
    if not valid_sha256(source_digest):
        raise MdProvenanceError("selected_md_input_source_digest_invalid")
    input_digest = canonical_digest(
        {
            "input_id": input_id,
            "record_id": row.get("record_id", ""),
            "canonical_kind": row.get("canonical_kind", ""),
            "canonical_value": input_value,
            "source_content_sha256": source_digest,
        }
    )
    return row, input_value, source_digest, input_digest


def validate_md_job_identity(job: dict[str, Any]) -> tuple[str, str]:
    job_id = str(job.get("job_id", "")).strip()
    invocation_id = str(job.get("invocation_id", "")).strip()
    if job_id != MD_JOB_ID and not job_id.startswith(f"{MD_JOB_ID}__"):
        raise MdProvenanceError("md_job_id_mismatch")
    if not invocation_id:
        raise MdProvenanceError("md_invocation_id_missing")
    return job_id, invocation_id


def has_applicable_md_input(job: dict[str, Any], run_dir: Path) -> bool:
    conversion_path = run_dir / "input_conversion_manifest.tsv"
    if conversion_path.exists() or conversion_path.is_symlink():
        path = resolve_run_artifact(run_dir, conversion_path)
        if any(
            row.get("conversion_status") == "converted"
            and row.get("canonical_kind") in {"small_molecule", "peptide"}
            and decode_tsv_cell(row.get("canonical_value", "")).strip()
            for row in read_tsv_dicts(path)
        ):
            return True
    input_manifest_value = str(job.get("input_manifest", "")).strip()
    if not input_manifest_value:
        return True
    input_manifest = runtime_path(input_manifest_value)
    try:
        smiles, _ = read_small_molecule_input(input_manifest)
        sequence, _ = read_peptide_sequence_input(input_manifest)
    except (OSError, ValueError, json.JSONDecodeError):
        return True
    return bool(smiles or sequence)


def validate_docking_binding(
    run_dir: Path,
    run_id: str,
    docking_job_id: str,
    docking_invocation_id: str,
    docking_manifest_digest: str,
    candidate_id: str,
    candidate_digest: str,
    input_id: str,
    record_id: str,
    source_digest: str,
    input_digest: str,
    manifest_artifact: str = "tool_outputs/docking_manifest.json",
) -> dict[str, Any]:
    manifest_path = resolve_run_artifact(run_dir, manifest_artifact)
    if not valid_sha256(docking_manifest_digest) or sha256_file(manifest_path) != docking_manifest_digest:
        raise MdProvenanceError("docking_manifest_sha256_mismatch")
    manifest = read_json_object(manifest_path)
    checks = (
        (
            manifest.get("artifact_schema_version") == "genoligand_docking_artifacts_v1",
            "docking_artifact_schema_version_mismatch",
        ),
        (manifest.get("status") == "completed", "docking_status_not_completed"),
        (manifest.get("run_id") == run_id, "docking_run_id_mismatch"),
        (manifest.get("job_id") == docking_job_id, "docking_job_id_mismatch"),
        (
            manifest.get("invocation_id") == docking_invocation_id,
            "docking_invocation_id_mismatch",
        ),
        (manifest.get("candidate_id") == candidate_id, "docking_candidate_id_mismatch"),
        (
            manifest.get("candidate_digest") == candidate_digest,
            "docking_candidate_digest_mismatch",
        ),
        (manifest.get("input_id") == input_id, "docking_input_id_mismatch"),
        (manifest.get("record_id") == record_id, "docking_record_id_mismatch"),
        (
            str(manifest.get("source_content_sha256", "")).lower() == source_digest,
            "docking_source_digest_mismatch",
        ),
    )
    for matches, reason in checks:
        if not matches:
            raise MdProvenanceError(reason)
    seed_provenance = manifest.get("seed_provenance")
    if not isinstance(seed_provenance, dict) or seed_provenance.get("input_digest") != input_digest:
        raise MdProvenanceError("docking_input_digest_mismatch")
    if manifest.get("input_digest") != input_digest:
        raise MdProvenanceError("docking_input_digest_mismatch")

    resolved: dict[str, Any] = {}
    for field_name in DOCKING_ARTIFACT_FIELDS:
        value = manifest.get(field_name)
        if value in {None, ""}:
            raise MdProvenanceError(f"docking_artifact_missing:{field_name}")
        if not isinstance(value, str):
            raise MdProvenanceError(f"docking_artifact_path_invalid:{field_name}")
        resolved[field_name] = resolve_run_artifact(run_dir, value)

    artifact_rows = manifest.get("artifacts")
    if not isinstance(artifact_rows, list) or not artifact_rows:
        raise MdProvenanceError("docking_artifacts_schema_invalid")
    expected_names = set(DOCKING_ARTIFACT_FIELDS)
    actual_names: set[str] = set()
    for row in artifact_rows:
        if not isinstance(row, dict):
            raise MdProvenanceError("docking_artifacts_schema_invalid")
        artifact_name = str(row.get("artifact_name", "")).strip()
        if artifact_name in actual_names:
            raise MdProvenanceError(f"docking_artifact_duplicate:{artifact_name}")
        actual_names.add(artifact_name)
        for field_name, expected in {
            "run_id": run_id,
            "job_id": docking_job_id,
            "invocation_id": docking_invocation_id,
            "candidate_id": candidate_id,
            "candidate_digest": candidate_digest,
            "input_id": input_id,
            "record_id": record_id,
            "input_digest": input_digest,
            "source_digest": source_digest,
        }.items():
            if row.get(field_name) != expected:
                raise MdProvenanceError(f"docking_artifact_{field_name}_mismatch")
        path = resolve_run_artifact(run_dir, str(row.get("artifact_path", "")))
        if artifact_name in resolved and path != resolved[artifact_name]:
            raise MdProvenanceError(f"docking_artifact_path_mismatch:{artifact_name}")
        expected = str(row.get("artifact_sha256", "")).lower()
        if not valid_sha256(expected) or sha256_file(path) != expected:
            raise MdProvenanceError(f"docking_artifact_sha256_mismatch:{path.name}")
    if actual_names != expected_names:
        raise MdProvenanceError("docking_artifact_set_mismatch")

    receptor = resolved["receptor_coordinate"]
    receptor_digest = str(manifest.get("receptor_digest", "")).lower()
    receptor_row = next(
        row for row in artifact_rows if row.get("artifact_name") == "receptor_coordinate"
    )
    if (
        not valid_sha256(receptor_digest)
        or manifest.get("receptor_sha256") != receptor_digest
        or receptor_row.get("receptor_digest") != receptor_digest
        or sha256_file(receptor) != receptor_digest
    ):
        raise MdProvenanceError("docking_receptor_sha256_mismatch")
    components = manifest.get("retained_receptor_components")
    contact_chains = manifest.get("receptor_contact_chains")
    if not isinstance(components, list) or not all(isinstance(row, dict) for row in components):
        raise MdProvenanceError("docking_receptor_components_invalid")
    if not isinstance(contact_chains, list) or not contact_chains:
        raise MdProvenanceError("docking_receptor_contact_chains_invalid")
    identities: set[tuple[str, int, str]] = set()
    for row in contact_chains:
        if not isinstance(row, dict):
            raise MdProvenanceError("docking_receptor_contact_chains_invalid")
        chain_id = row.get("chain_id")
        chain_ordinal = row.get("chain_ordinal")
        signature = row.get("chain_signature_sha256")
        minimum = row.get("minimum_distance_angstrom")
        if (
            not isinstance(chain_id, str)
            or isinstance(chain_ordinal, bool)
            or not isinstance(chain_ordinal, int)
            or chain_ordinal < 0
            or not isinstance(signature, str)
            or not valid_sha256(signature)
            or isinstance(minimum, bool)
            or not isinstance(minimum, (int, float))
            or not math.isfinite(float(minimum))
            or float(minimum) < 0.0
        ):
            raise MdProvenanceError("docking_receptor_contact_chains_invalid")
        identity = (chain_id, chain_ordinal, signature)
        if identity in identities:
            raise MdProvenanceError("docking_receptor_contact_chain_duplicate")
        identities.add(identity)
    resolved["receptor_components"] = tuple(components)
    resolved["receptor_contact_chains"] = tuple(contact_chains)
    return resolved


def resolve_md_invocation(job: dict[str, Any]) -> MdInvocationBinding:
    run_dir = resolve_md_run_dir(job)
    job_id, invocation_id = validate_md_job_identity(job)

    selection, candidate_digest = selected_md_candidate(
        run_dir,
        str(job.get("candidate_id", "")).strip(),
        str(job.get("input_id", "")).strip(),
        str(job.get("record_id", "")).strip(),
    )
    candidate_id = selection["candidate_id"]
    input_id = selection["input_id"]
    record_id = selection["record_id"]
    input_row, input_value, source_digest, input_digest = selected_md_input(
        run_dir,
        input_id,
        record_id,
    )
    expected_values = (
        ("candidate_id", candidate_id, "job_candidate_id_mismatch"),
        ("candidate_digest", candidate_digest, "job_candidate_digest_mismatch"),
        ("input_id", input_id, "job_input_id_mismatch"),
        ("record_id", record_id, "job_record_id_mismatch"),
        ("input_digest", input_digest, "job_input_digest_mismatch"),
        ("source_digest", source_digest, "job_source_digest_mismatch"),
    )
    for field_name, current, reason in expected_values:
        expected = str(job.get(field_name, "")).strip()
        if not expected:
            raise MdProvenanceError(f"{field_name}_missing")
        if expected != current:
            raise MdProvenanceError(reason)

    receptor_coordinate = None
    docking_ligand_sdf = None
    docking_ligand_pdbqt = None
    docking_pose_pdbqt = None
    docking_pose_sdf = None
    if input_row["canonical_kind"] == "small_molecule":
        run_id = str(job.get("run_id", "")).strip() or run_dir.name
        docking_manifest = str(job.get("docking_manifest", "")).strip()
        if not docking_manifest:
            raise MdProvenanceError("docking_manifest_missing")
        docking_job_id = str(job.get("docking_job_id", "")).strip()
        docking_invocation_id = str(job.get("docking_invocation_id", "")).strip()
        docking_manifest_digest = str(job.get("docking_manifest_digest", "")).strip().lower()
        if not docking_job_id or not docking_invocation_id or not valid_sha256(docking_manifest_digest):
            raise MdProvenanceError("docking_manifest_binding_missing")
        docking_artifacts = validate_docking_binding(
            run_dir,
            run_id,
            docking_job_id,
            docking_invocation_id,
            docking_manifest_digest,
            candidate_id,
            candidate_digest,
            input_id,
            input_row.get("record_id", ""),
            source_digest,
            input_digest,
            docking_manifest,
        )
        receptor_coordinate = docking_artifacts["receptor_pdb"]
        docking_ligand_sdf = docking_artifacts["ligand_sdf"]
        docking_ligand_pdbqt = docking_artifacts["ligand_pdbqt"]
        docking_pose_pdbqt = docking_artifacts["pose"]
        docking_pose_sdf = docking_artifacts["pose_sdf"]
    return MdInvocationBinding(
        run_dir=run_dir,
        run_id=str(job.get("run_id", "")).strip() or run_dir.name,
        job_id=job_id,
        invocation_id=invocation_id,
        candidate_id=candidate_id,
        candidate_digest=candidate_digest,
        input_id=input_id,
        record_id=record_id,
        input_digest=input_digest,
        source_digest=source_digest,
        input_kind=input_row["canonical_kind"],
        input_value=input_value,
        receptor_coordinate=receptor_coordinate,
        docking_ligand_sdf=docking_ligand_sdf,
        docking_ligand_pdbqt=docking_ligand_pdbqt,
        docking_pose_pdbqt=docking_pose_pdbqt,
        docking_pose_sdf=docking_pose_sdf,
        output_dir=md_output_dir(run_dir, candidate_id, invocation_id).resolve(),
        docking_receptor_components=tuple(
            docking_artifacts.get("receptor_components", ())
            if input_row["canonical_kind"] == "small_molecule"
            else ()
        ),
        docking_receptor_contact_chains=tuple(
            docking_artifacts.get("receptor_contact_chains", ())
            if input_row["canonical_kind"] == "small_molecule"
            else ()
        ),
    )


def clear_previous_md_outputs(run_dir: Path, candidate_id: str = "") -> None:
    root = run_dir.resolve(strict=True)
    if candidate_id:
        candidate_root = md_candidate_dir(root, candidate_id)
        if candidate_root.exists() or candidate_root.is_symlink():
            try:
                resolved = candidate_root.resolve(strict=True)
                resolved.relative_to(root)
            except (OSError, ValueError) as exc:
                raise MdProvenanceError("md_candidate_output_outside_run") from exc
            if candidate_root.is_symlink() or not candidate_root.is_dir():
                raise MdProvenanceError("md_candidate_output_not_directory")
            import shutil

            shutil.rmtree(candidate_root)
        return
    tool_outputs = root / "tool_outputs"
    if tool_outputs.is_symlink():
        try:
            resolved = tool_outputs.resolve(strict=True)
            resolved.relative_to(root)
        except (OSError, ValueError):
            tool_outputs.unlink()
            tool_outputs.mkdir()
        else:
            if not resolved.is_dir():
                tool_outputs.unlink()
                tool_outputs.mkdir()
    elif tool_outputs.exists():
        if not tool_outputs.is_dir():
            raise MdProvenanceError("tool_outputs_not_directory")
    else:
        tool_outputs.mkdir(parents=True)
    unexpected_directories = []
    for name in MD_OWNED_ARTIFACTS:
        path = tool_outputs / name
        if path.is_dir() and not path.is_symlink():
            unexpected_directories.append(name)
            continue
        if path.exists() or path.is_symlink():
            path.unlink()
    if unexpected_directories:
        raise MdProvenanceError(
            f"md_artifact_unexpected_directory:{','.join(unexpected_directories)}"
        )


def binding_fields(binding: MdInvocationBinding) -> dict[str, str]:
    return {
        "run_id": binding.run_id,
        "job_id": binding.job_id,
        "invocation_id": binding.invocation_id,
        "candidate_id": binding.candidate_id,
        "candidate_digest": binding.candidate_digest,
        "input_id": binding.input_id,
        "record_id": binding.record_id,
        "input_digest": binding.input_digest,
        "source_digest": binding.source_digest,
    }


def ensure_md_output_binding(
    run_dir: Path,
    candidate_id: str,
    input_id: str,
    binding: MdInvocationBinding,
) -> None:
    if run_dir.resolve(strict=True) != binding.run_dir:
        raise MdProvenanceError("md_output_run_dir_mismatch")
    if candidate_id != binding.candidate_id:
        raise MdProvenanceError("md_output_candidate_id_mismatch")
    if input_id != binding.input_id:
        raise MdProvenanceError("md_output_input_id_mismatch")


def relative_run_artifact(
    binding: MdInvocationBinding,
    artifact: str | Path,
    *,
    must_exist: bool = True,
) -> str:
    path = resolve_run_artifact(binding.run_dir, artifact, must_exist=must_exist)
    return path.relative_to(binding.run_dir).as_posix()


def write_json(path: Path, value: dict[str, Any]) -> None:
    with path.open("w", encoding="utf-8") as handle:
        json.dump(value, handle, indent=2, sort_keys=True)
        handle.write("\n")


def write_md_artifact_provenance(
    binding: MdInvocationBinding,
    artifacts: list[Path],
    *,
    tool_name: str,
    tool_version: str,
    parameters: dict[str, Any],
) -> Path:
    if not tool_name.strip() or not tool_version.strip():
        raise MdProvenanceError("md_tool_identity_missing")
    parameters_digest = canonical_digest(parameters)
    common = {
        **binding_fields(binding),
        "tool_name": tool_name,
        "tool_version": tool_version,
        "parameters_digest": parameters_digest,
    }
    rows = []
    seen = set()
    for artifact in artifacts:
        path = resolve_run_artifact(binding.run_dir, artifact)
        relative = path.relative_to(binding.run_dir).as_posix()
        if relative == md_artifact_relative(
            binding,
            MD_PROVENANCE_ARTIFACT,
            must_exist=False,
        ) or relative in seen:
            raise MdProvenanceError(f"duplicate_or_recursive_md_artifact:{relative}")
        seen.add(relative)
        rows.append(
            {
                **common,
                "artifact_path": relative,
                "artifact_sha256": sha256_file(path),
            }
        )
    if not rows:
        raise MdProvenanceError("md_artifact_list_empty")
    rows.sort(key=lambda row: row["artifact_path"])
    path = resolve_run_artifact(
        binding.run_dir,
        binding.output_dir / MD_PROVENANCE_ARTIFACT,
        must_exist=False,
    )
    write_json(
        path,
        {
            "schema_version": MD_PROVENANCE_SCHEMA_VERSION,
            **common,
            "artifacts": rows,
        },
    )
    return path


def validate_md_artifact_provenance(path: Path, binding: MdInvocationBinding) -> None:
    manifest_path = resolve_run_artifact(binding.run_dir, path)
    value = read_json_object(manifest_path)
    if value.get("schema_version") != MD_PROVENANCE_SCHEMA_VERSION:
        raise MdProvenanceError("provenance_schema_version_mismatch")
    for field_name, expected in binding_fields(binding).items():
        if value.get(field_name) != expected:
            raise MdProvenanceError(f"provenance_{field_name}_mismatch")
    parameters_digest = str(value.get("parameters_digest", "")).lower()
    if not valid_sha256(parameters_digest):
        raise MdProvenanceError("provenance_parameters_digest_invalid")
    tool_name = str(value.get("tool_name", "")).strip()
    tool_version = str(value.get("tool_version", "")).strip()
    if not tool_name or not tool_version:
        raise MdProvenanceError("provenance_tool_identity_invalid")
    artifacts = value.get("artifacts")
    if not isinstance(artifacts, list) or not artifacts:
        raise MdProvenanceError("provenance_artifacts_invalid")
    seen = set()
    for row in artifacts:
        if not isinstance(row, dict):
            raise MdProvenanceError("provenance_artifacts_invalid")
        for field_name, expected in binding_fields(binding).items():
            if row.get(field_name) != expected:
                raise MdProvenanceError(f"provenance_{field_name}_mismatch")
        if row.get("parameters_digest") != parameters_digest:
            raise MdProvenanceError("provenance_parameters_digest_mismatch")
        if row.get("tool_name") != tool_name or row.get("tool_version") != tool_version:
            raise MdProvenanceError("provenance_tool_identity_mismatch")
        artifact = resolve_run_artifact(binding.run_dir, str(row.get("artifact_path", "")))
        relative = artifact.relative_to(binding.run_dir).as_posix()
        if relative in seen:
            raise MdProvenanceError(f"provenance_duplicate_artifact:{relative}")
        seen.add(relative)
        expected_digest = str(row.get("artifact_sha256", "")).lower()
        if not valid_sha256(expected_digest) or sha256_file(artifact) != expected_digest:
            raise MdProvenanceError(f"artifact_sha256_mismatch:{relative}")


def finite_float(value: object, reason: str) -> float:
    try:
        number = float(str(value))
    except (TypeError, ValueError) as exc:
        raise MdProvenanceError(reason) from exc
    if not math.isfinite(number):
        raise MdProvenanceError(reason)
    return number


def validate_manifest_binding(manifest: dict[str, Any], binding: MdInvocationBinding) -> None:
    for field_name, expected in binding_fields(binding).items():
        if manifest.get(field_name) != expected:
            raise MdProvenanceError(f"md_manifest_{field_name}_mismatch")


def bound_tsv_rows(binding: MdInvocationBinding, artifact: str) -> list[dict[str, str]]:
    rows = read_tsv_dicts(md_artifact_path(binding, artifact))
    if not rows:
        raise MdProvenanceError(f"md_artifact_rows_missing:{artifact}")
    return rows


def validate_receptor_selection_semantics(
    binding: MdInvocationBinding,
    manifest: dict[str, Any],
    protocol: dict[tuple[str, str], dict[str, str]],
) -> None:
    system_value = manifest.get("system")
    system = system_value if isinstance(system_value, dict) else {}
    complex_manifest = read_json_object(md_artifact_path(binding, "complex_md_manifest.json"))
    complex_system_value = complex_manifest.get("system")
    complex_system = complex_system_value if isinstance(complex_system_value, dict) else {}
    protocol_markers = {
        parameter
        for parameter, _field_name, _unit in RECEPTOR_SELECTION_PROTOCOL_FIELDS
    }
    has_selection_schema = (
        any(field_name in system for field_name in RECEPTOR_SELECTION_FIELDS)
        or any(field_name in complex_system for field_name in RECEPTOR_SELECTION_FIELDS)
        or any(stage == "input" and parameter in protocol_markers for stage, parameter in protocol)
    )
    if not has_selection_schema:
        return

    missing = [field_name for field_name in RECEPTOR_SELECTION_FIELDS if field_name not in system]
    if missing:
        raise MdProvenanceError(f"md_receptor_selection_field_missing:{missing[0]}")
    if system["receptor_chain_selection_algorithm"] != RECEPTOR_CHAIN_SELECTION_VERSION:
        raise MdProvenanceError("md_receptor_selection_algorithm_invalid")
    if system["receptor_chain_atom_policy"] != "protein_and_ligand_heavy_atoms":
        raise MdProvenanceError("md_receptor_selection_atom_policy_invalid")

    cutoff = system["receptor_chain_contact_cutoff_angstrom"]
    minimum_distance = system["selected_chain_minimum_distance_angstrom"]
    if (
        isinstance(cutoff, bool)
        or not isinstance(cutoff, (int, float))
        or not math.isfinite(float(cutoff))
        or float(cutoff) != RECEPTOR_CHAIN_CONTACT_CUTOFF_ANGSTROM
    ):
        raise MdProvenanceError("md_receptor_selection_contact_cutoff_invalid")
    if (
        isinstance(minimum_distance, bool)
        or not isinstance(minimum_distance, (int, float))
        or not math.isfinite(float(minimum_distance))
        or not 0.0 <= float(minimum_distance) <= float(cutoff)
    ):
        raise MdProvenanceError("md_receptor_selection_minimum_distance_invalid")
    for field_name in ("original_protein_chain_count", "retained_protein_chain_count", "selected_chain_ordinal"):
        value = system[field_name]
        if isinstance(value, bool) or not isinstance(value, int) or value < 0:
            raise MdProvenanceError(f"md_receptor_selection_field_invalid:{field_name}")
    if system["original_protein_chain_count"] < 1 or system["retained_protein_chain_count"] != 1:
        raise MdProvenanceError("md_receptor_selection_chain_count_invalid")
    if not isinstance(system["selected_chain_id"], str):
        raise MdProvenanceError("md_receptor_selection_field_invalid:selected_chain_id")

    digest_fields = (
        "source_receptor_sha256",
        "docking_pose_sha256",
        "selected_chain_signature_sha256",
        "selected_chain_coordinate_sha256",
        "receptor_chain_selection_digest",
        "prepared_receptor_sha256",
    )
    for field_name in digest_fields:
        value = system[field_name]
        if not isinstance(value, str) or not valid_sha256(value):
            raise MdProvenanceError(f"md_receptor_selection_sha256_invalid:{field_name}")
    expected_selection_digest = canonical_digest(
        {field_name: system[field_name] for field_name in RECEPTOR_SELECTION_DIGEST_FIELDS}
    )
    if system["receptor_chain_selection_digest"] != expected_selection_digest:
        raise MdProvenanceError("md_receptor_selection_digest_mismatch")

    if binding.receptor_coordinate is None:
        raise MdProvenanceError("md_source_receptor_coordinate_missing")
    source_receptor = resolve_run_artifact(binding.run_dir, binding.receptor_coordinate)
    expected_source_relative = source_receptor.relative_to(binding.run_dir).as_posix()
    if system["source_receptor_coordinate"] != expected_source_relative:
        raise MdProvenanceError("md_source_receptor_coordinate_mismatch")
    if sha256_file(source_receptor) != system["source_receptor_sha256"]:
        raise MdProvenanceError("md_source_receptor_sha256_mismatch")
    if binding.docking_pose_sdf is None:
        raise MdProvenanceError("md_docking_pose_missing")
    docking_pose = resolve_run_artifact(binding.run_dir, binding.docking_pose_sdf)
    if sha256_file(docking_pose) != system["docking_pose_sha256"]:
        raise MdProvenanceError("md_docking_pose_sha256_mismatch")

    prepared_relative = md_artifact_relative(binding, "receptor_prepared.pdb")
    if manifest.get("receptor_coordinate") != prepared_relative:
        raise MdProvenanceError("md_prepared_receptor_path_mismatch")
    prepared_path = resolve_run_artifact(binding.run_dir, prepared_relative)
    prepared_sha256 = sha256_file(prepared_path)
    if prepared_sha256 != system["prepared_receptor_sha256"]:
        raise MdProvenanceError("md_prepared_receptor_sha256_mismatch")

    if not isinstance(complex_system_value, dict):
        raise MdProvenanceError("md_receptor_selection_manifest_mismatch")
    if complex_manifest.get("receptor_coordinate") != prepared_relative:
        raise MdProvenanceError("md_receptor_selection_manifest_mismatch")
    if any(complex_system.get(field_name) != system[field_name] for field_name in RECEPTOR_SELECTION_FIELDS):
        raise MdProvenanceError("md_receptor_selection_manifest_mismatch")

    for parameter, field_name, expected_unit in RECEPTOR_SELECTION_PROTOCOL_FIELDS:
        row = protocol.get(("input", parameter))
        if (
            row is None
            or row.get("value") != str(system[field_name])
            or row.get("unit", "") != expected_unit
        ):
            raise MdProvenanceError(f"md_receptor_selection_protocol_mismatch:{parameter}")

    provenance_relative = md_artifact_relative(binding, MD_PROVENANCE_ARTIFACT)
    if manifest.get("artifact_provenance") != provenance_relative:
        raise MdProvenanceError("md_prepared_receptor_provenance_mismatch")
    provenance = read_json_object(resolve_run_artifact(binding.run_dir, provenance_relative))
    provenance_rows = provenance.get("artifacts")
    if not isinstance(provenance_rows, list):
        raise MdProvenanceError("md_prepared_receptor_provenance_mismatch")
    prepared_rows = [
        row
        for row in provenance_rows
        if isinstance(row, dict) and row.get("artifact_path") == prepared_relative
    ]
    if len(prepared_rows) != 1 or prepared_rows[0].get("artifact_sha256") != prepared_sha256:
        raise MdProvenanceError("md_prepared_receptor_provenance_mismatch")


def validate_completed_md_semantics(binding: MdInvocationBinding) -> dict[str, Any]:
    manifest = read_json_object(md_artifact_path(binding, "md_run_manifest.json"))
    validate_manifest_binding(manifest, binding)
    method = str(manifest.get("method", "")).strip()
    if method == "rdkit_peptide_conformer_ensemble":
        return validate_peptide_conformer_semantics(binding, manifest)
    if method != "openmm_receptor_ligand_complex_dynamics":
        raise MdProvenanceError("md_method_invalid")
    return validate_complex_md_semantics(binding, manifest)


def validate_complex_md_semantics(
    binding: MdInvocationBinding,
    manifest: dict[str, Any],
) -> dict[str, Any]:
    if manifest.get("metric_units") != COMPLEX_MD_METRIC_UNITS:
        raise MdProvenanceError("md_metric_unit_schema_invalid")
    expected_metric_artifacts = {
        md_artifact_relative(binding, name, must_exist=False)
        for name in (
            "md_summary.tsv",
            "md_rmsd.tsv",
            "md_rmsf.tsv",
            "md_ligand_drift.tsv",
            "md_contacts.tsv",
            "md_hbonds.tsv",
        )
    }
    metric_artifacts = manifest.get("metric_artifacts")
    if not isinstance(metric_artifacts, list) or set(metric_artifacts) != expected_metric_artifacts:
        raise MdProvenanceError("md_metric_artifact_set_mismatch")
    for artifact in expected_metric_artifacts:
        if not resolve_run_artifact(
            binding.run_dir,
            artifact,
            must_exist=False,
        ).is_file():
            raise MdProvenanceError(f"md_metric_artifact_missing:{Path(artifact).name}")

    required_artifacts = {
        "trajectory": md_artifact_relative(binding, "md_trajectory.tsv"),
        "state_artifact": md_artifact_relative(binding, "md_state.tsv"),
        "protocol_artifact": md_artifact_relative(binding, "md_protocol.tsv"),
        "ligand_coordinate": md_artifact_relative(binding, "ligand_placement.tsv"),
        "complex_manifest": md_artifact_relative(binding, "complex_md_manifest.json"),
        "gpu_telemetry_artifact": md_artifact_relative(binding, "gpu_telemetry.tsv"),
        "gpu_telemetry_status_artifact": md_artifact_relative(
            binding,
            "gpu_telemetry_status.tsv",
        ),
    }
    for field_name, expected in required_artifacts.items():
        if manifest.get(field_name) != expected:
            raise MdProvenanceError(f"md_manifest_artifact_mismatch:{field_name}")
        resolve_run_artifact(binding.run_dir, expected)

    summary_rows = bound_tsv_rows(binding, "md_summary.tsv")
    if len(summary_rows) != 1 or summary_rows[0].get("candidate_id") != binding.candidate_id:
        raise MdProvenanceError("md_summary_candidate_id_mismatch")
    summary = summary_rows[0]
    metric_ranges = {
        "rmsd": (0.0, None),
        "rmsf": (0.0, None),
        "ligand_drift": (0.0, None),
        "hydrogen_bond_occupancy": (0.0, 1.0),
        "contact_occupancy": (0.0, 1.0),
        "interaction_persistence": (0.0, 1.0),
        "md_stability_score": (0.0, 1.0),
    }
    summary_values: dict[str, float] = {}
    for metric, (minimum, maximum) in metric_ranges.items():
        value = finite_float(summary.get(metric), "md_metric_value_invalid")
        if value < minimum or (maximum is not None and value > maximum):
            raise MdProvenanceError("md_metric_value_invalid")
        summary_values[metric] = value

    metric_files = {
        "md_rmsd.tsv": ("rmsd", "angstrom"),
        "md_rmsf.tsv": ("rmsf", "angstrom"),
        "md_ligand_drift.tsv": ("ligand_drift", "angstrom"),
        "md_contacts.tsv": ("contact_occupancy", "fraction"),
        "md_hbonds.tsv": ("hydrogen_bond_occupancy", "fraction"),
    }
    for file_name, (metric, unit) in metric_files.items():
        rows = bound_tsv_rows(binding, file_name)
        if len(rows) != 1:
            raise MdProvenanceError(f"md_metric_row_count_invalid:{file_name}")
        row = rows[0]
        if row.get("candidate_id") != binding.candidate_id:
            raise MdProvenanceError("md_metric_candidate_id_mismatch")
        if row.get("input_id") != binding.input_id:
            raise MdProvenanceError("md_metric_input_id_mismatch")
        if row.get("metric") != metric:
            raise MdProvenanceError("md_metric_name_mismatch")
        if row.get("unit") != unit:
            raise MdProvenanceError("md_metric_unit_invalid")
        value = finite_float(row.get("value"), "md_metric_value_invalid")
        if abs(value - summary_values[metric]) > 1e-6:
            raise MdProvenanceError("md_metric_summary_mismatch")

    protocol_rows = bound_tsv_rows(binding, "md_protocol.tsv")
    protocol: dict[tuple[str, str], dict[str, str]] = {}
    for row in protocol_rows:
        if row.get("candidate_id") != binding.candidate_id:
            raise MdProvenanceError("md_protocol_candidate_id_mismatch")
        if row.get("input_id") != binding.input_id:
            raise MdProvenanceError("md_protocol_input_id_mismatch")
        key = (row.get("stage", ""), row.get("parameter", ""))
        if key in protocol:
            raise MdProvenanceError("md_protocol_parameter_duplicate")
        protocol[key] = row
    required_protocol = {
        ("platform", "openmm_platform"): "",
        ("platform", "platform_policy"): "",
        ("platform", "available_openmm_platforms"): "",
        ("platform", "cuda_platform_detected"): "",
        ("platform", "context_creation_failures"): "",
        ("platform", "fallback_reason"): "",
        ("sampling_policy", "policy_id"): "",
        ("sampling_policy", "purpose"): "",
        ("integrator", "time_step"): "ps",
        ("production", "total_steps"): "steps",
        ("output", "frame_count"): "frames",
        ("output", "state_rows"): "rows",
    }
    for key, unit in required_protocol.items():
        row = protocol.get(key)
        if row is None:
            raise MdProvenanceError("md_protocol_parameter_missing")
        if row.get("unit", "") != unit:
            raise MdProvenanceError("md_protocol_unit_invalid")
    selected_platform = protocol[("platform", "openmm_platform")].get("value", "")
    platform_policy = protocol[("platform", "platform_policy")].get("value", "")
    fallback_reason = protocol[("platform", "fallback_reason")].get("value", "")
    available_platforms = manifest.get("available_openmm_platforms")
    context_failures = manifest.get("platform_context_failures")
    if (
        selected_platform != manifest.get("platform")
        or platform_policy != manifest.get("platform_policy")
        or platform_policy not in {"required", "preferred", "disabled"}
        or not isinstance(available_platforms, list)
        or not all(isinstance(name, str) and name for name in available_platforms)
        or selected_platform not in available_platforms
        or not isinstance(context_failures, list)
        or not all(isinstance(reason, str) and reason for reason in context_failures)
        or protocol[("platform", "available_openmm_platforms")].get("value", "")
        != ",".join(available_platforms)
        or protocol[("platform", "context_creation_failures")].get("value", "")
        != ";".join(context_failures)
    ):
        raise MdProvenanceError("md_platform_evidence_invalid")
    cuda_detected = "CUDA" in available_platforms
    if (
        manifest.get("cuda_platform_detected") is not cuda_detected
        or protocol[("platform", "cuda_platform_detected")].get("value", "")
        != bool_text(cuda_detected)
        or manifest.get("platform_fallback_reason") != fallback_reason
        or (selected_platform == "CUDA" and fallback_reason)
        or (selected_platform != "CUDA" and not fallback_reason)
        or (platform_policy == "required" and selected_platform != "CUDA")
        or (platform_policy == "disabled" and selected_platform == "CUDA")
        or (
            platform_policy == "preferred"
            and selected_platform != "CUDA"
            and cuda_detected
            and not any(reason.startswith("CUDA:") for reason in context_failures)
        )
        or (
            platform_policy == "preferred"
            and selected_platform != "CUDA"
            and not cuda_detected
            and "CUDA_platform_missing" not in fallback_reason.split(";")
        )
    ):
        raise MdProvenanceError("md_platform_fallback_evidence_invalid")
    sampling_policy = manifest.get("sampling_policy")
    if (
        not isinstance(sampling_policy, dict)
        or sampling_policy.get("policy_id") != SHORT_MD_POLICY_ID
        or protocol[("sampling_policy", "policy_id")].get("value", "")
        != sampling_policy.get("policy_id")
        or protocol[("sampling_policy", "purpose")].get("value", "")
        != sampling_policy.get("purpose")
    ):
        raise MdProvenanceError("md_sampling_policy_invalid")
    validate_receptor_selection_semantics(binding, manifest, protocol)
    timestep = finite_float(protocol[("integrator", "time_step")].get("value"), "md_timestep_invalid")
    production_steps = int(
        finite_float(protocol[("production", "total_steps")].get("value"), "md_production_steps_invalid")
    )
    frame_count = int(
        finite_float(protocol[("output", "frame_count")].get("value"), "md_frame_count_invalid")
    )
    state_row_count = int(
        finite_float(protocol[("output", "state_rows")].get("value"), "md_state_row_count_invalid")
    )
    if timestep <= 0.0:
        raise MdProvenanceError("md_timestep_invalid")
    if production_steps <= 0:
        raise MdProvenanceError("md_production_steps_invalid")
    if (
        abs(finite_float(sampling_policy.get("timestep_ps"), "md_sampling_policy_invalid") - timestep)
        > 1e-12
        or int(
            finite_float(
                sampling_policy.get("production_steps_total"),
                "md_sampling_policy_invalid",
            )
        )
        != production_steps
    ):
        raise MdProvenanceError("md_sampling_policy_invalid")
    if frame_count < MINIMUM_MD_PRODUCTION_SAMPLES:
        raise MdProvenanceError("md_frame_count_invalid")
    if state_row_count < 2:
        raise MdProvenanceError("md_state_row_count_invalid")
    if manifest.get("frame_count") != frame_count or manifest.get("state_row_count") != state_row_count:
        raise MdProvenanceError("md_manifest_sampling_count_mismatch")

    state_rows = bound_tsv_rows(binding, "md_state.tsv")
    if len(state_rows) != state_row_count:
        raise MdProvenanceError("md_state_row_count_invalid")
    steps = []
    for row in state_rows:
        if row.get("candidate_id") != binding.candidate_id:
            raise MdProvenanceError("md_state_candidate_id_mismatch")
        if row.get("input_id") != binding.input_id:
            raise MdProvenanceError("md_state_input_id_mismatch")
        step = int(finite_float(row.get("step"), "md_state_value_invalid"))
        if step < 0:
            raise MdProvenanceError("md_state_value_invalid")
        steps.append(step)
        for field_name in (
            "time_ps",
            "potential_energy_kj_mol",
            "kinetic_energy_kj_mol",
            "total_energy_kj_mol",
            "temperature_estimate_K",
        ):
            finite_float(row.get(field_name), "md_state_value_invalid")
    if max(steps) < production_steps:
        raise MdProvenanceError("md_state_steps_incomplete")

    trajectory_rows = bound_tsv_rows(binding, "md_trajectory.tsv")
    frames: set[int] = set()
    for row in trajectory_rows:
        if row.get("candidate_id") != binding.candidate_id:
            raise MdProvenanceError("md_trajectory_candidate_id_mismatch")
        frame = int(finite_float(row.get("frame"), "md_trajectory_frame_invalid"))
        if frame < 0:
            raise MdProvenanceError("md_trajectory_frame_invalid")
        frames.add(frame)
    if len(frames) != frame_count:
        raise MdProvenanceError("md_trajectory_frame_count_mismatch")
    return {
        "method": "openmm_receptor_ligand_complex_dynamics",
        "candidate_id": binding.candidate_id,
        "frame_count": frame_count,
        "production_steps": production_steps,
        "platform": str(manifest.get("platform", "")),
        "platform_policy": str(manifest.get("platform_policy", "")),
    }


def validate_peptide_conformer_semantics(
    binding: MdInvocationBinding,
    manifest: dict[str, Any],
) -> dict[str, Any]:
    if (
        manifest.get("artifact_schema_version") != "genoligand_peptide_conformer_v1"
        or manifest.get("method_class") != "conformer_ensemble"
        or manifest.get("has_receptor") is not False
        or manifest.get("uses_openmm") is not False
        or manifest.get("md_score") != 0.0
    ):
        raise MdProvenanceError("peptide_conformer_method_schema_invalid")
    expected_metrics = {
        md_artifact_relative(binding, "peptide_conformer_summary.tsv"),
        md_artifact_relative(binding, "peptide_conformer_metrics.tsv"),
    }
    if set(manifest.get("metric_artifacts", [])) != expected_metrics:
        raise MdProvenanceError("peptide_conformer_artifact_set_invalid")
    if manifest.get("coordinate_artifact") != md_artifact_relative(
        binding,
        "peptide_conformer_coordinates.tsv",
    ):
        raise MdProvenanceError("peptide_conformer_coordinate_artifact_invalid")
    if manifest.get("ensemble_artifact") != md_artifact_relative(
        binding,
        "peptide_conformer_ensemble.tsv",
    ):
        raise MdProvenanceError("peptide_conformer_ensemble_artifact_invalid")
    if (binding.output_dir / "md_summary.tsv").exists():
        raise MdProvenanceError("peptide_conformer_must_not_emit_md_summary")
    summary_rows = bound_tsv_rows(binding, "peptide_conformer_summary.tsv")
    if len(summary_rows) != 1:
        raise MdProvenanceError("peptide_conformer_summary_invalid")
    summary = summary_rows[0]
    if summary.get("candidate_id") != binding.candidate_id or summary.get("input_id") != binding.input_id:
        raise MdProvenanceError("peptide_conformer_binding_mismatch")
    if summary.get("has_receptor") != "false" or summary.get("uses_openmm") != "false":
        raise MdProvenanceError("peptide_conformer_method_schema_invalid")
    conformer_count = int(finite_float(summary.get("conformer_count"), "peptide_conformer_count_invalid"))
    if conformer_count <= 0 or manifest.get("conformer_count") != conformer_count:
        raise MdProvenanceError("peptide_conformer_count_invalid")
    metric_rows = bound_tsv_rows(binding, "peptide_conformer_metrics.tsv")
    expected_names = {
        "mean_heavy_atom_rmsd_angstrom",
        "mean_heavy_atom_rmsf_angstrom",
        "mean_centroid_drift_angstrom",
    }
    if {row.get("metric") for row in metric_rows} != expected_names:
        raise MdProvenanceError("peptide_conformer_metric_set_invalid")
    for row in metric_rows:
        if row.get("candidate_id") != binding.candidate_id or row.get("input_id") != binding.input_id:
            raise MdProvenanceError("peptide_conformer_binding_mismatch")
        if row.get("unit") != "angstrom" or finite_float(
            row.get("value"), "peptide_conformer_metric_invalid"
        ) < 0.0:
            raise MdProvenanceError("peptide_conformer_metric_invalid")
    ensemble_rows = bound_tsv_rows(binding, "peptide_conformer_ensemble.tsv")
    frames = {row.get("frame") for row in ensemble_rows if row.get("candidate_id") == binding.candidate_id}
    if len(frames) != conformer_count:
        raise MdProvenanceError("peptide_conformer_count_invalid")
    md_artifact_path(binding, "peptide_conformer_coordinates.tsv")
    return {
        "method": "rdkit_peptide_conformer_ensemble",
        "candidate_id": binding.candidate_id,
        "conformer_count": conformer_count,
        "md_score": 0.0,
    }


def bind_worker_result(path: Path, job: dict[str, Any]) -> None:
    run_dir = resolve_md_run_dir(job)
    output = resolve_worker_output_path(job, path)
    result = read_json_object(output)
    result["run_id"] = str(job.get("run_id", "")).strip() or run_dir.name
    result["invocation_id"] = str(job.get("invocation_id", "")).strip()
    for field_name in ("candidate_id", "input_id", "input_digest"):
        value = str(job.get(field_name, "")).strip()
        if value:
            result[f"expected_{field_name}"] = value
    input_manifest = runtime_path(str(job.get("input_manifest", "")).strip())
    if input_manifest.is_file():
        result["input_manifest_sha256"] = sha256_file(input_manifest)
    candidate_id = str(job.get("candidate_id", "")).strip()
    invocation_id = str(job.get("invocation_id", "")).strip()
    provenance_path = md_output_dir(run_dir, candidate_id, invocation_id) / MD_PROVENANCE_ARTIFACT
    if provenance_path.is_file():
        binding = resolve_md_invocation(job)
        validate_md_artifact_provenance(provenance_path, binding)
        if result.get("status") == "PASS":
            validate_completed_md_semantics(binding)
        result.update(binding_fields(binding))
        result["manifest"] = md_artifact_relative(binding, "md_run_manifest.json")
        result["artifact_provenance"] = provenance_path.relative_to(run_dir).as_posix()
    result["artifact_path"] = output.relative_to(run_dir).as_posix()
    write_json(output, result)


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


def read_docking_pose_ligand(
    pose_path: Path,
    ligand_path: Path,
    ligand_pdbqt: Path,
    pose_pdbqt: Path,
    Chem: Any,
) -> Any | None:
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


def read_receptor_structure(path: Path, app: Any) -> Any:
    return app.PDBFile(str(path)) if path.suffix.lower() in {".pdb", ".ent"} else app.PDBxFile(str(path))


def receptor_metal_components(path: Path, unit: Any) -> list[dict[str, Any]]:
    from openmm import app

    structure = read_receptor_structure(path, app)
    components: list[dict[str, Any]] = []
    for chain_ordinal, chain in enumerate(structure.topology.chains()):
        for residue in chain.residues():
            for atom in residue.atoms():
                if (
                    getattr(atom, "element", None) is None
                    or atom.element.symbol.upper() not in UNSUPPORTED_METAL_ELEMENTS
                ):
                    continue
                element = atom.element.symbol.upper()
                components.append(
                    {
                        "identity": f"{chain.id}:{residue.name}:{residue.id}:{atom.name}:{atom.index}:{element}",
                        "element": element,
                        "atom_name": atom.name,
                        "atom_index": atom.index,
                        "chain_id": str(chain.id),
                        "chain_ordinal": chain_ordinal,
                        "residue_name": residue.name,
                        "residue_id": str(residue.id),
                    }
                )
    return sorted(components, key=lambda row: (row["chain_ordinal"], row["atom_index"], row["identity"]))


def select_receptor_chain_from_pose(
    path: Path,
    mol: Any,
    unit: Any,
    expected_contact_chains: tuple[dict[str, Any], ...] = (),
) -> dict[str, Any]:
    import numpy as np
    from openmm import app

    structure = read_receptor_structure(path, app)
    positions = np.asarray(structure.positions.value_in_unit(unit.angstrom), dtype=float)
    conformer = mol.GetConformer()
    ligand_positions = np.asarray(
        [
            [
                conformer.GetAtomPosition(atom.GetIdx()).x,
                conformer.GetAtomPosition(atom.GetIdx()).y,
                conformer.GetAtomPosition(atom.GetIdx()).z,
            ]
            for atom in mol.GetAtoms()
            if atom.GetAtomicNum() > 1
        ],
        dtype=float,
    )
    if ligand_positions.size == 0 or not np.isfinite(ligand_positions).all():
        raise ValueError("docking_pose_heavy_atom_coordinates_invalid")
    protein_chains: list[tuple[int, Any, float]] = []
    for chain_ordinal, chain in enumerate(structure.topology.chains()):
        heavy_indices = [
            atom.index
            for residue in chain.residues()
            if residue.name.upper() in STANDARD_PROTEIN_RESIDUES
            for atom in residue.atoms()
            if getattr(atom, "element", None) is not None and atom.element.symbol.upper() != "H"
        ]
        if not heavy_indices:
            continue
        chain_positions = positions[heavy_indices]
        minimum = float(
            np.sqrt(((chain_positions[:, None, :] - ligand_positions[None, :, :]) ** 2).sum(axis=2)).min()
        )
        protein_chains.append((chain_ordinal, chain, minimum))
    expected_identities = {
        (
            str(row.get("chain_id", "")),
            int(row.get("chain_ordinal", -1)),
            str(row.get("chain_signature_sha256", "")),
        )
        for row in expected_contact_chains
    }
    actual_identities = {
        (str(chain.id), chain_ordinal, chain_signature(chain))
        for chain_ordinal, chain, _minimum in protein_chains
    }
    if expected_identities and not expected_identities <= actual_identities:
        raise ValueError("docking_receptor_contact_chain_mismatch")
    eligible = [
        row
        for row in protein_chains
        if row[2] <= RECEPTOR_CHAIN_CONTACT_CUTOFF_ANGSTROM
        and (
            not expected_identities
            or (str(row[1].id), row[0], chain_signature(row[1])) in expected_identities
        )
    ]
    if not eligible:
        raise ValueError("no_receptor_chain_within_pose_contact_cutoff")
    eligible.sort(key=lambda row: (row[2], row[0]))
    if len(eligible) > 1 and math.isclose(
        eligible[0][2],
        eligible[1][2],
        rel_tol=0.0,
        abs_tol=1e-6,
    ):
        nearest_distance = eligible[0][2]
        identities = ",".join(
            str(row[1].id)
            for row in eligible
            if math.isclose(row[2], nearest_distance, rel_tol=0.0, abs_tol=1e-6)
        )
        raise ValueError(f"ambiguous_receptor_chain_contacts:{identities}")
    selected_ordinal, selected_chain, minimum = eligible[0]
    coordinate_rows = []
    for residue in selected_chain.residues():
        if residue.name.upper() not in STANDARD_PROTEIN_RESIDUES:
            continue
        for atom in residue.atoms():
            if getattr(atom, "element", None) is None or atom.element.symbol.upper() == "H":
                continue
            coord = positions[atom.index]
            coordinate_rows.append(
                [
                    residue.name.upper(),
                    str(residue.id),
                    atom.name,
                    atom.element.symbol.upper(),
                    *(f"{float(value):.4f}" for value in coord),
                ]
            )
    coordinate_digest = hashlib.sha256(
        json.dumps(coordinate_rows, separators=(",", ":")).encode("utf-8")
    ).hexdigest()
    return {
        "receptor_chain_selection_algorithm": RECEPTOR_CHAIN_SELECTION_VERSION,
        "receptor_chain_atom_policy": "protein_and_ligand_heavy_atoms",
        "receptor_chain_contact_cutoff_angstrom": RECEPTOR_CHAIN_CONTACT_CUTOFF_ANGSTROM,
        "original_protein_chain_count": len(protein_chains),
        "retained_protein_chain_count": 1,
        "selected_chain_id": str(selected_chain.id),
        "selected_chain_ordinal": selected_ordinal,
        "selected_chain_signature_sha256": chain_signature(selected_chain),
        "selected_chain_coordinate_sha256": coordinate_digest,
        "selected_chain_minimum_distance_angstrom": minimum,
    }


def load_receptor_coordinate(path: Path, unit: Any, *, selected_chain_ordinal: int | None = None) -> ReceptorCoordinate:
    try:
        return load_receptor_with_pdbfixer(path, unit, selected_chain_ordinal=selected_chain_ordinal)
    except Exception as pdbfixer_error:  # noqa: BLE001 - fallback keeps real coordinate provenance
        receptor = load_receptor_with_openmm_app(path, unit, selected_chain_ordinal=selected_chain_ordinal)
        receptor.risk_flags.append(f"pdbfixer_failed:{safe_note(str(pdbfixer_error))}")
        return receptor


def load_receptor_with_pdbfixer(
    path: Path,
    unit: Any,
    *,
    selected_chain_ordinal: int | None = None,
) -> ReceptorCoordinate:
    import numpy as np
    from pdbfixer import PDBFixer

    fixer = PDBFixer(filename=str(path))
    if selected_chain_ordinal is not None:
        chain_count = len(list(fixer.topology.chains()))
        if selected_chain_ordinal < 0 or selected_chain_ordinal >= chain_count:
            raise ValueError("selected_receptor_chain_ordinal_invalid")
        fixer.removeChains([index for index in range(chain_count) if index != selected_chain_ordinal])
    fixer.findMissingResidues()
    fixer.missingResidues = {}
    fixer.findNonstandardResidues()
    fixer.replaceNonstandardResidues()
    fixer.removeHeterogens(keepWater=False)
    fixer.findMissingAtoms()
    fixer.addMissingAtoms()
    fixer.addMissingHydrogens(7.0)
    atoms, positions = atoms_from_topology(fixer.topology, fixer.positions, unit)
    return ReceptorCoordinate(
        path=path,
        topology=fixer.topology,
        atoms=atoms,
        positions=fixer.positions,
        positions_angstrom=np.asarray(positions, dtype=float),
        preparation="pdbfixer_observed_residues_prepared_receptor",
        risk_flags=[],
    )


def load_receptor_with_openmm_app(
    path: Path,
    unit: Any,
    *,
    selected_chain_ordinal: int | None = None,
) -> ReceptorCoordinate:
    import numpy as np
    from openmm import app

    structure = read_receptor_structure(path, app)
    modeller = app.Modeller(structure.topology, structure.positions)
    if selected_chain_ordinal is not None:
        chains = list(modeller.topology.chains())
        if selected_chain_ordinal < 0 or selected_chain_ordinal >= len(chains):
            raise ValueError("selected_receptor_chain_ordinal_invalid")
        modeller.delete([chain for index, chain in enumerate(chains) if index != selected_chain_ordinal])
    nonprotein = [
        residue
        for residue in modeller.topology.residues()
        if residue.name.upper() not in STANDARD_PROTEIN_RESIDUES
    ]
    if nonprotein:
        modeller.delete(nonprotein)
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


def write_invocation_prepared_receptor(receptor: ReceptorCoordinate, path: Path) -> ReceptorCoordinate:
    from openmm import app

    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as handle:
        app.PDBFile.writeFile(receptor.topology, receptor.positions, handle)
    receptor.path = path
    receptor.preparation = f"{receptor.preparation};pose_contact_chain_selected"
    receptor.model_metadata["prepared_receptor_sha256"] = sha256_file(path)
    return receptor


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


def md_platform_policy(job: dict[str, Any]) -> str:
    policy = str(job.get("platform_policy", "")).strip().lower() or "preferred"
    if policy not in {"required", "preferred", "disabled"}:
        raise ValueError("openmm_platform_policy_invalid")
    return policy


def build_md_sampling_policy(
    timestep_ps: float,
    equilibration_steps: int,
    production_segments: int,
    production_steps_per_segment: int,
) -> dict[str, Any]:
    production_steps = production_segments * production_steps_per_segment
    return {
        "policy_id": SHORT_MD_POLICY_ID,
        "purpose": "candidate_local_stability_screen",
        "timestep_ps": timestep_ps,
        "equilibration_steps": equilibration_steps,
        "equilibration_time_ps": equilibration_steps * timestep_ps,
        "production_segments": production_segments,
        "production_steps_per_segment": production_steps_per_segment,
        "production_steps_total": production_steps,
        "production_time_ps": production_steps * timestep_ps,
        "expected_frame_count": production_segments + 1,
    }


def available_openmm_platforms(openmm: Any) -> list[str]:
    return [
        openmm.Platform.getPlatform(index).getName()
        for index in range(openmm.Platform.getNumPlatforms())
    ]


def select_openmm_platform(openmm: Any, policy: str = "preferred") -> str:
    names = available_openmm_platforms(openmm)
    preferences = {
        "required": ["CUDA"],
        "preferred": ["CUDA", "CPU", "Reference"],
        "disabled": ["CPU", "Reference"],
    }
    if policy not in preferences:
        raise ValueError("openmm_platform_policy_invalid")
    for preferred in preferences[policy]:
        if preferred in names:
            return preferred
    if policy == "required":
        raise ValueError("gpu_required_context_unavailable:CUDA_platform_missing")
    raise ValueError(f"openmm_{policy}_platform_unavailable")


def create_openmm_context(
    openmm: Any,
    system: Any,
    integrator: Any,
    policy: str,
) -> tuple[Any, str, dict[str, str], list[str]]:
    names = set(available_openmm_platforms(openmm))
    preferences = {
        "required": ["CUDA"],
        "preferred": ["CUDA", "CPU", "Reference"],
        "disabled": ["CPU", "Reference"],
    }
    if policy not in preferences:
        raise ValueError("openmm_platform_policy_invalid")
    failures: list[str] = []
    for name in preferences[policy]:
        if name not in names:
            continue
        platform = openmm.Platform.getPlatformByName(name)
        properties = openmm_platform_properties(name, platform)
        try:
            if properties:
                context = openmm.Context(system, integrator, platform, properties)
            else:
                context = openmm.Context(system, integrator, platform)
        except Exception as exc:  # noqa: BLE001 - each policy defines deterministic fallback
            detail = safe_note(str(exc))
            failures.append(
                f"{name}:{type(exc).__name__}:{detail}"
                if detail
                else f"{name}:{type(exc).__name__}"
            )
            continue
        return context, name, properties, failures
    if policy == "required":
        detail = ";".join(failures) or "CUDA_platform_missing"
        raise ValueError(f"gpu_required_context_unavailable:{detail}")
    detail = ";".join(failures) or "no_allowed_platform"
    raise ValueError(f"openmm_{policy}_context_unavailable:{detail}")


def openmm_platform_execution_evidence(
    policy: str,
    selected_platform: str,
    available_platforms: list[str],
    platform_properties: dict[str, str],
    context_failures: list[str],
) -> dict[str, Any]:
    if selected_platform == "CUDA":
        fallback_reason = ""
    elif policy == "disabled":
        fallback_reason = ";".join(["cuda_disabled_by_policy", *context_failures])
    elif "CUDA" not in available_platforms:
        fallback_reason = ";".join(["CUDA_platform_missing", *context_failures])
    else:
        fallback_reason = ";".join(context_failures) or "CUDA_context_unavailable"
    return {
        "platform_policy": policy,
        "available_openmm_platforms": available_platforms,
        "cuda_platform_detected": "CUDA" in available_platforms,
        "selected_platform": selected_platform,
        "platform_properties": platform_properties,
        "context_creation_failures": context_failures,
        "fallback_reason": fallback_reason,
    }


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
            boxShape=DEFAULT_SOLVENT_BOX_SHAPE,
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
        **receptor.model_metadata,
        "protein_forcefield": protein_forcefield,
        "water_forcefield": water_forcefield,
        "small_molecule_forcefield": forcefield_name,
        "solvent_mode": solvent_mode,
        "solvent_box_shape": DEFAULT_SOLVENT_BOX_SHAPE if solvent_mode == "explicit" else "",
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
    import numpy as np

    state = context.getState(getPositions=True, getEnergy=True, getForces=True, enforcePeriodicBox=periodic)
    positions = state.getPositions(asNumpy=True).value_in_unit(unit.angstrom)
    periodic_box = (
        state.getPeriodicBoxVectors(asNumpy=True).value_in_unit(unit.angstrom)
        if periodic
        else None
    )
    potential = float(state.getPotentialEnergy().value_in_unit(unit.kilojoule_per_mole))
    kinetic = float(state.getKineticEnergy().value_in_unit(unit.kilojoule_per_mole))
    forces = state.getForces(asNumpy=True).value_in_unit(unit.kilojoule_per_mole / unit.nanometer)
    if (
        not np.isfinite(positions).all()
        or not np.isfinite(forces).all()
        or not math.isfinite(potential)
        or not math.isfinite(kinetic)
    ):
        raise ValueError(f"openmm_non_finite_state:{stage}")
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


def assert_openmm_state_finite(context: Any, unit: Any, stage: str) -> None:
    import numpy as np

    state = context.getState(getPositions=True, getEnergy=True, getForces=True)
    positions = state.getPositions(asNumpy=True).value_in_unit(unit.angstrom)
    potential = float(state.getPotentialEnergy().value_in_unit(unit.kilojoule_per_mole))
    forces = state.getForces(asNumpy=True).value_in_unit(unit.kilojoule_per_mole / unit.nanometer)
    if not np.isfinite(positions).all() or not np.isfinite(forces).all() or not math.isfinite(potential):
        raise ValueError(f"openmm_non_finite_state:{stage}")


def integrate_openmm(integrator: Any, steps: int, stage: str) -> None:
    try:
        integrator.step(steps)
    except Exception as exc:  # noqa: BLE001 - preserve the exact integration stage in worker evidence
        raise ValueError(f"openmm_integration_failed:{stage}:{safe_note(str(exc))}") from exc


def minimize_openmm(openmm: Any, context: Any, unit: Any, max_iterations: int) -> None:
    try:
        openmm.LocalEnergyMinimizer.minimize(
            context,
            tolerance=10 * unit.kilojoule_per_mole / unit.nanometer,
            maxIterations=max_iterations,
        )
    except Exception as exc:  # noqa: BLE001 - preserve the exact minimization stage in worker evidence
        raise ValueError(f"openmm_minimization_failed:post_minimization:{safe_note(str(exc))}") from exc


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
    sampling_policy = model_metadata.get("sampling_policy", {})
    device_evidence = model_metadata.get("execution_device", {})
    available_platforms = ",".join(device_evidence.get("available_openmm_platforms", []))
    production_steps = production_segments * production_steps_per_segment
    equilibration_time_ps = equilibration_steps * timestep_ps
    production_time_ps = production_steps * timestep_ps
    return [
        [candidate_id, input_id, "input", "receptor_coordinate", str(receptor.path), "", "structure_viewer"],
        [candidate_id, input_id, "input", "ligand_placement", placement_method, "", "docking_pose_sdf"],
        [candidate_id, input_id, "input", "ligand_atom_count", str(mol.GetNumAtoms()), "atoms", "rdkit"],
        [candidate_id, input_id, "input", "receptor_atom_count", str(len(receptor.atoms)), "atoms", "pdbfixer_or_coordinate_parser"],
        [candidate_id, input_id, "input", "receptor_chain_selection_algorithm", str(model_metadata.get("receptor_chain_selection_algorithm", "")), "", "GenoLigand"],
        [candidate_id, input_id, "input", "receptor_chain_atom_policy", str(model_metadata.get("receptor_chain_atom_policy", "")), "", "GenoLigand"],
        [candidate_id, input_id, "input", "receptor_chain_contact_cutoff", str(model_metadata.get("receptor_chain_contact_cutoff_angstrom", "")), "angstrom", "GenoLigand"],
        [candidate_id, input_id, "input", "original_protein_chain_count", str(model_metadata.get("original_protein_chain_count", "")), "chains", "structure_coordinate"],
        [candidate_id, input_id, "input", "retained_protein_chain_count", str(model_metadata.get("retained_protein_chain_count", "")), "chains", "receptor_prepared.pdb"],
        [candidate_id, input_id, "input", "selected_chain_id", str(model_metadata.get("selected_chain_id", "")), "", "structure_coordinate"],
        [candidate_id, input_id, "input", "selected_chain_ordinal", str(model_metadata.get("selected_chain_ordinal", "")), "", "structure_coordinate"],
        [candidate_id, input_id, "input", "selected_chain_signature_sha256", str(model_metadata.get("selected_chain_signature_sha256", "")), "", "structure_coordinate"],
        [candidate_id, input_id, "input", "selected_chain_coordinate_sha256", str(model_metadata.get("selected_chain_coordinate_sha256", "")), "", "structure_coordinate"],
        [candidate_id, input_id, "input", "selected_chain_minimum_distance", str(model_metadata.get("selected_chain_minimum_distance_angstrom", "")), "angstrom", "docking_pose_sdf"],
        [candidate_id, input_id, "input", "receptor_chain_selection_digest", str(model_metadata.get("receptor_chain_selection_digest", "")), "", "GenoLigand"],
        [candidate_id, input_id, "parameterization", "ligand_parameterization", parameterization, "", "OpenFF/RDKit"],
        [candidate_id, input_id, "parameterization", "protein_forcefield", str(model_metadata.get("protein_forcefield", "")), "", "OpenMM"],
        [candidate_id, input_id, "parameterization", "small_molecule_forcefield", str(model_metadata.get("small_molecule_forcefield", "")), "", "OpenFF"],
        [candidate_id, input_id, "system", "solvent_mode", str(model_metadata.get("solvent_mode", "")), "", "OpenMM Modeller"],
        [candidate_id, input_id, "system", "solvent_atom_count", str(model_metadata.get("solvent_atom_count", 0)), "atoms", "OpenMM Modeller"],
        [candidate_id, input_id, "system", "total_atom_count", str(model_metadata.get("total_atom_count", 0)), "atoms", "OpenMM"],
        [candidate_id, input_id, "system", "nonbonded_method", str(model_metadata.get("nonbonded_method", "")), "", "OpenMM"],
        [candidate_id, input_id, "platform", "openmm_platform", platform_name, "", "OpenMM"],
        [candidate_id, input_id, "platform", "openmm_platform_properties", properties, "", "OpenMM"],
        [candidate_id, input_id, "platform", "platform_policy", str(device_evidence.get("platform_policy", "")), "", "GenoLigand job"],
        [candidate_id, input_id, "platform", "available_openmm_platforms", available_platforms, "", "OpenMM"],
        [candidate_id, input_id, "platform", "cuda_platform_detected", bool_text(bool(device_evidence.get("cuda_platform_detected", False))), "", "OpenMM"],
        [candidate_id, input_id, "platform", "context_creation_failures", ";".join(device_evidence.get("context_creation_failures", [])), "", "OpenMM Context"],
        [candidate_id, input_id, "platform", "fallback_reason", str(device_evidence.get("fallback_reason", "")), "", "OpenMM Context"],
        [candidate_id, input_id, "sampling_policy", "policy_id", str(sampling_policy.get("policy_id", "")), "", "GenoLigand"],
        [candidate_id, input_id, "sampling_policy", "purpose", str(sampling_policy.get("purpose", "")), "", "GenoLigand"],
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
    return {
        "mean_heavy_atom_rmsd_angstrom": rmsd,
        "mean_heavy_atom_rmsf_angstrom": rmsf,
        "mean_centroid_drift_angstrom": drift,
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


def write_md_not_ready(
    run_dir: Path,
    reason: str,
    risk_flags: list[str],
    *,
    details: dict[str, Any] | None = None,
    binding: MdInvocationBinding,
) -> None:
    if run_dir.resolve(strict=True) != binding.run_dir:
        raise MdProvenanceError("md_output_run_dir_mismatch")
    tool_outputs = binding.output_dir
    tool_outputs.mkdir(parents=True, exist_ok=True)
    details = details or {}
    not_ready_path = tool_outputs / "md_not_ready.tsv"
    run_manifest_path = tool_outputs / "md_run_manifest.json"
    write_tsv(
        not_ready_path,
        ["reason", "risk_flags"],
        [[reason, ";".join(risk_flags)]],
    )
    write_json(
        run_manifest_path,
        {
            **binding_fields(binding),
            "method": "not_ready",
            "reason": reason,
            "risk_flags": risk_flags,
            "details": details,
            "metric_artifacts": [],
            "ligand_coordinate": "",
            "ligand_placement_manifest": "",
            "complex_manifest": "",
            "trajectory": "",
            "artifact_provenance": md_artifact_relative(
                binding,
                MD_PROVENANCE_ARTIFACT,
                must_exist=False,
            ),
        },
    )
    provenance_path = write_md_artifact_provenance(
        binding,
        [not_ready_path, run_manifest_path],
        tool_name="md_summary",
        tool_version="not_ready",
        parameters={"reason": reason, "risk_flags": risk_flags, "details": details},
    )
    upsert_md_manifest_index(
        binding,
        status="not_ready",
        reason=reason,
        manifest=run_manifest_path.relative_to(binding.run_dir).as_posix(),
        artifact_provenance=provenance_path.relative_to(binding.run_dir).as_posix(),
    )


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
    binding: MdInvocationBinding,
    tool_version: str,
    platform_policy: str,
    platform_failures: list[str],
) -> None:
    ensure_md_output_binding(run_dir, candidate_id, input_id, binding)
    tool_outputs = binding.output_dir
    tool_outputs.mkdir(parents=True, exist_ok=True)
    device_evidence = model_metadata["execution_device"]
    sampling_policy = model_metadata["sampling_policy"]
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
    summary_path = tool_outputs / "md_summary.tsv"
    summary_row = [
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
    write_tsv(summary_path, summary_header, [summary_row])
    metric_specs = [
        ("md_rmsd.tsv", "rmsd", "angstrom"),
        ("md_rmsf.tsv", "rmsf", "angstrom"),
        ("md_ligand_drift.tsv", "ligand_drift", "angstrom"),
        ("md_contacts.tsv", "contact_occupancy", "fraction"),
        ("md_hbonds.tsv", "hydrogen_bond_occupancy", "fraction"),
    ]
    metric_paths = []
    for file_name, metric, unit in metric_specs:
        metric_path = tool_outputs / file_name
        metric_paths.append(metric_path)
        write_tsv(
            metric_path,
            ["candidate_id", "input_id", "metric", "value", "unit", "source"],
            [
                [
                    candidate_id,
                    input_id,
                    metric,
                    fmt(metrics[metric]),
                    unit,
                    "openmm_receptor_ligand_complex_dynamics",
                ]
            ],
        )
    metric_artifacts = [
        md_artifact_relative(binding, name)
        for name in (
            "md_summary.tsv",
            "md_rmsd.tsv",
            "md_rmsf.tsv",
            "md_ligand_drift.tsv",
            "md_contacts.tsv",
            "md_hbonds.tsv",
        )
    ]
    ligand_placement_path = tool_outputs / "ligand_placement.tsv"
    trajectory_path = tool_outputs / "md_trajectory.tsv"
    state_path = tool_outputs / "md_state.tsv"
    protocol_path = tool_outputs / "md_protocol.tsv"
    gpu_telemetry_path = tool_outputs / "gpu_telemetry.tsv"
    gpu_status_path = tool_outputs / "gpu_telemetry_status.tsv"
    complex_manifest_path = tool_outputs / "complex_md_manifest.json"
    run_manifest_path = tool_outputs / "md_run_manifest.json"
    write_ligand_placement(
        ligand_placement_path,
        candidate_id,
        input_id,
        mol,
        ligand_positions,
        placement_method,
    )
    write_trajectory(trajectory_path, candidate_id, frames, mol, receptor, role_ranges)
    write_md_state(state_path, state_rows)
    bound_protocol_rows = [list(row) for row in protocol_rows]
    receptor_artifact = relative_run_artifact(binding, receptor.path)
    for row in bound_protocol_rows:
        if len(row) > 4 and row[3] == "receptor_coordinate":
            row[4] = receptor_artifact
    write_tsv(
        protocol_path,
        ["candidate_id", "input_id", "stage", "parameter", "value", "unit", "source"],
        bound_protocol_rows,
    )
    write_gpu_telemetry(gpu_telemetry_path, gpu_telemetry)
    write_gpu_telemetry_status(gpu_status_path, gpu_status)
    write_complex_md_manifest(
        complex_manifest_path,
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
        binding,
        platform_policy,
        platform_failures,
    )
    write_json(
        run_manifest_path,
        {
            **binding_fields(binding),
            "input_smiles": smiles,
            "method": "openmm_receptor_ligand_complex_dynamics",
            "platform": platform_name,
            "platform_policy": platform_policy,
            "platform_context_failures": platform_failures,
            "platform_properties": platform_properties,
            "available_openmm_platforms": device_evidence["available_openmm_platforms"],
            "cuda_platform_detected": device_evidence["cuda_platform_detected"],
            "platform_fallback_reason": device_evidence["fallback_reason"],
            "sampling_policy": sampling_policy,
            "cuda_ready": platform_name == "CUDA" and bool(gpu_status["gpu_observed"]),
            "gpu_observed": bool(gpu_status["gpu_observed"]),
            "gpu_telemetry_artifact": md_artifact_relative(binding, "gpu_telemetry.tsv"),
            "gpu_telemetry_status_artifact": md_artifact_relative(
                binding,
                "gpu_telemetry_status.tsv",
            ),
            "gpu_telemetry": gpu_status,
            "system": model_metadata,
            "receptor_coordinate": receptor_artifact,
            "receptor_preparation": receptor.preparation,
            "receptor_atom_count": len(receptor.atoms),
            "ligand_parameterization": parameterization,
            "ligand_coordinate": md_artifact_relative(binding, "ligand_placement.tsv"),
            "ligand_coordinate_format": "tsv_angstrom",
            "ligand_atom_count": mol.GetNumAtoms(),
            "ligand_placement": placement_method,
            "ligand_placement_manifest": md_artifact_relative(binding, "ligand_placement.tsv"),
            "complex_manifest": md_artifact_relative(binding, "complex_md_manifest.json"),
            "protocol_artifact": md_artifact_relative(binding, "md_protocol.tsv"),
            "frame_count": len(frames),
            "state_artifact": md_artifact_relative(binding, "md_state.tsv"),
            "state_row_count": len(state_rows),
            "trajectory": md_artifact_relative(binding, "md_trajectory.tsv"),
            "metric_artifacts": metric_artifacts,
            "metric_units": COMPLEX_MD_METRIC_UNITS,
            "risk_flags": risk_flags,
            "score_decomposition": relative_run_artifact(
                binding, "score_decomposition.tsv", must_exist=False
            ),
            "artifact_provenance": md_artifact_relative(
                binding,
                MD_PROVENANCE_ARTIFACT,
                must_exist=False,
            ),
        },
    )
    provenance_path = write_md_artifact_provenance(
        binding,
        [
            receptor.path,
            summary_path,
            *metric_paths,
            ligand_placement_path,
            trajectory_path,
            state_path,
            protocol_path,
            gpu_telemetry_path,
            gpu_status_path,
            complex_manifest_path,
            run_manifest_path,
        ],
        tool_name="OpenMM",
        tool_version=tool_version,
        parameters={
            "method": "openmm_receptor_ligand_complex_dynamics",
            "platform": platform_name,
            "platform_policy": platform_policy,
            "platform_context_failures": platform_failures,
            "platform_properties": platform_properties,
            "available_openmm_platforms": device_evidence["available_openmm_platforms"],
            "cuda_platform_detected": device_evidence["cuda_platform_detected"],
            "platform_fallback_reason": device_evidence["fallback_reason"],
            "sampling_policy": sampling_policy,
            "system": model_metadata,
            "placement_method": placement_method,
            "protocol": bound_protocol_rows,
        },
    )
    upsert_md_summary(binding.run_dir, summary_row)
    upsert_md_manifest_index(
        binding,
        status="completed",
        reason="complex_md_completed",
        manifest=run_manifest_path.relative_to(binding.run_dir).as_posix(),
        artifact_provenance=provenance_path.relative_to(binding.run_dir).as_posix(),
    )


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
    binding: MdInvocationBinding,
) -> None:
    ensure_md_output_binding(run_dir, candidate_id, input_id, binding)
    tool_outputs = binding.output_dir
    tool_outputs.mkdir(parents=True, exist_ok=True)
    risk_text = ";".join(risk_flags)
    summary_path = tool_outputs / "peptide_conformer_summary.tsv"
    write_tsv(
        summary_path,
        [
            "candidate_id",
            "input_id",
            "method",
            "conformer_count",
            "mean_heavy_atom_rmsd_angstrom",
            "mean_heavy_atom_rmsf_angstrom",
            "mean_centroid_drift_angstrom",
            "has_receptor",
            "uses_openmm",
            "notes",
        ],
        [
            [
                candidate_id,
                input_id,
                "rdkit_peptide_conformer_ensemble",
                str(len(frames)),
                fmt(metrics["mean_heavy_atom_rmsd_angstrom"]),
                fmt(metrics["mean_heavy_atom_rmsf_angstrom"]),
                fmt(metrics["mean_centroid_drift_angstrom"]),
                "false",
                "false",
                risk_text,
            ]
        ],
    )
    metrics_path = tool_outputs / "peptide_conformer_metrics.tsv"
    write_tsv(
        metrics_path,
        ["candidate_id", "input_id", "metric", "value", "unit", "source"],
        [
            [candidate_id, input_id, metric, fmt(value), "angstrom", "rdkit_peptide_conformer_ensemble"]
            for metric, value in sorted(metrics.items())
        ],
    )
    metric_artifacts = [
        md_artifact_relative(binding, "peptide_conformer_summary.tsv"),
        md_artifact_relative(binding, "peptide_conformer_metrics.tsv"),
    ]
    coordinate_path = tool_outputs / "peptide_conformer_coordinates.tsv"
    ensemble_path = tool_outputs / "peptide_conformer_ensemble.tsv"
    run_manifest_path = tool_outputs / "md_run_manifest.json"
    write_ligand_placement(
        coordinate_path,
        candidate_id,
        input_id,
        mol,
        frames[0],
        "rdkit_peptide_lowest_energy_conformer",
    )
    write_peptide_trajectory(ensemble_path, candidate_id, frames, mol)
    write_json(
        run_manifest_path,
        {
            **binding_fields(binding),
            "artifact_schema_version": "genoligand_peptide_conformer_v1",
            "input_kind": "peptide",
            "input_sequence": sequence,
            "method": "rdkit_peptide_conformer_ensemble",
            "method_class": "conformer_ensemble",
            "platform": "RDKit",
            "has_receptor": False,
            "uses_openmm": False,
            "md_score": 0.0,
            "rdkit_version": rdkit_version,
            "sequence_length": len(sequence),
            "conformer_parameterization": parameterization,
            "coordinate_artifact": md_artifact_relative(
                binding,
                "peptide_conformer_coordinates.tsv",
            ),
            "coordinate_format": "tsv_angstrom",
            "atom_count": mol.GetNumAtoms(),
            "heavy_atom_count": sum(1 for atom in mol.GetAtoms() if atom.GetAtomicNum() > 1),
            "conformer_count": len(frames),
            "ensemble_artifact": md_artifact_relative(binding, "peptide_conformer_ensemble.tsv"),
            "metric_artifacts": metric_artifacts,
            "risk_flags": risk_flags,
            "artifact_provenance": md_artifact_relative(
                binding,
                MD_PROVENANCE_ARTIFACT,
                must_exist=False,
            ),
        },
    )
    provenance_path = write_md_artifact_provenance(
        binding,
        [
            summary_path,
            metrics_path,
            coordinate_path,
            ensemble_path,
            run_manifest_path,
        ],
        tool_name="RDKit",
        tool_version=rdkit_version,
        parameters={
            "method": "rdkit_peptide_conformer_ensemble",
            "parameterization": parameterization,
            "frame_count": len(frames),
        },
    )
    upsert_md_manifest_index(
        binding,
        status="completed",
        reason="peptide_conformer_completed",
        manifest=run_manifest_path.relative_to(binding.run_dir).as_posix(),
        artifact_provenance=provenance_path.relative_to(binding.run_dir).as_posix(),
    )


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
    binding: MdInvocationBinding,
    platform_policy: str,
    platform_failures: list[str],
) -> None:
    ensure_md_output_binding(binding.run_dir, candidate_id, input_id, binding)
    path = resolve_run_artifact(binding.run_dir, path, must_exist=False)
    device_evidence = model_metadata["execution_device"]
    write_json(
        path,
        {
            **binding_fields(binding),
            "method": "openmm_receptor_ligand_complex_dynamics",
            "platform": platform_name,
            "platform_policy": platform_policy,
            "platform_context_failures": platform_failures,
            "platform_properties": platform_properties,
            "available_openmm_platforms": device_evidence["available_openmm_platforms"],
            "cuda_platform_detected": device_evidence["cuda_platform_detected"],
            "platform_fallback_reason": device_evidence["fallback_reason"],
            "sampling_policy": model_metadata["sampling_policy"],
            "system": model_metadata,
            "cuda_ready": platform_name == "CUDA" and bool(gpu_status["gpu_observed"]),
            "gpu_observed": bool(gpu_status["gpu_observed"]),
            "gpu_telemetry_artifact": md_artifact_relative(binding, "gpu_telemetry.tsv"),
            "gpu_telemetry_status_artifact": md_artifact_relative(
                binding,
                "gpu_telemetry_status.tsv",
            ),
            "gpu_telemetry": gpu_status,
            "receptor_coordinate": relative_run_artifact(binding, receptor.path),
            "receptor_preparation": receptor.preparation,
            "ligand_coordinate": md_artifact_relative(binding, "ligand_placement.tsv"),
            "ligand_parameterization": parameterization,
            "ligand_placement": placement_method,
            "protocol_artifact": md_artifact_relative(binding, "md_protocol.tsv"),
            "state_artifact": md_artifact_relative(binding, "md_state.tsv"),
            "trajectory": md_artifact_relative(binding, "md_trajectory.tsv"),
            "metric_artifacts": metric_artifacts,
            "metric_units": COMPLEX_MD_METRIC_UNITS,
            "risk_flags": risk_flags,
            "artifact_provenance": md_artifact_relative(
                binding,
                MD_PROVENANCE_ARTIFACT,
                must_exist=False,
            ),
        },
    )


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
