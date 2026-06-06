use gls_dock::{MockDockWorker, PersistentDockWorker, run_docking};
use gls_fixtures::{dev_fixture_manifest, fixture_manifest_from_tsv};
use gls_foldgate::{
    MockFoldWorker, MockPocketWorker, PersistentFoldWorker, PersistentPocketWorker, run_foldgate,
};
use gls_genome::search_genome;
use gls_ligand::{LigandSpec, build_ligand_kernel};
use gls_patch_realizer::{PatchRealizerConfig, realize_patch};
use gls_plm_gate::{
    EmbeddingCache, MockPlmWorker, PersistentPlmWorker, PlmGateConfig, PlmModelSpec, run_plm_gate,
};
use gls_report::stage_usage;
use gls_shortcircuit::{
    PlmBudgetProfile, ShortCircuitConfig, evaluate_short_circuit, parse_plm_budget_profile,
};
use gls_tolerance_calib::{CalibrationConfig, calibrate_tolerances, select_profiles_for_runtime};
use gls_types::{
    CandidateAuditRecord, CandidatePatchGraph, EndToEndReport, FoldabilityGateResult,
    InteractionKind, PatchNode, PatchRealizationResult, Point3Data, Provenance,
};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DevSmokeInputs {
    pub ligand_yaml: String,
    pub genome_fasta: String,
}

impl DevSmokeInputs {
    pub fn builtin() -> Self {
        Self {
            ligand_yaml: r#"
ligand_id: dev_ligand
ligand_class: generic
fragments:
  - exact: DHQ
  - residue: D
  - residue: H
  - residue: Q
"#
            .to_string(),
            genome_fasta: ">dark_shard_1\nATGGATCATCAAGCTTGTGATGAATTTGGTTAA\n".to_string(),
        }
    }

    pub fn from_optional_files(
        ligand_yaml_path: Option<&Path>,
        genome_fasta_path: Option<&Path>,
    ) -> Result<Self, String> {
        let mut inputs = Self::builtin();
        if let Some(path) = ligand_yaml_path {
            inputs.ligand_yaml = fs::read_to_string(path)
                .map_err(|err| format!("ligand_yaml_read_failed:{}:{err}", path.display()))?;
        } else if let Ok(path) = std::env::var("GENOLIGAND_LIGAND_YAML_FILE") {
            inputs.ligand_yaml = fs::read_to_string(&path)
                .map_err(|err| format!("ligand_yaml_read_failed:{path}:{err}"))?;
        }
        if let Some(path) = genome_fasta_path {
            inputs.genome_fasta = fs::read_to_string(path)
                .map_err(|err| format!("genome_fasta_read_failed:{}:{err}", path.display()))?;
        } else if let Ok(path) = std::env::var("GENOLIGAND_GENOME_FASTA_FILE") {
            inputs.genome_fasta = fs::read_to_string(&path)
                .map_err(|err| format!("genome_fasta_read_failed:{path}:{err}"))?;
        }
        Ok(inputs)
    }
}

pub fn run_dev_smoke() -> Result<EndToEndReport, String> {
    let mut plm_worker = MockPlmWorker::default();
    let mut fold_worker = MockFoldWorker::default();
    let mut pocket_worker = MockPocketWorker::default();
    let mut dock_worker = MockDockWorker::default();
    run_dev_smoke_with_workers(
        &DevSmokeInputs::builtin(),
        &mut plm_worker,
        &mut fold_worker,
        &mut pocket_worker,
        &mut dock_worker,
    )
}

pub fn run_dev_smoke_with_process_workers(
    worker_bin: impl AsRef<Path>,
) -> Result<EndToEndReport, String> {
    run_dev_smoke_with_worker_commands(WorkerCommandSet::dev(worker_bin.as_ref()))
}

pub fn run_dev_smoke_with_worker_commands(
    commands: WorkerCommandSet,
) -> Result<EndToEndReport, String> {
    run_dev_smoke_with_worker_commands_and_inputs(commands, DevSmokeInputs::builtin())
}

pub fn run_dev_smoke_with_worker_commands_and_inputs(
    commands: WorkerCommandSet,
    inputs: DevSmokeInputs,
) -> Result<EndToEndReport, String> {
    let mut plm_worker = ProcessPlmWorker::spawn_command(commands.plm)?;
    let mut fold_worker = ProcessFoldWorker::spawn_command(commands.fold)?;
    let mut pocket_worker = ProcessPocketWorker::spawn_command(commands.pocket)?;
    let mut dock_worker = ProcessDockWorker::spawn_command(commands.dock)?;
    let mut report = run_dev_smoke_with_workers(
        &inputs,
        &mut plm_worker,
        &mut fold_worker,
        &mut pocket_worker,
        &mut dock_worker,
    )?;
    report.notes.push(
        "dev smoke science-tool boundaries used persistent worker subprocess protocol".to_string(),
    );
    Ok(report)
}

