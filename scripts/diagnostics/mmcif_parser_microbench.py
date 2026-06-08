from __future__ import annotations

import argparse

from _phase082_failure_bootstrap import ROOT
from genoligand_phase082_failure_diagnosis.biolip2_failure import DIAG_DIRNAME, mmcif_parser_microbench, path_arg


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--structure-files", nargs="*", type=path_arg, default=[])
    parser.add_argument("--out", type=path_arg, required=True)
    parser.add_argument("--artifact-dir", type=path_arg, default=ROOT / DIAG_DIRNAME)
    args = parser.parse_args()
    return mmcif_parser_microbench(args.structure_files, args.out, args.artifact_dir)


if __name__ == "__main__":
    raise SystemExit(main())

