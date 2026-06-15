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
    if let Some(response) = route_project(method, path, options, request_body) {
        return response;
    }
    if let Some(response) = route_run(method, path, options) {
        return response;
    }
    let value = match (method, path) {
        ("GET", "/api/health") | ("GET", "/api/user/health") => {
            json!({"status":"ready","service":"gls-server","version":PRODUCT_VERSION})
        }
        ("GET", "/api/version") => {
            json!({"name":PRODUCT_NAME,"version":PRODUCT_VERSION,"science_flow":"scientific_pipeline"})
        }
        ("GET", "/api/user/projects") => user_projects_json(options),
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
        ("GET", "candidates") => table_endpoint_json(
            options,
            &run_id,
            "target_candidate_ranking.tsv",
            "candidates",
        ),
        ("GET", "evidence") => {
            table_endpoint_json(options, &run_id, "evidence_ledger.tsv", "evidence")
        }
        ("GET", "genome-context") => table_endpoint_json(
            options,
            &run_id,
            "metagenome_potential_matches.tsv",
            "genome_context",
        ),
        ("GET", "structure") => table_endpoint_json(
            options,
            &run_id,
            "structure_patch_summary.tsv",
            "structure_rows",
        ),
        ("GET", "md-summary") => table_endpoint_json(options, &run_id, "md_summary.tsv", "md_rows"),
        ("POST", "cancel") => update_run_state(options, &run_id, "cancelled"),
        ("POST", "resume") => update_run_state(options, &run_id, "resumable"),
        _ => json!({"error":"not_found"}),
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
                projects.push(json!({
                    "project_id": project.get("project_id").cloned().unwrap_or(Value::Null),
                    "project_name": project.get("project_name").cloned().unwrap_or(Value::Null),
                    "project_status": project.get("project_status").cloned().unwrap_or(Value::Null),
                    "latest_run_id": project.get("latest_run_id").cloned().unwrap_or(Value::Null)
                }));
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
        "supported_actions": ["create","preflight","run","export"]
    })
}

fn create_project(options: &ServerOptions) -> Value {
    let project_id = format!("project-{}", now_millis());
    let dir = project_dir(options, &project_id);
    if let Err(err) = fs::create_dir_all(dir.join("runs")) {
        return json!({"status":"failed","error":err.to_string()});
    }
    let _ = write_text(
        &dir.join("input_manifest.json"),
        "{\"inputs\":[],\"files\":[],\"status\":\"waiting_for_user_input\"}\n",
    );
    let project = read_project(options, &project_id);
    json!({"status":"created","project":project})
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
        "project_name": read_trimmed(&dir.join("project_name.txt")).unwrap_or_else(|| "GenoLigand research project".to_string()),
        "workspace_path": dir.display().to_string(),
        "project_status": status,
        "latest_run_id": latest_run_id,
        "contains_precomputed_results": false,
        "saved_state": ["input_manifest","run_state","artifact_index","report_paths"],
        "collaboration_mode": "local_editable"
    })
}

fn read_input_manifest(options: &ServerOptions, project_id: &str) -> Value {
    let path = project_dir(options, project_id).join("input_manifest.json");
    read_json_or(
        &path,
        json!({"inputs":[],"files":[],"status":"waiting_for_user_input"}),
    )
}

fn write_input_manifest(options: &ServerOptions, project_id: &str, body: &str) -> Value {
    let dir = project_dir(options, project_id);
    let path = dir.join("input_manifest.json");
    let value =
        serde_json::from_str::<Value>(body).unwrap_or_else(|_| json!({"inputs":[],"files":[]}));
    if let Some(name) = input_project_name(&value) {
        let _ = write_text(&dir.join("project_name.txt"), &name);
    }
    match write_text(&path, &(value.to_string() + "\n")) {
        Ok(()) => {
            json!({"status":"saved","input_manifest":value,"path":path.display().to_string()})
        }
        Err(err) => json!({"status":"failed","error":err}),
    }
}

