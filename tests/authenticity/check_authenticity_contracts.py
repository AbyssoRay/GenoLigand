from __future__ import annotations

import csv
import json
import re
import sys
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "python"))


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def read_rust_source_text(path: str) -> str:
    text = read(path)
    return re.sub(r"\\u\{([0-9a-fA-F]+)\}", lambda match: chr(int(match.group(1), 16)), text)


def require(condition: bool, message: str) -> None:
    if not condition:
        raise AssertionError(message)


def main() -> int:
    architecture = read("docs/architecture.md")
    science = read_rust_source_text("crates/gls-science/src/lib.rs")
    server = read("crates/gls-server/src/lib.rs")
    md_worker = read("python/genoligand_workers/md_summary_worker.py")
    alignment_worker = read("python/genoligand_workers/alignment_worker.py")
    worker_common = read("python/genoligand_workers/common.py")
    worker_result_schema = json.loads(read("schemas/worker_result.schema.json"))
    worker_cpu_requirements = read("python/requirements-workers-cpu.txt")
    worker_cuda_requirements = read("python/requirements-workers-cuda.txt")
    worker_environment_reference = json.loads(read("python/environment-workers.yml"))
    worker_lock_rows = list(
        csv.DictReader(
            read("python/requirements-workers.lock.txt").splitlines(),
            delimiter="\t",
        )
    )
    worker_runtime_requirements = {
        row["platform"]: read(f"python/{row['file']}")
        for row in worker_lock_rows
        if row["profile"] == "cpu" and row["artifact_id"].startswith("runtime_")
    }
    worker_cuda_platforms = {
        row["platform"] for row in worker_lock_rows if row["profile"] == "cuda"
    }
    worker_source_requirements = read("python/requirements-workers-sources.lock.tsv")

    require(
        not (ROOT / "configs" / "target_ranking.yml").exists(),
        "ranking policy must have one runtime-owned source of truth",
    )
    require("无输入、无数据库、无工具或无运行产物时" in architecture, "authenticity rule missing from architecture docs")
    require("candidate_count" in science, "science pipeline must write candidate count metadata")
    require("user_candidate_rows" in server, "server must derive candidate rows from run artifacts")
    require("candidate_detail_json" in server, "server must expose candidate details from run artifacts")
    require("structure_viewer_payload" in server, "server must expose viewer data only from run structure artifacts")
    require("md_chart_rows" in server, "server must derive MD charts from run MD artifacts")
    require("structure_viewer.pdb" in server, "viewer must be backed by explicit structure coordinate artifacts")
    require("input_conversion_manifest.tsv" in science, "science pipeline must write input conversion manifest")
    require(
        "selected_md_input" in md_worker and '"input_conversion_manifest.tsv"' in md_worker,
        "MD worker must bind the selected candidate to one canonical converted input",
    )
    require("read_peptide_sequence_input" in md_worker, "MD worker must accept peptide sequence inputs")
    require("MolFromFASTA" in md_worker, "MD worker must build peptide molecules through RDKit peptide conversion")
    require("rdkit_peptide_conformer_ensemble" in md_worker, "peptide MD artifacts must identify the real conformer calculation method")
    require("no_receptor_contact_model" in md_worker, "peptide MD artifacts must mark missing receptor contact modeling")
    require("requirements-workers.lock.txt" in worker_cpu_requirements, "CPU worker entry point must select a hashed platform lock")
    require("requirements-workers.lock.txt" in worker_cuda_requirements, "CUDA worker entry point must select hashed runtime and CUDA platform locks")
    require(
        worker_environment_reference.get("schema_version") == "genoligand_worker_environment_reference_v1"
        and worker_environment_reference.get("kind") == "generated_worker_environment_reference"
        and worker_environment_reference.get("installable") is False,
        "worker environment reference must be generated metadata rather than a Conda installation entry point",
    )
    require(
        worker_environment_reference.get("installation_entrypoint")
        == "scripts/setup/install_complete_scientific.py",
        "worker environment reference must direct installation through the complete installer",
    )
    worker_confirmed_versions = worker_environment_reference.get("confirmed_versions", {})
    require(
        set(worker_runtime_requirements) == worker_cuda_platforms
        == {"windows_x86_64", "manylinux_2_34_x86_64"},
        "CPU and CUDA worker profiles must cover the same supported platforms",
    )
    require("openmmforcefields" in worker_source_requirements, "worker source lock must pin OpenMM small-molecule force-field support")
    require(
        worker_confirmed_versions.get("openmmforcefields") == "0.15.0",
        "worker environment reference must report the locked OpenMM force-field version",
    )
    for dependency in ["xmltodict", "python-constraint", "lxml"]:
        require(
            all(dependency in requirements for requirements in worker_runtime_requirements.values()),
            f"every hashed worker runtime lock must install OpenFF runtime dependency: {dependency}",
        )
        require(
            dependency in worker_confirmed_versions,
            f"worker environment reference must report locked OpenFF runtime dependency: {dependency}",
        )
    require("openff-interchange" in worker_source_requirements, "worker source lock must pin OpenFF interchange")
    require("SMIRNOFFTemplateGenerator" in md_worker or "SystemGenerator" in md_worker, "complex MD must use OpenMM/OpenFF small-molecule parameterization")
    require("Chem.RemoveHs" in md_worker and "Chem.AddHs" in md_worker, "complex MD must rebuild hydrogens after docking pose placement")
    require("GpuTelemetryMonitor" in md_worker, "CUDA MD must poll nvidia-smi while OpenMM production steps are running")
    require("production_poll_" in md_worker, "CUDA MD telemetry must label production-time GPU samples")
    require("md_state.tsv" in md_worker, "OpenMM MD must write state data comparable to StateDataReporter output")
    require("temperature_estimate_K" in md_worker, "OpenMM MD state artifact must include temperature evidence")
    require("canonical_smiles" in science and "Chem.MolFromSmiles" in science, "BindingDB onsite matching must use canonical SMILES when RDKit is available")
    require("temp_python_script_path" in science, "onsite Python scripts must run from a script file to avoid Windows cmd quoting loss")
    require('command.push("-c"' not in science, "onsite Python scripts must not pass multiline code through python -c")
    require("CanonicalSourceBinding" in science, "science pipeline must model canonical source-manifest bindings")
    require("bind_database_evidence" in science, "science pipeline must bind database evidence before final consumption")
    require("source_file_manifest.tsv" in science, "database evidence must resolve through the canonical source manifest")
    for binding_field in ["source_id", "source_path", "source_digest", "run_id", "pipeline_invocation_id"]:
        require(binding_field in science, f"database evidence must carry positive provenance field: {binding_field}")
    require("validate_current_run_tsv_provenance" in science, "science pipeline must validate current-run provenance bindings")
    require("ensure_final_artifacts_authentic" in science, "science pipeline must gate final artifact consumption")
    require("authenticity_gate.tsv" in science, "science pipeline must write an authenticity gate report")
    for forbidden in [
        "MAX_RECEPTOR_ATOMS",
        "receptor_heavy_atom_subset",
        "coarse_restrained_complex_model",
        "short_screening_md_protocol",
        "CustomExternalForce",
        "HarmonicBondForce",
        "CustomCentroidBondForce",
    ]:
        require(forbidden not in md_worker, f"complex MD worker must not use nonstandard manual screening mechanics: {forbidden}")
    require('"easy-search"' not in alignment_worker, "alignment worker must use persistent MMseqs2/Foldseek databases instead of per-run easy-search target indexing")
    require("mmseqs_target_databases" in alignment_worker, "MMseqs2 worker must discover persistent target databases")
    require('"--dbtype"' in alignment_worker, "MMseqs2 query databases must declare sequence type")
    require('"--search-type"' in alignment_worker, "MMseqs2 searches must declare search type")
    require('"-a"' in alignment_worker, "MMseqs2 searches must retain alignment backtraces")
    require("source_namespace" in alignment_worker and "source_class" in alignment_worker, "MMseqs2 rows must retain target provenance")
    require("query,target,pident,alnlen,qcov,tcov,evalue,bits,qstart,qend,tstart,tend,cigar" in alignment_worker, "MMseqs2 identity must come from a real alignment")
    require("foldseek_target_database" in alignment_worker, "Foldseek worker must discover persistent target database")
    require('"-a"' in alignment_worker, "Foldseek search must request alignment backtrace for convertalis structure scores")
    require("WorkerOutcome" in alignment_worker and "WorkerOutcome" in md_worker, "alignment and MD workers must emit structured outcomes")
    require("worker_result_stage_error" in science, "Rust must reject not-ready and failed worker outcomes")
    require(
        set(worker_result_schema["properties"]["status"]["enum"])
        == {"PASS", "NOT_APPLICABLE", "NOT_READY", "FAIL"},
        "worker result schema must distinguish completed, inapplicable, not-ready and failed execution",
    )
    outcome_required = set(worker_result_schema["properties"]["outcomes"]["items"]["required"])
    require(
        outcome_required == {"status", "reason", "required", "database_id", "row_count"},
        "worker outcomes must retain status, reason, required flag, database identity and row count",
    )
    require("EXECUTION_RESULT_STATUS" in worker_common, "worker status mapping must be shared across scientific workers")
    require("当前运行没有可展示的候选假设" in science, "report must preserve empty-result language")
    forbidden_report_claims = ["已确认靶点", "确认靶点", "已确认机制", "确认机制", "真实亲和力", "真实靶点"]
    for term in forbidden_report_claims:
        require(term not in science and term not in server, f"confirmed biological claim is not allowed: {term}")
    require("受限 receptor" not in science and "受限 receptor" not in server, "MD report/API must not describe current complex MD as an old restricted receptor model")

    from genoligand_workers import md_summary_worker

    with tempfile.TemporaryDirectory() as tmp:
        tmp_path = Path(tmp)
        ambiguous_smiles = tmp_path / "ambiguous_smiles.json"
        ambiguous_smiles.write_text(
            '{"inputs":[{"field":"small_molecule","value":"CCN"}],"files":[]}',
            encoding="utf-8",
        )
        peptide_text = tmp_path / "peptide_text.json"
        peptide_text.write_text(
            '{"inputs":[{"field":"small_molecule","value":"IKVAV"}],"files":[]}',
            encoding="utf-8",
        )
        protein_ligand_text = tmp_path / "protein_ligand_text.json"
        protein_ligand_text.write_text(
            '{"inputs":[{"field":"protein_ligand","value":"CCN"}],"files":[]}',
            encoding="utf-8",
        )
        smiles, input_id = md_summary_worker.read_small_molecule_input(ambiguous_smiles)
        require((smiles, input_id) == ("CCN", "input_1"), "MD worker fallback must keep ambiguous small-molecule SMILES")
        smiles, _ = md_summary_worker.read_small_molecule_input(peptide_text)
        require(smiles == "", "MD worker fallback must not treat peptide text as a small molecule")
        sequence, input_id = md_summary_worker.read_peptide_sequence_input(peptide_text)
        require((sequence, input_id) == ("IKVAV", "input_1"), "MD worker fallback must recover converted peptide text")
        smiles, _ = md_summary_worker.read_small_molecule_input(protein_ligand_text)
        require(smiles == "", "protein_ligand field must not be treated as a small-molecule hint")
        sequence, input_id = md_summary_worker.read_peptide_sequence_input(protein_ligand_text)
        require((sequence, input_id) == ("CCN", "input_1"), "protein_ligand peptide text must remain peptide-capable")

        binding_run = tmp_path / "binding_run"
        tool_outputs = binding_run / "tool_outputs"
        tool_outputs.mkdir(parents=True)
        candidate_id = "candidate-bound-pdbqt"
        input_id = "input-bound-pdbqt"
        record_id = "record-bound"
        source_digest = "a" * 64
        (binding_run / "complex_candidate_selection.tsv").write_text(
            "candidate_id\tinput_id\trecord_id\ttarget_id\ttarget_name\torganism\t"
            "target_class\tsource\tpdb_id\tevidence\n"
            f"{candidate_id}\t{input_id}\t{record_id}\tT1\tTarget 1\tHomo sapiens\t"
            "protein\tchembl\t1ABC\tselected\n",
            encoding="utf-8",
        )
        (binding_run / "input_conversion_manifest.tsv").write_text(
            "input_id\trecord_id\tcanonical_kind\tcanonical_value\tconversion_status\tcontent_sha256\n"
            f"{input_id}\t{record_id}\tsmall_molecule\tCCO\tconverted\t{source_digest}\n",
            encoding="utf-8",
        )
        _, _, _, input_digest = md_summary_worker.selected_md_input(
            binding_run, input_id, record_id
        )
        _, candidate_digest = md_summary_worker.selected_md_candidate(binding_run, candidate_id, input_id, record_id)
        artifact_names = {
            "receptor_coordinate": "structure_viewer.pdb",
            "receptor_pdb": "tool_outputs/receptor-bound.pdb",
            "receptor_pdbqt": "tool_outputs/receptor-bound.pdbqt",
            "ligand_sdf": "tool_outputs/ligand-bound.sdf",
            "ligand_pdbqt": "tool_outputs/ligand-bound.pdbqt",
            "pose": "tool_outputs/pose-bound.pdbqt",
            "pose_sdf": "tool_outputs/pose-bound.sdf",
            "log": "tool_outputs/vina-bound.log",
        }
        for artifact_name in artifact_names.values():
            artifact_path = binding_run / artifact_name
            artifact_path.parent.mkdir(parents=True, exist_ok=True)
            artifact_path.write_text(f"fixture:{artifact_name}\n", encoding="utf-8")
        receptor_digest = md_summary_worker.sha256_file(binding_run / artifact_names["receptor_coordinate"])
        docking_job_id = "autodock_vina_complex_docking"
        docking_invocation_id = "invocation-bound-pdbqt"
        run_id = binding_run.name
        artifacts = []
        for artifact_name, artifact_value in artifact_names.items():
            artifact_path = binding_run / artifact_value
            artifact = {
                "artifact_name": artifact_name,
                "artifact_path": artifact_path.relative_to(binding_run).as_posix(),
                "artifact_sha256": md_summary_worker.sha256_file(artifact_path),
                "job_id": docking_job_id,
                "run_id": run_id,
                "invocation_id": docking_invocation_id,
                "candidate_id": candidate_id,
                "candidate_digest": candidate_digest,
                "input_id": input_id,
                "record_id": record_id,
                "input_digest": input_digest,
                "source_digest": source_digest,
            }
            if artifact_name == "receptor_coordinate":
                artifact["receptor_digest"] = receptor_digest
            artifacts.append(artifact)
        docking_manifest = tool_outputs / "docking_manifest.json"
        docking_manifest.write_text(
            json.dumps(
                {
                    "artifact_schema_version": "genoligand_docking_artifacts_v1",
                    "status": "completed",
                    "job_id": docking_job_id,
                    "run_id": run_id,
                    "invocation_id": docking_invocation_id,
                    "candidate_id": candidate_id,
                    "candidate_digest": candidate_digest,
                    "input_id": input_id,
                    "record_id": record_id,
                    "input_digest": input_digest,
                    "source_content_sha256": source_digest,
                    "receptor_sha256": receptor_digest,
                    "receptor_digest": receptor_digest,
                    "seed_provenance": {"input_digest": input_digest},
                    "artifacts": artifacts,
                    **artifact_names,
                },
                indent=2,
                sort_keys=True,
            )
            + "\n",
            encoding="utf-8",
        )
        docking_manifest_digest = md_summary_worker.sha256_file(docking_manifest)
        binding = md_summary_worker.resolve_md_invocation(
            {
                "job_id": "md_summary_ligand_dynamics",
                "invocation_id": docking_invocation_id,
                "run_id": run_id,
                "run_dir": str(binding_run),
                "candidate_id": candidate_id,
                "candidate_digest": candidate_digest,
                "input_id": input_id,
                "record_id": record_id,
                "input_digest": input_digest,
                "source_digest": source_digest,
                "docking_manifest": "tool_outputs/docking_manifest.json",
                "docking_manifest_digest": docking_manifest_digest,
                "docking_job_id": docking_job_id,
                "docking_invocation_id": docking_invocation_id,
            }
        )
        expected_ligand_pdbqt = (binding_run / artifact_names["ligand_pdbqt"]).resolve()
        expected_pose_pdbqt = (binding_run / artifact_names["pose"]).resolve()
        require(
            binding.docking_ligand_pdbqt == expected_ligand_pdbqt
            and binding.docking_pose_pdbqt == expected_pose_pdbqt,
            "MD invocation must bind the manifest-selected ligand and pose PDBQT artifacts",
        )

        ligand_marker = object()
        placed_marker = object()
        placement_calls: list[tuple[object, Path, Path, object]] = []

        class FixtureChem:
            @staticmethod
            def SDMolSupplier(path, **kwargs):
                return [ligand_marker]

        original_placement = md_summary_worker.ligand_with_vina_pose_coordinates

        def capture_placement(ligand, ligand_pdbqt, pose_pdbqt, chem):
            placement_calls.append((ligand, ligand_pdbqt, pose_pdbqt, chem))
            return placed_marker

        md_summary_worker.ligand_with_vina_pose_coordinates = capture_placement
        try:
            placed = md_summary_worker.read_docking_pose_ligand(
                binding.docking_pose_sdf,
                binding.docking_ligand_sdf,
                binding.docking_ligand_pdbqt,
                binding.docking_pose_pdbqt,
                FixtureChem,
            )
        finally:
            md_summary_worker.ligand_with_vina_pose_coordinates = original_placement
        require(placed is placed_marker, "complex MD must use Vina PDBQT placement before SDF fallback")
        require(
            placement_calls
            == [(ligand_marker, expected_ligand_pdbqt, expected_pose_pdbqt, FixtureChem)],
            "complex MD must pass the bound ligand and pose PDBQT paths to coordinate placement",
        )

    weak = md_summary_worker.GpuTelemetry(
        rows=[
            {
                "sample_label": "baseline_before_openmm_context",
                "gpu_index": "0",
                "utilization_gpu_percent": "0",
                "memory_used_mb": "1000",
                "current_pid_observed": "false",
            },
            {
                "sample_label": "production_segment_1",
                "gpu_index": "0",
                "utilization_gpu_percent": "35",
                "memory_used_mb": "1000",
                "current_pid_observed": "false",
            },
        ],
        available=True,
    )
    require(
        not md_summary_worker.summarize_gpu_telemetry("CUDA", weak)["gpu_observed"],
        "CUDA MD evidence must not accept unrelated utilization without process or memory evidence",
    )
    no_production = md_summary_worker.GpuTelemetry(
        rows=[
            {
                "sample_label": "after_openmm_context",
                "gpu_index": "0",
                "utilization_gpu_percent": "5",
                "memory_used_mb": "1200",
                "current_pid_observed": "true",
            },
        ],
        available=True,
    )
    require(
        not md_summary_worker.summarize_gpu_telemetry("CUDA", no_production)["gpu_observed"],
        "CUDA MD evidence must require production-time GPU samples",
    )
    strong = md_summary_worker.GpuTelemetry(
        rows=[
            {
                "sample_label": "baseline_before_openmm_context",
                "gpu_index": "0",
                "utilization_gpu_percent": "0",
                "memory_used_mb": "1000",
                "current_pid_observed": "false",
            },
            {
                "sample_label": "production_poll_1",
                "gpu_index": "0",
                "utilization_gpu_percent": "35",
                "memory_used_mb": "1130",
                "current_pid_observed": "false",
            },
        ],
        available=True,
    )
    strong_status = md_summary_worker.summarize_gpu_telemetry("CUDA", strong)
    require(strong_status["gpu_observed"], "CUDA MD evidence must accept production-time memory and utilization proof")
    require(strong_status["production_sample_count"] == 1, "CUDA MD status must count production-time GPU samples")
    require(strong_status["observation_basis"] == "memory_delta_and_utilization", "CUDA MD status must state the evidence basis")

    original_sample = md_summary_worker.sample_gpu_telemetry
    try:
        monitored = md_summary_worker.GpuTelemetry(rows=[])

        def fake_sample(telemetry, label):
            telemetry.available = True
            telemetry.rows.append(
                {
                    "sample_label": label,
                    "gpu_index": "0",
                    "utilization_gpu_percent": "18",
                    "memory_used_mb": "1200",
                    "current_pid_observed": "true",
                }
            )

        md_summary_worker.sample_gpu_telemetry = fake_sample
        with md_summary_worker.GpuTelemetryMonitor(
            monitored,
            enabled=True,
            label_prefix="production_poll",
            interval_seconds=0.01,
        ):
            time.sleep(0.03)
        require(
            any(row.get("sample_label", "").startswith("production_poll_") for row in monitored.rows),
            "CUDA telemetry monitor must capture production polling samples",
        )
    finally:
        md_summary_worker.sample_gpu_telemetry = original_sample

    print("authenticity_contracts\tPASS")
    return 0


if __name__ == "__main__":
    sys.exit(main())
