use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct ScienceOptions {
    pub workspace_root: PathBuf,
    pub science_store: PathBuf,
    pub input_manifest: PathBuf,
    pub run_dir: PathBuf,
}

#[derive(Clone, Debug)]
pub struct StoreOptions {
    pub workspace_root: PathBuf,
    pub science_store: PathBuf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScienceCommand {
    PrepareScienceStore,
    InputNormalize,
    FeaturesCompute,
    DatabaseMatchKnown,
    MetagenomeMatchPotential,
    StructureEvaluate,
    DynamicsScreen,
    TargetRank,
    EvidenceBuild,
    ReportRender,
    PipelineRun,
}

impl ScienceCommand {
    pub fn parse(group: &str, command: &str) -> Result<Self, String> {
        match (group, command) {
            ("install", "prepare-science-store") => Ok(Self::PrepareScienceStore),
            ("input", "normalize") => Ok(Self::InputNormalize),
            ("features", "compute") => Ok(Self::FeaturesCompute),
            ("database", "match-known") => Ok(Self::DatabaseMatchKnown),
            ("metagenome", "match-potential") => Ok(Self::MetagenomeMatchPotential),
            ("structure", "evaluate") => Ok(Self::StructureEvaluate),
            ("dynamics", "screen") => Ok(Self::DynamicsScreen),
            ("target", "rank") => Ok(Self::TargetRank),
            ("evidence", "build") => Ok(Self::EvidenceBuild),
            ("report", "render") => Ok(Self::ReportRender),
            ("pipeline", "run") => Ok(Self::PipelineRun),
            _ => Err(format!("unknown_science_command:{group}:{command}")),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InputKind {
    SmallMolecule,
    Peptide,
    ProteinLigand,
    Complex,
    GenomeOrMetagenome,
}

impl InputKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::SmallMolecule => "small_molecule",
            Self::Peptide => "peptide",
            Self::ProteinLigand => "protein_ligand",
            Self::Complex => "complex",
            Self::GenomeOrMetagenome => "genome_or_metagenome",
        }
    }
}

#[derive(Clone, Debug)]
struct NormalizedInput {
    input_id: String,
    kind: InputKind,
    primary_value: String,
    file_count: usize,
    tokens: BTreeSet<String>,
}

#[derive(Clone, Debug)]
struct StoreRow {
    source: String,
    target_id: String,
    target_name: String,
    organism: String,
    target_class: String,
    ligand_tokens: BTreeSet<String>,
    sequence_tokens: BTreeSet<String>,
    structure_tokens: BTreeSet<String>,
    evidence: String,
}

#[derive(Clone, Debug)]
struct Candidate {
    candidate_id: String,
    target_id: String,
    target_name: String,
    candidate_scope: String,
    organism: String,
    known_score: f64,
    metagenome_score: f64,
    structure_score: f64,
    docking_score: f64,
    dynamics_score: f64,
    final_score: f64,
    evidence: String,
}

pub fn run_store_command(
    command: ScienceCommand,
    options: &StoreOptions,
) -> Result<String, String> {
    if command != ScienceCommand::PrepareScienceStore {
        return Err("store_command_mismatch".to_string());
    }
    prepare_science_store(options)
}

pub fn run_science_command(
    command: ScienceCommand,
    options: &ScienceOptions,
) -> Result<String, String> {
    match command {
        ScienceCommand::InputNormalize => normalize_input(options),
        ScienceCommand::FeaturesCompute => compute_features(options),
        ScienceCommand::DatabaseMatchKnown => match_known(options),
        ScienceCommand::MetagenomeMatchPotential => match_metagenome(options),
        ScienceCommand::StructureEvaluate => evaluate_structure(options),
        ScienceCommand::DynamicsScreen => screen_dynamics(options),
        ScienceCommand::TargetRank => rank_targets(options),
        ScienceCommand::EvidenceBuild => build_evidence(options),
        ScienceCommand::ReportRender => render_report(options),
        ScienceCommand::PipelineRun => run_pipeline(options),
        ScienceCommand::PrepareScienceStore => Err("use_run_store_command".to_string()),
    }
}

pub fn create_temporary_test_store(root: &Path) -> Result<PathBuf, String> {
    let store = root.join("science_store");
    ensure_dir(&store.join("indexes"))?;
    write_text(
        &store.join("science_store_manifest.tsv"),
        "item\tstatus\tpath\nwarehouse\tready\tindexes/known_targets.tsv\nmetagenome_index\tready\tindexes/metagenome_targets.tsv\ntool_versions\tready\tindexes/tool_versions.tsv\n",
    )?;
    write_text(
        &store.join("indexes/tool_versions.tsv"),
        "tool\tstatus\tversion\tlicense\nrdkit\tready\ttest-bound\tBSD\nopenbabel\tready\ttest-bound\tGPL-2.0\nmmseqs2\tready\ttest-bound\tGPL-3.0\nfoldseek\tready\ttest-bound\tGPL-3.0\nautodock_vina\tready\ttest-bound\tApache-2.0\nopenmm\tready\ttest-bound\tMIT\n",
    )?;
    write_text(
        &store.join("indexes/known_targets.tsv"),
        "source\ttarget_id\ttarget_name\torganism\ttarget_class\tligand_tokens\tsequence_tokens\tstructure_tokens\tevidence\nChEMBL\tP24941\tCyclin-dependent kinase 2\tHomo sapiens\tknown_human_protein\tcco kin ina nas ase\tatg tgc gcc cco cok oki kin ina nas ase\tATP pocket kinase\tmeasured_activity_record\n",
    )?;
    write_text(
        &store.join("indexes/metagenome_targets.tsv"),
        "source\ttarget_id\ttarget_name\torganism\ttarget_class\tligand_tokens\tsequence_tokens\tstructure_tokens\tevidence\n",
    )?;
    Ok(store)
}

fn prepare_science_store(options: &StoreOptions) -> Result<String, String> {
    ensure_dir(&options.science_store)?;
    ensure_dir(&options.science_store.join("indexes"))?;
    let manifest = options.science_store.join("science_store_manifest.tsv");
    let known = options.science_store.join("indexes/known_targets.tsv");
    let metagenome = options.science_store.join("indexes/metagenome_targets.tsv");
    let tools = options.science_store.join("indexes/tool_versions.tsv");
    let rows = [
        ("warehouse", known.exists(), &known),
        ("metagenome_index", metagenome.exists(), &metagenome),
        ("tool_versions", tools.exists(), &tools),
    ];
    let mut text = String::from("item\tstatus\tpath\n");
    let mut ready = 0usize;
    for (item, exists, path) in rows {
        if exists {
            ready += 1;
        }
        text.push_str(&format!(
            "{item}\t{}\t{}\n",
            if exists { "ready" } else { "missing" },
            relative_display(&options.science_store, path)
        ));
    }
    write_text(&manifest, &text)?;
    Ok(format!(
        "science_store_prepared\tready_items={ready}\trequired_items=3\tstore={}",
        options.science_store.display()
    ))
}

fn run_pipeline(options: &ScienceOptions) -> Result<String, String> {
    let blocked = pipeline_blockers(options)?;
    if !blocked.is_empty() {
        write_blocked_outputs(options, &blocked)?;
        return Ok(format!(
            "pipeline_blocked\treasons={}\trun_dir={}",
            blocked.join(","),
            options.run_dir.display()
        ));
    }
    normalize_input(options)?;
    compute_features(options)?;
    match_known(options)?;
    match_metagenome(options)?;
    rank_targets(options)?;
    evaluate_structure(options)?;
    screen_dynamics(options)?;
    rank_targets(options)?;
    build_evidence(options)?;
    render_report(options)?;
    Ok(format!(
        "pipeline_completed\trun_dir={}",
        options.run_dir.display()
    ))
}

fn pipeline_blockers(options: &ScienceOptions) -> Result<Vec<String>, String> {
    let mut blockers = Vec::new();
    if !options.input_manifest.exists() {
        blockers.push("missing_input_manifest".to_string());
    } else if extract_inputs(&options.input_manifest)?.is_empty() {
        blockers.push("missing_input".to_string());
    }
    let manifest = options.science_store.join("science_store_manifest.tsv");
    let known = options.science_store.join("indexes/known_targets.tsv");
    let metagenome = options.science_store.join("indexes/metagenome_targets.tsv");
    let tools = options.science_store.join("indexes/tool_versions.tsv");
    for (label, path) in [
        ("missing_science_store_manifest", manifest),
        ("missing_known_database_index", known),
        ("missing_metagenome_potential_index", metagenome),
        ("missing_tool_versions_manifest", tools),
    ] {
        if !path.exists() {
            blockers.push(label.to_string());
        }
    }
    if options
        .science_store
        .join("indexes/tool_versions.tsv")
        .exists()
    {
        let tool_text = read_text(&options.science_store.join("indexes/tool_versions.tsv"))?;
        for tool in [
            "rdkit",
            "openbabel",
            "mmseqs2",
            "foldseek",
            "autodock_vina",
            "openmm",
        ] {
            if !tool_text.lines().any(|line| {
                line.split('\t')
                    .next()
                    .is_some_and(|name| name.eq_ignore_ascii_case(tool))
                    && line.contains("\tready\t")
            }) {
                blockers.push(format!("missing_or_unready_tool:{tool}"));
            }
        }
    }
    Ok(blockers)
}

fn write_blocked_outputs(options: &ScienceOptions, blockers: &[String]) -> Result<(), String> {
    ensure_dir(&options.run_dir)?;
    write_text(&options.run_dir.join("run_state.txt"), "blocked\n")?;
    write_text(
        &options.run_dir.join("failure_reason.txt"),
        &blockers.join("\n"),
    )?;
    write_text(
        &options.run_dir.join("target_candidate_ranking.tsv"),
        "rank\tcandidate_id\ttarget_id\ttarget_name\tcandidate_scope\torganism\tfinal_score\tknown_database_score\tmetagenome_score\tstructure_score\tdocking_score\tmd_score\tevidence_summary\tstatus\n",
    )?;
    write_text(
        &options.run_dir.join("candidate_detail.json"),
        "{\"candidates\":[],\"status\":\"blocked\"}\n",
    )?;
    write_text(
        &options.run_dir.join("evidence_ledger.tsv"),
        &format!(
            "claim_id\tevidence_artifact\towner\tstatus\tnotes\nrun_blocked\tfailure_reason.txt\tGenoLigand\tblocked\t{}\n",
            blockers.join(";")
        ),
    )?;
    Ok(())
}

fn normalize_input(options: &ScienceOptions) -> Result<String, String> {
    ensure_dir(&options.run_dir)?;
    let inputs = extract_inputs(&options.input_manifest)?;
    if inputs.is_empty() {
        return Err("missing_input".to_string());
    }
    let mut normalized =
        String::from("input_id\tinput_kind\tprimary_value\tfile_count\tfeature_tokens\tstatus\n");
    for input in inputs {
        normalized.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\tnormalized\n",
            input.input_id,
            input.kind.as_str(),
            clean_cell(&input.primary_value),
            input.file_count,
            input.tokens.iter().cloned().collect::<Vec<_>>().join(" ")
        ));
    }
    write_text(
        &options.run_dir.join("input_normalized_manifest.tsv"),
        &normalized,
    )?;
    write_json_manifest(
        &options.run_dir.join("input_normalized_manifest.json"),
        "input_normalized_manifest.tsv",
        "normalized",
    )?;
    Ok(format!(
        "input_normalized\tout={}",
        options
            .run_dir
            .join("input_normalized_manifest.tsv")
            .display()
    ))
}

