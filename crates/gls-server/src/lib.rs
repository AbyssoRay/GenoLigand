use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

pub const PRODUCT_NAME: &str = "GenoLigand Local WebUI";
pub const PRODUCT_VERSION: &str = env!("CARGO_PKG_VERSION");

const REQUIRED_TOOL_IDS: &[&str] = &[
    "java",
    "nextflow",
    "rdkit",
    "openbabel",
    "mmseqs2",
    "foldseek",
    "autodock_vina",
    "openmm",
    "pdbfixer",
    "openff_toolkit",
    "mdtraj",
    "mdanalysis",
];
const INSTALL_PROGRESS_STALE_SECONDS: u64 = 30 * 60;
const FAVICON_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64"><rect width="64" height="64" rx="10" fill="#001312"/><rect x="5" y="5" width="54" height="54" rx="8" fill="#082423" stroke="#24f2dd" stroke-width="3"/><text x="32" y="39" text-anchor="middle" font-family="Arial, sans-serif" font-size="22" font-weight="700" fill="#dffdf8">GL</text></svg>"##;

#[derive(Clone, Debug)]
pub struct ServerOptions {
    pub host: String,
    pub port: u16,
    pub artifact_dir: PathBuf,
    pub workspace_root: PathBuf,
}

pub fn run_server(options: &ServerOptions) -> Result<(), String> {
    fs::create_dir_all(projects_root(options))
        .map_err(|err| format!("create_projects_root_failed:{err}"))?;
    let worker = worker_environment_status(options);
    let worker_ready = worker
        .get("ready")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let worker_summary = worker
        .get("summary")
        .and_then(Value::as_str)
        .unwrap_or("Python worker environment status unknown");
    println!(
        "gls_worker_environment\tstatus={}\tsummary={}",
        if worker_ready {
            "ready"
        } else {
            "needs_repair"
        },
        worker_summary
    );
    let listener = TcpListener::bind(format!("{}:{}", options.host, options.port))
        .map_err(|err| format!("server_bind_failed:{err}"))?;
    println!(
        "gls_server_started\thost={}\tport={}\truntime_dir={}",
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
    let request = read_request(&mut stream)?;
    let mut lines = request.lines();
    let first = lines.next().ok_or_else(|| "empty_request".to_string())?;
    let parts = first.split_whitespace().collect::<Vec<_>>();
    if parts.len() < 2 {
        return Err("bad_request_line".to_string());
    }
    let method = parts[0];
    let path = parts[1].split('?').next().unwrap_or(parts[1]);
    let body_start = request
        .find("\r\n\r\n")
        .map(|index| index + 4)
        .unwrap_or(request.len());
    let request_body = &request[body_start..];
    let (status, content_type, body) = route(method, path, options, request_body);
    write_response(&mut stream, status, content_type, body.as_bytes())
}

fn route(
    method: &str,
    path: &str,
    options: &ServerOptions,
    request_body: &str,
) -> (u16, &'static str, String) {
    match (method, path) {
        ("GET", "/") | ("GET", "/index.html") => {
            return (
                200,
                "text/html; charset=utf-8",
                include_str!("../../../webui/app/index.html").to_string(),
            );
        }
        _ => {}
    }
    if method == "GET"
        && let Some(response) = web_asset_response(path, options)
    {
        return response;
    }
    if let Some(response) = route_user_project(method, path, options, request_body) {
        return response;
    }
    if let Some(response) = route_user_run(method, path, options) {
        return response;
    }
    if let Some(response) = route_project(method, path, options, request_body) {
        return response;
    }
    if let Some(response) = route_run(method, path, options) {
        return response;
    }
    let value = match (method, path) {
        ("GET", "/api/health") => {
            json!({"status":"ready","service":"gls-server","version":PRODUCT_VERSION})
        }
        ("GET", "/api/user/health") => user_health_json(),
        ("GET", "/api/version") => {
            json!({"name":PRODUCT_NAME,"version":PRODUCT_VERSION,"science_flow":"scientific_pipeline"})
        }
        ("GET", "/api/user/projects") => user_projects_json(options),
        ("GET", "/api/user/workspace-summary") => user_workspace_summary_json(options),
        ("POST", "/api/user/projects") => create_user_project(options),
        ("GET", "/api/projects") => projects_json(options),
        ("POST", "/api/projects") => create_project(options),
        ("GET", "/api/user/databases") => user_databases_json(options),
        ("GET", "/api/databases") => databases_json(options),
        ("GET", "/api/user/diagnostics") => user_diagnostics_json(options),
        ("GET", "/api/diagnostics") => diagnostics_json(options),
        ("POST", "/api/user/diagnostics/run") | ("POST", "/api/diagnostics/run") => {
            diagnostics_json(options)
        }
        ("GET", "/api/user/setup/status") => user_setup_status_json(options),
        ("GET", "/api/user/setup/progress") => user_setup_progress_json(options),
        ("POST", "/api/user/setup/run") => run_setup_repair_json(options),
        ("GET", "/api/user/setup/install-plan") => user_setup_install_plan_json(options),
        ("POST", "/api/user/setup/install") => run_setup_repair_json(options),
        ("GET", "/api/guides") => {
            json!({"guides":["docs/local_webui_user_guide.md","docs/windows_wsl2_installation_guide.md","docs/linux_server_installation_guide.md"]})
        }
        _ => {
            return (
                404,
                "application/json; charset=utf-8",
                json!({"error":"not_found"}).to_string(),
            );
        }
    };
    (200, "application/json; charset=utf-8", value.to_string())
}

fn web_asset_response(path: &str, _options: &ServerOptions) -> Option<(u16, &'static str, String)> {
    let rel = path.strip_prefix('/')?;
    if rel.contains("..") || rel.contains('\\') || !is_allowed_web_asset(rel) {
        return None;
    }
    let body = match rel {
        "app.js" => include_str!("../../../webui/app/app.js"),
        "styles.css" => include_str!("../../../webui/app/styles.css"),
        "state/app-state.js" => include_str!("../../../webui/app/state/app-state.js"),
        "api/client.js" => include_str!("../../../webui/app/api/client.js"),
        "pages/input.js" => include_str!("../../../webui/app/pages/input.js"),
        "pages/configure.js" => include_str!("../../../webui/app/pages/configure.js"),
        "pages/check.js" => include_str!("../../../webui/app/pages/check.js"),
        "pages/run.js" => include_str!("../../../webui/app/pages/run.js"),
        "pages/results.js" => include_str!("../../../webui/app/pages/results.js"),
        "pages/export.js" => include_str!("../../../webui/app/pages/export.js"),
        "core/actions.js" => include_str!("../../../webui/app/core/actions.js"),
        "core/selectors.js" => include_str!("../../../webui/app/core/selectors.js"),
        "components/common.js" => include_str!("../../../webui/app/components/common.js"),
        "components/viewers.js" => include_str!("../../../webui/app/components/viewers.js"),
        "styles/tokens.css" => include_str!("../../../webui/app/styles/tokens.css"),
        "styles/base.css" => include_str!("../../../webui/app/styles/base.css"),
        "styles/layout.css" => include_str!("../../../webui/app/styles/layout.css"),
        "styles/components.css" => include_str!("../../../webui/app/styles/components.css"),
        "styles/pages.css" => include_str!("../../../webui/app/styles/pages.css"),
        "favicon.svg" | "favicon.ico" => FAVICON_SVG,
        _ => return None,
    };
    Some((200, web_asset_content_type(rel), body.to_string()))
}

fn is_allowed_web_asset(rel: &str) -> bool {
    rel == "app.js"
        || rel == "styles.css"
        || rel.starts_with("state/")
        || rel.starts_with("api/")
        || rel.starts_with("pages/")
        || rel.starts_with("core/")
        || rel.starts_with("components/")
        || rel.starts_with("styles/")
        || rel == "favicon.svg"
        || rel == "favicon.ico"
}

fn web_asset_content_type(rel: &str) -> &'static str {
    if rel.ends_with(".css") {
        "text/css; charset=utf-8"
    } else if rel.ends_with(".svg") || rel.ends_with(".ico") {
        "image/svg+xml; charset=utf-8"
    } else {
        "application/javascript; charset=utf-8"
    }
}

fn route_project(
    method: &str,
    path: &str,
    options: &ServerOptions,
    request_body: &str,
) -> Option<(u16, &'static str, String)> {
    let rest = path.strip_prefix("/api/projects/")?;
    let mut parts = rest.split('/');
    let project_id = safe_id(parts.next().unwrap_or(""));
    if project_id.is_empty() {
        return Some(json_response(404, json!({"error":"project_not_found"})));
    }
    let suffix = parts.collect::<Vec<_>>().join("/");
    let value = match (method, suffix.as_str()) {
        ("GET", "") => read_project(options, &project_id),
        ("GET", "inputs") => read_input_manifest(options, &project_id),
        ("POST", "inputs") => write_input_manifest(options, &project_id, request_body),
        ("GET", "preflight") | ("POST", "preflight") => preflight_json(options, &project_id),
        ("GET", "runs") => project_runs_json(options, &project_id),
        ("POST", "runs") => start_run(options, &project_id),
        ("DELETE", "") => delete_project(options, &project_id),
        _ => json!({"error":"not_found"}),
    };
    Some((200, "application/json; charset=utf-8", value.to_string()))
}

fn route_user_project(
    method: &str,
    path: &str,
    options: &ServerOptions,
    request_body: &str,
) -> Option<(u16, &'static str, String)> {
    let rest = path.strip_prefix("/api/user/projects/")?;
    let mut parts = rest.split('/');
    let project_id = safe_id(parts.next().unwrap_or(""));
    if project_id.is_empty() {
        return Some(json_response(404, json!({"message":"未找到工作台"})));
    }
    let suffix = parts.collect::<Vec<_>>().join("/");
    let value = match (method, suffix.as_str()) {
        ("GET", "") => user_project_detail(options, &project_id),
        ("POST", "inputs") => write_user_input_manifest(options, &project_id, request_body),
        ("GET", "preflight") | ("POST", "preflight") => user_preflight_json(options, &project_id),
        ("POST", "runs") => start_user_run(options, &project_id),
        _ => json!({"message":"未找到工作台操作"}),
    };
    Some((200, "application/json; charset=utf-8", value.to_string()))
}

fn route_run(
    method: &str,
    path: &str,
    options: &ServerOptions,
) -> Option<(u16, &'static str, String)> {
    let rest = path.strip_prefix("/api/runs/")?;
    let mut parts = rest.split('/');
    let run_id = safe_id(parts.next().unwrap_or(""));
    let suffix = parts.collect::<Vec<_>>().join("/");
    let value = match (method, suffix.as_str()) {
        ("GET", "") => run_json(options, &run_id),
        ("GET", "logs") => logs_json(options, &run_id),
        ("GET", "artifacts") => artifacts_json(options, &run_id),
        ("GET", "candidates") => candidates_endpoint_json(options, &run_id),
        ("GET", "candidate-detail") => candidate_detail_json(options, &run_id),
        ("GET", "evidence") => evidence_endpoint_json(options, &run_id),
        ("GET", "genome-context") => genome_endpoint_json(options, &run_id),
        ("GET", "structure") => structure_endpoint_json(options, &run_id),
        ("GET", "md-summary") => md_endpoint_json(options, &run_id),
        ("POST", "cancel") => update_run_state(options, &run_id, "cancelled"),
        ("POST", "resume") => update_run_state(options, &run_id, "resumable"),
        _ => {
            if method == "GET"
                && let Some(file) = suffix.strip_prefix("download/")
            {
                return Some(download_artifact_response(options, &run_id, file));
            }
            json!({"error":"not_found"})
        }
    };
    Some((200, "application/json; charset=utf-8", value.to_string()))
}

fn route_user_run(
    method: &str,
    path: &str,
    options: &ServerOptions,
) -> Option<(u16, &'static str, String)> {
    let rest = path.strip_prefix("/api/user/runs/")?;
    let mut parts = rest.split('/');
    let run_id = safe_id(parts.next().unwrap_or(""));
    let suffix = parts.collect::<Vec<_>>().join("/");
    let value = match (method, suffix.as_str()) {
        ("GET", "") => user_run_json(options, &run_id),
        ("GET", "logs") => logs_json(options, &run_id),
        ("GET", "artifacts") => artifacts_json(options, &run_id),
        ("GET", "candidates") => candidates_endpoint_json(options, &run_id),
        ("GET", "candidate-detail") => candidate_detail_json(options, &run_id),
        ("GET", "evidence") => evidence_endpoint_json(options, &run_id),
        ("GET", "genome-context") => genome_endpoint_json(options, &run_id),
        ("GET", "structure") => structure_endpoint_json(options, &run_id),
        ("GET", "md-summary") => md_endpoint_json(options, &run_id),
        _ => {
            if method == "GET"
                && let Some(file) = suffix.strip_prefix("download/")
            {
                return Some(download_artifact_response(options, &run_id, file));
            }
            json!({"message":"未找到运行材料"})
        }
    };
    Some((200, "application/json; charset=utf-8", value.to_string()))
}

fn projects_json(options: &ServerOptions) -> Value {
    let mut projects = Vec::new();
    if let Ok(entries) = fs::read_dir(projects_root(options)) {
        for entry in entries.flatten() {
            if entry.file_type().map(|file| file.is_dir()).unwrap_or(false) {
                let id = entry.file_name().to_string_lossy().to_string();
                projects.push(read_project(options, &id));
            }
        }
    }
    projects.sort_by_key(|project| {
        project
            .get("project_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    });
    json!({
        "projects": projects,
        "templates": [],
        "supported_actions": ["create","open","delete","preflight","run","resume","export"]
    })
}

fn user_projects_json(options: &ServerOptions) -> Value {
    let mut projects = Vec::new();
    if let Ok(entries) = fs::read_dir(projects_root(options)) {
        for entry in entries.flatten() {
            if entry.file_type().map(|file| file.is_dir()).unwrap_or(false) {
                let id = entry.file_name().to_string_lossy().to_string();
                if !project_visible_to_user(options, &id) {
                    continue;
                }
                let project = read_project(options, &id);
                projects.push(user_project_summary(&project));
            }
        }
    }
    projects.sort_by_key(|project| {
        project
            .get("project_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    });
    json!({"projects": projects})
}

fn user_workspace_summary_json(options: &ServerOptions) -> Value {
    let mut project_count = 0usize;
    let mut run_count = 0usize;
    let mut candidate_count = 0usize;
    let mut result_count = 0usize;
    let mut export_item_count = 0usize;
    if let Ok(entries) = fs::read_dir(projects_root(options)) {
        for entry in entries.flatten() {
            if !entry.file_type().map(|file| file.is_dir()).unwrap_or(false) {
                continue;
            }
            let project_id = entry.file_name().to_string_lossy().to_string();
            if !project_visible_to_user(options, &project_id) {
                continue;
            }
            project_count += 1;
            let runs_dir = entry.path().join("runs");
            if let Ok(runs) = fs::read_dir(runs_dir) {
                for run in runs.flatten() {
                    if !run.file_type().map(|file| file.is_dir()).unwrap_or(false) {
                        continue;
                    }
                    run_count += 1;
                    let run_dir = run.path();
                    if run_state(&run_dir) == "completed" {
                        let candidates = rows_len(&run_dir.join("target_candidate_ranking.tsv"));
                        candidate_count += candidates;
                        result_count += candidates
                            + rows_len(&run_dir.join("evidence_ledger.tsv"))
                            + rows_len(&run_dir.join("metagenome_potential_matches.tsv"))
                            + rows_len(&run_dir.join("structure_patch_summary.tsv"))
                            + rows_len(&run_dir.join("md_summary.tsv"));
                    }
                    export_item_count += visible_export_count(&run_dir);
                }
            }
        }
    }
    json!({
        "project_count": project_count,
        "run_count": run_count,
        "candidate_count": candidate_count,
        "result_count": result_count,
        "export_item_count": export_item_count,
        "demo_count": 0,
        "sample_count": 0,
        "test_result_count": 0,
        "blank_workspace": project_count == 0 && run_count == 0 && candidate_count == 0 && export_item_count == 0
    })
}

fn create_project(options: &ServerOptions) -> Value {
    match create_project_record(options) {
        Ok(project) => json!({"status":"created","project":project}),
        Err(err) => json!({"status":"failed","error":err}),
    }
}

fn create_user_project(options: &ServerOptions) -> Value {
    match create_project_record(options) {
        Ok(project) => {
            json!({"project":user_project_summary(&project),"message":"空白工作台已创建"})
        }
        Err(_) => json!({"project":Value::Null,"message":"无法创建空白工作台，请检查安装目录权限"}),
    }
}

fn create_project_record(options: &ServerOptions) -> Result<Value, String> {
    let project_id = format!("project-{}", now_millis());
    let dir = project_dir(options, &project_id);
    if let Err(err) = fs::create_dir_all(dir.join("runs")) {
        return Err(err.to_string());
    }
    Ok(read_project(options, &project_id))
}

fn project_visible_to_user(options: &ServerOptions, project_id: &str) -> bool {
    let dir = project_dir(options, project_id);
    input_has_user_value(&dir.join("input_manifest.json")) || dir.join("latest_run_id.txt").exists()
}

fn user_project_summary(project: &Value) -> Value {
    let status = project
        .get("project_status")
        .and_then(Value::as_str)
        .unwrap_or_default();
    json!({
        "project_id": project.get("project_id").cloned().unwrap_or(Value::Null),
        "project_name": project.get("project_name").cloned().unwrap_or(Value::Null),
        "project_state": friendly_project_state(status),
        "latest_run_id": project.get("latest_run_id").cloned().unwrap_or(Value::Null)
    })
}

fn friendly_project_state(status: &str) -> &'static str {
    match status {
        "completed" => "已完成运行",
        "running" => "运行中",
        "failed" => "需要处理",
        "blocked" => "等待处理",
        _ => "等待输入",
    }
}

fn read_project(options: &ServerOptions, project_id: &str) -> Value {
    let dir = project_dir(options, project_id);
    let latest_run_id = read_trimmed(&dir.join("latest_run_id.txt"));
    let status = latest_run_id
        .as_deref()
        .map(|run_id| run_state(&dir.join("runs").join(run_id)))
        .unwrap_or_else(|| "created".to_string());
    json!({
        "project_id": project_id,
        "project_name": read_trimmed(&dir.join("project_name.txt")).unwrap_or_else(|| "未命名研究项目".to_string()),
        "project_status": status,
        "latest_run_id": latest_run_id
    })
}

fn user_project_detail(options: &ServerOptions, project_id: &str) -> Value {
    user_project_summary(&read_project(options, project_id))
}

fn read_input_manifest(options: &ServerOptions, project_id: &str) -> Value {
    let path = project_dir(options, project_id).join("input_manifest.json");
    read_json_or(
        &path,
        json!({"inputs":[],"files":[],"status":"waiting_for_user_input"}),
    )
}

fn write_user_input_manifest(options: &ServerOptions, project_id: &str, body: &str) -> Value {
    let response = write_input_manifest(options, project_id, body);
    let ok = response.get("status").and_then(Value::as_str) == Some("saved");
    let project = response
        .get("project")
        .map(user_project_summary)
        .unwrap_or_else(|| user_project_detail(options, project_id));
    json!({
        "ok": ok,
        "message": if ok {"输入已保存"} else {response.get("message").and_then(Value::as_str).unwrap_or("输入未保存，请检查填写内容")},
        "project": project
    })
}

fn write_input_manifest(options: &ServerOptions, project_id: &str, body: &str) -> Value {
    let dir = project_dir(options, project_id);
    let path = dir.join("input_manifest.json");
    let value = serde_json::from_str::<Value>(body.trim_start_matches('\u{feff}'))
        .unwrap_or_else(|_| json!({"inputs":[],"files":[]}));
    if !input_value_has_usable_user_input(&value) {
        return json!({
            "status":"failed",
            "error":"input_not_usable",
            "message":"\u{8bf7}\u{586b}\u{5199}\u{81f3}\u{5c11}\u{4e00}\u{4e2a}\u{53ef}\u{8bc6}\u{522b}\u{7684}\u{7814}\u{7a76}\u{8f93}\u{5165}\u{3002}",
            "project": read_project(options, project_id)
        });
    }
    if let Some(name) = input_project_name(&value) {
        let _ = write_text(&dir.join("project_name.txt"), &name);
    }
    match write_text(&path, &(value.to_string() + "\n")) {
        Ok(()) => {
            clear_current_run_selection(&dir);
            json!({"status":"saved","project":read_project(options, project_id)})
        }
        Err(err) => json!({"status":"failed","error":err}),
    }
}

fn clear_current_run_selection(project_dir: &Path) {
    let _ = fs::remove_file(project_dir.join("latest_run_id.txt"));
}

fn preflight_json(options: &ServerOptions, project_id: &str) -> Value {
    let input = project_dir(options, project_id).join("input_manifest.json");
    let store = science_store_path(options);
    let checks = vec![
        check(
            "input",
            input.exists() && input_has_user_value(&input),
            input.display().to_string(),
        ),
        check(
            "science_store",
            preflight_store_ready(options),
            store
                .join("science_store_manifest.tsv")
                .display()
                .to_string(),
        ),
        check(
            "known_database",
            store.join("indexes/known_targets.tsv").exists(),
            store
                .join("indexes/known_targets.tsv")
                .display()
                .to_string(),
        ),
        check(
            "metagenome",
            store.join("indexes/metagenome_targets.tsv").exists(),
            store
                .join("indexes/metagenome_targets.tsv")
                .display()
                .to_string(),
        ),
        check(
            "structure_index",
            store.join("indexes/structure_targets.tsv").exists(),
            store
                .join("indexes/structure_targets.tsv")
                .display()
                .to_string(),
        ),
        check(
            "tools",
            tool_versions_ready(&store.join("indexes/tool_versions.tsv")),
            store
                .join("indexes/tool_versions.tsv")
                .display()
                .to_string(),
        ),
        check(
            "python_worker",
            worker_environment_status(options)
                .get("ready")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            "Python worker environment".to_string(),
        ),
    ];
    let ready = checks
        .iter()
        .all(|row| row.get("ready").and_then(Value::as_bool).unwrap_or(false));
    json!({"status": if ready {"preflight_passed"} else {"blocked"}, "checks": checks, "candidate_count_if_blocked": 0})
}

fn user_preflight_json(options: &ServerOptions, project_id: &str) -> Value {
    let response = preflight_json(options, project_id);
    let checks = user_preflight_checks(response.get("checks"));
    let ready = checks.as_array().is_some_and(|rows| {
        rows.iter()
            .all(|row| row.get("ready").and_then(Value::as_bool).unwrap_or(false))
    });
    json!({
        "ok": ready,
        "message": if ready {"预检查已通过"} else {"还有项目需要处理"},
        "checks": checks
    })
}

fn user_preflight_checks(checks: Option<&Value>) -> Value {
    let rows = checks
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    Value::Array(
        rows.into_iter()
            .map(|row| {
                json!({
                    "id": row.get("id").cloned().unwrap_or(Value::Null),
                    "ready": row.get("ready").cloned().unwrap_or(Value::Bool(false))
                })
            })
            .collect(),
    )
}

fn runtime_command_environment(options: &ServerOptions) -> BTreeMap<String, String> {
    let mut values = launch_environment_values(options);
    for key in [
        "GENOLIGAND_PYTHON",
        "GENOLIGAND_GLS_CLI",
        "GENOLIGAND_GLS_SERVER",
        "GENOLIGAND_SCIENCE_STORE",
    ] {
        if !values.contains_key(key)
            && let Ok(value) = std::env::var(key)
            && !value.trim().is_empty()
        {
            values.insert(key.to_string(), value);
        }
    }
    values
        .entry("GENOLIGAND_SCIENCE_STORE".to_string())
        .or_insert_with(|| science_store_path(options).display().to_string());
    if let Some(path) = launch_environment_path(options) {
        values.entry("PATH".to_string()).or_insert(path);
    }
    values
}

fn runtime_env_value(options: &ServerOptions, key: &str) -> Option<String> {
    runtime_command_environment(options).remove(key)
}

fn launch_environment_values(options: &ServerOptions) -> BTreeMap<String, String> {
    let value = read_json_or(
        &options.artifact_dir.join("launch_environment.json"),
        json!({}),
    );
    value
        .get("environment")
        .and_then(Value::as_object)
        .map(|object| {
            object
                .iter()
                .filter_map(|(key, value)| {
                    value
                        .as_str()
                        .filter(|text| !text.trim().is_empty())
                        .map(|text| (key.clone(), text.to_string()))
                })
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default()
}

fn launch_environment_path(options: &ServerOptions) -> Option<String> {
    let value = read_json_or(
        &options.artifact_dir.join("launch_environment.json"),
        json!({}),
    );
    let mut parts = value
        .get("path_prepend")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .filter(|text| !text.trim().is_empty())
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if parts.is_empty() {
        return None;
    }
    if let Ok(current) = std::env::var("PATH")
        && !current.trim().is_empty()
    {
        parts.push(current);
    }
    Some(parts.join(if cfg!(windows) { ";" } else { ":" }))
}

fn worker_environment_status(options: &ServerOptions) -> Value {
    let env = runtime_command_environment(options);
    let python = env.get("GENOLIGAND_PYTHON").cloned().unwrap_or_default();
    let managed_python = worker_python_looks_managed(&python);
    let tools_ready =
        tool_versions_ready(&science_store_path(options).join("indexes/tool_versions.tsv"));
    let (dependencies_ready, dependency_summary) = if managed_python && tools_ready {
        worker_python_dependency_status(options, &env, &python)
    } else {
        (false, String::new())
    };
    let ready = managed_python && tools_ready && dependencies_ready;
    let summary = if python.trim().is_empty() {
        "未找到安装器写入的 Python worker 环境；请通过 start-genoligand-webui 启动，或运行安装/修复。"
    } else if !managed_python {
        "当前 Python 不是安装器管理的 worker 环境；请使用 start-genoligand-webui 启动，避免系统 Python 缺依赖。"
    } else if !tools_ready {
        "Python worker 环境已指定，但科学工具记录未就绪；请运行安装/修复。"
    } else if !dependencies_ready {
        dependency_summary.as_str()
    } else {
        "Python worker 环境来自安装器启动配置，科学工具记录已就绪。"
    };
    json!({
        "id": "python_worker",
        "label": "Python worker 环境",
        "ready": ready,
        "status": if ready {"就绪"} else {"需要处理"},
        "summary": summary,
        "next_action": if ready {"继续"} else {"通过 start-genoligand-webui 启动，或运行安装/修复"}
    })
}

fn worker_python_looks_managed(value: &str) -> bool {
    let normalized = value.replace('\\', "/").to_ascii_lowercase();
    normalized.contains("python-worker-env") || normalized.contains("genoligand-wsl-python")
}

fn worker_python_dependency_status(
    options: &ServerOptions,
    env: &BTreeMap<String, String>,
    python: &str,
) -> (bool, String) {
    let code = concat!(
        "import importlib, sys\n",
        "mods=['rdkit','openmm','pdbfixer','openff.toolkit','mdtraj','MDAnalysis']\n",
        "missing=[]\n",
        "for mod in mods:\n",
        "    try:\n",
        "        importlib.import_module(mod)\n",
        "    except Exception:\n",
        "        missing.append(mod)\n",
        "if missing:\n",
        "    print('missing:'+','.join(missing))\n",
        "    raise SystemExit(2)\n",
        "print('worker_python_ready')\n",
    );
    match worker_python_probe_output(options, env, python, code) {
        Ok(output) if output.status.success() => {
            (true, "Python worker 关键依赖可导入。".to_string())
        }
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let detail = stdout
                .lines()
                .chain(stderr.lines())
                .find(|line| !line.trim().is_empty())
                .unwrap_or("dependency probe failed");
            (
                false,
                format!("Python worker 环境缺少依赖或无法启动：{detail}；请运行安装/修复。"),
            )
        }
        Err(err) => (
            false,
            format!(
                "Python worker 环境无法启动：{err}；请通过 start-genoligand-webui 启动，或运行安装/修复。"
            ),
        ),
    }
}

fn worker_python_probe_output(
    options: &ServerOptions,
    env: &BTreeMap<String, String>,
    python: &str,
    code: &str,
) -> Result<std::process::Output, std::io::Error> {
    if cfg!(windows) && is_windows_script(python) {
        fs::create_dir_all(&options.artifact_dir)?;
        let probe = options
            .artifact_dir
            .join(format!("worker-dependency-probe-{}.py", now_millis()));
        fs::write(&probe, code)?;
        let output = Command::new("cmd.exe")
            .arg("/C")
            .arg(python)
            .arg(python_probe_script_arg(python, &probe))
            .current_dir(&options.workspace_root)
            .envs(env)
            .output();
        let _ = fs::remove_file(probe);
        return output;
    }
    Command::new(python)
        .arg("-c")
        .arg(code)
        .current_dir(&options.workspace_root)
        .envs(env)
        .output()
}

fn is_windows_script(command: &str) -> bool {
    Path::new(command)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("cmd") || ext.eq_ignore_ascii_case("bat"))
}

fn python_probe_script_arg(python: &str, script: &Path) -> String {
    let path = script.display().to_string();
    if python.replace('\\', "/").contains("genoligand-wsl-python") {
        return windows_path_to_wsl(&path).unwrap_or(path);
    }
    path
}

fn windows_path_to_wsl(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    if bytes.len() < 3 || bytes[1] != b':' || !bytes[0].is_ascii_alphabetic() {
        return None;
    }
    let slash = bytes[2];
    if slash != b'\\' && slash != b'/' {
        return None;
    }
    let drive = (bytes[0] as char).to_ascii_lowercase();
    let rest = value[3..].replace('\\', "/");
    Some(format!("/mnt/{drive}/{rest}"))
}

fn start_run(options: &ServerOptions, project_id: &str) -> Value {
    let project = project_dir(options, project_id);
    let run_id = format!("run-{}", now_millis());
    let run_dir = project.join("runs").join(&run_id);
    let _ = fs::create_dir_all(&run_dir);
    let _ = write_text(&run_dir.join("run_state.txt"), "running\n");
    let _ = write_text(&run_dir.join("current_stage.txt"), "pipeline_run\n");
    let worker = worker_environment_status(options);
    if !worker
        .get("ready")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        let reason = worker
            .get("summary")
            .and_then(Value::as_str)
            .unwrap_or("Python worker environment needs repair")
            .to_string();
        let _ = write_text(&run_dir.join("run_state.txt"), "blocked\n");
        let _ = write_text(
            &run_dir.join("failure_reason.txt"),
            &(reason.clone() + "\n"),
        );
        let _ = write_text(
            &run_dir.join("run.log"),
            &format!("state\tblocked\nreason\t{reason}\n"),
        );
        let _ = write_text(&project.join("latest_run_id.txt"), &run_id);
        return json!({"status":"blocked","run":run_json(options,&run_id)});
    }
    let input = project.join("input_manifest.json");
    let command = gls_cli_command(options, &input, &run_dir);
    let command_text = command.join(" ");
    let _ = write_text(&run_dir.join("current_command.txt"), &command_text);
    let output = Command::new(&command[0])
        .args(&command[1..])
        .current_dir(&options.workspace_root)
        .envs(runtime_command_environment(options))
        .output();
    let (state, reason, stdout, stderr) = match output {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            if output.status.success() {
                let blocked =
                    read_trimmed(&run_dir.join("run_state.txt")).as_deref() == Some("blocked");
                if blocked {
                    (
                        "blocked".to_string(),
                        read_trimmed(&run_dir.join("failure_reason.txt")).unwrap_or_default(),
                        stdout,
                        stderr,
                    )
                } else {
                    ("completed".to_string(), String::new(), stdout, stderr)
                }
            } else {
                (
                    "failed".to_string(),
                    format!("pipeline_command_failed_exit_{:?}", output.status.code()),
                    stdout,
                    stderr,
                )
            }
        }
        Err(err) => (
            "failed".to_string(),
            format!("pipeline_command_launch_failed:{err}"),
            String::new(),
            String::new(),
        ),
    };
    let _ = write_text(&run_dir.join("run_state.txt"), &(state.clone() + "\n"));
    let _ = write_text(
        &run_dir.join("failure_reason.txt"),
        &(reason.clone() + "\n"),
    );
    let _ = write_text(
        &run_dir.join("run.log"),
        &format!("command\t{command_text}\nstate\t{state}\nstdout\n{stdout}\nstderr\n{stderr}\n"),
    );
    let _ = write_text(&project.join("latest_run_id.txt"), &run_id);
    let artifact_index = artifact_index(&run_dir);
    let _ = write_text(
        &run_dir.join("artifact_index.json"),
        &(artifact_index.to_string() + "\n"),
    );
    json!({"status":state,"run":run_json(options,&run_id)})
}

fn start_user_run(options: &ServerOptions, project_id: &str) -> Value {
    let response = start_run(options, project_id);
    user_start_run_response(options, project_id, &response)
}

fn user_start_run_response(options: &ServerOptions, project_id: &str, response: &Value) -> Value {
    let run_id = response
        .get("run")
        .and_then(|run| run.get("run_id"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let run = user_run_json(options, run_id);
    let state = run.get("state").and_then(Value::as_str).unwrap_or_default();
    json!({
        "ok": matches!(state, "completed" | "running" | "blocked"),
        "message": friendly_run_message(state),
        "run": run,
        "project": user_project_detail(options, project_id)
    })
}

fn friendly_run_message(state: &str) -> &'static str {
    match state {
        "completed" => "运行已完成",
        "running" => "运行已开始",
        "blocked" => "运行已阻止，请处理缺失项",
        "failed" => "运行失败，请查看运行中心",
        _ => "运行状态待确认",
    }
}

fn gls_cli_command(options: &ServerOptions, input: &Path, run_dir: &Path) -> Vec<String> {
    let mut command = gls_cli_invocation(options);
    command.extend([
        "pipeline".to_string(),
        "run".to_string(),
        "--input-manifest".to_string(),
        input.display().to_string(),
        "--run-dir".to_string(),
        run_dir.display().to_string(),
        "--science-store".to_string(),
        science_store_path(options).display().to_string(),
        "--workspace-root".to_string(),
        options.workspace_root.display().to_string(),
    ]);
    command
}

fn gls_cli_invocation(options: &ServerOptions) -> Vec<String> {
    if let Ok(path) = std::env::var("GENOLIGAND_GLS_CLI")
        && !path.trim().is_empty()
    {
        return vec![path];
    }
    if let Some(path) = runtime_env_value(options, "GENOLIGAND_GLS_CLI") {
        return vec![path];
    }
    let exe = if cfg!(windows) {
        "gls-cli.exe"
    } else {
        "gls-cli"
    };
    let runtime_candidate = options.artifact_dir.join("bin").join(exe);
    if runtime_candidate.exists() {
        return vec![runtime_candidate.display().to_string()];
    }
    let candidate = options
        .workspace_root
        .join("target")
        .join("debug")
        .join(exe);
    if candidate.exists() {
        vec![candidate.display().to_string()]
    } else if options.workspace_root.join("Cargo.toml").exists() {
        vec![
            "cargo".to_string(),
            "run".to_string(),
            "-p".to_string(),
            "gls-cli".to_string(),
            "--".to_string(),
        ]
    } else {
        vec!["gls-cli".to_string()]
    }
}

fn gls_cli_help_command(options: &ServerOptions) -> Vec<String> {
    let mut command = gls_cli_invocation(options);
    command.push("--help".to_string());
    command
}

fn science_store_path(options: &ServerOptions) -> PathBuf {
    if let Some(path) = launch_environment_values(options).remove("GENOLIGAND_SCIENCE_STORE")
        && !path.trim().is_empty()
    {
        return PathBuf::from(path);
    }
    std::env::var("GENOLIGAND_SCIENCE_STORE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            #[cfg(test)]
            {
                return options.workspace_root.join("science_store");
            }
            #[allow(unreachable_code)]
            default_science_store(&options.workspace_root)
        })
}

fn default_science_store(workspace_root: &Path) -> PathBuf {
    if let Ok(root) = std::env::var("GENOLIGAND_DATA_ROOT") {
        let root = PathBuf::from(root);
        if !root.as_os_str().is_empty() {
            return root.join("science_store");
        }
    }
    if cfg!(windows) {
        let d_root = PathBuf::from(r"D:\GenoLigandData\science_store");
        if Path::new(r"D:\").exists() {
            return d_root;
        }
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            return PathBuf::from(local)
                .join("GenoLigand")
                .join("science_store");
        }
    } else {
        let data_root = PathBuf::from("/data/genoligand/science_store");
        if Path::new("/data").exists() {
            return data_root;
        }
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("genoligand")
                .join("science_store");
        }
    }
    workspace_root.join(".genoligand").join("science_store")
}

fn project_runs_json(options: &ServerOptions, project_id: &str) -> Value {
    let mut runs = Vec::new();
    let runs_dir = project_dir(options, project_id).join("runs");
    if let Ok(entries) = fs::read_dir(runs_dir) {
        for entry in entries.flatten() {
            if entry.file_type().map(|file| file.is_dir()).unwrap_or(false) {
                runs.push(run_json(options, &entry.file_name().to_string_lossy()));
            }
        }
    }
    json!({"runs":runs})
}

fn run_json(options: &ServerOptions, run_id: &str) -> Value {
    let Some(run_dir) = find_run_dir(options, run_id) else {
        return json!({"run_id":run_id,"state":"not_found"});
    };
    let state = run_state(&run_dir);
    json!({
        "run_id": run_id,
        "state": state,
        "current_stage": read_trimmed(&run_dir.join("current_stage.txt")).unwrap_or_default(),
        "failure_summary": friendly_failure_summary(read_trimmed(&run_dir.join("failure_reason.txt")).as_deref()),
        "trace": run_trace(options, &run_dir, run_id),
        "outputs": artifact_index(&run_dir)
    })
}

fn user_run_json(options: &ServerOptions, run_id: &str) -> Value {
    let mut run = run_json(options, run_id);
    if let Some(stage) = run.get("current_stage").and_then(Value::as_str) {
        run["current_stage"] = json!(stage_user_label(stage));
    }
    let state = run
        .get("state")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    run["state_label"] = json!(run_state_user_label(&state));
    if let Some(run_dir) = find_run_dir(options, run_id) {
        run["stages"] = json!(user_run_stages(&run_dir, &state));
    }
    run
}

fn user_run_stages(run_dir: &Path, state: &str) -> Vec<Value> {
    let specs = [
        ("input", "输入归一化", "input_normalize"),
        ("features", "特征计算", "features_compute"),
        ("known", "已知数据库比对", "database_match_known"),
        (
            "metagenome",
            "宏基因组潜在序列比对",
            "metagenome_match_potential",
        ),
        ("structure", "结构与口袋评估", "structure_evaluate"),
        ("md", "对接与短动力学筛选", "dynamics_screen"),
        ("ranking", "联合排序", "target_rank"),
        ("evidence", "证据链与报告", "evidence_build"),
    ];
    let current = read_trimmed(&run_dir.join("current_stage.txt")).unwrap_or_default();
    specs
        .iter()
        .map(|(id, label, stage_key)| {
            let (status, reason) = user_stage_status(run_dir, state, id, stage_key, &current);
            json!({
                "id": id,
                "label": label,
                "status": status,
                "status_label": user_stage_status_label(status),
                "reason": reason
            })
        })
        .collect()
}

fn user_stage_status<'a>(
    run_dir: &Path,
    state: &str,
    id: &str,
    stage_key: &str,
    current: &str,
) -> (&'a str, String) {
    if state == "completed" {
        return completed_stage_status(run_dir, id);
    }
    if state == "running" && current == stage_key {
        return ("running", String::new());
    }
    if matches!(state, "failed" | "blocked" | "cancelled") {
        return ("blocked", String::new());
    }
    ("waiting", String::new())
}

