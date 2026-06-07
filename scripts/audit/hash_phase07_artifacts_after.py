from __future__ import annotations

import argparse
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from _bootstrap import DEFAULT_PHASE07_DIR, DEFAULT_PHASE071_DIR, path_arg
from genoligand_phase07.audit_fix import write_phase07_after_hashes


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--artifact-dir", type=path_arg, default=DEFAULT_PHASE071_DIR)
    parser.add_argument("--phase07-dir", type=path_arg, default=DEFAULT_PHASE07_DIR)
    args = parser.parse_args()
    raise SystemExit(write_phase07_after_hashes(args.artifact_dir, args.phase07_dir))
