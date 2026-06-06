from __future__ import annotations

import argparse

from _bootstrap import DEFAULT_ARTIFACT_DIR, DEFAULT_PHASE05_DIR, ROOT, path_arg
from genoligand_phase06.pipeline import score_target_priors


def latest_sequence_evidence(phase05_dir):
    candidates = sorted(
        phase05_dir.glob("tmp/*phase_0_4_sequence_layer_exports_features_scores_and_report_controls/sequence_evidence.tsv"),
        key=lambda path: path.stat().st_mtime if path.exists() else 0,
        reverse=True,
    )
    if candidates:
        return candidates[0]
    fallback = phase05_dir / "outputs/sequence_evidence/sequence_evidence.tsv"
    return fallback


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--foldseek-hits", type=path_arg, default=DEFAULT_ARTIFACT_DIR / "outputs/structure_similarity/foldseek_hits.tsv")
    parser.add_argument("--usalign-tsv", type=path_arg, default=DEFAULT_ARTIFACT_DIR / "outputs/structure_similarity/usalign_confirmations.tsv")
    parser.add_argument("--patch-matches", type=path_arg, default=DEFAULT_ARTIFACT_DIR / "outputs/patch_matching/candidate_reference_patch_matches.tsv")
    parser.add_argument("--decoy-summary", type=path_arg, default=DEFAULT_ARTIFACT_DIR / "outputs/decoy_calibration/decoy_margin_summary.tsv")
    parser.add_argument("--sequence-evidence", type=path_arg)
    parser.add_argument("--structure-quality", type=path_arg, default=DEFAULT_PHASE05_DIR / "outputs/structure_features/structure_quality.tsv")
    parser.add_argument("--out-target", type=path_arg, default=DEFAULT_ARTIFACT_DIR / "outputs/target_prioritization/target_prior_table.tsv")
    parser.add_argument("--out-score", type=path_arg, default=DEFAULT_ARTIFACT_DIR / "outputs/target_prioritization/structure_score_decomposition.tsv")
    parser.add_argument("--score-config", type=path_arg, default=ROOT / "configs/phase_0_6_scoring_weights.yml")
    args = parser.parse_args()
    sequence_evidence = args.sequence_evidence or latest_sequence_evidence(DEFAULT_PHASE05_DIR)
    score_target_priors(
        args.foldseek_hits,
        args.usalign_tsv,
        args.patch_matches,
        args.decoy_summary,
        sequence_evidence,
        args.structure_quality,
        args.out_target,
        args.out_score,
        args.score_config,
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
