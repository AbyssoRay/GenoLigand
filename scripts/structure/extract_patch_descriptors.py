import argparse
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "python"))

from genoligand_gpu.io import read_tsv


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--patches", required=True)
    parser.add_argument("--min-rows", type=int, default=1)
    args = parser.parse_args()
    rows = read_tsv(Path(args.patches))
    if len(rows) < args.min_rows:
        raise SystemExit("patch_rows_below_minimum")
    print("patch_descriptors_ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
