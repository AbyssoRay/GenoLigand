from __future__ import annotations

import argparse
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
PYTHON_ROOT = ROOT / "python"
if str(PYTHON_ROOT) not in sys.path:
    sys.path.insert(0, str(PYTHON_ROOT))

from genoligand_phase081.database_audit_fix_strict import PHASE081_DIRNAME, check_direct_report_values, path_arg


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--artifact-dir", type=path_arg, default=ROOT / PHASE081_DIRNAME)
    parser.add_argument("--strict", action="store_true")
    args = parser.parse_args()
    code = check_direct_report_values(args.artifact_dir)
    print("phase_0_8_1_direct_report_values_ok" if code == 0 else "phase_0_8_1_direct_report_values_fail")
    raise SystemExit(code if args.strict else 0)

