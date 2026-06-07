from __future__ import annotations

import argparse
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from _bootstrap import DEFAULT_PHASE07_DIR, DEFAULT_PHASE071_DIR, path_arg
from genoligand_phase07.audit_fix import workflow_step


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--artifact-dir", type=path_arg, default=DEFAULT_PHASE071_DIR)
    parser.add_argument("--phase07-dir", type=path_arg, default=DEFAULT_PHASE07_DIR)
    parser.add_argument("--stage", required=True)
    args = parser.parse_args()
    raise SystemExit(workflow_step(args.artifact_dir, args.phase07_dir, args.stage))
