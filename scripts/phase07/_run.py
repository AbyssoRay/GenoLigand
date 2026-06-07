from __future__ import annotations

import argparse

from _bootstrap import (
    DEFAULT_ANTI_HALLUCINATION_CONFIG,
    DEFAULT_ARTIFACT_DIR,
    DEFAULT_CONTROLS_CONFIG,
    DEFAULT_DOCKING_CONFIG,
    DEFAULT_IFP_CONFIG,
    DEFAULT_LIGAND_INPUT,
    DEFAULT_PHASE05_DIR,
    DEFAULT_PHASE06_DIR,
    env_default,
    path_arg,
)
from genoligand_phase07 import pipeline


def main(command: str) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--artifact-dir", type=path_arg, default=DEFAULT_ARTIFACT_DIR)
    parser.add_argument("--phase06-dir", type=path_arg, default=DEFAULT_PHASE06_DIR)
    parser.add_argument("--phase05-dir", type=path_arg, default=DEFAULT_PHASE05_DIR)
    parser.add_argument("--ligand-input", type=path_arg, default=DEFAULT_LIGAND_INPUT)
    parser.add_argument("--docking-config", type=path_arg, default=DEFAULT_DOCKING_CONFIG)
    parser.add_argument("--controls-config", type=path_arg, default=DEFAULT_CONTROLS_CONFIG)
    parser.add_argument("--ifp-config", type=path_arg, default=DEFAULT_IFP_CONFIG)
    parser.add_argument("--anti-hallucination-config", type=path_arg, default=DEFAULT_ANTI_HALLUCINATION_CONFIG)
    parser.add_argument("--vina-bin", default=env_default("GENOLIGAND_PHASE07_VINA", "vina"))
    parser.add_argument("--ligand-prep-command", default=env_default("GENOLIGAND_PHASE07_LIGAND_PREP", "mk_prepare_ligand.py"))
    parser.add_argument("--receptor-prep-command", default=env_default("GENOLIGAND_PHASE07_RECEPTOR_PREP", "mk_prepare_receptor.py"))
    parser.add_argument("--obabel-bin", default=env_default("GENOLIGAND_PHASE07_OBABEL", "obabel"))
    parser.add_argument("--out", type=path_arg, default=None)
    args = parser.parse_args()

    artifact = args.artifact_dir
    if command == "check_phase06_inputs":
        return pipeline.check_phase06_inputs(args.phase06_dir, args.phase05_dir, args.out or artifact / "metrics/phase_0_3_to_0_6_regression_summary.tsv")
    if command == "validate_ligand_input":
        return pipeline.validate_ligand_input(args.ligand_input, args.out or artifact / "outputs/ligands/ligand_input_manifest.tsv", args.anti_hallucination_config)
    if command == "select_positive_control":
        return pipeline.select_positive_control(artifact, args.docking_config, args.out or artifact / "outputs/controls/positive_control_manifest.tsv")
    if command == "standardize_ligands":
        return pipeline.standardize_ligands(
            artifact,
            artifact / "outputs/ligands/ligand_input_manifest.tsv",
            args.out or artifact / "outputs/ligands/ligand_standardization.tsv",
            artifact / "outputs/ligands/ligand_conformers.sdf",
            artifact / "outputs/ligands/ligand_conformer_manifest.tsv",
            args.docking_config,
        )
    if command == "prepare_ligands_for_docking":
        return pipeline.prepare_ligands_for_docking(artifact, args.ligand_prep_command, args.out or artifact / "outputs/ligands/ligand_preparation_manifest.tsv", args.docking_config)
    if command == "prepare_receptors":
        return pipeline.prepare_receptors(artifact, args.phase05_dir, args.phase06_dir, args.receptor_prep_command, args.obabel_bin, args.out or artifact / "outputs/receptors/receptor_preparation_manifest.tsv", args.docking_config)
    if command == "generate_docking_boxes":
        return pipeline.generate_docking_boxes(artifact, args.phase05_dir, args.phase06_dir, args.out or artifact / "outputs/docking_boxes/docking_box_manifest.tsv", args.docking_config, args.controls_config)
    if command == "run_docking":
        return pipeline.run_docking(
            artifact,
            args.vina_bin,
            args.obabel_bin,
            args.out or artifact / "outputs/docking/docking_runs.tsv",
            artifact / "outputs/docking/docking_scores.tsv",
            artifact / "outputs/docking/docking_pose_manifest.tsv",
            args.docking_config,
        )
    if command == "compute_redocking_rmsd":
        return pipeline.compute_redocking_rmsd(artifact, args.out or artifact / "outputs/controls/redocking_control_results.tsv", args.docking_config)
    if command == "generate_interaction_fingerprints":
        return pipeline.generate_interaction_fingerprints(artifact, args.out or artifact / "outputs/interactions/interaction_fingerprints.tsv", artifact / "outputs/interactions/interaction_summary.tsv", args.ifp_config)
    if command == "calibrate_decoys":
        return pipeline.calibrate_decoys(artifact, args.out or artifact / "outputs/controls/decoy_docking_calibration.tsv", args.controls_config)
    if command == "build_target_prior":
        return pipeline.build_target_prior(artifact, args.out or artifact / "outputs/target_prior/ligand_aware_target_prior.tsv", artifact / "outputs/target_prior/ligand_aware_score_decomposition.tsv", args.docking_config)
    if command == "build_evidence_ledger":
        return pipeline.build_evidence_ledger(artifact, args.out or artifact / "outputs/audit/evidence_ledger.tsv")
    if command == "render_candidate_report":
        return pipeline.render_candidate_report(artifact, args.out or artifact / "outputs/reports/candidate_report_phase_0_7.md")
    if command == "write_external_manifest":
        return pipeline.write_external_manifest(artifact, args.out or artifact / "external_resource_manifest.tsv", args.vina_bin, args.ligand_prep_command, args.receptor_prep_command, args.obabel_bin)
    if command == "write_tool_versions":
        return pipeline.write_tool_versions(args.out or artifact / "metrics/tool_versions.tsv", args.vina_bin, args.ligand_prep_command, args.receptor_prep_command, args.obabel_bin)
    if command == "compute_semantic_hashes":
        return pipeline.semantic_hashes(artifact, args.out or artifact / "outputs/hashes/semantic_hashes.tsv")
    raise SystemExit(f"phase_0_7_unknown_command:{command}")
