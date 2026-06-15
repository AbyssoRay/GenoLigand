use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

pub const PRODUCT_NAME: &str = "GenoLigand Local WebUI";
pub const PRODUCT_VERSION: &str = "0.9.2";

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
    "mdtraj",
    "mdanalysis",
];

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
        ("GET", "/styles.css") => {
            return (
                200,
                "text/css; charset=utf-8",
                include_str!("../../../webui/app/styles.css").to_string(),
            );
        }
        ("GET", "/app.js") => {
            return (
                200,
                "application/javascript; charset=utf-8",
                include_str!("../../../webui/app/app.js").to_string(),
            );
        }
        _ => {}
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
    let _ = write_text(
        &dir.join("input_manifest.json"),
        "{\"inputs\":[],\"files\":[],\"status\":\"waiting_for_user_input\"}\n",
    );
    Ok(read_project(options, &project_id))
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
    let value =
        serde_json::from_str::<Value>(body).unwrap_or_else(|_| json!({"inputs":[],"files":[]}));
    if let Some(name) = input_project_name(&value) {
        let _ = write_text(&dir.join("project_name.txt"), &name);
    }
    if !input_value_has_usable_user_input(&value) {
        return json!({
            "status":"failed",
            "error":"input_not_usable",
            "message":"\u{8bf7}\u{586b}\u{5199}\u{81f3}\u{5c11}\u{4e00}\u{4e2a}\u{53ef}\u{8bc6}\u{522b}\u{7684}\u{7814}\u{7a76}\u{8f93}\u{5165}\u{3002}",
            "project": read_project(options, project_id)
        });
    }
    match write_text(&path, &(value.to_string() + "\n")) {
        Ok(()) => json!({"status":"saved","project":read_project(options, project_id)}),
        Err(err) => json!({"status":"failed","error":err}),
    }
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
            store.join("science_store_manifest.tsv").exists(),
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
            "tools",
            tool_versions_ready(&store.join("indexes/tool_versions.tsv")),
            store
                .join("indexes/tool_versions.tsv")
                .display()
                .to_string(),
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

fn start_run(options: &ServerOptions, project_id: &str) -> Value {
    let project = project_dir(options, project_id);
    let run_id = format!("run-{}", now_millis());
    let run_dir = project.join("runs").join(&run_id);
    let _ = fs::create_dir_all(&run_dir);
    let _ = write_text(&run_dir.join("run_state.txt"), "running\n");
    let _ = write_text(&run_dir.join("current_stage.txt"), "pipeline_run\n");
    let input = project.join("input_manifest.json");
    let command = gls_cli_command(options, &input, &run_dir);
    let command_text = command.join(" ");
    let _ = write_text(&run_dir.join("current_command.txt"), &command_text);
    let output = Command::new(&command[0])
        .args(&command[1..])
        .current_dir(&options.workspace_root)
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
    let exe = if cfg!(windows) {
        "gls-cli.exe"
    } else {
        "gls-cli"
    };
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
    std::env::var("GENOLIGAND_SCIENCE_STORE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| default_science_store(&options.workspace_root))
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
    let state = run.get("state").and_then(Value::as_str).unwrap_or_default();
    run["state_label"] = json!(run_state_user_label(state));
    run
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
            user_structure_rows(&run_dir),
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
        return json!({"run_id":run_id,"structure_rows":[],"row_count":0,"has_output":false});
    };
    let rows = user_structure_rows(&run_dir);
    json!({
        "run_id": run_id,
        "structure_rows": rows,
        "row_count": rows.len(),
        "has_output": !rows.is_empty()
    })
}