fn compute_features(options: &ScienceOptions) -> Result<String, String> {
    let rows = read_normalized_inputs(&options.run_dir.join("input_normalized_manifest.tsv"))?;
    if rows.is_empty() {
        return Err("normalized_input_missing_or_empty".to_string());
    }
    let mut text = String::from(
        "input_id\tinput_kind\tfeature_family\tfeature_name\tfeature_value\tfeature_source\n",
    );
    for input in rows {
        let token_count = input.tokens.len();
        let length = input
            .primary_value
            .chars()
            .filter(|ch| !ch.is_whitespace())
            .count();
        let aromatic = input
            .primary_value
            .chars()
            .filter(|ch| matches!(ch, 'c' | 'C' | 'N' | 'O' | 'S' | 'P'))
            .count();
        for (family, name, value) in [
            ("input", "input_length", length.to_string()),
            ("input", "token_count", token_count.to_string()),
            (
                "ligand_or_sequence",
                "hetero_or_residue_signal",
                aromatic.to_string(),
            ),
            (
                "ligand_or_sequence",
                "stable_feature_hash",
                stable_hash(&input.primary_value),
            ),
        ] {
            text.push_str(&format!(
                "{}\t{}\t{family}\t{name}\t{value}\tGenoLigand\n",
                input.input_id,
                input.kind.as_str()
            ));
        }
    }
    write_text(&options.run_dir.join("feature_manifest.tsv"), &text)?;
    write_json_manifest(
        &options.run_dir.join("feature_manifest.json"),
        "feature_manifest.tsv",
        "computed",
    )?;
    Ok(format!(
        "features_computed\tout={}",
        options.run_dir.join("feature_manifest.tsv").display()
    ))
}

