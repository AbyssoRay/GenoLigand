import argparse
import csv


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--candidates", required=True)
    parser.add_argument("--summary", required=True)
    parser.add_argument("--hits", required=True)
    args = parser.parse_args()
    with open(args.candidates, "r", encoding="utf-8-sig", newline="") as handle:
        candidates = list(csv.DictReader(handle, delimiter="\t"))
    with open(args.summary, "r", encoding="utf-8-sig", newline="") as handle:
        summary = list(csv.DictReader(handle, delimiter="\t"))
    candidate_ids = {row["candidate_id"] for row in candidates}
    summary_ids = {row["candidate_id"] for row in summary}
    if candidate_ids != summary_ids:
        raise SystemExit("similarity_summary_candidate_coverage_mismatch")
    if not summary:
        raise SystemExit("similarity_summary_empty")
    if any(row.get("similarity_backend") == "fixture_mock" for row in summary):
        raise SystemExit("similarity_backend_fixture_mock_not_allowed_for_phase_0_4_pass")
    for row in summary:
        if row["short_sequence_alignment_warning"] not in {"true", "false"}:
            raise SystemExit(f"bad_short_sequence_warning:{row['candidate_id']}")
    print(f"similarity_outputs_ok\trows={len(summary)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