fn md_endpoint_json(options: &ServerOptions, run_id: &str) -> Value {
    let Some(run_dir) = completed_run_dir(options, run_id) else {
        return json!({"run_id":run_id,"md_rows":[],"row_count":0,"has_output":false});
    };
    let rows = user_md_rows(&run_dir);
    json!({
        "run_id": run_id,
        "md_rows": rows,
        "row_count": rows.len(),
        "has_output": !rows.is_empty()
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

fn user_structure_rows(run_dir: &Path) -> Vec<Value> {
    let targets = candidate_target_lookup(run_dir);
    read_tsv_rows(&run_dir.join("structure_patch_summary.tsv"))
        .into_iter()
        .map(|row| user_structure_row(&row, &targets))
        .collect()
}

fn user_md_rows(run_dir: &Path) -> Vec<Value> {
    let targets = candidate_target_lookup(run_dir);
    read_tsv_rows(&run_dir.join("md_summary.tsv"))
        .into_iter()
        .map(|row| user_md_row(&row, &targets))
        .collect()
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
    json!({
        "candidate_id": row.get("candidate_id").cloned().unwrap_or_else(|| json!("")),
        "target_id": row.get("target_id").cloned().unwrap_or_else(|| json!("")),
        "target_name": row.get("target_name").cloned().unwrap_or_else(|| json!("")),
        "candidate_scope": row.get("candidate_scope").cloned().unwrap_or_else(|| json!("")),
        "organism": row.get("organism").cloned().unwrap_or_else(|| json!("")),
        "final_score": scores.get("final").cloned().unwrap_or_else(|| json!("")),
        "known_database_score": scores.get("known_database").cloned().unwrap_or_else(|| json!("")),
        "metagenome_score": scores.get("metagenome").cloned().unwrap_or_else(|| json!("")),
        "structure_score": scores.get("structure").cloned().unwrap_or_else(|| json!("")),
        "docking_score": scores.get("docking").cloned().unwrap_or_else(|| json!("")),
        "md_score": scores.get("md").cloned().unwrap_or_else(|| json!("")),
        "evidence_summary": row.get("evidence_summary").cloned().unwrap_or_else(|| json!("")),
        "status": "\u{8ba1}\u{7b97}\u{5019}\u{9009}",
        "detail_source": "\u{5f53}\u{524d}\u{8fd0}\u{884c}\u{7684}\u{5019}\u{9009}\u{8be6}\u{60c5}\u{8f93}\u{51fa}",
        "result_source": "\u{5019}\u{9009}\u{8be6}\u{60c5}\u{8f93}\u{51fa}"
    })
}

fn user_structure_row(row: &Value, targets: &BTreeMap<String, (String, String)>) -> Value {
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
        "viewer_status": "\u{672a}\u{63a5}\u{5165}\u{53ef}\u{7528}\u{4e09}\u{7ef4}\u{67e5}\u{770b}\u{8f93}\u{51fa}",
        "result_source": "\u{7ed3}\u{6784}\u{4e0e}\u{53e3}\u{888b}\u{6458}\u{8981}\u{8f93}\u{51fa}"
    })
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
        "contact_occupancy": contact_occupancy,
        "summary": summary,
        "status": if md_score.is_empty() && row_text(row, "rmsd").is_empty() {
            "\u{7b49}\u{5f85}\u{52a8}\u{529b}\u{5b66}\u{8f93}\u{51fa}"
        } else {
            "\u{5df2}\u{751f}\u{6210}\u{52a8}\u{529b}\u{5b66}\u{6458}\u{8981}"
        },
        "trajectory_status": "\u{672a}\u{63a5}\u{5165}\u{53ef}\u{7528}\u{8f68}\u{8ff9}\u{56fe}\u{8f93}\u{51fa}",
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
        "metagenome_potential" | "metagenome_potential_protein" => {
            "\u{5b8f}\u{57fa}\u{56e0}\u{7ec4}\u{6f5c}\u{5728}\u{5e8f}\u{5217}"
        }
        "structure_interaction" => "\u{7ed3}\u{6784}\u{4e92}\u{4f5c}",
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
        "missing" | "not_ready" => "\u{672a}\u{5c31}\u{7eea}",
        _ => "\u{5df2}\u{8bb0}\u{5f55}",
    }
}