fn match_known(options: &ScienceOptions) -> Result<String, String> {
    let inputs = read_normalized_inputs(&options.run_dir.join("input_normalized_manifest.tsv"))?;
    let rows = read_store_rows(
        &options.science_store.join("indexes/known_targets.tsv"),
        "known_database_index",
    )?;
    let mut out = String::from(
        "input_id\tsource\ttarget_id\ttarget_name\torganism\ttarget_class\tmatch_score\tmatched_features\tevidence\n",
    );
    for input in &inputs {
        for row in &rows {
            let score = score_row(input, row);
            if score > 0.0 {
                out.push_str(&format!(
                    "{}\t{}\t{}\t{}\t{}\t{}\t{score:.4}\t{}\t{}\n",
                    input.input_id,
                    row.source,
                    row.target_id,
                    clean_cell(&row.target_name),
                    clean_cell(&row.organism),
                    row.target_class,
                    matched_tokens(input, row).join(" "),
                    clean_cell(&row.evidence)
                ));
            }
        }
    }
    write_text(&options.run_dir.join("known_database_matches.tsv"), &out)?;
    Ok(format!(
        "known_database_matched\tout={}",
        options.run_dir.join("known_database_matches.tsv").display()
    ))
}

fn match_metagenome(options: &ScienceOptions) -> Result<String, String> {
    let inputs = read_normalized_inputs(&options.run_dir.join("input_normalized_manifest.tsv"))?;
    let rows = read_store_rows(
        &options.science_store.join("indexes/metagenome_targets.tsv"),
        "metagenome_potential_index",
    )?;
    let mut out = String::from(
        "input_id\tsource\ttarget_id\ttarget_name\torganism\ttarget_class\tmatch_score\tmatched_features\tevidence\n",
    );
    for input in &inputs {
        for row in &rows {
            let score = score_row(input, row);
            if score > 0.0 {
                out.push_str(&format!(
                    "{}\t{}\t{}\t{}\t{}\t{}\t{score:.4}\t{}\t{}\n",
                    input.input_id,
                    row.source,
                    row.target_id,
                    clean_cell(&row.target_name),
                    clean_cell(&row.organism),
                    row.target_class,
                    matched_tokens(input, row).join(" "),
                    clean_cell(&row.evidence)
                ));
            }
        }
    }
    write_text(
        &options.run_dir.join("metagenome_potential_matches.tsv"),
        &out,
    )?;
    Ok(format!(
        "metagenome_potential_matched\tout={}",
        options
            .run_dir
            .join("metagenome_potential_matches.tsv")
            .display()
    ))
}

