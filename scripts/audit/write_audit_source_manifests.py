from __future__ import annotations

import argparse
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from _bootstrap import DEFAULT_PHASE071_DIR, path_arg
from genoligand_phase07.audit_fix import write_source_manifests


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--artifact-dir", type=path_arg, default=DEFAULT_PHASE071_DIR)
    args = parser.parse_args()
    raise SystemExit(write_source_manifests(args.artifact_dir))
