from __future__ import annotations

import argparse
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
PYTHON_ROOT = ROOT / "python"
if str(PYTHON_ROOT) not in sys.path:
    sys.path.insert(0, str(PYTHON_ROOT))

from genoligand_phase082_failure_diagnosis.biolip2_failure import DIAG_DIRNAME, path_arg, run_report_dependencies


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--artifact-dir", type=path_arg, default=ROOT / DIAG_DIRNAME)
    args = parser.parse_args()
    return run_report_dependencies(args.artifact_dir)


if __name__ == "__main__":
    raise SystemExit(main())

