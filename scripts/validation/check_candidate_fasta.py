import argparse
import csv
from pathlib import Path


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--fasta", required=True)
    parser.add_argument("--tsv", required=True)
    args = parser.parse_args()
    ids = []
    with open(args.fasta, "r", encoding="utf-8-sig") as handle:
        for line in handle:
            if line.startswith(">"):
                header = line[1:].strip()
                for token in ["context=", "orf_len=", "source=", "phase=0.4"]:
                    if token not in header:
                        raise SystemExit(f"fasta_header_missing_{token}:{header}")
                ids.append(header.split("|", 1)[0])
    with open(args.tsv, "r", encoding="utf-8-sig", newline="") as handle:
        rows = list(csv.DictReader(handle, delimiter="\t"))
    tsv_ids = [row["candidate_id"] for row in rows]
    if sorted(ids) != sorted(tsv_ids):
        raise SystemExit("fasta_tsv_candidate_id_mismatch")
    for row in rows:
        if not row.get("aa_sequence_sha256") or len(row["aa_sequence_sha256"]) != 64:
            raise SystemExit(f"bad_sequence_sha256:{row.get('candidate_id')}")
    print(f"candidate_fasta_ok\trows={len(rows)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