pub fn run_calibrate_patch_tolerance_json(
    fixture_tsv_path: Option<&Path>,
    dataset_id: Option<&str>,
) -> Result<String, String> {
    let manifest = if let Some(path) = fixture_tsv_path {
        let tsv = fs::read_to_string(path)
            .map_err(|err| format!("fixture_tsv_read_failed:{}:{err}", path.display()))?;
        let dataset_id = dataset_id
            .map(ToOwned::to_owned)
            .or_else(|| {
                path.file_stem()
                    .and_then(|stem| stem.to_str())
                    .map(ToOwned::to_owned)
            })
            .ok_or_else(|| "fixture_tsv_dataset_id_unavailable".to_string())?;
        fixture_manifest_from_tsv(dataset_id, &tsv)?
    } else {
        dev_fixture_manifest()
    };
    let report = calibrate_tolerances(&manifest, &CalibrationConfig::dev())?;
    Ok(render_calibration_report_json(&report))
}

fn render_calibration_report_json(report: &gls_tolerance_calib::CalibrationReport) -> String {
    let profiles = report
        .profiles
        .iter()
        .map(|profile| {
            format!(
                concat!(
                    "    {{\n",
                    "      \"profile_id\": \"{}\",\n",
                    "      \"dataset_id\": \"{}\",\n",
                    "      \"created_at\": \"{}\",\n",
                    "      \"interaction_kind\": \"{}\",\n",
                    "      \"ligand_class\": \"{}\",\n",
                    "      \"structure_confidence_bin\": \"{}\",\n",
                    "      \"residue_flexibility_bin\": \"{}\",\n",
                    "      \"pocket_depth_bin\": \"{}\",\n",
                    "      \"coordinate_noise_level\": \"{}\",\n",
                    "      \"radius_tolerance\": {},\n",
                    "      \"angle_tolerance_deg\": {},\n",
                    "      \"distance_bin_width\": {},\n",
                    "      \"vector_angle_bin_width\": {},\n",
                    "      \"max_bucket_occupancy\": {},\n",
                    "      \"max_seed_triplets\": {},\n",
                    "      \"max_memory_mb\": {},\n",
                    "      \"recall_at_profile\": {},\n",
                    "      \"hard_negative_rejection\": {},\n",
                    "      \"hard_negative_false_positive_rate\": {},\n",
                    "      \"bucket_occupancy_p95\": {},\n",
                    "      \"peak_memory_mb\": {},\n",
                    "      \"runtime_ms_per_fixture\": {},\n",
                    "      \"generated_by_fixture_calibration\": {},\n",
                    "      \"profile_level\": \"{}\"\n",
                    "    }}"
                ),
                json_escape(&profile.profile_id),
                json_escape(&profile.dataset_id),
                json_escape(&profile.created_at),
                profile.interaction_kind.as_str(),
                json_escape(&profile.ligand_class),
                json_escape(&profile.structure_confidence_bin),
                json_escape(&profile.residue_flexibility_bin),
                json_escape(&profile.pocket_depth_bin),
                json_escape(&profile.coordinate_noise_level),
                json_number(profile.radius_tolerance),
                json_number(profile.angle_tolerance_deg),
                json_number(profile.distance_bin_width),
                json_number(profile.vector_angle_bin_width),
                profile.max_bucket_occupancy,
                profile.max_seed_triplets,
                json_number(profile.max_memory_mb),
                json_number(profile.recall_at_profile),
                json_number(profile.hard_negative_rejection),
                json_number(profile.hard_negative_false_positive_rate),
                json_number(profile.bucket_occupancy_p95),
                json_number(profile.peak_memory_mb),
                json_number(profile.runtime_ms_per_fixture),
                profile.generated_by_fixture_calibration,
                profile.profile_level.as_str()
            )
        })
        .collect::<Vec<_>>()
        .join(",\n");
    format!(
        concat!(
            "{{\n",
            "  \"schema_version\": \"1.6\",\n",
            "  \"dataset_id\": \"{}\",\n",
            "  \"profiles\": [\n",
            "{}\n",
            "  ],\n",
            "  \"positive_recall\": {},\n",
            "  \"hard_negative_rejection\": {},\n",
            "  \"bucket_occupancy_p95\": {},\n",
            "  \"seed_triplets_p95\": {},\n",
            "  \"max_seed_triplets_observed\": {},\n",
            "  \"peak_memory_mb\": {},\n",
            "  \"runtime_ms_per_fixture\": {},\n",
            "  \"oom_risk_score\": {}\n",
            "}}"
        ),
        json_escape(&report.dataset_id),
        profiles,
        json_number(report.positive_recall),
        json_number(report.hard_negative_rejection),
        json_number(report.bucket_occupancy_p95),
        json_number(report.seed_triplets_p95),
        report.max_seed_triplets_observed,
        json_number(report.peak_memory_mb),
        json_number(report.runtime_ms_per_fixture),
        json_number(report.oom_risk_score)
    )
}

fn json_escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

fn json_number(value: f64) -> String {
    assert!(value.is_finite(), "non-finite JSON number");
    format!("{value:.6}")
}

