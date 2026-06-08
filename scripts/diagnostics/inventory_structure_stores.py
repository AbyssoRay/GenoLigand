from __future__ import annotations

import argparse

from _phase082_failure_bootstrap import ROOT
from genoligand_phase082_failure_diagnosis.biolip2_failure import inventory_structure_stores, path_arg


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--repo-root", type=path_arg, default=ROOT)
    parser.add_argument("--out", type=path_arg, required=True)
    args = parser.parse_args()
    return inventory_structure_stores(args.repo_root, args.out)


if __name__ == "__main__":
    raise SystemExit(main())

