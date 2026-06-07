from __future__ import annotations

import argparse
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from _bootstrap import DEFAULT_PHASE071_DIR, path_arg
from genoligand_phase07.audit_fix import compare_semantic_hashes


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--artifact-dir", type=path_arg, default=DEFAULT_PHASE071_DIR)
    parser.add_argument("--first", type=path_arg, default=DEFAULT_PHASE071_DIR / "outputs/workflow/phase07_semantic_hash_first.tsv")
    parser.add_argument("--resume", type=path_arg, default=DEFAULT_PHASE071_DIR / "outputs/workflow/phase07_semantic_hash_resume.tsv")
    parser.add_argument("--out", type=path_arg, default=DEFAULT_PHASE071_DIR / "outputs/workflow/phase07_semantic_hash_compare.tsv")
    args = parser.parse_args()
    raise SystemExit(compare_semantic_hashes(args.first, args.resume, args.out))
