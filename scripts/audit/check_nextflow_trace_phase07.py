from __future__ import annotations

import argparse
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from _bootstrap import DEFAULT_PHASE071_DIR, path_arg
from genoligand_phase07.audit_fix import check_nextflow_trace


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--artifact-dir", type=path_arg, default=DEFAULT_PHASE071_DIR)
    parser.add_argument("--first-trace", type=path_arg, default=DEFAULT_PHASE071_DIR / "outputs/workflow/nextflow_trace_first.tsv")
    parser.add_argument("--resume-trace", type=path_arg, default=DEFAULT_PHASE071_DIR / "outputs/workflow/nextflow_trace_resume.tsv")
    args = parser.parse_args()
    raise SystemExit(check_nextflow_trace(args.artifact_dir, args.first_trace, args.resume_trace))
