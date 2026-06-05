use gls_dock::{MockDockWorker, run_docking};
use gls_fixtures::dev_fixture_manifest;
use gls_foldgate::{MockFoldWorker, MockPocketWorker, run_foldgate};
use gls_genome::search_genome;
use gls_ligand::{LigandSpec, build_ligand_kernel};
use gls_patch_realizer::{PatchRealizerConfig, realize_patch};
use gls_plm_gate::{EmbeddingCache, MockPlmWorker, PlmGateConfig, run_plm_gate};
use gls_report::stage_usage;
use gls_shortcircuit::{ShortCircuitConfig, evaluate_short_circuit};
use gls_tolerance_calib::{CalibrationConfig, calibrate_tolerances, select_profiles_for_runtime};
use gls_types::{EndToEndReport, FoldabilityGateResult, PatchRealizationResult};

pub fn run_dev_smoke() -> Result<EndToEndReport, String> {
    let ligand_yaml = r#"
ligand_id: dev_ligand
ligand_class: generic
fragments:
  - exact: DHQ
  - residue: D
  - residue: H
  - residue: Q
"#;
    let fasta = ">dark_shard_1\nATGGATCATCAAGCTTGTGATGAATTTGGTTAA\n";
    let spec = LigandSpec::from_yaml(ligand_yaml)?;
    let (icg, grammar) = build_ligand_kernel(&spec);
    let genome = search_genome(fasta, &grammar, 3)?;

    let mut short_config = ShortCircuitConfig::workstation();
    short_config.max_plm_candidates_per_shard = 4;
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

    let mut cache = EmbeddingCache::default();
    let mut plm_worker = MockPlmWorker::default();
    let foldability = run_plm_gate(
        &pass_candidates,
        &pass_gates,
        &PlmGateConfig::dev_gpu_lowmem(),
        &mut cache,
        &mut plm_worker,
    )?;

    let mut fold_worker = MockFoldWorker::default();
    let mut pocket_worker = MockPocketWorker::default();
    let structure_gate = run_foldgate(
        &pass_candidates[0],
        first_foldability(&foldability)?,
        &mut fold_worker,
        &mut pocket_worker,
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
    let profiles = select_profiles_for_runtime(
        &calibration.profiles,
        &icg.constraints[0].kind,
        &cpg.ligand_class,
        &cpg.confidence_bin,
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

    let mut dock_worker = MockDockWorker::default();
    let docking = run_docking(std::slice::from_ref(&patch_result), &mut dock_worker);

    Ok(report_from_stages(
        genome.candidates.len(),
        short_results,
        foldability,
        structure_gate.accepted_for_l15,
        patch_result,
        geometry_budget.peak_memory_like(),
        docking
            .iter()
            .filter(|result| result.score.is_some())
            .count(),
    ))
}

fn first_foldability(
    foldability: &[FoldabilityGateResult],
) -> Result<&FoldabilityGateResult, String> {
    foldability
        .first()
        .ok_or_else(|| "dev_smoke_missing_foldability_result".to_string())
}

fn report_from_stages(
    l1_candidates: usize,
    short_results: Vec<gls_types::ShortCircuitGateResult>,
    foldability: Vec<FoldabilityGateResult>,
    accepted_for_l15: bool,
    patch_result: PatchRealizationResult,
    geometry_peak_mb: f64,
    docking_count: usize,
) -> EndToEndReport {
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
        notes: vec![
            "L1 candidates never call pLM directly; only L1.20 pass_plm=true is accepted by L1.25"
                .to_string(),
            "IDP/IDR outputs are trend only; no macro free energy is emitted in MVP".to_string(),
        ],
    }
}

trait GeometryBudgetExt {
    fn peak_memory_like(&self) -> f64;
}

impl GeometryBudgetExt for gls_types::GeometryBudgetReport {
    fn peak_memory_like(&self) -> f64 {
        self.memory_estimate_mb
    }
}
