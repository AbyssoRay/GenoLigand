use gls_cli::run_dev_smoke;
use gls_dock::{MockDockWorker, PersistentDockWorker, run_docking};
use gls_plm_gate::{EmbeddingCache, MockPlmWorker, PlmGateConfig, run_plm_gate};
use gls_report::render_markdown;
use gls_types::{
    CandidateGrade, CandidateId, DockingResult, FoldabilityGateResult, LocusClass, OrfCandidate,
    PatchRealizationResult, Provenance, ShortCircuitDecision, ShortCircuitGateResult,
};
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
    for (schema, valid, invalid, validator) in [
        (
            "schemas/short_circuit_gate.schema.json",
            "schema_examples/short_circuit_gate.valid.json",
            "schema_examples/short_circuit_gate.invalid.json",
            validate_short_circuit_gate as fn(&str) -> bool,
        ),
        (
            "schemas/tolerance_profile.schema.json",
            "schema_examples/tolerance_profile.valid.json",
            "schema_examples/tolerance_profile.invalid.json",
            validate_tolerance_profile,
        ),
        (
            "schemas/fixture_manifest.schema.json",
            "schema_examples/fixture_manifest.valid.json",
            "schema_examples/fixture_manifest.invalid.json",
            validate_fixture_manifest,
        ),
        (
            "schemas/calibration_report.schema.json",
            "schema_examples/calibration_report.valid.json",
            "schema_examples/calibration_report.invalid.json",
            validate_calibration_report,
        ),
        (
            "schemas/geometry_budget_report.schema.json",
            "schema_examples/geometry_budget_report.valid.json",
            "schema_examples/geometry_budget_report.invalid.json",
            validate_geometry_budget_report,
        ),
        (
            "schemas/patch_realization.schema.json",
            "schema_examples/patch_realization.valid.json",
            "schema_examples/patch_realization.invalid.json",
            validate_patch_realization,
        ),
    ] {
        let schema_value = read_text(root.join(schema));
        let valid_value = read_text(root.join(valid));
        let invalid_value = read_text(root.join(invalid));

        assert!(validator(&valid_value), "{valid} should validate");
        assert!(!validator(&invalid_value), "{invalid} should be rejected");
        assert!(schema_value.contains("\"schema_version\""));
        assert!(schema_value.contains("\"const\": \"1.6\""));
        assert!(valid_value.contains("\"schema_version\": \"1.6\""));
        assert!(invalid_value.contains("\"schema_version\": \"1.6\""));
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

fn validate_short_circuit_gate(value: &str) -> bool {
    has(value, "\"candidate_id\"")
        && has(value, "\"decision\": \"pass_plm\"")
        && has(value, "\"pass_plm\": true")
        && has(value, "\"failure_reason\": null")
        && !has(value, "\"failure_reason\": \"budget exhausted\"")
}

fn validate_tolerance_profile(value: &str) -> bool {
    has(value, "\"generated_by_fixture_calibration\": true")
        && has(value, "\"radius_tolerance\"")
        && has(value, "\"angle_tolerance_deg\"")
        && has(value, "\"min_sasa\"")
        && has(value, "\"max_occlusion\"")
        && has(value, "\"min_pocket_overlap_score\"")
        && has(value, "\"min_rotamer_feasibility_score\"")
        && has(value, "\"max_bucket_occupancy\"")
        && has(value, "\"max_seed_triplets\"")
        && has(value, "\"hard_negative_false_positive_rate\"")
}

fn validate_fixture_manifest(value: &str) -> bool {
    has(value, "\"dataset_id\"")
        && has(value, "\"fixture_type\": \"positive_holo\"")
        && has(value, "\"ligand_id\"")
        && has(value, "\"pocket_id\"")
        && has(value, "\"interaction_labels\": [\"hydrogen_bond:D-H-Q\"]")
        && has(value, "\"resolution_angstrom\": 1.8")
        && has(value, "\"interaction_kind\": \"hydrogen_bond\"")
        && !has(value, "\"license_note\": \"\"")
        && !has(value, "\"interaction_labels\": []")
        && !has(value, "\"resolution_angstrom\": 0")
}

fn validate_calibration_report(value: &str) -> bool {
    has(value, "\"profiles\"")
        && has(value, "\"profile_level\": \"narrow\"")
        && has(value, "\"profile_level\": \"standard\"")
        && has(value, "\"profile_level\": \"relaxed\"")
        && has(value, "\"generated_by_fixture_calibration\": true")
        && has(value, "\"seed_triplets_p95\"")
        && has(value, "\"oom_risk_score\": 0.1")
        && !has(value, "\"generated_by_fixture_calibration\": false")
        && !has(value, "\"oom_risk_score\": 1.5")
}

fn validate_geometry_budget_report(value: &str) -> bool {
    has(value, "\"max_bucket_occupancy_observed\": 3")
        && has(value, "\"memory_estimate_mb\": 4.5")
        && has(
            value,
            "\"relaxed_profiles_tried\": [\"narrow\", \"standard\"]",
        )
        && has(value, "\"failure_reason\": null")
}

fn validate_patch_realization(value: &str) -> bool {
    has(value, "\"pass_l2\": true")
        && has(value, "\"matched_constraints\": 3")
        && has(value, "\"failure_reason\": null")
        && has(value, "\"source\": \"gls-patch-realizer\"")
}

fn has(value: &str, needle: &str) -> bool {
    value.contains(needle)
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