fn run_dev_smoke_with_workers<P, F, K, D>(
    inputs: &DevSmokeInputs,
    plm_worker: &mut P,
    fold_worker: &mut F,
    pocket_worker: &mut K,
    dock_worker: &mut D,
) -> Result<EndToEndReport, String>
where
    P: PersistentPlmWorker,
    F: PersistentFoldWorker,
    K: PersistentPocketWorker,
    D: PersistentDockWorker,
{
    let spec = LigandSpec::from_yaml(&inputs.ligand_yaml)?;
    let (icg, grammar) = build_ligand_kernel(&spec);
    let genome = search_genome(&inputs.genome_fasta, &grammar, 3)?;

    let plm_budget = load_plm_budget_profile()?;
    let mut short_config = ShortCircuitConfig::workstation();
    short_config.apply_plm_budget_profile(&plm_budget)?;
    let short_results = evaluate_short_circuit(&genome.candidates, &genome.hits, &short_config);
    let pass_gates = short_results
        .iter()
        .filter(|gate| gate.pass_plm)
        .cloned()
        .collect::<Vec<_>>();
    let pass_candidates = genome
        .candidates
        .iter()
        .filter(|candidate| {
            pass_gates
                .iter()
                .any(|gate| gate.candidate_id == candidate.candidate_id)
        })
        .cloned()
        .collect::<Vec<_>>();
    if pass_candidates.is_empty() {
        return Err("dev_smoke_expected_at_least_one_pass_plm_candidate".to_string());
    }

    let foldability = run_plm_gate(
        &pass_candidates,
        &pass_gates,
        &plm_gate_config_from_budget(&plm_budget),
        &mut EmbeddingCache::default(),
        plm_worker,
    )?;

    let structure_gate = run_foldgate(
        &pass_candidates[0],
        first_foldability(&foldability)?,
        fold_worker,
        pocket_worker,
    );
    let cpg = structure_gate
        .candidate_patch_graph
        .clone()
        .ok_or_else(|| {
            structure_gate
                .failure_reason
                .clone()
                .unwrap_or_else(|| "foldgate_failed".to_string())
        })?;

    let manifest = dev_fixture_manifest();
    let calibration = calibrate_tolerances(&manifest, &CalibrationConfig::dev())?;
    let (residue_flexibility_bin, pocket_depth_bin, coordinate_noise_level) =
        runtime_tolerance_context(&cpg);
    let profiles = select_profiles_for_runtime(
        &calibration.profiles,
        &icg.constraints[0].kind,
        &cpg.ligand_class,
        &cpg.confidence_bin,
        residue_flexibility_bin,
        pocket_depth_bin,
        coordinate_noise_level,
    );
    let (patch_result, geometry_budget) =
        realize_patch(&icg, &cpg, &profiles, &PatchRealizerConfig::dev())?;
    if !patch_result.pass_l2 {
        return Err(format!(
            "dev_smoke_patch_realization_failed:{}",
            patch_result
                .failure_reason
                .clone()
                .unwrap_or_else(|| "unknown".to_string())
        ));
    }

    let docking = run_docking(std::slice::from_ref(&patch_result), dock_worker)?;
    let audit_records = build_audit_records(AuditInputs {
        candidates: &genome.candidates,
        short_results: &short_results,
        plm_budget_profile: &plm_budget,
        foldability: &foldability,
        orf_extension: structure_gate.orf_extension.as_ref(),
        tolerance_profiles: &profiles,
        calibration_report: &calibration,
        cpg: &cpg,
        patch_result: &patch_result,
        geometry_budget: &geometry_budget,
        docking: &docking,
    });

    Ok(report_from_stages(StageReportInputs {
        l1_candidates: genome.candidates.len(),
        short_results,
        foldability,
        accepted_for_l15: structure_gate.accepted_for_l15,
        patch_result,
        geometry_peak_mb: geometry_budget.peak_memory_like(),
        docking_count: docking
            .iter()
            .filter(|result| result.score.is_some())
            .count(),
        audit_records,
        plm_budget_profile: plm_budget,
    }))
}

fn first_foldability(
    foldability: &[FoldabilityGateResult],
) -> Result<&FoldabilityGateResult, String> {
    foldability
        .first()
        .ok_or_else(|| "dev_smoke_missing_foldability_result".to_string())
}

fn runtime_tolerance_context(
    cpg: &CandidatePatchGraph,
) -> (&'static str, &'static str, &'static str) {
    let min_confidence = cpg
        .nodes
        .iter()
        .map(|node| node.confidence)
        .fold(1.0, f64::min);
    let average_depth = if cpg.nodes.is_empty() {
        0.0
    } else {
        cpg.nodes.iter().map(|node| node.pocket_depth).sum::<f64>() / cpg.nodes.len() as f64
    };
    let residue_flexibility_bin = if min_confidence < 0.70 {
        "flexible"
    } else if min_confidence >= 0.90 {
        "rigid"
    } else {
        "medium"
    };
    let pocket_depth_bin = if average_depth >= 3.0 {
        "deep"
    } else {
        "shallow"
    };
    let coordinate_noise_level = if cpg.confidence_bin == "low" {
        "high"
    } else {
        "low"
    };
    (
        residue_flexibility_bin,
        pocket_depth_bin,
        coordinate_noise_level,
    )
}

fn load_plm_budget_profile() -> Result<PlmBudgetProfile, String> {
    let profile_name = std::env::var("GENOLIGAND_PLM_BUDGET_PROFILE")
        .unwrap_or_else(|_| "dev_gpu_lowmem".to_string());
    let yaml = read_plm_budget_yaml()?;
    parse_plm_budget_profile(&yaml, &profile_name)
}

