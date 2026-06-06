import argparse
import csv
from pathlib import Path


FIELDS = [
    "candidate_id",
    "best_hit_accession",
    "best_hit_name",
    "best_hit_evalue",
    "best_hit_bitscore",
    "best_hit_pident",
    "best_hit_query_cover",
    "reviewed_hit_present",
    "known_similarity_flag",
    "novelty_flag",
    "short_sequence_alignment_warning",
    "similarity_backend",
    "similarity_database",
    "similarity_config_sha256",
]


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--candidate-tsv", required=True)
    parser.add_argument("--out-summary", required=True)
    parser.add_argument("--out-hits", required=True)
    args = parser.parse_args()
    with open(args.candidate_tsv, "r", encoding="utf-8-sig", newline="") as handle:
        candidates = list(csv.DictReader(handle, delimiter="\t"))
    rows = []
    for row in candidates:
        length = int(row["aa_length"])
        rows.append(
            {
                "candidate_id": row["candidate_id"],
                "best_hit_accession": "",
                "best_hit_name": "",
                "best_hit_evalue": "",
                "best_hit_bitscore": "",
                "best_hit_pident": "",
                "best_hit_query_cover": "",
                "reviewed_hit_present": "false",
                "known_similarity_flag": "false",
                "novelty_flag": "true",
                "short_sequence_alignment_warning": str(length < 30).lower(),
                "similarity_backend": "fixture_mock",
                "similarity_database": "fixture_mock",
                "similarity_config_sha256": "fixture_mock",
            }
        )
    write_tsv(args.out_summary, rows, FIELDS)
    write_tsv(args.out_hits, [], ["candidate_id", "backend", "database_id", "hit_rank"])
    return 0


def write_tsv(path: str, rows: list[dict], fields: list[str]) -> None:
    Path(path).parent.mkdir(parents=True, exist_ok=True)
    with open(path, "w", encoding="utf-8", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=fields, delimiter="\t", lineterminator="\n")
        writer.writeheader()
        writer.writerows(rows)


if __name__ == "__main__":
    raise SystemExit(main())
