from __future__ import annotations

import argparse

from _bootstrap import DEFAULT_ARTIFACT_DIR, path_arg
from genoligand_phase06.pipeline import render_candidate_report


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--artifact-dir", type=path_arg, default=DEFAULT_ARTIFACT_DIR)
    parser.add_argument("--out", type=path_arg, default=DEFAULT_ARTIFACT_DIR / "outputs/reports/candidate_report_phase_0_6.md")
    args = parser.parse_args()
    return render_candidate_report(args.artifact_dir, args.out)


if __name__ == "__main__":
    raise SystemExit(main())