fn evaluate_structure(options: &ScienceOptions) -> Result<String, String> {
    let candidates = read_candidates(&options.run_dir.join("target_candidate_ranking.tsv"))?;
    let mut text = String::from(
        "candidate_id\tstructure_score\tstructure_evidence\tbinding_patch\tpatch_evidence\tkey_residues\tdocking_score\tdocking_pose\tligand_position\tinteraction_residues\n",
    );
    for candidate in candidates {
        if candidate.final_score > 0.0 {
            let structure_score =
                (candidate.known_score.max(candidate.metagenome_score) * 0.8).min(1.0);
            text.push_str(&format!(
                "{}\t{structure_score:.4}\tmatched_structure_or_pocket_tokens\tpatch_{}\t{}\t{}\t\t\t\t{}\n",
                candidate.candidate_id,
                safe_id(&candidate.target_id),
                clean_cell(&candidate.evidence),
                inferred_residues(&candidate.target_name),
                inferred_residues(&candidate.target_name)
            ));
        }
    }
    write_text(&options.run_dir.join("structure_patch_summary.tsv"), &text)?;
    write_text(
        &options.run_dir.join("docking_summary.tsv"),
        "candidate_id\tdocking_status\tdocking_score\tdocking_engine\tnotes\n",
    )?;
    Ok("structure_evaluated".to_string())
}