fn completed_stage_status(run_dir: &Path, id: &str) -> (&'static str, String) {
    match id {
        "structure" => {
            if rows_len(&run_dir.join("structure_patch_summary.tsv")) > 0 {
                ("ready", String::new())
            } else {
                ("not_generated", structure_viewer_reason(run_dir))
            }
        }
        "md" => {
            if rows_len(&run_dir.join("md_summary.tsv")) > 0 {
                ("ready", String::new())
            } else {
                (
                    "not_generated",
                    read_status_reason(
                        &run_dir.join("md_summary_status.tsv"),
                        "当前运行没有生成 MD 摘要。",
                    ),
                )
            }
        }
        _ => ("ready", String::new()),
    }
}

fn user_stage_status_label(status: &str) -> &'static str {
    match status {
        "ready" => "完成",
        "running" => "进行中",
        "not_generated" => "未生成",
        "blocked" => "需处理",
        _ => "等待",
    }
}

fn logs_json(options: &ServerOptions, run_id: &str) -> Value {
    let Some(run_dir) = find_run_dir(options, run_id) else {
        return json!({"logs":[]});
    };
    json!({"logs":user_run_events(&run_dir, run_id)})
}

fn artifacts_json(options: &ServerOptions, run_id: &str) -> Value {
    find_run_dir(options, run_id)
        .map(|dir| json!({"run_id":run_id,"items":artifact_index(&dir)["items"].clone()}))
        .unwrap_or_else(|| json!({"run_id":run_id,"items":[]}))
}

fn download_artifact_response(
    options: &ServerOptions,
    run_id: &str,
    requested: &str,
) -> (u16, &'static str, String) {
    let artifact_id = requested.split('?').next().unwrap_or(requested);
    let Some(run_dir) = find_run_dir(options, run_id) else {
        return json_response(404, json!({"error":"run_not_found"}));
    };
    let Some((_, _, artifact_file)) = artifact_specs()
        .iter()
        .find(|(id, _, _)| *id == artifact_id)
    else {
        return json_response(404, json!({"error":"artifact_not_available"}));
    };
    let path = run_dir.join(artifact_file);
    let completed = run_state(&run_dir) == "completed";
    if !artifact_visible_for_user(artifact_id, &path, &run_dir, completed) {
        return json_response(404, json!({"error":"artifact_not_available"}));
    }
    match artifact_id {
        "research_report" => {
            let path = run_dir.join("research_report.md");
            match fs::read_to_string(&path) {
                Ok(text) => (200, "text/markdown; charset=utf-8", text),
                Err(_) => json_response(404, json!({"error":"artifact_not_available"})),
            }
        }
        "candidate_ranking" => user_tsv_download(
            user_candidate_rows(&run_dir),
            &[
                ("\u{6392}\u{5e8f}", "rank"),
                ("\u{5019}\u{9009}\u{7f16}\u{53f7}", "candidate_id"),
                ("\u{76ee}\u{6807}\u{7f16}\u{53f7}", "target_id"),
                ("\u{76ee}\u{6807}\u{540d}\u{79f0}", "target_name"),
                ("\u{5019}\u{9009}\u{8303}\u{56f4}", "candidate_scope_label"),
                ("\u{7269}\u{79cd}", "organism"),
                ("\u{7efc}\u{5408}\u{5206}\u{6570}", "final_score"),
                (
                    "\u{5df2}\u{77e5}\u{5e93}\u{5206}\u{6570}",
                    "known_database_score",
                ),
                (
                    "\u{5b8f}\u{57fa}\u{56e0}\u{7ec4}\u{5206}\u{6570}",
                    "metagenome_score",
                ),
                ("\u{7ed3}\u{6784}\u{5206}\u{6570}", "structure_score"),
                ("\u{5bf9}\u{63a5}\u{5206}\u{6570}", "docking_score"),
                ("\u{52a8}\u{529b}\u{5b66}\u{5206}\u{6570}", "md_score"),
                (
                    "\u{52a8}\u{529b}\u{5b66}\u{652f}\u{6301}",
                    "md_support_label",
                ),
                ("\u{98ce}\u{9669}\u{63d0}\u{793a}", "risk_note"),
                ("\u{8bc1}\u{636e}\u{6458}\u{8981}", "evidence_summary"),
                ("\u{72b6}\u{6001}", "status_label"),
                ("\u{8f93}\u{51fa}\u{6750}\u{6599}", "result_source"),
            ],
        ),
        "evidence" => user_tsv_download(
            user_evidence_rows(&run_dir),
            &[
                ("\u{8bc1}\u{636e}\u{7c7b}\u{578b}", "evidence_type"),
                ("\u{6765}\u{6e90}", "source"),
                ("\u{72b6}\u{6001}", "status"),
                ("\u{8bf4}\u{660e}", "summary"),
                ("\u{8f93}\u{51fa}\u{6750}\u{6599}", "result_source"),
            ],
        ),
        "structure_patch" => user_tsv_download(
            user_structure_rows(&run_dir, &structure_viewer_payload(&run_dir)),
            &[
                ("\u{5019}\u{9009}\u{7f16}\u{53f7}", "candidate_id"),
                ("\u{76ee}\u{6807}\u{7f16}\u{53f7}", "target_id"),
                ("\u{76ee}\u{6807}\u{540d}\u{79f0}", "target_name"),
                ("\u{7ed3}\u{6784}\u{5206}\u{6570}", "structure_score"),
                ("\u{5bf9}\u{63a5}\u{5206}\u{6570}", "docking_score"),
                ("\u{7ed3}\u{6784}\u{8bc1}\u{636e}", "summary"),
                ("\u{72b6}\u{6001}", "status"),
                ("\u{67e5}\u{770b}\u{5668}", "viewer_status"),
                ("\u{8f93}\u{51fa}\u{6750}\u{6599}", "result_source"),
            ],
        ),
        "md_summary" => user_tsv_download(
            user_md_rows(&run_dir),
            &[
                ("\u{5019}\u{9009}\u{7f16}\u{53f7}", "candidate_id"),
                ("\u{76ee}\u{6807}\u{7f16}\u{53f7}", "target_id"),
                ("\u{76ee}\u{6807}\u{540d}\u{79f0}", "target_name"),
                ("\u{52a8}\u{529b}\u{5b66}\u{5206}\u{6570}", "md_score"),
                ("RMSD", "rmsd"),
                ("RMSF", "rmsf"),
                ("\u{914d}\u{4f53}\u{6f02}\u{79fb}", "ligand_drift"),
                (
                    "\u{63a5}\u{89e6}\u{5360}\u{6709}\u{7387}",
                    "contact_occupancy",
                ),
                ("\u{6458}\u{8981}", "summary"),
                ("\u{72b6}\u{6001}", "status"),
                ("\u{8f68}\u{8ff9}\u{56fe}", "trajectory_status"),
                ("\u{8f93}\u{51fa}\u{6750}\u{6599}", "result_source"),
            ],
        ),
        _ => json_response(404, json!({"error":"artifact_not_available"})),
    }
}

fn evidence_endpoint_json(options: &ServerOptions, run_id: &str) -> Value {
    let Some(run_dir) = completed_run_dir(options, run_id) else {
        return json!({"run_id":run_id,"evidence":[],"row_count":0,"has_output":false});
    };
    let rows = user_evidence_rows(&run_dir);
    json!({
        "run_id": run_id,
        "evidence": rows,
        "row_count": rows.len(),
        "has_output": !rows.is_empty()
    })
}

fn candidates_endpoint_json(options: &ServerOptions, run_id: &str) -> Value {
    let Some(run_dir) = completed_run_dir(options, run_id) else {
        return json!({"run_id":run_id,"candidates":[],"row_count":0,"has_output":false});
    };
    let rows = user_candidate_rows(&run_dir);
    json!({
        "run_id": run_id,
        "candidates": rows,
        "row_count": rows.len(),
        "has_output": !rows.is_empty()
    })
}

fn candidate_detail_json(options: &ServerOptions, run_id: &str) -> Value {
    let Some(run_dir) = completed_run_dir(options, run_id) else {
        return json!({"run_id":run_id,"candidate_details":[],"candidate_count":0,"has_output":false});
    };
    let path = run_dir.join("candidate_detail.json");
    let value = read_json_or(&path, json!({}));
    let rows = value
        .get("candidates")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(user_candidate_detail_row)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let candidate_count = rows.len();
    json!({
        "run_id": run_id,
        "candidate_details": rows,
        "candidate_count": candidate_count,
        "has_output": candidate_count > 0
    })
}

fn structure_endpoint_json(options: &ServerOptions, run_id: &str) -> Value {
    let Some(run_dir) = completed_run_dir(options, run_id) else {
        return json!({"run_id":run_id,"structure_rows":[],"row_count":0,"has_output":false,"viewer":structure_viewer_unavailable("等待运行完成后读取结构坐标输出")});
    };
    let viewer = structure_viewer_payload(&run_dir);
    let rows = user_structure_rows(&run_dir, &viewer);
    json!({
        "run_id": run_id,
        "structure_rows": rows,
        "row_count": rows.len(),
        "has_output": !rows.is_empty(),
        "viewer": viewer
    })
}

fn md_endpoint_json(options: &ServerOptions, run_id: &str) -> Value {
    let Some(run_dir) = completed_run_dir(options, run_id) else {
        return json!({"run_id":run_id,"md_rows":[],"row_count":0,"has_output":false,"charts":[],"readiness":{"ready":false,"reason":"等待运行完成后读取 MD 摘要输出","chart_ready":false}});
    };
    let rows = user_md_rows(&run_dir);
    let charts = md_chart_rows(&run_dir, &rows);
    let readiness = md_readiness_payload(&run_dir, &rows, &charts);
    json!({
        "run_id": run_id,
        "md_rows": rows,
        "row_count": rows.len(),
        "has_output": !rows.is_empty(),
        "charts": charts,
        "readiness": readiness
    })
}

fn genome_endpoint_json(options: &ServerOptions, run_id: &str) -> Value {
    let Some(run_dir) = completed_run_dir(options, run_id) else {
        return json!({"run_id":run_id,"genome_context":[],"row_count":0,"has_output":false});
    };
    let rows = user_genome_rows(&run_dir);
    json!({
        "run_id": run_id,
        "genome_context": rows,
        "row_count": rows.len(),
        "has_output": !rows.is_empty()
    })
}

fn completed_run_dir(options: &ServerOptions, run_id: &str) -> Option<PathBuf> {
    let run_dir = find_run_dir(options, run_id)?;
    if run_state(&run_dir) == "completed" {
        Some(run_dir)
    } else {
        None
    }
}

fn user_candidate_rows(run_dir: &Path) -> Vec<Value> {
    read_tsv_rows(&run_dir.join("target_candidate_ranking.tsv"))
        .into_iter()
        .map(|row| user_candidate_row(&row))
        .collect()
}

fn user_evidence_rows(run_dir: &Path) -> Vec<Value> {
    read_tsv_rows(&run_dir.join("evidence_ledger.tsv"))
        .into_iter()
        .map(user_evidence_row)
        .collect()
}

fn user_structure_rows(run_dir: &Path, viewer: &Value) -> Vec<Value> {
    let targets = candidate_target_lookup(run_dir);
    read_tsv_rows(&run_dir.join("structure_patch_summary.tsv"))
        .into_iter()
        .map(|row| user_structure_row(&row, &targets, viewer))
        .collect()
}

fn user_md_rows(run_dir: &Path) -> Vec<Value> {
    let targets = candidate_target_lookup(run_dir);
    read_tsv_rows(&run_dir.join("md_summary.tsv"))
        .into_iter()
        .map(|row| user_md_row(&row, &targets))
        .collect()
}

fn structure_viewer_unavailable(reason: &str) -> Value {
    json!({
        "ready": false,
        "source": "等待当前运行结构坐标输出",
        "artifact": "",
        "reason": user_artifact_reason(reason),
        "atom_count": 0,
        "atoms": []
    })
}

fn structure_viewer_payload(run_dir: &Path) -> Value {
    for rel in [
        "structure_viewer.json",
        "viewer_structure.json",
        "tool_outputs/structure_viewer.json",
    ] {
        let path = run_dir.join(rel);
        if let Some(value) = read_viewer_json(&path, rel) {
            return value;
        }
    }
    for rel in [
        "structure_viewer.pdb",
        "complex.pdb",
        "docking_pose.pdb",
        "pose.pdb",
        "structure.pdb",
        "tool_outputs/structure_viewer.pdb",
    ] {
        let path = run_dir.join(rel);
        if let Some(value) = read_coordinate_text(&path, rel, CoordinateFormat::Pdb) {
            return value;
        }
    }
    for rel in [
        "structure_viewer.cif",
        "structure_viewer.mmcif",
        "complex.cif",
        "complex.mmcif",
        "tool_outputs/structure_viewer.cif",
        "tool_outputs/structure_viewer.mmcif",
    ] {
        let path = run_dir.join(rel);
        if let Some(value) = read_coordinate_text(&path, rel, CoordinateFormat::Mmcif) {
            return value;
        }
    }
    structure_viewer_unavailable(&structure_viewer_reason(run_dir))
}

fn read_viewer_json(path: &Path, artifact: &str) -> Option<Value> {
    let text = fs::read_to_string(path).ok()?;
    let value = serde_json::from_str::<Value>(&text).ok()?;
    let atoms = value
        .get("atoms")
        .and_then(Value::as_array)?
        .iter()
        .filter_map(sanitize_atom_json)
        .take(5000)
        .collect::<Vec<_>>();
    if atoms.is_empty() {
        return None;
    }
    Some(json!({
        "ready": true,
        "source": "当前运行结构坐标输出",
        "artifact": artifact,
        "reason": "已读取当前运行坐标产物",
        "atom_count": atoms.len(),
        "atoms": atoms
    }))
}

