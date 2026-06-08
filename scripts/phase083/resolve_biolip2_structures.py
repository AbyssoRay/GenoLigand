from __future__ import annotations

import argparse
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
PYTHON_ROOT = ROOT / "python"
if str(PYTHON_ROOT) not in sys.path:
    sys.path.insert(0, str(PYTHON_ROOT))

from genoligand_phase083.biolip2_resolver_patch_strict import PHASE083_DIRNAME, build_all_outputs, path_arg
from genoligand_phase08.full_database_strict import PHASE071_DIRNAME
from genoligand_phase083.biolip2_resolver_patch_strict import PHASE082_DIAG_DIRNAME, PHASE082_DIRNAME


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--artifact-dir", type=path_arg, default=ROOT / PHASE083_DIRNAME)
    parser.add_argument("--phase082-artifact-dir", type=path_arg, default=ROOT / PHASE082_DIRNAME)
    parser.add_argument("--diagnosis-dir", type=path_arg, default=ROOT / PHASE082_DIAG_DIRNAME)
    parser.add_argument("--phase071-dir", type=path_arg, default=ROOT / PHASE071_DIRNAME)
    parser.add_argument("--workers", type=int, default=20)
    args = parser.parse_args()
    return build_all_outputs(args.artifact_dir, args.phase082_artifact_dir, args.diagnosis_dir, args.phase071_dir, workers=args.workers)


if __name__ == "__main__":
    raise SystemExit(main())
