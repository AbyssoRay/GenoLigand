import argparse
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "python"))

from genoligand_gpu.io import read_tsv, write_tsv


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--first", required=True)
    parser.add_argument("--resume", required=True)
    parser.add_argument("--out", required=True)
    args = parser.parse_args()
    first = {row["candidate_id"]: row["semantic_structure_hash"] for row in read_tsv(Path(args.first))}
    resume = {row["candidate_id"]: row["semantic_structure_hash"] for row in read_tsv(Path(args.resume))}
    rows = []
    ok = True
    for candidate_id, hash_value in sorted(first.items()):
        match = resume.get(candidate_id) == hash_value
        ok = ok and match
        rows.append({"candidate_id": candidate_id, "first_hash": hash_value, "resume_hash": resume.get(candidate_id, ""), "match": str(match).lower()})
    write_tsv(Path(args.out), rows, ["candidate_id", "first_hash", "resume_hash", "match"])
    if not ok:
        raise SystemExit("semantic_structure_hash_mismatch")
    print("semantic_structure_hash_ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