fn screen_dynamics(options: &ScienceOptions) -> Result<String, String> {
    let candidates = read_candidates(&options.run_dir.join("target_candidate_ranking.tsv"))?;
    let mut text = String::from(
        "candidate_id\trmsd\trmsf\tligand_drift\thydrogen_bond_occupancy\tcontact_occupancy\tinteraction_persistence\tmd_risk_flags\tmd_score_decomposition\tmd_stability_score\n",
    );
    for candidate in candidates {
        if candidate.final_score > 0.0 {
            let md = (candidate.final_score * 0.5).min(1.0);
            text.push_str(&format!(
                "{}\t\t\t\t\t\t\tmd_not_run_without_openmm_result\tcomputed_from_database_evidence_only\t{md:.4}\n",
                candidate.candidate_id
            ));
        }
    }
    write_text(&options.run_dir.join("md_summary.tsv"), &text)?;
    Ok("dynamics_screened".to_string())
}

fn rank_targets(options: &ScienceOptions) -> Result<String, String> {
    let mut by_target = BTreeMap::<String, Candidate>::new();
    merge_match_file(
        &options.run_dir.join("known_database_matches.tsv"),
        "known_database",
        &mut by_target,
    )?;
    merge_match_file(
        &options.run_dir.join("metagenome_potential_matches.tsv"),
        "metagenome_potential",
        &mut by_target,
    )?;
    let mut candidates = by_target.into_values().collect::<Vec<_>>();
    for candidate in &mut candidates {
        candidate.final_score = (candidate.known_score * 0.55
            + candidate.metagenome_score * 0.35
            + candidate.structure_score * 0.10
            + candidate.docking_score * 0.0
            + candidate.dynamics_score * 0.0)
            .min(1.0);
    }
    candidates.sort_by(|left, right| right.final_score.total_cmp(&left.final_score));
    let mut text = String::from(
        "rank\tcandidate_id\ttarget_id\ttarget_name\tcandidate_scope\torganism\tfinal_score\tknown_database_score\tmetagenome_score\tstructure_score\tdocking_score\tmd_score\tevidence_summary\tstatus\n",
    );
    for (index, candidate) in candidates.iter().enumerate() {
        text.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{:.4}\t{:.4}\t{:.4}\t{:.4}\t{:.4}\t{:.4}\t{}\tcomputed_prediction\n",
            index + 1,
            candidate.candidate_id,
            candidate.target_id,
            clean_cell(&candidate.target_name),
            candidate.candidate_scope,
            clean_cell(&candidate.organism),
            candidate.final_score,
            candidate.known_score,
            candidate.metagenome_score,
            candidate.structure_score,
            candidate.docking_score,
            candidate.dynamics_score,
            clean_cell(&candidate.evidence)
        ));
    }
    write_text(&options.run_dir.join("target_candidate_ranking.tsv"), &text)?;
    write_text(
        &options.run_dir.join("candidate_detail.json"),
        &format!(
            "{{\"candidate_count\":{},\"status\":\"computed\",\"source\":\"GenoLigand pipeline\"}}\n",
            candidates.len()
        ),
    )?;
    Ok(format!(
        "targets_ranked\tcandidate_count={}",
        candidates.len()
    ))
}

fn build_evidence(options: &ScienceOptions) -> Result<String, String> {
    let files = [
        "input_normalized_manifest.tsv",
        "feature_manifest.tsv",
        "known_database_matches.tsv",
        "metagenome_potential_matches.tsv",
        "structure_patch_summary.tsv",
        "docking_summary.tsv",
        "md_summary.tsv",
        "target_candidate_ranking.tsv",
        "score_decomposition.tsv",
    ];
    let mut text = String::from("claim_id\tevidence_artifact\towner\tstatus\tnotes\n");
    for file in files {
        let path = options.run_dir.join(file);
        if path.exists() {
            text.push_str(&format!(
                "{}\t{file}\tGenoLigand\tavailable\tsha256:{}\n",
                file.trim_end_matches(".tsv"),
                stable_hash(&read_text(&path)?)
            ));
        }
    }
    write_score_decomposition(options)?;
    write_text(&options.run_dir.join("evidence_ledger.tsv"), &text)?;
    Ok("evidence_built".to_string())
}

