use gls_cli::run_dev_smoke;
use gls_dock::{MockDockWorker, PersistentDockWorker, run_docking};
use gls_plm_gate::{EmbeddingCache, MockPlmWorker, PlmGateConfig, run_plm_gate};
use gls_report::render_markdown;
use gls_types::{
    CandidateGrade, CandidateId, DockingResult, FoldabilityGateResult, LocusClass, OrfCandidate,
    PatchRealizationResult, Provenance, ShortCircuitDecision, ShortCircuitGateResult,
};
use serde_json::Value;
use std::collections::BTreeSet;
use std::process::Command;

#[test]
fn dev_smoke_runs_l0_to_l2_and_reports_every_required_layer() {
    let report = run_dev_smoke().expect("dev smoke should complete");
    let markdown = render_markdown(&report);

    for stage in ["L1", "L1.20", "L1.25", "L1.5", "L1.5 patch-realizer", "L2"] {
        assert!(markdown.contains(stage), "missing stage {stage}");
    }
    assert!(markdown.contains("Candidates In"));
    assert!(markdown.contains("Peak MB"));
    assert!(markdown.contains("trend only"));
    assert!(markdown.contains("Candidate Audit"));
    assert!(markdown.contains("Artifact Hash"));
    assert!(markdown.contains("gls-plm-gate"));
    assert!(markdown.contains("gls-foldgate-orf-extension"));
    assert!(markdown.contains("gls-tolerance-calib"));
    assert!(markdown.contains("gls-geometry"));
    assert!(markdown.contains("pLM budget profile dev_gpu_lowmem enforced"));
}

#[test]
fn cli_dev_smoke_uses_persistent_worker_subprocess_protocol() {
    let output = Command::new(env!("CARGO_BIN_EXE_gls-cli"))
        .arg("dev-smoke")
        .output()
        .expect("run gls-cli dev-smoke");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    assert!(stdout.contains("persistent worker subprocess protocol"));
    assert!(stdout.contains("gls-foldgate-pocket-worker"));
    assert!(stdout.contains("gls-dock"));
}

#[test]
fn cli_external_worker_override_fails_loudly_when_missing() {
    let output = Command::new(env!("CARGO_BIN_EXE_gls-cli"))
        .arg("dev-smoke")
        .env(
            "GENOLIGAND_PLM_WORKER_BIN",
            definitely_missing_worker_path(),
        )
        .output()
        .expect("run gls-cli dev-smoke with missing external worker");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("worker_spawn_failed:plm"), "{stderr}");
}

#[test]
fn cli_plm_budget_profile_is_enforced_from_yaml() {
    let output = Command::new(env!("CARGO_BIN_EXE_gls-cli"))
        .arg("dev-smoke")
        .env("GENOLIGAND_PLM_BUDGET_PROFILE", "dev_cpu_smoke")
        .output()
        .expect("run gls-cli dev-smoke with zero pLM budget");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("dev_smoke_expected_at_least_one_pass_plm_candidate"),
        "{stderr}"
    );
}

