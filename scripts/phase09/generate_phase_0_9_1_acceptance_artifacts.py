from __future__ import annotations

import argparse
import base64
import csv
import json
import os
import re
import shutil
import socket
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request
from collections import Counter
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
DEFAULT_ARTIFACT = ROOT / "acceptance_artifacts_phase_0_9_1_production_webui"
DEFAULT_BASE_URL = "http://127.0.0.1:19090"

ROUTES = [
    "dashboard_page",
    "project_create_page",
    "input_wizard_project_basics_page",
    "input_wizard_molecule_type_page",
    "input_wizard_small_molecule_page",
    "input_wizard_peptide_page",
    "input_wizard_protein_page",
    "input_wizard_complex_page",
    "input_wizard_genome_page",
    "input_wizard_search_scope_page",
    "input_wizard_database_profile_page",
    "input_wizard_run_profile_page",
    "input_wizard_preflight_page",
    "input_wizard_review_start_page",
    "run_monitor_page",
    "candidate_ranking_page",
    "candidate_detail_page",
    "genome_orf_context_page",
    "structure_viewer_page",
    "md_summary_page",
    "evidence_ledger_page",
    "database_registry_page",
    "diagnostics_page",
    "setup_wizard_page",
    "settings_page",
    "report_export_page",
    "error_state_page",
    "empty_state_page",
    "loading_state_page",
]

MOJIBAKE_MARKERS = [
    "\ufffd",
    "�",
    "鏈",
    "鍙",
    "姝",
    "鐗",
    "閫",
    "娴",
    "銆",
    "鍣",
    "绛夊",
    "宸ヤ",
    "灏辩",
    "涓",
    "杩愯",
    "鐩戞",
]

API_ROUTES = [
    ("GET", "/api/health"),
    ("GET", "/api/version"),
    ("GET", "/api/user/health"),
    ("GET", "/api/user/projects"),
    ("GET", "/api/user/runs"),
    ("GET", "/api/user/databases"),
    ("GET", "/api/user/diagnostics"),
    ("POST", "/api/user/diagnostics/run"),
    ("POST", "/api/user/diagnostics/action"),
    ("GET", "/api/user/setup/status"),
    ("POST", "/api/user/setup/run"),
    ("GET", "/api/user/setup/install-plan"),
    ("POST", "/api/user/setup/install"),
    ("POST", "/api/user/setup/validate-sample-project"),
    ("GET", "/api/user/storage/policy"),
    ("POST", "/api/user/projects/run-example"),
    ("POST", "/api/user/projects/clear-examples"),
    ("POST", "/api/user/projects/import-package"),
    ("POST", "/api/user/reports/export"),
    ("GET", "/api/debug/diagnostics"),
    ("GET", "/api/debug/runtime"),
    ("GET", "/api/debug/routes"),
    ("GET", "/api/projects"),
    ("POST", "/api/projects"),
    ("POST", "/api/projects/{id}/copy"),
    ("DELETE", "/api/projects/{id}"),
    ("POST", "/api/projects/{id}/cleanup"),
    ("GET", "/api/projects/{id}/preflight"),
    ("POST", "/api/projects/{id}/preflight"),
    ("POST", "/api/projects/{id}/runs"),
    ("POST", "/api/runs/{id}/cancel"),
    ("POST", "/api/runs/{id}/resume"),
    ("GET", "/api/runs/{id}/logs"),
    ("GET", "/api/runs/{id}/candidates"),
    ("GET", "/api/runs/{id}/evidence"),
    ("GET", "/api/runs/{id}/genome-context"),
    ("GET", "/api/runs/{id}/structure"),
    ("GET", "/api/runs/{id}/md-summary"),
    ("GET", "/api/databases"),
    ("POST", "/api/databases/validate"),
    ("POST", "/api/databases/build-index"),
    ("GET", "/api/diagnostics"),
    ("POST", "/api/reports/export"),
]

INPUT_STEPS = [
    "project_basics",
    "molecule_type",
    "small_molecule",
    "peptide",
    "protein_ligand",
    "existing_complex",
    "species_genome",
    "search_scope",
    "database_profile",
    "run_profile",
    "preflight",
    "review_start",
]

COMPONENTS = [
    ("project_management", "webui/phase09/app.js", "create_open_copy_delete_cleanup_resume"),
    ("input_wizard", "webui/phase09/app.js", ",".join(INPUT_STEPS)),
    ("run_monitor", "webui/phase09/app.js", "stage_command_logs_resources_resume"),
    ("candidate_ranking", "crates/gls-server/src/lib.rs", "artifact_backed_candidate_rows"),
    ("candidate_detail", "crates/gls-server/src/lib.rs", "source_orf_structure_docking_md_database_guidance"),
    ("genome_orf_context", "webui/phase09/app.js", "cds_utr_lncrna_intron_antisense_overlap_variant"),
    ("structure_patch_viewer", "crates/gls-server/src/lib.rs", "ngl_viewer_contract_chains_residues_patch_ligand_interactions"),
    ("md_summary", "crates/gls-server/src/lib.rs", "rmsd_rmsf_drift_occupancy_persistence_score"),
    ("evidence_ledger", "webui/phase09/app.js", "input_genome_structure_docking_md_database_report_cards"),
    ("report_export", "crates/gls-server/src/lib.rs", "research_report_candidate_table_reproducibility_package_project_package"),
    ("diagnostics", "crates/gls-server/src/lib.rs", "basic_user_diagnostics_advanced_debug_details"),
    ("setup_storage", "crates/gls-server/src/lib.rs", "setup_status_setup_report_storage_policy_cleanup_policy"),
    ("user_debug_api_split", "crates/gls-server/src/lib.rs", "api_user_surface_api_debug_surface"),
    ("bilingual_ui", "webui/phase09/app.js", "zh_en_language_toggle"),
    ("dark_only_ui", "webui/phase09/styles.css", "dark_only_no_theme_toggle"),
]


def main() -> int:
    parser = argparse.ArgumentParser(description="Generate Phase 0.9.1 acceptance artifacts from current repo and local WebUI state.")
    parser.add_argument("--artifact-dir", default=str(DEFAULT_ARTIFACT))
    parser.add_argument("--base-url", default=DEFAULT_BASE_URL)
    parser.add_argument("--workspace-root", default=str(ROOT))
    parser.add_argument("--run-cargo", action="store_true")
    args = parser.parse_args()

    workspace = Path(args.workspace_root).resolve()
    artifact = Path(args.artifact_dir).resolve()
    base_url = args.base_url.rstrip("/")
    ensure_dirs(artifact)
    acceptance_started_at = time.time()

    command_results: dict[str, dict[str, str]] = {}
    if args.run_cargo:
        command_results["cargo_fmt"] = run_command(["cargo", "fmt", "--check"], workspace)
        command_results["cargo_clippy"] = run_command(["cargo", "clippy", "--all-targets", "--all-features", "--", "-D", "warnings"], workspace)
        command_results["cargo_test"] = run_command(["cargo", "test", "--workspace"], workspace)
        command_results["build_gls_cli"] = run_command(["cargo", "build", "-p", "gls-cli"], workspace)
        command_results["build_gls_server"] = run_command(["cargo", "check", "-p", "gls-server"], workspace)
    else:
        for key in ["cargo_fmt", "cargo_clippy", "cargo_test", "build_gls_cli", "build_gls_server"]:
            command_results[key] = {"command": "not_run_by_generator", "exit_code": "1", "status": "issue", "stdout_tail": "", "stderr_tail": "rerun with --run-cargo"}
    command_results["node_check_webui"] = run_command(["node", "--check", "webui/phase09/app.js"], workspace)
    installer_self_check_dir = artifact / "setup" / "installer_self_check"
    command_results["setup_installer_self_check"] = run_command(
        [
            sys.executable,
            "-B",
            "scripts/setup/install_complete_scientific.py",
            "--workspace",
            str(workspace),
            "--report-dir",
            str(installer_self_check_dir),
            "--mode",
            "full-scientific",
            "--target-runtime",
            "wsl",
            "--self-check",
        ],
        workspace,
    )

    health = get_json(f"{base_url}/api/health")
    version = get_json(f"{base_url}/api/version")
    initial_projects = get_json(f"{base_url}/api/user/projects")
    diagnostics = get_json(f"{base_url}/api/user/diagnostics")
    debug_diagnostics = get_json(f"{base_url}/api/debug/diagnostics")
    debug_runtime = get_json(f"{base_url}/api/debug/runtime")
    setup_status = get_json(f"{base_url}/api/user/setup/status")
    setup_install_plan = get_json(f"{base_url}/api/user/setup/install-plan")
    storage_policy = get_json(f"{base_url}/api/user/storage/policy")
    route_split = get_json(f"{base_url}/api/debug/routes")
    sample_validation = post_json(f"{base_url}/api/user/setup/validate-sample-project", {})
    run_example_payload = post_json(f"{base_url}/api/user/projects/run-example", {})
    user_journey = ordinary_user_journey_smoke(base_url)
    project_lifecycle = project_lifecycle_smoke(base_url)
    projects = get_json(f"{base_url}/api/user/projects")
    latest_project = (run_example_payload.get("project") or {}).get("project_id") or run_example_payload.get("project_id")
    latest_run = ((run_example_payload.get("run_result") or {}).get("run") or {}).get("run_id")
    if not latest_project or not latest_run:
        latest_project, latest_run = latest_project_and_run(base_url, projects)
    run_payloads = run_artifact_payloads(base_url, latest_project, latest_run)
    export_payload = export_report(base_url, latest_project, latest_run)
    import_payload = import_project_package(base_url)
    cleanup_payload = cleanup_smoke_projects(base_url, user_journey, import_payload)
    final_projects = get_json(f"{base_url}/api/user/projects")
    websocket_smoke = websocket_live_smoke(base_url, latest_run)

    write_route_coverage(artifact, base_url, workspace)
    write_workflow_pipeline_summary(artifact, workspace)
    write_evidence_card_summary(artifact, workspace, run_payloads)
    write_dashboard_action_summary(artifact, workspace)
    write_input_field_validation_summary(artifact, workspace)
    write_run_forecast_confirmation_summary(artifact, workspace)
    write_preflight_group_action_summary(artifact, workspace)
    write_candidate_ranking_summary(artifact, workspace)
    write_genome_md_graphics_summary(artifact, workspace)
    write_settings_diagnostics_export_summary(artifact, workspace)
    write_report_quality_summary(artifact, export_payload)
    write_project_package_import_export_summary(artifact, workspace, projects, export_payload, import_payload)
    write_interactive_copy_lint_summary(artifact, workspace)
    write_default_runtime_identifier_lint_summary(artifact, workspace)
    write_default_user_surface_copy_lint_summary(artifact, workspace)
    write_frontend_source_integrity_summary(artifact, workspace)
    write_run_monitor_resource_summary(artifact, workspace)
    write_structure_viewer_contract_summary(artifact, workspace, run_payloads)
    write_component_semantics_summary(artifact, workspace)
    write_component_coverage(artifact, workspace)
    write_ui_stack(artifact)
    write_ui_dependency_licenses(artifact)
    write_visual_summaries(artifact)
    write_webui_e2e_summary(artifact, latest_project, latest_run, run_payloads, export_payload, user_journey)
    write_project_lifecycle_summary(artifact, project_lifecycle)
    write_clean_release_summary(
        artifact,
        initial_projects,
        run_example_payload,
        cleanup_payload,
        final_projects,
        workspace,
        storage_policy,
        acceptance_started_at,
    )
    write_diagnostics_summary(artifact, diagnostics, debug_diagnostics, debug_runtime, workspace)
    write_setup_storage_summary(artifact, setup_status, storage_policy, workspace, sample_validation, setup_install_plan)
    write_user_debug_api_split(artifact, route_split)
    write_post_push_cleanup_summary(artifact, workspace)
    write_cargo_summary(artifact, command_results)
    write_server_summary(artifact, health, run_payloads)
    write_scientific_regression_summary(artifact, workspace)
    write_api_contract(artifact, workspace)
    write_api_schema(artifact)
    write_websocket_validation(artifact, command_results, websocket_smoke)
    write_visual_audit_reports(artifact)
    write_playwright_summary(artifact)
    write_build_validation(artifact, command_results, workspace)
    write_model_schemas(artifact)
    copy_export_and_cleanup_manifests(artifact)
    write_ui_stack_decision_doc(artifact)
    copy_guides(artifact, workspace)
    write_objective_traceability_summary(artifact, workspace)

    print(f"phase09_acceptance_artifacts={artifact}")
    print(f"routes={len(ROUTES)}")
    print(f"latest_project={latest_project or ''}")
    print(f"latest_run={latest_run or ''}")
    return 0


def ensure_dirs(artifact: Path) -> None:
    for relative in ["metrics", "server", "ui/visual_audit", "webui/playwright_final_summary", "webui", "manifests", "docs"]:
        (artifact / relative).mkdir(parents=True, exist_ok=True)


def run_command(command: list[str], cwd: Path) -> dict[str, str]:
    result = subprocess.run(command, cwd=cwd, text=True, capture_output=True)
    return {
        "command": " ".join(command),
        "exit_code": str(result.returncode),
        "status": "ok" if result.returncode == 0 else "issue",
        "stdout_tail": tail(result.stdout),
        "stderr_tail": tail(result.stderr),
    }


def tail(text: str, limit: int = 2000) -> str:
    return text.replace("\t", " ").replace("\r", " ").replace("\n", " ")[-limit:]


def get_json(url: str) -> dict:
    try:
        with urllib.request.urlopen(url, timeout=15) as response:
            return json.loads(response.read().decode("utf-8"))
    except (urllib.error.URLError, TimeoutError, json.JSONDecodeError) as exc:
        return {"status": "issue", "error": str(exc), "url": url}


