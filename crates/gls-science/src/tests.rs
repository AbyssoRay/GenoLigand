use super::input_conversion::ConvertedInput;
use super::*;
use std::time::{SystemTime, UNIX_EPOCH};

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
    let python_root = PathBuf::from(r"D:\Project\rust\GenoLigand\python");
    let job = r"D:\user\codex\run\worker_job.json";

    let pythonpath =
        worker_pythonpath_env_from_paths(&candidate, vec![python_root]).expect("pythonpath");

    assert_eq!(
        pythonpath.to_string_lossy(),
        "/mnt/d/Project/rust/GenoLigand/python"
    );
    assert_eq!(
        python_script_arg(&candidate, job),
        "/mnt/d/user/codex/run/worker_job.json"
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
    let out = run_science_command(ScienceCommand::PipelineRun, &options).expect("blocked ok");
    assert!(out.contains("pipeline_blocked"));
    let ranking = read_text(&run.join("target_candidate_ranking.tsv")).expect("ranking");
    assert_eq!(ranking.lines().count(), 1);
    let context = read_text(&run.join("run_context.json")).expect("run context");
    let source_map = read_text(&run.join("artifact_source_map.tsv")).expect("source map");
    assert!(context.contains("\"state\":\"blocked\""));
    assert!(source_map.contains("target_candidate_ranking.tsv"));
    let _ = fs::remove_dir_all(root);
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
            "rank\tcandidate_id\ttarget_id\ttarget_name\tcandidate_scope\torganism\tfinal_score\tknown_database_score\tmetagenome_score\tstructure_score\tdocking_score\tmd_score\tevidence_summary\tstatus\n",
        )
        .expect("empty ranking");
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: store,
        input_manifest: input,
        run_dir: run.clone(),
    };

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
    assert!(evidence.contains("docking_summary.tsv\tGenoLigand\tnot_generated\tno docking rows"));
    assert!(evidence.contains("md_summary.tsv\tGenoLigand\tnot_generated\tno dynamics rows"));
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
fn scientific_worker_failures_are_recorded_without_smoke_artifacts() {
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

    let error = run_scientific_workers(&options).expect_err("missing worker runtime must fail");
    assert!(error.contains("scientific_workers_failed"));
    let summary = read_text(&run.join("scientific_runtime_summary.tsv")).expect("worker summary");
    let rows = summary.lines().skip(1).collect::<Vec<_>>();
    assert_eq!(rows.len(), SCIENTIFIC_WORKER_TASKS.len());
    for row in rows {
        assert!(
            row.contains("\ttrue\t"),
            "worker job/result schema was not recorded as valid: {row}"
        );
        assert!(
            row.ends_with("\ttrue\tfalse\tfalse"),
            "worker escaped primitive-only boundary: {row}"
        );
    }
    let job =
        read_text(&run.join("worker_jobs/md_summary_ligand_dynamics.json")).expect("worker job");
    let result = read_text(&run.join("worker_results/md_summary_ligand_dynamics.json"))
        .expect("worker result");

    assert!(job.contains("\"rust_owned_decision_logic\":true"));
    assert!(result.contains("\"status\":\"FAIL\""));
    assert!(result.contains("\"execution_status\":\"failed\""));
    assert!(result.contains("\"outcomes\":["));
    assert!(result.contains("\"python_owns_final_score\":false"));
    assert!(result.contains("\"python_owns_report_semantics\":false"));
    assert!(result.contains("\"primitive_execution_only\":true"));
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
    assert_eq!(file_row.record_id, "protein");

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
    assert_eq!(file_row.record_id, "ambiguous");
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
    assert_eq!(rows[0][1], "alpha");
    assert_eq!(rows[0][5], "MKTAYI");
    assert_eq!(rows[1][1], "beta");
    assert_eq!(rows[1][5], "ACDEFG");
    for row in rows {
        assert_eq!(row[9], "file");
        assert_eq!(row[10], fasta.display().to_string());
        assert_eq!(
            row[11],
            "ed9366cb433cae217f90fcee728054b3b3f5300c52cdd3f5b3349f8b19601366"
        );
    }
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn duplicate_fasta_record_ids_fail_normalization() {
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
    let error = run_science_command(
        ScienceCommand::InputNormalize,
        &science_options(&root, &input, &run),
    )
    .expect_err("duplicate FASTA record IDs must fail");
    fs::remove_dir_all(root).expect("cleanup");
    assert!(error.contains("duplicate_input_record_id"));
}

#[test]
fn feature_manifest_preserves_multi_record_identity() {
    let root = temp_root("feature_record_identity");
    let store = root.join("science_store");
    write_text(
        &store.join("indexes/known_targets.tsv"),
        "source\ttarget_id\ttarget_name\torganism\ttarget_class\tligand_tokens\tsequence_tokens\tstructure_tokens\tevidence\ntest\tT1\tRecord identity target\ttest\tprotein\t\tmkt acd\t\trecord_identity_fixture\n",
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
    assert!(features.lines().any(|line| line.ends_with("\talpha")));
    assert!(features.lines().any(|line| line.ends_with("\tbeta")));
    assert_eq!(computed.len(), 2);
    assert_eq!(
        computed
            .iter()
            .map(|input| input.record_id.as_str())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["alpha", "beta"])
    );
    assert!(matches.lines().any(|line| line.ends_with("\talpha")));
    assert!(matches.lines().any(|line| line.ends_with("\tbeta")));
}

#[test]
fn sequence_file_formats_are_resolved_from_content() {
    let root = temp_root("sequence_file_formats");
    let cases = [
        (
            "fa",
            "protein_ligand",
            ">alpha\nMKTAYI\n",
            "alpha",
            "MKTAYI",
        ),
        (
            "faa",
            "protein_ligand",
            ">alpha\nMKTAYI\n",
            "alpha",
            "MKTAYI",
        ),
        (
            "fna",
            "genome_or_metagenome",
            ">contig\nATGCGTACGTAA\n",
            "contig",
            "ATGCGTACGTAA",
        ),
        (
            "fastq",
            "genome_or_metagenome",
            "@read1\nATGCGTAA\n+\nFFFFFFFF\n",
            "read1",
            "ATGCGTAA",
        ),
        (
            "fq",
            "genome_or_metagenome",
            "@read1\nATGCGTAA\n+\nFFFFFFFF\n",
            "read1",
            "ATGCGTAA",
        ),
        (
            "gb",
            "genome_or_metagenome",
            "LOCUS       TEST\nORIGIN\n        1 atgcgtaa\n//\n",
            "record_1",
            "ATGCGTAA",
        ),
        (
            "gbk",
            "genome_or_metagenome",
            "LOCUS       TEST\nORIGIN\n        1 atgcgtaa\n//\n",
            "record_1",
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
        assert_eq!(rows[0][5], *expected, "extension {extension}");
        assert_eq!(rows[0][8], *extension, "extension {extension}");
        assert_eq!(rows[0][9], "file", "extension {extension}");
        assert_eq!(rows[0][10], source.display().to_string());
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
    assert!(error.contains("invalid_fastq_record"));
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
    assert_eq!(rows[0][1], "persisted");
    assert_eq!(rows[0][5], "MKTAYI");
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
    .and_then(|_| read_text(&run.join("input_conversion_manifest.tsv")));

    let work_dir = fs::canonicalize(current_dir.join("work")).expect("work directory");
    let shadow_path = fs::canonicalize(&process_shadow).expect("shadow path");
    assert!(shadow_path.starts_with(&work_dir));
    fs::remove_file(&shadow_path).expect("cleanup process shadow");
    fs::remove_dir_all(&root).expect("cleanup fixture");

    let manifest = outcome.expect("normalize relative input");
    assert!(manifest.contains("manifest-dir"));
    assert!(manifest.contains("\tMKTAYI\tconverted\t"));
    assert!(!manifest.contains("process-cwd"));
    assert!(!manifest.contains("\tACDEFG\tconverted\t"));
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
    ensure_dir(&store.join("indexes/mmseqs")).expect("mmseqs dir");
    write_text(
        &store.join("indexes/mmseqs/mmseqs_target_databases.tsv"),
        "source_id\tsource_path\tdb_path\tstatus\tnotes\nmetagenome\tdownloads/metagenome/mgy.fa.gz\tindexes/mmseqs/metagenome/target\tready\talready_available\n",
    )
    .expect("mmseqs manifest");
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
    ensure_dir(&store.join("indexes/mmseqs")).expect("mmseqs dir");
    ensure_dir(&run.join("tool_outputs")).expect("tool outputs");
    write_text(
        &store.join("indexes/mmseqs/mmseqs_target_databases.tsv"),
        "source_id\tsource_path\tdb_path\tstatus\tnotes\nmetagenome_mgy\tdownloads/metagenome/mgy.fa.gz\tindexes/mmseqs/metagenome_mgy/target\tready\talready_available\n",
    )
    .expect("mmseqs manifest");
    write_text(
        &run.join("tool_outputs/mmseqs_alignment_matches.tsv"),
        "input_id\tsource\ttarget_id\ttarget_name\torganism\ttarget_class\tmatch_score\tmatched_features\tevidence\ninput_1\tMMseqs2\tMG_REAL\tMGnify hit\tunknown\tpotential_metagenome_sequence\t0.9100\tikvav\tmmseqs_alignment;source_db=metagenome_mgy;query_id=input_1\n",
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
    assert!(matches.contains("\tmetagenome_mgy\tinput_1"));
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn foldseek_tool_rows_preserve_source_database_and_query_id() {
    let root = temp_root("foldseek_identity");
    let store = root.join("science_store");
    let run = root.join("run");
    ensure_dir(&run.join("tool_outputs")).expect("tool outputs");
    write_text(
        &run.join("tool_outputs/foldseek_structure_matches.tsv"),
        "input_id\tsource\ttarget_id\ttarget_name\torganism\ttarget_class\tmatch_score\tmatched_features\tevidence\ninput_1\tFoldseek\t1ABC\tRCSB structure\tHomo sapiens\tknown_structure\t0.8700\tpocket\tfoldseek_alignment;source_db=rcsb_mmcif;query_id=input_1\n",
    )
    .expect("foldseek output");
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: store,
        input_manifest: root.join("input.json"),
        run_dir: run.clone(),
    };
    let mut out = "input_id\tsource\ttarget_id\ttarget_name\torganism\ttarget_class\tmatch_score\tmatched_features\tevidence\tsource_database_id\tcurrent_run_query_id\n".to_string();

    append_tool_match_rows(&options, "foldseek_structure_matches.tsv", &mut out)
        .expect("append foldseek");

    assert!(out.contains("1ABC"));
    assert!(out.contains("\trcsb_mmcif\tinput_1"));
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn known_matching_searches_bindingdb_zip_for_current_small_molecule() {
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
fn known_matching_searches_chembl_sqlite_archive_for_current_small_molecule() {
    let root = temp_root("chembl_onsite_search");
    let store = root.join("science_store");
    let run = root.join("run");
    let input = root.join("input.json");
    write_text(
        &store.join("indexes/known_targets.tsv"),
        "source\ttarget_id\ttarget_name\torganism\ttarget_class\tligand_tokens\tsequence_tokens\tstructure_tokens\tevidence\n",
    )
    .expect("empty known index");
    let source_sqlite = root.join("chembl_source.sqlite");
    write_chembl_sqlite(&source_sqlite).expect("chembl sqlite");
    write_tar_gzip_file(
        &store.join("downloads/chembl/chembl_sqlite.tar.gz"),
        "chembl_36/chembl_36_sqlite/chembl_36.db",
        &source_sqlite,
    )
    .expect("chembl archive");
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
    assert!(matches.contains("current_run_onsite_query"));
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
    let ids = bindingdb_complex_pdb_ids(
        "ligand_code=AZM;pdb_ids=3HS4,2H4N;target_chain_pdb_ids=1YM3,2A5V",
    );
    assert_eq!(ids, vec!["2H4N".to_string(), "3HS4".to_string()]);

    let chain_only = bindingdb_complex_pdb_ids("pdb_ids=;target_chain_pdb_ids=1YM3,2A5V");
    assert!(chain_only.is_empty());
}

#[test]
fn structure_matching_uses_ligand_identity_from_known_search() {
    let root = temp_root("structure_uses_known_ligand_identity");
    install_not_applicable_worker_fixtures(&root).expect("worker fixtures");
    let store = root.join("science_store");
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

    assert!(matches.contains("2H4N"));
    assert!(matches.contains("azm"));
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
fn structure_and_dynamics_use_only_tool_outputs_when_present() {
    let root = temp_root("tool_outputs");
    install_not_applicable_worker_fixtures(&root).expect("worker fixtures");
    let run = root.join("run");
    let store = root.join("science_store");
    let input = root.join("input.json");
    write_text(&input, "{\"inputs\":[],\"files\":[]}\n").expect("input");
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
    let structure =
        run_science_command(ScienceCommand::StructureEvaluate, &options).expect("structure");
    let dynamics = run_science_command(ScienceCommand::DynamicsScreen, &options).expect("dynamics");
    let structure_text =
        read_text(&run.join("structure_patch_summary.tsv")).expect("structure summary");
    let docking_text = read_text(&run.join("docking_summary.tsv")).expect("docking summary");
    let md_text = read_text(&run.join("md_summary.tsv")).expect("md summary");

    assert!(structure.contains("structure_artifact=ready"));
    assert!(structure.contains("docking_artifact=ready"));
    assert!(dynamics.contains("md_artifact=ready"));
    assert!(structure_text.contains("worker_structure"));
    assert!(docking_text.contains("worker_pose"));
    assert!(md_text.contains("worker_md"));
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
    write_text(
            &run.join("md_summary.tsv"),
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

    rank_targets(&options).expect("rank");
    build_evidence(&options).expect("evidence");
    render_report(&options).expect("report");

    let ranking = read_text(&run.join("target_candidate_ranking.tsv")).expect("ranking");
    let detail = read_text(&run.join("candidate_detail.json")).expect("detail");
    let evidence = read_text(&run.join("evidence_ledger.tsv")).expect("evidence");
    let score = read_text(&run.join("score_decomposition.tsv")).expect("score");
    let report = read_text(&run.join("research_report.md")).expect("report");
    let source_map = read_text(&run.join("artifact_source_map.tsv")).expect("source map");

    assert!(ranking.contains("0.5800\t1.0000\t0.0000\t0.0000\t0.0000\t0.8000"));
    assert!(ranking.contains("ChEMBL:chembl_activity_row;MD:md_summary_stability_score"));
    assert!(detail.contains("\"md\":0.8"));
    assert!(detail.contains("tool_outputs/md_run_manifest.json"));
    assert!(detail.contains("tool_outputs/complex_md_manifest.json"));
    assert!(detail.contains("tool_outputs/ligand_placement.tsv"));
    assert!(
        evidence.contains("md_stability_score feeds md_score and final_score with weight 0.10")
    );
    assert!(evidence.contains("1 rows include md_score from current run MD Summary"));
    assert!(score.contains("target_T1\tmd_score_contribution\t0.0800\tGenoLigand"));
    assert!(report.contains("md_stability_score"));
    assert!(report.contains("0.10"));
    assert!(source_map.contains("target_candidate_ranking.tsv\tknown_database_matches.tsv;metagenome_potential_matches.tsv;structure_interaction_matches.tsv;md_summary.tsv"));
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn md_manifest_accepts_wsl_receptor_path_for_current_run_scoring() {
    if !cfg!(windows) {
        return;
    }
    let root = temp_root("md_wsl_manifest_path");
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
        "candidate_id\trmsd\trmsf\tligand_drift\thydrogen_bond_occupancy\tcontact_occupancy\tinteraction_persistence\tmd_risk_flags\tmd_score_decomposition\tmd_stability_score\ntarget_T1\t1.2\t0.4\t0.6\t0.72\t0.81\t0.77\tstable\tworker_md\t0.8\n",
    )
    .expect("md summary");
    let receptor = run.join("structure_viewer.cif");
    write_text(&receptor, "data_target_T1\n").expect("receptor");
    let receptor_wsl =
        windows_path_to_wsl(&receptor.display().to_string()).expect("receptor wsl path");
    write_text(
        &run.join("tool_outputs/complex_md_manifest.json"),
        &format!(
            r#"{{"candidate_id":"target_T1","method":"openmm_receptor_ligand_complex_dynamics","platform":"CUDA","receptor_coordinate":"{}","ligand_coordinate":"tool_outputs/ligand_placement.tsv","trajectory":"tool_outputs/md_trajectory.tsv","protocol_artifact":"tool_outputs/md_protocol.tsv","state_artifact":"tool_outputs/md_state.tsv","gpu_telemetry_artifact":"tool_outputs/gpu_telemetry.tsv","gpu_telemetry_status_artifact":"tool_outputs/gpu_telemetry_status.tsv","gpu_observed":true,"metric_artifacts":["tool_outputs/md_summary.tsv"]}}"#,
            receptor_wsl
        ),
    )
    .expect("complex md manifest");
    write_text(
        &run.join("tool_outputs/md_run_manifest.json"),
        &format!(
            r#"{{"candidate_id":"target_T1","method":"openmm_receptor_ligand_complex_dynamics","platform":"CUDA","receptor_coordinate":"{}","ligand_coordinate":"tool_outputs/ligand_placement.tsv","complex_manifest":"tool_outputs/complex_md_manifest.json","protocol_artifact":"tool_outputs/md_protocol.tsv","state_artifact":"tool_outputs/md_state.tsv","gpu_telemetry_artifact":"tool_outputs/gpu_telemetry.tsv","gpu_telemetry_status_artifact":"tool_outputs/gpu_telemetry_status.tsv","gpu_observed":true,"trajectory":"tool_outputs/md_trajectory.tsv"}}"#,
            receptor_wsl
        ),
    )
    .expect("md run manifest");
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

    rank_targets(&options).expect("rank");

    let ranking = read_text(&run.join("target_candidate_ranking.tsv")).expect("ranking");
    assert!(ranking.contains("0.5800\t1.0000\t0.0000\t0.0000\t0.0000\t0.8000"));
    assert!(ranking.contains("MD:md_summary_stability_score"));
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
        "{\"inputs\":[{\"field\":\"small_molecule\",\"value\":\"CCO\"}],\"files\":[]}\n",
    )
    .expect("input");
    let mut ranking = String::from(
        "rank\tcandidate_id\ttarget_id\ttarget_name\tcandidate_scope\torganism\tfinal_score\tknown_database_score\tmetagenome_score\tstructure_score\tdocking_score\tmd_score\tevidence_summary\tstatus\n",
    );
    for index in 1..=60 {
        ranking.push_str(&format!(
            "{index}\ttarget_T{index}\tT{index}\tTarget {index}\tknown_human_protein\tHomo sapiens\t0.1000\t0.2000\t0.0000\t0.0000\t0.0000\t0.0000\tChEMBL:activity\tcomputed_prediction\n"
        ));
    }
    write_text(&run.join("target_candidate_ranking.tsv"), &ranking).expect("ranking");
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: store,
        input_manifest: input,
        run_dir: run.clone(),
    };

    render_report(&options).expect("report");

    let report = read_text(&run.join("research_report.md")).expect("report");
    assert!(report.contains("共生成 60 个候选靶点"));
    assert!(report.contains("Target 50"));
    assert!(!report.contains("Target 51"));
    assert!(!report.contains("target_candidate_ranking.tsv"));
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn cuda_complex_md_requires_gpu_observation_before_scoring() {
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

    rank_targets(&options).expect("rank");

    let ranking = read_text(&run.join("target_candidate_ranking.tsv")).expect("ranking");
    assert!(ranking.contains("\t0.0000\tChEMBL:chembl_activity_row\t"));
    assert!(!ranking.contains("MD:md_summary_stability_score"));
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
fn ranking_ignores_md_summary_without_complex_manifest() {
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

    rank_targets(&options).expect("rank");
    let ranking = read_text(&run.join("target_candidate_ranking.tsv")).expect("ranking");

    assert!(ranking.contains("0.5000\t1.0000\t0.0000\t0.0000\t0.0000\t0.0000"));
    assert!(!ranking.contains("MD:md_summary_stability_score"));
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn ranking_ignores_md_summary_without_state_artifact() {
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

    rank_targets(&options).expect("rank");
    let ranking = read_text(&run.join("target_candidate_ranking.tsv")).expect("ranking");

    assert!(ranking.contains("\t0.0000\tChEMBL:chembl_activity_row\t"));
    assert!(!ranking.contains("MD:md_summary_stability_score"));
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn ranking_uses_completed_vina_docking_score() {
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

    rank_targets(&options).expect("rank");
    let ranking = read_text(&run.join("target_candidate_ranking.tsv")).expect("ranking");
    let score = read_text(&run.join("score_decomposition.tsv")).unwrap_or_default();

    assert!(
        ranking.contains("\t0.6000\t0.0000\tChEMBL:chembl_activity_row;Docking:vina_completed")
    );
    assert!(!score.contains("docking_score_contribution"));
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

    rank_targets(&options).expect("rank");
    let ranking = read_text(&run.join("target_candidate_ranking.tsv")).expect("ranking");

    assert!(ranking.contains("\t0.5000\t1.0000\t0.0000\t0.0000\t0.0000\t0.0000"));
    assert!(!ranking.contains("Docking:vina_completed"));
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn ranking_blocks_final_artifacts_with_static_fallback_rows() {
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

    let err = rank_targets(&options).expect_err("authenticity gate should block");

    assert!(err.contains("authenticity_gate_failed"));
    let gate = read_text(&run.join("authenticity_gate.tsv")).expect("gate report");
    assert!(gate.contains("known_database_matches.tsv"));
    assert!(gate.contains("static_fallback_candidate"));
    assert!(!run.join("target_candidate_ranking.tsv").exists());
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn structure_evaluate_writes_viewer_from_real_coordinate_file() {
    let root = temp_root("structure_viewer_real");
    install_not_applicable_worker_fixtures(&root).expect("worker fixtures");
    let run = root.join("run");
    let store = root.join("science_store");
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
            "ATOM 1 N N . GLY A 1 1 ? 11.104 13.207 2.100 1.00 20.00 ? 1 GLY A N 1\nATOM 2 C CA . GLY A 1 1 ? 12.456 13.607 2.500 1.00 20.00 ? 1 GLY A CA 1\n",
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
    let out = run_science_command(ScienceCommand::StructureEvaluate, &options).expect("structure");
    let viewer = read_text(&run.join("structure_viewer.cif")).expect("viewer cif");
    let status = read_text(&run.join("structure_viewer_status.tsv")).expect("viewer status");

    assert!(out.contains("viewer_artifact=ready"));
    assert!(viewer.contains("ATOM 1"));
    assert!(status.contains("ready\t10GS\t10GS\tstructure_viewer.cif"));
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn structure_evaluate_prefers_bindingdb_pdb_candidate_over_token_noise() {
    let root = temp_root("structure_bindingdb_candidate");
    install_not_applicable_worker_fixtures(&root).expect("worker fixtures");
    let run = root.join("run");
    let store = root.join("science_store");
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
        "_struct.title 'UNRELATED MYOGLOBIN COORDINATE'\nATOM 1 N N . GLY A 1 1 ? 1.0 1.0 1.0 1.00 20.00 ? 1 GLY A N 1\n",
    )
    .expect("noise coordinate");
    write_text(
        &store.join("downloads/rcsb/mmcif/h4/2h4n.cif"),
        "_struct.title 'HUMAN CARBONIC ANHYDRASE II COMPLEXED WITH ACETAZOLAMIDE'\nATOM 1 N N . GLY A 1 1 ? 2.0 2.0 2.0 1.00 20.00 ? 1 GLY A N 1\n",
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
    let out = run_science_command(ScienceCommand::StructureEvaluate, &options).expect("structure");
    let status = read_text(&run.join("structure_viewer_status.tsv")).expect("viewer status");
    let selected =
        read_text(&run.join("complex_candidate_selection.tsv")).expect("candidate selection");
    let summary = read_text(&run.join("structure_patch_summary.tsv")).expect("summary");

    assert!(out.contains("viewer_artifact=ready"));
    assert!(status.contains("ready\tP00918\t2H4N\tstructure_viewer.cif"));
    assert!(selected.contains("target_P00918\tinput_1\tP00918\tCarbonic anhydrase 2"));
    assert!(summary.contains("target_P00918\t1.0000"));
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn structure_evaluate_reads_canonical_index_rows_beyond_source_sampling_limit() {
    let root = temp_root("structure_index_full_scan");
    install_not_applicable_worker_fixtures(&root).expect("worker fixtures");
    let run = root.join("run");
    let store = root.join("science_store");
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
        "ATOM 1 N N . GLY A 1 1 ? 11.104 13.207 2.100 1.00 20.00 ? 1 GLY A N 1\nATOM 2 C CA . GLY A 1 1 ? 12.456 13.607 2.500 1.00 20.00 ? 1 GLY A CA 1\n",
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
    let out = run_science_command(ScienceCommand::StructureEvaluate, &options).expect("structure");
    let matches = read_text(&run.join("structure_interaction_matches.tsv")).expect("matches");
    let summary = read_text(&run.join("structure_patch_summary.tsv")).expect("summary");
    let status = read_text(&run.join("structure_viewer_status.tsv")).expect("viewer status");

    assert!(out.contains("structure_artifact=ready"));
    assert!(out.contains("viewer_artifact=ready"));
    assert!(matches.contains("1IKV"));
    assert!(summary.contains("target_1IKV"));
    assert!(status.contains("ready\t1IKV\t1IKV\tstructure_viewer.cif"));
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn dynamics_screen_aggregates_real_metric_artifacts() {
    let root = temp_root("md_metric_artifacts");
    let run = root.join("run");
    let store = root.join("science_store");
    let input = root.join("input.json");
    ensure_dir(&run.join("tool_outputs")).expect("tool outputs");
    write_text(&input, "{\"inputs\":[],\"files\":[]}\n").expect("input");
    for (file, value) in [
        ("rmsd.tsv", "1.2"),
        ("rmsf.tsv", "0.4"),
        ("ligand_drift.tsv", "0.6"),
        ("hydrogen_bonds.tsv", "0.72"),
        ("contacts.tsv", "0.81"),
    ] {
        write_text(
            &run.join("tool_outputs").join(file),
            &format!("candidate_id\tvalue\nC1\t{value}\n"),
        )
        .expect("metric");
    }
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: store,
        input_manifest: input,
        run_dir: run.clone(),
    };

    let out = run_science_command(ScienceCommand::DynamicsScreen, &options).expect("dynamics");
    let md = read_text(&run.join("md_summary.tsv")).expect("md summary");
    let status = read_text(&run.join("md_summary_status.tsv")).expect("md status");

    assert!(out.contains("md_artifact=ready"));
    assert!(md.contains("C1\t1.2\t0.4\t0.6\t0.72\t0.81"));
    assert!(status.contains("ready"));
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
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

    let out = run_science_command(ScienceCommand::DynamicsScreen, &options).expect("dynamics");
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
    let out = run_science_command(ScienceCommand::PipelineRun, &options).expect("blocked ok");
    let failure = read_text(&run.join("failure_reason.txt")).expect("failure");

    assert!(out.contains("pipeline_blocked"));
    assert!(failure.contains("missing_install_status"));
    assert!(failure.contains("missing_source_requirements"));

    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn pipeline_blocks_when_alignment_database_manifest_is_deferred() {
    let root = temp_root("deferred_alignment_manifest");
    let store = root.join("science_store");
    ensure_dir(&store.join("indexes/mmseqs")).expect("mmseqs dir");
    ensure_dir(&store.join("indexes/foldseek")).expect("foldseek dir");
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
    for rel in [
        "indexes/known_targets.tsv",
        "indexes/metagenome_targets.tsv",
        "indexes/structure_targets.tsv",
    ] {
        write_text(
            &store.join(rel),
            "source\ttarget_id\ttarget_name\torganism\ttarget_class\tligand_tokens\tsequence_tokens\tstructure_tokens\tevidence\n",
        )
        .expect("index");
    }
    write_text(
        &store.join("install_status.json"),
        r#"{"status":"ready","missing_components":[]}"#,
    )
    .expect("install status");
    write_text(
        &store.join("source_requirements.tsv"),
        "source_id\tstatus\nchembl\tready\nmetagenome\tready\nstructure_interaction\tready\n",
    )
    .expect("sources");
    write_text(
        &store.join("indexes/mmseqs/mmseqs_target_databases.tsv"),
        "source_id\tsource_path\tdb_path\tstatus\tnotes\nmetagenome\tdownloads/metagenome/mgy.fa.gz\tindexes/mmseqs/metagenome/target\tdeferred_large_source\tsource_exceeds_sync_index_threshold\n",
    )
    .expect("mmseqs manifest");
    write_text(
        &store.join("indexes/foldseek/foldseek_target_databases.tsv"),
        "source_id\tsource_path\tdb_path\tstatus\tnotes\nrcsb_mmcif\tdownloads/rcsb/mmcif\tindexes/foldseek/rcsb_mmcif/target\tready\talready_available\n",
    )
    .expect("foldseek manifest");
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

    let out = run_science_command(ScienceCommand::PipelineRun, &options).expect("blocked ok");
    let failure = read_text(&run.join("failure_reason.txt")).expect("failure");

    assert!(out.contains("pipeline_blocked"));
    assert!(failure.contains("alignment_database_needs_repair"));
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
    write_text(
        &store.join("downloads/rcsb/mmcif/ab/1abc.cif.gz"),
        "structure file marker",
    )
    .expect("write rcsb structure marker");
    write_gzip(
        &store.join("downloads/biolip2/BioLiP.txt.gz"),
        "1ABC\tA\tA\t1\tATP\tadenosine triphosphate\n",
    )
    .expect("write biolip2 table");
    write_text(
        &store.join("downloads/plinder/index/annotation_table.parquet"),
        "plinder annotation marker",
    )
    .expect("write plinder annotation marker");
    write_text(
        &store.join("downloads/binding_moad/bindingmoad.csv"),
        "pdb,ligand,affinity\n2XYZ,ATP,10 nM\n",
    )
    .expect("write moad table");
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
    assert!(manifest.contains("rcsb_structure_file_rows"));
    assert!(manifest.contains("biolip2_interaction_rows"));
    assert!(manifest.contains("binding_moad_complex_rows"));
    assert!(manifest.contains("plinder_annotation_rows"));
    assert!(status.contains("\"structure\":"));
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
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
    writer
        .start_file(member_name, options)
        .map_err(|err| format!("zip_start_failed:{}:{err}", path.display()))?;
    writer
        .write_all(text.as_bytes())
        .map_err(|err| format!("zip_write_failed:{}:{err}", path.display()))?;
    writer
        .finish()
        .map_err(|err| format!("zip_finish_failed:{}:{err}", path.display()))?;
    Ok(())
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

fn temp_root(label: &str) -> PathBuf {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_millis();
    std::env::temp_dir().join(format!("genoligand_science_{label}_{millis}"))
}

fn install_not_applicable_worker_fixtures(root: &Path) -> Result<(), String> {
    let package = root.join("python/genoligand_workers");
    write_text(&package.join("__init__.py"), "")?;
    let script = r#"import argparse
import json

parser = argparse.ArgumentParser()
parser.add_argument("--job", required=True)
parser.add_argument("--out", required=True)
args = parser.parse_args()
with open(args.job, "r", encoding="utf-8-sig") as handle:
    job = json.load(handle)
reason = "test_fixture_not_applicable"
result = {
    "schema_version": "genoligand_worker_current_v2",
    "job_id": job["job_id"],
    "worker": job["worker"],
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
with open(args.out, "w", encoding="utf-8") as handle:
    json.dump(result, handle)
"#;
    for module in [
        "alignment_worker.py",
        "docking_worker.py",
        "md_summary_worker.py",
    ] {
        write_text(&package.join(module), script)?;
    }
    Ok(())
}
