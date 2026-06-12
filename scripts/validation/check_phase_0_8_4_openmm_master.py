from __future__ import annotations

import argparse
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
PYTHON_ROOT = ROOT / "python"
if str(PYTHON_ROOT) not in sys.path:
    sys.path.insert(0, str(PYTHON_ROOT))

from genoligand_md.phase084 import PHASE084_DIRNAME, path_arg, validate_phase084


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--artifact-dir", type=path_arg, default=ROOT / PHASE084_DIRNAME)
    parser.add_argument("--config", type=path_arg, default=ROOT / "configs/phase_0_8_4_openmm_short_md_strict.yml")
    parser.add_argument("--strict", action="store_true")
    args = parser.parse_args()
    passed, _payload = validate_phase084(args.artifact_dir, strict=args.strict)
    print("phase_0_8_4_openmm_short_md_strict_ok" if passed else "phase_0_8_4_openmm_short_md_strict_fail")
    return 0 if passed else 1


if __name__ == "__main__":
    raise SystemExit(main())