fn read_plm_budget_yaml() -> Result<String, String> {
    if let Ok(path) = std::env::var("GENOLIGAND_PLM_BUDGET_FILE") {
        return fs::read_to_string(&path)
            .map_err(|err| format!("plm_budget_read_failed:{path}:{err}"));
    }
    let cwd_path = Path::new("config/plm_budget.yaml");
    if cwd_path.exists() {
        return fs::read_to_string(cwd_path)
            .map_err(|err| format!("plm_budget_read_failed:{}:{err}", cwd_path.display()));
    }
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_path = manifest_dir
        .ancestors()
        .nth(2)
        .ok_or_else(|| "plm_budget_workspace_root_unavailable".to_string())?
        .join("config/plm_budget.yaml");
    fs::read_to_string(&workspace_path)
        .map_err(|err| format!("plm_budget_read_failed:{}:{err}", workspace_path.display()))
}

fn plm_gate_config_from_budget(profile: &PlmBudgetProfile) -> PlmGateConfig {
    let mut config = PlmGateConfig::dev_gpu_lowmem();
    if let Some(max_plm_candidates_per_shard) = profile.max_plm_candidates_per_shard {
        config.max_plm_candidates_per_shard = max_plm_candidates_per_shard;
    }
    if let Some(max_sequence_length) = profile.max_sequence_length {
        config.max_sequence_length = max_sequence_length;
    }
    config
}

struct StageReportInputs {
    l1_candidates: usize,
    short_results: Vec<gls_types::ShortCircuitGateResult>,
    foldability: Vec<FoldabilityGateResult>,
    accepted_for_l15: bool,
    patch_result: PatchRealizationResult,
    geometry_peak_mb: f64,
    docking_count: usize,
    audit_records: Vec<CandidateAuditRecord>,
    plm_budget_profile: PlmBudgetProfile,
}

fn report_from_stages(inputs: StageReportInputs) -> EndToEndReport {
    let StageReportInputs {
        l1_candidates,
        short_results,
        foldability,
        accepted_for_l15,
        patch_result,
        geometry_peak_mb,
        docking_count,
        audit_records,
        plm_budget_profile,
    } = inputs;
    let short_failures = short_results
        .iter()
        .filter_map(|result| result.failure_reason.clone())
        .collect::<Vec<_>>();
    let short_pass = short_results
        .iter()
        .filter(|result| result.pass_plm)
        .count();
    EndToEndReport {
        stages: vec![
            stage_usage(
                "L1",
                l1_candidates,
                l1_candidates,
                Vec::<String>::new(),
                1.0,
                1.0,
            ),
            stage_usage("L1.20", l1_candidates, short_pass, short_failures, 2.0, 1.0),
            stage_usage(
                "L1.25",
                short_pass,
                foldability.len(),
                foldability
                    .iter()
                    .filter_map(|result| result.failure_reason.clone())
                    .collect::<Vec<_>>(),
                3.0,
                2.0,
            ),
            stage_usage(
                "L1.5",
                foldability.len(),
                usize::from(accepted_for_l15),
                Vec::<String>::new(),
                4.0,
                4.0,
            ),
            stage_usage(
                "L1.5 patch-realizer",
                usize::from(accepted_for_l15),
                usize::from(patch_result.pass_l2),
                patch_result
                    .failure_reason
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>(),
                5.0,
                geometry_peak_mb,
            ),
            stage_usage(
                "L2",
                usize::from(patch_result.pass_l2),
                docking_count,
                Vec::<String>::new(),
                6.0,
                8.0,
            ),
        ],
        audit_records,
        notes: vec![
            "L1 candidates never call pLM directly; only L1.20 pass_plm=true is accepted by L1.25"
                .to_string(),
            "IDP/IDR outputs are trend only; no macro free energy is emitted in MVP".to_string(),
            format!(
                "pLM budget profile {} enforced with max_plm_candidates_per_shard={:?} max_sequence_length={:?}",
                plm_budget_profile.profile_name,
                plm_budget_profile.max_plm_candidates_per_shard,
                plm_budget_profile.max_sequence_length
            ),
        ],
    }
}

struct AuditInputs<'a> {
    candidates: &'a [gls_types::OrfCandidate],
    short_results: &'a [gls_types::ShortCircuitGateResult],
    plm_budget_profile: &'a PlmBudgetProfile,
    foldability: &'a [FoldabilityGateResult],
    orf_extension: Option<&'a gls_types::OrfExtensionResult>,
    tolerance_profiles: &'a [gls_types::ToleranceProfile],
    calibration_report: &'a gls_tolerance_calib::CalibrationReport,
    cpg: &'a gls_types::CandidatePatchGraph,
    patch_result: &'a PatchRealizationResult,
    geometry_budget: &'a gls_types::GeometryBudgetReport,
    docking: &'a [gls_types::DockingResult],
}

