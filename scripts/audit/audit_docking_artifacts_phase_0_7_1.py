from __future__ import annotations

import argparse
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from _bootstrap import DEFAULT_PHASE07_DIR, DEFAULT_PHASE071_DIR, path_arg
from genoligand_phase07.audit_fix import audit_docking_artifacts, audit_redocking


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--artifact-dir", type=path_arg, default=DEFAULT_PHASE071_DIR)
    parser.add_argument("--phase07-dir", type=path_arg, default=DEFAULT_PHASE07_DIR)
    args = parser.parse_args()
    code = audit_docking_artifacts(args.artifact_dir, args.phase07_dir)
    redock_code = audit_redocking(args.artifact_dir, args.phase07_dir)
    raise SystemExit(0 if code == 0 and redock_code == 0 else 1)
