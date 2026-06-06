from __future__ import annotations

import argparse

from _bootstrap import DEFAULT_ARTIFACT_DIR, DEFAULT_BIOLIP_URL, ROOT, path_arg
from genoligand_phase06.pipeline import prepare_biolip2_subset


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--url", default=DEFAULT_BIOLIP_URL)
    parser.add_argument("--cache-gz", type=path_arg, default=ROOT / "data/external_phase_0_6/biolip2/BioLiP.txt.gz")
    parser.add_argument("--out", type=path_arg, default=DEFAULT_ARTIFACT_DIR / "outputs/reference_structures/biolip2_interactions.tsv")
    parser.add_argument("--schema-out", type=path_arg, default=DEFAULT_ARTIFACT_DIR / "metrics/biolip2_schema_check.tsv")
    parser.add_argument("--min-rows", type=int, default=200)
    parser.add_argument("--preferred-rows", type=int, default=500)
    args = parser.parse_args()
    prepare_biolip2_subset(args.url, args.cache_gz, args.out, args.min_rows, args.preferred_rows, args.schema_out)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
