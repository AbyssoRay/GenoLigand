from __future__ import annotations

import argparse

from _bootstrap import DEFAULT_ARTIFACT_DIR, DEFAULT_PHASE05_DIR, path_arg
from genoligand_phase06.pipeline import match_candidate_patches


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--candidate-patches", type=path_arg, default=DEFAULT_PHASE05_DIR / "outputs/patches/patch_descriptors.tsv")
    parser.add_argument("--reference-patches", type=path_arg, default=DEFAULT_ARTIFACT_DIR / "outputs/reference_patches/reference_binding_patches.tsv")
    parser.add_argument("--out", type=path_arg, default=DEFAULT_ARTIFACT_DIR / "outputs/patch_matching/candidate_reference_patch_matches.tsv")
    parser.add_argument("--summary-out", type=path_arg, default=DEFAULT_ARTIFACT_DIR / "metrics/patch_matching_summary.tsv")
    parser.add_argument("--top-n", type=int, default=5)
    args = parser.parse_args()
    match_candidate_patches(args.candidate_patches, args.reference_patches, args.out, args.summary_out, args.top_n)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