fn sanitize_atom_json(value: &Value) -> Option<Value> {
    let x = value.get("x").and_then(Value::as_f64)?;
    let y = value.get("y").and_then(Value::as_f64)?;
    let z = value.get("z").and_then(Value::as_f64)?;
    Some(json!({
        "element": clean_element(value.get("element").and_then(Value::as_str).unwrap_or("C")),
        "x": x,
        "y": y,
        "z": z
    }))
}

#[derive(Clone, Copy)]
enum CoordinateFormat {
    Pdb,
    Mmcif,
}

fn read_coordinate_text(path: &Path, artifact: &str, format: CoordinateFormat) -> Option<Value> {
    let text = fs::read_to_string(path).ok()?;
    let atoms = match format {
        CoordinateFormat::Pdb => parse_pdb_atoms(&text),
        CoordinateFormat::Mmcif => parse_mmcif_atoms(&text),
    };
    if atoms.is_empty() {
        return None;
    }
    Some(json!({
        "ready": true,
        "source": "当前运行结构坐标输出",
        "artifact": artifact,
        "reason": "已读取当前运行坐标产物",
        "atom_count": atoms.len(),
        "atoms": atoms
    }))
}

fn structure_viewer_reason(run_dir: &Path) -> String {
    read_status_reason(
        &run_dir.join("structure_viewer_status.tsv"),
        "当前运行没有可显示的结构坐标输出。",
    )
}

fn parse_pdb_atoms(text: &str) -> Vec<Value> {
    text.lines()
        .filter(|line| line.starts_with("ATOM") || line.starts_with("HETATM"))
        .filter_map(|line| {
            let x = fixed_f64(line, 30, 38)?;
            let y = fixed_f64(line, 38, 46)?;
            let z = fixed_f64(line, 46, 54)?;
            let element = fixed_slice(line, 76, 78)
                .filter(|value| !value.trim().is_empty())
                .or_else(|| fixed_slice(line, 12, 16))
                .unwrap_or("C");
            Some(json!({"element": clean_element(element), "x": x, "y": y, "z": z}))
        })
        .take(5000)
        .collect()
}

fn parse_mmcif_atoms(text: &str) -> Vec<Value> {
    text.lines()
        .filter(|line| line.starts_with("ATOM ") || line.starts_with("HETATM "))
        .filter_map(|line| {
            let cols = line.split_whitespace().collect::<Vec<_>>();
            let element = cols.get(2).copied().unwrap_or("C");
            let x = cols.get(10).and_then(|value| value.parse::<f64>().ok())?;
            let y = cols.get(11).and_then(|value| value.parse::<f64>().ok())?;
            let z = cols.get(12).and_then(|value| value.parse::<f64>().ok())?;
            Some(json!({"element": clean_element(element), "x": x, "y": y, "z": z}))
        })
        .take(5000)
        .collect()
}

fn fixed_slice(value: &str, start: usize, end: usize) -> Option<&str> {
    value.get(start..end)
}

fn fixed_f64(value: &str, start: usize, end: usize) -> Option<f64> {
    fixed_slice(value, start, end)?.trim().parse::<f64>().ok()
}

fn clean_element(value: &str) -> String {
    let mut chars = value
        .chars()
        .filter(|ch| ch.is_ascii_alphabetic())
        .take(2)
        .collect::<String>()
        .to_ascii_uppercase();
    if chars.is_empty() {
        chars = "C".to_string();
    }
    chars
}

fn md_chart_rows(run_dir: &Path, rows: &[Value]) -> Vec<Value> {
    if rows.is_empty() {
        return Vec::new();
    }
    let mut charts = Vec::new();
    for row in rows.iter().take(3) {
        let candidate = row_text(row, "candidate_id");
        for (key, label, unit) in [
            ("rmsd", "RMSD", "Å"),
            ("rmsf", "RMSF", "Å"),
            ("ligand_drift", "配体漂移", "Å"),
            ("contact_occupancy", "接触占有率", "fraction"),
            ("hydrogen_bond_occupancy", "氢键占有率", "fraction"),
        ] {
            if let Some(value) = row_number(row, key) {
                charts.push(json!({
                    "candidate_id": candidate,
                    "label": label,
                    "metric": key,
                    "value": value,
                    "display_value": format_metric_value(value, unit),
                    "unit": unit,
                    "time_step": first_non_empty(row_text(row, "time_step"), "summary"),
                    "artifact": "md_summary.tsv"
                }));
            }
        }
    }
    charts.extend(score_chart_rows(run_dir));
    charts
}

fn md_readiness_payload(run_dir: &Path, rows: &[Value], charts: &[Value]) -> Value {
    if !rows.is_empty() {
        return json!({
            "ready": true,
            "reason": "已读取当前运行的 MD 摘要 artifact",
            "chart_ready": !charts.is_empty()
        });
    }
    json!({
        "ready": false,
        "reason": read_status_reason(
            &run_dir.join("md_summary_status.tsv"),
            "当前运行没有真实轨迹或 MD 摘要 artifact。"
        ),
        "chart_ready": false
    })
}

fn read_status_reason(path: &Path, fallback: &str) -> String {
    let rows = read_tsv_rows(path);
    let reason = rows
        .first()
        .and_then(|row| row.get("reason").and_then(Value::as_str))
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(fallback)
        .to_string();
    user_artifact_reason(&reason)
}

fn user_artifact_reason(reason: &str) -> String {
    let lower = reason.to_ascii_lowercase();
    if lower.contains("no small_molecule input available for md summary") {
        return "当前 MD worker 需要可参数化的 SMILES/SDF 小分子配体或复合物坐标；序列类输入（包括短肽）已用于候选和序列匹配，尚未转换为 MD 配体/复合物输入。".to_string();
    }
    if lower.contains("no structure interaction match rows") {
        return "当前运行没有结构匹配，因此未生成结构查看数据或口袋摘要。".to_string();
    }
    if lower.contains("no md_summary.tsv") || lower.contains("no rmsd metric artifact") {
        return "当前运行没有真实 MD 轨迹或 RMSD 等指标，因此未生成动力学摘要。".to_string();
    }
    reason.to_string()
}

fn score_chart_rows(run_dir: &Path) -> Vec<Value> {
    read_tsv_rows(&run_dir.join("score_decomposition.tsv"))
        .into_iter()
        .take(8)
        .filter_map(|row| {
            let value = row_number(&row, "score").or_else(|| row_number(&row, "value"))?;
            Some(json!({
                "candidate_id": row_text(&row, "candidate_id"),
                "label": first_non_empty(row_text(&row, "component"), row_text(&row, "score_component")),
                "metric": "score_decomposition",
                "value": value,
                "display_value": format!("{value:.3}"),
                "unit": "score",
                "time_step": "summary",
                "artifact": "score_decomposition.tsv"
            }))
        })
        .collect()
}

fn row_number(row: &Value, key: &str) -> Option<f64> {
    row.get(key)
        .and_then(Value::as_str)
        .and_then(|value| value.trim().parse::<f64>().ok())
}

fn json_number(value: Option<&Value>) -> Option<f64> {
    value.and_then(|value| {
        value.as_f64().or_else(|| {
            value
                .as_str()
                .and_then(|text| text.trim().parse::<f64>().ok())
        })
    })
}

fn md_support_label(md_score: Option<f64>) -> &'static str {
    match md_score {
        Some(value) if value > 0.0 => "\u{6709}\u{77ed}\u{7a0b} MD \u{652f}\u{6301}",
        _ => "\u{672a}\u{751f}\u{6210} MD \u{652f}\u{6301}",
    }
}

fn candidate_risk_note(scope: &str, md_score: Option<f64>) -> String {
    let mut parts = Vec::new();
    if scope == "known_nonhuman_protein" {
        parts.push("\u{975e}\u{4eba}\u{6e90}\u{5df2}\u{77e5}\u{86cb}\u{767d}\u{ff1b}\u{7ed3}\u{5408}\u{7814}\u{7a76}\u{5bf9}\u{8c61}\u{89e3}\u{8bfb}");
    }
    if matches!(md_score, Some(value) if value > 0.0) {
        parts.push("\u{77ed}\u{7a0b}\u{53d7}\u{9650} receptor \u{6a21}\u{578b}\u{ff1b}\u{7528}\u{4e8e}\u{5019}\u{9009}\u{6392}\u{5e8f}");
    } else {
        parts.push("MD \u{652f}\u{6301}\u{672a}\u{751f}\u{6210}\u{ff1b}\u{6309}\u{5176}\u{4ed6}\u{8bc1}\u{636e}\u{6392}\u{5e8f}");
    }
    parts.join("\u{ff1b}")
}

fn format_metric_value(value: f64, unit: &str) -> String {
    if unit == "fraction" {
        format!("{value:.3}")
    } else {
        format!("{value:.3} {unit}")
    }
}

fn user_genome_rows(run_dir: &Path) -> Vec<Value> {
    read_tsv_rows(&run_dir.join("metagenome_potential_matches.tsv"))
        .into_iter()
        .map(|row| user_genome_row(&row))
        .collect()
}

fn user_candidate_row(row: &Value) -> Value {
    let scope = row_text(row, "candidate_scope");
    let status = first_non_empty(row_text(row, "status"), "computed_prediction");
    let md_score = row_number(row, "md_score");
    json!({
        "rank": row_text(row, "rank"),
        "candidate_id": row_text(row, "candidate_id"),
        "target_id": row_text(row, "target_id"),
        "target_name": row_text(row, "target_name"),
        "candidate_scope": scope,
        "candidate_scope_label": friendly_candidate_scope(scope),
        "organism": row_text(row, "organism"),
        "final_score": row_text(row, "final_score"),
        "known_database_score": row_text(row, "known_database_score"),
        "metagenome_score": row_text(row, "metagenome_score"),
        "structure_score": row_text(row, "structure_score"),
        "docking_score": row_text(row, "docking_score"),
        "md_score": row_text(row, "md_score"),
        "md_support_label": md_support_label(md_score),
        "risk_note": candidate_risk_note(scope, md_score),
        "evidence_summary": row_text(row, "evidence_summary"),
        "status": status,
        "status_label": friendly_status_label(status),
        "result_source": "候选排序输出"
    })
}

fn user_genome_row(row: &Value) -> Value {
    json!({
        "source": friendly_match_source(row_text(row, "source")),
        "target_id": row_text(row, "target_id"),
        "target_name": row_text(row, "target_name"),
        "organism": row_text(row, "organism"),
        "target_class": row_text(row, "target_class"),
        "match_score": row_text(row, "match_score"),
        "evidence": row_text(row, "evidence"),
        "result_source": "宏基因组比对输出"
    })
}

fn user_candidate_detail_row(row: &Value) -> Value {
    let scores = row.get("scores").unwrap_or(&Value::Null);
    let scope = row
        .get("candidate_scope")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let md_score = json_number(scores.get("md"));
    json!({
        "candidate_id": row.get("candidate_id").cloned().unwrap_or_else(|| json!("")),
        "target_id": row.get("target_id").cloned().unwrap_or_else(|| json!("")),
        "target_name": row.get("target_name").cloned().unwrap_or_else(|| json!("")),
        "candidate_scope": scope,
        "candidate_scope_label": friendly_candidate_scope(scope),
        "organism": row.get("organism").cloned().unwrap_or_else(|| json!("")),
        "final_score": scores.get("final").cloned().unwrap_or_else(|| json!("")),
        "known_database_score": scores.get("known_database").cloned().unwrap_or_else(|| json!("")),
        "metagenome_score": scores.get("metagenome").cloned().unwrap_or_else(|| json!("")),
        "structure_score": scores.get("structure").cloned().unwrap_or_else(|| json!("")),
        "docking_score": scores.get("docking").cloned().unwrap_or_else(|| json!("")),
        "md_score": scores.get("md").cloned().unwrap_or_else(|| json!("")),
        "md_support_label": md_support_label(md_score),
        "risk_note": candidate_risk_note(scope, md_score),
        "evidence_summary": row.get("evidence_summary").cloned().unwrap_or_else(|| json!("")),
        "status": "\u{8ba1}\u{7b97}\u{5019}\u{9009}",
        "status_label": "\u{8ba1}\u{7b97}\u{5019}\u{9009}",
        "detail_source": "\u{5f53}\u{524d}\u{8fd0}\u{884c}\u{7684}\u{5019}\u{9009}\u{8be6}\u{60c5}\u{8f93}\u{51fa}",
        "result_source": "\u{5019}\u{9009}\u{8be6}\u{60c5}\u{8f93}\u{51fa}"
    })
}

fn user_structure_row(
    row: &Value,
    targets: &BTreeMap<String, (String, String)>,
    viewer: &Value,
) -> Value {
    let candidate_id = row_text(row, "candidate_id");
    let (fallback_target_id, fallback_target_name) = targets
        .get(candidate_id)
        .map(|value| (value.0.as_str(), value.1.as_str()))
        .unwrap_or(("", ""));
    let target_id = first_non_empty(row_text(row, "target_id"), fallback_target_id);
    let target_name = first_non_empty(row_text(row, "target_name"), fallback_target_name);
    let structure_score = row_text(row, "structure_score");
    let docking_score = row_text(row, "docking_score");
    let summary = first_non_empty(
        row_text(row, "evidence_summary"),
        first_non_empty(
            row_text(row, "structure_evidence"),
            first_non_empty(
                row_text(row, "patch_evidence"),
                row_text(row, "key_residues"),
            ),
        ),
    );
    json!({
        "candidate_id": candidate_id,
        "target_id": target_id,
        "target_name": target_name,
        "structure_score": structure_score,
        "docking_score": docking_score,
        "summary": summary,
        "status": if structure_score.is_empty() && docking_score.is_empty() {
            "\u{7b49}\u{5f85}\u{7ed3}\u{6784}\u{8f93}\u{51fa}"
        } else {
            "\u{5df2}\u{751f}\u{6210}\u{7ed3}\u{6784}\u{6458}\u{8981}"
        },
        "viewer_status": structure_viewer_status_text(viewer),
        "result_source": "\u{7ed3}\u{6784}\u{4e0e}\u{53e3}\u{888b}\u{6458}\u{8981}\u{8f93}\u{51fa}"
    })
}

fn structure_viewer_status_text(viewer: &Value) -> &'static str {
    if viewer
        .get("ready")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        "三维查看已就绪"
    } else {
        "三维查看未就绪"
    }
}

fn user_md_row(row: &Value, targets: &BTreeMap<String, (String, String)>) -> Value {
    let candidate_id = row_text(row, "candidate_id");
    let (fallback_target_id, fallback_target_name) = targets
        .get(candidate_id)
        .map(|value| (value.0.as_str(), value.1.as_str()))
        .unwrap_or(("", ""));
    let target_id = first_non_empty(row_text(row, "target_id"), fallback_target_id);
    let target_name = first_non_empty(row_text(row, "target_name"), fallback_target_name);
    let md_score = first_non_empty(
        row_text(row, "md_score"),
        row_text(row, "md_stability_score"),
    );
    let contact_occupancy = row_text(row, "contact_occupancy");
    let hydrogen_bond_occupancy = row_text(row, "hydrogen_bond_occupancy");
    let interaction_persistence = row_text(row, "interaction_persistence");
    let summary = md_summary_text(
        contact_occupancy,
        hydrogen_bond_occupancy,
        interaction_persistence,
    );
    json!({
        "candidate_id": candidate_id,
        "target_id": target_id,
        "target_name": target_name,
        "md_score": md_score,
        "rmsd": row_text(row, "rmsd"),
        "rmsf": row_text(row, "rmsf"),
        "ligand_drift": row_text(row, "ligand_drift"),
        "hydrogen_bond_occupancy": hydrogen_bond_occupancy,
        "contact_occupancy": contact_occupancy,
        "interaction_persistence": interaction_persistence,
        "summary": summary,
        "status": if md_score.is_empty() && row_text(row, "rmsd").is_empty() {
            "\u{7b49}\u{5f85}\u{52a8}\u{529b}\u{5b66}\u{8f93}\u{51fa}"
        } else {
            "\u{5df2}\u{751f}\u{6210}\u{52a8}\u{529b}\u{5b66}\u{6458}\u{8981}"
        },
        "trajectory_status": "\u{8f68}\u{8ff9}\u{56fe}\u{672a}\u{5c31}\u{7eea}",
        "result_source": "\u{52a8}\u{529b}\u{5b66}\u{6458}\u{8981}\u{8f93}\u{51fa}"
    })
}

fn user_evidence_row(row: Value) -> Value {
    let claim = row
        .get("claim_id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let artifact = row
        .get("evidence_artifact")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let status = row
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let notes = row.get("notes").and_then(Value::as_str).unwrap_or_default();
    json!({
        "evidence_type": evidence_type_label(claim, artifact),
        "source": evidence_source_label(artifact),
        "status": evidence_status_label(status),
        "summary": evidence_note_summary(notes),
        "result_source": "证据链输出"
    })
}

fn candidate_target_lookup(run_dir: &Path) -> BTreeMap<String, (String, String)> {
    read_tsv_rows(&run_dir.join("target_candidate_ranking.tsv"))
        .into_iter()
        .filter_map(|row| {
            let candidate_id = row_text(&row, "candidate_id");
            if candidate_id.is_empty() {
                None
            } else {
                Some((
                    candidate_id.to_string(),
                    (
                        row_text(&row, "target_id").to_string(),
                        row_text(&row, "target_name").to_string(),
                    ),
                ))
            }
        })
        .collect()
}

fn row_text<'a>(row: &'a Value, key: &str) -> &'a str {
    row.get(key).and_then(Value::as_str).unwrap_or_default()
}

fn first_non_empty<'a>(primary: &'a str, fallback: &'a str) -> &'a str {
    if primary.trim().is_empty() {
        fallback
    } else {
        primary
    }
}

fn md_summary_text(contact: &str, hydrogen_bond: &str, persistence: &str) -> String {
    let mut parts = Vec::new();
    if !contact.trim().is_empty() {
        parts.push(format!(
            "\u{63a5}\u{89e6}\u{5360}\u{6709}\u{7387} {contact}"
        ));
    }
    if !hydrogen_bond.trim().is_empty() {
        parts.push(format!(
            "\u{6c22}\u{952e}\u{5360}\u{6709}\u{7387} {hydrogen_bond}"
        ));
    }
    if !persistence.trim().is_empty() {
        parts.push(format!(
            "\u{76f8}\u{4e92}\u{4f5c}\u{7528}\u{6301}\u{7eed}\u{6027} {persistence}"
        ));
    }
    parts.join("\u{ff1b}")
}

fn friendly_match_source(source: &str) -> String {
    let trimmed = source.trim();
    let lowered = trimmed.to_ascii_lowercase();
    if lowered.contains("metagenome") || lowered.contains("orf") {
        "\u{5b8f}\u{57fa}\u{56e0}\u{7ec4}\u{6f5c}\u{5728}\u{5e8f}\u{5217}\u{7d22}\u{5f15}"
            .to_string()
    } else if lowered.contains("chembl")
        || lowered.contains("bindingdb")
        || lowered.contains("uniprot")
    {
        "\u{5df2}\u{77e5}\u{86cb}\u{767d}\u{6570}\u{636e}\u{5e93}".to_string()
    } else if trimmed.contains('/')
        || trimmed.contains('\\')
        || trimmed.ends_with(".tsv")
        || trimmed.ends_with(".json")
    {
        "\u{79d1}\u{5b66}\u{6570}\u{636e}\u{5e93}".to_string()
    } else {
        trimmed.to_string()
    }
}

fn friendly_candidate_scope(scope: &str) -> &'static str {
    match scope {
        "known_database" | "known_human_protein" => {
            "\u{5df2}\u{77e5}\u{86cb}\u{767d}\u{6570}\u{636e}\u{5e93}"
        }
        "known_nonhuman_protein" => {
            "\u{5df2}\u{77e5}\u{975e}\u{4eba}\u{6e90}\u{86cb}\u{767d}\u{6570}\u{636e}\u{5e93}"
        }
        "metagenome_potential" | "metagenome_potential_protein" => {
            "\u{5b8f}\u{57fa}\u{56e0}\u{7ec4}\u{6f5c}\u{5728}\u{5e8f}\u{5217}"
        }
        "structure_index" | "structure_interaction" => "\u{7ed3}\u{6784}\u{7d22}\u{5f15}",
        _ => "\u{672a}\u{5206}\u{7c7b}",
    }
}

fn friendly_status_label(status: &str) -> &'static str {
    match status {
        "computed_prediction" => "\u{8ba1}\u{7b97}\u{5019}\u{9009}",
        "completed" => "\u{5df2}\u{5b8c}\u{6210}",
        "blocked" => "\u{9700}\u{8981}\u{5904}\u{7406}",
        "failed" => "\u{5931}\u{8d25}",
        "running" => "\u{8fd0}\u{884c}\u{4e2d}",
        _ => "\u{8bb0}\u{5f55}",
    }
}

fn user_tsv_download(
    rows: Vec<Value>,
    fields: &[(&'static str, &'static str)],
) -> (u16, &'static str, String) {
    if rows.is_empty() {
        return json_response(404, json!({"error":"artifact_not_available"}));
    }
    let mut text = fields
        .iter()
        .map(|(label, _)| escape_tsv(label))
        .collect::<Vec<_>>()
        .join("\t");
    text.push('\n');
    for row in rows {
        text.push_str(
            &fields
                .iter()
                .map(|(_, key)| escape_tsv(row_text(&row, key)))
                .collect::<Vec<_>>()
                .join("\t"),
        );
        text.push('\n');
    }
    (200, "text/tab-separated-values; charset=utf-8", text)
}

fn escape_tsv(value: &str) -> String {
    value
        .replace('\t', " ")
        .replace(['\r', '\n'], " ")
        .trim()
        .to_string()
}

fn evidence_type_label(claim: &str, artifact: &str) -> &'static str {
    let value = if artifact.is_empty() { claim } else { artifact };
    match value {
        "input_normalized_manifest.tsv" | "input_normalized_manifest" => {
            "\u{8f93}\u{5165}\u{89c4}\u{8303}\u{5316}"
        }
        "feature_manifest.tsv" | "feature_manifest" => "\u{7279}\u{5f81}\u{8ba1}\u{7b97}",
        "known_database_matches.tsv" | "known_database_matches" => {
            "\u{5df2}\u{77e5}\u{6570}\u{636e}\u{5e93}\u{6bd4}\u{5bf9}"
        }
        "metagenome_potential_matches.tsv" | "metagenome_potential_matches" => {
            "\u{5b8f}\u{57fa}\u{56e0}\u{7ec4}\u{6f5c}\u{5728}\u{5e8f}\u{5217}\u{6bd4}\u{5bf9}"
        }
        "structure_patch_summary.tsv" | "structure_patch_summary" => {
            "\u{7ed3}\u{6784}\u{4e0e}\u{53e3}\u{888b}"
        }
        "worker_runtime_summary.tsv" | "worker_runtime_summary" => {
            "\u{5de5}\u{5177}\u{9002}\u{914d}\u{5668}\u{68c0}\u{67e5}"
        }
        "docking_summary.tsv" | "docking_summary" => "\u{5bf9}\u{63a5}\u{6458}\u{8981}",
        "md_summary.tsv" | "md_summary" => "\u{52a8}\u{529b}\u{5b66}\u{6458}\u{8981}",
        "target_candidate_ranking.tsv" | "target_candidate_ranking" => {
            "\u{5019}\u{9009}\u{6392}\u{5e8f}"
        }
        "score_decomposition.tsv" | "score_decomposition" => "\u{5206}\u{6570}\u{62c6}\u{89e3}",
        "candidate_detail.json" | "candidate_detail" => "\u{5019}\u{9009}\u{8be6}\u{60c5}",
        "research_report.md" | "research_report" => "\u{79d1}\u{7814}\u{62a5}\u{544a}",
        _ => "\u{8fd0}\u{884c}\u{8bc1}\u{636e}",
    }
}

fn evidence_source_label(artifact: &str) -> &'static str {
    match artifact {
        "known_database_matches.tsv" => {
            "ChEMBL\u{3001}BindingDB\u{3001}UniProt \u{7b49}\u{5df2}\u{77e5}\u{6570}\u{636e}\u{5e93}"
        }
        "metagenome_potential_matches.tsv" => {
            "\u{5b8f}\u{57fa}\u{56e0}\u{7ec4}\u{6f5c}\u{5728}\u{5e8f}\u{5217}\u{7d22}\u{5f15}"
        }
        "worker_runtime_summary.tsv" => {
            "\u{5f53}\u{524d}\u{8fd0}\u{884c}\u{7684}\u{5de5}\u{5177}\u{9002}\u{914d}\u{5668}"
        }
        "structure_patch_summary.tsv" | "docking_summary.tsv" | "md_summary.tsv" => {
            "\u{5f53}\u{524d}\u{8fd0}\u{884c}\u{7684}\u{5de5}\u{5177}\u{8f93}\u{51fa}"
        }
        "target_candidate_ranking.tsv" | "score_decomposition.tsv" | "candidate_detail.json" => {
            "\u{5f53}\u{524d}\u{8fd0}\u{884c}\u{7684}\u{5019}\u{9009}\u{7ed3}\u{679c}"
        }
        "research_report.md" => {
            "\u{5f53}\u{524d}\u{8fd0}\u{884c}\u{7684}\u{62a5}\u{544a}\u{8f93}\u{51fa}"
        }
        _ => "\u{5f53}\u{524d}\u{8fd0}\u{884c}\u{8f93}\u{51fa}",
    }
}

fn evidence_status_label(status: &str) -> &'static str {
    match status {
        "available" => "\u{5df2}\u{751f}\u{6210}",
        "missing" | "not_ready" | "not_generated" => "\u{672a}\u{751f}\u{6210}",
        "no_match" => "\u{672a}\u{5339}\u{914d}",
        "no_candidate" => "\u{672a}\u{751f}\u{6210}\u{5019}\u{9009}",
        _ => "\u{5df2}\u{8bb0}\u{5f55}",
    }
}

