from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
PYTHON_ROOT = ROOT / "python"
if str(PYTHON_ROOT) not in sys.path:
    sys.path.insert(0, str(PYTHON_ROOT))

from genoligand_phase06.common import read_tsv, sha256_file, write_tsv
from genoligand_phase07.pipeline import (
    DECOY_CALIBRATION_FIELDS,
    DOCKING_BOX_FIELDS,
    DOCKING_RUN_FIELDS,
    IFP_FIELDS,
    LEDGER_FIELDS,
    LIGAND_INPUT_FIELDS,
    LIGAND_STANDARDIZATION_FIELDS,
    PLACEHOLDER_PATTERNS,
    PROHIBITED_REPORT_TERMS,
    RECEPTOR_PREP_FIELDS,
    REDOCKING_FIELDS,
)


DEFAULT_ARTIFACT_DIR = ROOT / "acceptance_artifacts_phase_0_7_anti_hallucination_strict"


def artifact_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser()
    parser.add_argument("--artifact-dir", default=str(DEFAULT_ARTIFACT_DIR))
    return parser


def require(path: Path) -> None:
    if not path.exists():
        raise SystemExit(f"phase_0_7_missing:{path}")


def rows(path: Path, min_rows: int = 1, label: str | None = None) -> list[dict[str, str]]:
    require(path)
    parsed = read_tsv(path)
    if len(parsed) < min_rows:
        raise SystemExit(f"phase_0_7_insufficient_rows:{label or path}:{len(parsed)}<{min_rows}")
    return parsed


def require_columns(parsed: list[dict[str, str]], columns: list[str], label: str) -> None:
    if not parsed:
        raise SystemExit(f"phase_0_7_empty_table:{label}")
    missing = [column for column in columns if column not in parsed[0]]
    if missing:
        raise SystemExit(f"phase_0_7_missing_columns:{label}:{','.join(missing)}")


def require_hash(path_text: str, expected: str, label: str) -> None:
    path = Path(path_text)
    if expected in {"missing", "validation_failed", "parse_failed", "not_run", "tool_incompatible"}:
        raise SystemExit(f"phase_0_7_hash_not_valid:{label}")
    if not path.exists() or not path.is_file() or path.stat().st_size <= 0:
        raise SystemExit(f"phase_0_7_referenced_file_missing_or_empty:{label}:{path}")
    actual = sha256_file(path)
    if actual != expected:
        raise SystemExit(f"phase_0_7_hash_mismatch:{label}:{path}")


def check_no_manual_edits(parsed: list[dict[str, str]], label: str) -> None:
    for row in parsed:
        if row.get("manual_edit_allowed") != "false":
            raise SystemExit(f"phase_0_7_manual_edit_forbidden:{label}:{row.get('row_id', '')}")


def check_ligand_input(root: Path) -> None:
    parsed = rows(root / "outputs/ligands/ligand_input_manifest.tsv", 2, "ligand_input_manifest")
    require_columns(parsed, LIGAND_INPUT_FIELDS, "ligand_input_manifest")
    check_no_manual_edits(parsed, "ligand_input_manifest")
    roles = {row.get("intended_role") for row in parsed}
    if "positive_control_ligand" not in roles or "negative_control_ligand" not in roles:
        raise SystemExit("phase_0_7_control_ligands_missing")
    if any(row.get("production_target_prior_allowed") != "false" for row in parsed):
        raise SystemExit("phase_0_7_control_only_target_prior_flag_invalid")


def check_ligand_standardization(root: Path) -> None:
    std = rows(root / "outputs/ligands/ligand_standardization.tsv", 2, "ligand_standardization")
    require_columns(std, LIGAND_STANDARDIZATION_FIELDS, "ligand_standardization")
    check_no_manual_edits(std, "ligand_standardization")
    if any(row.get("standardization_status") != "PASS" for row in std):
        raise SystemExit("phase_0_7_ligand_standardization_not_pass")
    conf = rows(root / "outputs/ligands/ligand_conformer_manifest.tsv", 2, "ligand_conformer_manifest")
    if not any(row.get("selected_for_docking") == "true" for row in conf):
        raise SystemExit("phase_0_7_no_selected_ligand_conformer")
    for row in conf:
        if row.get("selected_for_docking") == "true":
            require_hash(row.get("sdf_path", ""), row.get("sdf_sha256", ""), f"conformer:{row.get('ligand_id')}")


