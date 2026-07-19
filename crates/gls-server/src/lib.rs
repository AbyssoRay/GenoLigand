use gls_science::{
    InputInspection, ScienceOptions, evaluate_science_store_readiness, inspect_input_manifest,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, hash_map::RandomState};
use std::fs::{self, OpenOptions};
use std::hash::{BuildHasher, Hasher};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const PRODUCT_NAME: &str = "GenoLigand Local WebUI";
pub const PRODUCT_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const PRODUCT_COMMIT: &str = env!("GENOLIGAND_BUILD_COMMIT");
const RESULT_PAGE_SIZE: usize = 500;
const RUN_LOG_MAX_BYTES: usize = 1024 * 1024;
const RUN_LOG_READ_BYTES: usize = 128 * 1024;
const RUN_LOG_READ_LINES: usize = 500;
const PROCESS_POLL_INTERVAL: Duration = Duration::from_millis(50);
const INPUT_CAPABILITIES_JSON: &str = include_str!("../../../schemas/input_capabilities.json");
static INPUT_INSPECTION_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static UUID_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static SERVER_REGISTRY: OnceLock<Mutex<ServerRegistry>> = OnceLock::new();

#[derive(Default)]
struct ServerRegistry {
    project_locks: HashMap<PathBuf, Arc<Mutex<()>>>,
    run_state_locks: HashMap<PathBuf, Arc<Mutex<()>>>,
    runs: HashMap<PathBuf, Arc<RegisteredRun>>,
}

struct RegisteredRun {
    project_dir: PathBuf,
    process_id: u32,
    child: Mutex<Child>,
    log_lock: Arc<Mutex<()>>,
    monitor_finished: AtomicBool,
}

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
    let path = parts[1];
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
    let (path, query) = split_path_query(path);
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
    if let Some(response) = route_user_run(method, path, query, options) {
        return response;
    }
    if let Some(response) = route_project(method, path, options, request_body) {
        return response;
    }
    if let Some(response) = route_run(method, path, query, options) {
        return response;
    }
    let value = match (method, path) {
        ("GET", "/api/health") => user_health_json(options),
        ("GET", "/api/user/health") => user_health_json(options),
        ("GET", "/api/version") => {
            json!({"name":PRODUCT_NAME,"version":PRODUCT_VERSION,"science_flow":"scientific_pipeline"})
        }
        ("GET", "/api/user/projects") => user_projects_json(options),
        ("GET", "/api/user/workspace-summary") => user_workspace_summary_json(options),
        ("POST", "/api/user/projects") => create_user_project(options),
        ("GET", "/api/user/inputs/capabilities") => user_input_capabilities_json(),
        ("POST", "/api/user/inputs/inspect") => inspect_user_input_json(options, request_body),
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

fn split_path_query(raw: &str) -> (&str, &str) {
    raw.split_once('?').unwrap_or((raw, ""))
}

#[derive(Clone, Debug)]
struct ResultQuery {
    page: usize,
    page_size: usize,
    filter: String,
}

impl Default for ResultQuery {
    fn default() -> Self {
        Self {
            page: 1,
            page_size: RESULT_PAGE_SIZE,
            filter: String::new(),
        }
    }
}

impl ResultQuery {
    fn all() -> Self {
        Self {
            page: 1,
            page_size: usize::MAX,
            filter: String::new(),
        }
    }

    fn parse(query: &str) -> Self {
        let mut parsed = Self::default();
        for pair in query.split('&').filter(|part| !part.is_empty()) {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            let decoded = percent_decode(value);
            match key {
                "page" => {
                    parsed.page = decoded
                        .parse::<usize>()
                        .ok()
                        .filter(|value| *value > 0)
                        .unwrap_or(1);
                }
                "page_size" => {
                    parsed.page_size = decoded
                        .parse::<usize>()
                        .ok()
                        .map(|value| value.clamp(1, 5000))
                        .unwrap_or(RESULT_PAGE_SIZE);
                }
                "filter" => parsed.filter = decoded.trim().to_string(),
                _ => {}
            }
        }
        parsed
    }

    fn offset(&self) -> usize {
        self.page.saturating_sub(1).saturating_mul(self.page_size)
    }

    fn filters_active(&self) -> bool {
        !self.filter.trim().is_empty()
    }
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                out.push(b' ');
                index += 1;
            }
            b'%' if index + 2 < bytes.len() => {
                let hex = &value[index + 1..index + 3];
                if let Ok(decoded) = u8::from_str_radix(hex, 16) {
                    out.push(decoded);
                    index += 3;
                } else {
                    out.push(bytes[index]);
                    index += 1;
                }
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).to_string()
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
        "pages/project.js" => include_str!("../../../webui/app/pages/project.js"),
        "pages/input.js" => include_str!("../../../webui/app/pages/input.js"),
        "pages/check.js" => include_str!("../../../webui/app/pages/check.js"),
        "pages/run.js" => include_str!("../../../webui/app/pages/run.js"),
        "pages/results.js" => include_str!("../../../webui/app/pages/results.js"),
        "pages/report.js" => include_str!("../../../webui/app/pages/report.js"),
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
        ("GET", "inputs") => read_input_manifest(options, &project_id),
        ("POST", "inputs") => write_user_input_manifest(options, &project_id, request_body),
        ("GET", "preflight") | ("POST", "preflight") => user_preflight_json(options, &project_id),
        ("POST", "runs") => start_user_run(options, &project_id),
        ("DELETE", "") => delete_project(options, &project_id),
        _ => json!({"message":"未找到工作台操作"}),
    };
    Some((200, "application/json; charset=utf-8", value.to_string()))
}

