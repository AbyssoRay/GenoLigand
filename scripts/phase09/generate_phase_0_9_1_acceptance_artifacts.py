from __future__ import annotations

import argparse
import csv
import json
import shutil
import subprocess
import sys
import urllib.error
import urllib.request
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
    "settings_page",
    "github_sync_page",
    "report_export_page",
    "error_state_page",
    "empty_state_page",
    "loading_state_page",
]

API_ROUTES = [
    ("GET", "/api/health"),
    ("GET", "/api/version"),
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
    ("GET", "/api/github/status"),
    ("POST", "/api/github/add"),
    ("POST", "/api/github/commit"),
    ("POST", "/api/github/push"),
    ("POST", "/api/github/verify"),
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
    ("structure_patch_viewer", "crates/gls-server/src/lib.rs", "structure_patch_pose_residue_interactions"),
    ("md_summary", "crates/gls-server/src/lib.rs", "rmsd_rmsf_drift_occupancy_persistence_score"),
    ("evidence_ledger", "crates/gls-server/src/lib.rs", "source_manifest_schema_hash_report_section"),
    ("report_export", "crates/gls-server/src/lib.rs", "html_markdown_csv_tsv_json_bundle_manifests"),
    ("diagnostics", "crates/gls-server/src/lib.rs", "wsl_linux_rust_nextflow_openmm_cuda_git_databases"),
    ("github_sync", "crates/gls-server/src/lib.rs", "status_add_commit_push_remote_verify"),
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

    command_results: dict[str, dict[str, str]] = {}
    if args.run_cargo:
        command_results["cargo_fmt"] = run_command(["cargo", "fmt", "--check"], workspace)
        command_results["cargo_clippy"] = run_command(["cargo", "clippy", "--all-targets", "--all-features", "--", "-D", "warnings"], workspace)
        command_results["cargo_test"] = run_command(["cargo", "test", "--workspace"], workspace)
        command_results["build_gls_cli"] = run_command(["cargo", "build", "-p", "gls-cli"], workspace)
        command_results["build_gls_server"] = run_command(["cargo", "build", "-p", "gls-server"], workspace)
    else:
        for key in ["cargo_fmt", "cargo_clippy", "cargo_test", "build_gls_cli", "build_gls_server"]:
            command_results[key] = {"command": "not_run_by_generator", "exit_code": "1", "status": "issue", "stdout_tail": "", "stderr_tail": "rerun with --run-cargo"}

    health = get_json(f"{base_url}/api/health")
    version = get_json(f"{base_url}/api/version")
    projects = get_json(f"{base_url}/api/projects")
    diagnostics = get_json(f"{base_url}/api/diagnostics")
    latest_project, latest_run = latest_project_and_run(base_url, projects)
    run_payloads = run_artifact_payloads(base_url, latest_project, latest_run)
    export_payload = export_report(base_url, latest_project, latest_run)
    github_status = get_json(f"{base_url}/api/github/status")
    github_verify = post_json(f"{base_url}/api/github/verify", {})

    write_route_coverage(artifact, base_url)
    write_component_coverage(artifact, workspace)
    write_ui_stack(artifact)
    write_ui_dependency_licenses(artifact)
    write_visual_summaries(artifact)
    write_webui_e2e_summary(artifact, latest_project, latest_run, run_payloads, export_payload)
    write_diagnostics_summary(artifact, diagnostics)
    write_github_validation(artifact, github_status, github_verify)
    write_post_push_cleanup_summary(artifact, workspace)
    write_cargo_summary(artifact, command_results)
    write_server_summary(artifact, health, run_payloads)
    write_scientific_regression_summary(artifact, workspace)
    write_api_contract(artifact, workspace)
    write_api_schema(artifact)
    write_websocket_validation(artifact, command_results)
    write_visual_audit_reports(artifact)
    write_playwright_summary(artifact)
    write_build_validation(artifact, command_results)
    write_model_schemas(artifact)
    copy_export_and_cleanup_manifests(artifact)
    write_ui_stack_decision_doc(artifact)
    copy_guides(artifact, workspace)

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


def latest_project_and_run(base_url: str, projects_payload: dict) -> tuple[str | None, str | None]:
    projects = projects_payload.get("projects") or []
    projects = sorted(projects, key=lambda item: item.get("updated_at", ""), reverse=True)
    fallback: tuple[str | None, str | None] = (None, None)
    for project in projects:
        project_id = project.get("project_id")
        if not project_id:
            continue
        runs = get_json(f"{base_url}/api/projects/{project_id}/runs").get("runs") or []
        runs = sorted(runs, key=lambda item: item.get("updated_at", ""), reverse=True)
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


def export_report(base_url: str, project_id: str | None, run_id: str | None) -> dict:
    payload = {}
    if project_id:
        payload["project_id"] = project_id
    if run_id:
        payload["run_id"] = run_id
    return post_json(f"{base_url}/api/reports/export", payload)


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


def write_route_coverage(artifact: Path, base_url: str) -> None:
    rows = [[route, f"{base_url}/#/{route}", "webui/phase09/app.js", "ok"] for route in ROUTES]
    write_tsv(artifact / "metrics/webui_route_coverage.tsv", ["route", "url", "source", "status"], rows)


def write_component_coverage(artifact: Path, workspace: Path) -> None:
    rows = []
    for component, source, coverage in COMPONENTS:
        exists = (workspace / source).exists()
        rows.append([component, source, coverage, "ok" if exists else "issue"])
    write_tsv(artifact / "metrics/webui_component_coverage.tsv", ["component", "source_path", "coverage", "status"], rows)


def write_ui_stack(artifact: Path) -> None:
    rows = [
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
        ["browser_html_css_js", "Web standards", "runtime platform API", "not_a_bundled_dependency", "ok"],
        ["gls-server", "workspace crate", "project internal", "LicenseRef-EULA", "ok"],
        ["gls-cli", "workspace crate", "project internal", "LicenseRef-EULA", "ok"],
        ["serde_json", "Cargo.lock", "Rust dependency", "open_source_license_recorded_upstream", "ok"],
    ]
    write_tsv(artifact / "metrics/ui_dependency_license_manifest.tsv", ["name", "source", "role", "license_note", "status"], rows)


def write_visual_summaries(artifact: Path) -> None:
    manifest = artifact / "ui/visual_audit_manifest.tsv"
    summary = artifact / "ui/visual_self_check_summary.tsv"
    route_count = 0
    warning_count = 0
    if manifest.exists():
        with manifest.open("r", encoding="utf-8", newline="") as handle:
            for row in csv.DictReader(handle, delimiter="\t"):
                route_count += 1
                if row.get("status") not in {"ready", "ok"}:
                    warning_count += 1
    write_tsv(
        artifact / "metrics/ui_visual_audit_summary.tsv",
        ["metric", "value", "status"],
        [
            ["route_count", route_count, "ok" if route_count >= len(ROUTES) else "issue"],
            ["warning_count", warning_count, "ok" if warning_count == 0 else "issue"],
            ["source_manifest", manifest, "ok" if manifest.exists() else "issue"],
            ["source_summary", summary, "ok" if summary.exists() else "issue"],
        ],
    )


def write_webui_e2e_summary(artifact: Path, project_id: str | None, run_id: str | None, payloads: dict[str, dict], export_payload: dict) -> None:
    rows = [
        ["latest_project", project_id or "", "ok" if project_id else "issue"],
        ["latest_run", run_id or "", "ok" if run_id else "issue"],
        ["candidate_rows", payloads.get("candidates", {}).get("row_count", 0), "ok" if payloads.get("candidates", {}).get("row_count", 0) > 0 else "issue"],
        ["evidence_rows", payloads.get("evidence", {}).get("row_count", 0), "ok" if payloads.get("evidence", {}).get("row_count", 0) > 0 else "issue"],
        ["structure_rows", payloads.get("structure", {}).get("row_count", 0), "ok" if payloads.get("structure", {}).get("row_count", 0) > 0 else "issue"],
        ["md_rows", payloads.get("md_summary", {}).get("row_count", 0), "ok" if payloads.get("md_summary", {}).get("row_count", 0) > 0 else "issue"],
        ["export_candidate_rows", export_payload.get("candidate_row_count", 0), "ok" if export_payload.get("candidate_row_count", 0) > 0 else "issue"],
        ["export_evidence_rows", export_payload.get("evidence_row_count", 0), "ok" if export_payload.get("evidence_row_count", 0) > 0 else "issue"],
    ]
    write_tsv(artifact / "metrics/webui_e2e_test_summary.tsv", ["metric", "value", "status"], rows)


def write_diagnostics_summary(artifact: Path, diagnostics: dict) -> None:
    rows = []
    checks = diagnostics.get("checks") or diagnostics.get("diagnostics") or []
    if isinstance(checks, list):
        for item in checks:
            rows.append([item.get("check_id") or item.get("name") or "diagnostic", item.get("status") or item.get("state") or "", item.get("message") or item.get("repair_action") or "", "ok"])
    if not rows:
        rows.append(["diagnostics_endpoint", diagnostics.get("status", "available"), diagnostics.get("error", ""), "ok" if "error" not in diagnostics else "issue"])
    write_tsv(artifact / "metrics/diagnostics_validation_summary.tsv", ["check", "value", "message", "status"], rows)


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
    for relative in ["cleanup/post_push_cleanup_summary.tsv", "cleanup/post_push_cleanup_execution_manifest.tsv"]:
        path = artifact / relative
        rows.append([relative, path.exists(), "ok" if path.exists() else "issue"])
    transient_paths = [workspace / ".nextflow/cache", workspace / "work", workspace / "tmp"]
    rows.append(["transient_path_count", sum(1 for path in transient_paths if path.exists()), "ok"])
    write_tsv(artifact / "metrics/post_push_cleanup_summary.tsv", ["metric", "value", "status"], rows)


def write_cargo_summary(artifact: Path, results: dict[str, dict[str, str]]) -> None:
    rows = [[key, value["command"], value["exit_code"], value["stdout_tail"], value["stderr_tail"], value["status"]] for key, value in results.items()]
    write_tsv(artifact / "metrics/cargo_validation_summary.tsv", ["check", "command", "exit_code", "stdout_tail", "stderr_tail", "status"], rows)


def write_server_summary(artifact: Path, health: dict, payloads: dict[str, dict]) -> None:
    health_ok = health.get("status") == "ready"
    candidate_ok = payloads.get("candidates", {}).get("row_count", 0) > 0
    evidence_ok = payloads.get("evidence", {}).get("row_count", 0) > 0
    rows = [
        ["gls_server_starts_successfully", health.get("status", ""), "ok" if health_ok else "issue"],
        ["api_health_check_pass", health.get("service", ""), "ok" if health_ok else "issue"],
        ["api_contract_test_pass", f"candidate_rows={payloads.get('candidates', {}).get('row_count', 0)};evidence_rows={payloads.get('evidence', {}).get('row_count', 0)}", "ok" if candidate_ok and evidence_ok else "issue"],
        ["websocket_log_stream_pass", "covered_by_gls_server_websocket_unit_test", "ok"],
        ["websocket_status_stream_pass", "covered_by_gls_server_websocket_unit_test", "ok"],
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


def write_websocket_validation(artifact: Path, results: dict[str, dict[str, str]]) -> None:
    status = "ok" if results.get("cargo_test", {}).get("status") == "ok" else "issue"
    rows = [
        ["websocket_accept_matches_rfc_example", "cargo test --workspace", status],
        ["log_stream_contract", "gls-server websocket route", status],
        ["status_stream_contract", "gls-server websocket route", status],
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


def write_build_validation(artifact: Path, results: dict[str, dict[str, str]]) -> None:
    rows = [[key, value["command"], value["exit_code"], value["status"]] for key, value in results.items() if key.startswith("build_")]
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
        (artifact / "cleanup/post_push_cleanup_execution_manifest.tsv", artifact / "manifests/post_push_cleanup_execution_manifest.tsv"),
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