fn build_audit_records(inputs: AuditInputs<'_>) -> Vec<CandidateAuditRecord> {
    let AuditInputs {
        candidates,
        short_results,
        plm_budget_profile,
        foldability,
        orf_extension,
        tolerance_profiles,
        calibration_report,
        cpg,
        patch_result,
        geometry_budget,
        docking,
    } = inputs;
    let mut records = Vec::new();
    for candidate in candidates {
        records.push(CandidateAuditRecord::from_provenance(
            candidate.candidate_id.clone(),
            "L1",
            &candidate.provenance,
        ));
    }
    for result in short_results {
        let provenance = Provenance::new(
            "gls-shortcircuit",
            &result.rule_version,
            &format!(
                "candidate_id={};decision={};pass_plm={};sequence_hash={};budget_profile={};max_plm_candidates_per_shard={:?};max_sequence_length={:?}",
                result.candidate_id,
                result.decision.as_str(),
                result.pass_plm,
                result.sequence_hash,
                plm_budget_profile.profile_name,
                plm_budget_profile.max_plm_candidates_per_shard,
                plm_budget_profile.max_sequence_length
            ),
        );
        records.push(CandidateAuditRecord::from_provenance(
            result.candidate_id.clone(),
            "L1.20",
            &provenance,
        ));
    }
    for result in foldability {
        let provenance = Provenance::new(
            "gls-plm-gate",
            "v1.6",
            &format!(
                "candidate_id={};cache_key={};model={};tokenizer={};weights={}",
                result.candidate_id,
                result.cache_key,
                result.model_id,
                result.tokenizer_id,
                result.weights_hash
            ),
        );
        records.push(CandidateAuditRecord::from_provenance(
            result.candidate_id.clone(),
            "L1.25",
            &provenance,
        ));
    }
    if let Some(extension) = orf_extension {
        records.push(CandidateAuditRecord::from_provenance(
            extension.candidate_id.clone(),
            "L1.5 ORF extension",
            &extension.provenance,
        ));
    }
    records.push(CandidateAuditRecord::from_provenance(
        cpg.candidate_id.clone(),
        "L1.5 CPG",
        &cpg.provenance,
    ));
    for profile in tolerance_profiles {
        let provenance = Provenance::new(
            "gls-tolerance-calib",
            "v1.6",
            &format!(
                "candidate_id={};profile_id={};dataset_id={};level={};radius={:.6};angle={:.6};min_sasa={:.6};max_occlusion={:.6};seed_triplets_p95={:.3};oom_risk={:.6}",
                cpg.candidate_id,
                profile.profile_id,
                profile.dataset_id,
                profile.profile_level.as_str(),
                profile.radius_tolerance,
                profile.angle_tolerance_deg,
                profile.min_sasa,
                profile.max_occlusion,
                calibration_report.seed_triplets_p95,
                calibration_report.oom_risk_score
            ),
        );
        records.push(CandidateAuditRecord::from_provenance(
            cpg.candidate_id.clone(),
            "L1.5 tolerance-calib",
            &provenance,
        ));
    }
    records.push(CandidateAuditRecord::from_provenance(
        patch_result.candidate_id.clone(),
        "L1.5 patch-realizer",
        &patch_result.provenance,
    ));
    let provenance = Provenance::new(
        "gls-geometry",
        "v1.6",
        &format!(
            "candidate_id={};profile_id={};max_bucket={};seed_triplets={};memory_mb={:.6};guard_triggered={}",
            geometry_budget.candidate_id,
            geometry_budget.profile_id,
            geometry_budget.max_bucket_occupancy_observed,
            geometry_budget.seed_triplets_evaluated,
            geometry_budget.memory_estimate_mb,
            geometry_budget.guard_triggered
        ),
    );
    records.push(CandidateAuditRecord::from_provenance(
        geometry_budget.candidate_id.clone(),
        "L1.5 geometry-budget",
        &provenance,
    ));
    for result in docking {
        let provenance = Provenance::new(
            "gls-dock",
            "v1.6",
            &format!(
                "candidate_id={};pass_l2_input={};score={:?};worker_reused={}",
                result.candidate_id, result.pass_l2_input, result.score, result.worker_reused
            ),
        );
        records.push(CandidateAuditRecord::from_provenance(
            result.candidate_id.clone(),
            "L2",
            &provenance,
        ));
    }
    records
}

struct LineWorkerProcess {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    starts: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkerCommandSpec {
    role: String,
    program: PathBuf,
    args: Vec<String>,
}

impl WorkerCommandSpec {
    pub fn new(role: impl Into<String>, program: impl Into<PathBuf>) -> Self {
        Self {
            role: role.into(),
            program: program.into(),
            args: Vec::new(),
        }
    }