fn route_run(
    method: &str,
    path: &str,
    query: &str,
    options: &ServerOptions,
) -> Option<(u16, &'static str, String)> {
    let rest = path.strip_prefix("/api/runs/")?;
    let mut parts = rest.split('/');
    let run_id = safe_id(parts.next().unwrap_or(""));
    let suffix = parts.collect::<Vec<_>>().join("/");
    let result_query = ResultQuery::parse(query);
    let value = match (method, suffix.as_str()) {
        ("GET", "") => run_json(options, &run_id),
        ("GET", "logs") => logs_json(options, &run_id),
        ("GET", "artifacts") => artifacts_json(options, &run_id),
        ("GET", "candidates") => {
            candidates_endpoint_json_with_query(options, &run_id, &result_query)
        }
        ("GET", "candidate-detail") => {
            candidate_detail_json_with_query(options, &run_id, &result_query)
        }
        ("GET", "evidence") => evidence_endpoint_json_with_query(options, &run_id, &result_query),
        ("GET", "genome-context") => {
            genome_endpoint_json_with_query(options, &run_id, &result_query)
        }
        ("GET", "structure") => structure_endpoint_json_with_query(options, &run_id, &result_query),
        ("GET", "md-summary") => md_endpoint_json_with_query(options, &run_id, &result_query),
        ("POST", "cancel") => cancel_run(options, &run_id),
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
    query: &str,
    options: &ServerOptions,
) -> Option<(u16, &'static str, String)> {
    let rest = path.strip_prefix("/api/user/runs/")?;
    let mut parts = rest.split('/');
    let run_id = safe_id(parts.next().unwrap_or(""));
    let suffix = parts.collect::<Vec<_>>().join("/");
    let result_query = ResultQuery::parse(query);
    let value = match (method, suffix.as_str()) {
        ("GET", "") => user_run_json(options, &run_id),
        ("GET", "logs") => logs_json(options, &run_id),
        ("GET", "artifacts") => artifacts_json(options, &run_id),
        ("GET", "candidates") => {
            candidates_endpoint_json_with_query(options, &run_id, &result_query)
        }
        ("GET", "candidate-detail") => {
            candidate_detail_json_with_query(options, &run_id, &result_query)
        }
        ("GET", "evidence") => evidence_endpoint_json_with_query(options, &run_id, &result_query),
        ("GET", "genome-context") => {
            genome_endpoint_json_with_query(options, &run_id, &result_query)
        }
        ("GET", "structure") => structure_endpoint_json_with_query(options, &run_id, &result_query),
        ("GET", "md-summary") => md_endpoint_json_with_query(options, &run_id, &result_query),
        ("POST", "cancel") => cancel_run(options, &run_id),
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
                    if run_state(&run_dir) == "completed"
                        && authenticity_gate_allows_results(&run_dir)
                    {
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
    let root = projects_root(options);
    fs::create_dir_all(&root).map_err(|err| err.to_string())?;
    for _ in 0..8 {
        let project_id = format!("project-{}", new_uuid());
        let dir = root.join(&project_id);
        match fs::create_dir(&dir) {
            Ok(()) => {
                if let Err(err) = fs::create_dir(dir.join("runs")) {
                    let _ = fs::remove_dir(&dir);
                    return Err(err.to_string());
                }
                return Ok(read_project(options, &project_id));
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err.to_string()),
        }
    }
    Err("project_id_allocation_failed".to_string())
}

fn project_visible_to_user(options: &ServerOptions, project_id: &str) -> bool {
    project_dir(options, project_id).is_dir()
}

fn user_project_summary(project: &Value) -> Value {
    let status = project
        .get("project_status")
        .and_then(Value::as_str)
        .unwrap_or_default();
    json!({
        "project_id": project.get("project_id").cloned().unwrap_or(Value::Null),
        "project_name": project.get("project_name").cloned().unwrap_or(Value::Null),
        "project_status": status,
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
        "input_ready" => "输入已就绪",
        "draft" => "等待输入",
        _ => "等待输入",
    }
}

fn read_project(options: &ServerOptions, project_id: &str) -> Value {
    let dir = project_dir(options, project_id);
    let latest_run_id = read_trimmed(&dir.join("latest_run_id.txt"));
    let status = latest_run_id
        .as_deref()
        .map(|run_id| run_state(&dir.join("runs").join(run_id)))
        .unwrap_or_else(|| {
            if input_has_user_value(options, &dir.join("input_manifest.json")) {
                "input_ready".to_string()
            } else {
                "draft".to_string()
            }
        });
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

fn user_input_capabilities_json() -> Value {
    serde_json::from_str(INPUT_CAPABILITIES_JSON)
        .expect("schemas/input_capabilities.json must contain valid JSON")
}

fn inspect_user_input_json(options: &ServerOptions, body: &str) -> Value {
    inspect_user_input_json_with_cleanup(options, body, remove_pending_file)
}

fn inspect_user_input_json_with_cleanup<C>(
    options: &ServerOptions,
    body: &str,
    mut cleanup: C,
) -> Value
where
    C: FnMut(&Path) -> Result<(), String>,
{
    let inspection_dir = options.artifact_dir.join("input-inspection");
    let sequence = INPUT_INSPECTION_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let pending = inspection_dir.join(format!(
        "inspection-{}-{sequence}.pending.json",
        std::process::id()
    ));
    if let Err(err) = write_text(&pending, body) {
        let failure = failed_inspection_json(err);
        return inspection_after_cleanup(failure, &pending, &mut cleanup);
    }
    let response = match inspect_manifest(options, &pending) {
        Ok(inspection) => inspection_json(&inspection),
        Err(err) => failed_inspection_json(err),
    };
    inspection_after_cleanup(response, &pending, &mut cleanup)
}

fn inspection_after_cleanup<C>(response: Value, pending: &Path, cleanup: &mut C) -> Value
where
    C: FnMut(&Path) -> Result<(), String>,
{
    match cleanup(pending) {
        Ok(()) => response,
        Err(err) => cleanup_failure_inspection(response, err),
    }
}

fn inspect_manifest(options: &ServerOptions, path: &Path) -> Result<InputInspection, String> {
    inspect_input_manifest(&ScienceOptions {
        workspace_root: options.workspace_root.clone(),
        science_store: science_store_path(options),
        input_manifest: path.to_path_buf(),
        run_dir: path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| options.artifact_dir.clone()),
    })
}

fn inspection_json(inspection: &InputInspection) -> Value {
    let mut response = json!({
        "usable": inspection.usable,
        "summary": inspection.summary,
        "errors": inspection.errors,
        "inputs": inspection.inputs.iter().map(|input| json!({
            "input_id": input.input_id,
            "record_id": input.record_id,
            "original_field": input.original_field,
            "canonical_kind": input.canonical_kind,
            "conversion_method": input.conversion_method,
            "usable": input.usable,
            "error": input.error,
            "message": input.error.as_deref().map(|error| input_error_message(resolver_error_code(error)))
        })).collect::<Vec<_>>()
    });
    if !inspection.usable {
        let error = inspection_error_code(inspection);
        response["error"] = json!(error);
        response["message"] = json!(input_error_message(error));
    }
    response
}

fn inspection_error_code(inspection: &InputInspection) -> &str {
    let Some(error) = inspection.errors.first() else {
        return "input_not_usable";
    };
    let matched_reason = inspection
        .inputs
        .iter()
        .filter_map(|input| {
            let record_id = input.record_id.replace(['\t', '\r', '\n'], " ");
            let prefix = format!("{}:{record_id}:", input.input_id);
            error
                .strip_prefix(&prefix)
                .map(|reason| (prefix.len(), reason))
        })
        .max_by_key(|(prefix_len, _)| *prefix_len)
        .map(|(_, reason)| reason);
    if let Some(reason) = matched_reason {
        return resolver_error_code(reason);
    }
    resolver_error_code(error)
}

fn failed_inspection_json(error: String) -> Value {
    let code = resolver_error_code(&error);
    json!({
        "usable": false,
        "summary": "输入检查失败",
        "errors": [error],
        "inputs": [],
        "error": code,
        "message": input_error_message(code)
    })
}

fn resolver_error_code(error: &str) -> &str {
    let mut parts = error.split(':');
    let first = parts.next().unwrap_or("input_not_usable");
    let is_input_id = first
        .strip_prefix("input_")
        .is_some_and(|suffix| !suffix.is_empty() && suffix.chars().all(|ch| ch.is_ascii_digit()));
    if is_input_id {
        let _ = parts.next();
        parts.next().unwrap_or("input_not_usable")
    } else {
        first
    }
}

fn input_error_message(code: &str) -> &'static str {
    match code {
        "invalid_input_manifest_json" => "输入清单不是有效的 JSON，请检查后重试。",
        "invalid_input_manifest_schema" => "输入清单结构无效，请检查后重试。",
        "input_file_not_found" => "找不到输入文件，请检查文件路径。",
        "input_file_requires_reference_sequence" => "该输入文件还需要配套参考序列。",
        "unknown_compound_identifier" => "本地科学仓库中没有对应的化合物名称或标识符。",
        "input_manifest_replace_failed" => "无法发布新的输入清单，请检查项目目录权限。",
        "input_manifest_cleanup_failed" => "无法清理待发布的输入清单，请检查项目目录权限。",
        "project_run_active" => "项目正在运行，请先取消或等待当前运行结束。",
        "input_not_usable" => "请至少提供一项可转换为计算输入的内容。",
        _ => "无法转换为可计算结构或序列，请检查输入。",
    }
}

fn successful_inspection_message(inspection: &InputInspection) -> String {
    let mut labels = Vec::new();
    for input in &inspection.inputs {
        let label = match input.canonical_kind.as_str() {
            "small_molecule" => "小分子",
            "peptide" => "多肽",
            "protein_ligand" => "蛋白序列",
            "complex" => "复合物",
            "genome_or_metagenome" => "核酸或宏基因组序列",
            _ => "可计算输入",
        };
        if !labels.contains(&label) {
            labels.push(label);
        }
    }
    format!("已识别为{}", labels.join("、"))
}

fn remove_pending_file(path: &Path) -> Result<(), String> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(format!(
            "input_manifest_cleanup_failed:{}:{err}",
            path.display()
        )),
    }
}

fn write_user_input_manifest(options: &ServerOptions, project_id: &str, body: &str) -> Value {
    let response = write_input_manifest(options, project_id, body);
    let ok = response.get("status").and_then(Value::as_str) == Some("saved");
    let project = response
        .get("project")
        .map(user_project_summary)
        .unwrap_or_else(|| user_project_detail(options, project_id));
    let mut user_response = json!({
        "ok": ok,
        "message": if ok {"输入已保存"} else {response.get("message").and_then(Value::as_str).unwrap_or("输入未保存，请检查填写内容")},
        "project": project
    });
    if let Some(error) = response.get("error") {
        user_response["error"] = error.clone();
    }
    if let Some(inspection) = response.get("inspection") {
        user_response["inspection"] = inspection.clone();
    }
    user_response
}

fn write_input_manifest(options: &ServerOptions, project_id: &str, body: &str) -> Value {
    write_input_manifest_with_promoter(options, project_id, body, promote_input_manifest)
}

fn write_input_manifest_with_promoter<F>(
    options: &ServerOptions,
    project_id: &str,
    body: &str,
    promoter: F,
) -> Value
where
    F: FnOnce(&Path, &Path) -> Result<(), String>,
{
    write_input_manifest_with_operations(options, project_id, body, promoter, remove_pending_file)
}

fn write_input_manifest_with_operations<F, C>(
    options: &ServerOptions,
    project_id: &str,
    body: &str,
    promoter: F,
    mut cleanup: C,
) -> Value
where
    F: FnOnce(&Path, &Path) -> Result<(), String>,
    C: FnMut(&Path) -> Result<(), String>,
{
    let dir = project_dir(options, project_id);
    let project_lock = project_operation_lock(&dir);
    let _project_guard = lock_unpoisoned(&project_lock);
    if active_project_run_id(&dir).is_some() {
        return json!({
            "status": "failed",
            "error": "project_run_active",
            "message": input_error_message("project_run_active"),
            "project": read_project(options, project_id)
        });
    }
    let pending = dir.join("input_manifest.pending.json");
    let formal = dir.join("input_manifest.json");
    let value = match serde_json::from_str::<Value>(body.trim_start_matches('\u{feff}')) {
        Ok(value) => value,
        Err(err) => {
            let inspection = failed_inspection_json(format!("invalid_input_manifest_json:{err}"));
            return failed_save_after_cleanup(
                options,
                project_id,
                &pending,
                inspection,
                &mut cleanup,
            );
        }
    };
    if let Err(err) = write_text(&pending, body) {
        return failed_save_after_cleanup(
            options,
            project_id,
            &pending,
            failed_inspection_json(err),
            &mut cleanup,
        );
    }
    let inspection = match inspect_manifest(options, &pending) {
        Ok(inspection) => inspection_json(&inspection),
        Err(err) => failed_inspection_json(err),
    };
    if inspection.get("usable").and_then(Value::as_bool) != Some(true) {
        return failed_save_after_cleanup(options, project_id, &pending, inspection, &mut cleanup);
    }
    if let Err(err) = promoter(&pending, &formal) {
        return failed_save_after_cleanup(
            options,
            project_id,
            &pending,
            operation_failure_inspection(inspection, err),
            &mut cleanup,
        );
    }
    if let Some(name) = input_project_name(&value) {
        let _ = write_text(&dir.join("project_name.txt"), &name);
    }
    clear_current_run_selection(&dir);
    json!({
        "status": "saved",
        "inspection": inspection,
        "project": read_project(options, project_id)
    })
}

fn failed_save_after_cleanup<C>(
    options: &ServerOptions,
    project_id: &str,
    pending: &Path,
    inspection: Value,
    cleanup: &mut C,
) -> Value
where
    C: FnMut(&Path) -> Result<(), String>,
{
    let inspection = match cleanup(pending) {
        Ok(()) => inspection,
        Err(err) => cleanup_failure_inspection(inspection, err),
    };
    failed_input_save(options, project_id, inspection)
}

fn cleanup_failure_inspection(cause: Value, cleanup_error: String) -> Value {
    let cleanup_error = if resolver_error_code(&cleanup_error) == "input_manifest_cleanup_failed" {
        cleanup_error
    } else {
        format!("input_manifest_cleanup_failed:{cleanup_error}")
    };
    let mut errors = vec![Value::String(cleanup_error)];
    if let Some(cause_errors) = cause.get("errors").and_then(Value::as_array) {
        errors.extend(cause_errors.iter().cloned());
    }
    let inputs = cause.get("inputs").cloned().unwrap_or_else(|| json!([]));
    json!({
        "usable": false,
        "summary": "输入清单清理失败",
        "errors": errors,
        "inputs": inputs,
        "error": "input_manifest_cleanup_failed",
        "message": input_error_message("input_manifest_cleanup_failed"),
        "cause": cause
    })
}

fn operation_failure_inspection(cause: Value, error: String) -> Value {
    let mut failure = failed_inspection_json(error);
    failure["summary"] = json!("输入清单发布失败");
    failure["inputs"] = cause.get("inputs").cloned().unwrap_or_else(|| json!([]));
    failure["cause"] = cause;
    failure
}

fn failed_input_save(options: &ServerOptions, project_id: &str, inspection: Value) -> Value {
    json!({
        "status": "failed",
        "error": inspection.get("error").cloned().unwrap_or_else(|| json!("input_not_usable")),
        "message": inspection.get("message").cloned().unwrap_or_else(|| json!(input_error_message("input_not_usable"))),
        "inspection": inspection,
        "project": read_project(options, project_id)
    })
}

fn promote_input_manifest(pending: &Path, formal: &Path) -> Result<(), String> {
    fs::rename(pending, formal).map_err(|err| format!("input_manifest_replace_failed:{err}"))
}

fn clear_current_run_selection(project_dir: &Path) {
    let _ = fs::remove_file(project_dir.join("latest_run_id.txt"));
}

fn preflight_json(options: &ServerOptions, project_id: &str) -> Value {
    let input = project_dir(options, project_id).join("input_manifest.json");
    let store = science_store_path(options);
    let inspection = inspect_manifest(options, &input);
    let (input_ready, input_detail) = match inspection {
        Ok(inspection) if inspection.usable => (true, successful_inspection_message(&inspection)),
        Ok(inspection) => {
            let code = inspection_error_code(&inspection);
            (false, input_error_message(code).to_string())
        }
        Err(err) => (
            false,
            input_error_message(resolver_error_code(&err)).to_string(),
        ),
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
    let project_lock = project_operation_lock(&project);
    let _project_guard = lock_unpoisoned(&project_lock);
    if let Some(run_id) = active_project_run_id(&project) {
        let state = run_state(&project.join("runs").join(&run_id));
        return json!({"status":state,"run":run_json(options,&run_id)});
    }
    let (run_id, run_dir) = match allocate_run_directory(&project) {
        Ok(run) => run,
        Err(err) => return json!({"status":"failed","error":err}),
    };
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
    let _ = write_text(&run_dir.join("run.log"), "state\trunning\n");
    let _ = write_text(&project.join("latest_run_id.txt"), &run_id);
    match launch_run_process(options, &project, &run_dir, &command) {
        Ok(()) => json!({"status":"running","run":run_json(options,&run_id)}),
        Err(reason) => {
            let _ = compare_and_set_run_state(&run_dir, &["running"], "failed");
            let _ = write_text(
                &run_dir.join("failure_reason.txt"),
                &(reason.clone() + "\n"),
            );
            let _ = append_bounded_log(
                &run_dir.join("run.log"),
                format!("state\tfailed\nerror\t{reason}\n").as_bytes(),
                RUN_LOG_MAX_BYTES,
            );
            json!({"status":"failed","run":run_json(options,&run_id)})
        }
    }
}

fn allocate_run_directory(project: &Path) -> Result<(String, PathBuf), String> {
    let runs = project.join("runs");
    fs::create_dir_all(&runs).map_err(|err| format!("create_runs_dir_failed:{err}"))?;
    for _ in 0..8 {
        let run_id = format!("run-{}", new_uuid());
        let run_dir = runs.join(&run_id);
        match fs::create_dir(&run_dir) {
            Ok(()) => return Ok((run_id, run_dir)),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(format!("create_run_dir_failed:{err}")),
        }
    }
    Err("run_id_allocation_failed".to_string())
}

fn launch_run_process(
    options: &ServerOptions,
    project: &Path,
    run_dir: &Path,
    command: &[String],
) -> Result<(), String> {
    let Some(program) = command.first() else {
        return Err("pipeline_command_empty".to_string());
    };
    let mut process = Command::new(program);
    process
        .args(&command[1..])
        .current_dir(&options.workspace_root)
        .envs(runtime_command_environment(options))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        process.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        process.creation_flags(CREATE_NEW_PROCESS_GROUP);
    }
    let mut child = process
        .spawn()
        .map_err(|err| format!("pipeline_command_launch_failed:{err}"))?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let registered = Arc::new(RegisteredRun {
        project_dir: project.to_path_buf(),
        process_id: child.id(),
        child: Mutex::new(child),
        log_lock: Arc::new(Mutex::new(())),
        monitor_finished: AtomicBool::new(false),
    });
    if let Err(err) = register_run_process(run_dir, registered.clone()) {
        let stopped = terminate_registered_run(&registered);
        return Err(format!("{err};process_stopped={stopped}"));
    }
    let _ = write_text(
        &run_dir.join("process_id.txt"),
        &format!("{}\n", registered.process_id),
    );
    let background_run_dir = run_dir.to_path_buf();
    let background_process = registered.clone();
    let thread_name = run_dir
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("run")
        .to_string();
    if let Err(err) = std::thread::Builder::new()
        .name(format!("genoligand-{thread_name}"))
        .spawn(move || {
            let stdout_thread = stdout.map(|reader| {
                spawn_log_reader(
                    reader,
                    "stdout",
                    background_run_dir.join("run.log"),
                    background_process.log_lock.clone(),
                )
            });
            let stderr_thread = stderr.map(|reader| {
                spawn_log_reader(
                    reader,
                    "stderr",
                    background_run_dir.join("run.log"),
                    background_process.log_lock.clone(),
                )
            });
            let status = wait_for_run_process(&background_process);
            if let Some(thread) = stdout_thread {
                let _ = thread.join();
            }
            if let Some(thread) = stderr_thread {
                let _ = thread.join();
            }
            finish_run_process(&background_run_dir, &background_process.log_lock, status);
            unregister_run_process(&background_run_dir);
            background_process
                .monitor_finished
                .store(true, Ordering::Release);
        })
    {
        let stopped = terminate_registered_run(&registered);
        unregister_run_process(run_dir);
        return Err(format!(
            "pipeline_monitor_launch_failed:{err};process_stopped={stopped}"
        ));
    }
    Ok(())
}

fn spawn_log_reader<R>(
    mut reader: R,
    stream: &'static str,
    log_path: PathBuf,
    log_lock: Arc<Mutex<()>>,
) -> std::thread::JoinHandle<()>
where
    R: Read + Send + 'static,
{
    std::thread::spawn(move || {
        let mut buffer = [0_u8; 8192];
        loop {
            let count = match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) => count,
                Err(_) => break,
            };
            let mut chunk = Vec::with_capacity(stream.len() + count + 2);
            chunk.extend_from_slice(stream.as_bytes());
            chunk.push(b'\t');
            chunk.extend_from_slice(&buffer[..count]);
            if !chunk.ends_with(b"\n") {
                chunk.push(b'\n');
            }
            let _guard = lock_unpoisoned(&log_lock);
            let _ = append_bounded_log(&log_path, &chunk, RUN_LOG_MAX_BYTES);
        }
    })
}

