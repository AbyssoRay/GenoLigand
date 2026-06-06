from __future__ import annotations

import argparse

from _bootstrap import DEFAULT_ARTIFACT_DIR, DEFAULT_PHASE05_DIR, path_arg
from genoligand_phase06.pipeline import check_phase05_inputs


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--phase05-dir", type=path_arg, default=DEFAULT_PHASE05_DIR)
    parser.add_argument("--out", type=path_arg, default=DEFAULT_ARTIFACT_DIR / "metrics/phase05_input_check.json")
    args = parser.parse_args()
    return check_phase05_inputs(args.phase05_dir, args.out)


if __name__ == "__main__":
    raise SystemExit(main())