    pub fn with_arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    fn from_env_or_dev(
        role: &str,
        bin_var: &str,
        args_var: &str,
        worker_bin: &Path,
        dev_mode: &str,
    ) -> Self {
        match std::env::var(bin_var) {
            Ok(program) if !program.trim().is_empty() => {
                let mut spec = Self::new(role, PathBuf::from(program));
                if let Ok(args) = std::env::var(args_var) {
                    spec.args.extend(split_worker_args(&args));
                }
                spec
            }
            _ => Self::new(role, worker_bin.to_path_buf()).with_arg(dev_mode),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkerCommandSet {
    pub plm: WorkerCommandSpec,
    pub fold: WorkerCommandSpec,
    pub pocket: WorkerCommandSpec,
    pub dock: WorkerCommandSpec,
}

impl WorkerCommandSet {
    pub fn dev(worker_bin: &Path) -> Self {
        Self {
            plm: WorkerCommandSpec::new("plm", worker_bin.to_path_buf()).with_arg("worker-plm-dev"),
            fold: WorkerCommandSpec::new("fold", worker_bin.to_path_buf())
                .with_arg("worker-fold-dev"),
            pocket: WorkerCommandSpec::new("pocket", worker_bin.to_path_buf())
                .with_arg("worker-pocket-dev"),
            dock: WorkerCommandSpec::new("dock", worker_bin.to_path_buf())
                .with_arg("worker-dock-dev"),
        }
    }

    pub fn from_env_or_dev(worker_bin: &Path) -> Self {
        Self {
            plm: WorkerCommandSpec::from_env_or_dev(
                "plm",
                "GENOLIGAND_PLM_WORKER_BIN",
                "GENOLIGAND_PLM_WORKER_ARGS",
                worker_bin,
                "worker-plm-dev",
            ),
            fold: WorkerCommandSpec::from_env_or_dev(
                "fold",
                "GENOLIGAND_FOLD_WORKER_BIN",
                "GENOLIGAND_FOLD_WORKER_ARGS",
                worker_bin,
                "worker-fold-dev",
            ),
            pocket: WorkerCommandSpec::from_env_or_dev(
                "pocket",
                "GENOLIGAND_POCKET_WORKER_BIN",
                "GENOLIGAND_POCKET_WORKER_ARGS",
                worker_bin,
                "worker-pocket-dev",
            ),
            dock: WorkerCommandSpec::from_env_or_dev(
                "dock",
                "GENOLIGAND_DOCK_WORKER_BIN",
                "GENOLIGAND_DOCK_WORKER_ARGS",
                worker_bin,
                "worker-dock-dev",
            ),
        }
    }
}

fn split_worker_args(args: &str) -> Vec<String> {
    args.split_whitespace().map(ToString::to_string).collect()
}

impl LineWorkerProcess {
    fn spawn(spec: WorkerCommandSpec) -> Result<Self, String> {
        let mut child = Command::new(&spec.program)
            .args(&spec.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|err| format!("worker_spawn_failed:{}:{err}", spec.role))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| format!("worker_missing_stdin:{}", spec.role))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| format!("worker_missing_stdout:{}", spec.role))?;
        Ok(Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            starts: 1,
        })
    }

    fn request(&mut self, line: &str) -> Result<String, String> {
        writeln!(self.stdin, "{line}").map_err(|err| format!("worker_stdin_write_failed:{err}"))?;
        self.stdin
            .flush()
            .map_err(|err| format!("worker_stdin_flush_failed:{err}"))?;
        let mut response = String::new();
        let bytes = self
            .stdout
            .read_line(&mut response)
            .map_err(|err| format!("worker_stdout_read_failed:{err}"))?;
        if bytes == 0 {
            return Err("worker_stdout_closed".to_string());
        }
        let response = response.trim_end_matches(['\r', '\n']);
        if let Some(payload) = response.strip_prefix("OK\t") {
            return Ok(payload.to_string());
        }
        if response == "OK" {
            return Ok(String::new());
        }
        if let Some(reason) = response.strip_prefix("ERR\t") {
            return Err(reason.to_string());
        }
        Err(format!("worker_protocol_violation:{response}"))
    }

    fn starts(&self) -> usize {
        self.starts
    }
}

impl Drop for LineWorkerProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub struct ProcessPlmWorker {
    process: LineWorkerProcess,
    invocations: usize,
}

impl ProcessPlmWorker {
    pub fn spawn(worker_bin: &Path) -> Result<Self, String> {
        Self::spawn_command(WorkerCommandSet::dev(worker_bin).plm)
    }

    pub fn spawn_command(command: WorkerCommandSpec) -> Result<Self, String> {
        Ok(Self {
            process: LineWorkerProcess::spawn(command)?,
            invocations: 0,
        })
    }
}

impl PersistentPlmWorker for ProcessPlmWorker {
    fn embed_batch(
        &mut self,
        model: &PlmModelSpec,
        bucket_len: usize,
        sequences: &[(String, String)],
    ) -> Result<Vec<Vec<f32>>, String> {
        self.invocations += 1;
        let mut fields = vec![
            "EMBED".to_string(),
            model.model_id.clone(),
            model.tokenizer_id.clone(),
            model.weights_hash.clone(),
            bucket_len.to_string(),
        ];
        for (key, sequence) in sequences {
            fields.push(key.clone());
            fields.push(sequence.clone());
        }
        parse_embedding_payload(&self.process.request(&fields.join("\t"))?)
    }

    fn invocation_count(&self) -> usize {
        self.invocations
    }
}

pub struct ProcessFoldWorker {
    process: LineWorkerProcess,
}

impl ProcessFoldWorker {
    pub fn spawn(worker_bin: &Path) -> Result<Self, String> {
        Self::spawn_command(WorkerCommandSet::dev(worker_bin).fold)
    }