def check_receptor_preparation(root: Path) -> None:
    parsed = rows(root / "outputs/receptors/receptor_preparation_manifest.tsv", 9, "receptor_preparation")
    require_columns(parsed, RECEPTOR_PREP_FIELDS, "receptor_preparation")
    check_no_manual_edits(parsed, "receptor_preparation")
    if any(row.get("preparation_status") != "PASS" for row in parsed):
        raise SystemExit("phase_0_7_receptor_preparation_not_pass")
    for row in parsed:
        require_hash(row.get("prepared_receptor_path", ""), row.get("prepared_receptor_sha256", ""), f"receptor:{row.get('candidate_id')}")


def check_docking_boxes(root: Path) -> None:
    parsed = rows(root / "outputs/docking_boxes/docking_box_manifest.tsv", 10, "docking_box_manifest")
    require_columns(parsed, DOCKING_BOX_FIELDS, "docking_box_manifest")
    check_no_manual_edits(parsed, "docking_box_manifest")
    if not any(row.get("box_id") == "box_positive_control_native" for row in parsed):
        raise SystemExit("phase_0_7_positive_control_box_missing")
    if sum(1 for row in parsed if row.get("is_decoy") == "true") < 7:
        raise SystemExit("phase_0_7_decoy_or_random_boxes_missing")
    for row in parsed:
        for key in ["center_x", "center_y", "center_z", "size_x", "size_y", "size_z"]:
            try:
                value = float(row[key])
            except ValueError as exc:
                raise SystemExit(f"phase_0_7_box_numeric_parse_failed:{row.get('box_id')}:{key}") from exc
            if key.startswith("size") and (value <= 0 or value > 40):
                raise SystemExit(f"phase_0_7_box_size_out_of_range:{row.get('box_id')}:{key}:{value}")


def check_real_docking_runs(root: Path) -> None:
    runs = rows(root / "outputs/docking/docking_runs.tsv", 15, "docking_runs")
    require_columns(runs, DOCKING_RUN_FIELDS, "docking_runs")
    check_no_manual_edits(runs, "docking_runs")
    if any(row.get("docking_backend_version") in {"", "missing"} for row in runs):
        raise SystemExit("phase_0_7_docking_backend_version_missing")
    if any(row.get("exit_code") != "0" or row.get("run_status") != "PASS" for row in runs):
        raise SystemExit("phase_0_7_docking_run_failed")
    for row in runs:
        require_hash(row.get("input_ligand_path", ""), row.get("input_ligand_sha256", ""), f"input_ligand:{row.get('run_id')}")
        require_hash(row.get("input_receptor_path", ""), row.get("input_receptor_sha256", ""), f"input_receptor:{row.get('run_id')}")
    scores = rows(root / "outputs/docking/docking_scores.tsv", 15, "docking_scores")
    if any(row.get("score_parse_status") != "PASS" for row in scores):
        raise SystemExit("phase_0_7_docking_score_parse_failed")
    poses = rows(root / "outputs/docking/docking_pose_manifest.tsv", 15, "docking_pose_manifest")
    if any(row.get("pose_status") != "PASS" for row in poses):
        raise SystemExit("phase_0_7_pose_manifest_not_pass")


def check_redocking_controls(root: Path) -> None:
    parsed = rows(root / "outputs/controls/redocking_control_results.tsv", 1, "redocking_control_results")
    require_columns(parsed, REDOCKING_FIELDS, "redocking_control_results")
    check_no_manual_edits(parsed, "redocking_control_results")
    if not any(row.get("control_pass") == "true" for row in parsed):
        raise SystemExit("phase_0_7_redocking_control_not_pass")
    for row in parsed:
        if row.get("control_pass") == "true":
            float(row["heavy_atom_rmsd_angstrom"])