fn evidence_note_summary(notes: &str) -> String {
    if notes.is_empty() {
        return "\u{5f53}\u{524d}\u{8fd0}\u{884c}\u{5df2}\u{8bb0}\u{5f55}\u{8be5}\u{8bc1}\u{636e}\u{3002}".to_string();
    }
    if notes.contains("sha256:") {
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
    let missing = missing_resource_rows(&required_resource_rows(&science_store_path(options)));
    let checks = vec![
        json!({"label":"服务", "ready":true, "next_action":"继续"}),
        json!({"label":"运行组件", "ready":gls_cli.0, "next_action": if gls_cli.0 {"继续"} else {"重新运行安装程序"}}),
        json!({"label":"科学仓库", "ready":database_ready, "next_action": if database_ready {"继续"} else {"运行安装/修复"}}),
    ];
    json!({
        "checks": checks,
        "missing": missing,
        "science_store": user_databases_json(options)
    })
}

fn user_setup_status_json(options: &ServerOptions) -> Value {
    let ready = preflight_store_ready(options);
    let required = required_resource_rows(&science_store_path(options));
    json!({
        "database_ready": ready,
        "repair_action": if ready {"继续"} else {"运行安装/修复"},
        "checks": required,
        "missing": missing_resource_rows(&required)
    })
}

fn run_setup_repair_json(options: &ServerOptions) -> Value {
    let command = setup_repair_command(options);
    let output = Command::new(&command[0])
        .args(&command[1..])
        .current_dir(&options.workspace_root)
        .output();
    let (message, installer_payload) = match output {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            (
                "修复动作已执行。".to_string(),
                setup_installer_summary(&stdout),
            )
        }
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            (
                format!(
                    "修复动作未完成，退出码 {:?}。",
                    output.status.code().unwrap_or_default()
                ),
                setup_installer_summary(&stdout),
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
        "missing": missing_resource_rows(&required)
    });
    if let Some(installer_payload) = installer_payload {
        payload["installer"] = installer_payload;
    }
    payload
}