fn evidence_note_summary(notes: &str) -> String {
    if notes.is_empty() {
        return "\u{5f53}\u{524d}\u{8fd0}\u{884c}\u{5df2}\u{8bb0}\u{5f55}\u{8be5}\u{8bc1}\u{636e}\u{3002}".to_string();
    }
    if notes.contains("no docking rows") {
        return "\u{5f53}\u{524d}\u{8fd0}\u{884c}\u{6ca1}\u{6709}\u{751f}\u{6210}\u{5bf9}\u{63a5}\u{884c}\u{3002}".to_string();
    }
    if notes.contains("no dynamics rows") {
        return "\u{5f53}\u{524d}\u{8fd0}\u{884c}\u{6ca1}\u{6709}\u{751f}\u{6210}\u{52a8}\u{529b}\u{5b66}\u{884c}\u{3002}".to_string();
    }
    if notes.contains("no known database match rows") {
        return "\u{5df2}\u{5b8c}\u{6210}\u{5df2}\u{77e5}\u{6570}\u{636e}\u{5e93}\u{6bd4}\u{5bf9}\u{ff0c}\u{672a}\u{5f97}\u{5230}\u{5339}\u{914d}\u{884c}\u{3002}".to_string();
    }
    if notes.contains("no metagenome match rows") {
        return "\u{5df2}\u{5b8c}\u{6210}\u{5b8f}\u{57fa}\u{56e0}\u{7ec4}\u{5e8f}\u{5217}\u{6bd4}\u{5bf9}\u{ff0c}\u{672a}\u{5f97}\u{5230}\u{5339}\u{914d}\u{884c}\u{3002}".to_string();
    }
    if notes.contains("content_id:") || notes.contains(&["sha", "256:"].concat()) {
        return "\u{5df2}\u{8bb0}\u{5f55}\u{5185}\u{5bb9}\u{4e00}\u{81f4}\u{6027}\u{6807}\u{8bc6}\u{ff0c}\u{53ef}\u{7528}\u{4e8e}\u{590d}\u{6838}\u{3002}".to_string();
    }
    notes.replace(['\t', '\r', '\n'], " ")
}

fn databases_json(options: &ServerOptions) -> Value {
    let store = science_store_path(options);
    let required = [
        "science_store_manifest.tsv",
        "indexes/known_targets.tsv",
        "indexes/metagenome_targets.tsv",
        "indexes/structure_targets.tsv",
        "indexes/tool_versions.tsv",
    ];
    let rows = required
        .iter()
        .map(|rel| {
            let path = store.join(rel);
            let ready = if *rel == "indexes/tool_versions.tsv" {
                tool_versions_ready(&path)
            } else {
                path.exists()
            };
            json!({"item":rel,"path":path.display().to_string(),"ready":ready})
        })
        .collect::<Vec<_>>();
    json!({"science_store":store.display().to_string(),"required":rows,"user_configures_paths":false})
}

fn user_health_json() -> Value {
    json!({"status":"ready","application":PRODUCT_NAME,"version":PRODUCT_VERSION})
}

fn user_databases_json(options: &ServerOptions) -> Value {
    let store = science_store_path(options);
    let rows = required_resource_rows(&store);
    let missing = missing_resource_rows(&rows);
    json!({
        "required": rows,
        "sources": source_status_rows(&store),
        "missing": missing,
        "summary": if preflight_store_ready(options) {"科学仓库已就绪"} else {"需要运行安装/修复"},
        "repair_actions_available": store.join("repair_actions.tsv").exists()
    })
}

fn diagnostics_json(options: &ServerOptions) -> Value {
    let cargo = capture("cargo", &["--version"], &options.workspace_root);
    let gls_cli = capture_command(&gls_cli_help_command(options), &options.workspace_root);
    json!({
        "status": if cargo.0 && gls_cli.0 {"ready"} else {"warning"},
        "runtime": std::env::consts::OS,
        "cargo": {"ok":cargo.0,"text":cargo.1},
        "gls_cli": {"ok":gls_cli.0,"text":gls_cli.1},
        "science_store": databases_json(options)
    })
}

fn user_diagnostics_json(options: &ServerOptions) -> Value {
    let gls_cli = capture_command(&gls_cli_help_command(options), &options.workspace_root);
    let database_ready = preflight_store_ready(options);
    let store = science_store_path(options);
    let missing = missing_resource_rows(&required_resource_rows(&store));
    let openmm_platforms = openmm_platform_status(&store);
    let worker = worker_environment_status(options);
    let mut checks = vec![
        json!({"label":"服务", "ready":true, "next_action":"继续"}),
        json!({"label":"运行组件", "ready":gls_cli.0, "next_action": if gls_cli.0 {"继续"} else {"重新运行安装程序"}}),
        json!({"label":"科学仓库", "ready":database_ready, "next_action": if database_ready {"继续"} else {"运行安装/修复"}}),
        json!({
            "label": "Python worker",
            "ready": worker["ready"].as_bool().unwrap_or(false),
            "next_action": worker["next_action"].clone(),
            "detail": worker["summary"].clone()
        }),
    ];
    checks.push(json!({
        "label": "OpenMM/CUDA",
        "ready": openmm_platforms["openmm_ready"].as_bool().unwrap_or(false),
        "next_action": openmm_platforms["next_action"].clone(),
        "detail": openmm_platforms["summary"].clone()
    }));
    json!({
        "checks": checks,
        "missing": missing,
        "worker_environment": worker,
        "science_store": user_databases_json(options),
        "tool_platforms": [openmm_platforms]
    })
}

fn user_setup_status_json(options: &ServerOptions) -> Value {
    let ready = preflight_store_ready(options);
    let store = science_store_path(options);
    let required = required_resource_rows(&store);
    let worker = worker_environment_status(options);
    json!({
        "database_ready": ready && worker.get("ready").and_then(Value::as_bool).unwrap_or(false),
        "repair_action": if ready && worker.get("ready").and_then(Value::as_bool).unwrap_or(false) {"继续"} else {"运行安装/修复"},
        "checks": required,
        "missing": missing_resource_rows(&required),
        "worker_environment": worker,
        "source_records": source_records_status(&store),
        "tool_platforms": [openmm_platform_status(&store)]
    })
}

fn user_setup_progress_json(options: &ServerOptions) -> Value {
    let path = science_store_path(options).join("install_progress.json");
    let value = read_json_or(&path, json!({}));
    let mut progress = setup_user_install_progress(Some(&value));
    if setup_progress_is_stale(&progress) {
        progress["status"] = json!("needs_repair");
        progress["failed_steps"] = json!(1);
        progress["message"] =
            json!("安装进度长时间没有更新，请重新运行安装/修复并检查网络、磁盘和依赖状态。");
    }
    progress
}

fn run_setup_repair_json(options: &ServerOptions) -> Value {
    if setup_progress_is_running(options) {
        let ready = preflight_store_ready(options);
        let required = required_resource_rows(&science_store_path(options));
        return json!({
            "database_ready": ready,
            "message": "安装/修复正在运行，请保持页面打开并查看进度。",
            "checks": required,
            "missing": missing_resource_rows(&required),
            "source_records": source_records_status(&science_store_path(options)),
            "progress": user_setup_progress_json(options)
        });
    }
    let command = setup_repair_command(options);
    let started = Command::new(&command[0])
        .args(&command[1..])
        .current_dir(&options.workspace_root)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    let (message, installer_payload) = match started {
        Ok(_) => {
            write_setup_progress_start(options);
            (
                "安装/修复已启动，正在后台准备科学资源。".to_string(),
                None::<Value>,
            )
        }
        Err(err) => (format!("修复动作无法启动：{err}"), None),
    };
    let ready = preflight_store_ready(options);
    let required = required_resource_rows(&science_store_path(options));
    let user_message = if ready {
        "科学仓库已就绪。".to_string()
    } else {
        installer_payload
            .as_ref()
            .and_then(|payload| payload.get("summary").and_then(Value::as_str))
            .filter(|summary| !summary.trim().is_empty())
            .map(ToOwned::to_owned)
            .unwrap_or(message)
    };
    let mut payload = json!({
        "database_ready": ready,
        "message": user_message,
        "checks": required,
        "missing": missing_resource_rows(&required),
        "source_records": source_records_status(&science_store_path(options))
    });
    if let Some(installer_payload) = installer_payload {
        payload["installer"] = installer_payload;
    }
    payload["progress"] = user_setup_progress_json(options);
    payload
}

fn setup_progress_is_running(options: &ServerOptions) -> bool {
    let value = read_json_or(
        &science_store_path(options).join("install_progress.json"),
        json!({}),
    );
    let progress = setup_user_install_progress(Some(&value));
    progress.get("status").and_then(Value::as_str) == Some("running")
        && !setup_progress_is_stale(&progress)
}

fn setup_progress_is_stale(progress: &Value) -> bool {
    if progress.get("status").and_then(Value::as_str) != Some("running") {
        return false;
    }
    let updated_at = progress
        .get("updated_at")
        .and_then(|value| {
            value
                .as_u64()
                .or_else(|| value.as_str().and_then(|text| text.parse::<u64>().ok()))
        })
        .unwrap_or(0);
    updated_at > 0 && now_seconds().saturating_sub(updated_at) > INSTALL_PROGRESS_STALE_SECONDS
}

fn write_setup_progress_start(options: &ServerOptions) {
    let store = science_store_path(options);
    let _ = fs::create_dir_all(&store);
    let payload = json!({
        "status": "running",
        "stage": "准备安装",
        "current_resource": "科学资源",
        "completed_steps": 0,
        "total_steps": 6,
        "failed_steps": 0,
        "bytes_downloaded": 0,
        "bytes_total": "",
        "percent": 0,
        "message": "安装/修复已启动，正在准备科学资源。",
        "updated_at": now_seconds()
    });
    let _ = write_text(
        &store.join("install_progress.json"),
        &(payload.to_string() + "\n"),
    );
}

fn user_setup_install_plan_json(options: &ServerOptions) -> Value {
    let catalog_rows = config_tsv_data_row_count(options, "source_catalog.tsv");
    let sync_rows = config_tsv_data_row_count(options, "source_sync_plan.tsv");
    let store = science_store_path(options);
    json!({
        "action":"运行安装/修复",
        "description":"准备数据库、索引、计算工具、许可证和来源记录。普通运行不配置数据库路径。",
        "database_ready": preflight_store_ready(options),
        "tool_platforms": [openmm_platform_status(&store)],
        "source_records": source_records_status(&store),
        "direct_sources": {
            "ready": catalog_rows > 0,
            "source_count": catalog_rows,
            "next_action": if catalog_rows > 0 {"运行安装/修复"} else {"使用完整安装包重新运行安装/修复"}
        },
        "external_sources": {
            "ready": sync_rows > 0,
            "source_group_count": sync_rows,
            "next_action": if sync_rows > 0 {"运行安装/修复"} else {"使用完整安装包重新运行安装/修复"}
        }
    })
}

fn config_tsv_data_row_count(options: &ServerOptions, file_name: &str) -> usize {
    let path = options.workspace_root.join("configs").join(file_name);
    let Ok(text) = fs::read_to_string(path) else {
        return 0;
    };
    text.lines()
        .skip(1)
        .filter(|line| !line.trim().is_empty())
        .count()
}

fn setup_repair_command(options: &ServerOptions) -> Vec<String> {
    let installer = options
        .workspace_root
        .join("scripts")
        .join("setup")
        .join("install_complete_scientific.py");
    if installer.exists() {
        let mut command = python_invocation();
        command.extend([
            installer.display().to_string(),
            "--workspace-root".to_string(),
            options.workspace_root.display().to_string(),
            "--science-store".to_string(),
            science_store_path(options).display().to_string(),
        ]);
        if let Ok(catalog) = std::env::var("GENOLIGAND_SOURCE_CATALOG")
            && !catalog.trim().is_empty()
        {
            command.extend(["--source-catalog".to_string(), catalog]);
        }
        return command;
    }
    let mut command = gls_cli_invocation(options);
    command.extend([
        "install".to_string(),
        "prepare-science-store".to_string(),
        "--workspace-root".to_string(),
        options.workspace_root.display().to_string(),
        "--science-store".to_string(),
        science_store_path(options).display().to_string(),
    ]);
    command
}

fn python_invocation() -> Vec<String> {
    match std::env::var("GENOLIGAND_PYTHON") {
        Ok(value) if !value.trim().is_empty() => vec![value],
        _ => vec!["python".to_string()],
    }
}

#[cfg(test)]
fn setup_installer_summary(stdout: &str) -> Option<Value> {
    let payload: Value = serde_json::from_str(stdout.trim()).ok()?;
    let downloads = payload
        .get("downloads")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let resources = setup_user_resources(payload.get("resources"));
    let external_sources = setup_user_external_sources(payload.get("external_sources"));
    let source_records = setup_user_source_manifests(payload.get("source_manifests"));
    let tool_environment = setup_user_tool_environment(payload.get("tool_environment"));
    let progress = setup_user_install_progress(payload.get("progress"));
    let download_status = downloads
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or_default();
    Some(json!({
        "summary": payload.get("summary").cloned().unwrap_or_else(|| json!("")),
        "next_action": payload.get("next_action").cloned().unwrap_or_else(|| json!("运行安装/修复")),
        "downloads": {
            "completed": download_status == "ready",
            "ready": downloads.get("ready").cloned().unwrap_or_else(|| json!(0)),
            "failed": downloads.get("failed").cloned().unwrap_or_else(|| json!(0)),
            "message": downloads.get("message").cloned().unwrap_or_else(|| json!("")),
            "next_action": downloads.get("next_action").cloned().unwrap_or_else(|| json!("运行安装/修复"))
        },
        "external_sources": external_sources,
        "source_records": source_records,
        "resources": resources,
        "tool_environment": tool_environment,
        "progress": progress
    }))
}

fn setup_user_install_progress(progress: Option<&Value>) -> Value {
    let Some(progress) = progress else {
        return json!({
            "status": "idle",
            "stage": "等待安装",
            "current_resource": "",
            "completed_steps": 0,
            "total_steps": 0,
            "failed_steps": 0,
            "bytes_downloaded": 0,
            "bytes_total": 0,
            "percent": 0,
            "message": "尚未开始安装/修复",
            "updated_at": 0
        });
    };
    let text_field = |name: &str, fallback: &str| {
        progress
            .get(name)
            .and_then(Value::as_str)
            .unwrap_or(fallback)
            .to_string()
    };
    let number_field = |name: &str| {
        progress
            .get(name)
            .and_then(|value| {
                value
                    .as_u64()
                    .or_else(|| value.as_str().and_then(|text| text.parse::<u64>().ok()))
            })
            .unwrap_or(0)
    };
    json!({
        "status": text_field("status", "idle"),
        "stage": text_field("stage", "等待安装"),
        "current_resource": text_field("current_resource", ""),
        "completed_steps": number_field("completed_steps"),
        "total_steps": number_field("total_steps"),
        "failed_steps": number_field("failed_steps"),
        "bytes_downloaded": number_field("bytes_downloaded"),
        "bytes_total": number_field("bytes_total"),
        "percent": number_field("percent").min(100),
        "message": text_field("message", "尚未开始安装/修复"),
        "updated_at": number_field("updated_at")
    })
}

#[cfg(test)]
fn setup_user_source_manifests(source_manifests: Option<&Value>) -> Value {
    let Some(source_manifests) = source_manifests else {
        return json!({});
    };
    let status = source_manifests
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("needs_repair");
    let total = source_manifests
        .get("total")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let ready = source_manifests
        .get("ready")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let integrity_ready = source_manifests
        .get("hash_ready")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let license_ready = source_manifests
        .get("license_ready")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let completed = status == "ready";
    json!({
        "completed": completed,
        "record_directory": "manifests",
        "record_sets": [
            {
                "label": "来源文件记录",
                "ready": ready,
                "total": total,
                "completed": ready >= total && total > 0
            },
            {
                "label": "完整性记录",
                "ready": integrity_ready,
                "total": total,
                "completed": integrity_ready >= total && total > 0
            },
            {
                "label": "许可证记录",
                "ready": license_ready,
                "total": total,
                "completed": license_ready >= total && total > 0
            }
        ],
        "ready": ready,
        "total": total,
        "integrity_ready": integrity_ready,
        "license_ready": license_ready,
        "needs_action": !completed,
        "message": if completed {
            "来源、完整性和许可证记录已就绪"
        } else {
            "来源、完整性或许可证记录需要补齐"
        },
        "next_action": if completed {
            "继续"
        } else {
            "运行安装/修复"
        }
    })
}

#[cfg(test)]
fn setup_user_tool_environment(tool_environment: Option<&Value>) -> Value {
    let Some(tool_environment) = tool_environment else {
        return json!({});
    };
    let status = tool_environment
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("needs_repair");
    let ready = tool_environment
        .get("ready")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let total = tool_environment
        .get("total")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let completed = status == "ready";
    json!({
        "completed": completed,
        "ready": ready,
        "total": total,
        "needs_action": !completed,
        "message": if completed {
            "科学计算环境已就绪"
        } else {
            "科学计算环境需要准备"
        },
        "next_action": if completed {
            "继续"
        } else {
            "运行安装/修复"
        }
    })
}

#[cfg(test)]
fn setup_user_resources(resources: Option<&Value>) -> Value {
    let Some(resources) = resources else {
        return json!({});
    };
    let items = resources
        .get("items")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|item| {
                    let status = item
                        .get("status")
                        .and_then(Value::as_str)
                        .unwrap_or("missing");
                    let next_action = item
                        .get("next_action")
                        .or_else(|| item.get("repair_action"))
                        .and_then(Value::as_str)
                        .unwrap_or("install_or_repair");
                    json!({
                        "label": item
                            .get("label")
                            .and_then(Value::as_str)
                            .unwrap_or("\u{79d1}\u{5b66}\u{8d44}\u{6e90}"),
                        "status": friendly_resource_status(status),
                        "ready": item.get("ready").and_then(Value::as_bool).unwrap_or(status == "ready"),
                        "next_action": friendly_source_next_action(next_action, status)
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let ready = resources
        .get("ready")
        .and_then(Value::as_u64)
        .unwrap_or_else(|| {
            items
                .iter()
                .filter(|item| item.get("ready").and_then(Value::as_bool).unwrap_or(false))
                .count() as u64
        });
    let total = resources
        .get("total")
        .and_then(Value::as_u64)
        .unwrap_or(items.len() as u64);
    let needs_action = resources
        .get("needs_action")
        .and_then(Value::as_bool)
        .unwrap_or(ready < total);
    json!({
        "ready": ready,
        "total": total,
        "needs_action": needs_action,
        "items": items
    })
}

#[cfg(test)]
fn setup_user_external_sources(external_sources: Option<&Value>) -> Value {
    let Some(external_sources) = external_sources else {
        return json!({});
    };
    let items = external_sources
        .get("items")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|item| {
                    let status = item
                        .get("status")
                        .and_then(Value::as_str)
                        .unwrap_or("需要同步");
                    json!({
                        "label": item
                            .get("label")
                            .and_then(Value::as_str)
                            .unwrap_or("科学资源"),
                        "status": status,
                        "ready": item.get("ready").and_then(Value::as_bool).unwrap_or(false),
                        "next_action": item
                            .get("next_action")
                            .and_then(Value::as_str)
                            .unwrap_or("运行安装/修复")
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let ready = external_sources
        .get("ready")
        .and_then(Value::as_u64)
        .unwrap_or_else(|| {
            items
                .iter()
                .filter(|item| item.get("ready").and_then(Value::as_bool).unwrap_or(false))
                .count() as u64
        });
    let total = external_sources
        .get("total")
        .and_then(Value::as_u64)
        .unwrap_or(items.len() as u64);
    let needs_action = external_sources
        .get("needs_action")
        .and_then(Value::as_u64)
        .unwrap_or(total.saturating_sub(ready));
    json!({
        "ready": ready,
        "total": total,
        "needs_action": needs_action,
        "next_action": external_sources.get("next_action").cloned().unwrap_or_else(|| json!("运行安装/修复")),
        "items": items
    })
}

#[cfg(test)]
fn friendly_resource_status(status: &str) -> &'static str {
    match status {
        "ready" => "\u{5c31}\u{7eea}",
        "downloaded_needs_index" => {
            "\u{5df2}\u{4e0b}\u{8f7d}\u{ff0c}\u{9700}\u{91cd}\u{5efa}\u{7d22}\u{5f15}"
        }
        "missing" | "download_required" | "index_required" | "waiting_for_download" => {
            "\u{9700}\u{8981}\u{51c6}\u{5907}"
        }
        _ => "\u{9700}\u{8981}\u{68c0}\u{67e5}",
    }
}

fn update_run_state(options: &ServerOptions, run_id: &str, state: &str) -> Value {
    if let Some(run_dir) = find_run_dir(options, run_id) {
        let _ = write_text(&run_dir.join("run_state.txt"), &(state.to_string() + "\n"));
        json!({"status":state,"run_id":run_id})
    } else {
        json!({"status":"not_found","run_id":run_id})
    }
}

fn delete_project(options: &ServerOptions, project_id: &str) -> Value {
    let dir = project_dir(options, project_id);
    if dir.exists() {
        match fs::remove_dir_all(&dir) {
            Ok(()) => json!({"status":"deleted","project_id":project_id}),
            Err(err) => json!({"status":"failed","error":err.to_string()}),
        }
    } else {
        json!({"status":"not_found","project_id":project_id})
    }
}

fn preflight_store_ready(options: &ServerOptions) -> bool {
    let store = science_store_path(options);
    store.join("science_store_manifest.tsv").exists()
        && store.join("indexes/known_targets.tsv").exists()
        && store.join("indexes/metagenome_targets.tsv").exists()
        && store.join("indexes/structure_targets.tsv").exists()
        && tool_versions_ready(&store.join("indexes/tool_versions.tsv"))
        && install_status_ready(&store.join("install_status.json"))
        && source_requirements_ready(&store.join("source_requirements.tsv"))
        && source_manifests_ready(&store.join("manifests"))
}

fn install_status_ready(path: &Path) -> bool {
    let Ok(text) = fs::read_to_string(path) else {
        return false;
    };
    serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}'))
        .ok()
        .and_then(|value| {
            value
                .get("status")
                .and_then(Value::as_str)
                .map(|status| status == "ready")
        })
        .unwrap_or(false)
}

fn source_requirements_ready(path: &Path) -> bool {
    let Ok(text) = fs::read_to_string(path) else {
        return false;
    };
    let mut lines = text.lines();
    let Some(header) = lines.next() else {
        return false;
    };
    let headers = header.split('\t').collect::<Vec<_>>();
    let Some(status_index) = headers.iter().position(|value| *value == "status") else {
        return false;
    };
    let mut saw_row = false;
    for line in lines.filter(|line| !line.trim().is_empty()) {
        saw_row = true;
        let cols = line.split('\t').collect::<Vec<_>>();
        if cols.get(status_index).copied() != Some("ready") {
            return false;
        }
    }
    saw_row
}

fn source_manifests_ready(dir: &Path) -> bool {
    tsv_status_column_ready(&dir.join("source_file_manifest.tsv"), "status", &["ready"])
        && tsv_status_column_ready(&dir.join("hash_manifest.tsv"), "status", &["ready"])
        && tsv_status_column_ready(&dir.join("license_manifest.tsv"), "status", &["recorded"])
}

fn source_records_status(store: &Path) -> Value {
    let dir = store.join("manifests");
    let source_total = rows_len(&dir.join("source_file_manifest.tsv"));
    let integrity_total = rows_len(&dir.join("hash_manifest.tsv"));
    let license_total = rows_len(&dir.join("license_manifest.tsv"));
    let total = source_total.max(integrity_total).max(license_total);
    let source_ready =
        tsv_ready_row_count(&dir.join("source_file_manifest.tsv"), "status", &["ready"]);
    let integrity_ready = tsv_ready_row_count(&dir.join("hash_manifest.tsv"), "status", &["ready"]);
    let license_ready =
        tsv_ready_row_count(&dir.join("license_manifest.tsv"), "status", &["recorded"]);
    let completed =
        total > 0 && source_ready >= total && integrity_ready >= total && license_ready >= total;
    json!({
        "completed": completed,
        "record_directory": "manifests",
        "ready": source_ready,
        "total": total,
        "integrity_ready": integrity_ready,
        "license_ready": license_ready,
        "needs_action": !completed,
        "record_sets": [
            {
                "label": "来源文件记录",
                "ready": source_ready,
                "total": total,
                "completed": source_ready >= total && total > 0
            },
            {
                "label": "完整性记录",
                "ready": integrity_ready,
                "total": total,
                "completed": integrity_ready >= total && total > 0
            },
            {
                "label": "许可证记录",
                "ready": license_ready,
                "total": total,
                "completed": license_ready >= total && total > 0
            }
        ],
        "message": if completed {
            "来源、完整性和许可证记录已就绪"
        } else {
            "来源、完整性或许可证记录需要补齐"
        },
        "next_action": if completed {
            "继续"
        } else {
            "运行安装/修复"
        }
    })
}

fn tsv_ready_row_count(path: &Path, column_name: &str, ready_values: &[&str]) -> usize {
    let Ok(text) = fs::read_to_string(path) else {
        return 0;
    };
    let mut lines = text.lines();
    let Some(header) = lines.next() else {
        return 0;
    };
    let headers = header.split('\t').collect::<Vec<_>>();
    let Some(status_index) = headers.iter().position(|value| *value == column_name) else {
        return 0;
    };
    lines
        .filter(|line| !line.trim().is_empty())
        .filter(|line| {
            let cols = line.split('\t').collect::<Vec<_>>();
            let status = cols.get(status_index).copied().unwrap_or_default();
            ready_values.contains(&status)
        })
        .count()
}

fn tsv_status_column_ready(path: &Path, column_name: &str, ready_values: &[&str]) -> bool {
    let Ok(text) = fs::read_to_string(path) else {
        return false;
    };
    let mut lines = text.lines();
    let Some(header) = lines.next() else {
        return false;
    };
    let headers = header.split('\t').collect::<Vec<_>>();
    let Some(status_index) = headers.iter().position(|value| *value == column_name) else {
        return false;
    };
    let mut saw_row = false;
    for line in lines.filter(|line| !line.trim().is_empty()) {
        saw_row = true;
        let cols = line.split('\t').collect::<Vec<_>>();
        let status = cols.get(status_index).copied().unwrap_or_default();
        if !ready_values.contains(&status) {
            return false;
        }
    }
    saw_row
}

fn required_resource_rows(store: &Path) -> Vec<Value> {
    let required = [
        (
            "science_store",
            "science_store_manifest.tsv",
            "科学仓库",
            "运行安装/修复以生成科学仓库记录",
        ),
        (
            "known_database",
            "indexes/known_targets.tsv",
            "已知蛋白索引",
            "运行安装/修复以准备已知蛋白索引",
        ),
        (
            "metagenome",
            "indexes/metagenome_targets.tsv",
            "宏基因组索引",
            "运行安装/修复以准备宏基因组索引",
        ),
        (
            "structure_index",
            "indexes/structure_targets.tsv",
            "结构索引",
            "运行安装/修复以准备结构索引",
        ),
        (
            "tools",
            "indexes/tool_versions.tsv",
            "计算工具",
            "运行安装/修复以准备计算工具记录",
        ),
        (
            "install_status",
            "install_status.json",
            "安装状态",
            "运行安装/修复以完成安装状态检查",
        ),
        (
            "source_requirements",
            "source_requirements.tsv",
            "科学资源状态",
            "运行安装/修复以完成科学资源检查",
        ),
        (
            "source_records",
            "manifests/source_file_manifest.tsv",
            "来源与许可记录",
            "运行安装/修复以补齐来源、完整性和许可记录",
        ),
    ];
    required
        .iter()
        .map(|(id, rel, label, next_action)| {
            let path = store.join(rel);
            let ready = match *id {
                "tools" => tool_versions_ready(&path),
                "install_status" => install_status_ready(&path),
                "source_requirements" => source_requirements_ready(&path),
                "source_records" => source_manifests_ready(&store.join("manifests")),
                _ => path.exists(),
            };
            json!({
                "id": id,
                "label": label,
                "ready": ready,
                "status": if ready {"就绪"} else {"需要准备"},
                "next_action": if ready {"继续"} else {*next_action}
            })
        })
        .collect()
}

fn missing_resource_rows(rows: &[Value]) -> Vec<Value> {
    rows.iter()
        .filter(|row| !row.get("ready").and_then(Value::as_bool).unwrap_or(false))
        .map(|row| {
            json!({
                "id": row.get("id").cloned().unwrap_or_else(|| json!("resource")),
                "label": row.get("label").cloned().unwrap_or_else(|| json!("科学资源")),
                "next_action": row.get("next_action").cloned().unwrap_or_else(|| json!("运行安装/修复"))
            })
        })
        .collect()
}

fn tool_versions_ready(path: &Path) -> bool {
    let Ok(text) = fs::read_to_string(path) else {
        return false;
    };
    REQUIRED_TOOL_IDS.iter().all(|tool| {
        text.lines().any(|line| {
            let cols = line.split('\t').collect::<Vec<_>>();
            cols.first()
                .is_some_and(|name| name.eq_ignore_ascii_case(tool))
                && cols.get(1).copied() == Some("ready")
        })
    })
}

fn openmm_platform_status(store: &Path) -> Value {
    let path = store.join("indexes/tool_versions.tsv");
    let openmm = tool_version_row(&path, "openmm");
    let cuda = tool_version_row(&path, "openmm_cuda");
    let openmm_ready = openmm
        .as_ref()
        .and_then(|row| row.get("status"))
        .is_some_and(|status| status == "ready");
    let version = openmm
        .as_ref()
        .and_then(|row| row.get("version"))
        .cloned()
        .unwrap_or_else(|| "missing".to_string());
    let detail = cuda
        .as_ref()
        .and_then(|row| row.get("detail"))
        .cloned()
        .or_else(|| openmm.as_ref().and_then(|row| row.get("detail")).cloned())
        .unwrap_or_default();
    let platforms = detail_value(&detail, "platforms").unwrap_or_default();
    let cuda_toolkit_path = detail_value(&detail, "cuda_toolkit_path").unwrap_or_default();
    let cuda_path_ready =
        detail_value(&detail, "cuda_path_ready").is_some_and(|value| value == "true");
    let cuda_failure_reason = detail_value(&detail, "cuda_failure_reason").unwrap_or_default();
    let openmm_plugin_dir = detail_value(&detail, "openmm_plugin_dir").unwrap_or_default();
    let openmm_cuda_package = detail_value(&detail, "openmm_cuda_package").unwrap_or_default();
    let wsl_gpu = detail_value(&detail, "wsl_gpu").unwrap_or_default();
    let wsl_libcuda = detail_value(&detail, "wsl_libcuda").unwrap_or_default();
    let cuda_ready = cuda
        .as_ref()
        .and_then(|row| row.get("status"))
        .is_some_and(|status| status == "ready")
        || detail_value(&detail, "cuda").is_some_and(|value| value == "ready");
    let summary = if !openmm_ready {
        "OpenMM 未就绪".to_string()
    } else if platforms.is_empty() {
        format!("OpenMM {version} 已就绪；CUDA 平台状态等待安装器刷新")
    } else if cuda_ready {
        format!("OpenMM {version} 平台：{platforms}；CUDA 可用")
    } else if !cuda_toolkit_path.is_empty() && !cuda_failure_reason.is_empty() {
        format!(
            "OpenMM {version} 平台：{platforms}；CUDA 未检测到；Toolkit：{cuda_toolkit_path}；原因：{cuda_failure_reason}"
        )
    } else if !cuda_toolkit_path.is_empty() {
        format!("OpenMM {version} 平台：{platforms}；CUDA 未检测到；Toolkit：{cuda_toolkit_path}")
    } else {
        format!("OpenMM {version} 平台：{platforms}；CUDA 未检测到")
    };
    json!({
        "id": "openmm_cuda",
        "label": "OpenMM/CUDA 平台",
        "openmm_ready": openmm_ready,
        "cuda_ready": cuda_ready,
        "cuda_toolkit_path": cuda_toolkit_path,
        "cuda_path_ready": cuda_path_ready,
        "cuda_reason": cuda_failure_reason,
        "openmm_plugin_dir": openmm_plugin_dir,
        "openmm_cuda_package": openmm_cuda_package,
        "wsl_gpu": wsl_gpu,
        "wsl_libcuda": wsl_libcuda,
        "platforms": platforms,
        "summary": summary,
        "next_action": if openmm_ready {"继续"} else {"运行安装/修复"}
    })
}

fn tool_version_row(path: &Path, id: &str) -> Option<BTreeMap<String, String>> {
    let text = fs::read_to_string(path).ok()?;
    let mut lines = text.lines().filter(|line| !line.trim().is_empty());
    let headers = lines.next()?.split('\t').collect::<Vec<_>>();
    for line in lines {
        let cols = line.split('\t').collect::<Vec<_>>();
        if cols
            .first()
            .is_some_and(|tool| tool.eq_ignore_ascii_case(id))
        {
            let mut row = BTreeMap::new();
            for (index, header) in headers.iter().enumerate() {
                row.insert(
                    (*header).to_string(),
                    cols.get(index).copied().unwrap_or_default().to_string(),
                );
            }
            return Some(row);
        }
    }
    None
}

fn detail_value(detail: &str, key: &str) -> Option<String> {
    detail.split(';').find_map(|part| {
        let (left, right) = part.split_once('=')?;
        if left.trim() == key {
            Some(right.trim().to_string())
        } else {
            None
        }
    })
}

fn source_status_rows(store: &Path) -> Vec<Value> {
    let path = store.join("source_requirements.tsv");
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut lines = text.lines();
    let Some(header) = lines.next() else {
        return Vec::new();
    };
    let headers = header.split('\t').collect::<Vec<_>>();
    lines
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let cols = line.split('\t').collect::<Vec<_>>();
            let get = |name: &str| -> String {
                headers
                    .iter()
                    .position(|header| *header == name)
                    .and_then(|index| cols.get(index).copied())
                    .unwrap_or("")
                    .to_string()
            };
            let raw_status = get("status");
            let raw_action = get("repair_action");
            json!({
                "source_id": get("source_id"),
                "label": get("label"),
                "category": friendly_source_category(&get("category")),
                "status": friendly_source_status(&raw_status),
                "ready": raw_status == "ready",
                "next_action": friendly_source_next_action(&raw_action, &raw_status)
            })
        })
        .collect()
}

fn friendly_source_category(raw: &str) -> &'static str {
    match raw {
        "known_activity" => "已知活性",
        "known_protein" => "已知蛋白",
        "structure" => "结构数据",
        "structure_index" | "structure_interaction" => "结构索引",
        "metagenome" => "宏基因组序列",
        _ => "科学资源",
    }
}

fn friendly_source_status(raw: &str) -> &'static str {
    match raw {
        "ready" => "就绪",
        "downloaded_needs_index" => "已下载，需重建索引",
        "missing" => "需要准备",
        "download_required" => "需要下载",
        "index_required" => "需要重建索引",
        "waiting_for_download" => "等待下载",
        _ => "需要检查",
    }
}

