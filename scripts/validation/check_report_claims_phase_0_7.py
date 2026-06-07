from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from phase07_validation import artifact_parser, check_report_claims


if __name__ == "__main__":
    args = artifact_parser().parse_args()
    check_report_claims(Path(args.artifact_dir))
    print("phase_0_7_report_claims_ok")
