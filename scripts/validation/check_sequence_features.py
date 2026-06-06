import argparse
import csv
import math


REQUIRED = [
    "candidate_id",
    "aa_length",
    "aa_valid",
    "low_complexity_score",
    "max_homopolymer_run",
    "mean_kd_hydropathy",
    "sequence_risk_flags",
    "feature_config_sha256",
]


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("features")
    args = parser.parse_args()
    with open(args.features, "r", encoding="utf-8-sig", newline="") as handle:
        rows = list(csv.DictReader(handle, delimiter="\t"))
    if not rows:
        raise SystemExit("sequence_features_empty")
    for field in REQUIRED:
        if field not in rows[0]:
            raise SystemExit(f"sequence_features_missing_column:{field}")
    for row in rows:
        for field in ["low_complexity_score", "mean_kd_hydropathy"]:
            if not math.isfinite(float(row[field])):
                raise SystemExit(f"sequence_features_nonfinite:{row['candidate_id']}:{field}")
    if not any("low_complexity" in row["sequence_risk_flags"] for row in rows):
        raise SystemExit("sequence_features_missing_low_complexity_flag")
    print(f"sequence_features_ok\trows={len(rows)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