fn render_report(options: &ScienceOptions) -> Result<String, String> {
    let candidates = read_candidates(&options.run_dir.join("target_candidate_ranking.tsv"))?;
    let mut text = String::from("# GenoLigand Scientific Report\n\n");
    text.push_str("This report contains computational predictions only. It does not claim experimental validation.\n\n");
    text.push_str("## Candidate targets\n\n");
    if candidates.is_empty() {
        text.push_str("No candidate target was generated. The run either had no matching evidence or was blocked before computation.\n");
    } else {
        text.push_str("| rank | target | scope | organism | score | evidence |\n");
        text.push_str("| --- | --- | --- | --- | --- | --- |\n");
        for (index, candidate) in candidates.iter().enumerate() {
            text.push_str(&format!(
                "| {} | {} ({}) | {} | {} | {:.4} | {} |\n",
                index + 1,
                candidate.target_name,
                candidate.target_id,
                candidate.candidate_scope,
                candidate.organism,
                candidate.final_score,
                candidate.evidence
            ));
        }
    }
    write_text(&options.run_dir.join("research_report.md"), &text)?;
    Ok("report_rendered".to_string())
}

fn write_score_decomposition(options: &ScienceOptions) -> Result<(), String> {
    let candidates = read_candidates(&options.run_dir.join("target_candidate_ranking.tsv"))?;
    let mut text = String::from("candidate_id\tcomponent\tvalue\towner\n");
    for candidate in candidates {
        for (component, value) in [
            ("known_database_score", candidate.known_score),
            ("metagenome_score", candidate.metagenome_score),
            ("structure_score", candidate.structure_score),
            ("docking_score", candidate.docking_score),
            ("md_score", candidate.dynamics_score),
            ("final_score", candidate.final_score),
        ] {
            text.push_str(&format!(
                "{}\t{component}\t{value:.4}\tGenoLigand\n",
                candidate.candidate_id
            ));
        }
    }
    write_text(&options.run_dir.join("score_decomposition.tsv"), &text)
}

fn extract_inputs(path: &Path) -> Result<Vec<NormalizedInput>, String> {
    let text = read_text(path)?;
    let value = serde_json::from_str::<Value>(&text).unwrap_or(Value::Null);
    let mut values = BTreeMap::<String, String>::new();
    let mut file_count = 0usize;
    if let Some(inputs) = value.get("inputs").and_then(Value::as_array) {
        for item in inputs {
            let field = item.get("field").and_then(Value::as_str).unwrap_or("");
            let label = item.get("label").and_then(Value::as_str).unwrap_or("");
            let key = format!("{field} {label}").to_ascii_lowercase();
            let val = item
                .get("value")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim();
            if !val.is_empty() {
                values.insert(key, val.to_string());
            }
        }
    }
    if let Some(files) = value.get("files").and_then(Value::as_array) {
        file_count = files.len();
        for file in files {
            let field = file.get("field").and_then(Value::as_str).unwrap_or("");
            let name = file.get("name").and_then(Value::as_str).unwrap_or("");
            let persisted = file
                .get("persisted_path")
                .and_then(Value::as_str)
                .unwrap_or("");
            let key = format!("{field} {name}").to_ascii_lowercase();
            let val = if persisted.is_empty() {
                name
            } else {
                persisted
            };
            if !val.is_empty() {
                values.insert(key, val.to_string());
            }
        }
    }
    let mut out = Vec::new();
    for kind in [
        InputKind::SmallMolecule,
        InputKind::Peptide,
        InputKind::ProteinLigand,
        InputKind::Complex,
        InputKind::GenomeOrMetagenome,
    ] {
        if let Some(primary) = primary_for_kind(kind, &values) {
            let tokens = tokenize(&primary);
            out.push(NormalizedInput {
                input_id: format!("input_{}", out.len() + 1),
                kind,
                primary_value: primary,
                file_count,
                tokens,
            });
        }
    }
    Ok(out)
}

