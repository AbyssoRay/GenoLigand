from __future__ import annotations

import argparse

from _bootstrap import DEFAULT_ARTIFACT_DIR, env_or_default, path_arg
from genoligand_phase06.pipeline import run_usalign_confirmation


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--usalign-bin", type=path_arg, default=env_or_default("USALIGN_BIN", "USalign"))
    parser.add_argument("--foldseek-hits", type=path_arg, default=DEFAULT_ARTIFACT_DIR / "outputs/structure_similarity/foldseek_hits.tsv")
    parser.add_argument("--query-manifest", type=path_arg, default=DEFAULT_ARTIFACT_DIR / "outputs/query_structures/query_structure_manifest.tsv")
    parser.add_argument("--reference-manifest", type=path_arg, default=DEFAULT_ARTIFACT_DIR / "outputs/reference_structures/reference_structure_manifest.tsv")
    parser.add_argument("--out", type=path_arg, default=DEFAULT_ARTIFACT_DIR / "outputs/structure_similarity/usalign_confirmations.tsv")
    parser.add_argument("--raw-dir", type=path_arg, default=DEFAULT_ARTIFACT_DIR / "logs/usalign/raw_outputs")
    parser.add_argument("--summary-out", type=path_arg, default=DEFAULT_ARTIFACT_DIR / "metrics/usalign_confirmation_summary.tsv")
    parser.add_argument("--top-n", type=int, default=3)
    parser.add_argument("--min-self-tm", type=float, default=0.95)
    parser.add_argument("--min-positive-tm", type=float, default=0.95)
    parser.add_argument("--max-negative-tm", type=float, default=0.95)
    args = parser.parse_args()
    run_usalign_confirmation(
        args.usalign_bin,
        args.foldseek_hits,
        args.query_manifest,
        args.reference_manifest,
        args.out,
        args.raw_dir,
        args.summary_out,
        args.top_n,
        args.min_self_tm,
        args.min_positive_tm,
        args.max_negative_tm,
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
