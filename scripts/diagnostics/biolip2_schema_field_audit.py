from __future__ import annotations

import argparse

from _phase082_failure_bootstrap import ROOT
from genoligand_phase082_failure_diagnosis.biolip2_failure import audit_biolip2_schema_fields, path_arg


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--biolip2-table", type=path_arg, required=True)
    parser.add_argument("--out", type=path_arg, required=True)
    args = parser.parse_args()
    return audit_biolip2_schema_fields(args.biolip2_table, args.out)


if __name__ == "__main__":
    raise SystemExit(main())