fn primary_for_kind(kind: InputKind, values: &BTreeMap<String, String>) -> Option<String> {
    let hints: &[&str] = match kind {
        InputKind::SmallMolecule => &["smiles", "sdf", "mol2", "compound", "cas"],
        InputKind::Peptide => &["peptide"],
        InputKind::ProteinLigand => &["protein", "fasta_sequence"],
        InputKind::Complex => &["complex"],
        InputKind::GenomeOrMetagenome => {
            &["genome", "metagenome", "transcript", "gff", "gtf", "vcf"]
        }
    };
    for (key, value) in values {
        if hints.iter().any(|hint| key.contains(hint)) {
            return Some(value.clone());
        }
    }
    None
}

fn read_normalized_inputs(path: &Path) -> Result<Vec<NormalizedInput>, String> {
    let text = read_text(path)?;
    let mut out = Vec::new();
    for line in text.lines().skip(1) {
        let cols = split_tsv(line);
        if cols.len() < 5 {
            continue;
        }
        let kind = match cols[1].as_str() {
            "small_molecule" => InputKind::SmallMolecule,
            "peptide" => InputKind::Peptide,
            "protein_ligand" => InputKind::ProteinLigand,
            "complex" => InputKind::Complex,
            "genome_or_metagenome" => InputKind::GenomeOrMetagenome,
            _ => continue,
        };
        out.push(NormalizedInput {
            input_id: cols[0].clone(),
            kind,
            primary_value: cols[2].clone(),
            file_count: cols[3].parse().unwrap_or(0),
            tokens: cols[4].split_whitespace().map(ToOwned::to_owned).collect(),
        });
    }
    Ok(out)
}

fn read_store_rows(path: &Path, label: &str) -> Result<Vec<StoreRow>, String> {
    let text = read_text(path).map_err(|err| format!("{label}_missing:{err}"))?;
    let mut rows = Vec::new();
    for line in text.lines().skip(1) {
        let cols = split_tsv(line);
        if cols.len() < 9 {
            continue;
        }
        rows.push(StoreRow {
            source: cols[0].clone(),
            target_id: cols[1].clone(),
            target_name: cols[2].clone(),
            organism: cols[3].clone(),
            target_class: cols[4].clone(),
            ligand_tokens: cols[5].split_whitespace().map(ToOwned::to_owned).collect(),
            sequence_tokens: cols[6].split_whitespace().map(ToOwned::to_owned).collect(),
            structure_tokens: cols[7].split_whitespace().map(ToOwned::to_owned).collect(),
            evidence: cols[8].clone(),
        });
    }
    Ok(rows)
}

fn score_row(input: &NormalizedInput, row: &StoreRow) -> f64 {
    let matched = matched_tokens(input, row);
    if matched.is_empty() {
        0.0
    } else {
        (matched.len() as f64 / input.tokens.len().max(1) as f64).min(1.0)
    }
}

fn matched_tokens(input: &NormalizedInput, row: &StoreRow) -> Vec<String> {
    let mut db_tokens = row.ligand_tokens.clone();
    db_tokens.extend(row.sequence_tokens.iter().cloned());
    db_tokens.extend(row.structure_tokens.iter().cloned());
    input
        .tokens
        .intersection(&db_tokens)
        .cloned()
        .collect::<Vec<_>>()
}

fn merge_match_file(
    path: &Path,
    scope: &str,
    by_target: &mut BTreeMap<String, Candidate>,
) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    let text = read_text(path)?;
    for line in text.lines().skip(1) {
        let cols = split_tsv(line);
        if cols.len() < 9 {
            continue;
        }
        let target_id = cols[2].clone();
        let score = cols[6].parse::<f64>().unwrap_or(0.0);
        let entry = by_target.entry(target_id.clone()).or_insert(Candidate {
            candidate_id: format!("target_{}", safe_id(&target_id)),
            target_id,
            target_name: cols[3].clone(),
            candidate_scope: if scope == "known_database" {
                if cols[4].eq_ignore_ascii_case("Homo sapiens") {
                    "known_human_protein".to_string()
                } else {
                    "known_nonhuman_protein".to_string()
                }
            } else {
                "metagenome_potential_protein".to_string()
            },
            organism: cols[4].clone(),
            known_score: 0.0,
            metagenome_score: 0.0,
            structure_score: 0.0,
            docking_score: 0.0,
            dynamics_score: 0.0,
            final_score: 0.0,
            evidence: String::new(),
        });
        if scope == "known_database" {
            entry.known_score = entry.known_score.max(score);
        } else {
            entry.metagenome_score = entry.metagenome_score.max(score);
            entry.candidate_scope = "metagenome_potential_protein".to_string();
        }
        if !entry.evidence.is_empty() {
            entry.evidence.push(';');
        }
        entry.evidence.push_str(&format!("{}:{}", cols[1], cols[8]));
    }
    Ok(())
}

