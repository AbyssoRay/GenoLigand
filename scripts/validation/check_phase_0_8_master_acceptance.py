from __future__ import annotations

import argparse
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
PYTHON_ROOT = ROOT / "python"
if str(PYTHON_ROOT) not in sys.path:
    sys.path.insert(0, str(PYTHON_ROOT))

from genoligand_phase08.full_database_strict import PHASE08_DIRNAME, path_arg, validate_phase08


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--artifact-dir", type=path_arg, default=ROOT / PHASE08_DIRNAME)
    parser.add_argument("--strict", action="store_true")
    parser.add_argument("--pre-report", action="store_true")
    args = parser.parse_args()
    passed, _ = validate_phase08(args.artifact_dir, include_report=not args.pre_report)
    if passed:
        print("phase_0_8_full_database_strict_ok")
        raise SystemExit(0)
    print("phase_0_8_full_database_strict_fail")
    raise SystemExit(1 if args.strict else 0)

