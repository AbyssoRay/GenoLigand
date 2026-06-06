import argparse
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "python"))

from genoligand_gpu.io import read_tsv


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--manifest", required=True)
    args = parser.parse_args()
    rows = read_tsv(Path(args.manifest))
    missing = [row["candidate_id"] for row in rows if row.get("prediction_status") == "success" and not Path(row["structure_path"]).exists()]
    if missing:
        raise SystemExit("structure_files_missing:" + ",".join(missing))
    print("structure_outputs_parseable")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
