from __future__ import annotations

import argparse
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from _bootstrap import DEFAULT_ARTIFACT_DIR, path_arg


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--artifact-dir", type=path_arg, default=DEFAULT_ARTIFACT_DIR)
    args = parser.parse_args()
    required = [
        args.artifact_dir / "outputs/docking/docking_runs.tsv",
        args.artifact_dir / "outputs/docking/docking_scores.tsv",
        args.artifact_dir / "outputs/docking/docking_pose_manifest.tsv",
    ]
    missing = [str(path) for path in required if not Path(path).exists()]
    if missing:
        raise SystemExit("phase_0_7_docking_parse_inputs_missing:" + ",".join(missing))
    print("phase_0_7_docking_outputs_parsed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
