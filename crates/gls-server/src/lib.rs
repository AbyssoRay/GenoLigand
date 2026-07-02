use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

mod input_recognition;

use input_recognition::{input_recognition_summary, input_value_has_usable_user_input};

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
            "message":"无法转换为可计算结构/序列，请检查输入。",
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
    let input_ready = input.exists() && input_has_user_value(&input);
    let input_detail = if input_ready {
        input_recognition_summary(&read_json_or(&input, json!({})))
            .unwrap_or_else(|| "已识别为可计算输入".to_string())
    } else {
        "无法转换为可计算结构/序列".to_string()
    };
    let checks = vec![
        check_with_detail("input", input_ready, input_detail),
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
                let mut item = json!({
                    "id": row.get("id").cloned().unwrap_or(Value::Null),
                    "ready": row.get("ready").cloned().unwrap_or(Value::Bool(false))
                });
                if let Some(detail) = row.get("detail").and_then(Value::as_str) {
                    item["detail"] = json!(detail);
                }
                item
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
    let _ = write_text(&run_dir.join("current_stage.txt"), "input_normalize\n");
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
    let _ = write_text(
        &run_dir.join("run.log"),
        &format!("command\t{command_text}\nstate\trunning\n"),
    );
    let _ = write_text(&project.join("latest_run_id.txt"), &run_id);
    let background_options = options.clone();
    let background_project = project.clone();
    let background_run_dir = run_dir.clone();
    let background_command = command.clone();
    let background_command_text = command_text.clone();
    match std::thread::Builder::new()
        .name(format!("genoligand-run-{run_id}"))
        .spawn(move || {
            finish_run_command(
                &background_options,
                &background_project,
                &background_run_dir,
                &background_command,
                &background_command_text,
            );
        }) {
        Ok(_) => json!({"status":"running","run":run_json(options,&run_id)}),
        Err(err) => {
            let reason = format!("pipeline_thread_launch_failed:{err}");
            let _ = write_text(&run_dir.join("run_state.txt"), "failed\n");
            let _ = write_text(
                &run_dir.join("failure_reason.txt"),
                &(reason.clone() + "\n"),
            );
            let _ = write_text(
                &run_dir.join("run.log"),
                &format!("command\t{command_text}\nstate\tfailed\nstderr\n{reason}\n"),
            );
            json!({"status":"failed","run":run_json(options,&run_id)})
        }
    }
}

fn finish_run_command(
    options: &ServerOptions,
    project: &Path,
    run_dir: &Path,
    command: &[String],
    command_text: &str,
) {
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
    let run_id = run_dir
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    let _ = write_text(
        &project.join("latest_run_id.txt"),
        &(run_id.to_string() + "\n"),
    );
    let artifact_index = artifact_index(run_dir);
    let _ = write_text(
        &run_dir.join("artifact_index.json"),
        &(artifact_index.to_string() + "\n"),
    );
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
    let current_index = specs
        .iter()
        .position(|(_, _, stage_key)| *stage_key == current);
    specs
        .iter()
        .enumerate()
        .map(|(index, (id, label, stage_key))| {
            let (status, reason) = user_stage_status(
                run_dir,
                state,
                id,
                stage_key,
                &current,
                index,
                current_index,
            );
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
    stage_index: usize,
    current_index: Option<usize>,
) -> (&'a str, String) {
    if state == "completed" {
        return completed_stage_status(run_dir, id);
    }
    if state == "running" {
        if let Some(current_index) = current_index {
            if stage_index < current_index {
                return ("ready", String::new());
            }
            if current == stage_key {
                return ("running", String::new());
            }
        } else if current == "pipeline_run" && stage_index == 0 {
            return ("running", String::new());
        }
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

fn check_with_detail(id: &str, ready: bool, detail: String) -> Value {
    json!({"id":id,"ready":ready,"status":if ready {"ready"} else {"blocked"},"detail":detail})
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
mod tests;