fn preflight_json(options: &ServerOptions, project_id: &str) -> Value {
    let input = project_dir(options, project_id).join("input_manifest.json");
    let store = science_store_path(options);
    let checks = vec![
        check(
            "input_manifest",
            input.exists() && input_has_user_value(&input),
            input.display().to_string(),
        ),
        check(
            "science_store_manifest",
            store.join("science_store_manifest.tsv").exists(),
            store
                .join("science_store_manifest.tsv")
                .display()
                .to_string(),
        ),
        check(
            "known_database_index",
            store.join("indexes/known_targets.tsv").exists(),
            store
                .join("indexes/known_targets.tsv")
                .display()
                .to_string(),
        ),
        check(
            "metagenome_potential_index",
            store.join("indexes/metagenome_targets.tsv").exists(),
            store
                .join("indexes/metagenome_targets.tsv")
                .display()
                .to_string(),
        ),
        check(
            "tool_versions_manifest",
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
    json!({
        "run_id": run_id,
        "state": run_state(&run_dir),
        "current_stage": read_trimmed(&run_dir.join("current_stage.txt")).unwrap_or_default(),
        "current_command": read_trimmed(&run_dir.join("current_command.txt")),
        "failure_reason": read_trimmed(&run_dir.join("failure_reason.txt")),
        "output_dir": run_dir.display().to_string(),
        "artifact_index": artifact_index(&run_dir)
    })
}

fn logs_json(options: &ServerOptions, run_id: &str) -> Value {
    let text = find_run_dir(options, run_id)
        .and_then(|dir| fs::read_to_string(dir.join("run.log")).ok())
        .unwrap_or_default();
    json!({"logs":text.lines().collect::<Vec<_>>()})
}

fn artifacts_json(options: &ServerOptions, run_id: &str) -> Value {
    find_run_dir(options, run_id)
        .map(|dir| json!({"run_id":run_id,"artifact_root":dir.display().to_string(),"items":artifact_index(&dir)["items"].clone()}))
        .unwrap_or_else(|| json!({"run_id":run_id,"items":[]}))
}

fn table_endpoint_json(options: &ServerOptions, run_id: &str, file: &str, key: &str) -> Value {
    let Some(run_dir) = find_run_dir(options, run_id) else {
        return json!({"run_id":run_id,key:[],"row_count":0,"artifact_backed":false});
    };
    let path = run_dir.join(file);
    let rows = read_tsv_rows(&path);
    json!({
        "run_id": run_id,
        "source_path": if path.exists() {Some(path.display().to_string())} else {None},
        key: rows,
        "row_count": rows_len(&path),
        "artifact_backed": path.exists()
    })
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

fn user_databases_json(options: &ServerOptions) -> Value {
    let store = science_store_path(options);
    let required = [
        ("science_store_manifest.tsv", "科学仓库"),
        ("indexes/known_targets.tsv", "已知蛋白索引"),
        ("indexes/metagenome_targets.tsv", "宏基因组索引"),
        ("indexes/tool_versions.tsv", "计算工具"),
    ];
    let rows = required
        .iter()
        .map(|(rel, label)| {
            let path = store.join(rel);
            let ready = if *rel == "indexes/tool_versions.tsv" {
                tool_versions_ready(&path)
            } else {
                path.exists()
            };
            json!({"item":rel,"label":label,"ready":ready})
        })
        .collect::<Vec<_>>();
    json!({
        "required": rows,
        "sources": source_status_rows(&store),
        "repair_actions_available": store.join("repair_actions.tsv").exists(),
        "user_configures_paths": false
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
    let cargo = capture("cargo", &["--version"], &options.workspace_root);
    let gls_cli = capture_command(&gls_cli_help_command(options), &options.workspace_root);
    let database_ready = preflight_store_ready(options);
    let checks = vec![
        json!({"label":"服务", "ready":true, "next_action":"继续"}),
        json!({"label":"命令行工具", "ready":gls_cli.0, "next_action": if gls_cli.0 {"继续"} else {"重新安装 GenoLigand 命令行工具"}}),
        json!({"label":"构建环境", "ready":cargo.0, "next_action": if cargo.0 {"继续"} else {"安装 Rust 构建环境"}}),
        json!({"label":"科学仓库", "ready":database_ready, "next_action": if database_ready {"继续"} else {"运行安装/修复"}}),
    ];
    json!({
        "status": if cargo.0 && gls_cli.0 && database_ready {"ready"} else {"warning"},
        "runtime": std::env::consts::OS,
        "checks": checks,
        "science_store": user_databases_json(options)
    })
}

fn user_setup_status_json(options: &ServerOptions) -> Value {
    let ready = preflight_store_ready(options);
    json!({
        "status": if ready {"ready"} else {"needs_repair"},
        "database_ready": ready,
        "runtime": std::env::consts::OS,
        "can_run_repair": true,
        "repair_action": "prepare_science_store",
        "checks": user_databases_json(options)["required"].clone()
    })
}

fn run_setup_repair_json(options: &ServerOptions) -> Value {
    let command = setup_repair_command(options);
    let output = Command::new(&command[0])
        .args(&command[1..])
        .current_dir(&options.workspace_root)
        .output();
    let (executed, message) = match output {
        Ok(output) if output.status.success() => (true, "修复动作已执行。".to_string()),
        Ok(output) => (
            false,
            format!(
                "修复动作未完成，退出码 {:?}。",
                output.status.code().unwrap_or_default()
            ),
        ),
        Err(err) => (false, format!("修复动作无法启动：{err}")),
    };
    let ready = preflight_store_ready(options);
    json!({
        "status": if executed {"repair_executed"} else {"repair_failed"},
        "database_ready": ready,
        "message": if ready {"科学仓库已就绪。"} else {message.as_str()},
        "checks": user_databases_json(options)["required"].clone()
    })
}

fn user_setup_install_plan_json(options: &ServerOptions) -> Value {
    json!({
        "status":"ready_to_run",
        "action":"prepare_science_store",
        "description":"准备数据库、索引、工具版本和许可证记录。普通运行不配置数据库路径。",
        "can_run_from_webui": true,
        "database_ready": preflight_store_ready(options)
    })
}

fn setup_repair_command(options: &ServerOptions) -> Vec<String> {
    let mut command = gls_cli_invocation(options);
    command.extend([
        "install".to_string(),
        "prepare-science-store".to_string(),
        "--science-store".to_string(),
        science_store_path(options).display().to_string(),
    ]);
    command
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
            json!({
                "source_id": get("source_id"),
                "label": get("label"),
                "category": get("category"),
                "status": get("status"),
                "ready": get("status") == "ready",
                "next_action": get("repair_action")
            })
        })
        .collect()
}

fn check(id: &str, ready: bool, detail: String) -> Value {
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
    value
        .get("inputs")
        .and_then(Value::as_array)
        .map(|inputs| {
            inputs.iter().any(|item| {
                item.get("field")
                    .and_then(Value::as_str)
                    .map(|field| field != "project_name")
                    .unwrap_or(false)
                    && item
                        .get("value")
                        .and_then(Value::as_str)
                        .map(|value| !value.trim().is_empty())
                        .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

fn read_json_or(path: &Path, fallback: Value) -> Value {
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .unwrap_or(fallback)
}

fn artifact_index(run_dir: &Path) -> Value {
    let mut items = Vec::new();
    for name in [
        "run_context.json",
        "run_context.tsv",
        "artifact_source_map.tsv",
        "input_normalized_manifest.json",
        "feature_manifest.json",
        "known_database_matches.tsv",
        "metagenome_potential_matches.tsv",
        "structure_patch_summary.tsv",
        "docking_summary.tsv",
        "md_summary.tsv",
        "target_candidate_ranking.tsv",
        "candidate_detail.json",
        "score_decomposition.tsv",
        "evidence_ledger.tsv",
        "research_report.md",
    ] {
        let path = run_dir.join(name);
        items.push(json!({"path":name,"exists":path.exists()}));
    }
    json!({"items":items})
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
