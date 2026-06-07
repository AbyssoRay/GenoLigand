from __future__ import annotations

import argparse
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
PYTHON_ROOT = ROOT / "python"
if str(PYTHON_ROOT) not in sys.path:
    sys.path.insert(0, str(PYTHON_ROOT))

from genoligand_phase07.audit_fix import PHASE07_DIRNAME, PHASE071_DIRNAME, validate_phase071


def path_arg(value: str) -> Path:
    return Path(value).resolve()


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--artifact-dir", type=path_arg, default=ROOT / PHASE071_DIRNAME)
    parser.add_argument("--phase07-dir", type=path_arg, default=ROOT / PHASE07_DIRNAME)
    parser.add_argument("--strict", action="store_true")
    parser.add_argument("--pre-report", action="store_true")
    args = parser.parse_args()
    passed, _ = validate_phase071(args.artifact_dir, args.phase07_dir, include_report=not args.pre_report)
    if passed:
        print("phase_0_7_1_audit_fix_strict_ok")
        raise SystemExit(0)
    print("phase_0_7_1_audit_fix_strict_fail")
    raise SystemExit(1 if args.strict else 0)