fn wait_for_run_process(process: &RegisteredRun) -> Result<ExitStatus, String> {
    loop {
        let status = {
            let mut child = lock_unpoisoned(&process.child);
            child.try_wait()
        };
        match status {
            Ok(Some(status)) => return Ok(status),
            Ok(None) => std::thread::sleep(PROCESS_POLL_INTERVAL),
            Err(err) => return Err(format!("pipeline_process_wait_failed:{err}")),
        }
    }
}

fn finish_run_process(run_dir: &Path, log_lock: &Mutex<()>, result: Result<ExitStatus, String>) {
    let (state, reason) = match result {
        Ok(status) if status.success() => ("completed", String::new()),
        Ok(status) => (
            "failed",
            format!("pipeline_command_failed_exit_{:?}", status.code()),
        ),
        Err(err) => ("failed", err),
    };
    if !compare_and_set_run_state(run_dir, &["running"], state) {
        return;
    }
    let _ = write_text(
        &run_dir.join("failure_reason.txt"),
        &(reason.clone() + "\n"),
    );
    {
        let _guard = lock_unpoisoned(log_lock);
        let detail = if reason.is_empty() {
            format!("state\t{state}\n")
        } else {
            format!("state\t{state}\nerror\t{reason}\n")
        };
        let _ = append_bounded_log(
            &run_dir.join("run.log"),
            detail.as_bytes(),
            RUN_LOG_MAX_BYTES,
        );
    }
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
        ("query_structure", "输入结构准备", "query_structure_prepare"),
        ("features", "结构特征编码", "features_compute"),
        (
            "sequence_alignment",
            "本地序列比对",
            "onsite_sequence_alignment",
        ),
        ("known", "数据库精确与相似性检索", "database_match_known"),
        (
            "metagenome",
            "宏基因组潜在序列比对",
            "metagenome_match_potential",
        ),
        ("structure", "结构候选检索", "structure_evaluate"),
        ("shortlist", "候选粗排与短名单", "candidate_coarse_rank"),
        ("md", "对接与短动力学精筛", "dynamics_screen"),
        ("ranking", "分路线联合排序", "target_rank"),
        ("evidence", "证据链整理", "evidence_build"),
        ("report", "报告生成", "report_render"),
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
    let mut logs = user_run_events(&run_dir, run_id);
    let (mut streamed, truncated) = read_run_log_tail(&run_dir.join("run.log"));
    logs.append(&mut streamed);
    json!({"logs":logs,"truncated":truncated})
}

fn read_run_log_tail(path: &Path) -> (Vec<String>, bool) {
    let Ok(mut file) = OpenOptions::new().read(true).open(path) else {
        return (Vec::new(), false);
    };
    let length = file.metadata().map(|metadata| metadata.len()).unwrap_or(0);
    let start = length.saturating_sub(RUN_LOG_READ_BYTES as u64);
    if file.seek(SeekFrom::Start(start)).is_err() {
        return (Vec::new(), false);
    }
    let mut bytes = Vec::with_capacity((length - start) as usize);
    if (&mut file)
        .take(RUN_LOG_READ_BYTES as u64)
        .read_to_end(&mut bytes)
        .is_err()
    {
        return (Vec::new(), false);
    }
    if start > 0
        && let Some(first_line_end) = bytes.iter().position(|byte| *byte == b'\n')
    {
        bytes.drain(..=first_line_end);
    }
    let mut lines = String::from_utf8_lossy(&bytes)
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    let line_truncated = lines.len() > RUN_LOG_READ_LINES;
    if line_truncated {
        lines.drain(..lines.len() - RUN_LOG_READ_LINES);
    }
    (lines, start > 0 || line_truncated)
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
    if completed && path.exists() && !authenticity_gate_allows_results(&run_dir) {
        return json_response(409, json!({"error":"authenticity_gate_failed"}));
    }
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
                ("\u{8f93}\u{5165}\u{7f16}\u{53f7}", "input_id"),
                ("\u{8bb0}\u{5f55}\u{7f16}\u{53f7}", "record_id"),
                ("\u{8f93}\u{5165}\u{7c7b}\u{578b}", "input_kind"),
                ("\u{6392}\u{5e8f}\u{8def}\u{7ebf}", "ranking_policy"),
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
                    "\u{5df2}\u{77e5}\u{5e93}\u{6743}\u{91cd}",
                    "known_database_weight",
                ),
                (
                    "\u{5b8f}\u{57fa}\u{56e0}\u{7ec4}\u{6743}\u{91cd}",
                    "metagenome_weight",
                ),
                ("\u{7ed3}\u{6784}\u{6743}\u{91cd}", "structure_weight"),
                ("\u{5bf9}\u{63a5}\u{6743}\u{91cd}", "docking_weight"),
                ("\u{52a8}\u{529b}\u{5b66}\u{6743}\u{91cd}", "md_weight"),
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
        "onsite_search" => user_tsv_download(
            read_tsv_rows(&run_dir.join("known_database_matches.tsv")),
            &[
                ("\u{8f93}\u{5165}\u{7f16}\u{53f7}", "input_id"),
                ("\u{6765}\u{6e90}", "source"),
                ("\u{76ee}\u{6807}\u{7f16}\u{53f7}", "target_id"),
                ("\u{76ee}\u{6807}\u{540d}\u{79f0}", "target_name"),
                ("\u{7269}\u{79cd}", "organism"),
                ("\u{76ee}\u{6807}\u{7c7b}\u{578b}", "target_class"),
                ("\u{5339}\u{914d}\u{5206}\u{6570}", "match_score"),
                ("\u{5339}\u{914d}\u{7279}\u{5f81}", "matched_features"),
                ("\u{8bc1}\u{636e}", "evidence"),
            ],
        ),
        "query_structure" => user_tsv_download(
            read_tsv_rows(&run_dir.join("query_structure_manifest.tsv")),
            &[
                ("输入编号", "input_id"),
                ("记录编号", "record_id"),
                ("输入类型", "input_kind"),
                ("准备方法", "method"),
                ("结构特征", "features_json"),
                ("代表结构", "coordinate_artifact"),
                ("特征摘要", "feature_digest"),
                ("状态", "status"),
            ],
        ),
        "candidate_shortlist" => user_tsv_download(
            read_tsv_rows(&run_dir.join("candidate_shortlist.tsv")),
            &[
                ("短名单排序", "shortlist_rank"),
                ("候选编号", "candidate_id"),
                ("输入编号", "input_id"),
                ("记录编号", "record_id"),
                ("目标编号", "target_id"),
                ("粗排分数", "preliminary_score"),
                ("入选原因", "shortlist_reason"),
                ("坐标状态", "coordinate_status"),
            ],
        ),
        "docking_summary" => user_tsv_download(
            read_tsv_rows(&run_dir.join("docking_summary.tsv")),
            &[
                ("\u{5019}\u{9009}\u{7f16}\u{53f7}", "candidate_id"),
                ("\u{5bf9}\u{63a5}\u{72b6}\u{6001}", "docking_status"),
                ("\u{5bf9}\u{63a5}\u{5206}\u{6570}", "docking_score"),
                ("\u{5bf9}\u{63a5}\u{5f15}\u{64ce}", "docking_engine"),
                ("\u{8bf4}\u{660e}", "notes"),
            ],
        ),
        "md_protocol" => user_tsv_download(
            read_tsv_rows(&run_dir.join("tool_outputs/md_protocol.tsv")),
            &[
                ("\u{5019}\u{9009}\u{7f16}\u{53f7}", "candidate_id"),
                ("\u{8f93}\u{5165}\u{7f16}\u{53f7}", "input_id"),
                ("\u{9636}\u{6bb5}", "stage"),
                ("\u{53c2}\u{6570}", "parameter"),
                ("\u{6570}\u{503c}", "value"),
                ("\u{5355}\u{4f4d}", "unit"),
                ("\u{6765}\u{6e90}", "source"),
            ],
        ),
        "md_state" => user_tsv_download(
            read_tsv_rows(&run_dir.join("tool_outputs/md_state.tsv")),
            &[
                ("\u{5019}\u{9009}\u{7f16}\u{53f7}", "candidate_id"),
                ("\u{5e27}", "frame"),
                ("\u{6b65}\u{6570}", "step"),
                ("\u{65f6}\u{95f4} ps", "time_ps"),
                ("\u{9636}\u{6bb5}", "stage"),
                ("\u{52bf}\u{80fd} kJ/mol", "potential_energy_kj_mol"),
                ("\u{603b}\u{80fd} kJ/mol", "total_energy_kj_mol"),
                ("\u{6e29}\u{5ea6} K", "temperature_estimate_K"),
                ("\u{6765}\u{6e90}", "source"),
            ],
        ),
        "gpu_telemetry" => user_tsv_download(
            read_tsv_rows(&run_dir.join("tool_outputs/gpu_telemetry.tsv")),
            &[
                ("\u{91c7}\u{6837}\u{9636}\u{6bb5}", "sample_label"),
                ("\u{65f6}\u{95f4}", "timestamp"),
                ("GPU", "gpu_index"),
                ("\u{8bbe}\u{5907}", "name"),
                ("\u{5229}\u{7528}\u{7387} %", "utilization_gpu_percent"),
                ("\u{663e}\u{5b58} MB", "memory_used_mb"),
                ("\u{5f53}\u{524d}\u{8fdb}\u{7a0b}", "current_pid_observed"),
                (
                    "\u{8ba1}\u{7b97}\u{8fdb}\u{7a0b}\u{6570}",
                    "compute_process_count",
                ),
                ("\u{8bf4}\u{660e}", "failure_reason"),
            ],
        ),
        _ => json_response(404, json!({"error":"artifact_not_available"})),
    }
}

#[cfg(test)]
fn evidence_endpoint_json(options: &ServerOptions, run_id: &str) -> Value {
    evidence_endpoint_json_with_query(options, run_id, &ResultQuery::default())
}

fn evidence_endpoint_json_with_query(
    options: &ServerOptions,
    run_id: &str,
    query: &ResultQuery,
) -> Value {
    let Some(run_dir) = completed_run_dir(options, run_id) else {
        return json!({"run_id":run_id,"evidence":[],"row_count":0,"has_output":false,"pagination":pagination_payload(query,0,0,0)});
    };
    let paged = user_evidence_rows_paged(&run_dir, query);
    let returned_rows = paged.rows.len();
    json!({
        "run_id": run_id,
        "evidence": paged.rows,
        "row_count": paged.total_rows,
        "has_output": paged.total_rows > 0,
        "pagination": pagination_payload(query,paged.total_rows,paged.filtered_rows,returned_rows)
    })
}

#[cfg(test)]
fn candidates_endpoint_json(options: &ServerOptions, run_id: &str) -> Value {
    candidates_endpoint_json_with_query(options, run_id, &ResultQuery::default())
}

fn candidates_endpoint_json_with_query(
    options: &ServerOptions,
    run_id: &str,
    query: &ResultQuery,
) -> Value {
    let Some(run_dir) = completed_run_dir(options, run_id) else {
        return json!({"run_id":run_id,"candidates":[],"row_count":0,"has_output":false,"pagination":pagination_payload(query,0,0,0)});
    };
    let paged = user_candidate_rows_paged(&run_dir, query);
    let returned_rows = paged.rows.len();
    json!({
        "run_id": run_id,
        "candidates": paged.rows,
        "row_count": paged.total_rows,
        "has_output": paged.total_rows > 0,
        "pagination": pagination_payload(query,paged.total_rows,paged.filtered_rows,returned_rows)
    })
}

#[cfg(test)]
fn candidate_detail_json(options: &ServerOptions, run_id: &str) -> Value {
    candidate_detail_json_with_query(options, run_id, &ResultQuery::default())
}

fn candidate_detail_json_with_query(
    options: &ServerOptions,
    run_id: &str,
    query: &ResultQuery,
) -> Value {
    let Some(run_dir) = completed_run_dir(options, run_id) else {
        return json!({"run_id":run_id,"candidate_details":[],"candidate_count":0,"has_output":false,"pagination":pagination_payload(query,0,0,0)});
    };
    let path = run_dir.join("candidate_detail.json");
    let value = read_json_or(&path, json!({}));
    let all_rows = value
        .get("candidates")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(user_candidate_detail_row)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let candidate_count = all_rows.len();
    let filtered_rows = all_rows
        .into_iter()
        .filter(|row| json_row_matches_filter(row, &query.filter))
        .collect::<Vec<_>>();
    let filtered_count = filtered_rows.len();
    let rows = filtered_rows
        .into_iter()
        .skip(query.offset())
        .take(query.page_size)
        .collect::<Vec<_>>();
    let returned_rows = rows.len();
    json!({
        "run_id": run_id,
        "candidate_details": rows,
        "candidate_count": candidate_count,
        "has_output": candidate_count > 0,
        "pagination": pagination_payload(query,candidate_count,filtered_count,returned_rows)
    })
}

#[cfg(test)]
fn structure_endpoint_json(options: &ServerOptions, run_id: &str) -> Value {
    structure_endpoint_json_with_query(options, run_id, &ResultQuery::default())
}

