from __future__ import annotations

import argparse
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from _bootstrap import DEFAULT_ARTIFACT_DIR, path_arg
from genoligand_phase07.pipeline import render_acceptance_report


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--artifact-dir", type=path_arg, default=DEFAULT_ARTIFACT_DIR)
    parser.add_argument("--out", type=path_arg, default=DEFAULT_ARTIFACT_DIR / "acceptance_report_phase_0_7.md")
    parser.add_argument("--verdict", default="FAIL")
    parser.add_argument("--failure", action="append", default=[])
    args = parser.parse_args()
    return render_acceptance_report(args.artifact_dir, args.out, args.verdict, args.failure)


if __name__ == "__main__":
    raise SystemExit(main())
