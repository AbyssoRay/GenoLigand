from __future__ import annotations

import argparse

from _bootstrap import DEFAULT_ARTIFACT_DIR, path_arg
from genoligand_phase06.pipeline import render_acceptance_report


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--artifact-dir", type=path_arg, default=DEFAULT_ARTIFACT_DIR)
    parser.add_argument("--out", type=path_arg, default=DEFAULT_ARTIFACT_DIR / "acceptance_report_phase_0_6.md")
    parser.add_argument("--verdict", choices=["PASS", "FAIL"], default="FAIL")
    parser.add_argument("--failures", type=path_arg)
    args = parser.parse_args()
    failures: list[str] = []
    if args.failures and args.failures.exists():
        failures = [line.strip() for line in args.failures.read_text(encoding="utf-8-sig", errors="replace").splitlines() if line.strip()]
    return render_acceptance_report(args.artifact_dir, args.out, args.verdict, failures)


if __name__ == "__main__":
    raise SystemExit(main())