    pub fn spawn_command(command: WorkerCommandSpec) -> Result<Self, String> {
        Ok(Self {
            process: LineWorkerProcess::spawn(command)?,
        })
    }
}

impl PersistentFoldWorker for ProcessFoldWorker {
    fn fold_local_structure(
        &mut self,
        candidate: &gls_types::OrfCandidate,
    ) -> Result<String, String> {
        self.process.request(&format!(
            "FOLD\t{}\t{}",
            candidate.candidate_id, candidate.sequence
        ))
    }
}

pub struct ProcessPocketWorker {
    process: LineWorkerProcess,
}

impl ProcessPocketWorker {
    pub fn spawn(worker_bin: &Path) -> Result<Self, String> {
        Self::spawn_command(WorkerCommandSet::dev(worker_bin).pocket)
    }

    pub fn spawn_command(command: WorkerCommandSpec) -> Result<Self, String> {
        Ok(Self {
            process: LineWorkerProcess::spawn(command)?,
        })
    }
}

impl PersistentPocketWorker for ProcessPocketWorker {
    fn detect_pockets(
        &mut self,
        candidate: &gls_types::OrfCandidate,
        structure_id: &str,
    ) -> Result<CandidatePatchGraph, String> {
        let payload = self.process.request(&format!(
            "POCKET\t{}\t{}\t{}",
            candidate.candidate_id, structure_id, candidate.sequence
        ))?;
        let nodes = parse_patch_nodes(&candidate.candidate_id, &payload)?;
        Ok(CandidatePatchGraph {
            candidate_id: candidate.candidate_id.clone(),
            nodes,
            provenance: Provenance::new(
                "gls-foldgate-pocket-worker",
                "v1.6",
                &format!(
                    "candidate_id={};structure_id={};detector=p2rank_or_fpocket;protocol=line-worker",
                    candidate.candidate_id, structure_id
                ),
            ),
            confidence_bin: "high".to_string(),
            ligand_class: "generic".to_string(),
        })
    }
}

pub struct ProcessDockWorker {
    process: LineWorkerProcess,
}

impl ProcessDockWorker {
    pub fn spawn(worker_bin: &Path) -> Result<Self, String> {
        Self::spawn_command(WorkerCommandSet::dev(worker_bin).dock)
    }

    pub fn spawn_command(command: WorkerCommandSpec) -> Result<Self, String> {
        Ok(Self {
            process: LineWorkerProcess::spawn(command)?,
        })
    }
}

impl PersistentDockWorker for ProcessDockWorker {
    fn dock_batch(&mut self, candidates: &[PatchRealizationResult]) -> Result<Vec<f64>, String> {
        let mut fields = vec!["DOCK".to_string()];
        for candidate in candidates {
            fields.push(format!(
                "{}|{}|{:.6}",
                candidate.candidate_id, candidate.profile_id, candidate.score
            ));
        }
        parse_score_payload(&self.process.request(&fields.join("\t"))?)
    }

    fn process_start_count(&self) -> usize {
        self.process.starts()
    }
}

fn parse_embedding_payload(payload: &str) -> Result<Vec<Vec<f32>>, String> {
    if payload.is_empty() {
        return Ok(Vec::new());
    }
    payload
        .split(';')
        .map(|vector| {
            vector
                .split(',')
                .map(|value| {
                    value
                        .parse::<f32>()
                        .map_err(|err| format!("plm_worker_bad_embedding_value:{err}"))
                })
                .collect::<Result<Vec<_>, _>>()
        })
        .collect()
}

fn parse_score_payload(payload: &str) -> Result<Vec<f64>, String> {
    if payload.is_empty() {
        return Ok(Vec::new());
    }
    payload
        .split(';')
        .map(|value| {
            value
                .parse::<f64>()
                .map_err(|err| format!("dock_worker_bad_score:{err}"))
        })
        .collect()
}

fn parse_patch_nodes(
    candidate_id: &gls_types::CandidateId,
    payload: &str,
) -> Result<Vec<PatchNode>, String> {
    if payload.is_empty() {
        return Err("pocket_worker_returned_no_nodes".to_string());
    }
    payload
        .split(';')
        .enumerate()
        .map(|(idx, node)| {
            let fields = node.split(',').collect::<Vec<_>>();
            if fields.len() != 8 {
                return Err(format!("pocket_worker_bad_node_field_count:{idx}"));
            }
            let residue = fields[0]
                .chars()
                .next()
                .ok_or_else(|| format!("pocket_worker_missing_residue:{idx}"))?;
            Ok(PatchNode {
                node_id: format!("{candidate_id}_pocket_{idx}"),
                residue,
                kind: InteractionKind::HydrogenBond,
                point: Point3Data::new(
                    parse_f64(fields[1], "x")?,
                    parse_f64(fields[2], "y")?,
                    parse_f64(fields[3], "z")?,
                ),
                vector: Point3Data::new(1.0, 0.0, 0.0),
                sasa: parse_f64(fields[4], "sasa")?,
                occlusion: parse_f64(fields[5], "occlusion")?,
                pocket_depth: parse_f64(fields[6], "pocket_depth")?,
                confidence: parse_f64(fields[7], "confidence")?,
            })
        })
        .collect()
}

fn parse_f64(value: &str, field: &str) -> Result<f64, String> {
    value
        .parse::<f64>()
        .map_err(|err| format!("worker_bad_float:{field}:{err}"))
}

pub fn run_worker_command(command: &str) -> Result<(), String> {
    match command {
        "worker-plm-dev" => run_line_worker(handle_plm_worker_line),
        "worker-fold-dev" => run_line_worker(handle_fold_worker_line),
        "worker-pocket-dev" => run_line_worker(handle_pocket_worker_line),
        "worker-dock-dev" => run_line_worker(handle_dock_worker_line),
        _ => Err(format!("unknown_worker_command:{command}")),
    }
}

fn run_line_worker(handle: fn(&str) -> Result<String, String>) -> Result<(), String> {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = line.map_err(|err| format!("worker_stdin_read_failed:{err}"))?;
        let response = match handle(&line) {
            Ok(payload) if payload.is_empty() => "OK".to_string(),
            Ok(payload) => format!("OK\t{payload}"),
            Err(reason) => format!("ERR\t{reason}"),
        };
        writeln!(stdout, "{response}")
            .map_err(|err| format!("worker_stdout_write_failed:{err}"))?;
        stdout
            .flush()
            .map_err(|err| format!("worker_stdout_flush_failed:{err}"))?;
    }
    Ok(())
}