fn friendly_source_next_action(action: &str, status: &str) -> &'static str {
    if status == "ready" {
        return "继续";
    }
    match action {
        "download_and_index_chembl" => "运行安装/修复以准备 ChEMBL 活性数据",
        "download_and_index_bindingdb" => "运行安装/修复以准备 BindingDB 活性数据",
        "download_and_index_uniprot_human" => "运行安装/修复以准备 UniProt 人类蛋白索引",
        "download_and_index_rcsb_ligand_bound" => "运行安装/修复以准备 RCSB 结构数据",
        "download_and_index_biolip2" => "运行安装/修复以准备 BioLiP2 互作数据",
        "download_and_index_plinder" => "运行安装/修复以准备 PLINDER 结构数据",
        "download_and_index_binding_moad" => "运行安装/修复以准备 Binding MOAD 结构数据",
        "download_and_index_metagenome_sources" => "运行安装/修复以准备宏基因组序列索引",
        "install_or_repair_scientific_tools" => "运行安装/修复以准备计算工具",
        "build_known_database_index" => "运行安装/修复以重建已知蛋白索引",
        "build_metagenome_potential_index" => "运行安装/修复以重建宏基因组索引",
        _ if status == "downloaded_needs_index" || status == "index_required" => {
            "运行安装/修复以重建索引"
        }
        _ => "运行安装/修复以准备资源",
    }
}

fn check(id: &str, ready: bool, _detail: String) -> Value {
    json!({"id":id,"ready":ready,"status":if ready {"ready"} else {"blocked"}})
}

fn capture(program: &str, args: &[&str], cwd: &Path) -> (bool, String) {
    match Command::new(program).args(args).current_dir(cwd).output() {
        Ok(output) => (
            output.status.success(),
            format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ),
        ),
        Err(err) => (false, err.to_string()),
    }
}

fn capture_command(command: &[String], cwd: &Path) -> (bool, String) {
    let Some(program) = command.first() else {
        return (false, "empty_command".to_string());
    };
    match Command::new(program)
        .args(&command[1..])
        .current_dir(cwd)
        .output()
    {
        Ok(output) => (
            output.status.success(),
            format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ),
        ),
        Err(err) => (false, err.to_string()),
    }
}

fn read_request(stream: &mut TcpStream) -> Result<String, String> {
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
        let text = String::from_utf8_lossy(&bytes).to_string();
        if let Some(header_end) = text.find("\r\n\r\n") {
            let headers = parse_headers(&text);
            let length = headers
                .get("content-length")
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(0);
            if bytes.len() >= header_end + 4 + length {
                return Ok(text);
            }
        }
        if bytes.len() > 2_000_000 {
            return Err("request_too_large".to_string());
        }
    }
    Ok(String::from_utf8_lossy(&bytes).to_string())
}

fn parse_headers(request: &str) -> BTreeMap<String, String> {
    request
        .lines()
        .skip(1)
        .take_while(|line| !line.trim().is_empty())
        .filter_map(|line| {
            line.split_once(':')
                .map(|(key, value)| (key.trim().to_ascii_lowercase(), value.trim().to_string()))
        })
        .collect()
}

fn write_response(
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

fn json_response(status: u16, value: Value) -> (u16, &'static str, String) {
    (status, "application/json; charset=utf-8", value.to_string())
}

fn projects_root(options: &ServerOptions) -> PathBuf {
    options.artifact_dir.join("projects")
}

fn project_dir(options: &ServerOptions, project_id: &str) -> PathBuf {
    projects_root(options).join(safe_id(project_id))
}

fn find_run_dir(options: &ServerOptions, run_id: &str) -> Option<PathBuf> {
    for entry in fs::read_dir(projects_root(options)).ok()?.flatten() {
        let path = entry.path().join("runs").join(run_id);
        if path.exists() {
            return Some(path);
        }
    }
    None
}

fn run_state(run_dir: &Path) -> String {
    read_trimmed(&run_dir.join("run_state.txt")).unwrap_or_else(|| "created".to_string())
}

fn run_trace(options: &ServerOptions, run_dir: &Path, run_id: &str) -> Value {
    let project = run_dir
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf);
    let project_name = project
        .as_ref()
        .and_then(|dir| read_trimmed(&dir.join("project_name.txt")))
        .unwrap_or_else(|| "未命名研究项目".to_string());
    let input_ready = project
        .as_ref()
        .map(|dir| input_has_user_value(&dir.join("input_manifest.json")))
        .unwrap_or(false);
    let artifact_items = artifact_index(run_dir);
    let output_count = artifact_items["items"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter(|item| item.get("exists").and_then(Value::as_bool).unwrap_or(false))
                .count()
        })
        .unwrap_or(0);
    let state = run_state(run_dir);
    let completed = state == "completed";
    let candidate_rows = if completed {
        rows_len(&run_dir.join("target_candidate_ranking.tsv"))
    } else {
        0
    };
    let evidence_rows = if completed {
        rows_len(&run_dir.join("evidence_ledger.tsv"))
    } else {
        0
    };
    json!({
        "project_label": project_name,
        "run_label": if run_id.is_empty() { "未选择运行" } else { "当前运行" },
        "input_status": if input_ready { "已保存用户输入" } else { "未保存用户输入" },
        "database_status": if preflight_store_ready(options) { "科学仓库就绪" } else { "科学仓库未就绪" },
        "output_status": if completed && output_count > 0 { "已生成输出" } else if state == "blocked" { "已记录阻塞信息" } else { "暂无输出" },
        "output_count": output_count,
        "candidate_status": if candidate_rows > 0 { "已生成候选排序" } else { "暂无候选排序" },
        "candidate_count": candidate_rows,
        "evidence_status": if evidence_rows > 0 { "已生成证据链" } else { "暂无证据链" },
        "evidence_count": evidence_rows
    })
}

fn input_project_name(value: &Value) -> Option<String> {
    value
        .get("inputs")
        .and_then(Value::as_array)?
        .iter()
        .find(|item| item.get("field").and_then(Value::as_str) == Some("project_name"))
        .and_then(|item| item.get("value").and_then(Value::as_str))
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(ToOwned::to_owned)
}

fn input_has_user_value(path: &Path) -> bool {
    let value = read_json_or(path, json!({}));
    input_value_has_usable_user_input(&value)
}

fn input_value_has_usable_user_input(value: &Value) -> bool {
    let Some(inputs) = value.get("inputs").and_then(Value::as_array) else {
        return false;
    };
    let mut saw_usable_input = false;
    for item in inputs {
        let field = item.get("field").and_then(Value::as_str).unwrap_or("");
        let input_value = item.get("value").and_then(Value::as_str).unwrap_or("");
        if field == "project_name" || input_value.trim().is_empty() {
            continue;
        }
        if !input_field_is_usable(field, input_value) {
            return false;
        }
        saw_usable_input = true;
    }
    saw_usable_input
}

fn input_field_is_usable(field: &str, value: &str) -> bool {
    let value = value.trim();
    if value.is_empty() || field == "project_name" {
        return false;
    }
    let field = field.to_ascii_lowercase();
    match field.as_str() {
        "small_molecule" => {
            looks_like_path_with_ext(value, &["sdf", "mol2", "mol", "pdb", "cif", "mmcif"])
                || count_alphanumeric(value) >= 1
        }
        "peptide" => {
            looks_like_path_with_ext(value, &["fasta", "fa", "pdb", "cif", "mmcif"])
                || looks_like_peptide_sequence(value, 2)
        }
        "protein_ligand" => {
            looks_like_path_with_ext(value, &["fasta", "fa", "faa", "pdb", "cif", "mmcif"])
                || looks_like_peptide_sequence(value, 5)
        }
        "complex" => {
            looks_like_path_with_ext(value, &["pdb", "cif", "mmcif", "ent"])
                || count_alphanumeric(value) >= 5
        }
        "genome_or_metagenome" => {
            looks_like_path_with_ext(
                value,
                &[
                    "fasta", "fa", "fna", "faa", "fastq", "fq", "gb", "gff", "gtf", "vcf",
                ],
            ) || looks_like_nucleotide_sequence(value, 8)
        }
        _ => count_alphanumeric(value) >= 2,
    }
}

fn looks_like_path_with_ext(value: &str, extensions: &[&str]) -> bool {
    let value = value.trim().to_ascii_lowercase();
    extensions
        .iter()
        .any(|ext| value.ends_with(&format!(".{ext}")) || value.contains(&format!(".{ext}\n")))
}

fn compact_sequence_body(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.starts_with('>') {
        return trimmed.lines().skip(1).collect::<String>();
    }
    trimmed.chars().filter(|ch| !ch.is_whitespace()).collect()
}

fn looks_like_peptide_sequence(value: &str, min_len: usize) -> bool {
    let body = compact_sequence_body(value);
    body.len() >= min_len
        && body.chars().all(|ch| {
            matches!(
                ch.to_ascii_uppercase(),
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

fn looks_like_nucleotide_sequence(value: &str, min_len: usize) -> bool {
    let body = compact_sequence_body(value);
    body.len() >= min_len
        && body.chars().all(|ch| {
            matches!(
                ch.to_ascii_uppercase(),
                'A' | 'C'
                    | 'G'
                    | 'T'
                    | 'U'
                    | 'N'
                    | 'R'
                    | 'Y'
                    | 'S'
                    | 'W'
                    | 'K'
                    | 'M'
                    | 'B'
                    | 'D'
                    | 'H'
                    | 'V'
            )
        })
}

fn count_alphanumeric(value: &str) -> usize {
    value.chars().filter(|ch| ch.is_alphanumeric()).count()
}

fn read_json_or(path: &Path, fallback: Value) -> Value {
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}')).ok())
        .unwrap_or(fallback)
}

fn artifact_index(run_dir: &Path) -> Value {
    let mut items = Vec::new();
    let completed = run_state(run_dir) == "completed";
    for (id, label, file) in artifact_specs() {
        let path = run_dir.join(file);
        items.push(json!({"id":id,"label":label,"exists":artifact_visible_for_user(id, &path, run_dir, completed)}));
    }
    json!({"items":items})
}

fn visible_export_count(run_dir: &Path) -> usize {
    artifact_index(run_dir)["items"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter(|item| item.get("exists").and_then(Value::as_bool).unwrap_or(false))
                .count()
        })
        .unwrap_or(0)
}

fn artifact_visible_for_user(id: &str, path: &Path, run_dir: &Path, completed: bool) -> bool {
    if !path.exists() {
        return false;
    }
    match id {
        "candidate_ranking" => {
            completed && rows_len(&run_dir.join("target_candidate_ranking.tsv")) > 0
        }
        "evidence" | "structure_patch" | "md_summary" => completed && rows_len(path) > 0,
        "research_report" => {
            completed
                && rows_len(&run_dir.join("target_candidate_ranking.tsv")) > 0
                && rows_len(&run_dir.join("evidence_ledger.tsv")) > 0
        }
        _ => completed,
    }
}

fn artifact_specs() -> &'static [(&'static str, &'static str, &'static str)] {
    &[
        (
            "structure_patch",
            "\u{7ed3}\u{6784}\u{4e0e}\u{53e3}\u{888b}\u{6458}\u{8981}",
            "structure_patch_summary.tsv",
        ),
        (
            "md_summary",
            "\u{52a8}\u{529b}\u{5b66}\u{6458}\u{8981}",
            "md_summary.tsv",
        ),
        (
            "candidate_ranking",
            "\u{5019}\u{9009}\u{6392}\u{5e8f}",
            "target_candidate_ranking.tsv",
        ),
        (
            "evidence",
            "\u{8bc1}\u{636e}\u{94fe}",
            "evidence_ledger.tsv",
        ),
        (
            "research_report",
            "\u{79d1}\u{7814}\u{62a5}\u{544a}",
            "research_report.md",
        ),
    ]
}

fn friendly_failure_summary(raw: Option<&str>) -> String {
    let Some(raw) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return String::new();
    };
    raw.split([',', '\n'])
        .filter_map(|item| {
            let value = item.trim();
            if value.is_empty() {
                None
            } else {
                Some(match value {
                    "missing_input_manifest" | "missing_input" => "缺少输入记录".to_string(),
                    "missing_science_store_manifest" => "缺少科学仓库".to_string(),
                    "missing_known_database_index" => "缺少已知蛋白索引".to_string(),
                    "missing_metagenome_potential_index" => "缺少宏基因组索引".to_string(),
                    "missing_structure_index" | "missing_structure_interaction_index" => {
                        "缺少结构索引".to_string()
                    }
                    "missing_tool_versions_manifest" => "计算工具未就绪".to_string(),
                    _ if value.starts_with("missing_or_unready_tool:") => {
                        "计算工具未就绪".to_string()
                    }
                    _ => "运行条件未满足".to_string(),
                })
            }
        })
        .collect::<Vec<_>>()
        .join("；")
}

fn user_run_events(run_dir: &Path, run_id: &str) -> Vec<String> {
    let state = run_state(run_dir);
    let mut events = vec![
        format!(
            "运行记录：{}",
            if run_id.is_empty() {
                "未创建"
            } else {
                "已创建"
            }
        ),
        format!("当前状态：{}", run_state_user_label(&state)),
    ];
    let stage = read_trimmed(&run_dir.join("current_stage.txt"))
        .map(|value| stage_user_label(&value).to_string())
        .unwrap_or_else(|| "等待开始".to_string());
    events.push(format!("当前阶段：{stage}"));
    let failure =
        friendly_failure_summary(read_trimmed(&run_dir.join("failure_reason.txt")).as_deref());
    if !failure.is_empty() {
        events.push(format!("处理提示：{failure}"));
    }
    let generated = artifact_index(run_dir)["items"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter(|item| item.get("exists").and_then(Value::as_bool).unwrap_or(false))
                .count()
        })
        .unwrap_or(0);
    events.push(format!("已生成输出：{generated} 项"));
    events
}

fn run_state_user_label(state: &str) -> &'static str {
    match state {
        "created" => "已创建",
        "running" => "运行中",
        "completed" => "已完成",
        "blocked" => "已阻塞",
        "failed" => "失败",
        "cancelled" => "已取消",
        "resumable" => "可恢复",
        "not_found" => "未找到",
        _ => "未开始",
    }
}

fn stage_user_label(stage: &str) -> &'static str {
    match stage {
        "pipeline_run" => "完整流程",
        "input_normalize" => "输入归一化",
        "features_compute" => "特征计算",
        "database_match_known" => "已知数据库比对",
        "metagenome_match_potential" => "宏基因组潜在序列比对",
        "structure_evaluate" => "结构与口袋评估",
        "dynamics_screen" => "短动力学筛选",
        "target_rank" => "候选排序",
        "evidence_build" => "证据链整理",
        "report_render" => "报告生成",
        _ => "运行准备",
    }
}

fn read_tsv_rows(path: &Path) -> Vec<Value> {
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut lines = text.lines().filter(|line| !line.trim().is_empty());
    let Some(header) = lines.next() else {
        return Vec::new();
    };
    let headers = header
        .split('\t')
        .map(|value| value.trim().to_string())
        .collect::<Vec<_>>();
    lines
        .take(500)
        .map(|line| {
            let cols = line.split('\t').collect::<Vec<_>>();
            let mut row = serde_json::Map::new();
            for (index, header) in headers.iter().enumerate() {
                row.insert(
                    header.clone(),
                    Value::String(cols.get(index).copied().unwrap_or("").to_string()),
                );
            }
            Value::Object(row)
        })
        .collect()
}

fn rows_len(path: &Path) -> usize {
    fs::read_to_string(path)
        .map(|text| {
            text.lines()
                .filter(|line| !line.trim().is_empty())
                .skip(1)
                .count()
        })
        .unwrap_or(0)
}

fn write_text(path: &Path, text: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("create_parent_failed:{}:{err}", parent.display()))?;
    }
    fs::write(path, text).map_err(|err| format!("write_failed:{}:{err}", path.display()))
}

fn read_trimmed(path: &Path) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
}