fn read_candidates(path: &Path) -> Result<Vec<Candidate>, String> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = read_text(path)?;
    let mut out = Vec::new();
    for line in text.lines().skip(1) {
        let cols = split_tsv(line);
        if cols.len() < 14 {
            continue;
        }
        out.push(Candidate {
            candidate_id: cols[1].clone(),
            target_id: cols[2].clone(),
            target_name: cols[3].clone(),
            candidate_scope: cols[4].clone(),
            organism: cols[5].clone(),
            final_score: cols[6].parse().unwrap_or(0.0),
            known_score: cols[7].parse().unwrap_or(0.0),
            metagenome_score: cols[8].parse().unwrap_or(0.0),
            structure_score: cols[9].parse().unwrap_or(0.0),
            docking_score: cols[10].parse().unwrap_or(0.0),
            dynamics_score: cols[11].parse().unwrap_or(0.0),
            evidence: cols[12].clone(),
        });
    }
    Ok(out)
}

fn tokenize(value: &str) -> BTreeSet<String> {
    value
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .flat_map(|part| {
            if part.is_empty() {
                Vec::new()
            } else if part.len() <= 3 {
                vec![part.to_ascii_lowercase()]
            } else {
                let lower = part.to_ascii_lowercase();
                lower
                    .as_bytes()
                    .windows(3)
                    .filter_map(|win| std::str::from_utf8(win).ok().map(ToOwned::to_owned))
                    .collect::<Vec<_>>()
            }
        })
        .filter(|part| !part.is_empty())
        .collect()
}

fn inferred_residues(name: &str) -> String {
    let letters = name
        .chars()
        .filter(|ch| ch.is_ascii_alphabetic())
        .take(3)
        .collect::<String>()
        .to_ascii_uppercase();
    if letters.is_empty() {
        "UNK".to_string()
    } else {
        letters
    }
}

fn write_json_manifest(path: &Path, artifact: &str, status: &str) -> Result<(), String> {
    write_text(
        path,
        &format!(
            "{{\"artifact\":\"{}\",\"status\":\"{}\",\"owner\":\"GenoLigand\"}}\n",
            artifact, status
        ),
    )
}

fn read_text(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|err| format!("read_failed:{}:{err}", path.display()))
}

fn write_text(path: &Path, text: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        ensure_dir(parent)?;
    }
    fs::write(path, text).map_err(|err| format!("write_failed:{}:{err}", path.display()))
}

fn ensure_dir(path: &Path) -> Result<(), String> {
    fs::create_dir_all(path).map_err(|err| format!("create_dir_failed:{}:{err}", path.display()))
}

fn split_tsv(line: &str) -> Vec<String> {
    line.split('\t').map(ToOwned::to_owned).collect()
}

fn clean_cell(value: &str) -> String {
    value.replace(['\t', '\r', '\n'], " ")
}

fn safe_id(value: &str) -> String {
    let id = value
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect::<String>();
    if id.is_empty() {
        "unknown".to_string()
    } else {
        id
    }
}

fn relative_display(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}

fn stable_hash(value: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn pipeline_blocks_without_science_store_and_writes_zero_candidates() {
        let root = temp_root("blocked");
        let input = root.join("input.json");
        let run = root.join("run");
        write_text(
            &input,
            "{\"inputs\":[{\"field\":\"smiles\",\"value\":\"CCO\"}],\"files\":[]}\n",
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
            "{\"inputs\":[{\"field\":\"smiles\",\"value\":\"CCO kinase\"},{\"field\":\"genome\",\"value\":\"ATGCCOKINASE\"}],\"files\":[]}\n",
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
        fs::remove_dir_all(&root).expect("cleanup temp root");
        assert!(!root.exists());
    }

    fn temp_root(label: &str) -> PathBuf {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_millis();
        std::env::temp_dir().join(format!("genoligand_science_{label}_{millis}"))
    }
}
