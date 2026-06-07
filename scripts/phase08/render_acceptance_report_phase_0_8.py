from __future__ import annotations

import argparse
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
PYTHON_ROOT = ROOT / "python"
if str(PYTHON_ROOT) not in sys.path:
    sys.path.insert(0, str(PYTHON_ROOT))

from genoligand_phase08.full_database_strict import PHASE08_DIRNAME, path_arg, render_acceptance_report


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--artifact-dir", type=path_arg, default=ROOT / PHASE08_DIRNAME)
    args = parser.parse_args()
    raise SystemExit(render_acceptance_report(args.artifact_dir))

