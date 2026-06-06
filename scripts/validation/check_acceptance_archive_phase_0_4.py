import argparse
from pathlib import Path


REQUIRED = [
    "outputs/candidates/candidate_orfs.fasta",
    "outputs/candidates/candidate_orfs.tsv",
    "outputs/sequence_features/sequence_features.tsv",
    "outputs/plm_embeddings/embedding_manifest.tsv",
    "outputs/plm_embeddings/embedding_metadata.json",
    "outputs/similarity/similarity_hits.tsv",
    "outputs/similarity/similarity_summary.tsv",
    "outputs/sequence_evidence/sequence_evidence.tsv",
    "outputs/reports/candidate_report_phase_0_4.md",
    "metrics/command_summary.tsv",
    "metrics/cuda_env.json",
    "metrics/phase_0_4_gate_summary.tsv",
    "metrics/phase_0_3_regression_gate_summary.tsv",
    "metrics/model_resource_summary.tsv",
    "logs/python_env_create.log",
    "logs/python_package_freeze.txt",
    "logs/nextflow_first_run.log",
    "logs/nextflow_resume_run.log",
    "hashes.tsv",
    "external_resource_manifest.tsv",
    "acceptance_report_phase_0_4.md",
]


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("artifact_dir")
    args = parser.parse_args()
    root = Path(args.artifact_dir)
    missing = [path for path in REQUIRED if not (root / path).exists()]
    if missing:
        raise SystemExit("phase_0_4_archive_missing:" + ",".join(missing))
    report = (root / "acceptance_report_phase_0_4.md").read_text(encoding="utf-8-sig")
    if "FINAL_VERDICT: PASS" not in report:
        raise SystemExit("phase_0_4_archive_report_not_pass")
    print("phase_0_4_acceptance_archive_ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
