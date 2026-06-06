import argparse
from pathlib import Path

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
        "metrics/gpu_memory_probe_plm.json",
        "metrics/gpu_memory_probe_structure.json",
        "metrics/phase_0_5_gate_summary.tsv",
        "outputs/plm_gpu/plm_embeddings.tsv",
        "outputs/plm_gpu/plm_manifest.json",
        "outputs/structures/structure_manifest.tsv",
        "outputs/structures/structure_backend_manifest.json",
        "outputs/structure_features/structure_quality.tsv",
        "outputs/structure_features/ca_graph_summary.tsv",
        "outputs/patches/patch_descriptors.tsv",
        "outputs/reports/candidate_report_phase_0_5.md",
        "outputs/reports/structure_score_decomposition.tsv",
        "external_resource_manifest.tsv",
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
