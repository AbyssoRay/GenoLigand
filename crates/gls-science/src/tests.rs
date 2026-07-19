use super::input_conversion::{ConvertedInput, INPUT_MANIFEST_WRITE_BATCH_SIZE};
use super::*;
use flate2::{Compression, write::GzEncoder};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Clone)]
struct TestHttpResponse {
    status: u16,
    content_type: &'static str,
    body: Vec<u8>,
}

struct TestHttpServer {
    address: SocketAddr,
    base_url: String,
    requests: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl TestHttpServer {
    fn start(routes: BTreeMap<String, TestHttpResponse>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test HTTP server");
        listener
            .set_nonblocking(true)
            .expect("nonblocking test HTTP server");
        let address = listener.local_addr().expect("test HTTP address");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let thread_requests = Arc::clone(&requests);
        let thread_stop = Arc::clone(&stop);
        let thread = thread::spawn(move || {
            while !thread_stop.load(AtomicOrdering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        if thread_stop.load(AtomicOrdering::SeqCst) {
                            break;
                        }
                        serve_test_http_request(stream, &routes, &thread_requests);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            address,
            base_url: format!("http://{address}"),
            requests,
            stop,
            thread: Some(thread),
        }
    }

    fn requests(&self) -> Vec<String> {
        self.requests.lock().expect("test requests lock").clone()
    }
}

impl Drop for TestHttpServer {
    fn drop(&mut self) {
        self.stop.store(true, AtomicOrdering::SeqCst);
        let _ = TcpStream::connect(self.address);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn serve_test_http_request(
    mut stream: TcpStream,
    routes: &BTreeMap<String, TestHttpResponse>,
    requests: &Arc<Mutex<Vec<String>>>,
) {
    stream
        .set_nonblocking(false)
        .expect("blocking test HTTP stream");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("test HTTP read timeout");
    let mut reader = BufReader::new(stream.try_clone().expect("clone test HTTP stream"));
    let mut request_line = String::new();
    reader
        .read_line(&mut request_line)
        .expect("read test HTTP request line");
    let path = request_line
        .split_whitespace()
        .nth(1)
        .unwrap_or("/")
        .to_string();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or_default() == 0 || line == "\r\n" {
            break;
        }
    }
    requests
        .lock()
        .expect("test requests lock")
        .push(path.clone());
    let fallback = TestHttpResponse {
        status: 404,
        content_type: "text/plain",
        body: b"not found".to_vec(),
    };
    let response = routes.get(&path).unwrap_or(&fallback);
    let reason = if response.status == 200 {
        "OK"
    } else if response.status == 404 {
        "Not Found"
    } else {
        "Error"
    };
    write!(
        stream,
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        response.status,
        reason,
        response.content_type,
        response.body.len(),
    )
    .expect("write test HTTP headers");
    stream
        .write_all(&response.body)
        .expect("write test HTTP body");
}

fn test_gzip_bytes(text: &str) -> Vec<u8> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(text.as_bytes()).expect("write test gzip");
    encoder.finish().expect("finish test gzip")
}

fn test_rcsb_mmcif(pdb_id: &str) -> String {
    format!(
        "data_{pdb_id}\n_entry.id {pdb_id}\nloop_\n_atom_site.group_PDB\n_atom_site.id\n_atom_site.type_symbol\n_atom_site.label_atom_id\n_atom_site.Cartn_x\n_atom_site.Cartn_y\n_atom_site.Cartn_z\nATOM 1 N N 1.0 2.0 3.0\n#\n"
    )
}

fn test_rcsb_pdb(pdb_id: &str) -> String {
    format!(
        "{:<62}{pdb_id}\n{}",
        "HEADER    TEST STRUCTURE",
        minimal_pdb_block()
    )
}

fn write_test_rcsb_on_demand_policy(store: &Path) {
    write_text(
        &store.join("downloads/rcsb/on_demand_policy.json"),
        &(json!({
            "schema_version": RCSB_ON_DEMAND_POLICY_SCHEMA_VERSION,
            "status": "ready",
            "mode": "on_demand",
            "official_base_url": RCSB_OFFICIAL_DOWNLOAD_BASE_URL,
            "coordinate_template": RCSB_OFFICIAL_COORDINATE_TEMPLATE,
            "cache_root": RCSB_ON_DEMAND_CACHE_ROOT,
            "max_fetches_per_run": RCSB_ON_DEMAND_FETCH_LIMIT,
            "max_cache_bytes": RCSB_ON_DEMAND_CACHE_MAX_BYTES,
            "max_cache_files": RCSB_ON_DEMAND_CACHE_MAX_FILES,
            "validation": RCSB_ON_DEMAND_VALIDATION,
        })
        .to_string()
            + "\n"),
    )
    .expect("write RCSB on-demand policy");
}

#[test]
fn openmm_cuda_ready_follows_probe_detail_flag() {
    let cuda_probe = ToolProbe {
        ready: true,
        version: "8.2.0".to_string(),
        detail: "openmm_python_module_detected;platforms=Reference,CPU,CUDA;cuda=ready".to_string(),
    };
    let cpu_probe = ToolProbe {
        ready: true,
        version: "8.2.0".to_string(),
        detail: "openmm_python_module_detected;platforms=Reference,CPU;cuda=missing".to_string(),
    };
    assert!(openmm_cuda_ready(&cuda_probe));
    assert!(!openmm_cuda_ready(&cpu_probe));
}

#[test]
fn command_wrapper_dirs_include_genoligand_runtime_paths() {
    let root = temp_root("command_wrapper_dirs");
    let local = root.join("local");
    let default_wrappers = local.join("GenoLigand/runtime/python-worker-env/command-wrappers");
    let python_root = root.join("python-worker-env");
    let sibling_wrappers = python_root.join("command-wrappers");
    fs::create_dir_all(&default_wrappers).expect("default wrappers");
    fs::create_dir_all(&sibling_wrappers).expect("sibling wrappers");
    let python = python_root.join("genoligand-wsl-python.cmd");

    let dirs =
        command_wrapper_dirs_from_env(Some(local.into_os_string()), Some(python.into_os_string()));

    assert!(dirs.contains(&default_wrappers));
    assert!(dirs.contains(&sibling_wrappers));
    fs::remove_dir_all(root).ok();
}

#[test]
fn worker_python_candidates_prefer_installed_genoligand_runtime() {
    let root = temp_root("worker_python_candidates");
    let local = root.join("local");
    let runtime = local
        .join("GenoLigand")
        .join("runtime")
        .join("python-worker-env");
    fs::create_dir_all(&runtime).expect("runtime dir");
    let wrapper = runtime.join("genoligand-wsl-python.cmd");
    write_text(&wrapper, "@echo off\n").expect("wrapper");

    let candidates = python_worker_candidates_from_env(Some(local.into_os_string()));

    assert_eq!(
        candidates.first(),
        Some(&vec![wrapper.display().to_string()])
    );
    assert!(
        candidates
            .iter()
            .any(|candidate| candidate == &vec!["python".to_string()])
    );
    fs::remove_dir_all(root).ok();
}

#[test]
fn script_python_candidates_prefer_configured_worker_python() {
    let preferred =
        r"C:\Users\tester\GenoLigand\runtime\python-worker-env\genoligand-wsl-python.cmd";

    let candidates = script_python_command_candidates_from(Some(preferred.to_string()));

    assert_eq!(candidates.first(), Some(&vec![preferred.to_string()]));
    assert!(
        candidates
            .iter()
            .any(|candidate| candidate == &vec!["python".to_string()])
    );
}

#[test]
fn script_python_candidates_use_installed_runtime_worker_without_env_override() {
    let root = temp_root("script_python_runtime_worker");
    let local = root.join("local_appdata");
    let runtime = local
        .join("GenoLigand")
        .join("runtime")
        .join("python-worker-env");
    fs::create_dir_all(&runtime).expect("runtime dir");
    let wrapper = runtime.join("genoligand-wsl-python.cmd");
    write_text(&wrapper, "@echo off\n").expect("wrapper");

    let candidates = script_python_command_candidates_from_env(None, Some(local.into_os_string()));

    assert_eq!(
        candidates.first(),
        Some(&vec![wrapper.display().to_string()])
    );
    assert!(
        candidates
            .iter()
            .any(|candidate| candidate == &vec!["python".to_string()])
    );
    fs::remove_dir_all(root).ok();
}

#[test]
fn python_script_runner_uses_temp_script_file() {
    let path = temp_python_script_path("bindingdb onsite query");
    let second = temp_python_script_path("bindingdb onsite query");

    assert_eq!(path.extension().and_then(|ext| ext.to_str()), Some("py"));
    assert_ne!(path, second);
    assert!(
        path.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.contains("bindingdb_onsite_query"))
    );
}

#[test]
fn wsl_python_wrapper_uses_wsl_paths_for_worker_inputs() {
    if !cfg!(windows) {
        return;
    }
    let candidate = vec![r"C:\Users\tester\genoligand-wsl-python.cmd".to_string()];
    let python_root = PathBuf::from(r"D:\workspace\GenoLigand\python");
    let job = r"D:\workspace\GenoLigand\run\worker_job.json";

    let pythonpath =
        worker_pythonpath_env_from_paths(&candidate, vec![python_root]).expect("pythonpath");

    assert_eq!(
        pythonpath.to_string_lossy(),
        "/mnt/d/workspace/GenoLigand/python"
    );
    assert_eq!(
        python_script_arg(&candidate, job),
        "/mnt/d/workspace/GenoLigand/run/worker_job.json"
    );
    assert!(
        wsl_env_with_pythonpath()
            .to_string_lossy()
            .split(':')
            .any(|entry| entry.split('/').next() == Some("PYTHONPATH"))
    );
}

#[test]
fn valid_worker_fail_result_is_schema_valid_but_fails_the_stage() {
    let root = temp_root("worker_fail_result");
    let path = root.join("worker_result.json");
    let task = &SCIENTIFIC_WORKER_TASKS[0];
    write_text(
        &path,
        &format!(
            "{}\n",
            json!({
                "schema_version": WORKER_SCHEMA_VERSION,
                "job_id": task.job_id,
                "worker": task.worker,
                "status": "FAIL",
                "engine_version": "OpenMM platforms=Reference,CPU",
                "execution_status": "failed",
                "reason": "fixture_failed",
                "outcomes": [{
                    "status": "failed",
                    "reason": "fixture_failed",
                    "required": true,
                    "database_id": "md_summary",
                    "row_count": 0
                }],
                "python_owns_final_score": false,
                "python_owns_report_semantics": false,
                "primitive_execution_only": true
            })
        ),
    )
    .expect("write result");

    assert!(worker_result_file_is_valid(&path, task));
    let value: Value =
        serde_json::from_str(&read_text(&path).expect("result text")).expect("result json");
    assert!(worker_result_stage_error(&value, task).is_some());
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn worker_not_ready_requires_a_structured_required_outcome() {
    let task = &SEQUENCE_ALIGNMENT_WORKER_TASK;
    let mut value = json!({
        "schema_version": WORKER_SCHEMA_VERSION,
        "job_id": task.job_id,
        "worker": task.worker,
        "status": "NOT_READY",
        "engine_version": "MMseqs2 not executed",
        "execution_status": "not_ready",
        "reason": "no_persistent_mmseqs_databases",
        "outcomes": [{
            "status": "not_ready",
            "reason": "no_persistent_mmseqs_databases",
            "required": true,
            "database_id": "mmseqs_target_manifest",
            "row_count": 0
        }],
        "python_owns_final_score": false,
        "python_owns_report_semantics": false,
        "primitive_execution_only": true
    });

    assert!(validate_worker_result(&value, task));
    assert!(worker_result_stage_error(&value, task).is_some());
    value
        .as_object_mut()
        .expect("result object")
        .remove("outcomes");
    assert!(!validate_worker_result(&value, task));
}

#[test]
fn rust_worker_fallback_omits_empty_optional_bindings() {
    let root = temp_root("worker_fallback_optional_bindings");
    let path = root.join("worker_result.json");
    let task = &SEQUENCE_ALIGNMENT_WORKER_TASK;
    let job = json!({
        "run_id": "run-current",
        "invocation_id": "invocation-current"
    });

    write_worker_failure_result(&path, task, &job, "fixture_launch_failed")
        .expect("write Rust fallback");
    let value: Value =
        serde_json::from_str(&read_text(&path).expect("fallback text")).expect("fallback json");

    assert!(validate_worker_result(&value, task));
    for field in [
        "candidate_id",
        "candidate_digest",
        "input_id",
        "record_id",
        "input_digest",
        "source_digest",
        "receptor_digest",
    ] {
        assert!(value.get(field).is_none(), "unexpected empty {field}");
    }
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn worker_jobs_and_results_are_bound_to_current_run_invocation() {
    let root = temp_root("worker_invocation_binding");
    let run = root.join("run-current");
    ensure_dir(&run.join("worker_results")).expect("worker result dir");
    ensure_dir(&run.join("tool_outputs")).expect("tool outputs");
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: root.join("science_store"),
        input_manifest: run.join("input.json"),
        run_dir: run.clone(),
    };
    let first = start_worker_invocation(&options).expect("first invocation");
    let second = start_worker_invocation(&options).expect("second invocation");
    assert_ne!(first, second);
    let task = &SCIENTIFIC_WORKER_TASKS[0];
    let job = worker_job_value(&options, task).expect("worker job");
    assert_eq!(job["invocation_id"], second);

    let result_path = run.join("worker_results/md_summary_ligand_dynamics.json");
    let provenance_path = run.join("tool_outputs/md_artifact_provenance.json");
    write_text(&result_path, "{}\n").expect("result artifact");
    write_text(&provenance_path, "{}\n").expect("provenance artifact");
    let mut result = json!({
        "schema_version": WORKER_SCHEMA_VERSION,
        "job_id": task.job_id,
        "worker": task.worker,
        "run_id": "run-current",
        "status": "PASS",
        "engine_version": "OpenMM test",
        "execution_status": "completed",
        "reason": "completed",
        "outcomes": [{
            "status": "completed",
            "reason": "completed",
            "required": true,
            "database_id": "md_summary",
            "row_count": 1
        }],
        "python_owns_final_score": false,
        "python_owns_report_semantics": false,
        "primitive_execution_only": true,
        "invocation_id": second,
        "artifact_path": "worker_results/md_summary_ligand_dynamics.json",
        "artifact_provenance": "tool_outputs/md_artifact_provenance.json"
    });
    assert!(validate_worker_result_binding(
        &result, &job, &options, task
    ));

    result["invocation_id"] = json!("invocation-stale");
    assert!(!validate_worker_result_binding(
        &result, &job, &options, task
    ));
    result["invocation_id"] = job["invocation_id"].clone();
    result["artifact_path"] = json!(root.join("outside.json").display().to_string());
    write_text(&root.join("outside.json"), "{}\n").expect("outside artifact");
    assert!(!validate_worker_result_binding(
        &result, &job, &options, task
    ));

    fs::remove_dir_all(&root).expect("cleanup temp root");
}

#[test]
fn query_structure_jobs_and_results_are_invocation_bound() {
    let root = temp_root("query_structure_invocation_binding");
    let run = root.join("run-current");
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: root.join("science_store"),
        input_manifest: run.join("input.json"),
        run_dir: run,
    };
    let stale_invocation = "invocation-stale";
    let current_invocation = "invocation-current";
    let stale_job_id = query_structure_job_id("input_1", "record_1", stale_invocation);
    let current_job_id = query_structure_job_id("input_1", "record_1", current_invocation);
    assert_ne!(stale_job_id, current_job_id);

    let task = WorkerTask {
        job_id: &current_job_id,
        worker: "query_structure_prepare",
        module: "genoligand_workers.query_structure_worker",
        stage: "query_structure_prepare",
    };
    let job = json!({
        "run_id": "run-current",
        "invocation_id": current_invocation,
    });
    let mut result = json!({
        "schema_version": WORKER_SCHEMA_VERSION,
        "job_id": stale_job_id,
        "worker": task.worker,
        "run_id": "run-current",
        "invocation_id": stale_invocation,
        "status": "NOT_APPLICABLE",
        "engine_version": "test query structure fixture",
        "execution_status": "not_applicable",
        "reason": "test_fixture_not_applicable",
        "outcomes": [{
            "status": "not_applicable",
            "reason": "test_fixture_not_applicable",
            "required": false,
            "database_id": task.worker,
            "row_count": 0
        }],
        "python_owns_final_score": false,
        "python_owns_report_semantics": false,
        "primitive_execution_only": true
    });
    assert!(!validate_worker_result_binding(
        &result, &job, &options, &task
    ));

    result["job_id"] = json!(current_job_id);
    assert!(!validate_worker_result_binding(
        &result, &job, &options, &task
    ));
    result["invocation_id"] = json!(current_invocation);
    assert!(validate_worker_result_binding(
        &result, &job, &options, &task
    ));

    fs::remove_dir_all(&root).ok();
}

#[test]
fn worker_rerun_cleanup_removes_stale_docking_and_md_outputs() {
    let root = temp_root("worker_rerun_cleanup");
    let run = root.join("run");
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: root.join("science_store"),
        input_manifest: run.join("input.json"),
        run_dir: run.clone(),
    };
    let stale = [
        run.join("docking_summary.tsv"),
        run.join("tool_outputs/docking_manifest.json"),
        run.join("md_summary.tsv"),
        run.join("tool_outputs/md_summary.tsv"),
        run.join("tool_outputs/md_run_manifest.json"),
    ];
    for path in &stale {
        write_text(path, "stale\n").expect("stale artifact");
    }

    clear_worker_task_outputs(&options, &DOCKING_WORKER_TASK).expect("clear docking");
    clear_worker_task_outputs(&options, &SCIENTIFIC_WORKER_TASKS[0]).expect("clear md");

    assert!(stale.iter().all(|path| !path.exists()));
    fs::remove_dir_all(&root).expect("cleanup temp root");
}

#[test]
fn pipeline_blocks_without_science_store_and_writes_zero_candidates() {
    let root = temp_root("blocked");
    let input = root.join("input.json");
    let run = root.join("run");
    write_text(
            &input,
            "{\"inputs\":[{\"field\":\"smiles\",\"value\":\"CCO\"},{\"field\":\"complex\",\"value\":\"kinase pocket\"}],\"files\":[]}\n",
        )
        .expect("write input");
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: root.join("missing_store"),
        input_manifest: input,
        run_dir: run.clone(),
    };
    let error =
        run_science_command(ScienceCommand::PipelineRun, &options).expect_err("blocked outcome");
    assert!(error.contains("pipeline_blocked"));
    let ranking = read_text(&run.join("target_candidate_ranking.tsv")).expect("ranking");
    assert_eq!(ranking.lines().count(), 1);
    let context = read_text(&run.join("run_context.json")).expect("run context");
    let source_map = read_text(&run.join("artifact_source_map.tsv")).expect("source map");
    assert!(context.contains("\"state\":\"blocked\""));
    assert!(source_map.contains("target_candidate_ranking.tsv"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn structure_file_context_uses_publication_identity_without_reading_large_index() {
    let root = temp_root("structure_file_context");
    let store = root.join("science_store");
    let structure = store.join("indexes/structure_targets.tsv");
    ensure_dir(structure.parent().expect("structure parent")).expect("structure parent");
    fs::write(&structure, [0xff]).expect("non-UTF8 structure fixture");
    let digest = "A".repeat(64);
    write_text(
        &store.join(STRUCTURE_TARGETS_MANIFEST_REL_PATH),
        &format!(
            "{{\"schema_version\":\"{}\",\"status\":\"ready\",\"path\":\"indexes/structure_targets.tsv\",\"bytes\":1,\"sha256\":\"{}\"}}\n",
            STRUCTURE_TARGETS_MANIFEST_SCHEMA_VERSION, digest
        ),
    )
    .expect("structure manifest");

    let context = structure_file_context(&store, &structure);
    assert_eq!(context["content_id"], digest.to_ascii_lowercase());

    let mut invalid_manifest =
        read_json_value_if_file(&store.join(STRUCTURE_TARGETS_MANIFEST_REL_PATH))
            .expect("structure manifest JSON");
    invalid_manifest["bytes"] = json!(2);
    write_text(
        &store.join(STRUCTURE_TARGETS_MANIFEST_REL_PATH),
        &(serde_json::to_string(&invalid_manifest).expect("serialize invalid manifest") + "\n"),
    )
    .expect("invalid structure manifest");
    assert!(structure_file_context(&store, &structure)["content_id"].is_null());

    fs::remove_dir_all(&root).expect("cleanup temp root");
}

#[test]
fn pipeline_stage_errors_write_failed_terminal_state_for_every_boundary() {
    for stage in [
        "input_normalize",
        "database_match_known",
        "dynamics_screen",
        "target_rank",
        "report_render",
    ] {
        let root = temp_root(&format!("failed_terminal_{stage}"));
        let run = root.join("run");
        let options = ScienceOptions {
            workspace_root: root.clone(),
            science_store: root.join("science_store"),
            input_manifest: root.join("input.json"),
            run_dir: run.clone(),
        };
        ensure_dir(&run).expect("run dir");
        let invocation_id = start_pipeline_invocation(&options).expect("pipeline invocation");
        write_text(&run.join("last_successful_stage.txt"), "none\n").expect("last stage");
        write_run_context(&options, "starting", &[]).expect("starting context");

        let error = run_pipeline_stage(&options, &invocation_id, stage, || {
            Err(format!("forced_{stage}_failure"))
        })
        .expect_err("stage must fail");

        assert!(error.contains(&format!("pipeline_failed:{stage}")));
        assert_eq!(
            read_text(&run.join("run_state.txt")).expect("run state"),
            "failed\n"
        );
        assert_eq!(
            read_text(&run.join("current_stage.txt")).expect("current stage"),
            format!("{stage}\n")
        );
        assert_eq!(
            read_text(&run.join("last_successful_stage.txt")).expect("last stage"),
            "none\n"
        );
        let failure = read_text(&run.join("failure_reason.txt")).expect("failure reason");
        assert!(failure.contains(&format!("stage={stage}")));
        assert!(failure.contains(&format!("reason=forced_{stage}_failure")));
        let context = read_text(&run.join("run_context.json")).expect("run context");
        assert!(context.contains("\"state\":\"failed\""));
        assert!(!run.join("pipeline_done.txt").exists());
        fs::remove_dir_all(&root).expect("cleanup temp root");
    }
}

#[test]
fn report_empty_candidate_table_does_not_claim_result_outputs() {
    let root = temp_root("empty_report");
    let store = create_temporary_test_store(&root).expect("store");
    let input = root.join("input.json");
    let run = root.join("run");
    ensure_dir(&run).expect("run dir");
    write_text(
        &input,
        "{\"inputs\":[{\"field\":\"small_molecule\",\"value\":\"CCO\"}],\"files\":[]}\n",
    )
    .expect("input");
    write_text(
            &run.join("target_candidate_ranking.tsv"),
            "rank\tcandidate_id\ttarget_id\ttarget_name\tcandidate_scope\torganism\tfinal_score\tknown_database_score\tmetagenome_score\tstructure_score\tdocking_score\tmd_score\tevidence_summary\tstatus\tinput_id\trecord_id\tidentity_namespace\n",
        )
        .expect("empty ranking");
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: store,
        input_manifest: input,
        run_dir: run.clone(),
    };

    normalize_and_compute_fixture(&options, "small_molecule", "CCO")
        .expect("normalized report input");
    write_report_support(&options).expect("report support artifacts");

    render_report(&options).expect("report");
    let report = read_text(&run.join("research_report.md")).expect("report text");

    assert!(report.contains("当前运行没有可展示的候选假设"));
    assert!(report.contains("当前运行没有生成候选靶点"));
    assert!(!report.contains("生成 0 个候选假设"));
    assert!(!report.contains("排序、详情和证据摘要均来自当前运行"));
    assert!(!report.contains("target_candidate_ranking.tsv"));
    assert!(!report.contains("input_manifest"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn pipeline_uses_one_temporary_query_row_then_test_removes_it() {
    let root = temp_root("one_row");
    install_not_applicable_worker_fixtures(&root).expect("worker fixtures");
    let store = create_temporary_test_store(&root).expect("store");
    write_zip(
        &store.join(BINDINGDB_SOURCE_REL_PATH),
        "BindingDB_All.tsv",
        "Ligand SMILES\tBindingDB Ligand Name\tTarget Name\tTarget Source Organism According to Curator or DataSource\tKi (nM)\tUniProt (SwissProt) Primary ID of Target Chain 1\nCCO\tethanol\tCyclin-dependent kinase 2\tHomo sapiens\t12\tP24941\n",
    )
    .expect("BindingDB fixture archive");
    publish_test_store_bindingdb_query(&store).expect("BindingDB query index");
    write_test_science_store_readiness(&store, &[]).expect("refresh store readiness");
    let input = root.join("input.json");
    let run = root.join("run");
    write_text(
            &input,
            "{\"inputs\":[{\"field\":\"smiles\",\"value\":\"CCO\"},{\"field\":\"complex\",\"value\":\"kinase pocket\"}],\"files\":[]}\n",
        )
        .expect("write input");
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: store,
        input_manifest: input,
        run_dir: run.clone(),
    };
    let out = run_science_command(ScienceCommand::PipelineRun, &options).expect("pipeline");
    assert!(out.contains("pipeline_completed"));
    let ranking = read_text(&run.join("target_candidate_ranking.tsv")).expect("ranking");
    assert!(ranking.contains("P24941"));
    assert!(!ranking.contains("MG_ORF_0001"));
    let detail = read_text(&run.join("candidate_detail.json")).expect("candidate detail");
    let context = read_text(&run.join("run_context.json")).expect("run context");
    let source_map = read_text(&run.join("artifact_source_map.tsv")).expect("source map");
    let evidence = read_text(&run.join("evidence_ledger.tsv")).expect("evidence ledger");
    let report = read_text(&run.join("research_report.md")).expect("research report");
    let structure = read_text(&run.join("structure_patch_summary.tsv")).expect("structure summary");
    let docking = read_text(&run.join("docking_summary.tsv")).expect("docking summary");
    let md = read_text(&run.join("md_summary.tsv")).expect("md summary");
    assert!(detail.contains("\"candidates\""));
    assert!(detail.contains("\"candidate_id\""));
    assert!(context.contains("\"state\":\"completed\""));
    assert!(source_map.contains("research_report.md"));
    assert!(source_map.contains("structure_interaction_matches.tsv"));
    assert!(source_map.contains("schema-bound structure output when present"));
    assert!(source_map.contains("tool_outputs/md_summary.tsv"));
    assert!(source_map.contains("tool_outputs/md_rmsd.tsv"));
    assert!(source_map.contains("tool_outputs/ligand_placement.tsv"));
    assert!(source_map.contains("tool_outputs/complex_md_manifest.json"));
    assert!(source_map.contains("scientific_runtime_summary.tsv"));
    assert!(source_map.contains("current run scientific worker jobs"));
    assert!(!source_map.contains("worker_runtime_summary.tsv"));
    assert!(!run.join("worker_jobs/openmm_smoke.json").exists());
    assert!(!run.join("worker_results/openmm_smoke.json").exists());
    assert!(source_map.contains("docking_summary.tsv\t"));
    assert!(source_map.contains("md_summary.tsv\t"));
    assert!(source_map.contains("docking_summary.tsv\tschema-bound docking output when present;otherwise no generated rows\ttool_outputs/docking_manifest.json;tool_outputs/docking_pose.sdf;evidence_ledger.tsv\tno_rows"));
    assert!(source_map.contains("md_summary.tsv\ttool_outputs/docking_pose.sdf;tool_outputs/md_run_manifest.json;tool_outputs/md_protocol.tsv;tool_outputs/md_state.tsv;tool_outputs/gpu_telemetry_status.tsv;tool_outputs/md_summary.tsv;tool_outputs/md_rmsd.tsv;tool_outputs/md_rmsf.tsv;tool_outputs/md_ligand_drift.tsv;tool_outputs/md_contacts.tsv;tool_outputs/md_hbonds.tsv\ttarget_candidate_ranking.tsv;evidence_ledger.tsv\tno_rows"));
    assert!(source_map.contains("tool_outputs/md_summary.tsv\tschema-bound MD summary worker output\tmd_summary.tsv\tnot_yet_written"));
    assert!(evidence.contains("structure_interaction_matches"));
    assert!(evidence.contains("scientific_runtime_summary"));
    assert!(!evidence.contains("worker_runtime_summary"));
    assert!(evidence.contains("score_decomposition"));
    assert!(evidence.contains(
        "docking_summary.tsv\tGenoLigand\tnot_generated\tno current-run docking rows with verified pose provenance"
    ));
    assert!(evidence.contains(
        "md_summary.tsv\tGenoLigand\tnot_generated\tno current-run receptor-ligand MD artifacts with complete provenance"
    ));
    assert!(report.contains("# GenoLigand 科研报告"));
    assert!(report.contains("## 结果来源"));
    assert!(report.contains("项目编号"));
    assert!(report.contains("运行编号"));
    assert!(report.contains("输出材料"));
    assert!(report.contains("计算预测"));
    assert!(report.contains("建议把排序靠前的候选作为后续实验验证的优先对象"));
    assert!(report.contains("已知蛋白数据库") || report.contains("宏基因组潜在序列"));
    assert!(!report.contains("GenoLigand Scientific Report"));
    assert!(!report.contains("No candidate target"));
    assert!(!report.contains("input_manifest"));
    assert!(!report.contains("science_store"));
    assert!(!report.contains("target_candidate_ranking.tsv"));
    assert!(!report.contains("known_database"));
    assert!(!report.contains("metagenome_potential"));
    assert!(!report.contains("只包含"));
    assert!(!report.contains("不代表"));
    assert!(!report.contains("解释边界"));
    assert!(!report.contains("不应"));
    assert_eq!(structure.lines().count(), 2);
    assert_eq!(docking.lines().count(), 1);
    assert_eq!(md.lines().count(), 1);
    assert!(structure.contains("structure_index_match"));
    assert!(!structure.contains("matched_structure_or_pocket_tokens"));
    assert!(!structure.contains("patch_P24941"));
    assert!(!docking.contains("completed"));
    assert!(!md.contains("computed_from_database_evidence_only"));
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn scientific_workers_skip_when_no_coordinate_candidate_is_selected() {
    let root = temp_root("schema_workers");
    let store = create_temporary_test_store(&root).expect("store");
    let input = root.join("input.json");
    let run = root.join("run");
    write_text(
        &input,
        "{\"inputs\":[{\"field\":\"small_molecule\",\"value\":\"CCO\"}],\"files\":[]}\n",
    )
    .expect("write input");
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: store,
        input_manifest: input,
        run_dir: run.clone(),
    };

    let result = run_scientific_workers(&options).expect("no selected candidate is valid");

    assert!(result.contains("worker_count=0"));
    let summary = read_text(&run.join("scientific_runtime_summary.tsv")).expect("worker summary");
    assert_eq!(summary.lines().count(), 1);
    assert!(
        !run.join("worker_jobs/md_summary_ligand_dynamics.json")
            .exists()
    );
    assert!(
        !run.join("worker_results/md_summary_ligand_dynamics.json")
            .exists()
    );
    assert!(!run.join("worker_jobs/openmm_smoke.json").exists());
    assert!(!run.join("worker_results/openmm_smoke.json").exists());
    assert!(!run.join("worker_jobs/openff_rdkit_smoke.json").exists());
    assert!(!run.join("worker_results/openff_rdkit_smoke.json").exists());
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn feature_compute_emits_type_specific_features_for_all_supported_inputs() {
    let root = temp_root("typed_features");
    let input = root.join("input.json");
    let run = root.join("run");
    write_text(
            &input,
            r#"{"inputs":[
{"field":"small_molecule","value":"c1ccccc1O"},
{"field":"peptide","value":"ACDEK"},
{"field":"protein_ligand","value":">protein\nMKTAYIAKQRQISFVKSHFSRQDILDLWQMKTAYIAKQRQISFVKSHFSRQDILDLWQMKTAYIAKQRQISFVKSHFSRQDILDLWQ"},
{"field":"complex","value":"complex.pdb chain A ligand ATP"},
{"field":"genome_or_metagenome","value":">contig1\nATGCGCGTAA"}
],"files":[]}"#,
        )
        .expect("write input");
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: root.join("science_store"),
        input_manifest: input,
        run_dir: run.clone(),
    };

    run_science_command(ScienceCommand::InputNormalize, &options).expect("normalize");
    run_science_command(ScienceCommand::FeaturesCompute, &options).expect("features");
    let features = read_text(&run.join("feature_manifest.tsv")).expect("feature manifest");

    for expected in [
        "small_molecule\tatom_symbol_count",
        "peptide\tamino_acid_count",
        "protein_sequence\tamino_acid_count",
        "complex\tchain_marker_count",
        "genome_or_metagenome\tgc_fraction",
        "genome_or_metagenome\torf_start_marker_count",
        "genome_or_metagenome\ttranslated_orf_count\t1",
        "genome_or_metagenome\ttranslated_orf_feature_token\tmrv",
    ] {
        assert!(
            features.contains(expected),
            "missing feature row: {expected}"
        );
    }
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn identical_long_protein_uses_shared_database_and_query_tokens() {
    let sequence = "MKTAYIAKQRQISFVKSHFSRQDILDLWQ".repeat(4);
    let input_tokens = peptide_or_protein_features("protein_sequence", &sequence)
        .into_iter()
        .filter_map(|row| (!row.token.is_empty()).then_some(row.token))
        .collect::<BTreeSet<_>>();
    let input = ComputedFeatureInput {
        input_id: "input_1".to_string(),
        record_id: "record_1".to_string(),
        input_kind: "protein_ligand".to_string(),
        tokens: input_tokens,
    };
    let mut rows = Vec::new();
    push_fasta_row(
        "fixture",
        "test",
        "known_protein",
        "long_protein_fixture",
        "LONG_PROTEIN identical sequence",
        &sequence,
        &mut rows,
    );

    assert_eq!(rows.len(), 1);
    assert_eq!(score_row(&input, &rows[0]), 1.0);
    assert!(!matched_tokens(&input, &rows[0]).is_empty());
}

#[test]
fn query_structure_contract_rejects_identity_path_digest_and_content_tampering() {
    let root = temp_root("query_structure_contract_tampering");
    let input = root.join("input.json");
    let run = root.join("run");
    write_text(
        &input,
        r#"{"inputs":[
{"field":"small_molecule","value":"CCO"},
{"field":"genome_or_metagenome","value":">contig\nATGAAATAA"}
],"files":[]}"#,
    )
    .expect("write input");
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: root.join("science_store"),
        input_manifest: input,
        run_dir: run,
    };
    normalize_input(&options).expect("normalize");

    for field in [
        "schema_version",
        "status",
        "run_id",
        "job_id",
        "invocation_id",
        "input_id",
        "record_id",
        "input_kind",
        "input_digest",
        "source_content_sha256",
        "method_version",
        "features_json",
        "feature_digest",
        "coordinate_artifact",
    ] {
        let fixture = write_query_structure_contract_fixture(&options, 1).expect("fresh fixture");
        let mut manifest = read_json_value_if_file(&fixture.ready.manifest).expect("manifest");
        manifest[field] = json!(format!("tampered_{field}"));
        write_text(&fixture.ready.manifest, &(manifest.to_string() + "\n")).expect("tamper");
        assert!(
            validate_query_structure_contract_fixture(&options).is_err(),
            "manifest field accepted after tampering: {field}"
        );
    }

    for field in [
        "schema_version",
        "status",
        "run_id",
        "job_id",
        "invocation_id",
        "input_id",
        "record_id",
        "input_kind",
        "input_digest",
        "source_content_sha256",
        "method_version",
        "conversion_manifest_sha256",
        "coordinate_artifact",
    ] {
        let fixture = write_query_structure_contract_fixture(&options, 1).expect("fresh fixture");
        let mut features = read_json_value_if_file(&fixture.ready.features).expect("features");
        features[field] = json!(format!("tampered_{field}"));
        write_text(&fixture.ready.features, &(features.to_string() + "\n")).expect("tamper");
        assert!(
            validate_query_structure_contract_fixture(&options).is_err(),
            "feature field accepted after tampering: {field}"
        );
    }

    for field in [
        "run_id",
        "job_id",
        "invocation_id",
        "input_id",
        "record_id",
        "input_digest",
        "source_digest",
        "status",
        "manifest",
        "artifact_path",
    ] {
        let fixture = write_query_structure_contract_fixture(&options, 1).expect("fresh fixture");
        let mut result = read_json_value_if_file(&fixture.ready.result).expect("result");
        result[field] = json!(format!("tampered_{field}"));
        write_text(&fixture.ready.result, &(result.to_string() + "\n")).expect("tamper");
        assert!(
            validate_query_structure_contract_fixture(&options).is_err(),
            "result field accepted after tampering: {field}"
        );
    }

    let fixture = write_query_structure_contract_fixture(&options, 1).expect("fresh fixture");
    let mut job = read_json_value_if_file(&fixture.ready.job).expect("job");
    job["source_digest"] = json!("0".repeat(64));
    write_text(&fixture.ready.job, &(job.to_string() + "\n")).expect("tamper job");
    assert!(validate_query_structure_contract_fixture(&options).is_err());

    let fixture = write_query_structure_contract_fixture(&options, 1).expect("fresh fixture");
    let mut features = read_json_value_if_file(&fixture.ready.features).expect("features");
    features["physicochemical"]["hydrogen_bond_donors"] = json!(9);
    write_text(&fixture.ready.features, &(features.to_string() + "\n")).expect("tamper content");
    assert!(validate_query_structure_contract_fixture(&options).is_err());

    write_query_structure_contract_fixture(&options, 1).expect("fresh fixture");
    let index = read_text(&options.run_dir.join("query_structure_manifest.tsv")).expect("index");
    write_text(
        &options.run_dir.join("query_structure_manifest.tsv"),
        &index.replacen(QUERY_STRUCTURE_MANIFEST_HEADER, "input_id\trecord_id", 1),
    )
    .expect("tamper header");
    assert!(validate_query_structure_contract_fixture(&options).is_err());

    write_query_structure_contract_fixture(&options, 1).expect("fresh fixture");
    let mut lines = read_text(&options.run_dir.join("query_structure_manifest.tsv"))
        .expect("index")
        .lines()
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    lines[1].push_str("\textra");
    write_text(
        &options.run_dir.join("query_structure_manifest.tsv"),
        &(lines.join("\n") + "\n"),
    )
    .expect("tamper columns");
    assert!(validate_query_structure_contract_fixture(&options).is_err());
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn query_structure_ready_and_not_applicable_inputs_are_retained_with_bounded_tokens() {
    let root = temp_root("query_structure_ready_and_na");
    let input = root.join("input.json");
    let run = root.join("run");
    write_text(
        &input,
        r#"{"inputs":[
{"field":"small_molecule","value":"CCO"},
{"field":"genome_or_metagenome","value":">contig\nATGAAATAA"}
],"files":[]}"#,
    )
    .expect("write input");
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: root.join("science_store"),
        input_manifest: input,
        run_dir: run,
    };
    normalize_input(&options).expect("normalize");
    let fixture = write_query_structure_contract_fixture(&options, 2).expect("query fixture");
    let expected_query_tokens = query_structure_feature_rows(
        &read_json_value_if_file(&fixture.ready.features).expect("features"),
        "small_molecule",
    )
    .expect("compact features")
    .into_iter()
    .map(|row| row.token)
    .collect::<BTreeSet<_>>();
    compute_features(&options).expect("compute features");
    let feature_path = options.run_dir.join("feature_manifest.tsv");
    let feature_text = read_text(&feature_path).expect("feature text");
    let computed = read_computed_feature_inputs(&feature_path).expect("computed features");
    assert_eq!(computed.len(), 2);
    let ready = computed
        .iter()
        .find(|input| input.input_kind == "small_molecule")
        .expect("ready input");
    assert!(expected_query_tokens.is_subset(&ready.tokens));
    assert!(
        computed
            .iter()
            .find(|input| input.input_kind == "genome_or_metagenome")
            .is_some_and(|input| input.tokens.iter().all(|token| !token.starts_with("qs_")))
    );
    assert!(feature_text.contains("\tpreparation_status\tready\t"));
    assert!(feature_text.contains("\tpreparation_status\tnot_applicable\t"));
    assert!(!feature_text.contains("packed_hex"));
    assert!(!feature_text.contains("representative_points"));
    assert!(expected_query_tokens.len() <= MAX_QUERY_STRUCTURE_FEATURE_ROWS);
    assert!(fs::metadata(&feature_path).expect("feature metadata").len() < 64 * 1024);

    let sequence_rows = query_structure_feature_rows(
        &json!({
            "sequence_sha256": "f".repeat(64),
            "sequence_features": {
                "length": 4,
                "composition": {"A": 2, "D": 1, "V": 1},
                "estimated_net_charge_ph7": -1.0,
                "mean_kyte_doolittle_hydropathy": 1.0,
                "hydrophobic_fraction": 0.75,
                "secondary_structure_propensity": {
                    "method": "fixture",
                    "alpha_helix": 1.1,
                    "beta_sheet": 0.9,
                    "turn": 0.8,
                },
            },
        }),
        "protein_ligand",
    )
    .expect("sequence-prior features");
    assert!(sequence_rows.iter().any(|row| row.name == "alpha_helix"));
    assert!(sequence_rows.len() <= MAX_QUERY_STRUCTURE_FEATURE_ROWS);

    let coordinate_rows = query_structure_feature_rows(
        &json!({
            "source_content_sha256": "e".repeat(64),
            "structure": {
                "format": "pdb",
                "atom_count": 4,
                "heavy_atom_count": 4,
                "chain_count": 1,
                "residue_count": 2,
                "element_counts": {"C": 2, "N": 1, "O": 1},
                "residue_counts": {"ALA": 1, "GLY": 1},
                "bounding_box_min_angstrom": [0.0, 0.0, 0.0],
                "bounding_box_max_angstrom": [10.0, 5.0, 2.5],
                "radius_of_gyration_angstrom": 3.5,
                "coordinate_sha256": "e".repeat(64),
            },
        }),
        "complex",
    )
    .expect("coordinate features");
    assert!(
        coordinate_rows
            .iter()
            .any(|row| row.name == "sorted_extent_3_angstrom")
    );
    assert!(coordinate_rows.len() <= MAX_QUERY_STRUCTURE_FEATURE_ROWS);
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn query_structure_tokens_are_excluded_from_legacy_match_scores() {
    let query_rows =
        query_structure_feature_rows(&query_structure_small_molecule_fixture(1), "small_molecule")
            .expect("query structure features");
    let query_token = query_rows
        .iter()
        .find(|row| row.name == "hydrogen_bond_donors")
        .expect("query structure token")
        .token
        .clone();
    assert!(query_token.starts_with("qs_"));
    let input = ComputedFeatureInput {
        input_id: "input_1".to_string(),
        record_id: "record_1".to_string(),
        input_kind: "small_molecule".to_string(),
        tokens: BTreeSet::from(["ethanol".to_string(), query_token.clone()]),
    };
    let row = StoreRow {
        source: "fixture".to_string(),
        target_id: "TARGET".to_string(),
        target_name: "TARGET".to_string(),
        organism: "test".to_string(),
        target_class: "structure_interaction".to_string(),
        ligand_tokens: BTreeSet::from(["ethanol".to_string(), query_token.clone()]),
        sequence_tokens: BTreeSet::new(),
        structure_tokens: BTreeSet::from([query_token]),
        evidence: "query_structure_feature_fixture".to_string(),
    };
    assert_eq!(matched_tokens(&input, &row), vec!["ethanol".to_string()]);
    assert_eq!(score_row(&input, &row), 1.0);
    assert_eq!(
        input
            .tokens
            .iter()
            .filter(|token| structure_match_token_is_informative(token))
            .count(),
        1
    );
    assert_eq!(
        structure_matched_tokens(&input, &row),
        vec!["ethanol".to_string()]
    );
}

#[test]
fn ligand_similarity_rows_bind_to_the_current_query_structure_features() {
    let root = temp_root("ligand_similarity_current_query_binding");
    let input = root.join("input.json");
    let run = root.join("run");
    write_text(
        &input,
        r#"{"inputs":[
{"field":"small_molecule","value":"CCO"},
{"field":"genome_or_metagenome","value":">contig\nATGAAATAA"}
],"files":[]}"#,
    )
    .expect("write input");
    let options = science_options(&root, &input, &run);
    let publication = write_compact_ligand_similarity_publication_fixture(&options.science_store)
        .expect("compact publication");
    let publication_fields = compact_publication_fields_fixture(&publication);
    normalize_input(&options).expect("normalize");
    write_query_structure_contract_fixture(&options, 1).expect("initial query structure");
    let initial = read_query_structure_index(&run.join("query_structure_manifest.tsv"))
        .expect("initial query structure index")
        .into_iter()
        .find(|row| row.status == "ready")
        .expect("ready query structure");
    let query_id = ligand_similarity_query_id(
        &initial.input_id,
        &initial.record_id,
        &initial.feature_digest,
        "ChEMBL",
        "CHEMBL_T1",
        "CCO",
    );
    let current_row = format!(
        "input_id\tsource\ttarget_id\ttarget_name\torganism\ttarget_class\tmatch_score\tmatched_features\tevidence\tsource_row_provenance\tactivity_value\tligand_identity\tcurrent_run_query_id\trecord_id\n{}\tChEMBL\tCHEMBL_T1\tTarget one\tHomo sapiens\tenzyme\t0.9000\tcanonical_smiles=CCO;tanimoto=1.0\tligand_similarity_index;query_feature_digest={};{}\tligand_similarity_index;{}\tKi=12 nM\tligand_smiles=CCO\t{}\t{}\n",
        initial.input_id,
        initial.feature_digest,
        publication_fields,
        publication_fields,
        query_id,
        initial.record_id,
    );
    write_text(&run.join("known_database_matches.tsv"), &current_row).expect("current match row");
    validate_ligand_similarity_query_bindings(&options).expect("current row accepted");

    let forged_query_id = "0".repeat(64);
    write_text(
        &run.join("known_database_matches.tsv"),
        &current_row.replace(&query_id, &forged_query_id),
    )
    .expect("forged query id row");
    let error = validate_ligand_similarity_query_bindings(&options)
        .expect_err("forged query id must be rejected");
    assert!(error.contains("query_id"), "unexpected error: {error}");

    write_text(&run.join("known_database_matches.tsv"), &current_row).expect("restore old row");
    write_query_structure_contract_fixture(&options, 4).expect("rerun query structure");
    let current = read_query_structure_index(&run.join("query_structure_manifest.tsv"))
        .expect("current query structure index")
        .into_iter()
        .find(|row| row.status == "ready")
        .expect("current ready query structure");
    assert_ne!(initial.feature_digest, current.feature_digest);
    let error = validate_ligand_similarity_query_bindings(&options)
        .expect_err("stale feature row must be rejected");
    assert!(
        error.contains("feature_digest"),
        "unexpected stale-row error: {error}"
    );
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn rna_and_reverse_complement_orfs_keep_strand_and_frame_identity() {
    let rna = translate_orfs("AUGGCUUUUUAA");
    assert!(
        rna.iter()
            .any(|orf| orf.peptide == "MAF" && orf.strand == '+' && orf.frame == 0)
    );

    let reverse = translate_orfs("TTAGGGTTTCAT");
    assert!(
        reverse
            .iter()
            .any(|orf| orf.peptide == "MKP" && orf.strand == '-' && orf.frame == 0)
    );
}

#[test]
fn multi_fasta_records_do_not_form_cross_record_orfs() {
    let features = genome_or_metagenome_features(">alpha\nATGAAA\n>beta\nCCCTAA\n");
    assert!(
        features
            .iter()
            .any(|row| { row.name == "translated_orf_count" && row.value == "0" })
    );
    assert!(
        !features
            .iter()
            .any(|row| { row.name == "translated_orf_feature_token" && row.token == "mkp" })
    );
}

#[test]
fn genome_orf_features_preserve_fasta_record_strand_and_frame_identity() {
    let root = temp_root("genome_orf_record_identity");
    let fasta = root.join("contigs.fasta");
    write_text(&fasta, ">alpha\nATGCGTGTGTAA\n>beta\nTTAGGGTTTCAT\n").expect("fasta");
    let input = root.join("input.json");
    write_text(
        &input,
        &format!(
            r#"{{"inputs":[{{"field":"genome_or_metagenome","value":{}}}],"files":[]}}"#,
            serde_json::to_string(&fasta.display().to_string()).expect("path JSON")
        ),
    )
    .expect("input");
    let run = root.join("run");
    let options = science_options(&root, &input, &run);

    run_science_command(ScienceCommand::InputNormalize, &options).expect("normalize");
    run_science_command(ScienceCommand::FeaturesCompute, &options).expect("features");
    let features = read_text(&run.join("feature_manifest.tsv")).expect("feature manifest");

    assert!(features.lines().any(|line| {
        line.contains("\ttranslated_orf_identity\trecord=alpha__record_1;strand=+;frame=0;")
            && line.ends_with("\talpha__record_1")
    }));
    assert!(features.lines().any(|line| {
        line.contains("\ttranslated_orf_identity\trecord=beta__record_2;strand=-;frame=0;")
            && line.ends_with("\tbeta__record_2")
    }));
    assert!(features.lines().any(|line| {
        line.contains("\ttranslated_orf_feature_token\tmrv\t")
            && line.ends_with("\talpha__record_1")
    }));
    assert!(features.lines().any(|line| {
        line.contains("\ttranslated_orf_feature_token\tmkp\t") && line.ends_with("\tbeta__record_2")
    }));
    fs::remove_dir_all(root).expect("cleanup");
}

fn science_options(root: &Path, input: &Path, run: &Path) -> ScienceOptions {
    ScienceOptions {
        workspace_root: root.to_path_buf(),
        science_store: root.join("science_store"),
        input_manifest: input.to_path_buf(),
        run_dir: run.to_path_buf(),
    }
}

fn normalize_and_compute_fixture(
    options: &ScienceOptions,
    field: &str,
    value: &str,
) -> Result<(), String> {
    write_text(
        &options.input_manifest,
        &(serde_json::json!({
            "inputs": [{"field": field, "value": value}],
            "files": []
        })
        .to_string()
            + "\n"),
    )?;
    run_science_command(ScienceCommand::InputNormalize, options)?;
    run_science_command(ScienceCommand::FeaturesCompute, options)?;
    Ok(())
}

fn write_empty_optional_rank_artifacts(run: &Path) -> Result<(), String> {
    let match_header = "input_id\tsource\ttarget_id\ttarget_name\torganism\ttarget_class\tmatch_score\tmatched_features\tevidence\tsource_row_provenance\tactivity_value\tligand_identity\tcurrent_run_query_id\trecord_id\n";
    for relative in [
        "metagenome_potential_matches.tsv",
        "structure_interaction_matches.tsv",
        "tool_outputs/mmseqs_alignment_matches.tsv",
        "tool_outputs/foldseek_structure_matches.tsv",
    ] {
        let path = run.join(relative);
        if !path.is_file() {
            write_text(&path, match_header)?;
        }
    }
    let docking = run.join("docking_summary.tsv");
    if !docking.is_file() {
        write_text(
            &docking,
            "candidate_id\tdocking_status\tdocking_score\tdocking_engine\tnotes\n",
        )?;
    }
    let md = run.join("md_summary.tsv");
    if !md.is_file() {
        write_text(
            &md,
            "candidate_id\trmsd\trmsf\tligand_drift\thydrogen_bond_occupancy\tcontact_occupancy\tinteraction_persistence\tmd_risk_flags\tmd_score_decomposition\tmd_stability_score\n",
        )?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn prepare_bound_rank_fixture(
    options: &ScienceOptions,
    field: &str,
    value: &str,
    source: &str,
    target_id: &str,
    target_name: &str,
    evidence: &str,
    match_score: f64,
) -> Result<String, String> {
    normalize_and_compute_fixture(options, field, value)?;
    if current_pipeline_invocation_id(options).is_err() {
        start_pipeline_invocation(options)?;
    }
    let bound_evidence = bind_database_evidence(options, source, evidence)?;
    let bound_provenance = bind_database_evidence(options, source, "fixture")?;
    let query_digest = "b".repeat(64);
    write_text(
        &options.run_dir.join("known_database_matches.tsv"),
        &format!(
            "input_id\tsource\ttarget_id\ttarget_name\torganism\ttarget_class\tmatch_score\tmatched_features\tevidence\tsource_row_provenance\tactivity_value\tligand_identity\tcurrent_run_query_id\trecord_id\ninput_1\t{}\t{}\t{}\tHomo sapiens\tenzyme\t{match_score:.4}\tligand_identity\t{}\t{}\t\t{}\t{}\trecord_1\n",
            clean_cell(source),
            clean_cell(target_id),
            clean_cell(target_name),
            clean_cell(&bound_evidence),
            clean_cell(&bound_provenance),
            clean_cell(value),
            query_digest,
        ),
    )?;
    write_empty_optional_rank_artifacts(&options.run_dir)?;
    let namespace = canonical_target_namespace("", source, target_id);
    Ok(candidate_id_for(
        "input_1", "record_1", &namespace, target_id,
    ))
}

fn write_report_support(options: &ScienceOptions) -> Result<(), String> {
    if !options.run_dir.join("known_database_matches.tsv").is_file() {
        write_text(
            &options.run_dir.join("known_database_matches.tsv"),
            "input_id\tsource\ttarget_id\ttarget_name\torganism\ttarget_class\tmatch_score\tmatched_features\tevidence\tsource_row_provenance\tactivity_value\tligand_identity\tcurrent_run_query_id\trecord_id\n",
        )?;
    }
    write_empty_optional_rank_artifacts(&options.run_dir)?;
    let candidates = read_candidates(&options.run_dir.join("target_candidate_ranking.tsv"))?;
    write_candidate_detail(options, &candidates)?;
    write_text(
        &options.run_dir.join("evidence_ledger.tsv"),
        "claim_id\tevidence_artifact\towner\tstatus\tnotes\n",
    )
}

fn conversion_rows(run: &Path) -> Vec<Vec<String>> {
    read_text(&run.join("input_conversion_manifest.tsv"))
        .expect("conversion manifest")
        .lines()
        .skip(1)
        .map(split_tsv)
        .collect()
}

fn normalize_fixture_inputs(
    entries: &[(&str, &str)],
    identifiers: &[(&str, &str, &str)],
) -> Vec<ConvertedInput> {
    static FIXTURE_ID: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let fixture_id = FIXTURE_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let root = temp_root(&format!("input_identifier_fixture_{fixture_id}"));
    let store = root.join("science_store");
    let mut lookup =
        String::from("identifier_type\tidentifier\tcanonical_smiles\tdisplay_name\tsource\n");
    for (kind, identifier, smiles) in identifiers {
        lookup.push_str(&format!(
            "{kind}\t{identifier}\t{smiles}\t{identifier}\ttest\n"
        ));
    }
    write_text(&store.join("indexes/compound_identifiers.tsv"), &lookup).expect("lookup");
    let input = root.join("input.json");
    let rows = entries
        .iter()
        .map(|(field, value)| serde_json::json!({"field": field, "value": value}))
        .collect::<Vec<_>>();
    write_text(
        &input,
        &(serde_json::json!({"inputs": rows, "files": []}).to_string() + "\n"),
    )
    .expect("input");
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: store,
        input_manifest: input,
        run_dir: root.join("run"),
    };
    let converted = convert_inputs_from_manifest(&options).expect("convert");
    fs::remove_dir_all(root).expect("cleanup");
    converted
}

#[test]
fn explicit_small_molecule_field_prevents_sequence_reclassification() {
    let rows = normalize_fixture_inputs(
        &[
            ("small_molecule", "CCCCCCCC"),
            ("small_molecule", "caffeine"),
        ],
        &[("name", "caffeine", "Cn1c(=O)c2c(ncn2C)n(C)c1=O")],
    );

    assert_eq!(
        rows[0].canonical_kind,
        Some(InputKind::SmallMolecule),
        "{:#?}",
        rows[0]
    );
    assert_eq!(rows[0].canonical_value, "CCCCCCCC");
    assert_eq!(rows[0].conversion_method, "rdkit_smiles");
    assert_eq!(
        rows[1].canonical_kind,
        Some(InputKind::SmallMolecule),
        "{:#?}",
        rows[1]
    );
    assert_eq!(rows[1].canonical_value, "Cn1c(=O)c2c(ncn2C)n(C)c1=O");
    assert_eq!(rows[1].conversion_method, "local_identifier_lookup");
}

#[test]
fn inline_smiles_and_inchi_are_canonicalized_by_rdkit() {
    let rows =
        normalize_fixture_inputs(&[("smiles", "C(C)O"), ("inchi", "InChI=1S/CH4/h1H4")], &[]);

    assert_eq!(
        rows[0].canonical_kind,
        Some(InputKind::SmallMolecule),
        "{:#?}",
        rows[0]
    );
    assert_eq!(rows[0].canonical_value, "CCO");
    assert_eq!(rows[0].conversion_method, "rdkit_smiles");
    assert_eq!(rows[1].canonical_kind, Some(InputKind::SmallMolecule));
    assert_eq!(rows[1].canonical_value, "C");
    assert_eq!(rows[1].conversion_method, "rdkit_inchi");
}

#[test]
fn compound_identifier_lookup_validates_cas_and_canonicalizes_mapped_smiles() {
    let rows = normalize_fixture_inputs(
        &[("cas", "59-66-5"), ("small_molecule", "CAFÉ   MOLECULE")],
        &[
            ("cas", "59-66-5", "C(C)O"),
            ("name", "Café Molecule", "C(C)O"),
        ],
    );

    for row in &rows {
        assert_eq!(row.canonical_kind, Some(InputKind::SmallMolecule));
        assert_eq!(row.canonical_value, "CCO");
        assert_eq!(row.conversion_method, "local_identifier_lookup");
    }
}

#[test]
fn compound_identifier_lookup_rejects_invalid_cas_and_unknown_name() {
    let rows = normalize_fixture_inputs(
        &[
            ("cas", "59-66-4"),
            ("small_molecule", "unknown compound name"),
        ],
        &[("cas", "59-66-5", "CCO")],
    );

    assert_eq!(rows[0].conversion_status, "failed");
    assert!(rows[0].reason.contains("invalid_cas_check_digit"));
    assert_eq!(rows[1].conversion_status, "failed");
    assert!(rows[1].reason.contains("unknown_compound_identifier"));
}

#[test]
fn declared_sequence_and_complex_fields_determine_scientific_family() {
    let rows = normalize_fixture_inputs(
        &[
            ("peptide", "ACGTAC"),
            ("protein_ligand", "ACGTAC"),
            ("genome_or_metagenome", "ACGTACGT"),
            ("complex", "protein A with ligand B"),
        ],
        &[],
    );

    assert_eq!(rows[0].canonical_kind, Some(InputKind::Peptide));
    assert_eq!(rows[1].canonical_kind, Some(InputKind::ProteinLigand));
    assert_eq!(rows[2].canonical_kind, Some(InputKind::GenomeOrMetagenome));
    assert_eq!(rows[3].canonical_kind, Some(InputKind::Complex));
}

#[test]
fn compound_identifier_lookup_rejects_invalid_mapped_smiles() {
    let rows = normalize_fixture_inputs(
        &[("small_molecule", "broken mapping")],
        &[("name", "broken mapping", "not-a-smiles")],
    );

    assert_eq!(rows[0].conversion_status, "failed");
    assert!(rows[0].reason.contains("invalid_mapped_smiles"));
}

#[test]
fn input_inspection_reports_every_record_identity_and_failure() {
    let root = temp_root("input_inspection_failures");
    let input = root.join("input.json");
    write_text(
        &input,
        &(serde_json::json!({
            "inputs": [
                {"field": "project_name", "value": "inspection fixture"},
                {"field": "smiles", "value": "C(C)O"},
                {"field": "cas", "value": "59-66-4"}
            ],
            "files": []
        })
        .to_string()
            + "\n"),
    )
    .expect("input");

    let inspection = inspect_input_manifest(&science_options(&root, &input, &root.join("run")))
        .expect("inspect");
    fs::remove_dir_all(root).expect("cleanup");

    assert!(!inspection.usable);
    assert_eq!(inspection.inputs.len(), 2);
    assert_eq!(inspection.inputs[0].input_id, "input_1");
    assert_eq!(inspection.inputs[0].record_id, "record_1");
    assert_eq!(inspection.inputs[0].canonical_kind, "small_molecule");
    assert_eq!(inspection.inputs[0].conversion_method, "rdkit_smiles");
    assert_eq!(inspection.inputs[1].input_id, "input_2");
    assert_eq!(inspection.inputs[1].record_id, "record_1");
    assert_eq!(inspection.inputs[1].canonical_kind, "unknown");
    assert_eq!(inspection.errors.len(), 1);
    assert!(inspection.errors[0].contains("input_2:record_1:invalid_cas_check_digit"));
    assert!(inspection.summary.contains("1 of 2 input records failed"));
}

#[test]
fn input_inspection_is_usable_only_when_all_non_project_records_convert() {
    let root = temp_root("input_inspection_usable");
    let input = root.join("input.json");
    write_text(
        &input,
        &(serde_json::json!({
            "inputs": [
                {"field": "project_name", "value": "inspection fixture"},
                {"field": "small_molecule", "value": "CCO"},
                {"field": "peptide", "value": "IKVAV"}
            ],
            "files": []
        })
        .to_string()
            + "\n"),
    )
    .expect("input");

    let inspection = inspect_input_manifest(&science_options(&root, &input, &root.join("run")))
        .expect("inspect");
    fs::remove_dir_all(root).expect("cleanup");

    assert!(inspection.usable);
    assert!(inspection.errors.is_empty());
    assert_eq!(inspection.inputs.len(), 2);
    assert!(inspection.summary.contains("2 input records ready"));
}

#[test]
fn input_inspection_reports_each_malformed_non_project_input_value() {
    let root = temp_root("input_inspection_malformed_values");
    let input = root.join("input.json");
    write_text(
        &input,
        &(serde_json::json!({
            "inputs": [
                {"field": "project_name", "value": "inspection fixture"},
                {"field": "smiles", "value": "CCO"},
                {"field": "small_molecule", "value": 42},
                {"field": "peptide"},
                {"field": "protein_ligand", "value": ""}
            ],
            "files": []
        })
        .to_string()
            + "\n"),
    )
    .expect("input");

    let inspection = inspect_input_manifest(&science_options(&root, &input, &root.join("run")))
        .expect("inspect");
    fs::remove_dir_all(root).expect("cleanup");

    assert!(!inspection.usable);
    assert_eq!(inspection.inputs.len(), 4);
    for (index, row) in inspection.inputs.iter().enumerate() {
        assert_eq!(row.input_id, format!("input_{}", index + 1));
        assert_eq!(row.record_id, "record_1");
    }
    assert_eq!(inspection.inputs[0].canonical_kind, "small_molecule");
    assert_eq!(inspection.inputs[1].canonical_kind, "unknown");
    assert_eq!(inspection.inputs[2].canonical_kind, "unknown");
    assert_eq!(inspection.inputs[3].canonical_kind, "unknown");
    assert_eq!(
        inspection.errors,
        vec![
            "input_2:record_1:invalid_input_value_type",
            "input_3:record_1:missing_input_value",
            "input_4:record_1:empty_input_value",
        ]
    );
}

#[test]
fn input_inspection_reports_each_malformed_non_project_file_value() {
    let root = temp_root("input_inspection_malformed_files");
    let sdf = root.join("ethanol.sdf");
    write_text(&sdf, &format!("{}$$$$\n", ethanol_mol_block())).expect("sdf");
    let input = root.join("input.json");
    write_text(
        &input,
        &(serde_json::json!({
            "inputs": [{"field": "project_name", "value": "inspection fixture"}],
            "files": [
                {
                    "field": "small_molecule",
                    "name": "ethanol.sdf",
                    "persisted_path": sdf.display().to_string()
                },
                {"field": "small_molecule", "name": "bad.sdf", "persisted_path": 42},
                {"field": "peptide"},
                {"field": "protein_ligand", "name": "", "persisted_path": ""},
                {"field": "project_name", "name": "metadata"}
            ]
        })
        .to_string()
            + "\n"),
    )
    .expect("input");

    let converted =
        convert_inputs_from_manifest(&science_options(&root, &input, &root.join("run")))
            .expect("convert");
    fs::remove_dir_all(root).expect("cleanup");

    assert_eq!(converted.len(), 4);
    for (index, row) in converted.iter().enumerate() {
        assert_eq!(row.input_id, format!("input_{}", index + 1));
        assert_eq!(row.record_id, "record_1");
        assert_eq!(row.source_kind, "file");
    }
    assert_eq!(converted[0].canonical_kind, Some(InputKind::SmallMolecule));
    assert_eq!(converted[0].source_path, sdf.display().to_string());
    assert_eq!(converted[0].content_sha256.len(), 64);
    assert_eq!(converted[1].reason, "invalid_input_value_type");
    assert_eq!(converted[2].reason, "missing_input_value");
    assert_eq!(converted[3].reason, "empty_input_value");
}

#[test]
fn input_inspection_rejects_unsupported_file_paths_without_inline_fallback() {
    let root = temp_root("input_inspection_unsupported_file_paths");
    let input = root.join("input.json");
    let unknown_extension = root.join("ghost.unknown");
    let no_extension = root.join("ghost");
    let known_missing = root.join("ghost.sdf");
    write_text(
        &input,
        &(serde_json::json!({
            "inputs": [{"field": "smiles", "value": "CCO"}],
            "files": [
                {
                    "field": "small_molecule",
                    "name": "ghost.unknown",
                    "persisted_path": unknown_extension.display().to_string()
                },
                {
                    "field": "peptide",
                    "name": "ghost",
                    "persisted_path": no_extension.display().to_string()
                },
                {
                    "field": "small_molecule",
                    "name": "ghost.sdf",
                    "persisted_path": known_missing.display().to_string()
                }
            ]
        })
        .to_string()
            + "\n"),
    )
    .expect("input");
    let options = science_options(&root, &input, &root.join("run"));

    let converted = convert_inputs_from_manifest(&options).expect("convert");
    let inspection = inspect_input_manifest(&options).expect("inspect");
    fs::remove_dir_all(root).expect("cleanup");

    assert!(!inspection.usable);
    assert_eq!(converted.len(), 4);
    assert_eq!(converted[0].canonical_kind, Some(InputKind::SmallMolecule));
    for (index, row) in converted[1..].iter().enumerate() {
        assert_eq!(row.input_id, format!("input_{}", index + 2));
        assert_eq!(row.record_id, "record_1");
        assert_eq!(row.source_kind, "file");
        assert_eq!(row.conversion_status, "failed");
    }
    assert_eq!(
        converted[1].source_path,
        unknown_extension.display().to_string()
    );
    assert_eq!(converted[1].reason, "unsupported_input_file_format");
    assert_eq!(converted[2].source_path, no_extension.display().to_string());
    assert_eq!(converted[2].reason, "unsupported_input_file_format");
    assert!(converted[3].reason.contains("input_file_not_found"));
}

#[test]
fn input_manifest_rejects_non_array_input_and_file_sections() {
    let root = temp_root("input_manifest_array_schema");
    let input = root.join("input.json");

    write_text(&input, r#"{"inputs":{},"files":[]}"#).expect("input");
    let error = inspect_input_manifest(&science_options(&root, &input, &root.join("run")))
        .expect_err("inputs object must fail schema validation");
    assert_eq!(error, "invalid_input_manifest_schema:inputs_must_be_array");

    write_text(&input, r#"{"inputs":[],"files":"not-an-array"}"#).expect("input");
    let error = inspect_input_manifest(&science_options(&root, &input, &root.join("run")))
        .expect_err("files string must fail schema validation");
    assert_eq!(error, "invalid_input_manifest_schema:files_must_be_array");

    write_text(&input, "{}\n").expect("input");
    let inspection = inspect_input_manifest(&science_options(&root, &input, &root.join("run")))
        .expect("missing sections are empty arrays");
    assert!(!inspection.usable);
    assert!(inspection.inputs.is_empty());
    assert!(inspection.errors.is_empty());

    fs::remove_dir_all(root).expect("cleanup");
}

fn minimal_pdb_block() -> &'static str {
    "ATOM      1  N   GLY A   1       0.000   0.000   0.000  1.00  0.00           N  \nATOM      2  CA  GLY A   1       1.458   0.000   0.000  1.00  0.00           C  \nTER       3      GLY A   1\nEND\n"
}

fn convert_declared_file(root: &Path, label: &str, field: &str, source: &Path) -> ConvertedInput {
    let input = root.join(format!("input_{label}.json"));
    write_text(
        &input,
        &(serde_json::json!({
            "inputs": [{"field": field, "value": source.display().to_string()}],
            "files": []
        })
        .to_string()
            + "\n"),
    )
    .expect("input");
    let mut converted = convert_inputs_from_manifest(&science_options(
        root,
        &input,
        &root.join(format!("run_{label}")),
    ))
    .expect("convert");
    assert_eq!(converted.len(), 1);
    converted.remove(0)
}

#[test]
fn declared_fields_control_coordinate_and_molecule_file_compatibility() {
    let root = temp_root("declared_file_compatibility");
    let pdb = root.join("structure.pdb");
    let sdf = root.join("ethanol.sdf");
    write_text(&pdb, minimal_pdb_block()).expect("pdb");
    write_text(&sdf, &format!("{}$$$$\n", ethanol_mol_block())).expect("sdf");

    for (label, field, expected_kind) in [
        ("pdb_peptide", "peptide", InputKind::Peptide),
        ("pdb_protein", "protein_ligand", InputKind::ProteinLigand),
        ("pdb_complex", "complex", InputKind::Complex),
    ] {
        let row = convert_declared_file(&root, label, field, &pdb);
        assert_eq!(row.canonical_kind, Some(expected_kind), "field {field}");
        assert_eq!(row.record_id, "record_1");
        assert_eq!(row.source_kind, "file");
        assert_eq!(row.source_path, pdb.display().to_string());
        assert_eq!(row.content_sha256.len(), 64);
        assert_eq!(row.structure_atom_count, 2);
        assert_eq!(row.structure_chain_count, 1);
        assert_eq!(row.structure_residue_count, 1);
    }

    for (label, field) in [
        ("pdb_small", "small_molecule"),
        ("pdb_genome", "genome_or_metagenome"),
    ] {
        let row = convert_declared_file(&root, label, field, &pdb);
        assert_eq!(row.conversion_status, "failed", "field {field}");
        assert!(
            row.reason
                .contains(&format!("field_format_incompatible:{field}:pdb")),
            "{}",
            row.reason
        );
    }

    for (label, field) in [
        ("sdf_peptide", "peptide"),
        ("sdf_protein", "protein_ligand"),
        ("sdf_genome", "genome_or_metagenome"),
        ("sdf_complex", "complex"),
    ] {
        let row = convert_declared_file(&root, label, field, &sdf);
        assert_eq!(row.conversion_status, "failed", "field {field}");
        assert!(
            row.reason
                .contains(&format!("field_format_incompatible:{field}:sdf")),
            "{}",
            row.reason
        );
    }

    let untyped_sdf = convert_declared_file(&root, "untyped_sdf", "unknown", &sdf);
    assert_eq!(untyped_sdf.canonical_kind, Some(InputKind::SmallMolecule));
    let untyped_pdb = convert_declared_file(&root, "untyped_pdb", "unknown", &pdb);
    assert_eq!(untyped_pdb.canonical_kind, Some(InputKind::Complex));

    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn genome_field_accepts_nucleotide_or_protein_content_as_genome_family() {
    let root = temp_root("genome_nucleotide_or_protein");
    let faa = root.join("protein.faa");
    write_text(&faa, ">protein\nMKTAYI\n").expect("faa");

    let inline_protein = root.join("inline_protein.json");
    write_text(
        &inline_protein,
        r#"{"inputs":[{"field":"genome_or_metagenome","value":"MKTAYI"}],"files":[]}"#,
    )
    .expect("input");
    let options = science_options(&root, &inline_protein, &root.join("run_inline"));
    let inline_rows = convert_inputs_from_manifest(&options).expect("convert inline protein");
    assert_eq!(
        inline_rows[0].canonical_kind,
        Some(InputKind::GenomeOrMetagenome)
    );
    assert_eq!(
        inline_rows[0].conversion_method,
        "standard_amino_acid_sequence"
    );

    let file_row = convert_declared_file(&root, "genome_faa", "genome_or_metagenome", &faa);
    assert_eq!(file_row.canonical_kind, Some(InputKind::GenomeOrMetagenome));
    assert_eq!(file_row.canonical_value, "MKTAYI");
    assert_eq!(file_row.conversion_method, "standard_amino_acid_sequence");
    assert_eq!(file_row.record_id, "protein__record_1");

    let nucleotide = root.join("nucleotide.json");
    write_text(
        &nucleotide,
        r#"{"inputs":[{"field":"genome_or_metagenome","value":"ACGTACGT"}],"files":[]}"#,
    )
    .expect("input");
    let nucleotide_rows = convert_inputs_from_manifest(&science_options(
        &root,
        &nucleotide,
        &root.join("run_nucleotide"),
    ))
    .expect("convert nucleotide");
    assert_eq!(nucleotide_rows[0].conversion_method, "nucleotide_sequence");
    assert_eq!(
        nucleotide_rows[0].canonical_kind,
        Some(InputKind::GenomeOrMetagenome)
    );

    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn genome_field_falls_back_to_amino_acids_for_short_nucleotide_alphabet_sequences() {
    let root = temp_root("genome_short_nucleotide_alphabet");
    let inline_input = root.join("inline.json");
    write_text(
        &inline_input,
        r#"{"inputs":[{"field":"genome_or_metagenome","value":"ACGTAC"}],"files":[]}"#,
    )
    .expect("input");
    let inline_rows = convert_inputs_from_manifest(&science_options(
        &root,
        &inline_input,
        &root.join("run_inline"),
    ))
    .expect("convert inline");

    assert_eq!(inline_rows.len(), 1);
    assert_eq!(inline_rows[0].record_id, "record_1");
    assert_eq!(
        inline_rows[0].canonical_kind,
        Some(InputKind::GenomeOrMetagenome)
    );
    assert_eq!(
        inline_rows[0].conversion_method,
        "standard_amino_acid_sequence"
    );

    let faa = root.join("ambiguous.faa");
    write_text(&faa, ">ambiguous\nACGTAC\n").expect("faa");
    let file_row = convert_declared_file(
        &root,
        "genome_short_nucleotide_faa",
        "genome_or_metagenome",
        &faa,
    );
    assert_eq!(file_row.record_id, "ambiguous__record_1");
    assert_eq!(file_row.source_kind, "file");
    assert_eq!(file_row.canonical_kind, Some(InputKind::GenomeOrMetagenome));
    assert_eq!(file_row.conversion_method, "standard_amino_acid_sequence");

    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn input_conversion_manifest_records_inline_provenance() {
    let root = temp_root("inline_provenance");
    let input = root.join("input.json");
    write_text(
        &input,
        r#"{"inputs":[{"field":"small_molecule","value":"CCO"}],"files":[]}"#,
    )
    .expect("input");
    let run = root.join("run");
    run_science_command(
        ScienceCommand::InputNormalize,
        &science_options(&root, &input, &run),
    )
    .expect("normalize");

    let manifest = read_text(&run.join("input_conversion_manifest.tsv")).expect("manifest");
    assert_eq!(
        manifest.lines().next(),
        Some(
            "input_id\trecord_id\toriginal_field\tinferred_kind\tcanonical_kind\tcanonical_value\tconversion_status\tconversion_method\tformat\tsource_kind\tsource_path\tcontent_sha256\treason\tartifact\trisk_flags\tstructure_atom_count\tstructure_chain_count\tstructure_residue_count"
        )
    );
    let rows = conversion_rows(&run);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0][1], "record_1");
    assert_eq!(rows[0][9], "inline");
    assert_eq!(rows[0][10], "");
    assert_eq!(rows[0][11].len(), 64);
    assert!(rows[0][11].chars().all(|ch| ch.is_ascii_hexdigit()));
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn file_input_reads_fasta_records_and_hashes_content() {
    let root = temp_root("fasta_content_resolution");
    let fasta = root.join("queries.fasta");
    write_text(&fasta, ">alpha\nMKTAYI\n>beta\nACDEFG\n").expect("fasta");
    let input = root.join("input.json");
    write_text(
        &input,
        &format!(
            r#"{{"inputs":[{{"field":"protein_ligand","value":{}}}],"files":[]}}"#,
            serde_json::to_string(&fasta.display().to_string()).expect("path JSON")
        ),
    )
    .expect("input");
    let run = root.join("run");
    let options = science_options(&root, &input, &run);
    run_science_command(ScienceCommand::InputNormalize, &options).expect("normalize");
    let rows = conversion_rows(&run);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0][0], "input_1");
    assert_eq!(rows[0][1], "alpha__record_1");
    assert!(rows[0][5].is_empty());
    assert_eq!(rows[1][1], "beta__record_2");
    assert!(rows[1][5].is_empty());
    for (row, sequence) in rows.iter().zip(["MKTAYI", "ACDEFG"]) {
        assert_eq!(row[9], "file");
        assert_eq!(row[10], fasta.display().to_string());
        assert_eq!(
            row[11],
            format!("{:x}", Sha256::digest(sequence.as_bytes())),
            "each FASTA record must carry its own canonical content digest"
        );
        assert!(row[13].starts_with("input_sequence_spool.bin#offset="));
    }
    let restored = read_normalized_inputs(&run.join("input_normalized_manifest.tsv"))
        .expect("restore normalized sequences");
    assert_eq!(restored[0].primary_value, "MKTAYI");
    assert_eq!(restored[1].primary_value, "ACDEFG");
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn duplicate_fasta_headers_receive_bounded_stable_record_ids() {
    let root = temp_root("duplicate_fasta_record_ids");
    let fasta = root.join("queries.fasta");
    write_text(&fasta, ">duplicate\nMKTAYI\n>duplicate\nACDEFG\n").expect("fasta");
    let input = root.join("input.json");
    write_text(
        &input,
        &format!(
            r#"{{"inputs":[{{"field":"protein_ligand","value":{}}}],"files":[]}}"#,
            serde_json::to_string(&fasta.display().to_string()).expect("path JSON")
        ),
    )
    .expect("input");
    let run = root.join("run");
    run_science_command(
        ScienceCommand::InputNormalize,
        &science_options(&root, &input, &run),
    )
    .expect("duplicate source headers are disambiguated by record position");
    let rows = conversion_rows(&run);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0][1], "duplicate__record_1");
    assert_eq!(rows[1][1], "duplicate__record_2");
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn sequence_parse_failure_discards_partial_records_and_reports_the_source_line() {
    let root = temp_root("streaming_fasta_failure");
    let fasta = root.join("broken.fasta");
    write_text(&fasta, ">accepted\nMKTAYI\n>broken\n").expect("fasta");
    let input = root.join("input.json");
    write_text(
        &input,
        &format!(
            r#"{{"inputs":[{{"field":"protein_ligand","value":{}}}],"files":[]}}"#,
            serde_json::to_string(&fasta.display().to_string()).expect("path JSON")
        ),
    )
    .expect("input");

    let run = root.join("run");
    let error = run_science_command(
        ScienceCommand::InputNormalize,
        &science_options(&root, &input, &run),
    )
    .expect_err("malformed record must fail the complete source");
    let rows = conversion_rows(&run);

    assert!(error.contains("invalid_fasta_record:empty_sequence"));
    assert!(error.contains("line_3"));
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0][6], "converted");
    assert_eq!(rows[1][6], "failed");
    assert!(!run.join("input_normalized_manifest.tsv").exists());
    assert!(!run.join("input_conversion_manifest.tsv.pending").exists());
    assert!(!run.join("input_normalized_manifest.tsv.pending").exists());
    assert!(!run.join("input_sequence_spool.bin").exists());
    assert!(!run.join("input_sequence_spool.bin.pending").exists());
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn rerun_failure_or_inline_input_removes_published_sequence_spool() {
    let root = temp_root("sequence_spool_rerun_cleanup");
    let fasta = root.join("query.fasta");
    let input = root.join("input.json");
    let run = root.join("run");
    write_text(&fasta, ">accepted\nMKTAYI\n").expect("fasta");
    write_text(
        &input,
        &format!(
            r#"{{"inputs":[{{"field":"protein_ligand","value":{}}}],"files":[]}}"#,
            serde_json::to_string(&fasta.display().to_string()).expect("path JSON")
        ),
    )
    .expect("input");
    let options = science_options(&root, &input, &run);

    run_science_command(ScienceCommand::InputNormalize, &options).expect("initial normalize");
    assert!(run.join("input_sequence_spool.bin").is_file());

    write_text(&fasta, ">accepted\nMKTAYI\n>broken\n").expect("broken fasta");
    run_science_command(ScienceCommand::InputNormalize, &options)
        .expect_err("broken rerun must fail");
    assert!(!run.join("input_normalized_manifest.tsv").exists());
    assert!(!run.join("input_sequence_spool.bin").exists());
    assert!(!run.join("input_sequence_spool.bin.pending").exists());

    write_text(
        &input,
        r#"{"inputs":[{"field":"small_molecule","value":"CCO"}],"files":[]}"#,
    )
    .expect("inline input");
    run_science_command(ScienceCommand::InputNormalize, &options).expect("inline normalize");
    assert!(!run.join("input_sequence_spool.bin").exists());
    assert!(!run.join("input_sequence_spool.bin.pending").exists());
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn sequence_conversion_manifest_streams_across_write_batches() {
    let root = temp_root("streaming_fasta_batches");
    let fasta = root.join("many.fasta");
    let sequence = "MKTAYI".repeat(128);
    let mut source = String::new();
    for index in 0..(INPUT_MANIFEST_WRITE_BATCH_SIZE * 2 + 3) {
        source.push_str(&format!(">record_{index}\n{sequence}\n"));
    }
    write_text(&fasta, &source).expect("fasta");
    let input = root.join("input.json");
    write_text(
        &input,
        &format!(
            r#"{{"inputs":[{{"field":"protein_ligand","value":{}}}],"files":[]}}"#,
            serde_json::to_string(&fasta.display().to_string()).expect("path JSON")
        ),
    )
    .expect("input");
    let run = root.join("run");

    run_science_command(
        ScienceCommand::InputNormalize,
        &science_options(&root, &input, &run),
    )
    .expect("normalize");
    let rows = conversion_rows(&run);
    let conversion = read_text(&run.join("input_conversion_manifest.tsv")).expect("conversion");
    let normalized = read_text(&run.join("input_normalized_manifest.tsv")).expect("normalized");
    let normalized_rows = normalized
        .lines()
        .skip(1)
        .map(split_tsv)
        .collect::<Vec<_>>();

    assert_eq!(rows.len(), INPUT_MANIFEST_WRITE_BATCH_SIZE * 2 + 3);
    assert_eq!(normalized_rows.len(), rows.len());
    assert_eq!(
        rows.first().map(|row| row[1].as_str()),
        Some("record_0__record_1")
    );
    assert_eq!(
        rows.last().map(|row| row[1].as_str()),
        Some("record_514__record_515")
    );
    assert!(rows.iter().all(|row| row[11].len() == 64));
    assert!(rows.iter().all(|row| row[5].is_empty()));
    assert!(
        rows.iter()
            .all(|row| row[13].starts_with("input_sequence_spool.bin#offset="))
    );
    assert!(normalized_rows.iter().all(|row| row[2].is_empty()));
    assert!(
        normalized_rows
            .iter()
            .all(|row| row[4].split_whitespace().count() <= 17)
    );
    assert!(!conversion.contains(&sequence));
    assert!(!normalized.contains(&sequence));
    assert_eq!(
        fs::metadata(run.join("input_sequence_spool.bin"))
            .expect("sequence spool")
            .len(),
        (sequence.len() * rows.len()) as u64
    );
    let restored = read_normalized_inputs(&run.join("input_normalized_manifest.tsv"))
        .expect("restore normalized sequences");
    assert_eq!(
        restored
            .first()
            .expect("first restored sequence")
            .primary_value,
        sequence
    );
    assert!(!run.join("input_sequence_spool.bin.pending").exists());
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn feature_manifest_preserves_multi_record_identity() {
    let root = temp_root("feature_record_identity");
    let store = root.join("science_store");
    write_test_source_manifest(&store, &["chembl"]).expect("source manifest");
    write_text(
        &store.join("indexes/known_targets.tsv"),
        "source\ttarget_id\ttarget_name\torganism\ttarget_class\tligand_tokens\tsequence_tokens\tstructure_tokens\tevidence\nChEMBL\tT1\tRecord identity target\ttest\tprotein\t\tmkt acd\t\trecord_identity_fixture\n",
    )
    .expect("known targets");
    let fasta = root.join("queries.fasta");
    write_text(&fasta, ">alpha\nMKTAYI\n>beta\nACDEFG\n").expect("fasta");
    let input = root.join("input.json");
    write_text(
        &input,
        &format!(
            r#"{{"inputs":[{{"field":"protein_ligand","value":{}}}],"files":[]}}"#,
            serde_json::to_string(&fasta.display().to_string()).expect("path JSON")
        ),
    )
    .expect("input");
    let run = root.join("run");
    let options = science_options(&root, &input, &run);
    run_science_command(ScienceCommand::InputNormalize, &options).expect("normalize");
    run_science_command(ScienceCommand::FeaturesCompute, &options).expect("features");
    let features = read_text(&run.join("feature_manifest.tsv")).expect("feature manifest");
    let computed = read_computed_feature_inputs(&run.join("feature_manifest.tsv"))
        .expect("computed feature inputs");
    run_science_command(ScienceCommand::DatabaseMatchKnown, &options).expect("known matching");
    let matches = read_text(&run.join("known_database_matches.tsv")).expect("known matches");
    fs::remove_dir_all(root).expect("cleanup");
    assert!(
        features
            .lines()
            .next()
            .is_some_and(|line| line.ends_with("\trecord_id"))
    );
    assert!(
        features
            .lines()
            .any(|line| line.ends_with("\talpha__record_1"))
    );
    assert!(
        features
            .lines()
            .any(|line| line.ends_with("\tbeta__record_2"))
    );
    assert_eq!(computed.len(), 2);
    assert_eq!(
        computed
            .iter()
            .map(|input| input.record_id.as_str())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["alpha__record_1", "beta__record_2"])
    );
    assert!(
        matches
            .lines()
            .any(|line| line.ends_with("\talpha__record_1"))
    );
    assert!(
        matches
            .lines()
            .any(|line| line.ends_with("\tbeta__record_2"))
    );
}

#[test]
fn sequence_file_formats_are_resolved_from_content() {
    let root = temp_root("sequence_file_formats");
    let cases = [
        (
            "fa",
            "protein_ligand",
            ">alpha\nMKTAYI\n",
            "alpha__record_1",
            "MKTAYI",
        ),
        (
            "faa",
            "protein_ligand",
            ">alpha\nMKTAYI\n",
            "alpha__record_1",
            "MKTAYI",
        ),
        (
            "fna",
            "genome_or_metagenome",
            ">contig\nATGCGTACGTAA\n",
            "contig__record_1",
            "ATGCGTACGTAA",
        ),
        (
            "fastq",
            "genome_or_metagenome",
            "@read1\nATGCGTAA\n+\nFFFFFFFF\n",
            "read1__record_1",
            "ATGCGTAA",
        ),
        (
            "fq",
            "genome_or_metagenome",
            "@read1\nATGCGTAA\n+\nFFFFFFFF\n",
            "read1__record_1",
            "ATGCGTAA",
        ),
        (
            "gb",
            "genome_or_metagenome",
            "LOCUS       TEST\nORIGIN\n        1 atgcgtaa\n//\n",
            "TEST__record_1",
            "ATGCGTAA",
        ),
        (
            "gbk",
            "genome_or_metagenome",
            "LOCUS       TEST\nORIGIN\n        1 atgcgtaa\n//\n",
            "TEST__record_1",
            "ATGCGTAA",
        ),
    ];
    for (index, (extension, field, content, record_id, expected)) in cases.iter().enumerate() {
        let source = root.join(format!("sequence_{index}.{extension}"));
        write_text(&source, content).expect("source");
        let input = root.join(format!("input_{index}.json"));
        write_text(
            &input,
            &format!(
                r#"{{"inputs":[{{"field":"{field}","value":{}}}],"files":[]}}"#,
                serde_json::to_string(&source.display().to_string()).expect("path JSON")
            ),
        )
        .expect("input");
        let run = root.join(format!("run_{index}"));
        run_science_command(
            ScienceCommand::InputNormalize,
            &science_options(&root, &input, &run),
        )
        .expect("normalize");
        let rows = conversion_rows(&run);
        assert_eq!(rows.len(), 1, "extension {extension}");
        assert_eq!(rows[0][1], *record_id, "extension {extension}");
        assert!(rows[0][5].is_empty(), "extension {extension}");
        assert_eq!(rows[0][8], *extension, "extension {extension}");
        assert_eq!(rows[0][9], "file", "extension {extension}");
        assert_eq!(rows[0][10], source.display().to_string());
        let restored = read_normalized_inputs(&run.join("input_normalized_manifest.tsv"))
            .expect("restore normalized sequence");
        assert_eq!(
            restored[0].primary_value, *expected,
            "extension {extension}"
        );
    }
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn malformed_fastq_is_a_failed_conversion() {
    let root = temp_root("malformed_fastq");
    let fastq = root.join("broken.fastq");
    write_text(&fastq, "@read1\nATGCGTAA\nnot-plus\nFFFF\n").expect("fastq");
    let input = root.join("input.json");
    write_text(
        &input,
        &format!(
            r#"{{"inputs":[{{"field":"genome_or_metagenome","value":{}}}],"files":[]}}"#,
            serde_json::to_string(&fastq.display().to_string()).expect("path JSON")
        ),
    )
    .expect("input");
    let run = root.join("run");
    let error = run_science_command(
        ScienceCommand::InputNormalize,
        &science_options(&root, &input, &run),
    )
    .expect_err("malformed FASTQ must fail normalization");
    assert!(error.contains("invalid_fastq_record:missing_separator:line_3"));
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn multirecord_genbank_preserves_locus_identity_and_record_digest() {
    let root = temp_root("streaming_genbank_records");
    let genbank = root.join("records.gbk");
    write_text(
        &genbank,
        "LOCUS       FIRST\nORIGIN\n        1 atgcgtaa\n//\nLOCUS       SECOND\nORIGIN\n        1 acgtacgt\n//\n",
    )
    .expect("genbank");
    let input = root.join("input.json");
    write_text(
        &input,
        &format!(
            r#"{{"inputs":[{{"field":"genome_or_metagenome","value":{}}}],"files":[]}}"#,
            serde_json::to_string(&genbank.display().to_string()).expect("path JSON")
        ),
    )
    .expect("input");
    let run = root.join("run");

    run_science_command(
        ScienceCommand::InputNormalize,
        &science_options(&root, &input, &run),
    )
    .expect("normalize");
    let rows = conversion_rows(&run);

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0][1], "FIRST__record_1");
    assert_eq!(rows[1][1], "SECOND__record_2");
    assert_ne!(rows[0][11], rows[1][11]);
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn persisted_file_path_is_resolved_with_provenance() {
    let root = temp_root("persisted_file_resolution");
    let fasta = root.join("uploaded.faa");
    write_text(&fasta, ">persisted\nMKTAYI\n").expect("fasta");
    let input = root.join("input.json");
    write_text(
        &input,
        &format!(
            r#"{{"inputs":[],"files":[{{"field":"protein_ligand","name":"client-name.faa","persisted_path":{}}}]}}"#,
            serde_json::to_string(&fasta.display().to_string()).expect("path JSON")
        ),
    )
    .expect("input");
    let run = root.join("run");
    run_science_command(
        ScienceCommand::InputNormalize,
        &science_options(&root, &input, &run),
    )
    .expect("normalize");
    let rows = conversion_rows(&run);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0][1], "persisted__record_1");
    assert!(rows[0][5].is_empty());
    assert!(rows[0][13].starts_with("input_sequence_spool.bin#offset="));
    assert_eq!(rows[0][10], fasta.display().to_string());
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn relative_file_path_prefers_manifest_directory_over_process_cwd() {
    let root = temp_root("relative_file_resolution");
    let manifest_dir = root.join("manifest");
    let current_dir = std::env::current_dir().expect("current directory");
    let relative = PathBuf::from("work").join(format!(
        "input_resolution_shadow_{}_{}.faa",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    let process_shadow = current_dir.join(&relative);
    let manifest_source = manifest_dir.join(&relative);
    write_text(&process_shadow, ">process-cwd\nACDEFG\n").expect("process shadow");
    write_text(&manifest_source, ">manifest-dir\nMKTAYI\n").expect("manifest source");
    let input = manifest_dir.join("input.json");
    write_text(
        &input,
        &format!(
            r#"{{"inputs":[{{"field":"protein_ligand","value":{}}}],"files":[]}}"#,
            serde_json::to_string(&relative.display().to_string()).expect("path JSON")
        ),
    )
    .expect("input");
    let run = root.join("run");
    let outcome = run_science_command(
        ScienceCommand::InputNormalize,
        &science_options(&root, &input, &run),
    )
    .and_then(|_| {
        let manifest = read_text(&run.join("input_conversion_manifest.tsv"))?;
        let rows = conversion_rows(&run);
        let restored = read_normalized_inputs(&run.join("input_normalized_manifest.tsv"))?;
        Ok((manifest, rows, restored))
    });

    let work_dir = fs::canonicalize(current_dir.join("work")).expect("work directory");
    let shadow_path = fs::canonicalize(&process_shadow).expect("shadow path");
    assert!(shadow_path.starts_with(&work_dir));
    fs::remove_file(&shadow_path).expect("cleanup process shadow");
    fs::remove_dir_all(&root).expect("cleanup fixture");

    let (manifest, rows, restored) = outcome.expect("normalize relative input");
    assert!(manifest.contains("manifest-dir"));
    assert!(!manifest.contains("MKTAYI"));
    assert!(!manifest.contains("process-cwd"));
    assert!(!manifest.contains("ACDEFG"));
    assert_eq!(rows.len(), 1);
    assert!(rows[0][5].is_empty());
    assert!(rows[0][13].starts_with("input_sequence_spool.bin#offset="));
    assert_eq!(rows[0][10], manifest_source.display().to_string());
    assert_eq!(restored.len(), 1);
    assert_eq!(restored[0].record_id, "manifest-dir__record_1");
    assert_eq!(restored[0].primary_value, "MKTAYI");
    assert_eq!(
        restored[0].source_path,
        manifest_source.display().to_string()
    );
}

#[test]
fn relative_file_path_never_falls_back_to_process_cwd() {
    let root = temp_root("relative_file_no_cwd_fallback");
    let manifest_dir = root.join("manifest");
    let current_dir = std::env::current_dir().expect("current directory");
    let relative = PathBuf::from("work").join(format!(
        "input_resolution_cwd_only_{}_{}.faa",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    let process_shadow = current_dir.join(&relative);
    write_text(&process_shadow, ">process-cwd\nACDEFG\n").expect("process shadow");
    let input = manifest_dir.join("input.json");
    write_text(
        &input,
        &format!(
            r#"{{"inputs":[{{"field":"protein_ligand","value":{}}}],"files":[]}}"#,
            serde_json::to_string(&relative.display().to_string()).expect("path JSON")
        ),
    )
    .expect("input");
    let run = root.join("run");
    let outcome = run_science_command(
        ScienceCommand::InputNormalize,
        &science_options(&root, &input, &run),
    );

    let work_dir = fs::canonicalize(current_dir.join("work")).expect("work directory");
    let shadow_path = fs::canonicalize(&process_shadow).expect("shadow path");
    assert!(shadow_path.starts_with(&work_dir));
    fs::remove_file(&shadow_path).expect("cleanup process shadow");
    fs::remove_dir_all(&root).expect("cleanup fixture");

    let error = outcome.expect_err("relative path must not resolve from process cwd");
    assert!(error.contains("input_file_not_found"));
}

#[test]
fn nonexistent_declared_file_is_a_failed_conversion() {
    let root = temp_root("missing_declared_file");
    let input = root.join("input.json");
    write_text(
        &input,
        r#"{"inputs":[{"field":"small_molecule","value":"Z:\\missing\\ghost.sdf"}],"files":[]}"#,
    )
    .expect("input");
    let run = root.join("run");
    let options = science_options(&root, &input, &run);
    let error = run_science_command(ScienceCommand::InputNormalize, &options)
        .expect_err("missing file must fail normalization");
    assert!(error.contains("input_file_not_found"));
    let rows = conversion_rows(&run);
    assert_eq!(rows[0][6], "failed");
    assert_eq!(rows[0][5], "");
    assert!(rows[0][12].contains("input_file_not_found"));
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn annotation_files_require_a_reference_sequence() {
    let root = temp_root("annotation_reference_requirement");
    for extension in ["gff", "gtf", "vcf"] {
        let source = root.join(format!("annotations.{extension}"));
        write_text(&source, "##fileformat=test\n").expect("annotation");
        let input = root.join(format!("input_{extension}.json"));
        write_text(
            &input,
            &format!(
                r#"{{"inputs":[{{"field":"genome_or_metagenome","value":{}}}],"files":[]}}"#,
                serde_json::to_string(&source.display().to_string()).expect("path JSON")
            ),
        )
        .expect("input");
        let run = root.join(format!("run_{extension}"));
        let error = run_science_command(
            ScienceCommand::InputNormalize,
            &science_options(&root, &input, &run),
        )
        .expect_err("annotation without sequence must fail");
        assert!(
            error.contains("input_file_requires_reference_sequence"),
            "extension {extension}: {error}"
        );
    }
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn coordinate_file_formats_preserve_content_and_provenance() {
    let root = temp_root("coordinate_content_resolution");
    for extension in ["pdb", "cif", "mmcif", "ent"] {
        let content = if matches!(extension, "pdb" | "ent") {
            "ATOM      1  N   GLY A   1       0.000   0.000   0.000  1.00  0.00           N  \nATOM      2  CA  GLY A   1       1.458   0.000   0.000  1.00  0.00           C  \nTER       3      GLY A   1\nEND\n".to_string()
        } else {
            "data_test\n#\nloop_\n_atom_site.group_PDB\n_atom_site.id\n_atom_site.type_symbol\n_atom_site.label_atom_id\n_atom_site.label_alt_id\n_atom_site.label_comp_id\n_atom_site.label_asym_id\n_atom_site.label_entity_id\n_atom_site.label_seq_id\n_atom_site.pdbx_PDB_ins_code\n_atom_site.Cartn_x\n_atom_site.Cartn_y\n_atom_site.Cartn_z\n_atom_site.occupancy\n_atom_site.B_iso_or_equiv\n_atom_site.pdbx_formal_charge\n_atom_site.auth_seq_id\n_atom_site.auth_comp_id\n_atom_site.auth_asym_id\n_atom_site.auth_atom_id\n_atom_site.pdbx_PDB_model_num\nATOM 1 N N . GLY A 1 1 ? 0.000 0.000 0.000 1.00 0.00 ? 1 GLY A N 1\nATOM 2 C CA . GLY A 1 1 ? 1.458 0.000 0.000 1.00 0.00 ? 1 GLY A CA 1\n#\n".to_string()
        };
        let source = root.join(format!("complex.{extension}"));
        write_text(&source, &content).expect("coordinates");
        let input = root.join(format!("input_{extension}.json"));
        write_text(
            &input,
            &format!(
                r#"{{"inputs":[{{"field":"complex","value":{}}}],"files":[]}}"#,
                serde_json::to_string(&source.display().to_string()).expect("path JSON")
            ),
        )
        .expect("input");
        let run = root.join(format!("run_{extension}"));
        run_science_command(
            ScienceCommand::InputNormalize,
            &science_options(&root, &input, &run),
        )
        .expect("normalize");
        run_science_command(
            ScienceCommand::FeaturesCompute,
            &science_options(&root, &input, &run),
        )
        .expect("features");
        let rows = conversion_rows(&run);
        let features = read_text(&run.join("feature_manifest.tsv")).expect("feature manifest");
        assert_eq!(rows.len(), 1);
        assert_eq!(decode_tsv_cell(&rows[0][5]), content);
        assert_eq!(rows[0][8], extension);
        assert_eq!(rows[0][9], "file");
        assert_eq!(rows[0][10], source.display().to_string());
        assert_eq!(rows[0][11].len(), 64);
        assert_eq!(rows[0][15], "2");
        assert_eq!(rows[0][16], "1");
        assert_eq!(rows[0][17], "1");
        assert!(features.contains("structure\tparsed_structure\ttrue"));
        assert!(features.contains("structure\tatom_count\t2"));
        assert!(features.contains("structure\tchain_count\t1"));
        assert!(features.contains("structure\tresidue_count\t1"));
    }
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn coordinate_file_formats_reject_invalid_content() {
    let root = temp_root("invalid_coordinate_content");
    for extension in ["pdb", "cif", "mmcif", "ent"] {
        let source = root.join(format!("invalid.{extension}"));
        write_text(&source, "not a coordinate structure\n").expect("coordinates");
        let input = root.join(format!("input_{extension}.json"));
        write_text(
            &input,
            &format!(
                r#"{{"inputs":[{{"field":"complex","value":{}}}],"files":[]}}"#,
                serde_json::to_string(&source.display().to_string()).expect("path JSON")
            ),
        )
        .expect("input");
        let run = root.join(format!("run_{extension}"));
        let error = run_science_command(
            ScienceCommand::InputNormalize,
            &science_options(&root, &input, &run),
        )
        .expect_err("invalid coordinate file must fail");
        assert!(
            error.contains("structure_parse_failed"),
            "{extension}: {error}"
        );
    }
    fs::remove_dir_all(root).expect("cleanup");
}

fn ethanol_mol_block() -> &'static str {
    "ethanol\n  GenoLigand\n\n  3  2  0  0  0  0  0  0  0  0999 V2000\n    0.0000    0.0000    0.0000 C   0  0  0  0  0  0  0  0  0  0  0  0\n    1.5000    0.0000    0.0000 C   0  0  0  0  0  0  0  0  0  0  0  0\n    2.8000    0.0000    0.0000 O   0  0  0  0  0  0  0  0  0  0  0  0\n  1  2  1  0  0  0  0\n  2  3  1  0  0  0  0\nM  END\n"
}

fn assert_molecule_file_resolves(extension: &str, content: &str) {
    let root = temp_root(&format!("{extension}_content_resolution"));
    let source = root.join(format!("ethanol.{extension}"));
    write_text(&source, content).expect("molecule file");
    let input = root.join("input.json");
    write_text(
        &input,
        &format!(
            r#"{{"inputs":[{{"field":"small_molecule","value":{}}}],"files":[]}}"#,
            serde_json::to_string(&source.display().to_string()).expect("path JSON")
        ),
    )
    .expect("input");
    let run = root.join("run");
    run_science_command(
        ScienceCommand::InputNormalize,
        &science_options(&root, &input, &run),
    )
    .expect("normalize");
    let rows = conversion_rows(&run);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0][5], "CCO");
    assert_eq!(rows[0][6], "converted");
    assert_eq!(rows[0][8], extension);
    assert_eq!(rows[0][9], "file");
    assert_eq!(rows[0][10], source.display().to_string());
    assert!(!rows[0][5].contains(&source.display().to_string()));
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn sdf_file_is_resolved_to_canonical_smiles() {
    assert_molecule_file_resolves("sdf", &format!("{}$$$$\n", ethanol_mol_block()));
}

#[test]
fn mol_file_is_resolved_to_canonical_smiles() {
    assert_molecule_file_resolves("mol", ethanol_mol_block());
}

#[test]
fn mol2_file_is_resolved_to_canonical_smiles() {
    assert_molecule_file_resolves(
        "mol2",
        "@<TRIPOS>MOLECULE\nethanol\n 3 2 0 0 0\nSMALL\nNO_CHARGES\n\n@<TRIPOS>ATOM\n      1 C1          0.0000    0.0000    0.0000 C.3       1 ETH         0.0000\n      2 C2          1.5000    0.0000    0.0000 C.3       1 ETH         0.0000\n      3 O1          2.8000    0.0000    0.0000 O.3       1 ETH         0.0000\n@<TRIPOS>BOND\n     1    1    2 1\n     2    2    3 1\n",
    );
}

#[test]
fn multi_record_sdf_preserves_every_record() {
    let root = temp_root("multi_record_sdf");
    let sdf = root.join("two_records.sdf");
    write_text(
        &sdf,
        &format!("{}$$$$\n{}$$$$\n", ethanol_mol_block(), ethanol_mol_block()),
    )
    .expect("sdf");
    let input = root.join("input.json");
    write_text(
        &input,
        &format!(
            r#"{{"inputs":[{{"field":"small_molecule","value":{}}}],"files":[]}}"#,
            serde_json::to_string(&sdf.display().to_string()).expect("path JSON")
        ),
    )
    .expect("input");
    let run = root.join("run");
    run_science_command(
        ScienceCommand::InputNormalize,
        &science_options(&root, &input, &run),
    )
    .expect("normalize");
    let rows = conversion_rows(&run);
    fs::remove_dir_all(root).expect("cleanup");
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0][1], "record_1");
    assert_eq!(rows[1][1], "record_2");
    assert!(rows.iter().all(|row| row[5] == "CCO"));
}

#[test]
fn sdf_with_an_invalid_record_fails_the_entire_input() {
    let root = temp_root("invalid_record_sdf");
    let sdf = root.join("partially_invalid.sdf");
    write_text(
        &sdf,
        &format!("invalid molecule\n$$$$\n{}$$$$\n", ethanol_mol_block()),
    )
    .expect("sdf");
    let input = root.join("input.json");
    write_text(
        &input,
        &format!(
            r#"{{"inputs":[{{"field":"small_molecule","value":{}}}],"files":[]}}"#,
            serde_json::to_string(&sdf.display().to_string()).expect("path JSON")
        ),
    )
    .expect("input");
    let run = root.join("run");
    let error = run_science_command(
        ScienceCommand::InputNormalize,
        &science_options(&root, &input, &run),
    )
    .expect_err("partially invalid SDF must fail");
    fs::remove_dir_all(root).expect("cleanup");
    assert!(
        error.contains("rdkit_structure_parse_failed:record_1"),
        "{error}"
    );
}

#[test]
fn normalized_manifest_requires_complete_provenance_schema() {
    let root = temp_root("strict_normalized_schema");
    let run = root.join("run");
    write_text(
        &run.join("input_normalized_manifest.tsv"),
        "input_id\tinput_kind\tprimary_value\tfile_count\tfeature_tokens\tstatus\ninput_1\tpeptide\tMKTAYI\t0\tmkt\tnormalized\n",
    )
    .expect("legacy normalized manifest");
    let input = root.join("input.json");
    write_text(&input, "{\"inputs\":[],\"files\":[]}\n").expect("input");
    let error = run_science_command(
        ScienceCommand::FeaturesCompute,
        &science_options(&root, &input, &run),
    )
    .expect_err("legacy normalized schema must fail");
    fs::remove_dir_all(root).expect("cleanup");
    assert!(error.contains("normalized_input_manifest_schema_mismatch"));
}

#[test]
fn normalized_manifest_rejects_incomplete_or_mismatched_provenance() {
    let root = temp_root("normalized_provenance_validation");
    let input = root.join("input.json");
    let run = root.join("run");
    write_text(
        &input,
        r#"{"inputs":[{"field":"small_molecule","value":"CCO"}],"files":[]}"#,
    )
    .expect("input");
    let options = science_options(&root, &input, &run);
    run_science_command(ScienceCommand::InputNormalize, &options).expect("normalize");

    let path = run.join("input_normalized_manifest.tsv");
    let text = read_text(&path).expect("normalized manifest");
    let mut lines = text.lines();
    let header = lines.next().expect("header");
    let mut cols = split_tsv(lines.next().expect("row"));
    cols[6] = "file".to_string();
    cols[7].clear();
    write_text(&path, &format!("{header}\n{}\n", cols.join("\t"))).expect("mutate source");
    let error = run_science_command(ScienceCommand::FeaturesCompute, &options)
        .expect_err("file provenance without source path must fail");
    assert!(
        error.contains("normalized_input_manifest_missing_source_path"),
        "{error}"
    );

    run_science_command(ScienceCommand::InputNormalize, &options).expect("renormalize");
    let text = read_text(&path).expect("normalized manifest");
    let mut lines = text.lines();
    let header = lines.next().expect("header");
    let mut cols = split_tsv(lines.next().expect("row"));
    cols[8] = "a".repeat(64);
    write_text(&path, &format!("{header}\n{}\n", cols.join("\t"))).expect("mutate hash");
    let error = run_science_command(ScienceCommand::FeaturesCompute, &options)
        .expect_err("normalized provenance must match conversion manifest");
    fs::remove_dir_all(root).expect("cleanup");
    assert!(
        error.contains("normalized_input_manifest_provenance_mismatch"),
        "{error}"
    );
}

#[test]
fn normalized_manifest_rejects_duplicate_identity_before_feature_generation() {
    let root = temp_root("normalized_duplicate_identity");
    let input = root.join("input.json");
    let run = root.join("run");
    write_text(
        &input,
        r#"{"inputs":[{"field":"small_molecule","value":"CCO"}],"files":[]}"#,
    )
    .expect("input");
    let options = science_options(&root, &input, &run);
    run_science_command(ScienceCommand::InputNormalize, &options).expect("normalize");

    let path = run.join("input_normalized_manifest.tsv");
    let text = read_text(&path).expect("normalized manifest");
    let mut lines = text.lines();
    let header = lines.next().expect("header");
    let row = lines.next().expect("row");
    write_text(&path, &format!("{header}\n{row}\n{row}\n")).expect("duplicate row");
    let error = run_science_command(ScienceCommand::FeaturesCompute, &options)
        .expect_err("duplicate normalized identity must fail before features");
    fs::remove_dir_all(root).expect("cleanup");
    assert!(
        error.contains("normalized_input_manifest_duplicate_identity"),
        "{error}"
    );
}

#[test]
fn feature_manifest_rejects_empty_identity_and_kind_conflicts() {
    let root = temp_root("feature_identity_validation");
    let input = root.join("input.json");
    let run = root.join("run");
    write_text(
        &input,
        r#"{"inputs":[{"field":"small_molecule","value":"CCO"}],"files":[]}"#,
    )
    .expect("input");
    let options = science_options(&root, &input, &run);
    run_science_command(ScienceCommand::InputNormalize, &options).expect("normalize");
    run_science_command(ScienceCommand::FeaturesCompute, &options).expect("features");

    let path = run.join("feature_manifest.tsv");
    let text = read_text(&path).expect("feature manifest");
    let mut lines = text.lines();
    let header = lines.next().expect("header");
    let original_rows = lines.map(ToOwned::to_owned).collect::<Vec<_>>();
    let mut empty_source = split_tsv(&original_rows[0]);
    empty_source[5].clear();
    write_text(&path, &format!("{header}\n{}\n", empty_source.join("\t")))
        .expect("empty feature source");
    let error = run_science_command(ScienceCommand::DatabaseMatchKnown, &options)
        .expect_err("empty feature source must fail");
    assert!(
        error.contains("feature_manifest_missing_feature_source"),
        "{error}"
    );

    let mut empty_identity = split_tsv(&original_rows[0]);
    empty_identity[0].clear();
    write_text(&path, &format!("{header}\n{}\n", empty_identity.join("\t")))
        .expect("empty identity");
    let error = run_science_command(ScienceCommand::DatabaseMatchKnown, &options)
        .expect_err("empty feature identity must fail");
    assert!(
        error.contains("feature_manifest_missing_input_id"),
        "{error}"
    );

    let mut conflict = split_tsv(&original_rows[0]);
    conflict[1] = "peptide".to_string();
    write_text(
        &path,
        &format!("{header}\n{}\n{}\n", original_rows[0], conflict.join("\t")),
    )
    .expect("kind conflict");
    let error = run_science_command(ScienceCommand::DatabaseMatchKnown, &options)
        .expect_err("feature kind conflict must fail");
    fs::remove_dir_all(root).expect("cleanup");
    assert!(error.contains("feature_manifest_kind_conflict"), "{error}");
}

#[test]
fn input_conversion_prefers_declared_field_over_sequence_like_content() {
    let root = temp_root("free_text_conversion");
    let input = root.join("input.json");
    let run = root.join("run");
    write_text(
        &input,
        r#"{"inputs":[{"field":"small_molecule","value":"CCCCCCCC"}],"files":[]}"#,
    )
    .expect("write input");
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: root.join("science_store"),
        input_manifest: input,
        run_dir: run.clone(),
    };

    run_science_command(ScienceCommand::InputNormalize, &options).expect("normalize");
    let normalized =
        read_text(&run.join("input_normalized_manifest.tsv")).expect("normalized manifest");
    let conversion =
        read_text(&run.join("input_conversion_manifest.tsv")).expect("conversion manifest");
    let source_map = read_text(&run.join("artifact_source_map.tsv")).expect("source map");

    assert!(normalized.contains("input_1\tsmall_molecule\tCCCCCCCC"));
    assert!(conversion.contains("input_1\trecord_1\tsmall_molecule\tsmall_molecule\tsmall_molecule\tCCCCCCCC\tconverted\trdkit_smiles\tplain_text"));
    assert!(source_map.contains("input_conversion_manifest.tsv"));
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn input_conversion_rejects_malformed_manifest_json() {
    let root = temp_root("malformed_input_manifest");
    let input = root.join("input.json");
    write_text(&input, "{not-json").expect("input");
    let error = extract_inputs(&science_options(&root, &input, &root.join("run")))
        .expect_err("malformed JSON must fail");
    assert!(error.starts_with("invalid_input_manifest_json:"));
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn input_conversion_classifies_sequence_and_structure_text() {
    let root = temp_root("conversion_classification");
    let input = root.join("input.json");
    let run = root.join("run");
    write_text(
            &input,
            r#"{"inputs":[
{"field":"peptide","value":">pep\nIKVAV"},
{"field":"small_molecule","value":"CC(=O)OC1=CC=CC=C1C(=O)O"},
{"field":"protein_ligand","value":"MKTAYIAKQRQISFVKSHFSRQDILDLWQMKTAYIAKQRQISFVKSHFSRQDILDLWQMKTAYIAKQRQISFVKSHFSRQDILDLWQ"}
],"files":[]}"#,
        )
        .expect("write input");
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: root.join("science_store"),
        input_manifest: input,
        run_dir: run.clone(),
    };

    run_science_command(ScienceCommand::InputNormalize, &options).expect("normalize");
    let normalized =
        read_text(&run.join("input_normalized_manifest.tsv")).expect("normalized manifest");
    let conversion =
        read_text(&run.join("input_conversion_manifest.tsv")).expect("conversion manifest");

    assert!(normalized.contains("input_1\tpeptide\tIKVAV"));
    assert!(normalized.contains("input_2\tsmall_molecule\tCC(=O)Oc1ccccc1C(=O)O"));
    assert!(normalized.contains("input_3\tprotein_ligand\tMKTAYIAKQRQISFVKSHFSRQDILDLWQMKTAYIAKQRQISFVKSHFSRQDILDLWQMKTAYIAKQRQISFVKSHFSRQDILDLWQ"));
    assert!(conversion.contains(
        "input_1\trecord_1\tpeptide\tpeptide\tpeptide\tIKVAV\tconverted\tstandard_amino_acid_sequence\tfasta"
    ));
    assert!(conversion.contains("input_2\trecord_1\tsmall_molecule\tsmall_molecule\tsmall_molecule\tCC(=O)Oc1ccccc1C(=O)O\tconverted\trdkit_smiles\tplain_text"));
    assert!(conversion.contains("input_3\trecord_1\tprotein_ligand\tprotein_ligand\tprotein_ligand\tMKTAYIAKQRQISFVKSHFSRQDILDLWQMKTAYIAKQRQISFVKSHFSRQDILDLWQMKTAYIAKQRQISFVKSHFSRQDILDLWQ\tconverted\tstandard_amino_acid_sequence\tplain_text"));
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn input_conversion_does_not_reclassify_declared_small_molecule_as_nucleotide() {
    let root = temp_root("input_conversion_nucleotide");
    let input = root.join("input.json");
    let run = root.join("run");
    write_text(
        &input,
        r#"{"inputs":[{"field":"small_molecule","value":"ATGCGTACGTAA"}],"files":[]}"#,
    )
    .expect("write input");
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: root.join("science_store"),
        input_manifest: input,
        run_dir: run.clone(),
    };

    let error = run_science_command(ScienceCommand::InputNormalize, &options)
        .expect_err("invalid declared small molecule must fail");
    let conversion =
        read_text(&run.join("input_conversion_manifest.tsv")).expect("conversion manifest");

    assert!(error.contains("unknown_compound_identifier"), "{error}");
    assert!(conversion.contains("input_1\trecord_1\tsmall_molecule\tunknown\tunknown\t\tfailed"));
    assert!(!conversion.contains("genome_or_metagenome"));
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn input_conversion_uses_field_hint_for_ambiguous_short_smiles() {
    let root = temp_root("input_conversion_ambiguous_smiles");
    let input = root.join("input.json");
    let run = root.join("run");
    write_text(
        &input,
        r#"{"inputs":[{"field":"small_molecule","value":"CCN"}],"files":[]}"#,
    )
    .expect("write input");
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: root.join("science_store"),
        input_manifest: input,
        run_dir: run.clone(),
    };

    run_science_command(ScienceCommand::InputNormalize, &options).expect("normalize");
    let normalized =
        read_text(&run.join("input_normalized_manifest.tsv")).expect("normalized manifest");
    let conversion =
        read_text(&run.join("input_conversion_manifest.tsv")).expect("conversion manifest");

    assert!(normalized.contains("input_1\tsmall_molecule\tCCN"));
    assert!(conversion.contains(
            "input_1\trecord_1\tsmall_molecule\tsmall_molecule\tsmall_molecule\tCCN\tconverted\trdkit_smiles\tplain_text"
        ));
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn database_matching_requires_computed_feature_tokens() {
    let root = temp_root("feature_bound_matching");
    let store = root.join("science_store");
    let run = root.join("run");
    let input = root.join("input.json");
    write_test_source_manifest(&store, &["chembl"]).expect("source manifest");
    write_text(
            &store.join("indexes/known_targets.tsv"),
            "source\ttarget_id\ttarget_name\torganism\ttarget_class\tligand_tokens\tsequence_tokens\tstructure_tokens\tevidence\nChEMBL\tCHEMBL_T1\tEthanol binding protein\tHomo sapiens\tknown_human_protein\tcco\t\t\tactivity_row\n",
        )
        .expect("known index");
    write_text(
        &input,
        "{\"inputs\":[{\"field\":\"small_molecule\",\"value\":\"CCO\"}],\"files\":[]}\n",
    )
    .expect("input");
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: store,
        input_manifest: input,
        run_dir: run.clone(),
    };

    run_science_command(ScienceCommand::InputNormalize, &options).expect("normalize");
    run_science_command(ScienceCommand::FeaturesCompute, &options).expect("features");
    run_science_command(ScienceCommand::DatabaseMatchKnown, &options).expect("match known");
    let with_features = read_text(&run.join("known_database_matches.tsv")).expect("known matches");
    assert!(with_features.contains("CHEMBL_T1"));

    write_text(
        &run.join("feature_manifest.tsv"),
        "input_id\tinput_kind\tfeature_family\tfeature_name\tfeature_value\tfeature_source\tfeature_token\trecord_id\ninput_1\tsmall_molecule\tinput\tinput_length\t3\tGenoLigand\t\trecord_1\n",
        )
        .expect("strip feature tokens");
    run_science_command(ScienceCommand::DatabaseMatchKnown, &options)
        .expect("match known without feature tokens");
    let without_features =
        read_text(&run.join("known_database_matches.tsv")).expect("known matches");
    assert!(!without_features.contains("CHEMBL_T1"));

    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn metagenome_matching_uses_mmseqs_output_when_persistent_database_is_configured() {
    let root = temp_root("metagenome_requires_mmseqs_output");
    let store = root.join("science_store");
    let run = root.join("run");
    let input = root.join("input.json");
    write_text(
        &store.join("indexes/metagenome_targets.tsv"),
        "source\ttarget_id\ttarget_name\torganism\ttarget_class\tligand_tokens\tsequence_tokens\tstructure_tokens\tevidence\nmetagenome\tMG_FAKE\tTSV fallback row\tunknown\tpotential_metagenome_sequence\t\tikvav\t\tfasta_sequence_rows\n",
    )
    .expect("metagenome index");
    write_alignment_database_fixture(&store, "mmseqs", "metagenome", "MGnify", "metagenome")
        .expect("MMseqs v2 manifest");
    write_text(
        &input,
        "{\"inputs\":[{\"field\":\"peptide\",\"value\":\"IKVAV\"}],\"files\":[]}\n",
    )
    .expect("input");
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: store,
        input_manifest: input,
        run_dir: run.clone(),
    };

    run_science_command(ScienceCommand::InputNormalize, &options).expect("normalize");
    run_science_command(ScienceCommand::FeaturesCompute, &options).expect("features");
    run_science_command(ScienceCommand::MetagenomeMatchPotential, &options).expect("metagenome");
    let matches =
        read_text(&run.join("metagenome_potential_matches.tsv")).expect("metagenome matches");

    assert!(!matches.contains("MG_FAKE"));
    assert_eq!(matches.lines().count(), 1);
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn metagenome_matching_preserves_mmseqs_source_database_and_query_id() {
    let root = temp_root("metagenome_mmseqs_identity");
    let store = root.join("science_store");
    let run = root.join("run");
    let input = root.join("input.json");
    write_test_source_manifest(&store, &["metagenome_potential"]).expect("source manifest");
    ensure_dir(&run.join("tool_outputs")).expect("tool outputs");
    write_alignment_database_fixture(&store, "mmseqs", "metagenome_mgy", "MGnify", "metagenome")
        .expect("MMseqs v2 manifest");
    write_text(
        &run.join("tool_outputs/mmseqs_alignment_matches.tsv"),
        "input_id\tsource\ttarget_id\ttarget_name\torganism\ttarget_class\tmatch_score\tmatched_features\tevidence\tsource_database_id\tsource_namespace\tsource_class\tsource_digest\tbuilder_version\tmmseqs_version\tcurrent_run_query_id\trecord_id\tquery_kind\talignment_length\tquery_coverage\ttarget_coverage\talignment_cigar\ninput_1\tMGnify\tMG_REAL\tMGnify hit\tunknown\tpotential_metagenome_sequence\t0.9100\tikvav\tmmseqs_alignment\tmetagenome_mgy\tMGnify\tmetagenome\taaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\tmmseqs-builder-test\tMMseqs2-test\tquery_1\trecord_1\tprotein_ligand\t5\t1.0\t1.0\t5M\n",
    )
    .expect("mmseqs output");
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: store,
        input_manifest: input,
        run_dir: run.clone(),
    };

    normalize_and_compute_fixture(&options, "peptide", "IKVAV").expect("upstream manifests");
    run_science_command(ScienceCommand::MetagenomeMatchPotential, &options).expect("metagenome");
    let matches =
        read_text(&run.join("metagenome_potential_matches.tsv")).expect("metagenome matches");

    assert!(
        matches
            .lines()
            .next()
            .unwrap_or_default()
            .contains("source_database_id")
    );
    assert!(
        matches
            .lines()
            .next()
            .unwrap_or_default()
            .contains("current_run_query_id")
    );
    assert!(matches.contains("MG_REAL"));
    assert!(matches.contains("\tmetagenome_mgy\tquery_1\trecord_1"));
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn mmseqs_rows_route_known_and_metagenome_by_explicit_source_class() {
    let root = temp_root("mmseqs_source_class_routing");
    let run = root.join("run");
    let store = root.join("science_store");
    write_test_source_manifest_with_digests(
        &store,
        &[
            ("uniprot_human_reviewed", "a".repeat(64)),
            ("metagenome_potential", "b".repeat(64)),
        ],
    )
    .expect("source manifest");
    ensure_dir(&run.join("tool_outputs")).expect("tool outputs");
    write_text(
        &run.join("tool_outputs/mmseqs_alignment_matches.tsv"),
        "input_id\tsource\ttarget_id\ttarget_name\torganism\ttarget_class\tmatch_score\tmatched_features\tevidence\tsource_database_id\tsource_namespace\tsource_class\tsource_digest\tbuilder_version\tmmseqs_version\tcurrent_run_query_id\trecord_id\tquery_kind\talignment_length\tquery_coverage\ttarget_coverage\talignment_cigar\ninput_1\tUniProt\tP12345\tKnown hit\tunknown\tknown_protein\t0.9500\tMKTAYI\tmmseqs2_search\tuniprot_human\tUniProt\tknown\taaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\tmmseqs-builder-test\tMMseqs2-test\tquery_known\trecord_1\tprotein_ligand\t6\t1.0\t1.0\t6M\ninput_1\tMGnify\tMG_REAL\tMetagenome hit\tunknown\tpotential_metagenome_sequence\t0.9100\tMKTAYI\tmmseqs2_search\tmetagenome_mgy\tMGnify\tmetagenome\tbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\tmmseqs-builder-test\tMMseqs2-test\tquery_mgy\trecord_1\tprotein_ligand\t6\t1.0\t1.0\t6M\n",
    )
    .expect("MMseqs output");
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: store,
        input_manifest: root.join("input.json"),
        run_dir: run,
    };
    let mut known = String::new();
    let mut metagenome = String::new();

    let known_count =
        append_mmseqs_match_rows(&options, "known", &mut known, true).expect("route known rows");
    let metagenome_count = append_mmseqs_match_rows(&options, "metagenome", &mut metagenome, false)
        .expect("route metagenome rows");

    assert_eq!(known_count, 1);
    assert!(known.contains("P12345"));
    assert!(known.contains("source_digest=aaaaaaaa"));
    assert!(!known.contains("MG_REAL"));
    assert_eq!(metagenome_count, 1);
    assert!(metagenome.contains("MG_REAL"));
    assert!(!metagenome.contains("P12345"));
    fs::remove_dir_all(root).expect("cleanup temp root");
}

#[test]
fn mmseqs_rows_reject_unknown_source_class() {
    let root = temp_root("mmseqs_unknown_source_class");
    let run = root.join("run");
    ensure_dir(&run.join("tool_outputs")).expect("tool outputs");
    write_text(
        &run.join("tool_outputs/mmseqs_alignment_matches.tsv"),
        "input_id\tsource\ttarget_id\ttarget_name\torganism\ttarget_class\tmatch_score\tmatched_features\tevidence\tsource_database_id\tsource_namespace\tsource_class\tsource_digest\tbuilder_version\tmmseqs_version\tcurrent_run_query_id\trecord_id\ninput_1\tUnknown\tX1\tUnknown hit\tunknown\tunknown\t0.5\tMKT\tmmseqs2_search\tdb_unknown\tUnknown\tambiguous\taaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\tmmseqs-builder-test\tMMseqs2-test\tquery_1\trecord_1\n",
    )
    .expect("MMseqs output");
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: root.join("science_store"),
        input_manifest: root.join("input.json"),
        run_dir: run,
    };
    let error = append_mmseqs_match_rows(&options, "known", &mut String::new(), true)
        .expect_err("unknown source class must fail");

    assert!(error.contains("mmseqs_source_class_invalid"));
    fs::remove_dir_all(root).expect("cleanup temp root");
}

#[test]
fn foldseek_tool_rows_preserve_source_database_and_query_id() {
    let root = temp_root("foldseek_identity");
    let store = root.join("science_store");
    let run = root.join("run");
    write_test_source_manifest(&store, &["rcsb_ligand_bound"]).expect("source manifest");
    ensure_dir(&run.join("tool_outputs")).expect("tool outputs");
    write_text(
        &run.join("input_normalized_manifest.tsv"),
        &format!(
            "{NORMALIZED_INPUT_HEADER}\ninput_1\tcomplex\tdata_1abc\t0\t\trecord_1\tinline\t\t{}\tnormalized\t1\t1\t1\n",
            "c".repeat(64)
        ),
    )
    .expect("normalized input");
    write_text(
        &run.join("tool_outputs/foldseek_structure_matches.tsv"),
        &format!(
            "input_id\tsource\ttarget_id\ttarget_name\torganism\ttarget_class\tmatch_score\tmatched_features\tevidence\trecord_id\tsource_database_id\tsource_digest\tbuilder_version\tfoldseek_version\tcurrent_run_query_id\tquery_digest\ttm_score\ninput_1\tFoldseek\t1ABC\tRCSB structure\tHomo sapiens\tknown_structure\t0.8240\tpocket\tfoldseek_alignment;pident=80;bits=100\trecord_1\trcsb_mmcif\t{}\tfoldseek-builder-test\tFoldseek-test\tinput_1\t{}\t0.9000\n",
            "a".repeat(64),
            "b".repeat(64)
        ),
    )
    .expect("foldseek output");
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: store,
        input_manifest: root.join("input.json"),
        run_dir: run.clone(),
    };
    let mut out = "input_id\tsource\ttarget_id\ttarget_name\torganism\ttarget_class\tmatch_score\tmatched_features\tevidence\tsource_database_id\tcurrent_run_query_id\trecord_id\n".to_string();

    append_tool_match_rows(&options, "foldseek_structure_matches.tsv", &mut out)
        .expect("append foldseek");

    assert!(out.contains("1ABC"));
    assert!(out.contains("\trcsb_mmcif\tinput_1"));
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn known_matching_uses_bindingdb_query_index_after_source_archive_removal() {
    let root = temp_root("bindingdb_onsite_search");
    let store = root.join("science_store");
    let run = root.join("run");
    let input = root.join("input.json");
    write_text(
        &store.join("indexes/known_targets.tsv"),
        "source\ttarget_id\ttarget_name\torganism\ttarget_class\tligand_tokens\tsequence_tokens\tstructure_tokens\tevidence\n",
    )
    .expect("empty known index");
    write_zip(
        &store.join("downloads/bindingdb/bindingdb_all.tsv.zip"),
        "BindingDB_All.tsv",
        "Ligand SMILES\tBindingDB Ligand Name\tTarget Name\tTarget Source Organism According to Curator or DataSource\tKi (nM)\tPDB ID(s) for Ligand-Target Complex\tLigand HET ID in PDB\tUniProt (SwissProt) Primary ID of Target Chain 1\tBindingDB MonomerID\nCC(=O)Nc1nnc(s1)S(=O)(=O)N\tAcetazolamide, AZM\tCarbonic anhydrase 2\tHomo sapiens\t12\t3HS4,1AZM,2H4N\tAZM\tP00918\t10880\n",
    )
    .expect("bindingdb zip");
    write_test_source_manifest(&store, &["bindingdb"]).expect("bindingdb source manifest");
    publish_test_store_bindingdb_query(&store).expect("BindingDB query index");
    fs::remove_file(store.join(BINDINGDB_SOURCE_REL_PATH))
        .expect("remove BindingDB source archive");
    write_text(
        &input,
        "{\"inputs\":[{\"field\":\"small_molecule\",\"value\":\"CC(=O)Nc1nnc(s1)S(=O)(=O)N\"}],\"files\":[]}\n",
    )
    .expect("input");
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: store,
        input_manifest: input,
        run_dir: run.clone(),
    };

    run_science_command(ScienceCommand::InputNormalize, &options).expect("normalize");
    run_science_command(ScienceCommand::FeaturesCompute, &options).expect("features");
    run_science_command(ScienceCommand::DatabaseMatchKnown, &options).expect("match known");
    let matches = read_text(&run.join("known_database_matches.tsv")).expect("known matches");
    let header = matches.lines().next().unwrap_or_default();

    assert!(header.contains("source_row_provenance"));
    assert!(header.contains("activity_value"));
    assert!(header.contains("ligand_identity"));
    assert!(header.contains("current_run_query_id"));
    assert!(matches.contains("BindingDB"));
    assert!(matches.contains("P00918"));
    assert!(matches.contains("Carbonic anhydrase 2"));
    assert!(matches.contains("bindingdb_onsite_activity_row"));
    assert!(matches.contains("Ki (nM)=12"));
    assert!(matches.contains("ligand_smiles=CC(=O)Nc1nnc(s1)S(=O)(=O)N"));
    assert!(matches.contains("ligand_code=AZM"));
    assert!(matches.contains("pdb_ids=3HS4,1AZM,2H4N"));
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn prepare_science_store_migrates_legacy_ligand_queries_to_compact_authority() {
    let root = temp_root("ligand_similarity_migration");
    let store = root.join("science_store");
    install_ligand_similarity_builder_fixture(&root, false).expect("similarity builder");
    write_ligand_similarity_staging_fixture(&store).expect("legacy ligand staging");
    let options = StoreOptions {
        workspace_root: root.clone(),
        science_store: store.clone(),
    };
    let chembl_database = store.join("downloads/chembl/chembl_query.sqlite");
    let bindingdb_database = store.join(BINDINGDB_QUERY_DATABASE_REL_PATH);
    let stale_paths = [
        PathBuf::from(format!("{}-wal", chembl_database.display())),
        PathBuf::from(format!("{}-shm", bindingdb_database.display())),
        store.join("downloads/chembl/.chembl_query_stale.sqlite"),
        store.join("indexes/bindingdb/.bindingdb_query_stale.sqlite"),
        store.join("downloads/chembl/.chembl_query.sha256_cache/stale.json"),
    ];
    for path in &stale_paths {
        write_text(path, "stale\n").expect("stale legacy artifact");
    }

    run_store_command(ScienceCommand::PrepareScienceStore, &options).expect("prepare store");

    validate_ligand_similarity_runtime_contract(&options).expect("compact authority");
    assert!(!superseded_ligand_query_artifacts_present(&store).expect("legacy scan"));
    for path in superseded_ligand_query_fixed_paths(&store)
        .into_iter()
        .chain(stale_paths)
    {
        assert!(
            !path.exists(),
            "legacy artifact retained: {}",
            path.display()
        );
    }
    let build_manifest =
        read_text(&store.join("index_build_manifest.tsv")).expect("build manifest");
    assert!(build_manifest.contains("compact_ligand_similarity_used_by_onsite_matcher"));
    assert!(build_manifest.contains("compact_ligand_similarity_is_authoritative"));
    assert!(!build_manifest.contains("published_chembl_query_used_by_onsite_matcher"));
    let known_targets = read_text(&store.join("indexes/known_targets.tsv")).expect("known targets");
    assert!(!known_targets.contains("P_ONE"));
    let status: Value = serde_json::from_str(
        &read_text(&store.join("install_status.json")).expect("install status"),
    )
    .expect("install status JSON");
    assert_eq!(status["required_items"], 5);
    assert_eq!(status["ligand_similarity_detail"], "ready");

    let compact = store.join(LIGAND_SIMILARITY_DATABASE_REL_PATH);
    let initial_digest = sha256_file_hex(&compact).expect("compact digest");
    install_ligand_similarity_builder_fixture(&root, true).expect("failing builder sentinel");
    prepare_ligand_similarity_authority(&options).expect("idempotent compact reuse");
    assert_eq!(
        sha256_file_hex(&compact).expect("reused compact digest"),
        initial_digest
    );
    assert!(!store.join(BINDINGDB_QUERY_DATABASE_REL_PATH).exists());
    fs::remove_dir_all(&root).expect("cleanup temp root");
}

#[test]
fn prepare_reuses_pruned_published_store_without_rewriting_readiness_evidence() {
    let root = temp_root("reuse_pruned_published_store");
    let store = root.join("science_store");
    install_ligand_similarity_builder_fixture(&root, false).expect("similarity builder");
    write_ligand_similarity_staging_fixture(&store).expect("legacy ligand staging");
    let options = StoreOptions {
        workspace_root: root.clone(),
        science_store: store.clone(),
    };
    run_store_command(ScienceCommand::PrepareScienceStore, &options).expect("initial prepare");

    write_text(
        &store.join("indexes/known_targets.tsv"),
        &store_row_header(),
    )
    .expect("known published index");
    write_text(
        &store.join("indexes/metagenome_targets.tsv"),
        &store_row_header(),
    )
    .expect("metagenome compatibility header");
    write_text(
        &store.join("indexes/structure_targets.tsv"),
        "source\ttarget_id\ttarget_name\torganism\ttarget_class\tligand_tokens\tsequence_tokens\tstructure_tokens\tevidence\nBioLiP2\tBIO_SENTINEL\tPublished interaction\tHomo sapiens\tstructure_interaction_record\tlig\tprotein\tpocket\tpublished_biolip_projection\nPLINDER\tPLI_SENTINEL\tPublished complex\tHomo sapiens\tstructure_interaction_record\tlig\tprotein\tpocket\tpublished_plinder_projection\nRCSB\t1ABC\tPublished structure\tunknown\tstructure_interaction_record\tlig\tprotein\t1abc\tpublished_rcsb_projection\n",
    )
    .expect("structure published index");
    write_text(
        &store.join("index_build_manifest.tsv"),
        &format!(
            "index\tinput_path\tstatus\trows\toutput_path\tnotes\nknown_targets\tindexes/known_targets.tsv\tused\t0\tindexes/known_targets.tsv\tcompact_and_alignment_authority\nligand_similarity\t{}\tready\t1\t{}\tcompact_ligand_similarity_used_by_onsite_matcher\nmetagenome_targets\tdownloads/metagenome/mgnify/mgy_clusters.fa.gz\talignment_database_used\t0\tindexes/mmseqs/mgnify/target\tfull_searchable_alignment_database\nstructure_targets\tdownloads/biolip2/BioLiP.txt.gz\tused\t1\tindexes/structure_targets.tsv\tbiolip2_interaction_rows;complete_source_scan\nstructure_targets\tindexes/plinder_targets.tsv\tused\t1\tindexes/structure_targets.tsv\tplinder_canonical_rows;complete_source_scan\nstructure_targets\tdownloads/rcsb/mmcif/1abc.cif\tused\t1\tindexes/structure_targets.tsv\trcsb_structure_file_rows;complete_source_scan\n",
            LIGAND_SIMILARITY_MANIFEST_REL_PATH, LIGAND_SIMILARITY_DATABASE_REL_PATH,
        ),
    )
    .expect("published build manifest");
    write_text(
        &store.join("manifests/source_retention_manifest.tsv"),
        &format!(
            "schema_version\tsource_id\tstate\tderived_paths\n{0}\tuniprot_human_reviewed\tpruned\tindexes/mmseqs/mmseqs_target_databases.tsv\n{0}\tbiolip2\tpruned\tindexes/mmseqs/mmseqs_target_databases.tsv;indexes/structure_targets.tsv;indexes/structure_targets.manifest.json\n{0}\tmetagenome_potential\tpruned\tindexes/mmseqs/mmseqs_target_databases.tsv\n{0}\tplinder\tpruned\tindexes/structure_targets.tsv;indexes/structure_targets.manifest.json\n",
            SOURCE_RETENTION_SCHEMA_VERSION
        ),
    )
    .expect("source retention");
    write_text(
        &store.join("downloads/rcsb/mmcif/1abc.cif"),
        "data_1ABC\n_entry.id 1ABC\n_atom_site.group_PDB ATOM\n",
    )
    .expect("partial on-demand RCSB cache");
    write_text(
        &store.join("downloads/biolip2/BioLiP.txt.gz"),
        "published BioLiP source\n",
    )
    .expect("published BioLiP input");
    write_text(
        &store.join("indexes/plinder_targets.tsv"),
        "published PLINDER canonical input\n",
    )
    .expect("published PLINDER input");
    write_test_pruned_alignment_identity_fixture(&store)
        .expect("published and alignment source identities");
    let alignment_sources = [
        ("uniprot", "UniProt", "known"),
        ("biolip2_protein", "BioLiP2", "known"),
        ("biolip2_protein_nr", "BioLiP2", "known"),
        ("biolip2_peptide", "BioLiP2", "known"),
        ("biolip2_peptide_nr", "BioLiP2", "known"),
        ("mgnify", "MGnify", "metagenome"),
    ];
    for (source_id, namespace, class) in alignment_sources {
        write_alignment_database_fixture(&store, "mmseqs", source_id, namespace, class)
            .expect("alignment authority");
    }
    let mut alignment_manifest = String::from(
        "source_id\tmanifest_schema_version\tsource_namespace\tsource_class\tsource_path\tsource_digest\tsource_files\tsource_bytes\tbuilder_version\tdb_path\tstatus\n",
    );
    for (source_id, namespace, class) in alignment_sources {
        let source_path = match source_id {
            "uniprot" => "downloads/uniprot/uniprot_human_reviewed.fasta.gz",
            "biolip2_protein" => "downloads/biolip2/protein.fasta.gz",
            "biolip2_protein_nr" => "downloads/biolip2/protein_nr.fasta.gz",
            "biolip2_peptide" => "downloads/biolip2/peptide.fasta.gz",
            "biolip2_peptide_nr" => "downloads/biolip2/peptide_nr.fasta.gz",
            "mgnify" => "downloads/metagenome/mgnify/mgy_clusters.fa.gz",
            _ => unreachable!("unknown alignment fixture"),
        };
        alignment_manifest.push_str(&format!(
            "{source_id}\t{}\t{namespace}\t{class}\t{source_path}\t{}\t1\t1\tmmseqs-test\tindexes/mmseqs/{source_id}/target\tready\n",
            ALIGNMENT_DATABASE_MANIFEST_SCHEMA_VERSION,
            test_source_digest(source_id),
        ));
    }
    write_text(
        &store.join("indexes/mmseqs/mmseqs_target_databases.tsv"),
        &alignment_manifest,
    )
    .expect("combined MMseqs authority manifest");
    write_structure_targets_manifest(&store, &store.join("index_build_manifest.tsv"))
        .expect("structure publication");
    write_test_science_store_readiness(&store, &[]).expect("canonical readiness");

    let protected = [
        "indexes/known_targets.tsv",
        "indexes/metagenome_targets.tsv",
        "indexes/structure_targets.tsv",
        STRUCTURE_TARGETS_MANIFEST_REL_PATH,
        "index_build_manifest.tsv",
        "install_status.json",
        "science_store_readiness.json",
    ];
    let before = protected
        .iter()
        .map(|relative| {
            (
                *relative,
                fs::read(store.join(relative)).expect("read protected publication"),
            )
        })
        .collect::<BTreeMap<_, _>>();

    let result = run_store_command(ScienceCommand::PrepareScienceStore, &options)
        .expect("reuse published store");
    assert!(result.contains("reused_published_indexes=true"));
    for relative in protected {
        assert_eq!(
            fs::read(store.join(relative)).expect("read reused publication"),
            before[relative],
            "prepare rewrote protected publication {relative}"
        );
    }
    assert!(
        evaluate_science_store_readiness(&store)
            .expect("readiness after reuse")
            .ready
    );

    let database = store.join(LIGAND_SIMILARITY_DATABASE_REL_PATH);
    let mut bytes = fs::read(&database).expect("compact database bytes");
    bytes[0] ^= 1;
    fs::write(&database, bytes).expect("same-size compact tamper");
    assert_eq!(reusable_published_science_store(&options), None);
    fs::remove_dir_all(&root).expect("cleanup temp root");
}

#[test]
fn retained_mmseqs_authority_binds_sources_payloads_and_complete_databases() {
    let root = temp_root("retained_mmseqs_authority_identity");
    let store = root.join("science_store");
    write_test_pruned_alignment_identity_fixture(&store).expect("source retention identities");
    let alignment_sources = [
        ("uniprot", "UniProt", "known"),
        ("biolip2_protein", "BioLiP2", "known"),
        ("biolip2_protein_nr", "BioLiP2", "known"),
        ("biolip2_peptide", "BioLiP2", "known"),
        ("biolip2_peptide_nr", "BioLiP2", "known"),
        ("mgnify", "MGnify", "metagenome"),
    ];
    for (source_id, namespace, class) in alignment_sources {
        write_alignment_database_fixture(&store, "mmseqs", source_id, namespace, class)
            .expect("alignment database");
    }
    let mut manifest = String::from(
        "source_id\tmanifest_schema_version\tsource_namespace\tsource_class\tsource_path\tsource_digest\tsource_files\tsource_bytes\tbuilder_version\tdb_path\tstatus\n",
    );
    for (source_id, namespace, class, source_path) in [
        (
            "uniprot",
            "UniProt",
            "known",
            "downloads/uniprot/uniprot_human_reviewed.fasta.gz",
        ),
        (
            "biolip2_protein",
            "BioLiP2",
            "known",
            "downloads/biolip2/protein.fasta.gz",
        ),
        (
            "biolip2_protein_nr",
            "BioLiP2",
            "known",
            "downloads/biolip2/protein_nr.fasta.gz",
        ),
        (
            "biolip2_peptide",
            "BioLiP2",
            "known",
            "downloads/biolip2/peptide.fasta.gz",
        ),
        (
            "biolip2_peptide_nr",
            "BioLiP2",
            "known",
            "downloads/biolip2/peptide_nr.fasta.gz",
        ),
        (
            "mgnify",
            "MGnify",
            "metagenome",
            "downloads/metagenome/mgnify/mgy_clusters.fa.gz",
        ),
    ] {
        manifest.push_str(&format!(
            "{source_id}\t{}\t{namespace}\t{class}\t{source_path}\t{}\t1\t1\tmmseqs-test\tindexes/mmseqs/{source_id}/target\tready\n",
            ALIGNMENT_DATABASE_MANIFEST_SCHEMA_VERSION,
            test_source_digest(source_id),
        ));
    }
    let manifest_path = store.join("indexes/mmseqs/mmseqs_target_databases.tsv");
    write_text(&manifest_path, &manifest).expect("alignment manifest");
    assert!(ready_alignment_namespaces(
        &store,
        "known",
        &["UniProt", "BioLiP2"]
    ));
    assert!(ready_alignment_namespaces(
        &store,
        "metagenome",
        &["MGnify"]
    ));

    let wrong_digest = "b".repeat(64);
    let tampered_uniprot = manifest.replacen(&test_source_digest("uniprot"), &wrong_digest, 1);
    write_text(&manifest_path, &tampered_uniprot).expect("tampered UniProt aggregate");
    assert!(!ready_alignment_namespaces(&store, "known", &["UniProt"]));

    let mgnify_line = manifest
        .lines()
        .find(|line| line.starts_with("mgnify\t"))
        .expect("MGnify row");
    let tampered_mgnify = manifest.replace(
        mgnify_line,
        &mgnify_line.replacen(&test_source_digest("mgnify"), &wrong_digest, 1),
    );
    write_text(&manifest_path, &tampered_mgnify).expect("tampered MGnify payload digest");
    assert!(!ready_alignment_namespaces(
        &store,
        "metagenome",
        &["MGnify"]
    ));

    let duplicate_biolip = manifest.replace(
        "downloads/biolip2/peptide_nr.fasta.gz",
        "downloads/biolip2/peptide.fasta.gz",
    );
    write_text(&manifest_path, &duplicate_biolip).expect("duplicate BioLiP source path");
    assert!(!ready_alignment_namespaces(&store, "known", &["BioLiP2"]));

    write_text(&manifest_path, &manifest).expect("restore alignment manifest");
    fs::remove_file(store.join("indexes/mmseqs/biolip2_protein/target.lookup"))
        .expect("remove required MMseqs artifact");
    assert!(!ready_alignment_namespaces(&store, "known", &["BioLiP2"]));
    fs::remove_dir_all(&root).expect("cleanup temp root");
}

#[test]
fn build_store_indexes_keeps_nonempty_published_indexes_when_sources_are_partial() {
    let root = temp_root("preserve_partial_source_rebuild");
    let store = root.join("science_store");
    let header = store_row_header();
    let indexes = [
        (
            "known_targets",
            format!(
                "{header}UniProt\tKNOWN_SENTINEL\tKnown\tHomo sapiens\tknown_human_protein\t\tknown token\t\tpublished\n"
            ),
        ),
        (
            "metagenome_targets",
            format!(
                "{header}MGnify\tMETA_SENTINEL\tMeta\tmetagenome\tmetagenome_potential\t\tmeta token\t\tpublished\n"
            ),
        ),
        (
            "structure_targets",
            format!(
                "{header}PLINDER\tSTRUCT_SENTINEL\tStructure\tHomo sapiens\tstructure_interaction_record\tlig\tprotein\tpocket\tpublished\nRCSB\t1ABC\tStructure\tunknown\tstructure_interaction_record\tlig\tprotein\t1abc\tpublished\n"
            ),
        ),
    ];
    for (name, text) in &indexes {
        write_text(&store.join(format!("indexes/{name}.tsv")), text).expect("published index");
    }
    write_text(
        &store.join("downloads/rcsb/mmcif/1abc.cif"),
        "data_1ABC\n_entry.id 1ABC\n_atom_site.group_PDB ATOM\n",
    )
    .expect("partial source");
    write_text(
        &store.join("indexes/plinder_targets.tsv"),
        "published PLINDER canonical input\n",
    )
    .expect("published PLINDER input");
    write_test_source_manifest(&store, &["plinder", "rcsb_ligand_bound"])
        .expect("published source identities");
    let build_manifest = store.join("index_build_manifest.tsv");
    let mut build_manifest_text = String::from(
        "index\tinput_path\tstatus\trows\toutput_path\tnotes\nknown_targets\told/known\tused\t1\tindexes/known_targets.tsv\tcomplete_source_scan\nmetagenome_targets\told/meta\tused\t1\tindexes/metagenome_targets.tsv\tcomplete_source_scan\nstructure_targets\tindexes/plinder_targets.tsv\tused\t1\tindexes/structure_targets.tsv\tplinder_canonical_rows;complete_source_scan\nstructure_targets\tdownloads/rcsb/mmcif/1abc.cif\tused\t1\tindexes/structure_targets.tsv\trcsb_structure_file_rows;complete_source_scan\n",
    );
    append_store_index_publication_identity(
        &store,
        "known_targets",
        &store.join("indexes/known_targets.tsv"),
        1,
        &mut build_manifest_text,
    )
    .expect("known publication identity");
    append_store_index_publication_identity(
        &store,
        "metagenome_targets",
        &store.join("indexes/metagenome_targets.tsv"),
        1,
        &mut build_manifest_text,
    )
    .expect("metagenome publication identity");
    write_text(&build_manifest, &build_manifest_text).expect("published manifest");
    write_structure_targets_manifest(&store, &build_manifest).expect("structure publication");
    let before = indexes
        .iter()
        .map(|(name, _)| {
            (
                *name,
                fs::read(store.join(format!("indexes/{name}.tsv"))).expect("published bytes"),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let options = StoreOptions {
        workspace_root: root.clone(),
        science_store: store.clone(),
    };

    build_store_indexes(&options, &build_manifest).expect("preserving build");

    for (name, _) in indexes {
        assert_eq!(
            fs::read(store.join(format!("indexes/{name}.tsv"))).expect("preserved bytes"),
            before[name]
        );
    }
    let rebuilt_manifest = read_text(&build_manifest).expect("manifest text");
    assert!(
        rebuilt_manifest.contains("known_targets\tindexes/known_targets.tsv\tpublication_ready")
    );
    assert!(
        rebuilt_manifest
            .contains("metagenome_targets\tindexes/metagenome_targets.tsv\tpublication_ready")
    );
    let known_path = store.join("indexes/known_targets.tsv");
    let mut tampered = fs::read(&known_path).expect("known publication bytes");
    let offset = tampered
        .iter()
        .position(|byte| *byte == b'K')
        .expect("known tamper byte");
    tampered[offset] = b'X';
    fs::write(&known_path, &tampered).expect("same-size known tamper");
    let error = build_store_indexes(&options, &build_manifest)
        .expect_err("tampered known publication must not be restamped");
    assert!(error.contains("known_targets_publication_identity_mismatch"));
    assert_eq!(
        fs::read(&known_path).expect("tampered known remains untouched"),
        tampered
    );
    fs::remove_dir_all(&root).expect("cleanup temp root");
}

#[test]
fn materialized_known_input_update_rebuilds_publication() {
    let root = temp_root("materialized_known_input_update");
    let store = root.join("science_store");
    let input = store.join("imports/known_targets.tsv");
    let output = store.join("indexes/known_targets.tsv");
    let build_manifest = store.join("index_build_manifest.tsv");
    let options = StoreOptions {
        workspace_root: root.clone(),
        science_store: store.clone(),
    };
    write_text(
        &input,
        &format!(
            "{}Custom\tOLD_TARGET\tOld target\tHomo sapiens\tcustom_target\told_ligand\told_sequence\told_structure\told_evidence\n",
            store_row_header()
        ),
    )
    .expect("initial custom known input");

    build_store_indexes(&options, &build_manifest).expect("initial known publication");
    let initial_text = read_text(&output).expect("initial known publication text");
    let initial_digest = sha256_file_hex(&output).expect("initial known publication digest");
    assert!(initial_text.contains("\tOLD_TARGET\t"));
    assert_eq!(
        published_store_index_identity(&output, &build_manifest, "known_targets")
            .expect("initial known publication identity"),
        Some(1)
    );

    write_text(
        &input,
        &format!(
            "{}Custom\tNEW_TARGET\tNew target\tHomo sapiens\tcustom_target\tnew_ligand\tnew_sequence\tnew_structure\tnew_evidence\n",
            store_row_header()
        ),
    )
    .expect("updated custom known input");
    build_store_indexes(&options, &build_manifest).expect("updated known publication");

    let updated_text = read_text(&output).expect("updated known publication text");
    let updated_digest = sha256_file_hex(&output).expect("updated known publication digest");
    let updated_manifest = read_text(&build_manifest).expect("updated build manifest");
    assert!(updated_text.contains("\tNEW_TARGET\t"));
    assert!(!updated_text.contains("\tOLD_TARGET\t"));
    assert_ne!(updated_text, initial_text);
    assert_ne!(updated_digest, initial_digest);
    assert!(updated_manifest.contains(&format!("publication_sha256={updated_digest}")));
    assert_eq!(
        published_store_index_identity(&output, &build_manifest, "known_targets")
            .expect("updated known publication identity"),
        Some(1)
    );
    fs::remove_dir_all(&root).expect("cleanup temp root");
}

#[test]
fn changed_partial_structure_source_cannot_replace_published_index() {
    let root = temp_root("changed_partial_structure_source");
    let store = root.join("science_store");
    let source = store.join("downloads/biolip2/BioLiP.txt.gz");
    let structure = store.join("indexes/structure_targets.tsv");
    let build_manifest = store.join("index_build_manifest.tsv");
    write_text(&source, "old BioLiP input\n").expect("old BioLiP input");
    write_text(
        &structure,
        &format!(
            "{}BioLiP2\tOLD_TARGET\tOld target\tunknown\tstructure_interaction_record\tlig\tprotein\tpocket\tpublished\n",
            store_row_header()
        ),
    )
    .expect("published structure index");
    write_text(
        &build_manifest,
        "index\tinput_path\tstatus\trows\toutput_path\tnotes\nstructure_targets\tdownloads/biolip2/BioLiP.txt.gz\tused\t1\tindexes/structure_targets.tsv\tbiolip2_interaction_rows;complete_source_scan\n",
    )
    .expect("published build manifest");
    write_test_source_manifest(&store, &["biolip2"]).expect("BioLiP source identity");
    write_structure_targets_manifest(&store, &build_manifest).expect("structure publication");
    let before = fs::read(&structure).expect("published bytes");
    write_text(&source, "new partial BioLiP input\n").expect("new partial BioLiP input");
    let options = StoreOptions {
        workspace_root: root.clone(),
        science_store: store.clone(),
    };

    let error = build_store_indexes(&options, &build_manifest)
        .expect_err("partial new source cannot replace publication");

    assert!(error.contains("structure_targets_publication_invalid_without_rebuild_authority"));
    assert_eq!(
        fs::read(&structure).expect("preserved structure bytes"),
        before
    );
    assert_eq!(
        read_text(&source).expect("new source remains available"),
        "new partial BioLiP input\n"
    );
    fs::remove_dir_all(&root).expect("cleanup temp root");
}

#[test]
fn bounded_rcsb_retention_preserves_structure_publication_identity() {
    let root = temp_root("bounded_rcsb_structure_identity");
    let store = root.join("science_store");
    let source = store.join("downloads/rcsb/mmcif/1abc.cif");
    let structure = store.join("indexes/structure_targets.tsv");
    let build_manifest = store.join("index_build_manifest.tsv");
    write_text(
        &source,
        "data_1ABC\n_entry.id 1ABC\n_atom_site.group_PDB ATOM\n",
    )
    .expect("RCSB source");
    write_text(
        &structure,
        &format!(
            "{}RCSB\t1ABC\tStructure\tunknown\tstructure_interaction_record\tlig\tprotein\t1abc\tpublished\n",
            store_row_header()
        ),
    )
    .expect("structure publication");
    write_text(
        &build_manifest,
        "index\tinput_path\tstatus\trows\toutput_path\tnotes\nstructure_targets\tdownloads/rcsb/mmcif/1abc.cif\tused\t1\tindexes/structure_targets.tsv\trcsb_structure_file_rows;complete_source_scan\n",
    )
    .expect("build manifest");
    write_test_source_manifest(&store, &["rcsb_ligand_bound"]).expect("RCSB source identity");
    write_structure_targets_manifest(&store, &build_manifest).expect("structure manifest");
    let publication = read_json_value_if_file(&store.join(STRUCTURE_TARGETS_MANIFEST_REL_PATH))
        .expect("structure publication JSON");
    let contribution = publication["contributions"]
        .as_array()
        .and_then(|rows| rows.first())
        .expect("RCSB contribution");
    write_text(
        &store.join("manifests/source_retention_manifest.tsv"),
        &format!(
            "schema_version\tsource_id\tstate\tsource_manifest_sha256\trelease_identity\ttrust_source\tstructure_input_bytes\tstructure_input_digest\n{}\trcsb_ligand_bound\truntime_cache_bounded\t{}\t{}\t{}\t{}\t{}\n",
            SOURCE_RETENTION_SCHEMA_VERSION,
            contribution["source_digest"].as_str().expect("source digest"),
            contribution["release_identity"].as_str().expect("release identity"),
            contribution["trust_source"].as_str().expect("trust source"),
            contribution["input_bytes"].as_u64().expect("input bytes"),
            contribution["input_digest"].as_str().expect("input digest"),
        ),
    )
    .expect("bounded RCSB retention");
    let current = source_payload_identity(&store.join("downloads/rcsb/mmcif"))
        .expect("current bounded-cache payload identity");
    write_text(
        &store.join("manifests/source_file_manifest.tsv"),
        &format!(
            "source_id\tsource_label\tsource_class\tlocal_path\tstatus\tstorage_state\tfile_count\tbytes\tsha256\tprovenance\trelease_identity\ttrust_source\nrcsb_ligand_bound\tRCSB\tstructure\tdownloads/rcsb/mmcif\tready\tbounded_runtime_cache\t{}\t{}\t{}\ttest\tbounded-cache-release\tpublisher\n",
            current.file_count, current.bytes, current.digest,
        ),
    )
    .expect("current bounded-cache source identity");
    structure_source_identity(&store, "RCSB").expect("Python bounded cache state accepted");
    write_text(
        &source,
        "data_2XYZ\n_entry.id 2XYZ\n_atom_site.group_PDB ATOM\n",
    )
    .expect("mutate runtime cache after publication");

    validate_structure_targets_publication(&store)
        .expect("bounded retention validates original structure publication");
    fs::remove_dir_all(&root).expect("cleanup temp root");
}

#[test]
fn published_index_replace_failure_rolls_back_existing_file() {
    let root = temp_root("published_index_rollback");
    let output = root.join("indexes/known_targets.tsv");
    let missing_pending = root.join("indexes/missing.pending.tsv");
    write_text(&output, "published sentinel\n").expect("published output");

    let error = publish_store_row_index(&missing_pending, &output).expect_err("replace failure");

    assert!(error.contains("published_index_replace_failed"));
    assert_eq!(
        read_text(&output).expect("rolled back output"),
        "published sentinel\n"
    );
    fs::remove_dir_all(&root).expect("cleanup temp root");
}

#[test]
fn interrupted_structure_publication_restores_complete_previous_bundle() {
    let root = temp_root("structure_publication_reconcile");
    let store = root.join("science_store");
    let bundle = structure_publication_bundle_paths(&store);
    let previous = [
        "old known targets\n",
        "old metagenome targets\n",
        "old structure\n",
        "old build manifest\n",
        "old publication\n",
    ];
    for (path, text) in bundle.iter().zip(previous.iter().copied()) {
        write_text(path, text).expect("old publication member");
    }
    let refs = bundle.iter().map(PathBuf::as_path).collect::<Vec<_>>();
    let backups = backup_publication_bundle(&store, &refs).expect("backup publication bundle");
    assert_eq!(backups.len(), 5);
    for (index, path) in bundle.iter().take(4).enumerate() {
        write_text(path, &format!("partial new publication member {index}\n"))
            .expect("partial new publication");
    }
    fs::remove_file(&bundle[4]).expect("interrupt before final publication member");

    reconcile_structure_publication_transaction(&store).expect("reconcile publication");

    for (path, text) in bundle.iter().zip(previous.iter().copied()) {
        assert_eq!(read_text(path).expect("restored publication member"), text);
    }
    assert!(
        !store
            .join(STRUCTURE_PUBLICATION_TRANSACTION_REL_PATH)
            .exists()
    );
    assert!(
        backups
            .iter()
            .all(|entry| !entry.pending.exists() && !entry.backup.exists())
    );
    fs::remove_dir_all(&root).expect("cleanup temp root");
}

#[test]
fn incomplete_publication_backup_never_replaces_current_bundle() {
    let root = temp_root("structure_publication_incomplete_backup");
    let store = root.join("science_store");
    let bundle = structure_publication_bundle_paths(&store);
    for (index, path) in bundle.iter().enumerate() {
        write_text(path, &format!("old publication member {index}\n"))
            .expect("old publication member");
    }
    let refs = bundle.iter().map(PathBuf::as_path).collect::<Vec<_>>();
    let backups = backup_publication_bundle(&store, &refs).expect("backup publication bundle");
    for (index, path) in bundle.iter().enumerate() {
        write_text(path, &format!("current publication member {index}\n"))
            .expect("current publication member");
    }
    fs::remove_file(&backups[0].backup).expect("remove completed backup");
    write_text(&backups[0].pending, "incomplete backup\n").expect("partial pending backup");

    let error = reconcile_structure_publication_transaction(&store)
        .expect_err("incomplete backup blocks restoration");

    assert!(error.contains("publication_bundle_backup_identity_invalid"));
    for (index, path) in bundle.iter().enumerate() {
        assert_eq!(
            read_text(path).expect("current publication remains"),
            format!("current publication member {index}\n")
        );
    }
    assert!(
        store
            .join(STRUCTURE_PUBLICATION_TRANSACTION_REL_PATH)
            .is_file()
    );
    fs::remove_dir_all(&root).expect("cleanup temp root");
}

#[test]
fn failed_compact_ligand_build_keeps_all_legacy_staging() {
    let root = temp_root("ligand_similarity_build_failure");
    let store = root.join("science_store");
    install_ligand_similarity_builder_fixture(&root, true).expect("failing builder");
    write_ligand_similarity_staging_fixture(&store).expect("legacy ligand staging");
    let sidecar = PathBuf::from(format!(
        "{}-wal",
        store.join(BINDINGDB_QUERY_DATABASE_REL_PATH).display()
    ));
    write_text(&sidecar, "recoverable staging\n").expect("staging sidecar");
    let options = StoreOptions {
        workspace_root: root.clone(),
        science_store: store.clone(),
    };

    let out = run_store_command(ScienceCommand::PrepareScienceStore, &options)
        .expect("prepare records repair state");

    assert!(out.contains("status=needs_repair"));
    for path in [
        store.join("downloads/chembl/chembl_query.sqlite"),
        store.join(CHEMBL_QUERY_MANIFEST_REL_PATH),
        store.join(BINDINGDB_QUERY_DATABASE_REL_PATH),
        store.join(BINDINGDB_QUERY_MANIFEST_REL_PATH),
        sidecar,
    ] {
        assert!(
            path.exists(),
            "recoverable staging removed: {}",
            path.display()
        );
    }
    assert!(!store.join(LIGAND_SIMILARITY_DATABASE_REL_PATH).exists());
    assert!(!store.join(LIGAND_SIMILARITY_MANIFEST_REL_PATH).exists());
    let status = read_text(&store.join("install_status.json")).expect("install status");
    assert!(status.contains("ligand_similarity_build_python_failed"));
    fs::remove_dir_all(&root).expect("cleanup temp root");
}

#[test]
fn compact_known_rows_bind_to_the_current_database_and_manifest_publication() {
    let root = temp_root("compact_known_row_publication");
    let store = root.join("science_store");
    let run = root.join("run");
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: store.clone(),
        input_manifest: root.join("input.json"),
        run_dir: run.clone(),
    };
    write_test_source_manifest(&store, &["chembl"]).expect("source manifest");
    write_text(&run.join(PIPELINE_INVOCATION_FILE), "publication-test\n")
        .expect("pipeline invocation");
    let publication =
        write_compact_ligand_similarity_publication_fixture(&store).expect("publication fixture");
    let result_path = run.join("compact_rows.tsv");
    let write_row = |publication: &LigandSimilarityPublication| {
        write_text(&result_path, &compact_known_row_fixture(publication))
    };
    let validate = || append_bound_known_rows(&options, &result_path, &mut Vec::new());

    write_row(&publication).expect("valid row");
    validate().expect("current publication accepted");

    let database = store.join(LIGAND_SIMILARITY_DATABASE_REL_PATH);
    std::thread::sleep(std::time::Duration::from_millis(2));
    write_text(&database, "compact-publication-database-v2\n").expect("same-size database tamper");
    let error = validate().expect_err("database content tamper rejected");
    assert!(
        error.contains("database_digest"),
        "unexpected error: {error}"
    );
    std::thread::sleep(std::time::Duration::from_millis(2));
    write_text(&database, "compact-publication-database-v1\n").expect("restore database");

    let manifest = store.join(LIGAND_SIMILARITY_MANIFEST_REL_PATH);
    let manifest_text = read_text(&manifest).expect("manifest");
    write_text(&manifest, &(manifest_text.clone() + "\n")).expect("manifest byte tamper");
    let error = validate().expect_err("manifest digest tamper rejected");
    assert!(
        error.contains("compact_manifest_sha256"),
        "unexpected error: {error}"
    );
    write_text(&manifest, &manifest_text).expect("restore manifest");

    let mut schema_tamper = publication.clone();
    schema_tamper.schema_version = "genoligand_ligand_similarity_index_v1".to_string();
    write_row(&schema_tamper).expect("schema tamper row");
    let error = validate().expect_err("schema marker tamper rejected");
    assert!(
        error.contains("compact_schema_version"),
        "unexpected error: {error}"
    );

    let mut identity_tamper = publication.clone();
    identity_tamper.build_identity = "f".repeat(64);
    write_row(&identity_tamper).expect("identity tamper row");
    let error = validate().expect_err("build identity marker tamper rejected");
    assert!(
        error.contains("compact_build_identity"),
        "unexpected error: {error}"
    );

    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn known_matching_uses_published_chembl_query_index_and_ignores_archive() {
    let root = temp_root("chembl_onsite_search");
    let store = root.join("science_store");
    let run = root.join("run");
    let input = root.join("input.json");
    write_text(
        &store.join("indexes/known_targets.tsv"),
        "source\ttarget_id\ttarget_name\torganism\ttarget_class\tligand_tokens\tsequence_tokens\tstructure_tokens\tevidence\n",
    )
    .expect("empty known index");
    let query_index = store.join("downloads/chembl/chembl_query.sqlite");
    write_published_chembl_query_sqlite(&query_index).expect("published ChEMBL query index");
    let archive = store.join("downloads/chembl/chembl_sqlite.tar.gz");
    write_text(&archive, "archive must remain unread by runtime\n").expect("archive sentinel");
    write_test_source_manifest(&store, &["chembl"]).expect("ChEMBL source manifest");
    write_text(
        &input,
        "{\"inputs\":[{\"field\":\"small_molecule\",\"value\":\"CCO\"}],\"files\":[]}\n",
    )
    .expect("input");
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: store,
        input_manifest: input,
        run_dir: run.clone(),
    };

    run_science_command(ScienceCommand::InputNormalize, &options).expect("normalize");
    run_science_command(ScienceCommand::FeaturesCompute, &options).expect("features");
    run_science_command(ScienceCommand::DatabaseMatchKnown, &options).expect("match known");
    let matches = read_text(&run.join("known_database_matches.tsv")).expect("known matches");

    assert!(matches.contains("ChEMBL"));
    assert!(matches.contains("CHEMBL_T1"));
    assert!(matches.contains("Cyclin-dependent kinase 2"));
    assert!(matches.contains("chembl_onsite_activity_row"));
    assert!(matches.contains("IC50=42 nM"));
    assert!(matches.contains("canonical_smiles=CCO"));
    assert!(matches.contains("source_release=chembl-test-release"));
    assert!(matches.contains(&format!("source_digest={}", test_source_digest("chembl"))));
    assert!(matches.contains("query_digest="));
    assert!(matches.contains("run_id=run"));
    assert!(matches.contains("invocation_id=chembl_query_"));
    assert_eq!(
        read_text(&archive).expect("archive sentinel after query"),
        "archive must remain unread by runtime\n"
    );
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn chembl_runtime_rejects_missing_database_and_manifest_size_mismatch() {
    let root = temp_root("chembl_runtime_contract");
    let store = root.join("science_store");
    let run = root.join("run");
    let input = root.join("input.json");
    write_text(
        &store.join("indexes/known_targets.tsv"),
        "source\ttarget_id\ttarget_name\torganism\ttarget_class\tligand_tokens\tsequence_tokens\tstructure_tokens\tevidence\n",
    )
    .expect("empty known index");
    let query_index = store.join("downloads/chembl/chembl_query.sqlite");
    write_published_chembl_query_sqlite(&query_index).expect("published ChEMBL query index");
    write_test_source_manifest(&store, &["chembl"]).expect("ChEMBL source manifest");
    write_text(
        &input,
        "{\"inputs\":[{\"field\":\"small_molecule\",\"value\":\"CCO\"}],\"files\":[]}\n",
    )
    .expect("input");
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: store.clone(),
        input_manifest: input,
        run_dir: run.clone(),
    };
    run_science_command(ScienceCommand::InputNormalize, &options).expect("normalize");
    run_science_command(ScienceCommand::FeaturesCompute, &options).expect("features");
    write_test_science_store_readiness(&store, &[]).expect("readiness with ChEMBL manifest");

    fs::remove_file(&query_index).expect("remove published database");
    let missing = run_science_command(ScienceCommand::DatabaseMatchKnown, &options)
        .expect_err("missing published ChEMBL database must fail");
    assert!(
        missing.contains("chembl_query_contract_missing"),
        "{missing}"
    );

    write_published_chembl_query_sqlite(&query_index).expect("restore published database");
    let manifest_path = query_index.with_extension("manifest.json");
    let mut manifest =
        serde_json::from_str::<Value>(&read_text(&manifest_path).expect("published manifest"))
            .expect("manifest JSON");
    manifest["database_bytes"] =
        json!(fs::metadata(&query_index).expect("database metadata").len() + 1);
    write_text(&manifest_path, &format!("{manifest}\n")).expect("tampered manifest");
    write_test_science_store_readiness(&store, &[]).expect("readiness bound to tampered manifest");
    let mismatch = run_science_command(ScienceCommand::DatabaseMatchKnown, &options)
        .expect_err("manifest database byte mismatch must fail");
    assert!(
        mismatch.contains("chembl_query_manifest_contract_invalid"),
        "{mismatch}"
    );
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn p1_integrity_chembl_runtime_hash_cache_detects_same_size_tamper_and_reuses_digest() {
    let root = temp_root("chembl_runtime_hash_cache");
    let store = root.join("science_store");
    let database = store.join("downloads/chembl/chembl_query.sqlite");
    let manifest = database.with_extension("manifest.json");
    write_published_chembl_query_sqlite(&database).expect("published ChEMBL query index");
    write_test_source_manifest(&store, &["chembl"]).expect("ChEMBL source manifest");
    reset_chembl_query_database_hash_count(&database);
    clear_chembl_query_process_hash_cache(&database);

    validate_chembl_query_runtime_contract(&store, &database, &manifest)
        .expect("first validation hashes database");
    assert_eq!(chembl_query_database_hash_count(&database), 1);
    let manifest_text = read_text(&manifest).expect("manifest");
    let manifest_sha256 = sha256_bytes_hex(manifest_text.as_bytes());
    let initial_identity =
        chembl_query_hash_identity(&database, &manifest_sha256).expect("initial identity");
    let cache_path =
        chembl_query_hash_cache_path(&database, &initial_identity).expect("cache path");
    let cache_modified = fs::metadata(&cache_path)
        .and_then(|metadata| metadata.modified())
        .expect("cache modified time");

    validate_chembl_query_runtime_contract(&store, &database, &manifest)
        .expect("process cache hit");
    assert_eq!(chembl_query_database_hash_count(&database), 1);
    clear_chembl_query_process_hash_cache(&database);
    validate_chembl_query_runtime_contract(&store, &database, &manifest)
        .expect("persistent cache hit");
    assert_eq!(chembl_query_database_hash_count(&database), 1);
    assert_eq!(
        fs::metadata(&cache_path)
            .and_then(|metadata| metadata.modified())
            .expect("cache modified time after hits"),
        cache_modified
    );

    let original = fs::read(&database).expect("original database bytes");
    let mut tampered = original.clone();
    let offset = tampered.len() / 2;
    tampered[offset] ^= 0x01;
    thread::sleep(Duration::from_millis(2));
    fs::write(&database, &tampered).expect("same-size tamper");
    assert_eq!(
        fs::metadata(&database).expect("tampered metadata").len(),
        original.len() as u64
    );
    assert_ne!(
        chembl_query_hash_identity(&database, &manifest_sha256).expect("tampered identity"),
        initial_identity
    );
    let error = validate_chembl_query_runtime_contract(&store, &database, &manifest)
        .expect_err("tampered database must fail");
    assert_eq!(error, "database_sha256_mismatch");
    assert_eq!(chembl_query_database_hash_count(&database), 2);

    thread::sleep(Duration::from_millis(2));
    fs::write(&database, &original).expect("restore database");
    validate_chembl_query_runtime_contract(&store, &database, &manifest)
        .expect("restored database must pass");
    assert_eq!(chembl_query_database_hash_count(&database), 3);
    validate_chembl_query_runtime_contract(&store, &database, &manifest)
        .expect("restored process cache hit");
    assert_eq!(chembl_query_database_hash_count(&database), 3);

    clear_chembl_query_process_hash_cache(&database);
    fs::remove_dir_all(&root).expect("cleanup temp root");
}

#[test]
fn chembl_onsite_preserves_exact_relation_and_has_no_shared_row_cap() {
    let root = temp_root("chembl_relation_and_unbounded_targets");
    let store = root.join("science_store");
    let run = root.join("run");
    let input = root.join("input.json");
    write_text(
        &store.join("indexes/known_targets.tsv"),
        "source\ttarget_id\ttarget_name\torganism\ttarget_class\tligand_tokens\tsequence_tokens\tstructure_tokens\tevidence\n",
    )
    .expect("empty known index");
    let query_index = store.join("downloads/chembl/chembl_query.sqlite");
    write_many_published_chembl_activities(&query_index, 205)
        .expect("many published ChEMBL activities");
    write_test_source_manifest(&store, &["chembl"]).expect("ChEMBL source manifest");
    write_text(
        &input,
        "{\"inputs\":[{\"field\":\"small_molecule\",\"value\":\"CCO\"},{\"field\":\"small_molecule\",\"value\":\"CCN\"}],\"files\":[]}\n",
    )
    .expect("input");
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: store,
        input_manifest: input,
        run_dir: run.clone(),
    };

    run_science_command(ScienceCommand::InputNormalize, &options).expect("normalize");
    run_science_command(ScienceCommand::FeaturesCompute, &options).expect("features");
    run_science_command(ScienceCommand::DatabaseMatchKnown, &options).expect("match known");
    let matches = read_text(&run.join("known_database_matches.tsv")).expect("known matches");
    let rows = matches
        .lines()
        .skip(1)
        .map(split_tsv)
        .filter(|cols| cols.get(1).is_some_and(|source| source == "ChEMBL"))
        .collect::<Vec<_>>();

    assert_eq!(
        rows.iter()
            .filter(|cols| cols[0] == "input_1" && cols[2].starts_with("CHEMBL_CCO_"))
            .count(),
        205
    );
    assert_eq!(
        rows.iter()
            .filter(|cols| cols[0] == "input_2" && cols[2].starts_with("CHEMBL_CCN_"))
            .count(),
        205
    );
    let exact = rows
        .iter()
        .find(|cols| cols[2] == "CHEMBL_RELATION_BEST")
        .expect("relation best row");
    assert_eq!(exact[6], "0.9500");
    assert_eq!(exact[10], "IC50=100 nM");
    assert!(exact[8].contains("relation=="));
    let censored = rows
        .iter()
        .find(|cols| cols[2] == "CHEMBL_RELATION_CENSORED")
        .expect("censored row");
    assert_eq!(censored[6], "0.0000");
    assert_eq!(censored[10], "Ki<10 nM");
    assert!(censored[8].contains("relation=<"));

    fs::remove_dir_all(&root).expect("cleanup temp root");
}

#[test]
fn bindingdb_query_index_preserves_all_source_members_without_shared_row_cap() {
    let root = temp_root("bindingdb_all_members_unbounded_targets");
    let store = root.join("science_store");
    let run = root.join("run");
    let input = root.join("input.json");
    write_text(
        &store.join("indexes/known_targets.tsv"),
        "source\ttarget_id\ttarget_name\torganism\ttarget_class\tligand_tokens\tsequence_tokens\tstructure_tokens\tevidence\n",
    )
    .expect("empty known index");
    let header = "Ligand SMILES\tBindingDB Ligand Name\tTarget Name\tTarget Source Organism According to Curator or DataSource\tKi (nM)\tUniProt (SwissProt) Primary ID of Target Chain 1\tBindingDB MonomerID\n";
    let mut first = header.to_string();
    let mut second = header.to_string();
    for index in 0..205 {
        let target = if index % 2 == 0 {
            &mut first
        } else {
            &mut second
        };
        target.push_str(&format!(
            "CCO\tethanol\tCCO target {index}\tHomo sapiens\t{}\tP_CCO_{index}\tCCO_{index}\n",
            index + 1
        ));
        target.push_str(&format!(
            "CCN\tethylamine\tCCN target {index}\tHomo sapiens\t{}\tP_CCN_{index}\tCCN_{index}\n",
            index + 1
        ));
    }
    write_zip_members(
        &store.join("downloads/bindingdb/bindingdb_all.tsv.zip"),
        &[("z_part.tsv", &second), ("a_part.tsv", &first)],
    )
    .expect("bindingdb zip");
    write_test_source_manifest(&store, &["bindingdb"]).expect("bindingdb source manifest");
    publish_test_store_bindingdb_query(&store).expect("BindingDB query index");
    write_text(
        &input,
        "{\"inputs\":[{\"field\":\"small_molecule\",\"value\":\"CCO\"},{\"field\":\"small_molecule\",\"value\":\"CCN\"}],\"files\":[]}\n",
    )
    .expect("input");
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: store,
        input_manifest: input,
        run_dir: run.clone(),
    };

    run_science_command(ScienceCommand::InputNormalize, &options).expect("normalize");
    run_science_command(ScienceCommand::FeaturesCompute, &options).expect("features");
    run_science_command(ScienceCommand::DatabaseMatchKnown, &options).expect("match known");
    let matches = read_text(&run.join("known_database_matches.tsv")).expect("known matches");
    let rows = matches
        .lines()
        .skip(1)
        .map(split_tsv)
        .filter(|cols| cols.get(1).is_some_and(|source| source == "BindingDB"))
        .collect::<Vec<_>>();

    assert_eq!(rows.iter().filter(|cols| cols[0] == "input_1").count(), 205);
    assert_eq!(rows.iter().filter(|cols| cols[0] == "input_2").count(), 205);
    assert!(rows.iter().any(|cols| cols[2] == "P_CCO_0"));
    assert!(rows.iter().any(|cols| cols[2] == "P_CCO_204"));

    fs::remove_dir_all(&root).expect("cleanup temp root");
}

#[test]
fn onsite_query_scripts_require_published_index_and_disk_backed_deduplication() {
    let chembl = chembl_onsite_query_script();
    let bindingdb = bindingdb_onsite_query_script();
    let bindingdb_builder = bindingdb_query_build_script();

    assert!(chembl.contains("chembl_query_requires_published_sqlite"));
    assert!(chembl.contains("mode=ro"));
    assert!(chembl.contains("PRAGMA index_list(genoligand_onsite_activity)"));
    assert!(chembl.contains("EXPLAIN QUERY PLAN"));
    assert!(chembl.contains("chembl_query_plan_full_scan"));
    assert!(!chembl.contains("tarfile"));
    assert!(!chembl.contains("zipfile"));
    assert!(chembl.contains("result_path"));
    assert!(bindingdb.contains("mode=ro&immutable=1"));
    assert!(bindingdb.contains("idx_bindingdb_activity_canonical_smiles"));
    assert!(bindingdb.contains("bindingdb_query_requires_published_sqlite"));
    assert!(!bindingdb.contains("zipfile"));
    assert!(!bindingdb.contains("best = {}"));
    assert!(bindingdb.contains("CREATE TABLE best"));
    assert!(bindingdb_builder.contains("zipfile.ZipFile"));
    assert!(bindingdb_builder.contains("CREATE INDEX idx_bindingdb_activity_canonical_smiles"));
}

#[test]
fn onsite_aggregation_uses_input_namespace_target_key_across_records_and_sources() {
    let root = temp_root("onsite_namespace_aggregation_key");
    let query = root.join("queries.tsv");
    write_text(
        &query,
        "input_id\trecord_id\tquery\ninput_1\trecord_b\tCCO\ninput_1\trecord_a\tCCO\n",
    )
    .expect("queries");
    let chembl = root.join("chembl_query.sqlite");
    write_published_chembl_query_sqlite(&chembl).expect("published ChEMBL query index");
    let chembl_result = root.join("chembl_results.tsv");
    run_python_script(
        chembl_onsite_query_script(),
        &[
            chembl.display().to_string(),
            query.display().to_string(),
            chembl_result.display().to_string(),
            "run_namespace_test".to_string(),
            "chembl_invocation_namespace_test".to_string(),
        ],
        "chembl_namespace_key",
    )
    .expect("chembl query");
    let chembl_rows = read_text(&chembl_result).expect("chembl results");

    let bindingdb_archive = root.join("bindingdb.zip");
    write_zip(
        &bindingdb_archive,
        "bindingdb.tsv",
        "Ligand SMILES\tBindingDB Ligand Name\tTarget Name\tTarget Source Organism According to Curator or DataSource\tKi (nM)\tUniProt (SwissProt) Primary ID of Target Chain 1\nCCO\tethanol\tTarget one\tHomo sapiens\t12\tP_ONE\n",
    )
    .expect("bindingdb zip");
    let bindingdb = root.join("bindingdb_query.sqlite");
    publish_test_bindingdb_query(&bindingdb_archive, &bindingdb)
        .expect("published BindingDB query index");
    let bindingdb_result = root.join("bindingdb_results.tsv");
    run_python_script(
        bindingdb_onsite_query_script(),
        &[
            bindingdb.display().to_string(),
            query.display().to_string(),
            bindingdb_result.display().to_string(),
            test_source_digest("bindingdb"),
            "bindingdb-test-release".to_string(),
            "run_namespace_test".to_string(),
            "bindingdb_invocation_namespace_test".to_string(),
        ],
        "bindingdb_namespace_key",
    )
    .expect("bindingdb query");
    let bindingdb_rows = read_text(&bindingdb_result).expect("bindingdb results");

    assert_eq!(chembl_rows.matches("\tCHEMBL_T1\t").count(), 2);
    assert_eq!(bindingdb_rows.matches("\tP_ONE\t").count(), 2);
    assert!(chembl_rows.contains("\trecord_a\n"));
    assert!(chembl_rows.contains("\trecord_b\n"));
    assert!(bindingdb_rows.contains("\trecord_a\n"));
    assert!(bindingdb_rows.contains("\trecord_b\n"));
    fs::remove_dir_all(&root).expect("cleanup temp root");
}

#[test]
fn chembl_censored_relations_are_evidence_not_quantitative_scores() {
    let root = temp_root("chembl_relation_filter");
    let store = root.join("science_store");
    let run = root.join("run");
    let input = root.join("input.json");
    write_text(
        &store.join("indexes/known_targets.tsv"),
        "source\ttarget_id\ttarget_name\torganism\ttarget_class\tligand_tokens\tsequence_tokens\tstructure_tokens\tevidence\n",
    )
    .expect("empty known index");
    let sqlite = store.join("downloads/chembl/chembl_query.sqlite");
    write_published_chembl_query_sqlite(&sqlite).expect("published ChEMBL query index");
    add_published_chembl_activity_rows(
        &sqlite,
        &[
            (
                "CHEMBL_GT".to_string(),
                "CCO".to_string(),
                ">".to_string(),
                10.0,
            ),
            (
                "CHEMBL_LT".to_string(),
                "CCO".to_string(),
                "<".to_string(),
                10.0,
            ),
        ],
    )
    .expect("censored activities");
    write_test_source_manifest(&store, &["chembl"]).expect("ChEMBL source manifest");
    write_text(
        &input,
        "{\"inputs\":[{\"field\":\"small_molecule\",\"value\":\"CCO\"}],\"files\":[]}\n",
    )
    .expect("input");
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: store,
        input_manifest: input,
        run_dir: run.clone(),
    };

    run_science_command(ScienceCommand::InputNormalize, &options).expect("normalize");
    run_science_command(ScienceCommand::FeaturesCompute, &options).expect("features");
    run_science_command(ScienceCommand::DatabaseMatchKnown, &options).expect("match known");
    let matches = read_text(&run.join("known_database_matches.tsv")).expect("known matches");
    let chembl = matches
        .lines()
        .skip(1)
        .map(split_tsv)
        .filter(|cols| cols.get(1).is_some_and(|source| source == "ChEMBL"))
        .map(|cols| (cols[2].clone(), (cols[6].clone(), cols[8].clone())))
        .collect::<BTreeMap<_, _>>();

    assert_eq!(chembl["CHEMBL_T1"].0, "0.9500");
    for (target, relation) in [("CHEMBL_GT", ">"), ("CHEMBL_LT", "<")] {
        assert_eq!(chembl[target].0, "0.0000");
        assert!(chembl[target].1.contains(&format!("relation={relation}")));
    }
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn structure_priority_does_not_treat_activity_bounds_as_exact_values() {
    for (evidence, expected_nm) in [
        ("bindingdb;Ki (pM)=2500;relation==", 2.5),
        ("bindingdb;Kd (nM)=10;relation==", 10.0),
        ("chembl;IC50=0.25 uM;relation==", 250.0),
        ("chembl;EC50=0.001 mM;relation==", 1_000.0),
        ("chembl;Ki=0.000001 M;relation==", 1_000.0),
        ("chembl;Kd=0.5 µM;relation==", 500.0),
    ] {
        assert_eq!(activity_nm_from_evidence(evidence), Some(expected_nm));
    }
    for evidence in [
        "bindingdb;Ki (nM)=>10;relation=>",
        "bindingdb;IC50 (nM)=<5;relation=<",
        "bindingdb;Kd (nM)=>=10;relation=>=",
        "bindingdb;EC50 (nM)=<=5;relation=<=",
        "bindingdb;Ki (nM)=~10;relation=~",
        "chembl;IC50>10 nM;relation=>",
        "chembl;IC50<10 nM;relation=<",
        "chembl;IC50~10 nM;relation=~",
        "chembl;IC50=10 molar;relation==",
    ] {
        assert_eq!(activity_nm_from_evidence(evidence), None, "{evidence}");
    }
}

#[test]
fn chembl_onsite_aggregation_keeps_more_than_200_targets_for_each_input() {
    let root = temp_root("chembl_per_input_unbounded");
    let store = root.join("science_store");
    let run = root.join("run");
    let input = root.join("input.json");
    write_text(
        &store.join("indexes/known_targets.tsv"),
        "source\ttarget_id\ttarget_name\torganism\ttarget_class\tligand_tokens\tsequence_tokens\tstructure_tokens\tevidence\n",
    )
    .expect("empty known index");
    let sqlite = store.join("downloads/chembl/chembl_query.sqlite");
    write_published_chembl_query_sqlite(&sqlite).expect("published ChEMBL query index");
    let mut activities = Vec::new();
    for index in 0..205 {
        activities.push((
            format!("CHEMBL_CCO_{index:03}"),
            "CCO".to_string(),
            "=".to_string(),
            20.0 + index as f64,
        ));
        activities.push((
            format!("CHEMBL_CCN_{index:03}"),
            "CCN".to_string(),
            "=".to_string(),
            20.0 + index as f64,
        ));
    }
    add_published_chembl_activity_rows(&sqlite, &activities).expect("bulk ChEMBL activities");
    write_test_source_manifest(&store, &["chembl"]).expect("ChEMBL source manifest");
    write_text(
        &input,
        "{\"inputs\":[{\"field\":\"small_molecule\",\"value\":\"CCO\"},{\"field\":\"small_molecule\",\"value\":\"CCN\"}],\"files\":[]}\n",
    )
    .expect("input");
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: store,
        input_manifest: input,
        run_dir: run.clone(),
    };

    run_science_command(ScienceCommand::InputNormalize, &options).expect("normalize");
    run_science_command(ScienceCommand::FeaturesCompute, &options).expect("features");
    run_science_command(ScienceCommand::DatabaseMatchKnown, &options).expect("match known");
    let matches = read_text(&run.join("known_database_matches.tsv")).expect("known matches");
    let mut counts = BTreeMap::<String, usize>::new();
    for cols in matches.lines().skip(1).map(split_tsv) {
        if cols.get(1).is_some_and(|source| source == "ChEMBL") {
            *counts.entry(cols[0].clone()).or_default() += 1;
        }
    }

    assert_eq!(counts.len(), 2);
    assert!(counts.values().all(|count| *count >= 205), "{counts:?}");
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn bindingdb_query_aggregation_keeps_all_targets_without_a_200_target_cap() {
    let root = temp_root("bindingdb_per_input_unbounded");
    let store = root.join("science_store");
    let run = root.join("run");
    let input = root.join("input.json");
    write_text(
        &store.join("indexes/known_targets.tsv"),
        "source\ttarget_id\ttarget_name\torganism\ttarget_class\tligand_tokens\tsequence_tokens\tstructure_tokens\tevidence\n",
    )
    .expect("empty known index");
    let header = "Ligand SMILES\tBindingDB Ligand Name\tTarget Name\tTarget Source Organism According to Curator or DataSource\tKi (nM)\tUniProt (SwissProt) Primary ID of Target Chain 1\n";
    let mut first = header.to_string();
    let mut second = header.to_string();
    for index in 0..205 {
        let target = if index < 110 { &mut first } else { &mut second };
        target.push_str(&format!(
            "CCO\tethanol\tCCO target {index}\tHomo sapiens\t12\tP_CCO_{index:03}\n"
        ));
        target.push_str(&format!(
            "CCN\tethylamine\tCCN target {index}\tHomo sapiens\t12\tP_CCN_{index:03}\n"
        ));
    }
    write_zip_members(
        &store.join("downloads/bindingdb/bindingdb_all.tsv.zip"),
        &[
            ("z_second.tsv", second.as_str()),
            ("a_first.tsv", first.as_str()),
        ],
    )
    .expect("bindingdb zip");
    write_test_source_manifest(&store, &["bindingdb"]).expect("bindingdb source manifest");
    publish_test_store_bindingdb_query(&store).expect("BindingDB query index");
    write_text(
        &input,
        "{\"inputs\":[{\"field\":\"small_molecule\",\"value\":\"CCO\"},{\"field\":\"small_molecule\",\"value\":\"CCN\"}],\"files\":[]}\n",
    )
    .expect("input");
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: store,
        input_manifest: input,
        run_dir: run.clone(),
    };

    run_science_command(ScienceCommand::InputNormalize, &options).expect("normalize");
    run_science_command(ScienceCommand::FeaturesCompute, &options).expect("features");
    run_science_command(ScienceCommand::DatabaseMatchKnown, &options).expect("match known");
    let matches = read_text(&run.join("known_database_matches.tsv")).expect("known matches");
    let mut counts = BTreeMap::<String, usize>::new();
    for cols in matches.lines().skip(1).map(split_tsv) {
        if cols.get(1).is_some_and(|source| source == "BindingDB") {
            *counts.entry(cols[0].clone()).or_default() += 1;
        }
    }

    assert_eq!(counts.len(), 2);
    assert!(counts.values().all(|count| *count == 205), "{counts:?}");
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn known_matching_deduplicates_onsite_hits_and_downweights_token_noise() {
    let root = temp_root("bindingdb_dedupe_noise");
    let store = root.join("science_store");
    let run = root.join("run");
    let input = root.join("input.json");
    ensure_dir(&store.join("indexes")).expect("indexes");
    write_text(
        &store.join("indexes/known_targets.tsv"),
        "source\ttarget_id\ttarget_name\torganism\ttarget_class\tligand_tokens\tsequence_tokens\tstructure_tokens\tevidence\nChEMBL\tCHEMBL_NOISE\tToken-only noise target\tHomo sapiens\tknown_human_protein\tgeneric\t\t\tchembl_activity_row\n",
    )
    .expect("known index");
    write_zip(
        &store.join("downloads/bindingdb/bindingdb_all.tsv.zip"),
        "BindingDB_All.tsv",
        "Ligand SMILES\tBindingDB Ligand Name\tTarget Name\tTarget Source Organism According to Curator or DataSource\tKi (nM)\tPDB ID(s) for Ligand-Target Complex\tLigand HET ID in PDB\tUniProt (SwissProt) Primary ID of Target Chain 1\tBindingDB MonomerID\nCC(=O)Nc1nnc(s1)S(=O)(=O)N\tAcetazolamide, AZM\tCarbonic anhydrase 2\tHomo sapiens\t900\t2H4N\tAZM\tP00918\t10880\nCC(=O)Nc1nnc(s1)S(=O)(=O)N\tAcetazolamide, AZM\tCarbonic anhydrase 2\tHomo sapiens\t12\t2H4N\tAZM\tP00918\t10880\n",
    )
    .expect("bindingdb zip");
    write_test_source_manifest(&store, &["bindingdb", "chembl"]).expect("known source manifest");
    publish_test_store_bindingdb_query(&store).expect("BindingDB query index");
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: store,
        input_manifest: input,
        run_dir: run.clone(),
    };

    normalize_and_compute_fixture(&options, "small_molecule", "CC(=O)Nc1nnc(s1)S(=O)(=O)N")
        .expect("upstream manifests");
    write_text(
        &run.join("feature_manifest.tsv"),
        "input_id\tinput_kind\tfeature_family\tfeature_name\tfeature_value\tfeature_source\tfeature_token\trecord_id\ninput_1\tsmall_molecule\tsmall_molecule\tligand_fingerprint_token\tgeneric\tGenoLigand\tgeneric\trecord_1\n",
    )
    .expect("features");
    run_science_command(ScienceCommand::DatabaseMatchKnown, &options).expect("match known");
    let matches = read_text(&run.join("known_database_matches.tsv")).expect("known matches");
    write_empty_optional_rank_artifacts(&run).expect("empty optional rank artifacts");
    rank_targets(&options).expect("rank");
    let ranking = read_text(&run.join("target_candidate_ranking.tsv")).expect("ranking");
    let first_candidate = ranking.lines().nth(1).unwrap_or_default();

    assert!(matches.contains("CHEMBL_NOISE"));
    assert!(matches.contains(
        "CHEMBL_NOISE\tToken-only noise target\tHomo sapiens\tknown_human_protein\t0.2000"
    ));
    assert_eq!(matches.matches("\tP00918\t").count(), 1);
    assert!(matches.contains("Ki (nM)=12"));
    assert!(first_candidate.contains("target_P00918"));
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn bindingdb_complex_pdb_parser_ignores_target_chain_structures() {
    let evidence = "ligand_code=AZM;pdb_ids=3HS4,2H4N;target_chain_pdb_ids=1YM3,2A5V";
    let ids = bindingdb_complex_pdb_ids(evidence);
    assert_eq!(ids, vec!["2H4N".to_string(), "3HS4".to_string()]);

    let chain_only = bindingdb_complex_pdb_ids("pdb_ids=;target_chain_pdb_ids=1YM3,2A5V");
    assert!(chain_only.is_empty());

    assert_eq!(
        candidate_pdb_ids("P00918", "", evidence),
        vec![
            "1YM3".to_string(),
            "2A5V".to_string(),
            "2H4N".to_string(),
            "3HS4".to_string(),
        ]
    );
    let summary = candidate_evidence_summary("BindingDB", evidence);
    assert!(summary.contains("pdb_ids=3HS4,2H4N"));
    assert!(summary.contains("target_chain_pdb_ids=1YM3,2A5V"));
}

#[test]
fn legacy_structure_match_schema_is_migrated_without_changing_identity() {
    let root = temp_root("legacy_structure_match_schema");
    let run = root.join("run");
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: root.join("science_store"),
        input_manifest: root.join("input.json"),
        run_dir: run.clone(),
    };
    write_text(
        &run.join("structure_interaction_matches.tsv"),
        "input_id\tsource\ttarget_id\ttarget_name\torganism\ttarget_class\tmatch_score\tmatched_features\tevidence\trecord_id\ninput_1\tBindingDB\tQ1\tTarget 1\tHomo sapiens\tknown_human_protein\t1.0000\tcoordinate\tbound evidence\trecord_1\ninput_2\tBindingDB\tQ2\tTarget 2\tHomo sapiens\tknown_human_protein\t1.0000\tcoordinate\tbound evidence\tBindingDB\tinput_2\trecord_2\n",
    )
    .expect("legacy structure matches");

    assert!(normalize_legacy_structure_match_schema(&options).expect("migration"));
    let text = read_text(&run.join("structure_interaction_matches.tsv")).expect("matches");
    let rows = text.lines().map(split_tsv).collect::<Vec<_>>();
    assert!(rows.iter().all(|row| row.len() == 12));
    assert_eq!(rows[1][9], "BindingDB");
    assert_eq!(rows[1][10], "input_1");
    assert_eq!(rows[1][11], "record_1");
    assert_eq!(rows[2][9], "BindingDB");
    assert_eq!(rows[2][10], "input_2");
    assert_eq!(rows[2][11], "record_2");
    assert!(!normalize_legacy_structure_match_schema(&options).expect("idempotent migration"));
    fs::remove_dir_all(&root).expect("cleanup temp root");
}

#[test]
fn structure_matching_uses_ligand_identity_from_known_search() {
    let root = temp_root("structure_uses_known_ligand_identity");
    install_not_applicable_worker_fixtures(&root).expect("worker fixtures");
    let store = root.join("science_store");
    write_test_source_manifest(&store, &["rcsb_ligand_bound"]).expect("source manifest");
    write_test_rcsb_on_demand_policy(&store);
    let cached = rcsb_coordinate_cache_path(&store, "2H4N");
    write_gzip(&cached, &test_rcsb_mmcif("2H4N")).expect("cached coordinate");
    let run = root.join("run");
    let input = root.join("input.json");
    ensure_dir(&store.join("indexes")).expect("indexes");
    write_text(
        &run.join("known_database_matches.tsv"),
        "input_id\tsource\ttarget_id\ttarget_name\torganism\ttarget_class\tmatch_score\tmatched_features\tevidence\ninput_1\tBindingDB\tP00918\tCarbonic anhydrase 2\tHomo sapiens\tknown_human_protein\t1.0000\tligand_identity\tbindingdb_onsite_activity_row;ligand_code=AZM;pdb_ids=3HS4,1AZM,2H4N\n",
    )
    .expect("known matches");
    write_text(
        &store.join("indexes/structure_targets.tsv"),
        "source\ttarget_id\ttarget_name\torganism\ttarget_class\tligand_tokens\tsequence_tokens\tstructure_tokens\tevidence\nRCSB\t2H4N\tH94N Carbonic anhydrase II complexed with acetazolamide\tHomo sapiens\tstructure_record\tazm\t2h4 h4n\tace taz azm car bon\tmolecule_coordinate_row\n",
    )
    .expect("structure index");
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: store,
        input_manifest: input,
        run_dir: run.clone(),
    };

    normalize_and_compute_fixture(&options, "small_molecule", "CC(=O)Nc1nnc(s1)S(=O)(=O)N")
        .expect("upstream manifests");
    write_text(
        &run.join("feature_manifest.tsv"),
        "input_id\tinput_kind\tfeature_family\tfeature_name\tfeature_value\tfeature_source\tfeature_token\trecord_id\ninput_1\tsmall_molecule\tsmall_molecule\tligand_fingerprint_token\tcc\tGenoLigand\tcc\trecord_1\n",
    )
    .expect("features");
    run_science_command(ScienceCommand::StructureEvaluate, &options).expect("structure");
    let matches = read_text(&run.join("structure_interaction_matches.tsv")).expect("matches");
    let columns = matches.lines().map(split_tsv).collect::<Vec<_>>();

    assert!(matches.contains("2H4N"));
    assert!(matches.contains("azm"));
    assert!(columns.iter().all(|row| row.len() == 12));
    assert_eq!(columns[0][9], "source_database_id");
    assert_eq!(columns[0][10], "current_run_query_id");
    assert_eq!(columns[1][11], "record_1");
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn structure_matching_bounds_candidates_and_rejects_short_or_generic_known_noise() {
    let root = temp_root("structure_candidate_bound_and_noise");
    let store = root.join("science_store");
    let run = root.join("run");
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: store.clone(),
        input_manifest: root.join("input.json"),
        run_dir: run.clone(),
    };
    ensure_dir(&store.join("indexes")).expect("indexes");
    write_test_source_manifest(&store, &["rcsb_ligand_bound"]).expect("source manifest");
    normalize_and_compute_fixture(&options, "small_molecule", "NOS").expect("upstream manifests");
    write_text(
        &run.join("feature_manifest.tsv"),
        "input_id\tinput_kind\tfeature_family\tfeature_name\tfeature_value\tfeature_source\tfeature_token\trecord_id\ninput_1\tsmall_molecule\tligand\tatom\tn\tGenoLigand\tn\trecord_1\ninput_1\tsmall_molecule\tligand\tatom\to\tGenoLigand\to\trecord_1\ninput_1\tsmall_molecule\tligand\tatom\ts\tGenoLigand\ts\trecord_1\n",
    )
    .expect("features");
    let mut known = String::from(
        "input_id\tsource\ttarget_id\ttarget_name\torganism\ttarget_class\tmatch_score\tmatched_features\tevidence\tsource_row_provenance\tactivity_value\tligand_identity\tcurrent_run_query_id\trecord_id\n",
    );
    known.push_str(
        "input_1\tBindingDB\tP00918\tCarbonic anhydrase 2\tHomo sapiens\tknown_human_protein\t1.0000\tligand_code=AZM;pdb_ids=2H4N\tbindingdb_onsite_activity_row;ligand_code=AZM;pdb_ids=2H4N\tbindingdb_onsite_activity_row\tKi (nM)=12\tligand_name=Acetazolamide;ligand_code=AZM\tquery_1\trecord_1\n",
    );
    for row in 0..600usize {
        known.push_str(&format!(
            "input_1\tChEMBL\tCHEMBL_NOISE_{row}\tGeneric target provenance {row}\tunknown\tprotein\t0.2000\tgeneric known target\tchembl_activity_row;provenance=generic target\tchembl_source\t\t\tquery_{row}\trecord_1\n"
        ));
    }
    write_text(&run.join("known_database_matches.tsv"), &known).expect("known matches");
    let mut index = String::from(
        "source\ttarget_id\ttarget_name\torganism\ttarget_class\tligand_tokens\tsequence_tokens\tstructure_tokens\tevidence\n",
    );
    for row in 0..700usize {
        index.push_str(&format!(
            "RCSB\tNOISE_{row:04}\tGeneric noise structure {row}\tunknown\tstructure_record\tn o s\t\tgen ene ner eri ric\trcsb_structure_file\n"
        ));
    }
    index.push_str(
        "RCSB\t2H4N\tCarbonic anhydrase II with AZM\tHomo sapiens\tstructure_record\tazm\t\t2h4 h4n\trcsb_structure_file\n",
    );
    for row in 0..399usize {
        index.push_str(&format!(
            "RCSB\tZ_AZM_{row:04}\tAZM structure {row}\tunknown\tstructure_record\tazm\t\t\trcsb_structure_file\n"
        ));
    }
    write_text(&store.join("indexes/structure_targets.tsv"), &index).expect("structure index");

    let first = match_structure_interactions(&options).expect("first structure match");
    let first_output =
        read_text(&run.join("structure_interaction_matches.tsv")).expect("first output");
    let second = match_structure_interactions(&options).expect("second structure match");
    let second_output =
        read_text(&run.join("structure_interaction_matches.tsv")).expect("second output");

    assert_eq!(first.len(), 256);
    assert_eq!(second.len(), 256);
    assert_eq!(first_output, second_output);
    assert_eq!(first_output.lines().count(), 257);
    let identity_hit = first_output
        .lines()
        .find(|line| line.split('\t').nth(2) == Some("2H4N"))
        .expect("2H4N identity hit retained");
    let identity_tokens = identity_hit
        .split('\t')
        .nth(7)
        .unwrap_or_default()
        .split_whitespace()
        .collect::<BTreeSet<_>>();
    assert_eq!(identity_tokens, BTreeSet::from(["2h4", "azm", "h4n"]));
    assert!(!first_output.contains("NOISE_"));
    assert!(!first_output.contains("\tn\t"));
    assert!(!first_output.contains("\to\t"));
    assert!(!first_output.contains("\ts\t"));
    assert!(!first_output.contains("gen"));
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn webui_field_names_normalize_to_all_supported_input_types() {
    let root = temp_root("webui_fields");
    let input = root.join("input.json");
    write_text(
            &input,
            r#"{"inputs":[
{"field":"small_molecule","value":"CCO"},
{"field":"peptide","value":"ACDEK"},
{"field":"protein_ligand","value":"MKTAYIAKQRQISFVKSHFSRQDILDLWQMKTAYIAKQRQISFVKSHFSRQDILDLWQMKTAYIAKQRQISFVKSHFSRQDILDLWQ"},
{"field":"complex","value":"complex.pdb chain A ligand ATP"},
{"field":"genome_or_metagenome","value":"ATGCGCTAA"}
],"files":[]}"#,
        )
        .expect("write input");

    let inputs =
        extract_inputs(&science_options(&root, &input, &root.join("run"))).expect("extract inputs");
    let kinds = inputs
        .iter()
        .map(|input| input.kind.as_str())
        .collect::<BTreeSet<_>>();

    assert_eq!(inputs.len(), 5);
    for expected in [
        "small_molecule",
        "peptide",
        "protein_ligand",
        "complex",
        "genome_or_metagenome",
    ] {
        assert!(kinds.contains(expected), "missing input kind: {expected}");
    }
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn structure_and_dynamics_reject_unbound_tool_outputs_from_prior_runs() {
    let root = temp_root("tool_outputs");
    install_not_applicable_worker_fixtures(&root).expect("worker fixtures");
    let run = root.join("run");
    let store = create_temporary_test_store(&root).expect("store");
    let input = root.join("input.json");
    write_text(
        &input,
        "{\"inputs\":[{\"field\":\"small_molecule\",\"value\":\"CCO\"}],\"files\":[]}\n",
    )
    .expect("input");
    write_text(
            &run.join("tool_outputs/structure_patch_summary.tsv"),
            "candidate_id\tstructure_score\tstructure_evidence\tbinding_patch\tpatch_evidence\tkey_residues\tdocking_score\tdocking_pose\tligand_position\tinteraction_residues\ncandidate_1\t0.9\tworker_structure\tpatch_1\tworker_patch\tHIS57\t\t\t\tHIS57\n",
        )
        .expect("structure output");
    write_text(
            &run.join("tool_outputs/docking_summary.tsv"),
            "candidate_id\tdocking_status\tdocking_score\tdocking_engine\tnotes\ncandidate_1\tcompleted\t-7.1\tvina\tworker_pose\n",
        )
        .expect("docking output");
    write_text(
            &run.join("tool_outputs/md_summary.tsv"),
            "candidate_id\trmsd\trmsf\tligand_drift\thydrogen_bond_occupancy\tcontact_occupancy\tinteraction_persistence\tmd_risk_flags\tmd_score_decomposition\tmd_stability_score\ncandidate_1\t1.2\t0.4\t0.6\t0.72\t0.81\t0.77\tstable\tworker_md\t0.8\n",
        )
        .expect("md output");
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: store,
        input_manifest: input,
        run_dir: run.clone(),
    };
    run_science_command(ScienceCommand::InputNormalize, &options).expect("normalize");
    run_science_command(ScienceCommand::FeaturesCompute, &options).expect("features");
    let invocation_before_structure =
        read_text(&run.join(WORKER_INVOCATION_FILE)).expect("worker invocation");
    let structure =
        run_science_command(ScienceCommand::StructureEvaluate, &options).expect("structure");
    assert_eq!(
        read_text(&run.join(WORKER_INVOCATION_FILE)).expect("worker invocation after structure"),
        invocation_before_structure
    );
    assert!(
        read_text(&run.join("tool_outputs/docking_summary.tsv"))
            .expect("preserved docking output")
            .contains("worker_pose")
    );
    let dynamics = run_science_command(ScienceCommand::DynamicsScreen, &options).expect("dynamics");
    let structure_text =
        read_text(&run.join("structure_patch_summary.tsv")).expect("structure summary");
    let docking_text = read_text(&run.join("docking_summary.tsv")).expect("docking summary");
    let md_text = read_text(&run.join("md_summary.tsv")).expect("md summary");

    assert!(structure.contains("structure_artifact=ready"));
    assert!(!structure.contains("docking_artifact="));
    assert!(
        dynamics.contains("md_artifact=not_ready"),
        "unexpected dynamics result: {dynamics}"
    );
    assert!(structure_text.contains("worker_structure"));
    assert!(!docking_text.contains("worker_pose"));
    assert!(!md_text.contains("worker_md"));
    assert_eq!(docking_text.lines().count(), 1);
    assert_eq!(md_text.lines().count(), 1);
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn md_summary_score_participates_in_ranking_and_report() {
    let root = temp_root("md_score_ranking");
    let run = root.join("run");
    let store = create_temporary_test_store(&root).expect("store");
    let input = root.join("input.json");
    write_text(
        &input,
        "{\"inputs\":[{\"field\":\"small_molecule\",\"value\":\"CCO\"}],\"files\":[]}\n",
    )
    .expect("input");
    write_text(
            &run.join("known_database_matches.tsv"),
            "input_id\tsource\ttarget_id\ttarget_name\torganism\ttarget_class\tmatch_score\tmatched_features\tevidence\ninput_1\tChEMBL\tT1\tTarget 1\tHomo sapiens\tenzyme\t1.0\tcco\tchembl_activity_row\n",
        )
        .expect("known match");
    write_md_binding_manifests(&run, "target_T1", "input_1", "CCO").expect("binding manifests");
    write_text(
        &run.join("tool_outputs/md_summary.tsv"),
        "candidate_id\trmsd\trmsf\tligand_drift\thydrogen_bond_occupancy\tcontact_occupancy\tinteraction_persistence\tmd_risk_flags\tmd_score_decomposition\tmd_stability_score\ntarget_T1\t1.2\t0.4\t0.6\t0.72\t0.81\t0.77\tstable\tworker_md\t0.8\n",
    )
    .expect("md summary");
    write_text(
        &run.join("tool_outputs/complex_md_manifest.json"),
        r#"{"candidate_id":"target_T1","method":"openmm_receptor_ligand_complex_dynamics","platform":"CUDA","receptor_coordinate":"structure_viewer.cif","ligand_coordinate":"tool_outputs/ligand_placement.tsv","trajectory":"tool_outputs/md_trajectory.tsv","protocol_artifact":"tool_outputs/md_protocol.tsv","state_artifact":"tool_outputs/md_state.tsv","gpu_telemetry_artifact":"tool_outputs/gpu_telemetry.tsv","gpu_telemetry_status_artifact":"tool_outputs/gpu_telemetry_status.tsv","gpu_observed":true,"metric_artifacts":["tool_outputs/md_summary.tsv"]}"#,
    )
    .expect("complex md manifest");
    write_text(
        &run.join("tool_outputs/md_run_manifest.json"),
        r#"{"candidate_id":"target_T1","method":"openmm_receptor_ligand_complex_dynamics","platform":"CUDA","receptor_coordinate":"structure_viewer.cif","ligand_coordinate":"tool_outputs/ligand_placement.tsv","complex_manifest":"tool_outputs/complex_md_manifest.json","protocol_artifact":"tool_outputs/md_protocol.tsv","state_artifact":"tool_outputs/md_state.tsv","gpu_telemetry_artifact":"tool_outputs/gpu_telemetry.tsv","gpu_telemetry_status_artifact":"tool_outputs/gpu_telemetry_status.tsv","gpu_observed":true,"trajectory":"tool_outputs/md_trajectory.tsv"}"#,
    )
    .expect("md run manifest");
    write_text(&run.join("structure_viewer.cif"), "data_target_T1\n").expect("receptor");
    write_text(
        &run.join("tool_outputs/ligand_placement.tsv"),
        "candidate_id\tplacement_method\ntarget_T1\tdocking_pose_sdf\n",
    )
    .expect("ligand placement");
    write_text(
        &run.join("tool_outputs/md_trajectory.tsv"),
        "candidate_id\ttime_ps\trmsd\ntarget_T1\t0\t0.0\n",
    )
    .expect("trajectory");
    write_text(
        &run.join("tool_outputs/md_protocol.tsv"),
        "candidate_id\tinput_id\tstage\tparameter\tvalue\tunit\tsource\ntarget_T1\tinput_1\tproduction\ttotal_steps\t2000\tsteps\tOpenMM\n",
    )
    .expect("protocol");
    write_text(
        &run.join("tool_outputs/md_state.tsv"),
        "candidate_id\tstep\ttime_ps\tpotential_energy_kj_mol\ttemperature_estimate_K\ntarget_T1\t0\t0\t-10\t300\n",
    )
    .expect("state");
    write_text(
        &run.join("tool_outputs/gpu_telemetry.tsv"),
        "sample_label\ttimestamp\tgpu_index\tname\tutilization_gpu_percent\tmemory_used_mb\tmemory_total_mb\tpower_draw_w\tcurrent_pid_observed\tcompute_process_count\tcompute_process_memory_used_mb\tfailure_reason\nproduction\t2026/07/02 00:00:00\t0\tNVIDIA\t22.0\t512.0\t8192.0\t65.5\ttrue\t1\t512.0\t\n",
    )
    .expect("gpu telemetry");
    write_text(
        &run.join("tool_outputs/gpu_telemetry_status.tsv"),
        "cuda_platform\tnvidia_smi_available\tgpu_observed\tmax_utilization_gpu_percent\tmax_memory_delta_mb\tprocess_observed\tsample_count\tfailure_reason\nCUDA\ttrue\ttrue\t22.0\t128.0\ttrue\t1\t\n",
    )
    .expect("gpu status");
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: store,
        input_manifest: input,
        run_dir: run.clone(),
    };
    let candidate_id = prepare_bound_rank_fixture(
        &options,
        "small_molecule",
        "CCO",
        "ChEMBL",
        "T1",
        "Target 1",
        "chembl_activity_row",
        1.0,
    )
    .expect("bound ranking fixture");
    let rows = write_indexed_fixture_selection(
        &options,
        &[(
            candidate_id.as_str(),
            "input_1",
            "record_1",
            "small_molecule",
            "CCO",
        )],
    )
    .expect("indexed selection");
    let docking_fixture =
        write_indexed_docking_fixture(&options, &rows[0]).expect("indexed docking fixture");
    let docking =
        read_text(&run.join("tool_outputs/docking_summary.tsv")).expect("indexed docking summary");
    write_text(&run.join("docking_summary.tsv"), &docking).expect("publish docking summary");
    let md_fixture =
        write_indexed_md_fixture(&options, &rows[0], false).expect("indexed MD fixture");
    materialize_indexed_md_summary(&options).expect("publish indexed MD summary");
    let invocation_id = read_text(&run.join(PIPELINE_INVOCATION_FILE))
        .expect("pipeline invocation")
        .trim()
        .to_string();
    write_text(&run.join("run_state.txt"), "failed\n").expect("failed state");
    write_text(&run.join("current_stage.txt"), "target_rank\n").expect("failed stage");
    write_text(&run.join("last_successful_stage.txt"), "dynamics_screen\n")
        .expect("last successful stage");
    write_text(
        &run.join("failure_reason.txt"),
        &format!(
            "schema_version=genoligand_pipeline_failure_v1\nstate=failed\nerror_code=authenticity_gate_failed\nstage=target_rank\ninvocation_id={invocation_id}\nlast_successful_stage=dynamics_screen\nreason=authenticity_gate_failed:fixture\n"
        ),
    )
    .expect("failure reason");
    let preserved = [
        run.join(&docking_fixture.manifest),
        run.join(&md_fixture.manifest),
        run.join("tool_outputs/docking_manifest_index.json"),
        run.join("tool_outputs/md_manifest_index.json"),
    ]
    .map(|path| {
        let digest = sha256_file_hex(&path).expect("preserved artifact digest");
        (path, digest)
    });

    let finalized = finalize_pipeline(&options).expect("pipeline finalize");

    let ranking = read_text(&run.join("target_candidate_ranking.tsv")).expect("ranking");
    let detail = read_text(&run.join("candidate_detail.json")).expect("detail");
    let evidence = read_text(&run.join("evidence_ledger.tsv")).expect("evidence");
    let score = read_text(&run.join("score_decomposition.tsv")).expect("score");
    let report = read_text(&run.join("research_report.md")).expect("report");
    assert!(finalized.contains(&format!("pipeline_invocation_id={invocation_id}")));
    assert_eq!(
        read_text(&run.join(PIPELINE_INVOCATION_FILE))
            .expect("final invocation")
            .trim(),
        invocation_id
    );
    assert_eq!(
        read_text(&run.join("run_state.txt"))
            .expect("final state")
            .trim(),
        "completed"
    );
    assert!(!run.join("failure_reason.txt").exists());
    for (path, digest) in preserved {
        assert_eq!(sha256_file_hex(&path).as_deref(), Some(digest.as_str()));
    }
    assert!(ranking.contains(&candidate_id));
    assert!(ranking.contains("0.6933\t1.0000\t0.0000\t0.0000\t0.6667\t0.7167"));
    assert!(ranking.contains(
        "\tsmall_molecule\tsmall_molecule_route_v1\t0.4500\t0.0000\t0.2000\t0.1500\t0.2000"
    ));
    for fact in [
        "ChEMBL:chembl_activity_row",
        "source_id=chembl",
        "Docking:vina_completed",
        "MD:manifest_bound_stability_score",
    ] {
        assert!(ranking.contains(fact), "ranking missing {fact}");
    }
    assert!(detail.contains("\"md\":0.7167"));
    assert!(detail.contains("\"input_kind\":\"small_molecule\""));
    assert!(detail.contains("\"ranking_policy\":\"small_molecule_route_v1\""));
    assert!(detail.contains("\"metagenome\":0.0"));
    assert!(score.contains("\tsmall_molecule\tsmall_molecule_route_v1"));
    assert!(detail.contains("tool_outputs/md/"));
    assert!(detail.contains("md_run_manifest.json"));
    assert!(detail.contains("complex_md_manifest.json"));
    assert!(detail.contains("ligand_placement.tsv"));
    assert!(evidence.contains("final-score contribution follows each candidate ranking_policy"));
    assert!(evidence.contains("1 rows include md_score from current run MD Summary"));
    assert!(score.contains(&format!(
        "{candidate_id}\tmd_score_contribution\t0.1433\tGenoLigand"
    )));
    assert!(report.contains("md_stability_score"));
    assert!(report.contains("input_kind"));
    assert!(report.contains("md_weight"));
    let source_rows =
        read_simple_tsv_rows(&run.join("artifact_source_map.tsv")).expect("source-map rows");
    let ranking_source = source_rows
        .iter()
        .find(|row| row.get("artifact").map(String::as_str) == Some("target_candidate_ranking.tsv"))
        .expect("ranking source-map row");
    assert_eq!(
        ranking_source.get("upstream_source").map(String::as_str),
        Some(
            "input_conversion_manifest.tsv;known_database_matches.tsv;metagenome_potential_matches.tsv;structure_interaction_matches.tsv;docking_summary.tsv;md_summary.tsv"
        )
    );
    assert_eq!(
        ranking_source.get("downstream_use").map(String::as_str),
        Some("candidate_detail.json;score_decomposition.tsv")
    );
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn ranking_policy_digest_is_bound_to_context_decomposition_and_report() {
    let root = temp_root("ranking_policy_binding");
    let run = root.join("run");
    let store = create_temporary_test_store(&root).expect("store");
    let input = root.join("input.json");
    write_text(&input, "{\"inputs\":[],\"files\":[]}\n").expect("input");
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: store,
        input_manifest: input,
        run_dir: run.clone(),
    };
    write_text(
        &run.join("target_candidate_ranking.tsv"),
        "rank\tcandidate_id\ttarget_id\ttarget_name\tcandidate_scope\torganism\tfinal_score\tknown_database_score\tmetagenome_score\tstructure_score\tdocking_score\tmd_score\tevidence_summary\tstatus\tinput_id\trecord_id\tidentity_namespace\tinput_kind\tranking_policy\tknown_database_weight\tmetagenome_weight\tstructure_weight\tdocking_weight\tmd_weight\n1\tcandidate_policy\tT1\tTarget 1\tknown_database\tHomo sapiens\t0.3350\t0.4000\t0.3000\t0.2000\t0.1000\t0.5000\tpolicy fixture\tPASS\tinput_1\trecord_1\tuniprot\tsmall_molecule\tsmall_molecule_route_v1\t0.4500\t0.0000\t0.2000\t0.1500\t0.2000\n",
    )
    .expect("ranking fixture");

    let expected_policy = json!({
        "schema_version": "genoligand_target_ranking_v3",
        "score_semantics": "input_kind_route_weighted_sum",
        "shortlist_score_semantics": "route_weights_renormalized_over_known_metagenome_structure",
        "routes": {
            "small_molecule": {"ranking_policy":"small_molecule_route_v1","known_database_weight":0.45,"metagenome_weight":0.0,"structure_weight":0.20,"docking_weight":0.15,"md_weight":0.20,"preliminary_known_database_weight":0.6923076923076923,"preliminary_metagenome_weight":0.0,"preliminary_structure_weight":0.3076923076923077},
            "peptide": {"ranking_policy":"peptide_route_v2","known_database_weight":0.30,"metagenome_weight":0.20,"structure_weight":0.50,"docking_weight":0.0,"md_weight":0.0,"preliminary_known_database_weight":0.30,"preliminary_metagenome_weight":0.20,"preliminary_structure_weight":0.50},
            "protein_ligand": {"ranking_policy":"protein_ligand_route_v2","known_database_weight":0.20,"metagenome_weight":0.10,"structure_weight":0.70,"docking_weight":0.0,"md_weight":0.0,"preliminary_known_database_weight":0.20,"preliminary_metagenome_weight":0.10,"preliminary_structure_weight":0.70},
            "complex": {"ranking_policy":"complex_route_v2","known_database_weight":0.15,"metagenome_weight":0.05,"structure_weight":0.80,"docking_weight":0.0,"md_weight":0.0,"preliminary_known_database_weight":0.15,"preliminary_metagenome_weight":0.05,"preliminary_structure_weight":0.80},
            "genome_or_metagenome": {"ranking_policy":"genome_or_metagenome_route_v1","known_database_weight":0.45,"metagenome_weight":0.55,"structure_weight":0.0,"docking_weight":0.0,"md_weight":0.0,"preliminary_known_database_weight":0.45,"preliminary_metagenome_weight":0.55,"preliminary_structure_weight":0.0},
            "fallback": {"ranking_policy":"fallback_route_v2","known_database_weight":0.50,"metagenome_weight":0.30,"structure_weight":0.20,"docking_weight":0.0,"md_weight":0.0,"preliminary_known_database_weight":0.50,"preliminary_metagenome_weight":0.30,"preliminary_structure_weight":0.20},
        },
    });
    let expected_digest = canonical_json_sha256(&expected_policy);
    assert_eq!(ranking_policy_value(), expected_policy);
    assert_eq!(ranking_policy_digest(), expected_digest);

    write_run_context(&options, "completed", &[]).expect("run context");
    write_score_decomposition(&options).expect("decomposition");
    let context: Value =
        serde_json::from_str(&read_text(&run.join("run_context.json")).expect("run context text"))
            .expect("run context json");
    assert_eq!(context["ranking_policy"], expected_policy);
    assert_eq!(context["ranking_policy_digest"], expected_digest);

    let decomposition =
        read_text(&run.join("score_decomposition.tsv")).expect("decomposition text");
    let rows = decomposition.lines().skip(1).collect::<Vec<_>>();
    assert!(!rows.is_empty());
    assert!(rows.iter().all(|row| {
        let columns = row.split('\t').collect::<Vec<_>>();
        columns.get(4) == Some(&RANKING_POLICY_VERSION)
            && columns.get(5).copied() == Some(expected_digest.as_str())
            && columns.get(6) == Some(&"small_molecule")
            && columns.get(7) == Some(&"small_molecule_route_v1")
    }));
    for expected in [
        "known_database_score_weight\t0.4500",
        "metagenome_score_weight\t0.0000",
        "structure_score_weight\t0.2000",
        "docking_score_weight\t0.1500",
        "md_score_weight\t0.2000",
        "final_score\t0.3350",
    ] {
        assert!(decomposition.contains(expected), "missing {expected}");
    }

    let report = report_source_summary(&options, 1);
    assert!(report.contains(RANKING_POLICY_VERSION));
    assert!(report.contains(&expected_digest));
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn ranking_routes_sum_to_one_and_ignore_inapplicable_channels() {
    for input_kind in [
        "small_molecule",
        "peptide",
        "protein_ligand",
        "complex",
        "genome_or_metagenome",
        "unknown",
    ] {
        let route = ranking_route(input_kind);
        assert!(
            (route.weight_sum() - 1.0).abs() < 1e-9,
            "weights do not sum to one for {input_kind}"
        );
        let (known, metagenome, structure) = route.preliminary_weights();
        assert!(
            (known + metagenome + structure - 1.0).abs() < 1e-9,
            "preliminary weights do not sum to one for {input_kind}"
        );
    }

    let small_molecule = ranking_route("small_molecule");
    assert_eq!(small_molecule.metagenome, 0.0);
    assert_eq!(
        small_molecule.score_components(0.8, 0.0, 0.6, 0.5, 0.4),
        small_molecule.score_components(0.8, 1.0, 0.6, 0.5, 0.4),
    );

    let genome = ranking_route("genome_or_metagenome");
    assert_eq!(
        (genome.structure, genome.docking, genome.md),
        (0.0, 0.0, 0.0)
    );
    assert_eq!(
        genome.score_components(0.7, 0.6, 0.0, 0.0, 0.0),
        genome.score_components(0.7, 0.6, 1.0, 1.0, 1.0),
    );
    for input_kind in ["peptide", "protein_ligand", "complex"] {
        let route = ranking_route(input_kind);
        assert_eq!((route.docking, route.md), (0.0, 0.0));
        assert_eq!(route.score_components(1.0, 1.0, 1.0, 0.0, 0.0), 1.0);
    }
    assert_eq!(
        small_molecule.score_components(1.0, 0.0, 1.0, 1.0, 1.0),
        1.0
    );
    assert_eq!(genome.score_components(1.0, 1.0, 0.0, 0.0, 0.0), 1.0);
    assert!(ranking_route("complex").structure > ranking_route("complex").known);
    assert!(ranking_route("protein_ligand").structure > ranking_route("protein_ligand").known);
}

#[test]
fn target_ranking_orders_by_input_record_and_resets_group_rank() {
    let candidate = |candidate_id: &str, input_id: &str, final_score: f64| Candidate {
        candidate_id: candidate_id.to_string(),
        input_id: input_id.to_string(),
        record_id: "record_1".to_string(),
        input_kind: "small_molecule".to_string(),
        ranking_policy: "small_molecule_route_v1".to_string(),
        identity_namespace: "chembl".to_string(),
        target_id: candidate_id.to_string(),
        target_name: candidate_id.to_string(),
        target_class: "enzyme".to_string(),
        candidate_scope: "known_human_protein".to_string(),
        organism: "Homo sapiens".to_string(),
        sources: BTreeSet::new(),
        pdb_ids: BTreeSet::new(),
        metadata_quality: 0,
        known_score: 0.0,
        metagenome_score: 0.0,
        structure_score: 0.0,
        docking_score: 0.0,
        dynamics_score: 0.0,
        final_score,
        evidence: String::new(),
    };
    let mut candidates = vec![
        candidate("input_2_high", "input_2", 0.95),
        candidate("input_1_low", "input_1", 0.20),
        candidate("input_1_high", "input_1", 0.80),
    ];

    sort_ranked_candidates(&mut candidates);

    assert_eq!(
        candidates
            .iter()
            .map(|candidate| candidate.candidate_id.as_str())
            .collect::<Vec<_>>(),
        ["input_1_high", "input_1_low", "input_2_high"]
    );
    assert_eq!(candidate_group_ranks(&candidates), [1, 2, 1]);
}

#[test]
fn target_rank_rejects_unbound_root_md_summary() {
    let root = temp_root("target_rank_unbound_md");
    let run = root.join("run");
    let store = root.join("science_store");
    write_test_source_manifest(&store, &["chembl"]).expect("source manifest");
    let input = root.join("input.json");
    write_text(
        &input,
        "{\"inputs\":[{\"field\":\"small_molecule\",\"value\":\"CCO\"}],\"files\":[]}\n",
    )
    .expect("input");
    write_text(
        &run.join("known_database_matches.tsv"),
        "input_id\tsource\ttarget_id\ttarget_name\torganism\ttarget_class\tmatch_score\tmatched_features\tevidence\ninput_1\tChEMBL\tT1\tTarget 1\tHomo sapiens\tenzyme\t1.0\tcco\tchembl_activity_row\n",
    )
    .expect("known match");
    write_text(
        &run.join("md_summary.tsv"),
        "candidate_id\trmsd\trmsf\tligand_drift\thydrogen_bond_occupancy\tcontact_occupancy\tinteraction_persistence\tmd_risk_flags\tmd_score_decomposition\tmd_stability_score\ntarget_T1\t1.2\t0.4\t0.6\t0.72\t0.81\t0.77\tstale\tstale_unbound_md\t0.8\n",
    )
    .expect("stale root MD summary");
    for artifact in [
        "structure_viewer.cif",
        "tool_outputs/ligand_placement.tsv",
        "tool_outputs/md_trajectory.tsv",
        "tool_outputs/md_protocol.tsv",
        "tool_outputs/md_state.tsv",
    ] {
        write_text(&run.join(artifact), "current artifact\n").expect("manifest artifact");
    }
    write_text(
        &run.join("tool_outputs/complex_md_manifest.json"),
        r#"{"candidate_id":"target_T1","method":"openmm_receptor_ligand_complex_dynamics","platform":"CPU","receptor_coordinate":"structure_viewer.cif","ligand_coordinate":"tool_outputs/ligand_placement.tsv","trajectory":"tool_outputs/md_trajectory.tsv","protocol_artifact":"tool_outputs/md_protocol.tsv","state_artifact":"tool_outputs/md_state.tsv"}"#,
    )
    .expect("complex MD manifest");
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: store,
        input_manifest: input,
        run_dir: run.clone(),
    };

    prepare_bound_rank_fixture(
        &options,
        "small_molecule",
        "CCO",
        "ChEMBL",
        "T1",
        "Target 1",
        "chembl_activity_row",
        1.0,
    )
    .expect("bound ranking fixture");

    let error = rank_targets(&options).expect_err("unbound MD summary must be rejected");
    let gate = read_text(&run.join("authenticity_gate.tsv")).expect("authenticity gate");
    assert!(error.contains("authenticity_gate_failed"));
    assert!(gate.contains("invalid_md_provenance"));
    assert!(!run.join("target_candidate_ranking.tsv").exists());
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn current_run_artifact_relative_accepts_wsl_path_and_rejects_external_path() {
    if !cfg!(windows) {
        return;
    }
    let root = temp_root("md_wsl_manifest_path");
    let run = root.join("run");
    let receptor = run.join("tool_outputs/receptor.cif");
    let outside = root.join("outside.cif");
    write_text(&receptor, "data_receptor\n").expect("receptor");
    write_text(&outside, "data_outside\n").expect("outside");
    let receptor_wsl =
        windows_path_to_wsl(&receptor.display().to_string()).expect("receptor wsl path");
    let outside_wsl = windows_path_to_wsl(&outside.display().to_string()).expect("outside wsl");

    assert_eq!(
        current_run_artifact_relative(&run, &receptor_wsl),
        Some("tool_outputs/receptor.cif".to_string())
    );
    assert_eq!(current_run_artifact_relative(&run, &outside_wsl), None);
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn target_ranking_summarizes_long_bindingdb_evidence_for_display() {
    let root = temp_root("bindingdb_evidence_summary");
    let run = root.join("run");
    let store = create_temporary_test_store(&root).expect("store");
    let input = root.join("input.json");
    let long_pdb_ids = "1AZM,1DMY,1JD0,1RJ5,1RJ6,1YDA,2CBA,2H4N";
    write_text(
        &input,
        "{\"inputs\":[{\"field\":\"small_molecule\",\"value\":\"NS(=O)(=O)c1nnc(s1)NC(=O)C\"}],\"files\":[]}\n",
    )
    .expect("input");
    write_text(
        &run.join("known_database_matches.tsv"),
        &format!(
            "input_id\tsource\ttarget_id\ttarget_name\torganism\ttarget_class\tmatch_score\tmatched_features\tevidence\ninput_1\tBindingDB\tP00918\tCarbonic anhydrase 2\tHomo sapiens\tknown_human_protein\t1.0000\tligand_identity\tbindingdb_onsite_activity_row;Ki (nM)=0.012000;ligand_code=AZM;pdb_ids={long_pdb_ids};target_chain_pdb_ids=8ZWV,8C0Q,8C0P,7R15,7R14,7R13;pubchem_cid=1986\n"
        ),
    )
    .expect("known matches");
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: store,
        input_manifest: input,
        run_dir: run.clone(),
    };

    prepare_bound_rank_fixture(
        &options,
        "small_molecule",
        "NS(=O)(=O)c1nnc(s1)NC(=O)C",
        "BindingDB",
        "P00918",
        "Carbonic anhydrase 2",
        &format!(
            "bindingdb_onsite_activity_row;Ki (nM)=0.012000;ligand_code=AZM;pdb_ids={long_pdb_ids};target_chain_pdb_ids=8ZWV,8C0Q,8C0P,7R15,7R14,7R13;pubchem_cid=1986"
        ),
        1.0,
    )
    .expect("bound ranking fixture");

    rank_targets(&options).expect("rank");

    let source_matches = read_text(&run.join("known_database_matches.tsv")).expect("matches");
    let ranking = read_text(&run.join("target_candidate_ranking.tsv")).expect("ranking");
    let detail = read_text(&run.join("candidate_detail.json")).expect("detail");
    assert!(source_matches.contains(long_pdb_ids));
    assert!(ranking.contains("pdb_ids=1AZM,1DMY,1JD0,1RJ5,1RJ6(+3 more)"));
    assert!(ranking.contains("target_chain_pdb_ids=8ZWV,8C0Q,8C0P,7R15,7R14(+1 more)"));
    assert!(!ranking.contains("1AZM,1DMY,1JD0,1RJ5,1RJ6,1YDA,2CBA,2H4N"));
    assert!(detail.contains("pdb_ids=1AZM,1DMY,1JD0,1RJ5,1RJ6(+3 more)"));
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn report_limits_candidate_table_to_readable_top_results() {
    let root = temp_root("report_candidate_limit");
    let run = root.join("run");
    let store = create_temporary_test_store(&root).expect("store");
    let input = root.join("input.json");
    write_text(
        &input,
        "{\"inputs\":[{\"field\":\"small_molecule\",\"value\":\"CCO\"},{\"field\":\"small_molecule\",\"value\":\"CCN\"}],\"files\":[]}\n",
    )
    .expect("input");
    let mut ranking = String::from(
        "rank\tcandidate_id\ttarget_id\ttarget_name\tcandidate_scope\torganism\tfinal_score\tknown_database_score\tmetagenome_score\tstructure_score\tdocking_score\tmd_score\tevidence_summary\tstatus\tinput_id\trecord_id\tidentity_namespace\n",
    );
    for input_index in 1..=2 {
        let input_id = format!("input_{input_index}");
        for rank in 1..=60 {
            let target_id = format!("T{input_index}_{rank}");
            let candidate_id = candidate_id_for(&input_id, "record_1", "chembl", &target_id);
            ranking.push_str(&format!(
                "{rank}\t{candidate_id}\t{target_id}\tTarget {input_index}-{rank}\tknown_human_protein\tHomo sapiens\t0.1000\t0.2000\t0.0000\t0.0000\t0.0000\t0.0000\tChEMBL:activity\tcomputed_prediction\t{input_id}\trecord_1\tchembl\n"
            ));
        }
    }
    write_text(&run.join("target_candidate_ranking.tsv"), &ranking).expect("ranking");
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: store,
        input_manifest: input,
        run_dir: run.clone(),
    };

    run_science_command(ScienceCommand::InputNormalize, &options).expect("normalize report inputs");
    run_science_command(ScienceCommand::FeaturesCompute, &options).expect("compute report inputs");
    write_report_support(&options).expect("report support artifacts");

    render_report(&options).expect("report");

    let report = read_text(&run.join("research_report.md")).expect("report");
    assert!(report.contains("共生成 120 个候选靶点"));
    assert!(report.contains("Target 1-50"));
    assert!(report.contains("Target 2-50"));
    assert!(!report.contains("Target 1-51"));
    assert!(!report.contains("Target 2-51"));
    assert!(!report.contains("target_candidate_ranking.tsv"));
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn cuda_complex_md_without_bound_gpu_evidence_is_rejected() {
    let root = temp_root("cuda_md_requires_gpu_observation");
    let run = root.join("run");
    let store = create_temporary_test_store(&root).expect("store");
    let input = root.join("input.json");
    write_text(
        &input,
        "{\"inputs\":[{\"field\":\"small_molecule\",\"value\":\"CCO\"}],\"files\":[]}\n",
    )
    .expect("input");
    write_text(
        &run.join("known_database_matches.tsv"),
        "input_id\tsource\ttarget_id\ttarget_name\torganism\ttarget_class\tmatch_score\tmatched_features\tevidence\ninput_1\tChEMBL\tT1\tTarget 1\tHomo sapiens\tenzyme\t1.0\tcco\tchembl_activity_row\n",
    )
    .expect("known match");
    write_text(
        &run.join("md_summary.tsv"),
        "candidate_id\trmsd\trmsf\tligand_drift\thydrogen_bond_occupancy\tcontact_occupancy\tinteraction_persistence\tmd_risk_flags\tmd_score_decomposition\tmd_stability_score\ntarget_T1\t1.2\t0.4\t0.6\t0.72\t0.81\t0.77\tcuda_unobserved\tworker_md\t0.8\n",
    )
    .expect("md summary");
    write_text(
        &run.join("tool_outputs/complex_md_manifest.json"),
        r#"{"candidate_id":"target_T1","method":"openmm_receptor_ligand_complex_dynamics","platform":"CUDA","receptor_coordinate":"structure_viewer.cif","ligand_coordinate":"tool_outputs/ligand_placement.tsv","trajectory":"tool_outputs/md_trajectory.tsv","protocol_artifact":"tool_outputs/md_protocol.tsv","gpu_telemetry_status_artifact":"tool_outputs/gpu_telemetry_status.tsv","gpu_observed":false,"metric_artifacts":["tool_outputs/md_summary.tsv"]}"#,
    )
    .expect("complex md manifest");
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: store,
        input_manifest: input,
        run_dir: run.clone(),
    };

    prepare_bound_rank_fixture(
        &options,
        "small_molecule",
        "CCO",
        "ChEMBL",
        "T1",
        "Target 1",
        "chembl_activity_row",
        1.0,
    )
    .expect("bound ranking fixture");

    let error = rank_targets(&options).expect_err("unbound CUDA MD must be rejected");
    let gate = read_text(&run.join("authenticity_gate.tsv")).expect("authenticity gate");
    assert!(error.contains("authenticity_gate_failed"));
    assert!(gate.contains("invalid_md_provenance"));
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn md_protocol_and_gpu_telemetry_are_evidence_artifacts() {
    let root = temp_root("md_protocol_gpu_evidence");
    let run = root.join("run");
    let store = create_temporary_test_store(&root).expect("store");
    let input = root.join("input.json");
    write_text(
        &input,
        "{\"inputs\":[{\"field\":\"small_molecule\",\"value\":\"CCO\"}],\"files\":[]}\n",
    )
    .expect("input");
    write_text(
        &run.join("tool_outputs/md_protocol.tsv"),
        "candidate_id\tstage\tparameter\tvalue\ntarget_T1\tproduction\tsteps\t2000\n",
    )
    .expect("protocol");
    write_text(
        &run.join("tool_outputs/gpu_telemetry.tsv"),
        "sample_label\tgpu_index\tname\tutilization_gpu_percent\tmemory_used_mb\tmemory_total_mb\tpower_draw_w\nproduction\t0\tNVIDIA\t22\t512\t8192\t65.5\n",
    )
    .expect("telemetry");
    write_text(
        &run.join("tool_outputs/gpu_telemetry_status.tsv"),
        "cuda_platform\tnvidia_smi_available\tgpu_observed\tmax_utilization_gpu_percent\tmax_memory_delta_mb\tprocess_observed\tfailure_reason\nCUDA\ttrue\ttrue\t22\t128\ttrue\t\n",
    )
    .expect("telemetry status");
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: store,
        input_manifest: input,
        run_dir: run.clone(),
    };

    prepare_bound_rank_fixture(
        &options,
        "small_molecule",
        "CCO",
        "ChEMBL",
        "T1",
        "Target 1",
        "chembl_activity_row",
        1.0,
    )
    .expect("bound evidence fixture");
    rank_targets(&options).expect("rank evidence fixture");

    build_evidence(&options).expect("evidence");

    let source_map = read_text(&run.join("artifact_source_map.tsv")).expect("source map");
    let evidence = read_text(&run.join("evidence_ledger.tsv")).expect("evidence");
    assert!(source_map.contains("tool_outputs/md_protocol.tsv"));
    assert!(source_map.contains("tool_outputs/gpu_telemetry.tsv"));
    assert!(source_map.contains("tool_outputs/gpu_telemetry_status.tsv"));
    assert!(evidence.contains("tool_outputs/md_protocol.tsv"));
    assert!(evidence.contains("tool_outputs/gpu_telemetry.tsv"));
    assert!(evidence.contains("tool_outputs/gpu_telemetry_status.tsv"));
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn ranking_rejects_md_summary_without_complex_manifest() {
    let root = temp_root("md_without_complex_manifest");
    let run = root.join("run");
    let store = create_temporary_test_store(&root).expect("store");
    let input = root.join("input.json");
    write_text(
        &input,
        "{\"inputs\":[{\"field\":\"peptide\",\"value\":\"IKVAV\"}],\"files\":[]}\n",
    )
    .expect("input");
    write_text(
        &run.join("known_database_matches.tsv"),
        "input_id\tsource\ttarget_id\ttarget_name\torganism\ttarget_class\tmatch_score\tmatched_features\tevidence\ninput_1\tChEMBL\tT1\tTarget 1\tHomo sapiens\tenzyme\t1.0\tikvav\tchembl_activity_row\n",
    )
    .expect("known match");
    write_text(
        &run.join("md_summary.tsv"),
        "candidate_id\trmsd\trmsf\tligand_drift\thydrogen_bond_occupancy\tcontact_occupancy\tinteraction_persistence\tmd_risk_flags\tmd_score_decomposition\tmd_stability_score\ntarget_T1\t1.2\t0.4\t0.6\t0.72\t0.81\t0.77\tno_receptor_contact_model;rdkit_peptide_conformer_ensemble\tworker_md\t0.8\n",
    )
    .expect("md summary");
    write_text(
        &run.join("tool_outputs/md_run_manifest.json"),
        r#"{"candidate_id":"target_T1","method":"rdkit_peptide_conformer_ensemble","platform":"RDKit","receptor_coordinate":null,"complex_manifest":"","trajectory":"tool_outputs/md_trajectory.tsv"}"#,
    )
    .expect("md run manifest");
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: store,
        input_manifest: input,
        run_dir: run.clone(),
    };

    prepare_bound_rank_fixture(
        &options,
        "peptide",
        "IKVAV",
        "ChEMBL",
        "T1",
        "Target 1",
        "chembl_activity_row",
        1.0,
    )
    .expect("bound ranking fixture");

    let error = rank_targets(&options).expect_err("unbound peptide summary must be rejected");
    let gate = read_text(&run.join("authenticity_gate.tsv")).expect("authenticity gate");
    assert!(error.contains("authenticity_gate_failed"));
    assert!(gate.contains("invalid_md_provenance"));
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn ranking_rejects_md_summary_without_state_artifact() {
    let root = temp_root("md_without_state_artifact");
    let run = root.join("run");
    let store = create_temporary_test_store(&root).expect("store");
    let input = root.join("input.json");
    write_text(
        &input,
        "{\"inputs\":[{\"field\":\"small_molecule\",\"value\":\"CCO\"}],\"files\":[]}\n",
    )
    .expect("input");
    write_text(
        &run.join("known_database_matches.tsv"),
        "input_id\tsource\ttarget_id\ttarget_name\torganism\ttarget_class\tmatch_score\tmatched_features\tevidence\ninput_1\tChEMBL\tT1\tTarget 1\tHomo sapiens\tenzyme\t1.0\tcco\tchembl_activity_row\n",
    )
    .expect("known match");
    write_text(
        &run.join("md_summary.tsv"),
        "candidate_id\trmsd\trmsf\tligand_drift\thydrogen_bond_occupancy\tcontact_occupancy\tinteraction_persistence\tmd_risk_flags\tmd_score_decomposition\tmd_stability_score\ntarget_T1\t1.2\t0.4\t0.6\t0.72\t0.81\t0.77\tmissing_state\tworker_md\t0.8\n",
    )
    .expect("md summary");
    write_text(
        &run.join("tool_outputs/complex_md_manifest.json"),
        r#"{"candidate_id":"target_T1","method":"openmm_receptor_ligand_complex_dynamics","platform":"CPU","receptor_coordinate":"structure_viewer.cif","ligand_coordinate":"tool_outputs/ligand_placement.tsv","trajectory":"tool_outputs/md_trajectory.tsv","protocol_artifact":"tool_outputs/md_protocol.tsv","metric_artifacts":["tool_outputs/md_summary.tsv"]}"#,
    )
    .expect("complex md manifest");
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: store,
        input_manifest: input,
        run_dir: run.clone(),
    };

    prepare_bound_rank_fixture(
        &options,
        "small_molecule",
        "CCO",
        "ChEMBL",
        "T1",
        "Target 1",
        "chembl_activity_row",
        1.0,
    )
    .expect("bound ranking fixture");

    let error = rank_targets(&options).expect_err("MD without state must be rejected");
    let gate = read_text(&run.join("authenticity_gate.tsv")).expect("authenticity gate");
    assert!(error.contains("authenticity_gate_failed"));
    assert!(gate.contains("invalid_md_provenance"));
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn ranking_rejects_unmanifested_vina_docking_even_with_pose() {
    let root = temp_root("vina_docking_score");
    let run = root.join("run");
    let store = create_temporary_test_store(&root).expect("store");
    let input = root.join("input.json");
    write_text(
        &input,
        "{\"inputs\":[{\"field\":\"small_molecule\",\"value\":\"CCO\"}],\"files\":[]}\n",
    )
    .expect("input");
    write_text(
        &run.join("known_database_matches.tsv"),
        "input_id\tsource\ttarget_id\ttarget_name\torganism\ttarget_class\tmatch_score\tmatched_features\tevidence\ninput_1\tChEMBL\tT1\tTarget 1\tHomo sapiens\tenzyme\t1.0\tcco\tchembl_activity_row\n",
    )
    .expect("known match");
    write_text(
        &run.join("docking_summary.tsv"),
        "candidate_id\tdocking_status\tdocking_score\tdocking_engine\tnotes\ntarget_T1\tcompleted\t-7.2\tvina\tpose=tool_outputs/docking_pose_target_T1.pdbqt\n",
    )
    .expect("docking summary");
    write_text(
        &run.join("tool_outputs/docking_pose_target_T1.pdbqt"),
        "REMARK Vina pose\n",
    )
    .expect("docking pose");
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: store,
        input_manifest: input,
        run_dir: run.clone(),
    };

    prepare_bound_rank_fixture(
        &options,
        "small_molecule",
        "CCO",
        "ChEMBL",
        "T1",
        "Target 1",
        "chembl_activity_row",
        1.0,
    )
    .expect("bound ranking fixture");

    let error = rank_targets(&options).expect_err("unmanifested docking must be rejected");
    let gate = read_text(&run.join("authenticity_gate.tsv")).expect("authenticity gate");

    assert!(error.contains("authenticity_gate_failed"));
    assert!(gate.contains("invalid_docking_provenance"));
    assert!(!run.join("target_candidate_ranking.tsv").exists());
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn ranking_ignores_vina_docking_without_pose_artifact() {
    let root = temp_root("vina_without_pose_artifact");
    let run = root.join("run");
    let store = create_temporary_test_store(&root).expect("store");
    let input = root.join("input.json");
    write_text(
        &input,
        "{\"inputs\":[{\"field\":\"small_molecule\",\"value\":\"CCO\"}],\"files\":[]}\n",
    )
    .expect("input");
    write_text(
        &run.join("known_database_matches.tsv"),
        "input_id\tsource\ttarget_id\ttarget_name\torganism\ttarget_class\tmatch_score\tmatched_features\tevidence\ninput_1\tChEMBL\tT1\tTarget 1\tHomo sapiens\tenzyme\t1.0\tcco\tchembl_activity_row\n",
    )
    .expect("known match");
    write_text(
        &run.join("docking_summary.tsv"),
        "candidate_id\tdocking_status\tdocking_score\tdocking_engine\tnotes\ntarget_T1\tcompleted\t-7.2\tvina\tpose=tool_outputs/docking_pose_target_T1.pdbqt\n",
    )
    .expect("docking summary");
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: store,
        input_manifest: input,
        run_dir: run.clone(),
    };

    prepare_bound_rank_fixture(
        &options,
        "small_molecule",
        "CCO",
        "ChEMBL",
        "T1",
        "Target 1",
        "chembl_activity_row",
        1.0,
    )
    .expect("bound ranking fixture");

    let error = rank_targets(&options).expect_err("missing pose must be rejected");
    let gate = read_text(&run.join("authenticity_gate.tsv")).expect("authenticity gate");

    assert!(error.contains("authenticity_gate_failed"));
    assert!(gate.contains("invalid_docking_provenance"));
    assert!(!run.join("target_candidate_ranking.tsv").exists());
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn ranking_blocks_final_artifacts_without_positive_provenance() {
    let root = temp_root("authenticity_static_fallback");
    let run = root.join("run");
    let store = create_temporary_test_store(&root).expect("store");
    let input = root.join("input.json");
    write_text(
        &input,
        "{\"inputs\":[{\"field\":\"small_molecule\",\"value\":\"CCO\"}],\"files\":[]}\n",
    )
    .expect("input");
    write_text(
        &run.join("known_database_matches.tsv"),
        "input_id\tsource\ttarget_id\ttarget_name\torganism\ttarget_class\tmatch_score\tmatched_features\tevidence\ninput_1\tDemoDB\tT1\tTarget 1\tHomo sapiens\tenzyme\t1.0\tcco\tstatic_fallback_candidate;demo_data\n",
    )
    .expect("known match");
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: store,
        input_manifest: input,
        run_dir: run.clone(),
    };

    normalize_and_compute_fixture(&options, "small_molecule", "CCO")
        .expect("normalized ranking input");
    write_empty_optional_rank_artifacts(&run).expect("empty optional rank artifacts");

    let err = rank_targets(&options).expect_err("authenticity gate should block");

    assert!(err.contains("authenticity_gate_failed"));
    let gate = read_text(&run.join("authenticity_gate.tsv")).expect("gate report");
    assert!(gate.contains("known_database_matches.tsv"));
    assert!(gate.contains("provenance_schema_missing"));
    assert!(!run.join("target_candidate_ranking.tsv").exists());
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn authenticity_gate_binds_database_evidence_to_run_invocation_and_source_manifest() {
    for attack in [
        "valid",
        "forged_digest",
        "old_invocation",
        "cross_run",
        "wrong_source_path",
        "missing_manifest",
    ] {
        let root = temp_root(&format!("authenticity_binding_{attack}"));
        let run = root.join("run");
        let store = create_temporary_test_store(&root).expect("store");
        let input = root.join("input.json");
        write_text(
            &input,
            "{\"inputs\":[{\"field\":\"small_molecule\",\"value\":\"CCO\"}],\"files\":[]}\n",
        )
        .expect("input");
        let options = ScienceOptions {
            workspace_root: root.clone(),
            science_store: store.clone(),
            input_manifest: input,
            run_dir: run.clone(),
        };
        prepare_bound_rank_fixture(
            &options,
            "small_molecule",
            "CCO",
            "ChEMBL",
            "T1",
            "Target 1",
            "chembl_activity_row",
            1.0,
        )
        .expect("bound fixture");
        let matches_path = run.join("known_database_matches.tsv");
        let mut matches = read_text(&matches_path).expect("matches");
        match attack {
            "valid" => {}
            "forged_digest" => matches = matches.replace(&"a".repeat(64), &"c".repeat(64)),
            "old_invocation" => {
                let current = current_pipeline_invocation_id(&options).expect("invocation");
                matches = matches.replace(&current, "pipeline_old_invocation");
            }
            "cross_run" => matches = matches.replace("run_id=run", "run_id=foreign_run"),
            "wrong_source_path" => {
                matches = matches.replace(
                    "source_path=downloads/chembl/chembl_sqlite.tar.gz",
                    "source_path=downloads/foreign/source.bin",
                );
            }
            "missing_manifest" => {
                fs::remove_file(store.join("manifests/source_file_manifest.tsv"))
                    .expect("remove manifest");
            }
            _ => unreachable!(),
        }
        write_text(&matches_path, &matches).expect("mutated matches");
        if attack == "valid" {
            rank_targets(&options).expect("valid current evidence");
        } else {
            let error = rank_targets(&options).expect_err("attack must be rejected");
            assert!(
                error.contains("authenticity_gate_failed"),
                "{attack}: {error}"
            );
            let gate = read_text(&run.join("authenticity_gate.tsv")).expect("gate");
            assert!(
                gate.contains("source_manifest_binding")
                    || gate.contains("run_id_not_current")
                    || gate.contains("pipeline_invocation_not_current"),
                "{attack}: {gate}"
            );
        }
        fs::remove_dir_all(&root).expect("cleanup temp root");
    }
}

#[test]
fn direct_query_generation_rejects_stale_downstream_artifacts_until_match_reruns() {
    let root = temp_root("direct_query_generation_invalidation");
    let run = root.join("run");
    let store = create_temporary_test_store(&root).expect("store");
    let input = root.join("input.json");
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: store,
        input_manifest: input.clone(),
        run_dir: run.clone(),
    };

    prepare_bound_rank_fixture(
        &options,
        "small_molecule",
        "CCO",
        "ChEMBL",
        "T1",
        "Target 1",
        "chembl_activity_row",
        1.0,
    )
    .expect("initial bound fixture");
    run_science_command(ScienceCommand::TargetRank, &options).expect("initial target rank");
    let initial_pipeline = current_pipeline_invocation_id(&options).expect("initial pipeline");
    let initial_worker = current_worker_invocation(&options).expect("initial worker");

    write_text(
        &input,
        "{\"inputs\":[{\"field\":\"small_molecule\",\"value\":\"CCN\"}],\"files\":[]}\n",
    )
    .expect("replacement input");
    run_science_command(ScienceCommand::InputNormalize, &options).expect("replacement normalize");
    let normalized_pipeline =
        current_pipeline_invocation_id(&options).expect("normalized pipeline");
    let normalized_worker = current_worker_invocation(&options).expect("normalized worker");
    assert_ne!(normalized_pipeline, initial_pipeline);
    assert_ne!(normalized_worker, initial_worker);

    let normalize_error = run_science_command(ScienceCommand::TargetRank, &options)
        .expect_err("normalization must invalidate the old match generation");
    assert!(normalize_error.contains("authenticity_gate_failed"));
    assert!(
        read_text(&run.join("authenticity_gate.tsv"))
            .expect("normalization authenticity gate")
            .contains("pipeline_invocation_not_current")
    );

    run_science_command(ScienceCommand::FeaturesCompute, &options).expect("replacement features");
    let feature_pipeline = current_pipeline_invocation_id(&options).expect("feature pipeline");
    let feature_worker = current_worker_invocation(&options).expect("feature worker");
    assert_ne!(feature_pipeline, normalized_pipeline);
    assert_ne!(feature_worker, normalized_worker);
    let feature_error = run_science_command(ScienceCommand::TargetRank, &options)
        .expect_err("feature generation must keep old matches invalid");
    assert!(feature_error.contains("authenticity_gate_failed"));

    prepare_bound_rank_fixture(
        &options,
        "small_molecule",
        "CCN",
        "ChEMBL",
        "T1",
        "Target 1",
        "chembl_activity_row",
        1.0,
    )
    .expect("complete replacement generation");
    run_science_command(ScienceCommand::TargetRank, &options)
        .expect("complete rerun accepts current matches");
    let ranking = read_text(&run.join("target_candidate_ranking.tsv")).expect("current ranking");
    assert!(ranking.contains("input_1"));
    assert!(ranking.contains("record_1"));

    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn structure_evaluate_writes_viewer_from_real_coordinate_file() {
    let root = temp_root("structure_viewer_real");
    install_not_applicable_worker_fixtures(&root).expect("worker fixtures");
    let run = root.join("run");
    let store = root.join("science_store");
    write_test_source_manifest(&store, &["rcsb_ligand_bound"]).expect("source manifest");
    let input = root.join("input.json");
    ensure_dir(&store.join("indexes")).expect("indexes");
    ensure_dir(&store.join("downloads/rcsb/mmcif/0g")).expect("coords");
    write_text(
            &store.join("indexes/structure_targets.tsv"),
            "source\ttarget_id\ttarget_name\torganism\ttarget_class\tligand_tokens\tsequence_tokens\tstructure_tokens\tevidence\nRCSB\t10GS\tRCSB structure 10GS\tunknown\tstructure_record\taspirin\t10gs\t10gs pocket\trcsb_structure_file\n",
        )
        .expect("structure index");
    write_text(
            &store.join("downloads/rcsb/mmcif/0g/10gs.cif"),
            "data_10GS\nATOM 1 N N . GLY A 1 1 ? 11.104 13.207 2.100 1.00 20.00 ? 1 GLY A N 1\nATOM 2 C CA . GLY A 1 1 ? 12.456 13.607 2.500 1.00 20.00 ? 1 GLY A CA 1\n",
        )
        .expect("coordinate cif");
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: store,
        input_manifest: input,
        run_dir: run.clone(),
    };

    normalize_and_compute_fixture(&options, "small_molecule", "CCO").expect("upstream manifests");
    write_text(
        &run.join("feature_manifest.tsv"),
        "input_id\tinput_kind\tfeature_family\tfeature_name\tfeature_value\tfeature_source\tfeature_token\trecord_id\ninput_1\tsmall_molecule\tligand\ttext\taspirin\tuser\taspirin\trecord_1\n",
    )
    .expect("features");
    run_science_command(ScienceCommand::StructureEvaluate, &options).expect("structure");
    let out = coarse_rank_candidates(&options).expect("coarse rank");
    let viewer = read_text(&run.join("structure_viewer.cif")).expect("viewer cif");
    let status = read_text(&run.join("structure_viewer_status.tsv")).expect("viewer status");

    assert!(out.contains("viewer_artifact=ready"));
    assert!(viewer.contains("ATOM 1"));
    assert!(status.contains("ready\t10GS\t10GS\tstructure_candidates"));
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn structure_evaluate_prefers_bindingdb_pdb_candidate_over_token_noise() {
    let root = temp_root("structure_bindingdb_candidate");
    install_not_applicable_worker_fixtures(&root).expect("worker fixtures");
    let run = root.join("run");
    let store = root.join("science_store");
    write_test_source_manifest(&store, &["rcsb_ligand_bound"]).expect("source manifest");
    let input = root.join("input.json");
    ensure_dir(&store.join("indexes")).expect("indexes");
    ensure_dir(&store.join("downloads/rcsb/mmcif/12")).expect("noise coords");
    ensure_dir(&store.join("downloads/rcsb/mmcif/h4")).expect("target coords");
    ensure_dir(&run.join("tool_outputs")).expect("tool outputs");
    write_text(
        &run.join("known_database_matches.tsv"),
        "input_id\tsource\ttarget_id\ttarget_name\torganism\ttarget_class\tmatch_score\tmatched_features\tevidence\ninput_1\tBindingDB\tP00918\tCarbonic anhydrase 2\tHomo sapiens\tknown_human_protein\t1.0000\tligand_code=AZM;pdb_ids=2H4N\tbindingdb_onsite_activity_row;Ki (nM)=12;ligand_code=AZM;pdb_ids=2H4N\n",
    )
    .expect("known matches");
    write_text(
        &store.join("indexes/structure_targets.tsv"),
        "source\ttarget_id\ttarget_name\torganism\ttarget_class\tligand_tokens\tsequence_tokens\tstructure_tokens\tevidence\nRCSB\t112M\tUnrelated myoglobin structure\tunknown\tstructure_record\tazm\t112m\t112m pocket\trcsb_structure_file\nRCSB\t2H4N\tCarbonic anhydrase II complexed with acetazolamide\tHomo sapiens\tstructure_record\tazm\t2h4 h4n\tcarbonic anhydrase ii azm\trcsb_structure_file\n",
    )
    .expect("structure index");
    write_text(
        &store.join("downloads/rcsb/mmcif/12/112m.cif"),
        "data_112M\n_struct.title 'UNRELATED MYOGLOBIN COORDINATE'\nATOM 1 N N . GLY A 1 1 ? 1.0 1.0 1.0 1.00 20.00 ? 1 GLY A N 1\n",
    )
    .expect("noise coordinate");
    write_text(
        &store.join("downloads/rcsb/mmcif/h4/2h4n.cif"),
        "data_2H4N\n_struct.title 'HUMAN CARBONIC ANHYDRASE II COMPLEXED WITH ACETAZOLAMIDE'\nATOM 1 N N . GLY A 1 1 ? 2.0 2.0 2.0 1.00 20.00 ? 1 GLY A N 1\n",
    )
    .expect("target coordinate");
    write_text(
        &run.join("tool_outputs/docking_summary.tsv"),
        "candidate_id\tdocking_status\tdocking_score\tdocking_engine\tnotes\n",
    )
    .expect("skip worker");
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: store,
        input_manifest: input,
        run_dir: run.clone(),
    };

    normalize_and_compute_fixture(&options, "small_molecule", "CCN").expect("upstream manifests");
    write_text(
        &run.join("feature_manifest.tsv"),
        "input_id\tinput_kind\tfeature_family\tfeature_name\tfeature_value\tfeature_source\tfeature_token\trecord_id\ninput_1\tsmall_molecule\tligand\ttext\tazm\tuser\tazm\trecord_1\n",
    )
    .expect("features");
    run_science_command(ScienceCommand::StructureEvaluate, &options).expect("structure");
    let out = coarse_rank_candidates(&options).expect("coarse rank");
    let status = read_text(&run.join("structure_viewer_status.tsv")).expect("viewer status");
    let shortlist = read_text(&run.join("candidate_shortlist.tsv")).expect("shortlist");

    assert!(out.contains("viewer_artifact=ready"));
    assert!(status.contains("ready\tP00918\t2H4N\tstructure_candidates"));
    let selected_rows =
        read_simple_tsv_rows(&run.join("complex_candidate_selection.tsv")).expect("selected rows");
    assert_eq!(selected_rows.len(), COMPLEX_CANDIDATE_TOP_N);
    assert_eq!(selected_rows[0]["input_id"], "input_1");
    assert_eq!(selected_rows[0]["record_id"], "record_1");
    assert_eq!(selected_rows[0]["target_id"], "P00918");
    assert!(selected_rows[0]["candidate_id"].starts_with("target_P00918_"));
    assert!(shortlist.contains("target_P00918_"));
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn structure_evaluate_reads_canonical_index_rows_beyond_source_sampling_limit() {
    let root = temp_root("structure_index_full_scan");
    install_not_applicable_worker_fixtures(&root).expect("worker fixtures");
    let run = root.join("run");
    let store = root.join("science_store");
    write_test_source_manifest(&store, &["rcsb_ligand_bound"]).expect("source manifest");
    let input = root.join("input.json");
    ensure_dir(&store.join("indexes")).expect("indexes");
    ensure_dir(&store.join("downloads/rcsb/mmcif/ik")).expect("coords");
    let mut index = String::from(
        "source\ttarget_id\ttarget_name\torganism\ttarget_class\tligand_tokens\tsequence_tokens\tstructure_tokens\tevidence\n",
    );
    for row in 0..5_250usize {
        index.push_str(&format!(
            "RCSB\t9{row:03}\tFiller structure {row}\tunknown\tstructure_record\t\tfiller{row}\tfiller pocket\trcsb_structure_file\n"
        ));
    }
    index.push_str(
        "RCSB\t1IKV\tRCSB structure 1IKV\tunknown\tstructure_record\t\t1ik ikv\t1ikv pocket\trcsb_structure_file\n",
    );
    write_text(&store.join("indexes/structure_targets.tsv"), &index).expect("structure index");
    write_text(
        &store.join("downloads/rcsb/mmcif/ik/1ikv.cif"),
        "data_1IKV\nATOM 1 N N . GLY A 1 1 ? 11.104 13.207 2.100 1.00 20.00 ? 1 GLY A N 1\nATOM 2 C CA . GLY A 1 1 ? 12.456 13.607 2.500 1.00 20.00 ? 1 GLY A CA 1\n",
    )
    .expect("coordinate cif");
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: store,
        input_manifest: input,
        run_dir: run.clone(),
    };

    normalize_and_compute_fixture(&options, "peptide", "IKVAV").expect("upstream manifests");
    write_text(
        &run.join("feature_manifest.tsv"),
        "input_id\tinput_kind\tfeature_family\tfeature_name\tfeature_value\tfeature_source\tfeature_token\trecord_id\ninput_1\tpeptide\tpeptide\tsequence_feature_token\tikv\tGenoLigand\tikv\trecord_1\n",
    )
    .expect("features");
    let structure =
        run_science_command(ScienceCommand::StructureEvaluate, &options).expect("structure");
    let out = coarse_rank_candidates(&options).expect("coarse rank");
    let matches = read_text(&run.join("structure_interaction_matches.tsv")).expect("matches");
    let summary = read_text(&run.join("structure_patch_summary.tsv")).expect("summary");
    let status = read_text(&run.join("structure_viewer_status.tsv")).expect("viewer status");

    assert!(structure.contains("structure_artifact=ready"));
    assert!(out.contains("viewer_artifact=ready"));
    assert!(matches.contains("1IKV"));
    assert!(summary.contains("target_1IKV"));
    assert!(status.contains("ready\t1IKV\t1IKV\tstructure_candidates"));
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn dynamics_screen_aggregates_real_metric_artifacts() {
    let root = temp_root("md_metric_artifacts");
    let options = indexed_fixture_options(&root);
    let rows = write_indexed_fixture_selection(
        &options,
        &[(
            "candidate_A",
            "input_1",
            "record_1",
            "small_molecule",
            "CCO",
        )],
    )
    .expect("indexed selection");
    write_indexed_md_fixture(&options, &rows[0], false).expect("indexed MD fixture");

    let out = screen_dynamics(&options).expect("dynamics");
    let md = read_text(&options.run_dir.join("md_summary.tsv")).expect("md summary");
    let status = read_text(&options.run_dir.join("md_summary_status.tsv")).expect("md status");

    assert!(out.contains("md_artifact=ready"));
    assert!(md.contains("candidate_A\t1.0\t0.5\t1.0\t0.6\t0.8"));
    assert!(status.contains("ready"));
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn dynamics_screen_rejects_provenance_with_identity_missing_on_both_sides() {
    let root = temp_root("md_missing_identity");
    let run = root.join("run");
    let input = root.join("input.json");
    write_text(&input, "{\"inputs\":[],\"files\":[]}\n").expect("input");
    write_text(
        &run.join("tool_outputs/md_rmsd.tsv"),
        "candidate_id\tvalue\nC1\t1.2\n",
    )
    .expect("metric");
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: root.join("science_store"),
        input_manifest: input,
        run_dir: run.clone(),
    };
    write_md_binding_manifests(&run, "C1", "input_1", "CCO").expect("binding manifests");
    bind_current_md_artifacts(&options, &["md_rmsd.tsv"]).expect("bind current MD metric");

    let result_path = run.join("worker_results/md_summary_ligand_dynamics.json");
    let provenance_path = run.join("tool_outputs/md_artifact_provenance.json");
    let mut result: Value =
        serde_json::from_str(&read_text(&result_path).expect("result")).expect("result json");
    let mut provenance: Value =
        serde_json::from_str(&read_text(&provenance_path).expect("provenance"))
            .expect("provenance json");
    result
        .as_object_mut()
        .expect("result object")
        .remove("candidate_id");
    provenance
        .as_object_mut()
        .expect("provenance object")
        .remove("candidate_id");
    provenance["artifacts"][0]
        .as_object_mut()
        .expect("artifact object")
        .remove("candidate_id");
    write_text(&result_path, &(result.to_string() + "\n")).expect("write result");
    write_text(&provenance_path, &(provenance.to_string() + "\n")).expect("write provenance");

    let out = screen_dynamics(&options).expect("dynamics");
    let status = read_text(&run.join("md_summary_status.tsv")).expect("status");
    assert!(out.contains("md_artifact=not_ready"));
    assert!(status.contains("result_identity_missing"));
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn dynamics_screen_rejects_job_run_identity_drift() {
    let root = temp_root("md_job_run_identity_drift");
    let run = root.join("run-current");
    let input = root.join("input.json");
    write_text(
        &input,
        "{\"inputs\":[{\"field\":\"small_molecule\",\"value\":\"CCO\"}],\"files\":[]}\n",
    )
    .expect("input");
    write_md_binding_manifests(&run, "C1", "input_1", "CCO").expect("binding manifests");
    write_text(
        &run.join("tool_outputs/md_summary.tsv"),
        "candidate_id\trmsd\trmsf\tligand_drift\thydrogen_bond_occupancy\tcontact_occupancy\tinteraction_persistence\tmd_risk_flags\tmd_score_decomposition\tmd_stability_score\nC1\t1.2\t0.4\t0.6\t0.72\t0.81\t0.77\tstable\tbound\t0.8\n",
    )
    .expect("MD summary");
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: root.join("science_store"),
        input_manifest: input,
        run_dir: run.clone(),
    };
    bind_current_md_artifacts(&options, &["md_summary.tsv"]).expect("bind MD summary");
    let job_path = run.join("worker_jobs/md_summary_ligand_dynamics.json");
    let mut job: Value =
        serde_json::from_str(&read_text(&job_path).expect("job")).expect("job json");
    job["run_id"] = json!("run-stale");
    job["run_dir"] = json!(root.join("run-stale").display().to_string());
    ensure_dir(&root.join("run-stale")).expect("stale run dir");
    write_text(&job_path, &(job.to_string() + "\n")).expect("drift job identity");

    let out = screen_dynamics(&options).expect("dynamics");
    let status = read_text(&run.join("md_summary_status.tsv")).expect("status");
    assert!(out.contains("md_artifact=not_ready"));
    assert!(status.contains("job_run_identity_mismatch"));
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn md_binding_digest_matches_python_ascii_canonical_json() {
    let value = json!({"target_name": "蛋白", "emoji": "🧬"});
    assert_eq!(
        canonical_json_sha256(&value),
        "926673b9197113664316fd46c19c0703352634649a37bdfb303d4c55a99c884c"
    );
}

#[test]
fn dynamics_screen_rejects_candidate_manifest_changed_after_binding() {
    let root = temp_root("md_candidate_manifest_drift");
    let run = root.join("run");
    let input = root.join("input.json");
    write_text(
        &input,
        "{\"inputs\":[{\"field\":\"small_molecule\",\"value\":\"CCO\"}],\"files\":[]}\n",
    )
    .expect("input");
    write_md_binding_manifests(&run, "C1", "input_1", "CCO").expect("binding manifests");
    write_text(
        &run.join("tool_outputs/md_summary.tsv"),
        "candidate_id\trmsd\trmsf\tligand_drift\thydrogen_bond_occupancy\tcontact_occupancy\tinteraction_persistence\tmd_risk_flags\tmd_score_decomposition\tmd_stability_score\nC1\t1.2\t0.4\t0.6\t0.72\t0.81\t0.77\tstable\tbound\t0.8\n",
    )
    .expect("MD summary");
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: root.join("science_store"),
        input_manifest: input,
        run_dir: run.clone(),
    };
    bind_current_md_artifacts(&options, &["md_summary.tsv"]).expect("bind MD summary");
    write_md_binding_manifests(&run, "C2", "input_1", "CCO").expect("replace candidate manifest");

    let out = screen_dynamics(&options).expect("dynamics");
    let status = read_text(&run.join("md_summary_status.tsv")).expect("status");
    assert!(out.contains("md_artifact=not_ready"));
    assert!(status.contains("candidate_manifest_mismatch"));
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn dynamics_screen_rejects_input_manifest_changed_after_binding() {
    let root = temp_root("md_input_manifest_drift");
    let run = root.join("run");
    let input = root.join("input.json");
    write_text(
        &input,
        "{\"inputs\":[{\"field\":\"small_molecule\",\"value\":\"CCO\"}],\"files\":[]}\n",
    )
    .expect("input");
    write_md_binding_manifests(&run, "C1", "input_1", "CCO").expect("binding manifests");
    write_text(
        &run.join("tool_outputs/md_summary.tsv"),
        "candidate_id\trmsd\trmsf\tligand_drift\thydrogen_bond_occupancy\tcontact_occupancy\tinteraction_persistence\tmd_risk_flags\tmd_score_decomposition\tmd_stability_score\nC1\t1.2\t0.4\t0.6\t0.72\t0.81\t0.77\tstable\tbound\t0.8\n",
    )
    .expect("MD summary");
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: root.join("science_store"),
        input_manifest: input,
        run_dir: run.clone(),
    };
    bind_current_md_artifacts(&options, &["md_summary.tsv"]).expect("bind MD summary");
    write_md_binding_manifests(&run, "C1", "input_1", "CCC").expect("replace input manifest");

    let out = screen_dynamics(&options).expect("dynamics");
    let status = read_text(&run.join("md_summary_status.tsv")).expect("status");
    assert!(out.contains("md_artifact=not_ready"));
    assert!(status.contains("input_manifest_mismatch"));
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn dynamics_screen_uses_bound_non_pass_result_status_and_reason() {
    for (status, execution_status, outcome_status, required) in [
        ("FAIL", "failed", "failed", true),
        ("NOT_READY", "not_ready", "not_ready", true),
        ("NOT_APPLICABLE", "not_applicable", "not_applicable", false),
    ] {
        let root = temp_root(&format!("md_bound_{}", status.to_ascii_lowercase()));
        let run = root.join("run");
        let input = root.join("input.json");
        write_text(&input, "{\"inputs\":[],\"files\":[]}\n").expect("input");
        let options = ScienceOptions {
            workspace_root: root.clone(),
            science_store: root.join("science_store"),
            input_manifest: input,
            run_dir: run.clone(),
        };
        let invocation_id = start_worker_invocation(&options).expect("invocation");
        let task = &SCIENTIFIC_WORKER_TASKS[0];
        let job = worker_job_value(&options, task).expect("job");
        write_text(
            &run.join("worker_jobs/md_summary_ligand_dynamics.json"),
            &(job.to_string() + "\n"),
        )
        .expect("job file");
        let reason = format!("current_bound_{}_reason", status.to_ascii_lowercase());
        let result = json!({
            "schema_version": WORKER_SCHEMA_VERSION,
            "job_id": task.job_id,
            "worker": task.worker,
            "run_id": job["run_id"],
            "invocation_id": invocation_id,
            "status": status,
            "engine_version": "test MD status 1",
            "execution_status": execution_status,
            "reason": reason,
            "outcomes": [{
                "status": outcome_status,
                "reason": reason,
                "required": required,
                "database_id": "md_summary",
                "row_count": 0,
            }],
            "python_owns_final_score": false,
            "python_owns_report_semantics": false,
            "primitive_execution_only": true,
        });
        write_text(
            &run.join("worker_results/md_summary_ligand_dynamics.json"),
            &(result.to_string() + "\n"),
        )
        .expect("result file");
        write_text(
            &run.join("tool_outputs/md_not_ready.tsv"),
            "reason\trisk_flags\nstale_unbound_reason\tstale\n",
        )
        .expect("stale not-ready artifact");

        let out = screen_dynamics(&options).expect("dynamics");
        let summary_status = read_text(&run.join("md_summary_status.tsv")).expect("summary status");
        assert!(out.contains("md_artifact=not_ready"));
        assert!(summary_status.contains(&format!("{}\t{}", status.to_ascii_lowercase(), reason)));
        assert!(!summary_status.contains("stale_unbound_reason"));
        fs::remove_dir_all(&root).expect("cleanup temp root");
        assert!(!root.exists());
    }
}

#[test]
fn dynamics_screen_records_missing_md_reason_without_rows() {
    let root = temp_root("md_missing_reason");
    let run = root.join("run");
    let store = root.join("science_store");
    let input = root.join("input.json");
    write_text(&input, "{\"inputs\":[],\"files\":[]}\n").expect("input");
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: store,
        input_manifest: input,
        run_dir: run.clone(),
    };

    let out = screen_dynamics(&options).expect("dynamics");
    let md = read_text(&run.join("md_summary.tsv")).expect("md summary");
    let status = read_text(&run.join("md_summary_status.tsv")).expect("md status");

    assert!(out.contains("md_artifact=not_ready"));
    assert_eq!(md.lines().count(), 1);
    assert!(status.contains("no md_summary.tsv"));
    assert!(status.contains("no RMSD metric artifact"));
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn prepare_science_store_reports_needs_repair_until_sources_and_tools_are_ready() {
    let root = temp_root("empty_prepare");
    let store = root.join("science_store");
    let options = StoreOptions {
        workspace_root: root.clone(),
        science_store: store.clone(),
    };
    let out =
        run_store_command(ScienceCommand::PrepareScienceStore, &options).expect("prepare store");
    let status = read_text(&store.join("install_status.json")).expect("install status");

    assert!(out.contains("status=needs_repair"));
    assert!(out.contains("missing_components="));
    assert!(status.contains("\"status\":\"needs_repair\""));
    assert!(status.contains("source:chembl"));
    assert!(!status.contains("\"status\":\"ready\""));

    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn source_local_available_accepts_pruned_source_only_with_complete_retention_evidence() {
    for attack in [
        "valid",
        "wrong_schema",
        "wrong_state",
        "wrong_source_id",
        "wrong_retention_path",
        "wrong_source_path",
        "wrong_storage_state",
        "tampered_digest",
        "tampered_release",
        "tampered_trust",
        "missing_retention",
        "missing_source_manifest",
        "missing_derived",
        "empty_derived",
        "legacy_derived",
        "tampered_compact_database",
        "wrong_compact_schema",
        "wrong_compact_source_identity",
    ] {
        let root = temp_root(&format!("pruned_source_availability_{attack}"));
        let store = root.join("science_store");
        install_ligand_similarity_builder_fixture(&root, false).expect("similarity builder");
        write_ligand_similarity_staging_fixture(&store).expect("ligand staging");
        let source = REQUIRED_SOURCES
            .iter()
            .find(|source| source.id == "bindingdb")
            .expect("BindingDB required source");
        let options = StoreOptions {
            workspace_root: root.clone(),
            science_store: store.clone(),
        };
        prepare_ligand_similarity_authority(&options).expect("compact ligand authority");
        fs::remove_dir_all(root.join("python")).expect("remove source-tree worker fixture");
        fs::remove_file(store.join(BINDINGDB_SOURCE_REL_PATH))
            .expect("remove retained BindingDB source payload");
        let digest = test_source_digest("bindingdb");
        let source_id = if attack == "wrong_source_id" {
            "chembl"
        } else {
            source.id
        };
        let retention_path = if attack == "wrong_retention_path" {
            "downloads/bindingdb/foreign.zip"
        } else {
            source.local_rel_path
        };
        let source_path = if attack == "wrong_source_path" {
            "downloads/bindingdb/foreign.zip"
        } else {
            source.local_rel_path
        };
        let storage_state = if attack == "wrong_storage_state" {
            "materialized"
        } else {
            "indexed_source_pruned"
        };
        let retention_digest = if attack == "tampered_digest" {
            "c".repeat(64)
        } else {
            digest.clone()
        };
        let retention_release = if attack == "tampered_release" {
            "bindingdb-foreign-release"
        } else {
            "bindingdb-test-release"
        };
        let retention_trust = if attack == "tampered_trust" {
            "foreign_trust"
        } else {
            "unit_test"
        };
        let schema = if attack == "wrong_schema" {
            "genoligand_source_retention_foreign"
        } else {
            SOURCE_RETENTION_SCHEMA_VERSION
        };
        let state = if attack == "wrong_state" {
            "pruning"
        } else {
            "pruned"
        };
        let derived = if attack == "legacy_derived" {
            "indexes/bindingdb/bindingdb_query.sqlite".to_string()
        } else {
            format!(
                "{};{}",
                LIGAND_SIMILARITY_DATABASE_REL_PATH, LIGAND_SIMILARITY_MANIFEST_REL_PATH
            )
        };

        if attack == "missing_derived" {
            fs::remove_file(store.join(LIGAND_SIMILARITY_DATABASE_REL_PATH))
                .expect("remove compact database");
        } else if attack == "empty_derived" {
            write_text(&store.join(LIGAND_SIMILARITY_DATABASE_REL_PATH), "")
                .expect("empty compact database");
        } else if attack == "legacy_derived" {
            write_text(
                &store.join("indexes/bindingdb/bindingdb_query.sqlite"),
                "legacy\n",
            )
            .expect("legacy derived payload");
        }
        if attack == "tampered_compact_database" {
            write_text(
                &store.join(LIGAND_SIMILARITY_DATABASE_REL_PATH),
                "tampered compact database\n",
            )
            .expect("tamper compact database");
        } else if matches!(
            attack,
            "wrong_compact_schema" | "wrong_compact_source_identity"
        ) {
            let manifest_path = store.join(LIGAND_SIMILARITY_MANIFEST_REL_PATH);
            let mut manifest = serde_json::from_str::<Value>(
                &read_text(&manifest_path).expect("read compact manifest"),
            )
            .expect("parse compact manifest");
            if attack == "wrong_compact_schema" {
                manifest["schema_version"] = json!("genoligand_ligand_similarity_index_foreign");
            } else {
                manifest["inputs"]["bindingdb"]["source_digest"] = json!("c".repeat(64));
            }
            write_text(
                &manifest_path,
                &(serde_json::to_string_pretty(&manifest).expect("serialize compact manifest")
                    + "\n"),
            )
            .expect("write compact manifest attack");
        }
        if attack != "missing_source_manifest" {
            write_text(
                &store.join("manifests/source_file_manifest.tsv"),
                &format!(
                    "source_id\tlocal_path\tstatus\tstorage_state\tsha256\trelease_identity\ttrust_source\nchembl\tdownloads/chembl/chembl_sqlite.tar.gz\tready\tindexed_source_pruned\t{}\tchembl-test-release\tunit_test\n{}\t{}\tready\t{}\t{}\tbindingdb-test-release\tunit_test\n",
                    test_source_digest("chembl"), source.id, source_path, storage_state, digest
                ),
            )
            .expect("source manifest");
        } else {
            fs::remove_file(store.join("manifests/source_file_manifest.tsv"))
                .expect("remove source manifest");
        }
        if attack != "missing_retention" {
            write_text(
                &store.join("manifests/source_retention_manifest.tsv"),
                &format!(
                    "schema_version\tsource_id\tsource_local_path\tstate\tsource_manifest_sha256\trelease_identity\ttrust_source\tderived_paths\n{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                    schema,
                    source_id,
                    retention_path,
                    state,
                    retention_digest,
                    retention_release,
                    retention_trust,
                    derived
                ),
            )
            .expect("retention manifest");
        }

        assert_eq!(
            source_local_available(&options, source),
            attack == "valid",
            "unexpected availability for attack {attack}"
        );
        fs::remove_dir_all(&root).expect("cleanup temp root");
        assert!(!root.exists());
    }
}

#[test]
fn source_local_available_keeps_original_payload_path_behavior() {
    let root = temp_root("original_source_availability");
    let store = root.join("science_store");
    let source = REQUIRED_SOURCES
        .iter()
        .find(|source| source.id == "bindingdb")
        .expect("BindingDB required source");
    write_text(&store.join(source.local_rel_path), "bindingdb-archive")
        .expect("original source payload");
    let options = StoreOptions {
        workspace_root: root.clone(),
        science_store: store,
    };

    assert!(source_local_available(&options, source));
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn plinder_required_source_uses_the_canonical_download_scope() {
    let source = REQUIRED_SOURCES
        .iter()
        .find(|source| source.id == "plinder")
        .expect("PLINDER required source");
    assert_eq!(source.local_rel_path, "downloads/plinder/index");
}

#[test]
fn pipeline_blocks_when_store_lacks_install_closure() {
    let root = temp_root("missing_install_closure");
    let store = root.join("science_store");
    ensure_dir(&store.join("indexes")).expect("store dirs");
    write_text(
            &store.join("science_store_manifest.tsv"),
            "item\tstatus\tpath\nwarehouse\tready\tindexes/known_targets.tsv\nmetagenome_index\tready\tindexes/metagenome_targets.tsv\nstructure_index\tready\tindexes/structure_targets.tsv\ntool_versions\tready\tindexes/tool_versions.tsv\n",
        )
        .expect("manifest");
    write_text(
        &store.join("indexes/tool_versions.tsv"),
        &unit_test_tool_versions_text(),
    )
    .expect("tools");
    write_text(
            &store.join("indexes/known_targets.tsv"),
            "source\ttarget_id\ttarget_name\torganism\ttarget_class\tligand_tokens\tsequence_tokens\tstructure_tokens\tevidence\n",
        )
        .expect("known");
    write_text(
            &store.join("indexes/metagenome_targets.tsv"),
            "source\ttarget_id\ttarget_name\torganism\ttarget_class\tligand_tokens\tsequence_tokens\tstructure_tokens\tevidence\n",
        )
        .expect("metagenome");
    write_text(
            &store.join("indexes/structure_targets.tsv"),
            "source\ttarget_id\ttarget_name\torganism\ttarget_class\tligand_tokens\tsequence_tokens\tstructure_tokens\tevidence\n",
        )
        .expect("structure");
    let input = root.join("input.json");
    let run = root.join("run");
    write_text(
        &input,
        "{\"inputs\":[{\"field\":\"smiles\",\"value\":\"CCO\"}],\"files\":[]}\n",
    )
    .expect("input");
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: store,
        input_manifest: input,
        run_dir: run.clone(),
    };
    let error =
        run_science_command(ScienceCommand::PipelineRun, &options).expect_err("blocked outcome");
    let failure = read_text(&run.join("failure_reason.txt")).expect("failure");

    assert!(error.contains("pipeline_blocked"));
    assert!(failure.contains("science_store_readiness_missing"));

    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn canonical_readiness_is_shared_by_pipeline_and_report_and_rejects_stale_evidence() {
    let root = temp_root("canonical_readiness");
    let store = root.join("science_store");
    ensure_dir(&store).expect("science store");
    write_text(
        &store.join("science_store_readiness.json"),
        &format!(
            "{}\n",
            json!({
                "schema_version": SCIENCE_READINESS_SCHEMA_VERSION,
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
                "repair_actions": ["sync_rcsb_ligand_bound_scope"]
            })
        ),
    )
    .expect("canonical readiness");
    let input = root.join("input.json");
    write_text(
        &input,
        "{\"inputs\":[{\"field\":\"small_molecule\",\"value\":\"CCO\"}],\"files\":[]}\n",
    )
    .expect("input");
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: store.clone(),
        input_manifest: input,
        run_dir: root.join("run"),
    };

    let readiness = evaluate_science_store_readiness(&store).expect("readiness");
    assert_eq!(readiness.status, "needs_repair");
    assert_eq!(readiness.blockers, ["rcsb_scope_incomplete"]);
    assert_eq!(readiness.blocker_details[0].evidence, "ready=7 scope=9");
    assert_eq!(readiness.repair_actions, ["sync_rcsb_ligand_bound_scope"]);
    assert_eq!(readiness.sources[0]["logical_source"], "rcsb_ligand_bound");
    assert_eq!(readiness.indexes[0]["index"], "structure_targets");
    assert!(
        pipeline_blockers(&options)
            .expect("pipeline blockers")
            .contains(&"rcsb_scope_incomplete".to_string())
    );
    let report = report_source_summary(&options, 0);
    assert!(report.contains("rcsb_scope_incomplete"));
    assert!(report.contains("ready=7 scope=9"));
    assert!(report.contains("sync_rcsb_ligand_bound_scope"));

    write_text(&store.join("install_status.json"), "{}\n").expect("change evidence");
    let stale = evaluate_science_store_readiness(&store).expect("stale readiness");
    assert_eq!(stale.blockers, ["science_store_readiness_stale"]);
    fs::remove_dir_all(&root).expect("cleanup temp root");
}

#[test]
fn canonical_readiness_reports_foldseek_refresh_advisory_without_blocking() {
    let root = temp_root("foldseek_refresh_advisory");
    let store = create_temporary_test_store(&root).expect("science store");
    write_text(
        &store.join("indexes/foldseek/foldseek_target_databases.tsv"),
        &format!(
            "source_id\tmanifest_schema_version\tsource_namespace\tsource_class\tsource_digest\tsource_files\tsource_bytes\tbuilder_version\tscope_mode\tscope_count\tscope_coverage\trefresh_needed\tdb_path\tstatus\nrcsb_mmcif\t{}\tRCSB\tstructure\t{}\t22120\t3996832517\t10.941cd33\ton_demand\t167245\t22120/167245\tfalse\tindexes/foldseek/unit_test/target\tready\n",
            ALIGNMENT_DATABASE_MANIFEST_SCHEMA_VERSION,
            "a".repeat(64),
        ),
    )
    .expect("Foldseek manifest");
    write_text(
        &store.join(RCSB_INDEXED_PDB_IDS_REL_PATH),
        "pdb_id\tstatus\n1ABC\tvalidated\n",
    )
    .expect("indexed PDB identifiers");
    write_gzip(
        &rcsb_coordinate_cache_path(&store, "1ABC"),
        &test_rcsb_mmcif("1ABC"),
    )
    .expect("indexed cached coordinate");
    write_test_science_store_readiness(&store, &[]).expect("rebound readiness");

    let current = evaluate_science_store_readiness(&store).expect("current readiness");
    assert!(current.ready, "{:?}", current.blockers);
    assert!(current.advisories.is_empty());

    write_gzip(
        &rcsb_coordinate_cache_path(&store, "2BBB"),
        &test_rcsb_mmcif("2BBB"),
    )
    .expect("new cached coordinate");
    let readiness = evaluate_science_store_readiness(&store).expect("readiness after cache growth");
    assert!(readiness.ready, "{:?}", readiness.blockers);
    assert_eq!(readiness.advisories, ["rcsb_foldseek_refresh_needed"]);
    assert_eq!(
        readiness.advisory_details[0].evidence,
        "scope_coverage=22120/167245;cached_unindexed_count=1;cached_unindexed_sample=2BBB"
    );
    assert_eq!(
        readiness.advisory_details[0].action,
        "refresh_foldseek_when_convenient"
    );
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: store.clone(),
        input_manifest: root.join("input.json"),
        run_dir: root.join("run"),
    };
    let report = report_source_summary(&options, 0);
    assert!(report.contains("rcsb_foldseek_refresh_needed"));
    assert!(report.contains("refresh_foldseek_when_convenient"));
    fs::remove_dir_all(&root).expect("cleanup temp root");
}

#[test]
fn foldseek_refresh_advisory_ignores_non_rcsb_and_complete_scope_rows() {
    let root = temp_root("foldseek_refresh_advisory_scope_filter");
    let store = create_temporary_test_store(&root).expect("science store");
    write_text(
        &store.join("indexes/foldseek/foldseek_target_databases.tsv"),
        "source_id\tscope_mode\tscope_coverage\trefresh_needed\tstatus\nother_structure\ton_demand\t0/1\ttrue\tready\nrcsb_mmcif\tcomplete\t0/1\ttrue\tready\n",
    )
    .expect("Foldseek manifest");
    write_text(
        &store.join(RCSB_INDEXED_PDB_IDS_REL_PATH),
        "pdb_id\tstatus\n",
    )
    .expect("indexed PDB identifiers");
    write_gzip(
        &rcsb_coordinate_cache_path(&store, "2BBB"),
        &test_rcsb_mmcif("2BBB"),
    )
    .expect("uncached index coordinate");
    write_test_science_store_readiness(&store, &[]).expect("canonical readiness");

    let readiness = evaluate_science_store_readiness(&store).expect("readiness");
    assert!(readiness.ready, "{:?}", readiness.blockers);
    assert!(readiness.advisories.is_empty());
    fs::remove_dir_all(&root).expect("cleanup temp root");
}

#[test]
fn canonical_readiness_missing_and_invalid_have_stable_blockers() {
    let root = temp_root("canonical_readiness_invalid");
    let store = root.join("science_store");
    ensure_dir(&store).expect("science store");
    let missing = evaluate_science_store_readiness(&store).expect("missing readiness");
    assert_eq!(missing.blockers, ["science_store_readiness_missing"]);

    write_text(&store.join("science_store_readiness.json"), "{}\n").expect("invalid readiness");
    let invalid = evaluate_science_store_readiness(&store).expect("invalid readiness");
    assert_eq!(invalid.blockers, ["science_store_readiness_invalid"]);
    fs::remove_dir_all(&root).expect("cleanup temp root");
}

#[test]
fn readiness_evidence_digest_matches_python_contract() {
    let root = temp_root("readiness_evidence_digest");
    let store = root.join("science_store");
    ensure_dir(&store).expect("science store");
    let (empty, empty_digest) = science_readiness_evidence(&store).expect("empty evidence");
    assert!(empty.is_empty());
    assert_eq!(
        empty_digest,
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );

    write_text(
        &store.join("source_requirements.tsv"),
        "source_id\tstatus\n",
    )
    .expect("evidence file");
    write_text(
        &store.join("manifests/source_retention_manifest.tsv"),
        "schema_version\n",
    )
    .expect("source retention evidence");
    write_text(
        &store.join("downloads/rcsb/ligand_bound_pdb_scope.json"),
        "{}\n",
    )
    .expect("RCSB scope evidence");
    let (evidence, digest) = science_readiness_evidence(&store).expect("evidence");
    assert_eq!(
        evidence
            .iter()
            .map(|entry| entry.path.as_str())
            .collect::<Vec<_>>(),
        [
            "source_requirements.tsv",
            "manifests/source_retention_manifest.tsv",
            "downloads/rcsb/ligand_bound_pdb_scope.json",
        ]
    );
    assert_eq!(
        evidence[0].sha256,
        "c98fc19b87dc2a95f7717e75e77f39f30d426017f11522d54592e2920f750268"
    );
    assert_eq!(
        digest,
        "2e9a87483b26f9dca2144590d2b2dd9d551d8ae2658c54a4d6d80d88054f62a6"
    );
    fs::remove_dir_all(&root).expect("cleanup temp root");
}

#[test]
fn runtime_readiness_binds_external_sync_execution_evidence() {
    let root = temp_root("external_sync_readiness_evidence");
    let store = root.join("science_store");
    ensure_dir(&store).expect("science store");
    let execution = store.join("external_sync_execution_manifest.tsv");
    let ready = "source_id\tstatus\tsha256\nrcsb_ligand_bound\tready\tfixture\n";
    write_text(&execution, ready).expect("external sync execution evidence");
    write_test_science_store_readiness(&store, &[]).expect("canonical readiness");

    let accepted = evaluate_science_store_readiness(&store).expect("accepted readiness");
    assert!(accepted.ready);
    assert!(accepted.blockers.is_empty());

    write_text(
        &execution,
        "source_id\tstatus\tsha256\nrcsb_ligand_bound\tfailed\ttampered\n",
    )
    .expect("tampered execution evidence");
    let tampered = evaluate_science_store_readiness(&store).expect("tampered readiness");
    assert_eq!(tampered.blockers, ["science_store_readiness_stale"]);

    write_text(&execution, ready).expect("restored execution evidence");
    write_test_science_store_readiness(&store, &[]).expect("rebound readiness");
    fs::remove_file(&execution).expect("removed execution evidence");
    let removed = evaluate_science_store_readiness(&store).expect("removed readiness");
    assert_eq!(removed.blockers, ["science_store_readiness_stale"]);

    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn runtime_readiness_evidence_binds_small_manifests_without_large_database_hashes() {
    let root = temp_root("chembl_readiness_evidence");
    let store = root.join("science_store");
    let chembl_dir = store.join("downloads/chembl");
    let ligand_similarity_dir = store.join("indexes/ligand_similarity");
    let manifest_dir = store.join("manifests");
    let rcsb_index_dir = store.join("indexes/foldseek/rcsb_mmcif");
    ensure_dir(&chembl_dir).expect("ChEMBL directory");
    ensure_dir(&ligand_similarity_dir).expect("ligand similarity directory");
    ensure_dir(&manifest_dir).expect("manifest directory");
    ensure_dir(&rcsb_index_dir).expect("RCSB index directory");
    write_text(
        &chembl_dir.join("chembl_query.sqlite"),
        "large-database-sentinel\n",
    )
    .expect("database sentinel");
    write_text(
        &chembl_dir.join("chembl_query.manifest.json"),
        "{\"status\":\"ready\"}\n",
    )
    .expect("superseded query manifest");
    write_text(
        &ligand_similarity_dir.join("ligand_similarity.sqlite"),
        "large-compact-database-sentinel\n",
    )
    .expect("compact database sentinel");
    write_text(
        &ligand_similarity_dir.join("ligand_similarity.manifest.json"),
        "{\"schema_version\":\"genoligand_ligand_similarity_index_v2\",\"status\":\"ready\"}\n",
    )
    .expect("compact manifest");
    write_text(
        &manifest_dir.join("source_retention_manifest.tsv"),
        "schema_version\tsource_id\tstate\ngenoligand_source_retention_v1\tchembl\tpruned\n",
    )
    .expect("retention manifest");
    write_text(
        &rcsb_index_dir.join("indexed_pdb_ids.tsv"),
        "pdb_id\n1ABC\n",
    )
    .expect("indexed PDB identifiers");

    let (evidence, _) = science_readiness_evidence(&store).expect("readiness evidence");
    assert!(
        evidence
            .iter()
            .any(|row| row.path == LIGAND_SIMILARITY_MANIFEST_REL_PATH)
    );
    assert!(
        evidence
            .iter()
            .any(|row| row.path == "manifests/source_retention_manifest.tsv")
    );
    assert!(
        evidence
            .iter()
            .any(|row| row.path == RCSB_INDEXED_PDB_IDS_REL_PATH)
    );
    assert!(!evidence.iter().any(|row| row.path.ends_with(".sqlite")));
    assert!(
        !evidence
            .iter()
            .any(|row| row.path == CHEMBL_QUERY_MANIFEST_REL_PATH)
    );

    write_test_science_store_readiness(&store, &[]).expect("canonical readiness");
    write_text(
        &ligand_similarity_dir.join("ligand_similarity.manifest.json"),
        "{\"schema_version\":\"genoligand_ligand_similarity_index_v2\",\"status\":\"changed\"}\n",
    )
    .expect("changed compact manifest");
    let stale = evaluate_science_store_readiness(&store).expect("stale readiness");
    assert_eq!(stale.blockers, ["science_store_readiness_stale"]);
    fs::remove_dir_all(&root).expect("cleanup temp root");
}

#[test]
fn shared_store_readiness_and_pipeline_reject_legacy_5000_row_projection() {
    let root = temp_root("legacy_5000_projection");
    let store = create_temporary_test_store(&root).expect("test store");
    write_text(
        &store.join("index_build_manifest.tsv"),
        "index\tinput_path\tstatus\trows\toutput_path\tnotes\nknown_targets\tdownloads/bindingdb/bindingdb_all.tsv.zip\tused\t5000\tindexes/known_targets.tsv\tcanonical_or_source_table_rows\n",
    )
    .expect("legacy projection manifest");
    write_test_science_store_readiness(
        &store,
        &[(
            "index_projection_only:known_targets",
            "indexes",
            "downloads/bindingdb/bindingdb_all.tsv.zip:legacy_5000_row_projection",
            "build_full_searchable_index",
        )],
    )
    .expect("canonical readiness fixture");
    let readiness = evaluate_science_store_readiness(&store).expect("readiness evaluation");

    assert!(!readiness.ready);
    assert!(
        readiness
            .blockers
            .iter()
            .any(|reason| reason == "index_projection_only:known_targets")
    );

    let input = root.join("input.json");
    let run = root.join("run");
    write_text(
        &input,
        "{\"inputs\":[{\"field\":\"small_molecule\",\"value\":\"CCO\"}],\"files\":[]}\n",
    )
    .expect("input");
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: store,
        input_manifest: input,
        run_dir: run.clone(),
    };
    let error =
        run_science_command(ScienceCommand::PipelineRun, &options).expect_err("blocked pipeline");
    let failure = read_text(&run.join("failure_reason.txt")).expect("failure reasons");

    assert!(error.contains("pipeline_blocked"));
    assert!(failure.contains("index_projection_only:known_targets"));
    fs::remove_dir_all(&root).expect("cleanup temp root");
}

#[test]
fn shared_store_readiness_accepts_complete_5000_row_source_scan() {
    let root = temp_root("complete_5000_source_scan");
    let store = create_temporary_test_store(&root).expect("complete test store");
    write_text(
        &store.join("index_build_manifest.tsv"),
        "index\tinput_path\tstatus\trows\toutput_path\tnotes\nknown_targets\tdownloads/chembl/known_targets.tsv\tused\t5000\tindexes/known_targets.tsv\tcanonical_or_source_table_rows;complete_source_scan\nmetagenome_targets\tunit_test/metagenome_targets.tsv\tused\t1\tindexes/metagenome_targets.tsv\tfull_test_index\nstructure_targets\tunit_test/structure_targets.tsv\tused\t1\tindexes/structure_targets.tsv\tfull_test_index\n",
    )
    .expect("complete index manifest");
    write_test_science_store_readiness(&store, &[]).expect("canonical readiness fixture");

    let readiness = evaluate_science_store_readiness(&store).expect("readiness evaluation");

    assert!(readiness.ready, "{:?}", readiness.blockers);
    fs::remove_dir_all(&root).expect("cleanup temp root");
}

#[test]
fn pipeline_blocks_when_alignment_database_manifest_is_deferred() {
    let root = temp_root("deferred_alignment_manifest");
    let store = create_temporary_test_store(&root).expect("complete test store");
    write_text(
        &store.join("indexes/mmseqs/mmseqs_target_databases.tsv"),
        &format!(
            "source_id\tmanifest_schema_version\tsource_namespace\tsource_class\tsource_digest\tsource_files\tsource_bytes\tbuilder_version\tdb_path\tstatus\nmetagenome\tgenoligand_alignment_database_manifest_v2\tMGnify\tmetagenome\t{}\t1\t1\tmmseqs-test\tindexes/mmseqs/metagenome/target\tdeferred_large_source\n",
            test_source_digest("metagenome")
        ),
    )
    .expect("deferred v2 MMseqs manifest");
    write_test_science_store_readiness(
        &store,
        &[(
            "alignment_manifest_invalid:mmseqs:metagenome",
            "alignment",
            "indexes/mmseqs/mmseqs_target_databases.tsv",
            "migrate_or_rebuild_alignment_database",
        )],
    )
    .expect("canonical readiness fixture");
    let input = root.join("input.json");
    let run = root.join("run");
    write_text(
        &input,
        "{\"inputs\":[{\"field\":\"small_molecule\",\"value\":\"CCO\"}],\"files\":[]}\n",
    )
    .expect("input");
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: store,
        input_manifest: input,
        run_dir: run.clone(),
    };

    let error =
        run_science_command(ScienceCommand::PipelineRun, &options).expect_err("blocked outcome");
    let failure = read_text(&run.join("failure_reason.txt")).expect("failure");

    assert!(error.contains("pipeline_blocked"));
    assert!(failure.contains("alignment_manifest_invalid:mmseqs:metagenome"));
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn prepare_science_store_builds_indexes_from_existing_source_files() {
    let root = temp_root("build_indexes");
    let store = root.join("science_store");
    write_text(
            &store.join("downloads/chembl/known_targets.tsv"),
            "source\ttarget_id\ttarget_name\torganism\ttarget_class\tligand_tokens\tsequence_tokens\tstructure_tokens\tevidence\nChEMBL\tCHEMBL_T1\tAlpha kinase\tHomo sapiens\tknown_human_protein\tlig kinase\talpha kinase\tpocket alpha\tsource_table_row\n",
        )
        .expect("write known source");
    write_text(
        &store.join("downloads/metagenome/orfs.faa"),
        ">MG_ORF_1 candidate oxidase OS=uncultured bacterium\nMKTAYIAKQRQISFVKSHFSRQDILDLWQ\n",
    )
    .expect("write metagenome fasta");
    let options = StoreOptions {
        workspace_root: root.clone(),
        science_store: store.clone(),
    };
    let _ =
        run_store_command(ScienceCommand::PrepareScienceStore, &options).expect("prepare store");
    let known = read_text(&store.join("indexes/known_targets.tsv")).expect("known index");
    let metagenome =
        read_text(&store.join("indexes/metagenome_targets.tsv")).expect("metagenome index");
    let build_manifest =
        read_text(&store.join("index_build_manifest.tsv")).expect("build manifest");
    assert!(known.contains("CHEMBL_T1"));
    assert!(metagenome.contains("MG_ORF_1"));
    assert!(build_manifest.contains("canonical_or_source_table_rows"));
    assert!(build_manifest.contains("fasta_sequence_rows"));
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn prepare_science_store_builds_metagenome_index_from_gzipped_fasta() {
    let root = temp_root("build_gzip_index");
    let store = root.join("science_store");
    write_gzip(
        &store.join("downloads/metagenome/orfs.faa.gz"),
        ">MG_GZ_1 candidate oxidoreductase OS=environmental source\nMSTNPKPQRKTKRNTNRRPQDVKFPGG\n",
    )
    .expect("write gzip fasta");
    let options = StoreOptions {
        workspace_root: root.clone(),
        science_store: store.clone(),
    };
    let _ =
        run_store_command(ScienceCommand::PrepareScienceStore, &options).expect("prepare store");
    let metagenome =
        read_text(&store.join("indexes/metagenome_targets.tsv")).expect("metagenome index");
    assert!(metagenome.contains("MG_GZ_1"));
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn prepare_science_store_builds_metagenome_index_from_nested_mgnify_fasta() {
    let root = temp_root("build_mgnify_index");
    let store = root.join("science_store");
    write_gzip(
        &store.join("downloads/metagenome/mgnify/mgy_clusters.fa.gz"),
        ">MGYP000001 environmental protein OS=metagenome\nMSEQNNTEMTFQIQRIYTKDISFEAPNAPHVFQKDW\n",
    )
    .expect("write mgnify fasta");
    let options = StoreOptions {
        workspace_root: root.clone(),
        science_store: store.clone(),
    };
    let _ =
        run_store_command(ScienceCommand::PrepareScienceStore, &options).expect("prepare store");
    let metagenome =
        read_text(&store.join("indexes/metagenome_targets.tsv")).expect("metagenome index");
    let build_manifest =
        read_text(&store.join("index_build_manifest.tsv")).expect("build manifest");
    assert!(metagenome.contains("MGYP000001"));
    assert!(build_manifest.contains("mgy_clusters.fa.gz"));
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn large_metagenome_fasta_is_reserved_for_persistent_alignment_database() {
    let root = temp_root("large_metagenome_alignment_db");
    let path = root.join("downloads/metagenome/mgnify/mgy_clusters.fa.gz");
    ensure_dir(path.parent().expect("parent")).expect("parent dir");
    write_text(&path, ">MGY\nMSEQNNTEMTFQIQRIYTKDISFEAPNAPHVFQKDW\n").expect("fasta");

    assert!(!metagenome_fasta_requires_alignment_database(&path, 10_000));
    assert!(metagenome_fasta_requires_alignment_database(&path, 10));
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn large_metagenome_binds_ready_mmseqs_database_without_fake_tsv_rows() {
    let root = temp_root("large_metagenome_ready_mmseqs_binding");
    let store = root.join("science_store");
    let source = store.join("downloads/metagenome/mgnify/mgy_clusters.fa.gz");
    fs::create_dir_all(source.parent().expect("source parent")).expect("source parent");
    let source_file = fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&source)
        .expect("sparse metagenome source");
    source_file
        .set_len(LARGE_METAGENOME_FASTA_TSV_MAX_BYTES + 1)
        .expect("large sparse source");
    drop(source_file);
    let database = store.join("indexes/mmseqs/metagenome_mgy_clusters.fa/target");
    write_text(&database, "SEQUENCE\n").expect("database payload");
    write_text(
        &PathBuf::from(format!("{}.dbtype", database.display())),
        "ABCD",
    )
    .expect("database type");
    write_text(
        &PathBuf::from(format!("{}.index", database.display())),
        "0\t0\t9\n",
    )
    .expect("database index");
    write_text(
        &PathBuf::from(format!("{}.lookup", database.display())),
        "0\tMGYP003125700799\t0\n",
    )
    .expect("database lookup");
    write_text(
        &store.join("indexes/mmseqs/mmseqs_target_databases.tsv"),
        &format!(
            "source_id\tmanifest_schema_version\tsource_namespace\tsource_class\tsource_path\tsource_digest\tsource_files\tsource_bytes\tbuilder_version\tdb_path\tstatus\tnotes\nmetagenome\t{}\tMGnify\tmetagenome\tdownloads/metagenome/mgnify/mgy_clusters.fa.gz\t{}\t1\t{}\ttest-builder\tindexes/mmseqs/metagenome_mgy_clusters.fa/target\tready\tvalidated\n",
            ALIGNMENT_DATABASE_MANIFEST_SCHEMA_VERSION,
            "a".repeat(64),
            LARGE_METAGENOME_FASTA_TSV_MAX_BYTES + 1,
        ),
    )
    .expect("alignment manifest");
    let options = StoreOptions {
        workspace_root: root.clone(),
        science_store: store.clone(),
    };
    let build_manifest = store.join("index_build_manifest.tsv");

    build_store_indexes(&options, &build_manifest).expect("build indexes");
    let manifest = read_text(&build_manifest).expect("build manifest");
    let projection =
        read_text(&store.join("indexes/metagenome_targets.tsv")).expect("compatibility table");
    assert!(manifest.contains("alignment_database_used\t0\tindexes/mmseqs"));
    assert!(manifest.contains("full_searchable_alignment_database"));
    assert!(manifest.contains("consumer=mmseqs_alignment_worker"));
    assert!(manifest.contains(&format!("source_digest={}", "a".repeat(64))));
    assert_eq!(projection.lines().count(), 1);

    fs::remove_file(PathBuf::from(format!("{}.lookup", database.display())))
        .expect("remove lookup");
    build_store_indexes(&options, &build_manifest).expect("rebuild without database authority");
    let manifest = read_text(&build_manifest).expect("repaired build manifest");
    assert!(manifest.contains("requires_alignment_database"));
    assert!(!manifest.contains("alignment_database_used"));
    fs::remove_dir_all(&root).expect("cleanup temp root");
}

#[test]
fn prepare_science_store_builds_known_index_from_gzipped_target_table() {
    let root = temp_root("build_gzip_table");
    let store = root.join("science_store");
    write_gzip(
            &store.join("downloads/chembl/known_targets.tsv.gz"),
            "source\ttarget_id\ttarget_name\torganism\ttarget_class\tligand_tokens\tsequence_tokens\tstructure_tokens\tevidence\nChEMBL\tCHEMBL_GZ1\tBeta receptor\tHomo sapiens\tknown_human_protein\tbeta ligand\tbeta receptor\tbinding site\tgzipped_source_table\n",
        )
        .expect("write gzip table");
    let options = StoreOptions {
        workspace_root: root.clone(),
        science_store: store.clone(),
    };
    let _ =
        run_store_command(ScienceCommand::PrepareScienceStore, &options).expect("prepare store");
    let known = read_text(&store.join("indexes/known_targets.tsv")).expect("known index");
    assert!(known.contains("CHEMBL_GZ1"));
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn prepare_science_store_builds_indexes_from_zip_members() {
    let root = temp_root("build_zip_indexes");
    let store = root.join("science_store");
    write_zip(
            &store.join("downloads/chembl/known_targets.zip"),
            "nested/known_targets.tsv",
            "source\ttarget_id\ttarget_name\torganism\ttarget_class\tligand_tokens\tsequence_tokens\tstructure_tokens\tevidence\nChEMBL\tCHEMBL_ZIP1\tGamma enzyme\tHomo sapiens\tknown_human_protein\tgamma ligand\tgamma enzyme\tactive site\tzipped_source_table\n",
        )
        .expect("write zip table");
    write_zip(
            &store.join("downloads/metagenome/orfs.zip"),
            "orfs/orfs.faa",
            ">MG_ZIP_1 candidate transferase OS=environmental source\nMTEYKLVVVGAGGVGKSALTIQLIQNHFVDEYDPTIEDSY\n",
        )
        .expect("write zip fasta");
    let options = StoreOptions {
        workspace_root: root.clone(),
        science_store: store.clone(),
    };
    let _ =
        run_store_command(ScienceCommand::PrepareScienceStore, &options).expect("prepare store");
    let known = read_text(&store.join("indexes/known_targets.tsv")).expect("known index");
    let metagenome =
        read_text(&store.join("indexes/metagenome_targets.tsv")).expect("metagenome index");
    assert!(known.contains("CHEMBL_ZIP1"));
    assert!(metagenome.contains("MG_ZIP_1"));
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn prepare_science_store_reads_every_supported_zip_member() {
    let root = temp_root("build_all_zip_members");
    let store = root.join("science_store");
    let header = "source\ttarget_id\ttarget_name\torganism\ttarget_class\tligand_tokens\tsequence_tokens\tstructure_tokens\tevidence\n";
    let first = format!(
        "{header}ChEMBL\tCHEMBL_ZIP_A\tAlpha target\tHomo sapiens\tknown_human_protein\talpha\talpha target\t\tzip_member_a\n"
    );
    let second = format!(
        "{header}ChEMBL\tCHEMBL_ZIP_B\tBeta target\tHomo sapiens\tknown_human_protein\tbeta\tbeta target\t\tzip_member_b\n"
    );
    write_zip_members(
        &store.join("downloads/chembl/known_targets.zip"),
        &[("z_targets.tsv", &second), ("a_targets.tsv", &first)],
    )
    .expect("known zip");
    write_zip_members(
        &store.join("downloads/metagenome/orfs.zip"),
        &[
            ("z_sequences.faa", ">MG_Z\nMSEQNNTEMTFQIQRIYTKDISFEAP\n"),
            ("a_sequences.faa", ">MG_A\nMKTAYIAKQRQISFVKSHFSRQDIL\n"),
        ],
    )
    .expect("fasta zip");
    let options = StoreOptions {
        workspace_root: root.clone(),
        science_store: store.clone(),
    };

    run_store_command(ScienceCommand::PrepareScienceStore, &options).expect("prepare store");
    let known = read_text(&store.join("indexes/known_targets.tsv")).expect("known index");
    let metagenome =
        read_text(&store.join("indexes/metagenome_targets.tsv")).expect("metagenome index");

    assert!(known.contains("CHEMBL_ZIP_A"));
    assert!(known.contains("CHEMBL_ZIP_B"));
    assert!(metagenome.contains("MG_A"));
    assert!(metagenome.contains("MG_Z"));
    fs::remove_dir_all(&root).expect("cleanup temp root");
}

#[test]
fn prepare_science_store_visits_every_supported_zip_member_in_stable_name_order() {
    let root = temp_root("build_multi_member_zip_indexes");
    let store = root.join("science_store");
    let header = "source\ttarget_id\ttarget_name\torganism\ttarget_class\tligand_tokens\tsequence_tokens\tstructure_tokens\tevidence\n";
    let a_member = format!(
        "{header}ChEMBL\tCHEMBL_A\tAlpha target\tHomo sapiens\tknown_human_protein\talpha\talpha\t\tzip_a\nChEMBL\tCHEMBL_SHARED\tFrom A member\tHomo sapiens\tknown_human_protein\talpha\talpha\t\tzip_a\n"
    );
    let z_member = format!(
        "{header}ChEMBL\tCHEMBL_Z\tZeta target\tHomo sapiens\tknown_human_protein\tzeta\tzeta\t\tzip_z\nChEMBL\tCHEMBL_SHARED\tFrom Z member\tHomo sapiens\tknown_human_protein\tzeta\tzeta\t\tzip_z\n"
    );
    write_zip_members(
        &store.join("downloads/chembl/known_targets.zip"),
        &[
            ("z/targets.tsv", z_member.as_str()),
            ("ignored/readme.md", "ignored"),
            ("a/targets.tsv", a_member.as_str()),
        ],
    )
    .expect("known zip");
    write_zip_members(
        &store.join("downloads/metagenome/orfs.zip"),
        &[
            (
                "z/orfs.faa",
                ">MG_Z\nMTEYKLVVVGAGGVGKSALTIQLIQNHFVDEYDPTIEDSY\n",
            ),
            ("a/orfs.faa", ">MG_A\nMSTNPKPQRKTKRNTNRRPQDVKFPGG\n"),
        ],
    )
    .expect("fasta zip");
    let options = StoreOptions {
        workspace_root: root.clone(),
        science_store: store.clone(),
    };

    run_store_command(ScienceCommand::PrepareScienceStore, &options).expect("prepare store");
    let known = read_text(&store.join("indexes/known_targets.tsv")).expect("known index");
    let metagenome =
        read_text(&store.join("indexes/metagenome_targets.tsv")).expect("metagenome index");

    assert!(known.contains("CHEMBL_A"));
    assert!(known.contains("CHEMBL_Z"));
    assert!(known.contains("CHEMBL_SHARED\tFrom A member"));
    assert!(!known.contains("CHEMBL_SHARED\tFrom Z member"));
    assert!(metagenome.contains("MG_A"));
    assert!(metagenome.contains("MG_Z"));
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn tsv_and_fasta_archive_parsers_emit_bounded_row_batches() {
    let root = temp_root("bounded_archive_batches");
    let table_zip = root.join("targets.zip");
    let fasta_zip = root.join("targets_fasta.zip");
    let row_count = SOURCE_ROW_BATCH_SIZE * 2 + 17;
    let mut table = String::from(
        "source\ttarget_id\ttarget_name\torganism\ttarget_class\tligand_tokens\tsequence_tokens\tstructure_tokens\tevidence\n",
    );
    let mut fasta = String::new();
    for index in 0..row_count {
        table.push_str(&format!(
            "source\tT_{index}\tTarget {index}\tunknown\tknown_protein\t\tseq{index}\t\trow\n"
        ));
        fasta.push_str(&format!(
            ">F_{index}\nMSEQNNTEMTFQIQRIYTKDISFEAPNAPHVFQKDW\n"
        ));
    }
    write_zip_members(&table_zip, &[("targets.tsv", table.as_str())]).expect("table zip");
    write_zip_members(&fasta_zip, &[("targets.faa", fasta.as_str())]).expect("fasta zip");
    let mut table_total = 0usize;
    let mut table_max = 0usize;
    for_each_store_row_batch(&table_zip, "bounded_table", |batch| {
        table_total += batch.len();
        table_max = table_max.max(batch.len());
        Ok(())
    })
    .expect("stream table batches");
    let mut fasta_total = 0usize;
    let mut fasta_max = 0usize;
    for_each_fasta_row_batch(
        &fasta_zip,
        "metagenome",
        "unknown",
        "potential_metagenome_sequence",
        "sequence_record",
        |batch| {
            fasta_total += batch.len();
            fasta_max = fasta_max.max(batch.len());
            Ok(())
        },
    )
    .expect("stream fasta batches");

    assert_eq!(table_total, row_count);
    assert_eq!(fasta_total, row_count);
    assert!(table_max <= SOURCE_ROW_BATCH_SIZE);
    assert!(fasta_max <= SOURCE_ROW_BATCH_SIZE);
    fs::remove_dir_all(&root).expect("cleanup temp root");
}

#[test]
fn prepare_science_store_builds_known_index_from_chembl_sqlite() {
    let root = temp_root("build_chembl_sqlite");
    let store = root.join("science_store");
    write_chembl_sqlite(&store.join("downloads/chembl/chembl.sqlite"))
        .expect("write chembl sqlite");
    let options = StoreOptions {
        workspace_root: root.clone(),
        science_store: store.clone(),
    };
    let _ =
        run_store_command(ScienceCommand::PrepareScienceStore, &options).expect("prepare store");
    let known = read_text(&store.join("indexes/known_targets.tsv")).expect("known index");
    let requirements =
        read_text(&store.join("source_requirements.tsv")).expect("source requirements");
    let build_manifest =
        read_text(&store.join("index_build_manifest.tsv")).expect("build manifest");

    assert!(known.contains("ChEMBL"));
    assert!(known.contains("CHEMBL_T1"));
    assert!(known.contains("Cyclin-dependent kinase 2"));
    assert!(known.contains("known_human_protein"));
    assert!(known.contains("chembl_activity_row"));
    assert!(known.contains("cco"));
    assert!(known.contains("ccn"));
    assert!(known.contains("p24"));
    assert!(known.contains("249"));
    assert!(known.contains("mkt"));
    assert!(requirements.contains("chembl\tChEMBL\tknown_activity\tready"));
    assert!(build_manifest.contains("chembl_sqlite_rows"));
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn prepare_science_store_builds_known_index_from_chembl_sqlite_archive() {
    let root = temp_root("build_chembl_sqlite_archive");
    let store = root.join("science_store");
    let source_sqlite = root.join("chembl_source.sqlite");
    write_chembl_sqlite(&source_sqlite).expect("write chembl sqlite");
    write_tar_gzip_file(
        &store.join("downloads/chembl/chembl_sqlite.tar.gz"),
        "chembl_36/chembl_36_sqlite/chembl_36.db",
        &source_sqlite,
    )
    .expect("write chembl archive");
    let options = StoreOptions {
        workspace_root: root.clone(),
        science_store: store.clone(),
    };
    let _ =
        run_store_command(ScienceCommand::PrepareScienceStore, &options).expect("prepare store");
    let known = read_text(&store.join("indexes/known_targets.tsv")).expect("known index");
    let build_manifest =
        read_text(&store.join("index_build_manifest.tsv")).expect("build manifest");

    assert!(known.contains("ChEMBL"));
    assert!(known.contains("CHEMBL_T1"));
    assert!(build_manifest.contains("chembl_sqlite_archive_rows"));
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn prepare_science_store_does_not_treat_legacy_chembl_query_as_final_authority() {
    let root = temp_root("reject_legacy_chembl_query_authority");
    let store = root.join("science_store");
    let chembl_dir = store.join("downloads/chembl");
    let source_sqlite = root.join("chembl_source.sqlite");
    write_chembl_sqlite(&source_sqlite).expect("write ChEMBL source sqlite");
    write_tar_gzip_file(
        &chembl_dir.join("chembl_sqlite.tar.gz"),
        "chembl_36/chembl_36_sqlite/chembl_36.db",
        &source_sqlite,
    )
    .expect("write ChEMBL archive");
    write_published_chembl_query_sqlite(&chembl_dir.join("chembl_query.sqlite"))
        .expect("write published ChEMBL query database");
    write_text(
        &chembl_dir.join("chembl_query.manifest.json"),
        "{\"status\":\"ready\"}\n",
    )
    .expect("write published ChEMBL manifest");
    let options = StoreOptions {
        workspace_root: root.clone(),
        science_store: store.clone(),
    };

    run_store_command(ScienceCommand::PrepareScienceStore, &options).expect("prepare store");

    let build_manifest =
        read_text(&store.join("index_build_manifest.tsv")).expect("build manifest");
    let known = read_text(&store.join("indexes/known_targets.tsv")).expect("known index");
    assert!(!build_manifest.contains("published_chembl_query_used_by_onsite_matcher"));
    assert!(build_manifest.contains("chembl_sqlite_archive_rows"));
    assert!(known.contains("CHEMBL_T1"));
    fs::remove_dir_all(&root).expect("cleanup temp root");
}

#[test]
fn chembl_installer_archive_uses_unique_streamed_temporary_sqlite_and_one_source_query() {
    let root = temp_root("chembl_archive_unique_temp");
    let source_sqlite = root.join("chembl_source.sqlite");
    let archive = root.join("chembl_sqlite.tar.gz");
    write_chembl_sqlite(&source_sqlite).expect("chembl sqlite");
    write_tar_gzip_file(
        &archive,
        "chembl_36/chembl_36_sqlite/chembl_36.db",
        &source_sqlite,
    )
    .expect("chembl archive");
    let legacy_temp = std::env::temp_dir().join(format!(
        "genoligand_chembl_{}_{}_{}",
        std::process::id(),
        stable_hash(&archive.display().to_string()),
        safe_id("chembl_36.db")
    ));
    write_text(&legacy_temp, "sentinel\n").expect("legacy sentinel");

    let rows = parse_chembl_sqlite_archive(&archive).expect("parse archive");
    let legacy_after = fs::read_to_string(&legacy_temp).ok();
    let install_script = chembl_sqlite_query_script();

    let _ = fs::remove_file(&legacy_temp);
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!rows.is_empty());
    assert_eq!(legacy_after.as_deref(), Some("sentinel\n"));
    assert_eq!(
        install_script.matches("connection.execute(query)").count(),
        1
    );
    assert!(!root.exists());
}

#[test]
fn prepare_science_store_builds_known_index_from_bindingdb_activity_tsv() {
    let root = temp_root("build_bindingdb_table");
    let store = root.join("science_store");
    write_text(
            &store.join("downloads/bindingdb/bindingdb_all.tsv"),
            "Ligand SMILES\tLigand Name\tBindingDB MonomerID\tTarget Name\tTarget Source Organism According to Curator or DataSource\tKi (nM)\tUniProt (SwissProt) Primary ID of Target Chain\tBindingDB Target Chain Sequence\tTarget Chain PDB ID(s)\nCCO\tethanol-like ligand\t5001\tAcetylcholinesterase\tHomo sapiens\t12\tP22303\tMKTAYIAKQRQISFVKSHFSRQDILDLWQ\t1ACJ\n",
        )
        .expect("write bindingdb source");
    let options = StoreOptions {
        workspace_root: root.clone(),
        science_store: store.clone(),
    };
    let _ =
        run_store_command(ScienceCommand::PrepareScienceStore, &options).expect("prepare store");
    let known = read_text(&store.join("indexes/known_targets.tsv")).expect("known index");
    let requirements =
        read_text(&store.join("source_requirements.tsv")).expect("source requirements");

    assert!(known.contains("BindingDB"));
    assert!(known.contains("P22303"));
    assert!(known.contains("Acetylcholinesterase"));
    assert!(known.contains("bindingdb_activity_row"));
    assert!(known.contains("cco"));
    assert!(known.contains("eth"));
    assert!(known.contains("han"));
    assert!(known.contains("mkt"));
    assert!(requirements.contains("bindingdb\tBindingDB\tknown_activity\tready"));
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn prepare_science_store_builds_structure_index_from_downloaded_sources() {
    let root = temp_root("build_structure_index");
    let store = root.join("science_store");
    write_gzip(
        &store.join("downloads/rcsb/mmcif/ab/1abc.cif.gz"),
        &test_rcsb_mmcif("1ABC"),
    )
    .expect("write rcsb structure");
    write_gzip(
        &store.join("downloads/biolip2/BioLiP.txt.gz"),
        "1ABC\tA\tA\t1\tATP\tadenosine triphosphate\n",
    )
    .expect("write biolip2 table");
    let plinder_source = store.join("downloads/plinder/index/annotation_table.parquet");
    write_text(&plinder_source, "PAR1fixturePAR1").expect("write plinder parquet fixture marker");
    write_text(
        &store.join("indexes/plinder_targets.tsv"),
        "source\ttarget_id\ttarget_name\torganism\ttarget_class\tligand_tokens\tsequence_tokens\tstructure_tokens\tevidence\nPLINDER\tPLINDER_SYSTEM_REAL\tPLINDER 3GRT system PLINDER_SYSTEM_REAL\tunknown\tstructure_record\tfad\tchain_a\t3grt system_real\tplinder_2024-06_v2_parquet;structure_id=3GRT;system_id=PLINDER_SYSTEM_REAL;ligand_id=FAD\n",
    )
    .expect("write canonical plinder rows");
    let plinder_canonical = store.join("indexes/plinder_targets.tsv");
    let schema_mapping = json!({
        "system_id": "system_id",
        "entry_pdb_id": "entry_pdb_id",
        "ligand_unique_ccd_code": "ligand_unique_ccd_code",
    });
    write_text(
        &store.join("indexes/plinder_targets.manifest.json"),
        &(json!({
            "schema_version": PLINDER_CANONICAL_MANIFEST_SCHEMA_VERSION,
            "status": "ready",
            "source": {
                "path": plinder_source.display().to_string(),
                "size_bytes": fs::metadata(&plinder_source).expect("source metadata").len(),
                "sha256": sha256_file_hex(&plinder_source).expect("source digest"),
                "parquet_rows": 1,
                "row_groups": 1,
                "schema_field_count": 3,
                "schema_sha256": sha256_file_hex(&plinder_source).expect("schema digest"),
                "schema_mapping": schema_mapping.clone(),
            },
            "schema_mapping": schema_mapping,
            "canonical": {
                "path": plinder_canonical.display().to_string(),
                "row_count": 1,
                "size_bytes": fs::metadata(&plinder_canonical).expect("canonical metadata").len(),
                "sha256": sha256_file_hex(&plinder_canonical).expect("canonical digest"),
            }
        })
        .to_string()
            + "\n"),
    )
    .expect("write canonical plinder manifest");
    write_text(
        &store.join("downloads/binding_moad/bindingmoad.csv"),
        "pdb,ligand,affinity\n2XYZ,ATP,10 nM\n",
    )
    .expect("write moad table");
    write_test_source_manifest(
        &store,
        &["plinder", "biolip2", "binding_moad", "rcsb_ligand_bound"],
    )
    .expect("structure source identities");
    let options = StoreOptions {
        workspace_root: root.clone(),
        science_store: store.clone(),
    };
    let _ =
        run_store_command(ScienceCommand::PrepareScienceStore, &options).expect("prepare store");
    let structure =
        read_text(&store.join("indexes/structure_targets.tsv")).expect("structure index");
    let manifest = read_text(&store.join("index_build_manifest.tsv")).expect("build manifest");
    let status = read_text(&store.join("install_status.json")).expect("install status");

    assert!(structure.contains("RCSB"));
    assert!(structure.contains("1ABC"));
    assert!(structure.contains("BioLiP2"));
    assert!(structure.contains("Binding MOAD"));
    assert!(structure.contains("PLINDER"));
    assert!(structure.contains("PLINDER_SYSTEM_REAL"));
    assert!(manifest.contains("rcsb_structure_file_rows"));
    assert!(manifest.contains("biolip2_interaction_rows"));
    assert!(manifest.contains("binding_moad_complex_rows"));
    assert!(manifest.contains("plinder_canonical_rows"));
    let publication: Value = serde_json::from_str(
        &read_text(&store.join(STRUCTURE_TARGETS_MANIFEST_REL_PATH))
            .expect("structure publication"),
    )
    .expect("structure publication JSON");
    assert_eq!(publication["status"], "ready");
    assert_eq!(publication["rows"], 4);
    for source in ["RCSB", "BioLiP2", "PLINDER", "Binding MOAD"] {
        assert!(publication["contributions"].as_array().is_some_and(|rows| {
            rows.iter()
                .any(|row| row["source"] == source && row["rows"] == 1)
        }));
    }
    validate_structure_targets_publication(&store).expect("validated structure publication");
    assert!(status.contains("\"structure\":"));
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn plinder_canonical_and_structure_publications_reject_same_size_tampering() {
    let root = temp_root("structure_publication_tampering");
    let store = root.join("science_store");
    let plinder_source = store.join("downloads/plinder/index/annotation_table.parquet");
    write_text(&plinder_source, "PAR1fixturePAR1").expect("PLINDER source");
    let canonical = store.join("indexes/plinder_targets.tsv");
    write_text(
        &canonical,
        "source\ttarget_id\ttarget_name\torganism\ttarget_class\tligand_tokens\tsequence_tokens\tstructure_tokens\tevidence\nPLINDER\tSYSTEM_ONE\tSystem one\tunknown\tstructure_record\tatp\tchain_a\t1abc system_one\tplinder_row\n",
    )
    .expect("canonical PLINDER");
    let schema_mapping = json!({
        "system_id": "system_id",
        "entry_pdb_id": "entry_pdb_id",
        "ligand_unique_ccd_code": "ligand_unique_ccd_code",
    });
    write_text(
        &store.join("indexes/plinder_targets.manifest.json"),
        &(json!({
            "schema_version": PLINDER_CANONICAL_MANIFEST_SCHEMA_VERSION,
            "status": "ready",
            "source": {
                "path": plinder_source.display().to_string(),
                "size_bytes": fs::metadata(&plinder_source).expect("source metadata").len(),
                "sha256": sha256_file_hex(&plinder_source).expect("source digest"),
                "parquet_rows": 1,
                "row_groups": 1,
                "schema_field_count": 3,
                "schema_sha256": sha256_file_hex(&plinder_source).expect("schema digest"),
                "schema_mapping": schema_mapping.clone(),
            },
            "schema_mapping": schema_mapping,
            "canonical": {
                "path": canonical.display().to_string(),
                "row_count": 1,
                "size_bytes": fs::metadata(&canonical).expect("metadata").len(),
                "sha256": sha256_file_hex(&canonical).expect("digest"),
            }
        })
        .to_string()
            + "\n"),
    )
    .expect("PLINDER manifest");
    write_test_source_manifest(&store, &["plinder"]).expect("PLINDER source identity");
    validate_plinder_canonical_publication(&store).expect("valid PLINDER publication");
    let manifest_path = store.join("indexes/plinder_targets.manifest.json");
    let legacy_manifest = read_json_value_if_file(&manifest_path).expect("legacy manifest");
    let mut missing_filtered = legacy_manifest.clone();
    missing_filtered["source"]["parquet_rows"] = json!(2);
    write_text(
        &manifest_path,
        &(serde_json::to_string(&missing_filtered).expect("serialize missing filtered") + "\n"),
    )
    .expect("write missing filtered manifest");
    assert!(validate_plinder_canonical_publication(&store).is_err());
    missing_filtered["scan"] = json!({"filtered_rows": 1});
    write_text(
        &manifest_path,
        &(serde_json::to_string(&missing_filtered).expect("serialize conserved scan") + "\n"),
    )
    .expect("write conserved scan manifest");
    validate_plinder_canonical_publication(&store).expect("filtered rows conserve parquet scan");
    missing_filtered["scan"]["filtered_rows"] = json!(2);
    write_text(
        &manifest_path,
        &(serde_json::to_string(&missing_filtered).expect("serialize invalid scan") + "\n"),
    )
    .expect("write invalid scan manifest");
    assert!(validate_plinder_canonical_publication(&store).is_err());
    write_text(
        &manifest_path,
        &(serde_json::to_string(&legacy_manifest).expect("serialize legacy manifest") + "\n"),
    )
    .expect("restore legacy manifest");
    let original = fs::read(&canonical).expect("canonical bytes");
    let mut tampered = original.clone();
    let offset = tampered
        .iter()
        .position(|byte| *byte == b'O')
        .expect("tamper byte");
    tampered[offset] = b'X';
    fs::write(&canonical, &tampered).expect("same-size tamper");
    assert!(validate_plinder_canonical_publication(&store).is_err());
    fs::write(&canonical, original).expect("restore canonical");

    let options = StoreOptions {
        workspace_root: root.clone(),
        science_store: store.clone(),
    };
    let build_manifest = store.join("index_build_manifest.tsv");
    build_store_indexes(&options, &build_manifest).expect("publish structure index");
    validate_structure_targets_publication(&store).expect("valid structure publication");
    let structure = store.join("indexes/structure_targets.tsv");
    let mut bytes = fs::read(&structure).expect("structure bytes");
    let offset = bytes
        .iter()
        .position(|byte| *byte == b'O')
        .expect("structure tamper byte");
    bytes[offset] = b'X';
    fs::write(&structure, bytes).expect("same-size structure tamper");
    assert!(validate_structure_targets_publication(&store).is_err());
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn prepare_science_store_streams_all_structure_table_zip_members() {
    let root = temp_root("stream_structure_zip_members");
    let store = root.join("science_store");
    write_zip_members(
        &store.join("downloads/biolip2/BioLiP.zip"),
        &[
            ("b/BioLiP_b.txt", "2BBB\tB\tB\t1\tFAD\tflavin\n"),
            ("a/BioLiP_a.txt", "1AAA\tA\tA\t1\tATP\tadenosine\n"),
        ],
    )
    .expect("write BioLiP archive");
    write_zip_members(
        &store.join("downloads/binding_moad/bindingmoad.zip"),
        &[
            ("z/moad_z.csv", "pdb,ligand,affinity\n4DDD,FAD,20 nM\n"),
            ("a/moad_a.tsv", "pdb\tligand\taffinity\n3CCC\tATP\t10 nM\n"),
        ],
    )
    .expect("write Binding MOAD archive");
    write_test_source_manifest(&store, &["biolip2", "binding_moad"])
        .expect("structure source identities");
    let options = StoreOptions {
        workspace_root: root.clone(),
        science_store: store.clone(),
    };

    run_store_command(ScienceCommand::PrepareScienceStore, &options).expect("prepare store");
    let structure =
        read_text(&store.join("indexes/structure_targets.tsv")).expect("structure index");

    for target in ["1AAA_A", "2BBB_B", "3CCC", "4DDD"] {
        assert!(structure.contains(target), "missing {target}: {structure}");
    }
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn prepare_science_store_rejects_plinder_parquet_without_canonical_projection() {
    let root = temp_root("plinder_requires_canonical_projection");
    let store = root.join("science_store");
    write_text(
        &store.join("downloads/plinder/index/annotation_table.parquet"),
        "PAR1fixturePAR1",
    )
    .expect("write plinder parquet fixture marker");
    let options = StoreOptions {
        workspace_root: root.clone(),
        science_store: store,
    };

    let error = run_store_command(ScienceCommand::PrepareScienceStore, &options)
        .expect_err("raw PLINDER parquet must not produce placeholder rows");

    assert!(error.contains("plinder_canonical_index_missing"));
    fs::remove_dir_all(root).expect("cleanup temp root");
}

#[test]
fn structure_index_reads_ligand_names_from_mmcif_coordinates() {
    let root = temp_root("structure_mmcif_ligand_names");
    let store = root.join("science_store");
    write_gzip(
        &store.join("downloads/rcsb/mmcif/h4/2h4n.cif.gz"),
        "data_2H4N\n_struct.title 'H94N CARBONIC ANHYDRASE II COMPLEXED WITH ACETAZOLAMIDE'\nloop_\n_chem_comp.id\n_chem_comp.type\n_chem_comp.mon_nstd_flag\n_chem_comp.name\n_chem_comp.pdbx_synonyms\n_chem_comp.formula\n_chem_comp.formula_weight\nAZM non-polymer . 5-ACETAMIDO-1,3,4-THIADIAZOLE-2-SULFONAMIDE ACETAZOLAMIDE 'C4 H6 N4 O3 S2' 222.25\nATOM 1 N N . GLY A 1 1 ? 11.104 13.207 2.100 1.00 20.00 ? 1 GLY A N 1\n",
    )
    .expect("write mmcif");
    write_test_source_manifest(&store, &["rcsb_ligand_bound"]).expect("RCSB source identity");
    let options = StoreOptions {
        workspace_root: root.clone(),
        science_store: store.clone(),
    };

    run_store_command(ScienceCommand::PrepareScienceStore, &options).expect("prepare store");
    let structure =
        read_text(&store.join("indexes/structure_targets.tsv")).expect("structure index");

    assert!(structure.contains("2H4N"));
    assert!(structure.contains("ace"));
    assert!(structure.contains("taz"));
    assert!(structure.contains("azm"));
    assert!(structure.contains("car"));
    assert!(structure.contains("bon"));
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn prepare_science_store_keeps_canonical_target_rows_beyond_source_sampling_limit() {
    let root = temp_root("canonical_target_rows_beyond_sampling");
    let store = root.join("science_store");
    let filler_count = 5_250usize;
    let mut targets = String::from(
        "source\ttarget_id\ttarget_name\torganism\ttarget_class\tligand_tokens\tsequence_tokens\tstructure_tokens\tevidence\n",
    );
    for row in 0..filler_count {
        targets.push_str(&format!(
            "import\tFILLER_{row}\tFiller {row}\tunknown\tpotential_metagenome_sequence\t\tfiller{row}\t\timported_row\n"
        ));
    }
    targets.push_str(
        "import\tTAIL_TARGET\tTail target\tunknown\tpotential_metagenome_sequence\t\tikvav tailtoken\t\timported_row\n",
    );
    write_text(&store.join("imports/metagenome_targets.tsv"), &targets).expect("metagenome import");
    let options = StoreOptions {
        workspace_root: root.clone(),
        science_store: store.clone(),
    };

    run_store_command(ScienceCommand::PrepareScienceStore, &options).expect("prepare store");
    let metagenome =
        read_text(&store.join("indexes/metagenome_targets.tsv")).expect("metagenome index");

    assert!(metagenome.contains("TAIL_TARGET"));
    assert_eq!(metagenome.lines().skip(1).count(), filler_count + 1);
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn prepare_science_store_keeps_fasta_rows_beyond_source_sampling_limit() {
    let root = temp_root("fasta_rows_beyond_sampling");
    let store = root.join("science_store");
    let filler_count = 5_250usize;
    let mut fasta = String::new();
    for row in 0..filler_count {
        fasta.push_str(&format!(">MGFILLER_{row}\nMSEQNNTEMT{row}\n"));
    }
    fasta.push_str(">MGTAIL target tail\nMSEQNNTEMTFQIQRIYTKDISFEAPNAPHVFQKDW\n");
    write_text(&store.join("downloads/metagenome/mgnify/tail.fa"), &fasta)
        .expect("metagenome fasta");
    let options = StoreOptions {
        workspace_root: root.clone(),
        science_store: store.clone(),
    };

    run_store_command(ScienceCommand::PrepareScienceStore, &options).expect("prepare store");
    let metagenome =
        read_text(&store.join("indexes/metagenome_targets.tsv")).expect("metagenome index");

    assert!(metagenome.contains("MGTAIL"));
    assert_eq!(metagenome.lines().skip(1).count(), filler_count + 1);
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn chembl_sqlite_query_has_no_artificial_index_limit() {
    let query = chembl_sqlite_query_script();

    assert!(!query.contains("LIMIT "));
    assert!(!query.contains("LIMIT\n"));
}

#[test]
fn candidate_identity_keeps_database_namespaces_separate() {
    let root = temp_root("candidate_namespace_identity");
    let known = root.join("known.tsv");
    let metagenome = root.join("metagenome.tsv");
    write_text(
        &known,
        "input_id\tsource\ttarget_id\ttarget_name\torganism\ttarget_class\tmatch_score\tmatched_features\tevidence\ninput_1\tChEMBL\tSHARED\tHuman target\tHomo sapiens (Human)\tknown_human_protein\t0.9\tligand\tchembl_activity_row\n",
    )
    .expect("known matches");
    write_text(
        &metagenome,
        "input_id\tsource\ttarget_id\ttarget_name\torganism\ttarget_class\tmatch_score\tmatched_features\tevidence\ninput_1\tMMseqs2\tSHARED\tEnvironmental ORF\tuncultured bacterium\tmetagenome_potential_protein\t0.8\tsequence\tmmseqs_alignment;source_namespace=MGnify\n",
    )
    .expect("metagenome matches");

    let mut candidates = BTreeMap::new();
    merge_match_file(&known, "known_database", &mut candidates).expect("merge known");
    merge_match_file(&metagenome, "metagenome_potential", &mut candidates)
        .expect("merge metagenome");
    let mut candidates = candidates.into_values().collect::<Vec<_>>();
    assign_collision_free_candidate_ids(&mut candidates);

    assert_eq!(candidates.len(), 2);
    assert_eq!(
        candidates
            .iter()
            .map(|candidate| candidate.candidate_id.as_str())
            .collect::<BTreeSet<_>>()
            .len(),
        2
    );
    assert!(
        candidates
            .iter()
            .any(|candidate| candidate.known_score > 0.0)
    );
    assert!(
        candidates
            .iter()
            .any(|candidate| candidate.metagenome_score > 0.0)
    );
    fs::remove_dir_all(&root).expect("cleanup temp root");
}

#[test]
fn candidate_ids_disambiguate_sanitized_target_collisions() {
    let root = temp_root("candidate_sanitized_collision");
    let matches = root.join("matches.tsv");
    write_text(
        &matches,
        "input_id\tsource\ttarget_id\ttarget_name\torganism\ttarget_class\tmatch_score\tmatched_features\tevidence\ninput_1\tChEMBL\tA:B\tTarget colon\tHomo sapiens\tknown_human_protein\t0.9\tligand\tchembl_activity_row\ninput_1\tChEMBL\tA/B\tTarget slash\tHomo sapiens\tknown_human_protein\t0.8\tligand\tchembl_activity_row\n",
    )
    .expect("matches");

    let mut candidates = BTreeMap::new();
    merge_match_file(&matches, "known_database", &mut candidates).expect("merge matches");
    let mut candidates = candidates.into_values().collect::<Vec<_>>();
    assign_collision_free_candidate_ids(&mut candidates);
    let ids = candidates
        .iter()
        .map(|candidate| candidate.candidate_id.clone())
        .collect::<BTreeSet<_>>();

    assert_eq!(ids.len(), 2);
    assert!(
        ids.iter()
            .all(|candidate_id| candidate_id.starts_with("target_A_B_"))
    );
    fs::remove_dir_all(&root).expect("cleanup temp root");
}

#[test]
fn candidate_metadata_prefers_informative_human_rows() {
    let root = temp_root("candidate_metadata_quality");
    let matches = root.join("matches.tsv");
    write_text(
        &matches,
        "input_id\tsource\ttarget_id\ttarget_name\torganism\ttarget_class\tmatch_score\tmatched_features\tevidence\ninput_1\tChEMBL\tT1\tT1\tunknown\tknown_protein\t0.4\tligand\tprojected_index_row\ninput_2\tChEMBL\tT1\tCyclin-dependent kinase 2\tHomo sapiens (Human)\tknown_human_protein\t0.9\tligand\tchembl_onsite_activity_row\n",
    )
    .expect("matches");

    let mut candidates = BTreeMap::new();
    merge_match_file(&matches, "known_database", &mut candidates).expect("merge matches");
    let candidates = candidates.into_values().collect::<Vec<_>>();
    let candidate = candidates
        .iter()
        .find(|candidate| candidate.input_id == "input_2")
        .expect("input 2 candidate");

    assert_eq!(candidates.len(), 2);
    assert_eq!(candidate.target_name, "Cyclin-dependent kinase 2");
    assert_eq!(candidate.organism, "Homo sapiens (Human)");
    assert_eq!(candidate.candidate_scope, "known_human_protein");
    fs::remove_dir_all(&root).expect("cleanup temp root");
}

#[test]
fn chembl_canonical_target_runtime_merges_same_input_record_across_sources() {
    let root = temp_root("chembl_canonical_target_merge");
    let query = root.join("queries.tsv");
    write_text(
        &query,
        "input_id\trecord_id\tquery\ninput_1\trecord_a\tCCO\n",
    )
    .expect("query");

    let chembl = root.join("chembl_query.sqlite");
    write_published_chembl_query_sqlite(&chembl).expect("ChEMBL query database");
    update_published_chembl_target_identity(&chembl, "CCO", "P00918", "CHEMBL205")
        .expect("canonical ChEMBL target identity");
    let chembl_result = root.join("chembl.tsv");
    run_python_script(
        chembl_onsite_query_script(),
        &[
            chembl.display().to_string(),
            query.display().to_string(),
            chembl_result.display().to_string(),
            "run_canonical_target".to_string(),
            "chembl_canonical_target".to_string(),
        ],
        "chembl_canonical_target_query",
    )
    .expect("ChEMBL query");
    let chembl_rows = read_text(&chembl_result).expect("ChEMBL rows");
    let chembl_cols = split_tsv(chembl_rows.trim());
    assert_eq!(chembl_cols[2], "P00918");
    assert!(chembl_cols[7].contains("source_target_id=CHEMBL205"));
    assert!(chembl_cols[8].contains("source_target_id=CHEMBL205"));
    assert!(chembl_cols[9].contains("source_database_id=CHEMBL205"));

    let bindingdb_archive = root.join("bindingdb.zip");
    write_zip(
        &bindingdb_archive,
        "bindingdb.tsv",
        "Ligand SMILES\tBindingDB Ligand Name\tTarget Name\tTarget Source Organism According to Curator or DataSource\tKi (nM)\tUniProt (SwissProt) Primary ID of Target Chain 1\nCCO\tacetaldehyde\tCarbonic anhydrase 2\tHomo sapiens\t12\tP00918\n",
    )
    .expect("BindingDB zip");
    let bindingdb = root.join("bindingdb_query.sqlite");
    publish_test_bindingdb_query(&bindingdb_archive, &bindingdb)
        .expect("published BindingDB query index");
    let bindingdb_result = root.join("bindingdb.tsv");
    run_python_script(
        bindingdb_onsite_query_script(),
        &[
            bindingdb.display().to_string(),
            query.display().to_string(),
            bindingdb_result.display().to_string(),
            test_source_digest("bindingdb"),
            "bindingdb-test-release".to_string(),
            "run_canonical_target".to_string(),
            "bindingdb_canonical_target".to_string(),
        ],
        "bindingdb_canonical_target_query",
    )
    .expect("BindingDB query");
    let bindingdb_rows = read_text(&bindingdb_result).expect("BindingDB rows");

    let matches = root.join("matches.tsv");
    write_text(
        &matches,
        &format!(
            "input_id\tsource\ttarget_id\ttarget_name\torganism\ttarget_class\tmatch_score\tmatched_features\tevidence\tsource_row_provenance\tactivity_value\tligand_identity\tcurrent_run_query_id\trecord_id\n{chembl_rows}{bindingdb_rows}"
        ),
    )
    .expect("combined matches");
    let mut candidates = BTreeMap::new();
    merge_match_file(&matches, "known_database", &mut candidates).expect("merge matches");
    let candidates = candidates.into_values().collect::<Vec<_>>();

    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].target_id, "P00918");
    assert_eq!(candidates[0].identity_namespace, "uniprot");
    assert!(candidates[0].evidence.contains("ChEMBL"));
    assert!(candidates[0].evidence.contains("BindingDB"));
    fs::remove_dir_all(&root).expect("cleanup temp root");
}

#[test]
fn chembl_canonical_target_keeps_different_records_isolated() {
    let root = temp_root("chembl_canonical_target_record_isolation");
    let matches = root.join("matches.tsv");
    write_text(
        &matches,
        "input_id\tsource\ttarget_id\ttarget_name\torganism\ttarget_class\tmatch_score\tmatched_features\tevidence\trecord_id\ninput_1\tChEMBL\tP00918\tCAH2\tHomo sapiens\tknown_human_protein\t0.8\tligand\tchembl_onsite_activity_row;source_target_id=CHEMBL205\trecord_a\ninput_1\tBindingDB\tP00918\tCarbonic anhydrase 2\tHomo sapiens\tknown_human_protein\t0.9\tligand\tbindingdb_onsite_activity_row\trecord_b\n",
    )
    .expect("matches");

    let mut candidates = BTreeMap::new();
    merge_match_file(&matches, "known_database", &mut candidates).expect("merge matches");
    let mut candidates = candidates.into_values().collect::<Vec<_>>();
    assign_collision_free_candidate_ids(&mut candidates);

    assert_eq!(candidates.len(), 2);
    assert_eq!(
        candidates
            .iter()
            .map(|candidate| candidate.record_id.as_str())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["record_a", "record_b"])
    );
    assert_ne!(candidates[0].candidate_id, candidates[1].candidate_id);
    fs::remove_dir_all(&root).expect("cleanup temp root");
}

#[test]
fn chembl_canonical_target_multicomponent_fallback_does_not_merge() {
    let root = temp_root("chembl_canonical_target_multicomponent");
    let matches = root.join("matches.tsv");
    write_text(
        &matches,
        "input_id\tsource\ttarget_id\ttarget_name\torganism\ttarget_class\tmatch_score\tmatched_features\tevidence\trecord_id\ninput_1\tChEMBL\tCHEMBL_COMPLEX_1\tCA complex\tHomo sapiens\tknown_human_protein\t0.8\tligand\tchembl_onsite_activity_row;source_target_id=CHEMBL_COMPLEX_1\trecord_a\ninput_1\tBindingDB\tP00918\tCarbonic anhydrase 2\tHomo sapiens\tknown_human_protein\t0.9\tligand\tbindingdb_onsite_activity_row\trecord_a\n",
    )
    .expect("matches");

    let mut candidates = BTreeMap::new();
    merge_match_file(&matches, "known_database", &mut candidates).expect("merge matches");
    let candidates = candidates.into_values().collect::<Vec<_>>();

    assert_eq!(candidates.len(), 2);
    let fallback = candidates
        .iter()
        .find(|candidate| candidate.target_id == "CHEMBL_COMPLEX_1")
        .expect("multi-component fallback candidate");
    assert_eq!(fallback.identity_namespace, "chembl");
    assert!(candidates.iter().any(|candidate| {
        candidate.target_id == "P00918" && candidate.identity_namespace == "uniprot"
    }));
    fs::remove_dir_all(&root).expect("cleanup temp root");
}

#[test]
fn chembl_canonical_target_rejects_legacy_query_schema() {
    let root = temp_root("chembl_canonical_target_legacy_schema");
    let database = root.join("legacy.sqlite");
    write_legacy_published_chembl_query_sqlite(&database).expect("legacy database");
    let query = root.join("queries.tsv");
    write_text(
        &query,
        "input_id\trecord_id\tquery\ninput_1\trecord_a\tCCO\n",
    )
    .expect("query");
    let error = run_python_script(
        chembl_onsite_query_script(),
        &[
            database.display().to_string(),
            query.display().to_string(),
            root.join("result.tsv").display().to_string(),
            "run_legacy_schema".to_string(),
            "chembl_legacy_schema".to_string(),
        ],
        "chembl_canonical_target_legacy_schema",
    )
    .expect_err("legacy schema must be rejected");
    assert!(
        error.contains("chembl_query_legacy_11_column_schema_rejected"),
        "{error}"
    );
    fs::remove_dir_all(&root).expect("cleanup temp root");
}

#[test]
fn exact_small_molecule_scripts_preserve_stereochemistry_and_normalize_both_sides() {
    let chembl = chembl_onsite_query_script();
    let bindingdb = bindingdb_onsite_query_script();

    assert!(chembl.contains("isomericSmiles=True"));
    assert!(!chembl.contains("create_function"));
    assert!(chembl.contains("canonical_isomeric_smiles IN"));
    assert!(chembl.contains("chembl_query_canonical_index_missing"));
    assert!(bindingdb.contains("isomericSmiles=True"));
    assert!(!bindingdb.contains("isomericSmiles=False"));
}

#[test]
fn prepare_science_store_builds_full_index_without_row_limit_marker() {
    let root = temp_root("full_index_without_limit_marker");
    let store = root.join("science_store");
    let row_count = 30_250usize;
    let mut targets = String::from(
        "source\ttarget_id\ttarget_name\torganism\ttarget_class\tligand_tokens\tsequence_tokens\tstructure_tokens\tevidence\n",
    );
    for row in 0..row_count {
        targets.push_str(&format!(
            "import\tFULL_{row}\tFull target {row}\tunknown\tknown_protein\t\tseq{row}\t\timported_row\n"
        ));
    }
    write_text(&store.join("imports/known_targets.tsv"), &targets).expect("known import");
    let options = StoreOptions {
        workspace_root: root.clone(),
        science_store: store.clone(),
    };

    run_store_command(ScienceCommand::PrepareScienceStore, &options).expect("prepare store");
    let manifest = read_text(&store.join("index_build_manifest.tsv")).expect("index manifest");
    let known = read_text(&store.join("indexes/known_targets.tsv")).expect("known index");
    let status_text = read_text(&store.join("install_status.json")).expect("install status");
    let status: Value = serde_json::from_str(&status_text).expect("status json");

    assert!(!manifest.contains("index_row_limit_reached"));
    assert_eq!(known.lines().skip(1).count(), row_count);
    assert!(
        !status["missing_components"]
            .as_array()
            .expect("missing")
            .iter()
            .any(|item| item
                .as_str()
                .is_some_and(|value| value.starts_with("index_limit:")))
    );
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

fn write_gzip(path: &Path, text: &str) -> Result<(), String> {
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use std::io::Write;

    if let Some(parent) = path.parent() {
        ensure_dir(parent)?;
    }
    let file =
        fs::File::create(path).map_err(|err| format!("create_failed:{}:{err}", path.display()))?;
    let mut encoder = GzEncoder::new(file, Compression::default());
    encoder
        .write_all(text.as_bytes())
        .map_err(|err| format!("gzip_write_failed:{}:{err}", path.display()))?;
    encoder
        .finish()
        .map_err(|err| format!("gzip_finish_failed:{}:{err}", path.display()))?;
    Ok(())
}

fn write_zip(path: &Path, member_name: &str, text: &str) -> Result<(), String> {
    write_zip_members(path, &[(member_name, text)])
}

fn write_zip_members(path: &Path, members: &[(&str, &str)]) -> Result<(), String> {
    use std::io::Write;
    use zip::CompressionMethod;
    use zip::write::FileOptions;

    if let Some(parent) = path.parent() {
        ensure_dir(parent)?;
    }
    let file =
        fs::File::create(path).map_err(|err| format!("create_failed:{}:{err}", path.display()))?;
    let mut writer = zip::ZipWriter::new(file);
    let options: FileOptions<'_, ()> =
        FileOptions::default().compression_method(CompressionMethod::Deflated);
    for (member_name, text) in members {
        writer
            .start_file(*member_name, options)
            .map_err(|err| format!("zip_start_failed:{}:{err}", path.display()))?;
        writer
            .write_all(text.as_bytes())
            .map_err(|err| format!("zip_write_failed:{}:{err}", path.display()))?;
    }
    writer
        .finish()
        .map_err(|err| format!("zip_finish_failed:{}:{err}", path.display()))?;
    Ok(())
}

fn publish_test_bindingdb_query(source: &Path, database: &Path) -> Result<PathBuf, String> {
    let manifest = database.with_extension("manifest.json");
    let source_archive_sha256 =
        sha256_file_hex(source).ok_or_else(|| "test_bindingdb_source_sha256_failed".to_string())?;
    build_bindingdb_query_database(
        source,
        database,
        &manifest,
        &test_source_digest("bindingdb"),
        "bindingdb-test-release",
        &source_archive_sha256,
    )?;
    Ok(manifest)
}

fn publish_test_store_bindingdb_query(store: &Path) -> Result<(), String> {
    let source = store.join(BINDINGDB_SOURCE_REL_PATH);
    let database = store.join(BINDINGDB_QUERY_DATABASE_REL_PATH);
    let manifest = store.join(BINDINGDB_QUERY_MANIFEST_REL_PATH);
    let (source_digest, source_release) = science_store_source_identity(store, "bindingdb")?;
    let source_archive_sha256 = sha256_file_hex(&source)
        .ok_or_else(|| "test_bindingdb_source_sha256_failed".to_string())?;
    build_bindingdb_query_database(
        &source,
        &database,
        &manifest,
        &source_digest,
        &source_release,
        &source_archive_sha256,
    )?;
    Ok(())
}

fn write_ligand_similarity_staging_fixture(store: &Path) -> Result<(), String> {
    write_test_source_manifest(store, &["chembl", "bindingdb"])?;
    write_published_chembl_query_sqlite(&store.join("downloads/chembl/chembl_query.sqlite"))?;
    write_zip(
        &store.join(BINDINGDB_SOURCE_REL_PATH),
        "BindingDB_All.tsv",
        "Ligand SMILES\tBindingDB Ligand Name\tTarget Name\tTarget Source Organism According to Curator or DataSource\tKi (nM)\tUniProt (SwissProt) Primary ID of Target Chain 1\nCCO\tethanol\tTarget one\tHomo sapiens\t12\tP_ONE\n",
    )?;
    publish_test_store_bindingdb_query(store)
}

fn write_compact_ligand_similarity_publication_fixture(
    store: &Path,
) -> Result<LigandSimilarityPublication, String> {
    let database = store.join(LIGAND_SIMILARITY_DATABASE_REL_PATH);
    let manifest = store.join(LIGAND_SIMILARITY_MANIFEST_REL_PATH);
    write_text(&database, "compact-publication-database-v1\n")?;
    let database_sha256 = sha256_file_hex(&database)
        .ok_or_else(|| "compact_publication_fixture_database_hash_failed".to_string())?;
    let source_digest = "a".repeat(64);
    let value = json!({
        "schema_version": LIGAND_SIMILARITY_SCHEMA_VERSION,
        "status": "ready",
        "database": {
            "path": database.display().to_string(),
            "bytes": fs::metadata(&database).map_err(|err| err.to_string())?.len(),
            "sha256": database_sha256,
        },
        "inputs": {
            "chembl": {
                "source_digest": source_digest,
                "source_release": "test-release",
            },
            "bindingdb": {
                "source_digest": source_digest,
                "source_release": "test-release",
            },
        },
        "fingerprint": {
            "type": "morgan_ecfp4",
            "radius": 2,
            "bits": 2048,
            "include_chirality": true,
            "standardization": LIGAND_SIMILARITY_STANDARDIZATION,
            "tautomer_max_tautomers": 100,
            "tautomer_max_transforms": 100,
            "lsh_bands": 0,
            "search_algorithm": "popcount_bitbound_exact_tanimoto",
            "bitbound_formula": "ceil(t*q_popcount) <= db_popcount <= floor(q_popcount/t)",
            "reference": {"name":"FPSim2","license":"MIT","commit":"e03e669"},
            "encoding": "zlib1_rdkit_binary_text",
        },
    });
    write_text(
        &manifest,
        &(serde_json::to_string_pretty(&value).map_err(|err| err.to_string())? + "\n"),
    )?;
    current_ligand_similarity_publication(&ScienceOptions {
        workspace_root: store.to_path_buf(),
        science_store: store.to_path_buf(),
        input_manifest: store.join("input.json"),
        run_dir: store.join("run"),
    })
}

fn compact_publication_fields_fixture(publication: &LigandSimilarityPublication) -> String {
    format!(
        "compact_database_sha256={};compact_manifest_sha256={};compact_schema_version={};compact_build_identity={}",
        publication.database_sha256,
        publication.manifest_sha256,
        publication.schema_version,
        publication.build_identity,
    )
}

fn compact_known_row_fixture(publication: &LigandSimilarityPublication) -> String {
    let fields = compact_publication_fields_fixture(publication);
    format!(
        "input_id\tsource\ttarget_id\ttarget_name\torganism\ttarget_class\tmatch_score\tmatched_features\tevidence\tsource_row_provenance\tactivity_value\tligand_identity\tcurrent_run_query_id\trecord_id\ninput_1\tChEMBL\tCHEMBL_T1\tTarget one\tHomo sapiens\tenzyme\t0.9000\tcanonical_smiles=CCO;tanimoto=1.0\tligand_similarity_index;{fields}\tligand_similarity_index;{fields}\tKi=12 nM\tligand_smiles=CCO\tquery_1\trecord_1\n"
    )
}

fn install_ligand_similarity_builder_fixture(root: &Path, fail_build: bool) -> Result<(), String> {
    let package = root.join("python/genoligand_workers");
    write_text(&package.join("__init__.py"), "")?;
    let mut script = format!(
        "FAIL_BUILD = {}\n",
        if fail_build { "True" } else { "False" }
    );
    script.push_str(
        r#"import hashlib
import json
import os
import sqlite3
import sys
from pathlib import Path

if len(sys.argv) < 2 or sys.argv[1] != "build":
    print("fixture_only_supports_build", file=sys.stderr)
    raise SystemExit(2)
if FAIL_BUILD:
    print("intentional_ligand_similarity_build_failure", file=sys.stderr)
    raise SystemExit(2)

def option(name):
    index = sys.argv.index(name)
    return Path(sys.argv[index + 1]).resolve()

chembl_db = option("--chembl-db")
chembl_manifest = option("--chembl-manifest")
bindingdb_db = option("--bindingdb-db")
bindingdb_manifest = option("--bindingdb-manifest")
output = option("--output-db")
manifest_path = option("--manifest")
output.parent.mkdir(parents=True, exist_ok=True)
manifest_path.parent.mkdir(parents=True, exist_ok=True)
output.unlink(missing_ok=True)

def sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()

def input_record(database, manifest):
    payload = json.loads(manifest.read_text(encoding="utf-8-sig"))
    return {
        "path": str(database),
        "bytes": database.stat().st_size,
        "sha256": payload["database_sha256"],
        "source_digest": payload["source_digest"],
        "source_release": payload["source_release"],
        "manifest_path": str(manifest),
    }

inputs = {
    "chembl": input_record(chembl_db, chembl_manifest),
    "bindingdb": input_record(bindingdb_db, bindingdb_manifest),
}
fingerprint = {
    "type": "morgan_ecfp4",
    "radius": 2,
    "bits": 2048,
    "include_chirality": True,
    "standardization": "rdkit_cleanup_fragment_parent_reionize_canonical_tautomer_v1",
    "tautomer_max_tautomers": 100,
    "tautomer_max_transforms": 100,
    "lsh_bands": 0,
    "search_algorithm": "popcount_bitbound_exact_tanimoto",
    "bitbound_formula": "ceil(t*q_popcount) <= db_popcount <= floor(q_popcount/t)",
    "reference": {"name": "FPSim2", "license": "MIT", "commit": "e03e669"},
    "encoding": "rdkit_binary_text",
}
counts = {"molecules": 1, "targets": 1, "evidence": 1, "hints": 1}
source_identity = {
    source: {
        "source_digest": inputs[source]["source_digest"],
        "source_release": inputs[source]["source_release"],
    }
    for source in ("chembl", "bindingdb")
}
connection = sqlite3.connect(output)
try:
    connection.execute("PRAGMA page_size=512")
    connection.executescript("""
        CREATE TABLE metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL) WITHOUT ROWID;
        CREATE TABLE molecule (
            id INTEGER PRIMARY KEY,
            canonical_smiles TEXT NOT NULL UNIQUE,
            fingerprint BLOB NOT NULL,
            popcount INTEGER NOT NULL
        );
        CREATE TABLE target (
            id INTEGER PRIMARY KEY,
            source TEXT NOT NULL,
            target_id TEXT NOT NULL,
            source_target_id TEXT NOT NULL,
            target_name TEXT NOT NULL,
            organism TEXT NOT NULL,
            target_class TEXT NOT NULL,
            source_release TEXT NOT NULL,
            source_digest TEXT NOT NULL,
            UNIQUE(source, target_id)
        );
        CREATE TABLE evidence (
            molecule_id INTEGER NOT NULL REFERENCES molecule(id),
            target_fk INTEGER NOT NULL REFERENCES target(id),
            activity_score REAL NOT NULL,
            activity TEXT NOT NULL,
            relation_rank INTEGER NOT NULL,
            numeric_rank REAL NOT NULL,
            PRIMARY KEY(molecule_id, target_fk)
        ) WITHOUT ROWID;
        CREATE TABLE structure_hint (
            molecule_id INTEGER NOT NULL REFERENCES molecule(id),
            target_fk INTEGER NOT NULL REFERENCES target(id),
            ligand_code TEXT NOT NULL,
            pdb_ids TEXT NOT NULL,
            target_chain_pdb_ids TEXT NOT NULL,
            PRIMARY KEY(molecule_id, target_fk)
        ) WITHOUT ROWID;
        CREATE INDEX idx_molecule_popcount ON molecule(popcount, id);
    """)
    connection.execute("INSERT INTO molecule VALUES (1, 'CCO', ?, 1)", (b"\x01",))
    connection.execute(
        "INSERT INTO target VALUES (1, 'ChEMBL', 'CHEMBL_T1', 'CHEMBL_T1', "
        "'Target one', 'Homo sapiens', 'known_human_protein', ?, ?)",
        (inputs["chembl"]["source_release"], inputs["chembl"]["source_digest"]),
    )
    connection.execute("INSERT INTO evidence VALUES (1, 1, 0.9, 'Ki=12 nM', 0, 12.0)")
    connection.execute("INSERT INTO structure_hint VALUES (1, 1, 'LIG', '1ABC', '1ABC')")
    metadata = {
        "schema_version": "genoligand_ligand_similarity_index_v2",
        "status": "ready",
        "counts": json.dumps(counts, sort_keys=True, separators=(",", ":")),
        "fingerprint": json.dumps(fingerprint, sort_keys=True, separators=(",", ":")),
        "source_identity": json.dumps(source_identity, sort_keys=True, separators=(",", ":")),
    }
    connection.executemany(
        "INSERT INTO metadata(key, value) VALUES (?, ?)", sorted(metadata.items())
    )
    connection.commit()
finally:
    connection.close()

output_bytes = output.stat().st_size
input_bytes = sum(record["bytes"] for record in inputs.values())
if not 0 < output_bytes < input_bytes:
    print(f"fixture_compact_size_invalid:{output_bytes}>={input_bytes}", file=sys.stderr)
    raise SystemExit(2)
manifest = {
    "schema_version": "genoligand_ligand_similarity_index_v2",
    "status": "ready",
    "database": {
        "path": str(output),
        "bytes": output_bytes,
        "sha256": sha256(output),
    },
    "inputs": inputs,
    "counts": counts,
    "fingerprint": fingerprint,
    "reclaimed_candidate_bytes": input_bytes - output_bytes,
}
manifest_path.write_text(
    json.dumps(manifest, sort_keys=True, indent=2) + "\n", encoding="utf-8"
)
print(json.dumps({"status": "ready", "database": str(output)}, sort_keys=True))
"#,
    );
    write_text(&package.join("ligand_similarity_index.py"), &script)
}

fn write_tar_gzip_file(path: &Path, member_name: &str, source_path: &Path) -> Result<(), String> {
    use flate2::Compression;
    use flate2::write::GzEncoder;

    if let Some(parent) = path.parent() {
        ensure_dir(parent)?;
    }
    let file =
        fs::File::create(path).map_err(|err| format!("create_failed:{}:{err}", path.display()))?;
    let encoder = GzEncoder::new(file, Compression::default());
    let mut builder = tar::Builder::new(encoder);
    builder
        .append_path_with_name(source_path, member_name)
        .map_err(|err| format!("tar_append_failed:{}:{err}", path.display()))?;
    let encoder = builder
        .into_inner()
        .map_err(|err| format!("tar_finish_failed:{}:{err}", path.display()))?;
    encoder
        .finish()
        .map_err(|err| format!("gzip_finish_failed:{}:{err}", path.display()))?;
    Ok(())
}

fn test_source_digest(_source_id: &str) -> String {
    "a".repeat(64)
}

fn write_test_source_manifest(store: &Path, source_ids: &[&str]) -> Result<(), String> {
    let rows = source_ids
        .iter()
        .map(|source_id| (*source_id, test_source_digest(source_id)))
        .collect::<Vec<_>>();
    write_test_source_manifest_with_digests(store, &rows)
}

fn write_test_source_manifest_with_digests(
    store: &Path,
    sources: &[(&str, String)],
) -> Result<(), String> {
    let mut manifest = String::from(
        "source_id\tsource_label\tsource_class\tlocal_path\tstatus\tstorage_state\tfile_count\tbytes\tsha256\tprovenance\trelease_identity\ttrust_source\n",
    );
    for (source_id, digest) in sources {
        manifest.push_str(&format!(
            "{source_id}\t{source_id}\tunit_test\tdownloads/{source_id}\tready\tunit_test\t1\t1\t{digest}\tunit_test\t{source_id}-test-release\tunit_test\n",
        ));
    }
    write_text(&store.join("manifests/source_file_manifest.tsv"), &manifest)
}

fn write_test_pruned_alignment_identity_fixture(store: &Path) -> Result<(), String> {
    let digest = test_source_digest("alignment");
    let mut source_manifest = String::from(
        "source_id\tsource_label\tsource_class\tlocal_path\tstatus\tstorage_state\tfile_count\tbytes\tsha256\tprovenance\trelease_identity\ttrust_source\n",
    );
    for (source_id, local_path, storage_state, files, bytes) in [
        ("chembl", "downloads/chembl", "unit_test", 1, 1),
        ("bindingdb", "downloads/bindingdb", "unit_test", 1, 1),
        (
            "plinder",
            "downloads/plinder/index",
            "indexed_source_pruned",
            1,
            1,
        ),
        (
            "biolip2",
            "downloads/biolip2",
            "indexed_source_pruned",
            6,
            6,
        ),
        (
            "rcsb_ligand_bound",
            "downloads/rcsb/mmcif",
            "unit_test",
            1,
            1,
        ),
        (
            "uniprot_human_reviewed",
            "downloads/uniprot/uniprot_human_reviewed.fasta.gz",
            "indexed_source_pruned",
            1,
            1,
        ),
        (
            "metagenome_potential",
            "downloads/metagenome/mgnify",
            "indexed_source_pruned",
            4,
            4,
        ),
    ] {
        source_manifest.push_str(&format!(
            "{source_id}\t{source_id}\tunit_test\t{local_path}\tready\t{storage_state}\t{files}\t{bytes}\t{digest}\tunit_test\t{source_id}-test-release\tunit_test\n"
        ));
    }
    write_text(
        &store.join("manifests/source_file_manifest.tsv"),
        &source_manifest,
    )?;
    let payload = |path: &str, kind: &str, files: u64, bytes: u64| {
        json!([{
            "path": path,
            "kind": kind,
            "file_count": files,
            "bytes": bytes,
            "sha256": digest,
            "content_sha256": digest,
        }])
        .to_string()
    };
    let mut retention = String::from(
        "schema_version\tsource_id\tsource_local_path\tstate\tsource_files\tsource_bytes\tsource_manifest_sha256\trelease_identity\ttrust_source\tpayload_records_json\tderived_paths\n",
    );
    for (source_id, local_path, files, bytes, payload_records, derived) in [
        (
            "uniprot_human_reviewed",
            "downloads/uniprot/uniprot_human_reviewed.fasta.gz",
            1,
            1,
            payload(
                "downloads/uniprot/uniprot_human_reviewed.fasta.gz",
                "file",
                1,
                1,
            ),
            "indexes/mmseqs/mmseqs_target_databases.tsv",
        ),
        (
            "biolip2",
            "downloads/biolip2",
            6,
            6,
            payload("downloads/biolip2", "directory", 6, 6),
            "indexes/mmseqs/mmseqs_target_databases.tsv;indexes/structure_targets.tsv;indexes/structure_targets.manifest.json",
        ),
        (
            "metagenome_potential",
            "downloads/metagenome/mgnify",
            4,
            4,
            payload(
                "downloads/metagenome/mgnify/mgy_clusters.fa.gz",
                "file",
                1,
                1,
            ),
            "indexes/mmseqs/mmseqs_target_databases.tsv",
        ),
        (
            "plinder",
            "downloads/plinder/index",
            1,
            1,
            payload("downloads/plinder/index", "directory", 1, 1),
            "indexes/structure_targets.tsv;indexes/structure_targets.manifest.json",
        ),
    ] {
        retention.push_str(&format!(
            "{}\t{source_id}\t{local_path}\tpruned\t{files}\t{bytes}\t{digest}\t{source_id}-test-release\tunit_test\t{payload_records}\t{derived}\n",
            SOURCE_RETENTION_SCHEMA_VERSION
        ));
    }
    write_text(
        &store.join("manifests/source_retention_manifest.tsv"),
        &retention,
    )
}

fn write_alignment_database_fixture(
    store: &Path,
    tool: &str,
    source_id: &str,
    source_namespace: &str,
    source_class: &str,
) -> Result<(), String> {
    let db_rel = format!("indexes/{tool}/{source_id}/target");
    let prefix = store.join(&db_rel);
    ensure_dir(prefix.parent().expect("alignment database parent"))?;
    write_text(&prefix, "unit-test-database\n")?;
    write_text(
        &PathBuf::from(format!("{}.dbtype", prefix.display())),
        "1234",
    )?;
    write_text(
        &PathBuf::from(format!("{}.index", prefix.display())),
        "index\n",
    )?;
    write_text(
        &PathBuf::from(format!("{}.lookup", prefix.display())),
        "lookup\n",
    )?;
    write_text(
        &store.join(format!("indexes/{tool}/{tool}_target_databases.tsv")),
        &format!(
            "source_id\tmanifest_schema_version\tsource_namespace\tsource_class\tsource_digest\tsource_files\tsource_bytes\tbuilder_version\tdb_path\tstatus\n{source_id}\tgenoligand_alignment_database_manifest_v2\t{source_namespace}\t{source_class}\t{}\t1\t1\t{tool}-test\t{db_rel}\tready\n",
            test_source_digest(source_id)
        ),
    )
}

fn write_published_chembl_query_manifest(path: &Path) -> Result<(), String> {
    let database_bytes = fs::metadata(path)
        .map_err(|err| format!("published ChEMBL fixture metadata failed: {err}"))?
        .len();
    let database_sha256 = sha256_file_hex(path)
        .ok_or_else(|| "published ChEMBL fixture digest failed".to_string())?;
    write_text(
        &path.with_extension("manifest.json"),
        &format!(
            "{}\n",
            json!({
                "schema_version": CHEMBL_QUERY_MANIFEST_SCHEMA_VERSION,
                "status": "ready",
                "database": path.display().to_string(),
                "database_bytes": database_bytes,
                "database_sha256": database_sha256,
                "rows": 1,
                "table": "genoligand_onsite_activity",
                "source_release": "chembl-test-release",
                "source_digest": test_source_digest("chembl"),
            })
        ),
    )
}

fn write_published_chembl_query_sqlite(path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        ensure_dir(parent)?;
    }
    run_python_script(
        r#"
import pathlib
import sqlite3
import sys

path = pathlib.Path(sys.argv[1])
source_digest = sys.argv[2]
source_release = sys.argv[3]
path.parent.mkdir(parents=True, exist_ok=True)
connection = sqlite3.connect(path)
connection.executescript("""
CREATE TABLE genoligand_onsite_activity (
    canonical_isomeric_smiles TEXT NOT NULL,
    target_id TEXT NOT NULL,
    source_target_id TEXT NOT NULL,
    target_name TEXT NOT NULL,
    organism TEXT NOT NULL,
    compound_name TEXT NOT NULL,
    standard_type TEXT NOT NULL,
    standard_relation TEXT NOT NULL,
    standard_value REAL NOT NULL,
    standard_units TEXT NOT NULL,
    source_release TEXT NOT NULL,
    source_digest TEXT NOT NULL
);
CREATE INDEX idx_genoligand_onsite_activity_smiles
ON genoligand_onsite_activity(canonical_isomeric_smiles);
""")
connection.executemany(
    "INSERT INTO genoligand_onsite_activity VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    [
        ("CCO", "CHEMBL_T1", "CHEMBL_T1", "Cyclin-dependent kinase 2", "Homo sapiens", "compound one", "IC50", "=", 42.0, "nM", source_release, source_digest),
        ("CCN", "CHEMBL_T1", "CHEMBL_T1", "Cyclin-dependent kinase 2", "Homo sapiens", "compound two", "Ki", "=", 12.0, "nM", source_release, source_digest),
    ],
)
connection.commit()
connection.close()
"#,
        &[
            path.display().to_string(),
            test_source_digest("chembl"),
            "chembl-test-release".to_string(),
        ],
        "published_chembl_query_fixture",
    )?;
    write_published_chembl_query_manifest(path)
}

fn update_published_chembl_target_identity(
    path: &Path,
    smiles: &str,
    target_id: &str,
    source_target_id: &str,
) -> Result<(), String> {
    run_python_script(
        r#"
import sqlite3
import sys

connection = sqlite3.connect(sys.argv[1])
cursor = connection.execute(
    "UPDATE genoligand_onsite_activity SET target_id = ?, source_target_id = ? WHERE canonical_isomeric_smiles = ?",
    (sys.argv[3], sys.argv[4], sys.argv[2]),
)
if cursor.rowcount != 1:
    raise RuntimeError("unexpected fixture target count")
connection.commit()
connection.close()
"#,
        &[
            path.display().to_string(),
            smiles.to_string(),
            target_id.to_string(),
            source_target_id.to_string(),
        ],
        "published_chembl_target_identity_fixture",
    )?;
    write_published_chembl_query_manifest(path)
}

fn write_legacy_published_chembl_query_sqlite(path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        ensure_dir(parent)?;
    }
    run_python_script(
        r#"
import sqlite3
import sys

connection = sqlite3.connect(sys.argv[1])
connection.executescript("""
CREATE TABLE genoligand_onsite_activity (
    canonical_isomeric_smiles TEXT NOT NULL,
    target_id TEXT NOT NULL,
    target_name TEXT NOT NULL,
    organism TEXT NOT NULL,
    compound_name TEXT NOT NULL,
    standard_type TEXT NOT NULL,
    standard_relation TEXT NOT NULL,
    standard_value REAL NOT NULL,
    standard_units TEXT NOT NULL,
    source_release TEXT NOT NULL,
    source_digest TEXT NOT NULL
);
CREATE INDEX idx_genoligand_onsite_activity_smiles
ON genoligand_onsite_activity(canonical_isomeric_smiles);
""")
connection.commit()
connection.close()
"#,
        &[path.display().to_string()],
        "legacy_published_chembl_query_fixture",
    )?;
    write_published_chembl_query_manifest(path)
}

fn write_chembl_sqlite(path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        ensure_dir(parent)?;
    }
    run_python_script(
        r#"
import pathlib
import sqlite3
import sys

path = pathlib.Path(sys.argv[1])
path.parent.mkdir(parents=True, exist_ok=True)
connection = sqlite3.connect(path)
connection.executescript("""
CREATE TABLE target_dictionary (
    tid INTEGER PRIMARY KEY,
    chembl_id TEXT NOT NULL,
    pref_name TEXT,
    organism TEXT,
    target_type TEXT
);
CREATE TABLE assays (
    assay_id INTEGER PRIMARY KEY,
    tid INTEGER NOT NULL
);
CREATE TABLE molecule_dictionary (
    molregno INTEGER PRIMARY KEY,
    pref_name TEXT
);
CREATE TABLE compound_structures (
    molregno INTEGER PRIMARY KEY,
    canonical_smiles TEXT
);
CREATE TABLE activities (
    activity_id INTEGER PRIMARY KEY,
    assay_id INTEGER NOT NULL,
    molregno INTEGER NOT NULL,
    standard_type TEXT,
    standard_relation TEXT,
    standard_value REAL,
    standard_units TEXT
);
CREATE TABLE target_components (
    tid INTEGER NOT NULL,
    component_id INTEGER NOT NULL
);
CREATE TABLE component_sequences (
    component_id INTEGER PRIMARY KEY,
    accession TEXT,
    sequence TEXT
);
INSERT INTO target_dictionary VALUES
    (1, 'CHEMBL_T1', 'Cyclin-dependent kinase 2', 'Homo sapiens', 'SINGLE PROTEIN');
INSERT INTO assays VALUES (100, 1);
INSERT INTO molecule_dictionary VALUES
    (10, 'compound one'),
    (11, 'compound two');
INSERT INTO compound_structures VALUES
    (10, 'CCO'),
    (11, 'CCN');
INSERT INTO activities VALUES
    (1000, 100, 10, 'IC50', '=', 42, 'nM'),
    (1001, 100, 11, 'Ki', '=', 12, 'nM');
INSERT INTO target_components VALUES (1, 900);
INSERT INTO component_sequences VALUES
    (900, 'P24941', 'MKTAYIAKQRQISFVKSHFSRQDILDLWQ');
""")
connection.commit()
"#,
        &[path.display().to_string()],
        "chembl_sqlite_fixture",
    )
    .map(|_| ())
}

fn write_many_published_chembl_activities(
    path: &Path,
    targets_per_input: usize,
) -> Result<(), String> {
    write_published_chembl_query_sqlite(path)?;
    run_python_script(
        r#"
import sqlite3
import sys

target_count = int(sys.argv[2])
source_digest = sys.argv[3]
source_release = sys.argv[4]
connection = sqlite3.connect(sys.argv[1])
activities = []
for group in ("CCO", "CCN"):
    for index in range(target_count):
        target_id = f"CHEMBL_{group}_{index}"
        activities.append((group, target_id, target_id, f"{group} target {index}", "Homo sapiens", group, "Ki", "=", float(index + 1), "nM", source_release, source_digest))
activities.extend([
    ("CCO", "CHEMBL_RELATION_BEST", "CHEMBL_RELATION_BEST", "Relation best target", "Homo sapiens", "ethanol", "IC50", ">", 1.0, "nM", source_release, source_digest),
    ("CCO", "CHEMBL_RELATION_BEST", "CHEMBL_RELATION_BEST", "Relation best target", "Homo sapiens", "ethanol", "IC50", "=", 100.0, "nM", source_release, source_digest),
    ("CCO", "CHEMBL_RELATION_CENSORED", "CHEMBL_RELATION_CENSORED", "Censored relation target", "Homo sapiens", "ethanol", "Ki", "<", 10.0, "nM", source_release, source_digest),
])
connection.executemany(
    "INSERT INTO genoligand_onsite_activity VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    activities,
)
connection.commit()
connection.close()
"#,
        &[
            path.display().to_string(),
            targets_per_input.to_string(),
            test_source_digest("chembl"),
            "chembl-test-release".to_string(),
        ],
        "published_chembl_many_fixture",
    )?;
    write_published_chembl_query_manifest(path)
}

fn add_published_chembl_activity_rows(
    path: &Path,
    rows: &[(String, String, String, f64)],
) -> Result<(), String> {
    let payload = rows
        .iter()
        .map(|(target_id, smiles, relation, value)| {
            json!({
                "target_id": target_id,
                "smiles": smiles,
                "relation": relation,
                "value": value,
            })
        })
        .collect::<Vec<_>>();
    let payload_path = path.with_extension("activity_rows.json");
    write_text(&payload_path, &Value::Array(payload).to_string())?;
    let result = run_python_script(
        r#"
import json
import sqlite3
import sys

connection = sqlite3.connect(sys.argv[1])
with open(sys.argv[2], "r", encoding="utf-8") as handle:
    rows = json.load(handle)
source_digest = sys.argv[3]
source_release = sys.argv[4]
for row in rows:
    target_id = row["target_id"]
    connection.execute(
        "INSERT INTO genoligand_onsite_activity VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        (
            row["smiles"], target_id, target_id, f"Target {target_id}", "Homo sapiens",
            f"Compound {target_id}", "IC50", row["relation"], row["value"], "nM",
            source_release, source_digest,
        ),
    )
connection.commit()
connection.close()
"#,
        &[
            path.display().to_string(),
            payload_path.display().to_string(),
            test_source_digest("chembl"),
            "chembl-test-release".to_string(),
        ],
        "published_chembl_activity_fixture",
    );
    let _ = fs::remove_file(payload_path);
    result?;
    write_published_chembl_query_manifest(path)
}

fn temp_root(label: &str) -> PathBuf {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_millis();
    let root = std::env::temp_dir().join(format!("genoligand_science_{label}_{millis}"));
    install_query_structure_worker_fixture(&root).expect("query structure worker fixture");
    root
}

fn write_md_binding_manifests(
    run_dir: &Path,
    candidate_id: &str,
    input_id: &str,
    canonical_value: &str,
) -> Result<(), String> {
    write_text(
        &run_dir.join("complex_candidate_selection.tsv"),
        &format!(
            "candidate_id\tinput_id\trecord_id\ttarget_id\ttarget_name\torganism\ttarget_class\tsource\tpdb_id\tevidence\n{}\t{}\trecord_1\tT1\tTarget 1\tHomo sapiens\tenzyme\tRCSB\t1ABC\tstructure match\n",
            clean_cell(candidate_id),
            clean_cell(input_id)
        ),
    )?;
    write_text(
        &run_dir.join("input_conversion_manifest.tsv"),
        &format!(
            "{INPUT_CONVERSION_HEADER}\n{}\trecord_1\tsmall_molecule\tsmall_molecule\tsmall_molecule\t{}\tconverted\tidentity\tsmiles\tinline\t\t{}\t\t\t\t0\t0\t0\n",
            clean_cell(input_id),
            encode_tsv_cell(canonical_value),
            "a".repeat(64)
        ),
    )
}

#[test]
fn coordinate_top_n_uses_independent_docking_and_md_jobs() {
    let root = temp_root("coordinate_top_n_jobs");
    install_not_applicable_worker_fixtures(&root).expect("worker fixtures");
    let run = root.join("run");
    let store = root.join("science_store");
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: store.clone(),
        input_manifest: root.join("input.json"),
        run_dir: run.clone(),
    };
    normalize_and_compute_fixture(&options, "small_molecule", "CCO").expect("input conversion");

    let mut known = String::from(
        "input_id\tsource\ttarget_id\ttarget_name\torganism\ttarget_class\tmatch_score\tmatched_features\tevidence\tprovenance\tactivity\tstructure\tstructure_source\trecord_id\n",
    );
    for (index, (target_id, pdb_id)) in [
        ("P00001", "1AAA"),
        ("P00002", "2BBB"),
        ("P00003", "3CCC"),
        ("P00004", "4DDD"),
    ]
    .into_iter()
    .enumerate()
    {
        known.push_str(&format!(
            "input_1\tBindingDB\t{target_id}\tTarget {}\tHomo sapiens\tenzyme\t{}\tligand_code=LIG;pdb_ids={pdb_id}\tbindingdb_onsite_activity_row;Ki (nM)={};pdb_ids={pdb_id}\t\t\t\t\trecord_1\n",
            index + 1,
            1.0 - index as f64 * 0.1,
            10 + index,
        ));
        let split = &pdb_id.to_ascii_lowercase()[1..3];
        let coordinate = store
            .join("downloads/rcsb/mmcif")
            .join(split)
            .join(format!("{}.cif", pdb_id.to_ascii_lowercase()));
        write_text(
            &coordinate,
            &format!(
                "data_{pdb_id}\n_struct.title 'TARGET {target_id}'\nATOM 1 N N . GLY A 1 1 ? 1.0 1.0 1.0 1.00 20.00 ? 1 GLY A N 1\n"
            ),
        )
        .expect("coordinate");
    }
    write_text(&run.join("known_database_matches.tsv"), &known).expect("known matches");

    assert!(write_coordinate_backed_candidate_selection(&options, &[]).expect("selection"));
    let selection =
        read_simple_tsv_rows(&run.join("complex_candidate_selection.tsv")).expect("selection rows");
    assert_eq!(selection.len(), COMPLEX_CANDIDATE_TOP_N);
    assert!(selection.iter().all(|row| {
        row.get("record_id").map(String::as_str) == Some("record_1")
            && row
                .get("receptor_artifact")
                .is_some_and(|path| run.join(path).is_file())
            && row
                .get("coordinate_sha256")
                .is_some_and(|digest| valid_sha256_hex(digest))
    }));

    let result = run_science_command(ScienceCommand::DynamicsScreen, &options)
        .expect("direct dynamics jobs");
    assert!(result.contains("dynamics_status=executed"));
    let docking_index =
        read_json_value_if_file(&run.join("tool_outputs/docking_manifest_index.json"))
            .expect("docking index");
    let md_index = read_json_value_if_file(&run.join("tool_outputs/md_manifest_index.json"))
        .expect("md index");
    let invocation_ids = [&docking_index, &md_index]
        .into_iter()
        .flat_map(|index| index["entries"].as_array().expect("entries"))
        .filter_map(|entry| entry["invocation_id"].as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(invocation_ids.len(), 1);
    assert_eq!(
        invocation_ids.into_iter().next(),
        Some(
            read_text(&run.join(WORKER_INVOCATION_FILE))
                .expect("worker invocation")
                .trim()
        )
    );
    for (index, schema) in [
        (docking_index, "genoligand_docking_manifest_index_v1"),
        (md_index, "genoligand_md_manifest_index_v1"),
    ] {
        assert_eq!(index["schema_version"], schema);
        let entries = index["entries"].as_array().expect("entries");
        assert_eq!(entries.len(), COMPLEX_CANDIDATE_TOP_N);
        assert_eq!(
            entries
                .iter()
                .filter_map(|entry| entry["job_id"].as_str())
                .collect::<BTreeSet<_>>()
                .len(),
            COMPLEX_CANDIDATE_TOP_N
        );
        assert!(entries.iter().all(|entry| {
            entry["status"] == "not_ready"
                && entry["record_id"] == "record_1"
                && entry["job_id"]
                    .as_str()
                    .is_some_and(|value| value.contains("__target_"))
                && entry["invocation_id"]
                    .as_str()
                    .is_some_and(|value| !value.is_empty())
        }));
    }
    assert_eq!(
        rows_in_tsv(&run.join("tool_outputs/docking_outcomes.tsv")).expect("outcomes"),
        COMPLEX_CANDIDATE_TOP_N
    );
    assert_eq!(
        rows_in_tsv(&run.join("tool_outputs/docking_summary.tsv")).expect("summary"),
        COMPLEX_CANDIDATE_TOP_N
    );

    let mut ranking = String::from(
        "rank\tcandidate_id\ttarget_id\ttarget_name\tcandidate_scope\torganism\tfinal_score\tknown_database_score\tmetagenome_score\tstructure_score\tdocking_score\tmd_score\tevidence_summary\tstatus\tinput_id\trecord_id\n",
    );
    for (index, row) in selection.iter().enumerate() {
        ranking.push_str(&format!(
            "{}\t{}\t{}\tTarget\tknown\tHomo sapiens\t0.5\t0.5\t0\t1\t0\t0\tevidence\tcomputed_prediction\tinput_1\trecord_1\n",
            index + 1,
            row["candidate_id"],
            row["target_id"],
        ));
    }
    write_text(&run.join("target_candidate_ranking.tsv"), &ranking).expect("ranking");
    write_score_decomposition(&options).expect("decomposition");
    let decomposition = read_text(&run.join("score_decomposition.tsv")).expect("decomposition");
    for row in selection {
        let candidate_id = &row["candidate_id"];
        assert!(decomposition.contains(&format!(
            "{candidate_id}\tcoordinate_backed_candidate\t1.0000"
        )));
        assert!(
            decomposition.contains(&format!("{candidate_id}\tdocking_outcome_present\t1.0000"))
        );
        assert!(decomposition.contains(&format!("{candidate_id}\tmd_outcome_present\t1.0000")));
    }
    fs::remove_dir_all(&root).expect("cleanup temp root");
}

#[test]
fn candidate_shortlist_is_stable_resets_rank_per_input_and_uses_preliminary_evidence_only() {
    let root = temp_root("candidate_shortlist_stable");
    let options = indexed_fixture_options(&root);
    write_input_kind_manifest(
        &options,
        &[
            ("input_1", "record_1", "small_molecule"),
            ("input_2", "record_1", "small_molecule"),
        ],
    )
    .expect("input kinds");
    let header = "input_id\tsource\ttarget_id\ttarget_name\torganism\ttarget_class\tmatch_score\tmatched_features\tevidence\tsource_database_id\tcurrent_run_query_id\trecord_id\n";
    let rows = [
        "input_2\tKnown\tT9\tTarget 9\tHomo sapiens\tenzyme\t0.6000\tfeature\tknown evidence\tKnown\tquery\trecord_1",
        "input_1\tKnown\tT2\tTarget 2\tHomo sapiens\tenzyme\t0.7000\tfeature\tknown evidence\tKnown\tquery\trecord_1",
        "input_1\tKnown\tT1\tTarget 1\tHomo sapiens\tenzyme\t0.9000\tfeature\tknown evidence\tKnown\tquery\trecord_1",
    ];
    write_text(
        &options.run_dir.join("known_database_matches.tsv"),
        &format!("{header}{}\n", rows.join("\n")),
    )
    .expect("known matches");
    for name in [
        "metagenome_potential_matches.tsv",
        "structure_interaction_matches.tsv",
    ] {
        write_text(&options.run_dir.join(name), header).expect("empty matches");
    }

    assert_eq!(write_candidate_shortlist(&options).expect("shortlist"), 3);
    let first = read_text(&options.run_dir.join("candidate_shortlist.tsv")).expect("shortlist");
    write_text(
        &options.run_dir.join("known_database_matches.tsv"),
        &format!(
            "{header}{}\n",
            rows.into_iter().rev().collect::<Vec<_>>().join("\n")
        ),
    )
    .expect("reordered known matches");
    assert_eq!(write_candidate_shortlist(&options).expect("rerank"), 3);
    let second = read_text(&options.run_dir.join("candidate_shortlist.tsv")).expect("shortlist");
    let shortlist = read_simple_tsv_rows(&options.run_dir.join("candidate_shortlist.tsv"))
        .expect("shortlist rows");

    assert_eq!(first, second);
    assert_eq!(shortlist[0]["input_id"], "input_1");
    assert_eq!(shortlist[0]["target_id"], "T1");
    assert_eq!(shortlist[0]["shortlist_rank"], "1");
    assert_eq!(shortlist[2]["input_id"], "input_2");
    assert_eq!(shortlist[2]["shortlist_rank"], "1");
    assert!(shortlist.iter().all(|row| {
        row["shortlist_reason"].contains("known_database=")
            && !row.contains_key("docking_score")
            && !row.contains_key("md_score")
            && row["input_kind"] == "small_molecule"
            && row["ranking_policy"] == "small_molecule_route_v1"
            && row["preliminary_metagenome_weight"] == "0.0000"
    }));
    fs::remove_dir_all(&root).expect("cleanup temp root");
}

#[test]
fn candidate_coarse_rank_uses_complex_structure_route_before_top_n_selection() {
    let root = temp_root("candidate_shortlist_complex_route");
    let options = indexed_fixture_options(&root);
    write_input_kind_manifest(&options, &[("input_1", "record_1", "complex")]).expect("input kind");
    let header = "input_id\tsource\ttarget_id\ttarget_name\torganism\ttarget_class\tmatch_score\tmatched_features\tevidence\tsource_database_id\tcurrent_run_query_id\trecord_id\n";
    write_text(
        &options.run_dir.join("known_database_matches.tsv"),
        &format!(
            "{header}input_1\tKnown\tK1\tKnown 1\tHomo sapiens\tenzyme\t1.0000\tfeature\tknown evidence\tKnown\tquery\trecord_1\ninput_1\tKnown\tK2\tKnown 2\tHomo sapiens\tenzyme\t0.9500\tfeature\tknown evidence\tKnown\tquery\trecord_1\ninput_1\tKnown\tK3\tKnown 3\tHomo sapiens\tenzyme\t0.9000\tfeature\tknown evidence\tKnown\tquery\trecord_1\n"
        ),
    )
    .expect("known matches");
    write_text(
        &options.run_dir.join("metagenome_potential_matches.tsv"),
        header,
    )
    .expect("metagenome matches");
    write_text(
        &options.run_dir.join("structure_interaction_matches.tsv"),
        &format!(
            "{header}input_1\tRCSB\tS1\tStructure 1\tHomo sapiens\tstructure\t0.9000\tfeature\tstructure evidence\tRCSB\tquery\trecord_1\n"
        ),
    )
    .expect("structure matches");

    assert_eq!(
        ScienceCommand::parse("candidate", "coarse-rank").expect("parse command"),
        ScienceCommand::CandidateCoarseRank
    );
    run_science_command(ScienceCommand::CandidateCoarseRank, &options).expect("coarse rank");
    let shortlist =
        read_simple_tsv_rows(&options.run_dir.join("candidate_shortlist.tsv")).expect("shortlist");

    assert_eq!(shortlist.len(), 4);
    assert_eq!(shortlist[0]["target_id"], "S1");
    assert_eq!(shortlist[0]["input_kind"], "complex");
    assert_eq!(shortlist[0]["ranking_policy"], "complex_route_v2");
    assert_eq!(shortlist[0]["preliminary_structure_weight"], "0.8000");
    assert_eq!(shortlist[0]["docking_weight"], "0.0000");
    assert_eq!(shortlist[0]["md_weight"], "0.0000");
    fs::remove_dir_all(&root).expect("cleanup temp root");
}

#[test]
fn candidate_shortlist_writes_header_when_retrieval_has_no_candidates() {
    let root = temp_root("candidate_shortlist_empty");
    let options = indexed_fixture_options(&root);
    assert_eq!(
        write_candidate_shortlist(&options).expect("empty shortlist"),
        0
    );
    let text = read_text(&options.run_dir.join("candidate_shortlist.tsv")).expect("shortlist");
    assert_eq!(text, format!("{CANDIDATE_SHORTLIST_HEADER}\n"));
    assert!(
        selected_candidate_rows(&options)
            .expect("selected")
            .is_empty()
    );
    fs::remove_dir_all(&root).expect("cleanup temp root");
}

#[test]
fn shortlist_coordinate_top_n_isolated_per_input_record() {
    let root = temp_root("candidate_shortlist_per_input");
    let options = indexed_fixture_options(&root);
    let receptor = "structure_candidates/shared.cif";
    write_text(
        &options.run_dir.join(receptor),
        "data_1ABC\nATOM 1 N N . GLY A 1 1 ? 1.0 1.0 1.0 1.00 20.00 ? 1 GLY A N 1\n",
    )
    .expect("receptor");
    let digest = sha256_file_hex(&options.run_dir.join(receptor)).expect("digest");
    let mut shortlist = format!("{CANDIDATE_SHORTLIST_HEADER}\n");
    for input_id in ["input_1", "input_2"] {
        for rank in 1..=4 {
            shortlist.push_str(&format!(
                "{rank}\t{input_id}_candidate_{rank}\tuniprot\t{input_id}\trecord_1\tT{rank}\tTarget {rank}\tHomo sapiens\tenzyme\tknown_human_protein\tKnown\t0.5000\t0.5000\t0.0000\t0.0000\tknown_database=0.5000\t1ABC\tready\t{rank}\t{receptor}\t{digest}\t1ABC\n"
            ));
        }
    }
    write_text(&options.run_dir.join("candidate_shortlist.tsv"), &shortlist).expect("shortlist");

    let selected = selected_candidate_rows(&options).expect("selected");
    assert_eq!(selected.len(), COMPLEX_CANDIDATE_TOP_N * 2);
    for input_id in ["input_1", "input_2"] {
        let ranks = selected
            .iter()
            .filter(|row| row["input_id"] == input_id)
            .map(candidate_selection_rank)
            .collect::<Vec<_>>();
        assert_eq!(ranks, [1, 2, 3]);
    }
    fs::remove_dir_all(&root).expect("cleanup temp root");
}

#[test]
fn docking_and_md_prefer_ready_shortlist_over_legacy_selection() {
    let root = temp_root("candidate_shortlist_downstream");
    install_not_applicable_worker_fixtures(&root).expect("worker fixtures");
    let options = indexed_fixture_options(&root);
    let rows = write_indexed_fixture_selection(
        &options,
        &[(
            "candidate_shortlisted",
            "input_1",
            "record_1",
            "small_molecule",
            "CCO",
        )],
    )
    .expect("legacy selection fixture");
    let row = &rows[0];
    write_text(
        &options.run_dir.join("candidate_shortlist.tsv"),
        &format!(
            "{CANDIDATE_SHORTLIST_HEADER}\n1\t{}\tuniprot\t{}\t{}\t{}\tTarget\tHomo sapiens\tenzyme\tknown_human_protein\tBindingDB\t0.9000\t0.9000\t0.0000\t0.0000\tknown_database=0.9000\t1ABC\tready\t1\t{}\t{}\t1ABC\n",
            row["candidate_id"],
            row["input_id"],
            row["record_id"],
            row["target_id"],
            row["receptor_artifact"],
            row["coordinate_sha256"],
        ),
    )
    .expect("shortlist");
    write_text(
        &options.run_dir.join("complex_candidate_selection.tsv"),
        "candidate_id\tinput_id\trecord_id\ttarget_id\ttarget_name\torganism\ttarget_class\tsource\tpdb_id\tevidence\tselection_rank\treceptor_artifact\tcoordinate_sha256\tidentity_namespace\ncandidate_legacy\tinput_1\trecord_1\tLEGACY\tLegacy\tunknown\tenzyme\tRCSB\t9ZZZ\tlegacy\t1\tmissing.cif\tinvalid\tuniprot\n",
    )
    .expect("conflicting legacy selection");

    run_docking_candidate_workers(&options).expect("docking jobs");
    run_md_candidate_workers(&options).expect("md jobs");
    for kind in ["docking", "md"] {
        let index = read_json_value_if_file(
            &options
                .run_dir
                .join(format!("tool_outputs/{kind}_manifest_index.json")),
        )
        .expect("manifest index");
        let entries = index["entries"].as_array().expect("entries");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["candidate_id"], "candidate_shortlisted");
    }
    fs::remove_dir_all(&root).expect("cleanup temp root");
}

#[test]
fn dynamics_all_unsupported_candidates_publish_empty_indexes_and_not_applicable_outcomes() {
    let root = temp_root("dynamics_all_not_applicable");
    let options = indexed_fixture_options(&root);
    write_indexed_fixture_selection(
        &options,
        &[
            (
                "candidate_protein",
                "input_1",
                "record_1",
                "protein_ligand",
                "protein A",
            ),
            (
                "candidate_complex",
                "input_2",
                "record_1",
                "complex",
                "protein A with ligand B",
            ),
        ],
    )
    .expect("selection");

    let result = run_science_command(ScienceCommand::DynamicsScreen, &options)
        .expect("not applicable dynamics");
    assert!(result.contains("dynamics_status=not_applicable"));
    assert!(result.contains("not_applicable_count=2"));
    for (kind, task) in [
        ("docking", &DOCKING_WORKER_TASK),
        ("md", &SCIENTIFIC_WORKER_TASKS[0]),
    ] {
        let index = read_json_value_if_file(
            &options
                .run_dir
                .join(format!("tool_outputs/{kind}_manifest_index.json")),
        )
        .expect("manifest index");
        assert!(index["entries"].as_array().expect("entries").is_empty());
        assert!(
            validated_candidate_manifest_index(&options, kind, task)
                .expect("valid empty index")
                .is_empty()
        );
        let outcomes = read_text(
            &options
                .run_dir
                .join(format!("tool_outputs/{kind}_outcomes.tsv")),
        )
        .expect("outcomes");
        assert_eq!(outcomes.lines().count(), 3);
        assert!(outcomes.contains("input_kind_not_supported_for_docking_md:protein_ligand"));
        assert!(outcomes.contains("input_kind_not_supported_for_docking_md:complex"));
    }
    fs::remove_dir_all(&root).expect("cleanup temp root");
}

#[test]
fn dynamics_mixed_routes_index_only_runnable_small_molecule() {
    let root = temp_root("dynamics_mixed_applicability");
    install_not_applicable_worker_fixtures(&root).expect("worker fixtures");
    let options = indexed_fixture_options(&root);
    write_indexed_fixture_selection(
        &options,
        &[
            (
                "candidate_small",
                "input_1",
                "record_1",
                "small_molecule",
                "CCO",
            ),
            (
                "candidate_complex",
                "input_2",
                "record_1",
                "complex",
                "protein A with ligand B",
            ),
        ],
    )
    .expect("selection");

    let result =
        run_science_command(ScienceCommand::DynamicsScreen, &options).expect("mixed dynamics");
    assert!(result.contains("dynamics_status=executed"));
    assert!(result.contains("not_applicable_count=1"));
    for (kind, task) in [
        ("docking", &DOCKING_WORKER_TASK),
        ("md", &SCIENTIFIC_WORKER_TASKS[0]),
    ] {
        let entries =
            validated_candidate_manifest_index(&options, kind, task).expect("valid runnable index");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].facts.candidate_id, "candidate_small");
        let outcomes = read_text(
            &options
                .run_dir
                .join(format!("tool_outputs/{kind}_outcomes.tsv")),
        )
        .expect("outcomes");
        assert!(outcomes.contains("candidate_complex"));
        assert!(outcomes.contains("input_kind_not_supported_for_docking_md:complex"));
    }
    fs::remove_dir_all(&root).expect("cleanup temp root");
}

#[test]
fn p1_integrity_coordinate_cache_requires_matching_mmcif_or_pdb_identity() {
    let root = temp_root("coordinate_cache_identity");
    let correct_mmcif = root.join("1abc.cif");
    let entry_only_mmcif = root.join("1abc.mmcif");
    let wrong_mmcif = root.join("wrong.cif");
    let missing_mmcif = root.join("missing.cif");
    let conflicting_mmcif = root.join("conflicting.cif");
    let correct_pdb = root.join("1abc.pdb");
    let correct_ent = root.join("pdb1abc.ent");
    let wrong_pdb = root.join("wrong.pdb");
    let missing_pdb = root.join("missing.pdb");
    write_text(&correct_mmcif, &test_rcsb_mmcif("1ABC")).expect("correct mmCIF");
    write_text(
        &entry_only_mmcif,
        "_entry.id\n'1ABC'\nATOM 1 N N 1.0 2.0 3.0\n",
    )
    .expect("entry-only mmCIF");
    write_text(&wrong_mmcif, &test_rcsb_mmcif("9ZZZ")).expect("wrong mmCIF");
    write_text(&missing_mmcif, "ATOM 1 N N 1.0 2.0 3.0\n").expect("missing mmCIF");
    write_text(
        &conflicting_mmcif,
        "data_1ABC\n_entry.id 9ZZZ\nATOM 1 N N 1.0 2.0 3.0\n",
    )
    .expect("conflicting mmCIF");
    write_text(&correct_pdb, &test_rcsb_pdb("1ABC")).expect("correct PDB");
    write_text(&correct_ent, &test_rcsb_pdb("1ABC")).expect("correct ENT");
    write_text(&wrong_pdb, &test_rcsb_pdb("9ZZZ")).expect("wrong PDB");
    write_text(&missing_pdb, minimal_pdb_block()).expect("missing PDB header");

    assert!(coordinate_file_matches_pdb(&correct_mmcif, "1ABC"));
    assert!(coordinate_file_matches_pdb(&entry_only_mmcif, "1ABC"));
    assert!(!coordinate_file_matches_pdb(&wrong_mmcif, "1ABC"));
    assert!(!coordinate_file_matches_pdb(&missing_mmcif, "1ABC"));
    assert!(!coordinate_file_matches_pdb(&conflicting_mmcif, "1ABC"));
    assert!(coordinate_file_matches_pdb(&correct_pdb, "1ABC"));
    assert!(coordinate_file_matches_pdb(&correct_ent, "1ABC"));
    assert!(!coordinate_file_matches_pdb(&wrong_pdb, "1ABC"));
    assert!(!coordinate_file_matches_pdb(&missing_pdb, "1ABC"));
    fs::remove_dir_all(&root).expect("cleanup temp root");
}

#[test]
fn p1_integrity_invalid_coordinate_cache_fetches_or_stays_out_of_downstream() {
    let root = temp_root("invalid_coordinate_cache_downstream_gate");
    let run = root.join("run");
    let store = root.join("science_store");
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: store.clone(),
        input_manifest: root.join("input.json"),
        run_dir: run.clone(),
    };
    write_test_rcsb_on_demand_policy(&store);
    write_text(
        &run.join("known_database_matches.tsv"),
        "input_id\tsource\ttarget_id\ttarget_name\torganism\ttarget_class\tmatch_score\tmatched_features\tevidence\tprovenance\tactivity\tstructure\tstructure_source\trecord_id\ninput_1\tBindingDB\tP00001\tTarget one\tHomo sapiens\tenzyme\t1.0\tligand_code=LIG;pdb_ids=1ABC\tbindingdb_onsite_activity_row;Ki (nM)=10;pdb_ids=1ABC\t\t\t\t\trecord_1\ninput_2\tBindingDB\tP00002\tTarget two\tHomo sapiens\tenzyme\t0.9\tligand_code=LIG;pdb_ids=2BBB\tbindingdb_onsite_activity_row;Ki (nM)=20;pdb_ids=2BBB\t\t\t\t\trecord_1\n",
    )
    .expect("known matches");
    write_gzip(
        &rcsb_coordinate_cache_path(&store, "1ABC"),
        &test_rcsb_mmcif("9ZZZ"),
    )
    .expect("wrong cached identity");
    write_gzip(
        &rcsb_coordinate_cache_path(&store, "2BBB"),
        "ATOM 1 N N 1.0 2.0 3.0\n",
    )
    .expect("missing cached identity");
    let server = TestHttpServer::start(BTreeMap::from([
        (
            "/1ABC.cif.gz".to_string(),
            TestHttpResponse {
                status: 200,
                content_type: "application/gzip",
                body: test_gzip_bytes(&test_rcsb_mmcif("1ABC")),
            },
        ),
        (
            "/2BBB.cif.gz".to_string(),
            TestHttpResponse {
                status: 404,
                content_type: "text/plain",
                body: b"missing".to_vec(),
            },
        ),
    ]));

    assert!(
        write_coordinate_backed_candidate_selection_with_base_url(&options, &[], &server.base_url)
            .expect("identity-gated selection")
    );

    let audit = read_text(&run.join(RCSB_ON_DEMAND_FETCH_MANIFEST)).expect("fetch audit");
    assert!(audit.contains("1ABC\tfetched\t"));
    assert!(audit.contains("2BBB\tnot_found\t"));
    let selection = read_text(&run.join("complex_candidate_selection.tsv")).expect("selection");
    assert!(selection.contains("\t1ABC\t"));
    assert!(!selection.contains("\t2BBB\t"));
    let viewer = read_text(&run.join("structure_viewer.cif")).expect("viewer");
    assert!(viewer.contains("data_1ABC"));
    assert!(!viewer.contains("data_9ZZZ"));
    fs::remove_dir_all(&root).expect("cleanup temp root");
}

#[test]
fn p1_integrity_structure_index_excludes_wrong_or_missing_cached_identity() {
    let root = temp_root("structure_index_identity_gate");
    let store = root.join("science_store");
    write_gzip(
        &store.join("downloads/rcsb/mmcif/cc/3ccc.cif.gz"),
        &test_rcsb_mmcif("3CCC"),
    )
    .expect("correct cached coordinate");
    write_gzip(
        &store.join("downloads/rcsb/mmcif/dd/4ddd.cif.gz"),
        &test_rcsb_mmcif("9ZZZ"),
    )
    .expect("wrong cached coordinate");
    write_gzip(
        &store.join("downloads/rcsb/mmcif/ee/5eee.cif.gz"),
        "ATOM 1 N N 1.0 2.0 3.0\n",
    )
    .expect("identity-free cached coordinate");
    let options = StoreOptions {
        workspace_root: root.clone(),
        science_store: store.clone(),
    };

    run_store_command(ScienceCommand::PrepareScienceStore, &options).expect("prepare store");
    let structure =
        read_text(&store.join("indexes/structure_targets.tsv")).expect("structure index");
    assert!(structure.contains("\t3CCC\t"));
    assert!(!structure.contains("\t4DDD\t"));
    assert!(!structure.contains("\t5EEE\t"));
    fs::remove_dir_all(&root).expect("cleanup temp root");
}

#[test]
fn rcsb_on_demand_selection_fetches_valid_coordinate_and_publishes_audit() {
    let root = temp_root("rcsb_on_demand_selection_success");
    let run = root.join("run");
    let store = root.join("science_store");
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: store.clone(),
        input_manifest: root.join("input.json"),
        run_dir: run.clone(),
    };
    write_test_rcsb_on_demand_policy(&store);
    write_text(
        &run.join("known_database_matches.tsv"),
        "input_id\tsource\ttarget_id\ttarget_name\torganism\ttarget_class\tmatch_score\tmatched_features\tevidence\tprovenance\tactivity\tstructure\tstructure_source\trecord_id\ninput_1\tBindingDB\tP00001\tTarget one\tHomo sapiens\tenzyme\t1.0\tligand_code=LIG;pdb_ids=1ABC\tbindingdb_onsite_activity_row;Ki (nM)=10;pdb_ids=1ABC\t\t\t\t\trecord_1\n",
    )
    .expect("known matches");
    let cache_dir = store.join("downloads/rcsb/mmcif/ab");
    write_text(&cache_dir.join(".1abc.old.cif.gz.part"), "stale").expect("stale part");
    write_text(&cache_dir.join(".1abc.old.cif.gz.part.identity.json"), "{}")
        .expect("stale identity");
    let server = TestHttpServer::start(BTreeMap::from([(
        "/1ABC.cif.gz".to_string(),
        TestHttpResponse {
            status: 200,
            content_type: "application/gzip",
            body: test_gzip_bytes(&test_rcsb_mmcif("1ABC")),
        },
    )]));

    assert!(
        write_coordinate_backed_candidate_selection_with_base_url(&options, &[], &server.base_url,)
            .expect("on-demand selection")
    );

    let coordinate = rcsb_coordinate_cache_path(&store, "1ABC");
    assert!(coordinate_file_matches_pdb(&coordinate, "1ABC"));
    assert_eq!(server.requests(), ["/1ABC.cif.gz"]);
    let audit = read_text(&run.join(RCSB_ON_DEMAND_FETCH_MANIFEST)).expect("fetch audit");
    assert!(audit.contains("1ABC\tfetched\t"));
    assert!(audit.contains("validated_and_atomically_published"));
    let selection = read_text(&run.join("complex_candidate_selection.tsv")).expect("selection");
    assert!(selection.contains("rcsb_fetch_manifest=rcsb_on_demand_fetch.tsv"));
    assert!(selection.contains("rcsb_fetch_status=fetched"));
    assert!(
        fs::read_dir(&cache_dir)
            .expect("cache directory")
            .filter_map(Result::ok)
            .all(|entry| {
                let name = entry.file_name().to_string_lossy().to_string();
                !name.contains(".part") && !name.contains("identity") && !name.ends_with(".lock")
            })
    );
    fs::remove_dir_all(&root).expect("cleanup temp root");
}

#[test]
fn rcsb_on_demand_prioritizes_missing_known_hint_over_lower_ranked_local_candidates() {
    let root = temp_root("rcsb_on_demand_global_candidate_priority");
    let run = root.join("run");
    let store = root.join("science_store");
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: store.clone(),
        input_manifest: root.join("input.json"),
        run_dir: run.clone(),
    };
    write_test_rcsb_on_demand_policy(&store);
    write_text(
        &run.join("known_database_matches.tsv"),
        "input_id\tsource\ttarget_id\ttarget_name\torganism\ttarget_class\tmatch_score\tmatched_features\tevidence\tprovenance\tactivity\tstructure\tstructure_source\trecord_id\ninput_1\tBindingDB\tP_HIGH\tHigh priority target\tHomo sapiens\tenzyme\t1.0\tligand_code=LIG;pdb_ids=7ZUB\tbindingdb_onsite_activity_row;Ki (nM)=1;pdb_ids=7ZUB\t\t\t\t\trecord_1\ninput_1\tBindingDB\tP_LOW_1\tLow priority target 1\tHomo sapiens\tenzyme\t0.8\tligand_code=LIG;pdb_ids=1AAA\tbindingdb_onsite_activity_row;Ki (nM)=500;pdb_ids=1AAA\t\t\t\t\trecord_1\ninput_1\tBindingDB\tP_LOW_2\tLow priority target 2\tHomo sapiens\tenzyme\t0.7\tligand_code=LIG;pdb_ids=2BBB\tbindingdb_onsite_activity_row;Ki (nM)=600;pdb_ids=2BBB\t\t\t\t\trecord_1\ninput_1\tBindingDB\tP_LOW_3\tLow priority target 3\tHomo sapiens\tenzyme\t0.6\tligand_code=LIG;pdb_ids=3CCC\tbindingdb_onsite_activity_row;Ki (nM)=700;pdb_ids=3CCC\t\t\t\t\trecord_1\n",
    )
    .expect("known matches");
    for pdb_id in ["1AAA", "2BBB", "3CCC"] {
        write_gzip(
            &rcsb_coordinate_cache_path(&store, pdb_id),
            &test_rcsb_mmcif(pdb_id),
        )
        .expect("local coordinate");
    }
    let server = TestHttpServer::start(BTreeMap::from([(
        "/7ZUB.cif.gz".to_string(),
        TestHttpResponse {
            status: 200,
            content_type: "application/gzip",
            body: test_gzip_bytes(&test_rcsb_mmcif("7ZUB")),
        },
    )]));

    assert!(
        write_coordinate_backed_candidate_selection_with_base_url(&options, &[], &server.base_url)
            .expect("globally prioritized selection")
    );

    assert_eq!(server.requests(), ["/7ZUB.cif.gz"]);
    let selection =
        read_simple_tsv_rows(&run.join("complex_candidate_selection.tsv")).expect("selection");
    assert_eq!(selection.len(), COMPLEX_CANDIDATE_TOP_N);
    assert!(
        selection
            .iter()
            .any(|row| row.get("pdb_id").map(String::as_str) == Some("7ZUB"))
    );
    assert!(
        !selection
            .iter()
            .any(|row| row.get("pdb_id").map(String::as_str) == Some("3CCC"))
    );
    let audit = read_text(&run.join(RCSB_ON_DEMAND_FETCH_MANIFEST)).expect("fetch audit");
    assert!(audit.contains("7ZUB\tfetched\t"));
    let viewer = read_text(&run.join("structure_viewer.cif")).expect("viewer");
    assert!(viewer.contains("data_7ZUB"));
    fs::remove_dir_all(&root).expect("cleanup temp root");
}

#[test]
fn rcsb_on_demand_selection_continues_after_404() {
    let root = temp_root("rcsb_on_demand_404_then_success");
    let run = root.join("run");
    let store = root.join("science_store");
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: store.clone(),
        input_manifest: root.join("input.json"),
        run_dir: run.clone(),
    };
    write_test_rcsb_on_demand_policy(&store);
    write_text(
        &run.join("known_database_matches.tsv"),
        "input_id\tsource\ttarget_id\ttarget_name\torganism\ttarget_class\tmatch_score\tmatched_features\tevidence\tprovenance\tactivity\tstructure\tstructure_source\trecord_id\ninput_1\tBindingDB\tP00002\tTarget two\tHomo sapiens\tenzyme\t1.0\tligand_code=LIG;pdb_ids=1AAA,2BBB\tbindingdb_onsite_activity_row;Ki (nM)=10;pdb_ids=1AAA,2BBB\t\t\t\t\trecord_1\n",
    )
    .expect("known matches");
    let server = TestHttpServer::start(BTreeMap::from([
        (
            "/1AAA.cif.gz".to_string(),
            TestHttpResponse {
                status: 404,
                content_type: "text/plain",
                body: b"missing".to_vec(),
            },
        ),
        (
            "/2BBB.cif.gz".to_string(),
            TestHttpResponse {
                status: 200,
                content_type: "application/gzip",
                body: test_gzip_bytes(&test_rcsb_mmcif("2BBB")),
            },
        ),
    ]));

    assert!(
        write_coordinate_backed_candidate_selection_with_base_url(&options, &[], &server.base_url,)
            .expect("404 fallback selection")
    );

    assert_eq!(server.requests(), ["/1AAA.cif.gz", "/2BBB.cif.gz"]);
    let audit = read_text(&run.join(RCSB_ON_DEMAND_FETCH_MANIFEST)).expect("fetch audit");
    assert!(audit.contains("1AAA\tnot_found\t"));
    assert!(audit.contains("2BBB\tfetched\t"));
    let selection = read_text(&run.join("complex_candidate_selection.tsv")).expect("selection");
    assert!(selection.contains("\t2BBB\t"));
    fs::remove_dir_all(&root).expect("cleanup temp root");
}

#[test]
fn rcsb_on_demand_rejects_bad_gzip_and_wrong_entry_without_replacing_cache() {
    let root = temp_root("rcsb_on_demand_invalid_payloads");
    let run = root.join("run");
    let store = root.join("science_store");
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: store.clone(),
        input_manifest: root.join("input.json"),
        run_dir: run.clone(),
    };
    write_test_rcsb_on_demand_policy(&store);
    write_text(
        &run.join("known_database_matches.tsv"),
        "input_id\tsource\ttarget_id\ttarget_name\torganism\ttarget_class\tmatch_score\tmatched_features\tevidence\tprovenance\tactivity\tstructure\tstructure_source\trecord_id\ninput_1\tBindingDB\tP00003\tTarget three\tHomo sapiens\tenzyme\t1.0\tligand_code=LIG;pdb_ids=1AAA,2BBB,3CCC\tbindingdb_onsite_activity_row;Ki (nM)=10;pdb_ids=1AAA,2BBB,3CCC\t\t\t\t\trecord_1\n",
    )
    .expect("known matches");
    let invalid_cached = rcsb_coordinate_cache_path(&store, "2BBB");
    write_text(&invalid_cached, "invalid cached sentinel").expect("invalid cached coordinate");
    let server = TestHttpServer::start(BTreeMap::from([
        (
            "/1AAA.cif.gz".to_string(),
            TestHttpResponse {
                status: 200,
                content_type: "application/gzip",
                body: b"<html>upstream error</html>".to_vec(),
            },
        ),
        (
            "/2BBB.cif.gz".to_string(),
            TestHttpResponse {
                status: 200,
                content_type: "application/gzip",
                body: test_gzip_bytes(&test_rcsb_mmcif("9ZZZ")),
            },
        ),
        (
            "/3CCC.cif.gz".to_string(),
            TestHttpResponse {
                status: 200,
                content_type: "application/gzip",
                body: test_gzip_bytes(&test_rcsb_mmcif("3CCC")),
            },
        ),
    ]));

    assert!(
        write_coordinate_backed_candidate_selection_with_base_url(&options, &[], &server.base_url,)
            .expect("invalid payload fallback selection")
    );

    assert_eq!(
        read_text(&invalid_cached).expect("unchanged invalid cache"),
        "invalid cached sentinel"
    );
    let audit = read_text(&run.join(RCSB_ON_DEMAND_FETCH_MANIFEST)).expect("fetch audit");
    assert!(audit.contains("1AAA\tfailed\t"));
    assert!(audit.contains("gzip_validation_failed"));
    assert!(audit.contains("2BBB\tfailed\t"));
    assert!(audit.contains("pdb_entry_id_mismatch"));
    assert!(audit.contains("3CCC\tfetched\t"));
    fs::remove_dir_all(&root).expect("cleanup temp root");
}

#[test]
fn rcsb_on_demand_reuses_cache_without_network_and_serializes_same_id_fetches() {
    let root = temp_root("rcsb_on_demand_reuse_and_concurrency");
    let run = root.join("run");
    let store = root.join("science_store");
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: store.clone(),
        input_manifest: root.join("input.json"),
        run_dir: run.clone(),
    };
    write_test_rcsb_on_demand_policy(&store);
    write_text(
        &run.join("known_database_matches.tsv"),
        "input_id\tsource\ttarget_id\ttarget_name\torganism\ttarget_class\tmatch_score\tmatched_features\tevidence\tprovenance\tactivity\tstructure\tstructure_source\trecord_id\ninput_1\tBindingDB\tP00004\tTarget four\tHomo sapiens\tenzyme\t1.0\tligand_code=LIG;pdb_ids=4DDD\tbindingdb_onsite_activity_row;Ki (nM)=10;pdb_ids=4DDD\t\t\t\t\trecord_1\n",
    )
    .expect("known matches");
    let cached = rcsb_coordinate_cache_path(&store, "4DDD");
    write_gzip(&cached, &test_rcsb_mmcif("4DDD")).expect("cached coordinate");
    let cache_root = store.join(RCSB_ON_DEMAND_CACHE_ROOT);
    for index in 0..RCSB_ON_DEMAND_CACHE_MAX_FILES {
        write_text(&cache_root.join(format!("overflow-{index:04}.cif.gz")), "x")
            .expect("overflow cache coordinate");
    }
    let mut entries = Vec::new();
    collect_rcsb_cache_entries(&cache_root, &mut entries).expect("cache entries before reuse");
    assert_eq!(entries.len(), RCSB_ON_DEMAND_CACHE_MAX_FILES + 1);
    let cached_server = TestHttpServer::start(BTreeMap::new());
    let python = vec!["python".to_string()];
    let reused =
        fetch_rcsb_coordinate_with_python(&store, "4DDD", &cached_server.base_url, Some(&python));
    assert_eq!(reused.status, "reused");

    for _ in 0..2 {
        assert!(
            write_coordinate_backed_candidate_selection_with_base_url(
                &options,
                &[],
                &cached_server.base_url,
            )
            .expect("cached selection")
        );
    }
    assert!(cached_server.requests().is_empty());
    entries.clear();
    collect_rcsb_cache_entries(&cache_root, &mut entries).expect("cache entries after reuse");
    assert_eq!(entries.len(), RCSB_ON_DEMAND_CACHE_MAX_FILES);
    assert!(cached.exists());
    assert!(
        read_text(&run.join(RCSB_ON_DEMAND_FETCH_MANIFEST))
            .expect("cached fetch audit")
            .contains("4DDD\treused\t")
    );

    let concurrent_store = root.join("concurrent_store");
    write_test_rcsb_on_demand_policy(&concurrent_store);
    let concurrent_server = TestHttpServer::start(BTreeMap::from([(
        "/5EEE.cif.gz".to_string(),
        TestHttpResponse {
            status: 200,
            content_type: "application/gzip",
            body: test_gzip_bytes(&test_rcsb_mmcif("5EEE")),
        },
    )]));
    let mut workers = Vec::new();
    for _ in 0..2 {
        let worker_store = concurrent_store.clone();
        let base_url = concurrent_server.base_url.clone();
        workers.push(thread::spawn(move || {
            let python = vec!["python".to_string()];
            fetch_rcsb_coordinate_with_python(&worker_store, "5EEE", &base_url, Some(&python))
        }));
    }
    let results = workers
        .into_iter()
        .map(|worker| worker.join().expect("join concurrent fetch"))
        .collect::<Vec<_>>();
    assert!(
        results
            .iter()
            .all(|result| matches!(result.status.as_str(), "fetched" | "reused"))
    );
    assert_eq!(concurrent_server.requests(), ["/5EEE.cif.gz"]);
    let concurrent_coordinate = rcsb_coordinate_cache_path(&concurrent_store, "5EEE");
    assert!(coordinate_file_matches_pdb(&concurrent_coordinate, "5EEE"));
    let repeated = fetch_rcsb_coordinate_with_python(
        &concurrent_store,
        "5EEE",
        &concurrent_server.base_url,
        Some(&python),
    );
    assert_eq!(repeated.status, "reused");
    assert_eq!(concurrent_server.requests(), ["/5EEE.cif.gz"]);
    fs::remove_dir_all(&root).expect("cleanup temp root");
}

#[test]
fn rcsb_on_demand_cache_evicts_oldest_coordinates_inside_cache() {
    let root = temp_root("rcsb_on_demand_cache_eviction");
    let store = root.join("science_store");
    let oldest = rcsb_coordinate_cache_path(&store, "1AAA");
    let newer = rcsb_coordinate_cache_path(&store, "2BBB");
    let protected = rcsb_coordinate_cache_path(&store, "3CCC");
    let partial = oldest.with_extension("gz.part");
    let outside = root.join("outside.cif.gz");
    for path in [&oldest, &newer, &protected] {
        write_text(path, "abc").expect("cached coordinate");
    }
    write_text(&partial, "partial").expect("partial download");
    write_text(&outside, "outside").expect("outside coordinate");

    let escaped = enforce_rcsb_on_demand_cache_limit(
        &store,
        &outside,
        RcsbOnDemandPolicy {
            max_cache_bytes: 6,
            max_cache_files: 2,
        },
    )
    .expect_err("protected path outside cache must fail");
    assert!(escaped.contains("protected_file_outside_cache"));
    assert!(oldest.exists());

    enforce_rcsb_on_demand_cache_limit(
        &store,
        &protected,
        RcsbOnDemandPolicy {
            max_cache_bytes: 6,
            max_cache_files: 2,
        },
    )
    .expect("enforce bounded cache");

    assert!(!oldest.exists());
    assert!(newer.exists());
    assert!(protected.exists());
    assert!(partial.exists());
    assert!(outside.exists());
    fs::remove_dir_all(&root).expect("cleanup temp root");
}

#[test]
fn rcsb_on_demand_policy_requires_declared_cache_limits() {
    let root = temp_root("rcsb_on_demand_cache_policy_limits");
    let store = root.join("science_store");
    write_test_rcsb_on_demand_policy(&store);
    let policy_path = store.join("downloads/rcsb/on_demand_policy.json");
    let mut policy = serde_json::from_str::<Value>(&read_text(&policy_path).expect("policy"))
        .expect("policy JSON");
    policy["max_cache_files"] = json!(RCSB_ON_DEMAND_CACHE_MAX_FILES - 1);
    write_text(&policy_path, &(policy.to_string() + "\n")).expect("tampered policy");

    let error = validate_rcsb_on_demand_policy(&store).expect_err("cache limit must be fixed");
    assert_eq!(error, "rcsb_on_demand_policy_invalid:max_cache_files");
    fs::remove_dir_all(&root).expect("cleanup temp root");
}

#[test]
fn rcsb_on_demand_stops_at_declared_request_limit() {
    let root = temp_root("rcsb_on_demand_request_limit");
    let run = root.join("run");
    let store = root.join("science_store");
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: store.clone(),
        input_manifest: root.join("input.json"),
        run_dir: run.clone(),
    };
    write_test_rcsb_on_demand_policy(&store);
    let mut known = String::from(
        "input_id\tsource\ttarget_id\ttarget_name\torganism\ttarget_class\tmatch_score\tmatched_features\tevidence\tprovenance\tactivity\tstructure\tstructure_source\trecord_id\n",
    );
    for index in 0..(RCSB_ON_DEMAND_FETCH_LIMIT + 1) {
        let pdb_id = format!("1A{index:02}");
        known.push_str(&format!(
            "input_1\tBindingDB\tP{index:05}\tTarget {index}\tHomo sapiens\tenzyme\t1.0\tligand_code=LIG;pdb_ids={pdb_id}\tbindingdb_onsite_activity_row;Ki (nM)={};pdb_ids={pdb_id}\t\t\t\t\trecord_1\n",
            index + 1,
        ));
    }
    write_text(&run.join("known_database_matches.tsv"), &known).expect("known matches");
    let server = TestHttpServer::start(BTreeMap::new());

    assert!(
        !write_coordinate_backed_candidate_selection_with_base_url(
            &options,
            &[],
            &server.base_url,
        )
        .expect("bounded on-demand selection")
    );

    assert_eq!(server.requests().len(), RCSB_ON_DEMAND_FETCH_LIMIT);
    let audit = read_text(&run.join(RCSB_ON_DEMAND_FETCH_MANIFEST)).expect("fetch audit");
    assert_eq!(
        audit
            .lines()
            .skip(1)
            .filter(|line| line.contains("\tnot_found\t"))
            .count(),
        RCSB_ON_DEMAND_FETCH_LIMIT
    );
    assert!(audit.contains("\tskipped\t"));
    assert!(audit.contains("max_fetches_per_run_reached"));
    fs::remove_dir_all(&root).expect("cleanup temp root");
}

#[test]
fn rcsb_on_demand_rejects_unofficial_policy_before_network() {
    let root = temp_root("rcsb_on_demand_policy_gate");
    let run = root.join("run");
    let store = root.join("science_store");
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: store.clone(),
        input_manifest: root.join("input.json"),
        run_dir: run.clone(),
    };
    write_test_rcsb_on_demand_policy(&store);
    let policy_path = store.join("downloads/rcsb/on_demand_policy.json");
    let policy = read_text(&policy_path)
        .expect("policy")
        .replace(RCSB_OFFICIAL_DOWNLOAD_BASE_URL, "https://example.invalid");
    write_text(&policy_path, &policy).expect("tampered policy");
    write_text(
        &run.join("known_database_matches.tsv"),
        "input_id\tsource\ttarget_id\ttarget_name\torganism\ttarget_class\tmatch_score\tmatched_features\tevidence\tprovenance\tactivity\tstructure\tstructure_source\trecord_id\ninput_1\tBindingDB\tP00006\tTarget six\tHomo sapiens\tenzyme\t1.0\tligand_code=LIG;pdb_ids=6FFF\tbindingdb_onsite_activity_row;Ki (nM)=10;pdb_ids=6FFF\t\t\t\t\trecord_1\n",
    )
    .expect("known matches");
    let server = TestHttpServer::start(BTreeMap::new());

    let error =
        write_coordinate_backed_candidate_selection_with_base_url(&options, &[], &server.base_url)
            .expect_err("unofficial policy must fail closed");

    assert!(error.contains("rcsb_on_demand_policy_invalid:official_base_url"));
    assert!(server.requests().is_empty());
    assert!(
        read_text(&run.join(RCSB_ON_DEMAND_FETCH_MANIFEST))
            .expect("policy failure audit")
            .contains("rcsb_on_demand_policy_invalid:official_base_url")
    );
    fs::remove_dir_all(&root).expect("cleanup temp root");
}

#[derive(Clone)]
struct IndexedArtifactFixture {
    options: ScienceOptions,
    facts: CurrentMdRunFacts,
    job_id: String,
    invocation_id: String,
    manifest: String,
    provenance: Option<String>,
    output_dir: PathBuf,
}

fn indexed_fixture_options(root: &Path) -> ScienceOptions {
    ScienceOptions {
        workspace_root: root.to_path_buf(),
        science_store: root.join("science_store"),
        input_manifest: root.join("input.json"),
        run_dir: root.join("run"),
    }
}

fn write_input_kind_manifest(
    options: &ScienceOptions,
    rows: &[(&str, &str, &str)],
) -> Result<(), String> {
    let mut text = format!("{INPUT_CONVERSION_HEADER}\n");
    for (input_id, record_id, input_kind) in rows {
        let canonical_value = format!("{input_id}:{record_id}:{input_kind}");
        let source_digest = format!("{:x}", Sha256::digest(canonical_value.as_bytes()));
        text.push_str(&format!(
            "{input_id}\t{record_id}\t{input_kind}\t{input_kind}\t{input_kind}\t{canonical_value}\tconverted\tidentity\tinline\tinline\t\t{source_digest}\t\t\t\t0\t0\t0\n"
        ));
    }
    write_text(
        &options.run_dir.join("input_conversion_manifest.tsv"),
        &text,
    )
}

fn write_indexed_fixture_selection(
    options: &ScienceOptions,
    candidates: &[(&str, &str, &str, &str, &str)],
) -> Result<Vec<BTreeMap<String, String>>, String> {
    let mut selection = String::from(
        "candidate_id\tinput_id\trecord_id\ttarget_id\ttarget_name\torganism\ttarget_class\tsource\tpdb_id\tevidence\tselection_rank\treceptor_artifact\tcoordinate_sha256\tidentity_namespace\n",
    );
    let mut conversion = format!("{INPUT_CONVERSION_HEADER}\n");
    for (rank, (candidate_id, input_id, record_id, kind, canonical_value)) in
        candidates.iter().enumerate()
    {
        let receptor = format!("tool_outputs/receptors/{candidate_id}.cif");
        write_text(
            &options.run_dir.join(&receptor),
            &format!(
                "_struct.title '{candidate_id}'\nATOM 1 N N . GLY A 1 1 ? 1.0 1.0 1.0 1.00 20.00 ? 1 GLY A N 1\n"
            ),
        )?;
        let receptor_digest = sha256_file_hex(&options.run_dir.join(&receptor))
            .ok_or_else(|| "indexed_fixture_receptor_digest_missing".to_string())?;
        selection.push_str(&format!(
            "{candidate_id}\t{input_id}\t{record_id}\tT{}\tTarget {}\tHomo sapiens\tenzyme\tRCSB\t{}ABC\tindexed fixture\t{}\t{receptor}\t{receptor_digest}\tuniprot\n",
            rank + 1,
            rank + 1,
            rank + 1,
            rank + 1,
        ));
        let source_digest = format!("{:x}", Sha256::digest(canonical_value.as_bytes()));
        conversion.push_str(&format!(
            "{input_id}\t{record_id}\t{kind}\t{kind}\t{kind}\t{}\tconverted\tidentity\tinline\tinline\t\t{source_digest}\t\t\t\t0\t0\t0\n",
            encode_tsv_cell(canonical_value),
        ));
    }
    write_text(
        &options.run_dir.join("complex_candidate_selection.tsv"),
        &selection,
    )?;
    write_text(
        &options.run_dir.join("input_conversion_manifest.tsv"),
        &conversion,
    )?;
    write_text(&options.input_manifest, "{\"inputs\":[],\"files\":[]}\n")?;
    selected_candidate_rows(options)
}

fn candidate_fixture_result(
    task: &WorkerTask<'_>,
    job: &Value,
    manifest: &str,
    provenance: Option<&str>,
) -> Value {
    let mut result = json!({
        "schema_version": WORKER_SCHEMA_VERSION,
        "job_id": task.job_id,
        "worker": task.worker,
        "invocation_id": job["invocation_id"],
        "status": "PASS",
        "engine_version": "indexed artifact fixture 1",
        "execution_status": "completed",
        "reason": "completed",
        "outcomes": [{
            "status": "completed",
            "reason": "completed",
            "required": true,
            "database_id": task.worker,
            "row_count": 1,
        }],
        "python_owns_final_score": false,
        "python_owns_report_semantics": false,
        "primitive_execution_only": true,
        "manifest": manifest,
    });
    for field in [
        "run_id",
        "candidate_id",
        "candidate_digest",
        "input_id",
        "record_id",
        "input_digest",
        "source_digest",
        "receptor_digest",
    ] {
        if let Some(value) = job.get(field) {
            result[field] = value.clone();
        }
    }
    if let Some(provenance) = provenance {
        result["artifact_provenance"] = json!(provenance);
    }
    result
}

fn artifact_identity(fixture: &IndexedArtifactFixture) -> Value {
    json!({
        "run_id": fixture.facts.run_id,
        "job_id": fixture.job_id,
        "invocation_id": fixture.invocation_id,
        "candidate_id": fixture.facts.candidate_id,
        "candidate_digest": fixture.facts.candidate_digest,
        "input_id": fixture.facts.input_id,
        "record_id": fixture.facts.record_id,
        "input_digest": fixture.facts.input_digest,
        "source_digest": fixture.facts.source_digest,
    })
}

fn write_indexed_docking_fixture(
    options: &ScienceOptions,
    candidate: &BTreeMap<String, String>,
) -> Result<IndexedArtifactFixture, String> {
    let candidate_id = candidate
        .get("candidate_id")
        .ok_or_else(|| "indexed_fixture_candidate_missing".to_string())?;
    let job_id = candidate_worker_job_id(DOCKING_WORKER_TASK.job_id, candidate_id);
    let task = WorkerTask {
        job_id: &job_id,
        worker: DOCKING_WORKER_TASK.worker,
        module: DOCKING_WORKER_TASK.module,
        stage: DOCKING_WORKER_TASK.stage,
    };
    let (job, facts) = candidate_worker_job_value(options, &task, candidate)?;
    let invocation_id = job["invocation_id"]
        .as_str()
        .ok_or_else(|| "indexed_fixture_invocation_missing".to_string())?
        .to_string();
    let output_dir = options
        .run_dir
        .join("tool_outputs/docking")
        .join(safe_id(candidate_id))
        .join(safe_id(&invocation_id));
    ensure_dir(&output_dir)?;
    let receptor_artifact = candidate
        .get("receptor_artifact")
        .ok_or_else(|| "indexed_fixture_receptor_missing".to_string())?
        .to_string();
    let receptor_digest = candidate
        .get("coordinate_sha256")
        .ok_or_else(|| "indexed_fixture_receptor_digest_missing".to_string())?
        .to_string();
    let internal_artifacts = [
        ("receptor_pdb", "docking_receptor.pdb"),
        ("receptor_pdbqt", "docking_receptor.pdbqt"),
        ("ligand_sdf", "docking_ligand.sdf"),
        ("ligand_pdbqt", "docking_ligand.pdbqt"),
        ("pose", "docking_pose.pdbqt"),
        ("pose_sdf", "docking_pose.sdf"),
        ("log", "docking_vina.log"),
    ];
    for (artifact_name, file_name) in internal_artifacts {
        write_text(
            &output_dir.join(file_name),
            &format!("indexed docking fixture {artifact_name}\n"),
        )?;
    }
    let manifest_path = output_dir.join("docking_manifest.json");
    let manifest_relative = manifest_path
        .strip_prefix(&options.run_dir)
        .map_err(|err| err.to_string())?
        .to_string_lossy()
        .replace('\\', "/");
    let mut fixture = IndexedArtifactFixture {
        options: options.clone(),
        facts,
        job_id: job_id.clone(),
        invocation_id,
        manifest: manifest_relative.clone(),
        provenance: None,
        output_dir: output_dir.clone(),
    };
    let mut artifact_rows = Vec::new();
    let mut receptor_row = artifact_identity(&fixture);
    receptor_row["artifact_name"] = json!("receptor_coordinate");
    receptor_row["artifact_path"] = json!(receptor_artifact.clone());
    receptor_row["artifact_sha256"] = json!(receptor_digest.clone());
    receptor_row["receptor_digest"] = json!(receptor_digest.clone());
    artifact_rows.push(receptor_row);
    let mut named_paths = BTreeMap::new();
    for (artifact_name, file_name) in internal_artifacts {
        let relative = output_dir
            .join(file_name)
            .strip_prefix(&options.run_dir)
            .map_err(|err| err.to_string())?
            .to_string_lossy()
            .replace('\\', "/");
        let mut row = artifact_identity(&fixture);
        row["artifact_name"] = json!(artifact_name);
        row["artifact_path"] = json!(relative);
        row["artifact_sha256"] = json!(
            sha256_file_hex(&options.run_dir.join(&relative)).ok_or_else(|| format!(
                "indexed_fixture_artifact_digest_missing:{artifact_name}"
            ))?
        );
        named_paths.insert(artifact_name, relative);
        artifact_rows.push(row);
    }
    let mut manifest = artifact_identity(&fixture);
    manifest["artifact_schema_version"] = json!("genoligand_docking_artifacts_v1");
    manifest["status"] = json!("completed");
    manifest["method"] = json!("autodock_vina");
    manifest["engine"] = json!("vina");
    manifest["source_content_sha256"] = json!(fixture.facts.source_digest.clone());
    manifest["ligand_record_id"] = json!(fixture.facts.record_id.clone());
    manifest["receptor_coordinate"] = json!(receptor_artifact);
    manifest["receptor_digest"] = json!(receptor_digest.clone());
    manifest["receptor_sha256"] = json!(receptor_digest);
    for (field, path) in &named_paths {
        manifest[*field] = json!(path);
    }
    manifest["artifacts"] = json!(artifact_rows);
    write_text(&manifest_path, &(manifest.to_string() + "\n"))?;
    write_text(
        &options
            .run_dir
            .join("worker_jobs")
            .join(format!("{job_id}.json")),
        &(job.to_string() + "\n"),
    )?;
    let result = candidate_fixture_result(&task, &job, &manifest_relative, None);
    write_text(
        &options
            .run_dir
            .join("worker_results")
            .join(format!("{job_id}.json")),
        &(result.to_string() + "\n"),
    )?;
    ensure_manifest_index_entry(options, "docking", &fixture.facts, &task, &result)?;

    let summary_path = options.run_dir.join("tool_outputs/docking_summary.tsv");
    let mut summary = if summary_path.is_file() {
        read_text(&summary_path)?
    } else {
        "candidate_id\tdocking_status\tdocking_score\tdocking_engine\tnotes\n".to_string()
    };
    summary.push_str(&format!(
        "{}\tcompleted\t-8.0\tvina\tpose={}\n",
        fixture.facts.candidate_id,
        manifest["pose_sdf"].as_str().unwrap_or_default(),
    ));
    write_text(&summary_path, &summary)?;
    fixture.output_dir = manifest_path
        .parent()
        .ok_or_else(|| "indexed_fixture_output_dir_missing".to_string())?
        .to_path_buf();
    Ok(fixture)
}

fn write_indexed_md_fixture(
    options: &ScienceOptions,
    candidate: &BTreeMap<String, String>,
    peptide: bool,
) -> Result<IndexedArtifactFixture, String> {
    let base = SCIENTIFIC_WORKER_TASKS[0];
    let candidate_id = candidate
        .get("candidate_id")
        .ok_or_else(|| "indexed_fixture_candidate_missing".to_string())?;
    let job_id = candidate_worker_job_id(base.job_id, candidate_id);
    let task = WorkerTask {
        job_id: &job_id,
        worker: base.worker,
        module: base.module,
        stage: base.stage,
    };
    let (job, facts) = candidate_worker_job_value(options, &task, candidate)?;
    let invocation_id = job["invocation_id"]
        .as_str()
        .ok_or_else(|| "indexed_fixture_invocation_missing".to_string())?
        .to_string();
    let output_dir = options
        .run_dir
        .join("tool_outputs/md")
        .join(safe_id(candidate_id))
        .join(safe_id(&invocation_id));
    ensure_dir(&output_dir)?;
    let manifest_path = output_dir.join("md_run_manifest.json");
    let manifest_relative = manifest_path
        .strip_prefix(&options.run_dir)
        .map_err(|err| err.to_string())?
        .to_string_lossy()
        .replace('\\', "/");
    let provenance_path = output_dir.join("md_artifact_provenance.json");
    let provenance_relative = provenance_path
        .strip_prefix(&options.run_dir)
        .map_err(|err| err.to_string())?
        .to_string_lossy()
        .replace('\\', "/");
    let fixture = IndexedArtifactFixture {
        options: options.clone(),
        facts,
        job_id: job_id.clone(),
        invocation_id,
        manifest: manifest_relative.clone(),
        provenance: Some(provenance_relative.clone()),
        output_dir,
    };
    let (manifest, artifact_paths) = if peptide {
        write_indexed_peptide_artifacts(&fixture)?
    } else {
        write_indexed_complex_md_artifacts(&fixture)?
    };
    write_text(&manifest_path, &(manifest.to_string() + "\n"))?;
    let parameters_digest = "4".repeat(64);
    let identity = artifact_identity(&fixture);
    let rows = artifact_paths
        .iter()
        .map(|relative| {
            let mut row = identity.clone();
            row["parameters_digest"] = json!(parameters_digest);
            row["tool_name"] = json!(if peptide { "RDKit" } else { "OpenMM" });
            row["tool_version"] = json!("fixture 1");
            row["artifact_path"] = json!(relative);
            row["artifact_sha256"] = json!(
                sha256_file_hex(&options.run_dir.join(relative))
                    .expect("indexed fixture artifact digest")
            );
            row
        })
        .collect::<Vec<_>>();
    let mut provenance = identity;
    provenance["schema_version"] = json!(MD_PROVENANCE_SCHEMA_VERSION);
    provenance["parameters_digest"] = json!(parameters_digest);
    provenance["tool_name"] = json!(if peptide { "RDKit" } else { "OpenMM" });
    provenance["tool_version"] = json!("fixture 1");
    provenance["artifacts"] = json!(rows);
    write_text(&provenance_path, &(provenance.to_string() + "\n"))?;
    write_text(
        &options
            .run_dir
            .join("worker_jobs")
            .join(format!("{job_id}.json")),
        &(job.to_string() + "\n"),
    )?;
    let result =
        candidate_fixture_result(&task, &job, &manifest_relative, Some(&provenance_relative));
    write_text(
        &options
            .run_dir
            .join("worker_results")
            .join(format!("{job_id}.json")),
        &(result.to_string() + "\n"),
    )?;
    ensure_manifest_index_entry(options, "md", &fixture.facts, &task, &result)?;
    Ok(fixture)
}

fn fixture_relative(fixture: &IndexedArtifactFixture, name: &str) -> String {
    fixture
        .output_dir
        .join(name)
        .strip_prefix(&fixture.options.run_dir)
        .expect("fixture relative path")
        .to_string_lossy()
        .replace('\\', "/")
}

fn write_indexed_complex_md_artifacts(
    fixture: &IndexedArtifactFixture,
) -> Result<(Value, Vec<String>), String> {
    let candidate = &fixture.facts.candidate_id;
    let input = &fixture.facts.input_id;
    let summary = format!(
        "candidate_id\trmsd\trmsf\tligand_drift\thydrogen_bond_occupancy\tcontact_occupancy\tinteraction_persistence\tmd_risk_flags\tmd_score_decomposition\tmd_stability_score\n{candidate}\t1.0\t0.5\t1.0\t0.6\t0.8\t0.7\tstable\tworker_primitives\t0.7167\n"
    );
    write_text(&fixture.output_dir.join("md_summary.tsv"), &summary)?;
    write_text(
        &fixture.options.run_dir.join("tool_outputs/md_summary.tsv"),
        &summary,
    )?;
    for (name, metric, value, unit) in [
        ("md_rmsd.tsv", "rmsd", "1.0", "angstrom"),
        ("md_rmsf.tsv", "rmsf", "0.5", "angstrom"),
        ("md_ligand_drift.tsv", "ligand_drift", "1.0", "angstrom"),
        ("md_contacts.tsv", "contact_occupancy", "0.8", "fraction"),
        (
            "md_hbonds.tsv",
            "hydrogen_bond_occupancy",
            "0.6",
            "fraction",
        ),
    ] {
        write_text(
            &fixture.output_dir.join(name),
            &format!(
                "candidate_id\tinput_id\tmetric\tvalue\tunit\tsource\n{candidate}\t{input}\t{metric}\t{value}\t{unit}\tfixture\n"
            ),
        )?;
    }
    write_text(
        &fixture.output_dir.join("md_protocol.tsv"),
        &format!(
            "candidate_id\tinput_id\tstage\tparameter\tvalue\tunit\tsource\n{candidate}\t{input}\tplatform\topenmm_platform\tCPU\t\tfixture\n{candidate}\t{input}\tintegrator\ttime_step\t0.002\tps\tfixture\n{candidate}\t{input}\tproduction\ttotal_steps\t2\tsteps\tfixture\n{candidate}\t{input}\toutput\tframe_count\t2\tframes\tfixture\n{candidate}\t{input}\toutput\tstate_rows\t2\trows\tfixture\n"
        ),
    )?;
    write_text(
        &fixture.output_dir.join("md_state.tsv"),
        &format!(
            "candidate_id\tinput_id\tstep\ttime_ps\tpotential_energy_kj_mol\tkinetic_energy_kj_mol\ttotal_energy_kj_mol\ttemperature_estimate_K\n{candidate}\t{input}\t1\t0.002\t-10\t2\t-8\t300\n{candidate}\t{input}\t2\t0.004\t-9\t2\t-7\t301\n"
        ),
    )?;
    write_text(
        &fixture.output_dir.join("md_trajectory.tsv"),
        &format!(
            "candidate_id\tframe\tatom_index\tx\ty\tz\telement\trole\n{candidate}\t0\t1\t0\t0\t0\tC\tligand\n{candidate}\t1\t1\t0.1\t0\t0\tC\tligand\n"
        ),
    )?;
    for (name, contents) in [
        ("ligand_placement.tsv", "atom_index\tx\ty\tz\n1\t0\t0\t0\n"),
        ("gpu_telemetry.tsv", "sample\tstatus\n1\tnot_observed\n"),
        (
            "gpu_telemetry_status.tsv",
            "gpu_observed\treason\nfalse\tcpu_fixture\n",
        ),
        ("complex_md_manifest.json", "{\"status\":\"completed\"}\n"),
    ] {
        write_text(&fixture.output_dir.join(name), contents)?;
    }
    let names = [
        "md_summary.tsv",
        "md_rmsd.tsv",
        "md_rmsf.tsv",
        "md_ligand_drift.tsv",
        "md_contacts.tsv",
        "md_hbonds.tsv",
        "ligand_placement.tsv",
        "md_trajectory.tsv",
        "md_state.tsv",
        "md_protocol.tsv",
        "gpu_telemetry.tsv",
        "gpu_telemetry_status.tsv",
        "complex_md_manifest.json",
        "md_run_manifest.json",
    ];
    let paths = names
        .iter()
        .map(|name| fixture_relative(fixture, name))
        .collect::<Vec<_>>();
    let mut manifest = artifact_identity(fixture);
    manifest["method"] = json!("openmm_receptor_ligand_complex_dynamics");
    manifest["platform"] = json!("CPU");
    manifest["gpu_observed"] = json!(false);
    manifest["frame_count"] = json!(2);
    manifest["state_row_count"] = json!(2);
    manifest["trajectory"] = json!(fixture_relative(fixture, "md_trajectory.tsv"));
    manifest["state_artifact"] = json!(fixture_relative(fixture, "md_state.tsv"));
    manifest["protocol_artifact"] = json!(fixture_relative(fixture, "md_protocol.tsv"));
    manifest["ligand_coordinate"] = json!(fixture_relative(fixture, "ligand_placement.tsv"));
    manifest["complex_manifest"] = json!(fixture_relative(fixture, "complex_md_manifest.json"));
    manifest["gpu_telemetry_artifact"] = json!(fixture_relative(fixture, "gpu_telemetry.tsv"));
    manifest["gpu_telemetry_status_artifact"] =
        json!(fixture_relative(fixture, "gpu_telemetry_status.tsv"));
    manifest["metric_artifacts"] = json!([
        fixture_relative(fixture, "md_summary.tsv"),
        fixture_relative(fixture, "md_rmsd.tsv"),
        fixture_relative(fixture, "md_rmsf.tsv"),
        fixture_relative(fixture, "md_ligand_drift.tsv"),
        fixture_relative(fixture, "md_contacts.tsv"),
        fixture_relative(fixture, "md_hbonds.tsv"),
    ]);
    manifest["artifact_provenance"] =
        json!(fixture.provenance.as_deref().expect("fixture provenance"));
    Ok((manifest, paths))
}

fn write_indexed_peptide_artifacts(
    fixture: &IndexedArtifactFixture,
) -> Result<(Value, Vec<String>), String> {
    let candidate = &fixture.facts.candidate_id;
    let input = &fixture.facts.input_id;
    write_text(
        &fixture.output_dir.join("peptide_conformer_summary.tsv"),
        &format!(
            "candidate_id\tinput_id\tmethod\tconformer_count\tmean_heavy_atom_rmsd_angstrom\tmean_heavy_atom_rmsf_angstrom\tmean_centroid_drift_angstrom\thas_receptor\tuses_openmm\tnotes\n{candidate}\t{input}\trdkit_peptide_conformer_ensemble\t2\t0.4\t0.2\t0.3\tfalse\tfalse\tconformer_fixture\n"
        ),
    )?;
    write_text(
        &fixture.output_dir.join("peptide_conformer_metrics.tsv"),
        &format!(
            "candidate_id\tinput_id\tmetric\tvalue\tunit\tsource\n{candidate}\t{input}\tmean_centroid_drift_angstrom\t0.3\tangstrom\tfixture\n{candidate}\t{input}\tmean_heavy_atom_rmsd_angstrom\t0.4\tangstrom\tfixture\n{candidate}\t{input}\tmean_heavy_atom_rmsf_angstrom\t0.2\tangstrom\tfixture\n"
        ),
    )?;
    write_text(
        &fixture.output_dir.join("peptide_conformer_coordinates.tsv"),
        "atom_index\tx\ty\tz\n1\t0\t0\t0\n",
    )?;
    write_text(
        &fixture.output_dir.join("peptide_conformer_ensemble.tsv"),
        &format!(
            "candidate_id\tframe\tatom_index\tx\ty\tz\n{candidate}\t0\t1\t0\t0\t0\n{candidate}\t1\t1\t0.1\t0\t0\n"
        ),
    )?;
    let names = [
        "peptide_conformer_summary.tsv",
        "peptide_conformer_metrics.tsv",
        "peptide_conformer_coordinates.tsv",
        "peptide_conformer_ensemble.tsv",
        "md_run_manifest.json",
    ];
    let paths = names
        .iter()
        .map(|name| fixture_relative(fixture, name))
        .collect::<Vec<_>>();
    let mut manifest = artifact_identity(fixture);
    manifest["artifact_schema_version"] = json!("genoligand_peptide_conformer_v1");
    manifest["method"] = json!("rdkit_peptide_conformer_ensemble");
    manifest["method_class"] = json!("conformer_ensemble");
    manifest["platform"] = json!("RDKit");
    manifest["has_receptor"] = json!(false);
    manifest["uses_openmm"] = json!(false);
    manifest["md_score"] = json!(0.0);
    manifest["conformer_count"] = json!(2);
    manifest["coordinate_artifact"] = json!(fixture_relative(
        fixture,
        "peptide_conformer_coordinates.tsv"
    ));
    manifest["ensemble_artifact"] =
        json!(fixture_relative(fixture, "peptide_conformer_ensemble.tsv"));
    manifest["metric_artifacts"] = json!([
        fixture_relative(fixture, "peptide_conformer_summary.tsv"),
        fixture_relative(fixture, "peptide_conformer_metrics.tsv"),
    ]);
    manifest["artifact_provenance"] =
        json!(fixture.provenance.as_deref().expect("fixture provenance"));
    Ok((manifest, paths))
}

fn mutate_indexed_json(path: &Path, mutate: impl FnOnce(&mut Value)) -> Result<(), String> {
    let mut value = read_json_value_if_file(path)
        .ok_or_else(|| format!("indexed_fixture_json_invalid:{}", path.display()))?;
    mutate(&mut value);
    write_text(path, &(value.to_string() + "\n"))
}

fn refresh_md_provenance_digest(
    fixture: &IndexedArtifactFixture,
    artifact: &str,
) -> Result<(), String> {
    let provenance = fixture
        .provenance
        .as_deref()
        .ok_or_else(|| "indexed_fixture_provenance_missing".to_string())?;
    mutate_indexed_json(&fixture.options.run_dir.join(provenance), |value| {
        let digest = sha256_file_hex(&fixture.options.run_dir.join(artifact))
            .expect("mutated fixture digest");
        let row = value["artifacts"]
            .as_array_mut()
            .expect("provenance artifacts")
            .iter_mut()
            .find(|row| row["artifact_path"] == artifact)
            .expect("provenance artifact row");
        row["artifact_sha256"] = json!(digest);
    })
}

#[test]
fn indexed_artifact_docking_accepts_windows_selection_separator() {
    let root = temp_root("indexed_docking_windows_separator");
    let options = indexed_fixture_options(&root);
    let mut rows = write_indexed_fixture_selection(
        &options,
        &[(
            "candidate_A",
            "input_1",
            "record_1",
            "small_molecule",
            "CCO",
        )],
    )
    .expect("indexed selection");
    let receptor = rows[0]
        .get("receptor_artifact")
        .expect("receptor artifact")
        .clone();
    let windows_receptor = receptor.replace('/', "\\");
    rows[0].insert("receptor_artifact".to_string(), windows_receptor.clone());
    let selection_path = options.run_dir.join("complex_candidate_selection.tsv");
    let selection = read_text(&selection_path)
        .expect("selection")
        .replace(&receptor, &windows_receptor);
    write_text(&selection_path, &selection).expect("windows selection path");
    write_indexed_docking_fixture(&options, &rows[0]).expect("docking fixture");

    let scores = read_docking_scores(
        &options,
        &options.run_dir.join("tool_outputs/docking_summary.tsv"),
    )
    .expect("portable receptor path");
    assert!(scores.contains_key("candidate_A"));
    fs::remove_dir_all(&root).expect("cleanup fixture");
}

#[test]
fn indexed_artifact_docking_rejects_external_and_tampered_poses() {
    for (label, external) in [("external_pose", true), ("tampered_pose", false)] {
        let root = temp_root(&format!("indexed_docking_{label}"));
        let options = indexed_fixture_options(&root);
        let rows = write_indexed_fixture_selection(
            &options,
            &[(
                "candidate_A",
                "input_1",
                "record_1",
                "small_molecule",
                "CCO",
            )],
        )
        .expect("indexed selection");
        let fixture = write_indexed_docking_fixture(&options, &rows[0]).expect("docking fixture");
        if external {
            let outside = root.join("outside_pose.sdf");
            write_text(&outside, "external pose\n").expect("outside pose");
            write_text(
                &options.run_dir.join("tool_outputs/docking_summary.tsv"),
                &format!(
                    "candidate_id\tdocking_status\tdocking_score\tdocking_engine\tnotes\n{}\tcompleted\t-8.0\tvina\tpose={}\n",
                    fixture.facts.candidate_id,
                    outside.display()
                ),
            )
            .expect("external summary");
        } else {
            write_text(
                &fixture.output_dir.join("docking_pose.sdf"),
                "tampered pose\n",
            )
            .expect("tamper pose");
        }
        let error = read_docking_scores(
            &options,
            &options.run_dir.join("tool_outputs/docking_summary.tsv"),
        )
        .expect_err(label);
        let expected = if external {
            "docking_pose_outside_current_run"
        } else {
            "docking_artifact_binding_invalid:candidate_A:pose_sdf"
        };
        assert!(error.contains(expected), "{label}: {error}");
        fs::remove_dir_all(&root).expect("cleanup fixture");
    }
}

#[test]
fn indexed_artifact_docking_rejects_cross_candidate_manifest_and_pose() {
    let root = temp_root("indexed_docking_cross_candidate");
    let options = indexed_fixture_options(&root);
    let rows = write_indexed_fixture_selection(
        &options,
        &[
            (
                "candidate_A",
                "input_1",
                "record_1",
                "small_molecule",
                "CCO",
            ),
            (
                "candidate_B",
                "input_2",
                "record_2",
                "small_molecule",
                "CCN",
            ),
        ],
    )
    .expect("indexed selection");
    let candidate_a = write_indexed_docking_fixture(&options, &rows[0]).expect("candidate A");
    let candidate_b = write_indexed_docking_fixture(&options, &rows[1]).expect("candidate B");
    let index_path = options
        .run_dir
        .join("tool_outputs/docking_manifest_index.json");
    mutate_indexed_json(&index_path, |value| {
        let entry = value["entries"]
            .as_array_mut()
            .expect("index entries")
            .iter_mut()
            .find(|entry| entry["candidate_id"] == candidate_b.facts.candidate_id)
            .expect("candidate B entry");
        entry["manifest"] = json!(candidate_a.manifest);
    })
    .expect("cross-bind index");
    mutate_indexed_json(
        &options
            .run_dir
            .join("worker_results")
            .join(format!("{}.json", candidate_b.job_id)),
        |value| value["manifest"] = json!(candidate_a.manifest),
    )
    .expect("cross-bind result");
    let summary = read_text(&options.run_dir.join("tool_outputs/docking_summary.tsv"))
        .expect("docking summary")
        .replace(
            &fixture_relative(&candidate_b, "docking_pose.sdf"),
            &fixture_relative(&candidate_a, "docking_pose.sdf"),
        );
    write_text(
        &options.run_dir.join("tool_outputs/docking_summary.tsv"),
        &summary,
    )
    .expect("cross-bind summary");

    let error = read_docking_scores(
        &options,
        &options.run_dir.join("tool_outputs/docking_summary.tsv"),
    )
    .expect_err("cross-candidate manifest must fail");
    assert!(
        error.contains("docking_manifest_identity_mismatch")
            || error.contains("docking_candidate_artifact_index_mismatch"),
        "unexpected cross-candidate error: {error}"
    );
    fs::remove_dir_all(&root).expect("cleanup fixture");
}

#[test]
fn indexed_artifact_docking_rejects_stale_job_and_result_invocation() {
    let root = temp_root("indexed_docking_stale_invocation");
    let options = indexed_fixture_options(&root);
    let rows = write_indexed_fixture_selection(
        &options,
        &[(
            "candidate_A",
            "input_1",
            "record_1",
            "small_molecule",
            "CCO",
        )],
    )
    .expect("indexed selection");
    let fixture = write_indexed_docking_fixture(&options, &rows[0]).expect("docking fixture");
    for path in [
        options
            .run_dir
            .join("worker_jobs")
            .join(format!("{}.json", fixture.job_id)),
        options
            .run_dir
            .join("worker_results")
            .join(format!("{}.json", fixture.job_id)),
    ] {
        mutate_indexed_json(&path, |value| {
            value["invocation_id"] = json!("invocation_from_previous_run")
        })
        .expect("stale invocation");
    }

    let error = read_docking_scores(
        &options,
        &options.run_dir.join("tool_outputs/docking_summary.tsv"),
    )
    .expect_err("stale job/result must fail");
    assert!(
        error.contains("docking_candidate_job_binding_invalid"),
        "unexpected stale invocation error: {error}"
    );
    fs::remove_dir_all(&root).expect("cleanup fixture");
}

#[test]
fn indexed_artifact_md_rejects_identity_metric_unit_and_candidate_forgery() {
    for scenario in [
        "index_identity",
        "missing_metric",
        "wrong_unit",
        "wrong_candidate",
    ] {
        let root = temp_root(&format!("indexed_md_{scenario}"));
        let options = indexed_fixture_options(&root);
        let rows = write_indexed_fixture_selection(
            &options,
            &[(
                "candidate_A",
                "input_1",
                "record_1",
                "small_molecule",
                "CCO",
            )],
        )
        .expect("indexed selection");
        let fixture = write_indexed_md_fixture(&options, &rows[0], false).expect("MD fixture");
        match scenario {
            "index_identity" => {
                mutate_indexed_json(
                    &options.run_dir.join("tool_outputs/md_manifest_index.json"),
                    |value| {
                        value["entries"][0]["invocation_id"] = json!("invocation_from_previous_run")
                    },
                )
                .expect("stale index identity");
            }
            "missing_metric" => {
                fs::remove_file(fixture.output_dir.join("md_hbonds.tsv"))
                    .expect("remove required metric");
            }
            "wrong_unit" => {
                let relative = fixture_relative(&fixture, "md_rmsd.tsv");
                write_text(
                    &options.run_dir.join(&relative),
                    "candidate_id\tinput_id\tmetric\tvalue\tunit\tsource\ncandidate_A\tinput_1\trmsd\t1.0\tnm\tfixture\n",
                )
                .expect("wrong unit");
                refresh_md_provenance_digest(&fixture, &relative).expect("refresh unit digest");
            }
            "wrong_candidate" => {
                let relative = fixture_relative(&fixture, "md_rmsd.tsv");
                write_text(
                    &options.run_dir.join(&relative),
                    "candidate_id\tinput_id\tmetric\tvalue\tunit\tsource\ncandidate_B\tinput_1\trmsd\t1.0\tangstrom\tfixture\n",
                )
                .expect("wrong candidate");
                refresh_md_provenance_digest(&fixture, &relative)
                    .expect("refresh candidate digest");
            }
            _ => unreachable!(),
        }
        let error = verified_indexed_md_computations(&options).expect_err(scenario);
        let expected = match scenario {
            "index_identity" => "md_candidate_job_binding_invalid",
            "missing_metric" => "md_provenance_artifact_invalid",
            "wrong_unit" | "wrong_candidate" => "md_metric_binding_invalid",
            _ => unreachable!(),
        };
        assert!(error.contains(expected), "{scenario}: {error}");
        fs::remove_dir_all(&root).expect("cleanup fixture");
    }
}

#[test]
fn indexed_artifact_md_recomputes_and_rejects_forged_stability_score() {
    let root = temp_root("indexed_md_forged_stability");
    let options = indexed_fixture_options(&root);
    let rows = write_indexed_fixture_selection(
        &options,
        &[(
            "candidate_A",
            "input_1",
            "record_1",
            "small_molecule",
            "CCO",
        )],
    )
    .expect("indexed selection");
    let fixture = write_indexed_md_fixture(&options, &rows[0], false).expect("MD fixture");
    let forged = "candidate_id\trmsd\trmsf\tligand_drift\thydrogen_bond_occupancy\tcontact_occupancy\tinteraction_persistence\tmd_risk_flags\tmd_score_decomposition\tmd_stability_score\ncandidate_A\t1.0\t0.5\t1.0\t0.6\t0.8\t0.7\tstable\tforged\t0.9900\n";
    let relative = fixture_relative(&fixture, "md_summary.tsv");
    write_text(&options.run_dir.join(&relative), forged).expect("forged candidate summary");
    write_text(&options.run_dir.join("tool_outputs/md_summary.tsv"), forged)
        .expect("forged consolidated summary");
    refresh_md_provenance_digest(&fixture, &relative).expect("refresh forged digest");

    let error = verified_indexed_md_computations(&options).expect_err("forged score must fail");
    assert!(
        error.contains("md_stability_score_mismatch"),
        "unexpected forged-score error: {error}"
    );
    fs::remove_dir_all(&root).expect("cleanup fixture");
}

#[test]
fn indexed_artifact_peptide_conformer_never_becomes_md_score_or_openmm_claim() {
    let root = temp_root("indexed_peptide_no_md_claim");
    let options = indexed_fixture_options(&root);
    let rows = write_indexed_fixture_selection(
        &options,
        &[(
            "candidate_peptide",
            "input_1",
            "record_1",
            "peptide",
            "IKVAV",
        )],
    )
    .expect("indexed selection");
    write_indexed_md_fixture(&options, &rows[0], true).expect("peptide fixture");

    let error = verified_indexed_md_computations(&options)
        .expect_err("peptide conformer must stay outside the docking/MD route");
    assert!(
        error.contains("md_manifest_index_candidate_mismatch:candidate_peptide"),
        "unexpected peptide route error: {error}"
    );
    reset_candidate_manifest_index(&options, "md").expect("discard unsupported MD index entry");
    let verified = verified_indexed_md_computations(&options).expect("verified peptide fixture");
    assert!(verified.complex.is_empty());
    assert!(verified.peptide.is_empty());
    let summary = materialize_current_md_summary(&options).expect("materialize peptide summary");
    assert!(!summary.ready);
    assert_eq!(
        rows_in_tsv(&options.run_dir.join("md_summary.tsv")).expect("MD summary rows"),
        0
    );
    assert!(
        read_md_scores(&options.run_dir.join("md_summary.tsv"))
            .expect("MD scores")
            .is_empty()
    );
    let report = report_md_score_summary(&options, &[]).expect("peptide report summary");
    assert!(report.contains("md_score \u{4fdd}\u{6301} 0.0000"));
    assert!(!report.contains("OpenMM"));
    fs::remove_dir_all(&root).expect("cleanup fixture");
}

fn bind_current_md_artifacts(
    options: &ScienceOptions,
    artifact_names: &[&str],
) -> Result<(), String> {
    let invocation_id = start_worker_invocation(options)?;
    let task = &SCIENTIFIC_WORKER_TASKS[0];
    let job = worker_job_value(options, task)?;
    let facts = current_md_run_facts(options)?;
    let job_path = options
        .run_dir
        .join("worker_jobs")
        .join(format!("{}.json", task.job_id));
    let result_path = options
        .run_dir
        .join("worker_results")
        .join(format!("{}.json", task.job_id));
    write_text(&job_path, &(job.to_string() + "\n"))?;

    let identity = json!({
        "run_id": facts.run_id,
        "job_id": task.job_id,
        "invocation_id": invocation_id,
        "candidate_id": facts.candidate_id,
        "candidate_digest": facts.candidate_digest,
        "input_id": facts.input_id,
        "record_id": facts.record_id,
        "input_digest": facts.input_digest,
        "source_digest": facts.source_digest,
    });
    let mut rows = Vec::new();
    for artifact_name in artifact_names {
        let relative = format!("tool_outputs/{artifact_name}");
        let digest = sha256_file_hex(&options.run_dir.join(&relative))
            .ok_or_else(|| format!("fixture_artifact_unavailable:{relative}"))?;
        let mut row = identity.clone();
        row["parameters_digest"] = json!("4".repeat(64));
        row["tool_name"] = json!("test MD metrics");
        row["tool_version"] = json!("1");
        row["artifact_path"] = json!(relative);
        row["artifact_sha256"] = json!(digest);
        rows.push(row);
    }
    let mut provenance = identity.clone();
    provenance["schema_version"] = json!(MD_PROVENANCE_SCHEMA_VERSION);
    provenance["parameters_digest"] = json!("4".repeat(64));
    provenance["tool_name"] = json!("test MD metrics");
    provenance["tool_version"] = json!("1");
    provenance["artifacts"] = json!(rows);
    write_text(
        &options
            .run_dir
            .join("tool_outputs/md_artifact_provenance.json"),
        &(provenance.to_string() + "\n"),
    )?;

    let mut result = identity;
    result["schema_version"] = json!(WORKER_SCHEMA_VERSION);
    result["worker"] = json!(task.worker);
    result["status"] = json!("PASS");
    result["engine_version"] = json!("test MD metrics 1");
    result["execution_status"] = json!("completed");
    result["reason"] = json!("completed");
    result["outcomes"] = json!([{
        "status": "completed",
        "reason": "completed",
        "required": true,
        "database_id": "md_summary",
        "row_count": 1,
    }]);
    result["python_owns_final_score"] = json!(false);
    result["python_owns_report_semantics"] = json!(false);
    result["primitive_execution_only"] = json!(true);
    result["artifact_path"] = json!(format!("worker_results/{}.json", task.job_id));
    result["artifact_provenance"] = json!("tool_outputs/md_artifact_provenance.json");
    write_text(&result_path, &(result.to_string() + "\n"))
}

fn install_query_structure_worker_fixture(root: &Path) -> Result<(), String> {
    install_not_applicable_worker_fixture_modules(root, &["query_structure_worker.py"])
}

fn install_not_applicable_worker_fixtures(root: &Path) -> Result<(), String> {
    install_not_applicable_worker_fixture_modules(
        root,
        &[
            "alignment_worker.py",
            "docking_worker.py",
            "md_summary_worker.py",
            "query_structure_worker.py",
        ],
    )
}

fn install_not_applicable_worker_fixture_modules(
    root: &Path,
    modules: &[&str],
) -> Result<(), String> {
    let package = root.join("python/genoligand_workers");
    write_text(&package.join("__init__.py"), "")?;
    let script = r#"import argparse
import csv
import json
import os
from pathlib import Path

parser = argparse.ArgumentParser()
parser.add_argument("--job", required=True)
parser.add_argument("--out", required=True)
args = parser.parse_args()
with open(args.job, "r", encoding="utf-8-sig") as handle:
    job = json.load(handle)
headers = {
    "mmseqs_alignment": "input_id\tsource\ttarget_id\ttarget_name\torganism\ttarget_class\tmatch_score\tmatched_features\tevidence\tsource_database_id\tsource_namespace\tsource_class\tsource_digest\tbuilder_version\tmmseqs_version\tcurrent_run_query_id\trecord_id\tquery_kind\talignment_length\tquery_coverage\ttarget_coverage\talignment_cigar\n",
    "foldseek_alignment": "input_id\tsource\ttarget_id\ttarget_name\torganism\ttarget_class\tmatch_score\tmatched_features\tevidence\trecord_id\tsource_database_id\tsource_digest\tbuilder_version\tfoldseek_version\tcurrent_run_query_id\tquery_digest\ttm_score\n",
}
if job["worker"] in headers:
    raw_run_dir = str(job["run_dir"])
    if os.name != "nt" and len(raw_run_dir) >= 3 and raw_run_dir[1] == ":":
        run_dir = Path("/mnt") / raw_run_dir[0].lower() / raw_run_dir[3:].replace("\\", "/")
    else:
        run_dir = Path(raw_run_dir)
    output_dir = run_dir / "tool_outputs"
    output_dir.mkdir(parents=True, exist_ok=True)
    name = "mmseqs_alignment_matches.tsv" if job["worker"] == "mmseqs_alignment" else "foldseek_structure_matches.tsv"
    (output_dir / name).write_text(headers[job["worker"]], encoding="utf-8")
query_manifest_ref = ""
if job["worker"] == "query_structure_prepare":
    raw_run_dir = str(job["run_dir"])
    if os.name != "nt" and len(raw_run_dir) >= 3 and raw_run_dir[1] == ":":
        run_dir = Path("/mnt") / raw_run_dir[0].lower() / raw_run_dir[3:].replace("\\", "/")
    else:
        run_dir = Path(raw_run_dir)
    entry_dir = run_dir / "tool_outputs" / "query_structure" / str(job["job_id"])
    entry_dir.mkdir(parents=True, exist_ok=True)
    source_digest = str(job["source_digest"])
    manifest = {
        "schema_version": "genoligand_query_structure_artifact_v1",
        "status": "not_applicable",
        "run_id": str(job["run_id"]),
        "job_id": str(job["job_id"]),
        "invocation_id": str(job["invocation_id"]),
        "input_id": str(job["input_id"]),
        "record_id": str(job["record_id"]),
        "input_kind": str(job["input_kind"]),
        "input_digest": str(job["input_digest"]),
        "source_content_sha256": source_digest,
        "source_artifact_digest": source_digest,
        "method_version": "test_fixture_v1",
        "reason": "test_fixture_not_applicable",
    }
    manifest_path = entry_dir / "manifest.json"
    manifest_path.write_text(json.dumps(manifest, sort_keys=True) + "\n", encoding="utf-8")
    query_manifest_ref = manifest_path.relative_to(run_dir).as_posix()

    index_path = run_dir / "query_structure_manifest.tsv"
    index_fields = [
        "input_id", "record_id", "input_kind", "input_digest", "method",
        "features_json", "coordinate_artifact", "feature_digest", "status",
    ]
    rows = {}
    if index_path.is_file():
        with index_path.open("r", encoding="utf-8-sig", newline="") as handle:
            for row in csv.DictReader(handle, delimiter="\t"):
                rows[(row["input_id"], row["record_id"])] = row
    key = (str(job["input_id"]), str(job["record_id"]))
    rows[key] = {
        "input_id": key[0],
        "record_id": key[1],
        "input_kind": str(job["input_kind"]),
        "input_digest": str(job["input_digest"]),
        "method": "test_fixture_v1",
        "features_json": "",
        "coordinate_artifact": "",
        "feature_digest": "",
        "status": "not_applicable",
    }
    with index_path.open("w", encoding="utf-8", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=index_fields, delimiter="\t", lineterminator="\n")
        writer.writeheader()
        for key in sorted(rows):
            writer.writerow(rows[key])
reason = "test_fixture_not_applicable"
result = {
    "schema_version": "genoligand_worker_current_v2",
    "job_id": job["job_id"],
    "worker": job["worker"],
    "invocation_id": job["invocation_id"],
    "status": "NOT_APPLICABLE",
    "engine_version": "test worker fixture",
    "execution_status": "not_applicable",
    "reason": reason,
    "outcomes": [{
        "status": "not_applicable",
        "reason": reason,
        "required": False,
        "database_id": job["worker"],
        "row_count": 0,
    }],
    "python_owns_final_score": False,
    "python_owns_report_semantics": False,
    "primitive_execution_only": True,
}
for field in [
    "run_id",
    "candidate_id",
    "candidate_digest",
    "input_id",
    "record_id",
    "input_digest",
    "source_digest",
    "receptor_digest",
]:
    if field in job:
        result[field] = job[field]
if query_manifest_ref:
    result["manifest"] = query_manifest_ref
with open(args.out, "w", encoding="utf-8") as handle:
    json.dump(result, handle)
"#;
    for module in modules {
        write_text(&package.join(module), script)?;
    }
    Ok(())
}

struct QueryContractFixtureEntry {
    job: PathBuf,
    result: PathBuf,
    manifest: PathBuf,
    features: PathBuf,
}

struct QueryContractFixture {
    ready: QueryContractFixtureEntry,
    #[allow(dead_code)]
    not_applicable: QueryContractFixtureEntry,
}

fn query_structure_small_molecule_fixture(donor_count: u64) -> Value {
    let mut packed = vec![0_u8; 256];
    packed[0] = 0b0011_1111;
    json!({
        "canonical_smiles": "CCO",
        "isomeric_smiles": "CCO",
        "physicochemical": {
            "molecular_weight": 46.069,
            "exact_molecular_weight": 46.041865,
            "logp": -0.0014,
            "topological_polar_surface_area": 20.23,
            "hydrogen_bond_donors": donor_count,
            "hydrogen_bond_acceptors": 1,
            "rotatable_bonds": 0,
            "ring_count": 0,
            "aromatic_ring_count": 0,
            "fraction_csp3": 1.0,
            "formal_charge": 0,
            "heavy_atom_count": 3,
            "qed": 0.406808,
        },
        "fingerprint": {
            "algorithm": "morgan_ecfp4",
            "radius": 2,
            "n_bits": 2048,
            "include_chirality": true,
            "encoding": "rdkit_binary_text_hex_v1",
            "packed_hex": packed.iter().map(|byte| format!("{byte:02x}")).collect::<String>(),
            "population_count": 6,
            "packed_sha256": sha256_bytes_hex(&packed),
        },
        "pharmacophore": {
            "counts": {"Acceptor": 2, "Donor": donor_count},
            "representative_points": [],
            "point_count": donor_count + 2,
            "points_truncated": false,
        },
        "conformers": {
            "method": "fixture",
            "retained_coordinate_count": 0,
            "entries": [],
        },
        "engine_versions": {"fixture": "1"},
    })
}

fn write_query_structure_contract_fixture(
    options: &ScienceOptions,
    donor_count: u64,
) -> Result<QueryContractFixture, String> {
    let inputs = read_normalized_inputs(&options.run_dir.join("input_normalized_manifest.tsv"))?;
    let provenance =
        read_conversion_provenance(&options.run_dir.join("input_normalized_manifest.tsv"))?;
    let invocation_id = start_worker_invocation(options)?;
    let run_id = infer_run_id(&options.run_dir);
    let conversion_digest = sha256_file_hex(&options.run_dir.join("input_conversion_manifest.tsv"))
        .ok_or_else(|| "fixture_conversion_digest_missing".to_string())?;
    ensure_dir(&options.run_dir.join("worker_jobs"))?;
    ensure_dir(&options.run_dir.join("worker_results"))?;
    let mut index = format!("{QUERY_STRUCTURE_MANIFEST_HEADER}\n");
    let mut ready = None;
    let mut not_applicable = None;
    for input in &inputs {
        let identity = (input.input_id.clone(), input.record_id.clone());
        let source = provenance
            .get(&identity)
            .ok_or_else(|| "fixture_provenance_missing".to_string())?;
        let input_digest = query_structure_input_digest(input, source);
        let job_id = query_structure_job_id(&input.input_id, &input.record_id, &invocation_id);
        let task = WorkerTask {
            job_id: &job_id,
            worker: "query_structure_prepare",
            module: "genoligand_workers.query_structure_worker",
            stage: "query_structure_prepare",
        };
        let mut job_value = worker_job_value(options, &task)?;
        job_value["input_id"] = json!(&input.input_id);
        job_value["record_id"] = json!(&input.record_id);
        job_value["input_kind"] = json!(input.kind.as_str());
        job_value["input_digest"] = json!(&input_digest);
        job_value["source_digest"] = json!(&source.content_sha256);
        job_value["content_sha256"] = json!(&source.content_sha256);
        let job_path = options
            .run_dir
            .join("worker_jobs")
            .join(format!("{job_id}.json"));
        write_text(&job_path, &(job_value.to_string() + "\n"))?;

        let output_dir = options
            .run_dir
            .join("tool_outputs/query_structure")
            .join(&job_id);
        ensure_dir(&output_dir)?;
        let manifest_path = output_dir.join("manifest.json");
        let manifest_relative = manifest_path
            .strip_prefix(&options.run_dir)
            .map_err(|_| "fixture_manifest_relative_failed".to_string())?
            .to_string_lossy()
            .replace('\\', "/");
        let method = "test_query_structure_contract_v1";
        let is_ready = input.kind == InputKind::SmallMolecule;
        let features_path = output_dir.join("features.json");
        let mut manifest = json!({
            "schema_version": QUERY_STRUCTURE_ARTIFACT_SCHEMA_VERSION,
            "status": if is_ready { "ready" } else { "not_applicable" },
            "run_id": run_id,
            "job_id": job_id,
            "invocation_id": invocation_id,
            "input_id": input.input_id,
            "record_id": input.record_id,
            "input_kind": input.kind.as_str(),
            "input_digest": input_digest,
            "source_content_sha256": source.content_sha256,
            "method_version": method,
        });
        let mut features_relative = String::new();
        let mut feature_digest = String::new();
        if is_ready {
            let mut features = query_structure_small_molecule_fixture(donor_count);
            for (key, value) in json!({
                "schema_version": QUERY_STRUCTURE_FEATURE_SCHEMA_VERSION,
                "status": "ready",
                "run_id": run_id,
                "job_id": job_id,
                "invocation_id": invocation_id,
                "input_id": input.input_id,
                "record_id": input.record_id,
                "input_kind": input.kind.as_str(),
                "input_digest": input_digest,
                "source_content_sha256": source.content_sha256,
                "conversion_manifest_sha256": conversion_digest,
                "method_version": method,
                "coordinate_artifact": {
                    "path": "",
                    "sha256": "",
                    "size_bytes": 0,
                    "retention": "no_coordinate_artifact",
                },
            })
            .as_object()
            .expect("fixture common fields")
            {
                features[key] = value.clone();
            }
            write_text(&features_path, &(features.to_string() + "\n"))?;
            features_relative = features_path
                .strip_prefix(&options.run_dir)
                .map_err(|_| "fixture_features_relative_failed".to_string())?
                .to_string_lossy()
                .replace('\\', "/");
            feature_digest = sha256_file_hex(&features_path)
                .ok_or_else(|| "fixture_features_digest_failed".to_string())?;
            manifest["conversion_manifest_sha256"] = json!(&conversion_digest);
            manifest["features_json"] = json!(&features_relative);
            manifest["feature_digest"] = json!(&feature_digest);
            manifest["coordinate_artifact"] = json!("");
            manifest["coordinate_sha256"] = json!("");
            manifest["coordinate_size_bytes"] = json!(0);
            manifest["retention"] = json!("no_coordinate_artifact");
        } else {
            manifest["reason"] = json!("fixture_not_applicable");
        }
        write_text(&manifest_path, &(manifest.to_string() + "\n"))?;

        let (result_status, execution_status, reason, required, row_count) = if is_ready {
            ("PASS", "completed", "query_structure_prepared", true, 1)
        } else {
            (
                "NOT_APPLICABLE",
                "not_applicable",
                "fixture_not_applicable",
                false,
                0,
            )
        };
        let mut result = json!({
            "schema_version": WORKER_SCHEMA_VERSION,
            "job_id": job_id,
            "worker": "query_structure_prepare",
            "run_id": run_id,
            "invocation_id": invocation_id,
            "input_id": input.input_id,
            "record_id": input.record_id,
            "input_digest": input_digest,
            "source_digest": source.content_sha256,
            "status": result_status,
            "engine_version": "test query structure fixture 1",
            "execution_status": execution_status,
            "reason": reason,
            "outcomes": [{
                "status": execution_status,
                "reason": reason,
                "required": required,
                "database_id": "query_structure_prepare",
                "row_count": row_count,
            }],
            "python_owns_final_score": false,
            "python_owns_report_semantics": false,
            "primitive_execution_only": true,
            "manifest": manifest_relative,
        });
        if is_ready {
            result["artifact_path"] = json!(&features_relative);
        }
        let result_path = options
            .run_dir
            .join("worker_results")
            .join(format!("{job_id}.json"));
        write_text(&result_path, &(result.to_string() + "\n"))?;
        index.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t\t{}\t{}\n",
            input.input_id,
            input.record_id,
            input.kind.as_str(),
            input_digest,
            method,
            features_relative,
            feature_digest,
            if is_ready { "ready" } else { "not_applicable" },
        ));
        let entry = QueryContractFixtureEntry {
            job: job_path,
            result: result_path,
            manifest: manifest_path,
            features: features_path,
        };
        if is_ready {
            ready = Some(entry);
        } else {
            not_applicable = Some(entry);
        }
    }
    write_text(
        &options.run_dir.join("query_structure_manifest.tsv"),
        &index,
    )?;
    Ok(QueryContractFixture {
        ready: ready.ok_or_else(|| "fixture_ready_input_missing".to_string())?,
        not_applicable: not_applicable
            .ok_or_else(|| "fixture_not_applicable_input_missing".to_string())?,
    })
}

fn validate_query_structure_contract_fixture(
    options: &ScienceOptions,
) -> Result<BTreeMap<(String, String), QueryStructureArtifact>, String> {
    let inputs = read_normalized_inputs(&options.run_dir.join("input_normalized_manifest.tsv"))?;
    let provenance =
        read_conversion_provenance(&options.run_dir.join("input_normalized_manifest.tsv"))?;
    let invocation_id = current_worker_invocation(options)?;
    load_validated_query_structure_artifacts(options, &inputs, &provenance, &invocation_id)
}
