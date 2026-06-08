from __future__ import annotations

import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
PYTHON_ROOT = ROOT / "python"
if str(PYTHON_ROOT) not in sys.path:
    sys.path.insert(0, str(PYTHON_ROOT))

from genoligand_phase082.mapping_benchmark_storage_strict import run_acceptance


if __name__ == "__main__":
    raise SystemExit(run_acceptance())

