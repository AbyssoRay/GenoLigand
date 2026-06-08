from __future__ import annotations

import argparse

from _phase082_failure_bootstrap import ROOT
from genoligand_phase082_failure_diagnosis.biolip2_failure import DIAG_DIRNAME, audit_biolip2_structure_join, path_arg


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--biolip2-table", type=path_arg, required=True)
    parser.add_argument("--structure-stores", nargs="*", default=[])
    parser.add_argument("--out", type=path_arg, required=True)
    parser.add_argument("--artifact-dir", type=path_arg, default=ROOT / DIAG_DIRNAME)
    args = parser.parse_args()
    return audit_biolip2_structure_join(args.biolip2_table, args.out, args.artifact_dir)


if __name__ == "__main__":
    raise SystemExit(main())