fn user_setup_install_plan_json(options: &ServerOptions) -> Value {
    let catalog_rows = config_tsv_data_row_count(options, "source_catalog.tsv");
    let sync_rows = config_tsv_data_row_count(options, "source_sync_plan.tsv");
    json!({
        "action":"运行安装/修复",
        "description":"准备数据库、索引、计算工具、许可证和来源记录。普通运行不配置数据库路径。",
        "database_ready": preflight_store_ready(options),
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

fn setup_installer_summary(stdout: &str) -> Option<Value> {
    let payload: Value = serde_json::from_str(stdout.trim()).ok()?;
    let downloads = payload
        .get("downloads")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let resources = setup_user_resources(payload.get("resources"));
    let external_sources = setup_user_external_sources(payload.get("external_sources"));
    let download_status = downloads
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or_default();
    Some(json!({
        "summary": payload.get("summary").cloned().unwrap_or_else(|| json!("")),
        "next_action": payload.get("next_action").cloned().unwrap_or_else(|| json!("运行安装/修复")),
        "downloads": {
            "completed": matches!(download_status, "ready" | "skipped"),
            "ready": downloads.get("ready").cloned().unwrap_or_else(|| json!(0)),
            "failed": downloads.get("failed").cloned().unwrap_or_else(|| json!(0)),
            "message": downloads.get("message").cloned().unwrap_or_else(|| json!("")),
            "next_action": downloads.get("next_action").cloned().unwrap_or_else(|| json!("运行安装/修复"))
        },
        "external_sources": external_sources,
        "resources": resources
    }))
}

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
        && tool_versions_ready(&store.join("indexes/tool_versions.tsv"))
        && install_status_ready(&store.join("install_status.json"))
        && source_requirements_ready(&store.join("source_requirements.tsv"))
}

fn install_status_ready(path: &Path) -> bool {
    let Ok(text) = fs::read_to_string(path) else {
        return false;
    };
    serde_json::from_str::<Value>(&text)
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
    ];
    required
        .iter()
        .map(|(id, rel, label, next_action)| {
            let path = store.join(rel);
            let ready = match *id {
                "tools" => tool_versions_ready(&path),
                "install_status" => install_status_ready(&path),
                "source_requirements" => source_requirements_ready(&path),
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
        "structure_interaction" => "结构互作",
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
                || count_alphanumeric(value) >= 2
        }
        "protein_ligand" => {
            looks_like_path_with_ext(value, &["fasta", "fa", "faa", "pdb", "cif", "mmcif"])
                || looks_like_fasta_or_sequence(value, 5)
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
            ) || looks_like_fasta_or_sequence(value, 8)
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

fn looks_like_fasta_or_sequence(value: &str, min_alphanumeric: usize) -> bool {
    let trimmed = value.trim();
    if trimmed.starts_with('>') {
        return count_alphanumeric(trimmed.lines().skip(1).collect::<String>().as_str())
            >= min_alphanumeric;
    }
    count_alphanumeric(trimmed) >= min_alphanumeric
}

fn count_alphanumeric(value: &str) -> usize {
    value.chars().filter(|ch| ch.is_alphanumeric()).count()
}

fn read_json_or(path: &Path, fallback: Value) -> Value {
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
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
    fn setup_install_plan_reports_catalog_coverage_without_paths() {
        let root = std::env::temp_dir().join(format!("genoligand-install-plan-{}", now_millis()));
        let config_dir = root.join("configs");
        fs::create_dir_all(&config_dir).expect("config dir");
        fs::write(
            config_dir.join("source_catalog.tsv"),
            "source_id\turl\tlocal_path\nchembl\thttps://example.invalid/chembl.tar.gz\tdownloads/chembl/chembl_sqlite.tar.gz\n",
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
        assert!(!text.contains("https://example"));
        assert!(!text.contains("downloads/"));
        assert!(!text.contains(&root.display().to_string()));

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn first_launch_user_workspace_has_no_seeded_projects_or_templates() {
        let root = std::env::temp_dir().join(format!("genoligand-first-launch-{}", now_millis()));
        let options = server_options(root.clone());
        let payload = user_projects_json(&options);

        assert_eq!(payload["projects"].as_array().map(Vec::len), Some(0));
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
    fn bundled_user_web_assets_avoid_demo_developer_and_mojibake_language() {
        let user_assets = [
            include_str!("../../../webui/app/index.html"),
            include_str!("../../../webui/app/app.js"),
        ]
        .join("\n");
        let forbidden_terms = [
            "真实", "demo", "Demo", "sample", "Sample", "example", "Example", "示例", "样例",
            "测试", "GitHub", "commit", "remote", "schema", "manifest", "hash", "raw JSON",
            "raw json", "gate", "route", "验收", "phase_0", "�", "灏", "鏁", "杈", "绌",
        ];

        for term in forbidden_terms {
            assert!(
                !user_assets.contains(term),
                "bundled user web assets expose forbidden term: {term}"
            );
        }
    }

    #[test]
    fn first_launch_input_view_does_not_render_resource_status_panels() {
        let app = include_str!("../../../webui/app/app.js");
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
        let app = include_str!("../../../webui/app/app.js");
        let start = app.find("function artifactRows()").expect("artifactRows");
        let end = app.find("function table(").expect("table");
        let artifact_rows = &app[start..end];

        assert!(artifact_rows.contains("filter((item) => item.exists)"));
        assert!(artifact_rows.contains("可下载"));
        assert!(!artifact_rows.contains("未生成"));
    }

    #[test]
    fn user_project_creation_hides_internal_status_and_actions() {
        let root = std::env::temp_dir().join(format!("genoligand-user-create-{}", now_millis()));
        let options = server_options(root.clone());

        let payload = create_user_project(&options);
        let text = payload.to_string();

        assert!(payload.get("project").is_some());
        assert!(payload["project"].get("project_id").is_some());
        assert!(payload.get("message").is_some());
        assert!(payload.get("status").is_none());
        assert!(!text.contains("created"));
        assert!(!text.contains("supported_actions"));
        assert!(!text.contains("preflight"));
        assert!(!text.contains("export"));

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
        assert!(!input_value_has_usable_user_input(&json!({
            "inputs":[{"field":"complex","value":"---"}]
        })));
        assert!(!input_value_has_usable_user_input(&json!({
            "inputs":[
                {"field":"small_molecule","value":"CCO"},
                {"field":"protein_ligand","value":"???"}
            ]
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

        assert_eq!(missing.len(), 6);
        assert!(text.contains("科学仓库"));
        assert!(text.contains("已知蛋白索引"));
        assert!(text.contains("宏基因组索引"));
        assert!(text.contains("计算工具"));
        assert!(text.contains("安装状态"));
        assert!(text.contains("科学资源状态"));
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
            "missing_science_store_manifest,missing_tool_versions_manifest,missing_known_database_index",
        ));

        assert!(summary.contains("缺少科学仓库"));
        assert!(summary.contains("计算工具未就绪"));
        assert!(summary.contains("缺少已知蛋白索引"));
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
            "rank\tcandidate_id\ttarget_id\ttarget_name\tcandidate_scope\torganism\tfinal_score\tknown_database_score\tmetagenome_score\tstructure_score\tdocking_score\tmd_score\tevidence_summary\tstatus\tsha256\tdebug_path\n1\tC1\tP24941\tCDK2\tknown_database\tHomo sapiens\t0.80\t0.70\t0.00\t0.10\t0.00\t0.00\tmatched current run\tcomputed_prediction\tabc123\tD:/hidden\n",
        )
        .expect("candidate table");
        fs::write(
            run_dir.join("metagenome_potential_matches.tsv"),
            "input_id\tsource\ttarget_id\ttarget_name\torganism\ttarget_class\tmatch_score\tmatched_features\tevidence\tsha256\tdebug_path\ninput_1\tD:/hidden/metagenome_targets.tsv\tMG1\tPotential ORF\tunknown\tprotein\t0.66\tmotif\tsequence evidence\tabc123\tD:/hidden\n",
        )
        .expect("genome context");
        fs::write(
            run_dir.join("evidence_ledger.tsv"),
            "claim_id\tevidence_artifact\towner\tstatus\tnotes\nscore_decomposition\tscore_decomposition.tsv\tGenoLigand\tavailable\tsha256:abc123\n",
        )
        .expect("evidence");
        fs::write(
            run_dir.join("candidate_detail.json"),
            r#"{"candidate_count":1,"candidates":[{"candidate_id":"C1","target_id":"P24941","target_name":"CDK2","candidate_scope":"known_database","organism":"Homo sapiens","scores":{"final":0.8,"known_database":0.7,"metagenome":0.0,"structure":0.1,"docking":0.0,"md":0.0},"evidence_summary":"matched current run","source_artifacts":["target_candidate_ranking.tsv"],"sha256":"abc123","path":"D:/hidden"}]}"#,
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

        let responses = [
            read_project(&options, "project-1"),
            run,
            artifacts_json(&options, "run-1"),
            user_databases_json(&options),
            user_setup_status_json(&options),
            user_setup_install_plan_json(&options),
            preflight_json(&options, "project-1"),
            logs_json(&options, "run-1"),
            candidate_detail_json(&options, "run-1"),
            evidence_endpoint_json(&options, "run-1"),
            candidates_endpoint_json(&options, "run-1"),
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
            "claim_id\tevidence_artifact\towner\tstatus\tnotes\nscore_decomposition\tscore_decomposition.tsv\tGenoLigand\tavailable\tsha256:abc123\n",
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
}
