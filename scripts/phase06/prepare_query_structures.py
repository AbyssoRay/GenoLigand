from __future__ import annotations

import argparse

from _bootstrap import DEFAULT_ARTIFACT_DIR, DEFAULT_PHASE05_DIR, path_arg
from genoligand_phase06.pipeline import prepare_query_structures


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--phase05-dir", type=path_arg, default=DEFAULT_PHASE05_DIR)
    parser.add_argument("--out-dir", type=path_arg, default=DEFAULT_ARTIFACT_DIR / "outputs/query_structures")
    args = parser.parse_args()
    prepare_query_structures(args.phase05_dir, args.out_dir)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