fn structure_endpoint_json_with_query(
    options: &ServerOptions,
    run_id: &str,
    query: &ResultQuery,
) -> Value {
    let Some(run_dir) = completed_run_dir(options, run_id) else {
        return json!({"run_id":run_id,"structure_rows":[],"row_count":0,"has_output":false,"pagination":pagination_payload(query,0,0,0),"viewer":structure_viewer_unavailable("等待运行完成后读取结构坐标输出")});
    };
    let viewer = structure_viewer_payload(&run_dir);
    let paged = user_structure_rows_paged(&run_dir, &viewer, query);
    let returned_rows = paged.rows.len();
    json!({
        "run_id": run_id,
        "structure_rows": paged.rows,
        "row_count": paged.total_rows,
        "has_output": paged.total_rows > 0,
        "pagination": pagination_payload(query,paged.total_rows,paged.filtered_rows,returned_rows),
        "viewer": viewer
    })
}

#[cfg(test)]
fn md_endpoint_json(options: &ServerOptions, run_id: &str) -> Value {
    md_endpoint_json_with_query(options, run_id, &ResultQuery::default())
}

fn md_endpoint_json_with_query(
    options: &ServerOptions,
    run_id: &str,
    query: &ResultQuery,
) -> Value {
    let Some(run_dir) = completed_run_dir(options, run_id) else {
        return json!({
            "run_id":run_id,
            "md_rows":[],
            "row_count":0,
            "has_output":false,
            "pagination":pagination_payload(query,0,0,0),
            "charts":[],
            "readiness":{"ready":false,"reason":"等待运行完成后读取 MD 摘要输出","chart_ready":false},
            "protocol": md_protocol_payload(None),
            "state_trace": md_state_payload(None),
            "cuda_monitoring": md_cuda_monitoring_payload(None)
        });
    };
    let paged = user_md_rows_paged(&run_dir, query);
    let charts = md_chart_rows(&run_dir, &paged.rows);
    let readiness = md_readiness_payload(&run_dir, &paged.rows, &charts);
    let returned_rows = paged.rows.len();
    json!({
        "run_id": run_id,
        "md_rows": paged.rows,
        "row_count": paged.total_rows,
        "has_output": paged.total_rows > 0,
        "pagination": pagination_payload(query,paged.total_rows,paged.filtered_rows,returned_rows),
        "charts": charts,
        "readiness": readiness,
        "protocol": md_protocol_payload(Some(&run_dir)),
        "state_trace": md_state_payload(Some(&run_dir)),
        "cuda_monitoring": md_cuda_monitoring_payload(Some(&run_dir))
    })
}

#[cfg(test)]
fn genome_endpoint_json(options: &ServerOptions, run_id: &str) -> Value {
    genome_endpoint_json_with_query(options, run_id, &ResultQuery::default())
}

fn genome_endpoint_json_with_query(
    options: &ServerOptions,
    run_id: &str,
    query: &ResultQuery,
) -> Value {
    let Some(run_dir) = completed_run_dir(options, run_id) else {
        return json!({"run_id":run_id,"genome_context":[],"row_count":0,"has_output":false,"pagination":pagination_payload(query,0,0,0)});
    };
    let paged = user_genome_rows_paged(&run_dir, query);
    let returned_rows = paged.rows.len();
    json!({
        "run_id": run_id,
        "genome_context": paged.rows,
        "row_count": paged.total_rows,
        "has_output": paged.total_rows > 0,
        "pagination": pagination_payload(query,paged.total_rows,paged.filtered_rows,returned_rows)
    })
}

fn pagination_payload(
    query: &ResultQuery,
    total_rows: usize,
    filtered_rows: usize,
    returned_rows: usize,
) -> Value {
    let offset = query.offset();
    let active_filters = if query.filters_active() {
        vec![query.filter.clone()]
    } else {
        Vec::new()
    };
    json!({
        "page": query.page,
        "page_size": query.page_size,
        "total_rows": total_rows,
        "filtered_rows": filtered_rows,
        "returned_rows": returned_rows,
        "has_next_page": offset.saturating_add(returned_rows) < filtered_rows,
        "has_previous_page": query.page > 1 && filtered_rows > 0,
        "filters_active": query.filters_active(),
        "active_filters": active_filters
    })
}

fn completed_run_dir(options: &ServerOptions, run_id: &str) -> Option<PathBuf> {
    let run_dir = find_run_dir(options, run_id)?;
    if run_state(&run_dir) == "completed" && authenticity_gate_allows_results(&run_dir) {
        Some(run_dir)
    } else {
        None
    }
}

struct PagedRows {
    rows: Vec<Value>,
    total_rows: usize,
    filtered_rows: usize,
}

impl PagedRows {
    fn empty() -> Self {
        Self {
            rows: Vec::new(),
            total_rows: 0,
            filtered_rows: 0,
        }
    }

    fn map(self, f: impl Fn(Value) -> Value) -> Self {
        Self {
            rows: self.rows.into_iter().map(f).collect(),
            total_rows: self.total_rows,
            filtered_rows: self.filtered_rows,
        }
    }
}

fn user_candidate_rows(run_dir: &Path) -> Vec<Value> {
    read_tsv_rows(&run_dir.join("target_candidate_ranking.tsv"))
        .into_iter()
        .map(|row| user_candidate_row(&row))
        .collect()
}

fn user_candidate_rows_paged(run_dir: &Path, query: &ResultQuery) -> PagedRows {
    read_tsv_rows_page(&run_dir.join("target_candidate_ranking.tsv"), query)
        .map(|row| user_candidate_row(&row))
}

fn user_evidence_rows(run_dir: &Path) -> Vec<Value> {
    read_tsv_rows(&run_dir.join("evidence_ledger.tsv"))
        .into_iter()
        .map(user_evidence_row)
        .collect()
}

fn user_evidence_rows_paged(run_dir: &Path, query: &ResultQuery) -> PagedRows {
    read_tsv_rows_page(&run_dir.join("evidence_ledger.tsv"), query).map(user_evidence_row)
}

fn user_structure_rows(run_dir: &Path, viewer: &Value) -> Vec<Value> {
    let targets = candidate_target_lookup(run_dir);
    read_tsv_rows(&run_dir.join("structure_patch_summary.tsv"))
        .into_iter()
        .map(|row| user_structure_row(&row, &targets, viewer))
        .collect()
}

fn user_structure_rows_paged(run_dir: &Path, viewer: &Value, query: &ResultQuery) -> PagedRows {
    let targets = candidate_target_lookup(run_dir);
    read_tsv_rows_page(&run_dir.join("structure_patch_summary.tsv"), query)
        .map(|row| user_structure_row(&row, &targets, viewer))
}

fn user_md_rows(run_dir: &Path) -> Vec<Value> {
    let targets = candidate_target_lookup(run_dir);
    let trajectories = md_trajectory_lookup(run_dir);
    read_tsv_rows(&run_dir.join("md_summary.tsv"))
        .into_iter()
        .map(|row| {
            let trajectory = trajectories.get(row_text(&row, "candidate_id"));
            user_md_row_with_trajectory(&row, &targets, trajectory)
        })
        .collect()
}

fn user_md_rows_paged(run_dir: &Path, query: &ResultQuery) -> PagedRows {
    let targets = candidate_target_lookup(run_dir);
    let trajectories = md_trajectory_lookup(run_dir);
    read_tsv_rows_page(&run_dir.join("md_summary.tsv"), query).map(|row| {
        let trajectory = trajectories.get(row_text(&row, "candidate_id"));
        user_md_row_with_trajectory(&row, &targets, trajectory)
    })
}

fn user_genome_rows_paged(run_dir: &Path, query: &ResultQuery) -> PagedRows {
    read_tsv_rows_page(&run_dir.join("metagenome_potential_matches.tsv"), query)
        .map(|row| user_genome_row(&row))
}

fn json_row_matches_filter(row: &Value, filter: &str) -> bool {
    let needle = filter.trim().to_ascii_lowercase();
    needle.is_empty() || row.to_string().to_ascii_lowercase().contains(&needle)
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

fn md_protocol_payload(run_dir: Option<&Path>) -> Value {
    let Some(run_dir) = run_dir else {
        return json!({"ready":false,"reason":"等待运行完成后读取 MD 运行协议"});
    };
    let artifacts = md_artifact_paths(run_dir, "protocol_artifact", "tool_outputs/md_protocol.tsv");
    let rows = artifacts
        .iter()
        .flat_map(|path| read_tsv_rows(path))
        .collect::<Vec<_>>();
    if rows.is_empty() {
        return json!({"ready":false,"reason":"当前运行没有 MD 运行协议输出"});
    }
    json!({
        "ready": true,
        "candidate_count": artifacts.len(),
        "platform": protocol_value(&rows, "platform", "openmm_platform"),
        "integrator": protocol_value(&rows, "integrator", "integrator"),
        "time_step": protocol_value_with_unit(&rows, "integrator", "time_step"),
        "equilibration_time": protocol_value_with_unit(&rows, "equilibration", "time"),
        "production_time": protocol_value_with_unit(&rows, "production", "total_time"),
        "production_steps": protocol_value_with_unit(&rows, "production", "total_steps"),
        "frame_count": protocol_value_with_unit(&rows, "output", "frame_count"),
        "state_rows": protocol_value_with_unit(&rows, "output", "state_rows"),
        "artifact": if run_dir.join("tool_outputs/md_manifest_index.json").is_file() { "tool_outputs/md_manifest_index.json" } else { "tool_outputs/md_protocol.tsv" }
    })
}

fn md_state_payload(run_dir: Option<&Path>) -> Value {
    let Some(run_dir) = run_dir else {
        return json!({"ready":false,"reason":"等待运行完成后读取 MD 状态序列"});
    };
    let artifacts = md_artifact_paths(run_dir, "state_artifact", "tool_outputs/md_state.tsv");
    let rows = artifacts
        .iter()
        .flat_map(|path| read_tsv_rows(path))
        .collect::<Vec<_>>();
    let Some(last) = rows.iter().max_by(|left, right| {
        row_text(left, "time_ps")
            .parse::<f64>()
            .unwrap_or(f64::NEG_INFINITY)
            .total_cmp(
                &row_text(right, "time_ps")
                    .parse::<f64>()
                    .unwrap_or(f64::NEG_INFINITY),
            )
    }) else {
        return json!({"ready":false,"reason":"当前运行没有 MD 状态序列输出"});
    };
    json!({
        "ready": true,
        "candidate_count": artifacts.len(),
        "row_count": rows.len(),
        "last_stage": row_text(last, "stage"),
        "last_step": row_text(last, "step"),
        "last_time_ps": row_text(last, "time_ps"),
        "last_temperature_K": row_text(last, "temperature_estimate_K"),
        "last_total_energy_kj_mol": row_text(last, "total_energy_kj_mol"),
        "artifact": if run_dir.join("tool_outputs/md_manifest_index.json").is_file() { "tool_outputs/md_manifest_index.json" } else { "tool_outputs/md_state.tsv" }
    })
}

fn md_cuda_monitoring_payload(run_dir: Option<&Path>) -> Value {
    let Some(run_dir) = run_dir else {
        return json!({"ready":false,"reason":"等待运行完成后读取 CUDA 监测输出"});
    };
    let artifacts = md_artifact_paths(
        run_dir,
        "gpu_telemetry_status_artifact",
        "tool_outputs/gpu_telemetry_status.tsv",
    );
    let rows = artifacts
        .iter()
        .flat_map(|path| read_tsv_rows(path))
        .collect::<Vec<_>>();
    let Some(row) = rows.first() else {
        return json!({"ready":false,"reason":"当前运行没有 CUDA 监测状态输出"});
    };
    let platform = row_text(row, "cuda_platform");
    let gpu_observed = rows
        .iter()
        .all(|row| row_text(row, "gpu_observed") == "true");
    let nvidia_smi_available = rows
        .iter()
        .all(|row| row_text(row, "nvidia_smi_available") == "true");
    json!({
        "ready": rows.iter().all(|row| row_text(row, "cuda_platform") == "CUDA") && gpu_observed,
        "candidate_count": artifacts.len(),
        "platform": platform,
        "nvidia_smi_available": nvidia_smi_available,
        "gpu_observed": gpu_observed,
        "production_sample_count": sum_integer_field(&rows, "production_sample_count"),
        "production_max_utilization_gpu_percent": max_number_field(&rows, "production_max_utilization_gpu_percent"),
        "production_max_memory_delta_mb": row_text(row, "production_max_memory_delta_mb"),
        "observation_basis": row_text(row, "observation_basis"),
        "failure_reason": row_text(row, "failure_reason"),
        "artifact": if run_dir.join("tool_outputs/md_manifest_index.json").is_file() { "tool_outputs/md_manifest_index.json" } else { "tool_outputs/gpu_telemetry_status.tsv" }
    })
}

fn md_artifact_paths(run_dir: &Path, field: &str, fallback: &str) -> Vec<PathBuf> {
    let paths = md_run_manifests(run_dir)
        .iter()
        .filter_map(|manifest| manifest.get(field).and_then(Value::as_str))
        .filter_map(|relative| run_relative_file(run_dir, relative))
        .collect::<Vec<_>>();
    if !paths.is_empty() {
        return paths;
    }
    let fallback = run_dir.join(fallback);
    if fallback.is_file() {
        vec![fallback]
    } else {
        Vec::new()
    }
}

fn md_run_manifests(run_dir: &Path) -> Vec<Value> {
    read_json_or(
        &run_dir.join("tool_outputs/md_manifest_index.json"),
        json!({}),
    )["entries"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|entry| entry.get("status").and_then(Value::as_str) == Some("completed"))
        .filter_map(|entry| {
            let candidate_id = entry.get("candidate_id").and_then(Value::as_str)?;
            let manifest_path =
                run_relative_file(run_dir, entry.get("manifest").and_then(Value::as_str)?)?;
            let manifest = read_json_or(&manifest_path, json!({}));
            (manifest.get("candidate_id").and_then(Value::as_str) == Some(candidate_id)
                && manifest.get("method").and_then(Value::as_str)
                    == Some("openmm_receptor_ligand_complex_dynamics"))
            .then_some(manifest)
        })
        .collect()
}

fn run_relative_file(run_dir: &Path, relative: &str) -> Option<PathBuf> {
    let relative = Path::new(relative.trim());
    if relative.as_os_str().is_empty()
        || relative.is_absolute()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return None;
    }
    let path = run_dir.join(relative);
    path.is_file().then_some(path)
}

