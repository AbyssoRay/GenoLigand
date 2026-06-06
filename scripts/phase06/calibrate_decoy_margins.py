from __future__ import annotations

import argparse

from _bootstrap import DEFAULT_ARTIFACT_DIR, path_arg
from genoligand_phase06.pipeline import calibrate_decoy_margins


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--foldseek-hits", type=path_arg, default=DEFAULT_ARTIFACT_DIR / "outputs/structure_similarity/foldseek_hits.tsv")
    parser.add_argument("--usalign-tsv", type=path_arg, default=DEFAULT_ARTIFACT_DIR / "outputs/structure_similarity/usalign_confirmations.tsv")
    parser.add_argument("--patch-matches", type=path_arg, default=DEFAULT_ARTIFACT_DIR / "outputs/patch_matching/candidate_reference_patch_matches.tsv")
    parser.add_argument("--out", type=path_arg, default=DEFAULT_ARTIFACT_DIR / "outputs/decoy_calibration/decoy_margin_summary.tsv")
    parser.add_argument("--summary-out", type=path_arg, default=DEFAULT_ARTIFACT_DIR / "metrics/decoy_calibration_summary.tsv")
    args = parser.parse_args()
    calibrate_decoy_margins(args.foldseek_hits, args.usalign_tsv, args.patch_matches, args.out, args.summary_out)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
