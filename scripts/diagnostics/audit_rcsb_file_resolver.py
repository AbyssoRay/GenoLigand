from __future__ import annotations

import argparse

from _phase082_failure_bootstrap import ROOT
from genoligand_phase082_failure_diagnosis.biolip2_failure import audit_rcsb_file_resolver, path_arg


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--biolip2-table", type=path_arg, required=True)
    parser.add_argument("--rcsb-manifest", type=path_arg, required=True)
    parser.add_argument("--structure-root", type=path_arg, required=True)
    parser.add_argument("--out", type=path_arg, required=True)
    args = parser.parse_args()
    return audit_rcsb_file_resolver(args.biolip2_table, args.rcsb_manifest, args.structure_root, args.out)


if __name__ == "__main__":
    raise SystemExit(main())