def check_interaction_fingerprints(root: Path) -> None:
    parsed = rows(root / "outputs/interactions/interaction_fingerprints.tsv", 15, "interaction_fingerprints")
    require_columns(parsed, IFP_FIELDS, "interaction_fingerprints")
    check_no_manual_edits(parsed, "interaction_fingerprints")
    if any(row.get("interaction_parse_status") != "PASS" for row in parsed):
        raise SystemExit("phase_0_7_interaction_fingerprint_parse_failed")
    summary = rows(root / "outputs/interactions/interaction_summary.tsv", 1, "interaction_summary")
    if summary[0].get("status") != "PASS":
        raise SystemExit("phase_0_7_interaction_summary_not_pass")


def check_decoy_calibration(root: Path) -> None:
    parsed = rows(root / "outputs/controls/decoy_docking_calibration.tsv", 1, "decoy_docking_calibration")
    require_columns(parsed, DECOY_CALIBRATION_FIELDS, "decoy_docking_calibration")
    check_no_manual_edits(parsed, "decoy_docking_calibration")
    if parsed[0].get("calibration_status") != "PASS":
        raise SystemExit("phase_0_7_decoy_calibration_not_pass")


def check_evidence_ledger(root: Path) -> None:
    parsed = rows(root / "outputs/audit/evidence_ledger.tsv", 10, "evidence_ledger")
    require_columns(parsed, LEDGER_FIELDS, "evidence_ledger")
    if any(row.get("validation_status") != "PASS" for row in parsed):
        raise SystemExit("phase_0_7_evidence_ledger_contains_failed_support")
    for row in parsed:
        path = Path(row.get("supporting_artifact_path", ""))
        if not path.exists():
            raise SystemExit(f"phase_0_7_ledger_artifact_missing:{row.get('ledger_id')}")
        if path.is_file() and sha256_file(path) != row.get("supporting_artifact_sha256"):
            raise SystemExit(f"phase_0_7_ledger_hash_mismatch:{row.get('ledger_id')}")


def check_report_claims(root: Path) -> None:
    report_paths = [
        root / "outputs/reports/candidate_report_phase_0_7.md",
        root / "acceptance_report_phase_0_7.md",
    ]
    lint_rows = []
    for path in report_paths:
        if path.name == "candidate_report_phase_0_7.md":
            require(path)
        elif not path.exists():
            continue
        in_limitation = False
        for line_no, line in enumerate(path.read_text(encoding="utf-8-sig", errors="replace").splitlines(), start=1):
            lower = line.lower()
            if lower.startswith("## "):
                in_limitation = "limitation" in lower or "known limitations" in lower
            if lower.startswith("#"):
                continue
            for term in PROHIBITED_REPORT_TERMS:
                if term in lower and not in_limitation:
                    lint_rows.append({"path": str(path), "line": line_no, "term": term, "status": "FAIL", "notes": "forbidden_term_outside_limitation"})
            controlled_hit = any(term in lower for term in ["target-prior", "interaction", "hydrogen bond", "salt bridge", "docking score", "binding patch", "pocket", "redocking", "rmsd"])
            if controlled_hit and "ledger_" not in lower and not path.name.startswith("acceptance_report"):
                lint_rows.append({"path": str(path), "line": line_no, "term": "controlled_term", "status": "FAIL", "notes": "controlled_term_without_ledger_reference"})
    if not lint_rows:
        lint_rows.append({"path": "reports", "line": "not_applicable", "term": "not_applicable", "status": "PASS", "notes": "report_claim_lint_pass"})
    write_tsv(root / "metrics/report_claim_lint.tsv", lint_rows, ["path", "line", "term", "status", "notes"])
    if any(row["status"] != "PASS" for row in lint_rows):
        raise SystemExit("phase_0_7_report_claim_lint_failed")


