use super::*;
use std::sync::atomic::AtomicUsize;

fn server_options(workspace_root: PathBuf) -> ServerOptions {
    ServerOptions {
        host: "127.0.0.1".to_string(),
        port: 0,
        artifact_dir: workspace_root.join(".genoligand-test-runtime"),
        workspace_root,
    }
}

fn write_passed_authenticity_gate(run_dir: &Path) {
    fs::write(
        run_dir.join("authenticity_gate.tsv"),
        "artifact\tstatus\tline_number\tmarker\treason\nfinal_run_artifacts\tpassed\t0\t\tclean\n",
    )
    .expect("authenticity gate");
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
    let managed_python = options
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
    let script = PathBuf::from(r"D:\workspace\GenoLigand\work\probe.py");

    assert_eq!(
        python_probe_script_arg(
            r"D:\runtime\python-worker-env\genoligand-wsl-python.cmd",
            &script
        ),
        "/mnt/d/workspace/GenoLigand/work/probe.py"
    );
    assert_eq!(
        python_probe_script_arg(r"D:\runtime\python-worker-env\python.cmd", &script),
        r"D:\workspace\GenoLigand\work\probe.py"
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

#[test]
fn start_run_reuses_existing_running_run() {
    let root = std::env::temp_dir().join(format!("genoligand-running-run-{}", now_millis()));
    let options = server_options(root.clone());
    let project_id = "project-1";
    let project = project_dir(&options, project_id);
    let run_dir = project.join("runs").join("run-existing");
    fs::create_dir_all(&run_dir).expect("run dir");
    fs::write(
        project.join("input_manifest.json"),
        r#"{"inputs":[{"field":"small_molecule","value":"CCO"}],"files":[]}"#,
    )
    .expect("input");
    fs::write(project.join("latest_run_id.txt"), "run-existing\n").expect("latest run");
    fs::write(run_dir.join("run_state.txt"), "running\n").expect("state");
    fs::write(
        run_dir.join("process_id.txt"),
        format!("{}\n", std::process::id()),
    )
    .expect("process id");
    fs::write(run_dir.join("current_stage.txt"), "features_compute\n").expect("stage");

    let response = start_run(&options, project_id);
    let run_count = fs::read_dir(project.join("runs"))
        .expect("runs")
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_dir())
        .count();

    assert_eq!(response["status"], "running");
    assert_eq!(response["run"]["run_id"], "run-existing");
    assert_eq!(response["run"]["state"], "running");
    assert_eq!(run_count, 1);
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
    let root = std::env::temp_dir().join(format!("genoligand-workspace-summary-{}", now_millis()));
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
    write_passed_authenticity_gate(&run_dir);

    let summary = user_workspace_summary_json(&options);
    let projects = user_projects_json(&options);
    let draft = projects["projects"]
        .as_array()
        .and_then(|items| {
            items
                .iter()
                .find(|item| item["project_id"] == "hidden-project")
        })
        .expect("draft project");
    assert_eq!(summary["project_count"], 2);
    assert_eq!(summary["run_count"], 2);
    assert_eq!(summary["candidate_count"], 2);
    assert_eq!(summary["result_count"], 4);
    assert_eq!(summary["export_item_count"], 3);
    assert_eq!(projects["projects"].as_array().map(Vec::len), Some(2));
    assert_eq!(draft["project_status"], "draft");
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
        "pages/project.js",
        "pages/input.js",
        "pages/check.js",
        "pages/run.js",
        "pages/results.js",
        "pages/report.js",
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
    let app = web_asset_text("pages/report.js");
    let start = app.find("function renderReport()").expect("renderReport");
    let report_view = &app[start..];

    assert!(report_view.contains("filter((item) => item.exists)"));
    assert!(report_view.contains("可下载"));
    assert!(!report_view.contains("未生成"));
}

#[test]
fn export_view_only_renders_downloadable_artifacts() {
    let app = web_asset_text("pages/report.js");
    let start = app.find("function renderReport()").expect("renderReport");
    let export_view = &app[start..];

    assert!(export_view.contains("available.map"));
    assert!(export_view.contains("当前运行还没有可下载报告材料"));
    assert!(export_view.contains("下载"));
    assert!(!export_view.contains("当前运行未生成"));
    assert!(!export_view.contains("不可用"));
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
    assert!(selectors.contains("if (step === \"project\") return true"));
    assert!(
        selectors
            .contains("if (step === \"results\" || step === \"report\") return canViewResults();")
    );
    assert!(!selectors.contains("\"configure\""));
    assert!(!selectors.contains("\"export\""));
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
    let project = web_asset_text("pages/project.js");
    let input = web_asset_text("pages/input.js");

    assert!(app.contains("/api/user/workspace-summary"));
    assert!(app.contains("/api/user/projects"));
    assert!(project.contains("历史项目"));
    assert!(project.contains("data-open-project"));
    assert!(project.contains("data-delete-project"));
    assert!(project.contains("createNewProject"));
    assert!(!input.contains("继续已有项目"));
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
    let common = web_asset_text("components/common.js");
    assert!(app.contains("三维查看未就绪"));
    assert!(app.contains("轨迹图未就绪"));
    assert!(app.contains("protocol.candidate_count"));
    assert!(app.contains("stateTrace.candidate_count"));
    assert!(common.contains("trajectory_status"));
    assert!(common.contains("轨迹数据"));
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
    assert_eq!(md["trajectory_status"], "轨迹数据未就绪");
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
    assert_eq!(listed["projects"].as_array().map(Vec::len), Some(1));
    assert_eq!(listed["projects"][0]["project_status"], "draft");
    assert!(payload.get("status").is_none());
    assert!(!text.contains("created"));
    assert!(!text.contains("supported_actions"));
    assert!(!text.contains("preflight"));
    assert!(!text.contains("export"));

    fs::remove_dir_all(&root).ok();
}

#[test]
fn user_project_delete_route_removes_project_directory() {
    let root = std::env::temp_dir().join(format!("genoligand-user-delete-{}", now_millis()));
    let options = server_options(root.clone());
    let project = create_project_record(&options).expect("project");
    let project_id = project["project_id"]
        .as_str()
        .expect("project id")
        .to_string();
    let project_path = project_dir(&options, &project_id);

    let (status, _, body) = route(
        "DELETE",
        &format!("/api/user/projects/{project_id}"),
        &options,
        "",
    );
    let value: Value = serde_json::from_str(&body).expect("delete json");

    assert_eq!(status, 200);
    assert_eq!(value["status"], "deleted");
    assert!(!project_path.exists());
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
    let root = std::env::temp_dir().join(format!("genoligand-user-health-{}", now_millis()));
    let payload = user_health_json(&server_options(root.clone()));
    let text = payload.to_string();

    assert_eq!(payload["status"], "ready");
    assert_eq!(payload["application"], PRODUCT_NAME);
    assert_eq!(payload["version"], PRODUCT_VERSION);
    assert_eq!(payload["commit"], PRODUCT_COMMIT);
    assert_eq!(payload["build_id"], PRODUCT_BUILD_ID);
    assert!(payload["runtime"].is_object());
    assert_eq!(payload["runtime"]["version"], PRODUCT_VERSION);
    assert_eq!(payload["runtime"]["commit"], PRODUCT_COMMIT);
    assert_eq!(payload["runtime"]["build_id"], PRODUCT_BUILD_ID);
    assert_eq!(
        payload["science_store_readiness"]["schema_version"],
        "genoligand_science_store_readiness_v1"
    );
    assert!(payload["science_store_readiness"]["ready"].is_boolean());
    assert!(payload["science_store_readiness"]["blockers"].is_array());
    let (status, _, version_body) = route("GET", "/api/version", &server_options(root.clone()), "");
    let version: Value = serde_json::from_str(&version_body).expect("version json");
    assert_eq!(status, 200);
    assert_eq!(version["version"], PRODUCT_VERSION);
    assert_eq!(version["commit"], PRODUCT_COMMIT);
    assert_eq!(version["build_id"], PRODUCT_BUILD_ID);
    assert_eq!(version["runtime"]["version"], PRODUCT_VERSION);
    assert!(!text.contains("gls-server"));
    assert!(!text.contains("route"));
    assert!(!text.contains("debug"));
    fs::remove_dir_all(root).ok();
}

#[test]
fn runtime_health_removes_dead_instance_record() {
    let root = std::env::temp_dir().join(format!("genoligand-dead-instance-{}", now_millis()));
    fs::create_dir_all(&root).expect("runtime root");
    let record = root.join("gls-server.instance.json");
    fs::write(
        &record,
        json!({
            "instance_id":"dead-instance",
            "binary":"gls-server",
            "pid":u32::MAX,
            "started_at":1
        })
        .to_string(),
    )
    .expect("dead instance record");

    let status = reconcile_runtime_instance_record(&root);

    assert_eq!(status["status"], "stale_removed");
    assert!(!record.exists());
    fs::remove_dir_all(root).ok();
}

#[test]
fn health_and_setup_expose_canonical_readiness_blocker_evidence_and_repair() {
    let root =
        std::env::temp_dir().join(format!("genoligand-canonical-readiness-{}", now_millis()));
    let options = server_options(root.clone());
    let store = science_store_path(&options);
    fs::create_dir_all(&store).expect("science store");
    fs::write(
        store.join("science_store_readiness.json"),
        json!({
            "schema_version": "genoligand_science_store_readiness_v1",
            "status": "needs_repair",
            "ready": false,
            "generated_at": 1,
            "evidence_manifest": [],
            "evidence_digest": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            "sources": [{"logical_source": "rcsb_ligand_bound", "status": "needs_repair"}],
            "indexes": [{"index": "structure_targets", "readiness_status": "incomplete"}],
            "blockers": [{
                "code": "rcsb_scope_incomplete",
                "component": "rcsb",
                "evidence": "ready=7 scope=9",
                "repair_action": "sync_rcsb_ligand_bound_scope"
            }],
            "advisories": [{
                "code": "rcsb_foldseek_refresh_needed",
                "component": "rcsb",
                "evidence": "cached coordinate set changed after the published snapshot",
                "action": "refresh_foldseek_when_convenient"
            }],
            "repair_actions": ["sync_rcsb_ligand_bound_scope"]
        })
        .to_string(),
    )
    .expect("canonical readiness");

    let health = user_health_json(&options);
    let setup = user_setup_status_json(&options);
    for readiness in [&health["science_store_readiness"], &setup["readiness"]] {
        assert_eq!(readiness["status"], "needs_repair");
        assert_eq!(readiness["blockers"][0], "rcsb_scope_incomplete");
        assert_eq!(
            readiness["blocker_details"][0]["evidence"],
            "ready=7 scope=9"
        );
        assert_eq!(
            readiness["blocker_details"][0]["repair_action"],
            "sync_rcsb_ligand_bound_scope"
        );
        assert_eq!(
            readiness["sources"][0]["logical_source"],
            "rcsb_ligand_bound"
        );
        assert_eq!(readiness["indexes"][0]["index"], "structure_targets");
        assert_eq!(readiness["advisories"][0], "rcsb_foldseek_refresh_needed");
        assert_eq!(
            readiness["advisory_details"][0]["action"],
            "refresh_foldseek_when_convenient"
        );
    }
    fs::remove_dir_all(root).ok();
}

#[test]
fn user_setup_status_exposes_authoritative_readiness_without_runtime_controls() {
    let root = std::env::temp_dir().join(format!("genoligand-user-setup-status-{}", now_millis()));
    let options = server_options(root.clone());
    let payload = user_setup_status_json(&options);
    let text = payload.to_string();

    assert!(payload.get("database_ready").is_some());
    assert!(payload.get("repair_action").is_some());
    assert!(payload.get("status").is_none());
    assert!(payload.get("runtime").is_none());
    assert!(payload.get("can_run_repair").is_none());
    assert_eq!(
        payload["readiness"]["schema_version"],
        "genoligand_science_store_readiness_v1"
    );
    assert!(payload["readiness"]["ready"].is_boolean());
    assert!(payload["readiness"]["blockers"].is_array());
    assert!(!text.contains("runtime"));
    assert!(!text.contains("can_run_repair"));

    fs::remove_dir_all(&root).ok();
}

#[test]
fn user_diagnostics_hides_runtime_and_status_codes() {
    let root = std::env::temp_dir().join(format!("genoligand-user-diagnostics-{}", now_millis()));
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
    let root = std::env::temp_dir().join(format!("genoligand-openmm-platforms-{}", now_millis()));
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
    let root = std::env::temp_dir().join(format!("genoligand-openmm-cuda-reason-{}", now_millis()));
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

fn server_options_with_compound_lookup(root: &Path, data_row: &str) -> ServerOptions {
    let options = server_options(root.to_path_buf());
    let store = science_store_path(&options);
    fs::create_dir_all(store.join("indexes")).expect("indexes");
    fs::write(
        store.join("indexes/compound_identifiers.tsv"),
        format!(
            "identifier_type\tidentifier\tcanonical_smiles\tdisplay_name\tsource\n{data_row}\ttest\n"
        ),
    )
    .expect("compound lookup");
    options
}

fn inspection_pending_files(options: &ServerOptions) -> Vec<PathBuf> {
    let dir = options.artifact_dir.join("input-inspection");
    fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn user_input_inspection_uses_science_resolver() {
    let root = std::env::temp_dir().join(format!("genoligand-user-inspection-{}", now_millis()));
    let options = server_options_with_compound_lookup(
        &root,
        "name\tcaffeine\tCn1c(=O)n(C)c2ncn(C)c2c1=O\tCaffeine",
    );

    let response = inspect_user_input_json(
        &options,
        r#"{"inputs":[{"field":"small_molecule","value":"caffeine"}],"files":[]}"#,
    );

    assert_eq!(response["usable"], true);
    assert_eq!(response["errors"], json!([]));
    assert_eq!(response["inputs"][0]["input_id"], "input_1");
    assert_eq!(response["inputs"][0]["record_id"], "record_1");
    assert_eq!(response["inputs"][0]["canonical_kind"], "small_molecule");
    assert_eq!(
        response["inputs"][0]["conversion_method"],
        "local_identifier_lookup"
    );
    assert!(inspection_pending_files(&options).is_empty());
    fs::remove_dir_all(&root).ok();
}

#[test]
fn user_input_inspection_rejects_missing_structure_path() {
    let root = std::env::temp_dir().join(format!("genoligand-missing-inspection-{}", now_millis()));
    let options = server_options(root.clone());

    let response = inspect_user_input_json(
        &options,
        r#"{"inputs":[{"field":"small_molecule","value":"Z:\\missing\\ghost.sdf"}],"files":[]}"#,
    );

    assert_eq!(response["usable"], false);
    assert_eq!(response["error"], "input_file_not_found");
    assert!(
        response["errors"][0]
            .as_str()
            .unwrap_or_default()
            .contains("input_file_not_found")
    );
    assert!(inspection_pending_files(&options).is_empty());
    fs::remove_dir_all(&root).ok();
}

#[test]
fn user_input_inspection_cleanup_failure_preserves_resolver_cause() {
    let root = std::env::temp_dir().join(format!(
        "genoligand-inspection-cleanup-cause-{}",
        now_millis()
    ));
    let options = server_options(root.clone());

    let response = inspect_user_input_json_with_cleanup(
        &options,
        r#"{"inputs":[{"field":"small_molecule","value":"Z:\\missing\\ghost.sdf"}],"files":[]}"#,
        |_| Err("input_manifest_cleanup_failed:injected".to_string()),
    );

    assert_eq!(response["error"], "input_manifest_cleanup_failed");
    assert_eq!(response["cause"]["error"], "input_file_not_found");
    let errors = response["errors"].as_array().expect("errors");
    assert!(errors.iter().any(|error| {
        error
            .as_str()
            .unwrap_or_default()
            .contains("input_manifest_cleanup_failed")
    }));
    assert!(errors.iter().any(|error| {
        error
            .as_str()
            .unwrap_or_default()
            .contains("input_file_not_found")
    }));
    fs::remove_dir_all(&root).ok();
}

#[test]
fn user_input_inspection_rejects_malformed_json() {
    let root =
        std::env::temp_dir().join(format!("genoligand-malformed-inspection-{}", now_millis()));
    let options = server_options(root.clone());

    let response = inspect_user_input_json(&options, r#"{"inputs":["#);

    assert_eq!(response["usable"], false);
    assert_eq!(response["error"], "invalid_input_manifest_json");
    assert!(response["inputs"].as_array().expect("inputs").is_empty());
    assert!(inspection_pending_files(&options).is_empty());
    fs::remove_dir_all(&root).ok();
}

#[test]
fn user_input_inspection_rejects_project_only_and_empty_input() {
    let root = std::env::temp_dir().join(format!("genoligand-empty-inspection-{}", now_millis()));
    let options = server_options(root.clone());

    for body in [
        r#"{"inputs":[],"files":[]}"#,
        r#"{"inputs":[{"field":"project_name","value":"研究项目"}],"files":[]}"#,
    ] {
        let response = inspect_user_input_json(&options, body);
        assert_eq!(response["usable"], false);
        assert_eq!(response["error"], "input_not_usable");
        assert!(response["inputs"].as_array().expect("inputs").is_empty());
    }
    assert!(inspection_pending_files(&options).is_empty());
    fs::remove_dir_all(&root).ok();
}

#[test]
fn user_input_inspection_rejects_mixed_manifest_with_bad_record() {
    let root = std::env::temp_dir().join(format!("genoligand-mixed-inspection-{}", now_millis()));
    let options = server_options(root.clone());
    let response = inspect_user_input_json(
        &options,
        r#"{"inputs":[{"field":"small_molecule","value":"CCO"},{"field":"small_molecule","value":"Z:\\missing\\ghost.sdf"}],"files":[]}"#,
    );

    assert_eq!(response["usable"], false);
    assert_eq!(response["error"], "input_file_not_found");
    assert_eq!(response["inputs"].as_array().expect("inputs").len(), 2);
    assert_eq!(response["inputs"][0]["original_field"], "small_molecule");
    assert_eq!(response["inputs"][0]["usable"], true);
    assert_eq!(response["inputs"][1]["original_field"], "small_molecule");
    assert_eq!(response["inputs"][1]["usable"], false);
    assert_eq!(
        response["inputs"][1]["message"],
        "找不到输入文件，请检查文件路径。"
    );
    assert_eq!(response["errors"].as_array().expect("errors").len(), 1);
    fs::remove_dir_all(&root).ok();
}

#[test]
fn user_input_inspection_parses_error_after_colon_record_id() {
    let root = std::env::temp_dir().join(format!(
        "genoligand-colon-record-inspection-{}",
        now_millis()
    ));
    let options = server_options(root.clone());
    let fasta = root.join("invalid-peptide.fasta");
    fs::create_dir_all(&root).expect("root");
    fs::write(&fasta, ">sample\nACDEFG\n>sample:chain\nZZ\n").expect("fasta");
    let body = json!({
        "inputs": [{"field": "peptide", "value": fasta.display().to_string()}],
        "files": []
    })
    .to_string();

    let response = inspect_user_input_json(&options, &body);

    assert_eq!(response["usable"], false);
    assert_eq!(response["inputs"][1]["record_id"], "sample:chain__record_2");
    assert_eq!(response["error"], "invalid_peptide_sequence");
    fs::remove_dir_all(&root).ok();
}

#[test]
fn user_input_inspection_cleans_unique_pending_files() {
    let root = std::env::temp_dir().join(format!("genoligand-clean-inspection-{}", now_millis()));
    let options = server_options(root.clone());
    let body = r#"{"inputs":[{"field":"peptide","value":"ACDEFG"}],"files":[]}"#;

    assert_eq!(inspect_user_input_json(&options, body)["usable"], true);
    assert_eq!(inspect_user_input_json(&options, body)["usable"], true);
    assert!(inspection_pending_files(&options).is_empty());
    fs::remove_dir_all(&root).ok();
}

#[test]
fn user_input_inspection_route_returns_authoritative_response() {
    let root = std::env::temp_dir().join(format!("genoligand-inspection-route-{}", now_millis()));
    let options = server_options_with_compound_lookup(
        &root,
        "name\tcaffeine\tCn1c(=O)n(C)c2ncn(C)c2c1=O\tCaffeine",
    );
    let (status, _, body) = route(
        "POST",
        "/api/user/inputs/inspect",
        &options,
        r#"{"inputs":[{"field":"small_molecule","value":"caffeine"}],"files":[]}"#,
    );
    let response: Value = serde_json::from_str(&body).expect("inspection response");

    assert_eq!(status, 200);
    assert_eq!(response["usable"], true);
    assert_eq!(response["inputs"][0]["original_field"], "small_molecule");
    assert_eq!(response["inputs"][0]["usable"], true);
    assert_eq!(
        response["inputs"][0]["conversion_method"],
        "local_identifier_lookup"
    );
    assert!(inspection_pending_files(&options).is_empty());
    fs::remove_dir_all(&root).ok();
}

#[test]
fn user_input_save_rejects_missing_structure_path() {
    let root = std::env::temp_dir().join(format!("genoligand-missing-save-{}", now_millis()));
    let options = server_options(root.clone());
    let project = project_dir(&options, "project-1");
    fs::create_dir_all(&project).expect("project");

    let response = write_input_manifest(
        &options,
        "project-1",
        r#"{"inputs":[{"field":"small_molecule","value":"Z:\\missing\\ghost.sdf"}],"files":[]}"#,
    );

    assert_eq!(response["status"], "failed");
    assert_eq!(response["error"], "input_file_not_found");
    assert_eq!(response["inspection"]["usable"], false);
    assert!(!project.join("input_manifest.pending.json").exists());
    assert!(!project.join("input_manifest.json").exists());
    fs::remove_dir_all(&root).ok();
}

#[test]
fn user_input_save_replaces_existing_manifest_and_updates_project_state() {
    let root = std::env::temp_dir().join(format!("genoligand-replace-save-{}", now_millis()));
    let options = server_options(root.clone());
    let project = project_dir(&options, "project-1");
    fs::create_dir_all(&project).expect("project");

    let first = write_input_manifest(
        &options,
        "project-1",
        r#"{"inputs":[{"field":"project_name","value":"第一个项目"},{"field":"small_molecule","value":"CCO"}],"files":[]}"#,
    );
    assert_eq!(first["status"], "saved");
    fs::write(project.join("latest_run_id.txt"), "run-old\n").expect("latest run");

    let second = write_input_manifest(
        &options,
        "project-1",
        r#"{"inputs":[{"field":"project_name","value":"第二个项目"},{"field":"small_molecule","value":"CCN"}],"files":[]}"#,
    );
    let manifest = read_json_or(&project.join("input_manifest.json"), json!({}));

    assert_eq!(second["status"], "saved");
    assert_eq!(manifest["inputs"][1]["value"], "CCN");
    assert_eq!(
        read_trimmed(&project.join("project_name.txt")).as_deref(),
        Some("第二个项目")
    );
    assert!(!project.join("latest_run_id.txt").exists());
    assert!(!project.join("input_manifest.pending.json").exists());
    fs::remove_dir_all(&root).ok();
}

#[test]
fn user_input_save_failed_promotion_preserves_formal_state() {
    let root = std::env::temp_dir().join(format!("genoligand-failed-promotion-{}", now_millis()));
    let options = server_options(root.clone());
    let project = project_dir(&options, "project-1");
    fs::create_dir_all(&project).expect("project");
    let old_manifest = r#"{"inputs":[{"field":"project_name","value":"旧项目"},{"field":"peptide","value":"ACDEFG"}],"files":[]}"#;
    fs::write(project.join("input_manifest.json"), old_manifest).expect("old manifest");
    fs::write(project.join("project_name.txt"), "旧项目\n").expect("old project name");
    fs::write(project.join("latest_run_id.txt"), "run-old\n").expect("old run");

    let response = write_input_manifest_with_promoter(
        &options,
        "project-1",
        r#"{"inputs":[{"field":"project_name","value":"新项目"},{"field":"peptide","value":"MSTNPKPQR"}],"files":[]}"#,
        |_, _| Err("input_manifest_replace_failed:test".to_string()),
    );

    assert_eq!(response["status"], "failed");
    assert_eq!(response["error"], "input_manifest_replace_failed");
    assert_eq!(response["inspection"]["cause"]["usable"], true);
    assert_eq!(
        response["inspection"]["cause"]["inputs"][0]["canonical_kind"],
        "peptide"
    );
    assert_eq!(
        fs::read_to_string(project.join("input_manifest.json")).expect("manifest"),
        old_manifest
    );
    assert_eq!(
        read_trimmed(&project.join("project_name.txt")).as_deref(),
        Some("旧项目")
    );
    assert_eq!(
        read_trimmed(&project.join("latest_run_id.txt")).as_deref(),
        Some("run-old")
    );
    assert!(!project.join("input_manifest.pending.json").exists());
    fs::remove_dir_all(&root).ok();
}

#[test]
fn user_input_save_reports_cleanup_failure_with_original_context() {
    let root =
        std::env::temp_dir().join(format!("genoligand-failed-save-cleanup-{}", now_millis()));
    let options = server_options(root.clone());
    fs::create_dir_all(project_dir(&options, "project-1")).expect("project");

    let response = write_input_manifest_with_operations(
        &options,
        "project-1",
        r#"{"inputs":[{"field":"small_molecule","value":"Z:\\missing\\ghost.sdf"}],"files":[]}"#,
        |_, _| panic!("unusable inspection must not promote"),
        |_| Err("input_manifest_cleanup_failed:injected".to_string()),
    );

    assert_eq!(response["status"], "failed");
    assert_eq!(response["error"], "input_manifest_cleanup_failed");
    assert_eq!(
        response["inspection"]["cause"]["error"],
        "input_file_not_found"
    );
    assert!(
        response["inspection"]["errors"]
            .as_array()
            .expect("errors")
            .iter()
            .any(|error| error
                .as_str()
                .unwrap_or_default()
                .contains("input_file_not_found"))
    );
    fs::remove_dir_all(&root).ok();
}

#[test]
fn user_input_save_does_not_cleanup_after_successful_promotion() {
    let root =
        std::env::temp_dir().join(format!("genoligand-successful-promotion-{}", now_millis()));
    let options = server_options(root.clone());
    let project = project_dir(&options, "project-1");
    fs::create_dir_all(&project).expect("project");

    let response = write_input_manifest_with_operations(
        &options,
        "project-1",
        r#"{"inputs":[{"field":"project_name","value":"新项目"},{"field":"peptide","value":"ACDEFG"}],"files":[]}"#,
        |pending, formal| {
            fs::rename(pending, formal).map_err(|err| format!("promotion_failed:{err}"))
        },
        |_| panic!("successful promotion consumes the pending manifest"),
    );

    assert_eq!(response["status"], "saved");
    assert!(project.join("input_manifest.json").exists());
    assert!(!project.join("input_manifest.pending.json").exists());
    fs::remove_dir_all(&root).ok();
}

#[test]
fn input_validation_rejects_malformed_save_without_state_changes() {
    let root = std::env::temp_dir().join(format!("genoligand-malformed-save-{}", now_millis()));
    let options = server_options(root.clone());
    let project = project_dir(&options, "project-1");
    fs::create_dir_all(&project).expect("project");
    let old_manifest = r#"{"inputs":[{"field":"peptide","value":"ACDEFG"}],"files":[]}"#;
    fs::write(project.join("input_manifest.json"), old_manifest).expect("formal manifest");
    fs::write(project.join("input_manifest.pending.json"), "stale pending").expect("pending");
    fs::write(project.join("project_name.txt"), "旧项目\n").expect("project name");
    fs::write(project.join("latest_run_id.txt"), "run-old\n").expect("latest run");

    let response = write_input_manifest(&options, "project-1", r#"{"inputs":["#);

    assert_eq!(response["status"], "failed");
    assert_eq!(response["error"], "invalid_input_manifest_json");
    assert_eq!(
        fs::read_to_string(project.join("input_manifest.json")).expect("formal manifest"),
        old_manifest
    );
    assert!(!project.join("input_manifest.pending.json").exists());
    assert_eq!(
        read_trimmed(&project.join("project_name.txt")).as_deref(),
        Some("旧项目")
    );
    assert_eq!(
        read_trimmed(&project.join("latest_run_id.txt")).as_deref(),
        Some("run-old")
    );
    fs::remove_dir_all(&root).ok();
}

#[test]
fn input_validation_malformed_save_reports_cleanup_failure_with_json_cause() {
    let root = std::env::temp_dir().join(format!("genoligand-malformed-cleanup-{}", now_millis()));
    let options = server_options(root.clone());
    fs::create_dir_all(project_dir(&options, "project-1")).expect("project");

    let response = write_input_manifest_with_operations(
        &options,
        "project-1",
        r#"{"inputs":["#,
        |_, _| panic!("malformed JSON must not promote"),
        |_| Err("input_manifest_cleanup_failed:injected".to_string()),
    );

    assert_eq!(response["status"], "failed");
    assert_eq!(response["error"], "input_manifest_cleanup_failed");
    assert_eq!(
        response["inspection"]["cause"]["error"],
        "invalid_input_manifest_json"
    );
    assert!(
        response["inspection"]["errors"]
            .as_array()
            .expect("errors")
            .iter()
            .any(|error| error
                .as_str()
                .unwrap_or_default()
                .contains("invalid_input_manifest_json"))
    );
    fs::remove_dir_all(&root).ok();
}

#[test]
fn input_validation_preflight_uses_science_resolver() {
    let root = std::env::temp_dir().join(format!("genoligand-science-preflight-{}", now_millis()));
    let options = server_options(root.clone());
    let project = project_dir(&options, "project-1");
    fs::create_dir_all(&project).expect("project");
    fs::write(
        project.join("input_manifest.json"),
        r#"{"inputs":[{"field":"small_molecule","value":"Z:\\missing\\ghost.sdf"}],"files":[]}"#,
    )
    .expect("manifest");

    let preflight = user_preflight_json(&options, "project-1");
    let input = preflight["checks"]
        .as_array()
        .expect("checks")
        .iter()
        .find(|row| row["id"] == "input")
        .expect("input check");
    assert_eq!(input["ready"], false);
    assert!(
        input["detail"]
            .as_str()
            .unwrap_or_default()
            .contains("找不到输入文件")
    );
    assert!(!preflight.to_string().contains(&root.display().to_string()));
    fs::remove_dir_all(&root).ok();
}

#[test]
fn input_validation_has_no_server_owned_recognizer_module() {
    let module = ["input", "recognition"].join("_");
    let source_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let lib_source = fs::read_to_string(source_root.join("lib.rs")).expect("server source");
    let repo_root = source_root
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .expect("repository root");
    let e2e_source = fs::read_to_string(repo_root.join("tests/e2e/check_pipeline_contracts.py"))
        .expect("pipeline contract checker");
    let removed_path = format!("src/{module}.rs");

    assert!(!source_root.join(format!("{module}.rs")).exists());
    assert!(!lib_source.contains(&format!("mod {module}")));
    assert!(!e2e_source.replace('\\', "/").contains(&removed_path));
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
    let root = std::env::temp_dir().join(format!("genoligand-missing-summary-{}", now_millis()));
    let rows = required_resource_rows(&root);
    let missing = missing_resource_rows(&rows);
    let text = serde_json::to_string(&missing).expect("missing json");

    assert_eq!(missing.len(), 9);
    assert!(text.contains("科学仓库"));
    assert!(text.contains("已知蛋白索引"));
    assert!(text.contains("宏基因组索引"));
    assert!(text.contains("结构索引"));
    assert!(text.contains("索引完整性"));
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

    let md_summary = friendly_failure_summary(Some(
        "schema_version=genoligand_pipeline_failure_v1\nstate=failed\nreason=all_selected_md_candidates_failed:candidate_A:openmm_platform_unavailable;candidate_B:no_receptor_chain_within_pose_contact_cutoff\n",
    ));
    assert!(md_summary.starts_with("全部选中的 MD 候选均未产生有效结果："));
    assert!(md_summary.contains("candidate_A:openmm_platform_unavailable"));
    assert!(md_summary.contains("candidate_B:no_receptor_chain_within_pose_contact_cutoff"));
}

#[test]
fn user_diagnostics_do_not_request_developer_build_tools() {
    let root = std::env::temp_dir().join(format!("genoligand-user-diagnostics-{}", now_millis()));
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

    let (status, _, body) = download_artifact_response(&options, "run-1", "research_report");
    assert_eq!(status, 409);
    assert!(body.contains("authenticity_gate_failed"));

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

    let (status, _, body) = download_artifact_response(&options, "run-1", "research_report");
    assert_eq!(status, 409);
    assert!(body.contains("authenticity_gate_failed"));

    write_passed_authenticity_gate(&run_dir);
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
fn candidate_ranking_download_returns_full_completed_table() {
    let root = std::env::temp_dir().join(format!("genoligand-download-full-{}", now_millis()));
    let options = server_options(root.clone());
    let run_dir = projects_root(&options)
        .join("project-1")
        .join("runs")
        .join("run-1");
    fs::create_dir_all(&run_dir).expect("create run dir");
    fs::write(run_dir.join("run_state.txt"), "completed\n").expect("state");
    let mut ranking = String::from(
        "rank\tcandidate_id\ttarget_id\ttarget_name\tcandidate_scope\torganism\tfinal_score\tknown_database_score\tmetagenome_score\tstructure_score\tdocking_score\tmd_score\tevidence_summary\tstatus\n",
    );
    for index in 1..=505 {
        ranking.push_str(&format!(
            "{index}\ttarget_T{index}\tT{index}\tTarget {index}\tknown_human_protein\tHomo sapiens\t0.1\t0.2\t0.0\t0.0\t0.0\t0.0\tcurrent evidence\tcomputed_prediction\n"
        ));
    }
    fs::write(run_dir.join("target_candidate_ranking.tsv"), ranking).expect("ranking");
    write_passed_authenticity_gate(&run_dir);

    let (status, _, body) = download_artifact_response(&options, "run-1", "candidate_ranking");

    assert_eq!(status, 200);
    assert!(body.contains("target_T505"));
    assert_eq!(body.lines().count(), 506);
    fs::remove_dir_all(&root).ok();
}

#[test]
fn artifact_download_is_blocked_when_authenticity_gate_reports_blocked_rows() {
    let root = std::env::temp_dir().join(format!("genoligand-download-gated-{}", now_millis()));
    let options = server_options(root.clone());
    let run_dir = projects_root(&options)
        .join("project-1")
        .join("runs")
        .join("run-1");
    fs::create_dir_all(&run_dir).expect("create run dir");
    fs::write(run_dir.join("run_state.txt"), "completed\n").expect("state");
    fs::write(
        run_dir.join("target_candidate_ranking.tsv"),
        "rank\tcandidate_id\ttarget_id\ttarget_name\tcandidate_scope\torganism\tfinal_score\tknown_database_score\tmetagenome_score\tstructure_score\tdocking_score\tmd_score\tevidence_summary\tstatus\n1\ttarget_T1\tT1\tTarget 1\tknown_human_protein\tHomo sapiens\t0.1\t0.2\t0.0\t0.0\t0.0\t0.0\tstatic_fallback_candidate\tcomputed_prediction\n",
    )
    .expect("ranking");
    fs::write(
        run_dir.join("authenticity_gate.tsv"),
        "artifact\tstatus\tline_number\tmarker\treason\ntarget_candidate_ranking.tsv\tblocked\t2\tstatic_fallback_candidate\tstatic fallback row\n",
    )
    .expect("gate");

    let (status, _, body) = download_artifact_response(&options, "run-1", "candidate_ranking");

    assert_eq!(status, 409);
    assert!(body.contains("authenticity_gate_failed"));
    assert!(!body.contains("static_fallback_candidate"));
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
    write_passed_authenticity_gate(&run_dir);

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
fn user_input_save_endpoint_returns_structured_validation_error() {
    let root = std::env::temp_dir().join(format!("genoligand-user-flow-{}", now_millis()));
    let options = server_options(root.clone());
    fs::create_dir_all(project_dir(&options, "project-1")).expect("project dir");

    let invalid = write_user_input_manifest(
        &options,
        "project-1",
        r#"{"inputs":[{"field":"project_name","value":"研究项目"},{"field":"small_molecule","value":"Z:\\missing\\ghost.sdf"}],"files":[]}"#,
    );
    let invalid_text = invalid.to_string();
    assert_eq!(invalid["ok"], false);
    assert_eq!(invalid["error"], "input_file_not_found");
    assert_eq!(invalid["inspection"]["usable"], false);
    assert!(invalid.get("message").is_some());
    assert!(invalid.get("status").is_none());
    assert!(!invalid_text.contains(r#""status":"failed""#));

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
    let root = std::env::temp_dir().join(format!("genoligand-preflight-store-{}", now_millis()));
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
fn preflight_rejects_row_limited_science_indexes() {
    let root =
        std::env::temp_dir().join(format!("genoligand-preflight-index-limit-{}", now_millis()));
    let options = server_options(root.clone());
    let store = root.join("science_store");
    fs::create_dir_all(store.join("indexes")).expect("store indexes");
    write_ready_source_records(&store);
    write_ready_tool_versions(&store);
    fs::write(
        store.join("science_store_manifest.tsv"),
        "item\tstatus\tpath\nknown_database_index\tready\tindexes/known_targets.tsv\nmetagenome_potential_index\tready\tindexes/metagenome_targets.tsv\nstructure_interaction_index\tready\tindexes/structure_targets.tsv\ntool_versions\tready\tindexes/tool_versions.tsv\n",
    )
    .expect("store manifest");
    fs::write(
        store.join("indexes/known_targets.tsv"),
        "source\ttarget_id\ttarget_name\torganism\ttarget_class\tligand_tokens\tsequence_tokens\tstructure_tokens\tevidence\n",
    )
    .expect("known index");
    fs::write(
        store.join("indexes/metagenome_targets.tsv"),
        "source\ttarget_id\ttarget_name\torganism\ttarget_class\tligand_tokens\tsequence_tokens\tstructure_tokens\tevidence\n",
    )
    .expect("metagenome index");
    fs::write(
        store.join("indexes/structure_targets.tsv"),
        "source\ttarget_id\ttarget_name\torganism\ttarget_class\tligand_tokens\tsequence_tokens\tstructure_tokens\tevidence\n",
    )
    .expect("structure index");
    fs::write(
        store.join("install_status.json"),
        r#"{"status":"ready","missing_components":[]}"#,
    )
    .expect("install status");
    fs::write(
        store.join("source_requirements.tsv"),
        "source_id\tstatus\nchembl\tready\n",
    )
    .expect("source requirements");
    fs::write(
        store.join("index_build_manifest.tsv"),
        "index\tinput_path\tstatus\trows\toutput_path\tnotes\nknown_targets\tdownloads/chembl/chembl_sqlite.tar.gz\tused\t30000\tindexes/known_targets.tsv\tchembl_sqlite_archive_rows;index_row_limit_reached\n",
    )
    .expect("index build manifest");

    assert!(!preflight_store_ready(&options));
    let resources = required_resource_rows(&store);
    let index_build = resources
        .iter()
        .find(|row| row.get("id").and_then(Value::as_str) == Some("index_build"))
        .expect("index build row");
    assert_eq!(index_build["ready"], false);

    fs::remove_dir_all(&root).ok();
}

#[test]
fn saving_new_input_clears_current_run_selection() {
    let root = std::env::temp_dir().join(format!("genoligand-input-clears-run-{}", now_millis()));
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
fn running_run_marks_completed_prior_stages_and_current_stage() {
    let root = std::env::temp_dir().join(format!(
        "genoligand-running-stage-progress-{}",
        now_millis()
    ));
    let options = server_options(root.clone());
    let run_dir = project_dir(&options, "project-1")
        .join("runs")
        .join("run-1");
    fs::create_dir_all(&run_dir).expect("run dir");
    fs::write(run_dir.join("run_state.txt"), "running\n").expect("state");
    fs::write(run_dir.join("current_stage.txt"), "structure_evaluate\n").expect("stage");

    let payload = user_run_json(&options, "run-1");
    let stages = payload["stages"].as_array().expect("stages");
    let by_id = |id: &str| {
        stages
            .iter()
            .find(|stage| stage.get("id").and_then(Value::as_str) == Some(id))
            .expect("stage id")
    };

    assert_eq!(payload["current_stage"], "结构候选检索");
    assert_eq!(
        stages
            .iter()
            .filter_map(|stage| stage.get("id").and_then(Value::as_str))
            .collect::<Vec<_>>(),
        [
            "input",
            "query_structure",
            "features",
            "sequence_alignment",
            "known",
            "metagenome",
            "structure",
            "shortlist",
            "md",
            "ranking",
            "evidence",
            "report",
        ]
    );
    assert_eq!(by_id("input")["status"], "ready");
    assert_eq!(by_id("query_structure")["status"], "ready");
    assert_eq!(by_id("features")["status"], "ready");
    assert_eq!(by_id("sequence_alignment")["status"], "ready");
    assert_eq!(by_id("known")["status"], "ready");
    assert_eq!(by_id("metagenome")["status"], "ready");
    assert_eq!(by_id("structure")["status"], "running");
    assert_eq!(by_id("shortlist")["status"], "waiting");
    assert_eq!(by_id("md")["status"], "waiting");
    assert_eq!(by_id("report")["status"], "waiting");

    fs::remove_dir_all(&root).ok();
}

#[test]
fn user_run_stages_track_sequence_alignment_and_report_render() {
    let root = std::env::temp_dir().join(format!(
        "genoligand-user-run-stage-contract-{}",
        now_millis()
    ));
    let options = server_options(root.clone());
    let run_dir = project_dir(&options, "project-1")
        .join("runs")
        .join("run-1");
    fs::create_dir_all(&run_dir).expect("run dir");
    fs::write(run_dir.join("run_state.txt"), "running\n").expect("state");

    fs::write(
        run_dir.join("current_stage.txt"),
        "onsite_sequence_alignment\n",
    )
    .expect("sequence alignment stage");
    let alignment = user_run_json(&options, "run-1");
    assert_eq!(alignment["current_stage"], "本地序列比对");
    let alignment_stages = alignment["stages"].as_array().expect("alignment stages");
    assert_eq!(
        alignment_stages
            .iter()
            .find(|stage| stage["id"] == "sequence_alignment")
            .expect("sequence alignment")["status"],
        "running"
    );
    assert_eq!(
        alignment_stages
            .iter()
            .find(|stage| stage["id"] == "known")
            .expect("known stage")["status"],
        "waiting"
    );

    fs::write(run_dir.join("current_stage.txt"), "report_render\n").expect("report stage");
    let report = user_run_json(&options, "run-1");
    assert_eq!(report["current_stage"], "报告生成");
    let report_stages = report["stages"].as_array().expect("report stages");
    assert_eq!(
        report_stages
            .iter()
            .find(|stage| stage["id"] == "evidence")
            .expect("evidence stage")["status"],
        "ready"
    );
    assert_eq!(
        report_stages
            .iter()
            .find(|stage| stage["id"] == "report")
            .expect("report stage")["status"],
        "running"
    );

    let app_state = web_asset_text("state/app-state.js");
    let features = app_state.find("\"结构特征编码\"").expect("features label");
    let alignment = app_state.find("\"本地序列比对\"").expect("alignment label");
    let known = app_state
        .find("\"数据库精确与相似性检索\"")
        .expect("known label");
    let evidence = app_state.find("\"证据链整理\"").expect("evidence label");
    let report = app_state.find("\"报告生成\"").expect("report label");
    assert!(features < alignment && alignment < known);
    assert!(evidence < report);

    fs::remove_dir_all(&root).ok();
}

#[test]
fn completed_run_marks_missing_structure_and_md_stages_not_generated() {
    let root = std::env::temp_dir().join(format!("genoligand-user-run-stages-{}", now_millis()));
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
    write_passed_authenticity_gate(&run_dir);

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
            .contains("当前 run 的复合物 MD 证据已参与候选排序")
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
    fs::create_dir_all(run_dir.join("tool_outputs")).expect("tool outputs");
    fs::write(
        run_dir.join("known_database_matches.tsv"),
        "input_id\tsource\ttarget_id\ttarget_name\torganism\ttarget_class\tmatch_score\tmatched_features\tevidence\tdebug_path\nI1\tbindingdb_onsite\tP00918\tCA2\tHomo sapiens\tenzyme\t0.9\tacetazolamide\tassay\tD:/hidden\n",
    )
    .expect("known");
    fs::write(
        run_dir.join("query_structure_manifest.tsv"),
        "input_id\trecord_id\tinput_kind\tinput_digest\tmethod\tfeatures_json\tcoordinate_artifact\tfeature_digest\tstatus\nI1\tR1\tsmall_molecule\tinternal-input-digest\tETKDGv3\ttool_outputs/query/features.json\ttool_outputs/query/representative.sdf\tinternal-feature-digest\tready\n",
    )
    .expect("query structure");
    fs::write(
        run_dir.join("candidate_shortlist.tsv"),
        "shortlist_rank\tcandidate_id\tinput_id\trecord_id\ttarget_id\tpreliminary_score\tshortlist_reason\tcoordinate_status\tdebug_path\n1\tC1\tI1\tR1\tP00918\t0.9\tknown_database=0.9\tready\tD:/hidden\n",
    )
    .expect("candidate shortlist");
    fs::write(
        run_dir.join("docking_summary.tsv"),
        "candidate_id\tdocking_status\tdocking_score\tdocking_engine\tnotes\tdebug_path\nC1\tcompleted\t-6.4\tvina\tpose ready\tD:/hidden\n",
    )
    .expect("docking");
    fs::write(
        run_dir.join("tool_outputs/md_protocol.tsv"),
        "candidate_id\tinput_id\tstage\tparameter\tvalue\tunit\tsource\tdebug_path\nC1\tI1\tplatform\topenmm_platform\tCUDA\t\tOpenMM\tD:/hidden\n",
    )
    .expect("protocol");
    fs::write(
        run_dir.join("tool_outputs/md_state.tsv"),
        "candidate_id\tinput_id\tframe\tstep\ttime_ps\tstage\tpotential_energy_kj_mol\tkinetic_energy_kj_mol\ttotal_energy_kj_mol\ttemperature_estimate_K\tsource\tdebug_path\nC1\tI1\t1\t1000\t2.0000\tproduction\t-1.0\t1.0\t0.0\t300.0\tOpenMM Context state\tD:/hidden\n",
    )
    .expect("state");
    fs::write(
        run_dir.join("tool_outputs/gpu_telemetry.tsv"),
        "sample_label\ttimestamp\tgpu_index\tname\tutilization_gpu_percent\tmemory_used_mb\tmemory_total_mb\tpower_draw_w\tcurrent_pid_observed\tcompute_process_count\tcompute_process_memory_used_mb\tfailure_reason\tdebug_path\nproduction_poll_1\t2026-07-03\t0\tNVIDIA\t70\t2048\t8192\t80\ttrue\t1\t2048\t\tD:/hidden\n",
    )
    .expect("gpu");
    write_passed_authenticity_gate(&run_dir);

    for artifact_id in [
        "query_structure",
        "candidate_shortlist",
        "candidate_ranking",
        "evidence",
        "structure_patch",
        "md_summary",
        "onsite_search",
        "docking_summary",
        "md_protocol",
        "md_state",
        "gpu_telemetry",
    ] {
        let (status, content_type, body) =
            download_artifact_response(&options, "run-1", artifact_id);
        assert_eq!(status, 200);
        assert_eq!(content_type, "text/tab-separated-values; charset=utf-8");
        if matches!(
            artifact_id,
            "candidate_ranking" | "evidence" | "structure_patch" | "md_summary"
        ) {
            assert!(body.contains("输出材料"));
        }
        if artifact_id == "query_structure" {
            assert!(body.contains("输入编号\t记录编号\t输入类型\t准备方法"));
            assert!(body.contains("ETKDGv3"));
        }
        if artifact_id == "candidate_shortlist" {
            assert!(body.contains("短名单排序\t候选编号\t输入编号"));
            assert!(body.contains("known_database=0.9"));
        }
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
    let root = std::env::temp_dir().join(format!("genoligand-candidate-detail-{}", now_millis()));
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
    write_passed_authenticity_gate(&run_dir);

    let payload = candidate_detail_json(&options, "run-1");
    assert_eq!(payload["candidate_count"], 1);
    assert_eq!(payload["candidate_details"][0]["candidate_id"], "C1");
    assert_eq!(payload["candidate_details"][0]["target_id"], "P24941");
    assert_eq!(payload["candidate_details"][0]["input_id"], "");
    assert_eq!(payload["candidate_details"][0]["record_id"], "");
    assert_eq!(payload["candidate_details"][0]["input_kind"], "");
    assert_eq!(payload["candidate_details"][0]["ranking_policy"], "");
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
fn candidate_identity_and_ranking_route_survive_api_download_and_webui() {
    let root = std::env::temp_dir().join(format!(
        "genoligand-candidate-identity-{}",
        now_millis()
    ));
    let options = server_options(root.clone());
    let run_dir = projects_root(&options)
        .join("project-1")
        .join("runs")
        .join("run-1");
    fs::create_dir_all(&run_dir).expect("create run dir");
    fs::write(run_dir.join("run_state.txt"), "completed\n").expect("state");
    fs::write(
        run_dir.join("target_candidate_ranking.tsv"),
        "rank\tcandidate_id\ttarget_id\ttarget_name\tcandidate_scope\torganism\tfinal_score\tknown_database_score\tmetagenome_score\tstructure_score\tdocking_score\tmd_score\tevidence_summary\tstatus\tinput_id\trecord_id\tidentity_namespace\tinput_kind\tranking_policy\tknown_database_weight\tmetagenome_weight\tstructure_weight\tdocking_weight\tmd_weight\n1\tC-input-1\tP24941\tCDK2\tknown_human_protein\tHomo sapiens\t0.80\t0.70\t0.00\t0.10\t0.20\t0.30\tinput one evidence\tcomputed_prediction\tinput_1\trecord_1\tinput_1:record_1\tsmall_molecule\tsmall_molecule_route_v1\t0.45\t0.00\t0.20\t0.15\t0.20\n1\tC-input-2\tP24941\tCDK2\tknown_human_protein\tHomo sapiens\t0.75\t0.50\t0.10\t0.40\t0.00\t0.00\tinput two evidence\tcomputed_prediction\tinput_2\trecord_2\tinput_2:record_2\tpeptide\tpeptide_route_v2\t0.30\t0.20\t0.50\t0.00\t0.00\n",
    )
    .expect("ranking");
    fs::write(
        run_dir.join("candidate_detail.json"),
        r#"{"candidate_count":2,"candidates":[{"candidate_id":"C-input-1","input_id":"input_1","record_id":"record_1","input_kind":"small_molecule","ranking_policy":"small_molecule_route_v1","ranking_weights":{"known_database":0.45,"metagenome":0.0,"structure":0.2,"docking":0.15,"md":0.2},"target_id":"P24941","target_name":"CDK2","candidate_scope":"known_human_protein","organism":"Homo sapiens","scores":{"final":0.8,"known_database":0.7,"metagenome":0.0,"structure":0.1,"docking":0.2,"md":0.3},"evidence_summary":"input one evidence"},{"candidate_id":"C-input-2","input_id":"input_2","record_id":"record_2","input_kind":"peptide","ranking_policy":"peptide_route_v2","ranking_weights":{"known_database":0.3,"metagenome":0.2,"structure":0.5,"docking":0.0,"md":0.0},"target_id":"P24941","target_name":"CDK2","candidate_scope":"known_human_protein","organism":"Homo sapiens","scores":{"final":0.75,"known_database":0.5,"metagenome":0.1,"structure":0.4,"docking":0.0,"md":0.0},"evidence_summary":"input two evidence"}]}"#,
    )
    .expect("candidate detail");
    write_passed_authenticity_gate(&run_dir);

    let candidates = candidates_endpoint_json(&options, "run-1");
    assert_eq!(candidates["row_count"], 2);
    assert_eq!(candidates["candidates"][0]["target_id"], "P24941");
    assert_eq!(candidates["candidates"][1]["target_id"], "P24941");
    assert_eq!(candidates["candidates"][0]["input_id"], "input_1");
    assert_eq!(candidates["candidates"][1]["record_id"], "record_2");
    assert_eq!(candidates["candidates"][0]["input_kind"], "small_molecule");
    assert_eq!(
        candidates["candidates"][1]["ranking_policy"],
        "peptide_route_v2"
    );
    assert_eq!(
        candidates["candidates"][0]["known_database_weight"],
        "0.45"
    );

    let detail = candidate_detail_json(&options, "run-1");
    assert_eq!(detail["candidate_count"], 2);
    assert_eq!(detail["candidate_details"][0]["input_id"], "input_1");
    assert_eq!(detail["candidate_details"][1]["record_id"], "record_2");
    assert_eq!(detail["candidate_details"][0]["input_kind"], "small_molecule");
    assert_eq!(
        detail["candidate_details"][1]["ranking_policy"],
        "peptide_route_v2"
    );
    assert_eq!(
        detail["candidate_details"][1]["structure_weight"]
            .as_f64()
            .unwrap(),
        0.5
    );

    let (status, _, download) =
        download_artifact_response(&options, "run-1", "candidate_ranking");
    assert_eq!(status, 200);
    assert_eq!(download.lines().count(), 3);
    assert!(download.starts_with("排序\t输入编号\t记录编号\t输入类型\t排序路线\t"));
    assert!(download.contains("input_1\trecord_1\tsmall_molecule\tsmall_molecule_route_v1"));
    assert!(download.contains("input_2\trecord_2\tpeptide\tpeptide_route_v2"));

    let common = web_asset_text("components/common.js");
    let results = web_asset_text("pages/results.js");
    for field in ["input_id", "record_id", "input_kind", "ranking_policy"] {
        assert!(common.contains(field));
    }
    assert!(common.contains("rankingWeightSummary"));
    assert!(results.contains("metricRow(\"输入编号\""));
    assert!(results.contains("metricRow(\"排序路线\""));
    assert!(results.contains("metricRow(\"路线权重\""));

    fs::remove_dir_all(&root).ok();
}

#[test]
fn result_endpoints_expose_pagination_metadata_for_large_tables() {
    let root = std::env::temp_dir().join(format!("genoligand-pagination-{}", now_millis()));
    let options = server_options(root.clone());
    let run_dir = projects_root(&options)
        .join("project-1")
        .join("runs")
        .join("run-1");
    fs::create_dir_all(&run_dir).expect("create run dir");
    fs::write(run_dir.join("run_state.txt"), "completed\n").expect("state");
    let mut ranking = String::from(
        "rank\tcandidate_id\ttarget_id\ttarget_name\tcandidate_scope\torganism\tfinal_score\tknown_database_score\tmetagenome_score\tstructure_score\tdocking_score\tmd_score\tevidence_summary\tstatus\n",
    );
    for index in 1..=505 {
        ranking.push_str(&format!(
            "{index}\ttarget_T{index}\tT{index}\tTarget {index}\tknown_human_protein\tHomo sapiens\t0.1\t0.2\t0.0\t0.0\t0.0\t0.0\tcurrent evidence\tcomputed_prediction\n"
        ));
    }
    fs::write(run_dir.join("target_candidate_ranking.tsv"), ranking).expect("ranking");
    write_passed_authenticity_gate(&run_dir);

    let payload = candidates_endpoint_json(&options, "run-1");

    assert_eq!(payload["row_count"], 505);
    assert_eq!(payload["candidates"].as_array().expect("rows").len(), 500);
    assert_eq!(payload["pagination"]["total_rows"], 505);
    assert_eq!(payload["pagination"]["returned_rows"], 500);
    assert_eq!(payload["pagination"]["page"], 1);
    assert_eq!(payload["pagination"]["page_size"], 500);
    assert_eq!(payload["pagination"]["has_next_page"], true);
    assert_eq!(payload["pagination"]["filters_active"], false);
    fs::remove_dir_all(&root).ok();
}

#[test]
fn result_routes_apply_page_and_filter_query_parameters() {
    let root = std::env::temp_dir().join(format!("genoligand-pagination-query-{}", now_millis()));
    let options = server_options(root.clone());
    let run_dir = projects_root(&options)
        .join("project-1")
        .join("runs")
        .join("run-1");
    fs::create_dir_all(&run_dir).expect("create run dir");
    fs::write(run_dir.join("run_state.txt"), "completed\n").expect("state");
    let mut ranking = String::from(
        "rank\tcandidate_id\ttarget_id\ttarget_name\tcandidate_scope\torganism\tfinal_score\tknown_database_score\tmetagenome_score\tstructure_score\tdocking_score\tmd_score\tevidence_summary\tstatus\n",
    );
    for index in 1..=505 {
        ranking.push_str(&format!(
            "{index}\ttarget_T{index}\tT{index}\tTarget {index}\tknown_human_protein\tHomo sapiens\t0.1\t0.2\t0.0\t0.0\t0.0\t0.0\tcurrent evidence\tcomputed_prediction\n"
        ));
    }
    fs::write(run_dir.join("target_candidate_ranking.tsv"), ranking).expect("ranking");
    write_passed_authenticity_gate(&run_dir);

    let (status, _, body) = route(
        "GET",
        "/api/user/runs/run-1/candidates?page=2&page_size=200",
        &options,
        "",
    );
    assert_eq!(status, 200);
    let payload: Value = serde_json::from_str(&body).expect("page payload");
    assert_eq!(payload["row_count"], 505);
    assert_eq!(payload["candidates"].as_array().expect("rows").len(), 200);
    assert_eq!(payload["candidates"][0]["candidate_id"], "target_T201");
    assert_eq!(payload["pagination"]["page"], 2);
    assert_eq!(payload["pagination"]["page_size"], 200);
    assert_eq!(payload["pagination"]["has_previous_page"], true);
    assert_eq!(payload["pagination"]["has_next_page"], true);

    let (status, _, body) = route(
        "GET",
        "/api/user/runs/run-1/candidates?filter=T505",
        &options,
        "",
    );
    assert_eq!(status, 200);
    let payload: Value = serde_json::from_str(&body).expect("filter payload");
    assert_eq!(payload["row_count"], 505);
    assert_eq!(payload["candidates"].as_array().expect("rows").len(), 1);
    assert_eq!(payload["candidates"][0]["candidate_id"], "target_T505");
    assert_eq!(payload["pagination"]["filtered_rows"], 1);
    assert_eq!(payload["pagination"]["filters_active"], true);
    assert_eq!(payload["pagination"]["active_filters"][0], "T505");
    fs::remove_dir_all(&root).ok();
}

struct GeneratedTsvReader {
    total_rows: usize,
    next_row: usize,
    header_emitted: bool,
    buffer: Vec<u8>,
    consumed: usize,
    read_calls: Arc<AtomicUsize>,
    max_buffered_bytes: Arc<AtomicUsize>,
}

impl GeneratedTsvReader {
    fn new(
        total_rows: usize,
        read_calls: Arc<AtomicUsize>,
        max_buffered_bytes: Arc<AtomicUsize>,
    ) -> Self {
        Self {
            total_rows,
            next_row: 1,
            header_emitted: false,
            buffer: Vec::new(),
            consumed: 0,
            read_calls,
            max_buffered_bytes,
        }
    }

    fn refill(&mut self) {
        if self.consumed < self.buffer.len() {
            return;
        }
        self.buffer.clear();
        self.consumed = 0;
        if !self.header_emitted {
            self.buffer.extend_from_slice(b"rank\tcandidate_id\n");
            self.header_emitted = true;
        } else if self.next_row <= self.total_rows {
            self.buffer.extend_from_slice(
                format!("{}\tC{:06}\n", self.next_row, self.next_row).as_bytes(),
            );
            self.next_row += 1;
        }
        self.max_buffered_bytes
            .fetch_max(self.buffer.len(), Ordering::Relaxed);
    }
}

impl std::io::Read for GeneratedTsvReader {
    fn read(&mut self, _buffer: &mut [u8]) -> std::io::Result<usize> {
        self.read_calls.fetch_add(1, Ordering::Relaxed);
        Err(std::io::Error::other("full-reader path is forbidden"))
    }
}

impl std::io::BufRead for GeneratedTsvReader {
    fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
        self.refill();
        Ok(&self.buffer[self.consumed..])
    }

    fn consume(&mut self, amount: usize) {
        self.consumed = self.consumed.saturating_add(amount).min(self.buffer.len());
    }
}

#[test]
fn large_tsv_late_page_uses_bounded_bufread_scan() {
    let read_calls = Arc::new(AtomicUsize::new(0));
    let max_buffered_bytes = Arc::new(AtomicUsize::new(0));
    let reader = GeneratedTsvReader::new(100_000, read_calls.clone(), max_buffered_bytes.clone());
    let query = ResultQuery::parse("page=20000&page_size=3");

    let page = read_tsv_rows_page_from(reader, &query).expect("stream late page");

    assert_eq!(page.total_rows, 100_000);
    assert_eq!(page.filtered_rows, 100_000);
    assert_eq!(page.rows.len(), 3);
    assert_eq!(page.rows[0]["candidate_id"], "C059998");
    assert_eq!(page.rows[2]["candidate_id"], "C060000");
    assert_eq!(read_calls.load(Ordering::Relaxed), 0);
    assert!(max_buffered_bytes.load(Ordering::Relaxed) < 64);
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
    write_passed_authenticity_gate(&run_dir);

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
    write_passed_authenticity_gate(&run_dir);

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
    fs::create_dir_all(run_dir.join("tool_outputs")).expect("tool outputs");
    fs::write(
        run_dir.join("tool_outputs/md_protocol.tsv"),
        "candidate_id\tinput_id\tstage\tparameter\tvalue\tunit\tsource\nC1\tI1\tplatform\topenmm_platform\tCUDA\t\tOpenMM\nC1\tI1\tintegrator\tintegrator\tLangevinMiddleIntegrator\t\tOpenMM\nC1\tI1\tintegrator\ttime_step\t0.0020\tps\tOpenMM\nC1\tI1\tequilibration\ttime\t20.0000\tps\tOpenMM\nC1\tI1\tproduction\ttotal_time\t100.0000\tps\tOpenMM\nC1\tI1\tproduction\ttotal_steps\t50000\tsteps\tOpenMM\nC1\tI1\toutput\tframe_count\t50\tframes\tmd_trajectory.tsv\n",
    )
    .expect("protocol");
    fs::write(
        run_dir.join("tool_outputs/md_state.tsv"),
        "candidate_id\tinput_id\tframe\tstep\ttime_ps\tstage\tpotential_energy_kj_mol\tkinetic_energy_kj_mol\ttotal_energy_kj_mol\ttemperature_estimate_K\tsource\nC1\tI1\t0\t0\t0.0000\tminimized\t-1.0000\t0.0000\t-1.0000\t0.0000\tOpenMM Context state\nC1\tI1\t50\t60000\t120.0000\tproduction\t-2.0000\t1.0000\t-1.0000\t300.0000\tOpenMM Context state\n",
    )
    .expect("state");
    fs::write(
        run_dir.join("tool_outputs/gpu_telemetry_status.tsv"),
        "cuda_platform\tnvidia_smi_available\tgpu_observed\tmax_utilization_gpu_percent\tmax_memory_delta_mb\tprocess_observed\tsample_count\tproduction_sample_count\tproduction_max_utilization_gpu_percent\tproduction_max_memory_delta_mb\tmonitoring_mode\tobservation_basis\tfailure_reason\nCUDA\ttrue\ttrue\t70.0000\t512.0000\ttrue\t4\t2\t70.0000\t512.0000\tnvidia-smi_polling\tcurrent_pid\t\n",
    )
    .expect("gpu status");
    write_passed_authenticity_gate(&run_dir);

    let payload = md_endpoint_json(&options, "run-1");
    assert_eq!(payload["has_output"], true);
    assert_eq!(payload["readiness"]["ready"], true);
    assert_eq!(payload["protocol"]["platform"], "CUDA");
    assert_eq!(payload["state_trace"]["last_temperature_K"], "300.0000");
    assert_eq!(payload["cuda_monitoring"]["ready"], true);
    assert_eq!(
        payload["cuda_monitoring"]["observation_basis"],
        "current_pid"
    );
    assert!(payload["charts"].as_array().unwrap().len() >= 5);
    let text = payload.to_string();
    assert!(text.contains("md_summary.tsv"));
    assert!(text.contains("score_decomposition.tsv"));
    assert!(text.contains("gpu_telemetry_status.tsv"));
    assert!(!text.contains(&root.display().to_string()));

    fs::remove_dir_all(&root).ok();
}

#[test]
fn md_endpoint_reads_candidate_scoped_openmm_artifacts() {
    let root = std::env::temp_dir().join(format!("genoligand-md-scoped-{}", now_millis()));
    let options = server_options(root.clone());
    let run_dir = projects_root(&options)
        .join("project-1")
        .join("runs")
        .join("run-1");
    fs::create_dir_all(&run_dir).expect("create run dir");
    fs::write(run_dir.join("run_state.txt"), "completed\n").expect("state");
    fs::write(
        run_dir.join("md_summary.tsv"),
        "candidate_id\trmsd\tmd_stability_score\nC1\t1.2\t0.8\nC2\t1.1\t0.9\n",
    )
    .expect("md summary");

    let mut entries = Vec::new();
    for candidate_id in ["C1", "C2"] {
        let artifact_dir = run_dir
            .join("tool_outputs")
            .join("md")
            .join(candidate_id)
            .join("invocation-1");
        fs::create_dir_all(&artifact_dir).expect("candidate MD artifact dir");
        let prefix = format!("tool_outputs/md/{candidate_id}/invocation-1");
        fs::write(
            artifact_dir.join("md_protocol.tsv"),
            format!(
                "candidate_id\tinput_id\tstage\tparameter\tvalue\tunit\tsource\n{candidate_id}\tI1\tplatform\topenmm_platform\tCUDA\t\tOpenMM\n{candidate_id}\tI1\tintegrator\ttime_step\t0.0020\tps\tOpenMM\n{candidate_id}\tI1\tproduction\ttotal_time\t100.0000\tps\tOpenMM\n"
            ),
        )
        .expect("protocol");
        fs::write(
            artifact_dir.join("md_state.tsv"),
            format!(
                "candidate_id\tinput_id\tframe\tstep\ttime_ps\tstage\tpotential_energy_kj_mol\tkinetic_energy_kj_mol\ttotal_energy_kj_mol\ttemperature_estimate_K\tsource\n{candidate_id}\tI1\t0\t0\t0.0000\tminimized\t-1.0000\t0.0000\t-1.0000\t0.0000\tOpenMM Context state\n{candidate_id}\tI1\t50\t60000\t120.0000\tproduction\t-2.0000\t1.0000\t-1.0000\t300.0000\tOpenMM Context state\n"
            ),
        )
        .expect("state");
        fs::write(
            artifact_dir.join("md_trajectory.tsv"),
            format!(
                "candidate_id\tframe\tatom_index\trole\telement\tresidue\tx\ty\tz\n{candidate_id}\t0\t0\tligand\tC\tLIG\t0.0\t0.0\t0.0\n"
            ),
        )
        .expect("trajectory");
        fs::write(
            artifact_dir.join("gpu_telemetry_status.tsv"),
            "cuda_platform\tnvidia_smi_available\tgpu_observed\tproduction_sample_count\tproduction_max_utilization_gpu_percent\tobservation_basis\tfailure_reason\nCUDA\ttrue\ttrue\t2\t70.0000\tcurrent_pid\t\n",
        )
        .expect("gpu status");
        let manifest_rel = format!("{prefix}/md_run_manifest.json");
        fs::write(
            artifact_dir.join("md_run_manifest.json"),
            json!({
                "candidate_id": candidate_id,
                "method": "openmm_receptor_ligand_complex_dynamics",
                "platform": "CUDA",
                "frame_count": 51,
                "protocol_artifact": format!("{prefix}/md_protocol.tsv"),
                "state_artifact": format!("{prefix}/md_state.tsv"),
                "trajectory": format!("{prefix}/md_trajectory.tsv"),
                "gpu_telemetry_status_artifact": format!("{prefix}/gpu_telemetry_status.tsv")
            })
            .to_string(),
        )
        .expect("manifest");
        entries.push(json!({
            "candidate_id": candidate_id,
            "manifest": manifest_rel,
            "status": "completed"
        }));
    }
    fs::write(
        run_dir.join("tool_outputs/md_manifest_index.json"),
        json!({"entries": entries}).to_string(),
    )
    .expect("manifest index");
    write_passed_authenticity_gate(&run_dir);

    let payload = md_endpoint_json(&options, "run-1");
    assert_eq!(payload["protocol"]["ready"], true);
    assert_eq!(payload["protocol"]["platform"], "CUDA");
    assert_eq!(payload["protocol"]["candidate_count"], 2);
    assert_eq!(payload["state_trace"]["ready"], true);
    assert_eq!(payload["state_trace"]["candidate_count"], 2);
    assert_eq!(payload["state_trace"]["row_count"], 4);
    assert_eq!(payload["cuda_monitoring"]["ready"], true);
    assert_eq!(payload["cuda_monitoring"]["candidate_count"], 2);
    assert_eq!(payload["cuda_monitoring"]["production_sample_count"], "4");
    let rows = payload["md_rows"].as_array().expect("MD rows");
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|row| row["trajectory_ready"] == true));
    assert!(rows.iter().all(|row| row["trajectory_frame_count"] == 51));
    assert!(
        rows.iter()
            .all(|row| row["trajectory_status"] == "轨迹数据已就绪（51 帧）")
    );
    assert!(!payload.to_string().contains(&root.display().to_string()));

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
    write_passed_authenticity_gate(&run_dir);

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
