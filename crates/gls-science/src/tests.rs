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
fn valid_worker_fail_result_counts_as_executed_worker() {
    let root = temp_root("worker_fail_result");
    let path = root.join("worker_result.json");
    let task = &WORKER_TASKS[0];
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
                "python_owns_final_score": false,
                "python_owns_report_semantics": false,
                "primitive_execution_only": true
            })
        ),
    )
    .expect("write result");

    assert!(worker_result_file_is_valid(&path, task));
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
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
    assert!(source_map.contains("worker_runtime_summary.tsv"));
    assert!(source_map.contains("Rust-generated worker job specs"));
    assert!(source_map.contains("docking_summary.tsv\t"));
    assert!(source_map.contains("md_summary.tsv\t"));
    assert!(source_map.contains("docking_summary.tsv\tschema-bound docking output when present;otherwise no generated rows\tevidence_ledger.tsv\tno_rows"));
    assert!(source_map.contains("md_summary.tsv\ttool_outputs/md_run_manifest.json;tool_outputs/md_summary.tsv;tool_outputs/md_rmsd.tsv;tool_outputs/md_rmsf.tsv;tool_outputs/md_ligand_drift.tsv;tool_outputs/md_contacts.tsv;tool_outputs/md_hbonds.tsv\ttarget_candidate_ranking.tsv;evidence_ledger.tsv\tno_rows"));
    assert!(source_map.contains("tool_outputs/md_summary.tsv\tschema-bound MD summary worker output\tmd_summary.tsv\tnot_yet_written"));
    assert!(evidence.contains("structure_interaction_matches"));
    assert!(evidence.contains("worker_runtime_summary"));
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
fn pipeline_records_schema_bound_workers_without_delegating_scores() {
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

    run_science_command(ScienceCommand::PipelineRun, &options).expect("pipeline");
    let summary = read_text(&run.join("worker_runtime_summary.tsv")).expect("worker summary");
    let rows = summary.lines().skip(1).collect::<Vec<_>>();
    assert_eq!(rows.len(), WORKER_TASKS.len());
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
    let job = read_text(&run.join("worker_jobs/openmm_smoke.json")).expect("worker job");
    let result = read_text(&run.join("worker_results/openmm_smoke.json")).expect("worker result");
    let score = read_text(&run.join("score_decomposition.tsv")).expect("score");
    let source_map = read_text(&run.join("artifact_source_map.tsv")).expect("source map");

    assert!(job.contains("\"rust_owned_decision_logic\":true"));
    assert!(result.contains("\"python_owns_final_score\":false"));
    assert!(result.contains("\"python_owns_report_semantics\":false"));
    assert!(result.contains("\"primitive_execution_only\":true"));
    assert!(source_map.contains("worker_runtime_summary.tsv"));
    assert!(score.contains("\tGenoLigand\n"));
    assert!(!score.contains("Python"));

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
fn input_conversion_prefers_content_over_selected_field() {
    let root = temp_root("free_text_conversion");
    let input = root.join("input.json");
    let run = root.join("run");
    write_text(
        &input,
        r#"{"inputs":[{"field":"small_molecule","value":"ikv AV"}],"files":[]}"#,
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

    assert!(normalized.contains("input_1\tpeptide\tIKVAV"));
    assert!(conversion.contains("input_1\tsmall_molecule\tpeptide\tpeptide\tIKVAV\tconverted\tstandard_amino_acid_sequence\tplain_text"));
    assert!(source_map.contains("input_conversion_manifest.tsv"));
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
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
    assert!(normalized.contains("input_2\tsmall_molecule\tCC(=O)OC1=CC=CC=C1C(=O)O"));
    assert!(normalized.contains("input_3\tprotein_ligand\tMKTAYIAKQRQISFVKSHFSRQDILDLWQMKTAYIAKQRQISFVKSHFSRQDILDLWQMKTAYIAKQRQISFVKSHFSRQDILDLWQ"));
    assert!(conversion.contains(
        "input_1\tpeptide\tpeptide\tpeptide\tIKVAV\tconverted\tstandard_amino_acid_sequence\tfasta"
    ));
    assert!(conversion.contains("input_2\tsmall_molecule\tsmall_molecule\tsmall_molecule\tCC(=O)OC1=CC=CC=C1C(=O)O\tconverted\trdkit_smiles\tplain_text"));
    assert!(conversion.contains("input_3\tprotein_ligand\tprotein_ligand\tprotein_ligand\tMKTAYIAKQRQISFVKSHFSRQDILDLWQMKTAYIAKQRQISFVKSHFSRQDILDLWQMKTAYIAKQRQISFVKSHFSRQDILDLWQ\tconverted\tstandard_amino_acid_sequence\tplain_text"));
    fs::remove_dir_all(&root).expect("cleanup temp root");
    assert!(!root.exists());
}

#[test]
fn input_conversion_recognizes_nucleotide_sequence_independent_of_field() {
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

    run_science_command(ScienceCommand::InputNormalize, &options).expect("normalize");
    let normalized =
        read_text(&run.join("input_normalized_manifest.tsv")).expect("normalized manifest");
    let conversion =
        read_text(&run.join("input_conversion_manifest.tsv")).expect("conversion manifest");

    assert!(normalized.contains("input_1\tgenome_or_metagenome\tATGCGTACGTAA"));
    assert!(conversion.contains("input_1\tsmall_molecule\tgenome_or_metagenome\tgenome_or_metagenome\tATGCGTACGTAA\tconverted\tnucleotide_sequence\tplain_text"));
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
            "input_1\tsmall_molecule\tsmall_molecule\tsmall_molecule\tCCN\tconverted\trdkit_smiles\tplain_text"
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
            "input_id\tinput_kind\tfeature_family\tfeature_name\tfeature_value\tfeature_source\tfeature_token\ninput_1\tsmall_molecule\tinput\tinput_length\t3\tGenoLigand\t\n",
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

    let inputs = extract_inputs(&input).expect("extract inputs");
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

    assert!(ranking.contains("0.6300\t1.0000\t0.0000\t0.0000\t0.0000\t0.8000"));
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
fn structure_evaluate_writes_viewer_from_real_coordinate_file() {
    let root = temp_root("structure_viewer_real");
    let run = root.join("run");
    let store = root.join("science_store");
    let input = root.join("input.json");
    ensure_dir(&store.join("indexes")).expect("indexes");
    ensure_dir(&store.join("downloads/rcsb/mmcif/0g")).expect("coords");
    write_text(&input, "{\"inputs\":[],\"files\":[]}\n").expect("input");
    write_text(&run.join("input_normalized_manifest.tsv"), "input_id\n").expect("manifest");
    write_text(
            &run.join("feature_manifest.tsv"),
            "input_id\tinput_kind\tfeature_family\tfeature_name\tfeature_value\tfeature_source\tfeature_token\ninput_1\tsmall_molecule\tligand\ttext\taspirin\tuser\taspirin\n",
        )
        .expect("features");
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
fn structure_evaluate_reads_canonical_index_rows_beyond_source_sampling_limit() {
    let root = temp_root("structure_index_full_scan");
    let run = root.join("run");
    let store = root.join("science_store");
    let input = root.join("input.json");
    ensure_dir(&store.join("indexes")).expect("indexes");
    ensure_dir(&store.join("downloads/rcsb/mmcif/ik")).expect("coords");
    write_text(&input, "{\"inputs\":[],\"files\":[]}\n").expect("input");
    write_text(&run.join("input_normalized_manifest.tsv"), "input_id\n").expect("manifest");
    write_text(
        &run.join("feature_manifest.tsv"),
        "input_id\tinput_kind\tfeature_family\tfeature_name\tfeature_value\tfeature_source\tfeature_token\ninput_1\tpeptide\tpeptide\tsequence_feature_token\tikv\tGenoLigand\tikv\n",
    )
    .expect("features");
    let mut index = String::from(
        "source\ttarget_id\ttarget_name\torganism\ttarget_class\tligand_tokens\tsequence_tokens\tstructure_tokens\tevidence\n",
    );
    for row in 0..MAX_SOURCE_INDEX_ROWS {
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
