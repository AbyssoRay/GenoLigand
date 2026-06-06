import argparse
import csv


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--evidence", required=True)
    parser.add_argument("--report", required=True)
    args = parser.parse_args()
    with open(args.evidence, "r", encoding="utf-8-sig", newline="") as handle:
        rows = list(csv.DictReader(handle, delimiter="\t"))
    report = open(args.report, "r", encoding="utf-8-sig").read()
    for row in rows[:5]:
        if row["candidate_id"] not in report:
            raise SystemExit(f"report_missing_top_candidate:{row['candidate_id']}")
    print("sequence_evidence_report_sync_ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
