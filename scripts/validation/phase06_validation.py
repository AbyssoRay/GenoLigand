from __future__ import annotations

import argparse
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
PYTHON_ROOT = ROOT / "python"
if str(PYTHON_ROOT) not in sys.path:
    sys.path.insert(0, str(PYTHON_ROOT))

from genoligand_phase06.common import read_tsv
from genoligand_phase06.pipeline import PHASE06_CAUTION, REQUIRED_REPORT_SECTIONS, TARGET_PRIOR_CAUTION, check_target_language


DEFAULT_ARTIFACT_DIR = ROOT / "acceptance_artifacts_phase_0_6_structure_patch_strict"


def artifact_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser()
    parser.add_argument("--artifact-dir", default=str(DEFAULT_ARTIFACT_DIR))
    return parser


def require(path: Path) -> None:
    if not path.exists():
        raise SystemExit(f"phase_0_6_missing:{path}")


def rows(path: Path, min_rows: int = 1, label: str | None = None) -> list[dict[str, str]]:
    require(path)
    parsed = read_tsv(path)
    if len(parsed) < min_rows:
        raise SystemExit(f"phase_0_6_insufficient_rows:{label or path}:{len(parsed)}<{min_rows}")
    return parsed


def check_reference_manifests(root: Path, min_references: int = 200, min_patches: int = 500) -> None:
    refs = rows(root / "outputs/reference_structures/reference_structure_manifest.tsv", min_references, "reference_structure_manifest")
    patches = rows(root / "outputs/reference_patches/reference_binding_patches.tsv", min_patches, "reference_binding_patches")
    biolip = rows(root / "outputs/reference_structures/biolip2_interactions.tsv", min_patches, "biolip2_interactions")
    schema = rows(root / "metrics/biolip2_schema_compatibility.tsv", 1, "biolip2_schema")
    summary = rows(root / "metrics/reference_structure_summary.tsv", 1, "reference_structure_summary")
    manifest = rows(root / "external_resource_manifest.tsv", 4, "external_resource_manifest")
    if any(row.get("status") == "FAIL" for row in schema + summary + manifest):
        raise SystemExit("phase_0_6_reference_manifest_status_fail")
    if any(row.get("reference_source", "").lower().startswith("synthetic") for row in refs):
        raise SystemExit("phase_0_6_reference_manifest_contains_synthetic")
    if any(row.get("interaction_source") != "BioLiP2" for row in patches):
        raise SystemExit("phase_0_6_patch_source_not_biolip2")
    ligand_types = {row.get("ligand_type", "") for row in biolip}
    for required in ["small_molecule", "peptide", "metal"]:
        if required not in ligand_types:
            raise SystemExit(f"phase_0_6_biolip2_ligand_type_missing:{required}")
    patch_types = {row.get("ligand_type", "") for row in patches}
    for required in ["small_molecule", "peptide", "metal"]:
        if required not in patch_types:
            raise SystemExit(f"phase_0_6_reference_patch_ligand_type_missing:{required}")


def check_foldseek_outputs(root: Path) -> None:
    hits = rows(root / "outputs/structure_similarity/foldseek_hits.tsv", 1, "foldseek_hits")
    summary = rows(root / "metrics/foldseek_search_summary.tsv", 1, "foldseek_search_summary")
    compat = rows(root / "metrics/foldseek_field_compatibility.tsv", 1, "foldseek_field_compatibility")
    if summary[0].get("status") != "PASS":
        raise SystemExit("phase_0_6_foldseek_summary_not_pass")
    if not any(row.get("foldseek_version") for row in hits):
        raise SystemExit("phase_0_6_foldseek_version_missing_in_hits")
    if not any(row.get("field") == "query" for row in compat):
        raise SystemExit("phase_0_6_foldseek_field_compatibility_incomplete")


def check_usalign_outputs(root: Path) -> None:
    confirmations = rows(root / "outputs/structure_similarity/usalign_confirmations.tsv", 1, "usalign_confirmations")
    summary = rows(root / "metrics/usalign_confirmation_summary.tsv", 1, "usalign_confirmation_summary")
    control_rows = [row for row in confirmations if "control" in row.get("notes", "")]
    if not control_rows:
        raise SystemExit("phase_0_6_usalign_controls_missing")
    if any(not row.get("confirmation_status", "").startswith("success") for row in control_rows):
        raise SystemExit("phase_0_6_usalign_control_failed")
    if summary[0].get("positive_self_control_status") != "success":
        raise SystemExit("phase_0_6_usalign_self_control_not_success")