fn md_trajectory_lookup(run_dir: &Path) -> BTreeMap<String, Value> {
    md_run_manifests(run_dir)
        .into_iter()
        .filter_map(|manifest| {
            let candidate_id = manifest.get("candidate_id")?.as_str()?.to_string();
            let trajectory = manifest.get("trajectory")?.as_str()?;
            let ready = run_relative_file(run_dir, trajectory)
                .and_then(|path| fs::metadata(path).ok())
                .is_some_and(|metadata| metadata.len() > 0);
            let frame_count = manifest.get("frame_count").and_then(Value::as_u64);
            Some((
                candidate_id,
                json!({"ready": ready, "frame_count": frame_count}),
            ))
        })
        .collect()
}

fn sum_integer_field(rows: &[Value], key: &str) -> String {
    let values = rows
        .iter()
        .filter_map(|row| row_text(row, key).parse::<u64>().ok())
        .collect::<Vec<_>>();
    if values.is_empty() {
        String::new()
    } else {
        values.iter().sum::<u64>().to_string()
    }
}

fn max_number_field(rows: &[Value], key: &str) -> String {
    rows.iter()
        .filter_map(|row| {
            let text = row_text(row, key);
            text.parse::<f64>().ok().map(|value| (value, text))
        })
        .max_by(|left, right| left.0.total_cmp(&right.0))
        .map(|(_, text)| text.to_string())
        .unwrap_or_default()
}

fn protocol_value(rows: &[Value], stage: &str, parameter: &str) -> String {
    rows.iter()
        .find(|row| row_text(row, "stage") == stage && row_text(row, "parameter") == parameter)
        .map(|row| row_text(row, "value").to_string())
        .unwrap_or_default()
}