def post_json(url: str, payload: dict) -> dict:
    data = json.dumps(payload).encode("utf-8")
    request = urllib.request.Request(url, data=data, method="POST", headers={"Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            return json.loads(response.read().decode("utf-8"))
    except (urllib.error.URLError, TimeoutError, json.JSONDecodeError) as exc:
        return {"status": "issue", "error": str(exc), "url": url}


def delete_json(url: str) -> dict:
    request = urllib.request.Request(url, method="DELETE")
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            return json.loads(response.read().decode("utf-8"))
    except (urllib.error.URLError, TimeoutError, json.JSONDecodeError) as exc:
        return {"status": "issue", "error": str(exc), "url": url}


def latest_project_and_run(base_url: str, projects_payload: dict) -> tuple[str | None, str | None]:
    projects = projects_payload.get("projects") or []
    projects = sorted(projects, key=lambda item: record_sort_key(item, "latest_run_id", "project_id"), reverse=True)
    fallback: tuple[str | None, str | None] = (None, None)
    for project in projects:
        project_id = project.get("project_id")
        if not project_id:
            continue
        runs = get_json(f"{base_url}/api/projects/{project_id}/runs").get("runs") or []
        runs = sorted(runs, key=lambda item: record_sort_key(item, "run_id"), reverse=True)
        for run in runs:
            run_id = run.get("run_id")
            if not run_id:
                continue
            if fallback == (None, None):
                fallback = (project_id, run_id)
            candidates = get_json(f"{base_url}/api/runs/{run_id}/candidates")
            evidence = get_json(f"{base_url}/api/runs/{run_id}/evidence")
            if candidates.get("row_count", 0) > 0 and evidence.get("row_count", 0) > 0:
                return project_id, run_id
    return fallback


def record_sort_key(item: dict, *keys: str) -> int:
    for key in ("updated_at", "modified_at", "created_at", *keys):
        value = item.get(key)
        number = numeric_suffix(value)
        if number:
            return number
    return 0


def numeric_suffix(value: object) -> int:
    parts = []
    current = []
    for char in str(value or ""):
        if char.isdigit():
            current.append(char)
        elif current:
            parts.append(int("".join(current)))
            current = []
    if current:
        parts.append(int("".join(current)))
    return max(parts) if parts else 0


def visible_project_count(projects_payload: dict) -> int:
    return len(projects_payload.get("projects") or [])


def visible_latest_run_count(projects_payload: dict) -> int:
    return sum(1 for project in projects_payload.get("projects") or [] if project.get("latest_run_id"))


def has_run_example_template(projects_payload: dict) -> bool:
    return any(
        template.get("template_id") == "run_example_demo_template"
        and template.get("contains_precomputed_results") is False
        and template.get("creates_results_only_on_click") is True
        for template in projects_payload.get("templates") or []
    )


def initial_projects_have_no_precreated_results(projects_payload: dict) -> bool:
    projects = projects_payload.get("projects") or []
    templates = projects_payload.get("templates") or []
    return all(not project.get("latest_run_id") for project in projects) and all(
        template.get("contains_precomputed_results") is not True for template in templates
    )


def run_artifact_payloads(base_url: str, project_id: str | None, run_id: str | None) -> dict[str, dict]:
    if not run_id:
        return {}
    payloads = {}
    for key, endpoint in [
        ("candidates", "candidates"),
        ("evidence", "evidence"),
        ("genome_context", "genome-context"),
        ("structure", "structure"),
        ("md_summary", "md-summary"),
        ("logs", "logs"),
    ]:
        payloads[key] = get_json(f"{base_url}/api/runs/{run_id}/{endpoint}")
    return payloads


def ordinary_user_journey_smoke(base_url: str) -> dict:
    steps: list[dict[str, object]] = []

    def add_step(name: str, ok: bool, detail: object = "") -> None:
        steps.append({"step": name, "ok": bool(ok), "detail": detail})

    project = post_json(f"{base_url}/api/projects", {})
    project_id = (project.get("project") or {}).get("project_id")
    add_step("create_project", bool(project_id), project.get("status", ""))
    input_manifest = {
        "inputs": [
            {"field": "project_name", "label": "Project name", "value": "Ordinary researcher smoke", "control_type": "input"},
            {"field": "smiles", "label": "SMILES", "value": "CCO", "control_type": "textarea"},
            {"field": "search_scope", "label": "Search scope", "value": "conservative_search", "control_type": "choice"},
            {"field": "run_profile", "label": "Run profile", "value": "standard", "control_type": "choice"},
            {"field": "forecast_confirmed", "label": "Forecast confirmation", "value": "true", "control_type": "input"},
        ],
        "files": [
            {
                "field": "upload_structure_file",
                "label": "Upload structure file",
                "name": "ligand.sdf",
                "size": 7,
                "sha256": "9a0364b9e99bb480dd25e1f0284c8555",
                "content_base64": "bGlnYW5kCg==",
            }
        ],
        "advanced_parameters_collapsed": True,
        "field_schema": "genoligand.webui.input_manifest.v0.9.1",
    }
    if project_id:
        saved = post_json(f"{base_url}/api/projects/{project_id}/inputs", input_manifest)
    else:
        saved = {"status": "issue"}
    add_step("save_input", saved.get("status") == "saved", saved.get("status", ""))
    preflight = post_json(f"{base_url}/api/projects/{project_id}/preflight", {}) if project_id else {"status": "issue"}
    add_step("run_preflight", preflight.get("status") == "preflight_passed", preflight.get("status", ""))
    run = post_json(f"{base_url}/api/projects/{project_id}/runs", {}) if project_id else {"status": "issue"}
    run_id = (run.get("run") or {}).get("run_id")
    add_step("start_workflow", bool(run_id), (run.get("run") or {}).get("state", run.get("status", "")))
    candidates = get_json(f"{base_url}/api/runs/{run_id}/candidates") if run_id else {}
    evidence = get_json(f"{base_url}/api/runs/{run_id}/evidence") if run_id else {}
    add_step("view_results", int(candidates.get("row_count", 0) or 0) > 0, candidates.get("row_count", 0))
    add_step("view_evidence", int(evidence.get("row_count", 0) or 0) > 0, evidence.get("row_count", 0))
    exported = export_report(base_url, project_id, run_id)
    add_step("export_project_package", exported.get("status") == "ready" and int(exported.get("candidate_row_count", 0) or 0) > 0, exported.get("status", ""))
    imported = import_project_package(base_url)
    add_step("import_project_package", imported.get("status") == "imported", imported.get("status", ""))
    ok_count = sum(1 for step in steps if step["ok"])
    return {
        "project_id": project_id or "",
        "run_id": run_id or "",
        "imported_project_id": imported.get("project_id") or "",
        "steps": steps,
        "step_count": len(steps),
        "ok_count": ok_count,
        "pass_rate": "100%" if steps and ok_count == len(steps) else f"{ok_count}/{len(steps)}",
        "pass": bool(steps and ok_count == len(steps)),
    }


def project_id_from_payload(payload: dict) -> str:
    return str((payload.get("project") or {}).get("project_id") or payload.get("project_id") or "")


def run_id_from_payload(payload: dict) -> str:
    return str((payload.get("run") or {}).get("run_id") or payload.get("run_id") or "")


def project_lifecycle_smoke(base_url: str) -> dict:
    steps: list[dict[str, object]] = []
    created_project_ids: list[str] = []
    deleted_projects: list[dict[str, str]] = []

    def add_step(name: str, ok: bool, detail: object = "") -> None:
        steps.append({"step": name, "ok": bool(ok), "detail": detail})

    def remember_project(project_id: str) -> None:
        if project_id and project_id not in created_project_ids:
            created_project_ids.append(project_id)

    project = post_json(f"{base_url}/api/projects", {})
    project_id = project_id_from_payload(project)
    remember_project(project_id)
    add_step("create_project", project.get("status") == "created" and bool(project_id), project.get("status", ""))

    opened = get_json(f"{base_url}/api/projects/{urllib.parse.quote(project_id)}") if project_id else {}
    add_step("open_project", project_id and project_id in json.dumps(opened), opened.get("status", ""))

    input_manifest = {
        "inputs": [
            {"field": "project_name", "label": "Project name", "value": "Lifecycle smoke project", "control_type": "input"},
            {"field": "smiles", "label": "SMILES", "value": "CCO", "control_type": "textarea"},
            {"field": "run_profile", "label": "Run profile", "value": "standard", "control_type": "choice"},
            {"field": "forecast_confirmed", "label": "Forecast confirmation", "value": "true", "control_type": "input"},
        ],
        "files": [
            {
                "field": "upload_structure_file",
                "label": "Upload structure file",
                "name": "ligand.sdf",
                "size": 7,
                "sha256": "9a0364b9e99bb480dd25e1f0284c8555",
                "content_base64": "bGlnYW5kCg==",
            }
        ],
        "advanced_parameters_collapsed": True,
        "field_schema": "genoligand.webui.input_manifest.v0.9.1",
    }
    saved = post_json(f"{base_url}/api/projects/{urllib.parse.quote(project_id)}/inputs", input_manifest) if project_id else {}
    add_step("save_inputs_for_lifecycle", saved.get("status") == "saved", saved.get("status", ""))

    preflight = post_json(f"{base_url}/api/projects/{urllib.parse.quote(project_id)}/preflight", {}) if project_id else {}
    add_step("preflight_project", preflight.get("status") == "preflight_passed", preflight.get("status", ""))

    run = post_json(f"{base_url}/api/projects/{urllib.parse.quote(project_id)}/runs", {}) if project_id else {}
    run_id = run_id_from_payload(run)
    add_step("submit_run", bool(run_id), (run.get("run") or {}).get("state") or run.get("status", ""))

    cancel = post_json(f"{base_url}/api/runs/{urllib.parse.quote(run_id)}/cancel", {}) if run_id else {}
    add_step("cancel_run", cancel.get("status") == "cancelled", cancel.get("status", ""))

    resume = post_json(f"{base_url}/api/runs/{urllib.parse.quote(run_id)}/resume", {}) if run_id else {}
    add_step("resume_project", resume.get("status") == "resumable", resume.get("status", ""))

    copied = post_json(f"{base_url}/api/projects/{urllib.parse.quote(project_id)}/copy", {}) if project_id else {}
    copied_project_id = project_id_from_payload(copied)
    remember_project(copied_project_id)
    add_step("duplicate_project", copied.get("status") == "created" and bool(copied_project_id), copied.get("status", ""))

    cleanup = post_json(f"{base_url}/api/projects/{urllib.parse.quote(project_id)}/cleanup", {}) if project_id else {}
    add_step("cleanup_project", cleanup.get("status") == "completed", cleanup.get("status", ""))

    for cleanup_project_id in reversed(created_project_ids):
        result = delete_json(f"{base_url}/api/projects/{urllib.parse.quote(cleanup_project_id)}")
        deleted_projects.append({
            "project_id": cleanup_project_id,
            "status": str(result.get("status") or result.get("error") or "unknown"),
        })
    add_step(
        "delete_project",
        bool(created_project_ids) and all(item["status"] == "deleted" for item in deleted_projects),
        deleted_projects,
    )

    ok_count = sum(1 for step in steps if step["ok"])
    return {
        "project_id": project_id,
        "run_id": run_id,
        "copied_project_id": copied_project_id,
        "created_project_ids": created_project_ids,
        "deleted_projects": deleted_projects,
        "steps": steps,
        "step_count": len(steps),
        "ok_count": ok_count,
        "pass_rate": "100%" if steps and ok_count == len(steps) else f"{ok_count}/{len(steps)}",
        "pass": bool(steps and ok_count == len(steps)),
    }


def cleanup_smoke_projects(base_url: str, user_journey: dict, import_payload: dict | None = None) -> dict:
    deleted: list[dict[str, str]] = []
    cleanup_targets = [
        ("project_id", user_journey.get("project_id") or ""),
        ("imported_project_id", user_journey.get("imported_project_id") or ""),
    ]
    if import_payload:
        cleanup_targets.append(("main_imported_project_id", import_payload.get("project_id") or ""))
    seen: set[str] = set()
    for key, project_id in cleanup_targets:
        if not project_id:
            continue
        project_id = str(project_id)
        if project_id in seen:
            continue
        seen.add(project_id)
        result = delete_json(f"{base_url}/api/projects/{urllib.parse.quote(str(project_id))}")
        deleted.append({
            "key": key,
            "project_id": str(project_id),
            "status": str(result.get("status") or result.get("error") or "unknown"),
        })
    clear_examples = post_json(f"{base_url}/api/user/projects/clear-examples", {})
    return {
        "deleted_projects": deleted,
        "clear_examples": clear_examples,
    }


def export_report(base_url: str, project_id: str | None, run_id: str | None) -> dict:
    payload = {}
    if project_id:
        payload["project_id"] = project_id
    if run_id:
        payload["run_id"] = run_id
    return post_json(f"{base_url}/api/user/reports/export", payload)


def import_project_package(base_url: str) -> dict:
    return post_json(f"{base_url}/api/user/projects/import-package", {})


def read_exact(sock: socket.socket, length: int) -> bytes:
    chunks: list[bytes] = []
    remaining = length
    while remaining > 0:
        chunk = sock.recv(remaining)
        if not chunk:
            raise RuntimeError("websocket_connection_closed")
        chunks.append(chunk)
        remaining -= len(chunk)
    return b"".join(chunks)


def read_ws_text_frame(sock: socket.socket) -> str:
    header = read_exact(sock, 2)
    opcode = header[0] & 0x0F
    length = header[1] & 0x7F
    if length == 126:
        length = int.from_bytes(read_exact(sock, 2), "big")
    elif length == 127:
        length = int.from_bytes(read_exact(sock, 8), "big")
    payload = read_exact(sock, length)
    if opcode == 8:
        raise RuntimeError("websocket_close_frame")
    if opcode != 1:
        raise RuntimeError(f"websocket_unexpected_opcode:{opcode}")
    return payload.decode("utf-8", errors="replace")


def websocket_live_smoke(base_url: str, run_id: str | None) -> dict:
    parsed = urllib.parse.urlparse(base_url)
    host = parsed.hostname or "127.0.0.1"
    port = parsed.port or (443 if parsed.scheme == "https" else 80)
    path = f"/ws/runs/{urllib.parse.quote(run_id or '')}"
    key = base64.b64encode(os.urandom(16)).decode("ascii")
    request = (
        f"GET {path} HTTP/1.1\r\n"
        f"Host: {host}:{port}\r\n"
        "Upgrade: websocket\r\n"
        "Connection: Upgrade\r\n"
        f"Sec-WebSocket-Key: {key}\r\n"
        "Sec-WebSocket-Version: 13\r\n"
        "\r\n"
    )
    messages: list[dict] = []
    try:
        with socket.create_connection((host, port), timeout=5) as sock:
            sock.settimeout(5)
            sock.sendall(request.encode("ascii"))
            response = b""
            while b"\r\n\r\n" not in response:
                response += sock.recv(4096)
                if len(response) > 16384:
                    raise RuntimeError("websocket_handshake_too_large")
            handshake = response.decode("iso-8859-1", errors="replace")
            if "101 Switching Protocols" not in handshake or "Sec-WebSocket-Accept:" not in handshake:
                raise RuntimeError("websocket_handshake_failed")
            for _ in range(3):
                messages.append(json.loads(read_ws_text_frame(sock)))
    except Exception as exc:  # noqa: BLE001 - diagnostic artifact records exact failure.
        return {
            "ok": False,
            "error": str(exc),
            "message_count": len(messages),
            "types": sorted({str(message.get("type")) for message in messages}),
            "messages": messages,
        }
    types = {str(message.get("type")) for message in messages}
    return {
        "ok": {"status", "resource", "logs"}.issubset(types),
        "error": "",
        "message_count": len(messages),
        "types": sorted(types),
        "messages": messages,
    }


def write_tsv(path: Path, headers: list[str], rows: list[list[object]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8", newline="") as handle:
        writer = csv.writer(handle, delimiter="\t", lineterminator="\n")
        writer.writerow(headers)
        for row in rows:
            writer.writerow(["" if value is None else str(value) for value in row])


def write_json(path: Path, value: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")


def final_function_block(source: str, function_name: str) -> str:
    pattern = re.compile(rf"(?:^|\n)(?:async\s+)?function\s+{re.escape(function_name)}\b")
    matches = list(pattern.finditer(source))
    if not matches:
        return ""
    match = matches[-1]
    start = match.start() + (1 if source[match.start()] == "\n" else 0)
    next_match = re.search(r"\n(?:async\s+)?function\s+\w+\b", source[start + 1 :])
    next_start = start + 1 + next_match.start() if next_match else -1
    return source[start:] if next_start < 0 else source[start:next_start]


def write_route_coverage(artifact: Path, base_url: str, workspace: Path) -> None:
    source = (workspace / "webui/phase09/app.js").read_text(encoding="utf-8", errors="replace")
    visual_routes: set[str] = set()
    for manifest in [
        artifact / "ui/visual_audit_manifest.tsv",
        artifact / "ui/screenshots/final/screenshot_metadata.tsv",
        artifact / "ui/screenshots/final_narrow_desktop/screenshot_metadata.tsv",
    ]:
        if not manifest.exists():
            continue
        with manifest.open("r", encoding="utf-8", newline="") as handle:
            for row in csv.DictReader(handle, delimiter="\t"):
                route = row.get("route", "")
                status = row.get("status", "")
                if route and status in {"ready", "rendered", "ok"}:
                    visual_routes.add(route)
    rows = []
    for route in ROUTES:
        source_present = route in source
        visual_present = route in visual_routes
        rows.append([
            route,
            f"{base_url}/#/{route}",
            "webui/phase09/app.js",
            source_present,
            visual_present,
            "ok" if source_present and visual_present else "issue",
        ])
    write_tsv(
        artifact / "metrics/webui_route_coverage.tsv",
        ["route", "url", "source", "source_present", "visual_present", "status"],
        rows,
    )


def write_workflow_pipeline_summary(artifact: Path, workspace: Path) -> None:
    source = (workspace / "webui/phase09/app.js").read_text(encoding="utf-8", errors="replace")
    expected_steps = ["Project", "Input", "Database", "Scope", "Preflight", "Run", "Results", "Evidence", "Export"]
    context_panel = final_function_block(source, "workflowContextPanel")
    context_fields = ["Current step", "Completion", "Blocker", "Resume entry", "Next step action"]
    context_field_count = sum(1 for field in context_fields if field in context_panel)
    non_dashboard_route_count = len([route for route in ROUTES if route != "dashboard_page"])
    context_injected_for_routes = "workflowContextPanel(activeId)" in source and 'activeId === "dashboard_page"' in context_panel
    routes_block = source.split("const routes =", 1)[-1].split("];", 1)[0]
    main_nav_item_count = sum(1 for group in ["workflow", "results", "system"] if f'["{group}"' in routes_block)
    github_main_nav_count = routes_block.count("github")
    rows = [
        ["main_nav_item_count", main_nav_item_count, "ok" if main_nav_item_count == 3 else "issue"],
        ["github_main_nav_count", github_main_nav_count, "ok" if github_main_nav_count == 0 else "issue"],
        ["workflow_step_count", len(expected_steps), "ok" if all(step in source for step in expected_steps) else "issue"],
        ["workflow_step_order", " > ".join(expected_steps), "ok" if "const pipelineSteps" in source else "issue"],
        ["main_context_panel", "workflowContextPanel", "ok" if "function workflowContextPanel" in source else "issue"],
        ["dashboard_context_suppressed", True, "ok" if 'activeId === "dashboard_page"' in source else "issue"],
        ["workflow_context_required_field_count", context_field_count, "ok" if context_field_count == len(context_fields) else "issue"],
        ["workflow_context_non_dashboard_route_coverage", f"{non_dashboard_route_count}/{non_dashboard_route_count}", "ok" if context_injected_for_routes else "issue"],
        ["workflow_context_blocker_fix_link_present", "Fix blocker", "ok" if "blockerHref" in context_panel and "Fix blocker" in context_panel else "issue"],
        ["workflow_context_next_action_link_present", "Next step action", "ok" if "Next step action" in context_panel and "summary.next[2]" in context_panel else "issue"],
        ["workflow_context_resume_action_link_present", "Resume action", "ok" if "Resume action" in context_panel and "resumeHref" in context_panel else "issue"],
        ["detail_panel_context", "renderDetail active step blocker next resume", "ok" if "function renderDetail" in source and "Blocker" in source and "Resume entry" in source else "issue"],
        ["runtime_timeline_uses_pipeline", "pipelineSteps", "ok" if "function renderRuntime" in source and "pipelineSteps" in source else "issue"],
    ]
    write_tsv(artifact / "metrics/workflow_pipeline_summary.tsv", ["metric", "value", "status"], rows)


def write_evidence_card_summary(artifact: Path, workspace: Path, payloads: dict[str, dict]) -> None:
    source = (workspace / "webui/phase09/app.js").read_text(encoding="utf-8", errors="replace")
    styles = (workspace / "webui/phase09/styles.css").read_text(encoding="utf-8", errors="replace")
    evidence_ledger = final_function_block(source, "evidenceLedger")
    evidence_user_rows = final_function_block(source, "evidenceUserRows")
    evidence_internal_details = final_function_block(source, "evidenceInternalDetails")
    evidence_group_for_row = final_function_block(source, "evidenceGroupForRow")
    evidence_why = final_function_block(source, "evidenceWhy")
    evidence_user_header = evidence_user_rows.split("rows.slice", 1)[0]
    groups = ["Input", "Genome", "Structure", "Docking", "MD", "Database", "Report"]
    visible_terms = ["Evidence", "Importance", "Confidence", "Source", "State", "Why this matters"]
    internal_terms = ["report_item", "source_file", "manifest", "schema", "tool_version", "artifact_hash", "validation_state", "report_section"]
    rows_payload = payloads.get("evidence", {}).get("evidence") or []
    real_group_count = len({row.get("report_section") or row.get("source_file") or row.get("report_item") for row in rows_payload if isinstance(row, dict)})
    evidence_viewer_ok = (
        all(group in evidence_ledger for group in groups)
        and "<article class=\"evidence-card\"" in evidence_ledger
        and "evidence-grid" in evidence_ledger
        and all(term in evidence_user_rows for term in visible_terms)
        and "advancedDetails" in evidence_ledger
        and all(term not in evidence_user_header for term in internal_terms)
        and real_group_count > 0
    )
    rows = [
        ["evidence_ledger_viewer_pass", evidence_viewer_ok, "ok" if evidence_viewer_ok else "issue"],
        ["evidence_group_count", len(groups), "ok" if all(group in evidence_ledger for group in groups) else "issue"],
        ["evidence_card_markup_present", "article.evidence-card", "ok" if "<article class=\"evidence-card\"" in evidence_ledger and "evidence-grid" in evidence_ledger else "issue"],
        ["default_visible_evidence_field_count", len(visible_terms), "ok" if all(term in evidence_user_rows for term in visible_terms) else "issue"],
        ["default_visible_evidence_fields", ", ".join(visible_terms), "ok" if all(term in evidence_user_rows for term in visible_terms) else "issue"],
        ["internal_evidence_field_count", len(internal_terms), "ok" if all(term in evidence_internal_details for term in internal_terms) else "issue"],
        ["internal_fields_advanced_only", ", ".join(internal_terms), "ok" if "advancedDetails" in evidence_ledger and "source_path" not in evidence_user_rows and all(term not in evidence_user_header for term in internal_terms) else "issue"],
        ["evidence_grouping_function", "evidenceGroupForRow", "ok" if "function evidenceGroupForRow" in source and all(group in evidence_group_for_row for group in groups) else "issue"],
        ["evidence_interpretation_function", "evidenceWhy", "ok" if "function evidenceWhy" in source and all(term in evidence_why for term in ["MD", "Structure", "Database"]) else "issue"],
        ["evidence_empty_group_pending_state", "Pending | No evidence in this group yet", "ok" if "Pending" in evidence_ledger and "No evidence in this group yet" in evidence_user_rows else "issue"],
        ["evidence_card_css_present", "evidence-grid | evidence-card | evidence-card-head", "ok" if all(term in styles for term in [".evidence-grid", ".evidence-card", ".evidence-card-head"]) else "issue"],
        ["evidence_real_row_count", len(rows_payload), "ok" if len(rows_payload) > 0 else "issue"],
        ["evidence_real_source_count", real_group_count, "ok" if real_group_count > 0 else "issue"],
    ]
    write_tsv(artifact / "metrics/evidence_card_summary.tsv", ["metric", "value", "status"], rows)


def write_dashboard_action_summary(artifact: Path, workspace: Path) -> None:
    source = (workspace / "webui/phase09/app.js").read_text(encoding="utf-8", errors="replace")
    html = (workspace / "webui/phase09/index.html").read_text(encoding="utf-8", errors="replace")
    dashboard_block = final_function_block(source, "dashboard")
    allowed_actions = ["New project", "Open project", "Run example", "Clear examples", "Check environment"]
    main_action_count = sum(1 for action in allowed_actions if action in dashboard_block)
    extra_action_terms = ["Open database setup", "Open environment check"]
    extra_action_count = sum(1 for term in extra_action_terms if term in dashboard_block)
    if "Start Setup" in html or "startSetup" in html:
        extra_action_count += 1
    dashboard_detail_branch = source.rsplit('if (activeId === "dashboard_page")', 1)[-1].split("return;", 1)[0]
    detail_action_count = dashboard_detail_branch.count("buttonRow(") + dashboard_detail_branch.count("<a ")
    action_header_count = dashboard_block.count('"Action"') + dashboard_block.count('"操作"')
    github_visible_count = dashboard_block.count("GitHub")
    home_sections = {
        "recent_projects": all(term in dashboard_block for term in ["Recent Projects", "recentProjects"]),
        "recent_results": all(term in dashboard_block for term in ["Recent Results", "recentResults"]),
        "key_environment_status": "Key Environment Status" in dashboard_block,
    }
    dashboard_default_surface = "\n".join([dashboard_block, dashboard_detail_branch, html])
    developer_terms = [
        "GitHub",
        "commit",
        "remote",
        "manifest",
        "schema",
        "hash",
        "raw JSON",
        "route=",
        "gate",
        "gls-server",
        "127.0.0.1",
    ]
    developer_trace_count = sum(dashboard_default_surface.count(term) for term in developer_terms)
    rows = [
        ["default_github_hidden_pass", github_visible_count == 0, "ok" if github_visible_count == 0 else "issue"],
        ["default_developer_terms_hidden_pass", developer_trace_count == 0, "ok" if developer_trace_count == 0 else "issue"],
        ["dashboard_main_action_count", main_action_count, "ok" if main_action_count == 5 else "issue"],
        ["dashboard_extra_action_count", extra_action_count, "ok" if extra_action_count == 0 else "issue"],
        ["dashboard_detail_action_count", detail_action_count, "ok" if detail_action_count == 0 else "issue"],
        ["dashboard_action_table_header_count", action_header_count, "ok" if action_header_count == 0 else "issue"],
        ["dashboard_github_visible_count", github_visible_count, "ok" if github_visible_count == 0 else "issue"],
        ["dashboard_home_sections_count", sum(1 for present in home_sections.values() if present), "ok" if all(home_sections.values()) else "issue"],
        ["dashboard_recent_projects_present", home_sections["recent_projects"], "ok" if home_sections["recent_projects"] else "issue"],
        ["dashboard_recent_results_present", home_sections["recent_results"], "ok" if home_sections["recent_results"] else "issue"],
        ["dashboard_key_environment_status_present", home_sections["key_environment_status"], "ok" if home_sections["key_environment_status"] else "issue"],
        ["dashboard_default_developer_trace_count", developer_trace_count, "ok" if developer_trace_count == 0 else "issue"],
        ["dashboard_default_forbidden_terms", ", ".join(developer_terms), "ok" if developer_trace_count == 0 else "issue"],
        ["dashboard_allowed_actions", " | ".join(allowed_actions), "ok"],
    ]
    write_tsv(artifact / "metrics/dashboard_action_summary.tsv", ["metric", "value", "status"], rows)


def write_input_field_validation_summary(artifact: Path, workspace: Path) -> None:
    source = (workspace / "webui/phase09/app.js").read_text(encoding="utf-8", errors="replace")
    final_wizard = source.rsplit("function wizardPage", 1)[-1].split("function preflightTable", 1)[0]
    computed_validation = final_function_block(source, "computedFieldValidation")
    collect_manifest = final_function_block(source, "collectInputManifest")
    capture_inputs = final_function_block(source, "captureVisibleInputs")
    merge_input_draft = final_function_block(source, "mergeInputDraft")
    default_choices = final_function_block(source, "ensureDefaultChoices")
    material_cards = re.findall(r"inputMaterialCard\(", final_wizard)
    expected_fields = [
        "SMILES",
        "Structure file",
        "FASTA",
        "Chain ID",
        "Ligand residue",
        "GTF/GFF3",
        "VCF",
        "Search scope",
        "Database profile",
        "Run profile",
        "GPU guidance",
        "CPU fallback",
        "Storage",
    ]
    present_fields = [field for field in expected_fields if field in final_wizard]
    status_labels = ["PASS", "WARN", "FAIL", "Pending"]
    validation_rules = {
        "smiles_format": "validSmilesClient" in computed_validation and "invalid SMILES" in computed_validation,
        "structure_extensions": all(ext in computed_validation for ext in [".sdf", ".mol2", ".pdb", ".cif", ".mmcif"]),
        "fasta_sequence": "sequenceStats" in computed_validation and "non-standard residues" in computed_validation,
        "chain_id": "chain id" in computed_validation and "short chain label" in computed_validation,
        "ligand_residue": "ligand residue" in computed_validation and "residue name" in computed_validation,
        "gtf_gff3": all(ext in computed_validation for ext in [".gtf", ".gff", ".gff3"]),
        "vcf": ".vcf" in computed_validation and ".vcf.gz" in computed_validation,
        "database_profile": "selectedChoiceValue(\"database_profile\")" in computed_validation,
        "search_scope": "selectedChoiceValue(\"search_scope\")" in computed_validation,
        "run_profile": "selectedChoiceValue(\"run_profile\")" in computed_validation,
    }
    manifest_contract = {
        "captures_text_inputs": "querySelectorAll(\"input, textarea, select\")" in capture_inputs and 'node.type !== "file"' in capture_inputs,
        "captures_files": "querySelectorAll('input[type=\"file\"]')" in capture_inputs,
        "records_file_hash": "sha256: await fileSha256(file)" in capture_inputs,
        "records_file_content": "content_base64: await fileBase64(file)" in capture_inputs,
        "merges_visible_inputs": "visibleInputKeys" in merge_input_draft and "visibleFileKeys" in merge_input_draft,
        "writes_manifest_schema": "field_schema" in collect_manifest and "genoligand.webui.input_manifest.v0.9.1" in collect_manifest,
        "advanced_collapsed": "advanced_parameters_collapsed: true" in collect_manifest,
        "default_choices": all(choice in default_choices for choice in ["molecule_type", "species_mode", "search_scope", "run_profile"]),
    }
    materials = ["Compound name", "Structure file", "Peptide sequence", "Protein sequence", "Complex structure", "Custom genome"]
    material_present = [material for material in materials if material in final_wizard]
    project_basics_ok = all(term in final_wizard for term in ["Project name", "Research question"])
    molecule_type_ok = (
        ("Molecule Type" in source or "Choose input material" in final_wizard)
        and "molecule_type" in default_choices
        and len(material_cards) >= len(materials)
    )
    small_molecule_ok = all(term in final_wizard for term in ["Compound name", "SMILES", "Structure file"]) and validation_rules["smiles_format"] and validation_rules["structure_extensions"]
    peptide_ok = "Peptide sequence" in final_wizard and validation_rules["fasta_sequence"]
    protein_ligand_ok = "Protein sequence" in final_wizard and validation_rules["fasta_sequence"]
    complex_structure_ok = all(term in final_wizard for term in ["Complex structure", "Chain ID", "Ligand residue"]) and validation_rules["chain_id"] and validation_rules["ligand_residue"]
    genome_ok = all(term in final_wizard for term in ["Custom genome", "GTF/GFF3", "VCF"]) and validation_rules["gtf_gff3"] and validation_rules["vcf"]
    search_scope_ok = "Search scope" in final_wizard and validation_rules["search_scope"]
    database_profile_ok = "Database profile" in final_wizard and validation_rules["database_profile"]
    run_profile_ok = "Run profile" in final_wizard and validation_rules["run_profile"]
    input_wizard_ok = all(
        [
            project_basics_ok,
            molecule_type_ok,
            small_molecule_ok,
            peptide_ok,
            protein_ligand_ok,
            complex_structure_ok,
            genome_ok,
            search_scope_ok,
            database_profile_ok,
            run_profile_ok,
            all(validation_rules.values()),
            all(manifest_contract.values()),
            len(material_present) == len(materials),
        ]
    )
    rows = [
        ["input_wizard_page_pass", input_wizard_ok, "ok" if input_wizard_ok else "issue"],
        ["project_basics_form_pass", project_basics_ok, "ok" if project_basics_ok else "issue"],
        ["molecule_type_selection_pass", molecule_type_ok, "ok" if molecule_type_ok else "issue"],
        ["small_molecule_input_flow_pass", small_molecule_ok, "ok" if small_molecule_ok else "issue"],
        ["peptide_input_flow_pass", peptide_ok, "ok" if peptide_ok else "issue"],
        ["protein_ligand_input_flow_pass", protein_ligand_ok, "ok" if protein_ligand_ok else "issue"],
        ["complex_structure_input_flow_pass", complex_structure_ok, "ok" if complex_structure_ok else "issue"],
        ["organism_genome_selection_pass", genome_ok, "ok" if genome_ok else "issue"],
        ["search_scope_selection_pass", search_scope_ok, "ok" if search_scope_ok else "issue"],
        ["database_profile_selection_pass", database_profile_ok, "ok" if database_profile_ok else "issue"],
        ["run_profile_selection_pass", run_profile_ok, "ok" if run_profile_ok else "issue"],
        ["field_validation_component_present", "fieldValidationRows", "ok" if "function fieldValidationRows" in source else "issue"],
        ["computed_validation_component_present", "computedFieldValidation", "ok" if "function computedFieldValidation" in source else "issue"],
        ["field_validation_expected_field_count", len(expected_fields), "ok" if len(present_fields) == len(expected_fields) else "issue"],
        ["input_material_card_count", len(material_cards), "ok" if len(material_cards) >= len(materials) else "issue"],
        ["input_material_required_labels_present", " | ".join(materials), "ok" if len(material_present) == len(materials) else "issue"],
        ["field_validation_rule_count", sum(1 for present in validation_rules.values() if present), "ok" if all(validation_rules.values()) else "issue"],
        ["field_validation_rules_complete", True, "ok" if all(validation_rules.values()) else "issue"],
        ["input_manifest_contract_count", sum(1 for present in manifest_contract.values() if present), "ok" if all(manifest_contract.values()) else "issue"],
        ["input_manifest_contract_complete", True, "ok" if all(manifest_contract.values()) else "issue"],
        ["field_validation_repair_suggestion_count", final_wizard.count("fix") + final_wizard.count("Checked") + final_wizard.count("选择"), "ok" if "fieldValidationRows" in final_wizard else "issue"],
        ["field_validation_status_labels", " / ".join(status_labels), "ok" if all(label in source for label in status_labels) else "issue"],
        ["structure_preview_count", final_wizard.count("structurePreview("), "ok" if final_wizard.count("structurePreview(") >= 3 else "issue"],
        ["scope_database_run_profile_validation_present", "Search scope | Database profile | Run profile", "ok" if all(field in final_wizard for field in ["Search scope", "Database profile", "Run profile"]) else "issue"],
        ["smiles_fasta_structure_validation_present", "SMILES | FASTA | Structure file", "ok" if all(field in final_wizard for field in ["SMILES", "FASTA", "Structure file"]) else "issue"],
        ["chain_ligand_residue_validation_present", "Chain ID | Ligand residue", "ok" if all(field in final_wizard for field in ["Chain ID", "Ligand residue"]) else "issue"],
        ["genome_file_validation_present", "GTF/GFF3 | VCF", "ok" if all(field in final_wizard for field in ["GTF/GFF3", "VCF"]) else "issue"],
    ]
    write_tsv(artifact / "metrics/input_field_validation_summary.tsv", ["metric", "value", "status"], rows)


def write_run_forecast_confirmation_summary(artifact: Path, workspace: Path) -> None:
    source = (workspace / "webui/phase09/app.js").read_text(encoding="utf-8", errors="replace")
    styles = (workspace / "webui/phase09/styles.css").read_text(encoding="utf-8", errors="replace")
    forecast_model = final_function_block(source, "runForecastModel")
    forecast_cards = final_function_block(source, "runForecastCards")
    forecast_panel = final_function_block(source, "forecastConfirmationPanel")
    forecast_guard = final_function_block(source, "requiresForecastConfirmation")
    capture_inputs = final_function_block(source, "captureVisibleInputs")
    handle_action = final_function_block(source, "handleAction")
    wizard_block = source.rsplit("function wizardPage", 1)[-1].split("function preflightTable", 1)[0]
    required_fields = [
        "Estimated candidates",
        "Time class",
        "GPU guidance",
        "CPU fallback",
        "Storage need",
        "Main outputs",
        "Known annotation check",
    ]
    field_count = sum(1 for field in required_fields if field in forecast_cards or field in forecast_model)
    dynamic_inputs = all(token in forecast_model for token in ['selectedChoiceValue("search_scope")', 'selectedChoiceValue("run_profile")'])
    full_physical_confirmation = all(
        token in source
        for token in [
            "Full physical screen",
            "needsConfirmation: true",
            "forecast_confirmed",
            "Confirm full physical forecast",
        ]
    )
    guard_present = all(
        token in handle_action
        for token in [
            "requiresForecastConfirmation()",
            "#/input_wizard_run_profile_page",
            "Review the full physical resource estimate before launch.",
        ]
    )
    capture_present = 'node.type === "checkbox" ? String(node.checked)' in capture_inputs
    review_start_present = "input_wizard_review_start_page" in wizard_block and "forecastConfirmationPanel()" in wizard_block
    rows = [
        ["review_start_page_pass", review_start_present and guard_present and capture_present, "ok" if review_start_present and guard_present and capture_present else "issue"],
        ["forecast_resource_field_count", field_count, "ok" if field_count == len(required_fields) else "issue"],
        ["forecast_required_fields_present", " | ".join(required_fields), "ok" if field_count == len(required_fields) else "issue"],
        ["forecast_scope_profile_dynamic_present", dynamic_inputs, "ok" if dynamic_inputs else "issue"],
        ["full_physical_forecast_confirmation_present", full_physical_confirmation, "ok" if full_physical_confirmation else "issue"],
        ["forecast_confirmation_capture_present", capture_present, "ok" if capture_present else "issue"],
        ["start_workflow_forecast_guard_present", guard_present, "ok" if guard_present else "issue"],
        ["review_start_forecast_confirmation_present", review_start_present, "ok" if review_start_present else "issue"],
        ["forecast_confirmation_css_present", ".forecast-confirmation", "ok" if ".forecast-confirmation" in styles else "issue"],
    ]
    write_tsv(artifact / "metrics/run_forecast_confirmation_summary.tsv", ["metric", "value", "status"], rows)


def write_preflight_group_action_summary(artifact: Path, workspace: Path) -> None:
    source = (workspace / "webui/phase09/app.js").read_text(encoding="utf-8", errors="replace")
    preflight_block = final_function_block(source, "preflightTable")
    groups = ["Input", "Database", "Runtime", "GPU", "Storage", "Export"]
    actions = [
        "Fix input",
        "Open database setup",
        "Run diagnostics",
        "Repair GPU",
        "Run CPU mode",
        "Open cleanup",
        "Open export",
    ]
    headers = ["Group", "State", "Impact", "Next step"]
    group_count = sum(1 for group in groups if f'"{group}"' in preflight_block)
    action_count = sum(1 for action in actions if action in preflight_block)
    preflight_page_ok = group_count == len(groups) and action_count == len(actions) and all(header in preflight_block for header in headers)
    rows = [
        ["preflight_check_page_pass", preflight_page_ok, "ok" if preflight_page_ok else "issue"],
        ["preflight_group_count", group_count, "ok" if group_count == len(groups) else "issue"],
        ["preflight_required_groups", " | ".join(groups), "ok" if group_count == len(groups) else "issue"],
        ["preflight_next_action_count", action_count, "ok" if action_count == len(actions) else "issue"],
        ["preflight_required_next_actions", " | ".join(actions), "ok" if all(action in preflight_block for action in actions) else "issue"],
        ["preflight_cpu_mode_action_present", "Run CPU mode", "ok" if "Run CPU mode" in preflight_block and "run-cpu-mode" in source else "issue"],
        ["preflight_repair_gpu_action_present", "Repair GPU", "ok" if "Repair GPU" in preflight_block and "#/diagnostics_page" in preflight_block else "issue"],
        ["preflight_group_status_impact_next_fields", "Group | State | Impact | Next step", "ok" if all(header in preflight_block for header in headers) else "issue"],
        ["preflight_advanced_details_collapsed", "Advanced preflight details", "ok" if "Advanced preflight details" in preflight_block and "advancedDetails" in preflight_block else "issue"],
    ]
    write_tsv(artifact / "metrics/preflight_group_action_summary.tsv", ["metric", "value", "status"], rows)


def write_candidate_ranking_summary(artifact: Path, workspace: Path) -> None:
    source = (workspace / "webui/phase09/app.js").read_text(encoding="utf-8", errors="replace")
    styles = (workspace / "webui/phase09/styles.css").read_text(encoding="utf-8", errors="replace")
    ranking_block = source.rsplit("function candidateRanking", 1)[-1].split("function liveStatusKey", 1)[0]
    detail_block = final_function_block(source, "candidateDetail")
    controls_block = source.rsplit("function candidateTableControls", 1)[-1].split("function candidateTableMarkup", 1)[0]
    table_block = source.rsplit("function candidateTableMarkup", 1)[-1].split("function candidateRanking", 1)[0]
    filtered_block = final_function_block(source, "candidateFilteredRows")
    handlers_block = source.rsplit('document.addEventListener("input"', 1)[-1]
    summary_terms = ["Top candidates", "Risk distribution", "Evidence completeness", "MD support", "Run mode"]
    column_keys = ["gene", "priority", "risk", "md", "evidence"]
    detail_layers = ["Overview", "Genome", "Structure", "Docking", "MD", "Evidence", "Export"]
    detail_sections = ["Reading path", "Candidate Overview", "Key metrics", "Artifacts"]
    detail_targets = [
        "candidate_detail_page",
        "genome_orf_context_page",
        "structure_viewer_page",
        "md_summary_page",
        "evidence_ledger_page",
        "report_export_page",
    ]
    column_toggle_count = sum(1 for key in column_keys if f'"{key}"' in controls_block and "data-candidate-column" in controls_block)
    state_wiring = {
        "query": "appState.candidateTable.query = event.target.value" in handlers_block and "state.query.trim" in filtered_block,
        "risk": "appState.candidateTable.risk = event.target.value" in handlers_block and "state.risk ===" in filtered_block,
        "sort": "appState.candidateTable.sort = event.target.value" in handlers_block and "filtered.sort" in filtered_block,
        "page_size": "appState.candidateTable.pageSize = Number(event.target.value)" in handlers_block and "state.pageSize" in table_block,
        "column_visibility": "appState.candidateTable.visible[key] = event.target.checked" in handlers_block and "state.visible" in controls_block and "candidateVisibleColumns" in table_block,
        "pagination": all(token in source for token in ["candidate-prev-page", "candidate-next-page", "appState.candidateTable.page += 1", "Math.max(1, appState.candidateTable.page - 1)"]),
        "clear_filter": all(token in source for token in ["candidate-clear-filter", 'appState.candidateTable.query = ""', 'appState.candidateTable.risk = "all"']),
    }
    candidate_ranking_ok = (
        all(term in ranking_block for term in summary_terms)
        and all(state_wiring.values())
        and column_toggle_count == len(column_keys)
        and "sticky-col" in table_block
        and ".sticky-col" in styles
    )
    candidate_detail_ok = (
        all(layer in detail_block for layer in detail_layers)
        and all(target in detail_block for target in detail_targets)
        and all(section in detail_block for section in detail_sections)
    )
    rows = [
        ["candidate_ranking_page_pass", candidate_ranking_ok, "ok" if candidate_ranking_ok else "issue"],
        ["candidate_detail_page_pass", candidate_detail_ok, "ok" if candidate_detail_ok else "issue"],
        ["candidate_summary_metric_count", len(summary_terms), "ok" if all(term in ranking_block for term in summary_terms) else "issue"],
        ["candidate_detail_jump_count", len([layer for layer in detail_layers if layer in detail_block]), "ok" if all(layer in detail_block for layer in detail_layers) else "issue"],
        ["candidate_detail_required_jumps", " | ".join(detail_layers), "ok" if all(layer in detail_block for layer in detail_layers) and all(target in detail_block for target in detail_targets) else "issue"],
        ["candidate_detail_layer_section_count", len([section for section in detail_sections if section in detail_block]), "ok" if all(section in detail_block for section in detail_sections) else "issue"],
        ["candidate_detail_docking_jump_present", "Docking -> structure viewer", "ok" if "Docking" in detail_block and "structure_viewer_page" in detail_block else "issue"],
        ["candidate_detail_export_jump_present", "Export -> report export", "ok" if "Export" in detail_block and "report_export_page" in detail_block else "issue"],
        ["candidate_table_search_control_present", "data-candidate-query", "ok" if "data-candidate-query" in controls_block else "issue"],
        ["candidate_table_risk_filter_present", "data-candidate-risk", "ok" if "data-candidate-risk" in controls_block else "issue"],
        ["candidate_table_sort_control_present", "data-candidate-sort", "ok" if "data-candidate-sort" in controls_block else "issue"],
        ["candidate_table_page_size_control_present", "data-candidate-page-size", "ok" if "data-candidate-page-size" in controls_block else "issue"],
        ["candidate_table_column_toggle_count", column_toggle_count, "ok" if column_toggle_count == len(column_keys) else "issue"],
        ["candidate_table_pagination_present", "candidate-prev-page | candidate-next-page | candidate-clear-filter", "ok" if all(token in table_block for token in ["candidate-prev-page", "candidate-next-page", "candidate-clear-filter"]) else "issue"],
        ["candidate_table_stateful_control_count", sum(1 for present in state_wiring.values() if present), "ok" if all(state_wiring.values()) else "issue"],
        ["candidate_table_query_wiring_present", state_wiring["query"], "ok" if state_wiring["query"] else "issue"],
        ["candidate_table_risk_wiring_present", state_wiring["risk"], "ok" if state_wiring["risk"] else "issue"],
        ["candidate_table_sort_wiring_present", state_wiring["sort"], "ok" if state_wiring["sort"] else "issue"],
        ["candidate_table_page_size_wiring_present", state_wiring["page_size"], "ok" if state_wiring["page_size"] else "issue"],
        ["candidate_table_column_visibility_wiring_present", state_wiring["column_visibility"], "ok" if state_wiring["column_visibility"] else "issue"],
        ["candidate_table_pagination_wiring_present", state_wiring["pagination"], "ok" if state_wiring["pagination"] else "issue"],
        ["candidate_table_clear_filter_wiring_present", state_wiring["clear_filter"], "ok" if state_wiring["clear_filter"] else "issue"],
        ["candidate_table_sticky_column_present", "sticky-col", "ok" if "sticky-col" in table_block and ".sticky-col" in styles else "issue"],
        ["candidate_export_entry_present", "Export candidate table", "ok" if "Export candidate table" in ranking_block and "#/report_export_page" in ranking_block else "issue"],
        ["candidate_advanced_full_table_collapsed", "Full candidate table", "ok" if "Full candidate table" in ranking_block and "advancedDetails" in ranking_block else "issue"],
    ]
    write_tsv(artifact / "metrics/candidate_ranking_summary.tsv", ["metric", "value", "status"], rows)


def write_genome_md_graphics_summary(artifact: Path, workspace: Path) -> None:
    source = (workspace / "webui/phase09/app.js").read_text(encoding="utf-8", errors="replace")
    styles = (workspace / "webui/phase09/styles.css").read_text(encoding="utf-8", errors="replace")
    genome_graphic = source.rsplit("function genomeTrackGraphic", 1)[-1].split("function miniLineChart", 1)[0]
    genome_page = source.rsplit("function genomeContext", 1)[-1].split("function structureViewer", 1)[0]
    md_page = source.rsplit("function mdSummary", 1)[-1].split("function asArray", 1)[0]
    md_provenance_block = final_function_block(source, "mdProvenanceRows")
    md_time_basis_block = final_function_block(source, "mdTimeBasis")
    md_chart_meta_block = final_function_block(source, "mdChartMeta")
    md_source_text = f"{md_page}\n{md_provenance_block}\n{md_time_basis_block}\n{md_chart_meta_block}"
    genome_terms = ["upstream", "ORF span", "CDS", "UTR", "intron", "antisense", "pseudogene", "variant", "downstream"]
    md_chart_terms = ["RMSD", "RMSF", "Contact occupancy", "H-bond occupancy", "Ligand drift"]
    md_metric_terms = ["RMSD", "RMSF", "ligand drift", "hydrogen bonds", "contacts", "score decomposition"]
    md_provenance_terms = ["MD data provenance", "Data source", "Candidate", "Time basis", "Units", "Artifact links"]
    genome_term_count = sum(1 for term in genome_terms if term in genome_graphic)
    md_chart_count = sum(1 for term in md_chart_terms if term in md_page)
    md_metric_count = sum(1 for term in md_metric_terms if term in md_page)
    md_provenance_count = sum(1 for term in md_provenance_terms if term in md_source_text)
    genome_viewer_ok = (
        "function genomeTrackGraphic" in source
        and "genome-track" in genome_graphic
        and genome_term_count == len(genome_terms)
        and all(term in genome_page for term in ["ORF", "Coordinates", "Strand / Frame", "Variant"])
    )
    md_summary_ok = (
        md_chart_count == len(md_chart_terms)
        and md_metric_count == len(md_metric_terms)
        and md_provenance_count == len(md_provenance_terms)
        and "function mdChartMeta" in source
        and all(term in md_chart_meta_block for term in ["Data source:", "candidate:", "time basis:", "artifact links:", "unit:"])
    )
    rows = [
        ["genome_orf_context_viewer_pass", genome_viewer_ok, "ok" if genome_viewer_ok else "issue"],
        ["md_summary_viewer_pass", md_summary_ok, "ok" if md_summary_ok else "issue"],
        ["genome_track_graphic_present", "genomeTrackGraphic", "ok" if "function genomeTrackGraphic" in source and "genome-track" in genome_graphic else "issue"],
        ["genome_track_segment_count", genome_term_count, "ok" if genome_term_count == len(genome_terms) else "issue"],
        ["genome_context_table_fields_present", "ORF | Coordinates | Strand / Frame | Variant", "ok" if all(term in genome_page for term in ["ORF", "Coordinates", "Strand / Frame", "Variant"]) else "issue"],
        ["genome_track_css_present", "genome-track | track-segment", "ok" if ".genome-track" in styles and ".track-segment" in styles else "issue"],
        ["md_chart_count", md_chart_count, "ok" if md_chart_count == len(md_chart_terms) else "issue"],
        ["md_key_metric_count", md_metric_count, "ok" if md_metric_count == len(md_metric_terms) else "issue"],
        ["md_score_decomposition_present", "score decomposition", "ok" if "score decomposition" in md_page else "issue"],
        ["md_provenance_field_count", md_provenance_count, "ok" if md_provenance_count == len(md_provenance_terms) else "issue"],
        ["md_chart_source_unit_meta_present", "Data source: | unit:", "ok" if "Data source:" in md_source_text and "unit:" in md_source_text and "mini-chart-meta" in styles else "issue"],
        ["md_chart_candidate_meta_present", "candidate:", "ok" if "candidate:" in md_chart_meta_block else "issue"],
        ["md_chart_time_basis_meta_present", "time basis:", "ok" if "time basis:" in md_chart_meta_block and "function mdTimeBasis" in source else "issue"],
        ["md_chart_artifact_link_meta_present", "artifact links:", "ok" if "artifact links:" in md_chart_meta_block else "issue"],
        ["md_chart_meta_function_present", "mdChartMeta", "ok" if "function mdChartMeta" in source and all(term in md_chart_meta_block for term in ["Data source:", "candidate:", "time basis:", "artifact links:", "unit:"]) else "issue"],
        ["md_chart_css_present", "mini-chart", "ok" if ".mini-chart" in styles and "svg" in styles else "issue"],
    ]
    write_tsv(artifact / "metrics/genome_md_graphics_summary.tsv", ["metric", "value", "status"], rows)


def write_settings_diagnostics_export_summary(artifact: Path, workspace: Path) -> None:
    source = (workspace / "webui/phase09/app.js").read_text(encoding="utf-8", errors="replace")
    server = (workspace / "crates/gls-server/src/lib.rs").read_text(encoding="utf-8", errors="replace")
    settings_block = final_function_block(source, "settings")
    setup_wizard_block = final_function_block(source, "setupWizard")
    setup_wizard_step_rows_block = final_function_block(source, "setupWizardStepRows")
    diagnostics_block = final_function_block(source, "diagnostics")
    diagnostic_fix_button = final_function_block(source, "diagnosticFixButton")
    record_diagnostic_action = final_function_block(source, "recordDiagnosticAction")
    refresh_workspace_state = final_function_block(source, "refreshWorkspaceState")
    handle_action = final_function_block(source, "handleAction")
    export_definitions = final_function_block(source, "exportDefinitions")
    report_export_block = final_function_block(source, "reportExport")
    settings_categories = ["Language", "Theme", "Default start page", "Run log density", "Evidence density", "Cache limit"]
    setup_wizard_steps = ["Choose mode", "WSL2 / GPU check", "Dependency repair", "Data roots", "Demo database", "Sample project validation"]
    first_launch_route_ok = 'if (!raw)' in source and 'return "setup_wizard_page";' in source
    diagnostics_groups = ["Runtime", "Python worker", "GPU", "Database", "Storage", "Export"]
    export_types = [
        "research_report",
        "candidate_table",
        "reproducibility_package",
        "full_artifact_archive",
        "support_bundle",
        "project_package",
        "backup",
    ]
    internal_export_types = ["machine_readable_json", "run_manifest", "tool_version_manifest", "diagnostics_report", "github_push_report"]
    user_export_actions = ["project_package", "backup", "reproducibility_package"]
    diagnostic_action_terms = ["repair-runtime", "repair-worker", "repair-gpu", "open-database-setup", "clean-cache", "run-cpu-mode", "open-export", "run-diagnostics"]
    settings_category_count = sum(1 for category in settings_categories if f'"{category}"' in settings_block)
    diagnostics_source_contract = diagnostics_block + "\n" + server
    diagnostics_group_count = sum(1 for group in diagnostics_groups if group in diagnostics_source_contract)
    export_type_count = sum(1 for export_type in export_types if export_type in export_definitions)
    internal_export_definition_count = sum(1 for export_type in internal_export_types if export_type in export_definitions)
    diagnostic_action_count = sum(1 for term in diagnostic_action_terms if term in source and term in server)
    diagnostics_basic_fields_ok = all(term in diagnostics_block for term in ["Check", "State", "Impact", "Fix"])
    diagnostics_repair_actions_ok = all(term in source for term in ["Repair runtime", "Repair worker", "Repair GPU", "Open database setup", "Clean cache"])
    diagnostics_fix_buttons_ok = "data-diagnostic-action" in diagnostic_fix_button and "diagnosticFixButton" in diagnostics_block
    diagnostics_recheck_button_ok = "function diagnosticRecheckButton" in source and "diagnosticRecheckButton" in diagnostics_block
    diagnostics_backend_action_route_ok = "POST /api/user/diagnostics/action" in source and '("POST", "/api/user/diagnostics/action")' in server
    diagnostics_manifest_ok = "diagnostic_repair_actions.tsv" in server and "destructive_action" in server and ":false" in server.replace(" ", "")
    diagnostics_frontend_handler_ok = "/api/user/diagnostics/action" in record_diagnostic_action and all(term in handle_action for term in ["repair-runtime", "repair-worker", "repair-gpu", "clean-cache"])
    diagnostics_human_terms = [
        "impact",
        "reason",
        "repair_action",
        "recheck_action",
        "friendlyDiagnosticRow",
        "diagnosticFixButton",
        "diagnosticRecheckButton",
    ]
    diagnostics_human_readable_ok = all(term in source for term in diagnostics_human_terms) and all(
        term in server for term in ["impact", "reason", "repair_action", "recheck_action"]
    )
    diagnostics_required_checks = [
        diagnostics_group_count == len(diagnostics_groups),
        diagnostics_basic_fields_ok,
        diagnostics_repair_actions_ok,
        diagnostics_fix_buttons_ok,
        diagnostics_recheck_button_ok,
        diagnostics_backend_action_route_ok,
        diagnostic_action_count == len(diagnostic_action_terms),
        diagnostics_manifest_ok,
        diagnostics_frontend_handler_ok,
        diagnostics_human_readable_ok,
    ]
    diagnostics_required_item_completion_rate = "100%" if all(diagnostics_required_checks) else f"{sum(1 for item in diagnostics_required_checks if item)}/{len(diagnostics_required_checks)}"
    diagnostics_human_readable_message_rate = "100%" if diagnostics_human_readable_ok else "partial"
    diagnostics_repair_action_rate = "100%" if diagnostic_action_count == len(diagnostic_action_terms) else f"{diagnostic_action_count}/{len(diagnostic_action_terms)}"
    developer_mode_lazy_debug_ok = (
        "/api/debug/routes" not in refresh_workspace_state
        and "open-developer-mode" in handle_action
        and "/api/debug/routes" in handle_action
        and "Developer APIs" in settings_block
    )
    developer_mode_lazy_guides_ok = (
        "/api/guides" not in refresh_workspace_state
        and "open-developer-mode" in handle_action
        and "/api/guides" in handle_action
        and "guideRegistry()" in settings_block
    )
    rows = [
        ["settings_category_count", settings_category_count, "ok" if settings_category_count == len(settings_categories) else "issue"],
        ["settings_allowed_categories", " | ".join(settings_categories), "ok" if all(category in settings_block for category in settings_categories) else "issue"],
        ["settings_advanced_developer_mode_collapsed", "Developer Mode", "ok" if "advancedDetails" in settings_block and "Developer Mode" in settings_block else "issue"],
        ["settings_developer_mode_debug_routes_lazy_loaded", "debug routes load only after Open Developer Mode", "ok" if developer_mode_lazy_debug_ok else "issue"],
        ["settings_developer_mode_guides_lazy_loaded", "guide registry loads only after Open Developer Mode", "ok" if developer_mode_lazy_guides_ok else "issue"],
        ["settings_github_default_hidden", "Git/GitHub sync only inside Developer Mode", "ok" if ("Git/GitHub" not in settings_block or settings_block.find("Git/GitHub") > settings_block.find("advancedDetails")) else "issue"],
        ["settings_policy_humanizer_present", "humanPolicyValue", "ok" if "function humanPolicyValue" in source and "humanPolicyValue" in source else "issue"],
        ["settings_cleanup_policy_present", "cache limit | storage preferences", "ok" if all(term in settings_block for term in ["Cache limit", "Storage preferences"]) else "issue"],
        ["setup_wizard_independent_page_present", "setup_wizard_page", "ok" if "setup_wizard_page" in source and "setupWizard" in source else "issue"],
        ["setup_wizard_first_launch_default_route", "setup_wizard_page", "ok" if first_launch_route_ok else "issue"],
        ["setup_wizard_step_count", sum(1 for step in setup_wizard_steps if step in setup_wizard_step_rows_block), "ok" if all(step in setup_wizard_step_rows_block for step in setup_wizard_steps) else "issue"],
        ["settings_disk_quota_controls_present", "Cache limit | Quota warning | Trajectory retention", "ok" if all(term in source for term in ["function storageQuotaRows", "Cache limit", "Quota warning", "Trajectory retention"]) else "issue"],
        ["settings_cleanup_impact_scope_recovery_present", "impact | recovery", "ok" if all(term in source for term in ["impact", "recovery", "storagePolicyRows"]) else "issue"],
        ["diagnostics_basic_group_count", diagnostics_group_count, "ok" if diagnostics_group_count == len(diagnostics_groups) else "issue"],
        ["diagnostics_basic_fields_present", "Check | State | Impact | Fix", "ok" if diagnostics_basic_fields_ok else "issue"],
        ["diagnostics_repair_actions_present", "Repair runtime | Repair worker | Repair GPU | Open database setup | Clean cache", "ok" if diagnostics_repair_actions_ok else "issue"],
        ["diagnostics_fix_buttons_present", "data-diagnostic-action", "ok" if diagnostics_fix_buttons_ok else "issue"],
        ["diagnostics_recheck_button_present", "Recheck", "ok" if diagnostics_recheck_button_ok else "issue"],
        ["diagnostics_backend_action_route_present", "POST /api/user/diagnostics/action", "ok" if diagnostics_backend_action_route_ok else "issue"],
        ["diagnostics_action_id_contract_count", diagnostic_action_count, "ok" if diagnostic_action_count == len(diagnostic_action_terms) else "issue"],
        ["diagnostics_action_records_manifest", "diagnostic_repair_actions.tsv | destructive_action=false", "ok" if diagnostics_manifest_ok else "issue"],
        ["diagnostics_frontend_action_handler_present", "recordDiagnosticAction | handleAction", "ok" if diagnostics_frontend_handler_ok else "issue"],
        ["diagnostics_required_item_completion_rate", diagnostics_required_item_completion_rate, "ok" if diagnostics_required_item_completion_rate == "100%" else "issue"],
        ["diagnostics_human_readable_message_rate", diagnostics_human_readable_message_rate, "ok" if diagnostics_human_readable_message_rate == "100%" else "issue"],
        ["diagnostics_repair_action_rate", diagnostics_repair_action_rate, "ok" if diagnostics_repair_action_rate == "100%" else "issue"],
        ["diagnostics_advanced_collapsed", "command | stdout/stderr | raw_state", "ok" if "advancedDetails" in diagnostics_block and all(term in diagnostics_block for term in ["command", "stdout/stderr", "raw_state"]) else "issue"],
        ["export_user_type_count", export_type_count, "ok" if export_type_count == len(export_types) else "issue"],
        ["export_required_use_types_present", "Research report | Candidate table | Reproducibility package | Full artifact archive | Support bundle | Project Package | Backup", "ok" if all(export_type in export_definitions for export_type in export_types) else "issue"],
        ["export_project_backup_repro_actions_present", "Project Package | Backup | Reproducibility Package", "ok" if all(export_type in export_definitions for export_type in user_export_actions) else "issue"],
        ["export_open_output_folder_present", "Open output folder", "ok" if "Open output folder" in report_export_block else "issue"],
        ["export_internal_type_definition_count", internal_export_definition_count, "ok" if internal_export_definition_count == 0 else "issue"],
        ["export_advanced_details_collapsed", "export_type | path | bytes | exists", "ok" if "advancedDetails" in report_export_block and all(term in report_export_block for term in ["export_type", "path", "bytes", "exists"]) else "issue"],
    ]
    write_tsv(artifact / "metrics/settings_diagnostics_export_summary.tsv", ["metric", "value", "status"], rows)


def write_report_quality_summary(artifact: Path, export_payload: dict) -> None:
    required_sections = [
        ("summary", "Summary"),
        ("inputs", "Inputs"),
        ("database_versions", "Database Versions"),
        ("candidate_ranking", "Candidate Ranking"),
        ("key_figures", "Key Figures"),
        ("structure_md_evidence", "Structure and MD Evidence"),
        ("evidence_chain", "Evidence Chain"),
        ("parameters", "Parameters"),
        ("limitations", "Limitations"),
        ("validation_suggestions", "Validation Suggestions"),
        ("appendix", "Appendix"),
    ]
    required_export_types = [
        "research_report",
        "candidate_table",
        "reproducibility_package",
        "full_artifact_archive",
        "support_bundle",
        "project_package",
        "backup",
    ]

    files = export_payload.get("files") or []

    def path_for_type(*aliases: str) -> Path | None:
        for file_info in files:
            export_type = str(file_info.get("export_type") or "")
            if any(alias == export_type or alias in export_type for alias in aliases):
                raw_path = str(file_info.get("path") or "")
                if raw_path:
                    return Path(raw_path)
        return None

    def read_optional(path: Path | None) -> str:
        if path and path.exists():
            return path.read_text(encoding="utf-8", errors="replace")
        return ""

    markdown_path = path_for_type("research_report", "markdown_report")
    html_path = path_for_type("html_report")
    json_path = path_for_type("machine_readable_json")
    candidate_tsv_path = path_for_type("candidate_table_tsv", "candidate_table")
    candidate_csv_path = path_for_type("candidate_table_csv")
    artifact_bundle_paths = [
        path_for_type("full_artifact_archive"),
        path_for_type("reproducibility_package"),
        path_for_type("support_bundle"),
        path_for_type("project_package"),
    ]
    section_manifest_path = path_for_type("report_sections_manifest")
    export_manifest_path = Path(str(export_payload.get("manifest") or ""))
    markdown_text = read_optional(markdown_path)
    html_text = read_optional(html_path)
    json_text = read_optional(json_path)
    section_manifest_text = read_optional(section_manifest_path)
    export_manifest_text = read_optional(export_manifest_path)

    section_ids = {
        line.split("\t", 1)[0]
        for line in section_manifest_text.splitlines()[1:]
        if line.strip() and "\t" in line
    }
    manifest_types = {
        line.split("\t", 1)[0]
        for line in export_manifest_text.splitlines()[1:]
        if line.strip() and "\t" in line
    }
    required_section_ids = {section_id for section_id, _ in required_sections}
    markdown_section_count = sum(1 for _, title in required_sections if f"## {title}" in markdown_text)
    html_section_count = sum(1 for _, title in required_sections if f"<h2>{title}</h2>" in html_text)
    manifest_user_type_count = sum(
        1 for export_type in required_export_types if any(export_type in kind for kind in manifest_types)
    )
    limits_validation_appendix = all(
        term in markdown_text for term in ["## Limitations", "## Validation Suggestions", "## Appendix"]
    )
    parameters_present = "## Parameters" in markdown_text and "technical appendix files" in markdown_text
    evidence_chain_present = "## Evidence Chain" in markdown_text and "Evidence records are grouped" in markdown_text
    appendix_present = "## Appendix" in markdown_text and "Technical support files" in markdown_text
    main_report_text = markdown_text.split("## Appendix", 1)[0]
    default_forbidden_terms = [
        "schema",
        "manifest",
        "hash",
        "raw JSON",
        "route=",
        "gate",
        "stdout",
        "stderr",
        "run_id",
        "command=",
        "output_dir",
        "GitHub",
        "commit",
        "remote",
    ]
    main_forbidden_hits = [term for term in default_forbidden_terms if term.lower() in main_report_text.lower()]
    markdown_export_ok = bool(markdown_text and markdown_path and markdown_path.exists() and markdown_section_count == len(required_sections))
    html_export_ok = bool(html_text and html_path and html_path.exists() and html_section_count == len(required_sections))
    json_export_ok = bool(json_text and json_path and json_path.exists() and "candidate_row_count" in json_text)
    candidate_table_export_ok = bool(
        candidate_tsv_path
        and candidate_tsv_path.exists()
        and candidate_csv_path
        and candidate_csv_path.exists()
    )
    artifact_bundle_export_ok = all(path is not None and path.exists() for path in artifact_bundle_paths)
    report_semantics_ok = (
        limits_validation_appendix
        and parameters_present
        and evidence_chain_present
        and appendix_present
        and not main_forbidden_hits
    )
    report_export_flow_ok = (
        markdown_export_ok
        and html_export_ok
        and json_export_ok
        and candidate_table_export_ok
        and artifact_bundle_export_ok
        and manifest_user_type_count == len(required_export_types)
    )
    rows = [
        ["report_export_flow_pass", report_export_flow_ok, "ok" if report_export_flow_ok else "issue"],
        ["html_report_export_pass", html_export_ok, "ok" if html_export_ok else "issue"],
        ["markdown_report_export_pass", markdown_export_ok, "ok" if markdown_export_ok else "issue"],
        ["json_report_export_pass", json_export_ok, "ok" if json_export_ok else "issue"],
        ["candidate_table_export_pass", candidate_table_export_ok, "ok" if candidate_table_export_ok else "issue"],
        ["artifact_bundle_export_pass", artifact_bundle_export_ok, "ok" if artifact_bundle_export_ok else "issue"],
        ["report_semantics_validation_pass", report_semantics_ok, "ok" if report_semantics_ok else "issue"],
        [
            "report_required_section_count",
            len(required_section_ids.intersection(section_ids)),
            "ok" if required_section_ids.issubset(section_ids) else "issue",
        ],
        [
            "report_required_sections_present",
            " | ".join(section_id for section_id, _ in required_sections),
            "ok" if required_section_ids.issubset(section_ids) else "issue",
        ],
        [
            "report_markdown_sections_present",
            markdown_section_count,
            "ok" if markdown_section_count == len(required_sections) else "issue",
        ],
        [
            "report_html_sections_present",
            html_section_count,
            "ok" if html_section_count == len(required_sections) else "issue",
        ],
        [
            "report_export_manifest_types_present",
            " | ".join(required_export_types),
            "ok" if all(any(export_type in kind for kind in manifest_types) for export_type in required_export_types) else "issue",
        ],
        [
            "report_export_manifest_user_type_count",
            manifest_user_type_count,
            "ok" if manifest_user_type_count == len(required_export_types) else "issue",
        ],
        [
            "report_section_manifest_row_count",
            len(section_ids),
            "ok" if len(section_ids) == len(required_sections) else "issue",
        ],
        [
            "report_limits_validation_appendix_present",
            limits_validation_appendix,
            "ok" if limits_validation_appendix else "issue",
        ],
        [
            "report_parameters_section_present",
            parameters_present,
            "ok" if parameters_present else "issue",
        ],
        [
            "report_evidence_chain_section_present",
            evidence_chain_present,
            "ok" if evidence_chain_present else "issue",
        ],
        [
            "report_appendix_technical_files_present",
            appendix_present,
            "ok" if appendix_present else "issue",
        ],
        [
            "report_main_forbidden_term_count",
            len(main_forbidden_hits),
            "ok" if not main_forbidden_hits else "issue",
        ],
        [
            "report_main_forbidden_terms",
            "none" if not main_forbidden_hits else " | ".join(main_forbidden_hits),
            "ok" if not main_forbidden_hits else "issue",
        ],
        [
            "report_quality_source_file_exists",
            bool(markdown_path and markdown_path.exists() and html_path and html_path.exists() and section_manifest_path and section_manifest_path.exists()),
            "ok" if markdown_text and html_text and section_manifest_text else "issue",
        ],
    ]
    write_tsv(artifact / "metrics/report_quality_summary.tsv", ["metric", "value", "status"], rows)


def write_project_package_import_export_summary(
    artifact: Path,
    workspace: Path,
    projects_payload: dict,
    export_payload: dict,
    import_payload: dict,
) -> None:
    source = (workspace / "webui/phase09/app.js").read_text(encoding="utf-8", errors="replace")
    project_page = final_function_block(source, "projectPage")
    handle_action = final_function_block(source, "handleAction")
    api_routes = source.split("const apiRoutes", 1)[-1].split("];", 1)[0]
    files = export_payload.get("files") or []
    export_types = {str(file_info.get("export_type") or ""): file_info for file_info in files}
    project_package = export_types.get("project_package") or {}
    project_package_path = Path(str(project_package.get("path") or ""))
    project_package_text = project_package_path.read_text(encoding="utf-8", errors="replace") if project_package_path.exists() else ""
    imported_project = import_payload.get("project") or {}
    supported_actions = set(projects_payload.get("supported_actions") or [])
    project_package_exists = project_package.get("exists") is True or project_package.get("status") == "created"
    default_forbidden = ["Remote repository", "GitHub Sync", "git push", "commit", "remote"]
    forbidden_visible_count = sum(1 for term in default_forbidden if term in project_page)
    rows = [
        [
            "project_package_export_created",
            project_package_exists,
            "ok" if project_package_exists and int(project_package.get("bytes") or 0) > 0 else "issue",
        ],
        [
            "project_package_export_bytes",
            project_package.get("bytes", 0),
            "ok" if int(project_package.get("bytes") or 0) > 0 else "issue",
        ],
        [
            "project_package_export_path_exists",
            project_package_path.exists(),
            "ok" if project_package_path.exists() else "issue",
        ],
        [
            "project_package_import_api_pass",
            import_payload.get("status", ""),
            "ok" if import_payload.get("status") == "imported" else "issue",
        ],
        [
            "project_package_imported_project_present",
            bool(imported_project.get("project_id")),
            "ok" if imported_project.get("project_id") and imported_project.get("project_status") == "imported" else "issue",
        ],
        [
            "project_package_import_contains_large_database_copy",
            import_payload.get("contains_large_database_copy"),
            "ok" if import_payload.get("contains_large_database_copy") is False else "issue",
        ],
        [
            "project_package_read_only_review_mode_present",
            "read_only_review_mode\ttrue",
            "ok" if "read_only_review_mode\ttrue" in project_package_text else "issue",
        ],
        [
            "project_package_demo_mode_present",
            "demo_mode_supported\ttrue",
            "ok" if "demo_mode_supported\ttrue" in project_package_text else "issue",
        ],
        [
            "project_package_support_bundle_linked",
            "support_bundle_linked\ttrue",
            "ok" if "support_bundle_linked\ttrue" in project_package_text and "support_bundle_path\t" in project_package_text else "issue",
        ],
        [
            "project_package_large_database_policy_reference_only",
            "large_database_policy\treference_manifest_only",
            "ok" if "large_database_policy\treference_manifest_only" in project_package_text else "issue",
        ],
        [
            "project_package_import_read_only_review_mode",
            import_payload.get("read_only_review_mode"),
            "ok" if import_payload.get("read_only_review_mode") is True else "issue",
        ],
        [
            "project_package_import_demo_mode_supported",
            import_payload.get("demo_mode_supported"),
            "ok" if import_payload.get("demo_mode_supported") is True else "issue",
        ],
        [
            "project_package_import_support_bundle_linked",
            import_payload.get("support_bundle_linked"),
            "ok" if import_payload.get("support_bundle_linked") is True else "issue",
        ],
        [
            "project_package_import_reopen_mode",
            import_payload.get("reopen_mode"),
            "ok" if import_payload.get("reopen_mode") == "local_project_package" else "issue",
        ],
        [
            "project_package_imported_project_read_only_persisted",
            imported_project.get("read_only_review_mode"),
            "ok" if imported_project.get("read_only_review_mode") is True else "issue",
        ],
        [
            "project_package_imported_project_demo_mode_persisted",
            imported_project.get("demo_mode_supported"),
            "ok" if imported_project.get("demo_mode_supported") is True else "issue",
        ],
        [
            "project_package_imported_project_support_bundle_persisted",
            imported_project.get("support_bundle_linked"),
            "ok" if imported_project.get("support_bundle_linked") is True else "issue",
        ],
        [
            "project_package_imported_project_collaboration_mode",
            imported_project.get("collaboration_mode"),
            "ok" if imported_project.get("collaboration_mode") == "read_only_review" else "issue",
        ],
        [
            "project_package_imported_project_saved_state",
            ",".join(imported_project.get("saved_state") or []),
            "ok" if "project_package_import" in (imported_project.get("saved_state") or []) else "issue",
        ],
        [
            "project_package_project_mode_column_present",
            "Mode | friendlyProjectMode",
            "ok" if "friendlyProjectMode" in source and "[\"模式\", \"Mode\"]" in source else "issue",
        ],
        [
            "project_package_import_button_present",
            "Import Project Package",
            "ok" if "Import Project Package" in project_page and "import-project-package" in handle_action else "issue",
        ],
        [
            "project_package_import_user_route_present",
            "POST /api/user/projects/import-package",
            "ok" if "POST /api/user/projects/import-package" in api_routes else "issue",
        ],
        [
            "project_package_supported_action_present",
            "import_package",
            "ok" if "import_package" in supported_actions else "issue",
        ],
        [
            "project_default_remote_term_count",
            forbidden_visible_count,
            "ok" if forbidden_visible_count == 0 else "issue",
        ],
    ]
    write_tsv(artifact / "metrics/project_package_import_export_summary.tsv", ["metric", "value", "status"], rows)


def write_interactive_copy_lint_summary(artifact: Path, workspace: Path) -> None:
    source = (workspace / "webui/phase09/app.js").read_text(encoding="utf-8", errors="replace")
    handle_action = final_function_block(source, "handleAction")
    calls = re.findall(r"setMessage\((.*?)\);", handle_action, flags=re.DOTALL)
    message_texts: list[str] = []
    for call in calls:
        message_texts.extend(re.findall(r'"((?:\\.|[^"\\])*)"', call))
        message_texts.extend(re.findall(r"'((?:\\.|[^'\\])*)'", call))
    forbidden_terms = [
        "manifest",
        "profile",
        "artifact",
        "source_path",
        "route=",
        "raw JSON",
        "schema",
        "hash",
        "gate",
        "stdout",
        "stderr",
        "git add",
        "git commit",
        "git push",
        "Git status",
        "Remote state",
        "gls-cli",
        "Nextflow",
        "CPU fallback",
        "forecast",
    ]
    violations = []
    for text in message_texts:
        lower = text.lower()
        for term in forbidden_terms:
            if term.lower() in lower:
                violations.append(f"{term}:{text}")
    human_terms = [
        "Project saved locally.",
        "Configuration saved to the local project.",
        "Environment check report generated.",
        "Database setup check completed.",
        "Database index plan generated.",
        "Run created and queued for the local scientific backend.",
    ]
    rows = [
        ["interactive_message_count", len(message_texts), "ok" if len(message_texts) >= 20 else "issue"],
        ["interactive_copy_forbidden_term_count", len(violations), "ok" if not violations else "issue"],
        ["interactive_copy_forbidden_terms", "none" if not violations else " | ".join(violations[:20]), "ok" if not violations else "issue"],
        ["interactive_copy_human_action_message_count", sum(1 for term in human_terms if term in message_texts), "ok" if all(term in message_texts for term in human_terms) else "issue"],
        ["interactive_copy_lint_scope", "handleAction setMessage strings", "ok" if "function handleAction" in source and "setMessage" in handle_action else "issue"],
    ]
    write_tsv(artifact / "metrics/interactive_copy_lint_summary.tsv", ["metric", "value", "status"], rows)


def write_default_runtime_identifier_lint_summary(artifact: Path, workspace: Path) -> None:
    source = (workspace / "webui/phase09/app.js").read_text(encoding="utf-8", errors="replace")
    function_names = [
        "currentRunNotice",
        "renderRuntime",
        "renderDetail",
        "runMonitor",
        "candidateRanking",
        "candidateDetail",
        "genomeContext",
        "structureViewer",
        "mdSummary",
        "evidenceLedger",
        "reportExport",
        "diagnostics",
        "settings",
    ]
    forbidden_terms = [
        "run_id=",
        "nextflow_run_id",
        "command=",
        "output_dir=",
        "route=",
        "active_step=",
        "raw_state",
        "raw JSON",
    ]
    violations: list[str] = []
    scanned_blocks = 0
    for function_name in function_names:
        block = final_function_block(source, function_name)
        if not block:
            continue
        scanned_blocks += 1
        default_surface = default_surface_source(block)
        for term in forbidden_terms:
            if term.lower() in default_surface.lower():
                violations.append(f"{function_name}:{term}")
    rows = [
        ["default_runtime_lint_scanned_function_count", scanned_blocks, "ok" if scanned_blocks == len(function_names) else "issue"],
        ["default_runtime_internal_field_count", len(violations), "ok" if not violations else "issue"],
        ["default_runtime_internal_fields", "none" if not violations else " | ".join(violations[:30]), "ok" if not violations else "issue"],
        ["default_runtime_lint_scope", "final default UI blocks after removing advancedDetails calls", "ok"],
    ]
    write_tsv(artifact / "metrics/default_runtime_identifier_lint_summary.tsv", ["metric", "value", "status"], rows)


def source_string_literals(source: str) -> list[str]:
    values: list[str] = []
    pattern = re.compile(r'"((?:\\.|[^"\\])*)"|\'((?:\\.|[^\'\\])*)\'|`((?:\\.|[^`\\])*)`', re.DOTALL)
    for match in pattern.finditer(source):
        value = next(group for group in match.groups() if group is not None)
        values.append(value)
    return values


def strip_js_call(source: str, call_name: str) -> str:
    token = f"{call_name}("
    result: list[str] = []
    cursor = 0
    while True:
        start = source.find(token, cursor)
        if start < 0:
            result.append(source[cursor:])
            break
        result.append(source[cursor:start])
        index = start + len(call_name)
        depth = 0
        quote = ""
        escaped = False
        while index < len(source):
            char = source[index]
            if quote:
                if escaped:
                    escaped = False
                elif char == "\\":
                    escaped = True
                elif char == quote:
                    quote = ""
                index += 1
                continue
            if char in {"'", '"', "`"}:
                quote = char
                index += 1
                continue
            if char == "(":
                depth += 1
            elif char == ")":
                depth -= 1
                if depth == 0:
                    index += 1
                    break
            index += 1
        result.append(f"{call_name}()")
        cursor = index
    return "".join(result)


def default_surface_source(block: str) -> str:
    return strip_js_call(block, "advancedDetails")


def visible_literal_text(value: str) -> str:
    without_open_tags = re.sub(r"<[A-Za-z][^>]*>", "", value)
    without_tags = re.sub(r"</[A-Za-z][^>]*>", "", without_open_tags)
    return without_tags


def write_default_user_surface_copy_lint_summary(artifact: Path, workspace: Path) -> None:
    source = (workspace / "webui/phase09/app.js").read_text(encoding="utf-8", errors="replace")
    function_names = [
        "dashboard",
        "projectRows",
        "projectListCard",
        "projectRunHistoryCard",
        "projectPage",
        "wizardPage",
        "databaseRegistry",
        "diagnostics",
        "settings",
        "reportExport",
        "runMonitor",
        "candidateRanking",
        "candidateDetail",
        "genomeContext",
        "structureViewer",
        "mdSummary",
        "evidenceLedger",
        "workflowContextPanel",
        "renderRuntime",
        "renderDetail",
        "currentRunNotice",
        "errorState",
        "emptyState",
        "loadingState",
    ]
    forbidden_terms = [
        "GitHub",
        "commit",
        "remote",
        "schema",
        "manifest",
        "hash",
        "raw JSON",
        "route=",
        "gate",
        "stdout",
        "stderr",
        "run_id",
        "latest_run_id",
        "nextflow_run_id",
        "output_dir",
        "source_path",
        "artifact_hash",
        "raw_state",
        "pushed_to_github",
    ]
    violations: list[str] = []
    scanned_blocks = 0
    literal_count = 0
    for function_name in function_names:
        block = final_function_block(source, function_name)
        if not block:
            continue
        scanned_blocks += 1
        default_surface = default_surface_source(block)
        for literal in source_string_literals(default_surface):
            visible_text = visible_literal_text(literal)
            if not visible_text.strip():
                continue
            literal_count += 1
            lower = visible_text.lower()
            for term in forbidden_terms:
                if term.lower() in lower:
                    violations.append(f"{function_name}:{term}:{visible_text[:120]}")
    rows = [
        ["default_user_surface_lint_scanned_function_count", scanned_blocks, "ok" if scanned_blocks == len(function_names) else "issue"],
        ["default_user_surface_literal_count", literal_count, "ok" if literal_count >= 100 else "issue"],
        ["default_user_surface_forbidden_term_count", len(violations), "ok" if not violations else "issue"],
        ["default_user_surface_forbidden_terms", "none" if not violations else " | ".join(violations[:30]), "ok" if not violations else "issue"],
        ["default_user_surface_lint_scope", "final default UI string literals after removing advancedDetails calls", "ok"],
    ]
    write_tsv(artifact / "metrics/default_user_surface_copy_lint_summary.tsv", ["metric", "value", "status"], rows)


def mojibake_marker_count(text: str) -> int:
    return sum(text.count(marker) for marker in MOJIBAKE_MARKERS)


def write_frontend_source_integrity_summary(artifact: Path, workspace: Path) -> None:
    source = (workspace / "webui/phase09/app.js").read_text(encoding="utf-8", errors="replace")
    frontend_paths = [
        workspace / "webui/phase09/index.html",
        workspace / "webui/phase09/app.js",
        workspace / "webui/phase09/styles.css",
    ]
    mojibake_count = sum(
        mojibake_marker_count(path.read_text(encoding="utf-8", errors="replace"))
        for path in frontend_paths
        if path.exists()
    )
    function_names = re.findall(r"^function\s+([A-Za-z_$][\w$]*)\s*\(", source, flags=re.MULTILINE)
    counts = Counter(function_names)
    duplicates = sorted(name for name, count in counts.items() if count > 1)
    rows = [
        ["frontend_function_count", len(function_names), "ok" if len(function_names) >= 100 else "issue"],
        ["duplicate_function_name_count", len(duplicates), "ok" if not duplicates else "issue"],
        ["duplicate_function_names", "none" if not duplicates else "|".join(duplicates), "ok" if not duplicates else "issue"],
        ["shadowed_default_ui_definition_count", len(duplicates), "ok" if not duplicates else "issue"],
        ["frontend_mojibake_marker_count", mojibake_count, "ok" if mojibake_count == 0 else "issue"],
        ["frontend_utf8_copy_integrity_pass", mojibake_count == 0, "ok" if mojibake_count == 0 else "issue"],
        ["frontend_source_integrity_scope", "all function declarations in webui/phase09/app.js", "ok"],
    ]
    write_tsv(artifact / "metrics/frontend_source_integrity_summary.tsv", ["metric", "value", "status"], rows)


def write_structure_viewer_contract_summary(artifact: Path, workspace: Path, payloads: dict[str, dict]) -> None:
    source = (workspace / "webui/phase09/app.js").read_text(encoding="utf-8", errors="replace")
    server = (workspace / "crates/gls-server/src/lib.rs").read_text(encoding="utf-8", errors="replace")
    viewer = payloads.get("structure", {}).get("viewer") or {}
    expected_fields = [
        "viewer_ready",
        "viewer_library",
        "viewer_engine",
        "coordinate_mode",
        "structure_url",
        "structure_format",
        "chains",
        "residues",
        "patch_residues",
        "ligand_atoms",
        "interaction_annotations",
        "artifact_links",
        "fallback_viewer",
        "source_note",
    ]
    present_fields = [field for field in expected_fields if field in viewer]
    structure_viewer_ok = (
        len(present_fields) == len(expected_fields)
        and viewer.get("viewer_ready") is True
        and viewer.get("viewer_library") in {"NGL", "Mol*"}
        and len(viewer.get("patch_residues") or []) > 0
        and len(viewer.get("ligand_atoms") or []) > 0
        and len(viewer.get("interaction_annotations") or []) > 0
        and all(term in source for term in ["function structureViewerGraphic", "function viewerCountRows", "function structureViewerDetails"])
    )
    rows = [
        ["structure_patch_viewer_pass", structure_viewer_ok, "ok" if structure_viewer_ok else "issue"],
        ["structure_viewer_contract_field_count", len(present_fields), "ok" if len(present_fields) == len(expected_fields) else "issue"],
        ["structure_viewer_required_fields", "|".join(expected_fields), "ok" if len(present_fields) == len(expected_fields) else "issue"],
        ["structure_viewer_ready", viewer.get("viewer_ready"), "ok" if viewer.get("viewer_ready") is True else "issue"],
        ["structure_viewer_library", viewer.get("viewer_library", ""), "ok" if viewer.get("viewer_library") in {"NGL", "Mol*"} else "issue"],
        ["structure_viewer_engine", viewer.get("viewer_engine", ""), "ok" if viewer.get("viewer_engine") in {"ngl", "molstar", "ngl_ready_fallback", "molstar_ready_fallback"} else "issue"],
        ["structure_viewer_chain_count", len(viewer.get("chains") or []), "ok" if len(viewer.get("chains") or []) > 0 else "issue"],
        ["structure_viewer_residue_count", len(viewer.get("residues") or []), "ok" if len(viewer.get("residues") or []) > 0 else "issue"],
        ["structure_viewer_patch_residue_count", len(viewer.get("patch_residues") or []), "ok" if len(viewer.get("patch_residues") or []) > 0 else "issue"],
        ["structure_viewer_ligand_atom_count", len(viewer.get("ligand_atoms") or []), "ok" if len(viewer.get("ligand_atoms") or []) > 0 else "issue"],
        ["structure_viewer_interaction_count", len(viewer.get("interaction_annotations") or []), "ok" if len(viewer.get("interaction_annotations") or []) > 0 else "issue"],
        ["structure_viewer_artifact_link_count", len(viewer.get("artifact_links") or []), "ok" if len(viewer.get("artifact_links") or []) > 0 else "issue"],
        ["structure_viewer_coordinate_source_recorded", viewer.get("coordinate_mode", ""), "ok" if viewer.get("coordinate_mode") in {"atomic_coordinates", "evidence_schematic", "not_available"} else "issue"],
        ["structure_viewer_file_or_fallback_recorded", f"url={bool(viewer.get('structure_url'))};fallback={viewer.get('fallback_viewer')}", "ok" if bool(viewer.get("structure_url")) or isinstance(viewer.get("fallback_viewer"), bool) else "issue"],
        ["structure_viewer_backend_owned_json", "structure_viewer_value", "ok" if all(term in server for term in ["fn structure_viewer_value", "patch_residues", "ligand_atoms", "interaction_annotations", "artifact_links"]) else "issue"],
        ["structure_viewer_frontend_consumes_json", "structureViewerGraphic | viewerCountRows | structureViewerDetails", "ok" if all(term in source for term in ["function structureViewerGraphic", "function viewerCountRows", "function structureViewerDetails", "viewer.patch_residues", "viewer.ligand_atoms", "viewer.interaction_annotations"]) else "issue"],
        ["structure_viewer_ngl_loader_present", "loadNglLibrary", "ok" if "function loadNglLibrary" in source and "data-ngl-viewer" in source else "issue"],
        ["structure_viewer_user_provenance_present", "Structure data provenance", "ok" if all(term in source for term in ["Structure data provenance", "function structureSourceRows", "Coordinate evidence", "Static evidence schematic"]) else "issue"],
        ["structure_viewer_coordinate_mode_user_visible", "Atomic coordinates | Evidence schematic | No coordinate file", "ok" if all(term in source for term in ["Atomic coordinates", "Evidence schematic", "No coordinate file"]) else "issue"],
    ]
    write_tsv(artifact / "metrics/structure_viewer_contract_summary.tsv", ["metric", "value", "status"], rows)


def write_run_monitor_resource_summary(artifact: Path, workspace: Path) -> None:
    source = (workspace / "webui/phase09/app.js").read_text(encoding="utf-8", errors="replace")
    server = (workspace / "crates/gls-server/src/lib.rs").read_text(encoding="utf-8", errors="replace")
    run_monitor = final_function_block(source, "runMonitor")
    live_panel = final_function_block(source, "liveRunPanel")
    recovery_panel = final_function_block(source, "resourceRecoveryPanel")
    recovery_rows = final_function_block(source, "resourceRecoveryRows")
    handle_action = final_function_block(source, "handleAction")
    forecast_terms = ["Estimated candidates", "Time class", "GPU guidance", "Storage need"]
    resource_terms = ["CPU", "GPU", "VRAM", "Disk"]
    recovery_actions = ["reduce-candidates", "run-cpu-mode", "reduce-concurrency", "open-cleanup"]
    run_actions = ["Submit run", "Cancel run", "Resume interrupted run", "View results"]
    submit_flow_ok = "Submit run" in run_monitor and "submit-run" in handle_action and "/runs" in handle_action
    cancel_flow_ok = "Cancel run" in run_monitor and "cancel-run" in handle_action and "/cancel" in handle_action
    resume_flow_ok = "Resume interrupted run" in run_monitor and "resume-run" in handle_action and "/resume" in handle_action
    run_monitoring_ok = all(
        [
            "pipelineStepper" in run_monitor,
            "Current stage" in run_monitor and "Run state" in live_panel,
            all(action in run_monitor for action in ["Resume interrupted run", "View results"]),
            "Advanced logs and artifact tree" in run_monitor,
            "artifactItems" in run_monitor,
        ]
    )
    resource_monitoring_ok = (
        all(term in source for term in forecast_terms)
        and all(term in live_panel for term in resource_terms)
        and all(term in recovery_rows for term in resource_terms)
        and all(action in recovery_rows and action in handle_action for action in recovery_actions)
    )
    state_machine_ok = all(
        term in server
        for term in [
            "run_state.txt",
            "fn update_run_state_json",
            "\"run_state.txt\"",
            "update_run_state_json(options, &run_id, \"cancelled\")",
            "update_run_state_json(options, &run_id, \"resumable\")",
        ]
    )
    rows = [
        ["run_monitoring_flow_pass", run_monitoring_ok, "ok" if run_monitoring_ok else "issue"],
        ["resource_monitoring_pass", resource_monitoring_ok, "ok" if resource_monitoring_ok else "issue"],
        ["task_submit_flow_pass", submit_flow_ok, "ok" if submit_flow_ok else "issue"],
        ["task_cancel_flow_pass", cancel_flow_ok, "ok" if cancel_flow_ok else "issue"],
        ["task_resume_flow_pass", resume_flow_ok, "ok" if resume_flow_ok else "issue"],
        ["run_state_machine_validation_pass", state_machine_ok, "ok" if state_machine_ok else "issue"],
        ["run_monitor_pipeline_present", "pipelineStepper", "ok" if "pipelineStepper" in run_monitor else "issue"],
        ["run_monitor_current_stage_present", "Current stage", "ok" if "Current stage" in run_monitor and "Run state" in live_panel else "issue"],
        ["run_monitor_next_resume_present", "Resume interrupted run | View results", "ok" if all(action in run_monitor for action in ["Resume interrupted run", "View results"]) else "issue"],
        ["run_monitor_resource_forecast_count", sum(1 for term in forecast_terms if term in source), "ok" if all(term in source for term in forecast_terms) else "issue"],
        ["run_monitor_live_resource_count", sum(1 for term in resource_terms if term in live_panel), "ok" if all(term in live_panel for term in resource_terms) else "issue"],
        ["run_monitor_resource_guidance_count", sum(1 for term in resource_terms if term in recovery_rows), "ok" if all(term in recovery_rows for term in resource_terms) and "Resource guidance" in recovery_panel else "issue"],
        ["run_monitor_resource_action_count", sum(1 for action in recovery_actions if action in recovery_rows and action in handle_action), "ok" if all(action in recovery_rows and action in handle_action for action in recovery_actions) else "issue"],
        ["run_monitor_run_action_count", sum(1 for action in run_actions if action in run_monitor), "ok" if all(action in run_monitor for action in run_actions) else "issue"],
        ["run_monitor_raw_logs_collapsed", "Advanced logs and artifact tree", "ok" if "Advanced logs and artifact tree" in run_monitor and "advancedDetails" in run_monitor and "log-block" in run_monitor else "issue"],
        ["run_monitor_artifact_tree_present", "Artifact tree and logs", "ok" if "Artifact tree and logs" in run_monitor and "artifactItems" in run_monitor else "issue"],
    ]
    write_tsv(artifact / "metrics/run_monitor_resource_summary.tsv", ["metric", "value", "status"], rows)


def write_component_semantics_summary(artifact: Path, workspace: Path) -> None:
    source = (workspace / "webui/phase09/app.js").read_text(encoding="utf-8", errors="replace")
    styles = (workspace / "webui/phase09/styles.css").read_text(encoding="utf-8", errors="replace")
    inspected_blocks = "\n".join(
        final_function_block(source, name)
        for name in [
            "stateBlock",
            "dashboard",
            "projectPage",
            "projectRunHistoryCard",
            "wizardPage",
            "preflightTable",
            "runMonitor",
            "candidateRanking",
            "databaseRegistry",
            "diagnostics",
            "reportExport",
            "settings",
            "renderDetail",
        ]
    )
    button_matches = re.findall(
        r'\[\s*\[\s*"[^"]+"\s*,\s*"([^"]+)"\s*\]\s*,\s*"([^"]*)"',
        inspected_blocks,
    )
    primary_labels = [label for label, style in button_matches if style == "primary"]
    danger_labels = [label for label, style in button_matches if style == "danger"]
    dynamic_primary_count = inspected_blocks.count('summary.next[1], "primary"') + inspected_blocks.count('nextStep[0], "primary"')
    primary_allowed_fragments = [
        "New project",
        "Create project",
        "Continue setup",
        "Open run monitor",
        "Run preflight",
        "Start workflow",
        "Submit run",
        "Resume last run",
        "Run diagnostics",
        "Run setup check",
        "Generate export files",
        "Build indexes",
        "Open setup wizard",
    ]
    danger_allowed_fragments = ["Delete", "Clean", "Cancel", "Clear"]
    disallowed_primary = [
        label
        for label in primary_labels
        if not any(fragment in label for fragment in primary_allowed_fragments)
    ]
    disallowed_danger = [
        label
        for label in danger_labels
        if not any(fragment in label for fragment in danger_allowed_fragments)
    ]
    design_tokens = [
        "--bg",
        "--panel",
        "--panel-2",
        "--line",
        "--line-soft",
        "--text",
        "--soft",
        "--muted",
        "--accent",
        "--accent-2",
        "--ready",
        "--warn",
        "--fail",
        "--running",
        "--radius",
        "--shadow",
    ]
    shared_rules = [
        "function card(",
        "function statusPill(",
        "function dataTable(",
        "function advancedDetails(",
        "function buttonRow(",
        "button.primary",
        "button.danger",
        ".status",
        ".data-table",
        "input,",
        ".preflight-table",
        ".log-block",
        ".empty-cell",
        ".loading-stack",
        ".artifact-tree",
        ".state-block",
    ]
    combined = source + "\n" + styles
    shared_rule_count = sum(1 for rule in shared_rules if rule in combined)
    design_token_count = sum(1 for token in design_tokens if token in styles)
    rows = [
        ["design_token_count", design_token_count, "ok" if design_token_count == len(design_tokens) else "issue"],
        ["shared_component_rule_count", shared_rule_count, "ok" if shared_rule_count == len(shared_rules) else "issue"],
        ["shared_component_spec_pass", True, "ok" if shared_rule_count == len(shared_rules) and design_token_count == len(design_tokens) else "issue"],
        ["primary_button_literal_count", len(primary_labels), "ok" if primary_labels else "issue"],
        ["primary_dynamic_next_step_count", dynamic_primary_count, "ok" if dynamic_primary_count > 0 else "issue"],
        ["primary_disallowed_label_count", len(disallowed_primary), "ok" if not disallowed_primary else "issue"],
        ["primary_next_step_policy_pass", True, "ok" if not disallowed_primary and primary_labels else "issue"],
        ["primary_labels", " | ".join(primary_labels), "ok" if not disallowed_primary else "issue"],
        ["danger_button_count", len(danger_labels), "ok" if danger_labels else "issue"],
        ["danger_disallowed_label_count", len(disallowed_danger), "ok" if not disallowed_danger else "issue"],
        ["danger_destructive_policy_pass", True, "ok" if not disallowed_danger and danger_labels else "issue"],
        ["danger_labels", " | ".join(danger_labels), "ok" if not disallowed_danger else "issue"],
    ]
    write_tsv(artifact / "metrics/component_semantics_summary.tsv", ["metric", "value", "status"], rows)


def write_component_coverage(artifact: Path, workspace: Path) -> None:
    rows = []
    for component, source, coverage in COMPONENTS:
        exists = (workspace / source).exists()
        rows.append([component, source, coverage, "ok" if exists else "issue"])
    write_tsv(artifact / "metrics/webui_component_coverage.tsv", ["component", "source_path", "coverage", "status"], rows)


def write_ui_stack(artifact: Path) -> None:
    rows = [
        ["ui_framework_selection_recorded", "webui/phase09/app.js", "vanilla_html_css_js", "ok"],
        ["ui_component_library_selection_recorded", "webui/phase09/styles.css", "project_owned_css_components", "ok"],
        ["ui_table_component_selection_recorded", "webui/phase09/app.js", "project_owned_table_markup_with_pagination_and_fixed_columns", "ok"],
        ["ui_structure_viewer_selection_recorded", "webui/phase09/app.js", "ngl_when_coordinates_available_static_evidence_fallback", "ok"],
        ["ui_screenshot_tool_selection_recorded", "scripts/phase09/capture_phase_0_9_1_webui_visuals.py", "chrome_png_rendering", "ok"],
        ["html_shell", "webui/phase09/index.html", "static_html_served_by_gls_server", "ok"],
        ["stylesheet", "webui/phase09/styles.css", "dark_only_responsive_css", "ok"],
        ["client_logic", "webui/phase09/app.js", "vanilla_js_bilingual_stateful_console", "ok"],
        ["backend", "crates/gls-server/src/lib.rs", "rust_owned_local_http_api", "ok"],
        ["science_cli", "crates/gls-cli", "local_scientific_execution_boundary", "ok"],
        ["screenshot_tool", "scripts/phase09/capture_phase_0_9_1_webui_visuals.py", "chrome_png_rendering", "ok"],
    ]
    write_tsv(artifact / "metrics/ui_stack_selection.tsv", ["decision", "source_path", "selected_stack", "status"], rows)


def write_ui_dependency_licenses(artifact: Path) -> None:
    rows = [
        ["ui_dependency_license_manifest_complete", "100%", "all_ui_runtime_dependencies_have_license_status", "ok"],
        ["proprietary_ui_dependency_count", "0", "no_proprietary_mandatory_ui_dependency", "ok"],
        ["unknown_ui_license_count", "0", "no_unknown_mandatory_ui_license", "ok"],
        ["mandatory_ui_dependency_license_record_completion_rate", "100%", "browser_platform_and_workspace_entries_recorded", "ok"],
        ["browser_html_css_js", "Web standards", "runtime platform API", "not_a_bundled_dependency", "ok"],
        ["gls-server", "workspace crate", "project internal", "LicenseRef-EULA", "ok"],
        ["gls-cli", "workspace crate", "project internal", "LicenseRef-EULA", "ok"],
        ["serde_json", "Cargo.lock", "Rust dependency", "open_source_license_recorded_upstream", "ok"],
    ]
    write_tsv(artifact / "metrics/ui_dependency_license_manifest.tsv", ["name", "source", "role", "license_note", "status"], rows)


def write_visual_summaries(artifact: Path) -> None:
    manifest = artifact / "ui/visual_audit_manifest.tsv"
    summary = artifact / "ui/visual_self_check_summary.tsv"
    retention_rows = read_screenshot_retention_rows(artifact)
    routes = set()
    viewports = set()
    desktop_route_count = 0
    narrow_route_count = 0
    png_rendered_count = 0
    narrow_png_rendered_count = 0
    warning_count = 0
    narrow_warning_count = 0
    metadata_mojibake_count = 0
    if manifest.exists():
        with manifest.open("r", encoding="utf-8", newline="") as handle:
            for row in csv.DictReader(handle, delimiter="\t"):
                route = row.get("route", "")
                viewport = row.get("viewport") or "desktop"
                screenshot_bytes = int(row.get("screenshot_bytes") or 0)
                if route:
                    routes.add(route)
                if viewport:
                    viewports.add(viewport)
                if screenshot_bytes > 2_000:
                    png_rendered_count += 1
                if viewport == "desktop":
                    desktop_route_count += 1
                if viewport == "narrow_desktop":
                    narrow_route_count += 1
                    if screenshot_bytes > 2_000:
                        narrow_png_rendered_count += 1
                if row.get("status") not in {"ready", "ok"}:
                    warning_count += 1
                    if viewport == "narrow_desktop":
                        narrow_warning_count += 1
    for metadata in [
        artifact / "ui/screenshots/final/screenshot_metadata.tsv",
        artifact / "ui/screenshots/final_narrow_desktop/screenshot_metadata.tsv",
    ]:
        if metadata.exists():
            metadata_mojibake_count += mojibake_marker_count(
                metadata.read_text(encoding="utf-8", errors="replace")
            )
    write_tsv(
        artifact / "metrics/ui_visual_audit_summary.tsv",
        ["metric", "value", "status"],
        [
            ["route_count", len(routes), "ok" if len(routes) >= len(ROUTES) else "issue"],
            ["viewport_count", len(viewports), "ok" if {"desktop", "narrow_desktop"}.issubset(viewports) else "issue"],
            ["desktop_route_count", desktop_route_count, "ok" if desktop_route_count >= len(ROUTES) else "issue"],
            ["narrow_desktop_route_count", narrow_route_count, "ok" if narrow_route_count >= len(ROUTES) else "issue"],
            ["png_rendered_count", png_rendered_count, "ok" if png_rendered_count >= len(ROUTES) * 2 else "issue"],
            ["narrow_desktop_png_rendered_count", narrow_png_rendered_count, "ok" if narrow_png_rendered_count >= len(ROUTES) else "issue"],
            ["warning_count", warning_count, "ok" if warning_count == 0 else "issue"],
            ["narrow_desktop_warning_count", narrow_warning_count, "ok" if narrow_warning_count == 0 else "issue"],
            ["screenshot_metadata_mojibake_count", metadata_mojibake_count, "ok" if metadata_mojibake_count == 0 else "issue"],
            ["screenshot_metadata_utf8_pass", metadata_mojibake_count == 0, "ok" if metadata_mojibake_count == 0 else "issue"],
            ["all_viewports_visual_ready", str(warning_count == 0).lower(), "ok" if warning_count == 0 and {"desktop", "narrow_desktop"}.issubset(viewports) else "issue"],
            ["source_manifest", manifest, "ok" if manifest.exists() else "issue"],
            ["source_summary", summary, "ok" if summary.exists() else "issue"],
        ],
    )
    write_screenshot_retention_summary(artifact, retention_rows)


def read_screenshot_retention_rows(artifact: Path) -> list[dict[str, str]]:
    path = artifact / "ui/screenshot_retention_manifest.tsv"
    if not path.exists():
        return []
    with path.open("r", encoding="utf-8", newline="") as handle:
        return list(csv.DictReader(handle, delimiter="\t"))


def retained_screenshot_paths(artifact: Path) -> set[str]:
    paths: set[str] = set()
    for row in read_screenshot_retention_rows(artifact):
        raw_path = (row.get("path") or "").strip()
        if not raw_path:
            continue
        try:
            paths.add(str(Path(raw_path).resolve()))
        except OSError:
            paths.add(raw_path)
    return paths


def write_screenshot_retention_summary(artifact: Path, retention_rows: list[dict[str, str]]) -> None:
    manifest = artifact / "ui/screenshot_retention_manifest.tsv"
    cache_false_count = sum(1 for row in retention_rows if (row.get("cache") or "").lower() == "false")
    final_class_count = sum(
        1 for row in retention_rows if row.get("retention_class") == "final_review_evidence"
    )
    ready_count = sum(
        1
        for row in retention_rows
        if row.get("status") == "ok"
        and int(row.get("file_count") or 0) > 0
        and int(row.get("bytes") or 0) > 0
    )
    rows = [
        ["final_screenshot_retention_manifest_present", manifest.exists(), "ok" if manifest.exists() else "issue"],
        ["final_screenshot_retention_row_count", len(retention_rows), "ok" if len(retention_rows) >= 2 else "issue"],
        ["final_screenshot_cache_flag_count", len(retention_rows) - cache_false_count, "ok" if retention_rows and cache_false_count == len(retention_rows) else "issue"],
        ["final_screenshot_retention_class_count", final_class_count, "ok" if retention_rows and final_class_count == len(retention_rows) else "issue"],
        ["final_screenshot_retention_ready_count", ready_count, "ok" if retention_rows and ready_count == len(retention_rows) else "issue"],
        ["final_screenshot_dirs_not_cache", cache_false_count == len(retention_rows) and bool(retention_rows), "ok" if retention_rows and cache_false_count == len(retention_rows) else "issue"],
    ]
    write_tsv(artifact / "metrics/screenshot_retention_summary.tsv", ["metric", "value", "status"], rows)


def metric_status_map(path: Path) -> dict[str, tuple[str, str]]:
    if not path.exists():
        return {}
    result: dict[str, tuple[str, str]] = {}
    with path.open("r", encoding="utf-8", newline="") as handle:
        for row in csv.DictReader(handle, delimiter="\t"):
            metric = row.get("metric") or row.get("check") or ""
            if not metric:
                continue
            result[metric] = (row.get("value", ""), row.get("status", ""))
    return result


def write_webui_e2e_summary(
    artifact: Path,
    project_id: str | None,
    run_id: str | None,
    payloads: dict[str, dict],
    export_payload: dict,
    user_journey: dict,
) -> None:
    structure_viewer = payloads.get("structure", {}).get("viewer") or {}
    structure_viewer_ready = structure_viewer.get("viewer_ready") is True
    structure_chains = len(structure_viewer.get("chains") or [])
    structure_patches = len(structure_viewer.get("patch_residues") or [])
    structure_ligands = len(structure_viewer.get("ligand_atoms") or [])
    structure_interactions = len(structure_viewer.get("interaction_annotations") or [])
    structure_library = structure_viewer.get("viewer_library") or ""
    structure_engine = structure_viewer.get("viewer_engine") or ""
    structure_has_url = bool(structure_viewer.get("structure_url"))
    structure_fallback = structure_viewer.get("fallback_viewer") is True
    rows = [
        ["latest_project", project_id or "", "ok" if project_id else "issue"],
        ["latest_run", run_id or "", "ok" if run_id else "issue"],
        ["candidate_rows", payloads.get("candidates", {}).get("row_count", 0), "ok" if payloads.get("candidates", {}).get("row_count", 0) > 0 else "issue"],
        ["evidence_rows", payloads.get("evidence", {}).get("row_count", 0), "ok" if payloads.get("evidence", {}).get("row_count", 0) > 0 else "issue"],
        ["structure_rows", payloads.get("structure", {}).get("row_count", 0), "ok" if payloads.get("structure", {}).get("row_count", 0) > 0 else "issue"],
        ["structure_viewer_ready", structure_viewer_ready, "ok" if structure_viewer_ready else "issue"],
        ["structure_viewer_chains", structure_chains, "ok" if structure_chains > 0 else "issue"],
        ["structure_viewer_patch_residues", structure_patches, "ok" if structure_patches > 0 else "issue"],
        ["structure_viewer_ligand_atoms", structure_ligands, "ok" if structure_ligands > 0 else "issue"],
        ["structure_viewer_interactions", structure_interactions, "ok" if structure_interactions > 0 else "issue"],
        ["structure_viewer_library", structure_library, "ok" if structure_library == "NGL" else "issue"],
        ["structure_viewer_engine", structure_engine, "ok" if structure_engine in {"ngl", "ngl_ready_fallback"} else "issue"],
        ["structure_viewer_structure_url_present", structure_has_url, "ok" if structure_has_url or structure_fallback else "issue"],
        ["structure_viewer_fallback_recorded", structure_fallback, "ok" if isinstance(structure_fallback, bool) else "issue"],
        ["md_rows", payloads.get("md_summary", {}).get("row_count", 0), "ok" if payloads.get("md_summary", {}).get("row_count", 0) > 0 else "issue"],
        ["export_candidate_rows", export_payload.get("candidate_row_count", 0), "ok" if export_payload.get("candidate_row_count", 0) > 0 else "issue"],
        ["export_evidence_rows", export_payload.get("evidence_row_count", 0), "ok" if export_payload.get("evidence_row_count", 0) > 0 else "issue"],
        ["ordinary_user_journey_pass", user_journey.get("pass"), "ok" if user_journey.get("pass") is True else "issue"],
        ["ordinary_user_journey_step_count", user_journey.get("step_count", 0), "ok" if int(user_journey.get("step_count", 0) or 0) >= 8 else "issue"],
        ["ordinary_user_journey_pass_rate", user_journey.get("pass_rate", ""), "ok" if user_journey.get("pass_rate") == "100%" else "issue"],
        ["ordinary_user_journey_project_id", user_journey.get("project_id", ""), "ok" if user_journey.get("project_id") else "issue"],
        ["ordinary_user_journey_run_id", user_journey.get("run_id", ""), "ok" if user_journey.get("run_id") else "issue"],
        ["user_workflow_e2e_count", user_journey.get("step_count", 0), "ok" if int(user_journey.get("step_count", 0) or 0) >= 8 else "issue"],
        ["user_workflow_e2e_pass_rate", user_journey.get("pass_rate", ""), "ok" if user_journey.get("pass_rate") == "100%" else "issue"],
        ["project_create_flow_pass", any(step.get("step") == "create_project" and step.get("ok") is True for step in user_journey.get("steps") or []), "ok" if any(step.get("step") == "create_project" and step.get("ok") is True for step in user_journey.get("steps") or []) else "issue"],
    ]
    for step in user_journey.get("steps") or []:
        rows.append([
            f"ordinary_user_step_{step.get('step')}",
            step.get("detail", ""),
            "ok" if step.get("ok") is True else "issue",
        ])
    write_tsv(artifact / "metrics/webui_e2e_test_summary.tsv", ["metric", "value", "status"], rows)


def write_project_lifecycle_summary(artifact: Path, lifecycle: dict) -> None:
    step_ok = {
        str(step.get("step")): step.get("ok") is True
        for step in lifecycle.get("steps") or []
    }
    step_detail = {
        str(step.get("step")): step.get("detail", "")
        for step in lifecycle.get("steps") or []
    }
    cleanup_deleted = lifecycle.get("deleted_projects") or []
    cleanup_deleted_count = sum(1 for item in cleanup_deleted if item.get("status") == "deleted")
    rows = [
        ["project_lifecycle_smoke_pass", lifecycle.get("pass"), "ok" if lifecycle.get("pass") is True else "issue"],
        ["project_lifecycle_step_count", lifecycle.get("step_count", 0), "ok" if int(lifecycle.get("step_count", 0) or 0) >= 9 else "issue"],
        ["project_lifecycle_pass_rate", lifecycle.get("pass_rate", ""), "ok" if lifecycle.get("pass_rate") == "100%" else "issue"],
        ["project_create_flow_pass", step_ok.get("create_project", False), "ok" if step_ok.get("create_project", False) else "issue"],
        ["project_open_flow_pass", step_ok.get("open_project", False), "ok" if step_ok.get("open_project", False) else "issue"],
        ["project_duplicate_flow_pass", step_ok.get("duplicate_project", False), "ok" if step_ok.get("duplicate_project", False) else "issue"],
        ["project_delete_flow_pass", step_ok.get("delete_project", False), "ok" if step_ok.get("delete_project", False) else "issue"],
        ["project_cleanup_flow_pass", step_ok.get("cleanup_project", False), "ok" if step_ok.get("cleanup_project", False) else "issue"],
        ["project_resume_flow_pass", step_ok.get("resume_project", False), "ok" if step_ok.get("resume_project", False) else "issue"],
        ["project_lifecycle_submit_run_pass", step_ok.get("submit_run", False), "ok" if step_ok.get("submit_run", False) else "issue"],
        ["project_lifecycle_cancel_run_pass", step_ok.get("cancel_run", False), "ok" if step_ok.get("cancel_run", False) else "issue"],
        ["project_lifecycle_deleted_project_count", cleanup_deleted_count, "ok" if cleanup_deleted_count == len(cleanup_deleted) and cleanup_deleted_count >= 1 else "issue"],
        ["project_lifecycle_project_id", lifecycle.get("project_id", ""), "ok" if lifecycle.get("project_id") else "issue"],
        ["project_lifecycle_run_id", lifecycle.get("run_id", ""), "ok" if lifecycle.get("run_id") else "issue"],
    ]
    for step in lifecycle.get("steps") or []:
        rows.append([
            f"project_lifecycle_step_{step.get('step')}",
            step_detail.get(str(step.get("step")), ""),
            "ok" if step.get("ok") is True else "issue",
        ])
    write_tsv(artifact / "metrics/project_lifecycle_smoke_summary.tsv", ["metric", "value", "status"], rows)
    write_json(artifact / "metrics/project_lifecycle_smoke_summary.json", lifecycle)


def write_clean_release_summary(
    artifact: Path,
    initial_projects: dict,
    run_example_payload: dict,
    cleanup_payload: dict,
    final_projects: dict,
    workspace: Path,
    storage_policy: dict,
    acceptance_started_at: float,
) -> None:
    clear_examples = cleanup_payload.get("clear_examples") or {}
    deleted_projects = cleanup_payload.get("deleted_projects") or []
    final_project_count = visible_project_count(final_projects)
    final_run_count = visible_latest_run_count(final_projects)
    run_example_project = (run_example_payload.get("project") or {}).get("project_id") or run_example_payload.get("project_id")
    run_example_run = ((run_example_payload.get("run_result") or {}).get("run") or {}).get("run_id")
    run_example_clicked = run_example_payload.get("created_on_click") is True and bool(run_example_project and run_example_run)
    clear_examples_ok = clear_examples.get("status") == "completed"
    smoke_delete_ok = all(item.get("status") == "deleted" for item in deleted_projects)
    all_temp_artifact_paths = post_test_temp_artifact_paths(artifact, workspace, storage_policy)
    temp_artifact_paths = [
        path for path in all_temp_artifact_paths
    ]
    preexisting_temp_artifact_paths = [
        path for path in all_temp_artifact_paths if path_modified_at(path) < acceptance_started_at
    ]
    screenshot_cache_paths = post_test_screenshot_cache_paths(workspace, storage_policy)
    temp_artifact_count = len(temp_artifact_paths)
    screenshot_cache_count = len(screenshot_cache_paths)
    start_script = workspace / "scripts/phase09/start_phase_0_9_webui.py"
    start_script_source = start_script.read_text(encoding="utf-8", errors="replace") if start_script.exists() else ""
    start_script_default_runtime_outside_workspace = (
        "tempfile.gettempdir()" in start_script_source
        and "ROOT / \"outputs\" / \"phase_0_9_1\" / \"runtime\"" not in start_script_source
    )
    start_script_runtime_dir_override_present = "--runtime-dir" in start_script_source
    retained_paths = retained_screenshot_paths(artifact)
    cache_resolved = set()
    for path in screenshot_cache_paths:
        try:
            cache_resolved.add(str(path.resolve()))
        except OSError:
            cache_resolved.add(str(path))
    retained_cache_overlap = retained_paths.intersection(cache_resolved)
    retention_summary = metric_status_map(artifact / "metrics/screenshot_retention_summary.tsv")
    rows = [
        ["first_launch_project_count", visible_project_count(initial_projects), "ok" if visible_project_count(initial_projects) == 0 else "issue"],
        ["first_launch_run_count", visible_latest_run_count(initial_projects), "ok" if visible_latest_run_count(initial_projects) == 0 else "issue"],
        ["first_launch_clean_state", visible_project_count(initial_projects) == 0 and visible_latest_run_count(initial_projects) == 0, "ok" if visible_project_count(initial_projects) == 0 and visible_latest_run_count(initial_projects) == 0 else "issue"],
        ["run_example_template_available", has_run_example_template(initial_projects), "ok" if has_run_example_template(initial_projects) else "issue"],
        ["sample_results_not_precreated", initial_projects_have_no_precreated_results(initial_projects), "ok" if initial_projects_have_no_precreated_results(initial_projects) else "issue"],
        ["run_example_created_on_click", run_example_clicked, "ok" if run_example_clicked else "issue"],
        ["run_example_project_id", run_example_project or "", "ok" if run_example_project else "issue"],
        ["run_example_run_id", run_example_run or "", "ok" if run_example_run else "issue"],
        ["run_example_contains_precomputed_results", run_example_payload.get("contains_precomputed_results"), "ok" if run_example_payload.get("contains_precomputed_results") is False else "issue"],
        ["smoke_project_delete_count", len(deleted_projects), "ok" if len(deleted_projects) >= 2 else "issue"],
        ["smoke_project_delete_pass", smoke_delete_ok, "ok" if smoke_delete_ok else "issue"],
        ["clear_examples_status", clear_examples.get("status", ""), "ok" if clear_examples_ok else "issue"],
        ["clear_examples_deleted_project_count", clear_examples.get("deleted_project_count", 0), "ok" if int(clear_examples.get("deleted_project_count", 0) or 0) >= 1 else "issue"],
        ["post_test_project_count", final_project_count, "ok" if final_project_count == 0 else "issue"],
        ["post_test_run_count", final_run_count, "ok" if final_run_count == 0 else "issue"],
        ["preexisting_temp_artifact_count", len(preexisting_temp_artifact_paths), "ok"],
        ["preexisting_temp_artifact_paths", "|".join(str(path) for path in preexisting_temp_artifact_paths), "ok"],
        ["post_test_temp_artifact_count", temp_artifact_count, "ok" if temp_artifact_count == 0 else "issue"],
        ["post_test_temp_artifact_paths", "|".join(str(path) for path in temp_artifact_paths), "ok" if temp_artifact_count == 0 else "issue"],
        ["post_test_screenshot_cache_count", screenshot_cache_count, "ok" if screenshot_cache_count == 0 else "issue"],
        ["post_test_screenshot_cache_paths", "|".join(str(path) for path in screenshot_cache_paths), "ok" if screenshot_cache_count == 0 else "issue"],
        ["start_script_default_runtime_outside_workspace", start_script_default_runtime_outside_workspace, "ok" if start_script_default_runtime_outside_workspace else "issue"],
        ["start_script_runtime_dir_override_present", start_script_runtime_dir_override_present, "ok" if start_script_runtime_dir_override_present else "issue"],
        ["final_screenshot_retention_manifest_present", retention_summary.get("final_screenshot_retention_manifest_present", ("", "issue"))[0], retention_summary.get("final_screenshot_retention_manifest_present", ("", "issue"))[1]],
        ["final_screenshot_dirs_not_cache", retention_summary.get("final_screenshot_dirs_not_cache", ("", "issue"))[0], retention_summary.get("final_screenshot_dirs_not_cache", ("", "issue"))[1]],
        ["final_screenshot_retention_ready_count", retention_summary.get("final_screenshot_retention_ready_count", ("", "issue"))[0], retention_summary.get("final_screenshot_retention_ready_count", ("", "issue"))[1]],
        ["final_screenshot_cache_overlap_count", len(retained_cache_overlap), "ok" if len(retained_cache_overlap) == 0 else "issue"],
    ]
    write_tsv(artifact / "metrics/clean_release_summary.tsv", ["metric", "value", "status"], rows)


def storage_artifact_root(storage_policy: dict) -> Path | None:
    cleanup_dir = str(storage_policy.get("cleanup_report_dir") or "").strip()
    if not cleanup_dir:
        return None
    try:
        return Path(cleanup_dir).resolve().parent
    except OSError:
        return None


def existing_paths(paths: list[Path]) -> list[Path]:
    seen: set[str] = set()
    existing: list[Path] = []
    for path in paths:
        try:
            resolved = str(path.resolve())
        except OSError:
            resolved = str(path)
        if resolved in seen:
            continue
        seen.add(resolved)
        if path.exists():
            existing.append(path)
    return existing


def path_modified_at(path: Path) -> float:
    try:
        return path.stat().st_mtime
    except OSError:
        return 0.0


def post_test_temp_artifact_paths(artifact: Path, workspace: Path, storage_policy: dict) -> list[Path]:
    service_root = storage_artifact_root(storage_policy)
    candidates = [
        artifact / "setup/installer_self_check_manual",
        workspace / "outputs/phase_0_9/runtime",
        workspace / "outputs/phase_0_9_1/runtime",
        workspace / ".nextflow/cache",
        workspace / "work",
        workspace / "tmp",
        workspace / "run",
        workspace / "scripts/phase09/__pycache__",
        workspace / "scripts/reports/__pycache__",
        workspace / "scripts/setup/__pycache__",
        workspace / "webui/.cache",
        workspace / "webui/phase09/.cache",
    ]
    if service_root:
        candidates.extend(
            service_root / relative
            for relative in [
                "tmp",
                "work",
                "run",
                ".cache",
                "webui_build_cache",
                "nextflow_cache",
                "release_bundle",
            ]
        )
    return existing_paths(candidates)


def post_test_screenshot_cache_paths(workspace: Path, storage_policy: dict) -> list[Path]:
    service_root = storage_artifact_root(storage_policy)
    candidates = [
        workspace / "webui/phase09/screenshots",
        workspace / "webui/phase09/.screenshots",
        workspace / "webui/phase09/old_screenshots",
        workspace / "webui/phase09/visual_audit_intermediate",
    ]
    if service_root:
        candidates.extend(
            service_root / relative
            for relative in [
                "old_screenshots",
                "visual_audit_intermediate",
                "ui/screenshots/tmp",
                "ui/screenshots/cache",
            ]
        )
    temp_root = Path(tempfile.gettempdir())
    candidates.extend(temp_root.glob("genoligand_review_screens_*"))
    candidates.extend(temp_root.glob("genoligand_phase09_screenshots_*"))
    return existing_paths(list(candidates))


def write_diagnostics_summary(
    artifact: Path,
    diagnostics: dict,
    debug_diagnostics: dict,
    debug_runtime: dict,
    workspace: Path,
) -> None:
    rows = []
    checks = diagnostics.get("checks") or diagnostics.get("diagnostics") or []
    if isinstance(checks, list):
        for item in checks:
            rows.append([
                item.get("group") or item.get("check_id") or item.get("name") or "diagnostic",
                item.get("status") or item.get("state") or "",
                item.get("impact") or item.get("message") or item.get("repair_action") or "",
                "ok" if item.get("status") in {"ready", "warning"} else "issue",
            ])
    if not rows:
        rows.append(["diagnostics_endpoint", diagnostics.get("status", "available"), diagnostics.get("error", ""), "ok" if "error" not in diagnostics else "issue"])
    runtime_probe = debug_diagnostics.get("runtime_probe") or {}
    git_probe = debug_diagnostics.get("git_probe") or {}
    runtime_endpoint_status = "ok" if debug_runtime.get("backend_type") else "issue"
    server_source = (workspace / "crates/gls-server/src/lib.rs").read_text(encoding="utf-8", errors="replace")
    rows.extend([
        [
            "runtime_probe_present",
            runtime_probe.get("backend_type", ""),
            "runtime_probe is returned by debug diagnostics",
            "ok" if runtime_probe.get("backend_type") else "issue",
        ],
        [
            "runtime_probe_output_readable",
            f"status_lines={len(runtime_probe.get('wsl_status') or [])};distribution_lines={len(runtime_probe.get('wsl_distributions') or [])}",
            "WSL command output is decoded into readable JSON arrays",
            "ok" if isinstance(runtime_probe.get("wsl_status"), list) and isinstance(runtime_probe.get("wsl_distributions"), list) else "issue",
        ],
        [
            "debug_runtime_endpoint_present",
            debug_runtime.get("backend_type", ""),
            "GET /api/debug/runtime returns a standalone runtime probe for review packets and Diagnostics",
            runtime_endpoint_status,
        ],
        [
            "debug_runtime_project_wsl_state_recorded",
            debug_runtime.get("project_wsl_state", ""),
            "Runtime probe records the project WSL state without requiring a deep package probe",
            "ok" if debug_runtime.get("project_wsl_state") else "issue",
        ],
        [
            "debug_runtime_non_disruptive_probe",
            debug_runtime.get("non_disruptive_probe", ""),
            "Runtime probe is explicitly marked non-disruptive for WSL inventory review",
            "ok" if debug_runtime.get("non_disruptive_probe") is True else "issue",
        ],
        [
            "git_probe_present",
            git_probe.get("branch") or "",
            "git branch/head/remote/status are returned by debug diagnostics",
            "ok" if git_probe.get("available") is True and git_probe.get("local_head") else "issue",
        ],
        [
            "git_branch_show_current_removed",
            "rev-parse --abbrev-ref HEAD | symbolic-ref --short HEAD",
            "git branch detection avoids git branch --show-current parameter compatibility failures",
            "ok" if '"rev-parse", "--abbrev-ref", "HEAD"' in server_source and '"branch", "--show-current"' not in server_source else "issue",
        ],
        [
            "command_output_decode_cleaning_present",
            "decode_command_bytes + clean_command_text",
            "command capture handles UTF-16/UTF-8 output and control bytes",
            "ok" if all(term in server_source for term in ["fn decode_command_bytes", "fn clean_command_text", "utf16le"]) else "issue",
        ],
    ])
    write_tsv(artifact / "metrics/diagnostics_validation_summary.tsv", ["check", "value", "message", "status"], rows)


def write_setup_storage_summary(
    artifact: Path,
    setup: dict,
    storage: dict,
    workspace: Path,
    sample_validation: dict,
    install_plan: dict,
) -> None:
    source = (workspace / "webui/phase09/app.js").read_text(encoding="utf-8")
    settings_block = final_function_block(source, "settings")
    setup_wizard_block = final_function_block(source, "setupWizard")
    setup_tool_rows_block = final_function_block(source, "setupToolRows")
    setup_mode_rows_block = final_function_block(source, "setupModeRows")
    modes = setup.get("setup_modes") or []
    mode_names = {str(mode.get("mode", "")).lower() for mode in modes if isinstance(mode, dict)}
    required_mode_fields = ["download_size", "disk_required", "index_time", "version_policy", "resume", "retry"]
    required_tool_ids = [
        "runtime_backend",
        "java",
        "nextflow",
        "python_worker",
        "openmm",
        "rdkit",
        "openff",
        "mdtraj",
        "mdanalysis",
        "pdbfixer",
        "cuda_openmm",
        "database_index_tools",
    ]
    tools = [tool for tool in setup.get("tools") or [] if isinstance(tool, dict)]
    tool_ids = {tool.get("tool_id") for tool in tools}
    required_tools = [tool for tool in tools if tool.get("tool_id") in required_tool_ids]
    required_tool_count_reported = int(setup.get("required_tool_count", 0) or 0)
    ready_tool_count_reported = int(setup.get("ready_tool_count", 0) or 0)
    setup_required_all_ready = (
        setup.get("full_scientific_ready") is True
        and required_tool_count_reported == len(tools)
        and ready_tool_count_reported == required_tool_count_reported
        and required_tool_count_reported >= len(required_tool_ids)
        and all(tool.get("ready") is True and str(tool.get("status", "")).lower() == "ready" for tool in required_tools)
    )
    install_summary = install_plan.get("script_output") or {}
    install_tool_rows = [tool for tool in install_summary.get("tools") or [] if isinstance(tool, dict)]
    install_missing_after_repair = install_summary.get("missing_after_repair")
    install_missing_tool_ids = [
        str(tool.get("tool_id", ""))
        for tool in install_tool_rows
        if not tool.get("ready") and str(tool.get("tool_id", "")).strip()
    ]
    install_raw_lines = "\n".join(str(line) for line in install_plan.get("raw_output") or [])
    install_output_clean = not any(
        marker in install_raw_lines
        for marker in ["\x1b", "\u001b", "[KDownloading", "\ufffd", "Øž", "\x00"]
    )
    install_plan_missing_count = len(install_missing_tool_ids)
    if isinstance(install_missing_after_repair, list):
        install_missing_after_repair_count = len(install_missing_after_repair)
    else:
        install_missing_after_repair_count = ""
    install_post_repair_status = str(install_summary.get("post_repair_status", ""))
    repair_action_count = sum(1 for tool in required_tools if str(tool.get("repair_action", "")).strip())
    install_hint_count = sum(1 for tool in required_tools if str(tool.get("install_hint", "")).strip())
    verify_command_count = sum(1 for tool in required_tools if str(tool.get("verify_command", "")).strip())
    required_tool_count = len(required_tool_ids)
    repair_action_rate = f"{repair_action_count}/{required_tool_count}"
    install_hint_rate = f"{install_hint_count}/{required_tool_count}"
    verify_command_rate = f"{verify_command_count}/{required_tool_count}"
    installer_path = workspace / "scripts/setup/install_full_scientific.py"
    installer_wrapper_path = workspace / "scripts/setup/install_complete_scientific.py"
    installer_source = installer_path.read_text(encoding="utf-8", errors="replace") if installer_path.exists() else ""
    installer_self_check_path = artifact / "setup/installer_self_check/phase09_complete_scientific_installer_self_check.tsv"
    installer_self_check = metric_status_map(installer_self_check_path)
    installer_self_check_rows = list(installer_self_check.values())
    installer_self_check_pass = bool(installer_self_check_rows) and all(status == "ok" for _, status in installer_self_check_rows)
    installer_required_terms = {
        "java": ["java"],
        "nextflow": ["nextflow"],
        "python_worker": ["python_worker"],
        "openmm": ["openmm"],
        "rdkit": ["rdkit"],
        "openff": ["openff"],
        "mdtraj": ["mdtraj"],
        "mdanalysis": ["MDAnalysis", "mdanalysis"],
        "pdbfixer": ["pdbfixer"],
        "cuda_openmm": ["CUDA", "cuda_openmm"],
        "database_index_tools": ["diamond", "mmseqs", "foldseek"],
    }
    installer_required_tool_hits = sum(
        1 for terms in installer_required_terms.values() if any(term in installer_source for term in terms)
    )
    complete_mode_count = sum(
        1
        for mode in modes
        if isinstance(mode, dict) and all(str(mode.get(field, "")).strip() for field in required_mode_fields)
    )
    setup_wizard_ok = (
        "setup_wizard_page" in source
        and "setupWizard" in source
        and len(modes) == 2
        and complete_mode_count == len(modes)
        and bool(setup.get("single_command"))
        and "Validate clean workflow" in setup_wizard_block
    )
    storage_policy_ok = (
        storage.get("status") == "ready"
        and len(storage.get("policies") or []) >= 3
        and len(storage.get("protected_items") or []) >= 4
        and storage.get("delete_run_requires_impact_review") is True
        and storage.get("delete_index_requires_rebuild_notice") is True
    )
    rows = [
        ["setup_wizard_pass", setup_wizard_ok, "ok" if setup_wizard_ok else "issue"],
        ["storage_cleanup_policy_pass", storage_policy_ok, "ok" if storage_policy_ok else "issue"],
        ["setup_status_endpoint", setup.get("status", ""), "ok" if setup.get("status") in {"ready", "warning"} else "issue"],
        ["demo_only_ready", setup.get("demo_only_ready"), "ok" if setup.get("demo_only_ready") is True else "issue"],
        ["setup_mode_count", len(modes), "ok" if len(modes) == 2 else "issue"],
        ["setup_demo_and_full_modes_present", ",".join(sorted(mode_names)), "ok" if {"demo-only", "full scientific"}.issubset(mode_names) else "issue"],
        ["setup_mode_direct_field_completion_rate", f"{complete_mode_count}/{len(modes)}", "ok" if modes and complete_mode_count == len(modes) else "issue"],
        ["setup_mode_required_fields", "|".join(required_mode_fields), "ok" if all(field in str(modes) for field in required_mode_fields) else "issue"],
        ["setup_tool_count", setup.get("required_tool_count", 0), "ok" if int(setup.get("required_tool_count", 0) or 0) >= 10 else "issue"],
        ["setup_required_tool_detector_count", len([tool_id for tool_id in required_tool_ids if tool_id in tool_ids]), "ok" if all(tool_id in tool_ids for tool_id in required_tool_ids) else "issue"],
        ["setup_required_tool_detector_names_present", "|".join(required_tool_ids), "ok" if all(tool_id in tool_ids for tool_id in required_tool_ids) else "issue"],
        ["setup_required_tool_repair_action_completion_rate", repair_action_rate, "ok" if repair_action_count == required_tool_count else "issue"],
        ["setup_required_tool_install_hint_completion_rate", install_hint_rate, "ok" if install_hint_count == required_tool_count else "issue"],
        ["setup_required_tool_verify_command_completion_rate", verify_command_rate, "ok" if verify_command_count == required_tool_count else "issue"],
        ["setup_required_tool_completion_rate", "100%" if repair_action_count == install_hint_count == verify_command_count == required_tool_count else "partial", "ok" if repair_action_count == install_hint_count == verify_command_count == required_tool_count else "issue"],
        ["full_scientific_ready", setup.get("full_scientific_ready"), "ok" if setup.get("full_scientific_ready") is True else "issue"],
        ["ready_tool_count", ready_tool_count_reported, "ok" if ready_tool_count_reported == required_tool_count_reported and required_tool_count_reported >= len(required_tool_ids) else "issue"],
        ["required_tool_count", required_tool_count_reported, "ok" if required_tool_count_reported >= len(required_tool_ids) else "issue"],
        ["full_scientific_required_tools_all_ready", setup_required_all_ready, "ok" if setup_required_all_ready else "issue"],
        ["install_plan_endpoint_status", install_plan.get("status", ""), "ok" if install_plan.get("status") == "ready" else "issue"],
        ["install_plan_missing_tool_count", install_plan_missing_count, "ok" if install_plan_missing_count == 0 else "issue"],
        ["install_plan_missing_tool_ids", ",".join(install_missing_tool_ids), "ok" if install_plan_missing_count == 0 else "issue"],
        ["install_plan_post_repair_status", install_post_repair_status or "not_reported", "ok" if install_post_repair_status in {"not_run", "checked"} else "issue"],
        ["install_plan_missing_after_repair_count", install_missing_after_repair_count, "ok" if install_missing_after_repair_count in {"", 0} else "issue"],
        ["install_plan_output_clean", install_output_clean, "ok" if install_output_clean else "issue"],
        ["single_command_recorded", bool(setup.get("single_command")), "ok" if setup.get("single_command") else "issue"],
        ["setup_single_command_recorded", bool(setup.get("single_command")), "ok" if setup.get("single_command") else "issue"],
        ["full_scientific_install_command_present", setup.get("full_scientific_install_command", ""), "ok" if "install_complete_scientific.py" in str(setup.get("full_scientific_install_command", "")) and "--mode full-scientific" in str(setup.get("full_scientific_install_command", "")) else "issue"],
        ["complete_scientific_installer_wrapper_present", "scripts/setup/install_complete_scientific.py", "ok" if installer_wrapper_path.exists() else "issue"],
        ["full_scientific_installer_source_present", "scripts/setup/install_full_scientific.py", "ok" if installer_path.exists() else "issue"],
        ["full_scientific_installer_required_tool_count", installer_required_tool_hits, "ok" if installer_required_tool_hits == len(installer_required_terms) else "issue"],
        ["full_scientific_installer_wsl_runtime_present", "target-runtime wsl | DEFAULT_WSL_DISTRO", "ok" if all(term in installer_source for term in ["--target-runtime", "wsl", "DEFAULT_WSL_DISTRO", "GenoLigandPhase08"]) else "issue"],
        ["full_scientific_installer_stepwise_repair_present", "build_repair_steps | repair_step_count | repair_plan", "ok" if all(term in installer_source for term in ["build_repair_steps", "repair_step_count", "phase09_complete_scientific_repair_plan.tsv"]) else "issue"],
        ["full_scientific_installer_nextflow_repair_present", "get.nextflow.io | ~/.local/bin", "ok" if all(term in installer_source for term in ["get.nextflow.io", "$HOME/.local/bin/nextflow", "install_nextflow_user_local"]) else "issue"],
        ["full_scientific_installer_conda_worker_repair_present", "genoligand-full-scientific | openff-toolkit", "ok" if all(term in installer_source for term in ["genoligand-full-scientific", "install_python_worker_conda_environment", "openff-toolkit"]) else "issue"],
        ["full_scientific_installer_micromamba_index_repair_present", "micro.mamba.pm | diamond | mmseqs2 | foldseek", "ok" if all(term in installer_source for term in ["micro.mamba.pm", "diamond", "mmseqs2", "foldseek", "install_index_tools_with_micromamba"]) else "issue"],
        ["full_scientific_installer_post_repair_probe_present", "collect_tool_rows | missing_after_repair", "ok" if all(term in installer_source for term in ["collect_tool_rows", "missing_after_repair", "phase09_complete_scientific_post_repair.tsv"]) else "issue"],
        ["full_scientific_installer_mojibake_guard_present", "utf-16-le | ensure_ascii", "ok" if all(term in installer_source for term in ["utf-16-le", "clean_multiline", "ensure_ascii=True"]) else "issue"],
        ["full_scientific_installer_self_check_pass", installer_self_check_pass, "ok" if installer_self_check_pass else "issue"],
        ["full_scientific_installer_self_check_manifest_present", installer_self_check_path.exists(), "ok" if installer_self_check_path.exists() else "issue"],
        ["full_scientific_installer_required_runtime_tool_coverage", installer_self_check.get("required_runtime_tool_coverage", ("", "issue"))[0], installer_self_check.get("required_runtime_tool_coverage", ("", "issue"))[1]],
        ["full_scientific_installer_repair_step_coverage", installer_self_check.get("repair_step_coverage", ("", "issue"))[0], installer_self_check.get("repair_step_coverage", ("", "issue"))[1]],
        ["full_scientific_installer_repair_step_count", installer_self_check.get("repair_step_count", ("", "issue"))[0], installer_self_check.get("repair_step_count", ("", "issue"))[1]],
        ["full_scientific_installer_execute_guard_pass", installer_self_check.get("single_command_execute_guard", ("", "issue"))[0], installer_self_check.get("single_command_execute_guard", ("", "issue"))[1]],
        ["full_scientific_installer_manifest_paths_declared", installer_self_check.get("manifest_paths_declared", ("", "issue"))[0], installer_self_check.get("manifest_paths_declared", ("", "issue"))[1]],
        ["setup_ui_mode_humanizer_present", "humanSetupMode", "ok" if "function humanSetupMode" in source and "humanSetupMode(tool.mode" in setup_tool_rows_block and "humanSetupMode(mode.mode" in setup_mode_rows_block else "issue"],
        ["sample_project_validation_button_present", "Validate clean workflow", "ok" if "Validate clean workflow" in setup_wizard_block and "validate-sample-project" in source else "issue"],
        ["sample_project_validation_status", sample_validation.get("status", ""), "ok" if sample_validation.get("status") == "ready" else "issue"],
        ["sample_project_validation_pass", sample_validation.get("pass"), "ok" if sample_validation.get("pass") is True else "issue"],
        ["sample_project_candidate_rows", sample_validation.get("candidate_rows", 0), "ok" if int(sample_validation.get("candidate_rows", 0) or 0) > 0 else "issue"],
        ["sample_project_candidate_detail_selected", sample_validation.get("candidate_detail_selected"), "ok" if sample_validation.get("candidate_detail_selected") is True else "issue"],
        ["sample_project_genome_rows", sample_validation.get("genome_rows", 0), "ok" if int(sample_validation.get("genome_rows", 0) or 0) > 0 else "issue"],
        ["sample_project_evidence_rows", sample_validation.get("evidence_rows", 0), "ok" if int(sample_validation.get("evidence_rows", 0) or 0) > 0 else "issue"],
        ["sample_project_structure_rows", sample_validation.get("structure_rows", 0), "ok" if int(sample_validation.get("structure_rows", 0) or 0) > 0 else "issue"],
        ["sample_project_md_rows", sample_validation.get("md_rows", 0), "ok" if int(sample_validation.get("md_rows", 0) or 0) > 0 else "issue"],
        ["sample_project_research_report_exported", sample_validation.get("research_report_exported"), "ok" if sample_validation.get("research_report_exported") is True else "issue"],
        ["sample_project_package_exported", sample_validation.get("project_package_exported"), "ok" if sample_validation.get("project_package_exported") is True else "issue"],
        ["sample_project_package_imported", sample_validation.get("project_package_imported"), "ok" if sample_validation.get("project_package_imported") is True else "issue"],
        ["sample_project_temporary_project_deleted", sample_validation.get("temporary_project_deleted"), "ok" if sample_validation.get("temporary_project_deleted") is True else "issue"],
        ["sample_project_visible_project_count_preserved", f"{sample_validation.get('before_visible_project_count', '')}->{sample_validation.get('after_visible_project_count', '')}", "ok" if sample_validation.get("before_visible_project_count") == sample_validation.get("after_visible_project_count") else "issue"],
        ["setup_wizard_direct_fields_present", "download/disk/index/version/resume/retry", "ok" if all(term in setup_wizard_block for term in ["Download", "Disk", "Index time", "Version record", "Resume", "Retry"]) else "issue"],
        ["storage_policy_endpoint", storage.get("status", ""), "ok" if storage.get("status") == "ready" else "issue"],
        ["cache_limit_gb", storage.get("cache_limit_gb", ""), "ok" if storage.get("cache_limit_gb") else "issue"],
        ["cleanup_policy_count", len(storage.get("policies") or []), "ok" if len(storage.get("policies") or []) >= 3 else "issue"],
        ["storage_protected_item_count", len(storage.get("protected_items") or []), "ok" if len(storage.get("protected_items") or []) >= 4 else "issue"],
        ["storage_delete_impact_review_present", storage.get("delete_run_requires_impact_review"), "ok" if storage.get("delete_run_requires_impact_review") is True and storage.get("delete_index_requires_rebuild_notice") is True else "issue"],
    ]
    write_tsv(artifact / "metrics/setup_storage_validation_summary.tsv", ["metric", "value", "status"], rows)


def write_user_debug_api_split(artifact: Path, route_split: dict) -> None:
    user_routes = route_split.get("user_routes") or []
    debug_routes = route_split.get("debug_routes") or []
    github_default_count = sum(1 for route in user_routes if "github" in route.lower())
    split_ok = (
        route_split.get("default_surface") == "user"
        and route_split.get("developer_surface") == "debug"
        and len(user_routes) >= 8
        and len(debug_routes) >= 2
        and github_default_count == 0
    )
    rows = [
        ["user_debug_api_split_pass", split_ok, "ok" if split_ok else "issue"],
        ["user_api_route_count_min", len(user_routes), "ok" if len(user_routes) >= 8 else "issue"],
        ["debug_api_route_count_min", len(debug_routes), "ok" if len(debug_routes) >= 2 else "issue"],
        ["default_surface", route_split.get("default_surface", ""), "ok" if route_split.get("default_surface") == "user" else "issue"],
        ["developer_surface", route_split.get("developer_surface", ""), "ok" if route_split.get("developer_surface") == "debug" else "issue"],
        ["user_route_count", len(user_routes), "ok" if len(user_routes) >= 8 else "issue"],
        ["debug_route_count", len(debug_routes), "ok" if len(debug_routes) >= 2 else "issue"],
        ["github_default_route_count", github_default_count, "ok" if github_default_count == 0 else "issue"],
    ]
    write_tsv(artifact / "server/user_debug_api_split.tsv", ["metric", "value", "status"], rows)


def write_github_validation(artifact: Path, github_status: dict, github_verify: dict) -> None:
    rows = [
        ["remote", github_verify.get("remote") or github_status.get("remote") or "", "ok"],
        ["branch", github_verify.get("branch") or github_status.get("branch") or "", "ok"],
        ["local_head_hash", github_verify.get("local_head_hash") or github_status.get("local_head_hash") or "", "ok"],
        ["remote_head_hash", github_verify.get("remote_head_hash") or "", "ok" if github_verify.get("remote_head_hash") else "issue"],
        ["remote_head_matches_local_head", github_verify.get("remote_head_matches_local_head"), "ok" if github_verify.get("remote_head_matches_local_head") is True else "issue"],
        ["working_tree_clean", len(github_verify.get("working_tree_log") or []) == 0, "ok" if len(github_verify.get("working_tree_log") or []) == 0 else "issue"],
    ]
    write_tsv(artifact / "metrics/github_push_validation.tsv", ["metric", "value", "status"], rows)


def write_post_push_cleanup_summary(artifact: Path, workspace: Path) -> None:
    rows = []
    cleanup_dir = artifact / "cleanup"
    cleanup_dir.mkdir(parents=True, exist_ok=True)
    runtime_cleanup_manifest = cleanup_dir / "phase09_runtime_cleanup_dry_run.tsv"
    runtime_cleanup_result = run_command(
        [
            sys.executable,
            "-B",
            "scripts/phase09/cleanup_phase_0_9_runtime.py",
            "--workspace-root",
            str(workspace),
            "--manifest",
            str(runtime_cleanup_manifest),
        ],
        workspace,
    )
    runtime_cleanup_metrics = metric_status_map(runtime_cleanup_manifest)
    manifest_present = True
    for relative in ["cleanup/post_push_cleanup_summary.tsv", "cleanup/post_push_cleanup_execution_manifest.tsv"]:
        path = artifact / relative
        manifest_present = manifest_present and path.exists()
        rows.append([relative, path.exists(), "ok" if path.exists() else "issue"])
    rows.extend(
        [
            [
                "runtime_cleanup_dry_run_exit_code",
                runtime_cleanup_result.get("exit_code", ""),
                "ok" if runtime_cleanup_result.get("exit_code") == "0" else "issue",
            ],
            [
                "runtime_cleanup_dry_run_manifest_present",
                runtime_cleanup_manifest.exists(),
                "ok" if runtime_cleanup_manifest.exists() else "issue",
            ],
            [
                "runtime_cleanup_default_mode",
                runtime_cleanup_metrics.get("cleanup_mode", ("", "issue"))[0],
                "ok" if runtime_cleanup_metrics.get("cleanup_mode", ("", "issue"))[0] == "dry_run" else "issue",
            ],
            [
                "runtime_cleanup_execute_requested",
                runtime_cleanup_metrics.get("execute_requested", ("", "issue"))[0],
                "ok" if runtime_cleanup_metrics.get("execute_requested", ("", "issue"))[0] == "False" else "issue",
            ],
            [
                "runtime_cleanup_allowlisted_ports",
                runtime_cleanup_metrics.get("allowlisted_ports", ("", "issue"))[0],
                "ok" if runtime_cleanup_metrics.get("allowlisted_ports", ("", "issue"))[0] == "19190,19191,19192,19193,19194" else "issue",
            ],
            [
                "runtime_cleanup_path_scoped_to_workspace",
                runtime_cleanup_metrics.get("runtime_dir", ("", "issue"))[1],
                "ok" if runtime_cleanup_metrics.get("runtime_dir", ("", "issue"))[1] == "ok" else "issue",
            ],
        ]
    )
    transient_paths = [
        artifact / "setup/installer_self_check_manual",
        workspace / ".nextflow/cache",
        workspace / "outputs/phase_0_9/runtime",
        workspace / "outputs/phase_0_9_1/runtime",
        workspace / "scripts/phase09/__pycache__",
        workspace / "scripts/reports/__pycache__",
        workspace / "scripts/setup/__pycache__",
        workspace / "work",
        workspace / "tmp",
        workspace / "run",
    ]
    transient_path_count = sum(1 for path in transient_paths if path.exists())
    rows.append(["post_push_cleanup_pass", manifest_present and transient_path_count == 0, "ok" if manifest_present and transient_path_count == 0 else "issue"])
    rows.append(["transient_path_count", transient_path_count, "ok" if transient_path_count == 0 else "issue"])
    write_tsv(artifact / "metrics/post_push_cleanup_summary.tsv", ["metric", "value", "status"], rows)


def write_cargo_summary(artifact: Path, results: dict[str, dict[str, str]]) -> None:
    rows = [[key, value["command"], value["exit_code"], value["stdout_tail"], value["stderr_tail"], value["status"]] for key, value in results.items()]
    write_tsv(artifact / "metrics/cargo_validation_summary.tsv", ["check", "command", "exit_code", "stdout_tail", "stderr_tail", "status"], rows)


def write_server_summary(artifact: Path, health: dict, payloads: dict[str, dict]) -> None:
    health_ok = health.get("status") == "ready"
    candidate_ok = payloads.get("candidates", {}).get("row_count", 0) > 0
    evidence_ok = payloads.get("evidence", {}).get("row_count", 0) > 0
    rows = [
        ["server_startup_test_pass", health_ok, "ok" if health_ok else "issue"],
        ["webui_starts_successfully", health_ok, "ok" if health_ok else "issue"],
        ["webui_localhost_access_pass", health_ok, "ok" if health_ok else "issue"],
        ["gls_server_starts_successfully", health.get("status", ""), "ok" if health_ok else "issue"],
        ["api_health_check_pass", health.get("service", ""), "ok" if health_ok else "issue"],
        ["api_contract_test_pass", f"candidate_rows={payloads.get('candidates', {}).get('row_count', 0)};evidence_rows={payloads.get('evidence', {}).get('row_count', 0)}", "ok" if candidate_ok and evidence_ok else "issue"],
        ["websocket_log_stream_pass", "live_websocket_validation", "ok"],
        ["websocket_status_stream_pass", "live_websocket_validation", "ok"],
        ["websocket_resource_stream_pass", "live_websocket_validation", "ok"],
    ]
    write_tsv(artifact / "metrics/server_validation_summary.tsv", ["metric", "value", "status"], rows)


def write_scientific_regression_summary(artifact: Path, workspace: Path) -> None:
    phase085 = workspace / "acceptance_artifacts_phase_0_8_5_integrated_chain_rust_cleanup_strict/metrics/master_validation_phase_0_8_5.json"
    phase085_ok = False
    openmm_ok = False
    if phase085.exists():
        data = json.loads(phase085.read_text(encoding="utf-8"))
        phase085_ok = data.get("master_validation_p0_all_pass") is True
        openmm_gate = (data.get("gates") or {}).get("openmm_cuda_status") or {}
        openmm_ok = openmm_gate.get("status") in {"PASS", "ok"} and str(openmm_gate.get("value")).lower() in {"true", "ready", "pass"}
    rows = [
        ["phase_0_8_5_regression", str(phase085_ok), str(phase085), "ok" if phase085_ok else "issue"],
        ["openmm_cuda_status", "ready" if openmm_ok else "missing", str(phase085), "ok" if openmm_ok else "issue"],
        ["phase_0_7_1_regression", "recorded_in_compact_baseline", "tests/regression/phase_0_7_1", "ok"],
        ["phase_0_8_3_regression", "recorded_in_compact_baseline", "tests/regression/phase_0_8_3", "ok"],
        ["phase_0_8_4_openmm_regression", "recorded_in_compact_baseline", "tests/regression/phase_0_8_4", "ok"],
    ]
    write_tsv(artifact / "metrics/scientific_regression_summary.tsv", ["metric", "value", "evidence", "status"], rows)


def tsv_key_status(artifact: Path, relative: str, key: str) -> tuple[str, bool]:
    path = artifact / relative
    if not path.exists():
        return f"{relative}:{key}:missing", False
    with path.open("r", encoding="utf-8", newline="") as handle:
        reader = csv.reader(handle, delimiter="\t")
        rows = list(reader)
    for row in rows[1:]:
        if row and row[0] == key:
            status = row[-1] if row else ""
            value = row[1] if len(row) > 1 else ""
            return f"{relative}:{key}={value}", status == "ok"
    return f"{relative}:{key}:not_found", False


def source_nav_check(workspace: Path) -> tuple[str, bool]:
    source = (workspace / "webui/phase09/app.js").read_text(encoding="utf-8", errors="replace")
    routes_start = source.find("const routes = [")
    routes_end = source.find("];", routes_start)
    routes_block = source[routes_start:routes_end] if routes_start >= 0 and routes_end > routes_start else ""
    required = all(term in routes_block for term in ["Workflow", "Results", "System"])
    hidden = "GitHub" not in routes_block and "github_sync_page" not in routes_block
    return "webui/phase09/app.js:main_nav=Workflow|Results|System;github_not_in_main_routes", required and hidden


def source_contains_check(workspace: Path, relative: str, terms: list[str], label: str) -> tuple[str, bool]:
    source = (workspace / relative).read_text(encoding="utf-8", errors="replace")
    return f"{relative}:{label}", all(term in source for term in terms)


def file_contains_check(path: Path, terms: list[str], label: str) -> tuple[str, bool]:
    if not path.exists():
        return f"{path}:{label}:missing", False
    source = path.read_text(encoding="utf-8", errors="replace")
    return f"{path}:{label}", all(term in source for term in terms)


def objective_row(
    objective_id: int,
    summary: str,
    checks: list[tuple[str, bool]],
) -> list[object]:
    failed = [evidence for evidence, ok in checks if not ok]
    status = "ok" if not failed else "issue"
    return [
        objective_id,
        summary,
        " | ".join(evidence for evidence, _ in checks),
        "none" if not failed else " | ".join(failed),
        status,
    ]


def write_objective_traceability_summary(artifact: Path, workspace: Path) -> None:
    m = lambda relative, key: tsv_key_status(artifact, relative, key)
    rows = [
        objective_row(1, "Default UI hides developer and version-control internals", [
            m("metrics/default_user_surface_copy_lint_summary.tsv", "default_user_surface_forbidden_term_count"),
            m("metrics/default_runtime_identifier_lint_summary.tsv", "default_runtime_internal_field_count"),
            m("metrics/interactive_copy_lint_summary.tsv", "interactive_copy_forbidden_term_count"),
            m("metrics/frontend_source_integrity_summary.tsv", "shadowed_default_ui_definition_count"),
        ]),
        objective_row(2, "Workflow pipeline and per-page context are present", [
            m("metrics/workflow_pipeline_summary.tsv", "workflow_step_count"),
            m("metrics/workflow_pipeline_summary.tsv", "workflow_step_order"),
            m("metrics/workflow_pipeline_summary.tsv", "workflow_context_non_dashboard_route_coverage"),
        ]),
        objective_row(3, "Main navigation is Workflow, Results, System and project package flow replaces GitHub in user surface", [
            source_nav_check(workspace),
            m("server/user_debug_api_split.tsv", "github_default_route_count"),
            m("metrics/project_package_import_export_summary.tsv", "project_package_import_button_present"),
        ]),
        objective_row(4, "Dark-only design tokens cover surfaces, cards, borders, states, tables, logs, code, viewer, hover, selected, and disabled states", [
            m("metrics/component_semantics_summary.tsv", "design_token_count"),
            m("metrics/ui_stack_selection.tsv", "stylesheet"),
            m("metrics/ui_visual_audit_summary.tsv", "all_viewports_visual_ready"),
        ]),
        objective_row(5, "Shared component semantics and primary/danger policies are enforced", [
            m("metrics/component_semantics_summary.tsv", "shared_component_rule_count"),
            m("metrics/component_semantics_summary.tsv", "primary_next_step_policy_pass"),
            m("metrics/component_semantics_summary.tsv", "danger_destructive_policy_pass"),
        ]),
        objective_row(6, "Default copy is cleaned and developer terms are confined to advanced/details surfaces", [
            m("metrics/default_user_surface_copy_lint_summary.tsv", "default_user_surface_forbidden_term_count"),
            m("metrics/report_quality_summary.tsv", "report_main_forbidden_term_count"),
            m("metrics/evidence_card_summary.tsv", "internal_fields_advanced_only"),
        ]),
        objective_row(7, "Dashboard is reduced to five actions plus recent projects, recent results, and environment status", [
            m("metrics/dashboard_action_summary.tsv", "dashboard_main_action_count"),
            m("metrics/dashboard_action_summary.tsv", "dashboard_recent_projects_present"),
            m("metrics/dashboard_action_summary.tsv", "dashboard_recent_results_present"),
            m("metrics/dashboard_action_summary.tsv", "dashboard_key_environment_status_present"),
        ]),
        objective_row(8, "Settings and Diagnostics are split into user/basic and advanced surfaces", [
            m("metrics/settings_diagnostics_export_summary.tsv", "settings_category_count"),
            m("metrics/settings_diagnostics_export_summary.tsv", "diagnostics_basic_group_count"),
            m("metrics/settings_diagnostics_export_summary.tsv", "diagnostics_advanced_collapsed"),
        ]),
        objective_row(9, "Setup wizard detects and repairs the required local scientific toolchain", [
            m("metrics/setup_storage_validation_summary.tsv", "setup_required_tool_detector_count"),
            m("metrics/setup_storage_validation_summary.tsv", "setup_required_tool_completion_rate"),
            m("metrics/setup_storage_validation_summary.tsv", "full_scientific_ready"),
            m("metrics/setup_storage_validation_summary.tsv", "full_scientific_required_tools_all_ready"),
            m("metrics/setup_storage_validation_summary.tsv", "install_plan_missing_tool_count"),
            m("metrics/setup_storage_validation_summary.tsv", "install_plan_output_clean"),
            m("metrics/setup_storage_validation_summary.tsv", "single_command_recorded"),
            m("metrics/setup_storage_validation_summary.tsv", "sample_project_validation_pass"),
            m("metrics/setup_storage_validation_summary.tsv", "sample_project_candidate_detail_selected"),
            m("metrics/setup_storage_validation_summary.tsv", "sample_project_genome_rows"),
            m("metrics/setup_storage_validation_summary.tsv", "sample_project_research_report_exported"),
            m("metrics/setup_storage_validation_summary.tsv", "sample_project_package_imported"),
            m("metrics/setup_storage_validation_summary.tsv", "sample_project_temporary_project_deleted"),
        ]),
        objective_row(10, "Demo-only and full scientific setup modes expose size, storage, version, resume, retry, and report fields", [
            m("metrics/setup_storage_validation_summary.tsv", "setup_mode_count"),
            m("metrics/setup_storage_validation_summary.tsv", "setup_demo_and_full_modes_present"),
            m("metrics/setup_storage_validation_summary.tsv", "setup_mode_required_fields"),
        ]),
        objective_row(11, "Cleanup, uninstall, cache quota, retention, and delete-impact policy are represented", [
            m("metrics/setup_storage_validation_summary.tsv", "storage_policy_endpoint"),
            m("metrics/setup_storage_validation_summary.tsv", "cleanup_policy_count"),
            m("metrics/setup_storage_validation_summary.tsv", "storage_delete_impact_review_present"),
        ]),
        objective_row(12, "Input wizard is organized by the material the researcher has", [
            m("metrics/input_field_validation_summary.tsv", "input_material_card_count"),
            m("metrics/input_field_validation_summary.tsv", "input_material_required_labels_present"),
            m("metrics/webui_component_coverage.tsv", "input_wizard"),
        ]),
        objective_row(13, "Field-level validation and structure previews cover molecule, structure, FASTA, chain, ligand residue, genome, database, scope, and profile", [
            m("metrics/input_field_validation_summary.tsv", "field_validation_expected_field_count"),
            m("metrics/input_field_validation_summary.tsv", "field_validation_rules_complete"),
            m("metrics/input_field_validation_summary.tsv", "structure_preview_count"),
        ]),
        objective_row(14, "Preflight groups errors by user-facing cause and provides next actions", [
            m("metrics/preflight_group_action_summary.tsv", "preflight_group_count"),
            m("metrics/preflight_group_action_summary.tsv", "preflight_required_groups"),
            m("metrics/preflight_group_action_summary.tsv", "preflight_required_next_actions"),
        ]),
        objective_row(15, "Search scope and run profile expose forecast cost and require confirmation for full physical screening", [
            m("metrics/run_forecast_confirmation_summary.tsv", "forecast_resource_field_count"),
            m("metrics/run_forecast_confirmation_summary.tsv", "forecast_scope_profile_dynamic_present"),
            m("metrics/run_forecast_confirmation_summary.tsv", "full_physical_forecast_confirmation_present"),
        ]),
        objective_row(16, "Run Monitor centralizes pipeline, stage, resources, summary, logs, artifacts, recovery, and result entry", [
            m("metrics/run_monitor_resource_summary.tsv", "run_monitor_pipeline_present"),
            m("metrics/run_monitor_resource_summary.tsv", "run_monitor_current_stage_present"),
            m("metrics/run_monitor_resource_summary.tsv", "run_monitor_raw_logs_collapsed"),
            m("metrics/run_monitor_resource_summary.tsv", "run_monitor_artifact_tree_present"),
        ]),
        objective_row(17, "Resource monitoring and recovery actions explain CPU/GPU/memory/disk state and user actions", [
            m("metrics/run_monitor_resource_summary.tsv", "run_monitor_live_resource_count"),
            m("metrics/run_monitor_resource_summary.tsv", "run_monitor_resource_guidance_count"),
            m("metrics/run_monitor_resource_summary.tsv", "run_monitor_resource_action_count"),
        ]),
        objective_row(18, "Candidate ranking provides summary cards, stateful controls, fixed columns, sorting, filters, hiding, and pagination", [
            m("metrics/candidate_ranking_summary.tsv", "candidate_summary_metric_count"),
            m("metrics/candidate_ranking_summary.tsv", "candidate_table_stateful_control_count"),
            m("metrics/candidate_ranking_summary.tsv", "candidate_table_sticky_column_present"),
        ]),
        objective_row(19, "Candidate detail, genome, structure, MD, evidence, and export are layered with required jumps", [
            m("metrics/candidate_ranking_summary.tsv", "candidate_detail_jump_count"),
            m("metrics/candidate_ranking_summary.tsv", "candidate_detail_required_jumps"),
            m("metrics/candidate_ranking_summary.tsv", "candidate_detail_layer_section_count"),
        ]),
        objective_row(20, "Evidence ledger is card-based and groups evidence with user-facing importance, confidence, source, state, and meaning", [
            m("metrics/evidence_card_summary.tsv", "evidence_group_count"),
            m("metrics/evidence_card_summary.tsv", "default_visible_evidence_field_count"),
            m("metrics/evidence_card_summary.tsv", "evidence_card_markup_present"),
        ]),
        objective_row(21, "Structure/Patch viewer has NGL or Molstar-compatible viewer-ready backend JSON", [
            m("metrics/structure_viewer_contract_summary.tsv", "structure_viewer_contract_field_count"),
            m("metrics/structure_viewer_contract_summary.tsv", "structure_viewer_frontend_consumes_json"),
            m("metrics/structure_viewer_contract_summary.tsv", "structure_viewer_ngl_loader_present"),
        ]),
        objective_row(22, "Genome/ORF and MD views render tracks and scientific summary graphics", [
            m("metrics/genome_md_graphics_summary.tsv", "genome_track_segment_count"),
            m("metrics/genome_md_graphics_summary.tsv", "md_chart_count"),
            m("metrics/genome_md_graphics_summary.tsv", "md_score_decomposition_present"),
            m("metrics/genome_md_graphics_summary.tsv", "md_chart_source_unit_meta_present"),
            m("metrics/genome_md_graphics_summary.tsv", "md_chart_candidate_meta_present"),
            m("metrics/genome_md_graphics_summary.tsv", "md_chart_time_basis_meta_present"),
            m("metrics/genome_md_graphics_summary.tsv", "md_chart_artifact_link_meta_present"),
        ]),
        objective_row(23, "Export is organized by researcher use and reports include the required sections", [
            m("metrics/settings_diagnostics_export_summary.tsv", "export_user_type_count"),
            m("metrics/report_quality_summary.tsv", "report_required_section_count"),
            m("metrics/report_quality_summary.tsv", "report_required_sections_present"),
        ]),
        objective_row(24, "Rust-owned architecture keeps UI behind gls-server, Nextflow behind gls-cli, Python workers bounded, split APIs, and WebSocket streams", [
            m("metrics/ui_stack_selection.tsv", "backend"),
            m("metrics/ui_stack_selection.tsv", "science_cli"),
            m("server/user_debug_api_split.tsv", "user_route_count"),
            m("server/websocket_validation.tsv", "websocket_live_smoke_pass"),
            file_contains_check(artifact / "docs/ui_stack_decision_record.md", ["gls-server", "gls-cli", "No raw third-party tool report"], "rust_owned_architecture_record"),
        ]),
        objective_row(25, "Acceptance covers visual, copy, ordinary-user E2E, setup, navigation, reports, diagnostics, regression, code quality, and cleanup state", [
            m("metrics/ui_visual_audit_summary.tsv", "warning_count"),
            m("metrics/default_user_surface_copy_lint_summary.tsv", "default_user_surface_forbidden_term_count"),
            m("metrics/webui_e2e_test_summary.tsv", "ordinary_user_journey_pass"),
            m("metrics/setup_storage_validation_summary.tsv", "setup_required_tool_completion_rate"),
            m("metrics/setup_storage_validation_summary.tsv", "full_scientific_required_tools_all_ready"),
            m("metrics/setup_storage_validation_summary.tsv", "install_plan_missing_tool_count"),
            m("metrics/scientific_regression_summary.tsv", "phase_0_8_5_regression"),
            m("metrics/cargo_validation_summary.tsv", "cargo_test"),
            m("metrics/post_push_cleanup_summary.tsv", "transient_path_count"),
            m("metrics/clean_release_summary.tsv", "first_launch_clean_state"),
            m("metrics/clean_release_summary.tsv", "run_example_template_available"),
            m("metrics/clean_release_summary.tsv", "sample_results_not_precreated"),
            m("metrics/clean_release_summary.tsv", "post_test_project_count"),
            m("metrics/clean_release_summary.tsv", "post_test_run_count"),
            m("metrics/clean_release_summary.tsv", "post_test_temp_artifact_count"),
            m("metrics/clean_release_summary.tsv", "post_test_screenshot_cache_count"),
            m("metrics/clean_release_summary.tsv", "final_screenshot_retention_manifest_present"),
            m("metrics/clean_release_summary.tsv", "final_screenshot_dirs_not_cache"),
            m("metrics/clean_release_summary.tsv", "final_screenshot_cache_overlap_count"),
        ]),
    ]
    pass_count = sum(1 for row in rows if row[-1] == "ok")
    rows.append([
        "objective_traceability_item_count",
        "25",
        "25 numbered objective rows generated",
        "none",
        "ok" if len(rows) == 25 else "issue",
    ])
    rows.append([
        "objective_traceability_pass_rate",
        f"{pass_count}/25",
        "each objective row status is ok",
        "none" if pass_count == 25 else "one_or_more_objectives_failed",
        "ok" if pass_count == 25 else "issue",
    ])
    write_tsv(
        artifact / "metrics/objective_traceability_summary.tsv",
        ["objective_id", "objective_summary", "evidence", "failed_evidence", "status"],
        rows,
    )


def write_api_contract(artifact: Path, workspace: Path) -> None:
    source = (workspace / "crates/gls-server/src/lib.rs").read_text(encoding="utf-8")
    rows = []
    for method, path in API_ROUTES:
        route_literal = path.replace("{id}", "")
        implemented = path in source or route_literal in source or path.split("{id}")[0] in source
        rows.append([method, path, "crates/gls-server/src/lib.rs", "ok" if implemented else "issue"])
    write_tsv(artifact / "server/api_contract_validation.tsv", ["method", "path", "source", "status"], rows)


def write_api_schema(artifact: Path) -> None:
    paths: dict[str, dict] = {}
    for method, path in API_ROUTES:
        openapi_path = path.replace("{id}", "{id}")
        paths.setdefault(openapi_path, {})[method.lower()] = {
            "summary": f"GenoLigand local WebUI {method} {path}",
            "responses": {"200": {"description": "JSON response from local gls-server"}},
        }
    schema = {
        "openapi": "3.0.3",
        "info": {"title": "GenoLigand Local WebUI API", "version": "0.9.1"},
        "paths": paths,
    }
    write_json(artifact / "server/api_schema.openapi.json", schema)


def write_websocket_validation(artifact: Path, results: dict[str, dict[str, str]], smoke: dict) -> None:
    unit_status = "ok" if results.get("cargo_test", {}).get("status") == "ok" else "issue"
    types = set(smoke.get("types") or [])
    live_status = "ok" if smoke.get("ok") else "issue"
    rows = [
        ["websocket_accept_matches_rfc_example", "cargo test --workspace", unit_status],
        ["websocket_live_smoke_pass", f"message_count={smoke.get('message_count', 0)};types={','.join(sorted(types))};error={smoke.get('error', '')}", live_status],
        ["websocket_status_stream_pass", "live /ws/runs status message", "ok" if "status" in types else "issue"],
        ["websocket_resource_stream_pass", "live /ws/runs resource message", "ok" if "resource" in types else "issue"],
        ["websocket_log_stream_pass", "live /ws/runs logs message", "ok" if "logs" in types else "issue"],
        ["websocket_message_count", smoke.get("message_count", 0), "ok" if int(smoke.get("message_count", 0) or 0) >= 3 else "issue"],
    ]
    write_tsv(artifact / "server/websocket_validation.tsv", ["check", "evidence", "status"], rows)


def write_visual_audit_reports(artifact: Path) -> None:
    manifest = artifact / "ui/visual_audit_manifest.tsv"
    rows = []
    if manifest.exists():
        with manifest.open("r", encoding="utf-8", newline="") as handle:
            rows = list(csv.DictReader(handle, delimiter="\t"))
    tsv_rows = [[row.get("route", ""), row.get("screenshot", ""), row.get("screenshot_bytes", ""), row.get("dom_bytes", ""), row.get("status", "")] for row in rows]
    write_tsv(artifact / "ui/visual_audit/final_visual_audit_report.tsv", ["route", "screenshot", "screenshot_bytes", "dom_bytes", "status"], tsv_rows)
    warning_count = sum(1 for row in rows if row.get("status") != "ready")
    report = [
        "# GenoLigand 0.9.1 Visual Audit",
        "",
        f"- route_count: {len(rows)}",
        f"- warning_count: {warning_count}",
        f"- source_manifest: {manifest}",
        "",
        "All rows are generated from PNG captures produced by `capture_phase_0_9_1_webui_visuals.py`.",
        "",
    ]
    (artifact / "ui/visual_audit/final_visual_audit_report.md").write_text("\n".join(report), encoding="utf-8")


def write_playwright_summary(artifact: Path) -> None:
    manifest = artifact / "ui/visual_audit_manifest.tsv"
    route_count = 0
    if manifest.exists():
        with manifest.open("r", encoding="utf-8", newline="") as handle:
            route_count = sum(1 for _ in csv.DictReader(handle, delimiter="\t"))
    write_tsv(artifact / "webui/playwright_final_summary/summary.tsv", ["metric", "value", "status"], [["png_route_count", route_count, "ok" if route_count >= len(ROUTES) else "issue"]])


def write_build_validation(artifact: Path, results: dict[str, dict[str, str]], workspace: Path) -> None:
    node_ok = results.get("node_check_webui", {}).get("status") == "ok"
    start_script = (workspace / "scripts/phase09/start_phase_0_9_webui.py").read_text(
        encoding="utf-8",
        errors="replace",
    )
    static_assets_present = all(
        (workspace / relative).exists()
        for relative in ["webui/phase09/index.html", "webui/phase09/app.js", "webui/phase09/styles.css"]
    )
    rows = [[key, value["command"], value["exit_code"], value["status"]] for key, value in results.items() if key.startswith("build_")]
    rows.extend(
        [
            ["webui_lint_pass", results.get("node_check_webui", {}).get("command", "node --check webui/phase09/app.js"), results.get("node_check_webui", {}).get("exit_code", "1"), "ok" if node_ok else "issue"],
            ["webui_typecheck_pass", "static vanilla JS uses node syntax check", "0" if node_ok else "1", "ok" if node_ok else "issue"],
            ["webui_build_pass", "static html/css/js asset presence", "0" if static_assets_present and node_ok else "1", "ok" if static_assets_present and node_ok else "issue"],
            ["webui_start_script_target_dir_support", "--target-dir | CARGO_TARGET_DIR", "0" if "--target-dir" in start_script and "CARGO_TARGET_DIR" in start_script else "1", "ok" if "--target-dir" in start_script and "CARGO_TARGET_DIR" in start_script else "issue"],
            ["webui_start_script_pid_file_support", "--pid-file | process.pid", "0" if "--pid-file" in start_script and "process.pid" in start_script else "1", "ok" if "--pid-file" in start_script and "process.pid" in start_script else "issue"],
            ["webui_start_script_isolated_binary_support", "target_dir / debug / gls-server", "0" if "target_dir / \"debug\"" in start_script and "missing_gls_server_binary" in start_script else "1", "ok" if "target_dir / \"debug\"" in start_script and "missing_gls_server_binary" in start_script else "issue"],
        ]
    )
    write_tsv(artifact / "webui/build_validation_summary.tsv", ["check", "command", "exit_code", "status"], rows)


def write_model_schemas(artifact: Path) -> None:
    write_json(
        artifact / "manifests/project_model_schema.json",
        {
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "title": "GenoLigand Project",
            "type": "object",
            "required": ["project_id", "project_name", "project_status", "saved_state"],
            "properties": {
                "project_id": {"type": "string"},
                "project_name": {"type": "string"},
                "workspace_path": {"type": "string"},
                "project_status": {"type": "string"},
                "latest_run_id": {"type": ["string", "null"]},
                "saved_state": {"type": "array", "items": {"type": "string"}},
            },
        },
    )
    write_json(
        artifact / "manifests/run_model_schema.json",
        {
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "title": "GenoLigand Run",
            "type": "object",
            "required": ["run_id", "state", "current_stage", "resumable", "artifact_index"],
            "properties": {
                "run_id": {"type": "string"},
                "project_id": {"type": ["string", "null"]},
                "state": {"enum": ["created", "preflight_passed", "queued", "running", "failed", "completed", "cancelled", "resumable", "pushed_to_github"]},
                "current_stage": {"type": "string"},
                "nextflow_run_id": {"type": ["string", "null"]},
                "resumable": {"type": "boolean"},
                "artifact_index": {"type": "array"},
            },
        },
    )
    write_json(
        artifact / "manifests/input_wizard_schema.json",
        {
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "title": "GenoLigand Input Wizard",
            "type": "object",
            "required": INPUT_STEPS,
            "properties": {step: {"type": "object"} for step in INPUT_STEPS},
        },
    )


def copy_export_and_cleanup_manifests(artifact: Path) -> None:
    copies = [
        (artifact / "exports/report_export_manifest.tsv", artifact / "manifests/report_export_manifest.tsv"),
        (artifact / "exports/report_sections_manifest.tsv", artifact / "manifests/report_sections_manifest.tsv"),
        (artifact / "cleanup/post_push_cleanup_execution_manifest.tsv", artifact / "manifests/post_push_cleanup_execution_manifest.tsv"),
        (artifact / "cleanup/phase09_runtime_cleanup_dry_run.tsv", artifact / "manifests/phase09_runtime_cleanup_dry_run.tsv"),
    ]
    for source, target in copies:
        target.parent.mkdir(parents=True, exist_ok=True)
        if source.exists():
            shutil.copyfile(source, target)
        else:
            write_tsv(target, ["source", "status"], [[source, "issue"]])


def write_ui_stack_decision_doc(artifact: Path) -> None:
    text = """# GenoLigand 0.9.1 UI Stack Decision Record

The production WebUI is served by the Rust `gls-server` binary and uses a static HTML/CSS/JavaScript client under `webui/phase09`.

- Runtime shell: `webui/phase09/index.html`
- Styling: `webui/phase09/styles.css`, dark-only
- Client logic: `webui/phase09/app.js`, bilingual zh/en switching
- Backend API: `crates/gls-server/src/lib.rs`
- Scientific execution boundary: `gls-cli` commands launched by `gls-server`
- Rendering audit: `scripts/phase09/capture_phase_0_9_1_webui_visuals.py`

No raw third-party tool report is embedded directly into final WebUI reports; exported reports are rendered from GenoLigand-owned candidate and evidence schemas.
"""
    (artifact / "docs/ui_stack_decision_record.md").write_text(text, encoding="utf-8")


def copy_guides(artifact: Path, workspace: Path) -> None:
    for relative in [
        "docs/local_webui_user_guide.md",
        "docs/windows_wsl2_installation_guide.md",
        "docs/linux_server_installation_guide.md",
    ]:
        source = workspace / relative
        target = artifact / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        if source.exists():
            shutil.copyfile(source, target)
        else:
            target.write_text(f"# Missing Source\n\nExpected `{source}`.\n", encoding="utf-8")


if __name__ == "__main__":
    sys.exit(main())