def check_patch_matching_outputs(root: Path) -> None:
    matches = rows(root / "outputs/patch_matching/candidate_reference_patch_matches.tsv", 1, "candidate_reference_patch_matches")
    summary = rows(root / "metrics/patch_matching_summary.tsv", 1, "patch_matching_summary")
    if summary[0].get("status") != "PASS":
        raise SystemExit("phase_0_6_patch_matching_summary_not_pass")
    if not any(row.get("match_status") == "success" for row in matches):
        raise SystemExit("phase_0_6_patch_matching_success_missing")


def check_decoy_calibration(root: Path, min_decoys: int = 5) -> None:
    decoys = rows(root / "outputs/decoy_calibration/decoy_margin_summary.tsv", 1, "decoy_margin_summary")
    summary = rows(root / "metrics/decoy_calibration_summary.tsv", 1, "decoy_calibration_summary")
    decoy_rows = [row for row in decoys if row.get("is_decoy") == "true"]
    if len(decoy_rows) < min_decoys:
        raise SystemExit(f"phase_0_6_decoy_rows_missing:{len(decoy_rows)}<{min_decoys}")
    if summary[0].get("status") != "PASS":
        raise SystemExit("phase_0_6_decoy_summary_not_pass")


def check_target_prioritization(root: Path) -> None:
    targets = rows(root / "outputs/target_prioritization/target_prior_table.tsv", 1, "target_prior_table")
    scores = rows(root / "outputs/target_prioritization/structure_score_decomposition.tsv", 1, "structure_score_decomposition")
    if len(targets) != len(scores):
        raise SystemExit("phase_0_6_target_score_row_count_mismatch")
    if any(row.get("evidence_level", "") == "" for row in targets):
        raise SystemExit("phase_0_6_target_evidence_level_missing")


def check_candidate_report(root: Path) -> None:
    report = root / "outputs/reports/candidate_report_phase_0_6.md"
    require(report)
    text = report.read_text(encoding="utf-8-sig", errors="replace")
    for section in REQUIRED_REPORT_SECTIONS:
        if section not in text:
            raise SystemExit(f"phase_0_6_candidate_report_missing_section:{section}")
    if PHASE06_CAUTION not in text or TARGET_PRIOR_CAUTION not in text:
        raise SystemExit("phase_0_6_candidate_report_caution_missing")
    check_target_language(report)


def check_nextflow_trace(root: Path) -> None:
    first = root / "metrics/nextflow_trace_first.tsv"
    resume = root / "metrics/nextflow_trace_resume.tsv"
    require(first)
    require(resume)
    text = first.read_text(encoding="utf-8-sig", errors="replace") + "\n" + resume.read_text(encoding="utf-8-sig", errors="replace")
    for process in ["PHASE_0_6_REFERENCE_PREP", "PHASE_0_6_STRUCTURE_SEARCH", "PHASE_0_6_TARGET_REPORT"]:
        if process not in text:
            raise SystemExit(f"phase_0_6_nextflow_trace_missing_process:{process}")


def check_semantic_hash(root: Path) -> None:
    compare = root / "metrics/phase06_semantic_hash_compare.tsv"
    if compare.exists():
        parsed = rows(compare, 1, "phase06_semantic_hash_compare")
        if any(row.get("match") != "true" for row in parsed):
            raise SystemExit("phase_0_6_semantic_hash_compare_mismatch")
        return
    first = rows(root / "outputs/hashes/semantic_hashes.tsv", 1, "semantic_hashes")
    resume = rows(root / "outputs/hashes/semantic_hashes_resume.tsv", 1, "semantic_hashes_resume")
    first_map = {row["artifact"]: row["semantic_hash"] for row in first}
    resume_map = {row["artifact"]: row["semantic_hash"] for row in resume}
    if first_map != resume_map:
        raise SystemExit("phase_0_6_semantic_hash_mismatch")


def check_master(root: Path) -> None:
    required = [
        "metrics/phase05_input_check.json",
        "metrics/command_summary.tsv",
        "metrics/tool_versions.tsv",
        "metrics/phase_0_3_to_0_5_regression_summary.tsv",
        "logs/command_stdout_stderr",
        "outputs/query_structures/query_structure_manifest.tsv",
        "outputs/reports/candidate_report_phase_0_6.md",
        "hashes.tsv",
        "external_resource_manifest.tsv",
    ]
    for rel in required:
        require(root / rel)
    check_reference_manifests(root)
    check_foldseek_outputs(root)
    check_usalign_outputs(root)
    check_patch_matching_outputs(root)
    check_decoy_calibration(root)
    check_target_prioritization(root)
    check_candidate_report(root)
    check_nextflow_trace(root)
    check_semantic_hash(root)
