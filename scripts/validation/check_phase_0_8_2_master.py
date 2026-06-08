from __future__ import annotations

import argparse
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
PYTHON_ROOT = ROOT / "python"
if str(PYTHON_ROOT) not in sys.path:
    sys.path.insert(0, str(PYTHON_ROOT))

from genoligand_phase082.mapping_benchmark_storage_strict import PHASE081_DIRNAME, PHASE082_DIRNAME, path_arg, validate_phase082


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--artifact-dir", type=path_arg, default=ROOT / PHASE082_DIRNAME)
    parser.add_argument("--previous-artifact-dir", type=path_arg, default=ROOT / PHASE081_DIRNAME)
    parser.add_argument("--directory-structure", type=path_arg, default=ROOT / "genoligand_project_directory_structure_grouped.md")
    parser.add_argument("--strict", action="store_true")
    args = parser.parse_args()
    passed, _ = validate_phase082(args.artifact_dir, args.previous_artifact_dir)
    print("phase_0_8_2_mapping_benchmark_storage_strict_ok" if passed else "phase_0_8_2_mapping_benchmark_storage_strict_fail")
    raise SystemExit(0 if passed or not args.strict else 1)