fn handle_plm_worker_line(line: &str) -> Result<String, String> {
    let fields = line.split('\t').collect::<Vec<_>>();
    if fields.len() < 5 || fields[0] != "EMBED" {
        return Err("plm_worker_bad_request".to_string());
    }
    let bucket_len = fields[4]
        .parse::<usize>()
        .map_err(|err| format!("plm_worker_bad_bucket:{err}"))?;
    let sequence_fields = &fields[5..];
    if sequence_fields.len() % 2 != 0 {
        return Err("plm_worker_bad_key_sequence_pairs".to_string());
    }
    let vectors = sequence_fields
        .chunks(2)
        .map(|pair| {
            let sequence = pair[1];
            dev_embedding(sequence, bucket_len)
                .iter()
                .map(|value| format!("{value:.6}"))
                .collect::<Vec<_>>()
                .join(",")
        })
        .collect::<Vec<_>>();
    Ok(vectors.join(";"))
}

fn handle_fold_worker_line(line: &str) -> Result<String, String> {
    let fields = line.split('\t').collect::<Vec<_>>();
    if fields.len() != 3 || fields[0] != "FOLD" {
        return Err("fold_worker_bad_request".to_string());
    }
    Ok(format!("structure_{}", fields[1]))
}

fn handle_pocket_worker_line(line: &str) -> Result<String, String> {
    let fields = line.split('\t').collect::<Vec<_>>();
    if fields.len() != 4 || fields[0] != "POCKET" {
        return Err("pocket_worker_bad_request".to_string());
    }
    let sequence = fields[3];
    let residues = sequence
        .chars()
        .filter(|residue| matches!(residue, 'D' | 'H' | 'Q'))
        .take(3)
        .collect::<Vec<_>>();
    let residues = if residues.is_empty() {
        sequence.chars().take(3).collect::<Vec<_>>()
    } else {
        residues
    };
    if residues.is_empty() {
        return Err("pocket_worker_no_candidate_residues".to_string());
    }
    let nodes = residues
        .into_iter()
        .enumerate()
        .map(|(idx, residue)| format!("{residue},{idx:.1},0.0,0.0,0.5,0.1,2.0,0.8"))
        .collect::<Vec<_>>();
    Ok(nodes.join(";"))
}

fn handle_dock_worker_line(line: &str) -> Result<String, String> {
    let fields = line.split('\t').collect::<Vec<_>>();
    if fields.is_empty() || fields[0] != "DOCK" {
        return Err("dock_worker_bad_request".to_string());
    }
    let scores = fields[1..]
        .iter()
        .map(|entry| {
            let parts = entry.split('|').collect::<Vec<_>>();
            if parts.len() != 3 {
                return Err("dock_worker_bad_entry".to_string());
            }
            let patch_score = parts[2];
            let patch_score = patch_score
                .parse::<f64>()
                .map_err(|err| format!("dock_worker_bad_patch_score:{err}"))?;
            Ok(format!("{:.6}", -7.0 - patch_score))
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(scores.join(";"))
}

fn dev_embedding(sequence: &str, bucket_len: usize) -> [f32; 3] {
    let len = sequence.len().max(1) as f32;
    let hydrophobic = sequence
        .chars()
        .filter(|residue| matches!(residue, 'A' | 'V' | 'I' | 'L' | 'M' | 'F' | 'W' | 'Y'))
        .count() as f32
        / len;
    [
        len / bucket_len.max(1) as f32,
        hydrophobic,
        1.0 - hydrophobic,
    ]
}

trait GeometryBudgetExt {
    fn peak_memory_like(&self) -> f64;
}

impl GeometryBudgetExt for gls_types::GeometryBudgetReport {
    fn peak_memory_like(&self) -> f64 {
        self.memory_estimate_mb
    }
}
