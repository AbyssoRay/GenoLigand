import argparse
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "python"))

from genoligand_gpu.io import read_tsv


def require(path: Path) -> None:
    if not path.exists():
        raise SystemExit(f"missing_regression_artifact:{path}")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--phase04-dir", required=True)
    parser.add_argument("--out", required=True)
    args = parser.parse_args()
    root = Path(args.phase04_dir)
    required = [
        "outputs/candidates/candidate_orfs.fasta",
        "outputs/candidates/candidate_orfs.tsv",
        "outputs/sequence_features/sequence_features.tsv",
        "outputs/similarity/similarity_summary.tsv",
        "outputs/sequence_evidence/sequence_evidence.tsv",
        "outputs/reports/candidate_report_phase_0_4.md",
        "metrics/phase_0_3_regression_gate_summary.tsv",
        "acceptance_report_phase_0_4.md",
    ]
    for rel in required:
        require(root / rel)
    candidate_rows = read_tsv(root / "outputs/candidates/candidate_orfs.tsv")
    ids = {row["candidate_id"] for row in candidate_rows}
    checks = {
        "phase_0_4_report_pass": "FINAL_VERDICT: PASS" in (root / "acceptance_report_phase_0_4.md").read_text(encoding="utf-8-sig"),
        "d3_present": any("p04_d3" in item for item in ids),
        "d4_present": any("p04_d4" in item for item in ids),
        "d5_present": any("p04_d5" in item for item in ids),
        "k1_present": "p03_k1_cds_001" in ids,
        "k2_present": "p03_k2_alt_frame_001" in ids,
    }
    rows = [{"gate_id": key, "status": "PASS" if value else "FAIL"} for key, value in checks.items()]
    Path(args.out).parent.mkdir(parents=True, exist_ok=True)
    import csv

    with Path(args.out).open("w", encoding="utf-8", newline="") as handle:
        writer = csv.DictWriter(handle, delimiter="\t", fieldnames=["gate_id", "status"], lineterminator="\n")
        writer.writeheader()
        writer.writerows(rows)
    if not all(checks.values()):
        raise SystemExit("phase_0_5_regressions_failed")
    print("phase_0_5_regressions_ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