def check_no_placeholder_artifacts(root: Path) -> None:
    rows_out = []
    for path in sorted(root.rglob("*")):
        if not path.is_file():
            continue
        rel = path.relative_to(root).as_posix()
        if rel.startswith("logs/command_stdout_stderr/"):
            continue
        if rel == "metrics/no_placeholder_artifacts.tsv":
            continue
        if rel == "hashes.tsv":
            continue
        text = path.read_text(encoding="utf-8-sig", errors="replace") if path.suffix.lower() in {".tsv", ".md", ".txt", ".log", ".json"} else ""
        for pattern in PLACEHOLDER_PATTERNS:
            lower_text = text.lower()
            if pattern in {"TODO", "TBD"}:
                matched = re.search(rf"(?<![A-Za-z]){re.escape(pattern.lower())}(?![A-Za-z])", lower_text) is not None
            else:
                matched = pattern.lower() in lower_text
            if pattern == "placeholder" and rel == "acceptance_report_phase_0_7.md" and "placeholder artifacts:" in lower_text:
                matched = False
            if matched:
                rows_out.append({"path": rel, "pattern": pattern, "status": "FAIL"})
    if not rows_out:
        rows_out.append({"path": "artifact_dir", "pattern": "not_applicable", "status": "PASS"})
    write_tsv(root / "metrics/no_placeholder_artifacts.tsv", rows_out, ["path", "pattern", "status"])
    if any(row["status"] != "PASS" for row in rows_out):
        raise SystemExit("phase_0_7_placeholder_artifact_lint_failed")


def check_nextflow_repro(root: Path) -> None:
    first = root / "metrics/nextflow_trace_first.tsv"
    resume = root / "metrics/nextflow_trace_resume.tsv"
    require(first)
    require(resume)
    text = first.read_text(encoding="utf-8-sig", errors="replace") + "\n" + resume.read_text(encoding="utf-8-sig", errors="replace")
    for process in [
        "REGRESSION_PHASE03_TO_06",
        "VALIDATE_LIGAND_INPUT",
        "STANDARDIZE_LIGAND",
        "GENERATE_CONFORMERS",
        "PREPARE_LIGAND_FOR_DOCKING",
        "PREPARE_RECEPTORS",
        "GENERATE_DOCKING_BOXES",
        "RUN_POSITIVE_CONTROL_REDOCKING",
        "RUN_QUERY_DOCKING",
        "RUN_NEGATIVE_CONTROL_DOCKING",
        "PARSE_DOCKING_OUTPUTS",
        "COMPUTE_REDOCKING_RMSD",
        "GENERATE_INTERACTION_FINGERPRINTS",
        "CALIBRATE_DECOYS",
        "BUILD_EVIDENCE_LEDGER",
        "RENDER_PHASE07_REPORT",
        "ANTI_HALLUCINATION_LINT",
        "COLLECT_ACCEPTANCE_ARCHIVE",
    ]:
        if process not in text:
            raise SystemExit(f"phase_0_7_nextflow_trace_missing_process:{process}")
    compare = rows(root / "metrics/phase07_semantic_hash_compare.tsv", 1, "phase07_semantic_hash_compare")
    if any(row.get("match") != "true" for row in compare):
        raise SystemExit("phase_0_7_semantic_hash_mismatch")


def check_master(root: Path) -> None:
    for rel in [
        "metrics/command_summary.tsv",
        "metrics/tool_versions.tsv",
        "metrics/phase_0_3_to_0_6_regression_summary.tsv",
        "external_resource_manifest.tsv",
        "hashes.tsv",
        "outputs/reports/candidate_report_phase_0_7.md",
        "acceptance_report_phase_0_7.md",
    ]:
        require(root / rel)
    check_ligand_input(root)
    check_ligand_standardization(root)
    check_receptor_preparation(root)
    check_docking_boxes(root)
    check_real_docking_runs(root)
    check_redocking_controls(root)
    check_interaction_fingerprints(root)
    check_decoy_calibration(root)
    check_evidence_ledger(root)
    check_report_claims(root)
    check_no_placeholder_artifacts(root)
    check_nextflow_repro(root)
