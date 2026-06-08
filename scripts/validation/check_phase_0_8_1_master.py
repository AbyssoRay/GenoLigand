from __future__ import annotations

import argparse
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
PYTHON_ROOT = ROOT / "python"
if str(PYTHON_ROOT) not in sys.path:
    sys.path.insert(0, str(PYTHON_ROOT))

from genoligand_phase081.database_audit_fix_strict import PHASE081_DIRNAME, PHASE071_DIRNAME, PHASE08_DIRNAME, path_arg, validate_phase081


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--artifact-dir", type=path_arg, default=ROOT / PHASE081_DIRNAME)
    parser.add_argument("--phase08-dir", type=path_arg, default=ROOT / PHASE08_DIRNAME)
    parser.add_argument("--phase071-dir", type=path_arg, default=ROOT / PHASE071_DIRNAME)
    parser.add_argument("--strict", action="store_true")
    args = parser.parse_args()
    passed, _ = validate_phase081(args.artifact_dir, args.phase08_dir, args.phase071_dir)
    if passed:
        print("phase_0_8_1_database_audit_fix_strict_ok")
        raise SystemExit(0)
    print("phase_0_8_1_database_audit_fix_strict_fail")
    raise SystemExit(1 if args.strict else 0)

