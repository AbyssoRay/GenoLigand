use gls_cli::run_dev_smoke;
use gls_dock::{MockDockWorker, PersistentDockWorker, run_docking};
use gls_plm_gate::{EmbeddingCache, MockPlmWorker, PlmGateConfig, run_plm_gate};
use gls_report::render_markdown;
use gls_types::{
    CandidateGrade, CandidateId, DockingResult, FoldabilityGateResult, LocusClass, OrfCandidate,
    PatchRealizationResult, Provenance, ShortCircuitDecision, ShortCircuitGateResult,
};

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
    let results = run_docking(&[patch("no", false)], &mut worker);

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
        "schemas/geometry_budget_report.schema.json",
        "schemas/patch_realization.schema.json",
        "schema_examples/short_circuit_gate.valid.json",
        "schema_examples/short_circuit_gate.invalid.json",
        "schema_examples/tolerance_profile.valid.json",
        "schema_examples/tolerance_profile.invalid.json",
        "docs/pipeline_contract.md",
        "workflows/main.nf",
        "config/plm_budget.yaml",
    ] {
        assert!(root.join(path).exists(), "missing {path}");
    }
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
