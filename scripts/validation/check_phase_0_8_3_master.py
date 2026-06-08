from __future__ import annotations

import argparse
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
PYTHON_ROOT = ROOT / "python"
if str(PYTHON_ROOT) not in sys.path:
    sys.path.insert(0, str(PYTHON_ROOT))

from genoligand_phase083.biolip2_resolver_patch_strict import PHASE083_DIRNAME, path_arg, validate_phase083


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--artifact-dir", type=path_arg, default=ROOT / PHASE083_DIRNAME)
    parser.add_argument("--strict", action="store_true")
    args = parser.parse_args()
    passed, _gates = validate_phase083(args.artifact_dir, strict_report=True)
    print("phase_0_8_3_biolip2_resolver_patch_strict_ok" if passed else "phase_0_8_3_biolip2_resolver_patch_strict_fail")
    raise SystemExit(0 if passed or not args.strict else 1)

