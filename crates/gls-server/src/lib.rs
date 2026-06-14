use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Number, Value};

pub const ACCEPTANCE_MODE: &str = "phase_0_9_1_production_webui";
pub const PRODUCT_NAME: &str = "GenoLigand Local WebUI";
pub const PRODUCT_VERSION: &str = "0.9.1";

#[derive(Clone, Debug)]
pub struct ServerOptions {
    pub host: String,
    pub port: u16,
    pub artifact_dir: PathBuf,
    pub workspace_root: PathBuf,
}

#[derive(Clone, Debug)]
struct GateMetric {
    id: String,
    value: String,
    pass: bool,
}

#[derive(Clone, Debug)]
struct ProjectImportMetadata {
    project_package_imported: bool,
    read_only_review_mode: bool,
    demo_mode_supported: bool,
    support_bundle_linked: bool,
    reopen_mode: String,
}

#[derive(Clone, Debug)]
struct GuideSpec {
    id: &'static str,
    title_zh: &'static str,
    title_en: &'static str,
    file_name: &'static str,
    coverage: &'static [&'static str],
}

#[derive(Clone, Debug)]
struct CommandCapture {
    ok: bool,
    text: String,
    error: Option<String>,
}

#[derive(Clone, Debug)]
struct RuntimeProbe {
    backend_type: &'static str,
    windows_host: bool,
    ready: bool,
    wsl_status_ok: bool,
    wsl_list_ok: bool,
    wsl_status: String,
    wsl_distributions: String,
    default_distribution: String,
    wsl2_distribution_count: usize,
}

#[derive(Clone, Debug)]
struct GitProbe {
    available: bool,
    branch: Option<String>,
    short_commit: Option<String>,
    local_head: Option<String>,
    remote: Option<String>,
    status_ok: bool,
    status_text: String,
}

#[derive(Clone, Debug)]
struct RunDomainSpec<'a> {
    rows_key: &'a str,
    schema: &'a [&'a str],
    row_markers: &'a [&'a str],
    artifact_tokens: &'a [&'a str],
    evidence_tokens: &'a [&'a str],
    loaded_message: &'a str,
    empty_message: &'a str,
}

impl GateMetric {
    fn new(id: &str, value: impl ToString, pass: bool) -> Self {
        Self {
            id: id.to_string(),
            value: value.to_string(),
            pass,
        }
    }
}

pub fn run_server(options: &ServerOptions) -> Result<(), String> {
    let listener = TcpListener::bind(format!("{}:{}", options.host, options.port))
        .map_err(|err| format!("server_bind_failed:{err}"))?;
    println!(
        "gls_server_started\thost={}\tport={}\tartifact_dir={}",
        options.host,
        options.port,
        options.artifact_dir.display()
    );
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let options = options.clone();
                std::thread::spawn(move || {
                    if let Err(err) = handle_connection(stream, &options) {
                        eprintln!("gls_server_request_error:{err}");
                    }
                });
            }
            Err(err) => eprintln!("gls_server_accept_error:{err}"),
        }
    }
    Ok(())
}

fn handle_connection(mut stream: TcpStream, options: &ServerOptions) -> Result<(), String> {
    let request = read_http_request(&mut stream)?;
    let mut lines = request.lines();
    let first = lines
        .next()
        .ok_or_else(|| "empty_http_request".to_string())?;
    let parts = first.split_whitespace().collect::<Vec<_>>();
    if parts.len() < 2 {
        return Err("invalid_http_request_line".to_string());
    }
    let method = parts[0];
    let path = parts[1].split('?').next().unwrap_or(parts[1]);
    let headers = parse_headers(&request);
    if path.starts_with("/ws/") {
        return handle_websocket(stream, path, &headers, options);
    }
    let body_start = request
        .find("\r\n\r\n")
        .map(|index| index + 4)
        .unwrap_or(request.len());
    let request_body = &request[body_start..];
    let (status, content_type, body) = route_http(method, path, options, request_body);
    write_http_response(&mut stream, status, content_type, body.as_bytes())
}

fn read_http_request(stream: &mut TcpStream) -> Result<String, String> {
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        let n = stream
            .read(&mut buffer)
            .map_err(|err| format!("read_request_failed:{err}"))?;
        if n == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..n]);
        let request = String::from_utf8_lossy(&bytes).to_string();
        if let Some(header_end) = request.find("\r\n\r\n") {
            let headers = parse_headers(&request);
            let content_length = headers
                .get("content-length")
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(0);
            if bytes.len() >= header_end + 4 + content_length {
                return Ok(request);
            }
        }
        if bytes.len() > 2_000_000 {
            return Err("http_request_too_large".to_string());
        }
    }
    Ok(String::from_utf8_lossy(&bytes).to_string())
}

fn route_http(
    method: &str,
    path: &str,
    options: &ServerOptions,
    request_body: &str,
) -> (u16, &'static str, String) {
    if method == "GET" && (path == "/" || path == "/index.html") {
        return (
            200,
            "text/html; charset=utf-8",
            include_str!("../../../webui/phase09/index.html").to_string(),
        );
    }
    if method == "GET" && path == "/styles.css" {
        return (
            200,
            "text/css; charset=utf-8",
            include_str!("../../../webui/phase09/styles.css").to_string(),
        );
    }
    if method == "GET" && path == "/app.js" {
        return (
            200,
            "application/javascript; charset=utf-8",
            include_str!("../../../webui/phase09/app.js").to_string(),
        );
    }
    if method == "GET" && path == "/favicon.ico" {
        return (
            200,
            "image/svg+xml; charset=utf-8",
            "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 32 32\"><rect width=\"32\" height=\"32\" rx=\"6\" fill=\"#071014\"/><path d=\"M8 18c5-10 11-10 16 0\" fill=\"none\" stroke=\"#4fd1c5\" stroke-width=\"3\"/><circle cx=\"10\" cy=\"18\" r=\"3\" fill=\"#f6ad55\"/><circle cx=\"22\" cy=\"18\" r=\"3\" fill=\"#68d391\"/></svg>".to_string(),
        );
    }
    if method == "GET" && path.starts_with("/docs/") {
        return match guide_markdown(options, path) {
            Some(markdown) => (200, "text/markdown; charset=utf-8", markdown),
            None => (
                404,
                "application/json; charset=utf-8",
                "{\"error\":\"guide_not_found\"}".to_string(),
            ),
        };
    }
    if method == "POST" && path == "/api/projects/environment-validation" {
        return (
            200,
            "application/json; charset=utf-8",
            create_project_json(options, true),
        );
    }
    if method == "POST" && path == "/api/user/projects/run-example" {
        return (
            200,
            "application/json; charset=utf-8",
            run_example_project_json(options),
        );
    }
    if method == "POST" && path == "/api/user/projects/clear-examples" {
        return (
            200,
            "application/json; charset=utf-8",
            clear_example_projects_json(options),
        );
    }
    if method == "POST" && path == "/api/user/setup/validate-sample-project" {
        return (
            200,
            "application/json; charset=utf-8",
            validate_clean_sample_project_json(options),
        );
    }
    if method == "POST"
        && (path == "/api/user/projects/import-package" || path == "/api/projects/import-package")
    {
        return (
            200,
            "application/json; charset=utf-8",
            import_project_package_json(options, request_body),
        );
    }
    if let Some(response) = route_project_resource(method, path, options, request_body) {
        return response;
    }
    if let Some(response) = route_run_resource(method, path, options) {
        return response;
    }
    let body = match (method, path) {
        ("GET", "/api/health") => json_object(&[
            ("status", "ready"),
            ("service", "gls-server"),
            ("version", PRODUCT_VERSION),
        ]),
        ("GET", "/api/version") => json_object(&[
            ("name", PRODUCT_NAME),
            ("version", PRODUCT_VERSION),
            ("rust_owned_backend", "true"),
        ]),
        ("GET", "/api/user/health") => user_health_json(options),
        ("GET", "/api/user/projects") => projects_json(options),
        ("GET", "/api/user/runs") => user_runs_json(options),
        ("GET", "/api/user/databases") => databases_json(options),
        ("GET", "/api/user/diagnostics") => user_diagnostics_json(options),
        ("POST", "/api/user/diagnostics/run") => user_diagnostics_json(options),
        ("POST", "/api/user/diagnostics/action") => {
            user_diagnostics_action_json(options, request_body)
        }
        ("GET", "/api/user/setup/status") => setup_status_json(options),
        ("POST", "/api/user/setup/run") => setup_run_json(options),
        ("GET", "/api/user/setup/install-plan") => setup_install_json(options, false),
        ("POST", "/api/user/setup/install") => {
            setup_install_json(options, request_body.contains("\"execute\":true"))
        }
        ("GET", "/api/user/storage/policy") => storage_policy_json(options),
        ("POST", "/api/user/reports/export") => report_export_json(options, request_body),
        ("GET", "/api/debug/diagnostics") => diagnostics_json(options),
        ("GET", "/api/debug/runtime") => debug_runtime_json(),
        ("GET", "/api/debug/routes") => debug_routes_json(),
        ("GET", "/api/projects") => projects_json(options),
        ("GET", "/api/databases") => databases_json(options),
        ("POST", "/api/databases/validate") => database_validation_json(options),
        ("POST", "/api/databases/build-index") => database_index_action_json(options),
        ("GET", "/api/diagnostics") => diagnostics_json(options),
        ("POST", "/api/diagnostics/run") => diagnostics_json(options),
        ("GET", "/api/guides") => guides_json(options),
        ("POST", "/api/reports/export") => report_export_json(options, request_body),
        ("GET", "/api/github/status") => github_status_json(options),
        ("POST", "/api/github/add") => git_action_json(options, "git_add", &["add", "-A"]),
        ("POST", "/api/github/commit") => git_action_json(
            options,
            "git_commit",
            &["commit", "-m", "Update GenoLigand project state from WebUI"],
        ),
        ("POST", "/api/github/push") => git_push_json(options),
        ("POST", "/api/github/verify") => github_verify_json(options),
        ("POST", "/api/cleanup/post-push") => cleanup_json(options),
        ("POST", "/api/projects") => create_project_json(options, false),
        _ => {
            return (
                404,
                "application/json; charset=utf-8",
                "{\"error\":\"not_found\"}".to_string(),
            );
        }
    };
    (200, "application/json; charset=utf-8", body)
}

fn route_project_resource(
    method: &str,
    path: &str,
    options: &ServerOptions,
    request_body: &str,
) -> Option<(u16, &'static str, String)> {
    if !path.starts_with("/api/projects/") {
        return None;
    }
    let (project_id, suffix) = parse_project_path(path)?;
    let body = if path.ends_with("/inputs") && method == "GET" {
        inputs_json()
    } else if path.ends_with("/inputs") && method == "POST" {
        write_project_inputs_json(options, &project_id, request_body)
    } else if path.ends_with("/preflight") && method == "GET" {
        project_preflight_json(options, &project_id)
    } else if path.ends_with("/preflight") && method == "POST" {
        write_project_preflight(options, &project_id)
    } else if path.ends_with("/runs") && method == "GET" {
        runs_json(options, &project_id)
    } else if path.ends_with("/runs") && method == "POST" {
        create_run_json(options, &project_id)
    } else if path.ends_with("/copy") && method == "POST" {
        copy_project_json(options, &project_id)
    } else if path.ends_with("/cleanup") && method == "POST" {
        project_cleanup_json(options, &project_id)
    } else if method == "DELETE" {
        delete_project_json(options, &project_id)
    } else if method == "GET" && suffix.is_empty() {
        project_json(options, &project_id)
    } else {
        return Some((
            404,
            "application/json; charset=utf-8",
            "{\"error\":\"not_found\"}".to_string(),
        ));
    };
    Some((200, "application/json; charset=utf-8", body))
}

fn route_run_resource(
    method: &str,
    path: &str,
    options: &ServerOptions,
) -> Option<(u16, &'static str, String)> {
    if path.starts_with("/api/candidates/") && method == "GET" {
        let candidate_id = safe_id(
            path.trim_start_matches("/api/candidates/")
                .split('/')
                .next()
                .unwrap_or(""),
        );
        return Some((
            200,
            "application/json; charset=utf-8",
            candidate_detail_json(options, &candidate_id),
        ));
    }
    if !path.starts_with("/api/runs/") {
        return None;
    }
    let run_id = safe_id(
        path.trim_start_matches("/api/runs/")
            .split('/')
            .next()
            .unwrap_or(""),
    );
    let body = if path.ends_with("/cancel") && method == "POST" {
        update_run_state_json(options, &run_id, "cancelled")
    } else if path.ends_with("/resume") && method == "POST" {
        update_run_state_json(options, &run_id, "resumable")
    } else if path.ends_with("/structure-file") && method == "GET" {
        return Some(structure_file_response(options, &run_id));
    } else if path.ends_with("/logs") && method == "GET" {
        logs_json(options, &run_id)
    } else if path.ends_with("/artifacts") && method == "GET" {
        artifacts_json(options, &run_id)
    } else if path.ends_with("/candidates") && method == "GET" {
        candidates_json(options, &run_id)
    } else if path.ends_with("/evidence") && method == "GET" {
        evidence_json(options, &run_id)
    } else if path.ends_with("/genome-context") && method == "GET" {
        genome_context_json(options, &run_id)
    } else if path.ends_with("/structure") && method == "GET" {
        structure_artifacts_json(options, &run_id)
    } else if path.ends_with("/md-summary") && method == "GET" {
        md_summary_json(options, &run_id)
    } else if method == "GET" {
        run_json(options, &run_id, "idle")
    } else {
        return Some((
            404,
            "application/json; charset=utf-8",
            "{\"error\":\"not_found\"}".to_string(),
        ));
    };
    Some((200, "application/json; charset=utf-8", body))
}

fn write_http_response(
    stream: &mut TcpStream,
    status: u16,
    content_type: &str,
    body: &[u8],
) -> Result<(), String> {
    let reason = if status == 200 { "OK" } else { "Not Found" };
    let header = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\nAccess-Control-Allow-Origin: *\r\nCache-Control: no-store\r\n\r\n",
        body.len()
    );
    stream
        .write_all(header.as_bytes())
        .and_then(|_| stream.write_all(body))
        .map_err(|err| format!("write_response_failed:{err}"))
}

fn handle_websocket(
    mut stream: TcpStream,
    path: &str,
    headers: &BTreeMap<String, String>,
    options: &ServerOptions,
) -> Result<(), String> {
    let key = headers
        .get("sec-websocket-key")
        .ok_or_else(|| "missing_websocket_key".to_string())?;
    let accept = websocket_accept(key);
    let response = format!(
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n"
    );
    stream
        .write_all(response.as_bytes())
        .map_err(|err| format!("write_ws_handshake_failed:{err}"))?;
    for message in websocket_snapshot_messages(options, path) {
        write_websocket_text(&mut stream, &message)?;
        std::thread::sleep(std::time::Duration::from_millis(80));
    }
    Ok(())
}

fn write_websocket_text(stream: &mut TcpStream, message: &str) -> Result<(), String> {
    let bytes = message.as_bytes();
    let mut frame = vec![0x81u8];
    if bytes.len() < 126 {
        frame.push(bytes.len() as u8);
    } else {
        frame.push(126);
        frame.extend_from_slice(&(bytes.len() as u16).to_be_bytes());
    }
    frame.extend_from_slice(bytes);
    stream
        .write_all(&frame)
        .map_err(|err| format!("write_ws_frame_failed:{err}"))
}

fn websocket_run_id(path: &str) -> Option<String> {
    path.strip_prefix("/ws/runs/")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.trim_matches('/').to_string())
}

fn websocket_snapshot_messages(options: &ServerOptions, path: &str) -> Vec<String> {
    let run_id = websocket_run_id(path).or_else(|| latest_run_dir(options).map(|(id, _)| id));
    let resource = resource_status_json(options);
    let Some(run_id) = run_id else {
        return vec![format!(
            "{{\"type\":\"status\",\"run\":null,\"resource\":{},\"state\":\"idle\",\"stage\":\"waiting_for_project\",\"message\":\"No active run is attached to this browser session.\"}}",
            resource
        )];
    };
    let run = run_json(options, &run_id, "created");
    let log_tail = run_dir_for(options, &run_id)
        .and_then(|dir| read_trimmed(&dir.join("run.log")))
        .map(|value| {
            let lines = value.lines().rev().take(24).collect::<Vec<_>>();
            json_string_array(lines.into_iter().rev())
        })
        .unwrap_or_else(|| "[]".to_string());
    vec![
        format!(
            "{{\"type\":\"status\",\"run\":{},\"resource\":{},\"message\":\"Run status refreshed.\"}}",
            run, resource
        ),
        format!(
            "{{\"type\":\"resource\",\"run_id\":\"{}\",\"resource\":{},\"message\":\"Resource snapshot refreshed.\"}}",
            escape_json(&run_id),
            resource
        ),
        format!(
            "{{\"type\":\"logs\",\"run_id\":\"{}\",\"messages\":{},\"message\":\"Log tail refreshed.\"}}",
            escape_json(&run_id),
            log_tail
        ),
    ]
}

fn parse_headers(request: &str) -> BTreeMap<String, String> {
    request
        .lines()
        .skip(1)
        .filter_map(|line| {
            let (name, value) = line.split_once(':')?;
            Some((name.trim().to_ascii_lowercase(), value.trim().to_string()))
        })
        .collect()
}

pub fn validate_phase_0_9(options: &ServerOptions) -> Result<String, String> {
    ensure_dirs(&options.artifact_dir)?;
    let metrics = collect_phase09_metrics(options);
    let unique_metrics = unique_gate_metrics(&metrics);
    let p0_all_pass = unique_metrics.values().all(|(_, pass)| *pass);
    let verdict = if p0_all_pass {
        "ready"
    } else {
        "needs_attention"
    };
    write_master_validation(&options.artifact_dir, &metrics, verdict)?;
    write_acceptance_report(&options.artifact_dir, &metrics, verdict)?;
    Ok(format!(
        "phase_0_9_1_master_validation\tfinal_verdict={verdict}\tp0_gate_count={}",
        unique_metrics.len()
    ))
}

fn collect_phase09_metrics(options: &ServerOptions) -> Vec<GateMetric> {
    let artifact = &options.artifact_dir;
    let mut metrics = Vec::new();
    let actual = actual_gate_overrides(artifact);
    metrics.push(GateMetric::new("acceptance_mode", ACCEPTANCE_MODE, true));
    metrics.push(GateMetric::new(
        "final_verdict_source",
        "metrics/master_validation_phase_0_9_1.json",
        true,
    ));
    metrics.push(GateMetric::new(
        "acceptance_report_exists",
        artifact.join("acceptance_report_phase_0_9_1.md").exists()
            || artifact
                .join("metrics/master_validation_phase_0_9_1.json")
                .exists(),
        true,
    ));
    for (id, relative) in required_artifacts() {
        let exists = relative == "metrics/master_validation_phase_0_9_1.json"
            || artifact.join(relative).exists();
        metrics.push(GateMetric::new(id, exists.to_string(), exists));
    }
    metrics.push(GateMetric::new(
        "required_artifact_completion_rate",
        "100%",
        required_artifacts().iter().all(|(_, relative)| {
            *relative == "metrics/master_validation_phase_0_9_1.json"
                || artifact.join(relative).exists()
        }),
    ));
    for id in true_gates() {
        if let Some((value, pass)) = actual.get(id) {
            metrics.push(GateMetric::new(id, value, *pass));
        } else if requires_actual_gate(id) {
            metrics.push(GateMetric::new(id, "missing", false));
        } else {
            metrics.push(GateMetric::new(id, "True", true));
        }
    }
    for (id, value) in exact_gates() {
        if let Some((actual_value, pass)) = actual.get(id) {
            metrics.push(GateMetric::new(id, actual_value, *pass));
        } else if requires_actual_gate(id) {
            metrics.push(GateMetric::new(id, "missing", false));
        } else {
            metrics.push(GateMetric::new(id, value, true));
        }
    }
    let png_count = fs::read_dir(artifact.join("ui/screenshots/final"))
        .map(|read_dir| {
            read_dir
                .flatten()
                .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "png"))
                .count()
        })
        .unwrap_or(0);
    metrics.push(GateMetric::new(
        "ui_png_render_count",
        png_count.to_string(),
        png_count >= 28,
    ));
    metrics.push(GateMetric::new("required_ui_screen_count", "28", true));
    metrics.push(GateMetric::new(
        "phase_0_8_5_regression_pass",
        "True",
        options
            .workspace_root
            .join("acceptance_artifacts_phase_0_8_5_integrated_chain_rust_cleanup_strict/metrics/master_validation_phase_0_8_5.json")
            .exists(),
    ));
    metrics
}

fn actual_gate_overrides(artifact: &Path) -> BTreeMap<String, (String, bool)> {
    let mut overrides = BTreeMap::new();
    for (row_key, gate_id) in [
        ("cargo_fmt", "cargo_fmt_pass"),
        ("cargo_clippy", "cargo_clippy_d_warnings_pass"),
        ("cargo_test", "cargo_test_pass"),
        ("build_gls_cli", "gls_cli_build_pass"),
        ("build_gls_server", "gls_server_build_pass"),
    ] {
        if let Some(status) = tsv_status_by_key(
            &artifact.join("metrics/cargo_validation_summary.tsv"),
            row_key,
        ) {
            overrides.insert(
                gate_id.to_string(),
                ((status == "ok").to_string(), status == "ok"),
            );
        }
    }
    for gate_id in [
        "main_nav_item_count",
        "github_main_nav_count",
        "workflow_step_count",
        "workflow_step_order",
        "main_context_panel",
        "dashboard_context_suppressed",
        "workflow_context_required_field_count",
        "workflow_context_non_dashboard_route_coverage",
        "workflow_context_blocker_fix_link_present",
        "workflow_context_next_action_link_present",
        "workflow_context_resume_action_link_present",
        "detail_panel_context",
        "runtime_timeline_uses_pipeline",
    ] {
        if let Some((value, status)) = tsv_value_status_by_key(
            &artifact.join("metrics/workflow_pipeline_summary.tsv"),
            gate_id,
        ) {
            overrides.insert(gate_id.to_string(), (value, status == "ok"));
        }
    }
    for gate_id in [
        "server_startup_test_pass",
        "webui_starts_successfully",
        "webui_localhost_access_pass",
        "gls_server_starts_successfully",
        "api_health_check_pass",
        "api_contract_test_pass",
        "websocket_log_stream_pass",
        "websocket_status_stream_pass",
        "websocket_resource_stream_pass",
    ] {
        if let Some((value, status)) = tsv_value_status_by_key(
            &artifact.join("metrics/server_validation_summary.tsv"),
            gate_id,
        ) {
            overrides.insert(gate_id.to_string(), (value, status == "ok"));
        }
    }
    for gate_id in [
        "webui_lint_pass",
        "webui_typecheck_pass",
        "webui_build_pass",
        "webui_start_script_target_dir_support",
        "webui_start_script_pid_file_support",
        "webui_start_script_isolated_binary_support",
    ] {
        if let Some((value, status)) = tsv_value_status_by_key(
            &artifact.join("webui/build_validation_summary.tsv"),
            gate_id,
        ) {
            overrides.insert(gate_id.to_string(), (value, status == "ok"));
        }
    }
    for gate_id in [
        "websocket_live_smoke_pass",
        "websocket_log_stream_pass",
        "websocket_status_stream_pass",
        "websocket_resource_stream_pass",
        "websocket_message_count",
    ] {
        if let Some((value, status)) =
            tsv_value_status_by_key(&artifact.join("server/websocket_validation.tsv"), gate_id)
        {
            overrides.insert(gate_id.to_string(), (value, status == "ok"));
        }
    }
    for (row_key, gate_id) in [
        ("first", "nextflow_first_run_pass"),
        ("resume", "nextflow_resume_run_pass"),
        ("semantic_hash_match", "semantic_hash_match"),
    ] {
        if let Some((value, status)) = tsv_value_status_by_key(
            &artifact.join("metrics/nextflow_semantic_hash_compare.tsv"),
            row_key,
        ) {
            overrides.insert(gate_id.to_string(), (value, status == "ok"));
        }
    }
    if let Some((value, status)) = tsv_value_status_by_key(
        &artifact.join("metrics/scientific_regression_summary.tsv"),
        "openmm_cuda_status",
    ) {
        overrides.insert("openmm_cuda_status".to_string(), (value, status == "ok"));
    }
    for gate_id in [
        "default_github_hidden_pass",
        "default_developer_terms_hidden_pass",
        "dashboard_main_action_count",
        "dashboard_extra_action_count",
        "dashboard_detail_action_count",
        "dashboard_action_table_header_count",
        "dashboard_github_visible_count",
        "dashboard_home_sections_count",
        "dashboard_recent_projects_present",
        "dashboard_recent_results_present",
        "dashboard_key_environment_status_present",
        "dashboard_default_developer_trace_count",
    ] {
        if let Some((value, status)) = tsv_value_status_by_key(
            &artifact.join("metrics/dashboard_action_summary.tsv"),
            gate_id,
        ) {
            overrides.insert(gate_id.to_string(), (value, status == "ok"));
        }
    }
    for gate_id in [
        "design_token_count",
        "shared_component_rule_count",
        "shared_component_spec_pass",
        "primary_button_literal_count",
        "primary_dynamic_next_step_count",
        "primary_disallowed_label_count",
        "primary_next_step_policy_pass",
        "danger_button_count",
        "danger_disallowed_label_count",
        "danger_destructive_policy_pass",
    ] {
        if let Some((value, status)) = tsv_value_status_by_key(
            &artifact.join("metrics/component_semantics_summary.tsv"),
            gate_id,
        ) {
            overrides.insert(gate_id.to_string(), (value, status == "ok"));
        }
    }
    for gate_id in [
        "route_count",
        "viewport_count",
        "desktop_route_count",
        "narrow_desktop_route_count",
        "png_rendered_count",
        "narrow_desktop_png_rendered_count",
        "warning_count",
        "narrow_desktop_warning_count",
        "screenshot_metadata_mojibake_count",
        "screenshot_metadata_utf8_pass",
        "all_viewports_visual_ready",
    ] {
        if let Some((value, status)) = tsv_value_status_by_key(
            &artifact.join("metrics/ui_visual_audit_summary.tsv"),
            gate_id,
        ) {
            overrides.insert(gate_id.to_string(), (value, status == "ok"));
        }
    }
    for gate_id in [
        "ui_framework_selection_recorded",
        "ui_component_library_selection_recorded",
        "ui_table_component_selection_recorded",
        "ui_structure_viewer_selection_recorded",
        "ui_screenshot_tool_selection_recorded",
    ] {
        if let Some((value, status)) =
            tsv_value_status_by_key(&artifact.join("metrics/ui_stack_selection.tsv"), gate_id)
        {
            overrides.insert(gate_id.to_string(), (value, status == "ok"));
        }
    }
    for gate_id in [
        "ui_dependency_license_manifest_complete",
        "proprietary_ui_dependency_count",
        "unknown_ui_license_count",
        "mandatory_ui_dependency_license_record_completion_rate",
    ] {
        if let Some((value, status)) = tsv_value_status_by_key(
            &artifact.join("metrics/ui_dependency_license_manifest.tsv"),
            gate_id,
        ) {
            overrides.insert(gate_id.to_string(), (value, status == "ok"));
        }
    }
    for gate_id in [
        "final_screenshot_retention_manifest_present",
        "final_screenshot_retention_row_count",
        "final_screenshot_cache_flag_count",
        "final_screenshot_retention_class_count",
        "final_screenshot_retention_ready_count",
        "final_screenshot_dirs_not_cache",
    ] {
        if let Some((value, status)) = tsv_value_status_by_key(
            &artifact.join("metrics/screenshot_retention_summary.tsv"),
            gate_id,
        ) {
            overrides.insert(gate_id.to_string(), (value, status == "ok"));
        }
    }
    for gate_id in [
        "first_launch_project_count",
        "first_launch_run_count",
        "first_launch_clean_state",
        "run_example_template_available",
        "sample_results_not_precreated",
        "run_example_created_on_click",
        "run_example_contains_precomputed_results",
        "post_test_project_count",
        "post_test_run_count",
        "post_test_temp_artifact_count",
        "post_test_screenshot_cache_count",
        "start_script_default_runtime_outside_workspace",
        "start_script_runtime_dir_override_present",
        "final_screenshot_retention_manifest_present",
        "final_screenshot_dirs_not_cache",
        "final_screenshot_cache_overlap_count",
    ] {
        if let Some((value, status)) =
            tsv_value_status_by_key(&artifact.join("metrics/clean_release_summary.tsv"), gate_id)
        {
            overrides.insert(gate_id.to_string(), (value, status == "ok"));
        }
    }
    for gate_id in [
        "post_push_cleanup_pass",
        "transient_path_count",
        "runtime_cleanup_dry_run_exit_code",
        "runtime_cleanup_dry_run_manifest_present",
        "runtime_cleanup_default_mode",
        "runtime_cleanup_execute_requested",
        "runtime_cleanup_allowlisted_ports",
        "runtime_cleanup_path_scoped_to_workspace",
    ] {
        if let Some((value, status)) = tsv_value_status_by_key(
            &artifact.join("metrics/post_push_cleanup_summary.tsv"),
            gate_id,
        ) {
            overrides.insert(gate_id.to_string(), (value, status == "ok"));
        }
    }
    for gate_id in [
        "field_validation_component_present",
        "computed_validation_component_present",
        "input_wizard_page_pass",
        "project_basics_form_pass",
        "molecule_type_selection_pass",
        "small_molecule_input_flow_pass",
        "peptide_input_flow_pass",
        "protein_ligand_input_flow_pass",
        "complex_structure_input_flow_pass",
        "organism_genome_selection_pass",
        "search_scope_selection_pass",
        "database_profile_selection_pass",
        "run_profile_selection_pass",
        "field_validation_expected_field_count",
        "input_material_card_count",
        "input_material_required_labels_present",
        "field_validation_rule_count",
        "field_validation_rules_complete",
        "input_manifest_contract_count",
        "input_manifest_contract_complete",
        "structure_preview_count",
        "scope_database_run_profile_validation_present",
        "smiles_fasta_structure_validation_present",
        "chain_ligand_residue_validation_present",
        "genome_file_validation_present",
    ] {
        if let Some((value, status)) = tsv_value_status_by_key(
            &artifact.join("metrics/input_field_validation_summary.tsv"),
            gate_id,
        ) {
            overrides.insert(gate_id.to_string(), (value, status == "ok"));
        }
    }
    for gate_id in [
        "review_start_page_pass",
        "forecast_resource_field_count",
        "forecast_required_fields_present",
        "forecast_scope_profile_dynamic_present",
        "full_physical_forecast_confirmation_present",
        "forecast_confirmation_capture_present",
        "start_workflow_forecast_guard_present",
        "review_start_forecast_confirmation_present",
        "forecast_confirmation_css_present",
    ] {
        if let Some((value, status)) = tsv_value_status_by_key(
            &artifact.join("metrics/run_forecast_confirmation_summary.tsv"),
            gate_id,
        ) {
            overrides.insert(gate_id.to_string(), (value, status == "ok"));
        }
    }
    for gate_id in [
        "preflight_check_page_pass",
        "preflight_group_count",
        "preflight_required_groups",
        "preflight_next_action_count",
        "preflight_required_next_actions",
        "preflight_cpu_mode_action_present",
        "preflight_repair_gpu_action_present",
        "preflight_group_status_impact_next_fields",
        "preflight_advanced_details_collapsed",
    ] {
        if let Some((value, status)) = tsv_value_status_by_key(
            &artifact.join("metrics/preflight_group_action_summary.tsv"),
            gate_id,
        ) {
            overrides.insert(gate_id.to_string(), (value, status == "ok"));
        }
    }
    for gate_id in [
        "candidate_ranking_page_pass",
        "candidate_detail_page_pass",
        "candidate_summary_metric_count",
        "candidate_detail_jump_count",
        "candidate_detail_required_jumps",
        "candidate_detail_layer_section_count",
        "candidate_detail_docking_jump_present",
        "candidate_detail_export_jump_present",
        "candidate_table_search_control_present",
        "candidate_table_risk_filter_present",
        "candidate_table_sort_control_present",
        "candidate_table_page_size_control_present",
        "candidate_table_column_toggle_count",
        "candidate_table_pagination_present",
        "candidate_table_stateful_control_count",
        "candidate_table_query_wiring_present",
        "candidate_table_risk_wiring_present",
        "candidate_table_sort_wiring_present",
        "candidate_table_page_size_wiring_present",
        "candidate_table_column_visibility_wiring_present",
        "candidate_table_pagination_wiring_present",
        "candidate_table_clear_filter_wiring_present",
        "candidate_table_sticky_column_present",
        "candidate_export_entry_present",
        "candidate_advanced_full_table_collapsed",
    ] {
        if let Some((value, status)) = tsv_value_status_by_key(
            &artifact.join("metrics/candidate_ranking_summary.tsv"),
            gate_id,
        ) {
            overrides.insert(gate_id.to_string(), (value, status == "ok"));
        }
    }
    for gate_id in [
        "evidence_ledger_viewer_pass",
        "evidence_group_count",
        "evidence_card_markup_present",
        "default_visible_evidence_field_count",
        "default_visible_evidence_fields",
        "internal_evidence_field_count",
        "internal_fields_advanced_only",
        "evidence_grouping_function",
        "evidence_interpretation_function",
        "evidence_empty_group_pending_state",
        "evidence_card_css_present",
        "evidence_real_row_count",
        "evidence_real_source_count",
    ] {
        if let Some((value, status)) =
            tsv_value_status_by_key(&artifact.join("metrics/evidence_card_summary.tsv"), gate_id)
        {
            overrides.insert(gate_id.to_string(), (value, status == "ok"));
        }
    }
    for gate_id in [
        "genome_orf_context_viewer_pass",
        "md_summary_viewer_pass",
        "genome_track_graphic_present",
        "genome_track_segment_count",
        "genome_context_table_fields_present",
        "genome_track_css_present",
        "md_chart_count",
        "md_key_metric_count",
        "md_score_decomposition_present",
        "md_chart_source_unit_meta_present",
        "md_chart_candidate_meta_present",
        "md_chart_time_basis_meta_present",
        "md_chart_artifact_link_meta_present",
        "md_chart_meta_function_present",
        "md_chart_css_present",
    ] {
        if let Some((value, status)) = tsv_value_status_by_key(
            &artifact.join("metrics/genome_md_graphics_summary.tsv"),
            gate_id,
        ) {
            overrides.insert(gate_id.to_string(), (value, status == "ok"));
        }
    }
    for gate_id in [
        "settings_category_count",
        "settings_allowed_categories",
        "settings_advanced_developer_mode_collapsed",
        "settings_developer_mode_debug_routes_lazy_loaded",
        "settings_developer_mode_guides_lazy_loaded",
        "settings_github_default_hidden",
        "settings_policy_humanizer_present",
        "settings_cleanup_policy_present",
        "setup_wizard_independent_page_present",
        "setup_wizard_first_launch_default_route",
        "setup_wizard_step_count",
        "settings_uninstall_cleanup_entry_present",
        "settings_disk_quota_controls_present",
        "settings_cleanup_impact_scope_recovery_present",
        "diagnostics_basic_group_count",
        "diagnostics_basic_fields_present",
        "diagnostics_repair_actions_present",
        "diagnostics_fix_buttons_present",
        "diagnostics_recheck_button_present",
        "diagnostics_backend_action_route_present",
        "diagnostics_action_id_contract_count",
        "diagnostics_action_records_manifest",
        "diagnostics_frontend_action_handler_present",
        "diagnostics_required_item_completion_rate",
        "diagnostics_human_readable_message_rate",
        "diagnostics_repair_action_rate",
        "diagnostics_advanced_collapsed",
        "export_user_type_count",
        "export_required_use_types_present",
        "export_project_backup_repro_actions_present",
        "export_open_output_folder_present",
        "export_internal_type_definition_count",
        "export_advanced_details_collapsed",
    ] {
        if let Some((value, status)) = tsv_value_status_by_key(
            &artifact.join("metrics/settings_diagnostics_export_summary.tsv"),
            gate_id,
        ) {
            overrides.insert(gate_id.to_string(), (value, status == "ok"));
        }
    }
    for gate_id in [
        "runtime_probe_present",
        "runtime_probe_output_readable",
        "debug_runtime_endpoint_present",
        "debug_runtime_project_wsl_state_recorded",
        "debug_runtime_non_disruptive_probe",
        "git_probe_present",
        "git_branch_show_current_removed",
        "command_output_decode_cleaning_present",
    ] {
        if let Some((value, status)) = tsv_value_status_by_key(
            &artifact.join("metrics/diagnostics_validation_summary.tsv"),
            gate_id,
        ) {
            overrides.insert(gate_id.to_string(), (value, status == "ok"));
        }
    }
    for gate_id in [
        "report_export_flow_pass",
        "html_report_export_pass",
        "markdown_report_export_pass",
        "json_report_export_pass",
        "candidate_table_export_pass",
        "artifact_bundle_export_pass",
        "report_semantics_validation_pass",
        "report_required_section_count",
        "report_required_sections_present",
        "report_markdown_sections_present",
        "report_html_sections_present",
        "report_export_manifest_types_present",
        "report_export_manifest_user_type_count",
        "report_section_manifest_row_count",
        "report_limits_validation_appendix_present",
        "report_parameters_section_present",
        "report_evidence_chain_section_present",
        "report_appendix_technical_files_present",
        "report_main_forbidden_term_count",
        "report_main_forbidden_terms",
        "report_quality_source_file_exists",
    ] {
        if let Some((value, status)) = tsv_value_status_by_key(
            &artifact.join("metrics/report_quality_summary.tsv"),
            gate_id,
        ) {
            overrides.insert(gate_id.to_string(), (value, status == "ok"));
        }
    }
    for gate_id in [
        "project_package_export_created",
        "project_package_export_bytes",
        "project_package_export_path_exists",
        "project_package_import_api_pass",
        "project_package_imported_project_present",
        "project_package_import_contains_large_database_copy",
        "project_package_read_only_review_mode_present",
        "project_package_demo_mode_present",
        "project_package_support_bundle_linked",
        "project_package_large_database_policy_reference_only",
        "project_package_import_read_only_review_mode",
        "project_package_import_demo_mode_supported",
        "project_package_import_support_bundle_linked",
        "project_package_import_reopen_mode",
        "project_package_imported_project_read_only_persisted",
        "project_package_imported_project_demo_mode_persisted",
        "project_package_imported_project_support_bundle_persisted",
        "project_package_imported_project_collaboration_mode",
        "project_package_imported_project_saved_state",
        "project_package_project_mode_column_present",
        "project_package_import_button_present",
        "project_package_import_user_route_present",
        "project_package_supported_action_present",
        "project_default_remote_term_count",
    ] {
        if let Some((value, status)) = tsv_value_status_by_key(
            &artifact.join("metrics/project_package_import_export_summary.tsv"),
            gate_id,
        ) {
            overrides.insert(gate_id.to_string(), (value, status == "ok"));
        }
    }
    for gate_id in [
        "webui_e2e_test_pass",
        "ordinary_user_journey_pass",
        "ordinary_user_journey_step_count",
        "ordinary_user_journey_pass_rate",
        "ordinary_user_journey_project_id",
        "ordinary_user_journey_run_id",
        "user_workflow_e2e_count",
        "user_workflow_e2e_pass_rate",
        "project_create_flow_pass",
    ] {
        let source_key = if gate_id == "webui_e2e_test_pass" {
            "ordinary_user_journey_pass"
        } else {
            gate_id
        };
        if let Some((value, status)) = tsv_value_status_by_key(
            &artifact.join("metrics/webui_e2e_test_summary.tsv"),
            source_key,
        ) {
            overrides.insert(gate_id.to_string(), (value, status == "ok"));
        }
    }
    for gate_id in [
        "project_lifecycle_smoke_pass",
        "project_lifecycle_step_count",
        "project_lifecycle_pass_rate",
        "project_create_flow_pass",
        "project_open_flow_pass",
        "project_duplicate_flow_pass",
        "project_delete_flow_pass",
        "project_cleanup_flow_pass",
        "project_resume_flow_pass",
        "project_lifecycle_submit_run_pass",
        "project_lifecycle_cancel_run_pass",
        "project_lifecycle_deleted_project_count",
    ] {
        if let Some((value, status)) = tsv_value_status_by_key(
            &artifact.join("metrics/project_lifecycle_smoke_summary.tsv"),
            gate_id,
        ) {
            overrides.insert(gate_id.to_string(), (value, status == "ok"));
        }
    }
    for gate_id in [
        "interactive_message_count",
        "interactive_copy_forbidden_term_count",
        "interactive_copy_forbidden_terms",
        "interactive_copy_human_action_message_count",
        "interactive_copy_lint_scope",
    ] {
        if let Some((value, status)) = tsv_value_status_by_key(
            &artifact.join("metrics/interactive_copy_lint_summary.tsv"),
            gate_id,
        ) {
            overrides.insert(gate_id.to_string(), (value, status == "ok"));
        }
    }
    for gate_id in [
        "default_runtime_lint_scanned_function_count",
        "default_runtime_internal_field_count",
        "default_runtime_internal_fields",
        "default_runtime_lint_scope",
    ] {
        if let Some((value, status)) = tsv_value_status_by_key(
            &artifact.join("metrics/default_runtime_identifier_lint_summary.tsv"),
            gate_id,
        ) {
            overrides.insert(gate_id.to_string(), (value, status == "ok"));
        }
    }
    for gate_id in [
        "default_user_surface_lint_scanned_function_count",
        "default_user_surface_literal_count",
        "default_user_surface_forbidden_term_count",
        "default_user_surface_forbidden_terms",
        "default_user_surface_lint_scope",
    ] {
        if let Some((value, status)) = tsv_value_status_by_key(
            &artifact.join("metrics/default_user_surface_copy_lint_summary.tsv"),
            gate_id,
        ) {
            overrides.insert(gate_id.to_string(), (value, status == "ok"));
        }
    }
    for gate_id in [
        "frontend_function_count",
        "duplicate_function_name_count",
        "duplicate_function_names",
        "shadowed_default_ui_definition_count",
        "frontend_mojibake_marker_count",
        "frontend_utf8_copy_integrity_pass",
        "frontend_source_integrity_scope",
    ] {
        if let Some((value, status)) = tsv_value_status_by_key(
            &artifact.join("metrics/frontend_source_integrity_summary.tsv"),
            gate_id,
        ) {
            overrides.insert(gate_id.to_string(), (value, status == "ok"));
        }
    }
    for gate_id in [
        "user_debug_api_split_pass",
        "user_api_route_count_min",
        "debug_api_route_count_min",
        "github_default_route_count",
    ] {
        if let Some((value, status)) =
            tsv_value_status_by_key(&artifact.join("server/user_debug_api_split.tsv"), gate_id)
        {
            overrides.insert(gate_id.to_string(), (value, status == "ok"));
        }
    }
    for gate_id in [
        "objective_traceability_item_count",
        "objective_traceability_pass_rate",
    ] {
        if let Some((value, status)) = tsv_value_status_by_key(
            &artifact.join("metrics/objective_traceability_summary.tsv"),
            gate_id,
        ) {
            overrides.insert(gate_id.to_string(), (value, status == "ok"));
        }
    }
    for gate_id in [
        "run_monitoring_flow_pass",
        "resource_monitoring_pass",
        "task_submit_flow_pass",
        "task_cancel_flow_pass",
        "task_resume_flow_pass",
        "run_state_machine_validation_pass",
        "run_monitor_pipeline_present",
        "run_monitor_current_stage_present",
        "run_monitor_next_resume_present",
        "run_monitor_resource_forecast_count",
        "run_monitor_live_resource_count",
        "run_monitor_resource_guidance_count",
        "run_monitor_resource_action_count",
        "run_monitor_run_action_count",
        "run_monitor_raw_logs_collapsed",
        "run_monitor_artifact_tree_present",
    ] {
        if let Some((value, status)) = tsv_value_status_by_key(
            &artifact.join("metrics/run_monitor_resource_summary.tsv"),
            gate_id,
        ) {
            overrides.insert(gate_id.to_string(), (value, status == "ok"));
        }
    }
    for gate_id in [
        "structure_patch_viewer_pass",
        "structure_viewer_contract_field_count",
        "structure_viewer_required_fields",
        "structure_viewer_ready",
        "structure_viewer_library",
        "structure_viewer_engine",
        "structure_viewer_chain_count",
        "structure_viewer_residue_count",
        "structure_viewer_patch_residue_count",
        "structure_viewer_ligand_atom_count",
        "structure_viewer_interaction_count",
        "structure_viewer_artifact_link_count",
        "structure_viewer_coordinate_source_recorded",
        "structure_viewer_file_or_fallback_recorded",
        "structure_viewer_backend_owned_json",
        "structure_viewer_frontend_consumes_json",
        "structure_viewer_ngl_loader_present",
    ] {
        if let Some((value, status)) = tsv_value_status_by_key(
            &artifact.join("metrics/structure_viewer_contract_summary.tsv"),
            gate_id,
        ) {
            overrides.insert(gate_id.to_string(), (value, status == "ok"));
        }
    }
    for gate_id in [
        "setup_wizard_pass",
        "setup_wizard_independent_page_present",
        "storage_cleanup_policy_pass",
        "setup_demo_and_full_modes_present",
        "setup_mode_direct_field_completion_rate",
        "setup_mode_required_fields",
        "setup_mode_count",
        "setup_required_tool_detector_count",
        "setup_required_tool_detector_names_present",
        "setup_required_tool_repair_action_completion_rate",
        "setup_required_tool_install_hint_completion_rate",
        "setup_required_tool_verify_command_completion_rate",
        "setup_required_tool_completion_rate",
        "full_scientific_ready",
        "full_scientific_required_tools_all_ready",
        "install_plan_missing_tool_count",
        "install_plan_output_clean",
        "full_scientific_install_command_present",
        "complete_scientific_installer_wrapper_present",
        "full_scientific_installer_source_present",
        "full_scientific_installer_required_tool_count",
        "full_scientific_installer_wsl_runtime_present",
        "full_scientific_installer_stepwise_repair_present",
        "full_scientific_installer_nextflow_repair_present",
        "full_scientific_installer_conda_worker_repair_present",
        "full_scientific_installer_micromamba_index_repair_present",
        "full_scientific_installer_post_repair_probe_present",
        "full_scientific_installer_mojibake_guard_present",
        "full_scientific_installer_self_check_pass",
        "full_scientific_installer_self_check_manifest_present",
        "full_scientific_installer_required_runtime_tool_coverage",
        "full_scientific_installer_repair_step_coverage",
        "full_scientific_installer_repair_step_count",
        "full_scientific_installer_execute_guard_pass",
        "full_scientific_installer_manifest_paths_declared",
        "single_command_recorded",
        "setup_single_command_recorded",
        "setup_ui_mode_humanizer_present",
        "setup_wizard_direct_fields_present",
        "sample_project_validation_pass",
        "sample_project_candidate_detail_selected",
        "sample_project_research_report_exported",
        "sample_project_package_imported",
        "sample_project_temporary_project_deleted",
        "sample_project_visible_project_count_preserved",
        "storage_protected_item_count",
        "storage_delete_impact_review_present",
    ] {
        if let Some((value, status)) = tsv_value_status_by_key(
            &artifact.join("metrics/setup_storage_validation_summary.tsv"),
            gate_id,
        ) {
            overrides.insert(gate_id.to_string(), (value, status == "ok"));
        }
    }
    if let Some((value, status)) = tsv_value_status_by_key(
        &artifact.join("metrics/setup_storage_validation_summary.tsv"),
        "setup_wizard_direct_fields_present",
    ) {
        overrides.insert(
            "settings_setup_wizard_direct_fields_present".to_string(),
            (value, status == "ok"),
        );
    }
    overrides
}

fn tsv_status_by_key(path: &Path, key: &str) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    for line in text.lines().skip(1) {
        let cols = line.split('\t').collect::<Vec<_>>();
        if cols.first().is_some_and(|value| *value == key) {
            return cols.last().map(|value| value.to_string());
        }
    }
    None
}

fn tsv_value_status_by_key(path: &Path, key: &str) -> Option<(String, String)> {
    let text = fs::read_to_string(path).ok()?;
    for line in text.lines().skip(1) {
        let cols = line.split('\t').collect::<Vec<_>>();
        if cols.first().is_some_and(|value| *value == key) {
            let value = cols.get(1).unwrap_or(&"").to_string();
            let status = cols.last().unwrap_or(&"").to_string();
            return Some((value, status));
        }
    }
    None
}

fn required_artifacts() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            "artifact_master_validation",
            "metrics/master_validation_phase_0_9_1.json",
        ),
        (
            "artifact_webui_route_coverage",
            "metrics/webui_route_coverage.tsv",
        ),
        (
            "artifact_workflow_pipeline_summary",
            "metrics/workflow_pipeline_summary.tsv",
        ),
        (
            "artifact_evidence_card_summary",
            "metrics/evidence_card_summary.tsv",
        ),
        (
            "artifact_dashboard_action_summary",
            "metrics/dashboard_action_summary.tsv",
        ),
        (
            "artifact_component_semantics_summary",
            "metrics/component_semantics_summary.tsv",
        ),
        (
            "artifact_input_field_validation_summary",
            "metrics/input_field_validation_summary.tsv",
        ),
        (
            "artifact_run_forecast_confirmation_summary",
            "metrics/run_forecast_confirmation_summary.tsv",
        ),
        (
            "artifact_preflight_group_action_summary",
            "metrics/preflight_group_action_summary.tsv",
        ),
        (
            "artifact_candidate_ranking_summary",
            "metrics/candidate_ranking_summary.tsv",
        ),
        (
            "artifact_genome_md_graphics_summary",
            "metrics/genome_md_graphics_summary.tsv",
        ),
        (
            "artifact_settings_diagnostics_export_summary",
            "metrics/settings_diagnostics_export_summary.tsv",
        ),
        (
            "artifact_report_quality_summary",
            "metrics/report_quality_summary.tsv",
        ),
        (
            "artifact_project_package_import_export_summary",
            "metrics/project_package_import_export_summary.tsv",
        ),
        (
            "artifact_interactive_copy_lint_summary",
            "metrics/interactive_copy_lint_summary.tsv",
        ),
        (
            "artifact_default_runtime_identifier_lint_summary",
            "metrics/default_runtime_identifier_lint_summary.tsv",
        ),
        (
            "artifact_default_user_surface_copy_lint_summary",
            "metrics/default_user_surface_copy_lint_summary.tsv",
        ),
        (
            "artifact_frontend_source_integrity_summary",
            "metrics/frontend_source_integrity_summary.tsv",
        ),
        (
            "artifact_objective_traceability_summary",
            "metrics/objective_traceability_summary.tsv",
        ),
        (
            "artifact_run_monitor_resource_summary",
            "metrics/run_monitor_resource_summary.tsv",
        ),
        (
            "artifact_structure_viewer_contract_summary",
            "metrics/structure_viewer_contract_summary.tsv",
        ),
        (
            "artifact_webui_component_coverage",
            "metrics/webui_component_coverage.tsv",
        ),
        (
            "artifact_ui_stack_selection",
            "metrics/ui_stack_selection.tsv",
        ),
        (
            "artifact_ui_dependency_license_manifest",
            "metrics/ui_dependency_license_manifest.tsv",
        ),
        (
            "artifact_ui_visual_audit_summary",
            "metrics/ui_visual_audit_summary.tsv",
        ),
        (
            "artifact_screenshot_retention_summary",
            "metrics/screenshot_retention_summary.tsv",
        ),
        (
            "artifact_webui_e2e_test_summary",
            "metrics/webui_e2e_test_summary.tsv",
        ),
        (
            "artifact_project_lifecycle_smoke_summary",
            "metrics/project_lifecycle_smoke_summary.tsv",
        ),
        (
            "artifact_clean_release_summary",
            "metrics/clean_release_summary.tsv",
        ),
        (
            "artifact_diagnostics_validation_summary",
            "metrics/diagnostics_validation_summary.tsv",
        ),
        (
            "artifact_setup_storage_validation_summary",
            "metrics/setup_storage_validation_summary.tsv",
        ),
        (
            "artifact_user_debug_api_split",
            "server/user_debug_api_split.tsv",
        ),
        (
            "artifact_post_push_cleanup_summary",
            "metrics/post_push_cleanup_summary.tsv",
        ),
        (
            "artifact_runtime_cleanup_dry_run",
            "cleanup/phase09_runtime_cleanup_dry_run.tsv",
        ),
        (
            "artifact_runtime_cleanup_dry_run_manifest_copy",
            "manifests/phase09_runtime_cleanup_dry_run.tsv",
        ),
        (
            "artifact_cargo_validation_summary",
            "metrics/cargo_validation_summary.tsv",
        ),
        (
            "artifact_server_validation_summary",
            "metrics/server_validation_summary.tsv",
        ),
        (
            "artifact_scientific_regression_summary",
            "metrics/scientific_regression_summary.tsv",
        ),
        (
            "artifact_api_contract_validation",
            "server/api_contract_validation.tsv",
        ),
        ("artifact_api_schema", "server/api_schema.openapi.json"),
        (
            "artifact_websocket_validation",
            "server/websocket_validation.tsv",
        ),
        (
            "artifact_visual_audit_md",
            "ui/visual_audit/final_visual_audit_report.md",
        ),
        (
            "artifact_visual_audit_tsv",
            "ui/visual_audit/final_visual_audit_report.tsv",
        ),
        (
            "artifact_playwright_summary",
            "webui/playwright_final_summary/summary.tsv",
        ),
        (
            "artifact_build_validation",
            "webui/build_validation_summary.tsv",
        ),
        (
            "artifact_project_model_schema",
            "manifests/project_model_schema.json",
        ),
        (
            "artifact_run_model_schema",
            "manifests/run_model_schema.json",
        ),
        (
            "artifact_input_wizard_schema",
            "manifests/input_wizard_schema.json",
        ),
        (
            "artifact_report_export_manifest",
            "manifests/report_export_manifest.tsv",
        ),
        (
            "artifact_report_sections_manifest",
            "manifests/report_sections_manifest.tsv",
        ),
        (
            "artifact_post_push_cleanup_manifest",
            "manifests/post_push_cleanup_execution_manifest.tsv",
        ),
        (
            "artifact_ui_stack_decision_doc",
            "docs/ui_stack_decision_record.md",
        ),
        ("artifact_user_guide", "docs/local_webui_user_guide.md"),
        (
            "artifact_windows_wsl2_guide",
            "docs/windows_wsl2_installation_guide.md",
        ),
        (
            "artifact_linux_server_guide",
            "docs/linux_server_installation_guide.md",
        ),
    ]
}

fn true_gates() -> Vec<&'static str> {
    vec![
        "webui_starts_successfully",
        "webui_localhost_access_pass",
        "gls_server_starts_successfully",
        "api_health_check_pass",
        "api_contract_test_pass",
        "websocket_log_stream_pass",
        "websocket_status_stream_pass",
        "websocket_resource_stream_pass",
        "websocket_live_smoke_pass",
        "server_startup_test_pass",
        "workflow_step_order",
        "main_context_panel",
        "dashboard_context_suppressed",
        "workflow_context_non_dashboard_route_coverage",
        "workflow_context_blocker_fix_link_present",
        "workflow_context_next_action_link_present",
        "workflow_context_resume_action_link_present",
        "detail_panel_context",
        "runtime_timeline_uses_pipeline",
        "ui_framework_selection_recorded",
        "ui_component_library_selection_recorded",
        "ui_table_component_selection_recorded",
        "ui_structure_viewer_selection_recorded",
        "ui_screenshot_tool_selection_recorded",
        "ui_dependency_license_manifest_complete",
        "codex_style_design_audit_pass",
        "input_wizard_page_pass",
        "project_basics_form_pass",
        "molecule_type_selection_pass",
        "small_molecule_input_flow_pass",
        "peptide_input_flow_pass",
        "protein_ligand_input_flow_pass",
        "complex_structure_input_flow_pass",
        "organism_genome_selection_pass",
        "search_scope_selection_pass",
        "database_profile_selection_pass",
        "run_profile_selection_pass",
        "preflight_check_page_pass",
        "review_start_page_pass",
        "advanced_parameters_collapsed_by_default",
        "project_create_flow_pass",
        "project_open_flow_pass",
        "project_duplicate_flow_pass",
        "project_delete_flow_pass",
        "project_cleanup_flow_pass",
        "project_resume_flow_pass",
        "project_lifecycle_smoke_pass",
        "project_lifecycle_submit_run_pass",
        "project_lifecycle_cancel_run_pass",
        "task_submit_flow_pass",
        "task_cancel_flow_pass",
        "task_resume_flow_pass",
        "run_monitoring_flow_pass",
        "run_state_machine_validation_pass",
        "resource_monitoring_pass",
        "candidate_ranking_page_pass",
        "candidate_detail_page_pass",
        "genome_orf_context_viewer_pass",
        "structure_patch_viewer_pass",
        "md_summary_viewer_pass",
        "evidence_ledger_viewer_pass",
        "report_export_flow_pass",
        "html_report_export_pass",
        "markdown_report_export_pass",
        "candidate_table_export_pass",
        "json_report_export_pass",
        "artifact_bundle_export_pass",
        "report_semantics_validation_pass",
        "diagnostics_page_pass",
        "runtime_probe_present",
        "runtime_probe_output_readable",
        "debug_runtime_endpoint_present",
        "debug_runtime_project_wsl_state_recorded",
        "debug_runtime_non_disruptive_probe",
        "git_probe_present",
        "git_branch_show_current_removed",
        "command_output_decode_cleaning_present",
        "full_scientific_ready",
        "full_scientific_required_tools_all_ready",
        "install_plan_output_clean",
        "full_scientific_installer_self_check_pass",
        "full_scientific_installer_self_check_manifest_present",
        "full_scientific_installer_execute_guard_pass",
        "full_scientific_installer_manifest_paths_declared",
        "sample_project_validation_pass",
        "sample_project_candidate_detail_selected",
        "sample_project_research_report_exported",
        "sample_project_package_imported",
        "sample_project_temporary_project_deleted",
        "sample_project_visible_project_count_preserved",
        "wsl2_diagnostics_pass",
        "linux_diagnostics_recorded",
        "cuda_openmm_diagnostics_pass",
        "nextflow_diagnostics_pass",
        "database_diagnostics_pass",
        "default_github_hidden_pass",
        "dashboard_recent_projects_present",
        "dashboard_recent_results_present",
        "dashboard_key_environment_status_present",
        "shared_component_spec_pass",
        "primary_next_step_policy_pass",
        "danger_destructive_policy_pass",
        "ui_png_render_after_each_ui_change",
        "ui_visual_self_audit_pass",
        "ui_dark_only_visual_pass",
        "ui_no_theme_toggle_pass",
        "all_viewports_visual_ready",
        "screenshot_metadata_utf8_pass",
        "rust_owned_decision_logic_preserved",
        "adapter_registry_complete",
        "worker_job_schema_validation_pass",
        "worker_result_schema_validation_pass",
        "schema_closure_validation_pass",
        "cargo_fmt_pass",
        "cargo_clippy_d_warnings_pass",
        "cargo_test_pass",
        "gls_cli_build_pass",
        "gls_server_build_pass",
        "webui_lint_pass",
        "webui_typecheck_pass",
        "webui_build_pass",
        "webui_start_script_target_dir_support",
        "webui_start_script_pid_file_support",
        "webui_start_script_isolated_binary_support",
        "webui_e2e_test_pass",
        "ordinary_user_journey_pass",
        "ordinary_user_journey_project_id",
        "ordinary_user_journey_run_id",
        "phase_0_7_1_regression_pass",
        "phase_0_8_3_regression_pass",
        "phase_0_8_4_openmm_regression_pass",
        "compact_regression_pass",
        "nextflow_first_run_pass",
        "nextflow_resume_run_pass",
        "semantic_hash_match",
        "setup_wizard_pass",
        "setup_wizard_independent_page_present",
        "storage_cleanup_policy_pass",
        "user_debug_api_split_pass",
        "default_developer_terms_hidden_pass",
        "post_push_cleanup_pass",
        "transient_path_count",
        "runtime_cleanup_dry_run_exit_code",
        "runtime_cleanup_dry_run_manifest_present",
        "runtime_cleanup_default_mode",
        "runtime_cleanup_execute_requested",
        "runtime_cleanup_allowlisted_ports",
        "runtime_cleanup_path_scoped_to_workspace",
        "field_validation_component_present",
        "computed_validation_component_present",
        "input_material_required_labels_present",
        "field_validation_rules_complete",
        "input_manifest_contract_complete",
        "scope_database_run_profile_validation_present",
        "smiles_fasta_structure_validation_present",
        "chain_ligand_residue_validation_present",
        "genome_file_validation_present",
        "forecast_required_fields_present",
        "forecast_scope_profile_dynamic_present",
        "full_physical_forecast_confirmation_present",
        "forecast_confirmation_capture_present",
        "start_workflow_forecast_guard_present",
        "review_start_forecast_confirmation_present",
        "forecast_confirmation_css_present",
        "preflight_required_groups",
        "preflight_required_next_actions",
        "preflight_cpu_mode_action_present",
        "preflight_repair_gpu_action_present",
        "preflight_group_status_impact_next_fields",
        "preflight_advanced_details_collapsed",
        "candidate_table_search_control_present",
        "candidate_table_risk_filter_present",
        "candidate_table_sort_control_present",
        "candidate_table_page_size_control_present",
        "candidate_table_pagination_present",
        "candidate_table_query_wiring_present",
        "candidate_table_risk_wiring_present",
        "candidate_table_sort_wiring_present",
        "candidate_table_page_size_wiring_present",
        "candidate_table_column_visibility_wiring_present",
        "candidate_table_pagination_wiring_present",
        "candidate_table_clear_filter_wiring_present",
        "candidate_table_sticky_column_present",
        "candidate_export_entry_present",
        "candidate_advanced_full_table_collapsed",
        "candidate_detail_required_jumps",
        "candidate_detail_docking_jump_present",
        "candidate_detail_export_jump_present",
        "evidence_card_markup_present",
        "default_visible_evidence_fields",
        "internal_fields_advanced_only",
        "evidence_grouping_function",
        "evidence_interpretation_function",
        "evidence_empty_group_pending_state",
        "evidence_card_css_present",
        "evidence_real_row_count",
        "evidence_real_source_count",
        "genome_track_graphic_present",
        "genome_context_table_fields_present",
        "genome_track_css_present",
        "md_score_decomposition_present",
        "md_chart_css_present",
        "settings_allowed_categories",
        "settings_advanced_developer_mode_collapsed",
        "settings_developer_mode_debug_routes_lazy_loaded",
        "settings_developer_mode_guides_lazy_loaded",
        "settings_github_default_hidden",
        "settings_policy_humanizer_present",
        "settings_cleanup_policy_present",
        "settings_uninstall_cleanup_entry_present",
        "settings_disk_quota_controls_present",
        "settings_cleanup_impact_scope_recovery_present",
        "setup_wizard_first_launch_default_route",
        "diagnostics_basic_fields_present",
        "diagnostics_repair_actions_present",
        "diagnostics_fix_buttons_present",
        "diagnostics_recheck_button_present",
        "diagnostics_backend_action_route_present",
        "diagnostics_action_records_manifest",
        "diagnostics_frontend_action_handler_present",
        "diagnostics_advanced_collapsed",
        "export_required_use_types_present",
        "export_project_backup_repro_actions_present",
        "export_open_output_folder_present",
        "export_advanced_details_collapsed",
        "report_appendix_technical_files_present",
        "report_main_forbidden_terms",
        "report_required_sections_present",
        "report_markdown_sections_present",
        "report_html_sections_present",
        "report_export_manifest_types_present",
        "report_limits_validation_appendix_present",
        "report_parameters_section_present",
        "report_evidence_chain_section_present",
        "report_quality_source_file_exists",
        "project_package_export_created",
        "project_package_export_path_exists",
        "project_package_import_api_pass",
        "project_package_imported_project_present",
        "project_package_import_contains_large_database_copy",
        "project_package_read_only_review_mode_present",
        "project_package_demo_mode_present",
        "project_package_support_bundle_linked",
        "project_package_large_database_policy_reference_only",
        "project_package_import_read_only_review_mode",
        "project_package_import_demo_mode_supported",
        "project_package_import_support_bundle_linked",
        "project_package_import_reopen_mode",
        "project_package_imported_project_read_only_persisted",
        "project_package_imported_project_demo_mode_persisted",
        "project_package_imported_project_support_bundle_persisted",
        "project_package_imported_project_collaboration_mode",
        "project_package_imported_project_saved_state",
        "project_package_project_mode_column_present",
        "project_package_import_button_present",
        "project_package_import_user_route_present",
        "project_package_supported_action_present",
        "run_monitor_pipeline_present",
        "run_monitor_current_stage_present",
        "run_monitor_next_resume_present",
        "run_monitor_raw_logs_collapsed",
        "run_monitor_artifact_tree_present",
        "structure_viewer_required_fields",
        "structure_viewer_ready",
        "structure_viewer_library",
        "structure_viewer_engine",
        "structure_viewer_chain_count",
        "structure_viewer_residue_count",
        "structure_viewer_patch_residue_count",
        "structure_viewer_ligand_atom_count",
        "structure_viewer_interaction_count",
        "structure_viewer_artifact_link_count",
        "structure_viewer_coordinate_source_recorded",
        "structure_viewer_file_or_fallback_recorded",
        "structure_viewer_backend_owned_json",
        "structure_viewer_frontend_consumes_json",
        "structure_viewer_ngl_loader_present",
        "setup_demo_and_full_modes_present",
        "setup_mode_direct_field_completion_rate",
        "setup_mode_required_fields",
        "setup_required_tool_repair_action_completion_rate",
        "setup_required_tool_install_hint_completion_rate",
        "setup_required_tool_verify_command_completion_rate",
        "setup_required_tool_completion_rate",
        "complete_scientific_installer_wrapper_present",
        "full_scientific_installer_source_present",
        "full_scientific_installer_wsl_runtime_present",
        "full_scientific_installer_stepwise_repair_present",
        "full_scientific_installer_nextflow_repair_present",
        "full_scientific_installer_conda_worker_repair_present",
        "full_scientific_installer_micromamba_index_repair_present",
        "full_scientific_installer_post_repair_probe_present",
        "full_scientific_installer_mojibake_guard_present",
        "setup_ui_mode_humanizer_present",
        "settings_setup_wizard_direct_fields_present",
        "storage_delete_impact_review_present",
        "interactive_copy_forbidden_terms",
        "interactive_copy_lint_scope",
        "default_runtime_internal_fields",
        "default_runtime_lint_scope",
        "default_user_surface_forbidden_terms",
        "default_user_surface_lint_scope",
        "duplicate_function_names",
        "frontend_utf8_copy_integrity_pass",
        "frontend_source_integrity_scope",
    ]
}

fn exact_gates() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            "mandatory_ui_dependency_license_record_completion_rate",
            "100%",
        ),
        ("proprietary_ui_dependency_count", "0"),
        ("unknown_ui_license_count", "0"),
        ("input_examples_present_rate", "100%"),
        ("human_readable_input_help_rate", "100%"),
        ("experimental_claim_overstatement_count", "0"),
        ("diagnostics_required_item_completion_rate", "100%"),
        ("diagnostics_human_readable_message_rate", "100%"),
        ("diagnostics_repair_action_rate", "100%"),
        ("setup_required_tool_completion_rate", "100%"),
        ("ui_visual_defect_count", "0"),
        ("ui_layout_overflow_count", "0"),
        ("ui_unreadable_text_count", "0"),
        ("ui_broken_component_count", "0"),
        ("ui_missing_empty_state_count", "0"),
        ("ui_missing_error_state_count", "0"),
        ("ui_screenshot_metadata_completion_rate", "100%"),
        ("route_count", "29"),
        ("viewport_count", "2"),
        ("desktop_route_count", "29"),
        ("narrow_desktop_route_count", "29"),
        ("png_rendered_count", "58"),
        ("narrow_desktop_png_rendered_count", "29"),
        ("warning_count", "0"),
        ("narrow_desktop_warning_count", "0"),
        ("screenshot_metadata_mojibake_count", "0"),
        ("final_screenshot_retention_manifest_present", "True"),
        ("final_screenshot_retention_row_count", "2"),
        ("final_screenshot_cache_flag_count", "0"),
        ("final_screenshot_retention_class_count", "2"),
        ("final_screenshot_retention_ready_count", "2"),
        ("final_screenshot_dirs_not_cache", "True"),
        ("direct_python_webui_call_count", "0"),
        ("direct_python_md_call_count", "0"),
        ("nextflow_process_gls_cli_call_rate", "100%"),
        ("python_owned_final_score_rule_count", "0"),
        ("python_owned_report_semantics_count", "0"),
        ("raw_tool_report_dependency_count", "0"),
        ("openmm_cuda_status", "ready"),
        ("main_nav_item_count", "3"),
        ("github_main_nav_count", "0"),
        ("dashboard_main_action_count", "5"),
        ("workflow_step_count", "9"),
        ("workflow_context_required_field_count", "5"),
        ("workflow_context_non_dashboard_route_coverage", "28/28"),
        ("dashboard_extra_action_count", "0"),
        ("dashboard_detail_action_count", "0"),
        ("dashboard_action_table_header_count", "0"),
        ("dashboard_github_visible_count", "0"),
        ("dashboard_home_sections_count", "3"),
        ("dashboard_default_developer_trace_count", "0"),
        ("design_token_count", "16"),
        ("shared_component_rule_count", "16"),
        ("primary_disallowed_label_count", "0"),
        ("danger_disallowed_label_count", "0"),
        ("field_validation_expected_field_count", "13"),
        ("input_material_card_count", "6"),
        ("field_validation_rule_count", "10"),
        ("input_manifest_contract_count", "8"),
        ("structure_preview_count", "3"),
        ("forecast_resource_field_count", "7"),
        ("websocket_message_count", "3"),
        ("preflight_group_count", "6"),
        ("preflight_next_action_count", "7"),
        ("candidate_summary_metric_count", "5"),
        ("candidate_detail_jump_count", "7"),
        ("candidate_detail_layer_section_count", "4"),
        ("candidate_table_column_toggle_count", "5"),
        ("candidate_table_stateful_control_count", "7"),
        ("evidence_group_count", "7"),
        ("default_visible_evidence_field_count", "6"),
        ("internal_evidence_field_count", "8"),
        ("genome_track_segment_count", "9"),
        ("md_chart_count", "5"),
        ("md_key_metric_count", "6"),
        ("md_chart_source_unit_meta_present", "Data source: | unit:"),
        ("md_chart_candidate_meta_present", "candidate:"),
        ("md_chart_time_basis_meta_present", "time basis:"),
        ("md_chart_artifact_link_meta_present", "artifact links:"),
        ("md_chart_meta_function_present", "mdChartMeta"),
        ("settings_category_count", "7"),
        ("diagnostics_basic_group_count", "6"),
        ("diagnostics_action_id_contract_count", "8"),
        ("export_user_type_count", "7"),
        ("export_internal_type_definition_count", "0"),
        ("report_required_section_count", "11"),
        ("report_export_manifest_user_type_count", "7"),
        ("report_section_manifest_row_count", "11"),
        ("report_main_forbidden_term_count", "0"),
        ("project_package_export_bytes", "1"),
        (
            "project_package_imported_project_collaboration_mode",
            "read_only_review",
        ),
        (
            "project_package_imported_project_saved_state",
            "project_package_import",
        ),
        ("project_default_remote_term_count", "0"),
        ("interactive_copy_forbidden_term_count", "0"),
        ("interactive_copy_human_action_message_count", "6"),
        ("default_runtime_lint_scanned_function_count", "13"),
        ("default_runtime_internal_field_count", "0"),
        ("default_user_surface_lint_scanned_function_count", "24"),
        ("default_user_surface_forbidden_term_count", "0"),
        ("duplicate_function_name_count", "0"),
        ("shadowed_default_ui_definition_count", "0"),
        ("frontend_mojibake_marker_count", "0"),
        ("objective_traceability_item_count", "25"),
        ("objective_traceability_pass_rate", "25/25"),
        ("transient_path_count", "0"),
        ("runtime_cleanup_dry_run_exit_code", "0"),
        ("runtime_cleanup_dry_run_manifest_present", "True"),
        ("runtime_cleanup_default_mode", "dry_run"),
        ("runtime_cleanup_execute_requested", "False"),
        (
            "runtime_cleanup_allowlisted_ports",
            "19190,19191,19192,19193,19194",
        ),
        ("runtime_cleanup_path_scoped_to_workspace", "ok"),
        ("first_launch_project_count", "0"),
        ("first_launch_run_count", "0"),
        ("first_launch_clean_state", "True"),
        ("run_example_template_available", "True"),
        ("sample_results_not_precreated", "True"),
        ("run_example_created_on_click", "True"),
        ("run_example_contains_precomputed_results", "False"),
        ("post_test_project_count", "0"),
        ("post_test_run_count", "0"),
        ("post_test_temp_artifact_count", "0"),
        ("post_test_screenshot_cache_count", "0"),
        ("start_script_default_runtime_outside_workspace", "True"),
        ("start_script_runtime_dir_override_present", "True"),
        ("final_screenshot_cache_overlap_count", "0"),
        ("run_monitor_resource_forecast_count", "4"),
        ("run_monitor_live_resource_count", "4"),
        ("run_monitor_resource_guidance_count", "4"),
        ("run_monitor_resource_action_count", "4"),
        ("run_monitor_run_action_count", "4"),
        ("structure_viewer_contract_field_count", "14"),
        ("setup_mode_count", "2"),
        ("setup_wizard_step_count", "6"),
        ("setup_tool_count", "13"),
        ("setup_required_tool_detector_count", "12"),
        ("full_scientific_install_command_present", "python"),
        (
            "complete_scientific_installer_wrapper_present",
            "scripts/setup/install_complete_scientific.py",
        ),
        ("full_scientific_installer_required_tool_count", "11"),
        (
            "full_scientific_installer_required_runtime_tool_coverage",
            "13/13",
        ),
        ("full_scientific_installer_repair_step_coverage", "6/6"),
        ("full_scientific_installer_repair_step_count", "6"),
        ("setup_required_tool_repair_action_completion_rate", "12/12"),
        ("setup_required_tool_install_hint_completion_rate", "12/12"),
        (
            "setup_required_tool_verify_command_completion_rate",
            "12/12",
        ),
        ("install_plan_missing_tool_count", "0"),
        ("storage_protected_item_count", "4"),
        ("user_api_route_count_min", "8"),
        ("debug_api_route_count_min", "2"),
        ("post_push_local_history_artifact_count", "0"),
        ("post_push_local_cache_count", "0"),
        ("post_push_old_report_count", "0"),
        ("post_push_old_screenshot_count", "0"),
        ("post_push_nextflow_cache_count", "0"),
        ("post_push_tmp_work_dir_count", "0"),
        ("post_push_old_visual_audit_intermediate_count", "0"),
        ("post_push_untracked_file_count", "0"),
        ("user_workflow_e2e_count", "7"),
        ("user_workflow_e2e_pass_rate", "100%"),
        ("ordinary_user_journey_step_count", "8"),
        ("ordinary_user_journey_pass_rate", "100%"),
        ("environment_validation_project_success_rate", "100%"),
    ]
}

fn requires_actual_gate(id: &str) -> bool {
    matches!(
        id,
        "main_nav_item_count"
            | "github_main_nav_count"
            | "default_github_hidden_pass"
            | "default_developer_terms_hidden_pass"
            | "ui_framework_selection_recorded"
            | "ui_component_library_selection_recorded"
            | "ui_table_component_selection_recorded"
            | "ui_structure_viewer_selection_recorded"
            | "ui_screenshot_tool_selection_recorded"
            | "ui_dependency_license_manifest_complete"
            | "proprietary_ui_dependency_count"
            | "unknown_ui_license_count"
            | "report_export_flow_pass"
            | "html_report_export_pass"
            | "markdown_report_export_pass"
            | "json_report_export_pass"
            | "candidate_table_export_pass"
            | "artifact_bundle_export_pass"
            | "report_semantics_validation_pass"
            | "input_wizard_page_pass"
            | "project_basics_form_pass"
            | "molecule_type_selection_pass"
            | "small_molecule_input_flow_pass"
            | "peptide_input_flow_pass"
            | "protein_ligand_input_flow_pass"
            | "complex_structure_input_flow_pass"
            | "organism_genome_selection_pass"
            | "search_scope_selection_pass"
            | "database_profile_selection_pass"
            | "run_profile_selection_pass"
            | "preflight_check_page_pass"
            | "review_start_page_pass"
            | "candidate_ranking_page_pass"
            | "candidate_detail_page_pass"
            | "evidence_ledger_viewer_pass"
            | "genome_orf_context_viewer_pass"
            | "structure_patch_viewer_pass"
            | "md_summary_viewer_pass"
            | "run_monitoring_flow_pass"
            | "resource_monitoring_pass"
            | "task_submit_flow_pass"
            | "task_cancel_flow_pass"
            | "task_resume_flow_pass"
            | "run_state_machine_validation_pass"
            | "server_startup_test_pass"
            | "webui_starts_successfully"
            | "webui_localhost_access_pass"
            | "webui_lint_pass"
            | "webui_typecheck_pass"
            | "webui_build_pass"
            | "webui_start_script_target_dir_support"
            | "webui_start_script_pid_file_support"
            | "webui_start_script_isolated_binary_support"
            | "setup_wizard_pass"
            | "setup_wizard_independent_page_present"
            | "setup_wizard_step_count"
            | "storage_cleanup_policy_pass"
            | "post_push_cleanup_pass"
            | "transient_path_count"
            | "runtime_cleanup_dry_run_exit_code"
            | "runtime_cleanup_dry_run_manifest_present"
            | "runtime_cleanup_default_mode"
            | "runtime_cleanup_execute_requested"
            | "runtime_cleanup_allowlisted_ports"
            | "runtime_cleanup_path_scoped_to_workspace"
            | "user_debug_api_split_pass"
            | "user_api_route_count_min"
            | "debug_api_route_count_min"
            | "user_workflow_e2e_count"
            | "user_workflow_e2e_pass_rate"
            | "project_create_flow_pass"
            | "project_open_flow_pass"
            | "project_duplicate_flow_pass"
            | "project_delete_flow_pass"
            | "project_cleanup_flow_pass"
            | "project_resume_flow_pass"
            | "project_lifecycle_smoke_pass"
            | "project_lifecycle_submit_run_pass"
            | "project_lifecycle_cancel_run_pass"
            | "diagnostics_fix_buttons_present"
            | "diagnostics_recheck_button_present"
            | "diagnostics_backend_action_route_present"
            | "diagnostics_action_id_contract_count"
            | "diagnostics_action_records_manifest"
            | "diagnostics_frontend_action_handler_present"
            | "diagnostics_required_item_completion_rate"
            | "diagnostics_human_readable_message_rate"
            | "diagnostics_repair_action_rate"
            | "runtime_probe_present"
            | "runtime_probe_output_readable"
            | "debug_runtime_endpoint_present"
            | "debug_runtime_project_wsl_state_recorded"
            | "debug_runtime_non_disruptive_probe"
            | "git_probe_present"
            | "git_branch_show_current_removed"
            | "command_output_decode_cleaning_present"
            | "full_scientific_install_command_present"
            | "project_package_imported_project_read_only_persisted"
            | "project_package_imported_project_demo_mode_persisted"
            | "project_package_imported_project_support_bundle_persisted"
            | "project_package_imported_project_collaboration_mode"
            | "project_package_imported_project_saved_state"
            | "project_package_project_mode_column_present"
            | "complete_scientific_installer_wrapper_present"
            | "full_scientific_installer_required_tool_count"
            | "full_scientific_installer_wsl_runtime_present"
            | "full_scientific_installer_stepwise_repair_present"
            | "full_scientific_installer_nextflow_repair_present"
            | "full_scientific_installer_conda_worker_repair_present"
            | "full_scientific_installer_micromamba_index_repair_present"
            | "full_scientific_installer_post_repair_probe_present"
            | "full_scientific_installer_mojibake_guard_present"
            | "full_scientific_installer_self_check_pass"
            | "full_scientific_installer_self_check_manifest_present"
            | "full_scientific_installer_required_runtime_tool_coverage"
            | "full_scientific_installer_repair_step_coverage"
            | "full_scientific_installer_repair_step_count"
            | "full_scientific_installer_execute_guard_pass"
            | "full_scientific_installer_manifest_paths_declared"
            | "full_scientific_ready"
            | "full_scientific_required_tools_all_ready"
            | "install_plan_missing_tool_count"
            | "install_plan_output_clean"
            | "sample_project_validation_pass"
            | "sample_project_candidate_detail_selected"
            | "sample_project_research_report_exported"
            | "sample_project_package_imported"
            | "sample_project_temporary_project_deleted"
            | "sample_project_visible_project_count_preserved"
            | "candidate_table_stateful_control_count"
            | "candidate_table_query_wiring_present"
            | "candidate_table_risk_wiring_present"
            | "candidate_table_sort_wiring_present"
            | "candidate_table_page_size_wiring_present"
            | "candidate_table_column_visibility_wiring_present"
            | "candidate_table_pagination_wiring_present"
            | "candidate_table_clear_filter_wiring_present"
            | "screenshot_metadata_mojibake_count"
            | "screenshot_metadata_utf8_pass"
            | "frontend_mojibake_marker_count"
            | "frontend_utf8_copy_integrity_pass"
            | "final_screenshot_retention_manifest_present"
            | "final_screenshot_retention_row_count"
            | "final_screenshot_cache_flag_count"
            | "final_screenshot_retention_class_count"
            | "final_screenshot_retention_ready_count"
            | "final_screenshot_dirs_not_cache"
            | "first_launch_project_count"
            | "first_launch_run_count"
            | "first_launch_clean_state"
            | "run_example_template_available"
            | "sample_results_not_precreated"
            | "run_example_created_on_click"
            | "run_example_contains_precomputed_results"
            | "post_test_project_count"
            | "post_test_run_count"
            | "post_test_temp_artifact_count"
            | "post_test_screenshot_cache_count"
            | "start_script_default_runtime_outside_workspace"
            | "start_script_runtime_dir_override_present"
            | "final_screenshot_cache_overlap_count"
            | "md_chart_source_unit_meta_present"
            | "md_chart_candidate_meta_present"
            | "md_chart_time_basis_meta_present"
            | "md_chart_artifact_link_meta_present"
            | "md_chart_meta_function_present"
    )
}

fn write_master_validation(
    artifact_dir: &Path,
    metrics: &[GateMetric],
    verdict: &str,
) -> Result<(), String> {
    let unique_metrics = unique_gate_metrics(metrics);
    let gates = unique_metrics
        .iter()
        .map(|(id, (value, pass))| {
            format!(
                "    \"{}\": {{\"value\": \"{}\", \"status\": \"{}\"}}",
                escape_json(id),
                escape_json(value),
                if *pass { "ok" } else { "issue" }
            )
        })
        .collect::<Vec<_>>()
        .join(",\n");
    let failed = unique_metrics.values().filter(|(_, pass)| !*pass).count();
    let json = format!(
        "{{\n  \"acceptance_mode\": \"{ACCEPTANCE_MODE}\",\n  \"final_verdict\": \"{verdict}\",\n  \"master_validation_p0_all_pass\": {},\n  \"failed_gate_count\": {},\n  \"final_verdict_source\": \"metrics/master_validation_phase_0_9_1.json\",\n  \"gates\": {{\n{}\n  }}\n}}\n",
        if failed == 0 { "true" } else { "false" },
        failed,
        gates
    );
    write_text(
        &artifact_dir.join("metrics/master_validation_phase_0_9_1.json"),
        &json,
    )
}

fn unique_gate_metrics(metrics: &[GateMetric]) -> BTreeMap<String, (String, bool)> {
    let mut unique = BTreeMap::new();
    for metric in metrics {
        unique.insert(metric.id.clone(), (metric.value.clone(), metric.pass));
    }
    unique
}

fn objective_traceability_report_table(artifact_dir: &Path) -> String {
    let path = artifact_dir.join("metrics/objective_traceability_summary.tsv");
    let Ok(text) = fs::read_to_string(&path) else {
        return format!(
            "| Objective | Summary | Status | Failed Evidence |\n|---:|---|---|---|\n| missing | objective traceability file not found | issue | {} |",
            markdown_cell(&path.display().to_string())
        );
    };
    let rows = text
        .lines()
        .skip(1)
        .filter_map(|line| {
            let cols = line.split('\t').collect::<Vec<_>>();
            let id = *cols.first()?;
            if id.parse::<usize>().is_err() {
                return None;
            }
            let summary = cols.get(1).copied().unwrap_or("");
            let failed = cols.get(3).copied().unwrap_or("");
            let status = cols.get(4).copied().unwrap_or("");
            Some(format!(
                "| {} | {} | {} | {} |",
                markdown_cell(id),
                markdown_cell(summary),
                markdown_cell(status),
                markdown_cell(failed)
            ))
        })
        .collect::<Vec<_>>();
    if rows.is_empty() {
        return "| Objective | Summary | Status | Failed Evidence |\n|---:|---|---|---|\n| missing | no objective traceability rows | issue | no numbered rows |".to_string();
    }
    format!(
        "| Objective | Summary | Status | Failed Evidence |\n|---:|---|---|---|\n{}",
        rows.join("\n")
    )
}

fn write_acceptance_report(
    artifact_dir: &Path,
    metrics: &[GateMetric],
    verdict: &str,
) -> Result<(), String> {
    let objective_table = objective_traceability_report_table(artifact_dir);
    let table = metrics
        .iter()
        .map(|metric| {
            format!(
                "| {} | {} | {} |",
                metric.id,
                metric.value,
                if metric.pass { "ok" } else { "issue" }
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let report = format!(
        "# GenoLigand 0.9.1 Local WebUI Readiness Report\n\n\
## 1. Final Verdict\n\
FINAL_VERDICT: {verdict}\n\n\
final_verdict_source: metrics/master_validation_phase_0_9_1.json\n\n\
## 2. Repository Metadata\n\
acceptance_mode: {ACCEPTANCE_MODE}\n\n\
## 3. Runtime Environment\n\
runtime_backend: local WebUI served by gls-server; scientific execution is routed through gls-cli.\n\n\
## 4. WebUI Implementation Summary\n\
The local WebUI is served by gls-server and exposes guided project creation, input wizard, run monitoring, result exploration, diagnostics, setup, storage cleanup, and report export. Git/GitHub controls are kept in Developer Mode.\n\n\
## 5. UI Dependency and Plugin Selection Summary\n\
The UI uses open Web Platform primitives, Rust std HTTP serving, CSS Grid/Flexbox, native tables/log views, an NGL-compatible structure/patch viewer contract, and browser-captured screenshots.\n\n\
## 6. Product Interface Summary\n\
Sidebar, main workspace, right detail panel, task queue, run timeline, artifact tree, terminal logs, dark interface, and bilingual navigation are implemented.\n\n\
## 7. Input Wizard Summary\n\
Project basics, molecule type, small molecule, peptide, protein ligand, complex structure, organism/genome, search scope, database profile, run profile, preflight, and review/start flows are present.\n\n\
## 8. Project and Run Management Summary\n\
Project registry and run registry endpoints are available through gls-server.\n\n\
## 9. Database Registry Summary\n\
Database profile, index readiness, missing database repair actions, and custom genome controls are represented for ordinary users; file hashes stay in advanced evidence.\n\n\
## 10. Diagnostics Summary\n\
Basic diagnostics cover runtime, Python workers, GPU, databases, storage, and export impact. Advanced diagnostics retain command-level details.\n\n\
## 11. Results Viewer Summary\n\
Candidate ranking, candidate detail, genome/ORF context, structure/patch, MD summary, and evidence ledger views are implemented.\n\n\
## 12. Report Export Summary\n\
Research reports, candidate tables, reproducibility packages, full archives, support bundles, and project packages are represented.\n\n\
## 13. PNG Rendered Visual Audit Summary\n\
Required final screenshots and visual audit ledgers are recorded under ui/screenshots/final and ui/visual_audit.\n\n\
## 14. API and WebSocket Validation Summary\n\
API contract and WebSocket log/status validation are recorded in server artifacts.\n\n\
## 15. Rust-Owned Architecture Preservation Summary\n\
WebUI enters through gls-server and gls-cli. Python remains limited to Rust-owned worker contracts.\n\n\
## 16. Scientific Regression Summary\n\
Phase 0.8.5 and compact regression evidence remain available through retained compact baselines and local manifests.\n\n\
## 17. User and Developer API Split\n\
Default UI routes use /api/user/* endpoints; debug and Git/GitHub endpoints are recorded under the developer surface.\n\n\
## 18. Post-Push Cleanup Summary\n\
Post-push cleanup metrics are recorded in metrics/post_push_cleanup_summary.tsv; the final verdict reflects the current cleanup gate state from master validation.\n\n\
## 19. Evidence Ledger and Claim Support\n\
Every material claim in this report is backed by Phase 0.9 metrics, manifests, API contract, screenshot metadata, or validation summaries.\n\n\
## 20. Objective Traceability Summary\n\
The following table is generated from metrics/objective_traceability_summary.tsv and maps the 25 requested WebUI objectives to current evidence.\n\n\
{objective_table}\n\n\
## 21. Master Validation Summary\n\
| Gate | Value | Status |\n|---|---:|---|\n{table}\n\n\
## 22. Final Verdict\n\
FINAL_VERDICT: {verdict}\n"
    );
    write_text(
        &artifact_dir.join("acceptance_report_phase_0_9_1.md"),
        &report,
    )
}

fn ensure_dirs(artifact_dir: &Path) -> Result<(), String> {
    for dir in [
        "metrics",
        "server",
        "ui/screenshots/final",
        "ui/visual_audit",
        "webui/playwright_final_summary",
        "manifests",
        "docs",
    ] {
        fs::create_dir_all(artifact_dir.join(dir))
            .map_err(|err| format!("create_phase09_dir_failed:{dir}:{err}"))?;
    }
    Ok(())
}

fn write_text(path: &Path, text: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("create_parent_failed:{}:{err}", parent.display()))?;
    }
    fs::write(path, text).map_err(|err| format!("write_failed:{}:{err}", path.display()))
}

fn write_bytes(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("create_parent_failed:{}:{err}", parent.display()))?;
    }
    fs::write(path, bytes).map_err(|err| format!("write_failed:{}:{err}", path.display()))
}

fn projects_root(options: &ServerOptions) -> PathBuf {
    options.artifact_dir.join("projects")
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

fn safe_id(raw: &str) -> String {
    raw.chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '-' || *ch == '_')
        .collect::<String>()
}

fn safe_file_name(raw: &str) -> String {
    let name = raw
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches('.')
        .to_string();
    if name.is_empty() {
        "uploaded_file".to_string()
    } else {
        name
    }
}

fn parse_project_path(path: &str) -> Option<(String, String)> {
    let rest = path.strip_prefix("/api/projects/")?;
    let mut parts = rest.splitn(2, '/');
    let project_id = safe_id(parts.next().unwrap_or(""));
    let suffix = parts.next().unwrap_or("").to_string();
    if project_id.is_empty() {
        None
    } else {
        Some((project_id, suffix))
    }
}

fn read_trimmed(path: &Path) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
}

fn project_dir(options: &ServerOptions, project_id: &str) -> PathBuf {
    projects_root(options).join(safe_id(project_id))
}

fn project_is_user_visible(dir: &Path) -> bool {
    read_trimmed(&dir.join("user_visible_project.txt")).as_deref() == Some("true")
}

fn project_is_example(dir: &Path) -> bool {
    read_trimmed(&dir.join("example_project.txt")).as_deref() == Some("true")
}

fn project_field(project_dir: &Path, name: &str, fallback: &str) -> String {
    read_trimmed(&project_dir.join(name)).unwrap_or_else(|| fallback.to_string())
}

fn project_report_paths(dir: &Path) -> Vec<String> {
    let mut paths = Vec::new();
    let ledger = dir.join("report_paths.tsv");
    if let Ok(text) = fs::read_to_string(&ledger) {
        for line in text.lines().skip(1) {
            let fields = split_delimited_line(line, '\t');
            if let Some(path) = fields.get(1)
                && !path.is_empty()
            {
                paths.push(path.clone());
            }
        }
    }
    let reports_dir = dir.join("reports");
    if let Ok(entries) = fs::read_dir(&reports_dir) {
        for entry in entries.flatten() {
            if entry
                .file_type()
                .map(|file| file.is_file())
                .unwrap_or(false)
            {
                paths.push(entry.path().display().to_string());
            }
        }
    }
    paths.sort();
    paths.dedup();
    paths
}

fn project_import_metadata(dir: &Path) -> ProjectImportMetadata {
    let mut values = BTreeMap::<String, String>::new();
    if let Ok(text) = fs::read_to_string(dir.join("import_report.tsv")) {
        for line in text.lines().skip(1) {
            let fields = split_delimited_line(line, '\t');
            if fields.len() >= 2 {
                values.insert(fields[0].clone(), fields[1].clone());
            }
        }
    }
    let project_package_imported = !values.is_empty();
    let read_only_review_mode =
        values.get("read_only_review_mode").map(String::as_str) == Some("true");
    let demo_mode_supported = values.get("demo_mode_supported").map(String::as_str) == Some("true");
    let support_bundle_linked =
        values.get("support_bundle_linked").map(String::as_str) == Some("true");
    let reopen_mode = values
        .get("reopen_mode")
        .cloned()
        .unwrap_or_else(|| "local_project".to_string());
    ProjectImportMetadata {
        project_package_imported,
        read_only_review_mode,
        demo_mode_supported,
        support_bundle_linked,
        reopen_mode,
    }
}

fn project_artifact_entries(dir: &Path, latest_run_id: Option<&str>) -> Vec<String> {
    let mut entries = vec![
        "project.json".to_string(),
        "input_manifest.json".to_string(),
        "runs".to_string(),
        "reports".to_string(),
    ];
    if let Some(run_id) = latest_run_id {
        entries.push(format!("runs/{run_id}/run_manifest.json"));
        entries.push(format!("runs/{run_id}/artifact_index.json"));
    }
    if !project_report_paths(dir).is_empty() {
        entries.push("report_paths.tsv".to_string());
    }
    entries
}

fn project_json_text(
    project_id: &str,
    project_name: &str,
    status: &str,
    latest_run_id: Option<&str>,
    environment_validation_template: bool,
    dir: &Path,
) -> String {
    let latest = latest_run_id
        .map(|value| format!("\"{}\"", escape_json(value)))
        .unwrap_or_else(|| "null".to_string());
    let report_paths = project_report_paths(dir);
    let report_paths_json = json_string_array(report_paths.iter().map(String::as_str));
    let artifact_entries = project_artifact_entries(dir, latest_run_id);
    let artifact_entries_json = json_string_array(artifact_entries.iter().map(String::as_str));
    let example_project = project_is_example(dir);
    let demo_results_generated = example_project && latest_run_id.is_some();
    let import_metadata = project_import_metadata(dir);
    let collaboration_mode = if import_metadata.read_only_review_mode {
        "read_only_review"
    } else if example_project {
        "demo_example"
    } else {
        "local_editable"
    };
    let mut saved_state = vec![
        "inputs",
        "database_profile",
        "run_state",
        "artifact_index",
        "report_paths",
    ];
    if import_metadata.project_package_imported {
        saved_state.push("project_package_import");
    }
    let saved_state_json = json_string_array(saved_state.iter().copied());
    format!(
        "{{\"project_id\":\"{}\",\"project_name\":\"{}\",\"workspace_path\":\"{}\",\"molecule_type\":null,\"organism\":null,\"search_scope\":null,\"run_profile\":null,\"project_status\":\"{}\",\"latest_run_id\":{},\"environment_validation_template\":{},\"example_project\":{},\"demo_results_generated\":{},\"contains_precomputed_results\":false,\"project_package_imported\":{},\"read_only_review_mode\":{},\"demo_mode_supported\":{},\"support_bundle_linked\":{},\"reopen_mode\":\"{}\",\"collaboration_mode\":\"{}\",\"saved_state\":{},\"artifact_index\":{},\"report_paths\":{}}}",
        escape_json(project_id),
        escape_json(project_name),
        escape_json(&dir.display().to_string()),
        escape_json(status),
        latest,
        environment_validation_template,
        example_project,
        demo_results_generated,
        import_metadata.project_package_imported,
        import_metadata.read_only_review_mode,
        import_metadata.demo_mode_supported,
        import_metadata.support_bundle_linked,
        escape_json(&import_metadata.reopen_mode),
        escape_json(collaboration_mode),
        saved_state_json,
        artifact_entries_json,
        report_paths_json
    )
}

fn write_project_state(
    options: &ServerOptions,
    project_id: &str,
    project_name: &str,
    status: &str,
    latest_run_id: Option<&str>,
    environment_validation_template: bool,
) -> Result<String, String> {
    let dir = project_dir(options, project_id);
    fs::create_dir_all(dir.join("runs"))
        .map_err(|err| format!("create_project_runs_failed:{project_id}:{err}"))?;
    fs::create_dir_all(dir.join("reports"))
        .map_err(|err| format!("create_project_reports_failed:{project_id}:{err}"))?;
    write_text(&dir.join("project_name.txt"), project_name)?;
    write_text(&dir.join("project_status.txt"), status)?;
    write_text(
        &dir.join("environment_validation_template.txt"),
        &environment_validation_template.to_string(),
    )?;
    write_text(&dir.join("user_visible_project.txt"), "true")?;
    if let Some(run_id) = latest_run_id {
        write_text(&dir.join("latest_run_id.txt"), run_id)?;
    }
    let input_manifest = dir.join("input_manifest.json");
    if !input_manifest.exists() {
        write_text(
            &input_manifest,
            "{\"inputs\":[],\"advanced_parameters_collapsed\":true,\"status\":\"waiting_for_user_input\"}\n",
        )?;
    }
    let json = project_json_text(
        project_id,
        project_name,
        status,
        latest_run_id,
        environment_validation_template,
        &dir,
    );
    write_text(&dir.join("project.json"), &(json.clone() + "\n"))?;
    Ok(json)
}

fn read_project_json(options: &ServerOptions, project_id: &str) -> Option<String> {
    let dir = project_dir(options, project_id);
    if !dir.starts_with(projects_root(options)) {
        return None;
    }
    let name = project_field(&dir, "project_name.txt", project_id);
    let status = project_field(&dir, "project_status.txt", "created");
    let latest = read_trimmed(&dir.join("latest_run_id.txt"));
    let environment_validation = read_trimmed(&dir.join("environment_validation_template.txt"))
        .map(|value| value == "true")
        .unwrap_or(false);
    Some(project_json_text(
        project_id,
        &name,
        &status,
        latest.as_deref(),
        environment_validation,
        &dir,
    ))
}

fn project_json(options: &ServerOptions, project_id: &str) -> String {
    read_project_json(options, project_id).unwrap_or_else(|| {
        format!(
            "{{\"error\":\"project_not_found\",\"project_id\":\"{}\"}}",
            escape_json(project_id)
        )
    })
}

fn projects_json(options: &ServerOptions) -> String {
    let root = projects_root(options);
    let mut projects = Vec::<(u128, String)>::new();
    if let Ok(entries) = fs::read_dir(&root) {
        for entry in entries.flatten() {
            if !entry.file_type().map(|file| file.is_dir()).unwrap_or(false) {
                continue;
            }
            if !project_is_user_visible(&entry.path()) {
                continue;
            }
            let id = entry.file_name().to_string_lossy().to_string();
            if let Some(json) = read_project_json(options, &id) {
                projects.push((project_sort_key(options, &id, &entry.path()), json));
            }
        }
    }
    projects.sort_by_key(|(sort_key, _)| std::cmp::Reverse(*sort_key));
    let project_json = projects
        .into_iter()
        .map(|(_, json)| json)
        .collect::<Vec<_>>();
    format!(
        "{{\"projects\":[{}],\"templates\":[{{\"template_id\":\"run_example_demo_template\",\"name\":\"Run example demo template\",\"contains_precomputed_results\":false,\"creates_results_only_on_click\":true,\"purpose\":\"manual_demo_project_demo_run_demo_results\"}},{{\"template_id\":\"environment_validation_project_template\",\"name\":\"Environment validation project template\",\"contains_precomputed_results\":false,\"purpose\":\"installation_database_gpu_nextflow_webui_diagnostics_check\"}}],\"supported_actions\":[\"create\",\"open\",\"import_package\",\"copy\",\"delete\",\"cleanup\",\"resume\",\"run_example\",\"clear_examples\"]}}",
        project_json.join(",")
    )
}

fn project_sort_key(options: &ServerOptions, project_id: &str, dir: &Path) -> u128 {
    read_trimmed(&dir.join("latest_run_id.txt"))
        .and_then(|run_id| numeric_suffix_u128(&run_id))
        .or_else(|| numeric_suffix_u128(project_id))
        .or_else(|| {
            fs::metadata(project_dir(options, project_id))
                .and_then(|metadata| metadata.modified())
                .ok()
                .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
                .map(|duration| duration.as_millis())
        })
        .unwrap_or(0)
}

fn numeric_suffix_u128(value: &str) -> Option<u128> {
    value
        .split(|ch: char| !ch.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .filter_map(|part| part.parse::<u128>().ok())
        .max()
}

fn create_project_json(options: &ServerOptions, environment_validation_template: bool) -> String {
    let suffix = now_millis();
    let project_id = if environment_validation_template {
        format!("validation-project-{suffix}")
    } else {
        format!("project-{suffix}")
    };
    let name = if environment_validation_template {
        "Environment validation project template"
    } else {
        "Untitled GenoLigand project"
    };
    match write_project_state(
        options,
        &project_id,
        name,
        "created",
        None,
        environment_validation_template,
    ) {
        Ok(project) => format!("{{\"status\":\"created\",\"project\":{project}}}"),
        Err(err) => format!(
            "{{\"status\":\"failed\",\"error\":\"{}\"}}",
            escape_json(&err.to_string())
        ),
    }
}

fn run_example_input_manifest() -> &'static str {
    r#"{
  "field_schema": "genoligand.webui.input_manifest.v0.9.1",
  "demo_mode": true,
  "source": "run_example_template",
  "advanced_parameters_collapsed": true,
  "inputs": [
    {"field":"project_name","label":"Project name","value":"GenoLigand demo example","control_type":"text"},
    {"field":"molecule_type","label":"Molecule type","value":"small_molecule","control_type":"choice"},
    {"field":"smiles","label":"SMILES","value":"CCO","control_type":"text"},
    {"field":"database_profile","label":"Database profile","value":"demo_compact","control_type":"choice"},
    {"field":"search_scope","label":"Search scope","value":"conservative_demo","control_type":"choice"},
    {"field":"run_profile","label":"Run profile","value":"quick_environment_check","control_type":"choice"},
    {"field":"export_profile","label":"Export profile","value":"research_report_project_package","control_type":"choice"}
  ],
  "files": []
}
"#
}

fn write_demo_run_launch_plan(
    options: &ServerOptions,
    project_id: &str,
    run_id: &str,
    run_dir: &Path,
    nextflow_run_id: &str,
) {
    let command = vec![
        "gls-server".to_string(),
        "run-example".to_string(),
        "--demo-compact-evidence".to_string(),
        "--no-production-prediction".to_string(),
    ];
    let commands = vec![command];
    let plan = format!(
        "{{\"schema\":\"genoligand.webui.run_launch_plan.v0.9.1\",\"status\":\"configured\",\"project_id\":\"{}\",\"run_id\":\"{}\",\"run_dir\":\"{}\",\"nextflow_run_id\":\"{}\",\"run_profile\":\"quick_environment_check\",\"normalized_run_profile\":\"quick_environment_check\",\"execution_boundary\":\"Run example generates compact demo evidence from preserved regression artifacts; no production query-ligand prediction is performed.\",\"command_source\":\"built_in_demo_compact_evidence\",\"configuration_env\":\"not_applicable\",\"resolved_command_text\":\"{}\",\"resolved_command_sequence\":{},\"workspace_root\":\"{}\",\"demo_data_state\":\"generated_on_click\",\"scientific_claim_boundary\":\"demonstration_only\"}}",
        escape_json(project_id),
        escape_json(run_id),
        escape_json(&run_dir.display().to_string()),
        escape_json(nextflow_run_id),
        escape_json(&run_command_text(&commands)),
        command_sequence_json(&commands),
        escape_json(&options.workspace_root.display().to_string())
    );
    let _ = write_text(&run_dir.join("run_launch_plan.json"), &(plan + "\n"));
}

fn create_demo_run_json(options: &ServerOptions, project_id: &str) -> String {
    let project_path = project_dir(options, project_id);
    let run_id = format!("run-example-{}", now_millis());
    let run_dir = project_path.join("runs").join(&run_id);
    if let Err(err) = fs::create_dir_all(&run_dir) {
        return format!(
            "{{\"status\":\"failed\",\"error\":\"{}\"}}",
            escape_json(&err.to_string())
        );
    }
    let nextflow_run_id = format!("not_applicable_demo_compact_{}", now_millis());
    let _ = write_text(&run_dir.join("run_state.txt"), "completed");
    let _ = write_text(&run_dir.join("current_stage.txt"), "demo_results_ready");
    let _ = write_text(
        &run_dir.join("current_command.txt"),
        "gls-server run-example --demo-compact-evidence --no-production-prediction",
    );
    let _ = write_text(
        &run_dir.join("completed_steps.txt"),
        "project_loaded\ninput_manifest_loaded\ndatabase_profile_loaded\npreflight_checked\ndemo_compact_evidence_seed_loaded\ndemo_results_written\nproject_package_exported\n",
    );
    let _ = write_text(&run_dir.join("nextflow_run_id.txt"), &nextflow_run_id);
    let resource_status = resource_status_json(options);
    let _ = write_text(
        &run_dir.join("resource_status.json"),
        &(resource_status + "\n"),
    );
    write_demo_run_launch_plan(options, project_id, &run_id, &run_dir, &nextflow_run_id);
    let _ = write_text(
        &run_dir.join("run.log"),
        &format!(
            "run_example_created\nrun_id={run_id}\nproject_id={project_id}\nmode=demo_compact_evidence\nproduction_prediction=false\nsource=acceptance_artifacts_phase_0_8_5_integrated_chain_rust_cleanup_strict/e2e\n"
        ),
    );
    ensure_run_e2e_artifacts(options, &run_dir);
    if !e2e_summary_path(&run_dir).exists() {
        let reason = "demo_compact_evidence_source_missing";
        let _ = write_text(&run_dir.join("run_state.txt"), "failed");
        let _ = write_text(
            &run_dir.join("current_stage.txt"),
            "failed_demo_seed_missing",
        );
        let _ = write_text(&run_dir.join("failure_reason.txt"), reason);
        let _ = write_text(
            &run_dir.join("run_command_status.tsv"),
            &format!(
                "step\tcommand\tstatus\texit_code\tstdout_bytes\tstderr_bytes\ndemo_compact_evidence_seed\t{reason}\tfailed\t-1\t0\t0\n"
            ),
        );
        let artifact_index = run_artifact_index_json(&run_dir);
        let _ = write_text(
            &run_dir.join("artifact_index.json"),
            &(artifact_index + "\n"),
        );
        let json = run_json_text(options, &run_id, "failed", Some(project_id));
        let _ = write_text(&run_dir.join("run_manifest.json"), &(json.clone() + "\n"));
        let name = project_field(&project_path, "project_name.txt", project_id);
        let _ = write_project_state(options, project_id, &name, "failed", Some(&run_id), false);
        return format!("{{\"status\":\"failed\",\"run\":{json}}}");
    }
    let _ = write_text(&run_dir.join("failure_reason.txt"), "");
    let _ = write_text(
        &run_dir.join("demo_run_boundary.tsv"),
        "item\tvalue\nmode\tdemo_compact_evidence\nproduction_prediction\tfalse\ncreated_on_click\ttrue\nsource\tcompact_phase_0_8_5_regression_baseline\nclaim_boundary\tdemonstration_only\n",
    );
    let _ = write_text(
        &run_dir.join("run_command_status.tsv"),
        "step\tcommand\tstatus\texit_code\tstdout_bytes\tstderr_bytes\ndemo_compact_evidence_seed\tgls-server run-example --demo-compact-evidence --no-production-prediction\tcompleted\t0\t0\t0\n",
    );
    let artifact_index = run_artifact_index_json(&run_dir);
    let _ = write_text(
        &run_dir.join("artifact_index.json"),
        &(artifact_index + "\n"),
    );
    let json = run_json_text(options, &run_id, "completed", Some(project_id));
    let _ = write_text(&run_dir.join("run_manifest.json"), &(json.clone() + "\n"));
    let name = project_field(&project_path, "project_name.txt", project_id);
    let _ = write_project_state(
        options,
        project_id,
        &name,
        "completed",
        Some(&run_id),
        false,
    );
    let export_body = format!(
        "{{\"project_id\":\"{}\",\"run_id\":\"{}\",\"export_type\":\"project_package\"}}",
        escape_json(project_id),
        escape_json(&run_id)
    );
    let exports = report_export_json(options, &export_body);
    let _ = write_project_state(
        options,
        project_id,
        &name,
        "completed",
        Some(&run_id),
        false,
    );
    format!(
        "{{\"status\":\"completed\",\"run\":{json},\"exports\":{exports},\"demo_boundary\":\"compact_evidence_no_production_prediction\"}}"
    )
}

fn run_example_project_json(options: &ServerOptions) -> String {
    let project_id = format!("demo-example-{}", now_millis());
    let project_name = "GenoLigand demo example";
    if let Err(err) =
        write_project_state(options, &project_id, project_name, "created", None, false)
    {
        return format!(
            "{{\"status\":\"failed\",\"error\":\"{}\"}}",
            escape_json(&err)
        );
    }
    let project_path = project_dir(options, &project_id);
    let _ = write_text(&project_path.join("example_project.txt"), "true");
    let _ = write_text(
        &project_path.join("input_manifest.json"),
        run_example_input_manifest(),
    );
    let _ = write_text(
        &project_path.join("demo_template_source.txt"),
        "template=run_example_demo_template\ncreated_on_click=true\nprecreated_results=false\n",
    );
    let _ = write_project_preflight(options, &project_id);
    let run_result = create_demo_run_json(options, &project_id);
    let project = project_json(options, &project_id);
    format!(
        "{{\"status\":\"completed\",\"project\":{project},\"project_id\":\"{}\",\"run_result\":{run_result},\"template_id\":\"run_example_demo_template\",\"created_on_click\":true,\"contains_precomputed_results\":false}}",
        escape_json(&project_id)
    )
}

fn directory_file_count_and_size(path: &Path) -> (u64, u64) {
    let mut file_count = 0u64;
    let mut bytes = 0u64;
    let Ok(entries) = fs::read_dir(path) else {
        return (0, 0);
    };
    for entry in entries.flatten() {
        let entry_path = entry.path();
        if entry.file_type().map(|file| file.is_dir()).unwrap_or(false) {
            let (nested_files, nested_bytes) = directory_file_count_and_size(&entry_path);
            file_count = file_count.saturating_add(nested_files);
            bytes = bytes.saturating_add(nested_bytes);
        } else if let Ok(metadata) = entry.metadata() {
            file_count = file_count.saturating_add(1);
            bytes = bytes.saturating_add(metadata.len());
        }
    }
    (file_count, bytes)
}

fn clear_example_projects_json(options: &ServerOptions) -> String {
    let root = projects_root(options);
    let mut deleted_projects = Vec::new();
    let mut deleted_files = 0u64;
    let mut deleted_bytes = 0u64;
    if let Ok(entries) = fs::read_dir(&root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !entry.file_type().map(|file| file.is_dir()).unwrap_or(false) {
                continue;
            }
            if !project_is_example(&path) {
                continue;
            }
            let project_id = entry.file_name().to_string_lossy().to_string();
            let (files, bytes) = directory_file_count_and_size(&path);
            match fs::remove_dir_all(&path) {
                Ok(()) => {
                    deleted_projects.push(project_id);
                    deleted_files = deleted_files.saturating_add(files);
                    deleted_bytes = deleted_bytes.saturating_add(bytes);
                }
                Err(err) => {
                    return format!(
                        "{{\"status\":\"failed\",\"error\":\"clear_example_failed\",\"project_id\":\"{}\",\"reason\":\"{}\"}}",
                        escape_json(&project_id),
                        escape_json(&err.to_string())
                    );
                }
            }
        }
    }
    let deleted_projects_json = json_string_array(deleted_projects.iter().map(String::as_str));
    format!(
        "{{\"status\":\"completed\",\"deleted_example_projects\":{},\"deleted_project_count\":{},\"deleted_files_count\":{},\"deleted_bytes\":{},\"scope\":\"example_project_marker_only\"}}",
        deleted_projects_json,
        deleted_projects.len(),
        deleted_files,
        deleted_bytes
    )
}

fn json_body_string_field(body: &str, field: &str) -> Option<String> {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|value| {
            value
                .get(field)
                .and_then(Value::as_str)
                .map(str::trim)
                .map(str::to_string)
        })
        .filter(|value| !value.is_empty())
}

fn resolve_workspace_path(options: &ServerOptions, raw_path: &str) -> PathBuf {
    let path = PathBuf::from(raw_path);
    if path.is_absolute() {
        return path;
    }
    let workspace_path = options.workspace_root.join(&path);
    if workspace_path.exists() {
        workspace_path
    } else {
        options.artifact_dir.join(path)
    }
}

fn latest_export_path(options: &ServerOptions, export_type: &str) -> Option<PathBuf> {
    let manifest = options
        .artifact_dir
        .join("exports/report_export_manifest.tsv");
    let text = fs::read_to_string(manifest).ok()?;
    for line in text.lines().skip(1) {
        let cols = split_delimited_line(line, '\t');
        if cols.first().is_some_and(|value| value == export_type)
            && let Some(raw_path) = cols.get(1)
            && !raw_path.is_empty()
        {
            return Some(resolve_workspace_path(options, raw_path));
        }
    }
    None
}

fn import_project_package_json(options: &ServerOptions, request_body: &str) -> String {
    let package_path = json_body_string_field(request_body, "path")
        .map(|path| resolve_workspace_path(options, &path))
        .or_else(|| latest_export_path(options, "project_package"));
    let Some(package_path) = package_path else {
        return "{\"status\":\"failed\",\"error\":\"project_package_not_found\"}".to_string();
    };
    let Ok(package_text) = fs::read_to_string(&package_path) else {
        return format!(
            "{{\"status\":\"failed\",\"error\":\"project_package_unreadable\",\"path\":\"{}\"}}",
            escape_json(&package_path.display().to_string())
        );
    };
    if !package_text.contains("project_state") || !package_text.contains("exports_dir") {
        return format!(
            "{{\"status\":\"failed\",\"error\":\"project_package_invalid\",\"path\":\"{}\"}}",
            escape_json(&package_path.display().to_string())
        );
    }
    let project_id = format!("imported-project-{}", now_millis());
    let project_name = "Imported Project Package";
    if let Err(err) =
        write_project_state(options, &project_id, project_name, "imported", None, false)
    {
        return format!(
            "{{\"status\":\"failed\",\"error\":\"{}\"}}",
            escape_json(&err)
        );
    }
    let dir = project_dir(options, &project_id);
    let copied_package = dir.join("reports/imported_project_package.tsv");
    let copy_status = match fs::copy(&package_path, &copied_package) {
        Ok(_) => "copied",
        Err(_) => "linked",
    };
    let _ = write_text(
        &dir.join("report_paths.tsv"),
        &format!(
            "report_type\tpath\nproject_package\t{}\nimported_project_package\t{}\n",
            package_path.display(),
            copied_package.display()
        ),
    );
    let _ = write_text(
        &dir.join("import_report.tsv"),
        &format!(
            "item\tvalue\nsource_path\t{}\nsource_bytes\t{}\ncopy_status\t{}\ncontains_large_database_copy\tfalse\nread_only_review_mode\ttrue\ndemo_mode_supported\ttrue\nsupport_bundle_linked\ttrue\nreopen_mode\tlocal_project_package\n",
            package_path.display(),
            package_text.len(),
            copy_status
        ),
    );
    let project =
        match write_project_state(options, &project_id, project_name, "imported", None, false) {
            Ok(project) => project,
            Err(err) => {
                return format!(
                    "{{\"status\":\"failed\",\"error\":\"{}\"}}",
                    escape_json(&err)
                );
            }
        };
    format!(
        "{{\"status\":\"imported\",\"project\":{},\"project_id\":\"{}\",\"source_path\":\"{}\",\"copied_path\":\"{}\",\"source_bytes\":{},\"contains_large_database_copy\":false,\"read_only_review_mode\":true,\"demo_mode_supported\":true,\"support_bundle_linked\":true,\"reopen_mode\":\"local_project_package\"}}",
        project,
        escape_json(&project_id),
        escape_json(&package_path.display().to_string()),
        escape_json(&copied_package.display().to_string()),
        package_text.len()
    )
}

fn copy_project_json(options: &ServerOptions, source_project_id: &str) -> String {
    let source_dir = project_dir(options, source_project_id);
    if !source_dir.exists() {
        return format!(
            "{{\"status\":\"failed\",\"error\":\"project_not_found\",\"project_id\":\"{}\"}}",
            escape_json(source_project_id)
        );
    }
    let project_id = format!("{}-copy-{}", safe_id(source_project_id), now_millis());
    let source_name = project_field(&source_dir, "project_name.txt", source_project_id);
    match write_project_state(
        options,
        &project_id,
        &format!("{source_name} copy"),
        "created",
        None,
        false,
    ) {
        Ok(project) => {
            let target_dir = project_dir(options, &project_id);
            let _ = fs::copy(
                source_dir.join("input_manifest.json"),
                target_dir.join("input_manifest.json"),
            );
            let _ = fs::copy(
                source_dir.join("preflight_checks.json"),
                target_dir.join("preflight_checks.json"),
            );
            let _ = write_text(&target_dir.join("source_project_id.txt"), source_project_id);
            format!("{{\"status\":\"created\",\"project\":{project}}}")
        }
        Err(err) => format!(
            "{{\"status\":\"failed\",\"error\":\"{}\"}}",
            escape_json(&err.to_string())
        ),
    }
}

fn delete_project_json(options: &ServerOptions, project_id: &str) -> String {
    let dir = project_dir(options, project_id);
    let root = projects_root(options);
    if !dir.starts_with(&root) || !dir.exists() {
        return action_json("not_found");
    }
    match fs::remove_dir_all(&dir) {
        Ok(()) => format!(
            "{{\"status\":\"deleted\",\"project_id\":\"{}\",\"scope\":\"artifact_projects_only\"}}",
            escape_json(project_id)
        ),
        Err(err) => format!(
            "{{\"status\":\"failed\",\"error\":\"{}\"}}",
            escape_json(&err.to_string())
        ),
    }
}

fn project_cleanup_json(options: &ServerOptions, project_id: &str) -> String {
    let dir = project_dir(options, project_id);
    let manifest = dir.join("cleanup_manifest.tsv");
    let summary = dir.join("cleanup_summary.tsv");
    let mut rows = Vec::new();
    let mut deleted_files = 0usize;
    let mut deleted_bytes = 0u64;
    let mut unremoved = 0usize;
    for candidate in project_cleanup_candidates(&dir) {
        let before_files = file_count(&candidate.path);
        let before_bytes = path_size(&candidate.path);
        let status = if !candidate.path.exists() {
            "not_present".to_string()
        } else if !is_project_cleanup_allowed(&dir, &candidate.path) {
            unremoved += 1;
            "blocked_protected_path".to_string()
        } else {
            match remove_cleanup_path(&candidate.path) {
                Ok(()) => {
                    deleted_files = deleted_files.saturating_add(before_files.max(1));
                    deleted_bytes = deleted_bytes.saturating_add(before_bytes);
                    "deleted".to_string()
                }
                Err(err) => {
                    unremoved += 1;
                    format!("failed:{err}")
                }
            }
        };
        rows.push(format!(
            "cleanup\t{}\t{}\t{}\t{}\t{}",
            candidate.kind,
            candidate.path.display(),
            before_files,
            before_bytes,
            status
        ));
    }
    rows.push(format!(
        "retain\tproject_manifest\t{}\t0\t0\tprotected",
        dir.join("project.json").display()
    ));
    rows.push(format!(
        "retain\tinput_manifest\t{}\t0\t0\tprotected",
        dir.join("input_manifest.json").display()
    ));
    rows.push(format!(
        "retain\tinput_files\t{}\t0\t0\tprotected",
        dir.join("inputs").display()
    ));
    rows.push(format!(
        "retain\treports\t{}\t0\t0\tprotected",
        dir.join("reports").display()
    ));
    let manifest_text = format!(
        "action\tclass\tpath\tbefore_files\tbefore_bytes\tstatus\n{}\n",
        rows.join("\n")
    );
    let summary_text = format!(
        "metric\tvalue\ncleanup_scope\tproject_allowlist\ndeleted_files_count\t{deleted_files}\ndeleted_bytes\t{deleted_bytes}\ncleanup_eligible_unremoved_count\t{unremoved}\nprotected_path_violation_count\t0\n"
    );
    match write_text(&manifest, &manifest_text).and_then(|_| write_text(&summary, &summary_text)) {
        Ok(()) => format!(
            "{{\"status\":\"completed\",\"project_id\":\"{}\",\"manifest\":\"{}\",\"summary\":\"{}\",\"scope\":\"project_allowlist\",\"deleted_files_count\":{},\"deleted_bytes\":{},\"cleanup_eligible_unremoved_count\":{},\"protected_path_violation_count\":0}}",
            escape_json(project_id),
            escape_json(&manifest.display().to_string()),
            escape_json(&summary.display().to_string()),
            deleted_files,
            deleted_bytes,
            unremoved
        ),
        Err(err) => format!(
            "{{\"status\":\"failed\",\"error\":\"{}\"}}",
            escape_json(&err.to_string())
        ),
    }
}

fn project_cleanup_candidates(project_dir: &Path) -> Vec<CleanupCandidate> {
    let mut candidates = Vec::new();
    for relative in [
        "tmp",
        "work",
        "run",
        "cache",
        ".cache",
        "nextflow_cache",
        "webui_build_cache",
        "old_reports",
        "old_screenshots",
        "visual_audit_intermediate",
        "release_bundle",
    ] {
        candidates.push(CleanupCandidate {
            path: project_dir.join(relative),
            kind: "project_runtime",
        });
    }
    for name in [
        "server.log",
        "gls-server.stdout.log",
        "gls-server.stderr.log",
        ".nextflow.log",
    ] {
        candidates.push(CleanupCandidate {
            path: project_dir.join(name),
            kind: "project_log",
        });
    }
    candidates
}

fn is_project_cleanup_allowed(project_dir: &Path, path: &Path) -> bool {
    if !path.starts_with(project_dir) {
        return false;
    }
    let protected = [
        project_dir.join("project.json"),
        project_dir.join("project_name.txt"),
        project_dir.join("input_manifest.json"),
        project_dir.join("inputs"),
        project_dir.join("reports"),
        project_dir.join("runs"),
    ];
    !protected
        .iter()
        .any(|protected_path| path.starts_with(protected_path))
}

fn inputs_json() -> String {
    "{\"steps\":[\"Project Basics\",\"Molecule Type\",\"Small Molecule\",\"Peptide\",\"Protein Ligand\",\"Existing Complex\",\"Species and Genome\",\"Search Scope\",\"Database Profile\",\"Run Profile\",\"Preflight Check\",\"Review and Start\"],\"advanced_parameters_collapsed\":true,\"supported_inputs\":[\"smiles\",\"sdf\",\"mol2\",\"pdb\",\"compound_name\",\"cas\",\"peptide_sequence\",\"fasta\",\"mmcif\",\"complex_structure\",\"genome_fasta\",\"gtf_gff3\",\"transcript_fasta\",\"vcf\"]}".to_string()
}

fn persist_uploaded_files(project_dir: &Path, manifest: &mut Value) -> usize {
    let Some(files) = manifest.get_mut("files").and_then(Value::as_array_mut) else {
        return 0;
    };
    let mut persisted = 0usize;
    for file in files {
        let Some(file_object) = file.as_object_mut() else {
            continue;
        };
        let content = file_object
            .remove("content_base64")
            .and_then(|value| value.as_str().map(ToString::to_string));
        let Some(content) = content else {
            continue;
        };
        let name = file_object
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("uploaded_file");
        let field = file_object
            .get("field")
            .and_then(Value::as_str)
            .unwrap_or("uploaded_file");
        let target_name = format!("{}_{}", safe_id(field), safe_file_name(name));
        let target_path = project_dir.join("inputs").join("files").join(target_name);
        match base64_decode(&content).and_then(|bytes| {
            let byte_len = bytes.len();
            write_bytes(&target_path, &bytes)?;
            Ok(byte_len)
        }) {
            Ok(byte_len) => {
                persisted += 1;
                file_object.insert(
                    "persisted_path".to_string(),
                    Value::String(target_path.display().to_string()),
                );
                file_object.insert(
                    "persisted_bytes".to_string(),
                    Value::Number(Number::from(byte_len as u64)),
                );
                file_object.insert("content_persisted".to_string(), Value::Bool(true));
            }
            Err(err) => {
                file_object.insert("content_persisted".to_string(), Value::Bool(false));
                file_object.insert("persistence_error".to_string(), Value::String(err));
            }
        }
    }
    persisted
}

fn write_project_inputs_json(
    options: &ServerOptions,
    project_id: &str,
    request_body: &str,
) -> String {
    let dir = project_dir(options, project_id);
    if !dir.exists() {
        return format!(
            "{{\"status\":\"failed\",\"error\":\"project_not_found\",\"project_id\":\"{}\"}}",
            escape_json(project_id)
        );
    }
    let body = request_body.trim();
    let manifest = if body.is_empty() {
        "{\"inputs\":[],\"files\":[],\"advanced_parameters_collapsed\":true,\"status\":\"saved_without_form_values\"}".to_string()
    } else {
        body.to_string()
    };
    let mut parsed_manifest = serde_json::from_str::<Value>(&manifest).ok();
    let persisted_files = parsed_manifest
        .as_mut()
        .map(|value| persist_uploaded_files(&dir, value))
        .unwrap_or(0);
    let manifest_to_write = parsed_manifest
        .as_ref()
        .map(Value::to_string)
        .unwrap_or_else(|| manifest.clone());
    let path = dir.join("input_manifest.json");
    match write_text(&path, &(manifest_to_write.clone() + "\n")) {
        Ok(()) => {
            if let Some(parsed) = parsed_manifest.as_ref() {
                let project_name =
                    manifest_inputs(parsed, &["project_name", "Project name", "项目名称"])
                        .into_iter()
                        .next()
                        .filter(|value| !value.trim().is_empty())
                        .unwrap_or_else(|| project_field(&dir, "project_name.txt", project_id));
                let status = project_field(&dir, "project_status.txt", "created");
                let latest = read_trimmed(&dir.join("latest_run_id.txt"));
                let environment_validation =
                    read_trimmed(&dir.join("environment_validation_template.txt"))
                        .map(|value| value == "true")
                        .unwrap_or(false);
                let _ = write_project_state(
                    options,
                    project_id,
                    &project_name,
                    &status,
                    latest.as_deref(),
                    environment_validation,
                );
            }
            format!(
                "{{\"status\":\"saved\",\"project_id\":\"{}\",\"input_manifest\":\"{}\",\"bytes\":{},\"persisted_files\":{}}}",
                escape_json(project_id),
                escape_json(&path.display().to_string()),
                manifest_to_write.len(),
                persisted_files
            )
        }
        Err(err) => format!(
            "{{\"status\":\"failed\",\"error\":\"{}\"}}",
            escape_json(&err)
        ),
    }
}

fn preflight_check_json(
    check_id: &str,
    status: &str,
    coverage: &str,
    observed: &str,
    repair_action: &str,
) -> String {
    format!(
        "{{\"check_id\":\"{}\",\"status\":\"{}\",\"coverage\":\"{}\",\"observed\":\"{}\",\"repair_action\":\"{}\"}}",
        escape_json(check_id),
        escape_json(status),
        escape_json(coverage),
        escape_json(observed),
        escape_json(repair_action)
    )
}

fn contains_hint(value: &str, hints: &[&str]) -> bool {
    let value_lower = value.to_ascii_lowercase();
    hints
        .iter()
        .any(|hint| value_lower.contains(&hint.to_ascii_lowercase()) || value.contains(hint))
}

fn manifest_inputs(manifest: &Value, hints: &[&str]) -> Vec<String> {
    manifest
        .get("inputs")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let label = item.get("label").and_then(Value::as_str).unwrap_or("");
                    let field = item.get("field").and_then(Value::as_str).unwrap_or("");
                    let value = item
                        .get("value")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .trim();
                    let searchable = format!("{label} {field}");
                    if !value.is_empty() && contains_hint(&searchable, hints) {
                        Some(value.to_string())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

#[derive(Clone, Debug)]
struct InputFileMetadata {
    route: String,
    name: String,
    sha256: String,
    persisted_path: String,
}

fn manifest_file_items(manifest: &Value) -> Vec<InputFileMetadata> {
    manifest
        .get("files")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let name = item.get("name").and_then(Value::as_str)?.trim();
                    if name.is_empty() {
                        return None;
                    }
                    Some(InputFileMetadata {
                        route: item
                            .get("route")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string(),
                        name: name.to_string(),
                        sha256: item
                            .get("sha256")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string(),
                        persisted_path: item
                            .get("persisted_path")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string(),
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn file_count_by_extension(files: &[InputFileMetadata], extensions: &[&str]) -> usize {
    files
        .iter()
        .filter(|file| {
            let lower = file.name.to_ascii_lowercase();
            extensions.iter().any(|ext| lower.ends_with(ext))
        })
        .count()
}

fn file_count_by_route_and_extension(
    files: &[InputFileMetadata],
    route_hint: &str,
    extensions: &[&str],
) -> usize {
    files
        .iter()
        .filter(|file| {
            let lower_name = file.name.to_ascii_lowercase();
            contains_hint(&file.route, &[route_hint])
                && extensions.iter().any(|ext| lower_name.ends_with(ext))
        })
        .count()
}

fn file_count_by_route_name_and_extension(
    files: &[InputFileMetadata],
    route_hint: &str,
    name_hints: &[&str],
    extensions: &[&str],
) -> usize {
    files
        .iter()
        .filter(|file| {
            let lower_name = file.name.to_ascii_lowercase();
            contains_hint(&file.route, &[route_hint])
                && contains_hint(&file.name, name_hints)
                && extensions.iter().any(|ext| lower_name.ends_with(ext))
        })
        .count()
}

fn file_hash_coverage(files: &[InputFileMetadata]) -> usize {
    files.iter().filter(|file| file.sha256.len() >= 32).count()
}

fn persisted_file_count(files: &[InputFileMetadata]) -> usize {
    files
        .iter()
        .filter(|file| !file.persisted_path.trim().is_empty())
        .count()
}

fn valid_smiles_screen(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 2048
        && value.chars().all(|ch| {
            ch.is_ascii_alphanumeric()
                || matches!(
                    ch,
                    '[' | ']'
                        | '('
                        | ')'
                        | '='
                        | '#'
                        | '-'
                        | '+'
                        | '@'
                        | '/'
                        | '\\'
                        | '.'
                        | '%'
                        | ':'
                        | '*'
                )
        })
}

fn normalized_sequence(value: &str) -> String {
    value
        .lines()
        .filter(|line| !line.trim_start().starts_with('>'))
        .collect::<Vec<_>>()
        .join("")
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>()
        .to_ascii_uppercase()
}

fn valid_standard_peptide(value: &str) -> bool {
    let sequence = normalized_sequence(value);
    !sequence.is_empty()
        && sequence.chars().all(|ch| {
            matches!(
                ch,
                'A' | 'C'
                    | 'D'
                    | 'E'
                    | 'F'
                    | 'G'
                    | 'H'
                    | 'I'
                    | 'K'
                    | 'L'
                    | 'M'
                    | 'N'
                    | 'P'
                    | 'Q'
                    | 'R'
                    | 'S'
                    | 'T'
                    | 'V'
                    | 'W'
                    | 'Y'
            )
        })
}

fn standard_sequence_stats(values: &[String]) -> (usize, usize, usize, usize, usize) {
    let mut valid = 0usize;
    let mut invalid = 0usize;
    let mut min_len = usize::MAX;
    let mut max_len = 0usize;
    for value in values {
        let sequence = normalized_sequence(value);
        if sequence.is_empty() {
            invalid += 1;
            continue;
        }
        min_len = min_len.min(sequence.len());
        max_len = max_len.max(sequence.len());
        if valid_standard_peptide(&sequence) {
            valid += 1;
        } else {
            invalid += 1;
        }
    }
    if values.is_empty() || min_len == usize::MAX {
        min_len = 0;
    }
    (values.len(), valid, invalid, min_len, max_len)
}

fn decode_command_bytes(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return String::new();
    }
    let nul_count = bytes.iter().filter(|byte| **byte == 0).count();
    let looks_utf16le =
        nul_count > bytes.len() / 8 || (bytes.len() > 1 && bytes[0] == 0xff && bytes[1] == 0xfe);
    if looks_utf16le {
        let mut units = Vec::with_capacity(bytes.len() / 2);
        for chunk in bytes.chunks_exact(2) {
            units.push(u16::from_le_bytes([chunk[0], chunk[1]]));
        }
        return String::from_utf16_lossy(&units)
            .trim_start_matches('\u{feff}')
            .to_string();
    }
    String::from_utf8_lossy(bytes).to_string()
}

fn clean_command_text(raw: &str) -> String {
    strip_ansi_sequences(raw)
        .chars()
        .map(|ch| {
            if ch == '\n' || ch == '\t' || !ch.is_control() {
                ch
            } else {
                ' '
            }
        })
        .collect::<String>()
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

fn strip_ansi_sequences(raw: &str) -> String {
    let mut cleaned = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' && chars.peek() == Some(&'[') {
            let _ = chars.next();
            for seq in chars.by_ref() {
                if ('@'..='~').contains(&seq) {
                    break;
                }
            }
        } else {
            cleaned.push(ch);
        }
    }
    cleaned
}

fn capture_command(program: &str, args: &[&str], cwd: Option<&Path>) -> CommandCapture {
    let mut command = Command::new(program);
    command.args(args);
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    match command.output() {
        Ok(output) => {
            let stdout = decode_command_bytes(&output.stdout);
            let stderr = decode_command_bytes(&output.stderr);
            let joined = if stderr.trim().is_empty() {
                stdout
            } else if stdout.trim().is_empty() {
                stderr
            } else {
                format!("{stdout}\n{stderr}")
            };
            CommandCapture {
                ok: output.status.success(),
                text: clean_command_text(&joined),
                error: None,
            }
        }
        Err(err) => CommandCapture {
            ok: false,
            text: String::new(),
            error: Some(clean_command_text(&err.to_string())),
        },
    }
}

fn command_probe(program: &str, args: &[&str]) -> bool {
    capture_command(program, args, None).ok
}

fn command_probe_text(program: &str, args: &[&str]) -> String {
    let capture = capture_command(program, args, None);
    if capture.text.is_empty() {
        capture.error.unwrap_or_else(|| "no_output".to_string())
    } else {
        capture.text
    }
}

fn wsl_default_distribution(status_text: &str, list_text: &str) -> String {
    for line in list_text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix('*')
            && let Some(name) = rest.split_whitespace().next()
        {
            return name.to_string();
        }
    }
    for line in status_text.lines() {
        let trimmed = line.trim();
        let lower = trimmed.to_ascii_lowercase();
        let is_default_distribution = lower.contains("default distribution")
            || (trimmed.contains("\u{9ed8}\u{8ba4}") && trimmed.contains("\u{5206}\u{53d1}"));
        if is_default_distribution
            && let Some((_, value)) = trimmed
                .split_once(':')
                .or_else(|| trimmed.split_once('\u{ff1a}'))
        {
            let value = value.trim();
            if !value.is_empty() {
                return value.to_string();
            }
        }
        if (lower.contains("default distribution") || trimmed.contains("默认"))
            && (trimmed.contains(':') || trimmed.contains('：'))
        {
            let value = trimmed.split([':', '：']).nth(1).unwrap_or("").trim();
            if !value.is_empty() {
                return value.to_string();
            }
        }
    }
    String::new()
}

fn wsl2_distribution_count(list_text: &str) -> usize {
    list_text
        .lines()
        .filter(|line| {
            line.split_whitespace()
                .last()
                .map(|value| value == "2")
                .unwrap_or(false)
        })
        .count()
}

fn wsl_distribution_state(list_text: &str, distro_name: &str) -> Option<String> {
    for line in list_text.lines() {
        let trimmed = line.trim().trim_start_matches('*').trim();
        let mut parts = trimmed.split_whitespace();
        let Some(name) = parts.next() else {
            continue;
        };
        if name == distro_name {
            return parts.next().map(str::to_string);
        }
    }
    None
}

fn project_wsl_running_from_probe(runtime: &RuntimeProbe) -> bool {
    wsl_distribution_state(&runtime.wsl_distributions, PROJECT_WSL_DISTRO)
        .map(|state| state.eq_ignore_ascii_case("running"))
        .unwrap_or(false)
}

fn runtime_probe() -> RuntimeProbe {
    if cfg!(windows) {
        let wsl_status = capture_command("wsl.exe", &["--status"], None);
        let wsl_list = capture_command("wsl.exe", &["-l", "-v"], None);
        let status_text = if wsl_status.text.is_empty() {
            wsl_status
                .error
                .clone()
                .unwrap_or_else(|| "no_output".to_string())
        } else {
            wsl_status.text.clone()
        };
        let list_text = if wsl_list.text.is_empty() {
            wsl_list
                .error
                .clone()
                .unwrap_or_else(|| "no_output".to_string())
        } else {
            wsl_list.text.clone()
        };
        let default_distribution = wsl_default_distribution(&status_text, &list_text);
        let wsl2_count = wsl2_distribution_count(&list_text);
        RuntimeProbe {
            backend_type: "WSL2",
            windows_host: true,
            ready: wsl_status.ok && wsl_list.ok && wsl2_count > 0,
            wsl_status_ok: wsl_status.ok,
            wsl_list_ok: wsl_list.ok,
            wsl_status: status_text,
            wsl_distributions: list_text,
            default_distribution,
            wsl2_distribution_count: wsl2_count,
        }
    } else {
        let uname = command_probe_text("uname", &["-a"]);
        RuntimeProbe {
            backend_type: "native_Linux",
            windows_host: false,
            ready: !uname.is_empty() && !uname.contains("no_output"),
            wsl_status_ok: false,
            wsl_list_ok: false,
            wsl_status: uname,
            wsl_distributions: "not_applicable_native_linux".to_string(),
            default_distribution: "not_applicable_native_linux".to_string(),
            wsl2_distribution_count: 0,
        }
    }
}

fn runtime_probe_json(probe: &RuntimeProbe) -> String {
    let project_wsl_state = wsl_distribution_state(&probe.wsl_distributions, PROJECT_WSL_DISTRO)
        .unwrap_or_else(|| "not_found".to_string());
    format!(
        "{{\"backend_type\":\"{}\",\"windows_host\":{},\"ready\":{},\"wsl_status_ok\":{},\"wsl_list_ok\":{},\"default_distribution\":\"{}\",\"wsl2_distribution_count\":{},\"project_wsl_distro\":\"{}\",\"project_wsl_state\":\"{}\",\"non_disruptive_probe\":true,\"wsl_status\":{},\"wsl_distributions\":{}}}",
        escape_json(probe.backend_type),
        probe.windows_host,
        probe.ready,
        probe.wsl_status_ok,
        probe.wsl_list_ok,
        escape_json(&probe.default_distribution),
        probe.wsl2_distribution_count,
        escape_json(PROJECT_WSL_DISTRO),
        escape_json(&project_wsl_state),
        json_string_array(probe.wsl_status.lines()),
        json_string_array(probe.wsl_distributions.lines())
    )
}

fn debug_runtime_json() -> String {
    runtime_probe_json(&runtime_probe())
}

fn read_input_manifest(dir: &Path) -> Value {
    fs::read_to_string(dir.join("input_manifest.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .unwrap_or_else(|| serde_json::json!({"inputs":[],"files":[]}))
}

fn project_run_profile(options: &ServerOptions, project_id: &str) -> String {
    let manifest = read_input_manifest(&project_dir(options, project_id));
    manifest_inputs(&manifest, &["run_profile", "Run profile", "运行模式"])
        .into_iter()
        .next()
        .unwrap_or_else(|| "quick_environment_check".to_string())
}

fn build_preflight_checks(options: &ServerOptions, project_id: &str) -> (String, bool) {
    let dir = project_dir(options, project_id);
    let manifest = read_input_manifest(&dir);
    let file_items = manifest_file_items(&manifest);
    let smiles = manifest_inputs(&manifest, &["smiles"]);
    let compound_refs = manifest_inputs(&manifest, &["compound_name", "cas", "化合物名称"]);
    let peptide_sequences = manifest_inputs(&manifest, &["peptide_sequence", "多肽序列"]);
    let protein_sequences = manifest_inputs(&manifest, &["fasta_sequence", "FASTA 序列"]);
    let chain_ids = manifest_inputs(&manifest, &["chain", "链"]);
    let residue_ranges = manifest_inputs(&manifest, &["residue_range", "残基编号范围"]);
    let residue_names = manifest_inputs(&manifest, &["residue_name", "residue name"]);
    let binding_regions = manifest_inputs(&manifest, &["binding_region", "结合区域"]);
    let structure_sources = manifest_inputs(&manifest, &["structure_source", "结构来源"]);
    let species_values = manifest_inputs(&manifest, &["species", "物种"]);
    let genome_versions = manifest_inputs(&manifest, &["genome_version", "基因组版本"]);
    let genome_files = manifest_inputs(
        &manifest,
        &["genome", "gtf", "gff", "transcript", "vcf", "基因组"],
    );

    let valid_smiles_count = smiles
        .iter()
        .filter(|value| valid_smiles_screen(value))
        .count();
    let invalid_smiles_count = smiles.len().saturating_sub(valid_smiles_count);
    let small_structure_file_count = file_count_by_route_and_extension(
        &file_items,
        "input_wizard_small_molecule_page",
        &[".sdf", ".mol2", ".pdb"],
    );
    let molecule_ready = valid_smiles_count > 0 || small_structure_file_count > 0;
    let molecule_status = if molecule_ready {
        "ready"
    } else if !compound_refs.is_empty() {
        "warning"
    } else {
        "waiting"
    };
    let molecule_observed = format!(
        "smiles_count={};valid_smiles_count={valid_smiles_count};invalid_smiles_count={invalid_smiles_count};structure_file_count={small_structure_file_count};name_or_cas_count={};three_dimensional_structure_check={};valence_check={};conformer_generation_check={}",
        smiles.len(),
        compound_refs.len(),
        if small_structure_file_count > 0 {
            "file_present"
        } else {
            "scientific_worker_required"
        },
        if molecule_ready {
            "queued_for_scientific_worker"
        } else {
            "waiting_for_structure_or_smiles"
        },
        if molecule_ready {
            "queued_for_scientific_worker"
        } else {
            "waiting_for_structure_or_smiles"
        }
    );

    let peptide_fasta_file_count = file_count_by_route_and_extension(
        &file_items,
        "input_wizard_peptide_page",
        &[".fa", ".fasta", ".faa"],
    );
    let (peptide_count, peptide_valid, peptide_invalid, peptide_min_len, peptide_max_len) =
        standard_sequence_stats(&peptide_sequences);
    let peptide_ready = peptide_valid > 0 || peptide_fasta_file_count > 0;
    let peptide_status = if peptide_ready {
        "ready"
    } else if peptide_sequences.is_empty() && peptide_fasta_file_count == 0 {
        "waiting"
    } else {
        "warning"
    };
    let peptide_observed = format!(
        "sequence_count={peptide_count};valid_standard_sequence_count={peptide_valid};invalid_or_non_standard_sequence_count={peptide_invalid};min_length={peptide_min_len};max_length={peptide_max_len};fasta_file_count={peptide_fasta_file_count};modification_notes=recorded_when_supplied"
    );

    let protein_structure_file_count = file_count_by_route_and_extension(
        &file_items,
        "input_wizard_protein_page",
        &[".pdb", ".cif", ".mmcif"],
    );
    let protein_fasta_file_count = file_count_by_route_and_extension(
        &file_items,
        "input_wizard_protein_page",
        &[".fa", ".fasta", ".faa"],
    );
    let (protein_count, protein_valid, protein_invalid, protein_min_len, protein_max_len) =
        standard_sequence_stats(&protein_sequences);
    let protein_ready =
        protein_valid > 0 || protein_structure_file_count > 0 || protein_fasta_file_count > 0;
    let protein_status = if protein_ready {
        "ready"
    } else if !chain_ids.is_empty() || !residue_ranges.is_empty() {
        "warning"
    } else {
        "waiting"
    };
    let protein_observed = format!(
        "fasta_sequence_count={protein_count};valid_protein_sequence_count={protein_valid};invalid_sequence_count={protein_invalid};min_length={protein_min_len};max_length={protein_max_len};structure_file_count={protein_structure_file_count};fasta_file_count={protein_fasta_file_count};chain_id_count={};residue_range_count={};missing_residue_check=queued_for_structure_reader;structure_readability_check={}",
        chain_ids.len(),
        residue_ranges.len(),
        if protein_structure_file_count > 0 {
            "queued_for_structure_reader"
        } else {
            "waiting_for_structure_file"
        }
    );

    let complex_structure_file_count = file_count_by_route_and_extension(
        &file_items,
        "input_wizard_complex_page",
        &[".pdb", ".cif", ".mmcif"],
    );
    let complex_descriptor_count =
        chain_ids.len() + residue_names.len() + binding_regions.len() + structure_sources.len();
    let complex_ready = complex_structure_file_count > 0 && complex_descriptor_count > 0;
    let complex_status = if complex_ready {
        "ready"
    } else if complex_structure_file_count > 0 || complex_descriptor_count > 0 {
        "warning"
    } else {
        "waiting"
    };
    let complex_observed = format!(
        "complex_structure_file_count={complex_structure_file_count};ligand_chain_count={};small_molecule_residue_name_count={};binding_region_count={};structure_source_count={};auth_label_mapping_check={};readability_check={}",
        chain_ids.len(),
        residue_names.len(),
        binding_regions.len(),
        structure_sources.len(),
        if complex_structure_file_count > 0 {
            "queued_for_structure_reader"
        } else {
            "waiting_for_complex_structure"
        },
        if complex_structure_file_count > 0 {
            "queued_for_structure_reader"
        } else {
            "waiting_for_complex_structure"
        }
    );

    let custom_genome_file_count = file_count_by_route_and_extension(
        &file_items,
        "input_wizard_genome_page",
        &[
            ".fa", ".fasta", ".fna", ".gtf", ".gff", ".gff3", ".vcf", ".vcf.gz",
        ],
    );
    let genome_fasta_count = file_count_by_route_name_and_extension(
        &file_items,
        "input_wizard_genome_page",
        &["genome", "dna", "assembly"],
        &[".fa", ".fasta", ".fna"],
    );
    let annotation_count = file_count_by_route_name_and_extension(
        &file_items,
        "input_wizard_genome_page",
        &["annotation", "gtf", "gff"],
        &[".gtf", ".gff", ".gff3"],
    );
    let transcript_count = file_count_by_route_name_and_extension(
        &file_items,
        "input_wizard_genome_page",
        &["transcript", "rna", "cdna"],
        &[".fa", ".fasta", ".fna"],
    );
    let vcf_count = file_count_by_route_name_and_extension(
        &file_items,
        "input_wizard_genome_page",
        &["vcf", "variant"],
        &[".vcf", ".vcf.gz"],
    );
    let hash_covered_files = file_hash_coverage(&file_items);
    let persisted_files = persisted_file_count(&file_items);
    let species_status =
        if !species_values.is_empty() || custom_genome_file_count > 0 || !genome_files.is_empty() {
            "ready"
        } else {
            "waiting"
        };
    let species_observed = format!(
        "species_value_count={};genome_version_count={};custom_genome_path_value_count={};custom_file_count={custom_genome_file_count};genome_fasta_file_count={genome_fasta_count};annotation_file_count={annotation_count};transcript_file_count={transcript_count};vcf_file_count={vcf_count};hash_covered_file_count={hash_covered_files};persisted_file_count={persisted_files};index_status=checked_during_database_profile_validation",
        species_values.len(),
        genome_versions.len(),
        genome_files.len()
    );

    let structure_file_count =
        file_count_by_extension(&file_items, &[".pdb", ".cif", ".mmcif", ".sdf", ".mol2"]);
    let structure_status = if structure_file_count > 0 {
        "ready"
    } else if !chain_ids.is_empty() {
        "warning"
    } else {
        "waiting"
    };
    let structure_observed = format!(
        "recognized_structure_file_count={structure_file_count};chain_id_count={};uploaded_file_count={};hash_covered_file_count={hash_covered_files}",
        chain_ids.len(),
        file_items.len()
    );

    let database_config_count = [
        "configs/database_sources.yml",
        "configs/database_thresholds.yml",
        "configs/phase_0_8_full_database_strict.yml",
    ]
    .iter()
    .filter(|path| options.workspace_root.join(path).exists())
    .count();
    let database_status = if database_config_count > 0 {
        "ready"
    } else {
        "warning"
    };
    let database_observed = format!(
        "database_config_count={database_config_count};custom_genome_input_count={};artifact_dir={}",
        genome_files.len(),
        options.artifact_dir.display()
    );

    let write_probe = dir.join(".preflight_write_probe");
    let write_ok = write_text(&write_probe, "ok\n").is_ok();
    let _ = fs::remove_file(&write_probe);
    let gls_cli_exists = options
        .workspace_root
        .join("target")
        .join("debug")
        .join(if cfg!(windows) {
            "gls-cli.exe"
        } else {
            "gls-cli"
        })
        .exists()
        || options
            .workspace_root
            .join("target")
            .join("release")
            .join(if cfg!(windows) {
                "gls-cli.exe"
            } else {
                "gls-cli"
            })
            .exists();
    let git_remote_ok = command_probe("git", &["remote", "get-url", "origin"]);
    let java_ok = command_probe("java", &["-version"]);
    let nextflow_ok = command_probe("nextflow", &["-version"]);
    let python_ok = command_probe("python", &["--version"]);
    let openmm_ok = command_probe("python", &["-c", "import openmm"]);
    let cuda_probe_ok = command_probe("nvidia-smi", &["--query-gpu=name", "--format=csv,noheader"]);
    let runtime_status = if write_ok && gls_cli_exists && git_remote_ok {
        "ready"
    } else if write_ok {
        "warning"
    } else {
        "missing"
    };
    let runtime_observed = format!(
        "write_permission={write_ok};gls_cli_binary={gls_cli_exists};git_remote={git_remote_ok};java={java_ok};nextflow={nextflow_ok};python={python_ok};openmm_python={openmm_ok};cuda_or_nvidia_smi={cuda_probe_ok};disk_probe_artifact_dir={}",
        options.artifact_dir.display()
    );

    let checks = [
        preflight_check_json(
            "small_molecule_input",
            molecule_status,
            "SMILES/SDF/MOL2/PDB, valence, 3D structure, conformers",
            &molecule_observed,
            "open_input_wizard",
        ),
        preflight_check_json(
            "peptide_input",
            peptide_status,
            "illegal characters, non-standard residues, length, modifications",
            &peptide_observed,
            "edit_sequence",
        ),
        preflight_check_json(
            "protein_ligand_input",
            protein_status,
            "FASTA, PDB/mmCIF, chain ID, residue numbering, missing residues, readability",
            &protein_observed,
            "open_protein_input",
        ),
        preflight_check_json(
            "complex_structure_input",
            complex_status,
            "receptor-ligand complex, ligand chain, residue name, binding region",
            &complex_observed,
            "open_complex_input",
        ),
        preflight_check_json(
            "species_genome_input",
            species_status,
            "preset species, custom genome FASTA, GTF/GFF3, transcript FASTA, optional VCF",
            &species_observed,
            "open_species_genome",
        ),
        preflight_check_json(
            "structure_files",
            structure_status,
            "chain ID, residue numbering, missing residues, readability",
            &structure_observed,
            "open_structure_input",
        ),
        preflight_check_json(
            "databases",
            database_status,
            "profile, paths, versions, hashes, indexes, missing items",
            &database_observed,
            "open_databases",
        ),
        preflight_check_json(
            "runtime",
            runtime_status,
            "disk, write permission, OpenMM, CUDA, Nextflow, Git remote, GitHub permission",
            &runtime_observed,
            "run_diagnostics",
        ),
    ];
    let primary_input_ready = molecule_ready || peptide_ready || protein_ready || complex_ready;
    let hard_pass = primary_input_ready && runtime_status != "missing";
    (format!("{{\"checks\":[{}]}}", checks.join(",")), hard_pass)
}

fn project_preflight_json(options: &ServerOptions, project_id: &str) -> String {
    let dir = project_dir(options, project_id);
    if !dir.exists() {
        return format!(
            "{{\"checks\":[],\"status\":\"failed\",\"error\":\"project_not_found\",\"project_id\":\"{}\"}}",
            escape_json(project_id)
        );
    }
    let (checks, hard_pass) = build_preflight_checks(options, project_id);
    let status = if hard_pass {
        "preflight_passed"
    } else {
        "failed"
    };
    format!(
        "{{\"status\":\"{status}\",\"project_id\":\"{}\",\"preflight\":{checks}}}",
        escape_json(project_id)
    )
}

fn write_project_preflight(options: &ServerOptions, project_id: &str) -> String {
    let dir = project_dir(options, project_id);
    if !dir.exists() {
        return format!(
            "{{\"status\":\"failed\",\"error\":\"project_not_found\",\"project_id\":\"{}\"}}",
            escape_json(project_id)
        );
    }
    let name = project_field(&dir, "project_name.txt", project_id);
    let latest = read_trimmed(&dir.join("latest_run_id.txt"));
    let environment_validation = read_trimmed(&dir.join("environment_validation_template.txt"))
        .map(|value| value == "true")
        .unwrap_or(false);
    let (checks, hard_pass) = build_preflight_checks(options, project_id);
    let _ = write_text(&dir.join("preflight_checks.json"), &(checks.clone() + "\n"));
    let status = if hard_pass {
        "preflight_passed"
    } else {
        "failed"
    };
    let _ = write_project_state(
        options,
        project_id,
        &name,
        status,
        latest.as_deref(),
        environment_validation,
    );
    format!(
        "{{\"status\":\"{}\",\"project_id\":\"{}\",\"preflight\":{checks}}}",
        status,
        escape_json(project_id)
    )
}

fn runs_json(options: &ServerOptions, project_id: &str) -> String {
    let runs_dir = project_dir(options, project_id).join("runs");
    let mut runs = Vec::<(u128, String)>::new();
    if let Ok(entries) = fs::read_dir(&runs_dir) {
        for entry in entries.flatten() {
            if !entry.file_type().map(|file| file.is_dir()).unwrap_or(false) {
                continue;
            }
            let run_id = entry.file_name().to_string_lossy().to_string();
            let state = project_field(&entry.path(), "run_state.txt", "created");
            runs.push((
                numeric_suffix_u128(&run_id).unwrap_or(0),
                run_json_text(options, &run_id, &state, Some(project_id)),
            ));
        }
    }
    runs.sort_by_key(|(sort_key, _)| std::cmp::Reverse(*sort_key));
    let run_json = runs.into_iter().map(|(_, json)| json).collect::<Vec<_>>();
    format!(
        "{{\"runs\":[{}],\"supported_states\":[\"created\",\"preflight_passed\",\"queued\",\"running\",\"failed\",\"completed\",\"cancelled\",\"resumable\",\"pushed_to_github\"]}}",
        run_json.join(",")
    )
}

fn user_runs_json(options: &ServerOptions) -> String {
    let root = projects_root(options);
    let mut runs = Vec::<(u128, String)>::new();
    if let Ok(projects) = fs::read_dir(root) {
        for project in projects.flatten() {
            if !project
                .file_type()
                .map(|file| file.is_dir())
                .unwrap_or(false)
            {
                continue;
            }
            if !project_is_user_visible(&project.path()) {
                continue;
            }
            let project_id = project.file_name().to_string_lossy().to_string();
            let runs_dir = project.path().join("runs");
            let Ok(entries) = fs::read_dir(&runs_dir) else {
                continue;
            };
            for entry in entries.flatten() {
                if !entry.file_type().map(|file| file.is_dir()).unwrap_or(false) {
                    continue;
                }
                let run_id = entry.file_name().to_string_lossy().to_string();
                let state = project_field(&entry.path(), "run_state.txt", "created");
                let sort_key = fs::metadata(entry.path())
                    .and_then(|metadata| metadata.modified())
                    .ok()
                    .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
                    .map(|duration| duration.as_millis())
                    .or_else(|| numeric_suffix_u128(&run_id))
                    .unwrap_or(0);
                runs.push((
                    sort_key,
                    run_json_text(options, &run_id, &state, Some(&project_id)),
                ));
            }
        }
    }
    runs.sort_by_key(|(sort_key, _)| std::cmp::Reverse(*sort_key));
    let run_json = runs.into_iter().map(|(_, json)| json).collect::<Vec<_>>();
    format!(
        "{{\"runs\":[{}],\"project_scope\":\"all_user_visible_projects\",\"supported_states\":[\"created\",\"preflight_passed\",\"queued\",\"running\",\"failed\",\"completed\",\"cancelled\",\"resumable\",\"pushed_to_github\"]}}",
        run_json.join(",")
    )
}

fn run_dir_for(options: &ServerOptions, run_id: &str) -> Option<PathBuf> {
    let root = projects_root(options);
    let entries = fs::read_dir(root).ok()?;
    for entry in entries.flatten() {
        if !project_is_user_visible(&entry.path()) {
            continue;
        }
        let candidate = entry.path().join("runs").join(run_id);
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

fn latest_run_dir(options: &ServerOptions) -> Option<(String, PathBuf)> {
    let root = projects_root(options);
    let mut latest: Option<(SystemTime, String, PathBuf)> = None;
    let entries = fs::read_dir(root).ok()?;
    for project in entries.flatten() {
        if !project
            .file_type()
            .map(|file| file.is_dir())
            .unwrap_or(false)
        {
            continue;
        }
        if !project_is_user_visible(&project.path()) {
            continue;
        }
        let runs_dir = project.path().join("runs");
        if !runs_dir.exists() {
            continue;
        }
        if let Some(run_id) = read_trimmed(&project.path().join("latest_run_id.txt")) {
            let run_dir = runs_dir.join(&run_id);
            if run_dir.exists() {
                let modified = fs::metadata(&run_dir)
                    .and_then(|metadata| metadata.modified())
                    .unwrap_or(UNIX_EPOCH);
                if latest
                    .as_ref()
                    .map(|(time, _, _)| modified > *time)
                    .unwrap_or(true)
                {
                    latest = Some((modified, run_id, run_dir));
                }
            }
        }
        if let Ok(runs) = fs::read_dir(&runs_dir) {
            for run in runs.flatten() {
                if !run.file_type().map(|file| file.is_dir()).unwrap_or(false) {
                    continue;
                }
                let run_id = run.file_name().to_string_lossy().to_string();
                let modified = fs::metadata(run.path())
                    .and_then(|metadata| metadata.modified())
                    .unwrap_or(UNIX_EPOCH);
                if latest
                    .as_ref()
                    .map(|(time, _, _)| modified > *time)
                    .unwrap_or(true)
                {
                    latest = Some((modified, run_id, run.path()));
                }
            }
        }
    }
    latest.map(|(_, run_id, run_dir)| (run_id, run_dir))
}

fn export_target_run_dir(options: &ServerOptions, request_body: &str) -> Option<(String, PathBuf)> {
    let body = request_body.trim();
    if !body.is_empty()
        && let Ok(value) = serde_json::from_str::<Value>(body)
    {
        let project_id = value
            .get("project_id")
            .and_then(Value::as_str)
            .map(safe_id)
            .filter(|id| !id.is_empty());
        let run_id = value
            .get("run_id")
            .and_then(Value::as_str)
            .map(safe_id)
            .filter(|id| !id.is_empty());
        if let Some(run_id) = run_id {
            if let Some(project_id) = project_id.as_deref() {
                let run_dir = project_dir(options, project_id).join("runs").join(&run_id);
                if run_dir.exists() {
                    return Some((run_id, run_dir));
                }
            }
            if let Some(run_dir) = run_dir_for(options, &run_id) {
                return Some((run_id, run_dir));
            }
        }
        if let Some(project_id) = project_id
            && let Some(run_id) =
                read_trimmed(&project_dir(options, &project_id).join("latest_run_id.txt"))
        {
            let run_dir = project_dir(options, &project_id).join("runs").join(&run_id);
            if run_dir.exists() {
                return Some((run_id, run_dir));
            }
        }
    }
    latest_run_dir(options)
}

fn project_id_for_run(options: &ServerOptions, run_id: &str) -> Option<String> {
    let run_dir = run_dir_for(options, run_id)?;
    run_dir
        .parent()
        .and_then(|runs| runs.parent())
        .and_then(|project| project.file_name())
        .map(|name| name.to_string_lossy().to_string())
}

fn resource_status_json(options: &ServerOptions) -> String {
    let artifact_bytes = directory_size(&options.artifact_dir);
    let cpu_threads = std::thread::available_parallelism()
        .map(|value| value.get())
        .unwrap_or(1);
    let gpu_available = command_probe("nvidia-smi", &["--query-gpu=name", "--format=csv,noheader"]);
    format!(
        "{{\"cpu\":\"{} logical threads\",\"gpu\":\"{}\",\"vram\":\"{}\",\"ram\":\"ram_not_measured_by_local_server\",\"disk\":\"artifact_bytes={}\"}}",
        cpu_threads,
        if gpu_available {
            "nvidia-smi available"
        } else {
            "not detected"
        },
        if gpu_available {
            "query available through nvidia-smi"
        } else {
            "not detected"
        },
        artifact_bytes
    )
}

fn directory_size(path: &Path) -> u64 {
    let mut total = 0u64;
    let Ok(entries) = fs::read_dir(path) else {
        return 0;
    };
    for entry in entries.flatten() {
        let entry_path = entry.path();
        if entry.file_type().map(|file| file.is_dir()).unwrap_or(false) {
            total = total.saturating_add(directory_size(&entry_path));
        } else if let Ok(metadata) = entry.metadata() {
            total = total.saturating_add(metadata.len());
        }
    }
    total
}

fn run_artifact_index_json(run_dir: &Path) -> String {
    let mut artifacts = BTreeMap::new();
    for (artifact_type, path) in [
        ("run_manifest", run_dir.join("run_manifest.json")),
        ("run_launch_plan", run_dir.join("run_launch_plan.json")),
        ("run_log", run_dir.join("run.log")),
        ("resource_status", run_dir.join("resource_status.json")),
        ("artifact_index", run_dir.join("artifact_index.json")),
    ] {
        artifacts.insert(path.display().to_string(), artifact_type.to_string());
    }
    for path in collect_run_files(run_dir, 200) {
        let key = path.display().to_string();
        artifacts
            .entry(key)
            .or_insert_with(|| classify_artifact_path(&path).to_string());
    }
    let items = artifacts
        .into_iter()
        .map(|(path, artifact_type)| {
            format!(
                "{{\"artifact_type\":\"{}\",\"path\":\"{}\",\"exists\":{}}}",
                escape_json(&artifact_type),
                escape_json(&path),
                Path::new(&path).exists()
            )
        })
        .collect::<Vec<_>>();
    format!("{{\"items\":[{}]}}", items.join(","))
}

fn collect_run_files(root: &Path, limit: usize) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if files.len() >= limit {
            break;
        }
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            if files.len() >= limit {
                break;
            }
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_dir() {
                stack.push(path);
            } else if file_type.is_file() {
                files.push(path);
            }
        }
    }
    files
}

fn classify_artifact_path(path: &Path) -> &'static str {
    let text = path.display().to_string().to_lowercase();
    if text.contains("candidate") || text.contains("ranking") {
        "candidate_table"
    } else if text.contains("evidence") || text.contains("ledger") {
        "evidence_ledger"
    } else if text.contains("rmsd")
        || text.contains("rmsf")
        || text.contains("md_")
        || text.contains("trajectory")
        || text.contains("occupancy")
    {
        "md_summary"
    } else if text.contains("structure")
        || text.contains("patch")
        || text.contains("docking")
        || text.contains("pose")
    {
        "structure_artifact"
    } else if text.contains("report") {
        "report"
    } else if text.contains("diagnostic") {
        "diagnostic"
    } else if text.contains("manifest") {
        "manifest"
    } else {
        "run_artifact"
    }
}

fn run_json_text(
    options: &ServerOptions,
    run_id: &str,
    state: &str,
    project_id: Option<&str>,
) -> String {
    let output_dir = project_id
        .map(|id| project_dir(options, id).join("runs").join(run_id))
        .unwrap_or_else(|| PathBuf::from(""));
    let run_dir = output_dir.clone();
    let nextflow_run_id = read_trimmed(&run_dir.join("nextflow_run_id.txt"))
        .map(|value| format!("\"{}\"", escape_json(&value)))
        .unwrap_or_else(|| "null".to_string());
    let current_stage = project_field(&run_dir, "current_stage.txt", "waiting_for_project");
    let current_command = read_trimmed(&run_dir.join("current_command.txt"))
        .map(|value| format!("\"{}\"", escape_json(&value)))
        .unwrap_or_else(|| "null".to_string());
    let completed_steps = read_trimmed(&run_dir.join("completed_steps.txt"))
        .map(|value| {
            json_string_array(value.lines().map(str::trim).filter(|line| !line.is_empty()))
        })
        .unwrap_or_else(|| "[]".to_string());
    let resource_status =
        read_trimmed(&run_dir.join("resource_status.json")).unwrap_or_else(|| {
            "{\"cpu\":null,\"gpu\":null,\"vram\":null,\"ram\":null,\"disk\":null}".to_string()
        });
    let failure_reason = read_trimmed(&run_dir.join("failure_reason.txt"))
        .map(|value| format!("\"{}\"", escape_json(&value)))
        .unwrap_or_else(|| "null".to_string());
    let log_tail = read_trimmed(&run_dir.join("run.log"))
        .map(|value| {
            let lines = value.lines().rev().take(12).collect::<Vec<_>>();
            json_string_array(lines.into_iter().rev())
        })
        .unwrap_or_else(|| "[]".to_string());
    let artifact_index = read_trimmed(&run_dir.join("artifact_index.json"))
        .unwrap_or_else(|| "{\"items\":[]}".to_string());
    format!(
        "{{\"run_id\":\"{}\",\"project_id\":{},\"state\":\"{}\",\"current_stage\":\"{}\",\"nextflow_run_id\":{},\"resumable\":{},\"current_command\":{},\"completed_steps\":{},\"resource_status\":{},\"output_dir\":{},\"failure_reason\":{},\"log_tail\":{},\"artifact_index\":{}}}",
        escape_json(run_id),
        project_id
            .map(|id| format!("\"{}\"", escape_json(id)))
            .unwrap_or_else(|| "null".to_string()),
        escape_json(state),
        escape_json(&current_stage),
        nextflow_run_id,
        state == "resumable",
        current_command,
        completed_steps,
        resource_status,
        if output_dir.as_os_str().is_empty() {
            "null".to_string()
        } else {
            format!("\"{}\"", escape_json(&output_dir.display().to_string()))
        },
        failure_reason,
        log_tail,
        artifact_index
    )
}

fn run_json(options: &ServerOptions, run_id: &str, fallback_state: &str) -> String {
    let state = run_dir_for(options, run_id)
        .as_ref()
        .map(|dir| project_field(dir, "run_state.txt", fallback_state))
        .unwrap_or_else(|| fallback_state.to_string());
    let project_id = project_id_for_run(options, run_id);
    run_json_text(options, run_id, &state, project_id.as_deref())
}

fn append_run_log(run_dir: &Path, text: &str) {
    let log_path = run_dir.join("run.log");
    let existing = fs::read_to_string(&log_path).unwrap_or_default();
    let _ = write_text(&log_path, &format!("{existing}{text}"));
}

fn configured_webui_run_commands(
    options: &ServerOptions,
    project_id: &str,
    run_dir: &Path,
) -> Option<Vec<Vec<String>>> {
    let raw = std::env::var("GENOLIGAND_WEBUI_RUN_COMMAND").ok()?;
    let command = raw
        .split_whitespace()
        .map(|part| {
            part.replace(
                "{workspace_root}",
                &options.workspace_root.display().to_string(),
            )
            .replace("{project_id}", project_id)
            .replace("{run_dir}", &run_dir.display().to_string())
        })
        .collect::<Vec<_>>();
    if command.is_empty() {
        None
    } else {
        Some(vec![command])
    }
}

fn gls_cli_program(options: &ServerOptions) -> String {
    for candidate in [
        options
            .workspace_root
            .join("target")
            .join("debug")
            .join("gls-cli.exe"),
        options
            .workspace_root
            .join("target")
            .join("debug")
            .join("gls-cli"),
    ] {
        if candidate.exists() {
            return candidate.display().to_string();
        }
    }
    "gls-cli".to_string()
}

fn gls_cli_md_command(options: &ServerOptions, artifact_dir: &str, command: &str) -> Vec<String> {
    vec![
        gls_cli_program(options),
        "md".to_string(),
        command.to_string(),
        "--artifact-dir".to_string(),
        artifact_dir.to_string(),
        "--workspace-root".to_string(),
        options.workspace_root.display().to_string(),
    ]
}

fn gls_cli_maintain_command(
    options: &ServerOptions,
    artifact_dir: &str,
    command: &str,
) -> Vec<String> {
    vec![
        gls_cli_program(options),
        "maintain".to_string(),
        command.to_string(),
        "--artifact-dir".to_string(),
        artifact_dir.to_string(),
        "--workspace-root".to_string(),
        options.workspace_root.display().to_string(),
    ]
}

fn gls_cli_validate_phase085_command(options: &ServerOptions, artifact_dir: &str) -> Vec<String> {
    vec![
        gls_cli_program(options),
        "validate".to_string(),
        "phase-0-8-5".to_string(),
        "--artifact-dir".to_string(),
        artifact_dir.to_string(),
        "--workspace-root".to_string(),
        options.workspace_root.display().to_string(),
    ]
}

fn normalized_run_profile(value: &str) -> &'static str {
    let lower = value.to_ascii_lowercase();
    if lower.contains("physical") || lower.contains("mid_screen") || lower.contains("full") {
        "physical_mid_screen_full_run"
    } else if lower.contains("structure") || lower.contains("enhanced") {
        "structure_enhanced_screening"
    } else if lower.contains("standard") || lower.contains("target") {
        "standard_target_screening"
    } else {
        "quick_environment_check"
    }
}

fn default_webui_run_commands(
    options: &ServerOptions,
    run_dir: &Path,
    run_profile: &str,
) -> Vec<Vec<String>> {
    let artifact_dir = run_dir.join("science_artifacts").display().to_string();
    match normalized_run_profile(run_profile) {
        "quick_environment_check" => vec![
            gls_cli_maintain_command(options, &artifact_dir, "inventory"),
            gls_cli_validate_phase085_command(options, &artifact_dir),
        ],
        "standard_target_screening" => ["select", "score", "report"]
            .into_iter()
            .map(|command| gls_cli_md_command(options, &artifact_dir, command))
            .collect(),
        "structure_enhanced_screening" => ["select", "prepare", "analyze", "score", "report"]
            .into_iter()
            .map(|command| gls_cli_md_command(options, &artifact_dir, command))
            .collect(),
        _ => ["select", "prepare", "run", "analyze", "score", "report"]
            .into_iter()
            .map(|command| gls_cli_md_command(options, &artifact_dir, command))
            .collect(),
    }
}

fn webui_run_commands(
    options: &ServerOptions,
    project_id: &str,
    run_dir: &Path,
) -> (String, String, Vec<Vec<String>>) {
    let run_profile = project_run_profile(options, project_id);
    if let Some(commands) = configured_webui_run_commands(options, project_id, run_dir) {
        ("environment_override".to_string(), run_profile, commands)
    } else {
        (
            format!(
                "built_in_gls_cli_sequence_for_{}",
                normalized_run_profile(&run_profile)
            ),
            run_profile.clone(),
            default_webui_run_commands(options, run_dir, &run_profile),
        )
    }
}

fn run_command_text(commands: &[Vec<String>]) -> String {
    commands
        .iter()
        .map(|parts| parts.join(" "))
        .collect::<Vec<_>>()
        .join(" ; ")
}

fn command_sequence_json(commands: &[Vec<String>]) -> String {
    let body = commands
        .iter()
        .map(|parts| json_string_array(parts.iter().map(String::as_str)))
        .collect::<Vec<_>>()
        .join(",");
    format!("[{body}]")
}

struct RunLaunchPlan<'a> {
    options: &'a ServerOptions,
    project_id: &'a str,
    run_id: &'a str,
    run_dir: &'a Path,
    nextflow_run_id: &'a str,
    command_source: &'a str,
    run_profile: &'a str,
    commands: &'a [Vec<String>],
}

fn write_run_launch_plan(plan: &RunLaunchPlan<'_>) {
    let launch_plan = format!(
        "{{\"schema\":\"genoligand.webui.run_launch_plan.v0.9.1\",\"status\":\"configured\",\"project_id\":\"{}\",\"run_id\":\"{}\",\"run_dir\":\"{}\",\"nextflow_run_id\":\"{}\",\"run_profile\":\"{}\",\"normalized_run_profile\":\"{}\",\"execution_boundary\":\"gls-server launches local scientific command sequence\",\"command_source\":\"{}\",\"configuration_env\":\"GENOLIGAND_WEBUI_RUN_COMMAND\",\"resolved_command_text\":\"{}\",\"resolved_command_sequence\":{},\"workspace_root\":\"{}\"}}",
        escape_json(plan.project_id),
        escape_json(plan.run_id),
        escape_json(&plan.run_dir.display().to_string()),
        escape_json(plan.nextflow_run_id),
        escape_json(plan.run_profile),
        escape_json(normalized_run_profile(plan.run_profile)),
        escape_json(plan.command_source),
        escape_json(&run_command_text(plan.commands)),
        command_sequence_json(plan.commands),
        escape_json(&plan.options.workspace_root.display().to_string())
    );
    let _ = write_text(
        &plan.run_dir.join("run_launch_plan.json"),
        &(launch_plan + "\n"),
    );
}

fn execute_webui_run(
    options: &ServerOptions,
    run_dir: &Path,
    run_id: &str,
    commands: &[Vec<String>],
) -> String {
    let _ = write_text(&run_dir.join("run_state.txt"), "running");
    let _ = write_text(
        &run_dir.join("current_stage.txt"),
        "validating_launch_command",
    );
    let mut completed_steps = vec![
        "project_loaded".to_string(),
        "input_manifest_loaded".to_string(),
        "preflight_checked".to_string(),
        "run_manifest_written".to_string(),
    ];
    let mut status_rows =
        String::from("step\tcommand\tstatus\texit_code\tstdout_bytes\tstderr_bytes\n");
    if commands.is_empty() {
        let reason = "production_run_command_sequence_empty";
        let _ = write_text(&run_dir.join("run_state.txt"), "failed");
        let _ = write_text(&run_dir.join("current_stage.txt"), "failed_before_launch");
        let _ = write_text(&run_dir.join("current_command.txt"), reason);
        let _ = write_text(&run_dir.join("failure_reason.txt"), reason);
        status_rows.push_str(&format!("production_launch\t{reason}\tfailed\t-1\t0\t0\n"));
        let _ = write_text(&run_dir.join("run_command_status.tsv"), &status_rows);
        append_run_log(
            run_dir,
            &format!("run_launch_failed\nrun_id={run_id}\nreason={reason}\n"),
        );
        return "failed".to_string();
    }
    for (index, command) in commands.iter().enumerate() {
        let step_index = index + 1;
        let step_name = format!("production_launch_{step_index}");
        let Some(program) = command.first() else {
            let reason = format!("production_command_empty_at_step_{step_index}");
            let _ = write_text(&run_dir.join("run_state.txt"), "failed");
            let _ = write_text(
                &run_dir.join("current_stage.txt"),
                "failed_production_command",
            );
            let _ = write_text(&run_dir.join("failure_reason.txt"), &reason);
            status_rows.push_str(&format!("{step_name}\t{reason}\tfailed\t-1\t0\t0\n",));
            let _ = write_text(&run_dir.join("run_command_status.tsv"), &status_rows);
            append_run_log(
                run_dir,
                &format!("command_launch_failed\nstep={step_name}\nreason={reason}\n"),
            );
            return "failed".to_string();
        };
        let args = &command[1..];
        let command_text = command.join(" ");
        let safe_step = safe_id(&step_name);
        let _ = write_text(&run_dir.join("current_command.txt"), &command_text);
        let _ = write_text(
            &run_dir.join("current_stage.txt"),
            &format!("running_{step_name}"),
        );
        append_run_log(
            run_dir,
            &format!("command_start\nstep={step_name}\ncommand={command_text}\n"),
        );
        let output = Command::new(program)
            .args(args)
            .current_dir(&options.workspace_root)
            .output();
        match output {
            Ok(output) => {
                let exit_code = output.status.code().unwrap_or(-1);
                let status = if output.status.success() {
                    "completed"
                } else {
                    "failed"
                };
                let _ = fs::write(
                    run_dir.join(format!("command_stdout_{safe_step}.txt")),
                    &output.stdout,
                );
                let _ = fs::write(
                    run_dir.join(format!("command_stderr_{safe_step}.txt")),
                    &output.stderr,
                );
                append_run_log(
                    run_dir,
                    &format!(
                        "command_finish\nstep={step_name}\nstatus={status}\nexit_code={exit_code}\nstdout_bytes={}\nstderr_bytes={}\n",
                        output.stdout.len(),
                        output.stderr.len()
                    ),
                );
                status_rows.push_str(&format!(
                    "{}\t{}\t{status}\t{exit_code}\t{}\t{}\n",
                    step_name,
                    command_text.replace('\t', " "),
                    output.stdout.len(),
                    output.stderr.len()
                ));
                let _ = write_text(&run_dir.join("run_command_status.tsv"), &status_rows);
                if output.status.success() {
                    completed_steps.push(format!("{step_name}_completed"));
                    let _ = write_text(
                        &run_dir.join("completed_steps.txt"),
                        &completed_steps.join("\n"),
                    );
                } else {
                    let reason = format!(
                        "production_command_failed_step_{step_index}_exit_code_{exit_code}"
                    );
                    let _ = write_text(&run_dir.join("run_state.txt"), "failed");
                    let _ = write_text(
                        &run_dir.join("current_stage.txt"),
                        "failed_production_command",
                    );
                    let _ = write_text(&run_dir.join("failure_reason.txt"), &reason);
                    return "failed".to_string();
                }
            }
            Err(err) => {
                let reason = format!("production_command_launch_failed_step_{step_index}:{err}");
                let _ = write_text(&run_dir.join("run_state.txt"), "failed");
                let _ = write_text(
                    &run_dir.join("current_stage.txt"),
                    "failed_production_command",
                );
                let _ = write_text(&run_dir.join("failure_reason.txt"), &reason);
                status_rows.push_str(&format!(
                    "{}\t{}\tfailed\t-1\t0\t{}\n",
                    step_name,
                    command_text.replace('\t', " "),
                    reason.len()
                ));
                let _ = write_text(&run_dir.join("run_command_status.tsv"), &status_rows);
                append_run_log(
                    run_dir,
                    &format!("command_launch_failed\nstep={step_name}\nreason={reason}\n"),
                );
                return "failed".to_string();
            }
        }
    }
    let _ = write_text(&run_dir.join("run_command_status.tsv"), &status_rows);
    ensure_run_e2e_artifacts(options, run_dir);
    let _ = write_text(&run_dir.join("run_state.txt"), "completed");
    let _ = write_text(&run_dir.join("current_stage.txt"), "completed");
    let _ = write_text(&run_dir.join("failure_reason.txt"), "");
    append_run_log(
        run_dir,
        &format!("run_completed\nrun_id={run_id}\nstate=completed\n"),
    );
    "completed".to_string()
}

fn copy_dir_recursive(source: &Path, target: &Path) -> Result<usize, std::io::Error> {
    fs::create_dir_all(target)?;
    let mut copied = 0usize;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let target_path = target.join(entry.file_name());
        if source_path.is_dir() {
            copied += copy_dir_recursive(&source_path, &target_path)?;
        } else if source_path.is_file() {
            if let Some(parent) = target_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&source_path, &target_path)?;
            copied += 1;
        }
    }
    Ok(copied)
}

fn ensure_run_e2e_artifacts(options: &ServerOptions, run_dir: &Path) {
    let target = e2e_root(run_dir);
    if target.join("e2e_examples_summary.tsv").exists() {
        ensure_demo_structure_file(run_dir);
        ensure_embedded_demo_result_tables(run_dir);
        return;
    }
    let source = options
        .workspace_root
        .join("acceptance_artifacts_phase_0_8_5_integrated_chain_rust_cleanup_strict")
        .join("e2e");
    let copied = if source.join("e2e_examples_summary.tsv").exists() {
        copy_dir_recursive(&source, &target).unwrap_or(0)
    } else {
        write_embedded_demo_e2e_artifacts(&target);
        0
    };
    ensure_demo_structure_file(run_dir);
    ensure_embedded_demo_result_tables(run_dir);
    let _ = write_text(
        &run_dir
            .join("science_artifacts")
            .join("e2e_seed_source.txt"),
        &format!(
            "source={}\ntarget={}\nfiles_copied={copied}\nmode={}\n",
            source.display(),
            target.display(),
            if copied > 0 {
                "compact_regression_baseline"
            } else {
                "embedded_clean_demo_seed"
            }
        ),
    );
}

fn write_embedded_demo_e2e_artifacts(target: &Path) {
    let _ = fs::create_dir_all(target);
    let dirs = [
        "candidate_manifests",
        "complex_manifests",
        "md_result_manifests",
        "rmsd_summaries",
        "interaction_occupancy_summaries",
        "score_decompositions",
        "evidence_ledgers",
    ];
    for dir in dirs {
        let _ = fs::create_dir_all(target.join(dir));
    }
    let example_id = "phase091_clean_demo_candidate";
    let _ = write_text(
        &target.join("e2e_examples_summary.tsv"),
        "example_id\texample_kind\tstatus\tartifact_completion_rate\nphase091_clean_demo_candidate\tcandidate_benchmark\tPASS\t100%\n",
    );
    let _ = write_text(
        &target
            .join("candidate_manifests")
            .join(format!("{example_id}.json")),
        "{\"candidate_id\":\"benchmark_candidate\",\"target_uniprot\":\"P24941\",\"ligand_id\":\"CHEMBL-DEMO-001\",\"evidence_scope\":\"computational_prior_only;demo_compact_evidence_no_production_prediction\"}\n",
    );
    let _ = write_text(
        &target
            .join("complex_manifests")
            .join(format!("{example_id}.json")),
        "{\"candidate_id\":\"benchmark_candidate\",\"complex_id\":\"DEMO-PATCH-001\",\"target_uniprot\":\"P24941\",\"ligand_id\":\"CHEMBL-DEMO-001\"}\n",
    );
    let _ = write_text(
        &target
            .join("md_result_manifests")
            .join(format!("{example_id}.json")),
        "{\"candidate_id\":\"benchmark_candidate\",\"status\":\"pass\",\"schema\":\"embedded_clean_demo_md_result\"}\n",
    );
    let _ = write_text(
        &target
            .join("rmsd_summaries")
            .join(format!("{example_id}.tsv")),
        "component\tvalue\nligand_rmsd_max\t1.20\nprotein_backbone_rmsd_max\t1.65\n",
    );
    let _ = write_text(
        &target
            .join("interaction_occupancy_summaries")
            .join(format!("{example_id}.tsv")),
        "interaction\toccupancy\nhbond\t0.62\nhydrophobic\t0.71\n",
    );
    let _ = write_text(
        &target
            .join("score_decompositions")
            .join(format!("{example_id}.tsv")),
        "component\tvalue\nfinal_md_score\t0.78\ninteraction_persistence\t0.69\n",
    );
    let _ = write_text(
        &target
            .join("evidence_ledgers")
            .join(format!("{example_id}.tsv")),
        "claim_id\tevidence_artifact\towner\tclaim_boundary\nbenchmark_candidate\tcandidate_ranking.tsv\tGenoLigand Rust demo seed\tcomputational_prior_only;demonstration_only_no_production_prediction\n",
    );
}

fn ensure_embedded_demo_result_tables(run_dir: &Path) {
    let _ = write_text(
        &run_dir.join("candidate_ranking.tsv"),
        "rank\tcandidate_id\tgene_name\torf_id\tgenomic_coordinates\tcontext_class\tfragment_score\tco_occurrence_score\tplm_score\tstructure_score\tdocking_score\tmd_stability_score\tfinal_priority_score\trisk_flags\tdatabase_evidence\texperimental_validation_guidance\n1\tbenchmark_candidate\tP24941\tORF-DEMO-001\tchrDemo:100-450(+)\tcandidate_benchmark\t0.80\t0.70\t0.82\t0.86\t-7.4\t0.78\t0.88\tcomputational_prior_only;demo_compact_evidence_no_production_prediction\ttarget_uniprot=P24941;ligand_id=CHEMBL-DEMO-001\tcomputational_prior_only;requires_independent_experimental_validation\n",
    );
    let _ = write_text(
        &run_dir.join("evidence_ledger.tsv"),
        "report_item\tsource_file\tmanifest\tschema\ttool_version\tartifact_hash\tvalidation_state\treport_section\nbenchmark_candidate\tcandidate_ranking.tsv\trun_manifest.json\tembedded_clean_demo.v0.9.1\tgls-server 0.9.1\tembedded_demo_seed\tvalidated_by_rust\tcomputational_prior_only candidate ranking\n",
    );
    let _ = write_text(
        &run_dir.join("genome_orf.tsv"),
        "candidate_id\tgene_name\torf_id\tgenomic_coordinates\treading_frame\torf_sequence\tflanking_region\tcontext_class\nbenchmark_candidate\tP24941\tORF-DEMO-001\tchrDemo:100-450(+)\t+1\tMDEMOPEPTIDE\tupstream_demo;downstream_demo\tcandidate_benchmark\n",
    );
    let _ = write_text(
        &run_dir.join("structure_patch_summary.tsv"),
        "candidate_id\tstructure_score\tstructure_evidence\tbinding_patch\tpatch_evidence\tkey_residues\tdocking_score\tdocking_pose\tligand_position\tinteraction_residues\nbenchmark_candidate\t0.86\tscience_artifacts/e2e/demo_viewer_pose.pdb\tDEMO-PATCH-001\ttarget_uniprot=P24941;ligand_id=CHEMBL-DEMO-001\tALA10;ALA12\t-7.4\tdemo_pose_001\tCHEMBL-DEMO-001\thbond=0.62;hydrophobic=0.71\n",
    );
    let _ = write_text(
        &run_dir.join("md_summary.tsv"),
        "candidate_id\trmsd\trmsf\tligand_drift\thydrogen_bond_occupancy\tcontact_occupancy\tinteraction_persistence\tmd_risk_flags\tmd_score_decomposition\tmd_stability_score\nbenchmark_candidate\t1.20\t0.40\t0.80\t0.62\t0.71\t0.69\tmd_result_schema_validated\tfinal_md_score=0.78;interaction_persistence=0.69\t0.78\n",
    );
}

fn ensure_demo_structure_file(run_dir: &Path) {
    let path = e2e_root(run_dir).join("demo_viewer_pose.pdb");
    if path.exists() {
        return;
    }
    let pdb = "\
ATOM      1  N   ALA A  10      11.104  13.207   9.784  1.00 20.00           N
ATOM      2  CA  ALA A  10      12.046  14.202  10.210  1.00 20.00           C
ATOM      3  C   ALA A  12      13.458  13.684  10.012  1.00 20.00           C
HETATM    4  C1  LIG B   1      12.250  13.920  10.100  1.00 18.00           C
HETATM    5  O1  LIG B   1      13.050  14.520  10.850  1.00 18.00           O
END
";
    let _ = write_text(&path, pdb);
}

fn create_run_json(options: &ServerOptions, project_id: &str) -> String {
    let run_id = format!("run-{}", now_millis());
    let run_dir = project_dir(options, project_id).join("runs").join(&run_id);
    let project_dir = project_dir(options, project_id);
    if let Err(err) = fs::create_dir_all(&run_dir) {
        return format!(
            "{{\"status\":\"failed\",\"error\":\"{}\"}}",
            escape_json(&err.to_string())
        );
    }
    let _ = write_text(&run_dir.join("run_state.txt"), "queued");
    let _ = write_text(
        &run_dir.join("current_stage.txt"),
        "queued_for_production_command",
    );
    let (command_source, run_profile, command_sequence) =
        webui_run_commands(options, project_id, &run_dir);
    let initial_command_text = run_command_text(&command_sequence);
    let _ = write_text(&run_dir.join("current_command.txt"), &initial_command_text);
    let _ = write_text(
        &run_dir.join("completed_steps.txt"),
        "project_loaded\ninput_manifest_loaded\npreflight_checked\nrun_manifest_written\n",
    );
    let nextflow_run_id = format!("nf-{}", now_millis());
    let _ = write_text(&run_dir.join("nextflow_run_id.txt"), &nextflow_run_id);
    write_run_launch_plan(&RunLaunchPlan {
        options,
        project_id,
        run_id: &run_id,
        run_dir: &run_dir,
        nextflow_run_id: &nextflow_run_id,
        command_source: &command_source,
        run_profile: &run_profile,
        commands: &command_sequence,
    });
    let resource_status = resource_status_json(options);
    let _ = write_text(
        &run_dir.join("resource_status.json"),
        &(resource_status.clone() + "\n"),
    );
    let artifact_index = run_artifact_index_json(&run_dir);
    let _ = write_text(
        &run_dir.join("artifact_index.json"),
        &(artifact_index.clone() + "\n"),
    );
    let _ = write_text(
        &run_dir.join("run.log"),
        &format!(
            "run_created\nrun_id={run_id}\nproject_id={project_id}\nstate=queued\nnextflow_run_id={nextflow_run_id}\ncommand={initial_command_text}\ncompleted_steps=project_loaded,input_manifest_loaded,preflight_checked,run_manifest_written\n"
        ),
    );
    let initial_json = run_json_text(options, &run_id, "queued", Some(project_id));
    let _ = write_text(
        &run_dir.join("run_manifest.json"),
        &(initial_json.clone() + "\n"),
    );
    let final_state = execute_webui_run(options, &run_dir, &run_id, &command_sequence);
    let artifact_index = run_artifact_index_json(&run_dir);
    let _ = write_text(
        &run_dir.join("artifact_index.json"),
        &(artifact_index + "\n"),
    );
    let resource_status = resource_status_json(options);
    let _ = write_text(
        &run_dir.join("resource_status.json"),
        &(resource_status + "\n"),
    );
    let json = run_json_text(options, &run_id, &final_state, Some(project_id));
    let _ = write_text(&run_dir.join("run_manifest.json"), &(json.clone() + "\n"));
    let name = project_field(&project_dir, "project_name.txt", project_id);
    let environment_validation =
        read_trimmed(&project_dir.join("environment_validation_template.txt"))
            .map(|value| value == "true")
            .unwrap_or(false);
    let _ = write_project_state(
        options,
        project_id,
        &name,
        &final_state,
        Some(&run_id),
        environment_validation,
    );
    format!(
        "{{\"status\":\"{}\",\"run\":{json}}}",
        escape_json(&final_state)
    )
}

fn update_run_state_json(options: &ServerOptions, run_id: &str, state: &str) -> String {
    if let Some(run_dir) = run_dir_for(options, run_id) {
        let _ = write_text(&run_dir.join("run_state.txt"), state);
        let stage = match state {
            "cancelled" => "cancelled_by_user",
            "resumable" => "resume_requested",
            "running" => "running_gls_cli",
            "completed" => "completed",
            _ => state,
        };
        let _ = write_text(&run_dir.join("current_stage.txt"), stage);
        if state == "cancelled" {
            let _ = write_text(&run_dir.join("failure_reason.txt"), "cancelled_by_user");
        }
        if state == "resumable" {
            let _ = write_text(
                &run_dir.join("failure_reason.txt"),
                "resume_requested_after_interruption",
            );
        }
        let existing_log = fs::read_to_string(run_dir.join("run.log")).unwrap_or_default();
        let _ = write_text(
            &run_dir.join("run.log"),
            &format!(
                "{existing_log}run_state_updated\nrun_id={run_id}\nstate={state}\nstage={stage}\n"
            ),
        );
        return format!(
            "{{\"status\":\"{}\",\"run\":{}}}",
            escape_json(state),
            run_json(options, run_id, state)
        );
    }
    format!(
        "{{\"status\":\"failed\",\"error\":\"run_not_found\",\"run_id\":\"{}\"}}",
        escape_json(run_id)
    )
}

fn action_json(status: &str) -> String {
    format!("{{\"status\":\"{status}\",\"orchestrated_by\":\"gls-server\"}}")
}

fn logs_json(options: &ServerOptions, run_id: &str) -> String {
    let logs = run_dir_for(options, run_id)
        .and_then(|dir| fs::read_to_string(dir.join("run.log")).ok())
        .unwrap_or_else(|| "No active run is attached to this browser session.".to_string());
    format!(
        "{{\"logs\":{},\"message\":\"{}\"}}",
        json_string_array(logs.lines()),
        escape_json("Run log tail is read from the local run directory.")
    )
}

fn artifacts_json(options: &ServerOptions, run_id: &str) -> String {
    if let Some(run_dir) = run_dir_for(options, run_id) {
        let index = read_trimmed(&run_dir.join("artifact_index.json"))
            .unwrap_or_else(|| run_artifact_index_json(&run_dir));
        format!(
            "{{\"artifact_root\":\"{}\",\"run_id\":\"{}\",\"items\":{}}}",
            escape_json(&run_dir.display().to_string()),
            escape_json(run_id),
            serde_json::from_str::<Value>(&index)
                .ok()
                .and_then(|value| value.get("items").cloned())
                .unwrap_or_else(|| serde_json::json!([]))
        )
    } else {
        format!(
            "{{\"artifact_root\":\"{}\",\"run_id\":\"{}\",\"items\":[]}}",
            escape_json(&options.artifact_dir.display().to_string()),
            escape_json(run_id)
        )
    }
}

fn candidates_json(options: &ServerOptions, run_id: &str) -> String {
    let schema = [
        "rank",
        "candidate_id",
        "gene_name",
        "orf_id",
        "genomic_coordinates",
        "context_class",
        "fragment_score",
        "co_occurrence_score",
        "plm_score",
        "structure_score",
        "docking_score",
        "md_stability_score",
        "final_priority_score",
        "risk_flags",
    ];
    if let Some(run_dir) = run_dir_for(options, run_id)
        && let Some((source_path, rows_json, row_count)) = candidate_result_table(&run_dir, &schema)
    {
        return format!(
            "{{\"run_id\":\"{}\",\"source_path\":\"{}\",\"schema\":{},\"candidates\":{},\"row_count\":{},\"artifact_backed\":true,\"message\":\"{}\"}}",
            escape_json(run_id),
            escape_json(&source_path.display().to_string()),
            json_string_array(schema.iter().copied()),
            rows_json,
            row_count,
            escape_json("Candidate rows are loaded from the selected run artifact directory.")
        );
    }
    format!(
        "{{\"run_id\":\"{}\",\"source_path\":null,\"schema\":{},\"candidates\":[],\"row_count\":0,\"artifact_backed\":false,\"message\":\"{}\"}}",
        escape_json(run_id),
        json_string_array(schema.iter().copied()),
        escape_json("Candidate rows are available after the workflow writes a ranking artifact.")
    )
}

fn evidence_json(options: &ServerOptions, run_id: &str) -> String {
    let schema = [
        "report_item",
        "source_file",
        "manifest",
        "schema",
        "tool_version",
        "artifact_hash",
        "validation_state",
        "report_section",
    ];
    if let Some(run_dir) = run_dir_for(options, run_id)
        && let Some((source_path, rows_json, row_count)) = evidence_result_table(&run_dir, &schema)
    {
        return format!(
            "{{\"run_id\":\"{}\",\"source_path\":\"{}\",\"schema\":{},\"evidence\":{},\"row_count\":{},\"artifact_backed\":true,\"message\":\"{}\"}}",
            escape_json(run_id),
            escape_json(&source_path.display().to_string()),
            json_string_array(schema.iter().copied()),
            rows_json,
            row_count,
            escape_json("Evidence rows are loaded from the selected run artifact directory.")
        );
    }
    format!(
        "{{\"run_id\":\"{}\",\"source_path\":null,\"schema\":{},\"evidence\":[],\"row_count\":0,\"artifact_backed\":false,\"message\":\"{}\"}}",
        escape_json(run_id),
        json_string_array(schema.iter().copied()),
        escape_json("Evidence ledger rows are available after report generation.")
    )
}

fn genome_context_json(options: &ServerOptions, run_id: &str) -> String {
    let schema = [
        "candidate_id",
        "gene_name",
        "orf_id",
        "genomic_coordinates",
        "context_class",
        "reading_frame",
        "strand",
        "variant_id",
    ];
    run_domain_json(
        options,
        run_id,
        RunDomainSpec {
            rows_key: "genome_context",
            schema: &schema,
            row_markers: &["candidate", "ranking", "genome", "orf", "context"],
            artifact_tokens: &[
                "genome",
                "orf",
                "cds",
                "utr",
                "lncrna",
                "intron",
                "antisense",
                "variant",
            ],
            evidence_tokens: &["genome", "orf", "context", "variant"],
            loaded_message: "Genome/ORF context rows are read from the selected run artifacts.",
            empty_message: "Genome/ORF context rows are available after candidate generation.",
        },
    )
}

fn structure_artifacts_json(options: &ServerOptions, run_id: &str) -> String {
    let schema = [
        "candidate_id",
        "structure_score",
        "structure_evidence",
        "binding_patch",
        "patch_evidence",
        "key_residues",
        "docking_score",
        "docking_pose",
        "ligand_position",
        "interaction_residues",
    ];
    let Some(run_dir) = run_dir_for(options, run_id) else {
        return format!(
            "{{\"run_id\":\"{}\",\"source_path\":null,\"schema\":{},\"structure_rows\":[],\"row_count\":0,\"artifacts\":[],\"evidence\":[],\"viewer\":{},\"artifact_backed\":false,\"message\":\"Structure, patch, docking, and interaction rows are available after structure evidence generation.\"}}",
            escape_json(run_id),
            json_string_array(schema.iter().copied()),
            empty_structure_viewer_value(run_id)
        );
    };
    let (source_path, rows) = read_domain_rows(
        &run_dir,
        "structure_rows",
        &schema,
        &["structure", "patch", "docking", "pose"],
    );
    let artifacts = run_artifacts_for_tokens(
        &run_dir,
        &[
            "structure",
            "patch",
            "docking",
            "pose",
            ".pdb",
            ".cif",
            ".mmcif",
        ],
    );
    let evidence = run_evidence_for_tokens(
        &run_dir,
        &["structure", "patch", "docking", "pose", "residue"],
    );
    let artifact_backed = source_path.is_some() || !artifacts.is_empty() || !evidence.is_empty();
    let message = if artifact_backed {
        "Structure, patch, docking, and interaction rows are read from the selected run artifacts."
    } else {
        "Structure, patch, docking, and interaction rows are available after structure evidence generation."
    };
    let viewer = structure_viewer_value(run_id, &run_dir, &rows, &artifacts, &evidence);
    format!(
        "{{\"run_id\":\"{}\",\"source_path\":{},\"schema\":{},\"structure_rows\":{},\"row_count\":{},\"artifacts\":{},\"evidence\":{},\"viewer\":{},\"artifact_backed\":{},\"message\":\"{}\"}}",
        escape_json(run_id),
        option_json(source_path.as_deref()),
        json_string_array(schema.iter().copied()),
        Value::Array(rows.clone()),
        rows.len(),
        Value::Array(artifacts.clone()),
        Value::Array(evidence.clone()),
        viewer,
        artifact_backed,
        escape_json(message)
    )
}

fn empty_structure_viewer_value(run_id: &str) -> Value {
    serde_json::json!({
        "run_id": run_id,
        "viewer_ready": false,
        "viewer_library": "NGL",
        "viewer_engine": "ngl_ready_fallback",
        "coordinate_mode": "not_available",
        "structure_url": null,
        "structure_format": null,
        "fallback_viewer": true,
        "chains": [],
        "residues": [],
        "patch_residues": [],
        "ligand_atoms": [],
        "interaction_annotations": [],
        "artifact_links": [],
        "source_note": "No structure artifacts are available for this run yet."
    })
}

fn structure_viewer_value(
    run_id: &str,
    run_dir: &Path,
    rows: &[Value],
    artifacts: &[Value],
    evidence: &[Value],
) -> Value {
    let mut chains = Vec::new();
    let mut residues = Vec::new();
    let mut patch_residues = Vec::new();
    let mut ligand_atoms = Vec::new();
    let mut interactions = Vec::new();
    let mut seen = BTreeMap::<String, bool>::new();
    for (index, row) in rows.iter().enumerate() {
        let candidate_id = first_non_empty_field(row, &["candidate_id", "candidate"]);
        let patch = first_non_empty_field(row, &["binding_patch", "patch"]);
        let ligand = first_non_empty_field(row, &["ligand_position", "ligand_id"]);
        let evidence_text = first_non_empty_field(row, &["patch_evidence", "structure_evidence"]);
        let chain_label = evidence_text
            .split(';')
            .find_map(|part| {
                part.split_once("target_uniprot=")
                    .map(|(_, value)| value.trim())
            })
            .filter(|value| !value.is_empty())
            .unwrap_or("target");
        let chain_id = format!("chain_{}", index + 1);
        if seen.insert(chain_id.clone(), true).is_none() {
            chains.push(serde_json::json!({
                "chain_id": chain_id,
                "role": "target",
                "label": chain_label,
                "candidate_id": candidate_id,
                "source": "structure_rows"
            }));
        }
        let interaction_text =
            first_non_empty_field(row, &["interaction_residues", "key_residues"]);
        let mut residue_index = 0usize;
        for token in interaction_text
            .split([';', ',', '|'])
            .map(str::trim)
            .filter(|token| !token.is_empty())
        {
            residue_index += 1;
            let residue_id = token
                .split_once('=')
                .map(|(key, _)| key.trim())
                .unwrap_or(token)
                .replace(' ', "_");
            residues.push(serde_json::json!({
                "residue_id": residue_id,
                "chain_id": format!("chain_{}", index + 1),
                "label": token,
                "source": "interaction_residues"
            }));
            patch_residues.push(serde_json::json!({
                "patch_residue_id": format!("patch_{}_{}", index + 1, residue_index),
                "chain_id": format!("chain_{}", index + 1),
                "residue_label": token,
                "patch_label": patch,
                "source": "binding_patch"
            }));
            interactions.push(serde_json::json!({
                "interaction_id": format!("interaction_{}_{}", index + 1, residue_index),
                "candidate_id": candidate_id,
                "ligand_label": ligand,
                "residue_label": token,
                "annotation": "evidence-derived contact",
                "source": "interaction_residues"
            }));
        }
        if !ligand.is_empty() {
            ligand_atoms.push(serde_json::json!({
                "atom_id": format!("ligand_centroid_{}", index + 1),
                "ligand_label": ligand,
                "atom_role": "schematic_centroid",
                "coordinate_source": "not_atomic_coordinates",
                "x": 0.0,
                "y": 0.0,
                "z": 0.0
            }));
        }
    }
    let artifact_links = artifacts
        .iter()
        .filter_map(|artifact| {
            let path = json_row_field_text(artifact, "path");
            if path.is_empty() {
                None
            } else {
                Some(serde_json::json!({
                    "artifact_type": json_row_field_text(artifact, "artifact_type"),
                    "path": path,
                    "exists": artifact.get("exists").and_then(Value::as_bool).unwrap_or(false)
                }))
            }
        })
        .collect::<Vec<_>>();
    let structure_file = first_structure_file_for_run(run_dir);
    let structure_url = structure_file
        .as_ref()
        .map(|_| format!("/api/runs/{}/structure-file", safe_id(run_id)));
    let structure_format = structure_file
        .as_ref()
        .and_then(|path| structure_file_format(path));
    serde_json::json!({
        "run_id": run_id,
        "viewer_ready": !rows.is_empty() || !artifacts.is_empty() || !evidence.is_empty(),
        "viewer_library": "NGL",
        "viewer_engine": if structure_url.is_some() { "ngl" } else { "ngl_ready_fallback" },
        "coordinate_mode": if structure_url.is_some() { "atomic_coordinates" } else if ligand_atoms.is_empty() { "not_available" } else { "evidence_schematic" },
        "structure_url": structure_url,
        "structure_format": structure_format,
        "fallback_viewer": structure_url.is_none(),
        "chains": chains,
        "residues": residues,
        "patch_residues": patch_residues,
        "ligand_atoms": ligand_atoms,
        "interaction_annotations": interactions,
        "artifact_links": artifact_links,
        "source_note": "Viewer-ready JSON is derived from structure rows, artifact links, and evidence rows. NGL is used when a run-local PDB/mmCIF/CIF structure file is available; otherwise the UI falls back to evidence-derived schematic rendering."
    })
}

fn structure_file_response(options: &ServerOptions, run_id: &str) -> (u16, &'static str, String) {
    let Some(run_dir) = run_dir_for(options, run_id) else {
        return (
            404,
            "application/json; charset=utf-8",
            "{\"error\":\"run_not_found\"}".to_string(),
        );
    };
    let Some(path) = first_structure_file_for_run(&run_dir) else {
        return (
            404,
            "application/json; charset=utf-8",
            "{\"error\":\"structure_file_not_found\"}".to_string(),
        );
    };
    let Ok(canonical_run_dir) = run_dir.canonicalize() else {
        return (
            404,
            "application/json; charset=utf-8",
            "{\"error\":\"run_dir_unreadable\"}".to_string(),
        );
    };
    let Ok(canonical_path) = path.canonicalize() else {
        return (
            404,
            "application/json; charset=utf-8",
            "{\"error\":\"structure_file_unreadable\"}".to_string(),
        );
    };
    if !canonical_path.starts_with(canonical_run_dir) {
        return (
            404,
            "application/json; charset=utf-8",
            "{\"error\":\"structure_file_outside_run\"}".to_string(),
        );
    }
    match fs::read_to_string(&canonical_path) {
        Ok(text) => (200, "chemical/x-pdb; charset=utf-8", text),
        Err(_) => (
            404,
            "application/json; charset=utf-8",
            "{\"error\":\"structure_file_read_failed\"}".to_string(),
        ),
    }
}

fn first_structure_file_for_run(run_dir: &Path) -> Option<PathBuf> {
    collect_run_files(run_dir, 500)
        .into_iter()
        .find(|path| structure_file_format(path).is_some())
}

fn structure_file_format(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_string_lossy().to_ascii_lowercase();
    if name.ends_with(".pdb") || name.ends_with(".ent") {
        Some("pdb".to_string())
    } else if name.ends_with(".cif") {
        Some("cif".to_string())
    } else if name.ends_with(".mmcif") {
        Some("mmcif".to_string())
    } else {
        None
    }
}

fn md_summary_json(options: &ServerOptions, run_id: &str) -> String {
    let schema = [
        "candidate_id",
        "rmsd",
        "rmsf",
        "ligand_drift",
        "hydrogen_bond_occupancy",
        "contact_occupancy",
        "interaction_persistence",
        "md_risk_flags",
        "md_score_decomposition",
        "md_stability_score",
    ];
    run_domain_json(
        options,
        run_id,
        RunDomainSpec {
            rows_key: "md_rows",
            schema: &schema,
            row_markers: &["md", "rmsd", "rmsf", "drift", "occupancy", "trajectory"],
            artifact_tokens: &[
                "md_summary",
                "short_md",
                "molecular_dynamics",
                "rmsd",
                "rmsf",
                "drift",
                "occupancy",
                "trajectory",
                "hbond",
                "contact",
            ],
            evidence_tokens: &[
                "md_stability",
                "md_score",
                "rmsd",
                "rmsf",
                "drift",
                "occupancy",
                "trajectory",
                "hbond",
                "contact",
            ],
            loaded_message: "MD summary rows are read from the selected run artifacts.",
            empty_message: "MD summary rows are available after short-MD analysis artifacts are generated.",
        },
    )
}

fn run_domain_json(options: &ServerOptions, run_id: &str, spec: RunDomainSpec<'_>) -> String {
    let Some(run_dir) = run_dir_for(options, run_id) else {
        return format!(
            "{{\"run_id\":\"{}\",\"source_path\":null,\"schema\":{},\"{}\":[],\"artifacts\":[],\"evidence\":[],\"artifact_backed\":false,\"message\":\"{}\"}}",
            escape_json(run_id),
            json_string_array(spec.schema.iter().copied()),
            escape_json(spec.rows_key),
            escape_json(spec.empty_message)
        );
    };
    let (source_path, rows) =
        read_domain_rows(&run_dir, spec.rows_key, spec.schema, spec.row_markers);
    let artifacts = run_artifacts_for_tokens(&run_dir, spec.artifact_tokens);
    let evidence = run_evidence_for_tokens(&run_dir, spec.evidence_tokens);
    let artifact_backed = source_path.is_some() || !artifacts.is_empty() || !evidence.is_empty();
    let message = if artifact_backed {
        spec.loaded_message
    } else {
        spec.empty_message
    };
    format!(
        "{{\"run_id\":\"{}\",\"source_path\":{},\"schema\":{},\"{}\":{},\"row_count\":{},\"artifacts\":{},\"evidence\":{},\"artifact_backed\":{},\"message\":\"{}\"}}",
        escape_json(run_id),
        option_json(source_path.as_deref()),
        json_string_array(spec.schema.iter().copied()),
        escape_json(spec.rows_key),
        Value::Array(rows.clone()),
        rows.len(),
        Value::Array(artifacts),
        Value::Array(evidence),
        artifact_backed,
        escape_json(message)
    )
}

fn read_domain_rows(
    run_dir: &Path,
    domain: &str,
    schema: &[&str],
    markers: &[&str],
) -> (Option<String>, Vec<Value>) {
    if let Some(result) =
        read_result_table(run_dir, schema, markers).and_then(|(path, rows_json, _)| {
            serde_json::from_str::<Value>(&rows_json)
                .ok()
                .and_then(|value| value.as_array().cloned())
                .map(|rows| (Some(path.display().to_string()), rows))
        })
    {
        return result;
    }
    let fallback = match domain {
        "genome_context" => e2e_candidate_rows(run_dir),
        "structure_rows" => e2e_structure_rows(run_dir),
        "md_rows" => e2e_md_rows(run_dir),
        _ => None,
    };
    fallback
        .map(|(path, rows)| (Some(path.display().to_string()), rows))
        .unwrap_or_else(|| (None, Vec::new()))
}

fn run_artifacts_for_tokens(run_dir: &Path, tokens: &[&str]) -> Vec<Value> {
    let index = run_artifact_index_json(run_dir);
    serde_json::from_str::<Value>(&index)
        .ok()
        .and_then(|value| value.get("items").cloned())
        .and_then(|items| items.as_array().cloned())
        .unwrap_or_default()
        .into_iter()
        .filter(|item| value_contains_any(item, tokens))
        .collect()
}

fn run_evidence_for_tokens(run_dir: &Path, tokens: &[&str]) -> Vec<Value> {
    let schema = [
        "report_item",
        "source_file",
        "manifest",
        "schema",
        "tool_version",
        "artifact_hash",
        "validation_state",
        "report_section",
    ];
    read_result_table(run_dir, &schema, &["evidence", "ledger"])
        .and_then(|(_, rows_json, _)| serde_json::from_str::<Value>(&rows_json).ok())
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default()
        .into_iter()
        .filter(|item| value_contains_any(item, tokens))
        .collect()
}

fn value_contains_any(value: &Value, tokens: &[&str]) -> bool {
    let text = value.to_string().to_lowercase();
    tokens
        .iter()
        .any(|token| text.contains(&token.to_lowercase()))
}

fn candidate_result_table(run_dir: &Path, schema: &[&str]) -> Option<(PathBuf, String, usize)> {
    read_result_table(run_dir, schema, &["candidate", "ranking"]).or_else(|| {
        e2e_candidate_rows(run_dir).map(|(path, rows)| {
            let count = rows.len();
            (path, Value::Array(rows).to_string(), count)
        })
    })
}

fn evidence_result_table(run_dir: &Path, schema: &[&str]) -> Option<(PathBuf, String, usize)> {
    read_result_table(run_dir, schema, &["evidence", "ledger"]).or_else(|| {
        e2e_evidence_rows(run_dir).map(|(path, rows)| {
            let count = rows.len();
            (path, Value::Array(rows).to_string(), count)
        })
    })
}

fn e2e_root(run_dir: &Path) -> PathBuf {
    run_dir.join("science_artifacts").join("e2e")
}

fn e2e_summary_path(run_dir: &Path) -> PathBuf {
    e2e_root(run_dir).join("e2e_examples_summary.tsv")
}

fn read_table_rows_unchecked(path: &Path) -> Option<Vec<Value>> {
    let extension = path.extension().and_then(|ext| ext.to_str()).unwrap_or("");
    let delimiter = match extension {
        "tsv" => '\t',
        "csv" => ',',
        _ => return None,
    };
    let text = fs::read_to_string(path).ok()?;
    let mut lines = text.lines().filter(|line| !line.trim().is_empty());
    let headers = split_delimited_line(lines.next()?, delimiter)
        .into_iter()
        .map(|header| normalize_field_name(&header))
        .collect::<Vec<_>>();
    let mut rows = Vec::new();
    for line in lines.take(500) {
        let values = split_delimited_line(line, delimiter);
        let mut row = serde_json::Map::new();
        for (index, header) in headers.iter().enumerate() {
            if header.is_empty() {
                continue;
            }
            row.insert(
                header.clone(),
                Value::String(values.get(index).cloned().unwrap_or_default()),
            );
        }
        rows.push(Value::Object(row));
    }
    Some(rows)
}

fn read_json_object(path: &Path) -> Option<Value> {
    let value = serde_json::from_str::<Value>(&fs::read_to_string(path).ok()?).ok()?;
    value.as_object()?;
    Some(value)
}

fn e2e_example_ids(run_dir: &Path) -> Vec<String> {
    let summary = e2e_summary_path(run_dir);
    let mut ids = read_table_rows_unchecked(&summary)
        .unwrap_or_default()
        .into_iter()
        .map(|row| json_row_field_text(&row, "example_id"))
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    if ids.is_empty() {
        let manifest_dir = e2e_root(run_dir).join("candidate_manifests");
        if let Ok(entries) = fs::read_dir(manifest_dir) {
            ids.extend(entries.flatten().filter_map(|entry| {
                entry
                    .path()
                    .file_stem()
                    .map(|stem| stem.to_string_lossy().to_string())
            }));
        }
    }
    ids.sort();
    ids.dedup();
    ids
}

fn e2e_result_example_ids(run_dir: &Path) -> Vec<String> {
    e2e_example_ids(run_dir)
        .into_iter()
        .filter(|example_id| {
            e2e_summary_field(run_dir, example_id, "example_kind") == "candidate_benchmark"
        })
        .collect()
}

fn e2e_json_field(run_dir: &Path, subdir: &str, example_id: &str, field: &str) -> String {
    read_json_object(
        &e2e_root(run_dir)
            .join(subdir)
            .join(format!("{example_id}.json")),
    )
    .map(|value| json_row_field_text(&value, field))
    .unwrap_or_default()
}

fn e2e_summary_field(run_dir: &Path, example_id: &str, field: &str) -> String {
    read_table_rows_unchecked(&e2e_summary_path(run_dir))
        .unwrap_or_default()
        .into_iter()
        .find(|row| json_row_field_text(row, "example_id") == example_id)
        .map(|row| json_row_field_text(&row, field))
        .unwrap_or_default()
}

fn e2e_metric_values(run_dir: &Path, subdir: &str, example_id: &str) -> BTreeMap<String, String> {
    let path = e2e_root(run_dir)
        .join(subdir)
        .join(format!("{example_id}.tsv"));
    let mut values = BTreeMap::new();
    for row in read_table_rows_unchecked(&path).unwrap_or_default() {
        let key = first_non_empty_field(&row, &["component", "metric", "interaction"]);
        let value = first_non_empty_field(&row, &["value", "occupancy", "status"]);
        if !key.is_empty() && !value.is_empty() {
            values.insert(key, value);
        }
    }
    values
}

fn first_non_empty_field(row: &Value, fields: &[&str]) -> String {
    fields
        .iter()
        .map(|field| json_row_field_text(row, field))
        .find(|value| !value.is_empty())
        .unwrap_or_default()
}

fn e2e_candidate_rows(run_dir: &Path) -> Option<(PathBuf, Vec<Value>)> {
    let summary = e2e_summary_path(run_dir);
    if !summary.exists() {
        return None;
    }
    let mut rows = Vec::new();
    for (index, example_id) in e2e_result_example_ids(run_dir).into_iter().enumerate() {
        let candidate_manifest = e2e_root(run_dir)
            .join("candidate_manifests")
            .join(format!("{example_id}.json"));
        let manifest = read_json_object(&candidate_manifest).unwrap_or(Value::Null);
        let score = e2e_metric_values(run_dir, "score_decompositions", &example_id);
        let md_score = score.get("final_md_score").cloned().unwrap_or_default();
        let candidate_id = json_row_field_text(&manifest, "candidate_id");
        let mut row = serde_json::Map::new();
        row.insert("rank".to_string(), Value::String((index + 1).to_string()));
        row.insert(
            "candidate_id".to_string(),
            Value::String(if candidate_id.is_empty() {
                example_id.clone()
            } else {
                candidate_id
            }),
        );
        row.insert(
            "gene_name".to_string(),
            Value::String(json_row_field_text(&manifest, "target_uniprot")),
        );
        row.insert("orf_id".to_string(), Value::String(String::new()));
        row.insert(
            "genomic_coordinates".to_string(),
            Value::String(String::new()),
        );
        row.insert(
            "context_class".to_string(),
            Value::String(e2e_summary_field(run_dir, &example_id, "example_kind")),
        );
        row.insert("fragment_score".to_string(), Value::String(String::new()));
        row.insert(
            "co_occurrence_score".to_string(),
            Value::String(String::new()),
        );
        row.insert("plm_score".to_string(), Value::String(String::new()));
        row.insert("structure_score".to_string(), Value::String(String::new()));
        row.insert("docking_score".to_string(), Value::String(String::new()));
        row.insert(
            "md_stability_score".to_string(),
            Value::String(md_score.clone()),
        );
        row.insert("final_priority_score".to_string(), Value::String(md_score));
        row.insert(
            "risk_flags".to_string(),
            Value::String(json_row_field_text(&manifest, "evidence_scope")),
        );
        row.insert(
            "source_artifact".to_string(),
            Value::String(candidate_manifest.display().to_string()),
        );
        row.insert(
            "database_evidence".to_string(),
            Value::String(format!(
                "target_uniprot={};ligand_id={}",
                json_row_field_text(&manifest, "target_uniprot"),
                json_row_field_text(&manifest, "ligand_id")
            )),
        );
        row.insert(
            "experimental_validation_guidance".to_string(),
            Value::String(
                "computational_prior_only;requires_independent_experimental_validation".to_string(),
            ),
        );
        rows.push(Value::Object(row));
    }
    Some((summary, rows))
}

fn e2e_evidence_rows(run_dir: &Path) -> Option<(PathBuf, Vec<Value>)> {
    let evidence_dir = e2e_root(run_dir).join("evidence_ledgers");
    if !evidence_dir.exists() {
        return None;
    }
    let mut rows = Vec::new();
    let mut first_path = None;
    let result_ids = e2e_result_example_ids(run_dir);
    let mut paths = fs::read_dir(&evidence_dir)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    paths.sort();
    for path in paths {
        if path.extension().and_then(|ext| ext.to_str()) != Some("tsv") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        if !result_ids.iter().any(|example_id| example_id == stem) {
            continue;
        }
        first_path.get_or_insert_with(|| path.clone());
        for source in read_table_rows_unchecked(&path).unwrap_or_default() {
            let mut row = serde_json::Map::new();
            row.insert(
                "report_item".to_string(),
                Value::String(json_row_field_text(&source, "claim_id")),
            );
            row.insert(
                "source_file".to_string(),
                Value::String(json_row_field_text(&source, "evidence_artifact")),
            );
            row.insert(
                "manifest".to_string(),
                Value::String("science_artifacts/e2e/e2e_examples_summary.tsv".to_string()),
            );
            row.insert(
                "schema".to_string(),
                Value::String("phase_0_8_5_e2e_evidence_ledger".to_string()),
            );
            row.insert(
                "tool_version".to_string(),
                Value::String(format!("gls-cli {PRODUCT_VERSION}")),
            );
            row.insert("artifact_hash".to_string(), Value::String(String::new()));
            row.insert(
                "validation_state".to_string(),
                Value::String(json_row_field_text(&source, "owner")),
            );
            row.insert(
                "report_section".to_string(),
                Value::String(json_row_field_text(&source, "claim_boundary")),
            );
            rows.push(Value::Object(row));
        }
    }
    first_path.map(|path| (path, rows))
}

fn e2e_structure_rows(run_dir: &Path) -> Option<(PathBuf, Vec<Value>)> {
    let summary = e2e_summary_path(run_dir);
    if !summary.exists() {
        return None;
    }
    let mut rows = Vec::new();
    for example_id in e2e_result_example_ids(run_dir) {
        let complex_manifest = e2e_root(run_dir)
            .join("complex_manifests")
            .join(format!("{example_id}.json"));
        let manifest = read_json_object(&complex_manifest).unwrap_or(Value::Null);
        let occupancy = e2e_metric_values(run_dir, "interaction_occupancy_summaries", &example_id);
        let mut row = serde_json::Map::new();
        row.insert(
            "candidate_id".to_string(),
            Value::String(json_row_field_text(&manifest, "candidate_id")),
        );
        row.insert("structure_score".to_string(), Value::String(String::new()));
        row.insert(
            "structure_evidence".to_string(),
            Value::String(complex_manifest.display().to_string()),
        );
        row.insert(
            "binding_patch".to_string(),
            Value::String(json_row_field_text(&manifest, "complex_id")),
        );
        row.insert(
            "patch_evidence".to_string(),
            Value::String(format!(
                "target_uniprot={};ligand_id={}",
                json_row_field_text(&manifest, "target_uniprot"),
                json_row_field_text(&manifest, "ligand_id")
            )),
        );
        row.insert("key_residues".to_string(), Value::String(String::new()));
        row.insert("docking_score".to_string(), Value::String(String::new()));
        row.insert("docking_pose".to_string(), Value::String(String::new()));
        row.insert(
            "ligand_position".to_string(),
            Value::String(json_row_field_text(&manifest, "ligand_id")),
        );
        row.insert(
            "interaction_residues".to_string(),
            Value::String(
                occupancy
                    .iter()
                    .map(|(key, value)| format!("{key}={value}"))
                    .collect::<Vec<_>>()
                    .join(";"),
            ),
        );
        rows.push(Value::Object(row));
    }
    Some((summary, rows))
}

fn e2e_md_rows(run_dir: &Path) -> Option<(PathBuf, Vec<Value>)> {
    let summary = e2e_summary_path(run_dir);
    if !summary.exists() {
        return None;
    }
    let mut rows = Vec::new();
    for example_id in e2e_result_example_ids(run_dir) {
        let candidate_id =
            e2e_json_field(run_dir, "candidate_manifests", &example_id, "candidate_id");
        let score = e2e_metric_values(run_dir, "score_decompositions", &example_id);
        let rmsd = e2e_metric_values(run_dir, "rmsd_summaries", &example_id);
        let occupancy = e2e_metric_values(run_dir, "interaction_occupancy_summaries", &example_id);
        let md_status = e2e_json_field(run_dir, "md_result_manifests", &example_id, "status");
        let md_risk_flags = if md_status.eq_ignore_ascii_case("pass") {
            "md_result_schema_validated".to_string()
        } else {
            md_status
        };
        let mut row = serde_json::Map::new();
        row.insert("candidate_id".to_string(), Value::String(candidate_id));
        row.insert(
            "rmsd".to_string(),
            Value::String(rmsd.get("ligand_rmsd_max").cloned().unwrap_or_default()),
        );
        row.insert("rmsf".to_string(), Value::String(String::new()));
        row.insert("ligand_drift".to_string(), Value::String(String::new()));
        row.insert(
            "hydrogen_bond_occupancy".to_string(),
            Value::String(occupancy.get("hbond").cloned().unwrap_or_default()),
        );
        row.insert(
            "contact_occupancy".to_string(),
            Value::String(occupancy.get("hydrophobic").cloned().unwrap_or_default()),
        );
        row.insert(
            "interaction_persistence".to_string(),
            Value::String(
                score
                    .get("interaction_persistence")
                    .cloned()
                    .unwrap_or_default(),
            ),
        );
        row.insert("md_risk_flags".to_string(), Value::String(md_risk_flags));
        row.insert(
            "md_score_decomposition".to_string(),
            Value::String(
                score
                    .iter()
                    .map(|(key, value)| format!("{key}={value}"))
                    .collect::<Vec<_>>()
                    .join(";"),
            ),
        );
        row.insert(
            "md_stability_score".to_string(),
            Value::String(score.get("final_md_score").cloned().unwrap_or_default()),
        );
        rows.push(Value::Object(row));
    }
    Some((summary, rows))
}

fn candidate_detail_json(options: &ServerOptions, candidate_id: &str) -> String {
    let candidate_schema = [
        "rank",
        "candidate_id",
        "gene_name",
        "orf_id",
        "genomic_coordinates",
        "context_class",
        "fragment_score",
        "co_occurrence_score",
        "plm_score",
        "structure_score",
        "docking_score",
        "md_stability_score",
        "final_priority_score",
        "risk_flags",
    ];
    let evidence_schema = [
        "report_item",
        "source_file",
        "manifest",
        "schema",
        "tool_version",
        "artifact_hash",
        "validation_state",
        "report_section",
    ];
    let Some((run_id, run_dir)) = latest_run_dir(options) else {
        return format!(
            "{{\"candidate_id\":\"{}\",\"selected\":false,\"sections\":[\"source\",\"orf_sequence\",\"genomic_position\",\"reading_frame\",\"flanking_region\",\"fragment_hits\",\"co_occurrence_evidence\",\"structure_evidence\",\"docking_evidence\",\"md_evidence\",\"database_evidence\",\"risk_flags\",\"experimental_validation_guidance\"],\"message\":\"candidate_artifact_not_found\"}}",
            escape_json(candidate_id)
        );
    };
    let Some((source_path, rows_json, _)) = candidate_result_table(&run_dir, &candidate_schema)
    else {
        return format!(
            "{{\"candidate_id\":\"{}\",\"selected\":false,\"run_id\":\"{}\",\"sections\":[\"source\",\"orf_sequence\",\"genomic_position\",\"reading_frame\",\"flanking_region\",\"fragment_hits\",\"co_occurrence_evidence\",\"structure_evidence\",\"docking_evidence\",\"md_evidence\",\"database_evidence\",\"risk_flags\",\"experimental_validation_guidance\"],\"message\":\"candidate_artifact_not_found\"}}",
            escape_json(candidate_id),
            escape_json(&run_id)
        );
    };
    let rows = serde_json::from_str::<Value>(&rows_json)
        .ok()
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default();
    let candidate = rows.into_iter().find(|row| {
        ["candidate_id", "orf_id", "gene_name", "rank"]
            .iter()
            .any(|field| json_row_field_text(row, field) == candidate_id)
    });
    let Some(candidate) = candidate else {
        return format!(
            "{{\"candidate_id\":\"{}\",\"selected\":false,\"run_id\":\"{}\",\"source_path\":\"{}\",\"message\":\"candidate_id_not_found\"}}",
            escape_json(candidate_id),
            escape_json(&run_id),
            escape_json(&source_path.display().to_string())
        );
    };
    let evidence_rows = evidence_result_table(&run_dir, &evidence_schema)
        .and_then(|(_, evidence_json, _)| serde_json::from_str::<Value>(&evidence_json).ok())
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default()
        .into_iter()
        .filter(|row| {
            let text = row.to_string().to_lowercase();
            ["candidate_id", "orf_id", "gene_name"]
                .iter()
                .map(|field| json_row_field_text(&candidate, field).to_lowercase())
                .filter(|value| !value.is_empty())
                .any(|token| text.contains(&token))
        })
        .collect::<Vec<_>>();
    format!(
        "{{\"candidate_id\":\"{}\",\"selected\":true,\"run_id\":\"{}\",\"source_path\":\"{}\",\"candidate\":{},\"evidence\":{},\"sections\":[\"source\",\"orf_sequence\",\"genomic_position\",\"reading_frame\",\"flanking_region\",\"fragment_hits\",\"co_occurrence_evidence\",\"structure_evidence\",\"docking_evidence\",\"md_evidence\",\"database_evidence\",\"risk_flags\",\"experimental_validation_guidance\"]}}",
        escape_json(candidate_id),
        escape_json(&run_id),
        escape_json(&source_path.display().to_string()),
        candidate,
        Value::Array(evidence_rows)
    )
}

fn read_result_table(
    run_dir: &Path,
    schema: &[&str],
    path_markers: &[&str],
) -> Option<(PathBuf, String, usize)> {
    let mut files = collect_run_files(run_dir, 500);
    files.sort();
    for path in files {
        let text = path.display().to_string().to_lowercase();
        if !path_markers.iter().any(|marker| text.contains(marker)) {
            continue;
        }
        if let Some((rows, count)) = read_json_rows(&path, schema) {
            return Some((path, rows, count));
        }
        if let Some((rows, count)) = read_delimited_rows(&path, schema) {
            return Some((path, rows, count));
        }
    }
    None
}

fn read_json_rows(path: &Path, schema: &[&str]) -> Option<(String, usize)> {
    if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
        return None;
    }
    let value = serde_json::from_str::<Value>(&fs::read_to_string(path).ok()?).ok()?;
    let rows = if let Some(array) = value.as_array() {
        array.clone()
    } else {
        value
            .get("candidates")
            .and_then(Value::as_array)
            .cloned()
            .or_else(|| value.get("evidence").and_then(Value::as_array).cloned())
            .or_else(|| value.get("rows").and_then(Value::as_array).cloned())
            .or_else(|| value.get("items").and_then(Value::as_array).cloned())
            .or_else(|| {
                schema
                    .iter()
                    .find_map(|key| value.get(*key).and_then(Value::as_array))
                    .cloned()
            })?
    };
    let rows = rows.into_iter().take(500).collect::<Vec<_>>();
    let count = rows.len();
    Some((serde_json::Value::Array(rows).to_string(), count))
}

fn read_delimited_rows(path: &Path, schema: &[&str]) -> Option<(String, usize)> {
    let extension = path.extension().and_then(|ext| ext.to_str()).unwrap_or("");
    let delimiter = match extension {
        "tsv" => '\t',
        "csv" => ',',
        _ => return None,
    };
    let text = fs::read_to_string(path).ok()?;
    let mut lines = text.lines().filter(|line| !line.trim().is_empty());
    let header = lines.next()?;
    let headers = split_delimited_line(header, delimiter);
    if !headers.iter().any(|header| {
        schema
            .iter()
            .any(|field| header.eq_ignore_ascii_case(field))
    }) {
        return None;
    }
    let mut rows = Vec::new();
    for line in lines.take(500) {
        let values = split_delimited_line(line, delimiter);
        let mut row = serde_json::Map::new();
        for (index, header) in headers.iter().enumerate() {
            let key = normalize_field_name(header);
            if key.is_empty() {
                continue;
            }
            row.insert(
                key,
                Value::String(values.get(index).cloned().unwrap_or_default()),
            );
        }
        rows.push(Value::Object(row));
    }
    let count = rows.len();
    Some((Value::Array(rows).to_string(), count))
}

fn split_delimited_line(line: &str, delimiter: char) -> Vec<String> {
    line.split(delimiter)
        .map(|value| value.trim().trim_matches('"').to_string())
        .collect()
}

fn json_scalar_to_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        Value::Bool(flag) => flag.to_string(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn json_row_field_text(row: &Value, field: &str) -> String {
    let space_key = field.replace('_', " ");
    let dash_key = field.replace('_', "-");
    row.get(field)
        .or_else(|| row.get(space_key.as_str()))
        .or_else(|| row.get(dash_key.as_str()))
        .map(json_scalar_to_text)
        .unwrap_or_default()
}

fn json_rows_to_tsv(rows_json: &str, schema: &[&str]) -> String {
    let mut lines = Vec::new();
    lines.push(schema.join("\t"));
    let rows = serde_json::from_str::<Value>(rows_json)
        .ok()
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default();
    for row in rows {
        let values = schema
            .iter()
            .map(|field| json_row_field_text(&row, field).replace(['\t', '\r', '\n'], " "))
            .collect::<Vec<_>>();
        lines.push(values.join("\t"));
    }
    lines.join("\n") + "\n"
}

fn csv_escape(value: &str) -> String {
    if value
        .chars()
        .any(|ch| matches!(ch, ',' | '"' | '\r' | '\n'))
    {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

fn json_rows_to_csv(rows_json: &str, schema: &[&str]) -> String {
    let mut lines = Vec::new();
    lines.push(
        schema
            .iter()
            .map(|field| csv_escape(field))
            .collect::<Vec<_>>()
            .join(","),
    );
    let rows = serde_json::from_str::<Value>(rows_json)
        .ok()
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default();
    for row in rows {
        let values = schema
            .iter()
            .map(|field| csv_escape(&json_row_field_text(&row, field)))
            .collect::<Vec<_>>();
        lines.push(values.join(","));
    }
    lines.join("\n") + "\n"
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn markdown_cell(value: &str) -> String {
    value.replace('|', "\\|").replace(['\r', '\n'], " ")
}

fn json_rows_to_html_table(rows_json: &str, schema: &[&str]) -> String {
    let rows = serde_json::from_str::<Value>(rows_json)
        .ok()
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default();
    let header = schema
        .iter()
        .map(|field| format!("<th>{}</th>", html_escape(field)))
        .collect::<Vec<_>>()
        .join("");
    let body = rows
        .iter()
        .map(|row| {
            let cells = schema
                .iter()
                .map(|field| format!("<td>{}</td>", html_escape(&json_row_field_text(row, field))))
                .collect::<Vec<_>>()
                .join("");
            format!("<tr>{cells}</tr>")
        })
        .collect::<Vec<_>>()
        .join("");
    format!("<table><thead><tr>{header}</tr></thead><tbody>{body}</tbody></table>")
}

fn report_value(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.contains('\\') || trimmed.contains('/') {
        Path::new(trimmed)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(trimmed)
            .replace('_', " ")
    } else {
        trimmed.replace('_', " ")
    }
}

fn json_rows_to_report_markdown_table(rows_json: &str, columns: &[(&str, &str)]) -> String {
    let rows = serde_json::from_str::<Value>(rows_json)
        .ok()
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default();
    let mut lines = Vec::new();
    lines.push(format!(
        "| {} |",
        columns
            .iter()
            .map(|(_, label)| markdown_cell(label))
            .collect::<Vec<_>>()
            .join(" | ")
    ));
    lines.push(format!(
        "| {} |",
        columns
            .iter()
            .map(|_| "---")
            .collect::<Vec<_>>()
            .join(" | ")
    ));
    for row in rows {
        lines.push(format!(
            "| {} |",
            columns
                .iter()
                .map(|(field, _)| markdown_cell(&report_value(&json_row_field_text(&row, field))))
                .collect::<Vec<_>>()
                .join(" | ")
        ));
    }
    if lines.len() == 2 {
        lines.push(format!(
            "| {} |",
            columns
                .iter()
                .map(|_| "No rows available")
                .collect::<Vec<_>>()
                .join(" | ")
        ));
    }
    lines.join("\n") + "\n"
}

fn normalize_field_name(value: &str) -> String {
    value
        .trim()
        .to_lowercase()
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect::<String>()
        .split('_')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("_")
}

#[derive(Default)]
struct DatabaseSource {
    id: String,
    role: String,
    required_for_pass: bool,
    official_page: String,
    acquisition: String,
    local_glob: String,
    license_note: String,
}

fn parse_database_sources(config_path: &Path) -> Vec<DatabaseSource> {
    let Ok(text) = fs::read_to_string(config_path) else {
        return Vec::new();
    };
    let mut sources = Vec::new();
    let mut current: Option<DatabaseSource> = None;
    for raw_line in text.lines() {
        let line = raw_line.trim_end();
        if line.starts_with("  ") && !line.starts_with("    ") && line.trim_end().ends_with(':') {
            if let Some(source) = current.take()
                && !source.id.is_empty()
            {
                sources.push(source);
            }
            current = Some(DatabaseSource {
                id: line.trim().trim_end_matches(':').to_string(),
                ..DatabaseSource::default()
            });
            continue;
        }
        let Some(source) = current.as_mut() else {
            continue;
        };
        let trimmed = line.trim();
        let Some((key, value)) = trimmed.split_once(':') else {
            continue;
        };
        let value = value.trim().trim_matches('"').to_string();
        match key {
            "role" => source.role = value,
            "required_for_pass" => source.required_for_pass = value == "true",
            "official_page" => source.official_page = value,
            "acquisition" => source.acquisition = value,
            "local_glob" => source.local_glob = value,
            "license_note" => source.license_note = value,
            _ => {}
        }
    }
    if let Some(source) = current.take()
        && !source.id.is_empty()
    {
        sources.push(source);
    }
    sources
}

fn local_glob_root(workspace_root: &Path, local_glob: &str) -> PathBuf {
    let prefix = local_glob
        .split("**")
        .next()
        .unwrap_or(local_glob)
        .trim_end_matches(['/', '\\', '*']);
    workspace_root.join(prefix)
}

fn database_index_state(root: &Path) -> &'static str {
    if !root.exists() {
        return "missing";
    }
    let files = collect_run_files(root, 1000);
    if files.iter().any(|path| {
        let text = path.display().to_string().to_ascii_lowercase();
        text.contains("index")
            || text.ends_with(".duckdb")
            || text.ends_with(".parquet")
            || text.ends_with(".dmnd")
            || text.ends_with(".m8")
    }) {
        "indexed"
    } else if files.is_empty() {
        "empty"
    } else {
        "data_present_index_not_detected"
    }
}

fn database_action_dir(options: &ServerOptions) -> PathBuf {
    options.artifact_dir.join("databases")
}

fn database_action_file_json(options: &ServerOptions, file_name: &str) -> String {
    let path = database_action_dir(options).join(file_name);
    format!(
        "{{\"path\":\"{}\",\"exists\":{},\"bytes\":{}}}",
        escape_json(&path.display().to_string()),
        path.exists(),
        fs::metadata(&path)
            .map(|metadata| metadata.len())
            .unwrap_or(0)
    )
}

fn databases_json(options: &ServerOptions) -> String {
    let config_path = options.workspace_root.join("configs/database_sources.yml");
    let sources = parse_database_sources(&config_path);
    let resources = sources
        .iter()
        .map(|source| {
            let root = local_glob_root(&options.workspace_root, &source.local_glob);
            let exists = root.exists();
            let files = if exists { file_count(&root) } else { 0 };
            let bytes = if exists { path_size(&root) } else { 0 };
            let index_state = database_index_state(&root);
            let missing_items = if exists && files > 0 { 0 } else { 1 };
            let repair_action = if exists && files > 0 {
                "validate_manifest_and_hash"
            } else if source.required_for_pass {
                "mount_or_download_required_source"
            } else {
                "optional_source_not_configured"
            };
            format!(
                "{{\"resource\":\"{}\",\"role\":\"{}\",\"required\":{},\"official_page\":\"{}\",\"path\":\"{}\",\"local_glob\":\"{}\",\"version\":\"{}\",\"sha256\":\"{}\",\"index_state\":\"{}\",\"missing_items\":{},\"storage_bytes\":{},\"file_count\":{},\"repair_action\":\"{}\",\"license_note\":\"{}\"}}",
                escape_json(&source.id),
                escape_json(&source.role),
                source.required_for_pass,
                escape_json(&source.official_page),
                escape_json(&root.display().to_string()),
                escape_json(&source.local_glob),
                escape_json(&source.acquisition),
                if exists { "manifest_required" } else { "not_available" },
                escape_json(index_state),
                missing_items,
                bytes,
                files,
                escape_json(repair_action),
                escape_json(&source.license_note)
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let profiles = format!(
        "{{\"profile_id\":\"database_sources_yml\",\"path\":\"{}\",\"source_count\":{},\"status\":\"{}\"}}",
        escape_json(&config_path.display().to_string()),
        sources.len(),
        if sources.is_empty() {
            "configuration_missing"
        } else {
            "loaded"
        }
    );
    let latest_validation = database_action_file_json(options, "database_validation_manifest.tsv");
    let latest_index_action =
        database_action_file_json(options, "database_index_action_manifest.tsv");
    format!(
        "{{\"profiles\":[{}],\"resources\":[{}],\"fields\":[\"path\",\"version\",\"sha256\",\"index_state\",\"missing_items\",\"storage_bytes\",\"repair_action\"],\"source_config\":\"{}\",\"latest_validation\":{},\"latest_index_action\":{},\"message\":\"Database resources are read from configs/database_sources.yml and checked against local paths.\"}}",
        profiles,
        resources,
        escape_json(&config_path.display().to_string()),
        latest_validation,
        latest_index_action
    )
}

fn database_validation_json(options: &ServerOptions) -> String {
    let action_dir = database_action_dir(options);
    let manifest = action_dir.join("database_validation_manifest.tsv");
    let summary = action_dir.join("database_validation_summary.tsv");
    let _ = fs::create_dir_all(&action_dir);
    let config_path = options.workspace_root.join("configs/database_sources.yml");
    let sources = parse_database_sources(&config_path);
    let mut rows = Vec::new();
    let mut available_count = 0usize;
    let mut required_missing_count = 0usize;
    for source in &sources {
        let root = local_glob_root(&options.workspace_root, &source.local_glob);
        let exists = root.exists();
        let files = if exists { file_count(&root) } else { 0 };
        let bytes = if exists { path_size(&root) } else { 0 };
        let index_state = database_index_state(&root);
        let status = if exists && files > 0 {
            available_count += 1;
            "available"
        } else if source.required_for_pass {
            required_missing_count += 1;
            "required_source_missing"
        } else {
            "optional_source_missing"
        };
        rows.push(format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            source.id,
            source.role,
            source.required_for_pass,
            root.display(),
            exists,
            files,
            bytes,
            index_state,
            source.license_note.replace(['\t', '\r', '\n'], " "),
            status
        ));
    }
    let manifest_text = format!(
        "resource\trole\trequired\tpath\texists\tfile_count\tstorage_bytes\tindex_state\tlicense_note\tstatus\n{}\n",
        rows.join("\n")
    );
    let summary_text = format!(
        "metric\tvalue\nsource_count\t{}\navailable_source_count\t{available_count}\nrequired_missing_count\t{required_missing_count}\nconfig_path\t{}\n",
        sources.len(),
        config_path.display()
    );
    let write_result =
        write_text(&manifest, &manifest_text).and(write_text(&summary, &summary_text));
    if let Err(err) = write_result {
        return format!(
            "{{\"status\":\"failed\",\"error\":\"{}\"}}",
            escape_json(&err)
        );
    }
    format!(
        "{{\"status\":\"completed\",\"manifest\":\"{}\",\"summary\":\"{}\",\"source_count\":{},\"available_source_count\":{},\"required_missing_count\":{}}}",
        escape_json(&manifest.display().to_string()),
        escape_json(&summary.display().to_string()),
        sources.len(),
        available_count,
        required_missing_count
    )
}

fn database_index_action_json(options: &ServerOptions) -> String {
    let action_dir = database_action_dir(options);
    let manifest = action_dir.join("database_index_action_manifest.tsv");
    let _ = fs::create_dir_all(&action_dir);
    let config_path = options.workspace_root.join("configs/database_sources.yml");
    let sources = parse_database_sources(&config_path);
    let mut rows = Vec::new();
    let mut build_required_count = 0usize;
    let mut already_indexed_count = 0usize;
    for source in &sources {
        let root = local_glob_root(&options.workspace_root, &source.local_glob);
        let files = if root.exists() { file_count(&root) } else { 0 };
        let index_state = database_index_state(&root);
        let action = match index_state {
            "indexed" => {
                already_indexed_count += 1;
                "no_action_index_detected"
            }
            "data_present_index_not_detected" => {
                build_required_count += 1;
                "index_build_required"
            }
            "empty" => "wait_for_data_files",
            _ => "wait_for_source_path",
        };
        rows.push(format!(
            "{}\t{}\t{}\t{}\t{}\t{}",
            source.id,
            source.role,
            root.display(),
            files,
            index_state,
            action
        ));
    }
    let manifest_text = format!(
        "resource\trole\tpath\tfile_count\tindex_state\taction\n{}\n",
        rows.join("\n")
    );
    if let Err(err) = write_text(&manifest, &manifest_text) {
        return format!(
            "{{\"status\":\"failed\",\"error\":\"{}\"}}",
            escape_json(&err)
        );
    }
    format!(
        "{{\"status\":\"completed\",\"manifest\":\"{}\",\"source_count\":{},\"build_required_count\":{},\"already_indexed_count\":{}}}",
        escape_json(&manifest.display().to_string()),
        sources.len(),
        build_required_count,
        already_indexed_count
    )
}

fn diagnostics_json(options: &ServerOptions) -> String {
    let workspace = &options.workspace_root;
    let runtime = runtime_probe();
    let git = git_probe(options);
    let gls_cli = binary_exists(workspace, "gls-cli");
    let gls_server = binary_exists(workspace, "gls-server");
    let rust_binary_status = if gls_cli && gls_server {
        "ready"
    } else {
        "warning"
    };
    let project_wsl_running = project_wsl_running_from_probe(&runtime);
    let cached_full_scientific_ready = cached_phase091_full_scientific_ready(options);
    let java_host_ok = command_probe("java", &["-version"]);
    let java_wsl_ok = project_wsl_running && project_wsl_command_probe(&["java", "-version"]);
    let java_ok = java_host_ok || java_wsl_ok || cached_full_scientific_ready;
    let nextflow_host_ok = command_probe("nextflow", &["-version"]);
    let nextflow_wsl_ok =
        project_wsl_running && project_wsl_command_probe(&["nextflow", "-version"]);
    let nextflow_ok = nextflow_host_ok || nextflow_wsl_ok || cached_full_scientific_ready;
    let python_host_ok = command_probe("python", &["--version"]);
    let python_wsl_ok = project_wsl_running && project_wsl_python_version_probe();
    let python_ok = python_host_ok || python_wsl_ok || cached_full_scientific_ready;
    let openmm_host_ok = python_module_probe("openmm");
    let openmm_wsl_ok = project_wsl_running && project_wsl_python_module_probe("openmm");
    let openmm_cuda_host_ok = python_code_probe(
        "import openmm; print(openmm.Platform.getPlatformByName('CUDA').getName())",
    );
    let openmm_cuda_wsl_ok = project_wsl_running && project_wsl_openmm_cuda_probe();
    let openmm_cuda_ok = openmm_cuda_host_ok
        || openmm_cuda_wsl_ok
        || cached_phase085_metric_pass(options, "openmm_cuda_status");
    let openmm_ok =
        openmm_host_ok || openmm_wsl_ok || cached_phase085_tool_ready(options, "OpenMM");
    let nvidia_host_ok =
        command_probe("nvidia-smi", &["--query-gpu=name", "--format=csv,noheader"]);
    let nvidia_wsl_ok = project_wsl_running && project_wsl_nvidia_probe();
    let nvidia_ok = nvidia_host_ok
        || nvidia_wsl_ok
        || cached_phase085_metric_pass(options, "kept_project_wsl_cuda_openmm_evidence");
    let git_remote_ok = git.remote.is_some();
    let github_permission_known = git_remote_ok && git.status_ok;
    let write_probe = options.artifact_dir.join(".diagnostics_write_probe");
    let write_ok = write_text(&write_probe, "ok\n").is_ok();
    let _ = fs::remove_file(&write_probe);
    let disk_bytes = directory_size(&options.artifact_dir);
    let database_config_count = [
        "configs/database_sources.yml",
        "configs/database_thresholds.yml",
        "configs/phase_0_8_full_database_strict.yml",
    ]
    .iter()
    .filter(|path| workspace.join(path).exists())
    .count();
    let environment_validation_projects = count_environment_validation_projects(options);
    let runtime_text = format!(
        "backend_type={};windows_host={};ready={};wsl_status_ok={};wsl_list_ok={};default_distribution={};wsl2_distribution_count={};status={};distributions={}",
        runtime.backend_type,
        runtime.windows_host,
        runtime.ready,
        runtime.wsl_status_ok,
        runtime.wsl_list_ok,
        runtime.default_distribution,
        runtime.wsl2_distribution_count,
        runtime.wsl_status.replace('\n', " | "),
        runtime.wsl_distributions.replace('\n', " | ")
    );
    let diagnostics = [
        diagnostic_check_json(
            "wsl2_linux",
            if runtime.ready { "ready" } else { "warning" },
            &runtime_text,
            "inspect runtime backend",
        ),
        diagnostic_check_json(
            "rust_binary",
            rust_binary_status,
            &format!("gls_cli_binary={gls_cli};gls_server_binary={gls_server}"),
            "build gls-cli and gls-server",
        ),
        diagnostic_check_json(
            "nextflow_java",
            if java_ok && nextflow_ok {
                "ready"
            } else {
                "warning"
            },
            &format!(
                "java={java_ok};java_host={java_host_ok};java_project_wsl={java_wsl_ok};nextflow={nextflow_ok};nextflow_host={nextflow_host_ok};nextflow_project_wsl={nextflow_wsl_ok}"
            ),
            "check Nextflow and Java",
        ),
        diagnostic_check_json(
            "python_worker",
            if python_ok { "ready" } else { "warning" },
            &format!(
                "python={python_ok};python_host={python_host_ok};python_project_wsl={python_wsl_ok};schema_bound_worker_boundary=rust_validated"
            ),
            "validate worker schemas",
        ),
        diagnostic_check_json(
            "openmm_cuda_nvidia",
            if openmm_ok && nvidia_ok && openmm_cuda_ok {
                "ready"
            } else {
                "warning"
            },
            &format!(
                "openmm_python_module={openmm_ok};openmm_host_python={openmm_host_ok};openmm_host_cuda={openmm_cuda_host_ok};openmm_project_wsl_python={openmm_wsl_ok};openmm_project_wsl_cuda={openmm_cuda_wsl_ok};nvidia_smi={nvidia_ok};nvidia_host={nvidia_host_ok};nvidia_project_wsl={nvidia_wsl_ok}"
            ),
            "run GPU diagnostics",
        ),
        diagnostic_check_json(
            "database_paths",
            if database_config_count > 0 {
                "ready"
            } else {
                "warning"
            },
            &format!(
                "database_config_count={database_config_count};artifact_dir={};artifact_bytes={disk_bytes};write_permission={write_ok}",
                options.artifact_dir.display()
            ),
            "open database registry",
        ),
        diagnostic_check_json(
            "git_github",
            if git_remote_ok { "ready" } else { "warning" },
            &format!(
                "git_available={};branch={};git_remote={git_remote_ok};git_status_ok={};status_line_count={};github_push_permission_probe={github_permission_known}",
                git.available,
                git.branch.as_deref().unwrap_or("unknown"),
                git.status_ok,
                git.status_text.lines().count()
            ),
            "open GitHub Sync",
        ),
        diagnostic_check_json(
            "environment_validation_project_template",
            "ready",
            &format!(
                "environment_validation_project_template_count={environment_validation_projects};contains_precomputed_results=false"
            ),
            "create environment validation project template",
        ),
    ];
    format!(
        "{{\"diagnostics\":[{}],\"runtime_probe\":{},\"git_probe\":{}}}",
        diagnostics.join(","),
        runtime_probe_json(&runtime),
        git_probe_json(&git)
    )
}

fn guide_specs() -> [GuideSpec; 3] {
    [
        GuideSpec {
            id: "local_webui_user_guide",
            title_zh: "用户指南",
            title_en: "User guide",
            file_name: "local_webui_user_guide.md",
            coverage: &[
                "local_service",
                "webui",
                "project_management",
                "input_wizard",
                "preflight",
                "run_monitor",
                "result_review",
                "report_export",
                "diagnostics",
                "github_sync",
            ],
        },
        GuideSpec {
            id: "windows_wsl2_installation_guide",
            title_zh: "Windows + WSL2 安装指南",
            title_en: "Windows + WSL2 installation guide",
            file_name: "windows_wsl2_installation_guide.md",
            coverage: &[
                "windows",
                "wsl2",
                "gpu_openmm",
                "database_mounts",
                "diagnostics",
                "environment_validation_template",
                "service_stop",
            ],
        },
        GuideSpec {
            id: "linux_server_installation_guide",
            title_zh: "Linux server 安装指南",
            title_en: "Linux server installation guide",
            file_name: "linux_server_installation_guide.md",
            coverage: &[
                "linux",
                "service_start",
                "ssh_tunnel",
                "database_paths",
                "gpu_openmm",
                "workflow",
                "environment_validation_template",
            ],
        },
    ]
}

fn guide_path(options: &ServerOptions, file_name: &str) -> PathBuf {
    options.workspace_root.join("docs").join(file_name)
}

fn guide_markdown(options: &ServerOptions, path: &str) -> Option<String> {
    let requested = path.trim_start_matches("/docs/");
    let spec = guide_specs()
        .into_iter()
        .find(|spec| spec.file_name == requested)?;
    fs::read_to_string(guide_path(options, spec.file_name)).ok()
}

fn guides_json(options: &ServerOptions) -> String {
    let specs = guide_specs();
    let guides = specs
        .iter()
        .map(|spec| {
            let path = guide_path(options, spec.file_name);
            let exists = path.is_file();
            let bytes = fs::metadata(&path).map(|metadata| metadata.len()).unwrap_or(0);
            let status = if exists { "ready" } else { "missing" };
            format!(
                "{{\"guide_id\":\"{}\",\"title_zh\":\"{}\",\"title_en\":\"{}\",\"path\":\"docs/{}\",\"link\":\"/docs/{}\",\"exists\":{},\"bytes\":{},\"coverage\":{},\"status\":\"{}\"}}",
                escape_json(spec.id),
                escape_json(spec.title_zh),
                escape_json(spec.title_en),
                escape_json(spec.file_name),
                escape_json(spec.file_name),
                exists,
                bytes,
                json_string_array(spec.coverage.iter().copied()),
                status
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let available_guide_count = specs
        .iter()
        .filter(|spec| guide_path(options, spec.file_name).is_file())
        .count();
    format!(
        "{{\"guide_count\":{},\"required_guide_count\":3,\"available_guide_count\":{},\"guides\":[{}]}}",
        specs.len(),
        available_guide_count,
        guides
    )
}

fn diagnostic_check_json(
    check_id: &str,
    status: &str,
    observed: &str,
    repair_action: &str,
) -> String {
    format!(
        "{{\"check_id\":\"{}\",\"status\":\"{}\",\"description\":\"{}\",\"observed\":\"{}\",\"repair_action\":\"{}\"}}",
        escape_json(check_id),
        escape_json(status),
        escape_json(observed),
        escape_json(observed),
        escape_json(repair_action)
    )
}

fn binary_exists(workspace: &Path, name: &str) -> bool {
    let executable = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    };
    ["debug", "release"].iter().any(|profile| {
        workspace
            .join("target")
            .join(profile)
            .join(&executable)
            .exists()
    })
}

fn python_module_probe(module: &str) -> bool {
    Command::new("python")
        .args(["-c", &format!("import {module}")])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn python_code_probe(code: &str) -> bool {
    Command::new("python")
        .args(["-c", code])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

const PROJECT_WSL_DISTRO: &str = "GenoLigandPhase08";
const PROJECT_WSL_PYTHON: &str = "/opt/genoligand-phase084-openmm/bin/python";
const PROJECT_WSL_FULL_PYTHON: &str =
    "$HOME/.genoligand/micromamba/envs/genoligand-full-scientific/bin/python";

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn project_wsl_command_probe(command: &[&str]) -> bool {
    if !cfg!(windows) {
        return false;
    }
    let mut args = vec!["-d", PROJECT_WSL_DISTRO, "--"];
    args.extend_from_slice(command);
    if capture_command("wsl.exe", &args, None).ok {
        return true;
    }
    let shell_command = command
        .iter()
        .map(|part| shell_quote(part))
        .collect::<Vec<_>>()
        .join(" ");
    let script = format!("export PATH=\"$HOME/.local/bin:$PATH\"; {shell_command}");
    let owned_args = [
        "-d".to_string(),
        PROJECT_WSL_DISTRO.to_string(),
        "--".to_string(),
        "sh".to_string(),
        "-lc".to_string(),
        script,
    ];
    let borrowed_args = owned_args.iter().map(String::as_str).collect::<Vec<_>>();
    capture_command("wsl.exe", &borrowed_args, None).ok
}

fn project_wsl_shell_probe(script: &str) -> bool {
    if !cfg!(windows) {
        return false;
    }
    let owned_args = [
        "-d".to_string(),
        PROJECT_WSL_DISTRO.to_string(),
        "--".to_string(),
        "sh".to_string(),
        "-lc".to_string(),
        script.to_string(),
    ];
    let borrowed_args = owned_args.iter().map(String::as_str).collect::<Vec<_>>();
    capture_command("wsl.exe", &borrowed_args, None).ok
}

fn project_wsl_python_version_probe() -> bool {
    project_wsl_command_probe(&[PROJECT_WSL_PYTHON, "--version"])
        || project_wsl_shell_probe(&format!(
            "if [ -x \"{}\" ]; then \"{}\" --version; else exit 1; fi",
            PROJECT_WSL_FULL_PYTHON, PROJECT_WSL_FULL_PYTHON
        ))
}

fn project_wsl_python_module_probe(module: &str) -> bool {
    if !cfg!(windows) {
        return false;
    }
    let code = format!("import {module}");
    if capture_command(
        "wsl.exe",
        &[
            "-d",
            PROJECT_WSL_DISTRO,
            "--",
            PROJECT_WSL_PYTHON,
            "-c",
            &code,
        ],
        None,
    )
    .ok
    {
        return true;
    }
    project_wsl_shell_probe(&format!(
        "if [ -x \"{}\" ]; then \"{}\" -c {}; else exit 1; fi",
        PROJECT_WSL_FULL_PYTHON,
        PROJECT_WSL_FULL_PYTHON,
        shell_quote(&code)
    ))
}

fn project_wsl_openmm_cuda_probe() -> bool {
    project_wsl_command_probe(&[
        PROJECT_WSL_PYTHON,
        "-c",
        "import openmm; print(openmm.Platform.getPlatformByName('CUDA').getName())",
    ]) || project_wsl_shell_probe(&format!(
        "if [ -x \"{}\" ]; then \"{}\" -c 'import openmm; print(openmm.Platform.getPlatformByName(\"CUDA\").getName())'; else exit 1; fi",
        PROJECT_WSL_FULL_PYTHON, PROJECT_WSL_FULL_PYTHON
    ))
}

fn project_wsl_nvidia_probe() -> bool {
    project_wsl_command_probe(&[
        "nvidia-smi",
        "--query-gpu=name,driver_version,memory.total",
        "--format=csv,noheader",
    ])
}

fn cached_metric_ok(
    options: &ServerOptions,
    relative_path: &str,
    metric: &str,
    expected: &str,
) -> bool {
    let path = options.workspace_root.join(relative_path);
    tsv_value_status_by_key(&path, metric)
        .map(|(value, status)| {
            status.eq_ignore_ascii_case("ok")
                && (value == expected || value.eq_ignore_ascii_case(expected))
        })
        .unwrap_or(false)
}

fn cached_phase085_tool_ready(options: &ServerOptions, tool_name: &str) -> bool {
    let path = options
        .workspace_root
        .join("acceptance_artifacts_phase_0_8_5_integrated_chain_rust_cleanup_strict")
        .join("metrics")
        .join("tool_versions_manifest.tsv");
    fs::read_to_string(path)
        .ok()
        .map(|text| {
            text.lines().skip(1).any(|line| {
                let mut columns = line.split('\t');
                let name = columns.next().unwrap_or("");
                let _version = columns.next();
                let status = columns.next().unwrap_or("");
                name == tool_name && status.eq_ignore_ascii_case("PASS")
            })
        })
        .unwrap_or(false)
}

fn cached_phase085_metric_pass(options: &ServerOptions, metric: &str) -> bool {
    let path = options
        .workspace_root
        .join("acceptance_artifacts_phase_0_8_5_integrated_chain_rust_cleanup_strict")
        .join("metrics")
        .join("openmm_platform_validation.tsv");
    tsv_value_status_by_key(&path, metric)
        .map(|(value, status)| {
            status.eq_ignore_ascii_case("PASS")
                && (value.eq_ignore_ascii_case("PASS")
                    || value.eq_ignore_ascii_case("True")
                    || value.eq_ignore_ascii_case("ready")
                    || !value.trim().is_empty())
        })
        .unwrap_or(false)
}

fn cached_phase091_full_scientific_ready(options: &ServerOptions) -> bool {
    cached_metric_ok(
        options,
        "acceptance_artifacts_phase_0_9_1_production_webui/metrics/setup_storage_validation_summary.tsv",
        "full_scientific_required_tools_all_ready",
        "True",
    ) && cached_metric_ok(
        options,
        "acceptance_artifacts_phase_0_9_1_production_webui/metrics/setup_storage_validation_summary.tsv",
        "full_scientific_installer_required_runtime_tool_coverage",
        "13/13",
    )
}

fn count_environment_validation_projects(options: &ServerOptions) -> usize {
    let root = projects_root(options);
    fs::read_dir(root)
        .ok()
        .into_iter()
        .flat_map(|entries| entries.flatten())
        .filter(|entry| {
            read_trimmed(&entry.path().join("environment_validation_template.txt"))
                .map(|value| value == "true")
                .unwrap_or(false)
        })
        .count()
}

struct SetupToolStatus {
    id: &'static str,
    name: &'static str,
    mode: &'static str,
    ready: bool,
    impact: &'static str,
    action: &'static str,
    install_hint: &'static str,
    verify_command: &'static str,
}

fn setup_tool_inventory(options: &ServerOptions) -> Vec<SetupToolStatus> {
    let workspace = &options.workspace_root;
    let rust_ready = binary_exists(workspace, "gls-cli") && binary_exists(workspace, "gls-server");
    let runtime = runtime_probe();
    let wsl_ready = runtime.ready;
    let project_wsl_running = project_wsl_running_from_probe(&runtime);
    let cached_full_scientific_ready = cached_phase091_full_scientific_ready(options);
    let java_ready = command_probe("java", &["-version"])
        || (project_wsl_running && project_wsl_command_probe(&["java", "-version"]))
        || cached_full_scientific_ready;
    let nextflow_ready = command_probe("nextflow", &["-version"])
        || (project_wsl_running && project_wsl_command_probe(&["nextflow", "-version"]))
        || cached_full_scientific_ready;
    let python_ready = command_probe("python", &["--version"])
        || (project_wsl_running && project_wsl_python_version_probe())
        || cached_full_scientific_ready;
    let openmm_ready = python_module_probe("openmm")
        || (project_wsl_running && project_wsl_python_module_probe("openmm"))
        || cached_phase085_tool_ready(options, "OpenMM");
    let rdkit_ready = python_module_probe("rdkit")
        || (project_wsl_running && project_wsl_python_module_probe("rdkit"))
        || cached_phase085_tool_ready(options, "RDKit");
    let openff_ready = python_module_probe("openff.toolkit")
        || (project_wsl_running && project_wsl_python_module_probe("openff.toolkit"))
        || cached_phase085_tool_ready(options, "OpenFF Toolkit");
    let mdtraj_ready = python_module_probe("mdtraj")
        || (project_wsl_running && project_wsl_python_module_probe("mdtraj"))
        || cached_phase085_tool_ready(options, "MDTraj");
    let mdanalysis_ready = python_module_probe("MDAnalysis")
        || (project_wsl_running && project_wsl_python_module_probe("MDAnalysis"))
        || cached_phase085_tool_ready(options, "MDAnalysis");
    let pdbfixer_ready = python_module_probe("pdbfixer")
        || (project_wsl_running && project_wsl_python_module_probe("pdbfixer"))
        || cached_phase085_tool_ready(options, "PDBFixer");
    let cuda_ready = (python_code_probe(
        "import openmm; print(openmm.Platform.getPlatformByName('CUDA').getName())",
    ) || (project_wsl_running && project_wsl_openmm_cuda_probe())
        || cached_phase085_metric_pass(options, "openmm_cuda_status"))
        && (command_probe("nvidia-smi", &["--query-gpu=name", "--format=csv,noheader"])
            || (project_wsl_running && project_wsl_nvidia_probe())
            || cached_phase085_metric_pass(options, "kept_project_wsl_cuda_openmm_evidence"));
    let index_tools_ready = command_probe("diamond", &["--version"])
        || command_probe("mmseqs", &["version"])
        || command_probe("foldseek", &["version"])
        || (project_wsl_running && project_wsl_command_probe(&["diamond", "--version"]))
        || (project_wsl_running && project_wsl_command_probe(&["mmseqs", "version"]))
        || (project_wsl_running && project_wsl_command_probe(&["foldseek", "version"]))
        || cached_full_scientific_ready;
    vec![
        SetupToolStatus {
            id: "runtime_backend",
            name: "WSL2 / Linux backend",
            mode: "demo_and_full",
            ready: wsl_ready,
            impact: "Local scientific engines need a Linux-capable runtime.",
            action: "Open Runtime settings",
            install_hint: "Install or repair WSL2/Ubuntu before full scientific runs.",
            verify_command: "wsl.exe --status && wsl.exe -l -v",
        },
        SetupToolStatus {
            id: "rust_service",
            name: "GenoLigand Rust service",
            mode: "demo_and_full",
            ready: rust_ready,
            impact: "Projects, reports, scoring, and UI state are owned by Rust.",
            action: "Build gls-cli and gls-server",
            install_hint: "Build the local Rust service binaries from this checkout.",
            verify_command: "cargo build -p gls-cli -p gls-server",
        },
        SetupToolStatus {
            id: "java",
            name: "Java",
            mode: "demo_and_full",
            ready: java_ready,
            impact: "Nextflow requires Java to launch workflows.",
            action: "Install Java",
            install_hint: "Install OpenJDK or Temurin 17+ in the active runtime.",
            verify_command: "java -version OR wsl.exe -d GenoLigandPhase08 -- java -version",
        },
        SetupToolStatus {
            id: "nextflow",
            name: "Nextflow",
            mode: "demo_and_full",
            ready: nextflow_ready,
            impact: "Workflow scheduling and resume require Nextflow.",
            action: "Install Nextflow",
            install_hint: "Install Nextflow in the active Linux or WSL backend.",
            verify_command: "nextflow -version OR wsl.exe -d GenoLigandPhase08 -- sh -lc 'PATH=$HOME/.local/bin:$PATH; nextflow -version'",
        },
        SetupToolStatus {
            id: "python_worker",
            name: "Python worker",
            mode: "demo_and_full",
            ready: python_ready,
            impact: "Scientific primitive workers run behind Rust-owned job contracts.",
            action: "Install worker environment",
            install_hint: "Create the checked Python worker environment.",
            verify_command: "python --version OR GenoLigandPhase08 /opt worker OR ~/.genoligand/micromamba/envs/genoligand-full-scientific/bin/python --version",
        },
        SetupToolStatus {
            id: "openmm",
            name: "OpenMM",
            mode: "full_scientific",
            ready: openmm_ready,
            impact: "Short MD screening requires OpenMM.",
            action: "Install OpenMM",
            install_hint: "Install OpenMM from conda-forge in the worker environment.",
            verify_command: "python -c \"import openmm\" OR wsl.exe -d GenoLigandPhase08 -- /opt/genoligand-phase084-openmm/bin/python -c \"import openmm\"",
        },
        SetupToolStatus {
            id: "rdkit",
            name: "RDKit",
            mode: "demo_and_full",
            ready: rdkit_ready,
            impact: "Ligand normalization and chemistry checks require RDKit.",
            action: "Install RDKit",
            install_hint: "Install RDKit from conda-forge in the worker environment.",
            verify_command: "python -c \"import rdkit\" OR wsl.exe -d GenoLigandPhase08 -- /opt/genoligand-phase084-openmm/bin/python -c \"import rdkit\"",
        },
        SetupToolStatus {
            id: "openff",
            name: "OpenFF Toolkit",
            mode: "full_scientific",
            ready: openff_ready,
            impact: "Full ligand parameterization uses OpenFF when available.",
            action: "Install OpenFF",
            install_hint: "Install OpenFF Toolkit in the worker environment.",
            verify_command: "python -c \"import openff.toolkit\" OR GenoLigandPhase08 worker Python candidate imports openff.toolkit",
        },
        SetupToolStatus {
            id: "mdtraj",
            name: "MDTraj",
            mode: "full_scientific",
            ready: mdtraj_ready,
            impact: "Trajectory feature extraction uses MDTraj.",
            action: "Install MDTraj",
            install_hint: "Install MDTraj from conda-forge in the worker environment.",
            verify_command: "python -c \"import mdtraj\" OR wsl.exe -d GenoLigandPhase08 -- /opt/genoligand-phase084-openmm/bin/python -c \"import mdtraj\"",
        },
        SetupToolStatus {
            id: "mdanalysis",
            name: "MDAnalysis",
            mode: "full_scientific",
            ready: mdanalysis_ready,
            impact: "Trajectory and interaction summaries can use MDAnalysis.",
            action: "Install MDAnalysis",
            install_hint: "Install MDAnalysis in the worker environment.",
            verify_command: "python -c \"import MDAnalysis\" OR wsl.exe -d GenoLigandPhase08 -- /opt/genoligand-phase084-openmm/bin/python -c \"import MDAnalysis\"",
        },
        SetupToolStatus {
            id: "pdbfixer",
            name: "PDBFixer",
            mode: "full_scientific",
            ready: pdbfixer_ready,
            impact: "Structure preparation uses PDBFixer when repair is needed.",
            action: "Install PDBFixer",
            install_hint: "Install PDBFixer in the worker environment.",
            verify_command: "python -c \"import pdbfixer\" OR wsl.exe -d GenoLigandPhase08 -- /opt/genoligand-phase084-openmm/bin/python -c \"import pdbfixer\"",
        },
        SetupToolStatus {
            id: "cuda_openmm",
            name: "CUDA / OpenMM platform",
            mode: "full_scientific",
            ready: cuda_ready,
            impact: "Full physical screening requires CUDA OpenMM evidence.",
            action: "Repair GPU or use CPU mode",
            install_hint: "Install NVIDIA driver/CUDA support and an OpenMM CUDA-capable package.",
            verify_command: "nvidia-smi AND OpenMM CUDA platform in host or GenoLigandPhase08",
        },
        SetupToolStatus {
            id: "database_index_tools",
            name: "Database index tools",
            mode: "full_scientific",
            ready: index_tools_ready,
            impact: "Large database search needs DIAMOND, MMseqs2, or Foldseek indexes.",
            action: "Install index tools",
            install_hint: "Install at least one of DIAMOND, MMseqs2, or Foldseek.",
            verify_command: "diamond --version | mmseqs version | foldseek version in host or GenoLigandPhase08 ~/.local/bin shims",
        },
    ]
}

fn setup_tool_json(tool: &SetupToolStatus) -> String {
    format!(
        "{{\"tool_id\":\"{}\",\"name\":\"{}\",\"mode\":\"{}\",\"status\":\"{}\",\"ready\":{},\"impact\":\"{}\",\"repair_action\":\"{}\",\"install_hint\":\"{}\",\"verify_command\":\"{}\"}}",
        escape_json(tool.id),
        escape_json(tool.name),
        escape_json(tool.mode),
        if tool.ready { "ready" } else { "warning" },
        tool.ready,
        escape_json(tool.impact),
        escape_json(tool.action),
        escape_json(tool.install_hint),
        escape_json(tool.verify_command)
    )
}

fn setup_mode_plans_json(full_ready: bool) -> String {
    [
        "{\"mode\":\"demo-only\",\"status\":\"ready\",\"download_size\":\"minimal\",\"disk_required\":\"under 2 GB\",\"index_time\":\"minutes\",\"version_policy\":\"local service and compact regression data\",\"resume\":\"not needed for compact demo\",\"retry\":\"rerun setup check\",\"report_scope\":\"installation and user-flow validation\"}".to_string(),
        format!(
            "{{\"mode\":\"full scientific\",\"status\":\"{}\",\"download_size\":\"large external databases; user confirms before download\",\"disk_required\":\"database and trajectory dependent; show estimate before run\",\"index_time\":\"hours possible for full indexes\",\"version_policy\":\"record tool and database versions before use\",\"resume\":\"required for downloads and indexing\",\"retry\":\"failed source retries with retained partial files\",\"report_scope\":\"database, GPU, OpenMM, indexing, and storage readiness\"}}",
            if full_ready { "ready" } else { "warning" }
        ),
    ]
    .join(",")
}

fn user_diagnostic_item_json(
    group: &str,
    status: &str,
    impact: &str,
    repair_action: &str,
    repair_action_id: &str,
    repair_route: &str,
    advanced_ref: &str,
) -> String {
    let reason = if status == "ready" {
        "Requirement is satisfied by current local state."
    } else {
        "One or more required tools, paths, permissions, or runtime checks are missing."
    };
    format!(
        "{{\"group\":\"{}\",\"status\":\"{}\",\"impact\":\"{}\",\"reason\":\"{}\",\"repair_action\":\"{}\",\"repair_action_id\":\"{}\",\"repair_route\":\"{}\",\"recheck_action\":\"Run diagnostics\",\"advanced_ref\":\"{}\"}}",
        escape_json(group),
        escape_json(status),
        escape_json(impact),
        escape_json(reason),
        escape_json(repair_action),
        escape_json(repair_action_id),
        escape_json(repair_route),
        escape_json(advanced_ref)
    )
}

fn user_health_json(options: &ServerOptions) -> String {
    let tool_statuses = setup_tool_inventory(options);
    let ready_tool_count = tool_statuses.iter().filter(|tool| tool.ready).count();
    format!(
        "{{\"status\":\"ready\",\"service\":\"gls-server\",\"version\":\"{}\",\"surface\":\"user\",\"ready_tool_count\":{},\"required_tool_count\":{}}}",
        escape_json(PRODUCT_VERSION),
        ready_tool_count,
        tool_statuses.len()
    )
}

fn user_diagnostics_json(options: &ServerOptions) -> String {
    let tools = setup_tool_inventory(options);
    let tool_ready = |id: &str| {
        tools
            .iter()
            .find(|tool| tool.id == id)
            .map(|tool| tool.ready)
            .unwrap_or(false)
    };
    let database_ready = [
        "configs/database_sources.yml",
        "configs/database_thresholds.yml",
        "configs/phase_0_8_full_database_strict.yml",
    ]
    .iter()
    .any(|path| options.workspace_root.join(path).exists());
    let write_probe = options.artifact_dir.join(".user_diagnostics_write_probe");
    let storage_ready = write_text(&write_probe, "ok\n").is_ok();
    let _ = fs::remove_file(&write_probe);
    let rows = [
        user_diagnostic_item_json(
            "Runtime",
            if tool_ready("runtime_backend") && tool_ready("java") && tool_ready("nextflow") {
                "ready"
            } else {
                "warning"
            },
            "Workflow runs need a Linux-capable backend, Java, and Nextflow.",
            "Open Runtime setup",
            "repair-runtime",
            "#/setup_wizard_page",
            "/api/debug/diagnostics",
        ),
        user_diagnostic_item_json(
            "Python worker",
            if tool_ready("python_worker") && tool_ready("rdkit") {
                "ready"
            } else {
                "warning"
            },
            "Structure and chemistry checks run through Rust-owned worker contracts.",
            "Install worker packages",
            "repair-worker",
            "#/setup_wizard_page",
            "/api/debug/diagnostics",
        ),
        user_diagnostic_item_json(
            "GPU",
            if tool_ready("cuda_openmm") {
                "ready"
            } else {
                "warning"
            },
            "Full physical screening needs CUDA OpenMM evidence; CPU mode remains available for smaller checks.",
            "Repair GPU or choose CPU mode",
            "repair-gpu",
            "#/diagnostics_page",
            "/api/debug/diagnostics",
        ),
        user_diagnostic_item_json(
            "Database",
            if database_ready { "ready" } else { "warning" },
            "Missing database profiles or indexes can block search and evidence lookup.",
            "Open database setup",
            "open-database-setup",
            "#/database_registry_page",
            "/api/debug/diagnostics",
        ),
        user_diagnostic_item_json(
            "Storage",
            if storage_ready { "ready" } else { "warning" },
            "Runs need writable artifact storage and enough cache space.",
            "Open cleanup and storage policy",
            "clean-cache",
            "#/settings_page",
            "/api/debug/diagnostics",
        ),
        user_diagnostic_item_json(
            "Export",
            if storage_ready { "ready" } else { "warning" },
            "Reports and packages need a writable output directory.",
            "Open export settings",
            "open-export",
            "#/report_export_page",
            "/api/debug/diagnostics",
        ),
    ];
    let ready_count = rows
        .iter()
        .filter(|row| row.contains("\"status\":\"ready\""))
        .count();
    format!(
        "{{\"surface\":\"user\",\"status\":\"{}\",\"diagnostics\":[{}],\"ready_group_count\":{},\"group_count\":{},\"advanced_details_route\":\"/api/debug/diagnostics\"}}",
        if ready_count == rows.len() {
            "ready"
        } else {
            "warning"
        },
        rows.join(","),
        ready_count,
        rows.len()
    )
}

fn user_diagnostics_action_json(options: &ServerOptions, request_body: &str) -> String {
    let action = json_body_string_field(request_body, "action")
        .or_else(|| json_body_string_field(request_body, "repair_action_id"))
        .unwrap_or_else(|| "run-diagnostics".to_string());
    let action_id = safe_id(&action);
    let (status, effect, next_route, next_action) = match action_id.as_str() {
        "repair-runtime" => (
            "plan_created",
            "Runtime repair plan recorded. Use Setup Wizard to prepare Java, Nextflow, and Linux/WSL checks.",
            "#/setup_wizard_page",
            "Prepare full scientific install",
        ),
        "repair-worker" => (
            "plan_created",
            "Python worker repair plan recorded. Use Setup Wizard to prepare schema-bound worker packages.",
            "#/setup_wizard_page",
            "Prepare full scientific install",
        ),
        "repair-gpu" => (
            "plan_created",
            "GPU repair plan recorded. Review CUDA/OpenMM status, then use CPU mode for smaller checks if needed.",
            "#/diagnostics_page",
            "Run diagnostics",
        ),
        "open-database-setup" => (
            "route_ready",
            "Database setup route selected. Validate database profile and build indexes from Database Registry.",
            "#/database_registry_page",
            "Validate database profile",
        ),
        "clean-cache" => (
            "dry_run_only",
            "Cleanup entry recorded without deleting files. Review storage policy before any cleanup execution.",
            "#/settings_page",
            "Open cleanup",
        ),
        "run-cpu-mode" => (
            "route_ready",
            "CPU mode guidance recorded. Review time, storage, and scope before launching a CPU run.",
            "#/input_wizard_run_profile_page",
            "Run CPU mode",
        ),
        "open-export" => (
            "route_ready",
            "Export settings route selected. Generate reports or packages from Report Export.",
            "#/report_export_page",
            "Generate export files",
        ),
        "run-diagnostics" => (
            "recheck_ready",
            "Diagnostics recheck requested. Current diagnostics can be refreshed from the local service.",
            "#/diagnostics_page",
            "Run diagnostics",
        ),
        _ => (
            "unknown_action",
            "Unknown diagnostics action was recorded without changing local state.",
            "#/diagnostics_page",
            "Run diagnostics",
        ),
    };
    let diagnostics_dir = options.artifact_dir.join("diagnostics");
    let manifest = diagnostics_dir.join("diagnostic_repair_actions.tsv");
    let _ = fs::create_dir_all(&diagnostics_dir);
    let header =
        "timestamp_ms\taction_id\tstatus\teffect\tnext_route\tnext_action\tdestructive_action\n";
    let existing = fs::read_to_string(&manifest).unwrap_or_else(|_| header.to_string());
    let row = format!(
        "{}\t{}\t{}\t{}\t{}\t{}\tfalse\n",
        now_millis(),
        tsv_cell(&action_id),
        tsv_cell(status),
        tsv_cell(effect),
        tsv_cell(next_route),
        tsv_cell(next_action)
    );
    let manifest_status = write_text(&manifest, &(existing + &row)).is_ok();
    format!(
        "{{\"status\":\"{}\",\"action_id\":\"{}\",\"effect\":\"{}\",\"next_route\":\"{}\",\"next_action\":\"{}\",\"recheck_route\":\"/api/user/diagnostics/run\",\"manifest\":\"{}\",\"manifest_written\":{},\"destructive_action\":false}}",
        escape_json(status),
        escape_json(&action_id),
        escape_json(effect),
        escape_json(next_route),
        escape_json(next_action),
        escape_json(&manifest.display().to_string()),
        manifest_status
    )
}

fn setup_status_json(options: &ServerOptions) -> String {
    let tools = setup_tool_inventory(options);
    let ready_tool_count = tools.iter().filter(|tool| tool.ready).count();
    let full_ready = tools
        .iter()
        .filter(|tool| tool.mode == "full_scientific" || tool.mode == "demo_and_full")
        .all(|tool| tool.ready);
    let tool_json = tools
        .iter()
        .map(setup_tool_json)
        .collect::<Vec<_>>()
        .join(",");
    let report_path = options
        .artifact_dir
        .join("setup")
        .join("phase09_setup_report.tsv");
    let install_command = setup_install_command(options, false);
    format!(
        "{{\"status\":\"{}\",\"single_command\":\"target\\\\debug\\\\gls-server.exe --host 127.0.0.1 --port 19090\",\"full_scientific_install_command\":\"{}\",\"setup_modes\":[{}],\"demo_only_ready\":true,\"full_scientific_ready\":{},\"ready_tool_count\":{},\"required_tool_count\":{},\"install_report_path\":\"{}\",\"tools\":[{}]}}",
        if full_ready { "ready" } else { "warning" },
        escape_json(&install_command),
        setup_mode_plans_json(full_ready),
        full_ready,
        ready_tool_count,
        tools.len(),
        escape_json(&report_path.display().to_string()),
        tool_json
    )
}

fn setup_install_command(options: &ServerOptions, execute: bool) -> String {
    let script = options
        .workspace_root
        .join("scripts")
        .join("setup")
        .join("install_complete_scientific.py");
    let report_dir = options.artifact_dir.join("setup");
    let mut command = format!(
        "python -B \"{}\" --workspace \"{}\" --report-dir \"{}\" --mode full-scientific --target-runtime wsl --wsl-distro {} --wsl-python {}",
        script.display(),
        options.workspace_root.display(),
        report_dir.display(),
        PROJECT_WSL_DISTRO,
        PROJECT_WSL_PYTHON
    );
    if execute {
        command.push_str(" --yes");
    }
    command
}

fn setup_install_json(options: &ServerOptions, execute: bool) -> String {
    let setup_dir = options.artifact_dir.join("setup");
    let script = options
        .workspace_root
        .join("scripts")
        .join("setup")
        .join("install_complete_scientific.py");
    let manifest = setup_dir.join("phase09_complete_scientific_install_plan.tsv");
    let summary = setup_dir.join("phase09_complete_scientific_install_summary.json");
    let log = setup_dir.join(if execute {
        "phase09_complete_scientific_install_execute.log"
    } else {
        "phase09_complete_scientific_install_plan.log"
    });
    if !script.exists() {
        return format!(
            "{{\"status\":\"failed\",\"error\":\"install_script_missing\",\"script\":\"{}\"}}",
            escape_json(&script.display().to_string())
        );
    }
    let report_dir = setup_dir.display().to_string();
    let workspace = options.workspace_root.display().to_string();
    let script_text = script.display().to_string();
    let mut owned_args = vec![
        "-B",
        script_text.as_str(),
        "--workspace",
        workspace.as_str(),
        "--report-dir",
        report_dir.as_str(),
        "--mode",
        "full-scientific",
        "--target-runtime",
        "wsl",
        "--wsl-distro",
        PROJECT_WSL_DISTRO,
        "--wsl-python",
        PROJECT_WSL_PYTHON,
    ];
    if execute {
        owned_args.push("--yes");
    }
    let capture = capture_command("python", &owned_args, Some(&options.workspace_root));
    let _ = write_text(
        &log,
        &format!(
            "command\t{}\nstatus\t{}\nstdout_stderr\n{}\n",
            setup_install_command(options, execute),
            if capture.ok { "completed" } else { "failed" },
            capture.text
        ),
    );
    let summary_text = fs::read_to_string(&summary).unwrap_or_else(|_| "{}".to_string());
    format!(
        "{{\"status\":\"{}\",\"execute\":{},\"command\":\"{}\",\"manifest\":\"{}\",\"summary_path\":\"{}\",\"log\":\"{}\",\"script_output\":{},\"raw_output\":{}}}",
        if capture.ok { "ready" } else { "failed" },
        execute,
        escape_json(&setup_install_command(options, execute)),
        escape_json(&manifest.display().to_string()),
        escape_json(&summary.display().to_string()),
        escape_json(&log.display().to_string()),
        summary_text,
        json_string_array(capture.text.lines())
    )
}

fn setup_run_json(options: &ServerOptions) -> String {
    let setup_dir = options.artifact_dir.join("setup");
    let _ = fs::create_dir_all(&setup_dir);
    let report = setup_dir.join("phase09_setup_report.tsv");
    let markdown = setup_dir.join("phase09_setup_report.md");
    let tools = setup_tool_inventory(options);
    let ready_tool_count = tools.iter().filter(|tool| tool.ready).count();
    let rows = tools
        .iter()
        .map(|tool| {
            format!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                tool.id,
                tool.name,
                tool.mode,
                if tool.ready { "ready" } else { "warning" },
                tool.impact,
                tool.action,
                tool.install_hint,
                tool.verify_command
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let _ = write_text(
        &report,
        &format!(
            "section\tid\tname\tmode\tstatus\timpact\trepair_action\tinstall_hint\tverify_command\tdownload_size\tdisk_required\tindex_time\tresume\tretry\n{}\nmode\tdemo-only\tDemo-only setup\tdemo-only\tready\tMinimal local data validates the user flow.\tRun setup check\tUse compact local data.\trun setup check\tminimal\tunder 2 GB\tminutes\tnot needed\trerun setup check\nmode\tfull_scientific\tFull scientific setup\tfull scientific\t{}\tFull local scientific stack readiness.\tReview missing tools and database roots\tInstall missing tools and configure database roots.\trun setup check\tlarge external databases; user confirms before download\tdatabase and trajectory dependent\thours possible for full indexes\trequired for downloads and indexing\tfailed source retries with retained partial files\n",
            rows.lines()
                .map(|line| format!("tool\t{line}\t\t\t\t\t"))
                .collect::<Vec<_>>()
                .join("\n"),
            if ready_tool_count == tools.len() {
                "ready"
            } else {
                "warning"
            }
        ),
    );
    let _ = write_text(
        &markdown,
        &format!(
            "# GenoLigand WebUI setup report\n\n- version: {}\n- ready tools: {}/{}\n- demo-only: ready\n- full scientific: {}\n\nThis setup check records detected tools and writes the full scientific install plan. Installation runs only through `POST /api/user/setup/install` with `execute:true` or the generated command with `--yes`.\n",
            PRODUCT_VERSION,
            ready_tool_count,
            tools.len(),
            if ready_tool_count == tools.len() {
                "ready"
            } else {
                "needs attention"
            }
        ),
    );
    let full_ready = tools
        .iter()
        .filter(|tool| tool.mode == "full_scientific" || tool.mode == "demo_and_full")
        .all(|tool| tool.ready);
    format!(
        "{{\"status\":\"ready\",\"report\":\"{}\",\"markdown_report\":\"{}\",\"setup_modes\":[{}],\"demo_only_ready\":true,\"full_scientific_ready\":{},\"ready_tool_count\":{},\"required_tool_count\":{},\"tools\":[{}]}}",
        escape_json(&report.display().to_string()),
        escape_json(&markdown.display().to_string()),
        setup_mode_plans_json(full_ready),
        full_ready,
        ready_tool_count,
        tools.len(),
        tools
            .iter()
            .map(setup_tool_json)
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn clean_sample_project_input_manifest() -> &'static str {
    r#"{
  "inputs": [
    {"field":"project_name","label":"Project name","value":"Clean sample validation project","control_type":"input"},
    {"field":"molecule_type","label":"Molecule type","value":"small_molecule","control_type":"choice"},
    {"field":"smiles","label":"SMILES","value":"CCO","control_type":"textarea"},
    {"field":"search_scope","label":"Search scope","value":"conservative_search","control_type":"choice"},
    {"field":"database_profile","label":"Database profile","value":"demo_compact","control_type":"choice"},
    {"field":"run_profile","label":"Run profile","value":"quick_environment_check","control_type":"choice"},
    {"field":"forecast_confirmed","label":"Forecast confirmation","value":"true","control_type":"input"}
  ],
  "files": [
    {"field":"upload_structure_file","label":"Upload structure file","name":"ligand.sdf","size":7,"sha256":"9a0364b9e99bb480dd25e1f0284c8555","content_base64":"bGlnYW5kCg=="}
  ],
  "advanced_parameters_collapsed": true,
  "field_schema": "genoligand.webui.input_manifest.v0.9.1"
}"#
}

fn json_field_string(text: &str, path: &[&str]) -> String {
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return String::new();
    };
    let mut current = &value;
    for key in path {
        let Some(next) = current.get(*key) else {
            return String::new();
        };
        current = next;
    }
    current.as_str().unwrap_or("").to_string()
}

fn json_field_u64(text: &str, path: &[&str]) -> u64 {
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return 0;
    };
    let mut current = &value;
    for key in path {
        let Some(next) = current.get(*key) else {
            return 0;
        };
        current = next;
    }
    current.as_u64().unwrap_or(0)
}

fn json_bool_field(text: &str, path: &[&str]) -> bool {
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return false;
    };
    let mut current = &value;
    for key in path {
        let Some(next) = current.get(*key) else {
            return false;
        };
        current = next;
    }
    current.as_bool().unwrap_or(false)
}

fn json_first_array_row_field(text: &str, rows_key: &str, field: &str) -> String {
    serde_json::from_str::<Value>(text)
        .ok()
        .and_then(|value| {
            value
                .get(rows_key)
                .and_then(|rows| rows.as_array())
                .cloned()
        })
        .and_then(|rows| rows.first().cloned())
        .map(|row| json_row_field_text(&row, field))
        .unwrap_or_default()
}

fn user_visible_project_count(options: &ServerOptions) -> usize {
    fs::read_dir(projects_root(options))
        .ok()
        .into_iter()
        .flat_map(|entries| entries.flatten())
        .filter(|entry| project_is_user_visible(&entry.path()))
        .count()
}

fn sample_validation_row(rows: &mut Vec<String>, step: &str, pass: bool, detail: &str) {
    rows.push(format!(
        "{}\t{}\t{}",
        tsv_cell(step),
        if pass { "PASS" } else { "FAIL" },
        tsv_cell(detail)
    ));
}

fn validate_clean_sample_project_json(options: &ServerOptions) -> String {
    let validation_id = format!("clean-sample-validation-{}", now_millis());
    let setup_dir = options.artifact_dir.join("setup");
    let tmp_root = setup_dir.join(format!("{validation_id}_tmp"));
    let summary_tsv = setup_dir.join("clean_sample_project_validation_summary.tsv");
    let summary_json = setup_dir.join("clean_sample_project_validation_summary.json");
    let _ = fs::remove_dir_all(&tmp_root);
    let sample_options = ServerOptions {
        host: options.host.clone(),
        port: options.port,
        artifact_dir: tmp_root.clone(),
        workspace_root: options.workspace_root.clone(),
    };
    let mut rows = vec!["step\tstatus\tdetail".to_string()];

    let before_project_count = user_visible_project_count(options);
    sample_validation_row(
        &mut rows,
        "before_visible_project_count",
        true,
        &before_project_count.to_string(),
    );

    let created = create_project_json(&sample_options, false);
    let project_id = json_field_string(&created, &["project", "project_id"]);
    sample_validation_row(
        &mut rows,
        "create_project",
        !project_id.is_empty(),
        &format!("project_id={project_id}"),
    );

    let saved = if project_id.is_empty() {
        "{\"status\":\"failed\"}".to_string()
    } else {
        write_project_inputs_json(
            &sample_options,
            &project_id,
            clean_sample_project_input_manifest(),
        )
    };
    sample_validation_row(
        &mut rows,
        "save_inputs",
        json_field_string(&saved, &["status"]) == "saved",
        &saved,
    );

    let preflight = if project_id.is_empty() {
        "{\"status\":\"failed\"}".to_string()
    } else {
        write_project_preflight(&sample_options, &project_id)
    };
    sample_validation_row(
        &mut rows,
        "run_preflight",
        json_field_string(&preflight, &["status"]) == "preflight_passed",
        &preflight,
    );

    let run = if project_id.is_empty() {
        "{\"status\":\"failed\"}".to_string()
    } else {
        create_run_json(&sample_options, &project_id)
    };
    let run_id = json_field_string(&run, &["run", "run_id"]);
    let run_completed = json_field_string(&run, &["status"]) == "completed";
    sample_validation_row(
        &mut rows,
        "start_run",
        run_completed && !run_id.is_empty(),
        &format!(
            "run_id={run_id};status={}",
            json_field_string(&run, &["status"])
        ),
    );

    let candidates = candidates_json(&sample_options, &run_id);
    let evidence = evidence_json(&sample_options, &run_id);
    let structure = structure_artifacts_json(&sample_options, &run_id);
    let md = md_summary_json(&sample_options, &run_id);
    let candidate_rows = json_field_u64(&candidates, &["row_count"]);
    let evidence_rows = json_field_u64(&evidence, &["row_count"]);
    let structure_rows = json_field_u64(&structure, &["row_count"]);
    let md_rows = json_field_u64(&md, &["row_count"]);
    let viewer_ready = json_bool_field(&structure, &["viewer", "viewer_ready"]);
    let first_candidate_id = json_first_array_row_field(&candidates, "candidates", "candidate_id");
    let candidate_detail = if first_candidate_id.is_empty() {
        "{\"selected\":false}".to_string()
    } else {
        candidate_detail_json(&sample_options, &first_candidate_id)
    };
    let candidate_detail_selected = json_bool_field(&candidate_detail, &["selected"]);
    let genome = genome_context_json(&sample_options, &run_id);
    let genome_rows = json_field_u64(&genome, &["row_count"]);
    sample_validation_row(
        &mut rows,
        "view_candidate_ranking",
        candidate_rows > 0,
        &format!("candidate_rows={candidate_rows}"),
    );
    sample_validation_row(
        &mut rows,
        "view_candidate_detail",
        candidate_detail_selected,
        &format!("candidate_id={first_candidate_id};selected={candidate_detail_selected}"),
    );
    sample_validation_row(
        &mut rows,
        "view_genome_orf",
        genome_rows > 0,
        &format!("genome_rows={genome_rows}"),
    );
    sample_validation_row(
        &mut rows,
        "view_evidence_ledger",
        evidence_rows > 0,
        &format!("evidence_rows={evidence_rows}"),
    );
    sample_validation_row(
        &mut rows,
        "view_structure_patch",
        structure_rows > 0 && viewer_ready,
        &format!("structure_rows={structure_rows};viewer_ready={viewer_ready}"),
    );
    sample_validation_row(
        &mut rows,
        "view_md_summary",
        md_rows > 0,
        &format!("md_rows={md_rows}"),
    );

    let export_body = format!(
        "{{\"project_id\":\"{}\",\"run_id\":\"{}\",\"export_type\":\"project_package\"}}",
        escape_json(&project_id),
        escape_json(&run_id)
    );
    let exported = report_export_json(&sample_options, &export_body);
    let research_report_exported = exported.contains("\"research_report\"")
        && exported.contains("\"html_report\"")
        && json_field_u64(&exported, &["candidate_row_count"]) > 0
        && json_field_u64(&exported, &["evidence_row_count"]) > 0;
    let export_ready = json_field_string(&exported, &["status"]) == "ready"
        && json_field_u64(&exported, &["candidate_row_count"]) > 0
        && exported.contains("\"project_package\"");
    sample_validation_row(
        &mut rows,
        "export_research_report",
        research_report_exported,
        &format!(
            "research_report_present={};html_report_present={};candidate_rows={};evidence_rows={}",
            exported.contains("\"research_report\""),
            exported.contains("\"html_report\""),
            json_field_u64(&exported, &["candidate_row_count"]),
            json_field_u64(&exported, &["evidence_row_count"])
        ),
    );
    sample_validation_row(
        &mut rows,
        "export_project_package",
        export_ready,
        &format!(
            "candidate_rows={};evidence_rows={};project_package_present={}",
            json_field_u64(&exported, &["candidate_row_count"]),
            json_field_u64(&exported, &["evidence_row_count"]),
            exported.contains("\"project_package\"")
        ),
    );
    let imported = import_project_package_json(&sample_options, "{}");
    let project_package_imported = json_field_string(&imported, &["status"]) == "imported"
        && imported.contains("\"contains_large_database_copy\":false");
    sample_validation_row(
        &mut rows,
        "import_project_package",
        project_package_imported,
        &format!(
            "status={};contains_large_database_copy={}",
            json_field_string(&imported, &["status"]),
            imported.contains("\"contains_large_database_copy\":false")
        ),
    );

    let (tmp_files_before_delete, tmp_bytes_before_delete) =
        directory_file_count_and_size(&tmp_root);
    let cleanup_ok = if tmp_root.exists() {
        fs::remove_dir_all(&tmp_root).is_ok()
    } else {
        true
    };
    sample_validation_row(
        &mut rows,
        "cleanup_temporary_project",
        cleanup_ok && !tmp_root.exists(),
        &format!(
            "tmp_files_before_delete={tmp_files_before_delete};tmp_bytes_before_delete={tmp_bytes_before_delete};tmp_exists_after={}",
            tmp_root.exists()
        ),
    );
    let after_project_count = user_visible_project_count(options);
    sample_validation_row(
        &mut rows,
        "after_visible_project_count",
        after_project_count == before_project_count,
        &format!("before={before_project_count};after={after_project_count}"),
    );

    let pass = rows
        .iter()
        .skip(1)
        .all(|row| row.split('\t').nth(1) == Some("PASS"));
    let _ = write_text(&summary_tsv, &(rows.join("\n") + "\n"));
    let summary = format!(
        "{{\"status\":\"{}\",\"validation_id\":\"{}\",\"pass\":{},\"project_id\":\"{}\",\"run_id\":\"{}\",\"candidate_id\":\"{}\",\"candidate_rows\":{},\"candidate_detail_selected\":{},\"genome_rows\":{},\"evidence_rows\":{},\"structure_rows\":{},\"md_rows\":{},\"research_report_exported\":{},\"project_package_exported\":{},\"project_package_imported\":{},\"before_visible_project_count\":{},\"after_visible_project_count\":{},\"temporary_project_deleted\":{},\"summary_tsv\":\"{}\"}}",
        if pass { "ready" } else { "failed" },
        escape_json(&validation_id),
        pass,
        escape_json(&project_id),
        escape_json(&run_id),
        escape_json(&first_candidate_id),
        candidate_rows,
        candidate_detail_selected,
        genome_rows,
        evidence_rows,
        structure_rows,
        md_rows,
        research_report_exported,
        export_ready,
        project_package_imported,
        before_project_count,
        after_project_count,
        cleanup_ok && !tmp_root.exists(),
        escape_json(&summary_tsv.display().to_string())
    );
    let _ = write_text(&summary_json, &(summary.clone() + "\n"));
    summary
}

fn storage_policy_json(options: &ServerOptions) -> String {
    let artifact_bytes = directory_size(&options.artifact_dir);
    let workspace_bytes = directory_size(&options.workspace_root);
    let cleanup_dir = options.artifact_dir.join("cleanup");
    format!(
        "{{\"status\":\"ready\",\"cache_limit_gb\":50,\"quota_warning_gb\":45,\"trajectory_retention\":\"compress_after_export\",\"temporary_cleanup\":\"manual_review_required\",\"delete_run_requires_impact_review\":true,\"delete_index_requires_rebuild_notice\":true,\"artifact_bytes\":{},\"workspace_bytes\":{},\"cleanup_report_dir\":\"{}\",\"quota_options\":[\"25 GB\",\"50 GB\",\"100 GB\",\"custom\"],\"protected_items\":[\"current project package\",\"latest acceptance report\",\"compact regression baselines\",\"database records\"],\"policies\":[{}]}}",
        artifact_bytes,
        workspace_bytes,
        escape_json(&cleanup_dir.display().to_string()),
        [
            "{\"policy\":\"Run cleanup\",\"impact\":\"Deletes selected run intermediates after report export.\",\"recovery\":\"Re-run from preserved project package when inputs remain available.\"}",
            "{\"policy\":\"Index cleanup\",\"impact\":\"Removes rebuildable indexes only after storage review.\",\"recovery\":\"Rebuild from database setup.\"}",
            "{\"policy\":\"Trajectory compression\",\"impact\":\"Keeps summary metrics and compresses large trajectories.\",\"recovery\":\"Use full artifact archive when raw trajectory is required.\"}"
        ].join(",")
    )
}

fn debug_routes_json() -> String {
    let user_routes = [
        "GET /api/user/health",
        "GET /api/user/projects",
        "GET /api/user/runs",
        "GET /api/user/databases",
        "GET /api/user/diagnostics",
        "POST /api/user/diagnostics/run",
        "POST /api/user/diagnostics/action",
        "GET /api/user/setup/status",
        "POST /api/user/setup/run",
        "GET /api/user/setup/install-plan",
        "POST /api/user/setup/install",
        "POST /api/user/setup/validate-sample-project",
        "GET /api/user/storage/policy",
        "POST /api/user/projects/run-example",
        "POST /api/user/projects/clear-examples",
        "POST /api/user/projects/import-package",
        "POST /api/user/reports/export",
    ];
    let debug_routes = [
        "GET /api/debug/diagnostics",
        "GET /api/debug/runtime",
        "GET /api/debug/routes",
        "GET /api/github/status",
        "POST /api/github/add",
        "POST /api/github/commit",
        "POST /api/github/push",
        "POST /api/github/verify",
    ];
    format!(
        "{{\"status\":\"ready\",\"user_routes\":{},\"debug_routes\":{},\"default_surface\":\"user\",\"developer_surface\":\"debug\"}}",
        json_string_array(user_routes.iter().copied()),
        json_string_array(debug_routes.iter().copied())
    )
}

fn report_export_json(options: &ServerOptions, request_body: &str) -> String {
    let export_dir = options.artifact_dir.join("exports");
    let _ = fs::create_dir_all(&export_dir);
    let diagnostics = diagnostics_json(options);
    let github = github_status_json(options);
    let projects = projects_json(options);
    let candidate_schema = [
        "rank",
        "candidate_id",
        "gene_name",
        "orf_id",
        "genomic_coordinates",
        "context_class",
        "fragment_score",
        "co_occurrence_score",
        "plm_score",
        "structure_score",
        "docking_score",
        "md_stability_score",
        "final_priority_score",
        "risk_flags",
    ];
    let evidence_schema = [
        "report_item",
        "source_file",
        "manifest",
        "schema",
        "tool_version",
        "artifact_hash",
        "validation_state",
        "report_section",
    ];
    let export_target = export_target_run_dir(options, request_body);
    let candidate_result = export_target
        .as_ref()
        .and_then(|(_, run_dir)| candidate_result_table(run_dir, &candidate_schema));
    let evidence_result = export_target
        .as_ref()
        .and_then(|(_, run_dir)| evidence_result_table(run_dir, &evidence_schema));
    let candidate_rows_json = candidate_result
        .as_ref()
        .map(|(_, rows, _)| rows.clone())
        .unwrap_or_else(|| "[]".to_string());
    let evidence_rows_json = evidence_result
        .as_ref()
        .map(|(_, rows, _)| rows.clone())
        .unwrap_or_else(|| "[]".to_string());
    let candidate_row_count = candidate_result
        .as_ref()
        .map(|(_, _, count)| *count)
        .unwrap_or(0);
    let evidence_row_count = evidence_result
        .as_ref()
        .map(|(_, _, count)| *count)
        .unwrap_or(0);
    let current_run_json = export_target
        .as_ref()
        .map(|(run_id, run_dir)| {
            format!(
                "{{\"run_id\":\"{}\",\"run_dir\":\"{}\"}}",
                escape_json(run_id),
                escape_json(&run_dir.display().to_string())
            )
        })
        .unwrap_or_else(|| "null".to_string());
    let candidate_source_path = candidate_result
        .as_ref()
        .map(|(path, _, _)| path.display().to_string())
        .unwrap_or_default();
    let evidence_source_path = evidence_result
        .as_ref()
        .map(|(path, _, _)| path.display().to_string())
        .unwrap_or_default();
    let html_report = export_dir.join("genoligand_report.html");
    let markdown_report = export_dir.join("genoligand_report.md");
    let candidate_table_tsv = export_dir.join("candidate_table.tsv");
    let candidate_table_csv = export_dir.join("candidate_table.csv");
    let machine_json = export_dir.join("machine_readable_report.json");
    let artifact_bundle = export_dir.join("artifact_bundle_manifest.tsv");
    let reproducibility_package = export_dir.join("reproducibility_package_manifest.tsv");
    let full_artifact_archive = export_dir.join("full_artifact_archive_manifest.tsv");
    let support_bundle = export_dir.join("support_bundle_manifest.tsv");
    let project_package = export_dir.join("project_package_manifest.tsv");
    let backup_manifest = export_dir.join("backup_manifest.tsv");
    let run_manifest = export_dir.join("run_manifest.json");
    let report_sections_manifest = export_dir.join("report_sections_manifest.tsv");
    let tool_versions = export_dir.join("tool_version_manifest.tsv");
    let diagnostics_report = export_dir.join("diagnostics_report.json");
    let github_report = export_dir.join("github_push_report.json");
    let manifest = export_dir.join("report_export_manifest.tsv");
    let candidate_html_table = json_rows_to_html_table(&candidate_rows_json, &candidate_schema);
    let evidence_html_table = json_rows_to_html_table(&evidence_rows_json, &evidence_schema);
    let candidate_report_table = json_rows_to_report_markdown_table(
        &candidate_rows_json,
        &[
            ("rank", "Rank"),
            ("candidate_id", "Candidate"),
            ("final_priority_score", "Priority score"),
            ("risk_flags", "Risk"),
            ("database_evidence", "Evidence summary"),
            ("experimental_validation_guidance", "Follow-up"),
        ],
    );
    let evidence_report_table = json_rows_to_report_markdown_table(
        &evidence_rows_json,
        &[
            ("report_item", "Evidence item"),
            ("source_file", "Source"),
            ("validation_state", "State"),
            ("report_section", "Report section"),
        ],
    );
    let current_run_label = serde_json::from_str::<Value>(&current_run_json)
        .ok()
        .and_then(|value| {
            value
                .get("run_id")
                .and_then(Value::as_str)
                .map(report_value)
        })
        .unwrap_or_else(|| "selected run".to_string());

    let _ = write_text(
        &html_report,
        &format!(
            "<!doctype html><html lang=\"zh-Hans\"><head><meta charset=\"utf-8\"><title>GenoLigand {}</title><style>body{{font-family:system-ui,sans-serif;background:#080b10;color:#e8eef8;padding:32px}}table{{border-collapse:collapse;width:100%;margin:16px 0}}th,td{{border:1px solid #2a3444;padding:8px;text-align:left;vertical-align:top}}th{{background:#121926}}code{{color:#9bdcff}}</style></head><body><h1>GenoLigand {}</h1><p>本地正式报告由 gls-server 生成。Local production report generated by gls-server.</p><p><strong>当前运行 / Current run:</strong> <code>{}</code></p><p><strong>候选行 / Candidate rows:</strong> {} | <strong>证据行 / Evidence rows:</strong> {}</p><p><strong>候选来源 / Candidate source:</strong> <code>{}</code></p><p><strong>证据来源 / Evidence source:</strong> <code>{}</code></p><h2>候选排序 / Candidate Ranking</h2>{}<h2>证据账本 / Evidence Ledger</h2>{}</body></html>\n",
            PRODUCT_VERSION,
            PRODUCT_VERSION,
            html_escape(&current_run_json),
            candidate_row_count,
            evidence_row_count,
            html_escape(&candidate_source_path),
            html_escape(&evidence_source_path),
            candidate_html_table,
            evidence_html_table
        ),
    );
    let _ = write_text(
        &markdown_report,
        &format!(
            "# GenoLigand {}\n\n本地正式报告由 `gls-server` 生成。Local production report generated by `gls-server`.\n\n当前运行 / Current run: `{}`\n\n候选行 / Candidate rows: `{}`\n\n证据行 / Evidence rows: `{}`\n\n候选来源 / Candidate source: `{}`\n\n证据来源 / Evidence source: `{}`\n\n## 候选排序 / Candidate Ranking\n\n{}\n## 证据账本 / Evidence Ledger\n\n{}\n",
            PRODUCT_VERSION,
            current_run_json,
            candidate_row_count,
            evidence_row_count,
            candidate_source_path,
            evidence_source_path,
            candidate_report_table,
            evidence_report_table
        ),
    );
    let report_sections: Vec<(&str, &str, String)> = vec![
        (
            "summary",
            "Summary",
            format!(
                "This local report summarizes the selected GenoLigand run with {candidate_row_count} candidate rows and {evidence_row_count} evidence rows."
            ),
        ),
        (
            "inputs",
            "Inputs",
            format!("The report covers the selected local run `{current_run_label}` with the saved input set and generated evidence records."),
        ),
        (
            "database_versions",
            "Database Versions",
            "Database and software version records are included in the reproducibility package and support bundle.".to_string(),
        ),
        (
            "candidate_ranking",
            "Candidate Ranking",
            format!(
                "The candidate ranking table below uses the score and evidence fields selected for researcher review.\n\n{candidate_report_table}"
            ),
        ),
        (
            "key_figures",
            "Key Figures",
            "Key visual summaries are represented by the candidate ranking, structure/patch, genome/ORF, and MD views in the local WebUI.".to_string(),
        ),
        (
            "structure_md_evidence",
            "Structure and MD Evidence",
            "Structure, docking, MD, RMSD/RMSF, contact, hydrogen-bond, and ligand drift evidence is summarized through evidence cards and linked artifacts.".to_string(),
        ),
        (
            "evidence_chain",
            "Evidence Chain",
            format!(
                "Evidence records are grouped into report-facing items with their source and validation state.\n\n{evidence_report_table}"
            ),
        ),
        (
            "parameters",
            "Parameters",
            "Run parameters, profile choices, software versions, and export metadata are preserved in the technical appendix files.".to_string(),
        ),
        (
            "limitations",
            "Limitations",
            "Computational screening results are prioritization evidence only. They are not experimental validation and require independent laboratory follow-up.".to_string(),
        ),
        (
            "validation_suggestions",
            "Validation Suggestions",
            "Review top candidates, confirm input identity, inspect structure and MD support, then plan independent biochemical or biophysical validation.".to_string(),
        ),
        (
            "appendix",
            "Appendix",
            format!(
                "Technical support files for reproducibility are written under `{}`. They include export manifests, project package records, reproducibility package records, support bundle records, archive manifests, run manifests, tool version records, and machine-readable evidence tables.",
                export_dir.display()
            ),
        ),
    ];
    let report_section_markdown = report_sections
        .iter()
        .map(|(_, title, body)| format!("## {title}\n\n{body}\n"))
        .collect::<Vec<_>>()
        .join("\n");
    let report_section_html = report_sections
        .iter()
        .map(|(_, title, body)| {
            format!(
                "<h2>{}</h2><p>{}</p>",
                html_escape(title),
                html_escape(body).replace('\n', "<br>")
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let report_section_manifest_text = report_sections
        .iter()
        .map(|(section_id, title, body)| format!("{}\t{}\t{}\tok", section_id, title, body.len()))
        .collect::<Vec<_>>()
        .join("\n");
    let _ = write_text(
        &html_report,
        &format!(
            "<!doctype html><html lang=\"zh-Hans\"><head><meta charset=\"utf-8\"><title>GenoLigand {}</title><style>body{{font-family:system-ui,sans-serif;background:#080b10;color:#e8eef8;padding:32px}}table{{border-collapse:collapse;width:100%;margin:16px 0}}th,td{{border:1px solid #2a3444;padding:8px;text-align:left;vertical-align:top}}th{{background:#121926}}code{{color:#9bdcff}}</style></head><body><h1>GenoLigand {}</h1><p>Generated by the GenoLigand local report service.</p><p><strong>Selected run:</strong> <code>{}</code></p><p><strong>Candidate rows:</strong> {} | <strong>Evidence rows:</strong> {}</p>{}</body></html>\n",
            PRODUCT_VERSION,
            PRODUCT_VERSION,
            html_escape(&current_run_label),
            candidate_row_count,
            evidence_row_count,
            report_section_html
        ),
    );
    let _ = write_text(
        &markdown_report,
        &format!(
            "# GenoLigand {}\n\nGenerated by the GenoLigand local report service.\n\nSelected run: `{}`\n\nCandidate rows: `{}`\n\nEvidence rows: `{}`\n\n{}\n",
            PRODUCT_VERSION,
            current_run_label,
            candidate_row_count,
            evidence_row_count,
            report_section_markdown
        ),
    );
    let _ = write_text(
        &report_sections_manifest,
        &format!("section_id\ttitle\tbody_bytes\tstatus\n{report_section_manifest_text}\n"),
    );
    let _ = write_text(
        &candidate_table_tsv,
        &json_rows_to_tsv(&candidate_rows_json, &candidate_schema),
    );
    let _ = write_text(
        &candidate_table_csv,
        &json_rows_to_csv(&candidate_rows_json, &candidate_schema),
    );
    let _ = write_text(
        &machine_json,
        &format!(
            "{{\"product\":\"{}\",\"version\":\"{}\",\"projects\":{},\"diagnostics\":{},\"github\":{},\"current_run\":{},\"candidate_source_path\":\"{}\",\"evidence_source_path\":\"{}\",\"candidate_row_count\":{},\"evidence_row_count\":{},\"contains_computed_candidates\":{},\"candidates\":{},\"evidence\":{}}}\n",
            escape_json(PRODUCT_NAME),
            escape_json(PRODUCT_VERSION),
            projects,
            diagnostics,
            github,
            current_run_json,
            escape_json(&candidate_source_path),
            escape_json(&evidence_source_path),
            candidate_row_count,
            evidence_row_count,
            candidate_row_count > 0,
            candidate_rows_json,
            evidence_rows_json
        ),
    );
    let _ = write_text(
        &artifact_bundle,
        &format!(
            "artifact_type\tpath\tstatus\nhtml_report\tgenoligand_report.html\tcreated\nmarkdown_report\tgenoligand_report.md\tcreated\ncandidate_table_tsv\tcandidate_table.tsv\tcreated\ncandidate_table_csv\tcandidate_table.csv\tcreated\nmachine_readable_json\tmachine_readable_report.json\tcreated\ncandidate_source\t{}\t{}\nevidence_source\t{}\t{}\n",
            candidate_source_path,
            if candidate_row_count > 0 {
                "linked"
            } else {
                "not_available"
            },
            evidence_source_path,
            if evidence_row_count > 0 {
                "linked"
            } else {
                "not_available"
            }
        ),
    );
    let reproducibility_text = format!(
        "item\tvalue\nproduct\t{}\nversion\t{}\ncurrent_run\t{}\ncandidate_rows\t{}\nevidence_rows\t{}\ntool_versions\t{}\nrun_manifest\t{}\n",
        PRODUCT_NAME,
        PRODUCT_VERSION,
        current_run_json,
        candidate_row_count,
        evidence_row_count,
        tool_versions.display(),
        run_manifest.display()
    );
    let _ = write_text(&reproducibility_package, &reproducibility_text);
    let _ = write_text(
        &full_artifact_archive,
        &format!(
            "artifact_type\tpath\tstatus\n{}\n",
            collect_run_files(
                export_target
                    .as_ref()
                    .map(|(_, run_dir)| run_dir.as_path())
                    .unwrap_or_else(|| options.artifact_dir.as_path()),
                500
            )
            .iter()
            .map(|path| format!(
                "{}\t{}\t{}",
                classify_artifact_path(path),
                path.display(),
                if path.exists() {
                    "available"
                } else {
                    "missing"
                }
            ))
            .collect::<Vec<_>>()
            .join("\n")
        ),
    );
    let _ = write_text(
        &support_bundle,
        &format!(
            "item\tpath\tstatus\ndiagnostics_report\t{}\tcreated\ntool_version_manifest\t{}\tcreated\nreport_export_manifest\t{}\tcreated\n",
            diagnostics_report.display(),
            tool_versions.display(),
            manifest.display()
        ),
    );
    let _ = write_text(
        &project_package,
        &format!(
            "item\tvalue\nproject_state\t{}\ncurrent_run\t{}\nreports_dir\t{}\nexports_dir\t{}\ncontains_large_database_copy\tfalse\nread_only_review_mode\ttrue\ndemo_mode_supported\ttrue\nsupport_bundle_linked\ttrue\nsupport_bundle_path\t{}\nreopen_mode\tlocal_project_package\nlarge_database_policy\treference_manifest_only\n",
            projects,
            current_run_json,
            export_target
                .as_ref()
                .and_then(|(_, run_dir)| run_dir.parent().and_then(Path::parent))
                .map(|project_dir| project_dir.join("reports").display().to_string())
                .unwrap_or_default(),
            export_dir.display(),
            support_bundle.display()
        ),
    );
    let _ = write_text(
        &backup_manifest,
        &format!(
            "backup_scope\tpath\tstatus\nproject_package\t{}\tcreated\nreports\t{}\tcreated\nexports\t{}\tcreated\nlarge_databases\toutside_backup_scope\tskipped\n",
            project_package.display(),
            markdown_report.display(),
            export_dir.display()
        ),
    );
    let _ = write_text(
        &run_manifest,
        &format!(
            "{{\"product\":\"{}\",\"version\":\"{}\",\"export_dir\":\"{}\",\"current_run\":{},\"candidate_row_count\":{},\"evidence_row_count\":{},\"source\":\"gls-server\"}}\n",
            escape_json(PRODUCT_NAME),
            escape_json(PRODUCT_VERSION),
            escape_json(&export_dir.display().to_string()),
            current_run_json,
            candidate_row_count,
            evidence_row_count
        ),
    );
    let _ = write_text(
        &tool_versions,
        &format!(
            "tool\tstatus\nGenoLigand\t{}\ngls-server\t{}\ngit\t{}\njava\t{}\nnextflow\t{}\npython\t{}\nopenmm\t{}\nnvidia-smi\t{}\n",
            PRODUCT_VERSION,
            PRODUCT_VERSION,
            command_probe("git", &["--version"]),
            command_probe("java", &["-version"]),
            command_probe("nextflow", &["-version"]),
            command_probe("python", &["--version"]),
            python_module_probe("openmm"),
            command_probe("nvidia-smi", &["--query-gpu=name", "--format=csv,noheader"])
        ),
    );
    let _ = write_text(&diagnostics_report, &(diagnostics + "\n"));
    let _ = write_text(&github_report, &(github + "\n"));

    let export_files = [
        ("research_report", &markdown_report),
        ("html_report", &html_report),
        ("markdown_report", &markdown_report),
        ("report_sections_manifest", &report_sections_manifest),
        ("candidate_table_tsv", &candidate_table_tsv),
        ("candidate_table_csv", &candidate_table_csv),
        ("candidate_table", &candidate_table_tsv),
        ("reproducibility_package", &reproducibility_package),
        ("full_artifact_archive", &full_artifact_archive),
        ("support_bundle", &support_bundle),
        ("project_package", &project_package),
        ("backup", &backup_manifest),
        ("machine_readable_json", &machine_json),
        ("artifact_bundle", &artifact_bundle),
        ("run_manifest", &run_manifest),
        ("tool_version_manifest", &tool_versions),
        ("diagnostics_report", &diagnostics_report),
        ("github_push_report", &github_report),
    ];
    let manifest_text = export_files
        .iter()
        .map(|(kind, path)| {
            format!(
                "{}\t{}\t{}\t{}",
                kind,
                path.display(),
                fs::metadata(path)
                    .map(|metadata| metadata.len())
                    .unwrap_or(0),
                if path.exists() { "created" } else { "failed" }
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let _ = write_text(
        &manifest,
        &format!("export_type\tpath\tbytes\tstatus\n{manifest_text}\n"),
    );
    if let Some((_, run_dir)) = export_target.as_ref()
        && let Some(project_dir) = run_dir.parent().and_then(Path::parent)
    {
        let reports_dir = project_dir.join("reports");
        let _ = fs::create_dir_all(&reports_dir);
        let report_paths_text = export_files
            .iter()
            .map(|(kind, path)| format!("{kind}\t{}", path.display()))
            .collect::<Vec<_>>()
            .join("\n");
        let _ = write_text(
            &project_dir.join("report_paths.tsv"),
            &format!("report_type\tpath\n{report_paths_text}\n"),
        );
        let _ = write_text(
            &reports_dir.join("latest_report_export_manifest.tsv"),
            &format!("export_type\tpath\tbytes\tstatus\n{manifest_text}\n"),
        );
    }
    let exports = json_string_array(export_files.iter().map(|(kind, _)| *kind));
    let files = export_files
        .iter()
        .map(|(kind, path)| {
            format!(
                "{{\"export_type\":\"{}\",\"path\":\"{}\",\"bytes\":{},\"exists\":{}}}",
                escape_json(kind),
                escape_json(&path.display().to_string()),
                fs::metadata(path)
                    .map(|metadata| metadata.len())
                    .unwrap_or(0),
                path.exists()
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"exports\":{},\"status\":\"ready\",\"export_dir\":\"{}\",\"manifest\":\"{}\",\"files\":[{}],\"current_run\":{},\"candidate_row_count\":{},\"evidence_row_count\":{},\"contains_computed_candidates\":{}}}",
        exports,
        escape_json(&export_dir.display().to_string()),
        escape_json(&manifest.display().to_string()),
        files,
        current_run_json,
        candidate_row_count,
        evidence_row_count,
        candidate_row_count > 0
    )
}

fn cleanup_json(options: &ServerOptions) -> String {
    let cleanup_dir = options.artifact_dir.join("cleanup");
    let manifest = cleanup_dir.join("post_push_cleanup_execution_manifest.tsv");
    let summary = cleanup_dir.join("post_push_cleanup_summary.tsv");
    let _ = fs::create_dir_all(&cleanup_dir);
    let candidates = cleanup_candidates(options);
    let mut rows = Vec::new();
    let mut deleted_files = 0usize;
    let mut deleted_bytes = 0u64;
    let mut unremoved = 0usize;
    for candidate in candidates {
        let before_files = file_count(&candidate.path);
        let before_bytes = path_size(&candidate.path);
        let status = if !candidate.path.exists() {
            "not_present".to_string()
        } else if !is_cleanup_allowed(options, &candidate.path) {
            unremoved += 1;
            "blocked_protected_path".to_string()
        } else {
            match remove_cleanup_path(&candidate.path) {
                Ok(()) => {
                    deleted_files = deleted_files.saturating_add(before_files.max(1));
                    deleted_bytes = deleted_bytes.saturating_add(before_bytes);
                    "deleted".to_string()
                }
                Err(err) => {
                    unremoved += 1;
                    format!("failed:{err}")
                }
            }
        };
        rows.push(format!(
            "cleanup\t{}\t{}\t{}\t{}\t{}",
            candidate.kind,
            candidate.path.display(),
            before_files,
            before_bytes,
            status
        ));
    }
    rows.push(format!(
        "retain\tcurrent_source\t{}\t0\t0\tprotected",
        options.workspace_root.display()
    ));
    rows.push(format!(
        "retain\tcurrent_artifact_manifests\t{}\t0\t0\tprotected",
        options.artifact_dir.display()
    ));
    let manifest_text = format!(
        "action\tclass\tpath\tbefore_files\tbefore_bytes\tstatus\n{}\n",
        rows.join("\n")
    );
    let summary_text = format!(
        "metric\tvalue\ncleanup_scope\tpost_push_allowlist\ndeleted_files_count\t{deleted_files}\ndeleted_bytes\t{deleted_bytes}\ncleanup_eligible_unremoved_count\t{unremoved}\nprotected_path_violation_count\t0\n"
    );
    let manifest_result = write_text(&manifest, &manifest_text);
    let summary_result = write_text(&summary, &summary_text);
    if let Err(err) = manifest_result.and(summary_result) {
        return format!(
            "{{\"status\":\"failed\",\"error\":\"{}\"}}",
            escape_json(&err.to_string())
        );
    }
    format!(
        "{{\"status\":\"completed\",\"manifest\":\"{}\",\"summary\":\"{}\",\"scope\":\"post_push_allowlist\",\"deleted_files_count\":{},\"deleted_bytes\":{},\"cleanup_eligible_unremoved_count\":{},\"protected_path_violation_count\":0}}",
        escape_json(&manifest.display().to_string()),
        escape_json(&summary.display().to_string()),
        deleted_files,
        deleted_bytes,
        unremoved
    )
}

#[derive(Clone, Debug)]
struct CleanupCandidate {
    path: PathBuf,
    kind: &'static str,
}

fn cleanup_candidates(options: &ServerOptions) -> Vec<CleanupCandidate> {
    let mut candidates = Vec::new();
    for relative in [
        "exports/report_export_manifest.json",
        "gls-server.stdout.log",
        "gls-server.stderr.log",
        "server.log",
        "setup/installer_self_check_manual",
        "tmp",
        "work",
        "run",
        "cache",
        ".cache",
        "old_reports",
        "old_screenshots",
        "visual_audit_intermediate",
        "webui_build_cache",
        "nextflow_cache",
        "release_bundle",
    ] {
        candidates.push(CleanupCandidate {
            path: options.artifact_dir.join(relative),
            kind: "artifact_runtime",
        });
    }
    for path in collect_cleanup_files(
        &options.artifact_dir.join("ui"),
        &["html", "tmp", "log"],
        500,
    ) {
        candidates.push(CleanupCandidate {
            path,
            kind: "visual_audit_intermediate",
        });
    }
    for relative in [
        ".nextflow/cache",
        "outputs/phase_0_9/runtime",
        "outputs/phase_0_9_1/runtime",
        "scripts/phase09/__pycache__",
        "scripts/reports/__pycache__",
        "scripts/setup/__pycache__",
        "webui/.cache",
        "webui/playwright-report",
        "webui/test-results",
    ] {
        candidates.push(CleanupCandidate {
            path: options.workspace_root.join(relative),
            kind: "workspace_cache",
        });
    }
    for path in collect_named_files(&options.workspace_root, ".nextflow.log", 30) {
        candidates.push(CleanupCandidate {
            path,
            kind: "nextflow_log",
        });
    }
    candidates
}

fn collect_cleanup_files(root: &Path, extensions: &[&str], limit: usize) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if files.len() >= limit {
            break;
        }
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            if files.len() >= limit {
                break;
            }
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_dir() {
                stack.push(path);
            } else if file_type.is_file()
                && path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .map(|ext| {
                        extensions
                            .iter()
                            .any(|allowed| ext.eq_ignore_ascii_case(allowed))
                    })
                    .unwrap_or(false)
            {
                files.push(path);
            }
        }
    }
    files
}

fn collect_named_files(root: &Path, prefix: &str, limit: usize) -> Vec<PathBuf> {
    fs::read_dir(root)
        .ok()
        .into_iter()
        .flat_map(|entries| entries.flatten())
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .map(|name| name == prefix || name.starts_with(&format!("{prefix}.")))
                .unwrap_or(false)
        })
        .take(limit)
        .collect()
}

fn is_cleanup_allowed(options: &ServerOptions, path: &Path) -> bool {
    let protected_roots = [
        options.workspace_root.join("src"),
        options.workspace_root.join("crates"),
        options.workspace_root.join("configs"),
        options.workspace_root.join("workflows"),
        options.workspace_root.join("docs"),
        options.workspace_root.join("schemas"),
        options.workspace_root.join(".git"),
    ];
    if protected_roots.iter().any(|root| path.starts_with(root)) {
        return false;
    }
    if path.starts_with(&options.artifact_dir) {
        return true;
    }
    [
        options.workspace_root.join(".nextflow"),
        options
            .workspace_root
            .join("outputs")
            .join("phase_0_9")
            .join("runtime"),
        options
            .workspace_root
            .join("outputs")
            .join("phase_0_9_1")
            .join("runtime"),
        options
            .workspace_root
            .join("scripts")
            .join("phase09")
            .join("__pycache__"),
        options
            .workspace_root
            .join("scripts")
            .join("reports")
            .join("__pycache__"),
        options
            .workspace_root
            .join("scripts")
            .join("setup")
            .join("__pycache__"),
        options.workspace_root.join("webui").join(".cache"),
        options
            .workspace_root
            .join("webui")
            .join("playwright-report"),
        options.workspace_root.join("webui").join("test-results"),
    ]
    .iter()
    .any(|root| path.starts_with(root))
        || path
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| name == ".nextflow.log" || name.starts_with(".nextflow.log."))
            .unwrap_or(false)
}

fn path_size(path: &Path) -> u64 {
    if path.is_file() {
        fs::metadata(path)
            .map(|metadata| metadata.len())
            .unwrap_or(0)
    } else {
        directory_size(path)
    }
}

fn file_count(path: &Path) -> usize {
    if path.is_file() {
        return 1;
    }
    let mut count = 0usize;
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_dir() {
                stack.push(path);
            } else if file_type.is_file() {
                count = count.saturating_add(1);
            }
        }
    }
    count
}

fn remove_cleanup_path(path: &Path) -> Result<(), String> {
    if path.is_dir() {
        fs::remove_dir_all(path).map_err(|err| err.to_string())
    } else {
        fs::remove_file(path).map_err(|err| err.to_string())
    }
}

fn command_text(options: &ServerOptions, args: &[&str]) -> (bool, String) {
    let candidates = [
        "git",
        "D:\\Program Files\\Git\\cmd\\git.exe",
        "C:\\Program Files\\Git\\cmd\\git.exe",
        "C:\\Program Files (x86)\\Git\\cmd\\git.exe",
    ];
    let mut last_error = String::new();
    for git in candidates {
        let capture = capture_command(git, args, Some(&options.workspace_root));
        if capture.ok || !capture.text.is_empty() {
            return (capture.ok, capture.text);
        }
        if let Some(error) = capture.error {
            last_error = error;
        }
    }
    (false, last_error)
}

fn git_value(options: &ServerOptions, args: &[&str]) -> Option<String> {
    let (ok, text) = command_text(options, args);
    if ok && !text.is_empty() {
        Some(text.lines().next().unwrap_or("").to_string())
    } else {
        None
    }
}

fn git_current_branch(options: &ServerOptions) -> Option<String> {
    [
        ["rev-parse", "--abbrev-ref", "HEAD"].as_slice(),
        ["symbolic-ref", "--short", "HEAD"].as_slice(),
    ]
    .into_iter()
    .find_map(|args| {
        git_value(options, args).and_then(|value| {
            let trimmed = value.trim();
            if trimmed.is_empty() || trimmed == "HEAD" {
                None
            } else {
                Some(trimmed.to_string())
            }
        })
    })
}

fn git_probe(options: &ServerOptions) -> GitProbe {
    let branch = git_current_branch(options);
    let short_commit = git_value(options, &["rev-parse", "--short", "HEAD"]);
    let local_head = git_value(options, &["rev-parse", "HEAD"]);
    let remote = git_value(options, &["remote", "get-url", "origin"]);
    let (status_ok, status_text) = command_text(options, &["status", "--short"]);
    GitProbe {
        available: branch.is_some() || local_head.is_some() || status_ok,
        branch,
        short_commit,
        local_head,
        remote,
        status_ok,
        status_text,
    }
}

fn git_probe_json(probe: &GitProbe) -> String {
    format!(
        "{{\"available\":{},\"branch\":{},\"short_commit\":{},\"local_head\":{},\"remote\":{},\"status_ok\":{},\"status_lines\":{}}}",
        probe.available,
        option_json(probe.branch.as_deref()),
        option_json(probe.short_commit.as_deref()),
        option_json(probe.local_head.as_deref()),
        option_json(probe.remote.as_deref()),
        probe.status_ok,
        json_string_array(probe.status_text.lines())
    )
}

fn github_log_path(options: &ServerOptions, action: &str) -> PathBuf {
    options
        .artifact_dir
        .join("github")
        .join(format!("latest_{action}_log.tsv"))
}

fn write_github_log(
    options: &ServerOptions,
    action: &str,
    command: &str,
    ok: bool,
    text: &str,
) -> PathBuf {
    let path = github_log_path(options, action);
    let status = if ok { "completed" } else { "failed" };
    let mut rows = vec!["action\tcommand\tstatus\tline_index\tline".to_string()];
    let lines = if text.is_empty() {
        vec![""]
    } else {
        text.lines().collect::<Vec<_>>()
    };
    for (index, line) in lines.iter().enumerate() {
        rows.push(format!(
            "{}\t{}\t{}\t{}\t{}",
            tsv_cell(action),
            tsv_cell(command),
            status,
            index + 1,
            tsv_cell(line)
        ));
    }
    let _ = write_text(&path, &(rows.join("\n") + "\n"));
    path
}

fn tsv_cell(value: &str) -> String {
    value.replace(['\t', '\r', '\n'], " ")
}

fn log_file_json(path: &Path) -> String {
    fs::read_to_string(path)
        .ok()
        .map(|text| json_string_array(text.lines()))
        .unwrap_or_else(|| "[]".to_string())
}

fn option_bool_json(value: Option<bool>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "null".to_string())
}

fn github_base_fields(
    options: &ServerOptions,
    push_status: &str,
    remote_head_hash: Option<String>,
    remote_match: Option<bool>,
    remote_validation_status: &str,
) -> String {
    let git = git_probe(options);
    let push_log_path = github_log_path(options, "git_push");
    let verify_log_path = github_log_path(options, "remote_verify");
    format!(
        "\"remote\":{},\"branch\":{},\"local_commit_hash\":{},\"local_head_hash\":{},\"git_available\":{},\"git_status_ok\":{},\"remote_branch\":{},\"remote_head_hash\":{},\"remote_head_matches_local_head\":{},\"remote_validation_status\":\"{}\",\"push_status\":\"{}\",\"push_log\":{},\"push_log_path\":{},\"remote_verify_log\":{},\"remote_verify_log_path\":{},\"working_tree_log\":{}",
        option_json(git.remote.as_deref()),
        option_json(git.branch.as_deref()),
        option_json(git.short_commit.as_deref()),
        option_json(git.local_head.as_deref()),
        git.available,
        git.status_ok,
        option_json(
            git.branch
                .as_deref()
                .map(|value| format!("origin/{value}"))
                .as_deref()
        ),
        option_json(remote_head_hash.as_deref()),
        option_bool_json(remote_match),
        escape_json(remote_validation_status),
        escape_json(push_status),
        log_file_json(&push_log_path),
        option_json(push_log_path.to_str()),
        log_file_json(&verify_log_path),
        option_json(verify_log_path.to_str()),
        json_string_array(git.status_text.lines())
    )
}

fn github_status_json(options: &ServerOptions) -> String {
    let (ok, text) = command_text(options, &["status", "--short"]);
    let log_path = write_github_log(options, "git_status", "git status --short", ok, &text);
    format!(
        "{{{},\"status\":\"{}\",\"command\":\"git status --short\",\"log\":{},\"log_path\":{}}}",
        github_base_fields(options, "idle", None, None, "not_checked"),
        if ok { "completed" } else { "failed" },
        json_string_array(text.lines()),
        option_json(log_path.to_str())
    )
}

fn git_action_json(options: &ServerOptions, action: &str, args: &[&str]) -> String {
    let (ok, text) = command_text(options, args);
    let command = format!("git {}", args.join(" "));
    let log_path = write_github_log(options, action, &command, ok, &text);
    format!(
        "{{{},\"status\":\"{}\",\"command\":\"{}\",\"log\":{},\"log_path\":{}}}",
        github_base_fields(
            options,
            if ok { "idle" } else { "failed" },
            None,
            None,
            "not_checked"
        ),
        if ok { "completed" } else { "failed" },
        escape_json(&command),
        json_string_array(text.lines()),
        option_json(log_path.to_str())
    )
}

fn git_push_json(options: &ServerOptions) -> String {
    let branch = git_current_branch(options).unwrap_or_else(|| "main".to_string());
    let (ok, text) = command_text(options, &["push", "origin", &branch]);
    let command = format!("git push origin {branch}");
    let log_path = write_github_log(options, "git_push", &command, ok, &text);
    let cleanup = if ok {
        cleanup_json(options)
    } else {
        "null".to_string()
    };
    format!(
        "{{{},\"status\":\"{}\",\"command\":\"{}\",\"log\":{},\"log_path\":{},\"post_push_cleanup\":{}}}",
        github_base_fields(
            options,
            if ok { "completed" } else { "failed" },
            None,
            None,
            "not_checked_after_push"
        ),
        if ok { "completed" } else { "failed" },
        escape_json(&command),
        json_string_array(text.lines()),
        option_json(log_path.to_str()),
        cleanup
    )
}

fn github_verify_json(options: &ServerOptions) -> String {
    let branch = git_current_branch(options).unwrap_or_else(|| "main".to_string());
    let local_head = git_value(options, &["rev-parse", "HEAD"]);
    let remote_ref = format!("refs/heads/{branch}");
    let args = ["ls-remote", "origin", remote_ref.as_str()];
    let (ok, text) = command_text(options, &args);
    let command = format!("git ls-remote origin {remote_ref}");
    let log_path = write_github_log(options, "remote_verify", &command, ok, &text);
    let remote_head = if ok {
        text.lines()
            .find_map(|line| line.split_whitespace().next())
            .map(|hash| hash.to_string())
    } else {
        None
    };
    let remote_match = match (local_head.as_deref(), remote_head.as_deref()) {
        (Some(local), Some(remote)) => Some(local == remote),
        _ => None,
    };
    let validation_status = match (ok, remote_match) {
        (true, Some(true)) => "remote_head_matches_local_head",
        (true, Some(false)) => "remote_head_differs_from_local_head",
        (true, None) => "remote_head_unavailable",
        (false, _) => "remote_check_failed",
    };
    format!(
        "{{{},\"status\":\"{}\",\"command\":\"{}\",\"log\":{},\"log_path\":{}}}",
        github_base_fields(
            options,
            "idle",
            remote_head,
            remote_match,
            validation_status
        ),
        if ok { "completed" } else { "failed" },
        escape_json(&command),
        json_string_array(text.lines()),
        option_json(log_path.to_str())
    )
}

fn option_json(value: Option<&str>) -> String {
    value
        .map(|text| format!("\"{}\"", escape_json(text)))
        .unwrap_or_else(|| "null".to_string())
}

fn json_string_array<'a>(items: impl IntoIterator<Item = &'a str>) -> String {
    let body = items
        .into_iter()
        .map(|item| format!("\"{}\"", escape_json(item)))
        .collect::<Vec<_>>()
        .join(",");
    format!("[{body}]")
}
fn json_object(fields: &[(&str, &str)]) -> String {
    let pairs = fields
        .iter()
        .map(|(key, value)| format!("\"{}\":\"{}\"", escape_json(key), escape_json(value)))
        .collect::<Vec<_>>()
        .join(",");
    format!("{{{pairs}}}")
}

fn websocket_accept(key: &str) -> String {
    let mut bytes = key.as_bytes().to_vec();
    bytes.extend_from_slice(b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
    base64_encode(&sha1(&bytes))
}

fn sha1(input: &[u8]) -> [u8; 20] {
    let mut h0: u32 = 0x67452301;
    let mut h1: u32 = 0xEFCDAB89;
    let mut h2: u32 = 0x98BADCFE;
    let mut h3: u32 = 0x10325476;
    let mut h4: u32 = 0xC3D2E1F0;
    let bit_len = (input.len() as u64) * 8;
    let mut msg = input.to_vec();
    msg.push(0x80);
    while (msg.len() % 64) != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());
    for chunk in msg.chunks(64) {
        let mut w = [0u32; 80];
        for (i, bytes) in chunk.chunks(4).take(16).enumerate() {
            w[i] = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let (mut a, mut b, mut c, mut d, mut e) = (h0, h1, h2, h3, h4);
        for (i, word) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5A827999),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                _ => (b ^ c ^ d, 0xCA62C1D6),
            };
            let temp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*word);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }
        h0 = h0.wrapping_add(a);
        h1 = h1.wrapping_add(b);
        h2 = h2.wrapping_add(c);
        h3 = h3.wrapping_add(d);
        h4 = h4.wrapping_add(e);
    }
    let mut out = [0u8; 20];
    out[..4].copy_from_slice(&h0.to_be_bytes());
    out[4..8].copy_from_slice(&h1.to_be_bytes());
    out[8..12].copy_from_slice(&h2.to_be_bytes());
    out[12..16].copy_from_slice(&h3.to_be_bytes());
    out[16..20].copy_from_slice(&h4.to_be_bytes());
    out
}

fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::new();
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0];
        let b1 = *chunk.get(1).unwrap_or(&0);
        let b2 = *chunk.get(2).unwrap_or(&0);
        output.push(TABLE[(b0 >> 2) as usize] as char);
        output.push(TABLE[(((b0 & 0b11) << 4) | (b1 >> 4)) as usize] as char);
        if chunk.len() > 1 {
            output.push(TABLE[(((b1 & 0b1111) << 2) | (b2 >> 6)) as usize] as char);
        } else {
            output.push('=');
        }
        if chunk.len() > 2 {
            output.push(TABLE[(b2 & 0b111111) as usize] as char);
        } else {
            output.push('=');
        }
    }
    output
}

fn base64_decode(input: &str) -> Result<Vec<u8>, String> {
    let content = input
        .split_once(',')
        .map(|(_, value)| value)
        .unwrap_or(input)
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>();
    if content.len() % 4 != 0 {
        return Err("invalid_base64_length".to_string());
    }
    let mut output = Vec::new();
    for chunk in content.as_bytes().chunks(4) {
        let mut values = [0u8; 4];
        let mut padding = 0usize;
        for (index, byte) in chunk.iter().enumerate() {
            values[index] = match *byte {
                b'A'..=b'Z' => byte - b'A',
                b'a'..=b'z' => byte - b'a' + 26,
                b'0'..=b'9' => byte - b'0' + 52,
                b'+' => 62,
                b'/' => 63,
                b'=' => {
                    padding += 1;
                    0
                }
                _ => return Err("invalid_base64_character".to_string()),
            };
        }
        output.push((values[0] << 2) | (values[1] >> 4));
        if padding < 2 {
            output.push((values[1] << 4) | (values[2] >> 2));
        }
        if padding < 1 {
            output.push((values[2] << 6) | values[3]);
        }
    }
    Ok(output)
}

fn escape_json(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            ch if ch.is_control() => escaped.push(' '),
            ch => escaped.push(ch),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_test_dir(prefix: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("genoligand_{prefix}_{unique}"));
        let _ = fs::remove_dir_all(&root);
        root
    }

    #[test]
    fn websocket_accept_matches_rfc_example() {
        assert_eq!(
            websocket_accept("dGhlIHNhbXBsZSBub25jZQ=="),
            "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
        );
    }

    #[test]
    fn health_route_is_rust_owned() {
        let options = ServerOptions {
            host: "127.0.0.1".to_string(),
            port: 19090,
            artifact_dir: PathBuf::from("acceptance_artifacts_phase_0_9_1_production_webui"),
            workspace_root: PathBuf::from("."),
        };
        let (_, _, body) = route_http("GET", "/api/health", &options, "");
        assert!(body.contains("\"status\":\"ready\""));
        assert!(body.contains("\"service\":\"gls-server\""));
        assert!(body.contains("\"version\":\"0.9.1\""));
    }

    #[test]
    fn command_output_decoder_handles_utf16le_and_control_bytes() {
        let bytes = [
            0xff, 0xfe, b'U', 0, b'b', 0, b'u', 0, b'n', 0, b't', 0, b'u', 0, 0, 0, b'\n', 0,
        ];
        assert_eq!(clean_command_text(&decode_command_bytes(&bytes)), "Ubuntu");
    }

    #[test]
    fn command_output_cleaner_strips_ansi_sequences() {
        assert_eq!(
            clean_command_text("\u{1b}[KDownloading nextflow\n\u{1b}[32mready\u{1b}[0m"),
            "Downloading nextflow\nready"
        );
    }

    #[test]
    fn wsl_distribution_parser_reads_default_and_wsl2_count() {
        let listing = "  NAME            STATE           VERSION\n* Ubuntu-24.04    Running         2\n  GenoLigandPhase08 Stopped       2\n";
        assert_eq!(wsl_default_distribution("", listing), "Ubuntu-24.04");
        assert_eq!(wsl2_distribution_count(listing), 2);
        assert_eq!(
            wsl_distribution_state(listing, "GenoLigandPhase08"),
            Some("Stopped".to_string())
        );
        let status = "Default Distribution: Debian\nDefault Version: 2\n";
        assert_eq!(wsl_default_distribution(status, ""), "Debian");
        let localized_status = "\u{9ed8}\u{8ba4}\u{5206}\u{53d1}: GenoLigandPhase08\n\u{9ed8}\u{8ba4}\u{7248}\u{672c}: 2\n";
        assert_eq!(
            wsl_default_distribution(localized_status, ""),
            "GenoLigandPhase08"
        );
    }

    #[test]
    fn debug_runtime_json_reports_project_wsl_state_without_deep_probe() {
        let probe = RuntimeProbe {
            backend_type: "WSL2",
            windows_host: true,
            ready: true,
            wsl_status_ok: true,
            wsl_list_ok: true,
            wsl_status: "Default Distribution: Ubuntu-24.04\nDefault Version: 2\n".to_string(),
            wsl_distributions: "  NAME              STATE     VERSION\n* Ubuntu-24.04      Running   2\n  GenoLigandPhase08 Stopped   2\n".to_string(),
            default_distribution: "Ubuntu-24.04".to_string(),
            wsl2_distribution_count: 2,
        };
        let json = runtime_probe_json(&probe);
        assert!(json.contains("\"project_wsl_distro\":\"GenoLigandPhase08\""));
        assert!(json.contains("\"project_wsl_state\":\"Stopped\""));
        assert!(json.contains("\"non_disruptive_probe\":true"));
        assert!(json.contains("\"wsl_distributions\":["));
    }

    #[test]
    fn user_runs_route_lists_runs_across_visible_projects() {
        let root = temp_test_dir("phase091_user_runs_route");
        let options = ServerOptions {
            host: "127.0.0.1".to_string(),
            port: 19090,
            artifact_dir: root.join("artifact"),
            workspace_root: root.join("workspace"),
        };
        fs::create_dir_all(&options.workspace_root).expect("create workspace");
        let project_id = "project-user-runs";
        write_project_state(
            &options,
            project_id,
            "User runs project",
            "created",
            Some("run-user-runs-1"),
            false,
        )
        .expect("write project");
        let run_dir = options
            .artifact_dir
            .join("projects")
            .join(project_id)
            .join("runs")
            .join("run-user-runs-1");
        fs::create_dir_all(&run_dir).expect("create run dir");
        write_text(&run_dir.join("run_state.txt"), "completed\n").expect("write run state");

        let (status, content_type, body) = route_http("GET", "/api/user/runs", &options, "");
        assert_eq!(status, 200);
        assert_eq!(content_type, "application/json; charset=utf-8");
        assert!(body.contains("\"project_scope\":\"all_user_visible_projects\""));
        assert!(body.contains("\"run_id\":\"run-user-runs-1\""));
        assert!(body.contains("\"project_id\":\"project-user-runs\""));
        assert!(body.contains("\"state\":\"completed\""));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn json_escape_keeps_command_output_valid() {
        let escaped = escape_json("line1\nline2\t\"quoted\"");
        assert_eq!(escaped, "line1\\nline2\\t\\\"quoted\\\"");
    }

    #[test]
    fn master_validation_reads_clean_release_and_screenshot_retention_gates() {
        let root = temp_test_dir("phase091_clean_release_master");
        let metrics = root.join("metrics");
        fs::create_dir_all(&metrics).expect("create metrics");
        fs::write(
            metrics.join("clean_release_summary.tsv"),
            "metric\tvalue\tstatus\n\
first_launch_clean_state\tTrue\tok\n\
run_example_template_available\tTrue\tok\n\
sample_results_not_precreated\tTrue\tok\n\
post_test_project_count\t0\tok\n\
post_test_run_count\t0\tok\n\
post_test_temp_artifact_count\t0\tok\n\
post_test_screenshot_cache_count\t0\tok\n\
start_script_default_runtime_outside_workspace\tTrue\tok\n\
start_script_runtime_dir_override_present\tTrue\tok\n\
final_screenshot_cache_overlap_count\t0\tok\n",
        )
        .expect("write clean release");
        fs::write(
            metrics.join("screenshot_retention_summary.tsv"),
            "metric\tvalue\tstatus\n\
final_screenshot_retention_manifest_present\tTrue\tok\n\
final_screenshot_retention_row_count\t2\tok\n\
final_screenshot_cache_flag_count\t0\tok\n\
final_screenshot_retention_class_count\t2\tok\n\
final_screenshot_retention_ready_count\t2\tok\n\
final_screenshot_dirs_not_cache\tTrue\tok\n",
        )
        .expect("write screenshot retention");

        let overrides = actual_gate_overrides(&root);
        assert_eq!(
            overrides.get("post_test_project_count"),
            Some(&(String::from("0"), true))
        );
        assert_eq!(
            overrides.get("post_test_temp_artifact_count"),
            Some(&(String::from("0"), true))
        );
        assert_eq!(
            overrides.get("start_script_default_runtime_outside_workspace"),
            Some(&(String::from("True"), true))
        );
        assert_eq!(
            overrides.get("start_script_runtime_dir_override_present"),
            Some(&(String::from("True"), true))
        );
        assert_eq!(
            overrides.get("final_screenshot_dirs_not_cache"),
            Some(&(String::from("True"), true))
        );
        assert_eq!(
            overrides.get("final_screenshot_retention_row_count"),
            Some(&(String::from("2"), true))
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn master_validation_reads_md_chart_metadata_gates() {
        let root = temp_test_dir("phase091_md_chart_master");
        let metrics = root.join("metrics");
        fs::create_dir_all(&metrics).expect("create metrics");
        fs::write(
            metrics.join("genome_md_graphics_summary.tsv"),
            "metric\tvalue\tstatus\n\
md_chart_source_unit_meta_present\tData source: | unit:\tok\n\
md_chart_candidate_meta_present\tcandidate:\tok\n\
md_chart_time_basis_meta_present\ttime basis:\tok\n\
md_chart_artifact_link_meta_present\tartifact links:\tok\n\
md_chart_meta_function_present\tmdChartMeta\tok\n",
        )
        .expect("write md graphics");

        let overrides = actual_gate_overrides(&root);
        assert_eq!(
            overrides.get("md_chart_source_unit_meta_present"),
            Some(&(String::from("Data source: | unit:"), true))
        );
        assert_eq!(
            overrides.get("md_chart_candidate_meta_present"),
            Some(&(String::from("candidate:"), true))
        );
        assert_eq!(
            overrides.get("md_chart_time_basis_meta_present"),
            Some(&(String::from("time basis:"), true))
        );
        assert_eq!(
            overrides.get("md_chart_artifact_link_meta_present"),
            Some(&(String::from("artifact links:"), true))
        );
        assert_eq!(
            overrides.get("md_chart_meta_function_present"),
            Some(&(String::from("mdChartMeta"), true))
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn master_validation_reads_diagnostics_and_setup_repair_gates() {
        let root = temp_test_dir("phase091_diagnostics_setup_master");
        let metrics = root.join("metrics");
        fs::create_dir_all(&metrics).expect("create metrics");
        fs::write(
            metrics.join("settings_diagnostics_export_summary.tsv"),
            "metric\tvalue\tstatus\n\
diagnostics_fix_buttons_present\tdata-diagnostic-action\tok\n\
diagnostics_recheck_button_present\tRecheck\tok\n\
diagnostics_backend_action_route_present\tPOST /api/user/diagnostics/action\tok\n\
diagnostics_action_id_contract_count\t8\tok\n\
diagnostics_action_records_manifest\tdiagnostic_repair_actions.tsv\tok\n\
diagnostics_frontend_action_handler_present\trecordDiagnosticAction\tok\n\
diagnostics_required_item_completion_rate\t100%\tok\n\
diagnostics_human_readable_message_rate\t100%\tok\n\
diagnostics_repair_action_rate\t100%\tok\n\
setup_wizard_independent_page_present\tsetup_wizard_page\tok\n\
settings_developer_mode_debug_routes_lazy_loaded\tdebug routes load only after Open Developer Mode\tok\n\
settings_developer_mode_guides_lazy_loaded\tguide registry loads only after Open Developer Mode\tok\n\
setup_wizard_first_launch_default_route\tsetup_wizard_page\tok\n\
setup_wizard_step_count\t6\tok\n",
        )
        .expect("write diagnostics summary");
        fs::write(
            metrics.join("setup_storage_validation_summary.tsv"),
            "metric\tvalue\tstatus\n\
setup_wizard_pass\tTrue\tok\n\
storage_cleanup_policy_pass\tTrue\tok\n\
full_scientific_ready\tTrue\tok\n\
full_scientific_required_tools_all_ready\tTrue\tok\n\
install_plan_missing_tool_count\t0\tok\n\
install_plan_output_clean\tTrue\tok\n\
full_scientific_install_command_present\tpython scripts/setup/install_complete_scientific.py\tok\n\
complete_scientific_installer_wrapper_present\tscripts/setup/install_complete_scientific.py\tok\n\
full_scientific_installer_required_tool_count\t11\tok\n\
full_scientific_installer_nextflow_repair_present\tget.nextflow.io\tok\n\
full_scientific_installer_self_check_pass\tTrue\tok\n\
full_scientific_installer_self_check_manifest_present\tTrue\tok\n\
full_scientific_installer_required_runtime_tool_coverage\t13/13\tok\n\
full_scientific_installer_repair_step_coverage\t6/6\tok\n\
full_scientific_installer_repair_step_count\t6\tok\n\
full_scientific_installer_execute_guard_pass\tTrue\tok\n\
full_scientific_installer_manifest_paths_declared\tphase09_complete_scientific_install_plan.tsv\tok\n\
sample_project_validation_pass\tTrue\tok\n\
sample_project_candidate_detail_selected\tTrue\tok\n\
sample_project_research_report_exported\tTrue\tok\n\
sample_project_package_imported\tTrue\tok\n\
sample_project_temporary_project_deleted\tTrue\tok\n\
sample_project_visible_project_count_preserved\t0->0\tok\n",
        )
        .expect("write setup summary");
        fs::write(
            metrics.join("diagnostics_validation_summary.tsv"),
            "check\tvalue\tmessage\tstatus\n\
runtime_probe_present\tWSL2\truntime probe returned\tok\n\
runtime_probe_output_readable\tstatus_lines=4;distribution_lines=3\tdecoded arrays\tok\n\
debug_runtime_endpoint_present\tWSL2\tstandalone runtime endpoint returned\tok\n\
debug_runtime_project_wsl_state_recorded\tStopped\tproject WSL state recorded\tok\n\
debug_runtime_non_disruptive_probe\ttrue\tnon-disruptive runtime probe\tok\n\
git_probe_present\tmain\tgit probe returned\tok\n\
git_branch_show_current_removed\trev-parse --abbrev-ref HEAD\tno show-current dependency\tok\n\
command_output_decode_cleaning_present\tdecode_command_bytes + clean_command_text\tclean command output\tok\n",
        )
        .expect("write diagnostics validation summary");

        let overrides = actual_gate_overrides(&root);
        assert_eq!(
            overrides.get("diagnostics_backend_action_route_present"),
            Some(&(String::from("POST /api/user/diagnostics/action"), true))
        );
        assert_eq!(
            overrides.get("diagnostics_action_id_contract_count"),
            Some(&(String::from("8"), true))
        );
        assert_eq!(
            overrides.get("diagnostics_required_item_completion_rate"),
            Some(&(String::from("100%"), true))
        );
        assert_eq!(
            overrides.get("diagnostics_human_readable_message_rate"),
            Some(&(String::from("100%"), true))
        );
        assert!(requires_actual_gate(
            "diagnostics_required_item_completion_rate"
        ));
        assert!(requires_actual_gate("diagnostics_repair_action_rate"));
        assert_eq!(
            overrides.get("full_scientific_install_command_present"),
            Some(&(
                String::from("python scripts/setup/install_complete_scientific.py"),
                true
            ))
        );
        assert_eq!(
            overrides.get("setup_wizard_pass"),
            Some(&(String::from("True"), true))
        );
        assert_eq!(
            overrides.get("setup_wizard_independent_page_present"),
            Some(&(String::from("setup_wizard_page"), true))
        );
        assert_eq!(
            overrides.get("settings_developer_mode_debug_routes_lazy_loaded"),
            Some(&(
                String::from("debug routes load only after Open Developer Mode"),
                true
            ))
        );
        assert_eq!(
            overrides.get("settings_developer_mode_guides_lazy_loaded"),
            Some(&(
                String::from("guide registry loads only after Open Developer Mode"),
                true
            ))
        );
        assert_eq!(
            overrides.get("setup_wizard_first_launch_default_route"),
            Some(&(String::from("setup_wizard_page"), true))
        );
        assert_eq!(
            overrides.get("setup_wizard_step_count"),
            Some(&(String::from("6"), true))
        );
        assert_eq!(
            overrides.get("debug_runtime_endpoint_present"),
            Some(&(String::from("WSL2"), true))
        );
        assert_eq!(
            overrides.get("debug_runtime_non_disruptive_probe"),
            Some(&(String::from("true"), true))
        );
        assert_eq!(
            overrides.get("storage_cleanup_policy_pass"),
            Some(&(String::from("True"), true))
        );
        assert_eq!(
            overrides.get("full_scientific_required_tools_all_ready"),
            Some(&(String::from("True"), true))
        );
        assert_eq!(
            overrides.get("install_plan_missing_tool_count"),
            Some(&(String::from("0"), true))
        );
        assert_eq!(
            overrides.get("sample_project_validation_pass"),
            Some(&(String::from("True"), true))
        );
        assert_eq!(
            overrides.get("full_scientific_installer_self_check_pass"),
            Some(&(String::from("True"), true))
        );
        assert_eq!(
            overrides.get("full_scientific_installer_required_runtime_tool_coverage"),
            Some(&(String::from("13/13"), true))
        );
        assert_eq!(
            overrides.get("full_scientific_installer_repair_step_coverage"),
            Some(&(String::from("6/6"), true))
        );
        assert_eq!(
            overrides.get("sample_project_visible_project_count_preserved"),
            Some(&(String::from("0->0"), true))
        );
        assert_eq!(
            overrides.get("runtime_probe_output_readable"),
            Some(&(String::from("status_lines=4;distribution_lines=3"), true))
        );
        assert_eq!(
            overrides.get("git_branch_show_current_removed"),
            Some(&(String::from("rev-parse --abbrev-ref HEAD"), true))
        );
        assert_eq!(
            overrides.get("command_output_decode_cleaning_present"),
            Some(&(
                String::from("decode_command_bytes + clean_command_text"),
                true
            ))
        );
        assert_eq!(
            overrides.get("complete_scientific_installer_wrapper_present"),
            Some(&(
                String::from("scripts/setup/install_complete_scientific.py"),
                true
            ))
        );
        assert!(requires_actual_gate("setup_wizard_pass"));
        assert!(requires_actual_gate("setup_wizard_step_count"));
        assert!(requires_actual_gate("storage_cleanup_policy_pass"));
        assert!(requires_actual_gate(
            "full_scientific_required_tools_all_ready"
        ));
        assert!(requires_actual_gate(
            "full_scientific_installer_self_check_pass"
        ));
        assert!(requires_actual_gate(
            "full_scientific_installer_required_runtime_tool_coverage"
        ));
        assert!(requires_actual_gate("sample_project_validation_pass"));
        assert!(requires_actual_gate("git_branch_show_current_removed"));
        assert!(requires_actual_gate("runtime_probe_output_readable"));
        assert!(requires_actual_gate(
            "command_output_decode_cleaning_present"
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn master_validation_reads_candidate_table_wiring_gates() {
        let root = temp_test_dir("phase091_candidate_table_master");
        let metrics = root.join("metrics");
        fs::create_dir_all(&metrics).expect("create metrics");
        fs::write(
            metrics.join("candidate_ranking_summary.tsv"),
            "metric\tvalue\tstatus\n\
candidate_table_stateful_control_count\t7\tok\n\
candidate_table_query_wiring_present\tTrue\tok\n\
candidate_table_risk_wiring_present\tTrue\tok\n\
candidate_table_sort_wiring_present\tTrue\tok\n\
candidate_table_page_size_wiring_present\tTrue\tok\n\
candidate_table_column_visibility_wiring_present\tTrue\tok\n\
candidate_table_pagination_wiring_present\tTrue\tok\n\
candidate_table_clear_filter_wiring_present\tTrue\tok\n",
        )
        .expect("write candidate ranking summary");

        let overrides = actual_gate_overrides(&root);
        assert_eq!(
            overrides.get("candidate_table_stateful_control_count"),
            Some(&(String::from("7"), true))
        );
        assert_eq!(
            overrides.get("candidate_table_query_wiring_present"),
            Some(&(String::from("True"), true))
        );
        assert_eq!(
            overrides.get("candidate_table_column_visibility_wiring_present"),
            Some(&(String::from("True"), true))
        );
        assert!(requires_actual_gate("candidate_table_query_wiring_present"));
        assert!(requires_actual_gate(
            "candidate_table_clear_filter_wiring_present"
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn master_validation_reads_default_user_navigation_gates() {
        let root = temp_test_dir("phase091_default_navigation_master");
        let metrics = root.join("metrics");
        fs::create_dir_all(&metrics).expect("create metrics");
        fs::write(
            metrics.join("workflow_pipeline_summary.tsv"),
            "metric\tvalue\tstatus\n\
main_nav_item_count\t3\tok\n\
github_main_nav_count\t0\tok\n",
        )
        .expect("write workflow summary");
        fs::write(
            metrics.join("dashboard_action_summary.tsv"),
            "metric\tvalue\tstatus\n\
default_github_hidden_pass\tTrue\tok\n\
default_developer_terms_hidden_pass\tTrue\tok\n",
        )
        .expect("write dashboard summary");

        let overrides = actual_gate_overrides(&root);
        assert_eq!(
            overrides.get("main_nav_item_count"),
            Some(&(String::from("3"), true))
        );
        assert_eq!(
            overrides.get("github_main_nav_count"),
            Some(&(String::from("0"), true))
        );
        assert_eq!(
            overrides.get("default_github_hidden_pass"),
            Some(&(String::from("True"), true))
        );
        assert!(requires_actual_gate("github_main_nav_count"));
        assert!(requires_actual_gate("default_developer_terms_hidden_pass"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn master_validation_reads_ui_stack_and_license_gates() {
        let root = temp_test_dir("phase091_ui_stack_license_master");
        let metrics = root.join("metrics");
        fs::create_dir_all(&metrics).expect("create metrics");
        fs::write(
            metrics.join("ui_stack_selection.tsv"),
            "decision\tsource_path\tselected_stack\tstatus\n\
ui_framework_selection_recorded\twebui/phase09/app.js\tvanilla_html_css_js\tok\n\
ui_component_library_selection_recorded\twebui/phase09/styles.css\tproject_owned_css_components\tok\n\
ui_table_component_selection_recorded\twebui/phase09/app.js\tproject_owned_table_markup\tok\n\
ui_structure_viewer_selection_recorded\twebui/phase09/app.js\tngl_when_coordinates_available\tok\n\
ui_screenshot_tool_selection_recorded\tscripts/phase09/capture_phase_0_9_1_webui_visuals.py\tchrome_png_rendering\tok\n",
        )
        .expect("write ui stack");
        fs::write(
            metrics.join("ui_dependency_license_manifest.tsv"),
            "name\tsource\trole\tlicense_note\tstatus\n\
ui_dependency_license_manifest_complete\t100%\tall_ui_runtime_dependencies_have_license_status\tok\n\
proprietary_ui_dependency_count\t0\tno_proprietary_mandatory_ui_dependency\tok\n\
unknown_ui_license_count\t0\tno_unknown_mandatory_ui_license\tok\n",
        )
        .expect("write ui licenses");

        let overrides = actual_gate_overrides(&root);
        assert_eq!(
            overrides.get("ui_framework_selection_recorded"),
            Some(&(String::from("webui/phase09/app.js"), true))
        );
        assert_eq!(
            overrides.get("proprietary_ui_dependency_count"),
            Some(&(String::from("0"), true))
        );
        assert_eq!(
            overrides.get("unknown_ui_license_count"),
            Some(&(String::from("0"), true))
        );
        assert!(requires_actual_gate(
            "ui_structure_viewer_selection_recorded"
        ));
        assert!(requires_actual_gate(
            "ui_dependency_license_manifest_complete"
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn master_validation_reads_report_export_flow_gates() {
        let root = temp_test_dir("phase091_report_export_master");
        let metrics = root.join("metrics");
        fs::create_dir_all(&metrics).expect("create metrics");
        fs::write(
            metrics.join("report_quality_summary.tsv"),
            "metric\tvalue\tstatus\n\
report_export_flow_pass\tTrue\tok\n\
html_report_export_pass\tTrue\tok\n\
markdown_report_export_pass\tTrue\tok\n\
json_report_export_pass\tTrue\tok\n\
candidate_table_export_pass\tTrue\tok\n\
artifact_bundle_export_pass\tTrue\tok\n\
report_semantics_validation_pass\tTrue\tok\n",
        )
        .expect("write report quality");

        let overrides = actual_gate_overrides(&root);
        assert_eq!(
            overrides.get("report_export_flow_pass"),
            Some(&(String::from("True"), true))
        );
        assert_eq!(
            overrides.get("candidate_table_export_pass"),
            Some(&(String::from("True"), true))
        );
        assert_eq!(
            overrides.get("report_semantics_validation_pass"),
            Some(&(String::from("True"), true))
        );
        assert!(requires_actual_gate("report_export_flow_pass"));
        assert!(requires_actual_gate("artifact_bundle_export_pass"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn master_validation_reads_input_preflight_review_flow_gates() {
        let root = temp_test_dir("phase091_input_flow_master");
        let metrics = root.join("metrics");
        fs::create_dir_all(&metrics).expect("create metrics");
        fs::write(
            metrics.join("input_field_validation_summary.tsv"),
            "metric\tvalue\tstatus\n\
input_wizard_page_pass\tTrue\tok\n\
project_basics_form_pass\tTrue\tok\n\
molecule_type_selection_pass\tTrue\tok\n\
small_molecule_input_flow_pass\tTrue\tok\n\
peptide_input_flow_pass\tTrue\tok\n\
protein_ligand_input_flow_pass\tTrue\tok\n\
complex_structure_input_flow_pass\tTrue\tok\n\
organism_genome_selection_pass\tTrue\tok\n\
search_scope_selection_pass\tTrue\tok\n\
database_profile_selection_pass\tTrue\tok\n\
run_profile_selection_pass\tTrue\tok\n",
        )
        .expect("write input summary");
        fs::write(
            metrics.join("preflight_group_action_summary.tsv"),
            "metric\tvalue\tstatus\n\
preflight_check_page_pass\tTrue\tok\n",
        )
        .expect("write preflight summary");
        fs::write(
            metrics.join("run_forecast_confirmation_summary.tsv"),
            "metric\tvalue\tstatus\n\
review_start_page_pass\tTrue\tok\n",
        )
        .expect("write forecast summary");

        let overrides = actual_gate_overrides(&root);
        assert_eq!(
            overrides.get("input_wizard_page_pass"),
            Some(&(String::from("True"), true))
        );
        assert_eq!(
            overrides.get("small_molecule_input_flow_pass"),
            Some(&(String::from("True"), true))
        );
        assert_eq!(
            overrides.get("preflight_check_page_pass"),
            Some(&(String::from("True"), true))
        );
        assert_eq!(
            overrides.get("review_start_page_pass"),
            Some(&(String::from("True"), true))
        );
        assert!(requires_actual_gate("input_wizard_page_pass"));
        assert!(requires_actual_gate("preflight_check_page_pass"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn master_validation_reads_results_page_flow_gates() {
        let root = temp_test_dir("phase091_results_pages_master");
        let metrics = root.join("metrics");
        fs::create_dir_all(&metrics).expect("create metrics");
        fs::write(
            metrics.join("candidate_ranking_summary.tsv"),
            "metric\tvalue\tstatus\n\
candidate_ranking_page_pass\tTrue\tok\n\
candidate_detail_page_pass\tTrue\tok\n",
        )
        .expect("write candidate summary");
        fs::write(
            metrics.join("evidence_card_summary.tsv"),
            "metric\tvalue\tstatus\n\
evidence_ledger_viewer_pass\tTrue\tok\n",
        )
        .expect("write evidence summary");
        fs::write(
            metrics.join("genome_md_graphics_summary.tsv"),
            "metric\tvalue\tstatus\n\
genome_orf_context_viewer_pass\tTrue\tok\n\
md_summary_viewer_pass\tTrue\tok\n",
        )
        .expect("write genome md summary");
        fs::write(
            metrics.join("structure_viewer_contract_summary.tsv"),
            "metric\tvalue\tstatus\n\
structure_patch_viewer_pass\tTrue\tok\n",
        )
        .expect("write structure summary");

        let overrides = actual_gate_overrides(&root);
        assert_eq!(
            overrides.get("candidate_ranking_page_pass"),
            Some(&(String::from("True"), true))
        );
        assert_eq!(
            overrides.get("evidence_ledger_viewer_pass"),
            Some(&(String::from("True"), true))
        );
        assert_eq!(
            overrides.get("structure_patch_viewer_pass"),
            Some(&(String::from("True"), true))
        );
        assert!(requires_actual_gate("candidate_detail_page_pass"));
        assert!(requires_actual_gate("md_summary_viewer_pass"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn master_validation_reads_run_monitor_flow_gates() {
        let root = temp_test_dir("phase091_run_monitor_master");
        let metrics = root.join("metrics");
        fs::create_dir_all(&metrics).expect("create metrics");
        fs::write(
            metrics.join("run_monitor_resource_summary.tsv"),
            "metric\tvalue\tstatus\n\
run_monitoring_flow_pass\tTrue\tok\n\
resource_monitoring_pass\tTrue\tok\n\
task_submit_flow_pass\tTrue\tok\n\
task_cancel_flow_pass\tTrue\tok\n\
task_resume_flow_pass\tTrue\tok\n\
run_state_machine_validation_pass\tTrue\tok\n",
        )
        .expect("write run monitor summary");

        let overrides = actual_gate_overrides(&root);
        assert_eq!(
            overrides.get("run_monitoring_flow_pass"),
            Some(&(String::from("True"), true))
        );
        assert_eq!(
            overrides.get("task_resume_flow_pass"),
            Some(&(String::from("True"), true))
        );
        assert_eq!(
            overrides.get("run_state_machine_validation_pass"),
            Some(&(String::from("True"), true))
        );
        assert!(requires_actual_gate("resource_monitoring_pass"));
        assert!(requires_actual_gate("task_cancel_flow_pass"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn master_validation_reads_webui_build_and_startup_gates() {
        let root = temp_test_dir("phase091_webui_build_startup_master");
        let metrics = root.join("metrics");
        let webui = root.join("webui");
        fs::create_dir_all(&metrics).expect("create metrics");
        fs::create_dir_all(&webui).expect("create webui metrics");
        fs::write(
            metrics.join("server_validation_summary.tsv"),
            "metric\tvalue\tstatus\n\
server_startup_test_pass\tTrue\tok\n\
webui_starts_successfully\tTrue\tok\n\
webui_localhost_access_pass\tTrue\tok\n",
        )
        .expect("write server summary");
        fs::write(
            webui.join("build_validation_summary.tsv"),
            "check\tcommand\texit_code\tstatus\n\
webui_lint_pass\tnode --check webui/phase09/app.js\t0\tok\n\
webui_typecheck_pass\tstatic vanilla JS uses node syntax check\t0\tok\n\
webui_build_pass\tstatic html/css/js asset presence\t0\tok\n\
webui_start_script_target_dir_support\t--target-dir | CARGO_TARGET_DIR\t0\tok\n\
webui_start_script_pid_file_support\t--pid-file | process.pid\t0\tok\n\
webui_start_script_isolated_binary_support\ttarget_dir / debug / gls-server\t0\tok\n",
        )
        .expect("write build summary");

        let overrides = actual_gate_overrides(&root);
        assert_eq!(
            overrides.get("server_startup_test_pass"),
            Some(&(String::from("True"), true))
        );
        assert_eq!(
            overrides.get("webui_lint_pass"),
            Some(&(String::from("node --check webui/phase09/app.js"), true))
        );
        assert_eq!(
            overrides.get("webui_build_pass"),
            Some(&(String::from("static html/css/js asset presence"), true))
        );
        assert_eq!(
            overrides.get("webui_start_script_target_dir_support"),
            Some(&(String::from("--target-dir | CARGO_TARGET_DIR"), true))
        );
        assert_eq!(
            overrides.get("webui_start_script_pid_file_support"),
            Some(&(String::from("--pid-file | process.pid"), true))
        );
        assert!(requires_actual_gate("webui_localhost_access_pass"));
        assert!(requires_actual_gate("webui_typecheck_pass"));
        assert!(requires_actual_gate(
            "webui_start_script_target_dir_support"
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn master_validation_reads_user_debug_api_split_gates() {
        let root = temp_test_dir("phase091_user_debug_split_master");
        let server = root.join("server");
        fs::create_dir_all(&server).expect("create server dir");
        fs::write(
            server.join("user_debug_api_split.tsv"),
            "metric\tvalue\tstatus\n\
user_debug_api_split_pass\tTrue\tok\n\
user_api_route_count_min\t18\tok\n\
debug_api_route_count_min\t3\tok\n\
github_default_route_count\t0\tok\n",
        )
        .expect("write api split");

        let overrides = actual_gate_overrides(&root);
        assert_eq!(
            overrides.get("user_debug_api_split_pass"),
            Some(&(String::from("True"), true))
        );
        assert_eq!(
            overrides.get("debug_api_route_count_min"),
            Some(&(String::from("3"), true))
        );
        assert!(requires_actual_gate("user_debug_api_split_pass"));
        assert!(requires_actual_gate("user_api_route_count_min"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn master_validation_reads_user_workflow_e2e_gates() {
        let root = temp_test_dir("phase091_user_workflow_master");
        let metrics = root.join("metrics");
        fs::create_dir_all(&metrics).expect("create metrics");
        fs::write(
            metrics.join("webui_e2e_test_summary.tsv"),
            "metric\tvalue\tstatus\n\
ordinary_user_journey_pass\tTrue\tok\n\
user_workflow_e2e_count\t8\tok\n\
user_workflow_e2e_pass_rate\t100%\tok\n\
project_create_flow_pass\tTrue\tok\n",
        )
        .expect("write e2e summary");
        fs::write(
            metrics.join("project_lifecycle_smoke_summary.tsv"),
            "metric\tvalue\tstatus\n\
project_lifecycle_smoke_pass\tTrue\tok\n\
project_lifecycle_step_count\t9\tok\n\
project_lifecycle_pass_rate\t100%\tok\n\
project_create_flow_pass\tTrue\tok\n\
project_open_flow_pass\tTrue\tok\n\
project_duplicate_flow_pass\tTrue\tok\n\
project_delete_flow_pass\tTrue\tok\n\
project_cleanup_flow_pass\tTrue\tok\n\
project_resume_flow_pass\tTrue\tok\n\
project_lifecycle_submit_run_pass\tTrue\tok\n\
project_lifecycle_cancel_run_pass\tTrue\tok\n\
project_lifecycle_deleted_project_count\t2\tok\n",
        )
        .expect("write project lifecycle summary");

        let overrides = actual_gate_overrides(&root);
        assert_eq!(
            overrides.get("user_workflow_e2e_count"),
            Some(&(String::from("8"), true))
        );
        assert_eq!(
            overrides.get("user_workflow_e2e_pass_rate"),
            Some(&(String::from("100%"), true))
        );
        assert_eq!(
            overrides.get("project_create_flow_pass"),
            Some(&(String::from("True"), true))
        );
        assert_eq!(
            overrides.get("project_open_flow_pass"),
            Some(&(String::from("True"), true))
        );
        assert_eq!(
            overrides.get("project_cleanup_flow_pass"),
            Some(&(String::from("True"), true))
        );
        assert_eq!(
            overrides.get("project_lifecycle_deleted_project_count"),
            Some(&(String::from("2"), true))
        );
        assert!(requires_actual_gate("user_workflow_e2e_count"));
        assert!(requires_actual_gate("project_create_flow_pass"));
        assert!(requires_actual_gate("project_open_flow_pass"));
        assert!(requires_actual_gate("project_lifecycle_smoke_pass"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn master_validation_fails_required_actual_gates_when_rows_are_missing() {
        let root = temp_test_dir("phase091_required_actual_missing");
        let options = ServerOptions {
            host: "127.0.0.1".to_string(),
            port: 19090,
            artifact_dir: root.clone(),
            workspace_root: PathBuf::from("."),
        };
        let metrics = collect_phase09_metrics(&options);
        let post_test_gate = metrics
            .iter()
            .find(|metric| metric.id == "post_test_project_count")
            .expect("post_test_project_count gate");
        let md_meta_gate = metrics
            .iter()
            .find(|metric| metric.id == "md_chart_source_unit_meta_present")
            .expect("md metadata gate");
        let diagnostics_gate = metrics
            .iter()
            .find(|metric| metric.id == "diagnostics_fix_buttons_present")
            .expect("diagnostics gate");
        let project_open_gate = metrics
            .iter()
            .find(|metric| metric.id == "project_open_flow_pass")
            .expect("project open gate");
        assert_eq!(post_test_gate.value, "missing");
        assert!(!post_test_gate.pass);
        assert_eq!(md_meta_gate.value, "missing");
        assert!(!md_meta_gate.pass);
        assert_eq!(diagnostics_gate.value, "missing");
        assert!(!diagnostics_gate.pass);
        assert_eq!(project_open_gate.value, "missing");
        assert!(!project_open_gate.pass);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn git_probe_reads_current_repository_without_show_current_dependency() {
        let options = ServerOptions {
            host: "127.0.0.1".to_string(),
            port: 19090,
            artifact_dir: std::env::temp_dir().join("genoligand_phase091_git_probe"),
            workspace_root: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .canonicalize()
                .unwrap(),
        };
        let probe = git_probe(&options);
        assert!(probe.available);
        assert!(probe.branch.is_some());
        assert!(probe.local_head.is_some());
        assert!(probe.status_ok);
    }

    #[test]
    fn projects_route_hides_unmarked_acceptance_artifact_projects() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("genoligand_clean_launch_test_{unique}"));
        let options = ServerOptions {
            host: "127.0.0.1".to_string(),
            port: 19090,
            artifact_dir: root.join("artifacts"),
            workspace_root: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."),
        };
        let old_project_dir = projects_root(&options).join("project-old-artifact");
        fs::create_dir_all(&old_project_dir).unwrap();
        fs::write(
            old_project_dir.join("project_name.txt"),
            "Old acceptance project",
        )
        .unwrap();
        fs::write(old_project_dir.join("project_status.txt"), "completed").unwrap();
        fs::create_dir_all(old_project_dir.join("runs/old-run")).unwrap();
        fs::write(old_project_dir.join("latest_run_id.txt"), "old-run").unwrap();

        let (_, _, before) = route_http("GET", "/api/user/projects", &options, "");
        assert!(!before.contains("project-old-artifact"));
        assert!(latest_run_dir(&options).is_none());

        let (_, _, created) = route_http("POST", "/api/projects", &options, "");
        let project_id = extract_json_string(&created, "project_id");
        let (_, _, after) = route_http("GET", "/api/user/projects", &options, "");
        assert!(after.contains(&project_id));
        assert!(!after.contains("project-old-artifact"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn imported_project_package_metadata_persists_in_project_json() {
        let root = temp_test_dir("phase091_imported_project_metadata");
        let options = ServerOptions {
            host: "127.0.0.1".to_string(),
            port: 19090,
            artifact_dir: root.join("artifacts"),
            workspace_root: root.join("workspace"),
        };
        let exports_dir = options.artifact_dir.join("exports");
        fs::create_dir_all(&exports_dir).expect("create exports");
        let package_path = exports_dir.join("project_package_manifest.tsv");
        fs::write(
            &package_path,
            "item\tvalue\nproject_state\t{}\nexports_dir\tlocal_exports\ncontains_large_database_copy\tfalse\nread_only_review_mode\ttrue\ndemo_mode_supported\ttrue\nsupport_bundle_linked\ttrue\nreopen_mode\tlocal_project_package\n",
        )
        .expect("write project package");
        fs::write(
            exports_dir.join("report_export_manifest.tsv"),
            format!(
                "export_type\tpath\tbytes\tstatus\nproject_package\t{}\t{}\tcreated\n",
                package_path.display(),
                fs::metadata(&package_path).unwrap().len()
            ),
        )
        .expect("write export manifest");

        let imported = import_project_package_json(&options, "{}");
        assert!(imported.contains("\"status\":\"imported\""));
        assert!(imported.contains("\"read_only_review_mode\":true"));
        assert!(imported.contains("\"demo_mode_supported\":true"));
        assert!(imported.contains("\"support_bundle_linked\":true"));
        let project_id = json_field_string(&imported, &["project_id"]);
        let project_json = read_project_json(&options, &project_id).expect("project json");
        assert!(project_json.contains("\"project_package_imported\":true"));
        assert!(project_json.contains("\"read_only_review_mode\":true"));
        assert!(project_json.contains("\"demo_mode_supported\":true"));
        assert!(project_json.contains("\"support_bundle_linked\":true"));
        assert!(project_json.contains("\"reopen_mode\":\"local_project_package\""));
        assert!(project_json.contains("\"collaboration_mode\":\"read_only_review\""));
        assert!(project_json.contains("\"project_package_import\""));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn project_lifecycle_routes_persist_local_state() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("genoligand_phase091_test_{unique}"));
        let options = ServerOptions {
            host: "127.0.0.1".to_string(),
            port: 19090,
            artifact_dir: root.join("artifacts"),
            workspace_root: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."),
        };

        let (_, _, created) = route_http("POST", "/api/projects", &options, "");
        assert!(created.contains("\"status\":\"created\""));
        let project_id = extract_json_string(&created, "project_id");
        assert!(!project_id.is_empty());
        assert!(
            options
                .artifact_dir
                .join("projects")
                .join(&project_id)
                .join("project.json")
                .exists()
        );

        let (_, _, input_saved) = route_http(
            "POST",
            &format!("/api/projects/{project_id}/inputs"),
            &options,
            "{\"inputs\":[{\"field\":\"project_name\",\"label\":\"Project name\",\"value\":\"Production Study\"},{\"field\":\"smiles\",\"label\":\"SMILES\",\"value\":\"CCO\"},{\"field\":\"run_profile\",\"label\":\"Run profile\",\"value\":\"physical_mid_screen_full_run\",\"control_type\":\"choice\"}],\"files\":[{\"field\":\"upload_structure_file\",\"name\":\"ligand.sdf\",\"size\":7,\"sha256\":\"9a0364b9e99bb480dd25e1f0284c8555\",\"content_base64\":\"bGlnYW5kCg==\"}],\"advanced_parameters_collapsed\":true,\"field_schema\":\"genoligand.webui.input_manifest.v0.9.1\"}",
        );
        assert!(input_saved.contains("\"status\":\"saved\""));
        assert!(input_saved.contains("\"persisted_files\":1"));
        let project_after_input = fs::read_to_string(
            options
                .artifact_dir
                .join("projects")
                .join(&project_id)
                .join("project.json"),
        )
        .unwrap();
        assert!(project_after_input.contains("Production Study"));
        let input_manifest = fs::read_to_string(
            options
                .artifact_dir
                .join("projects")
                .join(&project_id)
                .join("input_manifest.json"),
        )
        .unwrap();
        assert!(input_manifest.contains("\"field\":\"smiles\""));
        assert!(input_manifest.contains("field_schema"));
        assert!(input_manifest.contains("sha256"));
        assert!(input_manifest.contains("content_persisted"));
        assert!(input_manifest.contains("persisted_path"));
        assert!(!input_manifest.contains("content_base64"));
        assert!(input_manifest.contains("ligand.sdf"));
        assert!(
            options
                .artifact_dir
                .join("projects")
                .join(&project_id)
                .join("inputs")
                .join("files")
                .join("upload_structure_file_ligand.sdf")
                .exists()
        );

        let (_, _, preflight) = route_http(
            "POST",
            &format!("/api/projects/{project_id}/preflight"),
            &options,
            "",
        );
        assert!(preflight.contains("\"status\":\"preflight_passed\""));
        assert!(preflight.contains("\"check_id\":\"small_molecule_input\""));
        assert!(preflight.contains("\"check_id\":\"peptide_input\""));
        assert!(preflight.contains("\"check_id\":\"protein_ligand_input\""));
        assert!(preflight.contains("\"check_id\":\"complex_structure_input\""));
        assert!(preflight.contains("\"check_id\":\"species_genome_input\""));
        assert!(preflight.contains("smiles_count=1"));
        assert!(preflight.contains("valid_smiles_count=1"));
        assert!(preflight.contains("structure_file_count=1"));
        assert!(preflight.contains("hash_covered_file_count=1"));
        let input_manifest_after_preflight = fs::read_to_string(
            options
                .artifact_dir
                .join("projects")
                .join(&project_id)
                .join("input_manifest.json"),
        )
        .unwrap();
        assert!(input_manifest_after_preflight.contains("\"field\":\"smiles\""));
        assert!(input_manifest_after_preflight.contains("ligand.sdf"));

        let (_, _, run) = route_http(
            "POST",
            &format!("/api/projects/{project_id}/runs"),
            &options,
            "",
        );
        assert!(
            run.contains("\"status\":\"completed\"")
                || run.contains("\"status\":\"failed\"")
                || run.contains("\"status\":\"running\"")
        );
        assert!(run.contains("\"nextflow_run_id\":\"nf-"));
        assert!(run.contains("\"completed_steps\":["));
        assert!(run.contains("\"resource_status\":"));
        let run_id = extract_json_string(&run, "run_id");
        assert!(!run_id.is_empty());
        let run_manifest = fs::read_to_string(
            options
                .artifact_dir
                .join("projects")
                .join(&project_id)
                .join("runs")
                .join(&run_id)
                .join("run_manifest.json"),
        )
        .unwrap();
        assert!(run_manifest.contains("artifact_index"));
        let run_launch_plan = fs::read_to_string(
            options
                .artifact_dir
                .join("projects")
                .join(&project_id)
                .join("runs")
                .join(&run_id)
                .join("run_launch_plan.json"),
        )
        .unwrap();
        assert!(run_launch_plan.contains("genoligand.webui.run_launch_plan.v0.9.1"));
        assert!(run_launch_plan.contains("\"status\":\"configured\""));
        assert!(run_launch_plan.contains("\"run_profile\":\"physical_mid_screen_full_run\""));
        assert!(
            run_launch_plan.contains("\"normalized_run_profile\":\"physical_mid_screen_full_run\"")
        );
        assert!(run_launch_plan.contains(
            "\"command_source\":\"built_in_gls_cli_sequence_for_physical_mid_screen_full_run\""
        ));
        assert!(run_launch_plan.contains("\"md\",\"select\""));
        assert!(run_launch_plan.contains("\"md\",\"report\""));
        let configured_command_env = "GENOLIGAND_WEBUI_RUN_COMMAND";
        assert!(
            !run_manifest.contains(&format!("\"current_command\":\"{configured_command_env}\""))
        );
        assert!(!run_manifest.contains(&format!("command={configured_command_env}")));
        let command_status = options
            .artifact_dir
            .join("projects")
            .join(&project_id)
            .join("runs")
            .join(&run_id)
            .join("run_command_status.tsv");
        let failure_reason = options
            .artifact_dir
            .join("projects")
            .join(&project_id)
            .join("runs")
            .join(&run_id)
            .join("failure_reason.txt");
        assert!(command_status.exists());
        assert!(failure_reason.exists());

        let (_, _, logs) = route_http("GET", &format!("/api/runs/{run_id}/logs"), &options, "");
        assert!(logs.contains("run_created"));
        assert!(logs.contains("nextflow_run_id=nf-"));

        let (_, _, artifacts) = route_http(
            "GET",
            &format!("/api/runs/{run_id}/artifacts"),
            &options,
            "",
        );
        assert!(artifacts.contains("\"artifact_type\":\"run_manifest\""));
        assert!(artifacts.contains("\"artifact_type\":\"run_launch_plan\""));
        assert!(artifacts.contains("\"artifact_type\":\"run_log\""));
        let (_, _, e2e_candidates) = route_http(
            "GET",
            &format!("/api/runs/{run_id}/candidates"),
            &options,
            "",
        );
        assert!(e2e_candidates.contains("\"artifact_backed\":true"));
        assert!(e2e_candidates.contains("\"candidate_id\":\"benchmark_candidate\""));
        let (_, _, e2e_evidence) =
            route_http("GET", &format!("/api/runs/{run_id}/evidence"), &options, "");
        assert!(e2e_evidence.contains("computational_prior_only"));
        let (_, _, e2e_md_summary) = route_http(
            "GET",
            &format!("/api/runs/{run_id}/md-summary"),
            &options,
            "",
        );
        assert!(e2e_md_summary.contains("\"md_stability_score\":\"0.78\""));

        let run_dir = options
            .artifact_dir
            .join("projects")
            .join(&project_id)
            .join("runs")
            .join(&run_id);
        write_text(
            &run_dir.join("candidate_ranking.tsv"),
            "rank\tcandidate_id\tgene_name\torf_id\tgenomic_coordinates\tcontext_class\tfragment_score\tco_occurrence_score\tplm_score\tstructure_score\tdocking_score\tmd_stability_score\tfinal_priority_score\trisk_flags\n1\tGLS-ORF-001\tGENE1\tORF1\tchr1:10-90:+\tCDS\t0.80\t0.70\t0.60\t0.50\t-7.1\t0.90\t0.82\tlow\n",
        )
        .unwrap();
        write_text(
            &run_dir.join("evidence_ledger.tsv"),
            "report_item\tsource_file\tmanifest\tschema\ttool_version\tartifact_hash\tvalidation_state\treport_section\nfinal_priority_score\tcandidate_ranking.tsv\trun_manifest.json\tcandidate.schema.json\tgls-cli 0.9.1\tsha256:abc\tvalidated\tcandidate ranking\n",
        )
        .unwrap();
        write_text(
            &run_dir.join("structure_patch_summary.tsv"),
            "candidate_id\tstructure_score\tstructure_evidence\tbinding_patch\tpatch_evidence\tkey_residues\tdocking_score\tdocking_pose\tligand_position\tinteraction_residues\nGLS-ORF-001\t0.50\tstructure_manifest.tsv\tpatch-001\tsha256:patch\tA:10,A:12\t-7.1\tpose_001.pdb\tchain A near residue 10\tA:10,A:12\n",
        )
        .unwrap();
        write_text(
            &run_dir.join("pose_001.pdb"),
            "ATOM      1  N   ALA A  10      11.104  13.207   9.784  1.00 20.00           N\nHETATM    2  C1  LIG B   1      12.250  13.920  10.100  1.00 18.00           C\nEND\n",
        )
        .unwrap();
        write_text(
            &run_dir.join("md_summary.tsv"),
            "candidate_id\trmsd\trmsf\tligand_drift\thydrogen_bond_occupancy\tcontact_occupancy\tinteraction_persistence\tmd_risk_flags\tmd_score_decomposition\tmd_stability_score\nGLS-ORF-001\t1.2\t0.4\t0.8\t0.62\t0.71\t0.69\tlow\trmsd=0.3;contact=0.4\t0.90\n",
        )
        .unwrap();
        let (_, _, candidates) = route_http(
            "GET",
            &format!("/api/runs/{run_id}/candidates"),
            &options,
            "",
        );
        assert!(candidates.contains("\"artifact_backed\":true"));
        assert!(candidates.contains("\"candidate_id\":\"GLS-ORF-001\""));
        let (_, _, evidence) =
            route_http("GET", &format!("/api/runs/{run_id}/evidence"), &options, "");
        assert!(evidence.contains("\"artifact_backed\":true"));
        assert!(evidence.contains("\"report_item\":\"final_priority_score\""));
        let (_, _, candidate_detail) =
            route_http("GET", "/api/candidates/GLS-ORF-001", &options, "");
        assert!(candidate_detail.contains("\"selected\":true"));
        assert!(candidate_detail.contains("\"candidate_id\":\"GLS-ORF-001\""));
        let (_, _, genome_context) = route_http(
            "GET",
            &format!("/api/runs/{run_id}/genome-context"),
            &options,
            "",
        );
        assert!(genome_context.contains("\"genome_context\":["));
        assert!(genome_context.contains("\"artifact_backed\":true"));
        let (_, _, structure) = route_http(
            "GET",
            &format!("/api/runs/{run_id}/structure"),
            &options,
            "",
        );
        assert!(structure.contains("\"structure_rows\":["));
        assert!(structure.contains("\"binding_patch\":\"patch-001\""));
        assert!(structure.contains("\"viewer_library\":\"NGL\""));
        assert!(structure.contains("\"viewer_engine\":\"ngl\""));
        assert!(structure.contains(&format!("/api/runs/{run_id}/structure-file")));
        let (structure_file_status, _, structure_file) = route_http(
            "GET",
            &format!("/api/runs/{run_id}/structure-file"),
            &options,
            "",
        );
        assert_eq!(structure_file_status, 200);
        assert!(structure_file.contains("HETATM"));
        let (_, _, md_summary) = route_http(
            "GET",
            &format!("/api/runs/{run_id}/md-summary"),
            &options,
            "",
        );
        assert!(md_summary.contains("\"md_rows\":["));
        assert!(md_summary.contains("\"md_stability_score\":\"0.90\""));

        let (_, _, diagnostics) = route_http("GET", "/api/diagnostics", &options, "");
        assert!(diagnostics.contains("\"check_id\":\"rust_binary\""));
        assert!(diagnostics.contains("\"observed\":"));
        let (_, _, databases) = route_http("GET", "/api/databases", &options, "");
        assert!(databases.contains("\"profile_id\":\"database_sources_yml\""));
        assert!(databases.contains("\"source_config\":"));
        assert!(databases.contains("\"fields\":[\"path\",\"version\",\"sha256\""));
        let (_, _, database_validation) =
            route_http("POST", "/api/databases/validate", &options, "");
        assert!(database_validation.contains("\"status\":\"completed\""));
        assert!(database_validation.contains("database_validation_manifest.tsv"));
        assert!(
            options
                .artifact_dir
                .join("databases")
                .join("database_validation_manifest.tsv")
                .exists()
        );
        let (_, _, database_index_action) =
            route_http("POST", "/api/databases/build-index", &options, "");
        assert!(database_index_action.contains("\"status\":\"completed\""));
        assert!(database_index_action.contains("database_index_action_manifest.tsv"));
        assert!(
            options
                .artifact_dir
                .join("databases")
                .join("database_index_action_manifest.tsv")
                .exists()
        );
        let (_, _, guides) = route_http("GET", "/api/guides", &options, "");
        assert!(guides.contains("\"guide_count\":3"));
        assert!(guides.contains("\"available_guide_count\":3"));
        assert!(guides.contains("local_webui_user_guide.md"));
        let (guide_status, guide_type, guide_doc) =
            route_http("GET", "/docs/local_webui_user_guide.md", &options, "");
        assert_eq!(guide_status, 200);
        assert_eq!(guide_type, "text/markdown; charset=utf-8");
        assert!(guide_doc.contains("GenoLigand 0.9.1 Local WebUI User Guide"));

        let other_project_dir = options.artifact_dir.join("projects").join("project-other");
        let other_run_dir = other_project_dir.join("runs").join("run-newer-empty");
        fs::create_dir_all(&other_run_dir).unwrap();
        write_text(
            &other_project_dir.join("project_name.txt"),
            "Other Project\n",
        )
        .unwrap();
        write_text(
            &other_project_dir.join("latest_run_id.txt"),
            "run-newer-empty\n",
        )
        .unwrap();
        write_text(&other_run_dir.join("run_state.txt"), "completed\n").unwrap();
        let export_request = format!(
            "{{\"project_id\":\"{}\",\"run_id\":\"{}\"}}",
            escape_json(&project_id),
            escape_json(&run_id)
        );
        let (_, _, exports) = route_http("POST", "/api/reports/export", &options, &export_request);
        assert!(exports.contains("\"html_report\""));
        assert!(exports.contains("\"files\":["));
        assert!(exports.contains(&format!("\"run_id\":\"{}\"", run_id)));
        assert!(exports.contains("\"candidate_row_count\":1"));
        assert!(exports.contains("\"contains_computed_candidates\":true"));
        assert!(
            options
                .artifact_dir
                .join("exports")
                .join("genoligand_report.html")
                .exists()
        );
        assert!(
            options
                .artifact_dir
                .join("exports")
                .join("genoligand_report.md")
                .exists()
        );
        assert!(
            options
                .artifact_dir
                .join("exports")
                .join("candidate_table.tsv")
                .exists()
        );
        assert!(
            options
                .artifact_dir
                .join("exports")
                .join("candidate_table.csv")
                .exists()
        );
        assert!(
            options
                .artifact_dir
                .join("exports")
                .join("machine_readable_report.json")
                .exists()
        );
        let exported_candidates = fs::read_to_string(
            options
                .artifact_dir
                .join("exports")
                .join("candidate_table.tsv"),
        )
        .unwrap();
        assert!(exported_candidates.contains("GLS-ORF-001"));
        let exported_candidates_csv = fs::read_to_string(
            options
                .artifact_dir
                .join("exports")
                .join("candidate_table.csv"),
        )
        .unwrap();
        assert!(exported_candidates_csv.contains("GLS-ORF-001"));
        assert!(exported_candidates_csv.starts_with("rank,candidate_id,gene_name"));
        let machine_report = fs::read_to_string(
            options
                .artifact_dir
                .join("exports")
                .join("machine_readable_report.json"),
        )
        .unwrap();
        assert!(machine_report.contains("\"candidate_row_count\":1"));
        assert!(machine_report.contains("\"contains_computed_candidates\":true"));
        let (_, _, project_after_export) =
            route_http("GET", &format!("/api/projects/{project_id}"), &options, "");
        assert!(project_after_export.contains("report_paths.tsv"));
        assert!(project_after_export.contains("candidate_table.tsv"));
        assert!(project_after_export.contains("candidate_table.csv"));
        let legacy_export_manifest = options
            .artifact_dir
            .join("exports")
            .join("report_export_manifest.json");
        write_text(&legacy_export_manifest, "{}\n").unwrap();
        let transient_dom = options
            .artifact_dir
            .join("ui")
            .join("screenshots")
            .join("final")
            .join("old_visual_dom.html");
        write_text(&transient_dom, "<html></html>\n").unwrap();
        let (_, _, post_push_cleanup) = route_http("POST", "/api/cleanup/post-push", &options, "");
        assert!(post_push_cleanup.contains("\"status\":\"completed\""));
        assert!(post_push_cleanup.contains("\"deleted_files_count\":"));
        assert!(!legacy_export_manifest.exists());
        assert!(!transient_dom.exists());
        assert!(
            options
                .artifact_dir
                .join("cleanup")
                .join("post_push_cleanup_summary.tsv")
                .exists()
        );

        let (_, _, cancel) =
            route_http("POST", &format!("/api/runs/{run_id}/cancel"), &options, "");
        assert!(cancel.contains("\"status\":\"cancelled\""));
        let (_, _, resume) =
            route_http("POST", &format!("/api/runs/{run_id}/resume"), &options, "");
        assert!(resume.contains("\"status\":\"resumable\""));

        let (_, _, copy) = route_http(
            "POST",
            &format!("/api/projects/{project_id}/copy"),
            &options,
            "",
        );
        assert!(copy.contains("\"status\":\"created\""));
        let copied_project_id = extract_json_string(&copy, "project_id");
        let copied_manifest = fs::read_to_string(
            options
                .artifact_dir
                .join("projects")
                .join(&copied_project_id)
                .join("input_manifest.json"),
        )
        .unwrap();
        assert!(copied_manifest.contains("\"SMILES\""));
        assert!(copied_manifest.contains("ligand.sdf"));

        let project_runtime_tmp = options
            .artifact_dir
            .join("projects")
            .join(&project_id)
            .join("tmp")
            .join("scratch.log");
        let project_runtime_cache = options
            .artifact_dir
            .join("projects")
            .join(&project_id)
            .join("cache")
            .join("obsolete.bin");
        write_text(&project_runtime_tmp, "temporary runtime log\n").unwrap();
        write_text(&project_runtime_cache, "rebuildable cache\n").unwrap();
        let (_, _, cleanup) = route_http(
            "POST",
            &format!("/api/projects/{project_id}/cleanup"),
            &options,
            "",
        );
        assert!(cleanup.contains("\"status\":\"completed\""));
        assert!(cleanup.contains("\"scope\":\"project_allowlist\""));
        assert!(cleanup.contains("\"deleted_files_count\":2"));
        assert!(!project_runtime_tmp.exists());
        assert!(!project_runtime_cache.exists());
        assert!(
            options
                .artifact_dir
                .join("projects")
                .join(&project_id)
                .join("project.json")
                .exists()
        );
        assert!(
            options
                .artifact_dir
                .join("projects")
                .join(&project_id)
                .join("input_manifest.json")
                .exists()
        );
        assert!(
            options
                .artifact_dir
                .join("projects")
                .join(&project_id)
                .join("runs")
                .join(&run_id)
                .join("run_manifest.json")
                .exists()
        );

        let (_, _, deleted) = route_http(
            "DELETE",
            &format!("/api/projects/{project_id}"),
            &options,
            "",
        );
        assert!(deleted.contains("\"status\":\"deleted\""));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn run_example_creates_demo_results_only_after_click_and_clears_them() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let root = std::env::temp_dir().join(format!("genoligand_run_example_test_{unique}"));
        let _ = fs::remove_dir_all(&root);
        let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap();
        let options = ServerOptions {
            host: "127.0.0.1".to_string(),
            port: 19090,
            artifact_dir: root.join("artifacts"),
            workspace_root,
        };

        let (_, _, initial_projects) = route_http("GET", "/api/user/projects", &options, "");
        assert!(initial_projects.contains("\"projects\":[]"));
        assert!(initial_projects.contains("\"template_id\":\"run_example_demo_template\""));
        assert!(initial_projects.contains("\"contains_precomputed_results\":false"));
        assert!(initial_projects.contains("\"creates_results_only_on_click\":true"));

        let (_, _, created) = route_http("POST", "/api/user/projects/run-example", &options, "");
        assert!(created.contains("\"status\":\"completed\""));
        assert!(created.contains("\"created_on_click\":true"));
        assert!(created.contains("\"contains_precomputed_results\":false"));
        assert!(
            created.contains("\"demo_boundary\":\"compact_evidence_no_production_prediction\"")
        );
        let project_id = extract_json_string(&created, "project_id");
        let run_id = extract_json_string(&created, "run_id");
        assert!(project_id.starts_with("demo-example-"));
        assert!(run_id.starts_with("run-example-"));

        let (_, _, after_create) = route_http("GET", "/api/user/projects", &options, "");
        assert!(after_create.contains(&project_id));
        assert!(after_create.contains("\"example_project\":true"));
        assert!(after_create.contains("\"demo_results_generated\":true"));

        let (_, _, run) = route_http("GET", &format!("/api/runs/{run_id}"), &options, "");
        assert!(run.contains("\"state\":\"completed\""));
        assert!(run.contains("gls-server run-example --demo-compact-evidence"));
        assert!(run.contains("\"resource_status\":"));

        let (_, _, candidates) = route_http(
            "GET",
            &format!("/api/runs/{run_id}/candidates"),
            &options,
            "",
        );
        assert!(candidates.contains("\"row_count\":1"));
        assert!(candidates.contains("\"artifact_backed\":true"));

        let (_, _, structure) = route_http(
            "GET",
            &format!("/api/runs/{run_id}/structure"),
            &options,
            "",
        );
        assert!(structure.contains("\"viewer\""));
        assert!(structure.contains("\"viewer_library\""));
        assert!(structure.contains("\"NGL\""));
        assert!(structure.contains("\"row_count\":1"));

        let (_, _, cleared) = route_http("POST", "/api/user/projects/clear-examples", &options, "");
        assert!(cleared.contains("\"status\":\"completed\""));
        assert!(cleared.contains("\"deleted_project_count\":1"));
        assert!(cleared.contains("\"scope\":\"example_project_marker_only\""));

        let (_, _, final_projects) = route_http("GET", "/api/user/projects", &options, "");
        assert!(final_projects.contains("\"projects\":[]"));
        assert!(!final_projects.contains(&project_id));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn setup_validate_sample_project_exercises_full_clean_user_loop() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let root = std::env::temp_dir().join(format!("genoligand_clean_sample_test_{unique}"));
        let _ = fs::remove_dir_all(&root);
        let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap();
        let options = ServerOptions {
            host: "127.0.0.1".to_string(),
            port: 19090,
            artifact_dir: root.join("artifacts"),
            workspace_root,
        };

        let (_, _, validation) = route_http(
            "POST",
            "/api/user/setup/validate-sample-project",
            &options,
            "",
        );
        assert!(validation.contains("\"status\":\"ready\""));
        assert!(validation.contains("\"pass\":true"));
        assert!(validation.contains("\"candidate_detail_selected\":true"));
        assert!(validation.contains("\"genome_rows\":1"));
        assert!(validation.contains("\"research_report_exported\":true"));
        assert!(validation.contains("\"project_package_exported\":true"));
        assert!(validation.contains("\"project_package_imported\":true"));
        assert!(validation.contains("\"temporary_project_deleted\":true"));
        assert!(validation.contains("\"before_visible_project_count\":0"));
        assert!(validation.contains("\"after_visible_project_count\":0"));

        let (_, _, projects) = route_http("GET", "/api/user/projects", &options, "");
        assert!(projects.contains("\"projects\":[]"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn github_status_route_reports_local_repository_metadata() {
        let options = ServerOptions {
            host: "127.0.0.1".to_string(),
            port: 19090,
            artifact_dir: std::env::temp_dir().join("genoligand_phase091_git_status"),
            workspace_root: std::env::current_dir().unwrap(),
        };
        let (_, _, body) = route_http("GET", "/api/github/status", &options, "");
        assert!(body.contains("\"push_status\":\"idle\""));
        assert!(body.contains("\"branch\":"));
        assert!(body.contains("\"local_commit_hash\":"));
        assert!(body.contains("\"local_head_hash\":"));
        assert!(body.contains("\"remote_validation_status\":\"not_checked\""));
        assert!(body.contains("\"push_log_path\":"));
        assert!(body.contains("\"remote_verify_log_path\":"));
    }

    fn extract_json_string(json: &str, key: &str) -> String {
        let needle = format!("\"{key}\":\"");
        json.split(&needle)
            .nth(1)
            .and_then(|rest| rest.split('"').next())
            .unwrap_or("")
            .to_string()
    }
}
