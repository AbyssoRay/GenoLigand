from __future__ import annotations

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
    worker_cpu_requirements = read("python/requirements-workers-cpu.txt")
    worker_cuda_requirements = read("python/requirements-workers-cuda.txt")
    worker_environment = read("python/environment-workers.yml")

    require("无输入、无数据库、无工具或无运行产物时" in architecture, "authenticity rule missing from architecture docs")
    require("candidate_count" in science, "science pipeline must write candidate count metadata")
    require("user_candidate_rows" in server, "server must derive candidate rows from run artifacts")
    require("candidate_detail_json" in server, "server must expose candidate details from run artifacts")
    require("structure_viewer_payload" in server, "server must expose viewer data only from run structure artifacts")
    require("md_chart_rows" in server, "server must derive MD charts from run MD artifacts")
    require("structure_viewer.pdb" in server, "viewer must be backed by explicit structure coordinate artifacts")
    require("input_conversion_manifest.tsv" in science, "science pipeline must write input conversion manifest")
    require("read_input_conversion_manifest" in md_worker, "MD worker must prefer canonical converted inputs")
    require("read_peptide_sequence_input" in md_worker, "MD worker must accept peptide sequence inputs")
    require("MolFromFASTA" in md_worker, "MD worker must build peptide molecules through RDKit peptide conversion")
    require("rdkit_peptide_conformer_ensemble" in md_worker, "peptide MD artifacts must identify the real conformer calculation method")
    require("no_receptor_contact_model" in md_worker, "peptide MD artifacts must mark missing receptor contact modeling")
    require("openmmforcefields" in worker_cpu_requirements, "CPU worker environment must install OpenMM small-molecule force-field support")
    require("openmmforcefields" in worker_cuda_requirements, "CUDA worker environment must install OpenMM small-molecule force-field support")
    require("openmmforcefields" in worker_environment, "conda worker environment must install OpenMM small-molecule force-field support")
    for dependency in ["xmltodict", "python-constraint", "lxml"]:
        require(dependency in worker_cpu_requirements, f"CPU worker environment must install OpenFF runtime dependency: {dependency}")
        require(dependency in worker_cuda_requirements, f"CUDA worker environment must install OpenFF runtime dependency: {dependency}")
        require(dependency in worker_environment, f"conda worker environment must install OpenFF runtime dependency: {dependency}")
    require("openff-interchange" in worker_cpu_requirements, "CPU worker environment must install OpenFF interchange")
    require("openff-interchange" in worker_cuda_requirements, "CUDA worker environment must install OpenFF interchange")
    require("openff-interchange" in worker_environment, "conda worker environment must install OpenFF interchange")
    require("SMIRNOFFTemplateGenerator" in md_worker or "SystemGenerator" in md_worker, "complex MD must use OpenMM/OpenFF small-molecule parameterization")
    require("ligand_with_vina_pose_coordinates" in md_worker, "complex MD must map Vina pose coordinates onto the RDKit ligand topology")
    require("docking_ligand.pdbqt" in md_worker and "docking_pose.pdbqt" in md_worker, "complex MD must use Vina PDBQT coordinates for ligand placement")
    require("Chem.RemoveHs" in md_worker and "Chem.AddHs" in md_worker, "complex MD must rebuild hydrogens after docking pose placement")
    require("GpuTelemetryMonitor" in md_worker, "CUDA MD must poll nvidia-smi while OpenMM production steps are running")
    require("production_poll_" in md_worker, "CUDA MD telemetry must label production-time GPU samples")
    require("md_state.tsv" in md_worker, "OpenMM MD must write state data comparable to StateDataReporter output")
    require("temperature_estimate_K" in md_worker, "OpenMM MD state artifact must include temperature evidence")
    require("canonical_smiles" in science and "Chem.MolFromSmiles" in science, "BindingDB onsite matching must use canonical SMILES when RDKit is available")
    require("temp_python_script_path" in science, "onsite Python scripts must run from a script file to avoid Windows cmd quoting loss")
    require('command.push("-c"' not in science, "onsite Python scripts must not pass multiline code through python -c")
    require("AUTHENTICITY_FORBIDDEN_MARKERS" in science, "science pipeline must define forbidden final artifact source markers")
    require("ensure_final_artifacts_authentic" in science, "science pipeline must gate final artifact consumption")
    require("authenticity_gate.tsv" in science, "science pipeline must write an authenticity gate report")
    for forbidden_source in ["static_fallback_candidate", "demo_data", "smoke", "fixture"]:
        require(forbidden_source in science, f"authenticity gate must block {forbidden_source}")
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
    require("foldseek_target_database" in alignment_worker, "Foldseek worker must discover persistent target database")
    require('"-a"' in alignment_worker, "Foldseek search must request alignment backtrace for convertalis structure scores")
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
