import argparse
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "python"))

from genoligand_gpu.io import read_json, read_tsv


MANDATORY = [
    "p03_k1_cds_001",
    "p03_k2_alt_frame_001",
    "p03_d1_low_complexity_001",
    "p04_d3_shuffled_p03_k1_cds_001",
    "p04_d4_reversed_p03_k1_cds_001",
    "p04_d5_random_matched_length_001",
]


def require(path: Path) -> None:
    if not path.exists():
        raise SystemExit(f"phase_0_5_missing:{path}")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--artifact-dir", required=True)
    parser.add_argument("--config", required=True)
    args = parser.parse_args()
    root = Path(args.artifact_dir)
    required = [
        "metrics/cuda_env.json",
        "metrics/cuda_tensor_smoke_test.json",
        "metrics/torch_cuda_probe.json",
        "metrics/gpu_memory_probe_plm.json",
        "metrics/gpu_memory_probe_structure.json",
        "metrics/phase_0_5_gate_summary.tsv",
        "metrics/python_package_freeze.txt",
        "metrics/nextflow_trace_first.tsv",
        "metrics/nextflow_trace_resume.tsv",
        "metrics/nextflow_report.html",
        "metrics/nextflow_timeline.html",
        "metrics/structure_semantic_hash_compare.tsv",
        "outputs/plm_gpu/plm_embeddings.tsv",
        "outputs/plm_gpu/plm_manifest.json",
        "outputs/plm_gpu/plm_embedding_hashes.tsv",
        "outputs/structures/structure_manifest.tsv",
        "outputs/structures/structure_backend_manifest.json",
        "outputs/structures/structure_hashes.tsv",
        "outputs/structures/structure_semantic_hashes_first.tsv",
        "outputs/structures/structure_semantic_hashes_resume.tsv",
        "outputs/structure_features/structure_quality.tsv",
        "outputs/structure_features/ca_graph_summary.tsv",
        "outputs/structure_features/secondary_structure_summary.tsv",
        "outputs/patches/patch_manifest.json",
        "outputs/patches/patch_descriptors.tsv",
        "outputs/reports/candidate_report_phase_0_5.md",
        "outputs/reports/structure_score_decomposition.tsv",
        "outputs/reports/structure_evidence_summary.tsv",
        "logs/cuda_env_stdout_stderr.txt",
        "logs/plm_gpu_stdout_stderr.txt",
        "logs/structure_gpu_stdout_stderr.txt",
        "logs/nextflow_gpu_stdout_stderr.txt",
        "logs/python_package_freeze.txt",
        "external_resource_manifest.tsv",
        "hashes.tsv",
    ]
    for rel in required:
        require(root / rel)
    cuda = read_json(root / "metrics/cuda_env.json")
    if not cuda.get("phase_0_5_gpu_eligible"):
        raise SystemExit("cuda_env_not_gpu_eligible")
    if cuda.get("torch_version_contains_cpu"):
        raise SystemExit("torch_cpu_wheel_rejected")
    smoke = read_json(root / "metrics/cuda_tensor_smoke_test.json")
    if not smoke.get("success"):
        raise SystemExit("cuda_tensor_smoke_failed")
    plm_rows = read_tsv(root / "outputs/plm_gpu/plm_embeddings.tsv")
    if not plm_rows or any(not row.get("device", "").startswith("cuda") for row in plm_rows):
        raise SystemExit("plm_gpu_rows_missing_or_not_cuda")
    structure_rows = read_tsv(root / "outputs/structures/structure_manifest.tsv")
    by_id = {row["candidate_id"]: row for row in structure_rows}
    for candidate_id in MANDATORY:
        row = by_id.get(candidate_id)
        if not row or row.get("prediction_status") != "success":
            raise SystemExit(f"mandatory_structure_missing:{candidate_id}")
        if not row.get("device", "").startswith("cuda"):
            raise SystemExit(f"mandatory_structure_not_cuda:{candidate_id}")
        if not Path(row["structure_path"]).exists():
            raise SystemExit(f"mandatory_structure_file_missing:{candidate_id}")
        for column in [
            "mean_plddt",
            "median_plddt",
            "min_plddt",
            "low_confidence_fraction_plddt_lt_50",
            "very_low_confidence_fraction_plddt_lt_30",
            "residue_count_in_structure",
            "chain_count",
            "gpu_memory_peak_mib",
            "runtime_seconds",
        ]:
            if row.get(column, "") == "":
                raise SystemExit(f"mandatory_structure_missing_column_value:{candidate_id}:{column}")
        if not row.get("mmcif_path"):
            raise SystemExit(f"mandatory_mmcif_missing:{candidate_id}")
        mmcif_path = Path(row["mmcif_path"])
        if not mmcif_path.exists() or not row.get("mmcif_sha256"):
            raise SystemExit(f"mandatory_mmcif_missing:{candidate_id}")
        if not row.get("structure_sha256"):
            raise SystemExit(f"mandatory_structure_hash_missing:{candidate_id}")
        if int(float(row.get("atom_count", "0"))) <= 0:
            raise SystemExit(f"mandatory_structure_empty_atoms:{candidate_id}")
        if int(float(row.get("residue_count_in_structure", "0"))) <= 0:
            raise SystemExit(f"mandatory_structure_empty_residues:{candidate_id}")
    quality_ids = {row["candidate_id"] for row in read_tsv(root / "outputs/structure_features/structure_quality.tsv")}
    patch_ids = {row["candidate_id"] for row in read_tsv(root / "outputs/patches/patch_descriptors.tsv")}
    for candidate_id in MANDATORY:
        if candidate_id not in quality_ids:
            raise SystemExit(f"quality_row_missing:{candidate_id}")
        if candidate_id not in patch_ids:
            raise SystemExit(f"patch_row_missing:{candidate_id}")
    memory = read_json(root / "metrics/gpu_memory_probe_structure.json")
    if not memory.get("success"):
        raise SystemExit("structure_gpu_memory_probe_failed")
    if int(memory.get("peak_delta_mib", 0)) < 100 and int(memory.get("torch_max_memory_allocated_bytes", 0)) <= 0:
        raise SystemExit("structure_gpu_memory_delta_missing")
    compare = read_tsv(root / "metrics/structure_semantic_hash_compare.tsv")
    if not compare or any(row.get("match") != "true" for row in compare):
        raise SystemExit("structure_semantic_hash_resume_mismatch")
    report = (root / "outputs/reports/candidate_report_phase_0_5.md").read_text(encoding="utf-8-sig")
    for section in [
        "## CUDA Execution Summary",
        "## GPU pLM Embedding Summary",
        "## Structure Prediction Summary",
        "## 3D Patch Descriptor Summary",
        "## Candidate Ranking Table",
        "## Interpretation Cautions",
    ]:
        if section not in report:
            raise SystemExit(f"candidate_report_missing_section:{section}")
    print("phase_0_5_gpu_strict_ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
