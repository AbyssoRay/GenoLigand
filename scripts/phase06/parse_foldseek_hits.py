from __future__ import annotations

import argparse

from _bootstrap import DEFAULT_ARTIFACT_DIR, path_arg
from genoligand_phase06.common import read_tsv, write_tsv
from genoligand_phase06.pipeline import FOLDSEEK_HIT_FIELDS


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--input", type=path_arg, default=DEFAULT_ARTIFACT_DIR / "outputs/structure_similarity/foldseek_hits.tsv")
    parser.add_argument("--out", type=path_arg, default=DEFAULT_ARTIFACT_DIR / "outputs/structure_similarity/foldseek_hits.parsed.tsv")
    parser.add_argument("--summary-out", type=path_arg, default=DEFAULT_ARTIFACT_DIR / "metrics/foldseek_parse_summary.tsv")
    args = parser.parse_args()
    rows = read_tsv(args.input)
    if not rows:
        raise SystemExit("foldseek_parse_empty")
    write_tsv(args.out, rows, FOLDSEEK_HIT_FIELDS)
    write_tsv(args.summary_out, [{"input_rows": len(rows), "output_rows": len(rows), "status": "PASS"}], ["input_rows", "output_rows", "status"])
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
