import argparse
import csv
import math


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("scores")
    args = parser.parse_args()
    with open(args.scores, "r", encoding="utf-8-sig", newline="") as handle:
        rows = list(csv.DictReader(handle, delimiter="\t"))
    if not rows:
        raise SystemExit("sequence_evidence_empty")
    ranks = sorted(int(row["sequence_evidence_rank"]) for row in rows)
    if ranks != list(range(1, len(rows) + 1)):
        raise SystemExit("sequence_evidence_bad_rank_sequence")
    for row in rows:
        score = float(row["sequence_evidence_score"])
        if not math.isfinite(score) or not (0 <= score <= 1):
            raise SystemExit(f"sequence_evidence_score_out_of_range:{row['candidate_id']}")
    if not any("control" in row["sequence_evidence_flags"] for row in rows):
        raise SystemExit("sequence_evidence_missing_control_flags")
    print(f"sequence_evidence_scores_ok\trows={len(rows)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