#[test]
fn cli_dev_smoke_accepts_ligand_and_genome_files() {
    let dir = std::env::temp_dir().join(format!("genoligand-inputs-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let ligand = dir.join("ligand.yaml");
    let fasta = dir.join("genome.fa");
    std::fs::write(
        &ligand,
        "\
ligand_id: file_ligand
ligand_class: generic
fragments:
  - exact: DHQ
  - residue: D
  - residue: H
  - residue: Q
",
    )
    .expect("write ligand");
    std::fs::write(&fasta, ">file_shard\nATGGATCATCAAGCTTGTGATGAATTTGGTTAA\n")
        .expect("write fasta");

    let output = Command::new(env!("CARGO_BIN_EXE_gls-cli"))
        .arg("dev-smoke")
        .arg("--ligand-yaml")
        .arg(&ligand)
        .arg("--genome-fasta")
        .arg(&fasta)
        .output()
        .expect("run gls-cli dev-smoke with files");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    assert!(stdout.contains("file_shard"));
    assert!(stdout.contains("L2"));
}

#[test]
fn cli_calibrate_patch_tolerance_outputs_schema_valid_report() {
    let root = workspace_root();
    let schema = parse_json(
        &read_text(root.join("schemas/calibration_report.schema.json")),
        "schemas/calibration_report.schema.json",
    );
    let output = Command::new(env!("CARGO_BIN_EXE_gls-cli"))
        .arg("gls-calibrate-patch-tolerance")
        .output()
        .expect("run calibration command");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = parse_json(
        &String::from_utf8(output.stdout).expect("utf8 calibration stdout"),
        "calibration stdout",
    );
    validate_schema(&schema, &report).expect("calibration output validates");
    assert_eq!(report["dataset_id"], "dev-fixtures-v1.6");
    assert_eq!(report["profiles"].as_array().expect("profiles").len(), 15);
    assert!(report["profiles"].as_array().expect("profiles").iter().any(
        |profile| profile["interaction_kind"] == "metal_coordination"
            && profile["coordinate_noise_level"] == "high"
    ));
}

#[test]
fn cli_calibrate_patch_tolerance_accepts_curated_fixture_tsv() {
    let dir = std::env::temp_dir().join(format!("genoligand-fixtures-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let tsv = dir.join("fixtures.tsv");
    std::fs::write(
        &tsv,
        "\
fixture_id\tfixture_type\tsplit\tsource\tlicense_note\tligand_id\tpocket_id\tinteraction_labels\tresolution_angstrom\tinteraction_kind\tligand_class\tstructure_confidence_bin\tresidue_flexibility_bin\tpocket_depth_bin\tcoordinate_noise_level\tresidue_signature\tobserved_radius_error\tobserved_angle_error_deg\tobserved_sasa\tobserved_occlusion\tobserved_pocket_overlap_score\tobserved_rotamer_feasibility_score\tobserved_bucket_occupancy\tobserved_seed_triplets\truntime_ms\tpeak_memory_mb
pos1\tpositive_holo\ttrain\tcurated-positive\tinternal\tlig1\tpocket1\thydrogen_bond:D-H-Q\t1.8\thydrogen_bond\tgeneric\thigh\tmedium\tshallow\tlow\tD-H-Q\t0.2\t5.0\t0.5\t0.2\t0.4\t0.8\t2\t4\t2.0\t1.0
neg1\thard_negative_same_residues\tholdout\tcurated-negative\tinternal\tlig2\tpocket2\thydrogen_bond:D-H-Q\t2.0\thydrogen_bond\tgeneric\thigh\tmedium\tshallow\tlow\tD-H-Q\t3.0\t60.0\t0.1\t0.8\t0.1\t0.3\t1\t4\t2.0\t1.0",
    )
    .expect("write fixture tsv");

    let output = Command::new(env!("CARGO_BIN_EXE_gls-cli"))
        .arg("calibrate-patch-tolerance")
        .arg("--fixture-tsv")
        .arg(&tsv)
        .arg("--dataset-id")
        .arg("curated-cli-v1")
        .output()
        .expect("run calibration command with TSV");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = parse_json(
        &String::from_utf8(output.stdout).expect("utf8 calibration stdout"),
        "calibration stdout",
    );
    assert_eq!(report["dataset_id"], "curated-cli-v1");
    assert_eq!(report["profiles"].as_array().expect("profiles").len(), 3);
}

#[test]
fn plm_gate_contract_rejects_any_non_pass_plm_input() {
    let candidate = candidate("c", "DHQACDEFG");
    let gate = gate(&candidate, false);
    let err = run_plm_gate(
        std::slice::from_ref(&candidate),
        &[gate],
        &PlmGateConfig::dev_gpu_lowmem(),
        &mut EmbeddingCache::default(),
        &mut MockPlmWorker::default(),
    )
    .expect_err("contract violation should fail");

    assert_eq!(err, "gls_plm_gate_input_contains_pass_plm_false");
}

#[test]
fn docking_contract_keeps_non_pass_l2_out_of_worker_results() {
    let mut worker = MockDockWorker::default();
    let results = run_docking(&[patch("no", false)], &mut worker).expect("dock");

    assert_eq!(
        results,
        vec![DockingResult {
            candidate_id: CandidateId::from("no"),
            pass_l2_input: false,
            score: None,
            worker_reused: true,
            failure_reason: Some("docking_requires_pass_l2_true".to_string()),
        }]
    );
    assert_eq!(worker.process_start_count(), 0);
}

#[test]
fn v16_contract_files_exist_with_positive_and_negative_examples() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("workspace root");
    for path in [
        "schemas/short_circuit_gate.schema.json",
        "schemas/tolerance_profile.schema.json",
        "schemas/fixture_manifest.schema.json",
        "schemas/calibration_report.schema.json",
        "schemas/geometry_budget_report.schema.json",
        "schemas/patch_realization.schema.json",
        "schema_examples/short_circuit_gate.valid.json",
        "schema_examples/short_circuit_gate.invalid.json",
        "schema_examples/tolerance_profile.valid.json",
        "schema_examples/tolerance_profile.invalid.json",
        "schema_examples/fixture_manifest.valid.json",
        "schema_examples/fixture_manifest.invalid.json",
        "schema_examples/calibration_report.valid.json",
        "schema_examples/calibration_report.invalid.json",
        "schema_examples/geometry_budget_report.valid.json",
        "schema_examples/geometry_budget_report.invalid.json",
        "schema_examples/patch_realization.valid.json",
        "schema_examples/patch_realization.invalid.json",
        "docs/pipeline_contract.md",
        "workflows/main.nf",
        "workflows/modules/gls_dev_smoke.nf",
        "tools/cargo-runner.sh",
        "config/plm_budget.yaml",
        "workers/plm-worker/README.md",
        "workers/plm-worker/external_line_worker.py",
        "workers/fold-worker/README.md",
        "workers/fold-worker/external_line_worker.py",
        "workers/pocket-worker/README.md",
        "workers/pocket-worker/external_line_worker.py",
        "workers/dock-worker/README.md",
        "workers/dock-worker/vina_worker.py",
    ] {
        assert!(root.join(path).exists(), "missing {path}");
    }

    let workflow = std::fs::read_to_string(root.join("workflows/main.nf"))
        .expect("workflow should be readable");
    let module = std::fs::read_to_string(root.join("workflows/modules/gls_dev_smoke.nf"))
        .expect("workflow module should be readable");
    assert!(workflow.contains("include { GLS_DEV_SMOKE }"));
    assert!(workflow.contains("sourceFingerprint()"));
    assert!(workflow.contains("'workers'"));
    assert!(module.contains("storeDir"));
    assert!(module.contains("source_fingerprint"));
    assert!(module.contains("tools/cargo-runner.sh"));
    assert!(module.contains("--manifest-path"));
}

#[test]
fn v16_schema_examples_validate_and_reject() {
    let root = workspace_root();
    for (schema, valid, invalid) in [
        (
            "schemas/short_circuit_gate.schema.json",
            "schema_examples/short_circuit_gate.valid.json",
            "schema_examples/short_circuit_gate.invalid.json",
        ),
        (
            "schemas/tolerance_profile.schema.json",
            "schema_examples/tolerance_profile.valid.json",
            "schema_examples/tolerance_profile.invalid.json",
        ),
        (
            "schemas/fixture_manifest.schema.json",
            "schema_examples/fixture_manifest.valid.json",
            "schema_examples/fixture_manifest.invalid.json",
        ),
        (
            "schemas/calibration_report.schema.json",
            "schema_examples/calibration_report.valid.json",
            "schema_examples/calibration_report.invalid.json",
        ),
        (
            "schemas/geometry_budget_report.schema.json",
            "schema_examples/geometry_budget_report.valid.json",
            "schema_examples/geometry_budget_report.invalid.json",
        ),
        (
            "schemas/patch_realization.schema.json",
            "schema_examples/patch_realization.valid.json",
            "schema_examples/patch_realization.invalid.json",
        ),
    ] {
        let schema_text = read_text(root.join(schema));
        let schema_value = parse_json(&schema_text, schema);
        let valid_value = parse_json(&read_text(root.join(valid)), valid);
        let invalid_value = parse_json(&read_text(root.join(invalid)), invalid);

        validate_schema(&schema_value, &valid_value)
            .unwrap_or_else(|err| panic!("{valid} should validate against {schema}: {err}"));
        assert!(
            validate_schema(&schema_value, &invalid_value).is_err(),
            "{invalid} should be rejected by {schema}"
        );
        assert!(schema_text.contains("\"schema_version\""));
        assert!(schema_text.contains("\"const\": \"1.6\""));
        assert_eq!(valid_value["schema_version"], "1.6");
        assert_eq!(invalid_value["schema_version"], "1.6");
    }
}

fn workspace_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("workspace root")
        .to_path_buf()
}

fn read_text(path: impl AsRef<std::path::Path>) -> String {
    let path = path.as_ref();
    std::fs::read_to_string(path).unwrap_or_else(|err| {
        panic!("failed to read {}: {err}", path.display());
    })
}

fn parse_json(text: &str, path: &str) -> Value {
    serde_json::from_str(text).unwrap_or_else(|err| panic!("failed to parse {path}: {err}"))
}

fn validate_schema(schema: &Value, value: &Value) -> Result<(), String> {
    validate_schema_at("$", schema, value)
}

fn validate_schema_at(path: &str, schema: &Value, value: &Value) -> Result<(), String> {
    if let Some(expected) = schema.get("const")
        && value != expected
    {
        return Err(format!("{path}: const mismatch"));
    }
    if let Some(options) = schema.get("enum").and_then(Value::as_array)
        && !options.iter().any(|option| option == value)
    {
        return Err(format!("{path}: enum mismatch"));
    }
    if let Some(schema_type) = schema.get("type") {
        let allowed = match schema_type {
            Value::String(one) => vec![one.as_str()],
            Value::Array(many) => many.iter().filter_map(Value::as_str).collect::<Vec<_>>(),
            _ => return Err(format!("{path}: invalid schema type field")),
        };
        if !allowed.iter().any(|kind| json_type_matches(kind, value)) {
            return Err(format!("{path}: type mismatch expected {allowed:?}"));
        }
    }

    validate_number_bounds(path, schema, value)?;
    validate_string_bounds(path, schema, value)?;
    validate_array_bounds(path, schema, value)?;
    validate_object_shape(path, schema, value)?;
    validate_all_of(path, schema, value)?;
    Ok(())
}

fn json_type_matches(kind: &str, value: &Value) -> bool {
    match kind {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "number" => value.is_number(),
        "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
        "boolean" => value.is_boolean(),
        "null" => value.is_null(),
        _ => false,
    }
}

fn validate_number_bounds(path: &str, schema: &Value, value: &Value) -> Result<(), String> {
    let Some(number) = value.as_f64() else {
        return Ok(());
    };
    if let Some(minimum) = schema.get("minimum").and_then(Value::as_f64)
        && number < minimum
    {
        return Err(format!("{path}: below minimum {minimum}"));
    }
    if let Some(maximum) = schema.get("maximum").and_then(Value::as_f64)
        && number > maximum
    {
        return Err(format!("{path}: above maximum {maximum}"));
    }
    if let Some(minimum) = schema.get("exclusiveMinimum").and_then(Value::as_f64)
        && number <= minimum
    {
        return Err(format!("{path}: not above exclusive minimum {minimum}"));
    }
    Ok(())
}

fn validate_string_bounds(path: &str, schema: &Value, value: &Value) -> Result<(), String> {
    let Some(text) = value.as_str() else {
        return Ok(());
    };
    if let Some(min_len) = schema.get("minLength").and_then(Value::as_u64)
        && text.chars().count() < min_len as usize
    {
        return Err(format!("{path}: shorter than minLength {min_len}"));
    }
    if let Some(pattern) = schema.get("pattern").and_then(Value::as_str)
        && !pattern_matches(pattern, text)
    {
        return Err(format!("{path}: pattern mismatch {pattern}"));
    }
    Ok(())
}

fn validate_array_bounds(path: &str, schema: &Value, value: &Value) -> Result<(), String> {
    let Some(items) = value.as_array() else {
        return Ok(());
    };
    if let Some(min_items) = schema.get("minItems").and_then(Value::as_u64)
        && items.len() < min_items as usize
    {
        return Err(format!("{path}: fewer than minItems {min_items}"));
    }
    if let Some(item_schema) = schema.get("items") {
        for (index, item) in items.iter().enumerate() {
            validate_schema_at(&format!("{path}[{index}]"), item_schema, item)?;
        }
    }
    Ok(())
}

fn validate_object_shape(path: &str, schema: &Value, value: &Value) -> Result<(), String> {
    let Some(object) = value.as_object() else {
        return Ok(());
    };
    if let Some(required) = schema.get("required").and_then(Value::as_array) {
        for key in required.iter().filter_map(Value::as_str) {
            if !object.contains_key(key) {
                return Err(format!("{path}: missing required property {key}"));
            }
        }
    }
    let properties = schema.get("properties").and_then(Value::as_object);
    if schema.get("additionalProperties").and_then(Value::as_bool) == Some(false) {
        let allowed = properties
            .map(|props| props.keys().cloned().collect::<BTreeSet<_>>())
            .unwrap_or_default();
        for key in object.keys() {
            if !allowed.contains(key) {
                return Err(format!("{path}: additional property {key}"));
            }
        }
    }
    if let Some(properties) = properties {
        for (key, property_schema) in properties {
            if let Some(property_value) = object.get(key) {
                validate_schema_at(&format!("{path}.{key}"), property_schema, property_value)?;
            }
        }
    }
    Ok(())
}

fn validate_all_of(path: &str, schema: &Value, value: &Value) -> Result<(), String> {
    let Some(entries) = schema.get("allOf").and_then(Value::as_array) else {
        return Ok(());
    };
    for (index, entry) in entries.iter().enumerate() {
        if let Some(if_schema) = entry.get("if") {
            if validate_schema_at(path, if_schema, value).is_ok()
                && let Some(then_schema) = entry.get("then")
            {
                validate_schema_at(&format!("{path}.allOf[{index}].then"), then_schema, value)?;
            }
        } else {
            validate_schema_at(&format!("{path}.allOf[{index}]"), entry, value)?;
        }
    }
    Ok(())
}

fn pattern_matches(pattern: &str, text: &str) -> bool {
    match pattern {
        "^[0-9a-f]{16,}$" => text.len() >= 16 && text.chars().all(|ch| ch.is_ascii_hexdigit()),
        "^[a-z0-9_]+$" => {
            !text.is_empty()
                && text
                    .chars()
                    .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_')
        }
        _ => true,
    }
}

fn definitely_missing_worker_path() -> String {
    let mut path = std::env::temp_dir();
    path.push(format!("genoligand-missing-worker-{}", std::process::id()));
    path.display().to_string()
}

fn candidate(id: &str, sequence: &str) -> OrfCandidate {
    OrfCandidate {
        candidate_id: CandidateId::from(id),
        shard_id: "s".to_string(),
        sequence: sequence.to_string(),
        nucleotide_range: (0, sequence.len() * 3),
        frame: 1,
        locus_class: LocusClass::Unknown,
        stop_to_stop: true,
        fragment_family: "fam".to_string(),
        provenance: Provenance::new("test", "1", id),
        grade: CandidateGrade::G2,
    }
}

fn gate(candidate: &OrfCandidate, pass_plm: bool) -> ShortCircuitGateResult {
    ShortCircuitGateResult {
        candidate_id: candidate.candidate_id.clone(),
        decision: if pass_plm {
            ShortCircuitDecision::PassPlm
        } else {
            ShortCircuitDecision::HoldNoPlm
        },
        rule_version: "v1.6".to_string(),
        sequence_hash: candidate.sequence_hash(),
        canonical_cluster_id: Some(candidate.candidate_id.to_string()),
        aa_len: candidate.sequence.len(),
        low_complexity_score: 0.0,
        entropy_score: 2.0,
        fragment_info_score: 8.0,
        cooccur_q_value: Some(0.01),
        locus_class: LocusClass::Unknown,
        context_priority_score: 0.0,
        budget_rank: 1,
        pass_plm,
        failure_reason: None,
    }
}

fn patch(id: &str, pass_l2: bool) -> PatchRealizationResult {
    PatchRealizationResult {
        candidate_id: CandidateId::from(id),
        profile_id: "p".to_string(),
        pass_l2,
        matched_constraints: if pass_l2 { 3 } else { 0 },
        rmsd: 0.1,
        score: 0.9,
        failure_reason: None,
        audit: Vec::new(),
        provenance: Provenance::new("test", "1", id),
    }
}

#[allow(dead_code)]
fn _foldability(candidate: &OrfCandidate) -> FoldabilityGateResult {
    FoldabilityGateResult {
        candidate_id: candidate.candidate_id.clone(),
        sequence_hash: candidate.sequence_hash(),
        model_id: "m".to_string(),
        tokenizer_id: "t".to_string(),
        weights_hash: "w".to_string(),
        cache_key: "k".to_string(),
        embedding_from_cache: true,
        pass_structure: true,
        failure_reason: None,
        bucket_len: 64,
    }
}