fn protocol_value_with_unit(rows: &[Value], stage: &str, parameter: &str) -> String {
    rows.iter()
        .find(|row| row_text(row, "stage") == stage && row_text(row, "parameter") == parameter)
        .map(|row| {
            let value = row_text(row, "value");
            let unit = row_text(row, "unit");
            if unit.is_empty() {
                value.to_string()
            } else {
                format!("{value} {unit}")
            }
        })
        .unwrap_or_default()
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
        parts.push("\u{5f53}\u{524d} run \u{7684}\u{590d}\u{5408}\u{7269} MD \u{8bc1}\u{636e}\u{5df2}\u{53c2}\u{4e0e}\u{5019}\u{9009}\u{6392}\u{5e8f}");
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

fn user_candidate_row(row: &Value) -> Value {
    let scope = row_text(row, "candidate_scope");
    let status = first_non_empty(row_text(row, "status"), "computed_prediction");
    let md_score = row_number(row, "md_score");
    json!({
        "rank": row_text(row, "rank"),
        "candidate_id": row_text(row, "candidate_id"),
        "input_id": row_text(row, "input_id"),
        "record_id": row_text(row, "record_id"),
        "input_kind": row_text(row, "input_kind"),
        "ranking_policy": row_text(row, "ranking_policy"),
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
        "known_database_weight": row_text(row, "known_database_weight"),
        "metagenome_weight": row_text(row, "metagenome_weight"),
        "structure_weight": row_text(row, "structure_weight"),
        "docking_weight": row_text(row, "docking_weight"),
        "md_weight": row_text(row, "md_weight"),
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
    let ranking_weights = row.get("ranking_weights").unwrap_or(&Value::Null);
    let scope = row
        .get("candidate_scope")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let md_score = json_number(scores.get("md"));
    json!({
        "candidate_id": row.get("candidate_id").cloned().unwrap_or_else(|| json!("")),
        "input_id": row.get("input_id").cloned().unwrap_or_else(|| json!("")),
        "record_id": row.get("record_id").cloned().unwrap_or_else(|| json!("")),
        "input_kind": row.get("input_kind").cloned().unwrap_or_else(|| json!("")),
        "ranking_policy": row.get("ranking_policy").cloned().unwrap_or_else(|| json!("")),
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
        "known_database_weight": ranking_weights.get("known_database").cloned().unwrap_or_else(|| json!("")),
        "metagenome_weight": ranking_weights.get("metagenome").cloned().unwrap_or_else(|| json!("")),
        "structure_weight": ranking_weights.get("structure").cloned().unwrap_or_else(|| json!("")),
        "docking_weight": ranking_weights.get("docking").cloned().unwrap_or_else(|| json!("")),
        "md_weight": ranking_weights.get("md").cloned().unwrap_or_else(|| json!("")),
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

#[cfg(test)]
fn user_md_row(row: &Value, targets: &BTreeMap<String, (String, String)>) -> Value {
    user_md_row_with_trajectory(row, targets, None)
}

fn user_md_row_with_trajectory(
    row: &Value,
    targets: &BTreeMap<String, (String, String)>,
    trajectory: Option<&Value>,
) -> Value {
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
    let trajectory_ready = trajectory
        .and_then(|value| value.get("ready"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let trajectory_frame_count = trajectory
        .and_then(|value| value.get("frame_count"))
        .and_then(Value::as_u64);
    let trajectory_status = match (trajectory_ready, trajectory_frame_count) {
        (true, Some(count)) => format!("轨迹数据已就绪（{count} 帧）"),
        (true, None) => "轨迹数据已就绪".to_string(),
        _ => "轨迹数据未就绪".to_string(),
    };
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
        "trajectory_ready": trajectory_ready,
        "trajectory_frame_count": trajectory_frame_count,
        "trajectory_status": trajectory_status,
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
        "query_structure_manifest.tsv" | "query_structure_manifest" => "输入结构准备",
        "feature_manifest.tsv" | "feature_manifest" => "\u{7279}\u{5f81}\u{8ba1}\u{7b97}",
        "known_database_matches.tsv" | "known_database_matches" => {
            "\u{5df2}\u{77e5}\u{6570}\u{636e}\u{5e93}\u{6bd4}\u{5bf9}"
        }
        "metagenome_potential_matches.tsv" | "metagenome_potential_matches" => {
            "\u{5b8f}\u{57fa}\u{56e0}\u{7ec4}\u{6f5c}\u{5728}\u{5e8f}\u{5217}\u{6bd4}\u{5bf9}"
        }
        "tool_outputs/mmseqs_alignment_matches.tsv" | "mmseqs_alignment_matches" => {
            "\u{5e8f}\u{5217}\u{73b0}\u{573a}\u{6bd4}\u{5bf9}"
        }
        "tool_outputs/foldseek_structure_matches.tsv" | "foldseek_structure_matches" => {
            "\u{7ed3}\u{6784}\u{73b0}\u{573a}\u{6bd4}\u{5bf9}"
        }
        "structure_patch_summary.tsv" | "structure_patch_summary" => {
            "\u{7ed3}\u{6784}\u{4e0e}\u{53e3}\u{888b}"
        }
        "worker_runtime_summary.tsv" | "worker_runtime_summary" => {
            "\u{5de5}\u{5177}\u{9002}\u{914d}\u{5668}\u{68c0}\u{67e5}"
        }
        "scientific_runtime_summary.tsv" | "scientific_runtime_summary" => {
            "\u{79d1}\u{5b66}\u{8fd0}\u{884c}\u{6458}\u{8981}"
        }
        "docking_summary.tsv" | "docking_summary" => "\u{5bf9}\u{63a5}\u{6458}\u{8981}",
        "md_summary.tsv" | "md_summary" => "\u{52a8}\u{529b}\u{5b66}\u{6458}\u{8981}",
        "tool_outputs/md_protocol.tsv" | "md_protocol" => "MD \u{8fd0}\u{884c}\u{534f}\u{8bae}",
        "tool_outputs/md_state.tsv" | "md_state" => "MD \u{72b6}\u{6001}\u{5e8f}\u{5217}",
        "tool_outputs/gpu_telemetry.tsv" | "gpu_telemetry" => "CUDA \u{76d1}\u{6d4b}",
        "tool_outputs/gpu_telemetry_status.tsv" | "gpu_telemetry_status" => "CUDA \u{72b6}\u{6001}",
        "target_candidate_ranking.tsv" | "target_candidate_ranking" => {
            "\u{5019}\u{9009}\u{6392}\u{5e8f}"
        }
        "candidate_shortlist.tsv" | "candidate_shortlist" => "候选粗排与短名单",
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
        "tool_outputs/mmseqs_alignment_matches.tsv" => "MMseqs2 \u{73b0}\u{573a}\u{6bd4}\u{5bf9}",
        "tool_outputs/foldseek_structure_matches.tsv" => {
            "Foldseek \u{73b0}\u{573a}\u{7ed3}\u{6784}\u{6bd4}\u{5bf9}"
        }
        "worker_runtime_summary.tsv" => {
            "\u{5f53}\u{524d}\u{8fd0}\u{884c}\u{7684}\u{5de5}\u{5177}\u{9002}\u{914d}\u{5668}"
        }
        "scientific_runtime_summary.tsv" => {
            "\u{5f53}\u{524d}\u{8fd0}\u{884c}\u{7684}\u{79d1}\u{5b66}\u{5de5}\u{5177}\u{8f93}\u{51fa}"
        }
        "structure_patch_summary.tsv" | "docking_summary.tsv" | "md_summary.tsv" => {
            "\u{5f53}\u{524d}\u{8fd0}\u{884c}\u{7684}\u{5de5}\u{5177}\u{8f93}\u{51fa}"
        }
        "tool_outputs/md_protocol.tsv" => {
            "\u{5f53}\u{524d} MD \u{7684}\u{534f}\u{8bae}\u{8bb0}\u{5f55}"
        }
        "tool_outputs/md_state.tsv" => {
            "\u{5f53}\u{524d} OpenMM MD \u{7684}\u{6b65}\u{6570}\u{3001}\u{80fd}\u{91cf}\u{548c}\u{6e29}\u{5ea6}\u{72b6}\u{6001}\u{5e8f}\u{5217}"
        }
        "tool_outputs/gpu_telemetry.tsv" | "tool_outputs/gpu_telemetry_status.tsv" => {
            "\u{5f53}\u{524d} CUDA \u{8fd0}\u{884c}\u{76d1}\u{6d4b}"
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
        "index_build_manifest.tsv",
    ];
    let rows = required
        .iter()
        .map(|rel| {
            let path = store.join(rel);
            let ready = if *rel == "indexes/tool_versions.tsv" {
                tool_versions_ready(&path)
            } else if *rel == "index_build_manifest.tsv" {
                index_build_ready(&path)
            } else {
                path.exists()
            };
            json!({"item":rel,"path":path.display().to_string(),"ready":ready})
        })
        .collect::<Vec<_>>();
    json!({"science_store":store.display().to_string(),"required":rows,"user_configures_paths":false})
}

fn user_health_json(options: &ServerOptions) -> Value {
    json!({
        "status":"ready",
        "application":PRODUCT_NAME,
        "version":PRODUCT_VERSION,
        "commit":PRODUCT_COMMIT,
        "build_id":PRODUCT_COMMIT,
        "runtime":runtime_binary_identity(),
        "science_store_readiness":science_store_readiness_json(options)
    })
}

fn science_store_readiness_json(options: &ServerOptions) -> Value {
    match evaluate_science_store_readiness(&science_store_path(options)) {
        Ok(readiness) => {
            let blocker_details = readiness
                .blocker_details
                .iter()
                .map(|blocker| {
                    json!({
                        "code": blocker.code,
                        "component": blocker.component,
                        "evidence": blocker.evidence,
                        "repair_action": blocker.repair_action,
                    })
                })
                .collect::<Vec<_>>();
            let evidence_manifest = readiness
                .evidence_manifest
                .iter()
                .map(|row| json!({"path": row.path, "sha256": row.sha256}))
                .collect::<Vec<_>>();
            let advisory_details = readiness
                .advisory_details
                .iter()
                .map(|advisory| {
                    json!({
                        "code": advisory.code,
                        "component": advisory.component,
                        "evidence": advisory.evidence,
                        "action": advisory.action,
                    })
                })
                .collect::<Vec<_>>();
            json!({
                "schema_version":"genoligand_science_store_readiness_v1",
                "status":readiness.status,
                "ready":readiness.ready,
                "blockers":readiness.blockers,
                "blocker_details":blocker_details,
                "advisories":readiness.advisories,
                "advisory_details":advisory_details,
                "repair_actions":readiness.repair_actions,
                "evidence_manifest":evidence_manifest,
                "evidence_digest":readiness.evidence_digest,
                "sources":readiness.sources,
                "indexes":readiness.indexes,
            })
        }
        Err(reason) => json!({
            "schema_version":"genoligand_science_store_readiness_v1",
            "status":"needs_repair",
            "ready":false,
            "blockers":["science_store_readiness_evaluation_failed"],
            "blocker_details":[{
                "code":"science_store_readiness_evaluation_failed",
                "component":"science_store_readiness",
                "evidence":reason,
                "repair_action":"rerun_science_store_installer"
            }],
            "advisories":[],
            "advisory_details":[],
            "repair_actions":["rerun_science_store_installer"],
            "evidence_manifest":[],
            "evidence_digest":"",
            "sources":[],
            "indexes":[]
        }),
    }
}

fn sha256_file(path: &Path) -> Option<String> {
    let mut file = fs::File::open(path).ok()?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 1024 * 1024];
    loop {
        let read = file.read(&mut buffer).ok()?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Some(format!("{:x}", hasher.finalize()))
}

fn runtime_binary_manifest_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("GENOLIGAND_RUNTIME_DIR")
        .map(PathBuf::from)
        .map(|path| path.join("runtime_binary_manifest.json"))
        .filter(|path| path.is_file())
    {
        return Some(path);
    }
    if let Ok(executable) = std::env::current_exe()
        && let Some(runtime_dir) = executable.parent().and_then(Path::parent)
    {
        let path = runtime_dir.join("runtime_binary_manifest.json");
        if path.is_file() {
            return Some(path);
        }
    }
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .map(|path| {
            path.join("GenoLigand")
                .join("runtime")
                .join("runtime_binary_manifest.json")
        })
        .filter(|path| path.is_file())
}

fn runtime_binary_identity() -> Value {
    let Some(manifest_path) = runtime_binary_manifest_path() else {
        return json!({
            "status":"unpublished",
            "commit":PRODUCT_COMMIT,
            "manifest_digest":""
        });
    };
    let manifest = read_json_or(&manifest_path, json!({}));
    let manifest_digest = manifest
        .get("manifest_digest")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let tsv_path = manifest
        .get("manifest")
        .and_then(Value::as_str)
        .map(PathBuf::from);
    let current_executable = std::env::current_exe()
        .ok()
        .and_then(|path| path.canonicalize().ok());
    let server = manifest
        .get("binaries")
        .and_then(Value::as_array)
        .and_then(|rows| {
            rows.iter()
                .find(|row| row.get("binary").and_then(Value::as_str) == Some("gls-server"))
        });
    let verified = manifest.get("schema_version").and_then(Value::as_str)
        == Some("genoligand_runtime_binary_manifest_v1")
        && manifest.get("status").and_then(Value::as_str) == Some("ready")
        && manifest_digest.len() == 64
        && tsv_path
            .as_ref()
            .and_then(|path| sha256_file(path))
            .as_deref()
            == Some(manifest_digest)
        && server.is_some_and(|record| {
            let recorded_path = record
                .get("path")
                .and_then(Value::as_str)
                .map(PathBuf::from)
                .and_then(|path| path.canonicalize().ok());
            record.get("status").and_then(Value::as_str) == Some("ready")
                && record.get("smoke").and_then(Value::as_str) == Some("ready")
                && record.get("version").and_then(Value::as_str) == Some(PRODUCT_VERSION)
                && record.get("commit").and_then(Value::as_str) == Some(PRODUCT_COMMIT)
                && recorded_path == current_executable
                && current_executable
                    .as_ref()
                    .and_then(|path| sha256_file(path))
                    .as_deref()
                    == record.get("sha256").and_then(Value::as_str)
        });
    json!({
        "status":if verified {"verified"} else {"mismatch"},
        "commit":PRODUCT_COMMIT,
        "manifest_digest":manifest_digest,
        "manifest":manifest_path.display().to_string()
    })
}

fn user_databases_json(options: &ServerOptions) -> Value {
    let store = science_store_path(options);
    let rows = required_resource_rows(&store);
    let missing = missing_resource_rows(&rows);
    json!({
        "required": rows,
        "sources": source_status_rows(&store),
        "missing": missing,
        "readiness": science_store_readiness_json(options),
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
    let reconciliation = reconcile_setup_status_progress(options);
    let ready = reconciliation.database_ready;
    let store = science_store_path(options);
    let required = required_resource_rows(&store);
    let worker = worker_environment_status(options);
    let setup_ready = ready
        && worker
            .get("ready")
            .and_then(Value::as_bool)
            .unwrap_or(false);
    json!({
        "database_ready": ready,
        "readiness": science_store_readiness_json(options),
        "repair_action": if setup_ready {"继续"} else {"运行安装/修复"},
        "checks": required,
        "missing": missing_resource_rows(&required),
        "worker_environment": worker,
        "source_records": source_records_status(&store),
        "tool_platforms": [openmm_platform_status(&store)]
    })
}

fn user_setup_progress_json(options: &ServerOptions) -> Value {
    reconcile_setup_status_progress(options).progress
}

struct SetupReconciliation {
    database_ready: bool,
    progress: Value,
}

fn reconcile_setup_status_progress(options: &ServerOptions) -> SetupReconciliation {
    let database_ready = preflight_store_ready(options);
    let path = science_store_path(options).join("install_progress.json");
    let value = read_json_or(&path, json!({}));
    let mut progress = setup_user_install_progress(Some(&value));
    let historical_status = progress
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("idle")
        .to_string();
    let historical_ready = matches!(
        historical_status.as_str(),
        "ready" | "completed" | "success"
    ) || progress.get("percent").and_then(Value::as_u64) == Some(100);
    if database_ready {
        progress["status"] = json!("ready");
        progress["failed_steps"] = json!(0);
        progress["percent"] = json!(100);
        progress["message"] = json!("科学仓库已就绪。");
    } else if setup_progress_is_stale(&progress) {
        progress["status"] = json!("needs_repair");
        progress["failed_steps"] = json!(1);
        progress["percent"] = json!(progress["percent"].as_u64().unwrap_or(0).min(99));
        progress["message"] =
            json!("安装进度长时间没有更新，请重新运行安装/修复并检查网络、磁盘和依赖状态。");
    } else if historical_status == "running" {
        progress["percent"] = json!(progress["percent"].as_u64().unwrap_or(0).min(99));
    } else if historical_ready {
        progress["status"] = json!("needs_repair");
        progress["failed_steps"] = json!(progress["failed_steps"].as_u64().unwrap_or(0).max(1));
        progress["percent"] = json!(progress["percent"].as_u64().unwrap_or(0).min(99));
        progress["message"] = json!("实时检查发现科学仓库需要修复，请重新运行安装/修复。");
    }
    progress["database_ready"] = json!(database_ready);
    SetupReconciliation {
        database_ready,
        progress,
    }
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

fn cancel_run(options: &ServerOptions, run_id: &str) -> Value {
    let Some(run_dir) = find_run_dir(options, run_id) else {
        return json!({"status":"not_found","run_id":run_id,"process_stopped":true});
    };
    let Some(project) = run_dir.parent().and_then(Path::parent) else {
        return json!({"status":"failed","run_id":run_id,"process_stopped":false});
    };
    let project_lock = project_operation_lock(project);
    let _project_guard = lock_unpoisoned(&project_lock);
    let process = registered_run_process(&run_dir);
    let state_before = run_state(&run_dir);
    if state_before == "running" && process.is_none() {
        return json!({
            "status":"failed",
            "run_id":run_id,
            "error":"active_run_handle_missing",
            "process_stopped":false
        });
    }
    let cancelled = compare_and_set_run_state(&run_dir, &["running"], "cancelled");
    let process_stopped = process
        .as_deref()
        .map(terminate_registered_run)
        .unwrap_or(state_before != "running");
    if cancelled {
        let log_lock = process
            .as_ref()
            .map(|process| process.log_lock.clone())
            .unwrap_or_else(|| Arc::new(Mutex::new(())));
        let _guard = lock_unpoisoned(&log_lock);
        let _ = append_bounded_log(
            &run_dir.join("run.log"),
            b"state\tcancelled\n",
            RUN_LOG_MAX_BYTES,
        );
    }
    if process_stopped && let Some(process) = process.as_deref() {
        let _ = wait_for_monitor_completion(process, Duration::from_secs(2));
    }
    json!({
        "status": run_state(&run_dir),
        "run_id": run_id,
        "process_stopped": process_stopped
    })
}

fn terminate_registered_run(process: &RegisteredRun) -> bool {
    if run_process_exited(process) {
        return true;
    }
    std::thread::sleep(Duration::from_millis(100));
    if run_process_exited(process) {
        return true;
    }
    terminate_process_tree(process.process_id);
    if wait_for_process_exit(process, Duration::from_secs(3)) {
        return true;
    }
    let _ = lock_unpoisoned(&process.child).kill();
    wait_for_process_exit(process, Duration::from_secs(2))
}

fn run_process_exited(process: &RegisteredRun) -> bool {
    lock_unpoisoned(&process.child)
        .try_wait()
        .map(|status| status.is_some())
        .unwrap_or(true)
}

fn wait_for_process_exit(process: &RegisteredRun, timeout: Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if run_process_exited(process) {
            return true;
        }
        std::thread::sleep(PROCESS_POLL_INTERVAL);
    }
    run_process_exited(process)
}

fn wait_for_monitor_completion(process: &RegisteredRun, timeout: Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if process.monitor_finished.load(Ordering::Acquire) {
            return true;
        }
        std::thread::sleep(PROCESS_POLL_INTERVAL);
    }
    process.monitor_finished.load(Ordering::Acquire)
}

#[cfg(windows)]
fn terminate_process_tree(process_id: u32) {
    let _ = Command::new("taskkill")
        .args(["/PID", &process_id.to_string(), "/T", "/F"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[cfg(unix)]
fn terminate_process_tree(process_id: u32) {
    let _ = Command::new("kill")
        .args(["-TERM", "--", &format!("-{process_id}")])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

fn delete_project(options: &ServerOptions, project_id: &str) -> Value {
    let dir = project_dir(options, project_id);
    let project_lock = project_operation_lock(&dir);
    let _project_guard = lock_unpoisoned(&project_lock);
    if !dir.exists() {
        return json!({"status":"not_found","project_id":project_id});
    }
    let processes = registered_project_processes(&dir);
    if active_project_run_id(&dir).is_some() && processes.is_empty() {
        return json!({
            "status":"failed",
            "project_id":project_id,
            "error":"active_run_handle_missing"
        });
    }
    for (run_dir, process) in &processes {
        let _ = compare_and_set_run_state(run_dir, &["running"], "cancelled");
        if !terminate_registered_run(process) {
            return json!({
                "status":"failed",
                "project_id":project_id,
                "error":"process_tree_termination_failed"
            });
        }
    }
    for (_, process) in &processes {
        if !wait_for_monitor_completion(process, Duration::from_secs(3)) {
            return json!({
                "status":"failed",
                "project_id":project_id,
                "error":"run_monitor_shutdown_timeout"
            });
        }
    }
    match fs::remove_dir_all(&dir) {
        Ok(()) => json!({"status":"deleted","project_id":project_id}),
        Err(err) => json!({"status":"failed","error":err.to_string()}),
    }
}

fn preflight_store_ready(options: &ServerOptions) -> bool {
    evaluate_science_store_readiness(&science_store_path(options))
        .is_ok_and(|readiness| readiness.ready)
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

fn index_build_ready(path: &Path) -> bool {
    let Ok(text) = fs::read_to_string(path) else {
        return false;
    };
    let mut saw_row = false;
    for line in text.lines().skip(1).filter(|line| !line.trim().is_empty()) {
        saw_row = true;
        if line.contains("index_row_limit_reached") {
            return false;
        }
    }
    saw_row
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
        (
            "index_build",
            "index_build_manifest.tsv",
            "索引完整性",
            "运行安装/修复以构建完整搜索索引",
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
                "index_build" => index_build_ready(&path),
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

fn server_registry() -> &'static Mutex<ServerRegistry> {
    SERVER_REGISTRY.get_or_init(|| Mutex::new(ServerRegistry::default()))
}

fn lock_unpoisoned<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn project_operation_lock(project: &Path) -> Arc<Mutex<()>> {
    let mut registry = lock_unpoisoned(server_registry());
    registry
        .project_locks
        .entry(project.to_path_buf())
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone()
}

fn run_state_lock(run_dir: &Path) -> Arc<Mutex<()>> {
    let mut registry = lock_unpoisoned(server_registry());
    registry
        .run_state_locks
        .entry(run_dir.to_path_buf())
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone()
}

fn register_run_process(
    run_dir: &Path,
    process: Arc<RegisteredRun>,
) -> Result<Arc<RegisteredRun>, String> {
    let mut registry = lock_unpoisoned(server_registry());
    if registry.runs.contains_key(run_dir) {
        return Err("run_process_already_registered".to_string());
    }
    registry.runs.insert(run_dir.to_path_buf(), process.clone());
    Ok(process)
}

fn registered_run_process(run_dir: &Path) -> Option<Arc<RegisteredRun>> {
    lock_unpoisoned(server_registry())
        .runs
        .get(run_dir)
        .cloned()
}

fn registered_project_processes(project: &Path) -> Vec<(PathBuf, Arc<RegisteredRun>)> {
    lock_unpoisoned(server_registry())
        .runs
        .iter()
        .filter(|(_, process)| process.project_dir == project)
        .map(|(run_dir, process)| (run_dir.clone(), process.clone()))
        .collect()
}

fn unregister_run_process(run_dir: &Path) {
    lock_unpoisoned(server_registry()).runs.remove(run_dir);
}

fn project_dir(options: &ServerOptions, project_id: &str) -> PathBuf {
    projects_root(options).join(safe_id(project_id))
}

fn active_project_run_id(project: &Path) -> Option<String> {
    if let Some(run_id) = lock_unpoisoned(server_registry())
        .runs
        .iter()
        .find(|(_, process)| process.project_dir == project)
        .and_then(|(run_dir, _)| run_dir.file_name())
        .map(|run_id| run_id.to_string_lossy().to_string())
    {
        return Some(run_id);
    }
    if let Some(run_id) = read_trimmed(&project.join("latest_run_id.txt"))
        && run_state(&project.join("runs").join(&run_id)) == "running"
    {
        return Some(run_id);
    }
    fs::read_dir(project.join("runs"))
        .ok()?
        .flatten()
        .find_map(|entry| {
            let path = entry.path();
            (path.is_dir() && run_state(&path) == "running")
                .then(|| entry.file_name().to_string_lossy().to_string())
        })
}

fn find_run_dir(options: &ServerOptions, run_id: &str) -> Option<PathBuf> {
    let mut found = None;
    for entry in fs::read_dir(projects_root(options)).ok()?.flatten() {
        let path = entry.path().join("runs").join(run_id);
        if path.is_dir() {
            if found.is_some() {
                return None;
            }
            found = Some(path);
        }
    }
    found
}

fn run_state(run_dir: &Path) -> String {
    read_trimmed(&run_dir.join("run_state.txt")).unwrap_or_else(|| "created".to_string())
}

fn compare_and_set_run_state(run_dir: &Path, expected: &[&str], next: &str) -> bool {
    let state_lock = run_state_lock(run_dir);
    let _state_guard = lock_unpoisoned(&state_lock);
    if !expected.contains(&run_state(run_dir).as_str()) {
        return false;
    }
    write_text(&run_dir.join("run_state.txt"), &(next.to_string() + "\n")).is_ok()
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
        .map(|dir| input_has_user_value(options, &dir.join("input_manifest.json")))
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
    let results_visible = completed && authenticity_gate_allows_results(run_dir);
    let candidate_rows = if results_visible {
        rows_len(&run_dir.join("target_candidate_ranking.tsv"))
    } else {
        0
    };
    let evidence_rows = if results_visible {
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

fn input_has_user_value(options: &ServerOptions, path: &Path) -> bool {
    inspect_manifest(options, path)
        .map(|inspection| inspection.usable)
        .unwrap_or(false)
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
    if !authenticity_gate_allows_results(run_dir) {
        return false;
    }
    match id {
        "candidate_ranking" => {
            completed && rows_len(&run_dir.join("target_candidate_ranking.tsv")) > 0
        }
        "evidence" | "structure_patch" | "md_summary" | "md_protocol" | "gpu_telemetry"
        | "onsite_search" | "docking_summary" => completed && rows_len(path) > 0,
        "research_report" => {
            completed
                && rows_len(&run_dir.join("target_candidate_ranking.tsv")) > 0
                && rows_len(&run_dir.join("evidence_ledger.tsv")) > 0
        }
        _ => completed,
    }
}

fn authenticity_gate_allows_results(run_dir: &Path) -> bool {
    let path = run_dir.join("authenticity_gate.tsv");
    let Ok(text) = fs::read_to_string(path) else {
        return false;
    };
    let mut lines = text.lines().filter(|line| !line.trim().is_empty());
    let Some(header) = lines.next() else {
        return false;
    };
    let headers = header
        .trim_start_matches('\u{feff}')
        .split('\t')
        .collect::<Vec<_>>();
    if !headers.contains(&"artifact") {
        return false;
    }
    let Some(status_index) = headers.iter().position(|header| *header == "status") else {
        return false;
    };
    let mut saw_row = false;
    for line in lines {
        saw_row = true;
        if line.split('\t').nth(status_index).map(str::trim) != Some("passed") {
            return false;
        }
    }
    saw_row
}

fn artifact_specs() -> &'static [(&'static str, &'static str, &'static str)] {
    &[
        (
            "query_structure",
            "输入结构准备",
            "query_structure_manifest.tsv",
        ),
        (
            "onsite_search",
            "\u{73b0}\u{573a}\u{641c}\u{7d22}\u{7ed3}\u{679c}",
            "known_database_matches.tsv",
        ),
        (
            "candidate_shortlist",
            "候选粗排与短名单",
            "candidate_shortlist.tsv",
        ),
        (
            "docking_summary",
            "\u{5bf9}\u{63a5}\u{6458}\u{8981}",
            "docking_summary.tsv",
        ),
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
            "md_protocol",
            "MD \u{8fd0}\u{884c}\u{534f}\u{8bae}",
            "tool_outputs/md_protocol.tsv",
        ),
        (
            "md_state",
            "MD \u{72b6}\u{6001}\u{5e8f}\u{5217}",
            "tool_outputs/md_state.tsv",
        ),
        (
            "gpu_telemetry",
            "CUDA \u{76d1}\u{6d4b}",
            "tool_outputs/gpu_telemetry.tsv",
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
        "query_structure_prepare" => "输入结构准备",
        "features_compute" => "结构特征编码",
        "onsite_sequence_alignment" => "本地序列比对",
        "database_match_known" => "数据库精确与相似性检索",
        "metagenome_match_potential" => "宏基因组潜在序列比对",
        "structure_evaluate" => "结构候选检索",
        "candidate_coarse_rank" => "候选粗排与短名单",
        "dynamics_screen" => "对接与短动力学精筛",
        "target_rank" => "分路线候选排序",
        "evidence_build" => "证据链整理",
        "report_render" => "报告生成",
        _ => "运行准备",
    }
}

fn read_tsv_rows(path: &Path) -> Vec<Value> {
    read_tsv_rows_page(path, &ResultQuery::all()).rows
}

fn read_tsv_rows_page(path: &Path, query: &ResultQuery) -> PagedRows {
    let Ok(file) = fs::File::open(path) else {
        return PagedRows::empty();
    };
    read_tsv_rows_page_from(BufReader::new(file), query).unwrap_or_else(|_| PagedRows::empty())
}

fn read_tsv_rows_page_from(
    mut reader: impl BufRead,
    query: &ResultQuery,
) -> std::io::Result<PagedRows> {
    let mut line = String::new();
    let headers = loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            return Ok(PagedRows::empty());
        }
        let header = line.trim_end_matches(['\r', '\n']);
        if !header.trim().is_empty() {
            break header
                .split('\t')
                .map(|value| value.trim().to_string())
                .collect::<Vec<_>>();
        }
    };

    let filter = query.filter.trim().to_ascii_lowercase();
    let offset = query.offset();
    let mut rows = Vec::new();
    let mut total_rows = 0;
    let mut filtered_rows = 0;
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        let line = line.trim_end_matches(['\r', '\n']);
        if line.trim().is_empty() {
            continue;
        }
        total_rows += 1;
        if filter.is_empty() || line.to_ascii_lowercase().contains(&filter) {
            let filtered_index = filtered_rows;
            filtered_rows += 1;
            if filtered_index < offset || rows.len() >= query.page_size {
                continue;
            }
            let cols = line.split('\t').collect::<Vec<_>>();
            let mut row = serde_json::Map::new();
            for (index, header) in headers.iter().enumerate() {
                row.insert(
                    header.clone(),
                    Value::String(cols.get(index).copied().unwrap_or("").to_string()),
                );
            }
            rows.push(Value::Object(row));
        }
    }
    Ok(PagedRows {
        rows,
        total_rows,
        filtered_rows,
    })
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

fn append_bounded_log(path: &Path, data: &[u8], max_bytes: usize) -> Result<(), String> {
    if max_bytes == 0 || data.is_empty() {
        return Ok(());
    }
    let data = if data.len() > max_bytes {
        &data[data.len() - max_bytes..]
    } else {
        data
    };
    let mut file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(path)
        .map_err(|err| format!("open_log_failed:{}:{err}", path.display()))?;
    let existing_len = file
        .metadata()
        .map_err(|err| format!("read_log_metadata_failed:{}:{err}", path.display()))?
        .len();
    if existing_len.saturating_add(data.len() as u64) <= max_bytes as u64 {
        file.seek(SeekFrom::End(0))
            .and_then(|_| file.write_all(data))
            .map_err(|err| format!("append_log_failed:{}:{err}", path.display()))?;
        return Ok(());
    }
    let keep = max_bytes.saturating_sub(data.len());
    let mut tail = vec![0_u8; keep];
    if keep > 0 {
        let available = existing_len.min(keep as u64) as usize;
        if available < keep {
            tail.drain(..keep - available);
        }
        file.seek(SeekFrom::Start(
            existing_len.saturating_sub(available as u64),
        ))
        .and_then(|_| file.read_exact(&mut tail))
        .map_err(|err| format!("read_log_tail_failed:{}:{err}", path.display()))?;
    }
    file.set_len(0)
        .and_then(|_| file.seek(SeekFrom::Start(0)))
        .and_then(|_| file.write_all(&tail))
        .and_then(|_| file.write_all(data))
        .map_err(|err| format!("truncate_log_failed:{}:{err}", path.display()))
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

fn new_uuid() -> String {
    let sequence = UUID_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let state = RandomState::new();
    let mut first = state.build_hasher();
    first.write_u128(timestamp);
    first.write_u64(sequence);
    first.write_u32(std::process::id());
    let first = first.finish();
    let mut second = state.build_hasher();
    second.write_u64(first);
    second.write_u64(sequence.rotate_left(17));
    second.write_u128(timestamp.rotate_left(43));
    let second = second.finish();
    let mut bytes = [0_u8; 16];
    bytes[..8].copy_from_slice(&first.to_be_bytes());
    bytes[8..].copy_from_slice(&second.to_be_bytes());
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    format!(
        "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
        u32::from_be_bytes(bytes[0..4].try_into().expect("uuid field")),
        u16::from_be_bytes(bytes[4..6].try_into().expect("uuid field")),
        u16::from_be_bytes(bytes[6..8].try_into().expect("uuid field")),
        u16::from_be_bytes(bytes[8..10].try_into().expect("uuid field")),
        u64::from_be_bytes([
            0, 0, bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
        ])
    )
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
    include!("tests.rs");

    fn looks_like_uuid(value: &str) -> bool {
        let parts = value.split('-').collect::<Vec<_>>();
        parts.len() == 5
            && parts.iter().map(|part| part.len()).eq([8, 4, 4, 4, 12])
            && value.chars().all(|ch| ch == '-' || ch.is_ascii_hexdigit())
    }

    #[test]
    fn project_ids_are_uuids_and_drafts_are_visible() {
        let root = std::env::temp_dir().join(format!("genoligand-project-id-{}", now_millis()));
        let options = server_options(root.clone());

        let first = create_project_record(&options).expect("first project");
        let second = create_project_record(&options).expect("second project");
        let first_id = first["project_id"].as_str().expect("first project id");
        let second_id = second["project_id"].as_str().expect("second project id");
        let first_project_dir = project_dir(&options, first_id);
        let (first_run_id, _) = allocate_run_directory(&first_project_dir).expect("first run id");
        let (second_run_id, _) = allocate_run_directory(&first_project_dir).expect("second run id");
        let listed = user_projects_json(&options);

        assert!(looks_like_uuid(
            first_id.strip_prefix("project-").expect("project prefix")
        ));
        assert!(looks_like_uuid(
            second_id.strip_prefix("project-").expect("project prefix")
        ));
        assert_ne!(first_id, second_id);
        assert!(looks_like_uuid(
            first_run_id.strip_prefix("run-").expect("run prefix")
        ));
        assert!(looks_like_uuid(
            second_run_id.strip_prefix("run-").expect("run prefix")
        ));
        assert_ne!(first_run_id, second_run_id);
        assert_eq!(first["project_status"], "draft");
        assert_eq!(listed["projects"].as_array().map(Vec::len), Some(2));
        assert_eq!(listed["projects"][0]["project_status"], "draft");

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn saved_inputs_are_reloadable_and_mark_input_ready() {
        let root = std::env::temp_dir().join(format!("genoligand-input-ready-{}", now_millis()));
        let options = server_options(root.clone());
        let project = create_project_record(&options).expect("project");
        let project_id = project["project_id"].as_str().expect("project id");
        let manifest = r#"{"inputs":[{"field":"project_name","value":"Input reload project"},{"field":"small_molecule","value":"CCO"}],"files":[]}"#;

        let saved = write_input_manifest(&options, project_id, manifest);
        let (status, _, body) = route(
            "GET",
            &format!("/api/user/projects/{project_id}/inputs"),
            &options,
            "",
        );
        let reloaded: Value = serde_json::from_str(&body).expect("input response");
        let detail = user_project_detail(&options, project_id);

        assert_eq!(saved["status"], "saved");
        assert_eq!(status, 200);
        assert_eq!(reloaded["inputs"][1]["value"], "CCO");
        assert_eq!(detail["project_status"], "input_ready");
        assert_eq!(detail["project_state"], "输入已就绪");

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn input_capabilities_route_exposes_the_runtime_input_contract() {
        let root =
            std::env::temp_dir().join(format!("genoligand-input-capabilities-{}", now_millis()));
        let options = server_options(root.clone());

        let (status, content_type, body) =
            route("GET", "/api/user/inputs/capabilities", &options, "");
        let capabilities: Value = serde_json::from_str(&body).expect("capabilities JSON");
        let input_types = capabilities["input_types"]
            .as_array()
            .expect("input types array");

        assert_eq!(status, 200);
        assert_eq!(content_type, "application/json; charset=utf-8");
        assert_eq!(
            capabilities["schema_version"],
            "genoligand_input_capabilities_v1"
        );
        assert_eq!(input_types.len(), 5);
        for capability in input_types {
            assert!(
                capability["kind"]
                    .as_str()
                    .is_some_and(|value| !value.is_empty())
            );
            assert!(
                capability["field"]
                    .as_str()
                    .is_some_and(|value| !value.is_empty())
            );
            assert_eq!(capability["text"]["supported"], true);
            assert_eq!(capability["file"]["supported"], true);
            assert!(
                capability["file"]["extensions"]
                    .as_array()
                    .is_some_and(|extensions| !extensions.is_empty())
            );
            assert_eq!(capability["constraints"]["minimum_records"], 1);
        }

        let (missing_status, _, _) = route("GET", "/api/user/inputs/capability", &options, "");
        assert_eq!(missing_status, 404);
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn save_rejects_changes_while_run_is_active() {
        let root = std::env::temp_dir().join(format!("genoligand-save-lock-{}", now_millis()));
        let options = server_options(root.clone());
        let project_id = "project-save-lock";
        let project = project_dir(&options, project_id);
        let run_dir = project.join("runs").join("run-active");
        fs::create_dir_all(&run_dir).expect("run dir");
        let original = r#"{"inputs":[{"field":"small_molecule","value":"CCO"}],"files":[]}"#;
        fs::write(project.join("input_manifest.json"), original).expect("original manifest");
        fs::write(project.join("latest_run_id.txt"), "run-active\n").expect("latest run");
        fs::write(run_dir.join("run_state.txt"), "running\n").expect("run state");

        let response = write_input_manifest(
            &options,
            project_id,
            r#"{"inputs":[{"field":"small_molecule","value":"CCC"}],"files":[]}"#,
        );

        assert_eq!(response["status"], "failed");
        assert_eq!(response["error"], "project_run_active");
        assert_eq!(
            fs::read_to_string(project.join("input_manifest.json")).expect("manifest"),
            original
        );
        assert_eq!(
            read_trimmed(&project.join("latest_run_id.txt")).as_deref(),
            Some("run-active")
        );

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn run_completion_uses_compare_and_set() {
        let root = std::env::temp_dir().join(format!("genoligand-run-cas-{}", now_millis()));
        let run_dir = root.join("projects/project-1/runs/run-1");
        fs::create_dir_all(&run_dir).expect("run dir");
        fs::write(run_dir.join("run_state.txt"), "running\n").expect("running state");

        assert!(compare_and_set_run_state(
            &run_dir,
            &["running"],
            "completed"
        ));
        fs::write(run_dir.join("run_state.txt"), "cancelled\n").expect("cancelled state");
        assert!(!compare_and_set_run_state(
            &run_dir,
            &["running"],
            "completed"
        ));
        assert_eq!(run_state(&run_dir), "cancelled");

        fs::remove_dir_all(&root).ok();
    }

    fn streaming_test_command() -> Vec<String> {
        if cfg!(windows) {
            vec![
                "cmd.exe".to_string(),
                "/D".to_string(),
                "/C".to_string(),
                "echo stream-first & ping -n 20 127.0.0.1 & echo stream-second".to_string(),
            ]
        } else {
            vec![
                "sh".to_string(),
                "-c".to_string(),
                "printf 'stream-first\\n'; sleep 5; printf 'stream-second\\n'".to_string(),
            ]
        }
    }

    #[test]
    fn running_process_streams_logs_and_cancel_stops_child() {
        let root = std::env::temp_dir().join(format!("genoligand-stream-cancel-{}", now_millis()));
        let options = server_options(root.clone());
        let project = project_dir(&options, "project-stream");
        let run_dir = project.join("runs").join("run-stream");
        fs::create_dir_all(&run_dir).expect("run dir");
        fs::write(run_dir.join("run_state.txt"), "running\n").expect("running state");

        launch_run_process(&options, &project, &run_dir, &streaming_test_command())
            .expect("launch process");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
        while std::time::Instant::now() < deadline {
            if fs::read_to_string(run_dir.join("run.log"))
                .unwrap_or_default()
                .contains("stream-first")
            {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }

        assert!(
            fs::read_to_string(run_dir.join("run.log"))
                .unwrap_or_default()
                .contains("stream-first")
        );
        assert_eq!(run_state(&run_dir), "running");
        let cancel_started = std::time::Instant::now();
        let cancelled = cancel_run(&options, "run-stream");
        assert_eq!(cancelled["status"], "cancelled");
        assert_eq!(cancelled["process_stopped"], true);
        assert!(cancel_started.elapsed() < std::time::Duration::from_secs(5));
        assert_eq!(run_state(&run_dir), "cancelled");

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn cancel_without_registered_child_fails_closed() {
        let root =
            std::env::temp_dir().join(format!("genoligand-cancel-missing-handle-{}", now_millis()));
        let options = server_options(root.clone());
        let run_dir = project_dir(&options, "project-missing-handle")
            .join("runs")
            .join("run-missing-handle");
        fs::create_dir_all(&run_dir).expect("run dir");
        fs::write(run_dir.join("run_state.txt"), "running\n").expect("run state");

        let response = cancel_run(&options, "run-missing-handle");

        assert_eq!(response["status"], "failed");
        assert_eq!(response["error"], "active_run_handle_missing");
        assert_eq!(response["process_stopped"], false);
        assert_eq!(run_state(&run_dir), "running");

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn run_logs_are_bounded_on_disk_and_in_api() {
        let root = std::env::temp_dir().join(format!("genoligand-bounded-log-{}", now_millis()));
        let options = server_options(root.clone());
        let run_dir = project_dir(&options, "project-log")
            .join("runs")
            .join("run-log");
        fs::create_dir_all(&run_dir).expect("run dir");
        fs::write(run_dir.join("run_state.txt"), "running\n").expect("run state");
        let log_path = run_dir.join("run.log");
        for index in 0..200 {
            append_bounded_log(
                &log_path,
                format!("stdout\tlog-line-{index:03}-xxxxxxxxxxxxxxxxxxxxxxxx\n").as_bytes(),
                1024,
            )
            .expect("append log");
        }

        let payload = logs_json(&options, "run-log");
        let logs = payload["logs"].as_array().expect("logs");
        assert!(fs::metadata(&log_path).expect("log metadata").len() <= 1024);
        assert!(
            logs.iter()
                .any(|line| line.as_str().unwrap_or_default().contains("log-line-199"))
        );
        assert!(logs.len() <= RUN_LOG_READ_LINES + 8);

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn delete_stops_registered_process_before_removing_project() {
        let root = std::env::temp_dir().join(format!("genoligand-delete-run-{}", now_millis()));
        let options = server_options(root.clone());
        let project_id = "project-delete-run";
        let project = project_dir(&options, project_id);
        let run_dir = project.join("runs").join("run-delete");
        fs::create_dir_all(&run_dir).expect("run dir");
        fs::write(run_dir.join("run_state.txt"), "running\n").expect("run state");
        launch_run_process(&options, &project, &run_dir, &streaming_test_command())
            .expect("launch process");
        let process = registered_run_process(&run_dir).expect("registered process");

        let delete_started = std::time::Instant::now();
        let deleted = delete_project(&options, project_id);

        assert_eq!(deleted["status"], "deleted");
        assert!(delete_started.elapsed() < std::time::Duration::from_secs(5));
        assert!(run_process_exited(&process));
        assert!(registered_run_process(&run_dir).is_none());
        assert!(!project.exists());

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn duplicate_historical_run_ids_fail_closed() {
        let root = std::env::temp_dir().join(format!("genoligand-run-lookup-{}", now_millis()));
        let options = server_options(root.clone());
        for project_id in ["project-a", "project-b"] {
            let run_dir = project_dir(&options, project_id)
                .join("runs")
                .join("run-duplicate");
            fs::create_dir_all(&run_dir).expect("run dir");
            fs::write(run_dir.join("run_state.txt"), "completed\n").expect("run state");
        }

        assert!(find_run_dir(&options, "run-duplicate").is_none());
        assert_eq!(run_json(&options, "run-duplicate")["state"], "not_found");

        fs::remove_dir_all(&root).ok();
    }

    fn write_authenticity_result_fixture(run_dir: &Path) {
        fs::create_dir_all(run_dir).expect("run dir");
        fs::write(run_dir.join("run_state.txt"), "completed\n").expect("run state");
        fs::write(
            run_dir.join("target_candidate_ranking.tsv"),
            "rank\tcandidate_id\ttarget_id\ttarget_name\tcandidate_scope\torganism\tfinal_score\tknown_database_score\tmetagenome_score\tstructure_score\tdocking_score\tmd_score\tevidence_summary\tstatus\n1\tC1\tT1\tTarget 1\tknown_human_protein\tHomo sapiens\t0.9\t0.8\t0.0\t0.1\t0.0\t0.0\tcurrent evidence\tcomputed_prediction\n",
        )
        .expect("ranking");
        fs::write(
            run_dir.join("evidence_ledger.tsv"),
            "claim_id\tevidence_artifact\towner\tstatus\tnotes\nclaim-1\ttarget_candidate_ranking.tsv\tGenoLigand\tavailable\tcurrent run\n",
        )
        .expect("evidence");
        fs::write(run_dir.join("research_report.md"), "# Authentic report\n").expect("report");
    }

    fn assert_authenticity_gate_denies_results<F>(label: &str, setup_gate: F)
    where
        F: FnOnce(&Path),
    {
        let root =
            std::env::temp_dir().join(format!("genoligand-authenticity-{label}-{}", now_millis()));
        let options = server_options(root.clone());
        let run_id = format!("run-{label}");
        let run_dir = project_dir(&options, &format!("project-{label}"))
            .join("runs")
            .join(&run_id);
        write_authenticity_result_fixture(&run_dir);
        setup_gate(&run_dir.join("authenticity_gate.tsv"));

        let candidates = candidates_endpoint_json(&options, &run_id);
        let evidence = evidence_endpoint_json(&options, &run_id);
        let artifacts = artifacts_json(&options, &run_id);
        let workspace = user_workspace_summary_json(&options);
        let (download_status, _, download_body) =
            download_artifact_response(&options, &run_id, "research_report");

        assert_eq!(candidates["has_output"], false);
        assert!(
            candidates["candidates"]
                .as_array()
                .is_some_and(Vec::is_empty)
        );
        assert_eq!(evidence["has_output"], false);
        assert!(evidence["evidence"].as_array().is_some_and(Vec::is_empty));
        assert_eq!(workspace["candidate_count"], 0);
        assert_eq!(workspace["result_count"], 0);
        for id in ["candidate_ranking", "evidence", "research_report"] {
            let visible = artifacts["items"]
                .as_array()
                .and_then(|items| {
                    items
                        .iter()
                        .find(|item| item.get("id").and_then(Value::as_str) == Some(id))
                })
                .and_then(|item| item.get("exists"))
                .and_then(Value::as_bool)
                .unwrap_or(true);
            assert!(!visible, "{id} must be hidden when gate is {label}");
        }
        assert_eq!(download_status, 409);
        assert!(download_body.contains("authenticity_gate_failed"));
        assert!(!download_body.contains("Authentic report"));

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn authenticity_gate_missing_fails_closed() {
        assert_authenticity_gate_denies_results("missing", |_| {});
    }

    #[test]
    fn authenticity_gate_read_failure_fails_closed() {
        assert_authenticity_gate_denies_results("unreadable", |gate| {
            fs::create_dir(gate).expect("unreadable gate path");
        });
    }

    #[test]
    fn authenticity_gate_non_allowed_status_fails_closed() {
        assert_authenticity_gate_denies_results("not-allowed", |gate| {
            fs::write(
                gate,
                "artifact\tstatus\tline_number\tmarker\treason\nfinal_run_artifacts\tready\t0\t\tunchecked\n",
            )
            .expect("gate");
        });
    }

    #[test]
    fn authenticity_gate_passed_allows_results() {
        let root =
            std::env::temp_dir().join(format!("genoligand-authenticity-passed-{}", now_millis()));
        let options = server_options(root.clone());
        let run_dir = project_dir(&options, "project-passed")
            .join("runs")
            .join("run-passed");
        write_authenticity_result_fixture(&run_dir);
        fs::write(
            run_dir.join("authenticity_gate.tsv"),
            "artifact\tstatus\tline_number\tmarker\treason\nfinal_run_artifacts\tpassed\t0\t\tclean\n",
        )
        .expect("gate");

        assert_eq!(
            candidates_endpoint_json(&options, "run-passed")["has_output"],
            true
        );
        assert_eq!(
            evidence_endpoint_json(&options, "run-passed")["has_output"],
            true
        );
        let (status, _, body) =
            download_artifact_response(&options, "run-passed", "research_report");
        assert_eq!(status, 200);
        assert!(body.contains("Authentic report"));

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn setup_progress_reconciles_historical_ready_with_live_status() {
        let root =
            std::env::temp_dir().join(format!("genoligand-setup-reconcile-{}", now_millis()));
        let options = server_options(root.clone());
        let store = science_store_path(&options);
        fs::create_dir_all(&store).expect("store");
        fs::write(
            store.join("install_progress.json"),
            json!({
                "status": "ready",
                "stage": "安装完成",
                "current_resource": "科学仓库",
                "completed_steps": 6,
                "total_steps": 6,
                "failed_steps": 0,
                "percent": 100,
                "message": "科学仓库已就绪",
                "updated_at": now_seconds()
            })
            .to_string(),
        )
        .expect("progress");

        let status = user_setup_status_json(&options);
        let progress = user_setup_progress_json(&options);

        assert_eq!(status["database_ready"], false);
        assert_eq!(progress["database_ready"], false);
        assert_eq!(progress["status"], "needs_repair");
        assert!(progress["percent"].as_u64().unwrap_or(100) < 100);
        assert!(progress["failed_steps"].as_u64().unwrap_or(0) >= 1);

        fs::remove_dir_all(&root).ok();
    }
}