fn safe_id(raw: &str) -> String {
    let id = raw
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '-' || *ch == '_')
        .collect::<String>();
    if id.is_empty() {
        "unknown".to_string()
    } else {
        id
    }
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server_options(workspace_root: PathBuf) -> ServerOptions {
        ServerOptions {
            host: "127.0.0.1".to_string(),
            port: 0,
            artifact_dir: workspace_root.join(".genoligand-test-runtime"),
            workspace_root,
        }
    }

    #[test]
    fn gls_cli_invocation_prefers_runtime_bin_over_cargo_fallback() {
        let root = std::env::temp_dir().join(format!("genoligand-runtime-bin-{}", now_millis()));
        fs::create_dir_all(&root).expect("create root");
        fs::write(root.join("Cargo.toml"), "[workspace]\n").expect("cargo marker");
        let options = server_options(root.clone());
        let bin_dir = options.artifact_dir.join("bin");
        fs::create_dir_all(&bin_dir).expect("runtime bin");
        let exe = if cfg!(windows) {
            "gls-cli.exe"
        } else {
            "gls-cli"
        };
        let runtime_cli = bin_dir.join(exe);
        fs::write(&runtime_cli, "runtime cli").expect("runtime cli");

        let command = gls_cli_invocation(&options);
        let joined = command.join(" ");

        assert_eq!(command, vec![runtime_cli.display().to_string()]);
        assert!(!joined.contains("cargo"));
        fs::remove_dir_all(&root).ok();
    }

    fn write_ready_tool_versions(store: &Path) {
        let indexes = store.join("indexes");
        fs::create_dir_all(&indexes).expect("indexes");
        let mut text = String::from("tool\tstatus\tversion\tdetail\n");
        for tool in REQUIRED_TOOL_IDS {
            text.push_str(&format!("{tool}\tready\ttest\tok\n"));
        }
        fs::write(indexes.join("tool_versions.tsv"), text).expect("tool versions");
    }

    fn write_launch_environment(options: &ServerOptions, python: &str, cli: &str) {
        fs::create_dir_all(&options.artifact_dir).expect("runtime");
        let payload = json!({
            "status": "ready",
            "environment": {
                "GENOLIGAND_SCIENCE_STORE": options.workspace_root.join("science_store").display().to_string(),
                "GENOLIGAND_GLS_CLI": cli,
                "GENOLIGAND_PYTHON": python
            },
            "path_prepend": [options.artifact_dir.join("bin").display().to_string()]
        });
        fs::write(
            options.artifact_dir.join("launch_environment.json"),
            payload.to_string(),
        )
        .expect("launch environment");
    }

    #[test]
    fn worker_environment_uses_installed_launch_environment() {
        let root = std::env::temp_dir().join(format!("genoligand-worker-env-{}", now_millis()));
        let options = server_options(root.clone());
        let managed_python =
            options
                .artifact_dir
                .join("python-worker-env")
                .join(if cfg!(windows) {
                    "genoligand-wsl-python.cmd"
                } else {
                    "bin/python"
                });
        fs::create_dir_all(managed_python.parent().expect("managed python parent"))
            .expect("managed python dir");
        if cfg!(windows) {
            fs::write(&managed_python, "@echo off\r\nexit /b 0\r\n").expect("fake wrapper");
        } else {
            fs::write(&managed_python, "#!/usr/bin/env sh\nexit 0\n").expect("fake wrapper");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut permissions = fs::metadata(&managed_python)
                    .expect("metadata")
                    .permissions();
                permissions.set_mode(0o755);
                fs::set_permissions(&managed_python, permissions).expect("chmod");
            }
        }
        write_ready_tool_versions(&root.join("science_store"));
        write_launch_environment(
            &options,
            &managed_python.display().to_string(),
            &options
                .artifact_dir
                .join("bin")
                .join("gls-cli.exe")
                .display()
                .to_string(),
        );

        let status = worker_environment_status(&options);
        let env = runtime_command_environment(&options);

        assert_eq!(status["ready"], true);
        assert_eq!(
            env.get("GENOLIGAND_PYTHON").cloned(),
            Some(managed_python.display().to_string())
        );
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn python_probe_script_arg_uses_wsl_path_for_wsl_wrapper() {
        let script = PathBuf::from(r"D:\Project\rust\GenoLigand\work\probe.py");

        assert_eq!(
            python_probe_script_arg(
                r"D:\runtime\python-worker-env\genoligand-wsl-python.cmd",
                &script
            ),
            "/mnt/d/Project/rust/GenoLigand/work/probe.py"
        );
        assert_eq!(
            python_probe_script_arg(r"D:\runtime\python-worker-env\python.cmd", &script),
            r"D:\Project\rust\GenoLigand\work\probe.py"
        );
    }

    #[test]
    fn start_run_blocks_system_python_worker_environment() {
        let root = std::env::temp_dir().join(format!("genoligand-system-python-{}", now_millis()));
        let options = server_options(root.clone());
        let project_id = "project-1";
        let project = project_dir(&options, project_id);
        fs::create_dir_all(&project).expect("project");
        fs::write(
            project.join("input_manifest.json"),
            r#"{"inputs":[{"value":"CCO"}]}"#,
        )
        .expect("input");
        write_ready_tool_versions(&root.join("science_store"));
        write_launch_environment(&options, "python", "gls-cli");

        let response = start_run(&options, project_id);
        let run_id = response["run"]["run_id"].as_str().expect("run id");
        let run_dir = project.join("runs").join(run_id);

        assert_eq!(response["status"], "blocked");
        assert_eq!(
            fs::read_to_string(run_dir.join("run_state.txt")).expect("run state"),
            "blocked\n"
        );
        assert!(
            fs::read_to_string(run_dir.join("failure_reason.txt"))
                .expect("failure")
                .contains("worker")
        );
        fs::remove_dir_all(&root).ok();
    }

    fn write_ready_source_records(store: &Path) {
        let dir = store.join("manifests");
        fs::create_dir_all(&dir).expect("source records dir");
        fs::write(
            dir.join("source_file_manifest.tsv"),
            "source_id\tstatus\nchembl\tready\n",
        )
        .expect("source file records");
        fs::write(
            dir.join("hash_manifest.tsv"),
            "source_id\tstatus\nchembl\tready\n",
        )
        .expect("integrity records");
        fs::write(
            dir.join("license_manifest.tsv"),
            "source_id\tstatus\nchembl\trecorded\n",
        )
        .expect("license records");
    }

    #[test]
    fn setup_status_exposes_user_readable_source_record_sets() {
        let root =
            std::env::temp_dir().join(format!("genoligand-source-record-status-{}", now_millis()));
        let options = server_options(root.clone());
        let store = root.join("science_store");
        write_ready_source_records(&store);

        let records = source_records_status(&store);
        let status = user_setup_status_json(&options);

        assert!(status.get("source_records").is_some());
        assert_eq!(records["completed"], true);
        assert_eq!(records["record_directory"], "manifests");
        assert_eq!(records["record_sets"][0]["label"], "来源文件记录");
        assert_eq!(records["record_sets"][1]["label"], "完整性记录");
        assert_eq!(records["record_sets"][2]["label"], "许可证记录");
        let text = records.to_string();
        assert!(!text.contains("source_file_manifest"));
        assert!(!text.contains("hash_manifest"));
        assert!(!text.contains("license_manifest"));
        assert!(!text.contains("sha256"));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn setup_repair_uses_complete_installer_when_available() {
        let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root")
            .to_path_buf();
        let command = setup_repair_command(&server_options(workspace_root));
        let joined = command.join(" ");

        assert!(joined.contains("install_complete_scientific.py"));
        assert!(joined.contains("--workspace-root"));
        assert!(joined.contains("--science-store"));
        assert!(!joined.contains(" prepare-science-store "));
    }

    #[test]
    fn setup_repair_falls_back_to_cli_without_installer_script() {
        let workspace_root =
            std::env::temp_dir().join(format!("genoligand-no-installer-{}", now_millis()));
        fs::create_dir_all(&workspace_root).expect("create temp workspace");
        let command = setup_repair_command(&server_options(workspace_root.clone()));
        let joined = command.join(" ");
        fs::remove_dir_all(&workspace_root).ok();

        assert!(joined.contains("install prepare-science-store"));
        assert!(joined.contains("--workspace-root"));
        assert!(joined.contains("--science-store"));
    }

    #[test]
    fn setup_installer_summary_keeps_user_relevant_fields_only() {
        let payload = setup_installer_summary(
            r#"{"status":"needs_repair","runtime_backend":"WSL2","summary":"安装/修复已执行，但仍有资源需要准备。","next_action":"补齐缺失资源后重新运行安装/修复","missing_components_count":3,"debug":{"command":["cargo","run"],"stdout":"raw"},"downloads":{"status":"not_configured","ready":0,"failed":0,"message":"安装包未包含科学资源下载清单，科学仓库尚未准备完成。","next_action":"使用包含科学资源清单的完整安装包重新运行安装/修复"}}"#,
        )
        .expect("installer json");

        assert_eq!(payload["next_action"], "补齐缺失资源后重新运行安装/修复");
        assert_eq!(payload["downloads"]["completed"], false);
        assert_eq!(payload["downloads"]["ready"], 0);
        assert_eq!(
            payload["downloads"]["message"],
            "安装包未包含科学资源下载清单，科学仓库尚未准备完成。"
        );
        assert_eq!(
            payload["downloads"]["next_action"],
            "使用包含科学资源清单的完整安装包重新运行安装/修复"
        );
        assert!(payload.get("resources").is_some());
        assert!(payload.get("debug").is_none());
        assert!(payload.get("command").is_none());
        assert!(payload.get("stdout").is_none());
        let text = payload.to_string();
        assert!(!text.contains("needs_repair"));
        assert!(!text.contains("runtime_backend"));
        assert!(!text.contains("missing_components_count"));
        assert!(!text.contains("not_configured"));
    }

    #[test]
    fn setup_installer_progress_hides_paths_and_internal_fields() {
        let payload = setup_installer_summary(
            r#"{"status":"needs_repair","progress":{"status":"running","stage":"下载数据库","current_resource":"ChEMBL","completed_steps":2,"total_steps":8,"failed_steps":0,"bytes_downloaded":1048576,"bytes_total":2097152,"percent":50,"message":"正在下载数据库资源","updated_at":123,"path":"D:/hidden/store/download.part","sha256":"abc123","manifest":"download_execution_manifest.tsv"}}"#,
        )
        .expect("installer json");

        assert_eq!(payload["progress"]["status"], "running");
        assert_eq!(payload["progress"]["stage"], "下载数据库");
        assert_eq!(payload["progress"]["current_resource"], "ChEMBL");
        assert_eq!(payload["progress"]["percent"], 50);
        assert_eq!(payload["progress"]["bytes_downloaded"], 1048576);
        assert_eq!(payload["progress"]["bytes_total"], 2097152);
        let text = payload.to_string();
        assert!(!text.contains("D:/hidden"));
        assert!(!text.contains("sha256"));
        assert!(!text.contains("abc123"));
        assert!(!text.contains("download_execution_manifest"));
    }

    #[test]
    fn source_record_gate_requires_all_source_integrity_and_license_rows() {
        let root = std::env::temp_dir().join(format!("genoligand-source-records-{}", now_millis()));
        let store = root.join("science_store");
        write_ready_source_records(&store);
        assert!(source_manifests_ready(&store.join("manifests")));

        fs::write(
            store.join("manifests/license_manifest.tsv"),
            "source_id\tstatus\nchembl\tmissing\n",
        )
        .expect("license records");
        assert!(!source_manifests_ready(&store.join("manifests")));

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn setup_installer_resource_summary_hides_internal_fields() {
        let payload = setup_installer_summary(
            r#"{"status":"needs_repair","missing_components_count":1,"downloads":{"status":"ready","ready":0,"failed":0},"resources":{"total":1,"ready":0,"needs_action":true,"items":[{"label":"ChEMBL","status":"missing","next_action":"download_and_index_chembl","path":"D:/hidden/science_store_manifest.tsv","sha256":"abc123","manifest":"source_requirements.tsv"}]}}"#,
        )
        .expect("installer json");

        assert_eq!(payload["resources"]["total"], 1);
        assert_eq!(payload["resources"]["items"][0]["label"], "ChEMBL");
        assert_eq!(payload["resources"]["items"][0]["status"], "需要准备");
        assert_eq!(
            payload["resources"]["items"][0]["next_action"],
            "运行安装/修复以准备 ChEMBL 活性数据"
        );
        let text = payload.to_string();
        assert!(!text.contains("path"));
        assert!(!text.contains("sha256"));
        assert!(!text.contains("manifest"));
        assert!(!text.contains("D:/hidden"));
        assert!(!text.contains("abc123"));
        assert!(!text.contains("source_requirements"));
    }

    #[test]
    fn setup_installer_download_summary_treats_skipped_as_incomplete() {
        let payload = setup_installer_summary(
            r#"{"status":"needs_repair","summary":"本次未执行科学资源下载。","next_action":"保持联网后重新运行安装/修复","downloads":{"status":"skipped","ready":0,"failed":0,"message":"本次未执行科学资源下载。","next_action":"保持联网后重新运行安装/修复"}}"#,
        )
        .expect("installer json");

        assert_eq!(payload["downloads"]["completed"], false);
        assert_eq!(payload["downloads"]["message"], "本次未执行科学资源下载。");
        let text = payload.to_string();
        assert!(!text.contains("skipped"));
    }

    #[test]
    fn setup_installer_external_summary_hides_sync_internals() {
        let payload = setup_installer_summary(
            r#"{"status":"needs_repair","external_sources":{"status":"needs_sync","ready":0,"total":1,"needs_action":1,"message":"大型科学资源同步状态已检查。","next_action":"运行安装/修复以同步缺失资源","items":[{"label":"PLINDER","status":"需要同步","ready":false,"next_action":"运行安装/修复以同步 PLINDER 结构数据","landing_url":"gs://plinder/2024-06/v2","target_path":"D:/hidden/downloads/plinder","ready_marker":"downloads/plinder"}]}}"#,
        )
        .expect("installer json");

        assert_eq!(payload["external_sources"]["total"], 1);
        assert_eq!(payload["external_sources"]["items"][0]["label"], "PLINDER");
        assert_eq!(
            payload["external_sources"]["items"][0]["next_action"],
            "运行安装/修复以同步 PLINDER 结构数据"
        );
        let text = payload.to_string();
        assert!(!text.contains("needs_sync"));
        assert!(!text.contains("大型科学资源同步状态已检查"));
        assert!(!text.contains("gs://"));
        assert!(!text.contains("landing_url"));
        assert!(!text.contains("target_path"));
        assert!(!text.contains("ready_marker"));
        assert!(!text.contains("D:/hidden"));
    }

    #[test]
    fn setup_installer_source_records_hide_manifest_paths_and_hashes() {
        let payload = setup_installer_summary(
            r#"{"status":"needs_repair","source_manifests":{"status":"needs_repair","total":4,"ready":2,"hash_ready":2,"license_ready":3,"source_file_manifest":"D:/hidden/source_file_manifest.tsv","hash_manifest":"D:/hidden/hash_manifest.tsv","license_manifest":"D:/hidden/license_manifest.tsv","sha256":"abc123"}}"#,
        )
        .expect("installer json");

        assert!(payload.get("source_manifests").is_none());
        assert_eq!(payload["source_records"]["total"], 4);
        assert_eq!(payload["source_records"]["ready"], 2);
        assert_eq!(payload["source_records"]["integrity_ready"], 2);
        assert_eq!(payload["source_records"]["license_ready"], 3);
        assert_eq!(payload["source_records"]["completed"], false);
        assert_eq!(payload["source_records"]["record_directory"], "manifests");
        assert_eq!(
            payload["source_records"]["record_sets"][0]["label"],
            "来源文件记录"
        );
        assert_eq!(
            payload["source_records"]["record_sets"][1]["label"],
            "完整性记录"
        );
        assert_eq!(
            payload["source_records"]["record_sets"][2]["label"],
            "许可证记录"
        );
        let text = payload.to_string();
        assert!(!text.contains("source_manifests"));
        assert!(!text.contains("source_file_manifest"));
        assert!(!text.contains("hash_manifest"));
        assert!(!text.contains("license_manifest"));
        assert!(!text.contains("hash_ready"));
        assert!(!text.contains("sha256"));
        assert!(!text.contains("abc123"));
        assert!(!text.contains("D:/hidden"));
    }

    #[test]
    fn setup_installer_tool_environment_summary_hides_internal_fields() {
        let payload = setup_installer_summary(
            r#"{"status":"needs_repair","tool_environment":{"status":"needs_repair","python":"D:/hidden/runtime/python-worker-env/Scripts/python.exe","manifest":"D:/hidden/runtime/python-worker-env/tool_environment_manifest.tsv","ready":4,"total":6,"environment":"created","notes":"pip install failed"}}"#,
        )
        .expect("installer json");

        assert_eq!(payload["tool_environment"]["ready"], 4);
        assert_eq!(payload["tool_environment"]["total"], 6);
        assert_eq!(payload["tool_environment"]["completed"], false);
        assert_eq!(payload["tool_environment"]["next_action"], "运行安装/修复");
        let text = payload.to_string();
        assert!(!text.contains("D:/hidden"));
        assert!(!text.contains("python-worker-env"));
        assert!(!text.contains("tool_environment_manifest"));
        assert!(!text.contains("pip install failed"));
    }

    #[test]
    fn setup_install_plan_reports_catalog_coverage_without_paths() {
        let root = std::env::temp_dir().join(format!("genoligand-install-plan-{}", now_millis()));
        let config_dir = root.join("configs");
        fs::create_dir_all(&config_dir).expect("config dir");
        fs::write(
            config_dir.join("source_catalog.tsv"),
            "source_id\turl\tlocal_path\nchembl\thttps://invalid.invalid/chembl.tar.gz\tdownloads/chembl/chembl_sqlite.tar.gz\n",
        )
        .expect("source catalog");
        fs::write(
            config_dir.join("source_sync_plan.tsv"),
            "source_id\tsource_label\ttarget_rel_path\nplinder\tPLINDER\tdownloads/plinder\n",
        )
        .expect("source sync plan");

        let payload = user_setup_install_plan_json(&server_options(root.clone()));
        assert_eq!(payload["direct_sources"]["source_count"], 1);
        assert_eq!(payload["external_sources"]["source_group_count"], 1);
        assert_eq!(payload["direct_sources"]["ready"], true);
        assert_eq!(payload["external_sources"]["ready"], true);
        assert_eq!(payload["direct_sources"]["next_action"], "运行安装/修复");
        let text = payload.to_string();
        assert!(!text.contains("can_run_from_webui"));
        assert!(!text.contains("ready_to_run"));
        assert!(!text.contains("needs_package_update"));
        assert!(!text.contains("source_catalog"));
        assert!(!text.contains("source_sync_plan"));
        assert!(!text.contains("https://invalid"));
        assert!(!text.contains("downloads/"));
        assert!(!text.contains(&root.display().to_string()));

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn source_sync_plan_uses_executable_sync_methods() {
        let plan = include_str!("../../../configs/source_sync_plan.tsv");
        let mut lines = plan.lines();
        let header = lines.next().expect("source sync header");
        let headers = header.split('\t').collect::<Vec<_>>();
        let source_id = headers
            .iter()
            .position(|value| *value == "source_id")
            .expect("source_id");
        let method = headers
            .iter()
            .position(|value| *value == "sync_method")
            .expect("sync_method");
        let source_url = headers
            .iter()
            .position(|value| *value == "source_url")
            .expect("source_url");
        let target = headers
            .iter()
            .position(|value| *value == "target_rel_path")
            .expect("target_rel_path");
        let ready = headers
            .iter()
            .position(|value| *value == "ready_marker")
            .expect("ready_marker");
        let release = headers
            .iter()
            .position(|value| *value == "release_or_date")
            .expect("release_or_date");
        let license = headers
            .iter()
            .position(|value| *value == "license_note")
            .expect("license_note");
        let allowed = [
            "html_links",
            "html_tree",
            "gcs_prefix",
            "rcsb_ligand_bound_scope",
            "rsync",
            "direct",
        ];
        let mut rows = 0usize;
        for line in lines {
            if line.trim().is_empty() {
                continue;
            }
            let cols = line.split('\t').collect::<Vec<_>>();
            rows += 1;
            assert!(!cols[source_id].trim().is_empty());
            assert!(allowed.contains(&cols[method]));
            assert!(!cols[source_url].trim().is_empty());
            assert!(!cols[target].trim().is_empty());
            assert!(!cols[ready].trim().is_empty());
            assert!(!cols[release].trim().is_empty());
            assert!(!cols[license].trim().is_empty());
            assert_ne!(
                cols[method], "direct",
                "external sources must not degrade to opaque landing pages"
            );
        }
        assert_eq!(rows, 4);
    }

    #[test]
    fn first_launch_user_workspace_has_no_seeded_projects_or_templates() {
        let root = std::env::temp_dir().join(format!("genoligand-first-launch-{}", now_millis()));
        let options = server_options(root.clone());
        fs::create_dir_all(projects_root(&options).join("project-stale").join("runs"))
            .expect("stale project dir");
        let payload = user_projects_json(&options);
        let summary = user_workspace_summary_json(&options);

        assert_eq!(payload["projects"].as_array().map(Vec::len), Some(0));
        assert_eq!(summary["project_count"], 0);
        assert_eq!(summary["run_count"], 0);
        assert_eq!(summary["candidate_count"], 0);
        assert_eq!(summary["result_count"], 0);
        assert_eq!(summary["export_item_count"], 0);
        assert_eq!(summary["blank_workspace"], true);
        assert!(payload.get("templates").is_none());
        let text = payload.to_string();
        assert!(!text.contains("templates"));
        assert!(!text.contains("template"));
        assert!(!text.contains("demo"));
        assert!(!text.contains("sample"));
        assert!(!text.contains("example"));
        assert!(!text.contains("GitHub"));
        assert!(!text.contains("commit"));
        assert!(!text.contains("remote"));
        assert!(!text.contains("supported_actions"));
        assert!(!text.contains("preflight"));

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn user_workspace_summary_counts_visible_completed_run_artifacts() {
        let root =
            std::env::temp_dir().join(format!("genoligand-workspace-summary-{}", now_millis()));
        let options = server_options(root.clone());
        let hidden_run_dir = projects_root(&options)
            .join("hidden-project")
            .join("runs")
            .join("run-hidden");
        fs::create_dir_all(&hidden_run_dir).expect("hidden run dir");
        fs::write(hidden_run_dir.join("run_state.txt"), "completed\n").expect("hidden state");
        fs::write(
            hidden_run_dir.join("target_candidate_ranking.tsv"),
            "candidate_id\nHIDDEN\n",
        )
        .expect("hidden candidates");

        let project_dir = projects_root(&options).join("project-1");
        let run_dir = project_dir.join("runs").join("run-1");
        fs::create_dir_all(&run_dir).expect("run dir");
        fs::write(
            project_dir.join("input_manifest.json"),
            r#"{"inputs":[{"field":"small_molecule","value":"CCO"}],"files":[]}"#,
        )
        .expect("input manifest");
        fs::write(project_dir.join("latest_run_id.txt"), "run-1\n").expect("latest run");
        fs::write(run_dir.join("run_state.txt"), "completed\n").expect("state");
        fs::write(
            run_dir.join("target_candidate_ranking.tsv"),
            "candidate_id\ttarget_id\nC1\tP1\nC2\tP2\n",
        )
        .expect("candidate ranking");
        fs::write(
            run_dir.join("evidence_ledger.tsv"),
            "claim_id\tstatus\ncandidate_ranking\tavailable\n",
        )
        .expect("evidence");
        fs::write(
            run_dir.join("metagenome_potential_matches.tsv"),
            "orf_id\tmatch_score\nORF1\t0.42\n",
        )
        .expect("genome rows");
        fs::write(
            run_dir.join("structure_patch_summary.tsv"),
            "candidate_id\n",
        )
        .expect("structure header");
        fs::write(run_dir.join("md_summary.tsv"), "candidate_id\n").expect("md header");
        fs::write(run_dir.join("research_report.md"), "# Report\n").expect("report");

        let summary = user_workspace_summary_json(&options);
        assert_eq!(summary["project_count"], 1);
        assert_eq!(summary["run_count"], 1);
        assert_eq!(summary["candidate_count"], 2);
        assert_eq!(summary["result_count"], 4);
        assert_eq!(summary["export_item_count"], 3);
        assert_eq!(summary["demo_count"], 0);
        assert_eq!(summary["sample_count"], 0);
        assert_eq!(summary["test_result_count"], 0);
        assert_eq!(summary["blank_workspace"], false);

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn bundled_user_web_assets_avoid_public_debug_and_mojibake_language() {
        let user_assets = web_user_assets();
        let forbidden_terms = forbidden_user_asset_terms();

        for term in &forbidden_terms {
            assert!(
                !user_assets.contains(term.as_str()),
                "bundled user web assets expose forbidden term: {term}"
            );
        }
    }

    fn forbidden_user_asset_terms() -> Vec<String> {
        vec![
            vec!['\u{771f}', '\u{5b9e}'].into_iter().collect(),
            ["de", "mo"].concat(),
            ["De", "mo"].concat(),
            ["sam", "ple"].concat(),
            ["Sam", "ple"].concat(),
            ["ex", "ample"].concat(),
            ["Ex", "ample"].concat(),
            vec!['\u{793a}', '\u{4f8b}'].into_iter().collect(),
            vec!['\u{6837}', '\u{4f8b}'].into_iter().collect(),
            vec!['\u{6d4b}', '\u{8bd5}'].into_iter().collect(),
            ["Git", "Hub"].concat(),
            ["com", "mit"].concat(),
            ["re", "mote"].concat(),
            ["sche", "ma"].concat(),
            ["mani", "fest"].concat(),
            ["ha", "sh"].concat(),
            ["raw ", "JSON"].concat(),
            ["raw ", "json"].concat(),
            ["ga", "te"].concat(),
            ["ro", "ute"].concat(),
            vec!['\u{9a8c}', '\u{6536}'].into_iter().collect(),
            ["phase", "_0"].concat(),
            "\u{fffd}".to_string(),
            vec!['\u{704f}'].into_iter().collect(),
            vec!['\u{93c1}'].into_iter().collect(),
            vec!['\u{6748}'].into_iter().collect(),
            vec!['\u{7ecc}'].into_iter().collect(),
        ]
    }

    fn workspace_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root")
            .to_path_buf()
    }

    fn web_asset_text(rel: &str) -> String {
        fs::read_to_string(workspace_root().join("webui").join("app").join(rel))
            .unwrap_or_else(|err| panic!("read web asset {rel}: {err}"))
    }

    fn web_user_assets() -> String {
        [
            "index.html",
            "app.js",
            "state/app-state.js",
            "api/client.js",
            "pages/input.js",
            "pages/configure.js",
            "pages/check.js",
            "pages/run.js",
            "pages/results.js",
            "pages/export.js",
            "core/actions.js",
            "core/selectors.js",
            "components/common.js",
        ]
        .iter()
        .map(|rel| web_asset_text(rel))
        .collect::<Vec<_>>()
        .join("\n")
    }

    #[test]
    fn split_user_web_assets_are_served_from_whitelist() {
        let options = server_options(workspace_root());

        let (status, content_type, body) = route("GET", "/state/app-state.js", &options, "");
        assert_eq!(status, 200);
        assert_eq!(content_type, "application/javascript; charset=utf-8");
        assert!(body.contains("const state ="));

        let (status, content_type, body) = route("GET", "/styles/tokens.css", &options, "");
        assert_eq!(status, 200);
        assert_eq!(content_type, "text/css; charset=utf-8");
        assert!(body.contains("--accent"));

        let (status, content_type, body) = route("GET", "/favicon.svg", &options, "");
        assert_eq!(status, 200);
        assert_eq!(content_type, "image/svg+xml; charset=utf-8");
        assert!(body.contains("<svg"));

        let (status, _, _) = route("GET", "/../Cargo.toml", &options, "");
        assert_eq!(status, 404);
    }

    #[test]
    fn first_launch_input_view_does_not_render_resource_status_panels() {
        let app = web_asset_text("pages/input.js");
        let start = app.find("function renderInput()").expect("renderInput");
        let end = app
            .find("function renderInputTypeCard")
            .expect("renderInputTypeCard");
        let render_input = &app[start..end];

        assert!(!render_input.contains("storeStatusRows()"));
        assert!(!render_input.contains("sourceStatusTable()"));
        assert!(!render_input.contains("数据库状态"));
        assert!(!render_input.contains("科学资源"));
    }

    #[test]
    fn run_monitor_export_list_only_renders_downloadable_artifacts() {
        let app = web_asset_text("core/selectors.js");
        let start = app.find("function artifactRows()").expect("artifactRows");
        let artifact_rows = &app[start..];

        assert!(artifact_rows.contains("filter((item) => item.exists)"));
        assert!(artifact_rows.contains("可下载"));
        assert!(!artifact_rows.contains("未生成"));
    }

    #[test]
    fn export_view_only_renders_downloadable_artifacts() {
        let app = web_asset_text("pages/export.js");
        let selectors = web_asset_text("core/selectors.js");
        let start = app.find("function renderExport()").expect("renderExport");
        let export_view = &app[start..];

        assert!(selectors.contains("available.map"));
        assert!(export_view.contains("当前运行还没有可导出材料"));
        assert!(selectors.contains("下载"));
        assert!(!export_view.contains("当前运行未生成"));
        assert!(!selectors.contains("不可用"));
    }

    #[test]
    fn run_monitor_only_links_to_results_after_completion() {
        let render_run = web_asset_text("pages/run.js");
        let selectors = web_asset_text("core/selectors.js");

        assert!(render_run.contains("canViewResults()"));
        assert!(!render_run.contains("state.run ? \"\" : \"disabled\""));
        assert!(selectors.contains("function canViewResults()"));
        assert!(selectors.contains("return state.run?.state === \"completed\""));
    }

    #[test]
    fn workflow_navigation_unlocks_results_only_after_completed_run() {
        let app = web_asset_text("app.js");
        let selectors = web_asset_text("core/selectors.js");

        assert!(selectors.contains("function canOpenStep(step)"));
        assert!(selectors.contains("if (step === \"input\") return true"));
        assert!(
            selectors.contains(
                "if (step === \"results\" || step === \"export\") return canViewResults();"
            )
        );
        assert!(selectors.contains("button.disabled = !enabled"));
        assert!(app.contains("if (!canOpenStep(step)) return;"));
    }

    #[test]
    fn saving_input_clears_stale_run_state_in_webui() {
        let app = web_asset_text("core/actions.js");
        let start = app.find("async function saveInput()").expect("saveInput");
        let end = app[start..]
            .find("async function runPreflight()")
            .map(|offset| start + offset)
            .expect("runPreflight");
        let save_input = &app[start..end];

        assert!(save_input.contains("state.run = null"));
        assert!(save_input.contains("state.runData = null"));
        assert!(save_input.contains("state.preflight = null"));
    }

    #[test]
    fn webui_loads_existing_projects_without_seeding_workspace() {
        let app = web_asset_text("app.js");
        let input = web_asset_text("pages/input.js");

        assert!(app.contains("/api/user/workspace-summary"));
        assert!(app.contains("/api/user/projects"));
        assert!(input.contains("继续已有项目"));
        assert!(input.contains("data-open-project"));
        assert!(input.contains("openExistingProject"));
    }

    #[test]
    fn webui_uses_structure_index_requirement_id() {
        let selectors = web_asset_text("core/selectors.js");
        assert!(selectors.contains("row.id === \"structure_index\""));
        assert!(selectors.contains("checkRow(byId, \"structure_index\""));
        assert!(!selectors.contains("row.id === \"structure_interaction\""));
        assert!(!selectors.contains("checkRow(byId, \"structure_interaction\""));
    }

    #[test]
    fn visual_result_sections_use_clear_not_ready_language() {
        let app = web_asset_text("pages/results.js");
        assert!(app.contains("三维查看未就绪"));
        assert!(app.contains("轨迹图未就绪"));
        assert!(!app.contains("未接入可用输出"));

        let structure = user_structure_row(
            &json!({"candidate_id":"C1"}),
            &BTreeMap::new(),
            &json!({"ready": false}),
        );
        let ready_structure = user_structure_row(
            &json!({"candidate_id":"C1"}),
            &BTreeMap::new(),
            &json!({"ready": true}),
        );
        let md = user_md_row(&json!({"candidate_id":"C1"}), &BTreeMap::new());
        assert_eq!(structure["viewer_status"], "三维查看未就绪");
        assert_eq!(ready_structure["viewer_status"], "三维查看已就绪");
        assert_eq!(md["trajectory_status"], "轨迹图未就绪");
    }

    #[test]
    fn user_project_creation_hides_internal_status_and_actions() {
        let root = std::env::temp_dir().join(format!("genoligand-user-create-{}", now_millis()));
        let options = server_options(root.clone());

        let payload = create_user_project(&options);
        let text = payload.to_string();
        let listed = user_projects_json(&options);

        assert!(payload.get("project").is_some());
        assert!(payload["project"].get("project_id").is_some());
        assert!(payload.get("message").is_some());
        assert_eq!(listed["projects"].as_array().map(Vec::len), Some(0));
        assert!(payload.get("status").is_none());
        assert!(!text.contains("created"));
        assert!(!text.contains("supported_actions"));
        assert!(!text.contains("preflight"));
        assert!(!text.contains("export"));

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn setup_progress_marks_stale_running_state_as_repairable() {
        let root = std::env::temp_dir().join(format!("genoligand-stale-progress-{}", now_millis()));
        let options = server_options(root.clone());
        let store = science_store_path(&options);
        fs::create_dir_all(&store).expect("store");
        fs::write(
            store.join("install_progress.json"),
            json!({
                "status": "running",
                "stage": "下载数据库",
                "current_resource": "ChEMBL",
                "completed_steps": 1,
                "total_steps": 6,
                "failed_steps": 0,
                "bytes_downloaded": 1024,
                "bytes_total": 2048,
                "percent": 50,
                "message": "正在下载数据库资源",
                "updated_at": 1
            })
            .to_string(),
        )
        .expect("progress");

        let progress = user_setup_progress_json(&options);
        assert_eq!(progress["status"], "needs_repair");
        assert_eq!(progress["failed_steps"], 1);
        assert!(
            progress["message"]
                .as_str()
                .unwrap_or_default()
                .contains("安装进度长时间没有更新")
        );

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn user_health_hides_backend_service_name() {
        let payload = user_health_json();
        let text = payload.to_string();

        assert_eq!(payload["status"], "ready");
        assert_eq!(payload["application"], PRODUCT_NAME);
        assert!(!text.contains("gls-server"));
        assert!(!text.contains("route"));
        assert!(!text.contains("debug"));
    }

    #[test]
    fn user_setup_status_hides_runtime_and_internal_status_codes() {
        let root =
            std::env::temp_dir().join(format!("genoligand-user-setup-status-{}", now_millis()));
        let options = server_options(root.clone());
        let payload = user_setup_status_json(&options);
        let text = payload.to_string();

        assert!(payload.get("database_ready").is_some());
        assert!(payload.get("repair_action").is_some());
        assert!(payload.get("status").is_none());
        assert!(payload.get("runtime").is_none());
        assert!(payload.get("can_run_repair").is_none());
        assert!(!text.contains("needs_repair"));
        assert!(!text.contains("runtime"));
        assert!(!text.contains("can_run_repair"));

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn user_diagnostics_hides_runtime_and_status_codes() {
        let root =
            std::env::temp_dir().join(format!("genoligand-user-diagnostics-{}", now_millis()));
        let options = server_options(root.clone());
        let payload = user_diagnostics_json(&options);
        let text = payload.to_string();

        assert!(payload.get("checks").is_some());
        assert!(payload.get("missing").is_some());
        assert!(payload.get("status").is_none());
        assert!(payload.get("runtime").is_none());
        assert!(!text.contains("warning"));
        assert!(!text.contains("runtime"));

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn openmm_platform_status_reads_cuda_row_from_tool_versions() {
        let root =
            std::env::temp_dir().join(format!("genoligand-openmm-platforms-{}", now_millis()));
        let store = root.join("science_store");
        let index_dir = store.join("indexes");
        fs::create_dir_all(&index_dir).expect("index dir");
        fs::write(
            index_dir.join("tool_versions.tsv"),
            "tool\tstatus\tversion\tlicense\tlabel\tdetail\nopenmm\tready\t8.2.0\tMIT\tOpenMM\topenmm_python_module_detected;platforms=Reference,CPU,CUDA;cuda=ready\nopenmm_cuda\tready\t8.2.0\tMIT\tOpenMM CUDA Platform\topenmm_python_module_detected;platforms=Reference,CPU,CUDA;cuda=ready\n",
        )
        .expect("tool versions");

        let status = openmm_platform_status(&store);
        assert_eq!(status["openmm_ready"], true);
        assert_eq!(status["cuda_ready"], true);
        assert_eq!(status["platforms"], "Reference,CPU,CUDA");
        assert_eq!(status["cuda_path_ready"], false);
        assert!(
            status["summary"]
                .as_str()
                .unwrap_or_default()
                .contains("CUDA")
        );

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn openmm_platform_status_reports_cuda_toolkit_and_failure_reason() {
        let root =
            std::env::temp_dir().join(format!("genoligand-openmm-cuda-reason-{}", now_millis()));
        let store = root.join("science_store");
        let index_dir = store.join("indexes");
        fs::create_dir_all(&index_dir).expect("index dir");
        fs::write(
            index_dir.join("tool_versions.tsv"),
            "tool\tstatus\tversion\tlicense\tlabel\tdetail\nopenmm\tready\t8.5.2\tMIT\tOpenMM\topenmm_python_module_detected;platforms=Reference,CPU;cuda=missing;cuda_failure_reason=missing:CUDA platform plugin not loaded;cuda_toolkit_path=D:\\Program Files\\CUDA;cuda_path_ready=true;cuda_nvcc=present\nopenmm_cuda\tmissing\t8.5.2\tMIT\tOpenMM CUDA Platform\topenmm_python_module_detected;platforms=Reference,CPU;cuda=missing;cuda_failure_reason=missing:CUDA platform plugin not loaded;cuda_toolkit_path=D:\\Program Files\\CUDA;cuda_path_ready=true;cuda_nvcc=present\n",
        )
        .expect("tool versions");

        let status = openmm_platform_status(&store);

        assert_eq!(status["openmm_ready"], true);
        assert_eq!(status["cuda_ready"], false);
        assert_eq!(status["cuda_path_ready"], true);
        assert_eq!(status["cuda_toolkit_path"], r"D:\Program Files\CUDA");
        assert!(
            status["cuda_reason"]
                .as_str()
                .unwrap_or_default()
                .contains("plugin")
        );
        assert!(
            status["summary"]
                .as_str()
                .unwrap_or_default()
                .contains(r"D:\Program Files\CUDA")
        );

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn user_setup_and_diagnostics_expose_openmm_platforms() {
        let root =
            std::env::temp_dir().join(format!("genoligand-user-openmm-platforms-{}", now_millis()));
        let options = server_options(root.clone());

        let diagnostics = user_diagnostics_json(&options);
        let setup = user_setup_status_json(&options);
        let plan = user_setup_install_plan_json(&options);

        let checks = diagnostics["checks"].as_array().expect("checks");
        assert!(
            checks
                .iter()
                .any(|row| row.get("label").and_then(Value::as_str) == Some("OpenMM/CUDA"))
        );
        assert_eq!(diagnostics["tool_platforms"][0]["id"], "openmm_cuda");
        assert_eq!(setup["tool_platforms"][0]["id"], "openmm_cuda");
        assert_eq!(plan["tool_platforms"][0]["id"], "openmm_cuda");

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn input_validation_blocks_punctuation_only_payloads() {
        let root =
            std::env::temp_dir().join(format!("genoligand-input-validation-{}", now_millis()));
        let options = server_options(root.clone());
        fs::create_dir_all(project_dir(&options, "project-1")).expect("project dir");

        let response = write_input_manifest(
            &options,
            "project-1",
            r#"{"inputs":[{"field":"project_name","value":"研究项目"},{"field":"small_molecule","value":"!!!"}],"files":[]}"#,
        );
        assert_eq!(response["status"], "failed");
        assert_eq!(response["error"], "input_not_usable");
        assert!(
            !project_dir(&options, "project-1")
                .join("project_name.txt")
                .exists()
        );
        assert!(
            !project_dir(&options, "project-1")
                .join("input_manifest.json")
                .exists()
        );
        assert!(!input_value_has_usable_user_input(&json!({
            "inputs":[{"field":"complex","value":"---"}]
        })));
        assert!(!input_value_has_usable_user_input(&json!({
            "inputs":[
                {"field":"small_molecule","value":"CCO"},
                {"field":"protein_ligand","value":"???"}
            ]
        })));
        assert!(!input_value_has_usable_user_input(&json!({
            "inputs":[{"field":"peptide","value":"lqtu0we9ur"}]
        })));
        assert!(!input_value_has_usable_user_input(&json!({
            "inputs":[{"field":"protein_ligand","value":"lqtu0we9ur"}]
        })));
        assert!(!input_value_has_usable_user_input(&json!({
            "inputs":[{"field":"genome_or_metagenome","value":"lqtu0we9ur"}]
        })));

        let preflight = preflight_json(&options, "project-1");
        let input_ready = preflight["checks"]
            .as_array()
            .and_then(|rows| {
                rows.iter()
                    .find(|row| row.get("id").and_then(Value::as_str) == Some("input"))
            })
            .and_then(|row| row.get("ready").and_then(Value::as_bool))
            .unwrap_or(true);
        assert!(!input_ready);

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn input_validation_accepts_supported_input_types() {
        for (field, value) in [
            ("small_molecule", "CCO"),
            ("peptide", "ACDEFG"),
            ("protein_ligand", ">protein\nMSTNPKPQR"),
            ("complex", "complex_chain_A_ligand_LIG.pdb"),
            ("genome_or_metagenome", ">orf1\nATGCGTACGTAA"),
        ] {
            assert!(
                input_value_has_usable_user_input(&json!({
                    "inputs":[{"field":field,"value":value}]
                })),
                "{field} should be accepted"
            );
        }
    }

    #[test]
    fn source_status_actions_are_user_facing() {
        assert_eq!(friendly_source_category("known_activity"), "已知活性");
        assert_eq!(
            friendly_source_status("downloaded_needs_index"),
            "已下载，需重建索引"
        );
        assert_eq!(
            friendly_source_next_action("download_and_index_biolip2", "missing"),
            "运行安装/修复以准备 BioLiP2 互作数据"
        );
        assert_eq!(friendly_source_next_action("anything", "ready"), "继续");
        assert!(
            !friendly_source_next_action("download_and_index_chembl", "missing")
                .contains("download_and_index")
        );
    }

    #[test]
    fn missing_resource_summary_is_user_facing() {
        let root =
            std::env::temp_dir().join(format!("genoligand-missing-summary-{}", now_millis()));
        let rows = required_resource_rows(&root);
        let missing = missing_resource_rows(&rows);
        let text = serde_json::to_string(&missing).expect("missing json");

        assert_eq!(missing.len(), 8);
        assert!(text.contains("科学仓库"));
        assert!(text.contains("已知蛋白索引"));
        assert!(text.contains("宏基因组索引"));
        assert!(text.contains("结构索引"));
        assert!(text.contains("计算工具"));
        assert!(text.contains("安装状态"));
        assert!(text.contains("科学资源状态"));
        assert!(text.contains("来源与许可记录"));
        assert!(text.contains("运行安装/修复"));
        assert!(!text.contains("science_store_manifest"));
        assert!(!text.contains("known_targets.tsv"));
        assert!(!text.contains("tool_versions.tsv"));
        assert!(!text.contains(&root.display().to_string()));
    }

    #[test]
    fn user_database_status_hides_path_configuration_fields() {
        let root = std::env::temp_dir().join(format!("genoligand-user-databases-{}", now_millis()));
        let options = server_options(root.clone());

        let payload = user_databases_json(&options);
        let text = payload.to_string();

        assert!(!text.contains("user_configures_paths"));
        assert!(!text.contains("science_store_manifest"));
        assert!(!text.contains(&root.display().to_string()));

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn failure_summary_hides_internal_resource_names() {
        let summary = friendly_failure_summary(Some(
            "missing_science_store_manifest,missing_tool_versions_manifest,missing_known_database_index,missing_structure_interaction_index",
        ));

        assert!(summary.contains("缺少科学仓库"));
        assert!(summary.contains("计算工具未就绪"));
        assert!(summary.contains("缺少已知蛋白索引"));
        assert!(summary.contains("缺少结构索引"));
        assert!(!summary.contains("manifest"));
        assert!(!summary.contains("tool_versions"));
        assert!(!summary.contains("science_store"));
    }

    #[test]
    fn user_diagnostics_do_not_request_developer_build_tools() {
        let root =
            std::env::temp_dir().join(format!("genoligand-user-diagnostics-{}", now_millis()));
        let options = server_options(root.clone());

        let payload = user_diagnostics_json(&options);
        let text = payload.to_string();

        assert!(text.contains("运行组件"));
        assert!(!text.contains("命令行入口"));
        assert!(!text.contains("gls-cli"));
        assert!(!text.contains("Rust"));
        assert!(!text.contains("cargo"));
        assert!(!text.contains("构建环境"));

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn artifact_download_is_limited_to_current_run_allowlist() {
        let root = std::env::temp_dir().join(format!("genoligand-download-{}", now_millis()));
        let options = server_options(root.clone());
        let run_dir = projects_root(&options)
            .join("project-1")
            .join("runs")
            .join("run-1");
        fs::create_dir_all(&run_dir).expect("create run dir");
        fs::write(run_dir.join("run_state.txt"), "completed\n").expect("state");
        fs::write(run_dir.join("research_report.md"), "# Report\n").expect("write report");
        fs::write(run_dir.join("secret.txt"), "hidden\n").expect("write secret");

        let (status, _, _) = download_artifact_response(&options, "run-1", "research_report");
        assert_eq!(status, 404);

        fs::write(
            run_dir.join("target_candidate_ranking.tsv"),
            "rank\tcandidate_id\ttarget_id\ttarget_name\tcandidate_scope\torganism\tfinal_score\tknown_database_score\tmetagenome_score\tstructure_score\tdocking_score\tmd_score\tevidence_summary\tstatus\n1\tC1\tP24941\tCDK2\tknown_database\tHomo sapiens\t0.80\t0.70\t0.00\t0.10\t0.00\t0.00\tmatched current run\tcomputed_prediction\n",
        )
        .expect("candidate table");
        fs::write(
            run_dir.join("evidence_ledger.tsv"),
            "claim_id\tevidence_artifact\towner\tstatus\tnotes\nscore_decomposition\tscore_decomposition.tsv\tGenoLigand\tavailable\tcandidate evidence\n",
        )
        .expect("evidence");

        let (status, content_type, body) =
            download_artifact_response(&options, "run-1", "research_report");
        assert_eq!(status, 200);
        assert_eq!(content_type, "text/markdown; charset=utf-8");
        assert!(body.contains("# Report"));

        let (status, _, _) = download_artifact_response(&options, "run-1", "secret");
        assert_eq!(status, 404);

        let (status, _, _) = download_artifact_response(&options, "run-1", "../secret.txt");
        assert_eq!(status, 404);

        let (status, _, _) = download_artifact_response(&options, "run-1", "normalized_input");
        assert_eq!(status, 404);

        let (status, _, _) = download_artifact_response(&options, "run-1", "score_breakdown");
        assert_eq!(status, 404);

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn blocked_run_trace_does_not_present_scientific_results() {
        let root = std::env::temp_dir().join(format!("genoligand-blocked-trace-{}", now_millis()));
        let options = server_options(root.clone());
        let project_dir = projects_root(&options).join("project-1");
        let run_dir = project_dir.join("runs").join("run-1");
        fs::create_dir_all(&run_dir).expect("create run dir");
        fs::write(
            project_dir.join("input_manifest.json"),
            r#"{"inputs":[{"field":"small_molecule","value":"CCO"}]}"#,
        )
        .expect("input");
        fs::write(run_dir.join("run_state.txt"), "blocked\n").expect("run state");
        fs::write(
            run_dir.join("target_candidate_ranking.tsv"),
            "candidate_id\n",
        )
        .expect("candidate table");
        fs::write(
            run_dir.join("evidence_ledger.tsv"),
            "claim_id\tstatus\nblocked\tmissing_store\n",
        )
        .expect("blocked evidence");
        fs::write(
            run_dir.join("candidate_detail.json"),
            r#"{"candidates":[{"candidate_id":"stale","target_id":"P00000"}]}"#,
        )
        .expect("stale detail");
        fs::write(
            run_dir.join("metagenome_potential_matches.tsv"),
            "target_id\ttarget_name\nMG1\tstale\n",
        )
        .expect("stale genome");
        fs::write(
            run_dir.join("structure_patch_summary.tsv"),
            "candidate_id\tstructure_score\nC1\t0.5\n",
        )
        .expect("stale structure");
        fs::write(
            run_dir.join("md_summary.tsv"),
            "candidate_id\trmsd\nC1\t1.0\n",
        )
        .expect("stale md");
        fs::write(run_dir.join("research_report.md"), "# stale\n").expect("stale report");

        let run = run_json(&options, "run-1");
        assert_eq!(run["trace"]["output_status"], "已记录阻塞信息");
        assert_eq!(run["trace"]["candidate_status"], "暂无候选排序");
        assert_eq!(run["trace"]["candidate_count"], 0);
        assert_eq!(run["trace"]["evidence_status"], "暂无证据链");
        assert_eq!(run["trace"]["evidence_count"], 0);
        let items = run["outputs"]["items"].as_array().expect("output items");
        let candidate_visible = items
            .iter()
            .find(|item| item.get("id").and_then(Value::as_str) == Some("candidate_ranking"))
            .and_then(|item| item.get("exists").and_then(Value::as_bool))
            .unwrap_or(true);
        let evidence_visible = items
            .iter()
            .find(|item| item.get("id").and_then(Value::as_str) == Some("evidence"))
            .and_then(|item| item.get("exists").and_then(Value::as_bool))
            .unwrap_or(true);
        assert!(!candidate_visible);
        assert!(!evidence_visible);
        assert_eq!(candidates_endpoint_json(&options, "run-1")["row_count"], 0);
        assert_eq!(
            candidate_detail_json(&options, "run-1")["candidate_count"],
            0
        );
        assert_eq!(evidence_endpoint_json(&options, "run-1")["row_count"], 0);
        assert_eq!(genome_endpoint_json(&options, "run-1")["row_count"], 0);
        assert_eq!(structure_endpoint_json(&options, "run-1")["row_count"], 0);
        assert_eq!(md_endpoint_json(&options, "run-1")["row_count"], 0);
        for response in [
            candidates_endpoint_json(&options, "run-1"),
            candidate_detail_json(&options, "run-1"),
            evidence_endpoint_json(&options, "run-1"),
            genome_endpoint_json(&options, "run-1"),
            structure_endpoint_json(&options, "run-1"),
            md_endpoint_json(&options, "run-1"),
        ] {
            assert_eq!(response["has_output"], false);
            let text = response.to_string();
            assert!(!text.contains("stale"));
            assert!(!text.contains("missing_store"));
        }
        for artifact_id in [
            "research_report",
            "candidate_ranking",
            "evidence",
            "structure_patch",
            "md_summary",
        ] {
            let (status, _, body) = download_artifact_response(&options, "run-1", artifact_id);
            assert_eq!(status, 404);
            assert!(!body.contains("stale"));
            assert!(!body.contains("missing_store"));
        }

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn completed_header_only_result_files_do_not_count_as_user_outputs() {
        let root = std::env::temp_dir().join(format!("genoligand-empty-results-{}", now_millis()));
        let options = server_options(root.clone());
        let project_dir = projects_root(&options).join("project-1");
        let run_dir = project_dir.join("runs").join("run-1");
        fs::create_dir_all(&run_dir).expect("create run dir");
        fs::write(run_dir.join("run_state.txt"), "completed\n").expect("run state");
        fs::write(
            run_dir.join("target_candidate_ranking.tsv"),
            "candidate_id\ttarget_id\n",
        )
        .expect("candidate table");
        fs::write(run_dir.join("evidence_ledger.tsv"), "claim_id\tstatus\n").expect("evidence");
        fs::write(
            run_dir.join("candidate_detail.json"),
            r#"{"candidates":[]}"#,
        )
        .expect("candidate detail");
        fs::write(
            run_dir.join("metagenome_potential_matches.tsv"),
            "target_id\ttarget_name\n",
        )
        .expect("genome");
        fs::write(
            run_dir.join("structure_patch_summary.tsv"),
            "candidate_id\tstructure_score\n",
        )
        .expect("structure");
        fs::write(run_dir.join("md_summary.tsv"), "candidate_id\trmsd\n").expect("md");
        fs::write(run_dir.join("research_report.md"), "# Empty report\n").expect("report");

        for response in [
            candidates_endpoint_json(&options, "run-1"),
            candidate_detail_json(&options, "run-1"),
            evidence_endpoint_json(&options, "run-1"),
            genome_endpoint_json(&options, "run-1"),
            structure_endpoint_json(&options, "run-1"),
            md_endpoint_json(&options, "run-1"),
        ] {
            assert_eq!(response["has_output"], false);
        }
        assert_eq!(candidates_endpoint_json(&options, "run-1")["row_count"], 0);
        assert_eq!(
            candidate_detail_json(&options, "run-1")["candidate_count"],
            0
        );
        assert_eq!(evidence_endpoint_json(&options, "run-1")["row_count"], 0);
        assert_eq!(genome_endpoint_json(&options, "run-1")["row_count"], 0);
        assert_eq!(structure_endpoint_json(&options, "run-1")["row_count"], 0);
        assert_eq!(md_endpoint_json(&options, "run-1")["row_count"], 0);
        let artifacts = artifacts_json(&options, "run-1");
        let report_visible = artifacts["items"]
            .as_array()
            .expect("items")
            .iter()
            .find(|item| item.get("id").and_then(Value::as_str) == Some("research_report"))
            .and_then(|item| item.get("exists").and_then(Value::as_bool))
            .unwrap_or(true);
        assert!(!report_visible);
        let (status, _, body) = download_artifact_response(&options, "run-1", "research_report");
        assert_eq!(status, 404);
        assert!(!body.contains("Empty report"));

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn user_input_and_preflight_endpoints_hide_internal_status_codes() {
        let root = std::env::temp_dir().join(format!("genoligand-user-flow-{}", now_millis()));
        let options = server_options(root.clone());
        fs::create_dir_all(project_dir(&options, "project-1")).expect("project dir");

        let invalid = write_user_input_manifest(
            &options,
            "project-1",
            r#"{"inputs":[{"field":"project_name","value":"研究项目"},{"field":"small_molecule","value":"!!!"}],"files":[]}"#,
        );
        let invalid_text = invalid.to_string();
        assert_eq!(invalid["ok"], false);
        assert!(invalid.get("message").is_some());
        assert!(invalid.get("status").is_none());
        assert!(!invalid_text.contains("failed"));
        assert!(!invalid_text.contains("input_not_usable"));

        let preflight = user_preflight_json(&options, "project-1");
        let preflight_text = preflight.to_string();
        assert_eq!(preflight["ok"], false);
        assert!(preflight.get("checks").is_some());
        assert!(preflight.get("message").is_some());
        assert!(preflight.get("status").is_none());
        assert!(preflight.get("candidate_count_if_blocked").is_none());
        assert!(!preflight_text.contains("preflight_passed"));
        assert!(!preflight_text.contains("blocked"));
        assert!(!preflight_text.contains(&root.display().to_string()));

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn preflight_requires_closed_install_state_not_just_files() {
        let root =
            std::env::temp_dir().join(format!("genoligand-preflight-store-{}", now_millis()));
        let options = server_options(root.clone());
        let project_dir = project_dir(&options, "project-1");
        let store = root.join("science_store");
        fs::create_dir_all(store.join("indexes")).expect("store indexes");
        fs::create_dir_all(&project_dir).expect("project dir");
        fs::write(
            project_dir.join("input_manifest.json"),
            r#"{"inputs":[{"field":"small_molecule","value":"CCO"}]}"#,
        )
        .expect("input");
        fs::write(
            store.join("science_store_manifest.tsv"),
            "source\tstatus\nstore\tready\n",
        )
        .expect("store manifest");
        fs::write(
            store.join("indexes/known_targets.tsv"),
            "source\ttarget_id\ttarget_name\torganism\ttarget_class\tfeatures\tevidence\n",
        )
        .expect("known index");
        fs::write(
            store.join("indexes/metagenome_targets.tsv"),
            "source\ttarget_id\ttarget_name\torganism\ttarget_class\tfeatures\tevidence\n",
        )
        .expect("metagenome index");
        fs::write(
            store.join("indexes/structure_targets.tsv"),
            "source\ttarget_id\ttarget_name\torganism\ttarget_class\tfeatures\tevidence\n",
        )
        .expect("structure index");
        fs::write(
            store.join("indexes/tool_versions.tsv"),
            "tool\tstatus\tversion\tdetail\nrdkit\tready\tinstalled\tok\n",
        )
        .expect("tools");
        fs::write(
            store.join("install_status.json"),
            r#"{"status":"needs_repair"}"#,
        )
        .expect("install status");
        fs::write(
            store.join("source_requirements.tsv"),
            "source_id\tstatus\nchembl\tready\n",
        )
        .expect("source requirements");

        let preflight = preflight_json(&options, "project-1");
        let science_store_ready = preflight["checks"]
            .as_array()
            .and_then(|rows| {
                rows.iter()
                    .find(|row| row.get("id").and_then(Value::as_str) == Some("science_store"))
            })
            .and_then(|row| row.get("ready").and_then(Value::as_bool))
            .unwrap_or(true);
        assert!(!science_store_ready);

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn saving_new_input_clears_current_run_selection() {
        let root =
            std::env::temp_dir().join(format!("genoligand-input-clears-run-{}", now_millis()));
        let options = server_options(root.clone());
        let project_id = "project-1";
        let project_dir = project_dir(&options, project_id);
        fs::create_dir_all(&project_dir).expect("project dir");
        fs::write(project_dir.join("latest_run_id.txt"), "run-old\n").expect("latest run");

        let response = write_input_manifest(
            &options,
            project_id,
            r#"{"inputs":[{"field":"project_name","value":"研究项目"},{"field":"protein_ligand","value":">p\nMSTNPKPQR"}],"files":[]}"#,
        );

        assert_eq!(response["status"], "saved");
        assert!(
            !project_dir.join("latest_run_id.txt").exists(),
            "new input must detach stale run outputs from current project"
        );
        assert!(read_project(&options, project_id)["latest_run_id"].is_null());

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn user_run_detail_uses_stage_labels_not_internal_stage_keys() {
        let root = std::env::temp_dir().join(format!("genoligand-user-run-{}", now_millis()));
        let options = server_options(root.clone());
        let run_dir = project_dir(&options, "project-1")
            .join("runs")
            .join("run-1");
        fs::create_dir_all(&run_dir).expect("run dir");
        fs::write(run_dir.join("run_state.txt"), "running\n").expect("state");
        fs::write(run_dir.join("current_stage.txt"), "pipeline_run\n").expect("stage");

        let payload = user_run_json(&options, "run-1");
        let text = payload.to_string();

        assert_eq!(payload["current_stage"], "完整流程");
        assert_eq!(payload["state_label"], "运行中");
        assert!(!text.contains("pipeline_run"));
        assert!(!text.contains("current_command"));
        assert!(!text.contains("artifact_index"));

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn completed_run_marks_missing_structure_and_md_stages_not_generated() {
        let root =
            std::env::temp_dir().join(format!("genoligand-user-run-stages-{}", now_millis()));
        let options = server_options(root.clone());
        let run_dir = project_dir(&options, "project-1")
            .join("runs")
            .join("run-1");
        fs::create_dir_all(&run_dir).expect("run dir");
        fs::write(run_dir.join("run_state.txt"), "completed\n").expect("state");
        fs::write(run_dir.join("current_stage.txt"), "pipeline_run\n").expect("stage");
        fs::write(
            run_dir.join("target_candidate_ranking.tsv"),
            "rank\tcandidate_id\tfinal_score\n1\tC1\t0.5\n",
        )
        .expect("ranking");
        fs::write(
            run_dir.join("structure_patch_summary.tsv"),
            "candidate_id\tstructure_score\n",
        )
        .expect("structure header");
        fs::write(
            run_dir.join("structure_viewer_status.tsv"),
            "status\treason\nnot_ready\tno structure interaction match rows for this run\n",
        )
        .expect("structure status");
        fs::write(
            run_dir.join("md_summary.tsv"),
            "candidate_id\trmsd\tmd_stability_score\n",
        )
        .expect("md header");
        fs::write(
            run_dir.join("md_summary_status.tsv"),
            "status\treason\nnot_ready\tno small_molecule input available for MD summary\n",
        )
        .expect("md status");

        let payload = user_run_json(&options, "run-1");
        let stages = payload["stages"].as_array().expect("stages");
        let structure = stages
            .iter()
            .find(|stage| stage.get("id").and_then(Value::as_str) == Some("structure"))
            .expect("structure stage");
        let md = stages
            .iter()
            .find(|stage| stage.get("id").and_then(Value::as_str) == Some("md"))
            .expect("md stage");

        assert_eq!(structure["status"], "not_generated");
        assert_eq!(structure["status_label"], "未生成");
        assert_eq!(md["status"], "not_generated");
        assert_eq!(md["status_label"], "未生成");
        assert!(
            structure["reason"]
                .as_str()
                .unwrap_or_default()
                .contains("没有结构匹配")
        );
        assert!(
            md["reason"]
                .as_str()
                .unwrap_or_default()
                .contains("序列类输入")
        );

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn user_start_run_returns_user_stage_labels() {
        let root = std::env::temp_dir().join(format!("genoligand-user-start-run-{}", now_millis()));
        let options = server_options(root.clone());
        let project_id = "project-1";
        let project_dir = project_dir(&options, project_id);
        let run_dir = project_dir.join("runs").join("run-1");
        fs::create_dir_all(&run_dir).expect("run dir");
        fs::write(
            project_dir.join("input_manifest.json"),
            r#"{"inputs":[{"field":"small_molecule","value":"CCO"}]}"#,
        )
        .expect("input");
        fs::write(run_dir.join("run_state.txt"), "running\n").expect("state");
        fs::write(run_dir.join("current_stage.txt"), "pipeline_run\n").expect("stage");

        let response = json!({"run":{"run_id":"run-1","current_stage":"pipeline_run"}});
        let payload = user_start_run_response(&options, project_id, &response);
        let text = payload.to_string();

        assert_eq!(payload["ok"], true);
        assert_eq!(payload["run"]["current_stage"], "完整流程");
        assert!(payload["run"].get("state_label").is_some());
        assert!(!text.contains("pipeline_run"));
        assert!(!text.contains("current_command"));
        assert!(!text.contains("artifact_index"));

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn user_api_responses_do_not_expose_local_paths_or_commands() {
        let root = std::env::temp_dir().join(format!("genoligand-user-api-{}", now_millis()));
        let options = server_options(root.clone());
        let project_dir = projects_root(&options).join("project-1");
        let run_dir = project_dir.join("runs").join("run-1");
        fs::create_dir_all(&run_dir).expect("create run dir");
        fs::write(project_dir.join("latest_run_id.txt"), "run-1\n").expect("latest run");
        fs::write(project_dir.join("project_name.txt"), "蛋白靶点筛选\n").expect("project name");
        fs::write(
            project_dir.join("input_manifest.json"),
            r#"{"inputs":[{"field":"project_name","value":"蛋白靶点筛选"},{"field":"small_molecule","value":"CCO"}]}"#,
        )
        .expect("input");
        fs::write(run_dir.join("run_state.txt"), "completed\n").expect("run state");
        fs::write(run_dir.join("current_command.txt"), "cargo run hidden\n").expect("command");
        fs::write(
            run_dir.join("target_candidate_ranking.tsv"),
            "rank\tcandidate_id\ttarget_id\ttarget_name\tcandidate_scope\torganism\tfinal_score\tknown_database_score\tmetagenome_score\tstructure_score\tdocking_score\tmd_score\tevidence_summary\tstatus\tsha256\tdebug_path\n1\tC1\tP24941\tCDK2\tknown_nonhuman_protein\tStreptococcus pneumoniae\t0.80\t0.70\t0.00\t0.10\t0.00\t0.80\tmatched current run\tcomputed_prediction\tabc123\tD:/hidden\n",
        )
        .expect("candidate table");
        fs::write(
            run_dir.join("metagenome_potential_matches.tsv"),
            "input_id\tsource\ttarget_id\ttarget_name\torganism\ttarget_class\tmatch_score\tmatched_features\tevidence\tsha256\tdebug_path\ninput_1\tD:/hidden/metagenome_targets.tsv\tMG1\tPotential ORF\tunknown\tprotein\t0.66\tmotif\tsequence evidence\tabc123\tD:/hidden\n",
        )
        .expect("genome context");
        fs::write(
            run_dir.join("evidence_ledger.tsv"),
            format!(
                "claim_id\tevidence_artifact\towner\tstatus\tnotes\nscore_decomposition\tscore_decomposition.tsv\tGenoLigand\tavailable\t{}abc123\n",
                ["sha", "256:"].concat()
            ),
        )
        .expect("evidence");
        fs::write(
            run_dir.join("candidate_detail.json"),
            r#"{"candidate_count":1,"candidates":[{"candidate_id":"C1","target_id":"P24941","target_name":"CDK2","candidate_scope":"known_nonhuman_protein","organism":"Streptococcus pneumoniae","scores":{"final":0.8,"known_database":0.7,"metagenome":0.0,"structure":0.1,"docking":0.0,"md":0.8},"evidence_summary":"matched current run","source_artifacts":["target_candidate_ranking.tsv"],"sha256":"abc123","path":"D:/hidden"}]}"#,
        )
        .expect("candidate detail");
        fs::write(
            run_dir.join("structure_patch_summary.tsv"),
            "candidate_id\tstructure_score\tstructure_evidence\tbinding_patch\tpatch_evidence\tkey_residues\tdocking_score\tdocking_pose\tligand_position\tinteraction_residues\nC1\t0.60\tworker_structure\tpatch_internal\tworker_patch\tHIS57\t0.50\tpose_internal\tligand_internal\tHIS57\n",
        )
        .expect("structure");
        fs::write(
            run_dir.join("md_summary.tsv"),
            "candidate_id\trmsd\trmsf\tligand_drift\thydrogen_bond_occupancy\tcontact_occupancy\tinteraction_persistence\tmd_risk_flags\tmd_score_decomposition\tmd_stability_score\nC1\t1.2\t0.4\t0.6\t0.72\t0.81\t0.77\tstable_internal\tscore_internal\t0.8\n",
        )
        .expect("md");
        fs::write(run_dir.join("run_context.json"), r#"{"path":"hidden"}"#).expect("context");
        fs::write(run_dir.join("artifact_source_map.tsv"), "hidden\n").expect("source trace");
        fs::write(run_dir.join("research_report.md"), "# Report\n").expect("report");

        let run = run_json(&options, "run-1");
        assert_eq!(run["trace"]["project_label"], "蛋白靶点筛选");
        assert_eq!(run["trace"]["input_status"], "已保存用户输入");
        assert_eq!(run["trace"]["candidate_count"], 1);
        assert_eq!(run["trace"]["evidence_count"], 1);

        let candidate_detail = candidate_detail_json(&options, "run-1");
        let candidates = candidates_endpoint_json(&options, "run-1");
        assert_eq!(
            candidate_detail["candidate_details"][0]["candidate_scope_label"],
            "已知非人源蛋白数据库"
        );
        assert_eq!(
            candidates["candidates"][0]["candidate_scope_label"],
            "已知非人源蛋白数据库"
        );
        assert_eq!(
            candidates["candidates"][0]["md_support_label"],
            "有短程 MD 支持"
        );
        assert!(
            candidates["candidates"][0]["risk_note"]
                .as_str()
                .unwrap_or_default()
                .contains("短程受限 receptor 模型")
        );

        let responses = [
            read_project(&options, "project-1"),
            run,
            artifacts_json(&options, "run-1"),
            user_databases_json(&options),
            user_setup_status_json(&options),
            user_setup_install_plan_json(&options),
            preflight_json(&options, "project-1"),
            logs_json(&options, "run-1"),
            candidate_detail,
            evidence_endpoint_json(&options, "run-1"),
            candidates,
            genome_endpoint_json(&options, "run-1"),
            structure_endpoint_json(&options, "run-1"),
            md_endpoint_json(&options, "run-1"),
        ];
        for response in responses {
            let text = response.to_string();
            assert!(!text.contains("workspace_path"));
            assert!(!text.contains("output_dir"));
            assert!(!text.contains("current_command"));
            assert!(!text.contains("failure_reason"));
            assert!(!text.contains("artifact_root"));
            assert!(!text.contains("artifact_backed"));
            assert!(!text.contains("artifact_index"));
            assert!(!text.contains("source_path"));
            assert!(!text.contains("cargo run hidden"));
            assert!(!text.contains("D:/hidden"));
            assert!(!text.contains("debug_path"));
            assert!(!text.contains("matched_features"));
            assert!(!text.contains(&root.display().to_string()));
            assert!(!text.contains("input_manifest"));
            assert!(!text.contains("science_store_manifest"));
            assert!(!text.contains("tool_versions_manifest"));
            assert!(!text.contains("indexes/known_targets.tsv"));
            assert!(!text.contains("indexes/metagenome_targets.tsv"));
            assert!(!text.contains("indexes/structure_targets.tsv"));
            assert!(!text.contains("evidence_artifact"));
            assert!(!text.contains("sha256"));
            assert!(!text.contains("abc123"));
            assert!(!text.contains("binding_patch"));
            assert!(!text.contains("patch_internal"));
            assert!(!text.contains("docking_pose"));
            assert!(!text.contains("pose_internal"));
            assert!(!text.contains("ligand_position"));
            assert!(!text.contains("ligand_internal"));
            assert!(!text.contains("md_risk_flags"));
            assert!(!text.contains("stable_internal"));
            assert!(!text.contains("md_score_decomposition"));
            assert!(!text.contains("score_internal"));
            assert!(!text.contains("prepare_science_store"));
            assert!(!text.contains("download_and_index"));
            assert!(!text.contains("install_or_repair"));
        }

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn user_artifact_index_hides_internal_trace_files() {
        let root = std::env::temp_dir().join(format!("genoligand-user-artifacts-{}", now_millis()));
        let options = server_options(root.clone());
        let project_dir = projects_root(&options).join("project-1");
        let run_dir = project_dir.join("runs").join("run-1");
        fs::create_dir_all(&run_dir).expect("create run dir");
        fs::write(run_dir.join("run_state.txt"), "completed\n").expect("state");
        fs::write(run_dir.join("run_context.json"), r#"{"internal":true}"#).expect("context");
        fs::write(run_dir.join("run_context.tsv"), "field\tvalue\n").expect("context table");
        fs::write(
            run_dir.join("artifact_source_map.tsv"),
            "artifact\tupstream\n",
        )
        .expect("source map");
        fs::write(run_dir.join("research_report.md"), "# Report\n").expect("report");

        let artifacts = artifacts_json(&options, "run-1");
        let text = artifacts.to_string();
        assert!(!text.contains("run_summary"));
        assert!(!text.contains("run_table"));
        assert!(!text.contains("source_trace"));
        assert!(!text.contains("run_context"));
        assert!(!text.contains("artifact_source_map"));
        assert!(!text.contains("normalized_input"));
        assert!(!text.contains("features"));
        assert!(!text.contains("score_breakdown"));
        assert!(!text.contains("candidate_detail"));

        let (status, _, _) = download_artifact_response(&options, "run-1", "run_summary");
        assert_eq!(status, 404);
        let (status, _, _) = download_artifact_response(&options, "run-1", "source_trace");
        assert_eq!(status, 404);

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn user_downloads_are_sanitized_exports() {
        let root = std::env::temp_dir().join(format!("genoligand-user-downloads-{}", now_millis()));
        let options = server_options(root.clone());
        let run_dir = projects_root(&options)
            .join("project-1")
            .join("runs")
            .join("run-1");
        fs::create_dir_all(&run_dir).expect("create run dir");
        fs::write(run_dir.join("run_state.txt"), "completed\n").expect("state");
        fs::write(
            run_dir.join("target_candidate_ranking.tsv"),
            "rank\tcandidate_id\ttarget_id\ttarget_name\tcandidate_scope\torganism\tfinal_score\tknown_database_score\tmetagenome_score\tstructure_score\tdocking_score\tmd_score\tevidence_summary\tstatus\tsha256\tdebug_path\n1\tC1\tP24941\tCDK2\tknown_database\tHomo sapiens\t0.80\t0.70\t0.00\t0.10\t0.00\t0.00\tmatched current run\tcomputed_prediction\tabc123\tD:/hidden\n",
        )
        .expect("candidate table");
        fs::write(
            run_dir.join("evidence_ledger.tsv"),
            format!(
                "claim_id\tevidence_artifact\towner\tstatus\tnotes\nscore_decomposition\tscore_decomposition.tsv\tGenoLigand\tavailable\t{}abc123\n",
                ["sha", "256:"].concat()
            ),
        )
        .expect("evidence");
        fs::write(
            run_dir.join("structure_patch_summary.tsv"),
            "candidate_id\tstructure_score\tstructure_evidence\tbinding_patch\tpatch_evidence\tkey_residues\tdocking_score\tdocking_pose\tligand_position\tinteraction_residues\nC1\t0.60\tworker_structure\tpatch_internal\tworker_patch\tHIS57\t0.50\tpose_internal\tligand_internal\tHIS57\n",
        )
        .expect("structure");
        fs::write(
            run_dir.join("md_summary.tsv"),
            "candidate_id\trmsd\trmsf\tligand_drift\thydrogen_bond_occupancy\tcontact_occupancy\tinteraction_persistence\tmd_risk_flags\tmd_score_decomposition\tmd_stability_score\nC1\t1.2\t0.4\t0.6\t0.72\t0.81\t0.77\tstable_internal\tscore_internal\t0.8\n",
        )
        .expect("md");

        for artifact_id in [
            "candidate_ranking",
            "evidence",
            "structure_patch",
            "md_summary",
        ] {
            let (status, content_type, body) =
                download_artifact_response(&options, "run-1", artifact_id);
            assert_eq!(status, 200);
            assert_eq!(content_type, "text/tab-separated-values; charset=utf-8");
            assert!(body.contains("输出材料"));
            assert!(!body.contains("sha256"));
            assert!(!body.contains("debug_path"));
            assert!(!body.contains("D:/hidden"));
            assert!(!body.contains("binding_patch"));
            assert!(!body.contains("patch_internal"));
            assert!(!body.contains("md_score_decomposition"));
            assert!(!body.contains("score_internal"));
        }

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn candidate_detail_endpoint_uses_detail_artifact_without_internal_fields() {
        let root =
            std::env::temp_dir().join(format!("genoligand-candidate-detail-{}", now_millis()));
        let options = server_options(root.clone());
        let run_dir = projects_root(&options)
            .join("project-1")
            .join("runs")
            .join("run-1");
        fs::create_dir_all(&run_dir).expect("create run dir");
        fs::write(run_dir.join("run_state.txt"), "completed\n").expect("state");
        fs::write(
            run_dir.join("candidate_detail.json"),
            r#"{"candidate_count":1,"project_id":"project-1","run_id":"run-1","candidates":[{"candidate_id":"C1","target_id":"P24941","target_name":"CDK2","candidate_scope":"known_database","organism":"Homo sapiens","scores":{"final":0.8,"known_database":0.7,"metagenome":0.0,"structure":0.1,"docking":0.0,"md":0.0},"evidence_summary":"matched current run","source_artifacts":["target_candidate_ranking.tsv","evidence_ledger.tsv"],"sha256":"abc123","path":"D:/hidden"}]}"#,
        )
        .expect("candidate detail");

        let payload = candidate_detail_json(&options, "run-1");
        assert_eq!(payload["candidate_count"], 1);
        assert_eq!(payload["candidate_details"][0]["candidate_id"], "C1");
        assert_eq!(payload["candidate_details"][0]["target_id"], "P24941");
        assert_eq!(
            payload["candidate_details"][0]["final_score"]
                .as_f64()
                .unwrap(),
            0.8
        );
        assert_eq!(
            payload["candidate_details"][0]["detail_source"],
            "当前运行的候选详情输出"
        );
        assert_eq!(
            payload["candidate_details"][0]["result_source"],
            "候选详情输出"
        );
        let text = payload.to_string();
        assert!(!text.contains("source_artifacts"));
        assert!(!text.contains("target_candidate_ranking.tsv"));
        assert!(!text.contains("evidence_ledger.tsv"));
        assert!(!text.contains("sha256"));
        assert!(!text.contains("D:/hidden"));

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn structure_endpoint_exposes_viewer_coordinates_only_from_completed_run_artifact() {
        let root = std::env::temp_dir().join(format!("genoligand-viewer-{}", now_millis()));
        let options = server_options(root.clone());
        let run_dir = projects_root(&options)
            .join("project-1")
            .join("runs")
            .join("run-1");
        fs::create_dir_all(&run_dir).expect("create run dir");
        fs::write(run_dir.join("run_state.txt"), "completed\n").expect("state");
        fs::write(
            run_dir.join("structure_patch_summary.tsv"),
            "candidate_id\tstructure_score\tstructure_evidence\nC1\t0.60\tcurrent_run_structure\n",
        )
        .expect("structure");
        fs::write(
            run_dir.join("structure_viewer.pdb"),
            "ATOM      1  N   GLY A   1      11.104  13.207   2.100  1.00 20.00           N\nATOM      2  CA  GLY A   1      12.456  13.607   2.500  1.00 20.00           C\n",
        )
        .expect("viewer pdb");

        let payload = structure_endpoint_json(&options, "run-1");
        assert_eq!(payload["has_output"], true);
        assert_eq!(payload["viewer"]["ready"], true);
        assert_eq!(payload["viewer"]["atom_count"], 2);
        assert_eq!(payload["viewer"]["artifact"], "structure_viewer.pdb");
        assert_eq!(
            payload["structure_rows"][0]["viewer_status"],
            "三维查看已就绪"
        );
        let text = payload.to_string();
        assert!(!text.contains(&root.display().to_string()));
        assert!(!text.contains("debug_path"));

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn structure_endpoint_reports_missing_viewer_reason() {
        let root = std::env::temp_dir().join(format!("genoligand-viewer-reason-{}", now_millis()));
        let options = server_options(root.clone());
        let run_dir = projects_root(&options)
            .join("project-1")
            .join("runs")
            .join("run-1");
        fs::create_dir_all(&run_dir).expect("create run dir");
        fs::write(run_dir.join("run_state.txt"), "completed\n").expect("state");
        fs::write(
            run_dir.join("structure_patch_summary.tsv"),
            "candidate_id\tstructure_score\tstructure_evidence\nC1\t0.60\tcurrent_run_structure\n",
        )
        .expect("structure");
        fs::write(
            run_dir.join("structure_viewer_status.tsv"),
            "status\ttarget_id\tpdb_id\tartifact\texpected_path\treason\nnot_ready\t10GS\t10GS\t\tdownloads/rcsb/mmcif/0g/10gs.cif.gz\tmatched structure rows did not have a local coordinate artifact\n",
        )
        .expect("viewer status");

        let payload = structure_endpoint_json(&options, "run-1");

        assert_eq!(payload["viewer"]["ready"], false);
        assert_eq!(
            payload["structure_rows"][0]["viewer_status"],
            "三维查看未就绪"
        );
        assert!(
            payload["viewer"]["reason"]
                .as_str()
                .unwrap_or_default()
                .contains("local coordinate artifact")
        );
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn md_endpoint_builds_chart_rows_from_current_run_summaries() {
        let root = std::env::temp_dir().join(format!("genoligand-md-chart-{}", now_millis()));
        let options = server_options(root.clone());
        let run_dir = projects_root(&options)
            .join("project-1")
            .join("runs")
            .join("run-1");
        fs::create_dir_all(&run_dir).expect("create run dir");
        fs::write(run_dir.join("run_state.txt"), "completed\n").expect("state");
        fs::write(
            run_dir.join("md_summary.tsv"),
            "candidate_id\trmsd\trmsf\tligand_drift\thydrogen_bond_occupancy\tcontact_occupancy\nC1\t1.2\t0.4\t0.6\t0.72\t0.81\n",
        )
        .expect("md");
        fs::write(
            run_dir.join("score_decomposition.tsv"),
            "candidate_id\tcomponent\tscore\nC1\tstructure\t0.5\n",
        )
        .expect("score");

        let payload = md_endpoint_json(&options, "run-1");
        assert_eq!(payload["has_output"], true);
        assert_eq!(payload["readiness"]["ready"], true);
        assert!(payload["charts"].as_array().unwrap().len() >= 5);
        let text = payload.to_string();
        assert!(text.contains("md_summary.tsv"));
        assert!(text.contains("score_decomposition.tsv"));
        assert!(!text.contains(&root.display().to_string()));

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn md_endpoint_reports_missing_md_reason() {
        let root = std::env::temp_dir().join(format!("genoligand-md-reason-{}", now_millis()));
        let options = server_options(root.clone());
        let run_dir = projects_root(&options)
            .join("project-1")
            .join("runs")
            .join("run-1");
        fs::create_dir_all(&run_dir).expect("create run dir");
        fs::write(run_dir.join("run_state.txt"), "completed\n").expect("state");
        fs::write(run_dir.join("md_summary.tsv"), "candidate_id\trmsd\n").expect("md");
        fs::write(
            run_dir.join("md_summary_status.tsv"),
            "status\treason\nnot_ready\tno RMSD metric artifact;no trajectory artifact\n",
        )
        .expect("md status");

        let payload = md_endpoint_json(&options, "run-1");

        assert_eq!(payload["has_output"], false);
        assert_eq!(payload["readiness"]["ready"], false);
        assert_eq!(payload["readiness"]["chart_ready"], false);
        assert!(payload["charts"].as_array().unwrap().is_empty());
        assert!(
            payload["readiness"]["reason"]
                .as_str()
                .unwrap_or_default()
                .contains("RMSD")
        );
        fs::remove_dir_all(&root).ok();
    }
}
