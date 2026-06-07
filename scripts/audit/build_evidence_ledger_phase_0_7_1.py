from __future__ import annotations

import argparse
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from _bootstrap import DEFAULT_PHASE07_DIR, DEFAULT_PHASE071_DIR, path_arg
from genoligand_phase07.audit_fix import build_evidence_and_claim_map


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--artifact-dir", type=path_arg, default=DEFAULT_PHASE071_DIR)
    parser.add_argument("--phase07-dir", type=path_arg, default=DEFAULT_PHASE07_DIR)
    args = parser.parse_args()
    raise SystemExit(build_evidence_and_claim_map(args.artifact_dir, args.phase07_dir))
