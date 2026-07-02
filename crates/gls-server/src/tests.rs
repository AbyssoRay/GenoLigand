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
    let root = std::env::temp_dir().join(format!("genoligand-user-setup-status-{}", now_millis()));
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

#[test]
fn input_validation_blocks_punctuation_only_payloads() {
    let root = std::env::temp_dir().join(format!("genoligand-input-validation-{}", now_millis()));
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
    assert!(!input_value_has_usable_user_input(&json!({
        "inputs":[{"field":"small_molecule","value":"lqtu0we9ur"}]
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
        ("small_molecule", "IKVAV"),
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
fn preflight_reports_user_facing_input_recognition() {
    assert_eq!(
        input_recognition_summary(&json!({
            "inputs":[{"field":"small_molecule","value":"ikv AV"}]
        }))
        .as_deref(),
        Some("已识别为多肽")
    );
    assert_eq!(
        input_recognition_summary(&json!({
            "inputs":[{"field":"small_molecule","value":"CC(=O)OC1=CC=CC=C1C(=O)O"}]
        }))
        .as_deref(),
        Some("已识别为小分子")
    );
    assert_eq!(
        input_recognition_summary(&json!({
            "inputs":[{"field":"small_molecule","value":"ATGCGTACGTAA"}]
        }))
        .as_deref(),
        Some("已识别为核酸序列")
    );
    assert_eq!(
        input_recognition_summary(&json!({
            "inputs":[{"field":"small_molecule","value":"CCN"}]
        }))
        .as_deref(),
        Some("已识别为小分子")
    );
    assert_eq!(
        input_recognition_summary(&json!({
            "inputs":[{"field":"peptide","value":">pep1\nIKVAV\n>pep2\nIKVAV"}]
        }))
        .as_deref(),
        Some("已识别为多肽")
    );

    let root = std::env::temp_dir().join(format!("genoligand-preflight-input-{}", now_millis()));
    let options = server_options(root.clone());
    let project_dir = project_dir(&options, "project-1");
    fs::create_dir_all(&project_dir).expect("project dir");
    fs::write(
        project_dir.join("input_manifest.json"),
        r#"{"inputs":[{"field":"small_molecule","value":"ikv AV"}],"files":[]}"#,
    )
    .expect("input");

    let preflight = user_preflight_json(&options, "project-1");
    let input_detail = preflight["checks"]
        .as_array()
        .and_then(|rows| {
            rows.iter()
                .find(|row| row.get("id").and_then(Value::as_str) == Some("input"))
        })
        .and_then(|row| row.get("detail").and_then(Value::as_str))
        .unwrap_or("");
    assert_eq!(input_detail, "已识别为多肽");
    assert!(!preflight.to_string().contains(&root.display().to_string()));

    fs::remove_dir_all(&root).ok();
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

    assert_eq!(payload["current_stage"], "结构与口袋评估");
    assert_eq!(by_id("input")["status"], "ready");
    assert_eq!(by_id("features")["status"], "ready");
    assert_eq!(by_id("known")["status"], "ready");
    assert_eq!(by_id("metagenome")["status"], "ready");
    assert_eq!(by_id("structure")["status"], "running");
    assert_eq!(by_id("md")["status"], "waiting");

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
