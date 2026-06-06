import argparse
import csv


SCHEMAS = {
    "candidates": ["candidate_id", "aa_sequence", "aa_sequence_sha256", "export_status"],
    "features": ["candidate_id", "aa_length", "low_complexity_score", "sequence_risk_flags"],
    "embeddings": ["candidate_id", "backend", "embedding_dim", "plm_status"],
    "similarity": ["candidate_id", "similarity_backend", "short_sequence_alignment_warning"],
    "evidence": ["candidate_id", "sequence_evidence_score", "sequence_evidence_rank"],
}


def main() -> int:
    parser = argparse.ArgumentParser()
    for name in SCHEMAS:
        parser.add_argument(f"--{name}", required=True)
    args = parser.parse_args()
    for name, columns in SCHEMAS.items():
        path = getattr(args, name)
        with open(path, "r", encoding="utf-8-sig", newline="") as handle:
            reader = csv.DictReader(handle, delimiter="\t")
            missing = [column for column in columns if column not in (reader.fieldnames or [])]
            if missing:
                raise SystemExit(f"schema_missing:{name}:{','.join(missing)}")
            if not list(reader):
                raise SystemExit(f"schema_empty:{name}")
    print("phase_0_4_schema_ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
