from __future__ import annotations

import argparse
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
PYTHON_ROOT = ROOT / "python"
if str(PYTHON_ROOT) not in sys.path:
    sys.path.insert(0, str(PYTHON_ROOT))

from genoligand_phase06.common import read_tsv
from genoligand_phase081.database_audit_fix_strict import PHASE081_DIRNAME, path_arg


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--artifact-dir", type=path_arg, default=ROOT / PHASE081_DIRNAME)
    parser.add_argument("--strict", action="store_true")
    args = parser.parse_args()
    path = args.artifact_dir / "outputs/benchmark/openbindbench_reconstruction_audit.tsv"
    rows = read_tsv(path) if path.exists() else []
    passed = bool(rows) and all(row.get("status") == "PASS" and row.get("pdbbind_only") == "False" and row.get("raw_lineage_present") == "True" for row in rows)
    print("phase_0_8_1_openbindbench_reconstruction_audit_ok" if passed else "phase_0_8_1_openbindbench_reconstruction_audit_fail")
    raise SystemExit(0 if passed or not args.strict else 1)

