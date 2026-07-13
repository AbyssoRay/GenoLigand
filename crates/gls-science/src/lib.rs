use flate2::read::GzDecoder;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use zip::ZipArchive;

mod input_conversion;

use input_conversion::{
    INPUT_CONVERSION_HEADER, convert_inputs_from_manifest, extract_inputs,
    normalized_inputs_from_conversions, write_input_conversion_manifest,
};

const WORKER_SCHEMA_VERSION: &str = "genoligand_worker_current_v2";
const WORKER_PIPELINE_MODE: &str = "genoligand_scientific_pipeline";
const MAX_SEQUENCE_TOKEN_CHARS: usize = 96;
const MAX_STRUCTURE_METADATA_CHARS: usize = 16_384;
const LARGE_METAGENOME_FASTA_TSV_MAX_BYTES: u64 = 1_000_000_000;
const KNOWN_SCORE_WEIGHT: f64 = 0.50;
const METAGENOME_SCORE_WEIGHT: f64 = 0.25;
const STRUCTURE_SCORE_WEIGHT: f64 = 0.10;
const DOCKING_SCORE_WEIGHT: f64 = 0.05;
const MD_SCORE_WEIGHT: f64 = 0.10;
static PYTHON_SCRIPT_SEQUENCE: AtomicU64 = AtomicU64::new(0);
const AUTHENTICITY_FORBIDDEN_MARKERS: &[(&str, &str)] = &[
    ("unit_test_fixture", "unit test fixture row"),
    ("unit-test-fixture", "unit test fixture row"),
    ("unit_test_store_fixture", "unit test fixture row"),
    ("test_only", "test-only source row"),
    ("test-only", "test-only source row"),
    ("fixture", "fixture source row"),
    ("smoke", "smoke output row"),
    ("sample_output", "sample output row"),
    ("sample_data", "sample data row"),
    ("synthetic_placeholder", "synthetic placeholder row"),
    ("placeholder_row", "synthetic placeholder row"),
    ("static_fallback_candidate", "static fallback row"),
    ("static_fallback", "static fallback row"),
    ("static fallback", "static fallback row"),
    ("demodb", "demo source row"),
    ("demo_data", "demo source row"),
    ("demo/sample", "demo source row"),
];

#[derive(Clone, Debug)]
pub struct ScienceOptions {
    pub workspace_root: PathBuf,
    pub science_store: PathBuf,
    pub input_manifest: PathBuf,
    pub run_dir: PathBuf,
}

#[derive(Clone, Debug)]
pub struct InputInspectionRow {
    pub input_id: String,
    pub record_id: String,
    pub original_field: String,
    pub canonical_kind: String,
    pub conversion_method: String,
    pub usable: bool,
    pub error: Option<String>,
}

#[derive(Clone, Debug)]
pub struct InputInspection {
    pub usable: bool,
    pub summary: String,
    pub errors: Vec<String>,
    pub inputs: Vec<InputInspectionRow>,
}

pub fn inspect_input_manifest(options: &ScienceOptions) -> Result<InputInspection, String> {
    let conversions = convert_inputs_from_manifest(options)?;
    let errors = conversions
        .iter()
        .filter(|input| input.conversion_status == "failed")
        .map(|input| {
            format!(
                "{}:{}:{}",
                input.input_id,
                clean_cell(&input.record_id),
                clean_cell(&input.reason)
            )
        })
        .collect::<Vec<_>>();
    let inputs = conversions
        .iter()
        .map(|input| InputInspectionRow {
            input_id: input.input_id.clone(),
            record_id: input.record_id.clone(),
            original_field: input.original_field.clone(),
            canonical_kind: input
                .canonical_kind
                .map(InputKind::as_str)
                .unwrap_or("unknown")
                .to_string(),
            conversion_method: input.conversion_method.to_string(),
            usable: input.conversion_status != "failed",
            error: (input.conversion_status == "failed").then(|| input.reason.clone()),
        })
        .collect::<Vec<_>>();
    let usable = !inputs.is_empty() && errors.is_empty();
    let summary = if inputs.is_empty() {
        "no non-project input records".to_string()
    } else if errors.is_empty() {
        format!("{} input records ready", inputs.len())
    } else {
        format!("{} of {} input records failed", errors.len(), inputs.len())
    };
    Ok(InputInspection {
        usable,
        summary,
        errors,
        inputs,
    })
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
    record_id: String,
    kind: InputKind,
    primary_value: String,
    file_count: usize,
    tokens: BTreeSet<String>,
    source_kind: &'static str,
    source_path: String,
    content_sha256: String,
    structure_atom_count: usize,
    structure_chain_count: usize,
    structure_residue_count: usize,
}

const NORMALIZED_INPUT_HEADER: &str = "input_id\tinput_kind\tprimary_value\tfile_count\tfeature_tokens\trecord_id\tsource_kind\tsource_path\tcontent_sha256\tstatus\tstructure_atom_count\tstructure_chain_count\tstructure_residue_count";
const FEATURE_MANIFEST_HEADER: &str = "input_id\tinput_kind\tfeature_family\tfeature_name\tfeature_value\tfeature_source\tfeature_token\trecord_id";

#[derive(Clone, Copy, Debug)]
struct WorkerTask {
    job_id: &'static str,
    worker: &'static str,
    module: &'static str,
    stage: &'static str,
}

const SCIENTIFIC_WORKER_TASKS: &[WorkerTask] = &[WorkerTask {
    job_id: "md_summary_ligand_dynamics",
    worker: "md_summary",
    module: "genoligand_workers.md_summary_worker",
    stage: "dynamics_summary_primitive",
}];

const SEQUENCE_ALIGNMENT_WORKER_TASK: WorkerTask = WorkerTask {
    job_id: "mmseqs_onsite_sequence_alignment",
    worker: "mmseqs_alignment",
    module: "genoligand_workers.alignment_worker",
    stage: "onsite_sequence_alignment",
};

const STRUCTURE_ALIGNMENT_WORKER_TASK: WorkerTask = WorkerTask {
    job_id: "foldseek_onsite_structure_alignment",
    worker: "foldseek_alignment",
    module: "genoligand_workers.alignment_worker",
    stage: "onsite_structure_alignment",
};

const DOCKING_WORKER_TASK: WorkerTask = WorkerTask {
    job_id: "autodock_vina_complex_docking",
    worker: "docking",
    module: "genoligand_workers.docking_worker",
    stage: "docking_primitive",
};

const REPORT_CANDIDATE_LIMIT: usize = 50;

#[derive(Clone, Debug)]
struct ComputedFeatureInput {
    input_id: String,
    record_id: String,
    input_kind: String,
    tokens: BTreeSet<String>,
}

#[derive(Clone, Debug)]
struct ConversionProvenance {
    canonical_kind: String,
    source_kind: String,
    source_path: String,
    content_sha256: String,
}

#[derive(Clone, Debug)]
struct FeatureRow {
    family: String,
    name: String,
    value: String,
    token: String,
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

#[derive(Clone, Debug)]
struct IndexMatch {
    input_id: String,
    record_id: String,
    source: String,
    target_id: String,
    target_name: String,
    organism: String,
    target_class: String,
    match_score: f64,
    matched_features: String,
    evidence: String,
}

#[derive(Clone, Debug)]
struct KnownStructureHint {
    input_id: String,
    target_id: String,
    target_name: String,
    organism: String,
    target_class: String,
    match_score: f64,
    evidence: String,
    pdb_ids: Vec<String>,
}

#[derive(Clone, Debug)]
struct SelectedComplexCandidate {
    candidate_id: String,
    input_id: String,
    target_id: String,
    target_name: String,
    organism: String,
    target_class: String,
    source: String,
    pdb_id: String,
    evidence: String,
}

#[derive(Clone, Copy, Debug)]
struct RequiredTool {
    id: &'static str,
    label: &'static str,
    license: &'static str,
    install_hint: &'static str,
}

#[derive(Clone, Copy, Debug)]
struct RequiredSource {
    id: &'static str,
    label: &'static str,
    category: &'static str,
    landing_url: &'static str,
    access_method: &'static str,
    license_note: &'static str,
    local_rel_path: &'static str,
    index_rel_path: &'static str,
    repair_action: &'static str,
}

const REQUIRED_TOOLS: &[RequiredTool] = &[
    RequiredTool {
        id: "java",
        label: "Java",
        license: "varies_by_distribution",
        install_hint: "install_java_runtime",
    },
    RequiredTool {
        id: "nextflow",
        label: "Nextflow",
        license: "Apache-2.0",
        install_hint: "install_nextflow",
    },
    RequiredTool {
        id: "rdkit",
        label: "RDKit",
        license: "BSD-3-Clause",
        install_hint: "install_python_rdkit",
    },
    RequiredTool {
        id: "openbabel",
        label: "Open Babel",
        license: "GPL-2.0",
        install_hint: "install_openbabel",
    },
    RequiredTool {
        id: "mmseqs2",
        label: "MMseqs2",
        license: "GPL-3.0",
        install_hint: "install_mmseqs2",
    },
    RequiredTool {
        id: "foldseek",
        label: "Foldseek",
        license: "GPL-3.0",
        install_hint: "install_foldseek",
    },
    RequiredTool {
        id: "autodock_vina",
        label: "AutoDock Vina",
        license: "Apache-2.0",
        install_hint: "install_autodock_vina",
    },
    RequiredTool {
        id: "openmm",
        label: "OpenMM",
        license: "MIT",
        install_hint: "install_python_openmm",
    },
    RequiredTool {
        id: "pdbfixer",
        label: "PDBFixer",
        license: "MIT",
        install_hint: "install_python_pdbfixer",
    },
    RequiredTool {
        id: "openff_toolkit",
        label: "OpenFF Toolkit",
        license: "MIT",
        install_hint: "install_python_openff_toolkit",
    },
    RequiredTool {
        id: "mdtraj",
        label: "MDTraj",
        license: "LGPL-2.1",
        install_hint: "install_python_mdtraj",
    },
    RequiredTool {
        id: "mdanalysis",
        label: "MDAnalysis",
        license: "GPL-2.0",
        install_hint: "install_python_mdanalysis",
    },
];

const REQUIRED_SOURCES: &[RequiredSource] = &[
    RequiredSource {
        id: "chembl",
        label: "ChEMBL",
        category: "known_activity",
        landing_url: "https://chembl.gitbook.io/chembl-interface-documentation/downloads",
        access_method: "download_sqlite_release",
        license_note: "ChEMBL release license and DOI must be recorded from the selected release.",
        local_rel_path: "downloads/chembl/chembl_sqlite.tar.gz",
        index_rel_path: "indexes/known_targets.tsv",
        repair_action: "download_and_index_chembl",
    },
    RequiredSource {
        id: "bindingdb",
        label: "BindingDB",
        category: "known_activity",
        landing_url: "https://www.bindingdb.org/rwd/bind/chemsearch/marvin/Download.jsp",
        access_method: "download_monthly_tsv_or_archived_release",
        license_note: "BindingDB curated data license and imported-source licenses must be recorded.",
        local_rel_path: "downloads/bindingdb/bindingdb_all.tsv.zip",
        index_rel_path: "indexes/known_targets.tsv",
        repair_action: "download_and_index_bindingdb",
    },
    RequiredSource {
        id: "uniprot_human_reviewed",
        label: "UniProt reviewed human proteome",
        category: "known_protein",
        landing_url: "https://www.uniprot.org/proteomes/UP000005640",
        access_method: "download_reviewed_human_fasta",
        license_note: "UniProt license and release date must be recorded.",
        local_rel_path: "downloads/uniprot/uniprot_human_reviewed.fasta.gz",
        index_rel_path: "indexes/known_targets.tsv",
        repair_action: "download_and_index_uniprot_human",
    },
    RequiredSource {
        id: "rcsb_ligand_bound",
        label: "RCSB ligand-bound structures",
        category: "structure",
        landing_url: "https://www.rcsb.org/docs/programmatic-access/file-download-services",
        access_method: "download_selected_mmcif_entries",
        license_note: "RCSB PDB data policy and file provenance must be recorded per downloaded entry.",
        local_rel_path: "downloads/rcsb/mmcif",
        index_rel_path: "indexes/structure_targets.tsv",
        repair_action: "download_and_index_rcsb_ligand_bound",
    },
    RequiredSource {
        id: "biolip2",
        label: "BioLiP2",
        category: "structure_interaction",
        landing_url: "https://zhanggroup.org/BioLiP/",
        access_method: "download_biolip2_annotations_and_structures",
        license_note: "BioLiP2 source, publication, and usage notes must be recorded.",
        local_rel_path: "downloads/biolip2",
        index_rel_path: "indexes/structure_targets.tsv",
        repair_action: "download_and_index_biolip2",
    },
    RequiredSource {
        id: "plinder",
        label: "PLINDER",
        category: "structure_interaction",
        landing_url: "https://github.com/plinder-org/plinder",
        access_method: "download_plinder_release",
        license_note: "PLINDER data and package license records must be recorded.",
        local_rel_path: "downloads/plinder",
        index_rel_path: "indexes/structure_targets.tsv",
        repair_action: "download_and_index_plinder",
    },
    RequiredSource {
        id: "binding_moad",
        label: "Binding MOAD",
        category: "structure_interaction",
        landing_url: "https://github.com/Mugsie1969/BindingMOAD/",
        access_method: "download_sunset_archive_with_provenance",
        license_note: "Binding MOAD sunset archive provenance and citation must be recorded.",
        local_rel_path: "downloads/binding_moad/bindingmoad.csv",
        index_rel_path: "indexes/structure_targets.tsv",
        repair_action: "download_and_index_binding_moad",
    },
    RequiredSource {
        id: "metagenome_potential",
        label: "Metagenome potential sequence index",
        category: "metagenome",
        landing_url: "local_or_configured_public_metagenome_sources",
        access_method: "download_or_mount_metagenome_sequence_sources",
        license_note: "Each metagenome source license, release, and hash must be recorded.",
        local_rel_path: "downloads/metagenome",
        index_rel_path: "indexes/metagenome_targets.tsv",
        repair_action: "download_and_index_metagenome_sources",
    },
];

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

#[cfg(test)]
fn create_temporary_test_store(root: &Path) -> Result<PathBuf, String> {
    let store = root.join("science_store");
    ensure_dir(&store.join("indexes"))?;
    write_text(
        &store.join("science_store_manifest.tsv"),
        "item\tstatus\tpath\nwarehouse\tready\tindexes/known_targets.tsv\nmetagenome_index\tready\tindexes/metagenome_targets.tsv\nstructure_index\tready\tindexes/structure_targets.tsv\ntool_versions\tready\tindexes/tool_versions.tsv\n",
    )?;
    write_text(
        &store.join("indexes/tool_versions.tsv"),
        &unit_test_tool_versions_text(),
    )?;
    write_text(
        &store.join("indexes/known_targets.tsv"),
        "source\ttarget_id\ttarget_name\torganism\ttarget_class\tligand_tokens\tsequence_tokens\tstructure_tokens\tevidence\nChEMBL\tP24941\tCyclin-dependent kinase 2\tHomo sapiens\tknown_human_protein\tcco kin ina nas ase\tatg tgc gcc cco cok oki kin ina nas ase\tATP pocket kinase\tmeasured_activity_record\n",
    )?;
    write_text(
        &store.join("indexes/metagenome_targets.tsv"),
        "source\ttarget_id\ttarget_name\torganism\ttarget_class\tligand_tokens\tsequence_tokens\tstructure_tokens\tevidence\n",
    )?;
    write_text(
        &store.join("indexes/structure_targets.tsv"),
        "source\ttarget_id\ttarget_name\torganism\ttarget_class\tligand_tokens\tsequence_tokens\tstructure_tokens\tevidence\nRCSB\tP24941\tCyclin-dependent kinase 2 structure\tHomo sapiens\tstructure_interaction_record\tatp\tkin ase p24 249\tpocket kinase\tstructure_interaction_record\n",
    )?;
    write_text(
        &store.join("install_status.json"),
        &format!(
            "{}\n",
            json!({
                "status": "ready",
                "test_context": "unit_only",
                "missing_components": []
            })
        ),
    )?;
    let mut sources = String::from(
        "source_id\tlabel\tcategory\tstatus\tlanding_url\taccess_method\tlicense_note\tlocal_path\tindex_target\trepair_action\n",
    );
    for source in REQUIRED_SOURCES {
        sources.push_str(&format!(
            "{}\t{}\t{}\tready\t{}\t{}\t{}\t{}\t{}\t{}\n",
            source.id,
            source.label,
            source.category,
            source.landing_url,
            source.access_method,
            clean_cell(source.license_note),
            source.local_rel_path,
            source.index_rel_path,
            source.repair_action
        ));
    }
    write_text(&store.join("source_requirements.tsv"), &sources)?;
    Ok(store)
}

#[cfg(test)]
fn unit_test_tool_versions_text() -> String {
    let mut text = String::from("tool\tstatus\tversion\tlicense\tlabel\tdetail\n");
    for tool in REQUIRED_TOOLS {
        text.push_str(&format!(
            "{}\tready\tunit-test-fixture\t{}\t{}\tunit_test_store_fixture\n",
            tool.id, tool.license, tool.label
        ));
    }
    text
}

fn prepare_science_store(options: &StoreOptions) -> Result<String, String> {
    ensure_dir(&options.science_store)?;
    ensure_dir(&options.science_store.join("indexes"))?;
    let manifest = options.science_store.join("science_store_manifest.tsv");
    let known = options.science_store.join("indexes/known_targets.tsv");
    let metagenome = options.science_store.join("indexes/metagenome_targets.tsv");
    let structure = options.science_store.join("indexes/structure_targets.tsv");
    let tools = options.science_store.join("indexes/tool_versions.tsv");
    let repair = options.science_store.join("repair_actions.tsv");
    let status = options.science_store.join("install_status.json");
    let source_requirements = options.science_store.join("source_requirements.tsv");
    let download_plan = options.science_store.join("download_plan.tsv");
    let index_plan = options.science_store.join("index_plan.tsv");
    let index_build = options.science_store.join("index_build_manifest.tsv");
    write_tool_versions(&tools)?;
    let build_summary = build_store_indexes(options, &index_build)?;
    write_source_requirements(options, &source_requirements)?;
    write_download_plan(options, &download_plan)?;
    write_index_plan(options, &index_plan)?;
    let rows = [
        (
            "known_database_index",
            known.exists(),
            &known,
            "build_known_database_index",
        ),
        (
            "metagenome_potential_index",
            metagenome.exists(),
            &metagenome,
            "build_metagenome_potential_index",
        ),
        (
            "structure_interaction_index",
            structure.exists(),
            &structure,
            "build_structure_interaction_index",
        ),
        (
            "tool_versions",
            tools_ready(&tools),
            &tools,
            "install_or_repair_scientific_tools",
        ),
    ];
    let mut text = String::from("item\tstatus\tpath\n");
    let mut repair_text = String::from("component\tstatus\taction\tdetail\n");
    let mut missing = Vec::new();
    let mut ready = 0usize;
    for (item, is_ready, path, action) in rows {
        if is_ready {
            ready += 1;
        } else {
            missing.push(item.to_string());
        }
        text.push_str(&format!(
            "{item}\t{}\t{}\n",
            if is_ready { "ready" } else { "missing" },
            relative_display(&options.science_store, path)
        ));
        repair_text.push_str(&format!(
            "{item}\t{}\t{action}\t{}\n",
            if is_ready { "ready" } else { "missing" },
            relative_display(&options.science_store, path)
        ));
    }
    for tool in REQUIRED_TOOLS {
        let probe = probe_tool(tool);
        if !probe.ready {
            missing.push(format!("tool:{}", tool.id));
        }
        repair_text.push_str(&format!(
            "tool:{}\t{}\t{}\t{}\n",
            tool.id,
            if probe.ready { "ready" } else { "missing" },
            tool.install_hint,
            clean_cell(&probe.detail)
        ));
    }
    for source in REQUIRED_SOURCES {
        let index_path = options.science_store.join(source.index_rel_path);
        let source_ready = source_local_available(options, source) && index_path.exists();
        if !source_ready {
            missing.push(format!("source:{}", source.id));
        }
        repair_text.push_str(&format!(
            "source:{}\t{}\t{}\t{}\n",
            source.id,
            if source_ready { "ready" } else { "missing" },
            source.repair_action,
            clean_cell(source.landing_url)
        ));
    }
    write_text(&manifest, &text)?;
    write_text(&repair, &repair_text)?;
    write_text(
        &status,
        &format!(
            "{}\n",
            json!({
                "status": if missing.is_empty() {"ready"} else {"needs_repair"},
                "ready_items": ready,
                "required_items": 4,
                "missing_components": missing,
                "science_store": options.science_store.display().to_string(),
                "workspace_root": options.workspace_root.display().to_string(),
                "repair_actions": "repair_actions.tsv",
                "tool_versions": "indexes/tool_versions.tsv",
                "source_requirements": "source_requirements.tsv",
                "download_plan": "download_plan.tsv",
                "index_plan": "index_plan.tsv",
                "index_build_manifest": "index_build_manifest.tsv",
                "index_rows_built": {
                    "known": build_summary.known_rows,
                    "metagenome": build_summary.metagenome_rows,
                    "structure": build_summary.structure_rows
                }
            })
        ),
    )?;
    Ok(format!(
        "science_store_prepared\tready_items={ready}\trequired_items=4\tstatus={}\tmissing_components={}\tstore={}",
        if missing.is_empty() {
            "ready"
        } else {
            "needs_repair"
        },
        missing.len(),
        options.science_store.display()
    ))
}

#[derive(Clone, Copy, Debug, Default)]
struct IndexBuildSummary {
    known_rows: usize,
    metagenome_rows: usize,
    structure_rows: usize,
}

fn build_store_indexes(
    options: &StoreOptions,
    manifest_path: &Path,
) -> Result<IndexBuildSummary, String> {
    let known_sources = [
        "imports/known_targets.tsv",
        "imports/known_targets.tsv.gz",
        "imports/known_targets.zip",
        "downloads/known_targets.tsv",
        "downloads/known_targets.tsv.gz",
        "downloads/known_targets.zip",
        "downloads/chembl/known_targets.tsv",
        "downloads/chembl/known_targets.tsv.gz",
        "downloads/chembl/known_targets.zip",
        "downloads/bindingdb/known_targets.tsv",
        "downloads/bindingdb/known_targets.tsv.gz",
        "downloads/bindingdb/known_targets.zip",
        "downloads/bindingdb/bindingdb_all.tsv",
        "downloads/bindingdb/bindingdb_all.tsv.gz",
        "downloads/bindingdb/bindingdb_all.tsv.zip",
        "downloads/bindingdb/bindingdb_all.txt",
        "downloads/bindingdb/bindingdb_all.txt.gz",
        "downloads/bindingdb/bindingdb_all.txt.zip",
        "downloads/uniprot/known_targets.tsv",
        "downloads/uniprot/known_targets.tsv.gz",
        "downloads/uniprot/known_targets.zip",
    ];
    let metagenome_sources = [
        "imports/metagenome_targets.tsv",
        "imports/metagenome_targets.tsv.gz",
        "imports/metagenome_targets.zip",
        "downloads/metagenome_targets.tsv",
        "downloads/metagenome_targets.tsv.gz",
        "downloads/metagenome_targets.zip",
        "downloads/metagenome/metagenome_targets.tsv",
        "downloads/metagenome/metagenome_targets.tsv.gz",
        "downloads/metagenome/metagenome_targets.zip",
    ];
    let mut manifest = String::from("index\tinput_path\tstatus\trows\toutput_path\tnotes\n");
    let mut known_rows = Vec::new();
    for rel in known_sources {
        let path = options.science_store.join(rel);
        append_canonical_rows(
            "known_targets",
            &options.science_store,
            &path,
            &mut known_rows,
            &mut manifest,
        )?;
    }
    let chembl_dir = options.science_store.join("downloads/chembl");
    for path in discover_chembl_sqlite_sources(&chembl_dir)? {
        append_canonical_rows(
            "known_targets",
            &options.science_store,
            &path,
            &mut known_rows,
            &mut manifest,
        )?;
    }
    let uniprot_dir = options.science_store.join("downloads/uniprot");
    for path in discover_sequence_files(&uniprot_dir)? {
        let rows = parse_fasta_targets(
            &path,
            "UniProt",
            "Homo sapiens",
            "known_human_protein",
            "protein_sequence_record",
        )?;
        append_built_rows(
            "known_targets",
            &options.science_store,
            &path,
            rows,
            &mut known_rows,
            &mut manifest,
            "fasta_sequence_rows",
        );
    }

    let mut metagenome_rows = Vec::new();
    for rel in metagenome_sources {
        let path = options.science_store.join(rel);
        append_canonical_rows(
            "metagenome_targets",
            &options.science_store,
            &path,
            &mut metagenome_rows,
            &mut manifest,
        )?;
    }
    let metagenome_dir = options.science_store.join("downloads/metagenome");
    for path in discover_sequence_files(&metagenome_dir)? {
        if large_metagenome_fasta_requires_alignment_database(&path) {
            append_alignment_database_required(
                "metagenome_targets",
                &options.science_store,
                &path,
                &mut manifest,
            );
            continue;
        }
        let rows = parse_fasta_targets(
            &path,
            "metagenome",
            "metagenome_or_custom_source",
            "potential_metagenome_sequence",
            "sequence_record",
        )?;
        append_built_rows(
            "metagenome_targets",
            &options.science_store,
            &path,
            rows,
            &mut metagenome_rows,
            &mut manifest,
            "fasta_sequence_rows",
        );
    }

    known_rows = dedupe_store_rows(known_rows);
    metagenome_rows = dedupe_store_rows(metagenome_rows);
    write_store_rows(
        &options.science_store.join("indexes/known_targets.tsv"),
        &known_rows,
    )?;
    write_store_rows(
        &options.science_store.join("indexes/metagenome_targets.tsv"),
        &metagenome_rows,
    )?;
    let mut structure_rows = build_structure_index_rows(options, &mut manifest)?;
    structure_rows = dedupe_store_rows(structure_rows);
    write_store_rows(
        &options.science_store.join("indexes/structure_targets.tsv"),
        &structure_rows,
    )?;
    write_text(manifest_path, &manifest)?;
    Ok(IndexBuildSummary {
        known_rows: known_rows.len(),
        metagenome_rows: metagenome_rows.len(),
        structure_rows: structure_rows.len(),
    })
}

fn append_canonical_rows(
    index_name: &str,
    root: &Path,
    path: &Path,
    rows: &mut Vec<StoreRow>,
    manifest: &mut String,
) -> Result<(), String> {
    if !path.exists() {
        manifest.push_str(&format!(
            "{index_name}\t{}\tmissing\t0\t{}\tnot_present\n",
            relative_display(root, path),
            index_output_for(index_name)
        ));
        return Ok(());
    }
    let (parsed, notes) = read_source_rows(path, index_name)?;
    let count = parsed.len();
    rows.extend(parsed);
    manifest.push_str(&format!(
        "{index_name}\t{}\t{}\t{count}\t{}\t{notes}\n",
        relative_display(root, path),
        if count > 0 { "used" } else { "empty" },
        index_output_for(index_name)
    ));
    Ok(())
}

fn append_built_rows(
    index_name: &str,
    root: &Path,
    path: &Path,
    built_rows: Vec<StoreRow>,
    rows: &mut Vec<StoreRow>,
    manifest: &mut String,
    notes: &str,
) {
    let total = built_rows.len();
    rows.extend(built_rows);
    manifest.push_str(&format!(
        "{index_name}\t{}\t{}\t{total}\t{}\t{notes}\n",
        relative_display(root, path),
        if total > 0 { "used" } else { "empty" },
        index_output_for(index_name)
    ));
}

fn append_alignment_database_required(
    index_name: &str,
    root: &Path,
    path: &Path,
    manifest: &mut String,
) {
    manifest.push_str(&format!(
        "{index_name}\t{}\trequires_alignment_database\t0\t{}\tlarge_metagenome_source_uses_mmseqs_target_database\n",
        relative_display(root, path),
        index_output_for(index_name)
    ));
}

fn large_metagenome_fasta_requires_alignment_database(path: &Path) -> bool {
    metagenome_fasta_requires_alignment_database(path, LARGE_METAGENOME_FASTA_TSV_MAX_BYTES)
}

fn metagenome_fasta_requires_alignment_database(path: &Path, max_tsv_source_bytes: u64) -> bool {
    if max_tsv_source_bytes == 0 {
        return false;
    }
    fs::metadata(path)
        .map(|metadata| metadata.len() > max_tsv_source_bytes)
        .unwrap_or(false)
}

fn index_output_for(index_name: &str) -> &'static str {
    match index_name {
        "known_targets" => "indexes/known_targets.tsv",
        "metagenome_targets" => "indexes/metagenome_targets.tsv",
        "structure_targets" => "indexes/structure_targets.tsv",
        _ => "indexes/unknown.tsv",
    }
}

fn build_structure_index_rows(
    options: &StoreOptions,
    manifest: &mut String,
) -> Result<Vec<StoreRow>, String> {
    let mut rows = Vec::new();
    for path in discover_structure_files(&options.science_store.join("downloads/rcsb/mmcif"))? {
        let built = structure_file_row("RCSB", &path);
        append_built_rows(
            "structure_targets",
            &options.science_store,
            &path,
            vec![built],
            &mut rows,
            manifest,
            "rcsb_structure_file_rows",
        );
    }
    for path in discover_biolip_tables(&options.science_store.join("downloads/biolip2"))? {
        let built = parse_biolip_structure_rows(&path)?;
        append_built_rows(
            "structure_targets",
            &options.science_store,
            &path,
            built,
            &mut rows,
            manifest,
            "biolip2_interaction_rows",
        );
    }
    let plinder_sources =
        discover_plinder_tables(&options.science_store.join("downloads/plinder"))?;
    if !plinder_sources.is_empty() {
        let canonical = options.science_store.join("indexes/plinder_targets.tsv");
        if !canonical.is_file() {
            return Err(format!(
                "plinder_canonical_index_missing:{}",
                canonical.display()
            ));
        }
        let built = read_store_rows(&canonical, "plinder_canonical_index")?;
        if built.is_empty() {
            return Err(format!(
                "plinder_canonical_index_empty:{}",
                canonical.display()
            ));
        }
        append_built_rows(
            "structure_targets",
            &options.science_store,
            &canonical,
            built,
            &mut rows,
            manifest,
            "plinder_canonical_rows",
        );
    }
    for path in discover_moad_tables(&options.science_store.join("downloads/binding_moad"))? {
        let built = parse_moad_structure_rows(&path)?;
        append_built_rows(
            "structure_targets",
            &options.science_store,
            &path,
            built,
            &mut rows,
            manifest,
            "binding_moad_complex_rows",
        );
    }
    Ok(rows)
}

fn discover_sequence_files(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    if !root.exists() {
        return Ok(out);
    }
    discover_sequence_files_inner(root, &mut out)?;
    out.sort();
    Ok(out)
}

fn discover_sequence_files_inner(root: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in
        fs::read_dir(root).map_err(|err| format!("read_dir_failed:{}:{err}", root.display()))?
    {
        let entry =
            entry.map_err(|err| format!("read_dir_entry_failed:{}:{err}", root.display()))?;
        let path = entry.path();
        if path.is_dir() {
            discover_sequence_files_inner(&path, out)?;
        } else if is_sequence_file(&path) {
            out.push(path);
        }
    }
    Ok(())
}

fn discover_chembl_sqlite_sources(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    if !root.exists() {
        return Ok(out);
    }
    discover_chembl_sqlite_sources_inner(root, &mut out)?;
    out.sort();
    Ok(out)
}

fn discover_chembl_sqlite_sources_inner(root: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in
        fs::read_dir(root).map_err(|err| format!("read_dir_failed:{}:{err}", root.display()))?
    {
        let entry =
            entry.map_err(|err| format!("read_dir_entry_failed:{}:{err}", root.display()))?;
        let path = entry.path();
        if path.is_dir() {
            discover_chembl_sqlite_sources_inner(&path, out)?;
        } else if is_chembl_sqlite_source(&path) {
            out.push(path);
        }
    }
    Ok(())
}

fn discover_structure_files(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    if !root.exists() {
        return Ok(out);
    }
    discover_matching_files(root, &mut out, is_structure_file)?;
    out.sort();
    Ok(out)
}

fn discover_biolip_tables(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    if !root.exists() {
        return Ok(out);
    }
    discover_matching_files(root, &mut out, |path| {
        let name = lower_file_name(path);
        name == "biolip.txt"
            || name == "biolip.txt.gz"
            || name == "biolip_nr.txt"
            || name == "biolip_nr.txt.gz"
    })?;
    out.sort();
    Ok(out)
}

fn discover_plinder_tables(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    if !root.exists() {
        return Ok(out);
    }
    discover_matching_files(root, &mut out, |path| {
        let name = lower_file_name(path);
        name == "annotation_table.parquet" || name == "annotation_table_nonredundant.parquet"
    })?;
    out.sort();
    Ok(out)
}

fn discover_moad_tables(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    if !root.exists() {
        return Ok(out);
    }
    discover_matching_files(root, &mut out, |path| {
        let name = lower_file_name(path);
        name.ends_with(".csv") || name.ends_with(".tsv") || name.ends_with(".txt")
    })?;
    out.sort();
    Ok(out)
}

fn discover_matching_files(
    root: &Path,
    out: &mut Vec<PathBuf>,
    predicate: fn(&Path) -> bool,
) -> Result<(), String> {
    for entry in
        fs::read_dir(root).map_err(|err| format!("read_dir_failed:{}:{err}", root.display()))?
    {
        let entry =
            entry.map_err(|err| format!("read_dir_entry_failed:{}:{err}", root.display()))?;
        let path = entry.path();
        if path.is_dir() {
            discover_matching_files(&path, out, predicate)?;
        } else if predicate(&path) {
            out.push(path);
        }
    }
    Ok(())
}

fn is_chembl_sqlite_source(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    is_sqlite_name(&name) || (name.ends_with(".tar.gz") && name.contains("chembl"))
}

fn is_structure_file(path: &Path) -> bool {
    let name = lower_file_name(path);
    name.ends_with(".cif")
        || name.ends_with(".cif.gz")
        || name.ends_with(".mmcif")
        || name.ends_with(".mmcif.gz")
        || name.ends_with(".pdb")
        || name.ends_with(".pdb.gz")
        || name.ends_with(".ent")
        || name.ends_with(".ent.gz")
}

fn lower_file_name(path: &Path) -> String {
    path.file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn is_sqlite_name(name: &str) -> bool {
    name.ends_with(".sqlite")
        || name.ends_with(".sqlite3")
        || name.ends_with(".db")
        || name.ends_with(".db3")
}

fn is_sequence_file(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    matches_sequence_name(&name) || name.ends_with(".zip")
}

fn matches_sequence_name(name: &str) -> bool {
    matches!(
        name,
        value if value.ends_with(".fa")
            || value.ends_with(".faa")
            || value.ends_with(".fasta")
            || value.ends_with(".fa.gz")
            || value.ends_with(".faa.gz")
            || value.ends_with(".fasta.gz")
            || value.ends_with(".fa.zip")
            || value.ends_with(".faa.zip")
            || value.ends_with(".fasta.zip")
    )
}

fn parse_fasta_targets(
    path: &Path,
    source: &str,
    default_organism: &str,
    target_class: &str,
    evidence: &str,
) -> Result<Vec<StoreRow>, String> {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if name.ends_with(".gz") {
        let file =
            fs::File::open(path).map_err(|err| format!("open_failed:{}:{err}", path.display()))?;
        let decoder = GzDecoder::new(file);
        return parse_fasta_reader(
            BufReader::new(decoder),
            source,
            default_organism,
            target_class,
            evidence,
        )
        .map_err(|err| format!("gzip_read_failed:{}:{err}", path.display()));
    }
    if name.ends_with(".zip") {
        let text = read_zip_text(path, matches_sequence_name)?;
        return parse_fasta_lines(
            text.lines(),
            source,
            default_organism,
            target_class,
            evidence,
        );
    }
    let file =
        fs::File::open(path).map_err(|err| format!("open_failed:{}:{err}", path.display()))?;
    parse_fasta_reader(
        BufReader::new(file),
        source,
        default_organism,
        target_class,
        evidence,
    )
    .map_err(|err| format!("read_failed:{}:{err}", path.display()))
}

fn parse_fasta_reader<R: BufRead>(
    reader: R,
    source: &str,
    default_organism: &str,
    target_class: &str,
    evidence: &str,
) -> Result<Vec<StoreRow>, std::io::Error> {
    let context = FastaParseContext {
        source,
        default_organism,
        target_class,
        evidence,
    };
    let mut rows = Vec::new();
    let mut header = String::new();
    let mut sequence = String::new();
    for line in reader.lines() {
        let line = line?;
        if parse_fasta_line(&line, &context, &mut header, &mut sequence, &mut rows) {
            return Ok(rows);
        }
    }
    push_fasta_row(
        context.source,
        context.default_organism,
        context.target_class,
        context.evidence,
        &header,
        &sequence,
        &mut rows,
    );
    Ok(rows)
}

fn parse_fasta_lines<'a, I>(
    lines: I,
    source: &str,
    default_organism: &str,
    target_class: &str,
    evidence: &str,
) -> Result<Vec<StoreRow>, String>
where
    I: IntoIterator<Item = &'a str>,
{
    let context = FastaParseContext {
        source,
        default_organism,
        target_class,
        evidence,
    };
    let mut rows = Vec::new();
    let mut header = String::new();
    let mut sequence = String::new();
    for line in lines {
        if parse_fasta_line(line, &context, &mut header, &mut sequence, &mut rows) {
            return Ok(rows);
        }
    }
    push_fasta_row(
        context.source,
        context.default_organism,
        context.target_class,
        context.evidence,
        &header,
        &sequence,
        &mut rows,
    );
    Ok(rows)
}

struct FastaParseContext<'a> {
    source: &'a str,
    default_organism: &'a str,
    target_class: &'a str,
    evidence: &'a str,
}

fn parse_fasta_line(
    line: &str,
    context: &FastaParseContext<'_>,
    header: &mut String,
    sequence: &mut String,
    rows: &mut Vec<StoreRow>,
) -> bool {
    let line = line.trim_start_matches('\u{feff}');
    if let Some(rest) = line.strip_prefix('>') {
        push_fasta_row(
            context.source,
            context.default_organism,
            context.target_class,
            context.evidence,
            header,
            sequence,
            rows,
        );
        *header = rest.trim().to_string();
        sequence.clear();
        false
    } else {
        sequence.push_str(line.trim());
        false
    }
}

fn structure_file_row(source: &str, path: &Path) -> StoreRow {
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    let structure_id = structure_id_from_file_name(file_name);
    let mut structure_tokens = tokenize(&structure_id);
    structure_tokens.extend(tokenize(file_name));
    structure_tokens.extend(tokenize(&path.display().to_string()));
    structure_tokens.extend(structure_metadata_tokens(path));
    StoreRow {
        source: source.to_string(),
        target_id: safe_id(&structure_id),
        target_name: format!("{source} structure {structure_id}"),
        organism: "unknown".to_string(),
        target_class: "structure_record".to_string(),
        ligand_tokens: BTreeSet::new(),
        sequence_tokens: tokenize(&structure_id),
        structure_tokens,
        evidence: format!(
            "{}_structure_file",
            source.to_ascii_lowercase().replace(' ', "_")
        ),
    }
}

fn structure_metadata_tokens(path: &Path) -> BTreeSet<String> {
    read_structure_text_best_effort(path)
        .map(|text| tokenize(&structure_metadata_text(&text)))
        .unwrap_or_default()
}

fn read_structure_text_best_effort(path: &Path) -> Result<String, String> {
    let name = lower_file_name(path);
    if name.ends_with(".gz") {
        return read_gzip_text(path).or_else(|_| read_text(path));
    }
    read_text(path)
}

fn structure_metadata_text(text: &str) -> String {
    let mut out = String::new();
    let mut capture_loop = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("_struct.title") || trimmed.starts_with("_citation.title") {
            push_limited_metadata(&mut out, line);
        }
        if trimmed.starts_with("_chem_comp.") || trimmed.starts_with("_pdbx_entity_nonpoly.") {
            capture_loop = true;
            push_limited_metadata(&mut out, line);
            continue;
        }
        if capture_loop {
            if trimmed == "#" {
                capture_loop = false;
                continue;
            }
            push_limited_metadata(&mut out, line);
        }
        if out.len() >= MAX_STRUCTURE_METADATA_CHARS {
            break;
        }
    }
    out
}

fn push_limited_metadata(out: &mut String, line: &str) {
    if out.len() >= MAX_STRUCTURE_METADATA_CHARS {
        return;
    }
    let remaining = MAX_STRUCTURE_METADATA_CHARS - out.len();
    for ch in line.chars() {
        if out.len() + ch.len_utf8() > remaining {
            break;
        }
        out.push(ch);
    }
    out.push('\n');
}

fn structure_id_from_file_name(file_name: &str) -> String {
    let mut name = file_name.to_string();
    for suffix in [
        ".mmcif.gz",
        ".cif.gz",
        ".pdb.gz",
        ".ent.gz",
        ".mmcif",
        ".cif",
        ".pdb",
        ".ent",
        ".parquet",
    ] {
        if name.to_ascii_lowercase().ends_with(suffix) {
            let keep = name.len().saturating_sub(suffix.len());
            name.truncate(keep);
            break;
        }
    }
    if name.len() == 7 && name.to_ascii_lowercase().starts_with("pdb") {
        name = name[3..].to_string();
    }
    safe_id(&name.to_ascii_uppercase())
}

fn parse_biolip_structure_rows(path: &Path) -> Result<Vec<StoreRow>, String> {
    let text = read_table_text(path)?;
    let mut rows = Vec::new();
    for line in text.lines() {
        let line = line.trim_start_matches('\u{feff}').trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let cols = split_loose_columns(line);
        if cols.is_empty() || cols[0].eq_ignore_ascii_case("pdb_id") {
            continue;
        }
        let pdb = cols.first().map(|value| value.trim()).unwrap_or_default();
        if pdb.is_empty() {
            continue;
        }
        let chain = cols.get(1).map(|value| value.trim()).unwrap_or_default();
        let ligand = cols
            .get(4)
            .or_else(|| cols.get(3))
            .map(|value| value.trim())
            .unwrap_or_default();
        let target_id = if chain.is_empty() {
            safe_id(&pdb.to_ascii_uppercase())
        } else {
            safe_id(&format!("{}_{}", pdb.to_ascii_uppercase(), chain))
        };
        let mut structure_tokens = tokenize(line);
        structure_tokens.extend(tokenize(pdb));
        structure_tokens.extend(tokenize(chain));
        rows.push(StoreRow {
            source: "BioLiP2".to_string(),
            target_id,
            target_name: if chain.is_empty() {
                format!("BioLiP2 interaction {}", pdb.to_ascii_uppercase())
            } else {
                format!(
                    "BioLiP2 interaction {} chain {chain}",
                    pdb.to_ascii_uppercase()
                )
            },
            organism: "unknown".to_string(),
            target_class: "structure_interaction_record".to_string(),
            ligand_tokens: tokenize(ligand),
            sequence_tokens: tokenize(&format!("{pdb} {chain}")),
            structure_tokens: tokenize_limited(
                &structure_tokens.into_iter().collect::<Vec<_>>().join(" "),
                MAX_SEQUENCE_TOKEN_CHARS,
            ),
            evidence: "biolip2_interaction_row".to_string(),
        });
    }
    Ok(rows)
}

fn parse_moad_structure_rows(path: &Path) -> Result<Vec<StoreRow>, String> {
    let text = read_table_text(path)?;
    let mut lines = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'));
    let Some(first_line) = lines.next() else {
        return Ok(Vec::new());
    };
    let delimiter = if first_line.contains('\t') {
        Some('\t')
    } else if first_line.contains(',') {
        Some(',')
    } else {
        None
    };
    let first_cols = split_optional_delimited(first_line, delimiter);
    let first_is_header = first_cols.iter().any(|col| {
        let key = normalize_header(col);
        matches!(
            key.as_str(),
            "pdb" | "pdbid" | "pdbcode" | "pdbfile" | "ligand" | "ligandid" | "hetid"
        )
    });
    let headers = if first_is_header {
        first_cols
    } else {
        Vec::new()
    };
    let pdb_idx = find_header(&headers, &["pdb", "pdb_id", "pdbid", "pdb code", "pdbFile"]);
    let ligand_idx = find_header(&headers, &["ligand", "ligand_id", "het_id", "het id"]);
    let affinity_idx = find_header(
        &headers,
        &[
            "affinity",
            "ki",
            "kd",
            "ic50",
            "ec50",
            "binding data",
            "measurement",
            "magnitude",
            "units",
        ],
    );
    let mut rows = Vec::new();
    let mut row_lines = Vec::new();
    if !first_is_header {
        row_lines.push(first_line.to_string());
    }
    row_lines.extend(lines.map(ToOwned::to_owned));
    for line in row_lines {
        let cols = split_optional_delimited(&line, delimiter);
        let pdb = if first_is_header {
            cell(&cols, pdb_idx)
        } else {
            cols.first()
                .map(|value| value.trim().to_string())
                .unwrap_or_default()
        };
        if pdb.is_empty() {
            continue;
        }
        let ligand = if first_is_header {
            cell(&cols, ligand_idx)
        } else {
            cols.get(1)
                .map(|value| value.trim().to_string())
                .unwrap_or_default()
        };
        let affinity = if first_is_header {
            cell(&cols, affinity_idx)
        } else {
            cols.get(2)
                .map(|value| value.trim().to_string())
                .unwrap_or_default()
        };
        let mut ligand_tokens = tokenize(&ligand);
        ligand_tokens.extend(tokenize(&affinity));
        rows.push(StoreRow {
            source: "Binding MOAD".to_string(),
            target_id: safe_id(&pdb.to_ascii_uppercase()),
            target_name: format!("Binding MOAD complex {}", pdb.to_ascii_uppercase()),
            organism: "unknown".to_string(),
            target_class: "structure_interaction_record".to_string(),
            ligand_tokens,
            sequence_tokens: tokenize(&pdb),
            structure_tokens: tokenize_limited(&line, MAX_SEQUENCE_TOKEN_CHARS),
            evidence: "binding_moad_complex_row".to_string(),
        });
    }
    Ok(rows)
}

fn split_loose_columns(line: &str) -> Vec<String> {
    if line.contains('\t') {
        split_tsv(line)
    } else {
        line.split_whitespace().map(ToOwned::to_owned).collect()
    }
}

fn split_optional_delimited(line: &str, delimiter: Option<char>) -> Vec<String> {
    match delimiter {
        Some('\t') => split_tsv(line),
        Some(',') => split_csv_line(line),
        _ => split_loose_columns(line),
    }
}

fn split_csv_line(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '"' if quoted && chars.peek() == Some(&'"') => {
                current.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            ',' if !quoted => {
                out.push(current.trim().to_string());
                current.clear();
            }
            _ => current.push(ch),
        }
    }
    out.push(current.trim().to_string());
    out
}

fn read_table_text(path: &Path) -> Result<String, String> {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if name.ends_with(".gz") {
        read_gzip_text(path)
    } else if name.ends_with(".zip") {
        read_zip_text(path, matches_table_name)
    } else {
        read_text(path)
    }
}

fn matches_table_name(name: &str) -> bool {
    name.ends_with(".tsv") || name.ends_with(".txt")
}

fn read_gzip_text(path: &Path) -> Result<String, String> {
    let file =
        fs::File::open(path).map_err(|err| format!("open_failed:{}:{err}", path.display()))?;
    let mut decoder = GzDecoder::new(file);
    let mut text = String::new();
    decoder
        .read_to_string(&mut text)
        .map_err(|err| format!("gzip_read_failed:{}:{err}", path.display()))?;
    Ok(text)
}

fn read_zip_text(path: &Path, accept: fn(&str) -> bool) -> Result<String, String> {
    let file =
        fs::File::open(path).map_err(|err| format!("open_failed:{}:{err}", path.display()))?;
    let mut archive =
        ZipArchive::new(file).map_err(|err| format!("zip_open_failed:{}:{err}", path.display()))?;
    for index in 0..archive.len() {
        let mut member = archive
            .by_index(index)
            .map_err(|err| format!("zip_member_failed:{}:{err}", path.display()))?;
        if member.is_dir() {
            continue;
        }
        let name = member.name().to_ascii_lowercase();
        if accept(&name) {
            let mut text = String::new();
            member
                .read_to_string(&mut text)
                .map_err(|err| format!("zip_read_failed:{}:{err}", path.display()))?;
            return Ok(text);
        }
    }
    Err(format!("zip_no_supported_text_member:{}", path.display()))
}

fn push_fasta_row(
    source: &str,
    default_organism: &str,
    target_class: &str,
    evidence: &str,
    header: &str,
    sequence: &str,
    rows: &mut Vec<StoreRow>,
) {
    if header.is_empty() || sequence.is_empty() {
        return;
    }
    let (target_id, target_name) = fasta_id_and_name(header);
    let organism = fasta_organism(header).unwrap_or_else(|| default_organism.to_string());
    let mut sequence_tokens = protein_sequence_tokens(sequence);
    sequence_tokens.extend(tokenize(header));
    rows.push(StoreRow {
        source: source.to_string(),
        target_id,
        target_name,
        organism,
        target_class: target_class.to_string(),
        ligand_tokens: BTreeSet::new(),
        sequence_tokens,
        structure_tokens: tokenize(header),
        evidence: evidence.to_string(),
    });
}

fn fasta_id_and_name(header: &str) -> (String, String) {
    let first = header.split_whitespace().next().unwrap_or(header);
    let parts = first.split('|').collect::<Vec<_>>();
    let target_id = if parts.len() >= 2 { parts[1] } else { first };
    let name_part = if parts.len() >= 3 {
        parts[2].replace('_', " ")
    } else {
        header
            .split_once(' ')
            .map(|(_, rest)| rest)
            .unwrap_or(target_id)
            .to_string()
    };
    let target_name = name_part
        .split(" OS=")
        .next()
        .unwrap_or(&name_part)
        .trim()
        .to_string();
    (target_id.to_string(), target_name)
}

fn fasta_organism(header: &str) -> Option<String> {
    let (_, after_os) = header.split_once(" OS=")?;
    let organism = after_os
        .split(" OX=")
        .next()
        .unwrap_or(after_os)
        .trim()
        .to_string();
    if organism.is_empty() {
        None
    } else {
        Some(organism)
    }
}

fn dedupe_store_rows(rows: Vec<StoreRow>) -> Vec<StoreRow> {
    let mut by_key = BTreeMap::new();
    for row in rows {
        by_key
            .entry((row.source.clone(), row.target_id.clone()))
            .and_modify(|existing: &mut StoreRow| merge_store_row(existing, &row))
            .or_insert(row);
    }
    by_key.into_values().collect()
}

fn merge_store_row(existing: &mut StoreRow, row: &StoreRow) {
    if existing.target_name == "unknown" && row.target_name != "unknown" {
        existing.target_name = row.target_name.clone();
    }
    if existing.organism == "unknown" && row.organism != "unknown" {
        existing.organism = row.organism.clone();
    }
    if existing.target_class != "known_human_protein" && row.target_class == "known_human_protein" {
        existing.target_class = row.target_class.clone();
    }
    existing
        .ligand_tokens
        .extend(row.ligand_tokens.iter().cloned());
    existing
        .sequence_tokens
        .extend(row.sequence_tokens.iter().cloned());
    existing
        .structure_tokens
        .extend(row.structure_tokens.iter().cloned());
    if !existing
        .evidence
        .split(';')
        .any(|item| item == row.evidence)
    {
        existing.evidence.push(';');
        existing.evidence.push_str(&row.evidence);
    }
}

fn write_store_rows(path: &Path, rows: &[StoreRow]) -> Result<(), String> {
    let mut text = store_row_header();
    for row in rows {
        text.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            clean_cell(&row.source),
            clean_cell(&row.target_id),
            clean_cell(&row.target_name),
            clean_cell(&row.organism),
            clean_cell(&row.target_class),
            row.ligand_tokens
                .iter()
                .cloned()
                .collect::<Vec<_>>()
                .join(" "),
            row.sequence_tokens
                .iter()
                .cloned()
                .collect::<Vec<_>>()
                .join(" "),
            row.structure_tokens
                .iter()
                .cloned()
                .collect::<Vec<_>>()
                .join(" "),
            clean_cell(&row.evidence)
        ));
    }
    write_text(path, &text)
}

fn store_row_header() -> String {
    "source\ttarget_id\ttarget_name\torganism\ttarget_class\tligand_tokens\tsequence_tokens\tstructure_tokens\tevidence\n".to_string()
}

#[derive(Clone, Debug)]
struct ToolProbe {
    ready: bool,
    version: String,
    detail: String,
}

fn write_tool_versions(path: &Path) -> Result<(), String> {
    let mut text = String::from("tool\tstatus\tversion\tlicense\tlabel\tdetail\n");
    let mut openmm_probe = None;
    for tool in REQUIRED_TOOLS {
        let probe = probe_tool(tool);
        if tool.id == "openmm" {
            openmm_probe = Some(probe.clone());
        }
        text.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\n",
            tool.id,
            if probe.ready { "ready" } else { "missing" },
            clean_cell(&probe.version),
            tool.license,
            tool.label,
            clean_cell(&probe.detail)
        ));
    }
    if let Some(probe) = openmm_probe {
        text.push_str(&format!(
            "openmm_cuda\t{}\t{}\tMIT\tOpenMM CUDA Platform\t{}\n",
            if openmm_cuda_ready(&probe) {
                "ready"
            } else {
                "missing"
            },
            clean_cell(&probe.version),
            clean_cell(&probe.detail)
        ));
    }
    write_text(path, &text)
}

fn write_source_requirements(options: &StoreOptions, path: &Path) -> Result<(), String> {
    let mut text = String::from(
        "source_id\tlabel\tcategory\tstatus\tlanding_url\taccess_method\tlicense_note\tlocal_path\tindex_target\trepair_action\n",
    );
    for source in REQUIRED_SOURCES {
        let local_path = options.science_store.join(source.local_rel_path);
        let index_path = options.science_store.join(source.index_rel_path);
        let local_available = source_local_available(options, source);
        let status = if local_available && index_path.exists() {
            "ready"
        } else if local_available {
            "downloaded_needs_index"
        } else {
            "missing"
        };
        text.push_str(&format!(
            "{}\t{}\t{}\t{status}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            source.id,
            source.label,
            source.category,
            source.landing_url,
            source.access_method,
            clean_cell(source.license_note),
            relative_display(&options.science_store, &local_path),
            source.index_rel_path,
            source.repair_action
        ));
    }
    write_text(path, &text)
}

fn write_download_plan(options: &StoreOptions, path: &Path) -> Result<(), String> {
    let mut text = String::from(
        "source_id\tstatus\taction\tlanding_url\tlocal_path\tresume_policy\tverification_policy\n",
    );
    for source in REQUIRED_SOURCES {
        let local_path = options.science_store.join(source.local_rel_path);
        text.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\tresume_existing_partial_file\tsha256_or_release_manifest_required\n",
            source.id,
            if source_local_available(options, source) {
                "present"
            } else {
                "download_required"
            },
            source.repair_action,
            source.landing_url,
            relative_display(&options.science_store, &local_path)
        ));
    }
    write_text(path, &text)
}

fn write_index_plan(options: &StoreOptions, path: &Path) -> Result<(), String> {
    let mut text = String::from("source_id\tstatus\taction\tindex_target\tdepends_on\n");
    for source in REQUIRED_SOURCES {
        let index_path = options.science_store.join(source.index_rel_path);
        let status = if index_path.exists() {
            "ready"
        } else if source_local_available(options, source) {
            "index_required"
        } else {
            "waiting_for_download"
        };
        text.push_str(&format!(
            "{}\t{status}\t{}\t{}\t{}\n",
            source.id,
            source.repair_action,
            source.index_rel_path,
            relative_display(
                &options.science_store,
                &options.science_store.join(source.local_rel_path)
            )
        ));
    }
    write_text(path, &text)
}

fn source_local_available(options: &StoreOptions, source: &RequiredSource) -> bool {
    let store = &options.science_store;
    let local_path = store.join(source.local_rel_path);
    if local_path.is_file() || path_has_any_file(&local_path) {
        return true;
    }
    match source.id {
        "chembl" => {
            any_source_path_exists(
                store,
                &[
                    "imports/known_targets.tsv",
                    "imports/known_targets.tsv.gz",
                    "imports/known_targets.zip",
                    "downloads/chembl/known_targets.tsv",
                    "downloads/chembl/known_targets.tsv.gz",
                    "downloads/chembl/known_targets.zip",
                ],
            ) || discover_chembl_sqlite_sources(&store.join("downloads/chembl"))
                .map(|files| !files.is_empty())
                .unwrap_or(false)
        }
        "bindingdb" => any_source_path_exists(
            store,
            &[
                "downloads/bindingdb/known_targets.tsv",
                "downloads/bindingdb/known_targets.tsv.gz",
                "downloads/bindingdb/known_targets.zip",
                "downloads/bindingdb/bindingdb_all.tsv",
                "downloads/bindingdb/bindingdb_all.tsv.gz",
                "downloads/bindingdb/bindingdb_all.tsv.zip",
                "downloads/bindingdb/bindingdb_all.txt",
                "downloads/bindingdb/bindingdb_all.txt.gz",
                "downloads/bindingdb/bindingdb_all.txt.zip",
            ],
        ),
        "uniprot_human_reviewed" => {
            any_source_path_exists(
                store,
                &[
                    "downloads/uniprot/known_targets.tsv",
                    "downloads/uniprot/known_targets.tsv.gz",
                    "downloads/uniprot/known_targets.zip",
                ],
            ) || discover_sequence_files(&store.join("downloads/uniprot"))
                .map(|files| !files.is_empty())
                .unwrap_or(false)
        }
        "metagenome_potential" => {
            any_source_path_exists(
                store,
                &[
                    "imports/metagenome_targets.tsv",
                    "imports/metagenome_targets.tsv.gz",
                    "imports/metagenome_targets.zip",
                    "downloads/metagenome_targets.tsv",
                    "downloads/metagenome_targets.tsv.gz",
                    "downloads/metagenome_targets.zip",
                    "downloads/metagenome/metagenome_targets.tsv",
                    "downloads/metagenome/metagenome_targets.tsv.gz",
                    "downloads/metagenome/metagenome_targets.zip",
                ],
            ) || discover_sequence_files(&store.join("downloads/metagenome"))
                .map(|files| !files.is_empty())
                .unwrap_or(false)
        }
        _ => false,
    }
}

fn any_source_path_exists(store: &Path, rel_paths: &[&str]) -> bool {
    rel_paths.iter().any(|rel| store.join(rel).exists())
}

fn path_has_any_file(path: &Path) -> bool {
    if !path.is_dir() {
        return false;
    }
    let Ok(entries) = fs::read_dir(path) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() || path_has_any_file(&path) {
            return true;
        }
    }
    false
}

fn tools_ready(path: &Path) -> bool {
    let Ok(text) = fs::read_to_string(path) else {
        return false;
    };
    REQUIRED_TOOLS.iter().all(|tool| {
        text.lines().any(|line| {
            let cols = line.split('\t').collect::<Vec<_>>();
            cols.first()
                .is_some_and(|name| name.eq_ignore_ascii_case(tool.id))
                && cols.get(1).copied() == Some("ready")
        })
    })
}

fn probe_tool(tool: &RequiredTool) -> ToolProbe {
    match tool.id {
        "java" => probe_command(tool.id, "java", &["-version"]),
        "nextflow" => probe_command(tool.id, "nextflow", &["-version"]),
        "rdkit" => probe_python_module(tool.id, "rdkit", "rdkit.__version__"),
        "openbabel" => {
            let command_probe = probe_command(tool.id, "obabel", &["-V"]);
            if command_probe.ready {
                command_probe
            } else {
                probe_python_module(tool.id, "openbabel", "'installed'")
            }
        }
        "mmseqs2" => probe_command(tool.id, "mmseqs", &["version"]),
        "foldseek" => probe_command(tool.id, "foldseek", &["version"]),
        "autodock_vina" => probe_command(tool.id, "vina", &["--version"]),
        "openmm" => probe_openmm(),
        "pdbfixer" => probe_python_module(tool.id, "pdbfixer", "'installed'"),
        "openff_toolkit" => probe_openff_toolkit(),
        "mdtraj" => probe_python_module(tool.id, "mdtraj", "mdtraj.__version__"),
        "mdanalysis" => probe_python_module(tool.id, "MDAnalysis", "MDAnalysis.__version__"),
        _ => ToolProbe {
            ready: false,
            version: "unknown".to_string(),
            detail: format!("no_probe_defined_for:{}", tool.id),
        },
    }
}

fn probe_openmm() -> ToolProbe {
    let code = concat!(
        "import openmm, importlib.metadata as metadata, pathlib, site, subprocess, shutil; ",
        "version = getattr(openmm.version, 'version', getattr(openmm, '__version__', 'installed')); ",
        "platforms = [openmm.Platform.getPlatform(i).getName() for i in range(openmm.Platform.getNumPlatforms())]; ",
        "cuda_probe = 'ready' if any(name.lower() == 'cuda' for name in platforms) else 'missing:openmm_platform_list_does_not_include_CUDA'; ",
        "roots = site.getsitepackages() + [str(pathlib.Path(openmm.__file__).resolve().parent.parent)]; ",
        "plugin_dirs = [str(pathlib.Path(root) / 'OpenMM.libs' / 'lib' / 'plugins') for root in roots if (pathlib.Path(root) / 'OpenMM.libs' / 'lib' / 'plugins').exists()]; ",
        "plugin_dir = plugin_dirs[0] if plugin_dirs else ''; ",
        "cuda_pkg = next((dist.version for dist in metadata.distributions() if dist.metadata.get('Name', '').lower() == 'openmm-cuda-12'), ''); ",
        "wsl_libcuda = 'true' if pathlib.Path('/usr/lib/wsl/lib/libcuda.so').exists() or pathlib.Path('/usr/lib/wsl/lib/libcuda.so.1').exists() else 'false'; ",
        "wsl_gpu = 'true' if shutil.which('nvidia-smi') and subprocess.run(['nvidia-smi','-L'], capture_output=True, text=True, timeout=5).returncode == 0 else 'false'; ",
        "print(str(version) + '\\t' + ','.join(platforms) + '\\t' + cuda_probe + '\\t' + plugin_dir + '\\t' + cuda_pkg + '\\t' + wsl_gpu + '\\t' + wsl_libcuda)"
    );
    let mut errors = Vec::new();
    let cuda_toolkit = cuda_toolkit_detail();
    for python in python_command_candidates() {
        let mut command = python.clone();
        command.extend(["-c".to_string(), code.to_string()]);
        match command_output(&command) {
            Ok(output) => {
                let text = process_output_text(&output);
                if output.status.success() {
                    let line =
                        first_nonempty_line(&text).unwrap_or_else(|| "installed\t".to_string());
                    let cols = line.split('\t').collect::<Vec<_>>();
                    let version = cols.first().copied().unwrap_or("installed");
                    let platforms = cols.get(1).copied().unwrap_or("");
                    let cuda_probe = cols.get(2).copied().unwrap_or("");
                    let plugin_dir = cols.get(3).copied().unwrap_or("");
                    let cuda_package = cols.get(4).copied().unwrap_or("");
                    let wsl_gpu = cols.get(5).copied().unwrap_or("");
                    let wsl_libcuda = cols.get(6).copied().unwrap_or("");
                    let cuda = platforms
                        .split(',')
                        .any(|name| name.trim().eq_ignore_ascii_case("cuda"));
                    let cuda_reason = if cuda {
                        "ready".to_string()
                    } else if cuda_probe.is_empty() {
                        "openmm_cuda_platform_missing".to_string()
                    } else {
                        clean_cell(cuda_probe)
                    };
                    return ToolProbe {
                        ready: true,
                        version: version.to_string(),
                        detail: format!(
                            "openmm_python_module_detected;platforms={};cuda={};cuda_failure_reason={};openmm_plugin_dir={};openmm_cuda_package={};wsl_gpu={};wsl_libcuda={};{}",
                            platforms,
                            if cuda { "ready" } else { "missing" },
                            cuda_reason,
                            plugin_dir,
                            cuda_package,
                            wsl_gpu,
                            wsl_libcuda,
                            cuda_toolkit
                        ),
                    };
                }
                errors.push(format!(
                    "{}:{}",
                    python.join(" "),
                    first_nonempty_line(&text).unwrap_or_else(|| "module_missing".to_string())
                ));
            }
            Err(err) => errors.push(format!("{}:{err}", python.join(" "))),
        }
    }
    ToolProbe {
        ready: false,
        version: "missing".to_string(),
        detail: format!(
            "openmm_python_module_missing:{};{}",
            errors.join("|"),
            cuda_toolkit
        ),
    }
}

fn cuda_toolkit_detail() -> String {
    match detect_cuda_toolkit() {
        Some(path) => {
            let nvcc = cuda_nvcc_path(&path);
            let nvcc_status = if nvcc.is_file() { "present" } else { "missing" };
            format!(
                "cuda_toolkit_path={};cuda_path_ready=true;cuda_nvcc={nvcc_status};cuda_wsl_path={}",
                clean_cell(&path.display().to_string()),
                clean_cell(&windows_path_to_wsl(&path.display().to_string()).unwrap_or_default())
            )
        }
        None => {
            "cuda_toolkit_path=;cuda_path_ready=false;cuda_nvcc=missing;cuda_wsl_path=".to_string()
        }
    }
}

fn detect_cuda_toolkit() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    for key in ["CUDA_PATH", "CUDA_HOME", "CUDA_ROOT"] {
        if let Some(path) = std::env::var_os(key).map(PathBuf::from) {
            candidates.push(path);
        }
    }
    if cfg!(windows) {
        candidates.push(PathBuf::from(r"D:\Program Files\CUDA"));
        if let Some(program_files) = std::env::var_os("ProgramFiles").map(PathBuf::from) {
            candidates.push(program_files.join("NVIDIA GPU Computing Toolkit/CUDA"));
        }
    }
    candidates.into_iter().find(|path| {
        path.is_dir() && (cuda_nvcc_path(path).is_file() || path.join("version.json").is_file())
    })
}

fn cuda_nvcc_path(path: &Path) -> PathBuf {
    if cfg!(windows) {
        path.join("bin/nvcc.exe")
    } else {
        path.join("bin/nvcc")
    }
}

fn openmm_cuda_ready(probe: &ToolProbe) -> bool {
    probe.ready
        && probe.detail.split(';').any(|part| {
            part.split_once('=')
                .is_some_and(|(key, value)| key.trim() == "cuda" && value.trim() == "ready")
        })
}

fn probe_command(tool_id: &str, program: &str, args: &[&str]) -> ToolProbe {
    let mut errors = Vec::new();
    for command in command_candidates(program) {
        let mut full_command = vec![command.clone()];
        full_command.extend(args.iter().map(|arg| (*arg).to_string()));
        match command_output(&full_command) {
            Ok(output) => {
                let text = process_output_text(&output);
                if output.status.success() {
                    return ToolProbe {
                        ready: true,
                        version: first_nonempty_line(&text)
                            .unwrap_or_else(|| "installed".to_string()),
                        detail: format!("{tool_id}_detected"),
                    };
                }
                errors.push(format!(
                    "{}:{}",
                    command,
                    first_nonempty_line(&text).unwrap_or_else(|| "probe_failed".to_string())
                ));
            }
            Err(err) => errors.push(format!("{command}:{err}")),
        }
    }
    ToolProbe {
        ready: false,
        version: "missing".to_string(),
        detail: format!("{tool_id}_not_found:{}", errors.join("|")),
    }
}

fn probe_python_module(tool_id: &str, module: &str, version_expr: &str) -> ToolProbe {
    let code = format!("import {module}; print({version_expr})");
    let mut errors = Vec::new();
    for python in python_command_candidates() {
        let mut command = python.clone();
        command.extend(["-c".to_string(), code.clone()]);
        match command_output(&command) {
            Ok(output) => {
                let text = process_output_text(&output);
                if output.status.success() {
                    return ToolProbe {
                        ready: true,
                        version: first_nonempty_line(&text)
                            .unwrap_or_else(|| "installed".to_string()),
                        detail: format!("{tool_id}_python_module_detected"),
                    };
                }
                errors.push(format!(
                    "{}:{}",
                    python.join(" "),
                    first_nonempty_line(&text).unwrap_or_else(|| "module_missing".to_string())
                ));
            }
            Err(err) => errors.push(format!("{}:{err}", python.join(" "))),
        }
    }
    ToolProbe {
        ready: false,
        version: "missing".to_string(),
        detail: format!("{tool_id}_python_module_missing:{}", errors.join("|")),
    }
}

fn probe_openff_toolkit() -> ToolProbe {
    let code = concat!(
        "from rdkit import Chem; ",
        "from openff.toolkit import Molecule; ",
        "mol = Chem.AddHs(Chem.MolFromSmiles('CCO')); ",
        "off = Molecule.from_rdkit(mol, allow_undefined_stereo=True, hydrogens_are_explicit=True); ",
        "print('installed;openff_molecule_from_rdkit_atoms=' + str(off.n_atoms))"
    );
    let mut errors = Vec::new();
    for python in python_command_candidates() {
        let mut command = python.clone();
        command.extend(["-c".to_string(), code.to_string()]);
        match command_output(&command) {
            Ok(output) => {
                let text = process_output_text(&output);
                if output.status.success() {
                    return ToolProbe {
                        ready: true,
                        version: first_nonempty_line(&text)
                            .unwrap_or_else(|| "installed".to_string()),
                        detail: "openff_toolkit_functional_probe_detected".to_string(),
                    };
                }
                errors.push(format!(
                    "{}:{}",
                    python.join(" "),
                    first_nonempty_line(&text)
                        .unwrap_or_else(|| "openff_functional_probe_failed".to_string())
                ));
            }
            Err(err) => errors.push(format!("{}:{err}", python.join(" "))),
        }
    }
    ToolProbe {
        ready: false,
        version: "missing".to_string(),
        detail: format!(
            "openff_toolkit_functional_probe_missing:{}",
            errors.join("|")
        ),
    }
}

fn command_candidates(program: &str) -> Vec<String> {
    let mut candidates = vec![program.to_string()];
    if cfg!(windows) && !program.contains(['\\', '/']) {
        let mut dirs = Vec::new();
        if let Some(path_var) = env::var_os("PATH") {
            dirs.extend(env::split_paths(&path_var));
        }
        dirs.extend(command_wrapper_dirs_from_env(
            env::var_os("LOCALAPPDATA"),
            env::var_os("GENOLIGAND_PYTHON"),
        ));
        for dir in dirs {
            for ext in ["exe", "cmd", "bat"] {
                let candidate = dir.join(format!("{program}.{ext}"));
                if candidate.is_file() {
                    candidates.push(candidate.display().to_string());
                }
            }
        }
    }
    candidates.sort();
    candidates.dedup();
    candidates
}

fn command_wrapper_dirs_from_env(
    local_appdata: Option<OsString>,
    genoligand_python: Option<OsString>,
) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(python) = genoligand_python
        && let Some(parent) = Path::new(&python).parent()
    {
        dirs.push(parent.join("command-wrappers"));
    }
    if let Some(local_appdata) = local_appdata {
        dirs.push(
            PathBuf::from(local_appdata)
                .join("GenoLigand")
                .join("runtime")
                .join("python-worker-env")
                .join("command-wrappers"),
        );
    }
    dirs.into_iter().filter(|dir| dir.is_dir()).collect()
}

fn command_output(command: &[String]) -> Result<Output, std::io::Error> {
    if command.is_empty() {
        return Command::new("").output();
    }
    if cfg!(windows) && is_windows_script(&command[0]) {
        return Command::new("cmd.exe")
            .arg("/C")
            .arg(&command[0])
            .args(&command[1..])
            .output();
    }
    Command::new(&command[0]).args(&command[1..]).output()
}

fn is_windows_script(command: &str) -> bool {
    Path::new(command)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("cmd") || ext.eq_ignore_ascii_case("bat"))
}

fn process_output_text(output: &Output) -> String {
    format!(
        "{}{}",
        decode_process_bytes(&output.stdout),
        decode_process_bytes(&output.stderr)
    )
}

fn decode_process_bytes(data: &[u8]) -> String {
    if data.is_empty() {
        return String::new();
    }
    let zero_count = data.iter().filter(|byte| **byte == 0).count();
    if data.len().is_multiple_of(2) && zero_count * 4 >= data.len() {
        let words = data
            .chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]));
        return char::decode_utf16(words)
            .map(|item| item.unwrap_or(char::REPLACEMENT_CHARACTER))
            .collect::<String>()
            .replace('\0', "");
    }
    String::from_utf8_lossy(data).replace('\0', "")
}

fn first_nonempty_line(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(ToOwned::to_owned)
}

fn run_pipeline(options: &ScienceOptions) -> Result<String, String> {
    ensure_dir(&options.run_dir)?;
    write_run_context(options, "starting", &[])?;
    let blocked = pipeline_blockers(options)?;
    if !blocked.is_empty() {
        write_blocked_outputs(options, &blocked)?;
        write_run_context(options, "blocked", &blocked)?;
        return Ok(format!(
            "pipeline_blocked\treasons={}\trun_dir={}",
            blocked.join(","),
            options.run_dir.display()
        ));
    }
    run_pipeline_stage(options, "input_normalize", || normalize_input(options))?;
    run_pipeline_stage(options, "features_compute", || compute_features(options))?;
    run_pipeline_stage(options, "onsite_sequence_alignment", || {
        run_single_worker_task(options, &SEQUENCE_ALIGNMENT_WORKER_TASK)?;
        Ok("onsite_sequence_alignment_checked".to_string())
    })?;
    run_pipeline_stage(options, "database_match_known", || match_known(options))?;
    run_pipeline_stage(options, "metagenome_match_potential", || {
        match_metagenome(options)
    })?;
    run_pipeline_stage(options, "target_rank", || rank_targets(options))?;
    run_pipeline_stage(options, "structure_evaluate", || {
        evaluate_structure(options)
    })?;
    run_pipeline_stage(options, "dynamics_screen", || {
        run_scientific_workers(options)?;
        screen_dynamics(options)
    })?;
    run_pipeline_stage(options, "target_rank", || rank_targets(options))?;
    run_pipeline_stage(options, "evidence_build", || build_evidence(options))?;
    run_pipeline_stage(options, "report_render", || render_report(options))?;
    write_artifact_source_map(options)?;
    write_text(&options.run_dir.join("run_state.txt"), "completed\n")?;
    write_run_context(options, "completed", &[])?;
    Ok(format!(
        "pipeline_completed\trun_dir={}",
        options.run_dir.display()
    ))
}

fn run_pipeline_stage<F>(
    options: &ScienceOptions,
    stage: &str,
    operation: F,
) -> Result<String, String>
where
    F: FnOnce() -> Result<String, String>,
{
    write_text(
        &options.run_dir.join("current_stage.txt"),
        &(stage.to_string() + "\n"),
    )?;
    operation()
}

fn pipeline_blockers(options: &ScienceOptions) -> Result<Vec<String>, String> {
    let mut blockers = Vec::new();
    if !options.input_manifest.exists() {
        blockers.push("missing_input_manifest".to_string());
    } else if extract_inputs(options)?.is_empty() {
        blockers.push("missing_input".to_string());
    }
    let manifest = options.science_store.join("science_store_manifest.tsv");
    let known = options.science_store.join("indexes/known_targets.tsv");
    let metagenome = options.science_store.join("indexes/metagenome_targets.tsv");
    let structure = options.science_store.join("indexes/structure_targets.tsv");
    let tools = options.science_store.join("indexes/tool_versions.tsv");
    for (label, path) in [
        ("missing_science_store_manifest", manifest),
        ("missing_known_database_index", known),
        ("missing_metagenome_potential_index", metagenome),
        ("missing_structure_interaction_index", structure),
        ("missing_tool_versions_manifest", tools),
    ] {
        if !path.exists() {
            blockers.push(label.to_string());
        }
    }
    let status = options.science_store.join("install_status.json");
    if status.exists() {
        let status_text = read_text(&status)?;
        let value: Value = serde_json::from_str(status_text.trim_start_matches('\u{feff}'))
            .map_err(|err| format!("install_status_parse_failed:{err}"))?;
        if value.get("status").and_then(Value::as_str) != Some("ready") {
            blockers.push("science_store_needs_repair".to_string());
        }
    } else {
        blockers.push("missing_install_status".to_string());
    }
    let source_requirements = options.science_store.join("source_requirements.tsv");
    if source_requirements.exists() {
        for source in unready_source_ids(&source_requirements)? {
            blockers.push(format!("source_needs_repair:{source}"));
        }
    } else {
        blockers.push("missing_source_requirements".to_string());
    }
    if options
        .science_store
        .join("indexes/tool_versions.tsv")
        .exists()
    {
        let tool_text = read_text(&options.science_store.join("indexes/tool_versions.tsv"))?;
        for tool in REQUIRED_TOOLS {
            if !tool_text.lines().any(|line| {
                line.split('\t')
                    .next()
                    .is_some_and(|name| name.eq_ignore_ascii_case(tool.id))
                    && line.contains("\tready\t")
            }) {
                blockers.push(format!("missing_or_unready_tool:{}", tool.id));
            }
        }
    }
    if alignment_database_manifest_required(&options.science_store)
        && !alignment_database_manifest_ready(&options.science_store)?
    {
        blockers.push("alignment_database_needs_repair".to_string());
    }
    Ok(blockers)
}

fn alignment_database_manifest_required(store: &Path) -> bool {
    store
        .join("indexes/mmseqs/mmseqs_target_databases.tsv")
        .exists()
        || store
            .join("indexes/foldseek/foldseek_target_databases.tsv")
            .exists()
}

fn alignment_database_manifest_ready(store: &Path) -> Result<bool, String> {
    for rel in [
        "indexes/mmseqs/mmseqs_target_databases.tsv",
        "indexes/foldseek/foldseek_target_databases.tsv",
    ] {
        let path = store.join(rel);
        if !path.exists() {
            return Ok(false);
        }
        let rows = read_simple_tsv_rows(&path)?;
        if rows.is_empty()
            || rows
                .iter()
                .any(|row| row.get("status").map(String::as_str) != Some("ready"))
        {
            return Ok(false);
        }
    }
    Ok(true)
}

fn unready_source_ids(path: &Path) -> Result<Vec<String>, String> {
    let text = read_text(path)?;
    let mut lines = text.lines();
    let Some(header) = lines.next() else {
        return Ok(vec!["source_requirements_empty".to_string()]);
    };
    let headers = header.split('\t').collect::<Vec<_>>();
    let source_index = headers
        .iter()
        .position(|value| *value == "source_id")
        .unwrap_or(0);
    let status_index = headers
        .iter()
        .position(|value| *value == "status")
        .unwrap_or(3);
    Ok(lines
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| {
            let cols = line.split('\t').collect::<Vec<_>>();
            let status = cols.get(status_index).copied().unwrap_or("");
            if status == "ready" {
                None
            } else {
                Some(
                    cols.get(source_index)
                        .copied()
                        .unwrap_or("source")
                        .to_string(),
                )
            }
        })
        .collect())
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
    write_artifact_source_map(options)?;
    Ok(())
}

fn normalize_input(options: &ScienceOptions) -> Result<String, String> {
    ensure_dir(&options.run_dir)?;
    let conversions = convert_inputs_from_manifest(options)?;
    write_input_conversion_manifest(options, &conversions)?;
    if let Some(failed) = conversions
        .iter()
        .find(|input| input.conversion_status == "failed")
    {
        write_artifact_source_map(options)?;
        return Err(format!(
            "input_conversion_failed:{}:{}",
            failed.input_id,
            clean_cell(&failed.reason)
        ));
    }
    let inputs = normalized_inputs_from_conversions(&conversions);
    if inputs.is_empty() {
        write_artifact_source_map(options)?;
        return Err("missing_input".to_string());
    }
    let mut normalized = format!("{NORMALIZED_INPUT_HEADER}\n");
    for input in inputs {
        normalized.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\tnormalized\t{}\t{}\t{}\n",
            input.input_id,
            input.kind.as_str(),
            encode_tsv_cell(&input.primary_value),
            input.file_count,
            input.tokens.iter().cloned().collect::<Vec<_>>().join(" "),
            clean_cell(&input.record_id),
            input.source_kind,
            clean_cell(&input.source_path),
            input.content_sha256,
            input.structure_atom_count,
            input.structure_chain_count,
            input.structure_residue_count,
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
    write_artifact_source_map(options)?;
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
    let mut text = format!("{FEATURE_MANIFEST_HEADER}\n");
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
                "{}\t{}\t{family}\t{name}\t{value}\tGenoLigand\t\t{}\n",
                input.input_id,
                input.kind.as_str(),
                clean_cell(&input.record_id),
            ));
        }
        for row in typed_feature_rows(&input) {
            text.push_str(&format!(
                "{}\t{}\t{}\t{}\t{}\tGenoLigand\t{}\t{}\n",
                input.input_id,
                input.kind.as_str(),
                clean_cell(&row.family),
                clean_cell(&row.name),
                clean_cell(&row.value),
                clean_cell(&row.token),
                clean_cell(&input.record_id),
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
    let inputs = read_computed_feature_inputs(&options.run_dir.join("feature_manifest.tsv"))?;
    let normalized =
        read_normalized_inputs(&options.run_dir.join("input_normalized_manifest.tsv"))?;
    let rows = read_store_rows(
        &options.science_store.join("indexes/known_targets.tsv"),
        "known_database_index",
    )?;
    let mut out = String::from(
        "input_id\tsource\ttarget_id\ttarget_name\torganism\ttarget_class\tmatch_score\tmatched_features\tevidence\tsource_row_provenance\tactivity_value\tligand_identity\tcurrent_run_query_id\trecord_id\n",
    );
    for input in &inputs {
        for row in &rows {
            let score = known_database_match_score(input, row);
            if score > 0.0 {
                let matched = matched_tokens(input, row).join(" ");
                out.push_str(&format!(
                    "{}\t{}\t{}\t{}\t{}\t{}\t{score:.4}\t{}\t{}\t{}\t\t{}\tprojection_index_match\t{}\n",
                    input.input_id,
                    row.source,
                    row.target_id,
                    clean_cell(&row.target_name),
                    clean_cell(&row.organism),
                    row.target_class,
                    clean_cell(&matched),
                    clean_cell(&row.evidence),
                    clean_cell(&row.evidence),
                    clean_cell(&matched),
                    clean_cell(&input.record_id),
                ));
            }
        }
    }
    append_chembl_onsite_matches(options, &normalized, &mut out)?;
    append_bindingdb_onsite_matches(options, &normalized, &mut out)?;
    append_mmseqs_match_rows(options, "known", &mut out, true)?;
    write_text(&options.run_dir.join("known_database_matches.tsv"), &out)?;
    Ok(format!(
        "known_database_matched\tout={}",
        options.run_dir.join("known_database_matches.tsv").display()
    ))
}

fn known_database_match_score(input: &ComputedFeatureInput, row: &StoreRow) -> f64 {
    let score = score_row(input, row);
    if input.input_kind == "small_molecule" && !row.evidence.contains("bindingdb_onsite") {
        score.min(0.20)
    } else {
        score
    }
}

fn append_chembl_onsite_matches(
    options: &ScienceOptions,
    inputs: &[NormalizedInput],
    out: &mut String,
) -> Result<(), String> {
    let small_molecules = inputs
        .iter()
        .filter(|input| input.kind == InputKind::SmallMolecule)
        .collect::<Vec<_>>();
    if small_molecules.is_empty() {
        return Ok(());
    }
    let sources = discover_chembl_sqlite_sources(&options.science_store.join("downloads/chembl"))?;
    if sources.is_empty() {
        return Ok(());
    }
    ensure_dir(&options.run_dir)?;
    let query_path = options.run_dir.join("chembl_onsite_queries.tsv");
    let mut queries = String::from("input_id\trecord_id\tquery\n");
    for input in small_molecules {
        queries.push_str(&format!(
            "{}\t{}\t{}\n",
            clean_cell(&input.input_id),
            clean_cell(&input.record_id),
            encode_tsv_cell(&input.primary_value)
        ));
    }
    write_text(&query_path, &queries)?;
    for source in sources {
        let matches = run_python_script(
            chembl_onsite_query_script(),
            &[
                source.display().to_string(),
                query_path.display().to_string(),
                "200".to_string(),
            ],
            "chembl_onsite_query",
        )?;
        for line in matches.lines().filter(|line| !line.trim().is_empty()) {
            out.push_str(line);
            out.push('\n');
        }
    }
    Ok(())
}

fn chembl_onsite_query_script() -> &'static str {
    r#"
import csv
import os
import sqlite3
import sys
import tarfile
import tempfile

source_path, query_path, max_rows = sys.argv[1], sys.argv[2], int(sys.argv[3])

def clean(value):
    return str(value or "").replace("\t", " ").replace("\r", " ").replace("\n", " ").strip()

def decode(value):
    text = str(value or "")
    out = []
    index = 0
    while index < len(text):
        ch = text[index]
        if ch != "\\" or index + 1 >= len(text):
            out.append(ch)
            index += 1
            continue
        nxt = text[index + 1]
        if nxt == "t":
            out.append("\t")
        elif nxt == "n":
            out.append("\n")
        elif nxt == "r":
            out.append("\r")
        elif nxt == "\\":
            out.append("\\")
        else:
            out.append(nxt)
        index += 2
    return "".join(out)

try:
    from rdkit import Chem
except Exception:
    Chem = None

canonical_cache = {}

def canonical_smiles(value):
    text = clean(value)
    if not text:
        return ""
    if text in canonical_cache:
        return canonical_cache[text]
    if Chem is None:
        canonical_cache[text] = "".join(text.split())
        return canonical_cache[text]
    mol = Chem.MolFromSmiles(text)
    if mol is None:
        canonical_cache[text] = "".join(text.split())
    else:
        canonical_cache[text] = Chem.MolToSmiles(mol, isomericSmiles=False)
    return canonical_cache[text]

def target_class(organism):
    low = organism.lower()
    if "homo sapiens" in low or "human" in low:
        return "known_human_protein"
    return "known_protein"

def numeric_activity(value):
    text = clean(value).replace(",", "")
    try:
        return float(text) if text else float("inf")
    except ValueError:
        return float("inf")

def activity_score(value):
    numeric = numeric_activity(value)
    if numeric == float("inf"):
        return 0.65
    if numeric <= 10:
        return 1.0
    if numeric <= 100:
        return 0.95
    if numeric <= 1000:
        return 0.85
    if numeric <= 10000:
        return 0.70
    if numeric <= 100000:
        return 0.55
    return 0.40

def format_activity_number(value):
    text = clean(value)
    try:
        numeric = float(text)
    except ValueError:
        return text
    if numeric.is_integer():
        return str(int(numeric))
    return text

queries = []
with open(query_path, "r", encoding="utf-8-sig", newline="") as handle:
    for row in csv.DictReader(handle, delimiter="\t"):
        query = decode(row.get("query", "")).strip()
        canonical = canonical_smiles(query)
        if query and canonical:
            queries.append((
                clean(row.get("input_id", "input_1")) or "input_1",
                clean(row.get("record_id", "record_1")) or "record_1",
                canonical,
            ))

if not queries:
    raise SystemExit(0)

query_by_smiles = {}
for input_id, record_id, canonical in queries:
    query_by_smiles.setdefault(canonical, []).append((input_id, record_id))

def sqlite_path_from_source(path):
    lower = path.lower()
    if not lower.endswith(".tar.gz"):
        return path, None
    tmp = tempfile.TemporaryDirectory(prefix="genoligand_chembl_onsite_")
    with tarfile.open(path, "r:gz") as archive:
        for member in archive.getmembers():
            name = member.name.lower()
            if member.isfile() and (name.endswith(".db") or name.endswith(".sqlite") or name.endswith(".sqlite3")):
                extracted = archive.extractfile(member)
                if extracted is None:
                    continue
                db_path = os.path.join(tmp.name, os.path.basename(member.name))
                with open(db_path, "wb") as handle:
                    handle.write(extracted.read())
                return db_path, tmp
    tmp.cleanup()
    return "", None

db_path, tmp_dir = sqlite_path_from_source(source_path)
if not db_path:
    raise SystemExit(0)

placeholders = ",".join("?" for _ in query_by_smiles)
sql = f"""
SELECT
    t.chembl_id AS target_id,
    COALESCE(t.pref_name, t.chembl_id) AS target_name,
    COALESCE(t.organism, '') AS organism,
    COALESCE(cs.canonical_smiles, '') AS canonical_smiles,
    COALESCE(md.pref_name, '') AS compound_name,
    COALESCE(act.standard_type, '') AS standard_type,
    COALESCE(act.standard_relation, '') AS standard_relation,
    COALESCE(CAST(act.standard_value AS TEXT), '') AS standard_value,
    COALESCE(act.standard_units, '') AS standard_units
FROM activities act
JOIN assays a ON act.assay_id = a.assay_id
JOIN target_dictionary t ON a.tid = t.tid
LEFT JOIN molecule_dictionary md ON act.molregno = md.molregno
LEFT JOIN compound_structures cs ON act.molregno = cs.molregno
WHERE act.standard_type IN ('Ki', 'Kd', 'IC50', 'EC50')
  AND act.standard_value IS NOT NULL
  AND (act.standard_units IS NULL OR act.standard_units = '' OR act.standard_units = 'nM')
  AND cs.canonical_smiles IN ({placeholders})
"""

try:
    connection = sqlite3.connect(db_path)
    writer = csv.writer(sys.stdout, delimiter="\t", lineterminator="\n")
    best = {}
    for row in connection.execute(sql, list(query_by_smiles.keys())):
        target_id, target_name, organism, smiles, compound_name, standard_type, relation, value, units = [clean(item) for item in row]
        if not target_id or not smiles:
            continue
        activity = f"{standard_type}={format_activity_number(value)} {units}".strip()
        matched = f"canonical_smiles={smiles};compound_name={compound_name}"
        evidence = f"chembl_onsite_activity_row;{activity};relation={relation};compound_name={compound_name}"
        for input_id, record_id in query_by_smiles.get(smiles, []):
            out_row = [
                input_id,
                "ChEMBL",
                target_id,
                target_name or target_id,
                organism or "unknown",
                target_class(organism),
                f"{activity_score(value):.4f}",
                matched,
                evidence,
                "chembl_onsite_activity_row",
                activity,
                matched,
                "current_run_onsite_query",
                record_id,
            ]
            key = (input_id, record_id, target_id)
            rank = (numeric_activity(value), target_name.lower(), organism.lower())
            if key not in best or rank < best[key][0]:
                best[key] = (rank, out_row)
    for _, out_row in sorted(best.values(), key=lambda item: item[0])[:max_rows]:
        writer.writerow(out_row)
finally:
    if tmp_dir is not None:
        tmp_dir.cleanup()
"#
}

fn append_bindingdb_onsite_matches(
    options: &ScienceOptions,
    inputs: &[NormalizedInput],
    out: &mut String,
) -> Result<(), String> {
    let bindingdb = options
        .science_store
        .join("downloads/bindingdb/bindingdb_all.tsv.zip");
    if !bindingdb.exists() {
        return Ok(());
    }
    let small_molecules = inputs
        .iter()
        .filter(|input| input.kind == InputKind::SmallMolecule)
        .collect::<Vec<_>>();
    if small_molecules.is_empty() {
        return Ok(());
    }
    ensure_dir(&options.run_dir)?;
    let query_path = options.run_dir.join("bindingdb_onsite_queries.tsv");
    let mut queries = String::from("input_id\trecord_id\tquery\n");
    for input in small_molecules {
        queries.push_str(&format!(
            "{}\t{}\t{}\n",
            clean_cell(&input.input_id),
            clean_cell(&input.record_id),
            encode_tsv_cell(&input.primary_value)
        ));
    }
    write_text(&query_path, &queries)?;
    let matches = run_python_script(
        bindingdb_onsite_query_script(),
        &[
            bindingdb.display().to_string(),
            query_path.display().to_string(),
            "200".to_string(),
        ],
        "bindingdb_onsite_query",
    )?;
    for line in matches.lines().filter(|line| !line.trim().is_empty()) {
        out.push_str(line);
        out.push('\n');
    }
    Ok(())
}

fn bindingdb_onsite_query_script() -> &'static str {
    r#"
import csv
import sys
import zipfile

zip_path, query_path, max_rows = sys.argv[1], sys.argv[2], int(sys.argv[3])

def clean(value):
    return str(value or "").replace("\t", " ").replace("\r", " ").replace("\n", " ").strip()

def decode(value):
    text = str(value or "")
    out = []
    index = 0
    while index < len(text):
        ch = text[index]
        if ch != "\\" or index + 1 >= len(text):
            out.append(ch)
            index += 1
            continue
        nxt = text[index + 1]
        if nxt == "t":
            out.append("\t")
        elif nxt == "n":
            out.append("\n")
        elif nxt == "r":
            out.append("\r")
        elif nxt == "\\":
            out.append("\\")
        else:
            out.append(nxt)
        index += 2
    return "".join(out)

def first(row, names):
    for name in names:
        value = clean(row.get(name, ""))
        if value:
            return value
    return ""

def target_class(organism):
    low = organism.lower()
    if "homo sapiens" in low or "human" in low:
        return "known_human_protein"
    return "known_protein"

def numeric_activity(value):
    text = clean(value).replace(",", "")
    number = []
    seen_digit = False
    for ch in text:
        if ch.isdigit() or ch == ".":
            number.append(ch)
            seen_digit = True
        elif seen_digit:
            break
    try:
        return float("".join(number)) if number else float("inf")
    except ValueError:
        return float("inf")

def activity_score(value):
    numeric = numeric_activity(value)
    if numeric == float("inf"):
        return 0.65
    if numeric <= 10:
        score = 1.0
    elif numeric <= 100:
        score = 0.95
    elif numeric <= 1000:
        score = 0.85
    elif numeric <= 10000:
        score = 0.70
    elif numeric <= 100000:
        score = 0.55
    else:
        score = 0.40
    if ">" in str(value):
        score = min(score, 0.40)
    return score

try:
    from rdkit import Chem
except Exception:
    Chem = None

def canonical_smiles(value):
    text = clean(value)
    if not text:
        return ""
    if text in canonical_cache:
        return canonical_cache[text]
    if Chem is None:
        canonical_cache[text] = "".join(text.split()).lower()
        return canonical_cache[text]
    mol = Chem.MolFromSmiles(text)
    if mol is None:
        canonical_cache[text] = "".join(text.split()).lower()
    else:
        canonical_cache[text] = Chem.MolToSmiles(mol, isomericSmiles=False)
    return canonical_cache[text]

def atom_counts(smiles):
    counts = {}
    text = clean(smiles)
    index = 0
    while index < len(text):
        ch = text[index]
        symbol = ""
        if ch == "[":
            end = text.find("]", index + 1)
            token = text[index + 1:end if end != -1 else len(text)]
            letters = "".join(part for part in token if part.isalpha())
            if letters:
                symbol = letters[:2] if letters[:2] in {"Cl", "Br"} else letters[:1]
            index = (end + 1) if end != -1 else len(text)
        elif ch.isalpha():
            two = text[index:index + 2]
            symbol = two if two in {"Cl", "Br"} else ch
            index += len(symbol)
        else:
            index += 1
        if symbol:
            symbol = symbol[0].upper() + symbol[1:].lower()
            counts[symbol] = counts.get(symbol, 0) + 1
    return tuple(sorted(counts.items()))

canonical_cache = {}

def query_matches(query_info, smiles, ligand_name):
    _input_id, _record_id, query, query_canonical, query_counts = query_info
    q = query.strip()
    q_lower = q.lower()
    compact = "".join(q.split()).lower()
    if compact and compact == "".join(smiles.split()).lower():
        return True
    if query_canonical and query_counts and atom_counts(smiles) == query_counts and query_canonical == canonical_smiles(smiles):
        return True
    if q_lower and q_lower in ligand_name.lower():
        return True
    return False

queries = []
with open(query_path, "r", encoding="utf-8-sig", newline="") as handle:
    for row in csv.DictReader(handle, delimiter="\t"):
        query = decode(row.get("query", "")).strip()
        if query:
            queries.append((
                clean(row.get("input_id", "input_1")) or "input_1",
                clean(row.get("record_id", "record_1")) or "record_1",
                query,
                canonical_smiles(query),
                atom_counts(query),
            ))

if not queries:
    raise SystemExit(0)

with zipfile.ZipFile(zip_path) as archive:
    members = [name for name in archive.namelist() if name.lower().endswith((".tsv", ".txt"))]
    if not members:
        raise SystemExit(0)
    with archive.open(members[0]) as raw:
        text = (line.decode("utf-8", "replace") for line in raw)
        reader = csv.DictReader(text, delimiter="\t")
        best = {}
        writer = csv.writer(sys.stdout, delimiter="\t", lineterminator="\n")
        for row in reader:
            smiles = first(row, ["Ligand SMILES", "SMILES"])
            ligand_name = first(row, ["BindingDB Ligand Name", "Ligand Name", "Compound Name"])
            for query_info in queries:
                input_id = query_info[0]
                record_id = query_info[1]
                if not query_matches(query_info, smiles, ligand_name):
                    continue
                target_name = first(row, ["Target Name", "UniProt (SwissProt) Recommended Name of Target Chain 1"])
                organism = first(row, ["Target Source Organism According to Curator or DataSource", "Organism"])
                target_id = first(row, [
                    "UniProt (SwissProt) Primary ID of Target Chain 1",
                    "UniProt (TrEMBL) Primary ID of Target Chain 1",
                    "BindingDB MonomerID",
                    "Target Name",
                ])
                if not target_name:
                    target_name = target_id
                if not target_id:
                    continue
                ligand_code = first(row, ["Ligand HET ID in PDB"])
                ligand_complex_pdb_ids = first(row, ["PDB ID(s) for Ligand-Target Complex"])
                target_chain_pdb_ids = first(row, ["PDB ID(s) of Target Chain 1"])
                activity = first(row, ["Ki (nM)", "IC50 (nM)", "Kd (nM)", "EC50 (nM)"])
                activity_type = next((name for name in ["Ki (nM)", "IC50 (nM)", "Kd (nM)", "EC50 (nM)"] if clean(row.get(name, ""))), "activity")
                pubchem = first(row, ["PubChem CID"])
                matched = f"ligand_name={ligand_name};ligand_smiles={smiles};ligand_code={ligand_code};pdb_ids={ligand_complex_pdb_ids};target_chain_pdb_ids={target_chain_pdb_ids}"
                activity_value = f"{activity_type}={activity}".strip()
                ligand_identity = f"ligand_smiles={smiles};ligand_name={ligand_name};ligand_code={ligand_code}"
                evidence = f"bindingdb_onsite_activity_row;{activity_value};ligand_code={ligand_code};pdb_ids={ligand_complex_pdb_ids};target_chain_pdb_ids={target_chain_pdb_ids};pubchem_cid={pubchem}"
                out_row = [
                    input_id,
                    "BindingDB",
                    target_id,
                    target_name,
                    organism or "unknown",
                    target_class(organism),
                    f"{activity_score(activity):.4f}",
                    clean(matched),
                    clean(evidence),
                    "bindingdb_onsite_activity_row",
                    clean(activity_value),
                    clean(ligand_identity),
                    "current_run_onsite_query",
                    record_id,
                ]
                key = (input_id, record_id, target_id)
                rank = (numeric_activity(activity), target_name.lower(), organism.lower())
                if key not in best or rank < best[key][0]:
                    best[key] = (rank, out_row)
        for _, out_row in sorted(best.values(), key=lambda item: item[0])[:max_rows]:
            writer.writerow(out_row)
"#
}

fn match_metagenome(options: &ScienceOptions) -> Result<String, String> {
    let inputs = read_computed_feature_inputs(&options.run_dir.join("feature_manifest.tsv"))?;
    let rows = if alignment_database_manifest_required(&options.science_store) {
        Vec::new()
    } else {
        read_store_rows(
            &options.science_store.join("indexes/metagenome_targets.tsv"),
            "metagenome_potential_index",
        )?
    };
    let mut out = String::from(
        "input_id\tsource\ttarget_id\ttarget_name\torganism\ttarget_class\tmatch_score\tmatched_features\tevidence\tsource_database_id\tcurrent_run_query_id\trecord_id\n",
    );
    for input in &inputs {
        for row in &rows {
            let score = score_row(input, row);
            if score > 0.0 {
                out.push_str(&format!(
                    "{}\t{}\t{}\t{}\t{}\t{}\t{score:.4}\t{}\t{}\t{}\t{}\t{}\n",
                    input.input_id,
                    row.source,
                    row.target_id,
                    clean_cell(&row.target_name),
                    clean_cell(&row.organism),
                    row.target_class,
                    matched_tokens(input, row).join(" "),
                    clean_cell(&row.evidence),
                    clean_cell(&row.source),
                    clean_cell(&input.input_id),
                    clean_cell(&input.record_id),
                ));
            }
        }
    }
    append_mmseqs_match_rows(options, "metagenome", &mut out, false)?;
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
    let structure_matches = if options
        .run_dir
        .join("input_normalized_manifest.tsv")
        .exists()
    {
        match_structure_interactions(options)?
    } else {
        write_empty_structure_interaction_matches(options)?;
        Vec::new()
    };
    let structure = options.run_dir.join("structure_patch_summary.tsv");
    let mut structure_ready =
        if copy_tool_output_if_available(options, "structure_patch_summary.tsv", &structure)? {
            true
        } else {
            write_structure_summary_from_matches(&structure, &structure_matches)?;
            !structure_matches.is_empty()
        };
    let viewer_ready = copy_structure_viewer_artifact_if_available(options)?
        || write_structure_viewer_from_known_hints(options)?
        || write_structure_viewer_from_matches(options, &structure_matches)?;
    run_single_worker_task(options, &STRUCTURE_ALIGNMENT_WORKER_TASK)?;
    if append_tool_match_rows_to_file(
        options,
        &options.run_dir.join("structure_interaction_matches.tsv"),
        "foldseek_structure_matches.tsv",
    )? > 0
    {
        structure_ready = true;
    }
    if append_selected_structure_match(
        &options.run_dir.join("structure_interaction_matches.tsv"),
        &options.run_dir.join("complex_candidate_selection.tsv"),
    )? {
        structure_ready = true;
    }
    if append_selected_structure_summary(
        &structure,
        &options.run_dir.join("complex_candidate_selection.tsv"),
    )? {
        structure_ready = true;
    }
    if viewer_ready
        && !options
            .run_dir
            .join("tool_outputs/docking_summary.tsv")
            .exists()
    {
        run_single_worker_task(options, &DOCKING_WORKER_TASK)?;
    }
    let docking = options.run_dir.join("docking_summary.tsv");
    let docking_ready = copy_tool_output_or_header(
        options,
        "docking_summary.tsv",
        "candidate_id\tdocking_status\tdocking_score\tdocking_engine\tnotes\n",
        &docking,
    )?;
    Ok(format!(
        "structure_evaluated\tstructure_artifact={}\tdocking_artifact={}\tviewer_artifact={}",
        if structure_ready {
            "ready"
        } else {
            "not_ready"
        },
        if docking_ready { "ready" } else { "not_ready" },
        if viewer_ready { "ready" } else { "not_ready" }
    ))
}

fn write_empty_structure_interaction_matches(options: &ScienceOptions) -> Result<(), String> {
    write_text(
        &options.run_dir.join("structure_interaction_matches.tsv"),
        "input_id\tsource\ttarget_id\ttarget_name\torganism\ttarget_class\tmatch_score\tmatched_features\tevidence\tsource_database_id\tcurrent_run_query_id\trecord_id\n",
    )
}

fn append_tool_match_rows(
    options: &ScienceOptions,
    tool_output_name: &str,
    out: &mut String,
) -> Result<usize, String> {
    let path = options.run_dir.join("tool_outputs").join(tool_output_name);
    if !path.exists() {
        return Ok(0);
    }
    let text = read_text(&path)?;
    let mut lines = text.lines();
    let Some(header) = lines.next() else {
        return Ok(0);
    };
    let headers = split_tsv(header);
    let source_db_idx = find_header(
        &headers,
        &["source_database_id", "source_db", "database_id"],
    );
    let query_idx = find_header(&headers, &["current_run_query_id", "query_id"]);
    let record_idx = find_header(&headers, &["record_id"]);
    let mut count = 0usize;
    for line in lines.filter(|line| !line.trim().is_empty()) {
        let cols = split_tsv(line);
        if cols.len() < 9 {
            continue;
        }
        let evidence = cols[8].clone();
        let source_database_id = first_nonempty(&[
            cell(&cols, source_db_idx),
            evidence_field(&evidence, &["source_db", "source_id", "database_id"]),
            cols[1].clone(),
        ]);
        let current_run_query_id = first_nonempty(&[
            cell(&cols, query_idx),
            evidence_field(&evidence, &["query_id", "current_run_query_id"]),
            cols[0].clone(),
        ]);
        out.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            clean_cell(&cols[0]),
            clean_cell(&cols[1]),
            clean_cell(&cols[2]),
            clean_cell(&cols[3]),
            clean_cell(&cols[4]),
            clean_cell(&cols[5]),
            clean_cell(&cols[6]),
            clean_cell(&cols[7]),
            clean_cell(&cols[8]),
            clean_cell(&source_database_id),
            clean_cell(&current_run_query_id),
            clean_cell(&first_nonempty(&[
                cell(&cols, record_idx),
                evidence_field(&evidence, &["record_id"]),
                "record_1".to_string(),
            ])),
        ));
        count += 1;
    }
    Ok(count)
}

fn append_mmseqs_match_rows(
    options: &ScienceOptions,
    expected_source_class: &str,
    out: &mut String,
    known_output: bool,
) -> Result<usize, String> {
    let path = options
        .run_dir
        .join("tool_outputs/mmseqs_alignment_matches.tsv");
    if !path.exists() {
        return Ok(0);
    }
    let text = read_text(&path)?;
    let mut lines = text.lines();
    let Some(header) = lines.next() else {
        return Err("mmseqs_provenance_header_missing".to_string());
    };
    let headers = split_tsv(header);
    let required_header = |name: &str| {
        find_header(&headers, &[name])
            .ok_or_else(|| format!("mmseqs_provenance_header_missing:{name}"))
    };
    let source_db_idx = required_header("source_database_id")?;
    let namespace_idx = required_header("source_namespace")?;
    let source_class_idx = required_header("source_class")?;
    let source_digest_idx = required_header("source_digest")?;
    let builder_version_idx = required_header("builder_version")?;
    let mmseqs_version_idx = required_header("mmseqs_version")?;
    let query_idx = required_header("current_run_query_id")?;
    let record_idx = required_header("record_id")?;
    let mut count = 0usize;
    for (line_index, line) in lines.enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let cols = split_tsv(line);
        if cols.len() < 9 {
            return Err(format!("mmseqs_result_row_invalid:line={}", line_index + 2));
        }
        let source_database_id = cell(&cols, Some(source_db_idx));
        let source_namespace = cell(&cols, Some(namespace_idx));
        let source_class = cell(&cols, Some(source_class_idx)).to_ascii_lowercase();
        let source_digest = cell(&cols, Some(source_digest_idx));
        let builder_version = cell(&cols, Some(builder_version_idx));
        let mmseqs_version = cell(&cols, Some(mmseqs_version_idx));
        let current_run_query_id = cell(&cols, Some(query_idx));
        let record_id = cell(&cols, Some(record_idx));
        if source_database_id.is_empty()
            || source_namespace.is_empty()
            || source_digest.is_empty()
            || builder_version.is_empty()
            || mmseqs_version.is_empty()
            || current_run_query_id.is_empty()
            || record_id.is_empty()
        {
            return Err(format!("mmseqs_provenance_missing:line={}", line_index + 2));
        }
        if !matches!(source_class.as_str(), "known" | "metagenome") {
            return Err(format!(
                "mmseqs_source_class_invalid:line={}:class={}",
                line_index + 2,
                clean_cell(&source_class)
            ));
        }
        if source_class != expected_source_class {
            continue;
        }
        let target_class = if source_class == "known" {
            "known_protein"
        } else {
            "potential_metagenome_sequence"
        };
        let evidence = format!(
            "{};source_digest={};builder_version={};mmseqs_version={}",
            clean_cell(&cols[8]),
            clean_cell(&source_digest),
            clean_cell(&builder_version),
            clean_cell(&mmseqs_version)
        );
        if known_output {
            let provenance = format!(
                "mmseqs_database={};source_namespace={};source_class={};source_digest={};builder_version={};mmseqs_version={}",
                clean_cell(&source_database_id),
                clean_cell(&source_namespace),
                source_class,
                clean_cell(&source_digest),
                clean_cell(&builder_version),
                clean_cell(&mmseqs_version)
            );
            out.push_str(&format!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t\t\t{}\t{}\n",
                clean_cell(&cols[0]),
                clean_cell(&source_namespace),
                clean_cell(&cols[2]),
                clean_cell(&cols[3]),
                clean_cell(&cols[4]),
                target_class,
                clean_cell(&cols[6]),
                clean_cell(&cols[7]),
                evidence,
                provenance,
                clean_cell(&current_run_query_id),
                clean_cell(&record_id),
            ));
        } else {
            out.push_str(&format!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                clean_cell(&cols[0]),
                clean_cell(&source_namespace),
                clean_cell(&cols[2]),
                clean_cell(&cols[3]),
                clean_cell(&cols[4]),
                target_class,
                clean_cell(&cols[6]),
                clean_cell(&cols[7]),
                evidence,
                clean_cell(&source_database_id),
                clean_cell(&current_run_query_id),
                clean_cell(&record_id),
            ));
        }
        count += 1;
    }
    Ok(count)
}

fn evidence_field(evidence: &str, keys: &[&str]) -> String {
    for part in evidence.split(';').map(str::trim) {
        let Some((key, value)) = part.split_once('=') else {
            continue;
        };
        if keys.iter().any(|candidate| key.trim() == *candidate) {
            return value.trim().to_string();
        }
    }
    String::new()
}

fn append_tool_match_rows_to_file(
    options: &ScienceOptions,
    target_path: &Path,
    tool_output_name: &str,
) -> Result<usize, String> {
    let mut text = if target_path.exists() {
        read_text(target_path)?
    } else {
        "input_id\tsource\ttarget_id\ttarget_name\torganism\ttarget_class\tmatch_score\tmatched_features\tevidence\tsource_database_id\tcurrent_run_query_id\trecord_id\n".to_string()
    };
    let count = append_tool_match_rows(options, tool_output_name, &mut text)?;
    write_text(target_path, &text)?;
    Ok(count)
}

fn run_scientific_workers(options: &ScienceOptions) -> Result<String, String> {
    let job_dir = options.run_dir.join("worker_jobs");
    let result_dir = options.run_dir.join("worker_results");
    ensure_dir(&job_dir)?;
    ensure_dir(&result_dir)?;
    let mut summary = String::from(
        "job_id\tworker\tstage\tjob_schema_valid\tlaunch_status\tresult_schema_valid\tresult_status\texecution_status\treason\toutcome_count\tengine_version\tprimitive_execution_only\tpython_owns_final_score\tpython_owns_report_semantics\n",
    );
    let mut worker_errors = Vec::new();
    for task in SCIENTIFIC_WORKER_TASKS {
        let job_path = job_dir.join(format!("{}.json", task.job_id));
        let result_path = result_dir.join(format!("{}.json", task.job_id));
        let job = worker_job_value(options, task);
        write_text(&job_path, &(job.to_string() + "\n"))?;
        let job_schema_valid = validate_worker_job(&job, task);
        let launch_status = match run_worker_task(options, task, &job_path, &result_path) {
            Ok(()) => "ok".to_string(),
            Err(err) => {
                if !result_path.exists() {
                    write_worker_failure_result(&result_path, task, &err)?;
                }
                format!("failed:{}", clean_cell(&err))
            }
        };
        if !result_path.exists() {
            write_worker_failure_result(&result_path, task, "worker_result_missing")?;
        }
        let result = read_worker_result_or_failure(&result_path, task)?;
        let result_schema_valid = validate_worker_result(&result, task);
        let result_status = result
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("FAIL");
        let engine_version = result
            .get("engine_version")
            .and_then(Value::as_str)
            .unwrap_or("missing");
        let execution_status = result
            .get("execution_status")
            .and_then(Value::as_str)
            .unwrap_or("missing");
        let reason = result
            .get("reason")
            .and_then(Value::as_str)
            .unwrap_or("missing");
        let outcome_count = result
            .get("outcomes")
            .and_then(Value::as_array)
            .map(Vec::len)
            .unwrap_or_default();
        let primitive_execution_only = result
            .get("primitive_execution_only")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let python_owns_final_score = result
            .get("python_owns_final_score")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let python_owns_report_semantics = result
            .get("python_owns_report_semantics")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        summary.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            task.job_id,
            task.worker,
            task.stage,
            job_schema_valid,
            clean_cell(&launch_status),
            result_schema_valid,
            clean_cell(result_status),
            clean_cell(execution_status),
            clean_cell(reason),
            outcome_count,
            clean_cell(engine_version),
            primitive_execution_only,
            python_owns_final_score,
            python_owns_report_semantics
        ));
        if let Some(error) = worker_result_stage_error(&result, task) {
            worker_errors.push(error);
        }
    }
    write_text(
        &options.run_dir.join("scientific_runtime_summary.tsv"),
        &summary,
    )?;
    if !worker_errors.is_empty() {
        return Err(format!(
            "scientific_workers_failed:{}",
            clean_cell(&worker_errors.join(" | "))
        ));
    }
    Ok(format!(
        "scientific_workers_checked\tworker_count={}",
        SCIENTIFIC_WORKER_TASKS.len()
    ))
}

fn run_single_worker_task(options: &ScienceOptions, task: &WorkerTask) -> Result<(), String> {
    let job_dir = options.run_dir.join("worker_jobs");
    let result_dir = options.run_dir.join("worker_results");
    ensure_dir(&job_dir)?;
    ensure_dir(&result_dir)?;
    let job_path = job_dir.join(format!("{}.json", task.job_id));
    let result_path = result_dir.join(format!("{}.json", task.job_id));
    let job = worker_job_value(options, task);
    write_text(&job_path, &(job.to_string() + "\n"))?;
    if let Err(err) = run_worker_task(options, task, &job_path, &result_path)
        && !result_path.exists()
    {
        write_worker_failure_result(&result_path, task, &err)?;
    }
    let result = read_worker_result_or_failure(&result_path, task)?;
    match worker_result_stage_error(&result, task) {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

fn worker_job_value(options: &ScienceOptions, task: &WorkerTask) -> Value {
    json!({
        "schema_version": WORKER_SCHEMA_VERSION,
        "pipeline_mode": WORKER_PIPELINE_MODE,
        "job_id": task.job_id,
        "worker": task.worker,
        "stage": task.stage,
        "rust_owned_decision_logic": true,
        "decision_owner": "GenoLigand Rust core",
        "input_manifest": options.input_manifest.display().to_string(),
        "run_dir": options.run_dir.display().to_string(),
        "science_store": options.science_store.display().to_string(),
        "expected_result_schema": WORKER_SCHEMA_VERSION
    })
}

fn validate_worker_job(value: &Value, task: &WorkerTask) -> bool {
    value.get("schema_version").and_then(Value::as_str) == Some(WORKER_SCHEMA_VERSION)
        && value.get("pipeline_mode").and_then(Value::as_str) == Some(WORKER_PIPELINE_MODE)
        && value.get("job_id").and_then(Value::as_str) == Some(task.job_id)
        && value.get("worker").and_then(Value::as_str) == Some(task.worker)
        && value.get("stage").and_then(Value::as_str) == Some(task.stage)
        && value
            .get("rust_owned_decision_logic")
            .and_then(Value::as_bool)
            == Some(true)
        && value.get("expected_result_schema").and_then(Value::as_str)
            == Some(WORKER_SCHEMA_VERSION)
}

fn run_worker_task(
    options: &ScienceOptions,
    task: &WorkerTask,
    job_path: &Path,
    result_path: &Path,
) -> Result<(), String> {
    let python_root = options.workspace_root.join("python");
    let mut errors = Vec::new();
    for candidate in python_command_candidates() {
        let mut command = Command::new(&candidate[0]);
        command.args(&candidate[1..]);
        command
            .arg("-m")
            .arg(task.module)
            .arg("--job")
            .arg(python_script_arg(
                &candidate,
                &job_path.display().to_string(),
            ))
            .arg("--out")
            .arg(python_script_arg(
                &candidate,
                &result_path.display().to_string(),
            ));
        if python_root.exists()
            && let Some(joined) = worker_pythonpath_env(&candidate, &python_root)
        {
            command.env("PYTHONPATH", joined);
            if is_wsl_python_wrapper(&candidate) {
                command.env("WSLENV", wsl_env_with_pythonpath());
            }
        }
        match command.output() {
            Ok(output) if output.status.success() => return Ok(()),
            Ok(output) => {
                if worker_result_file_is_valid(result_path, task) {
                    return Ok(());
                }
                let stderr = String::from_utf8_lossy(&output.stderr);
                let stdout = String::from_utf8_lossy(&output.stdout);
                let detail = stderr
                    .lines()
                    .find(|line| !line.trim().is_empty())
                    .or_else(|| stdout.lines().find(|line| !line.trim().is_empty()))
                    .unwrap_or("worker_exit_nonzero");
                errors.push(format!("{}:{detail}", candidate.join(" ")));
            }
            Err(err) => errors.push(format!("{}:{err}", candidate.join(" "))),
        }
    }
    Err(format!(
        "{}_launch_failed:{}",
        task.worker,
        clean_cell(&errors.join(" | "))
    ))
}

fn worker_result_file_is_valid(path: &Path, task: &WorkerTask) -> bool {
    read_text(path)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}')).ok())
        .is_some_and(|value| validate_worker_result(&value, task))
}

fn worker_pythonpath_env(candidate: &[String], python_root: &Path) -> Option<OsString> {
    let mut paths = vec![python_root.to_path_buf()];
    if let Some(existing) = std::env::var_os("PYTHONPATH") {
        paths.extend(std::env::split_paths(&existing));
    }
    worker_pythonpath_env_from_paths(candidate, paths)
}

fn worker_pythonpath_env_from_paths(candidate: &[String], paths: Vec<PathBuf>) -> Option<OsString> {
    if is_wsl_python_wrapper(candidate) {
        let converted = paths
            .into_iter()
            .map(|path| {
                let value = path.display().to_string();
                windows_path_to_wsl(&value).unwrap_or(value)
            })
            .collect::<Vec<_>>();
        return Some(OsString::from(converted.join(":")));
    }
    std::env::join_paths(paths).ok()
}

fn wsl_env_with_pythonpath() -> OsString {
    let mut entries = std::env::var("WSLENV")
        .ok()
        .map(|value| {
            value
                .split(':')
                .filter(|entry| !entry.trim().is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if !entries
        .iter()
        .any(|entry| entry.split('/').next() == Some("PYTHONPATH"))
    {
        entries.push("PYTHONPATH/u".to_string());
    }
    OsString::from(entries.join(":"))
}

fn read_worker_result_or_failure(path: &Path, task: &WorkerTask) -> Result<Value, String> {
    match read_text(path)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}')).ok())
    {
        Some(value) => Ok(value),
        None => {
            write_worker_failure_result(path, task, "worker_result_parse_failed")?;
            serde_json::from_str::<Value>(&read_text(path)?)
                .map_err(|err| format!("worker_result_reparse_failed:{}:{err}", path.display()))
        }
    }
}

fn write_worker_failure_result(path: &Path, task: &WorkerTask, reason: &str) -> Result<(), String> {
    let reason = clean_cell(reason);
    let value = json!({
        "schema_version": WORKER_SCHEMA_VERSION,
        "job_id": task.job_id,
        "worker": task.worker,
        "status": "FAIL",
        "engine_version": format!("worker unavailable: {reason}"),
        "execution_status": "failed",
        "reason": reason,
        "outcomes": [{
            "status": "failed",
            "reason": reason,
            "required": true,
            "database_id": task.worker,
            "row_count": 0
        }],
        "python_owns_final_score": false,
        "python_owns_report_semantics": false,
        "primitive_execution_only": true
    });
    write_text(path, &(value.to_string() + "\n"))
}

fn validate_worker_result(value: &Value, task: &WorkerTask) -> bool {
    let result_status = value.get("status").and_then(Value::as_str);
    let execution_status = value.get("execution_status").and_then(Value::as_str);
    let outcomes = value.get("outcomes").and_then(Value::as_array);
    let status_matches = matches!(
        (result_status, execution_status),
        (Some("PASS"), Some("completed"))
            | (Some("NOT_APPLICABLE"), Some("not_applicable"))
            | (Some("NOT_READY"), Some("not_ready"))
            | (Some("FAIL"), Some("failed"))
    );
    let outcomes_valid = outcomes.is_some_and(|rows| {
        !rows.is_empty()
            && rows.iter().all(validate_worker_outcome)
            && match execution_status {
                Some("completed") => rows.iter().all(|row| {
                    row.get("required").and_then(Value::as_bool) == Some(false)
                        || row.get("status").and_then(Value::as_str) == Some("completed")
                }),
                Some("not_applicable") => rows.iter().all(|row| {
                    row.get("required").and_then(Value::as_bool) == Some(false)
                        && row.get("status").and_then(Value::as_str) == Some("not_applicable")
                }),
                Some("not_ready") => rows.iter().any(|row| {
                    row.get("required").and_then(Value::as_bool) == Some(true)
                        && row.get("status").and_then(Value::as_str) == Some("not_ready")
                }),
                Some("failed") => rows.iter().any(|row| {
                    row.get("required").and_then(Value::as_bool) == Some(true)
                        && row.get("status").and_then(Value::as_str) == Some("failed")
                }),
                _ => false,
            }
    });
    value.get("schema_version").and_then(Value::as_str) == Some(WORKER_SCHEMA_VERSION)
        && value.get("job_id").and_then(Value::as_str) == Some(task.job_id)
        && value.get("worker").and_then(Value::as_str) == Some(task.worker)
        && status_matches
        && value
            .get("engine_version")
            .and_then(Value::as_str)
            .is_some_and(|text| !text.trim().is_empty())
        && value
            .get("reason")
            .and_then(Value::as_str)
            .is_some_and(|text| !text.trim().is_empty())
        && outcomes_valid
        && value
            .get("python_owns_final_score")
            .and_then(Value::as_bool)
            == Some(false)
        && value
            .get("python_owns_report_semantics")
            .and_then(Value::as_bool)
            == Some(false)
        && value
            .get("primitive_execution_only")
            .and_then(Value::as_bool)
            == Some(true)
}

fn validate_worker_outcome(value: &Value) -> bool {
    matches!(
        value.get("status").and_then(Value::as_str),
        Some("completed") | Some("not_applicable") | Some("not_ready") | Some("failed")
    ) && value
        .get("reason")
        .and_then(Value::as_str)
        .is_some_and(|text| !text.trim().is_empty())
        && value.get("required").and_then(Value::as_bool).is_some()
        && value
            .get("database_id")
            .and_then(Value::as_str)
            .is_some_and(|text| !text.trim().is_empty())
        && value.get("row_count").and_then(Value::as_u64).is_some()
}

fn worker_result_stage_error(value: &Value, task: &WorkerTask) -> Option<String> {
    if !validate_worker_result(value, task) {
        return Some(format!("{}_result_invalid", task.worker));
    }
    match value.get("status").and_then(Value::as_str) {
        Some("PASS") | Some("NOT_APPLICABLE") => None,
        Some(status) => Some(format!(
            "{}_{}:{}",
            task.worker,
            status.to_ascii_lowercase(),
            clean_cell(
                value
                    .get("reason")
                    .and_then(Value::as_str)
                    .unwrap_or("missing_reason")
            )
        )),
        None => Some(format!("{}_result_status_missing", task.worker)),
    }
}

fn match_structure_interactions(options: &ScienceOptions) -> Result<Vec<IndexMatch>, String> {
    let mut inputs = read_computed_feature_inputs(&options.run_dir.join("feature_manifest.tsv"))?;
    enrich_feature_inputs_from_known_matches(
        &mut inputs,
        &options.run_dir.join("known_database_matches.tsv"),
    )?;
    let rows = read_store_rows(
        &options.science_store.join("indexes/structure_targets.tsv"),
        "structure_interaction_index",
    )?;
    let mut matches = Vec::new();
    let mut out = String::from(
        "input_id\tsource\ttarget_id\ttarget_name\torganism\ttarget_class\tmatch_score\tmatched_features\tevidence\trecord_id\n",
    );
    for input in &inputs {
        for row in &rows {
            let score = score_row(input, row);
            if score <= 0.0 {
                continue;
            }
            let matched_features = matched_tokens(input, row).join(" ");
            let item = IndexMatch {
                input_id: input.input_id.clone(),
                record_id: input.record_id.clone(),
                source: row.source.clone(),
                target_id: row.target_id.clone(),
                target_name: row.target_name.clone(),
                organism: row.organism.clone(),
                target_class: row.target_class.clone(),
                match_score: score,
                matched_features,
                evidence: row.evidence.clone(),
            };
            out.push_str(&format!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{:.4}\t{}\t{}\t{}\n",
                item.input_id,
                clean_cell(&item.source),
                clean_cell(&item.target_id),
                clean_cell(&item.target_name),
                clean_cell(&item.organism),
                clean_cell(&item.target_class),
                item.match_score,
                clean_cell(&item.matched_features),
                clean_cell(&item.evidence),
                clean_cell(&item.record_id),
            ));
            matches.push(item);
        }
    }
    write_text(
        &options.run_dir.join("structure_interaction_matches.tsv"),
        &out,
    )?;
    Ok(matches)
}

fn enrich_feature_inputs_from_known_matches(
    inputs: &mut [ComputedFeatureInput],
    path: &Path,
) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    let mut by_input = inputs
        .iter_mut()
        .map(|input| ((input.input_id.clone(), input.record_id.clone()), input))
        .collect::<BTreeMap<_, _>>();
    let text = read_text(path)?;
    let mut lines = text.lines();
    let headers = lines.next().map(split_tsv).unwrap_or_default();
    let record_idx = find_header(&headers, &["record_id"]);
    for line in lines {
        let cols = split_tsv(line);
        if cols.len() < 9 {
            continue;
        }
        let key = (
            cols[0].clone(),
            first_nonempty(&[cell(&cols, record_idx), "record_1".to_string()]),
        );
        if let Some(input) = by_input.get_mut(&key) {
            for token in tokenize(&format!("{} {} {}", cols[3], cols[7], cols[8])) {
                input.tokens.insert(token);
            }
        }
    }
    Ok(())
}

fn write_structure_summary_from_matches(path: &Path, matches: &[IndexMatch]) -> Result<(), String> {
    let mut text = String::from(
        "candidate_id\tstructure_score\tstructure_evidence\tbinding_patch\tpatch_evidence\tkey_residues\tdocking_score\tdocking_pose\tligand_position\tinteraction_residues\n",
    );
    let mut by_target = BTreeMap::<String, &IndexMatch>::new();
    for item in matches {
        let key = item.target_id.clone();
        match by_target.get(&key) {
            Some(existing) if existing.match_score >= item.match_score => {}
            _ => {
                by_target.insert(key, item);
            }
        }
    }
    for item in by_target.values() {
        text.push_str(&format!(
            "target_{}\t{:.4}\t{}\t{}\t{}\t{}\t\t\t\t{}\n",
            safe_id(&item.target_id),
            item.match_score,
            clean_cell(&format!("{} {}", item.source, item.evidence)),
            "structure_index_match",
            clean_cell(&item.target_class),
            clean_cell(&item.matched_features),
            clean_cell(&item.matched_features)
        ));
    }
    write_text(path, &text)
}

fn copy_structure_viewer_artifact_if_available(options: &ScienceOptions) -> Result<bool, String> {
    for rel in [
        "structure_viewer.json",
        "structure_viewer.pdb",
        "structure_viewer.cif",
        "structure_viewer.mmcif",
        "structure_viewer.pdb.gz",
        "structure_viewer.cif.gz",
        "structure_viewer.mmcif.gz",
    ] {
        let source = options.run_dir.join("tool_outputs").join(rel);
        if !source.exists() {
            continue;
        }
        if let Some(artifact) = write_coordinate_viewer_artifact(&options.run_dir, &source) {
            write_structure_viewer_status(
                options,
                "ready",
                "",
                "",
                &artifact,
                &relative_display(&options.run_dir, &source),
                "worker coordinate artifact copied from current run output",
            )?;
            return Ok(true);
        }
    }
    Ok(false)
}

fn write_structure_viewer_from_known_hints(options: &ScienceOptions) -> Result<bool, String> {
    let hints = known_structure_hints(&options.run_dir.join("known_database_matches.tsv"))?;
    let mut candidates = Vec::new();
    for hint in hints {
        for pdb_id in &hint.pdb_ids {
            if let Some(path) = find_structure_coordinate_file(&options.science_store, pdb_id) {
                let rank = known_hint_rank(&hint);
                let score = coordinate_hint_score(&path, &hint);
                candidates.push((rank, score, hint.clone(), pdb_id.clone(), path));
            }
        }
    }
    candidates.sort_by(|left, right| {
        right
            .0
            .cmp(&left.0)
            .then_with(|| right.1.total_cmp(&left.1))
            .then_with(|| right.2.match_score.total_cmp(&left.2.match_score))
            .then_with(|| left.2.target_id.cmp(&right.2.target_id))
            .then_with(|| left.3.cmp(&right.3))
    });
    for (_, _, hint, pdb_id, path) in candidates {
        if let Some(artifact) = write_coordinate_viewer_artifact(&options.run_dir, &path) {
            write_structure_viewer_status(
                options,
                "ready",
                &hint.target_id,
                &pdb_id,
                &artifact,
                &relative_display(&options.science_store, &path),
                "coordinate artifact resolved from BindingDB ligand-target PDB evidence",
            )?;
            write_selected_complex_candidate(
                options,
                &SelectedComplexCandidate {
                    candidate_id: format!("target_{}", safe_id(&hint.target_id)),
                    input_id: hint.input_id,
                    target_id: hint.target_id,
                    target_name: hint.target_name,
                    organism: hint.organism,
                    target_class: hint.target_class,
                    source: "BindingDB".to_string(),
                    pdb_id,
                    evidence: hint.evidence,
                },
            )?;
            return Ok(true);
        }
    }
    Ok(false)
}

fn known_structure_hints(path: &Path) -> Result<Vec<KnownStructureHint>, String> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = read_text(path)?;
    let mut by_target = BTreeMap::<String, KnownStructureHint>::new();
    for line in text.lines().skip(1) {
        let cols = split_tsv(line);
        if cols.len() < 9 || !cols[8].contains("bindingdb_onsite_activity_row") {
            continue;
        }
        let pdb_ids = bindingdb_complex_pdb_ids(&format!("{};{}", cols[7], cols[8]));
        if pdb_ids.is_empty() {
            continue;
        }
        let hint = KnownStructureHint {
            input_id: cols[0].clone(),
            target_id: cols[2].clone(),
            target_name: cols[3].clone(),
            organism: cols[4].clone(),
            target_class: cols[5].clone(),
            match_score: cols[6].parse().unwrap_or(0.0),
            evidence: cols[8].clone(),
            pdb_ids,
        };
        let key = format!("{}\t{}", hint.input_id, hint.target_id);
        match by_target.get(&key) {
            Some(existing) if known_hint_rank(existing) >= known_hint_rank(&hint) => {}
            _ => {
                by_target.insert(key, hint);
            }
        }
    }
    Ok(by_target.into_values().collect())
}
fn write_structure_viewer_from_matches(
    options: &ScienceOptions,
    matches: &[IndexMatch],
) -> Result<bool, String> {
    let mut ordered = matches.iter().collect::<Vec<_>>();
    ordered.sort_by(|left, right| right.match_score.total_cmp(&left.match_score));
    let mut first_target = String::new();
    let mut first_pdb_id = String::new();
    let mut first_expected_path = String::new();
    for item in ordered {
        if first_target.is_empty() {
            first_target = item.target_id.clone();
        }
        for pdb_id in structure_coordinate_ids(item) {
            if first_pdb_id.is_empty() {
                first_pdb_id = pdb_id.clone();
                first_expected_path =
                    expected_structure_coordinate_paths(&options.science_store, &pdb_id);
            }
            if let Some(path) = find_structure_coordinate_file(&options.science_store, &pdb_id)
                && let Some(artifact) = write_coordinate_viewer_artifact(&options.run_dir, &path)
            {
                write_structure_viewer_status(
                    options,
                    "ready",
                    &item.target_id,
                    &pdb_id,
                    &artifact,
                    &relative_display(&options.science_store, &path),
                    "coordinate artifact resolved from local structure database",
                )?;
                write_selected_complex_candidate(
                    options,
                    &SelectedComplexCandidate {
                        candidate_id: format!("target_{}", safe_id(&item.target_id)),
                        input_id: item.input_id.clone(),
                        target_id: item.target_id.clone(),
                        target_name: item.target_name.clone(),
                        organism: item.organism.clone(),
                        target_class: item.target_class.clone(),
                        source: item.source.clone(),
                        pdb_id,
                        evidence: item.evidence.clone(),
                    },
                )?;
                return Ok(true);
            }
        }
    }
    let reason = if first_target.is_empty() {
        "no structure interaction match rows for this run"
    } else {
        "matched structure rows did not have a local coordinate artifact"
    };
    write_structure_viewer_status(
        options,
        "not_ready",
        &first_target,
        &first_pdb_id,
        "",
        &first_expected_path,
        reason,
    )?;
    Ok(false)
}

fn bindingdb_complex_pdb_ids(value: &str) -> Vec<String> {
    let mut ids = BTreeSet::new();
    for part in value.split(';') {
        let Some((key, raw_value)) = part.split_once('=') else {
            continue;
        };
        if key.trim() != "pdb_ids" {
            continue;
        }
        ids.extend(pdb_ids_from_text(raw_value));
    }
    ids.into_iter().collect()
}

fn known_hint_rank(hint: &KnownStructureHint) -> i64 {
    let activity = activity_nm_from_evidence(&hint.evidence).unwrap_or(1_000_000.0);
    let pdb_bonus = if hint.pdb_ids.is_empty() { 0 } else { 1000 };
    let human_bonus = if hint.organism.eq_ignore_ascii_case("Homo sapiens") {
        100
    } else {
        0
    };
    pdb_bonus + human_bonus + (1000.0 - activity.min(1000.0)).round() as i64
}

fn activity_nm_from_evidence(value: &str) -> Option<f64> {
    for marker in ["Ki (nM)=", "IC50 (nM)=", "Kd (nM)=", "EC50 (nM)="] {
        if let Some(index) = value.find(marker) {
            let tail = &value[index + marker.len()..];
            let number = tail
                .chars()
                .skip_while(|ch| matches!(ch, '<' | '>' | '=' | '~' | ' '))
                .take_while(|ch| ch.is_ascii_digit() || matches!(*ch, '.' | 'e' | 'E' | '+' | '-'))
                .collect::<String>();
            if let Ok(parsed) = number.parse::<f64>() {
                return Some(parsed);
            }
        }
    }
    None
}

fn coordinate_hint_score(path: &Path, hint: &KnownStructureHint) -> f64 {
    let mut score = hint.match_score;
    if let Ok(text) = read_structure_text_best_effort(path) {
        let metadata = structure_metadata_text(&text);
        let metadata_tokens = tokens_with_numeric_aliases(&metadata);
        let target_tokens =
            tokens_with_numeric_aliases(&format!("{} {}", hint.target_name, hint.organism));
        let matched = target_tokens
            .intersection(&metadata_tokens)
            .filter(|token| token.len() > 1)
            .count();
        score += matched as f64 * 0.25;
    }
    score
}

fn tokens_with_numeric_aliases(value: &str) -> BTreeSet<String> {
    let mut tokens = tokenize(value);
    for token in tokenize(value) {
        if let Some(alias) = numeric_alias(&token) {
            tokens.insert(alias.to_string());
        }
    }
    tokens
}

fn numeric_alias(value: &str) -> Option<&'static str> {
    match value {
        "1" => Some("i"),
        "2" => Some("ii"),
        "3" => Some("iii"),
        "4" => Some("iv"),
        "5" => Some("v"),
        "6" => Some("vi"),
        "7" => Some("vii"),
        "8" => Some("viii"),
        "9" => Some("ix"),
        "10" => Some("x"),
        "11" => Some("xi"),
        "12" => Some("xii"),
        _ => None,
    }
}

fn pdb_ids_from_text(value: &str) -> Vec<String> {
    let mut ids = BTreeSet::new();
    for token in value.split(|ch: char| !ch.is_ascii_alphanumeric()) {
        let upper = token.trim().to_ascii_uppercase();
        if is_pdb_id(&upper) {
            ids.insert(upper);
        }
    }
    ids.into_iter().collect()
}

fn pdb_ids_from_text_in_order(value: &str) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut ids = Vec::new();
    for token in value.split(|ch: char| !ch.is_ascii_alphanumeric()) {
        let upper = token.trim().to_ascii_uppercase();
        if is_pdb_id(&upper) && seen.insert(upper.clone()) {
            ids.push(upper);
        }
    }
    ids
}

fn write_selected_complex_candidate(
    options: &ScienceOptions,
    selected: &SelectedComplexCandidate,
) -> Result<(), String> {
    write_text(
        &options.run_dir.join("complex_candidate_selection.tsv"),
        &format!(
            "candidate_id\tinput_id\ttarget_id\ttarget_name\torganism\ttarget_class\tsource\tpdb_id\tevidence\n{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            clean_cell(&selected.candidate_id),
            clean_cell(&selected.input_id),
            clean_cell(&selected.target_id),
            clean_cell(&selected.target_name),
            clean_cell(&selected.organism),
            clean_cell(&selected.target_class),
            clean_cell(&selected.source),
            clean_cell(&selected.pdb_id),
            clean_cell(&selected.evidence)
        ),
    )
}

fn append_selected_structure_match(
    match_path: &Path,
    selected_path: &Path,
) -> Result<bool, String> {
    if !match_path.exists() || !selected_path.exists() {
        return Ok(false);
    }
    let selected = read_text(selected_path)?;
    let Some(line) = selected.lines().nth(1) else {
        return Ok(false);
    };
    let cols = split_tsv(line);
    if cols.len() < 9 {
        return Ok(false);
    }
    let mut text = read_text(match_path)?;
    if text.contains(&format!("\t{}\t", cols[2])) {
        return Ok(false);
    }
    text.push_str(&format!(
        "{}\t{}\t{}\t{}\t{}\t{}\t1.0000\t{}\t{}\n",
        clean_cell(&cols[1]),
        clean_cell(&cols[6]),
        clean_cell(&cols[2]),
        clean_cell(&cols[3]),
        clean_cell(&cols[4]),
        clean_cell(&cols[5]),
        clean_cell(&format!("bindingdb_pdb_coordinate pdb_id={}", cols[7])),
        clean_cell(&cols[8])
    ));
    write_text(match_path, &text)?;
    Ok(true)
}

fn append_selected_structure_summary(
    selection_path: &Path,
    selected_path: &Path,
) -> Result<bool, String> {
    if !selection_path.exists() || !selected_path.exists() {
        return Ok(false);
    }
    let selected = read_text(selected_path)?;
    let Some(line) = selected.lines().nth(1) else {
        return Ok(false);
    };
    let cols = split_tsv(line);
    if cols.len() < 9 {
        return Ok(false);
    }
    let mut text = read_text(selection_path)?;
    if text.contains(&format!("{}\t", cols[0])) {
        return Ok(false);
    }
    text.push_str(&format!(
        "{}\t1.0000\t{}\t{}\t{}\t{}\t\t\t\t{}\n",
        clean_cell(&cols[0]),
        clean_cell(&format!("{} pdb_id={}", cols[6], cols[7])),
        "bindingdb_pdb_coordinate",
        clean_cell(&cols[5]),
        clean_cell(&cols[8]),
        clean_cell(&cols[8])
    ));
    write_text(selection_path, &text)?;
    Ok(true)
}

fn write_structure_viewer_status(
    options: &ScienceOptions,
    status: &str,
    target_id: &str,
    pdb_id: &str,
    artifact: &str,
    expected_path: &str,
    reason: &str,
) -> Result<(), String> {
    write_text(
        &options.run_dir.join("structure_viewer_status.tsv"),
        &format!(
            "status\ttarget_id\tpdb_id\tartifact\texpected_path\treason\n{}\t{}\t{}\t{}\t{}\t{}\n",
            clean_cell(status),
            clean_cell(target_id),
            clean_cell(pdb_id),
            clean_cell(artifact),
            clean_cell(expected_path),
            clean_cell(reason)
        ),
    )
}

fn structure_coordinate_ids(item: &IndexMatch) -> Vec<String> {
    if item.evidence.contains("bindingdb_onsite_activity_row") {
        return bindingdb_complex_pdb_ids(&item.evidence);
    }
    let mut text = String::new();
    for value in [
        item.target_id.as_str(),
        item.target_name.as_str(),
        item.matched_features.as_str(),
        item.evidence.as_str(),
    ] {
        text.push(' ');
        text.push_str(value);
    }
    pdb_ids_from_text(&text)
}

fn is_pdb_id(value: &str) -> bool {
    value.len() == 4
        && value.chars().next().is_some_and(|ch| ch.is_ascii_digit())
        && value.chars().all(|ch| ch.is_ascii_alphanumeric())
}

fn find_structure_coordinate_file(science_store: &Path, pdb_id: &str) -> Option<PathBuf> {
    structure_coordinate_candidate_paths(science_store, pdb_id)
        .into_iter()
        .find(|path| path.exists())
}

fn expected_structure_coordinate_paths(science_store: &Path, pdb_id: &str) -> String {
    structure_coordinate_candidate_paths(science_store, pdb_id)
        .into_iter()
        .take(4)
        .map(|path| relative_display(science_store, &path))
        .collect::<Vec<_>>()
        .join(";")
}

fn structure_coordinate_candidate_paths(science_store: &Path, pdb_id: &str) -> Vec<PathBuf> {
    let lower = pdb_id.to_ascii_lowercase();
    let split = if lower.len() >= 3 { &lower[1..3] } else { "" };
    let root = science_store.join("downloads/rcsb/mmcif");
    let mut candidates = Vec::new();
    for name in [
        format!("{lower}.cif.gz"),
        format!("{lower}.mmcif.gz"),
        format!("{lower}.pdb.gz"),
        format!("pdb{lower}.ent.gz"),
        format!("{lower}.cif"),
        format!("{lower}.mmcif"),
        format!("{lower}.pdb"),
        format!("pdb{lower}.ent"),
    ] {
        if !split.is_empty() {
            candidates.push(root.join(split).join(&name));
        }
        candidates.push(root.join(&name));
    }
    candidates
}

fn write_coordinate_viewer_artifact(run_dir: &Path, source: &Path) -> Option<String> {
    let text = read_maybe_gz_text(source).ok()?;
    let lower = lower_file_name(source);
    let artifact = if lower.contains(".json") {
        if !text.contains("\"atoms\"") {
            return None;
        }
        "structure_viewer.json"
    } else if lower.contains(".pdb") || lower.contains(".ent") {
        if !coordinate_text_has_atoms(&text) {
            return None;
        }
        "structure_viewer.pdb"
    } else {
        if !coordinate_text_has_atoms(&text) {
            return None;
        }
        "structure_viewer.cif"
    };
    write_text(&run_dir.join(artifact), &text).ok()?;
    Some(artifact.to_string())
}

fn read_maybe_gz_text(path: &Path) -> Result<String, String> {
    let name = lower_file_name(path);
    if name.ends_with(".gz") {
        let file =
            fs::File::open(path).map_err(|err| format!("open_failed:{}:{err}", path.display()))?;
        let mut decoder = GzDecoder::new(file);
        let mut text = String::new();
        decoder
            .read_to_string(&mut text)
            .map_err(|err| format!("gzip_read_failed:{}:{err}", path.display()))?;
        return Ok(text);
    }
    read_text(path)
}

fn coordinate_text_has_atoms(text: &str) -> bool {
    text.lines()
        .any(|line| line.starts_with("ATOM") || line.starts_with("HETATM"))
}

fn screen_dynamics(options: &ScienceOptions) -> Result<String, String> {
    let md_path = options.run_dir.join("md_summary.tsv");
    let ready = copy_tool_output_if_available(options, "md_summary.tsv", &md_path)?
        || write_md_summary_from_metric_artifacts(options, &md_path)?;
    if ready {
        write_md_summary_status(
            options,
            "ready",
            "real MD summary artifact available for this run",
        )?;
    } else {
        write_text(&md_path, md_summary_header())?;
        write_md_summary_status(options, "not_ready", &md_not_ready_reason(options))?;
    }
    Ok(format!(
        "dynamics_screened\tmd_artifact={}",
        if ready { "ready" } else { "not_ready" }
    ))
}

fn md_summary_header() -> &'static str {
    "candidate_id\trmsd\trmsf\tligand_drift\thydrogen_bond_occupancy\tcontact_occupancy\tinteraction_persistence\tmd_risk_flags\tmd_score_decomposition\tmd_stability_score\n"
}

fn write_md_summary_from_metric_artifacts(
    options: &ScienceOptions,
    out_path: &Path,
) -> Result<bool, String> {
    let mut rows = BTreeMap::<String, BTreeMap<String, String>>::new();
    for (metric, files) in [
        ("rmsd", &["md_rmsd.tsv", "rmsd.tsv"][..]),
        ("rmsf", &["md_rmsf.tsv", "rmsf.tsv"][..]),
        (
            "ligand_drift",
            &["md_ligand_drift.tsv", "ligand_drift.tsv"][..],
        ),
        (
            "hydrogen_bond_occupancy",
            &["md_hbonds.tsv", "hydrogen_bonds.tsv", "hbond_occupancy.tsv"][..],
        ),
        (
            "contact_occupancy",
            &["md_contacts.tsv", "contacts.tsv", "contact_occupancy.tsv"][..],
        ),
    ] {
        for (candidate, value) in read_metric_values(options, files)? {
            rows.entry(candidate)
                .or_default()
                .insert(metric.to_string(), value);
        }
    }
    if rows.is_empty() {
        return Ok(false);
    }
    let mut text = String::from(md_summary_header());
    for (candidate, metrics) in rows {
        let rmsd = metrics.get("rmsd").cloned().unwrap_or_default();
        let rmsf = metrics.get("rmsf").cloned().unwrap_or_default();
        let drift = metrics.get("ligand_drift").cloned().unwrap_or_default();
        let hbond = metrics
            .get("hydrogen_bond_occupancy")
            .cloned()
            .unwrap_or_default();
        let contact = metrics
            .get("contact_occupancy")
            .cloned()
            .unwrap_or_default();
        text.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t\t\tmetric_artifacts\t\n",
            clean_cell(&candidate),
            clean_cell(&rmsd),
            clean_cell(&rmsf),
            clean_cell(&drift),
            clean_cell(&hbond),
            clean_cell(&contact)
        ));
    }
    write_text(out_path, &text)?;
    Ok(true)
}

fn read_metric_values(
    options: &ScienceOptions,
    file_names: &[&str],
) -> Result<BTreeMap<String, String>, String> {
    let mut values = BTreeMap::new();
    for file_name in file_names {
        let path = options.run_dir.join("tool_outputs").join(file_name);
        if !path.exists() {
            continue;
        }
        for row in read_simple_tsv_rows(&path)? {
            let candidate =
                first_metric_value(&row, &["candidate_id", "candidate", "target_id", "id"])
                    .unwrap_or("current_run");
            let value =
                first_metric_value(&row, &["value", "mean", "average", "summary", "occupancy"])
                    .or_else(|| {
                        row.iter()
                            .find(|(key, _)| {
                                !matches!(
                                    key.as_str(),
                                    "candidate_id" | "candidate" | "target_id" | "id" | "time"
                                )
                            })
                            .map(|(_, value)| value.as_str())
                    })
                    .unwrap_or("");
            if !candidate.trim().is_empty() && !value.trim().is_empty() {
                values.insert(candidate.to_string(), value.to_string());
            }
        }
    }
    Ok(values)
}

fn read_simple_tsv_rows(path: &Path) -> Result<Vec<BTreeMap<String, String>>, String> {
    let text = read_text(path)?;
    let mut lines = text.lines();
    let Some(header) = lines.next() else {
        return Ok(Vec::new());
    };
    let headers = split_tsv(header.trim_start_matches('\u{feff}'));
    let mut rows = Vec::new();
    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        let cols = split_tsv(line);
        let mut row = BTreeMap::new();
        for (index, header) in headers.iter().enumerate() {
            row.insert(
                header.to_string(),
                cols.get(index).cloned().unwrap_or_default(),
            );
        }
        rows.push(row);
    }
    Ok(rows)
}

fn first_metric_value<'a>(row: &'a BTreeMap<String, String>, keys: &[&str]) -> Option<&'a str> {
    keys.iter().find_map(|key| {
        row.get(*key)
            .map(String::as_str)
            .filter(|value| !value.trim().is_empty())
    })
}

fn write_md_summary_status(
    options: &ScienceOptions,
    status: &str,
    reason: &str,
) -> Result<(), String> {
    write_text(
        &options.run_dir.join("md_summary_status.tsv"),
        &format!(
            "status\treason\n{}\t{}\n",
            clean_cell(status),
            clean_cell(reason)
        ),
    )
}

fn md_not_ready_reason(options: &ScienceOptions) -> String {
    let mut reasons = Vec::new();
    if !options.run_dir.join("tool_outputs/md_summary.tsv").exists() {
        reasons.push("no md_summary.tsv in current run tool_outputs".to_string());
    }
    if options
        .run_dir
        .join("tool_outputs/md_not_ready.tsv")
        .exists()
        && let Ok(rows) =
            read_simple_tsv_rows(&options.run_dir.join("tool_outputs/md_not_ready.tsv"))
    {
        for row in rows {
            if let Some(reason) = first_metric_value(&row, &["reason", "message", "risk_flags"]) {
                reasons.push(reason.to_string());
            }
        }
    }
    if !options.run_dir.join("tool_outputs/md_rmsd.tsv").exists()
        && !options.run_dir.join("tool_outputs/rmsd.tsv").exists()
    {
        reasons.push("no RMSD metric artifact".to_string());
    }
    if !options.run_dir.join("tool_outputs/md_rmsf.tsv").exists()
        && !options.run_dir.join("tool_outputs/rmsf.tsv").exists()
    {
        reasons.push("no RMSF metric artifact".to_string());
    }
    if !options
        .run_dir
        .join("tool_outputs/md_ligand_drift.tsv")
        .exists()
        && !options
            .run_dir
            .join("tool_outputs/ligand_drift.tsv")
            .exists()
    {
        reasons.push("no ligand drift metric artifact".to_string());
    }
    if !options
        .run_dir
        .join("tool_outputs/md_contacts.tsv")
        .exists()
        && !options.run_dir.join("tool_outputs/contacts.tsv").exists()
    {
        reasons.push("no contact metric artifact".to_string());
    }
    if !options.run_dir.join("tool_outputs/md_hbonds.tsv").exists()
        && !options
            .run_dir
            .join("tool_outputs/hydrogen_bonds.tsv")
            .exists()
    {
        reasons.push("no hydrogen-bond metric artifact".to_string());
    }
    reasons.join(";")
}

fn copy_tool_output_if_available(
    options: &ScienceOptions,
    file_name: &str,
    out_path: &Path,
) -> Result<bool, String> {
    let tool_output = options.run_dir.join("tool_outputs").join(file_name);
    if tool_output.exists() {
        let text = read_text(&tool_output)?;
        write_text(out_path, &text)?;
        return Ok(text.lines().skip(1).any(|line| !line.trim().is_empty()));
    }
    Ok(false)
}

fn copy_tool_output_or_header(
    options: &ScienceOptions,
    file_name: &str,
    header: &str,
    out_path: &Path,
) -> Result<bool, String> {
    let tool_output = options.run_dir.join("tool_outputs").join(file_name);
    if tool_output.exists() {
        let text = read_text(&tool_output)?;
        write_text(out_path, &text)?;
        return Ok(text.lines().skip(1).any(|line| !line.trim().is_empty()));
    }
    write_text(out_path, header)?;
    Ok(false)
}

struct AuthenticityViolation {
    artifact: String,
    line_number: usize,
    marker: &'static str,
    reason: &'static str,
}

fn ensure_final_artifacts_authentic(
    options: &ScienceOptions,
    artifacts: &[&str],
) -> Result<(), String> {
    let violations = final_artifact_authenticity_violations(options, artifacts)?;
    write_authenticity_gate_report(options, &violations)?;
    if violations.is_empty() {
        return Ok(());
    }
    Err(format!(
        "authenticity_gate_failed:forbidden_final_artifact_rows={}",
        violations.len()
    ))
}

fn final_artifact_authenticity_violations(
    options: &ScienceOptions,
    artifacts: &[&str],
) -> Result<Vec<AuthenticityViolation>, String> {
    let mut violations = Vec::new();
    for artifact in artifacts {
        let path = options.run_dir.join(artifact);
        if !path.exists() || !path.is_file() {
            continue;
        }
        let text = read_text(&path)?;
        let skip_header = artifact.ends_with(".tsv");
        for (index, line) in text.lines().enumerate() {
            if skip_header && index == 0 {
                continue;
            }
            if line.trim().is_empty() {
                continue;
            }
            if let Some((marker, reason)) = authenticity_marker(line) {
                violations.push(AuthenticityViolation {
                    artifact: (*artifact).to_string(),
                    line_number: index + 1,
                    marker,
                    reason,
                });
            }
        }
    }
    Ok(violations)
}

fn authenticity_marker(line: &str) -> Option<(&'static str, &'static str)> {
    let lower = line.to_ascii_lowercase();
    AUTHENTICITY_FORBIDDEN_MARKERS
        .iter()
        .copied()
        .find(|(marker, _)| lower.contains(marker))
}

fn write_authenticity_gate_report(
    options: &ScienceOptions,
    violations: &[AuthenticityViolation],
) -> Result<(), String> {
    let mut text = String::from("artifact\tstatus\tline_number\tmarker\treason\n");
    if violations.is_empty() {
        text.push_str("final_run_artifacts\tpassed\t0\t\tno_forbidden_fixture_sample_smoke_static_fallback_demo_markers\n");
    } else {
        for violation in violations {
            text.push_str(&format!(
                "{}\tblocked\t{}\t{}\t{}\n",
                clean_cell(&violation.artifact),
                violation.line_number,
                clean_cell(violation.marker),
                clean_cell(violation.reason)
            ));
        }
    }
    write_text(&options.run_dir.join("authenticity_gate.tsv"), &text)
}

fn rank_targets(options: &ScienceOptions) -> Result<String, String> {
    ensure_final_artifacts_authentic(
        options,
        &[
            "known_database_matches.tsv",
            "metagenome_potential_matches.tsv",
            "structure_interaction_matches.tsv",
            "tool_outputs/mmseqs_alignment_matches.tsv",
            "tool_outputs/foldseek_structure_matches.tsv",
            "docking_summary.tsv",
            "md_summary.tsv",
        ],
    )?;
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
    merge_match_file(
        &options.run_dir.join("structure_interaction_matches.tsv"),
        "structure_interaction",
        &mut by_target,
    )?;
    let mut candidates = by_target.into_values().collect::<Vec<_>>();
    apply_docking_scores(
        &mut candidates,
        &options.run_dir.join("docking_summary.tsv"),
    )?;
    apply_md_scores(&mut candidates, options)?;
    for candidate in &mut candidates {
        candidate.final_score = (candidate.known_score * KNOWN_SCORE_WEIGHT
            + candidate.metagenome_score * METAGENOME_SCORE_WEIGHT
            + candidate.structure_score * STRUCTURE_SCORE_WEIGHT
            + candidate.docking_score * DOCKING_SCORE_WEIGHT
            + candidate.dynamics_score * MD_SCORE_WEIGHT)
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
    write_candidate_detail(options, &candidates)?;
    Ok(format!(
        "targets_ranked\tcandidate_count={}",
        candidates.len()
    ))
}

fn apply_docking_scores(candidates: &mut [Candidate], path: &Path) -> Result<(), String> {
    let docking_scores = read_docking_scores(path)?;
    if docking_scores.is_empty() {
        return Ok(());
    }
    for candidate in candidates {
        if let Some(score) = docking_scores.get(&candidate.candidate_id) {
            candidate.docking_score = *score;
            append_evidence_once(&mut candidate.evidence, "Docking:vina_completed");
        }
    }
    Ok(())
}

fn read_docking_scores(path: &Path) -> Result<BTreeMap<String, f64>, String> {
    if !path.exists() {
        return Ok(BTreeMap::new());
    }
    let run_dir = path.parent().unwrap_or_else(|| Path::new(""));
    let text = read_text(path)?;
    let mut lines = text.lines();
    let Some(header) = lines.next() else {
        return Ok(BTreeMap::new());
    };
    let headers = split_tsv(header);
    let candidate_idx = find_header(&headers, &["candidate_id"]);
    let status_idx = find_header(&headers, &["docking_status", "status"]);
    let score_idx = find_header(&headers, &["docking_score", "affinity", "binding_affinity"]);
    let engine_idx = find_header(&headers, &["docking_engine", "engine"]);
    let notes_idx = find_header(&headers, &["notes", "pose", "artifact"]);
    let mut scores = BTreeMap::new();
    for line in lines {
        let cols = split_tsv(line);
        let candidate_id = cell(&cols, candidate_idx);
        let status = cell(&cols, status_idx).to_ascii_lowercase();
        let engine = cell(&cols, engine_idx).to_ascii_lowercase();
        if candidate_id.is_empty() || status != "completed" || !engine.contains("vina") {
            continue;
        }
        if !docking_pose_artifact_ready(run_dir, &cell(&cols, notes_idx)) {
            continue;
        }
        if let Some(score) = docking_score_from_affinity(&cell(&cols, score_idx)) {
            scores.insert(candidate_id, score);
        }
    }
    Ok(scores)
}

fn docking_pose_artifact_ready(run_dir: &Path, notes: &str) -> bool {
    for key in ["pose", "pose_sdf", "docking_pose", "docking_pose_sdf"] {
        let value = evidence_field(notes, &[key]);
        if value.is_empty() {
            continue;
        }
        let path = Path::new(&value);
        let artifact = if path.is_absolute() {
            path.to_path_buf()
        } else {
            run_dir.join(path)
        };
        if artifact.exists() && artifact.is_file() {
            return true;
        }
    }
    false
}

fn docking_score_from_affinity(value: &str) -> Option<f64> {
    let raw = value.trim().parse::<f64>().ok()?;
    if !raw.is_finite() {
        return None;
    }
    if (0.0..=1.0).contains(&raw) {
        Some(clamp_score(raw))
    } else if raw < 0.0 {
        Some(clamp_score((-raw) / 12.0))
    } else {
        None
    }
}

fn apply_md_scores(candidates: &mut [Candidate], options: &ScienceOptions) -> Result<(), String> {
    let complex_candidates = complex_md_candidate_ids(options)?;
    if complex_candidates.is_empty() {
        return Ok(());
    }
    let md_scores = read_md_scores(&options.run_dir.join("md_summary.tsv"))?;
    if md_scores.is_empty() {
        return Ok(());
    }
    for candidate in candidates {
        if !complex_candidates.contains(&candidate.candidate_id) {
            continue;
        }
        if let Some(score) = md_scores.get(&candidate.candidate_id) {
            candidate.dynamics_score = *score;
            append_evidence_once(&mut candidate.evidence, "MD:md_summary_stability_score");
        }
    }
    Ok(())
}

fn complex_md_candidate_ids(options: &ScienceOptions) -> Result<BTreeSet<String>, String> {
    let mut ids = BTreeSet::new();
    for rel in [
        "tool_outputs/complex_md_manifest.json",
        "tool_outputs/md_run_manifest.json",
    ] {
        let path = options.run_dir.join(rel);
        if !path.exists() {
            continue;
        }
        let value = serde_json::from_str::<Value>(read_text(&path)?.trim_start_matches('\u{feff}'))
            .map_err(|err| format!("md_manifest_parse_failed:{}:{err}", path.display()))?;
        if !complex_md_manifest_value_ready(&value)
            || !complex_md_manifest_artifacts_ready(options, &value)
        {
            continue;
        }
        if let Some(candidate_id) = value.get("candidate_id").and_then(Value::as_str)
            && !candidate_id.trim().is_empty()
        {
            ids.insert(candidate_id.to_string());
        }
    }
    Ok(ids)
}

fn complex_md_manifest_value_ready(value: &Value) -> bool {
    if value.get("method").and_then(Value::as_str)
        != Some("openmm_receptor_ligand_complex_dynamics")
        || !nonempty_json_string(value, "receptor_coordinate")
        || !nonempty_json_string(value, "ligand_coordinate")
        || !nonempty_json_string(value, "trajectory")
        || !nonempty_json_string(value, "protocol_artifact")
        || !nonempty_json_string(value, "state_artifact")
    {
        return false;
    }
    if value.get("platform").and_then(Value::as_str) == Some("CUDA") {
        return value
            .get("gpu_observed")
            .and_then(Value::as_bool)
            .unwrap_or(false)
            && nonempty_json_string(value, "gpu_telemetry_artifact")
            && nonempty_json_string(value, "gpu_telemetry_status_artifact");
    }
    true
}

fn complex_md_manifest_artifacts_ready(options: &ScienceOptions, value: &Value) -> bool {
    for field in [
        "receptor_coordinate",
        "ligand_coordinate",
        "trajectory",
        "protocol_artifact",
        "state_artifact",
    ] {
        if !manifest_file_exists(options, value, field) {
            return false;
        }
    }
    if value.get("platform").and_then(Value::as_str) == Some("CUDA") {
        return manifest_file_exists(options, value, "gpu_telemetry_artifact")
            && manifest_file_exists(options, value, "gpu_telemetry_status_artifact");
    }
    true
}

fn manifest_file_exists(options: &ScienceOptions, value: &Value, field: &str) -> bool {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(|raw| {
            manifest_path_candidates(options, raw)
                .iter()
                .any(|artifact| artifact.exists() && artifact.is_file())
        })
        .unwrap_or(false)
}

fn manifest_path_candidates(options: &ScienceOptions, raw: &str) -> Vec<PathBuf> {
    let trimmed = raw.trim();
    let path = Path::new(trimmed);
    let mut candidates = vec![if path.is_absolute() {
        path.to_path_buf()
    } else {
        options.run_dir.join(path)
    }];
    if let Some(windows_path) = wsl_path_to_windows(trimmed)
        && !candidates.contains(&windows_path)
    {
        candidates.push(windows_path);
    }
    candidates
}

fn nonempty_json_string(value: &Value, key: &str) -> bool {
    value
        .get(key)
        .and_then(Value::as_str)
        .is_some_and(|text| !text.trim().is_empty())
}

fn read_md_scores(path: &Path) -> Result<BTreeMap<String, f64>, String> {
    if !path.exists() {
        return Ok(BTreeMap::new());
    }
    let text = read_text(path)?;
    let mut lines = text.lines();
    let Some(header) = lines.next() else {
        return Ok(BTreeMap::new());
    };
    let headers = split_tsv(header);
    let candidate_idx = find_header(&headers, &["candidate_id"]);
    let stability_idx = find_header(&headers, &["md_stability_score"]);
    let rmsd_idx = find_header(&headers, &["rmsd"]);
    let drift_idx = find_header(&headers, &["ligand_drift"]);
    let hbond_idx = find_header(&headers, &["hydrogen_bond_occupancy"]);
    let contact_idx = find_header(&headers, &["contact_occupancy"]);
    let persistence_idx = find_header(&headers, &["interaction_persistence"]);
    let mut scores = BTreeMap::new();
    for line in lines {
        let cols = split_tsv(line);
        let candidate_id = cell(&cols, candidate_idx);
        if candidate_id.is_empty() {
            continue;
        }
        if let Some(score) = md_score_from_row(
            &cols,
            stability_idx,
            rmsd_idx,
            drift_idx,
            hbond_idx,
            contact_idx,
            persistence_idx,
        ) {
            scores.insert(candidate_id, score);
        }
    }
    Ok(scores)
}

fn md_score_from_row(
    cols: &[String],
    stability_idx: Option<usize>,
    rmsd_idx: Option<usize>,
    drift_idx: Option<usize>,
    hbond_idx: Option<usize>,
    contact_idx: Option<usize>,
    persistence_idx: Option<usize>,
) -> Option<f64> {
    parse_score_cell(&cell(cols, stability_idx)).or_else(|| {
        let rmsd = parse_score_cell(&cell(cols, rmsd_idx))?;
        let drift = parse_score_cell(&cell(cols, drift_idx))?;
        let hbond = parse_score_cell(&cell(cols, hbond_idx)).unwrap_or(0.0);
        let contact = parse_score_cell(&cell(cols, contact_idx)).unwrap_or(0.0);
        let persistence = parse_score_cell(&cell(cols, persistence_idx)).unwrap_or(contact);
        let rmsd_component = 1.0 - (rmsd / 4.0).min(1.0);
        let drift_component = 1.0 - (drift / 3.0).min(1.0);
        Some(clamp_score(
            rmsd_component * 0.30
                + drift_component * 0.25
                + contact * 0.20
                + hbond * 0.10
                + persistence * 0.15,
        ))
    })
}

fn parse_score_cell(value: &str) -> Option<f64> {
    let score = value.trim().parse::<f64>().ok()?;
    if score.is_finite() {
        Some(clamp_score(score))
    } else {
        None
    }
}

fn clamp_score(value: f64) -> f64 {
    value.clamp(0.0, 1.0)
}

fn append_evidence_once(evidence: &mut String, item: &str) {
    if evidence.split(';').any(|part| part == item) {
        return;
    }
    if !evidence.is_empty() {
        evidence.push(';');
    }
    evidence.push_str(item);
}

fn build_evidence(options: &ScienceOptions) -> Result<String, String> {
    ensure_final_artifacts_authentic(
        options,
        &[
            "known_database_matches.tsv",
            "metagenome_potential_matches.tsv",
            "structure_interaction_matches.tsv",
            "tool_outputs/mmseqs_alignment_matches.tsv",
            "tool_outputs/foldseek_structure_matches.tsv",
            "docking_summary.tsv",
            "md_summary.tsv",
            "target_candidate_ranking.tsv",
            "candidate_detail.json",
        ],
    )?;
    write_score_decomposition(options)?;
    write_artifact_source_map(options)?;
    let files = [
        "run_context.json",
        "authenticity_gate.tsv",
        "artifact_source_map.tsv",
        "input_conversion_manifest.tsv",
        "input_normalized_manifest.tsv",
        "feature_manifest.tsv",
        "tool_outputs/mmseqs_alignment_matches.tsv",
        "tool_outputs/mmseqs_alignment_status.tsv",
        "known_database_matches.tsv",
        "metagenome_potential_matches.tsv",
        "structure_interaction_matches.tsv",
        "tool_outputs/foldseek_structure_matches.tsv",
        "tool_outputs/foldseek_alignment_status.tsv",
        "complex_candidate_selection.tsv",
        "scientific_runtime_summary.tsv",
        "structure_patch_summary.tsv",
        "docking_summary.tsv",
        "tool_outputs/docking_manifest.json",
        "tool_outputs/docking_pose.sdf",
        "md_summary.tsv",
        "tool_outputs/md_summary.tsv",
        "tool_outputs/md_rmsd.tsv",
        "tool_outputs/md_rmsf.tsv",
        "tool_outputs/md_ligand_drift.tsv",
        "tool_outputs/md_contacts.tsv",
        "tool_outputs/md_hbonds.tsv",
        "tool_outputs/md_protocol.tsv",
        "tool_outputs/md_state.tsv",
        "tool_outputs/gpu_telemetry.tsv",
        "tool_outputs/gpu_telemetry_status.tsv",
        "tool_outputs/ligand_placement.tsv",
        "tool_outputs/md_trajectory.tsv",
        "tool_outputs/complex_md_manifest.json",
        "tool_outputs/md_run_manifest.json",
        "target_candidate_ranking.tsv",
        "score_decomposition.tsv",
        "candidate_detail.json",
    ];
    let mut text = String::from("claim_id\tevidence_artifact\towner\tstatus\tnotes\n");
    for file in files {
        let path = options.run_dir.join(file);
        if path.exists() {
            let status = artifact_evidence_status(file, &path)?;
            text.push_str(&format!(
                "{}\t{file}\tGenoLigand\t{}\t{};content_id:{}\n",
                file.trim_end_matches(".tsv"),
                status.label,
                clean_cell(&status.notes),
                stable_hash(&read_text(&path)?)
            ));
        }
    }
    write_text(&options.run_dir.join("evidence_ledger.tsv"), &text)?;
    Ok("evidence_built".to_string())
}

struct ArtifactEvidenceStatus {
    label: &'static str,
    notes: String,
}

fn artifact_evidence_status(file: &str, path: &Path) -> Result<ArtifactEvidenceStatus, String> {
    let rows = if file.ends_with(".tsv") {
        rows_in_tsv(path)?
    } else {
        0
    };
    let (label, notes) = match file {
        "run_context.json" => ("available", "run context recorded".to_string()),
        "artifact_source_map.tsv" => ("available", format!("{rows} source-map rows")),
        "input_conversion_manifest.tsv" => {
            if rows > 0 {
                ("available", format!("{rows} input conversion rows"))
            } else {
                ("not_generated", "no input conversion rows".to_string())
            }
        }
        "input_normalized_manifest.tsv" => {
            if rows > 0 {
                ("available", format!("{rows} normalized input rows"))
            } else {
                ("not_generated", "no normalized input rows".to_string())
            }
        }
        "feature_manifest.tsv" => {
            if rows > 0 {
                ("available", format!("{rows} computed feature rows"))
            } else {
                ("not_generated", "no computed feature rows".to_string())
            }
        }
        "tool_outputs/mmseqs_alignment_matches.tsv" => {
            if rows > 0 {
                ("available", format!("{rows} MMseqs2 onsite alignment rows"))
            } else {
                ("no_match", "no MMseqs2 onsite alignment rows".to_string())
            }
        }
        "tool_outputs/mmseqs_alignment_status.tsv" => {
            if rows > 0 {
                (
                    "available",
                    "MMseqs2 onsite alignment status recorded".to_string(),
                )
            } else {
                (
                    "not_generated",
                    "no MMseqs2 alignment status row".to_string(),
                )
            }
        }
        "known_database_matches.tsv" => {
            if rows > 0 {
                ("available", format!("{rows} known database match rows"))
            } else {
                ("no_match", "no known database match rows".to_string())
            }
        }
        "metagenome_potential_matches.tsv" => {
            if rows > 0 {
                ("available", format!("{rows} metagenome match rows"))
            } else {
                ("no_match", "no metagenome match rows".to_string())
            }
        }
        "structure_interaction_matches.tsv" => {
            if rows > 0 {
                (
                    "available",
                    format!("{rows} structure interaction match rows"),
                )
            } else {
                (
                    "no_match",
                    "no structure interaction match rows".to_string(),
                )
            }
        }
        "tool_outputs/foldseek_structure_matches.tsv" => {
            if rows > 0 {
                (
                    "available",
                    format!("{rows} Foldseek onsite structure alignment rows"),
                )
            } else {
                (
                    "no_match",
                    "no Foldseek onsite structure alignment rows".to_string(),
                )
            }
        }
        "tool_outputs/foldseek_alignment_status.tsv" => {
            if rows > 0 {
                (
                    "available",
                    "Foldseek onsite alignment status recorded".to_string(),
                )
            } else {
                (
                    "not_generated",
                    "no Foldseek alignment status row".to_string(),
                )
            }
        }
        "complex_candidate_selection.tsv" => {
            if rows > 0 {
                (
                    "available",
                    format!("{rows} selected complex candidate rows"),
                )
            } else {
                (
                    "not_generated",
                    "no selected complex candidate rows".to_string(),
                )
            }
        }
        "scientific_runtime_summary.tsv" => {
            if rows > 0 {
                let text = read_text(path)?;
                let pass_count = text
                    .lines()
                    .skip(1)
                    .filter(|line| line.contains("\tPASS\t"))
                    .count();
                (
                    "available",
                    format!("{rows} scientific worker runs recorded; {pass_count} passed"),
                )
            } else {
                ("not_generated", "no scientific runtime rows".to_string())
            }
        }
        "structure_patch_summary.tsv" => {
            if rows > 0 {
                ("available", format!("{rows} structure or pocket rows"))
            } else {
                ("not_generated", "no structure or pocket rows".to_string())
            }
        }
        "docking_summary.tsv" => {
            if rows > 0 {
                ("available", format!("{rows} docking rows"))
            } else {
                ("not_generated", "no docking rows".to_string())
            }
        }
        "tool_outputs/docking_manifest.json" => (
            "available",
            "AutoDock Vina docking manifest recorded".to_string(),
        ),
        "tool_outputs/docking_pose.sdf" => (
            "available",
            "AutoDock Vina docking pose SDF recorded".to_string(),
        ),
        "md_summary.tsv" => {
            if rows > 0 {
                (
                    "available",
                    format!(
                        "{rows} OpenMM complex MD rows from current run metrics; md_stability_score feeds md_score and final_score with weight {MD_SCORE_WEIGHT:.2}; receptor-ligand force-field system evidence"
                    ),
                )
            } else {
                ("not_generated", "no dynamics rows".to_string())
            }
        }
        "tool_outputs/md_protocol.tsv" => {
            if rows > 0 {
                ("available", format!("{rows} MD protocol rows"))
            } else {
                ("not_generated", "no MD protocol rows".to_string())
            }
        }
        "tool_outputs/md_state.tsv" => {
            if rows > 0 {
                ("available", format!("{rows} OpenMM state rows"))
            } else {
                ("not_generated", "no OpenMM state rows".to_string())
            }
        }
        file if file.starts_with("tool_outputs/md_") && file.ends_with(".tsv") => {
            if rows > 0 {
                (
                    "available",
                    format!("{rows} current-run MD metric artifact rows for complex summary"),
                )
            } else {
                ("not_generated", "no MD metric artifact rows".to_string())
            }
        }
        "tool_outputs/gpu_telemetry.tsv" => {
            if rows > 0 {
                ("available", format!("{rows} CUDA GPU telemetry samples"))
            } else {
                ("not_generated", "no CUDA GPU telemetry samples".to_string())
            }
        }
        "tool_outputs/gpu_telemetry_status.tsv" => {
            if rows > 0 {
                (
                    "available",
                    "CUDA GPU observation status recorded for this run".to_string(),
                )
            } else {
                (
                    "not_generated",
                    "no CUDA GPU observation status row".to_string(),
                )
            }
        }
        "tool_outputs/ligand_placement.tsv" => {
            if rows > 0 {
                (
                    "available",
                    format!("{rows} ligand placement coordinate rows"),
                )
            } else {
                (
                    "not_generated",
                    "no ligand placement coordinate rows".to_string(),
                )
            }
        }
        "target_candidate_ranking.tsv" => {
            if rows > 0 {
                let md_ranked = read_candidates(path)?
                    .iter()
                    .filter(|candidate| candidate.dynamics_score > 0.0)
                    .count();
                (
                    "available",
                    format!(
                        "{rows} ranked candidate rows; {md_ranked} rows include md_score from current run MD Summary"
                    ),
                )
            } else {
                ("no_candidate", "no ranked candidate rows".to_string())
            }
        }
        "score_decomposition.tsv" => {
            if rows > 0 {
                ("available", format!("{rows} score component rows"))
            } else {
                ("not_generated", "no score component rows".to_string())
            }
        }
        "candidate_detail.json" => {
            let value =
                serde_json::from_str::<Value>(read_text(path)?.trim_start_matches('\u{feff}'))
                    .unwrap_or(Value::Null);
            let count = value
                .get("candidates")
                .and_then(Value::as_array)
                .map(Vec::len)
                .unwrap_or(0);
            if count > 0 {
                ("available", format!("{count} candidate detail rows"))
            } else {
                ("no_candidate", "no candidate detail rows".to_string())
            }
        }
        _ => ("available", "artifact recorded".to_string()),
    };
    Ok(ArtifactEvidenceStatus { label, notes })
}

fn rows_in_tsv(path: &Path) -> Result<usize, String> {
    Ok(read_text(path)?
        .lines()
        .skip(1)
        .filter(|line| !line.trim().is_empty())
        .count())
}

fn render_report(options: &ScienceOptions) -> Result<String, String> {
    ensure_final_artifacts_authentic(
        options,
        &[
            "target_candidate_ranking.tsv",
            "candidate_detail.json",
            "evidence_ledger.tsv",
            "known_database_matches.tsv",
            "metagenome_potential_matches.tsv",
            "structure_interaction_matches.tsv",
            "docking_summary.tsv",
            "md_summary.tsv",
        ],
    )?;
    let candidates = read_candidates(&options.run_dir.join("target_candidate_ranking.tsv"))?;
    let mut text = String::from("# GenoLigand \u{79d1}\u{7814}\u{62a5}\u{544a}\n\n");
    text.push_str(
        "\u{672c}\u{62a5}\u{544a}\u{5305}\u{542b}\u{8ba1}\u{7b97}\u{9884}\u{6d4b}\u{3001}\u{5019}\u{9009}\u{4f18}\u{5148}\u{7ea7}\u{548c}\u{8bc1}\u{636e}\u{6458}\u{8981}\u{ff1b}\u{5b9e}\u{9a8c}\u{9a8c}\u{8bc1}\u{3001}\u{4f53}\u{5185}\u{7ed3}\u{5408}\u{3001}\u{836f}\u{6548}\u{548c}\u{4f5c}\u{7528}\u{673a}\u{5236}\u{7ed3}\u{8bba}\u{7531}\u{540e}\u{7eed}\u{5b9e}\u{9a8c}\u{8865}\u{5145}\u{3002}\n\n",
    );
    text.push_str(&report_source_summary(options, candidates.len()));
    text.push_str(&report_md_score_summary(options, &candidates)?);
    text.push_str("## \u{5019}\u{9009}\u{9776}\u{70b9}\n\n");
    if candidates.is_empty() {
        text.push_str("\u{5f53}\u{524d}\u{8fd0}\u{884c}\u{6ca1}\u{6709}\u{751f}\u{6210}\u{5019}\u{9009}\u{9776}\u{70b9}\u{3002}\u{53ef}\u{80fd}\u{539f}\u{56e0}\u{662f}\u{6ca1}\u{6709}\u{5339}\u{914d}\u{8bc1}\u{636e}\u{ff0c}\u{6216}\u{8ba1}\u{7b97}\u{5728}\u{5f00}\u{59cb}\u{524d}\u{88ab}\u{8fd0}\u{884c}\u{6761}\u{4ef6}\u{963b}\u{585e}\u{3002}\n");
    } else {
        text.push_str("| \u{6392}\u{5e8f} | \u{9776}\u{70b9} | \u{5019}\u{9009}\u{8303}\u{56f4} | \u{7269}\u{79cd} | \u{7efc}\u{5408}\u{5206}\u{6570} | \u{8bc1}\u{636e}\u{6458}\u{8981} |\n");
        text.push_str("| --- | --- | --- | --- | --- | --- |\n");
        for (index, candidate) in candidates.iter().take(REPORT_CANDIDATE_LIMIT).enumerate() {
            text.push_str(&format!(
                "| {} | {} ({}) | {} | {} | {:.4} | {} |\n",
                index + 1,
                clean_cell(&candidate.target_name),
                clean_cell(&candidate.target_id),
                report_scope_label(&candidate.candidate_scope),
                clean_cell(&candidate.organism),
                candidate.final_score,
                clean_cell(&candidate.evidence)
            ));
        }
        if candidates.len() > REPORT_CANDIDATE_LIMIT {
            text.push_str(&format!(
                "\n\u{5171}\u{751f}\u{6210} {} \u{4e2a}\u{5019}\u{9009}\u{9776}\u{70b9}\u{ff1b}\u{62a5}\u{544a}\u{5c55}\u{793a}\u{524d} {} \u{4e2a}\u{ff0c}\u{5b8c}\u{6574}\u{6392}\u{540d}\u{53ef}\u{5728}\u{5bfc}\u{51fa}\u{7684}\u{5019}\u{9009}\u{6392}\u{5e8f}\u{8868}\u{4e2d}\u{67e5}\u{770b}\u{3002}\n",
                candidates.len(),
                REPORT_CANDIDATE_LIMIT
            ));
        }
        text.push_str(
            "\n\u{5efa}\u{8bae}\u{628a}\u{6392}\u{5e8f}\u{9760}\u{524d}\u{7684}\u{5019}\u{9009}\u{4f5c}\u{4e3a}\u{540e}\u{7eed}\u{5b9e}\u{9a8c}\u{9a8c}\u{8bc1}\u{7684}\u{4f18}\u{5148}\u{5bf9}\u{8c61}\u{ff0c}\u{5e76}\u{7ed3}\u{5408}\u{5b9e}\u{9a8c}\u{7ed3}\u{679c}\u{5f62}\u{6210}\u{751f}\u{7269}\u{5b66}\u{7ed3}\u{8bba}\u{3002}\n",
        );
    }
    write_text(&options.run_dir.join("research_report.md"), &text)?;
    Ok("report_rendered".to_string())
}

fn report_md_score_summary(
    options: &ScienceOptions,
    candidates: &[Candidate],
) -> Result<String, String> {
    let md_path = options.run_dir.join("md_summary.tsv");
    let md_rows = rows_in_tsv(&md_path).unwrap_or(0);
    let ranked_md = candidates
        .iter()
        .filter(|candidate| candidate.dynamics_score > 0.0)
        .count();
    if md_rows == 0 {
        return Ok(
            "## \u{52a8}\u{529b}\u{5b66}\u{8bc4}\u{5206}\n\n\u{5f53}\u{524d}\u{8fd0}\u{884c}\u{6ca1}\u{6709}\u{53ef}\u{7528}\u{7684} MD Summary \u{884c}\u{ff0c}md_score \u{4fdd}\u{6301} 0.0000\u{ff0c}\u{5bf9} final_score \u{8d21}\u{732e}\u{4e3a} 0\u{3002}\n\n".to_string(),
        );
    }
    let platform =
        read_md_manifest_field(options, "platform").unwrap_or_else(|| "unknown".to_string());
    let trajectory = read_md_manifest_field(options, "trajectory")
        .unwrap_or_else(|| "tool_outputs/md_trajectory.tsv".to_string());
    let protocol = read_md_manifest_field(options, "protocol_artifact")
        .unwrap_or_else(|| "tool_outputs/md_protocol.tsv".to_string());
    let state = read_md_manifest_field(options, "state_artifact")
        .unwrap_or_else(|| "tool_outputs/md_state.tsv".to_string());
    let telemetry = read_md_manifest_field(options, "gpu_telemetry_artifact")
        .unwrap_or_else(|| "tool_outputs/gpu_telemetry.tsv".to_string());
    let gpu_observed = if read_md_manifest_bool(options, "gpu_observed").unwrap_or(false) {
        "\u{5df2}\u{89c2}\u{6d4b}"
    } else {
        "\u{672a}\u{89c2}\u{6d4b}"
    };
    Ok(format!(
        "## \u{52a8}\u{529b}\u{5b66}\u{8bc4}\u{5206}\n\n- \u{6765}\u{6e90}\u{ff1a}md_summary.tsv \u{4e2d}\u{7684} md_stability_score\u{ff0c}\u{7531} tool_outputs/md_rmsd.tsv\u{3001}tool_outputs/md_rmsf.tsv\u{3001}tool_outputs/md_ligand_drift.tsv\u{3001}tool_outputs/md_contacts.tsv \u{548c} tool_outputs/md_hbonds.tsv \u{7b49}\u{5f53}\u{524d}\u{8fd0}\u{884c}\u{8f68}\u{8ff9}\u{6307}\u{6807}\u{6c47}\u{603b}\u{5f97}\u{5230}\u{3002}\n- \u{7528}\u{9014}\u{ff1a}\u{5199}\u{5165} Candidate Ranking \u{7684} md_score\u{ff0c}\u{5e76}\u{4ee5} {MD_SCORE_WEIGHT:.2} \u{6743}\u{91cd}\u{53c2}\u{4e0e} final_score\u{ff1b}\u{672c}\u{8fd0}\u{884c} {ranked_md} \u{4e2a}\u{5019}\u{9009}\u{6709} MD \u{5206}\u{6570}\u{3002}\n- \u{4f7f}\u{7528}\u{8bf4}\u{660e}\u{ff1a}\u{5f53}\u{524d} MD \u{4f7f}\u{7528}\u{5f53}\u{524d} run \u{7684} receptor-ligand complex\u{3001}\u{914d}\u{4f53}\u{5bf9}\u{63a5}\u{4f4d}\u{59ff}\u{3001}OpenFF/RDKit \u{914d}\u{4f53}\u{53c2}\u{6570}\u{3001}OpenMM \u{86cb}\u{767d}\u{529b}\u{573a}\u{3001}\u{663e}\u{5f0f}\u{6eb6}\u{5242}\u{548c} PME\u{ff1b}\u{534f}\u{8bae}\u{3001}\u{72b6}\u{6001}\u{5e8f}\u{5217}\u{548c} CUDA \u{89c2}\u{6d4b}\u{8bc1}\u{636e}\u{7531}\u{5f53}\u{524d}\u{8fd0}\u{884c}\u{4ea7}\u{7269}\u{8bb0}\u{5f55}\u{3002}\n- \u{8fd0}\u{884c}\u{8bb0}\u{5f55}\u{ff1a}OpenMM \u{5e73}\u{53f0} {platform}\u{ff0c}CUDA GPU {gpu_observed}\u{ff0c}\u{534f}\u{8bae}\u{4ea7}\u{7269} {protocol}\u{ff0c}\u{72b6}\u{6001}\u{5e8f}\u{5217} {state}\u{ff0c}\u{76d1}\u{6d4b}\u{4ea7}\u{7269} {telemetry}\u{ff0c}\u{8f68}\u{8ff9}\u{4ea7}\u{7269} {trajectory}\u{3002}\n\n"
    ))
}

fn read_md_manifest_field(options: &ScienceOptions, field: &str) -> Option<String> {
    read_text(&options.run_dir.join("tool_outputs/md_run_manifest.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}')).ok())
        .and_then(|value| {
            value
                .get(field)
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
}

fn read_md_manifest_bool(options: &ScienceOptions, field: &str) -> Option<bool> {
    read_text(&options.run_dir.join("tool_outputs/md_run_manifest.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}')).ok())
        .and_then(|value| value.get(field).and_then(Value::as_bool))
}

fn report_source_summary(options: &ScienceOptions, candidate_count: usize) -> String {
    let project_id = clean_cell(&infer_project_id(&options.run_dir));
    let run_id = clean_cell(&infer_run_id(&options.run_dir));
    let input_status = if options.input_manifest.exists() {
        "\u{5df2}\u{8bfb}\u{53d6}\u{5f53}\u{524d}\u{9879}\u{76ee}\u{8f93}\u{5165}"
    } else {
        "\u{672a}\u{8bfb}\u{53d6}\u{5230}\u{5f53}\u{524d}\u{9879}\u{76ee}\u{8f93}\u{5165}"
    };
    let required = [
        options.science_store.join("science_store_manifest.tsv"),
        options.science_store.join("indexes/known_targets.tsv"),
        options.science_store.join("indexes/metagenome_targets.tsv"),
        options.science_store.join("indexes/structure_targets.tsv"),
        options.science_store.join("indexes/tool_versions.tsv"),
    ];
    let install_ready = read_text(&options.science_store.join("install_status.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}')).ok())
        .and_then(|value| {
            value
                .get("status")
                .and_then(Value::as_str)
                .map(|status| status == "ready")
        })
        .unwrap_or(false);
    let database_status = if install_ready && required.iter().all(|path| path.exists()) {
        "\u{79d1}\u{5b66}\u{8d44}\u{6e90}\u{5df2}\u{5c31}\u{7eea}"
    } else {
        "\u{79d1}\u{5b66}\u{8d44}\u{6e90}\u{9700}\u{8981}\u{68c0}\u{67e5}\u{6216}\u{4fee}\u{590d}"
    };
    let output_status = if candidate_count > 0 {
        format!(
            "\u{5f53}\u{524d}\u{8fd0}\u{884c}\u{751f}\u{6210} {candidate_count} \u{4e2a}\u{5019}\u{9009}\u{5047}\u{8bbe}\u{ff0c}\u{6392}\u{5e8f}\u{3001}\u{8be6}\u{60c5}\u{548c}\u{8bc1}\u{636e}\u{6458}\u{8981}\u{5747}\u{6765}\u{81ea}\u{5f53}\u{524d}\u{8fd0}\u{884c}"
        )
    } else {
        "\u{5f53}\u{524d}\u{8fd0}\u{884c}\u{6ca1}\u{6709}\u{53ef}\u{5c55}\u{793a}\u{7684}\u{5019}\u{9009}\u{5047}\u{8bbe}".to_string()
    };
    format!(
        "## \u{7ed3}\u{679c}\u{6765}\u{6e90}\n\n- \u{9879}\u{76ee}\u{7f16}\u{53f7}\u{ff1a}{project_id}\n- \u{8fd0}\u{884c}\u{7f16}\u{53f7}\u{ff1a}{run_id}\n- \u{8f93}\u{5165}\u{ff1a}{input_status}\n- \u{6570}\u{636e}\u{5e93}\u{ff1a}{database_status}\n- \u{8f93}\u{51fa}\u{6750}\u{6599}\u{ff1a}{output_status}\n\n",
    )
}

fn report_scope_label(value: &str) -> &'static str {
    match value {
        "known_database" | "known_human_protein" => {
            "\u{5df2}\u{77e5}\u{86cb}\u{767d}\u{6570}\u{636e}\u{5e93}"
        }
        "known_nonhuman_protein" => {
            "\u{5df2}\u{77e5}\u{975e}\u{4eba}\u{6e90}\u{86cb}\u{767d}\u{6570}\u{636e}\u{5e93}"
        }
        "metagenome_potential" | "metagenome_potential_protein" => {
            "\u{5b8f}\u{57fa}\u{56e0}\u{7ec4}\u{6f5c}\u{5728}\u{5e8f}\u{5217}"
        }
        "structure_interaction" => "\u{7ed3}\u{6784}\u{7d22}\u{5f15}\u{8bc1}\u{636e}",
        _ => "\u{8ba1}\u{7b97}\u{5019}\u{9009}",
    }
}
fn write_run_context(
    options: &ScienceOptions,
    state: &str,
    blockers: &[String],
) -> Result<(), String> {
    let project_id = infer_project_id(&options.run_dir);
    let run_id = infer_run_id(&options.run_dir);
    let input_status = file_context(&options.input_manifest);
    let known_index = options.science_store.join("indexes/known_targets.tsv");
    let metagenome_index = options.science_store.join("indexes/metagenome_targets.tsv");
    let structure_index = options.science_store.join("indexes/structure_targets.tsv");
    let tool_versions = options.science_store.join("indexes/tool_versions.tsv");
    let context = json!({
        "state": state,
        "project_id": project_id,
        "run_id": run_id,
        "workspace_root": options.workspace_root.display().to_string(),
        "science_store": options.science_store.display().to_string(),
        "run_dir": options.run_dir.display().to_string(),
        "input_manifest": input_status,
        "database_indexes": {
            "known_targets": file_context(&known_index),
            "metagenome_targets": file_context(&metagenome_index),
            "structure_targets": file_context(&structure_index),
            "tool_versions": file_context(&tool_versions)
        },
        "blockers": blockers,
    });
    write_text(
        &options.run_dir.join("run_context.json"),
        &(context.to_string() + "\n"),
    )?;
    let mut text = String::from("field\tvalue\n");
    for (field, value) in [
        ("state", state.to_string()),
        ("project_id", project_id),
        ("run_id", run_id),
        (
            "workspace_root",
            options.workspace_root.display().to_string(),
        ),
        ("science_store", options.science_store.display().to_string()),
        ("run_dir", options.run_dir.display().to_string()),
        (
            "input_manifest",
            options.input_manifest.display().to_string(),
        ),
        ("known_targets_index", known_index.display().to_string()),
        (
            "metagenome_targets_index",
            metagenome_index.display().to_string(),
        ),
        (
            "structure_targets_index",
            structure_index.display().to_string(),
        ),
        ("tool_versions", tool_versions.display().to_string()),
        ("blockers", blockers.join(";")),
    ] {
        text.push_str(&format!("{field}\t{}\n", clean_cell(&value)));
    }
    write_text(&options.run_dir.join("run_context.tsv"), &text)
}

fn file_context(path: &Path) -> Value {
    let exists = path.exists();
    let bytes = fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
    let content_id = if exists {
        read_text(path).ok().map(|text| stable_hash(&text))
    } else {
        None
    };
    json!({
        "path": path.display().to_string(),
        "exists": exists,
        "bytes": bytes,
        "content_id": content_id,
    })
}

fn infer_project_id(run_dir: &Path) -> String {
    run_dir
        .parent()
        .and_then(|path| path.parent())
        .and_then(|path| path.file_name())
        .and_then(|name| name.to_str())
        .unwrap_or("standalone_project")
        .to_string()
}

fn infer_run_id(run_dir: &Path) -> String {
    run_dir
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("standalone_run")
        .to_string()
}

fn write_candidate_detail(
    options: &ScienceOptions,
    candidates: &[Candidate],
) -> Result<(), String> {
    let project_id = infer_project_id(&options.run_dir);
    let run_id = infer_run_id(&options.run_dir);
    let md_available = rows_in_tsv(&options.run_dir.join("md_summary.tsv")).unwrap_or(0) > 0;
    let rows = candidates
        .iter()
        .map(|candidate| {
            let mut source_artifacts = vec![
                "input_conversion_manifest.tsv",
                "input_normalized_manifest.tsv",
                "feature_manifest.tsv",
                "known_database_matches.tsv",
                "metagenome_potential_matches.tsv",
                "structure_interaction_matches.tsv",
                "complex_candidate_selection.tsv",
                "target_candidate_ranking.tsv",
                "score_decomposition.tsv",
                "evidence_ledger.tsv",
            ];
            if md_available && candidate.dynamics_score > 0.0 {
                source_artifacts.extend([
                    "md_summary.tsv",
                    "tool_outputs/complex_md_manifest.json",
                    "tool_outputs/ligand_placement.tsv",
                    "tool_outputs/md_run_manifest.json",
                    "tool_outputs/md_protocol.tsv",
                    "tool_outputs/md_state.tsv",
                    "tool_outputs/gpu_telemetry.tsv",
                    "tool_outputs/gpu_telemetry_status.tsv",
                    "tool_outputs/md_trajectory.tsv",
                    "tool_outputs/md_rmsd.tsv",
                    "tool_outputs/md_rmsf.tsv",
                    "tool_outputs/md_ligand_drift.tsv",
                    "tool_outputs/md_contacts.tsv",
                    "tool_outputs/md_hbonds.tsv",
                ]);
            }
            json!({
                "candidate_id": candidate.candidate_id,
                "target_id": candidate.target_id,
                "target_name": candidate.target_name,
                "candidate_scope": candidate.candidate_scope,
                "organism": candidate.organism,
                "scores": {
                    "final": candidate.final_score,
                    "known_database": candidate.known_score,
                    "metagenome": candidate.metagenome_score,
                    "structure": candidate.structure_score,
                    "docking": candidate.docking_score,
                    "md": candidate.dynamics_score
                },
                "evidence_summary": candidate.evidence,
                "source_artifacts": source_artifacts,
                "project_id": project_id,
                "run_id": run_id
            })
        })
        .collect::<Vec<_>>();
    let value = json!({
        "candidate_count": candidates.len(),
        "status": "computed",
        "project_id": project_id,
        "run_id": run_id,
        "candidates": rows
    });
    write_text(
        &options.run_dir.join("candidate_detail.json"),
        &(value.to_string() + "\n"),
    )
}

fn write_artifact_source_map(options: &ScienceOptions) -> Result<(), String> {
    let project_id = infer_project_id(&options.run_dir);
    let run_id = infer_run_id(&options.run_dir);
    let rows = [
        (
            "run_context.json",
            "workspace, science store, input manifest, database index files",
            "run_context.tsv",
        ),
        (
            "input_normalized_manifest.tsv",
            "input_conversion_manifest.tsv",
            "feature_manifest.tsv",
        ),
        (
            "input_conversion_manifest.tsv",
            "input_manifest.json",
            "input_normalized_manifest.tsv;feature_manifest.tsv;evidence_ledger.tsv",
        ),
        (
            "feature_manifest.tsv",
            "input_conversion_manifest.tsv;input_normalized_manifest.tsv",
            "known_database_matches.tsv;metagenome_potential_matches.tsv;tool_outputs/mmseqs_alignment_matches.tsv",
        ),
        (
            "tool_outputs/mmseqs_alignment_matches.tsv",
            "input_conversion_manifest.tsv;indexes/mmseqs;MMseqs2 persistent database search",
            "metagenome_potential_matches.tsv;evidence_ledger.tsv",
        ),
        (
            "tool_outputs/mmseqs_alignment_status.tsv",
            "MMseqs2 persistent database search execution",
            "evidence_ledger.tsv",
        ),
        (
            "known_database_matches.tsv",
            "indexes/known_targets.tsv",
            "target_candidate_ranking.tsv",
        ),
        (
            "metagenome_potential_matches.tsv",
            "indexes/metagenome_targets.tsv",
            "target_candidate_ranking.tsv",
        ),
        (
            "target_candidate_ranking.tsv",
            "known_database_matches.tsv;metagenome_potential_matches.tsv;structure_interaction_matches.tsv;md_summary.tsv",
            "candidate_detail.json;score_decomposition.tsv",
        ),
        (
            "structure_interaction_matches.tsv",
            "indexes/structure_targets.tsv;feature_manifest.tsv;tool_outputs/foldseek_structure_matches.tsv",
            "target_candidate_ranking.tsv;structure_patch_summary.tsv;complex_candidate_selection.tsv",
        ),
        (
            "tool_outputs/foldseek_structure_matches.tsv",
            "structure_viewer coordinate artifact;indexes/foldseek;Foldseek persistent database search",
            "structure_interaction_matches.tsv;evidence_ledger.tsv",
        ),
        (
            "tool_outputs/foldseek_alignment_status.tsv",
            "Foldseek persistent database search execution",
            "evidence_ledger.tsv",
        ),
        (
            "complex_candidate_selection.tsv",
            "known_database_matches.tsv;structure_interaction_matches.tsv;local coordinate files",
            "structure_viewer_status.tsv;docking_summary.tsv;md_summary.tsv;target_candidate_ranking.tsv",
        ),
        (
            "scientific_runtime_summary.tsv",
            "current run scientific worker jobs and results",
            "evidence_ledger.tsv",
        ),
        (
            "structure_patch_summary.tsv",
            "structure_interaction_matches.tsv;schema-bound structure output when present;otherwise index-match summary",
            "evidence_ledger.tsv",
        ),
        (
            "docking_summary.tsv",
            "schema-bound docking output when present;otherwise no generated rows",
            "tool_outputs/docking_manifest.json;tool_outputs/docking_pose.sdf;evidence_ledger.tsv",
        ),
        (
            "tool_outputs/docking_manifest.json",
            "AutoDock Vina command, receptor coordinate, ligand structure and docking box",
            "docking_summary.tsv;md_summary.tsv;evidence_ledger.tsv",
        ),
        (
            "tool_outputs/docking_pose.sdf",
            "AutoDock Vina pose converted from docking_pose.pdbqt by Open Babel",
            "tool_outputs/ligand_placement.tsv;tool_outputs/complex_md_manifest.json;md_summary.tsv",
        ),
        (
            "md_summary.tsv",
            "tool_outputs/docking_pose.sdf;tool_outputs/md_run_manifest.json;tool_outputs/md_protocol.tsv;tool_outputs/md_state.tsv;tool_outputs/gpu_telemetry_status.tsv;tool_outputs/md_summary.tsv;tool_outputs/md_rmsd.tsv;tool_outputs/md_rmsf.tsv;tool_outputs/md_ligand_drift.tsv;tool_outputs/md_contacts.tsv;tool_outputs/md_hbonds.tsv",
            "target_candidate_ranking.tsv;evidence_ledger.tsv",
        ),
        (
            "tool_outputs/md_summary.tsv",
            "schema-bound MD summary worker output",
            "md_summary.tsv",
        ),
        (
            "tool_outputs/md_rmsd.tsv",
            "current run MD or peptide conformer trajectory metrics",
            "md_summary.tsv",
        ),
        (
            "tool_outputs/md_rmsf.tsv",
            "current run MD or peptide conformer trajectory metrics",
            "md_summary.tsv",
        ),
        (
            "tool_outputs/md_ligand_drift.tsv",
            "current run MD or peptide conformer trajectory metrics",
            "md_summary.tsv",
        ),
        (
            "tool_outputs/md_contacts.tsv",
            "current run MD or peptide conformer contact metrics",
            "md_summary.tsv",
        ),
        (
            "tool_outputs/md_hbonds.tsv",
            "current run MD or peptide conformer hydrogen-bond metrics",
            "md_summary.tsv",
        ),
        (
            "tool_outputs/md_protocol.tsv",
            "current run MD protocol: coordinates, platform, minimization, equilibration, production and output cadence",
            "md_summary.tsv;tool_outputs/md_run_manifest.json;evidence_ledger.tsv",
        ),
        (
            "tool_outputs/md_state.tsv",
            "current run OpenMM state data: step, time, potential energy, kinetic energy and temperature estimate",
            "md_summary.tsv;tool_outputs/md_run_manifest.json;evidence_ledger.tsv",
        ),
        (
            "tool_outputs/gpu_telemetry.tsv",
            "nvidia-smi samples collected around OpenMM CUDA context, equilibration and production",
            "tool_outputs/gpu_telemetry_status.tsv;tool_outputs/md_run_manifest.json;evidence_ledger.tsv",
        ),
        (
            "tool_outputs/gpu_telemetry_status.tsv",
            "CUDA platform observation status derived from nvidia-smi utilization, memory delta and process samples",
            "md_summary.tsv;tool_outputs/complex_md_manifest.json;tool_outputs/md_run_manifest.json;evidence_ledger.tsv",
        ),
        (
            "tool_outputs/ligand_placement.tsv",
            "AutoDock Vina pose SDF for small molecules or RDKit peptide conformer coordinates",
            "md_summary.tsv;tool_outputs/complex_md_manifest.json;tool_outputs/md_run_manifest.json",
        ),
        (
            "tool_outputs/md_trajectory.tsv",
            "current run MD or peptide conformer trajectory coordinates",
            "md_summary.tsv",
        ),
        (
            "tool_outputs/complex_md_manifest.json",
            "current run receptor coordinate, AutoDock Vina ligand pose, OpenMM platform, trajectory and metric artifacts",
            "evidence_ledger.tsv;tool_outputs/md_run_manifest.json",
        ),
        (
            "tool_outputs/md_run_manifest.json",
            "current run MD method, platform or runtime, coordinates, trajectory and metric artifacts",
            "evidence_ledger.tsv",
        ),
        (
            "score_decomposition.tsv",
            "target_candidate_ranking.tsv",
            "candidate_detail.json;evidence_ledger.tsv;md_summary.tsv",
        ),
        (
            "candidate_detail.json",
            "target_candidate_ranking.tsv",
            "research_report.md",
        ),
        (
            "evidence_ledger.tsv",
            "all current run artifacts",
            "research_report.md",
        ),
        (
            "research_report.md",
            "candidate_detail.json;evidence_ledger.tsv",
            "export",
        ),
    ];
    let mut text =
        String::from("project_id\trun_id\tartifact\tupstream_source\tdownstream_use\tstatus\n");
    for (artifact, upstream, downstream) in rows {
        let artifact_path = options.run_dir.join(artifact);
        let status = artifact_source_status(&artifact_path)?;
        text.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\n",
            clean_cell(&project_id),
            clean_cell(&run_id),
            artifact,
            clean_cell(upstream),
            clean_cell(downstream),
            status
        ));
    }
    write_text(&options.run_dir.join("artifact_source_map.tsv"), &text)
}

fn artifact_source_status(path: &Path) -> Result<&'static str, String> {
    if !path.exists() {
        return Ok("not_yet_written");
    }
    if path.extension().and_then(|value| value.to_str()) == Some("tsv") && rows_in_tsv(path)? == 0 {
        return Ok("no_rows");
    }
    Ok("available")
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
            ("docking_score_weight", DOCKING_SCORE_WEIGHT),
            (
                "docking_score_contribution",
                candidate.docking_score * DOCKING_SCORE_WEIGHT,
            ),
            ("md_score", candidate.dynamics_score),
            ("md_score_weight", MD_SCORE_WEIGHT),
            (
                "md_score_contribution",
                candidate.dynamics_score * MD_SCORE_WEIGHT,
            ),
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

fn read_conversion_provenance(
    normalized_path: &Path,
) -> Result<BTreeMap<(String, String), ConversionProvenance>, String> {
    let parent = normalized_path
        .parent()
        .ok_or_else(|| "normalized_input_manifest_missing_parent".to_string())?;
    let path = parent.join("input_conversion_manifest.tsv");
    if !path.is_file() {
        return Err(format!(
            "normalized_input_manifest_missing_conversion_provenance:{}",
            path.display()
        ));
    }
    let text = read_text(&path)?;
    let mut lines = text.lines();
    let header = lines
        .next()
        .map(|line| line.trim_start_matches('\u{feff}'))
        .unwrap_or_default();
    if header != INPUT_CONVERSION_HEADER {
        return Err("input_conversion_manifest_schema_mismatch:header".to_string());
    }
    let mut by_identity = BTreeMap::new();
    for (index, line) in lines.enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let cols = split_tsv(line);
        if cols.len() != 18 {
            return Err(format!(
                "input_conversion_manifest_schema_mismatch:line_{}:columns_{}",
                index + 2,
                cols.len()
            ));
        }
        if cols[6] != "converted" {
            continue;
        }
        if cols[0].trim().is_empty() || cols[1].trim().is_empty() {
            return Err(format!(
                "input_conversion_manifest_missing_identity:line_{}",
                index + 2
            ));
        }
        if !matches!(
            cols[4].as_str(),
            "small_molecule" | "peptide" | "protein_ligand" | "complex" | "genome_or_metagenome"
        ) {
            return Err(format!(
                "input_conversion_manifest_invalid_kind:line_{}:{}",
                index + 2,
                clean_cell(&cols[4])
            ));
        }
        match cols[9].as_str() {
            "file" if cols[10].trim().is_empty() => {
                return Err(format!(
                    "input_conversion_manifest_missing_source_path:line_{}",
                    index + 2
                ));
            }
            "inline" if !cols[10].trim().is_empty() => {
                return Err(format!(
                    "input_conversion_manifest_unexpected_source_path:line_{}",
                    index + 2
                ));
            }
            "file" | "inline" => {}
            value => {
                return Err(format!(
                    "input_conversion_manifest_invalid_source_kind:line_{}:{}",
                    index + 2,
                    clean_cell(value)
                ));
            }
        }
        if cols[11].len() != 64 || !cols[11].chars().all(|ch| ch.is_ascii_hexdigit()) {
            return Err(format!(
                "input_conversion_manifest_invalid_sha256:line_{}",
                index + 2
            ));
        }
        let key = (cols[0].clone(), cols[1].clone());
        let provenance = ConversionProvenance {
            canonical_kind: cols[4].clone(),
            source_kind: cols[9].clone(),
            source_path: cols[10].clone(),
            content_sha256: cols[11].clone(),
        };
        if by_identity.insert(key.clone(), provenance).is_some() {
            return Err(format!(
                "input_conversion_manifest_duplicate_identity:{}:{}",
                clean_cell(&key.0),
                clean_cell(&key.1)
            ));
        }
    }
    Ok(by_identity)
}

fn read_normalized_inputs(path: &Path) -> Result<Vec<NormalizedInput>, String> {
    let text = read_text(path)?;
    let mut lines = text.lines();
    let header = lines
        .next()
        .map(|line| line.trim_start_matches('\u{feff}'))
        .unwrap_or_default();
    if header != NORMALIZED_INPUT_HEADER {
        return Err("normalized_input_manifest_schema_mismatch:header".to_string());
    }
    let mut out = Vec::new();
    let mut conversion_provenance = None;
    let mut normalized_identities = BTreeSet::new();
    for (index, line) in lines.enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let cols = split_tsv(line);
        if cols.len() != 13 {
            return Err(format!(
                "normalized_input_manifest_schema_mismatch:line_{}:columns_{}",
                index + 2,
                cols.len()
            ));
        }
        let kind = match cols[1].as_str() {
            "small_molecule" => InputKind::SmallMolecule,
            "peptide" => InputKind::Peptide,
            "protein_ligand" => InputKind::ProteinLigand,
            "complex" => InputKind::Complex,
            "genome_or_metagenome" => InputKind::GenomeOrMetagenome,
            _ => {
                return Err(format!(
                    "normalized_input_manifest_invalid_kind:line_{}:{}",
                    index + 2,
                    clean_cell(&cols[1])
                ));
            }
        };
        if cols[0].trim().is_empty() {
            return Err(format!(
                "normalized_input_manifest_missing_input_id:line_{}",
                index + 2
            ));
        }
        if cols[5].trim().is_empty() {
            return Err(format!(
                "normalized_input_manifest_missing_record_id:line_{}",
                index + 2
            ));
        }
        let source_kind = match cols[6].as_str() {
            "file" if cols[7].trim().is_empty() => {
                return Err(format!(
                    "normalized_input_manifest_missing_source_path:line_{}",
                    index + 2
                ));
            }
            "inline" if !cols[7].trim().is_empty() => {
                return Err(format!(
                    "normalized_input_manifest_unexpected_source_path:line_{}",
                    index + 2
                ));
            }
            "file" => "file",
            "inline" => "inline",
            value => {
                return Err(format!(
                    "normalized_input_manifest_invalid_source_kind:line_{}:{}",
                    index + 2,
                    clean_cell(value)
                ));
            }
        };
        if cols[8].len() != 64 || !cols[8].chars().all(|ch| ch.is_ascii_hexdigit()) {
            return Err(format!(
                "normalized_input_manifest_invalid_sha256:line_{}",
                index + 2
            ));
        }
        if cols[9] != "normalized" {
            return Err(format!(
                "normalized_input_manifest_invalid_status:line_{}:{}",
                index + 2,
                clean_cell(&cols[9])
            ));
        }
        let parse_usize = |column: usize, name: &str| {
            cols[column].parse::<usize>().map_err(|_| {
                format!(
                    "normalized_input_manifest_invalid_{name}:line_{}",
                    index + 2
                )
            })
        };
        if conversion_provenance.is_none() {
            conversion_provenance = Some(read_conversion_provenance(path)?);
        }
        let key = (cols[0].clone(), cols[5].clone());
        if !normalized_identities.insert(key.clone()) {
            return Err(format!(
                "normalized_input_manifest_duplicate_identity:{}:{}",
                clean_cell(&key.0),
                clean_cell(&key.1)
            ));
        }
        let expected = conversion_provenance
            .as_ref()
            .and_then(|rows| rows.get(&key))
            .ok_or_else(|| {
                format!(
                    "normalized_input_manifest_missing_provenance:{}:{}",
                    clean_cell(&key.0),
                    clean_cell(&key.1)
                )
            })?;
        if expected.canonical_kind != cols[1]
            || expected.source_kind != cols[6]
            || expected.source_path != cols[7]
            || expected.content_sha256 != cols[8]
        {
            return Err(format!(
                "normalized_input_manifest_provenance_mismatch:{}:{}",
                clean_cell(&key.0),
                clean_cell(&key.1)
            ));
        }
        out.push(NormalizedInput {
            input_id: cols[0].clone(),
            record_id: cols[5].clone(),
            kind,
            primary_value: decode_tsv_cell(&cols[2]),
            file_count: parse_usize(3, "file_count")?,
            tokens: cols[4].split_whitespace().map(ToOwned::to_owned).collect(),
            source_kind,
            source_path: cols[7].clone(),
            content_sha256: cols[8].clone(),
            structure_atom_count: parse_usize(10, "structure_atom_count")?,
            structure_chain_count: parse_usize(11, "structure_chain_count")?,
            structure_residue_count: parse_usize(12, "structure_residue_count")?,
        });
    }
    Ok(out)
}

fn read_computed_feature_inputs(path: &Path) -> Result<Vec<ComputedFeatureInput>, String> {
    let text = read_text(path)?;
    let mut lines = text.lines();
    let header = lines
        .next()
        .map(|line| line.trim_start_matches('\u{feff}'))
        .unwrap_or_default();
    if header != FEATURE_MANIFEST_HEADER {
        return Err("feature_manifest_schema_mismatch:header".to_string());
    }
    let normalized = read_normalized_inputs(&path.with_file_name("input_normalized_manifest.tsv"))?;
    let mut normalized_kinds = BTreeMap::<(String, String), String>::new();
    for input in normalized {
        let key = (input.input_id.clone(), input.record_id.clone());
        if normalized_kinds
            .insert(key.clone(), input.kind.as_str().to_string())
            .is_some()
        {
            return Err(format!(
                "normalized_input_manifest_duplicate_identity:{}:{}",
                clean_cell(&key.0),
                clean_cell(&key.1)
            ));
        }
    }
    let mut by_input = BTreeMap::<(String, String), ComputedFeatureInput>::new();
    for (index, line) in lines.enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let cols = split_tsv(line);
        if cols.len() != 8 {
            return Err(format!(
                "feature_manifest_schema_mismatch:line_{}:columns_{}",
                index + 2,
                cols.len()
            ));
        }
        if !matches!(
            cols[1].as_str(),
            "small_molecule" | "peptide" | "protein_ligand" | "complex" | "genome_or_metagenome"
        ) {
            return Err(format!(
                "feature_manifest_invalid_kind:line_{}:{}",
                index + 2,
                clean_cell(&cols[1])
            ));
        }
        if cols[0].trim().is_empty() {
            return Err(format!(
                "feature_manifest_missing_input_id:line_{}",
                index + 2
            ));
        }
        if cols[5].trim().is_empty() {
            return Err(format!(
                "feature_manifest_missing_feature_source:line_{}",
                index + 2
            ));
        }
        if cols[7].trim().is_empty() {
            return Err(format!(
                "feature_manifest_missing_record_id:line_{}",
                index + 2
            ));
        }
        let token = cols[6].trim();
        let key = (cols[0].clone(), cols[7].clone());
        if by_input
            .get(&key)
            .is_some_and(|input| input.input_kind != cols[1])
        {
            return Err(format!(
                "feature_manifest_kind_conflict:{}:{}",
                clean_cell(&key.0),
                clean_cell(&key.1)
            ));
        }
        let expected_kind = normalized_kinds.get(&key).ok_or_else(|| {
            format!(
                "feature_manifest_missing_normalized_identity:{}:{}",
                clean_cell(&key.0),
                clean_cell(&key.1)
            )
        })?;
        if expected_kind != &cols[1] {
            return Err(format!(
                "feature_manifest_normalized_kind_mismatch:{}:{}",
                clean_cell(&key.0),
                clean_cell(&key.1)
            ));
        }
        let entry = by_input.entry(key).or_insert_with(|| ComputedFeatureInput {
            input_id: cols[0].clone(),
            record_id: cols[7].clone(),
            input_kind: cols[1].clone(),
            tokens: BTreeSet::new(),
        });
        if !token.is_empty() {
            entry.tokens.insert(token.to_ascii_lowercase());
        }
    }
    Ok(by_input.into_values().collect())
}

fn read_source_rows(path: &Path, label: &str) -> Result<(Vec<StoreRow>, &'static str), String> {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if is_sqlite_name(&name) {
        return parse_chembl_sqlite(path).map(|rows| (rows, "chembl_sqlite_rows"));
    }
    if name.ends_with(".tar.gz") && name.contains("chembl") {
        return parse_chembl_sqlite_archive(path).map(|rows| (rows, "chembl_sqlite_archive_rows"));
    }
    read_store_rows(path, label).map(|rows| (rows, "canonical_or_source_table_rows"))
}

fn read_store_rows(path: &Path, label: &str) -> Result<Vec<StoreRow>, String> {
    let name = lower_file_name(path);
    if name.ends_with(".zip") {
        let text = read_zip_text(path, matches_table_name)
            .map_err(|err| format!("{label}_missing:{err}"))?;
        return parse_store_rows_from_lines(text.lines().map(|line| Ok(line.to_string())), label);
    }
    if name.ends_with(".gz") {
        let file = fs::File::open(path)
            .map_err(|err| format!("{label}_missing:{}:{err}", path.display()))?;
        let decoder = GzDecoder::new(file);
        return parse_store_rows_from_lines(BufReader::new(decoder).lines(), label)
            .map_err(|err| format!("{label}_missing:{err}"));
    }
    let file =
        fs::File::open(path).map_err(|err| format!("{label}_missing:{}:{err}", path.display()))?;
    parse_store_rows_from_lines(BufReader::new(file).lines(), label)
        .map_err(|err| format!("{label}_missing:{err}"))
}

fn parse_store_rows_from_lines<I>(mut lines: I, label: &str) -> Result<Vec<StoreRow>, String>
where
    I: Iterator<Item = Result<String, std::io::Error>>,
{
    let Some(header_line) = lines.next() else {
        return Ok(Vec::new());
    };
    let header_line = header_line.map_err(|err| format!("{label}_read_failed:{err}"))?;
    let headers = split_tsv(header_line.trim_start_matches('\u{feff}'));
    if is_bindingdb_header(&headers) {
        return parse_bindingdb_rows(&headers, lines, label);
    }
    let mut rows = Vec::new();
    for line in lines {
        let line = line.map_err(|err| format!("{label}_read_failed:{err}"))?;
        let line = line.trim_start_matches('\u{feff}');
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

fn parse_chembl_sqlite_archive(path: &Path) -> Result<Vec<StoreRow>, String> {
    let file =
        fs::File::open(path).map_err(|err| format!("open_failed:{}:{err}", path.display()))?;
    let decoder = GzDecoder::new(file);
    let mut archive = tar::Archive::new(decoder);
    let entries = archive
        .entries()
        .map_err(|err| format!("tar_entries_failed:{}:{err}", path.display()))?;
    for entry in entries {
        let mut entry =
            entry.map_err(|err| format!("tar_entry_failed:{}:{err}", path.display()))?;
        let member_path = entry
            .path()
            .map_err(|err| format!("tar_member_path_failed:{}:{err}", path.display()))?;
        let member_name = member_path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if !is_sqlite_name(&member_name) {
            continue;
        }
        let tmp_path = std::env::temp_dir().join(format!(
            "genoligand_chembl_{}_{}_{}",
            std::process::id(),
            stable_hash(&path.display().to_string()),
            safe_id(&member_name)
        ));
        entry
            .unpack(&tmp_path)
            .map_err(|err| format!("tar_unpack_failed:{}:{err}", path.display()))?;
        let result = parse_chembl_sqlite(&tmp_path);
        let _ = fs::remove_file(&tmp_path);
        return result;
    }
    Err(format!("chembl_sqlite_member_missing:{}", path.display()))
}

fn parse_chembl_sqlite(path: &Path) -> Result<Vec<StoreRow>, String> {
    let mut rows = Vec::new();
    let output = run_python_script(
        chembl_sqlite_query_script(),
        &[path.display().to_string()],
        "chembl_sqlite_query",
    )?;
    for line in output.lines() {
        let cols = split_tsv(line);
        if cols.len() < 12 {
            continue;
        }
        rows.push(chembl_store_row(ChEMBLActivityRow {
            target_id: cols[0].clone(),
            target_name: cols[1].clone(),
            organism: cols[2].clone(),
            target_type: cols[3].clone(),
            canonical_smiles: cols[4].clone(),
            compound_name: cols[5].clone(),
            standard_type: cols[6].clone(),
            standard_relation: cols[7].clone(),
            standard_value: cols[8].clone(),
            standard_units: cols[9].clone(),
            accessions: cols[10].clone(),
            sequences: cols[11].clone(),
        }));
    }
    Ok(rows)
}

fn chembl_sqlite_query_script() -> &'static str {
    r#"
import csv
import sqlite3
import sys

query = """
SELECT
    t.chembl_id AS target_id,
    COALESCE(t.pref_name, t.chembl_id) AS target_name,
    COALESCE(t.organism, '') AS organism,
    COALESCE(t.target_type, '') AS target_type,
    COALESCE(cs.canonical_smiles, '') AS canonical_smiles,
    COALESCE(md.pref_name, '') AS compound_name,
    COALESCE(act.standard_type, '') AS standard_type,
    COALESCE(act.standard_relation, '') AS standard_relation,
    COALESCE(CAST(act.standard_value AS TEXT), '') AS standard_value,
    COALESCE(act.standard_units, '') AS standard_units,
    COALESCE(GROUP_CONCAT(DISTINCT cseq.accession), '') AS accessions,
    COALESCE(GROUP_CONCAT(DISTINCT cseq.sequence), '') AS sequences
FROM activities act
JOIN assays a ON act.assay_id = a.assay_id
JOIN target_dictionary t ON a.tid = t.tid
LEFT JOIN molecule_dictionary md ON act.molregno = md.molregno
LEFT JOIN compound_structures cs ON act.molregno = cs.molregno
LEFT JOIN target_components tc ON t.tid = tc.tid
LEFT JOIN component_sequences cseq ON tc.component_id = cseq.component_id
WHERE act.standard_type IN ('Ki', 'Kd', 'IC50', 'EC50')
  AND act.standard_value IS NOT NULL
  AND (act.standard_units IS NULL OR act.standard_units = '' OR act.standard_units = 'nM')
  AND (cs.canonical_smiles IS NOT NULL OR cseq.sequence IS NOT NULL)
GROUP BY
    t.chembl_id,
    t.pref_name,
    t.organism,
    t.target_type,
    cs.canonical_smiles,
    md.pref_name,
    act.standard_type,
    act.standard_relation,
    act.standard_value,
    act.standard_units
"""

connection = sqlite3.connect(sys.argv[1])
writer = csv.writer(sys.stdout, delimiter="\t", lineterminator="\n")
for row in connection.execute(query):
    writer.writerow(["" if value is None else value for value in row])
"#
}

#[derive(Clone, Debug)]
struct ChEMBLActivityRow {
    target_id: String,
    target_name: String,
    organism: String,
    target_type: String,
    canonical_smiles: String,
    compound_name: String,
    standard_type: String,
    standard_relation: String,
    standard_value: String,
    standard_units: String,
    accessions: String,
    sequences: String,
}

fn chembl_store_row(row: ChEMBLActivityRow) -> StoreRow {
    let mut ligand_tokens = tokenize(&row.canonical_smiles);
    ligand_tokens.extend(tokenize(&row.compound_name));
    ligand_tokens.extend(tokenize(&row.standard_type));
    ligand_tokens.extend(tokenize(&row.standard_relation));
    ligand_tokens.extend(tokenize(&row.standard_value));
    ligand_tokens.extend(tokenize(&row.standard_units));
    let mut sequence_tokens = tokenize(&row.target_id);
    sequence_tokens.extend(tokenize(&row.target_name));
    sequence_tokens.extend(tokenize(&row.accessions));
    sequence_tokens.extend(protein_sequence_tokens(&row.sequences));
    let organism = if row.organism.trim().is_empty() {
        "unknown".to_string()
    } else {
        row.organism
    };
    let target_class = if organism.to_ascii_lowercase().contains("homo sapiens")
        || organism.to_ascii_lowercase().contains("human")
    {
        "known_human_protein"
    } else {
        "known_protein"
    };
    StoreRow {
        source: "ChEMBL".to_string(),
        target_id: row.target_id,
        target_name: row.target_name,
        organism,
        target_class: target_class.to_string(),
        ligand_tokens,
        sequence_tokens,
        structure_tokens: tokenize(&row.target_type),
        evidence: "chembl_activity_row".to_string(),
    }
}

fn is_bindingdb_header(headers: &[String]) -> bool {
    find_header(
        headers,
        &[
            "BindingDB Target Chain Sequence",
            "BindingDB Target Chain Sequence 1",
            "Target Chain Sequence",
            "Target Sequence",
        ],
    )
    .is_some()
        && find_header(headers, &["Target Name", "Target", "BindingDB Target Name"]).is_some()
}

fn parse_bindingdb_rows<I>(
    headers: &[String],
    lines: I,
    label: &str,
) -> Result<Vec<StoreRow>, String>
where
    I: Iterator<Item = Result<String, std::io::Error>>,
{
    let target_name_idx = find_header(headers, &["Target Name", "Target", "BindingDB Target Name"]);
    let sequence_idx = find_header(
        headers,
        &[
            "BindingDB Target Chain Sequence",
            "BindingDB Target Chain Sequence 1",
            "Target Chain Sequence",
            "Target Sequence",
        ],
    );
    let organism_idx = find_header(
        headers,
        &[
            "Target Source Organism According to Curator or DataSource",
            "Target Source Organism",
            "Organism",
        ],
    );
    let smiles_idx = find_header(headers, &["Ligand SMILES", "SMILES", "Ligand Smiles"]);
    let ligand_name_idx = find_header(headers, &["Ligand Name", "Name", "Compound Name"]);
    let monomer_idx = find_header(
        headers,
        &["BindingDB MonomerID", "MonomerID", "Target MonomerID"],
    );
    let swiss_idx = find_header(
        headers,
        &[
            "UniProt (SwissProt) Primary ID of Target Chain",
            "UniProt (SwissProt) Primary ID of Target Chain 1",
            "UniProt SwissProt Primary ID of Target Chain",
            "UniProt Primary ID",
        ],
    );
    let trembl_idx = find_header(
        headers,
        &[
            "UniProt (TrEMBL) Primary ID of Target Chain",
            "UniProt (TrEMBL) Primary ID of Target Chain 1",
            "UniProt TrEMBL Primary ID of Target Chain",
        ],
    );
    let pdb_idx = find_header(
        headers,
        &[
            "Target Chain PDB ID(s)",
            "PDB ID(s) of Target Chain 1",
            "PDB ID(s) for Target Chain",
            "PDB IDs",
        ],
    );
    let activity_indexes = [
        find_header(headers, &["Ki (nM)", "Ki"]),
        find_header(headers, &["IC50 (nM)", "IC50"]),
        find_header(headers, &["Kd (nM)", "Kd"]),
        find_header(headers, &["EC50 (nM)", "EC50"]),
    ];

    let mut rows = Vec::new();
    for line in lines {
        let line = line.map_err(|err| format!("{label}_read_failed:{err}"))?;
        let cols = split_tsv(line.trim_start_matches('\u{feff}'));
        let target_name = cell(&cols, target_name_idx);
        let sequence = cell(&cols, sequence_idx);
        if target_name.is_empty() && sequence.is_empty() {
            continue;
        }
        let organism = cell(&cols, organism_idx);
        let smiles = cell(&cols, smiles_idx);
        let ligand_name = cell(&cols, ligand_name_idx);
        let uniprot = first_nonempty(&[cell(&cols, swiss_idx), cell(&cols, trembl_idx)]);
        let monomer = cell(&cols, monomer_idx);
        let target_id = first_nonempty(&[uniprot.clone(), monomer.clone(), target_name.clone()]);
        let target_id = if target_id.is_empty() {
            format!("BINDINGDB_{}", stable_hash(&sequence))
        } else {
            safe_id(&target_id)
        };
        let target_name = if target_name.is_empty() {
            target_id.clone()
        } else {
            target_name
        };
        let target_class = if organism.to_ascii_lowercase().contains("homo sapiens")
            || organism.to_ascii_lowercase().contains("human")
        {
            "known_human_protein"
        } else {
            "known_protein"
        };
        let mut ligand_tokens = tokenize(&smiles);
        ligand_tokens.extend(tokenize(&ligand_name));
        for idx in activity_indexes.iter().flatten() {
            ligand_tokens.extend(tokenize(cell(&cols, Some(*idx)).as_str()));
        }
        let mut sequence_tokens = protein_sequence_tokens(&sequence);
        sequence_tokens.extend(tokenize(&target_name));
        sequence_tokens.extend(tokenize(&uniprot));
        rows.push(StoreRow {
            source: "BindingDB".to_string(),
            target_id,
            target_name,
            organism: if organism.is_empty() {
                "unknown".to_string()
            } else {
                organism
            },
            target_class: target_class.to_string(),
            ligand_tokens,
            sequence_tokens,
            structure_tokens: tokenize(&cell(&cols, pdb_idx)),
            evidence: "bindingdb_activity_row".to_string(),
        });
    }
    Ok(rows)
}

fn find_header(headers: &[String], candidates: &[&str]) -> Option<usize> {
    let candidate_keys = candidates
        .iter()
        .map(|candidate| normalize_header(candidate))
        .collect::<BTreeSet<_>>();
    headers
        .iter()
        .position(|header| candidate_keys.contains(&normalize_header(header)))
}

fn normalize_header(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(|ch| ch.to_lowercase())
        .collect()
}

fn cell(cols: &[String], idx: Option<usize>) -> String {
    idx.and_then(|value| cols.get(value))
        .map(|value| value.trim().to_string())
        .unwrap_or_default()
}

fn first_nonempty(values: &[String]) -> String {
    values
        .iter()
        .find(|value| !value.trim().is_empty())
        .cloned()
        .unwrap_or_default()
}

fn typed_feature_rows(input: &NormalizedInput) -> Vec<FeatureRow> {
    if input.structure_atom_count > 0 {
        return coordinate_structure_features(input);
    }
    match input.kind {
        InputKind::SmallMolecule => small_molecule_features(&input.primary_value),
        InputKind::Peptide => peptide_or_protein_features("peptide", &input.primary_value),
        InputKind::ProteinLigand => {
            peptide_or_protein_features("protein_sequence", &input.primary_value)
        }
        InputKind::Complex => complex_features(&input.primary_value),
        InputKind::GenomeOrMetagenome => genome_or_metagenome_features(&input.primary_value),
    }
}

fn coordinate_structure_features(input: &NormalizedInput) -> Vec<FeatureRow> {
    let extension = Path::new(&input.source_path)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    vec![
        feature_row("structure", "parsed_structure", "true".to_string(), ""),
        feature_row(
            "structure",
            "structure_format",
            if extension.is_empty() {
                "unknown".to_string()
            } else {
                extension
            },
            "",
        ),
        feature_row(
            "structure",
            "atom_count",
            input.structure_atom_count.to_string(),
            "",
        ),
        feature_row(
            "structure",
            "chain_count",
            input.structure_chain_count.to_string(),
            "",
        ),
        feature_row(
            "structure",
            "residue_count",
            input.structure_residue_count.to_string(),
            "",
        ),
        feature_row("input", "file_or_inline", "file".to_string(), ""),
    ]
}

fn small_molecule_features(value: &str) -> Vec<FeatureRow> {
    let compact = compact_sequence(value);
    let atom_symbol_count = compact
        .chars()
        .filter(|ch| ch.is_ascii_uppercase() || matches!(ch, 'c' | 'n' | 'o' | 's' | 'p'))
        .count();
    let aromatic_symbol_count = compact
        .chars()
        .filter(|ch| matches!(ch, 'c' | 'n' | 'o' | 's' | 'p'))
        .count();
    let mut rows = vec![
        feature_row(
            "small_molecule",
            "atom_symbol_count",
            atom_symbol_count.to_string(),
            "",
        ),
        feature_row(
            "small_molecule",
            "aromatic_symbol_count",
            aromatic_symbol_count.to_string(),
            "",
        ),
        feature_row(
            "small_molecule",
            "ring_marker_count",
            compact
                .chars()
                .filter(|ch| ch.is_ascii_digit())
                .count()
                .to_string(),
            "",
        ),
        feature_row(
            "small_molecule",
            "branch_marker_count",
            compact
                .chars()
                .filter(|ch| matches!(ch, '(' | ')'))
                .count()
                .to_string(),
            "",
        ),
        feature_row(
            "small_molecule",
            "charge_marker_count",
            compact
                .chars()
                .filter(|ch| matches!(ch, '+' | '-'))
                .count()
                .to_string(),
            "",
        ),
        feature_row(
            "small_molecule",
            "chiral_marker_count",
            compact.chars().filter(|ch| *ch == '@').count().to_string(),
            "",
        ),
        feature_row(
            "input",
            "file_or_inline",
            file_or_inline(value).to_string(),
            "",
        ),
    ];
    rows.extend(token_feature_rows(
        "small_molecule",
        "ligand_fingerprint_token",
        tokenize(&compact),
    ));
    rows
}

fn peptide_or_protein_features(family: &str, value: &str) -> Vec<FeatureRow> {
    let sequence = compact_sequence(value);
    let amino_acids = sequence
        .chars()
        .filter(|ch| is_standard_amino_acid(*ch))
        .collect::<Vec<_>>();
    let aa_count = amino_acids.len();
    let hydrophobic_count = amino_acids
        .iter()
        .filter(|ch| {
            matches!(
                ch.to_ascii_uppercase(),
                'A' | 'I' | 'L' | 'M' | 'F' | 'W' | 'Y' | 'V'
            )
        })
        .count();
    let charged_count = amino_acids
        .iter()
        .filter(|ch| matches!(ch.to_ascii_uppercase(), 'D' | 'E' | 'K' | 'R' | 'H'))
        .count();
    let cysteine_count = amino_acids
        .iter()
        .filter(|ch| ch.eq_ignore_ascii_case(&'C'))
        .count();
    let mut rows = vec![
        feature_row(family, "amino_acid_count", aa_count.to_string(), ""),
        feature_row(
            family,
            "hydrophobic_residue_count",
            hydrophobic_count.to_string(),
            "",
        ),
        feature_row(
            family,
            "charged_residue_count",
            charged_count.to_string(),
            "",
        ),
        feature_row(family, "cysteine_count", cysteine_count.to_string(), ""),
        feature_row(
            family,
            "fasta_header_count",
            value
                .lines()
                .filter(|line| line.starts_with('>'))
                .count()
                .to_string(),
            "",
        ),
        feature_row(
            "input",
            "file_or_inline",
            file_or_inline(value).to_string(),
            "",
        ),
    ];
    rows.extend(token_feature_rows(
        family,
        "sequence_feature_token",
        protein_sequence_tokens(&sequence),
    ));
    rows
}

fn complex_features(value: &str) -> Vec<FeatureRow> {
    let lower = value.to_ascii_lowercase();
    let path = Path::new(value.trim());
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let mut rows = vec![
        feature_row(
            "complex",
            "structure_file_exists",
            path.exists().to_string(),
            "",
        ),
        feature_row(
            "complex",
            "structure_extension",
            if extension.is_empty() {
                "inline_or_untyped".to_string()
            } else {
                extension
            },
            "",
        ),
        feature_row(
            "complex",
            "chain_marker_count",
            count_word_markers(&lower, &["chain", "链"]).to_string(),
            "",
        ),
        feature_row(
            "complex",
            "ligand_marker_count",
            count_word_markers(&lower, &["ligand", "配体"]).to_string(),
            "",
        ),
        feature_row(
            "input",
            "file_or_inline",
            file_or_inline(value).to_string(),
            "",
        ),
    ];
    rows.extend(token_feature_rows(
        "complex",
        "structure_feature_token",
        tokenize(&lower),
    ));
    rows
}

fn genome_or_metagenome_features(value: &str) -> Vec<FeatureRow> {
    let records = nucleotide_records(value);
    let nucleotide_count = records
        .iter()
        .map(|(_, sequence)| sequence.len())
        .sum::<usize>();
    let gc_count = records
        .iter()
        .flat_map(|(_, sequence)| sequence.chars())
        .filter(|ch| matches!(ch, 'G' | 'C'))
        .count();
    let gc_fraction = if nucleotide_count > 0 {
        format!("{:.4}", gc_count as f64 / nucleotide_count as f64)
    } else {
        "0.0000".to_string()
    };
    let translated_orfs = records
        .iter()
        .flat_map(|(record_id, sequence)| {
            translate_orfs(sequence)
                .into_iter()
                .map(|orf| (record_id.clone(), orf))
        })
        .collect::<Vec<_>>();
    let start_marker_count = records
        .iter()
        .map(|(_, sequence)| {
            sequence.matches("ATG").count() + reverse_complement(sequence).matches("ATG").count()
        })
        .sum::<usize>();
    let mut rows = vec![
        feature_row(
            "genome_or_metagenome",
            "nucleotide_count",
            nucleotide_count.to_string(),
            "",
        ),
        feature_row("genome_or_metagenome", "gc_fraction", gc_fraction, ""),
        feature_row(
            "genome_or_metagenome",
            "fasta_record_count",
            value
                .lines()
                .filter(|line| line.starts_with('>'))
                .count()
                .to_string(),
            "",
        ),
        feature_row(
            "genome_or_metagenome",
            "orf_start_marker_count",
            start_marker_count.to_string(),
            "",
        ),
        feature_row(
            "genome_or_metagenome",
            "translated_orf_count",
            translated_orfs.len().to_string(),
            "",
        ),
        feature_row(
            "input",
            "file_or_inline",
            file_or_inline(value).to_string(),
            "",
        ),
    ];
    for (record_id, orf) in translated_orfs {
        rows.push(feature_row(
            "genome_or_metagenome",
            "translated_orf_identity",
            format!(
                "record={record_id};strand={};frame={};start={}",
                orf.strand, orf.frame, orf.start
            ),
            "",
        ));
        rows.extend(token_feature_rows(
            "genome_or_metagenome",
            "translated_orf_feature_token",
            protein_sequence_tokens(&orf.peptide),
        ));
    }
    if rows
        .iter()
        .filter(|row| row.name == "translated_orf_feature_token")
        .count()
        == 0
    {
        rows.extend(token_feature_rows(
            "genome_or_metagenome",
            "sequence_feature_token",
            records
                .iter()
                .flat_map(|(_, sequence)| tokenize(sequence))
                .collect(),
        ));
    }
    rows
}

fn feature_row(family: &str, name: &str, value: String, token: &str) -> FeatureRow {
    FeatureRow {
        family: family.to_string(),
        name: name.to_string(),
        value,
        token: token.to_ascii_lowercase(),
    }
}

fn token_feature_rows(family: &str, name: &str, tokens: BTreeSet<String>) -> Vec<FeatureRow> {
    tokens
        .into_iter()
        .map(|token| feature_row(family, name, token.clone(), &token))
        .collect()
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TranslatedOrf {
    peptide: String,
    strand: char,
    frame: usize,
    start: usize,
}

fn translate_orfs(sequence: &str) -> Vec<TranslatedOrf> {
    let forward = normalize_dna(sequence);
    let mut peptides = Vec::new();
    for (strand, strand_sequence) in [('+', forward.clone()), ('-', reverse_complement(&forward))] {
        let bases = strand_sequence.as_bytes();
        for frame in 0..3 {
            let mut pos = frame;
            while pos + 3 <= bases.len() {
                if &strand_sequence[pos..pos + 3] != "ATG" {
                    pos += 3;
                    continue;
                }
                let mut peptide = String::new();
                let mut codon_pos = pos;
                while codon_pos + 3 <= bases.len() {
                    let codon = &strand_sequence[codon_pos..codon_pos + 3];
                    if matches!(codon, "TAA" | "TAG" | "TGA") {
                        break;
                    }
                    peptide.push(codon_to_amino_acid(codon));
                    codon_pos += 3;
                }
                if peptide.len() >= 3 {
                    peptides.push(TranslatedOrf {
                        peptide,
                        strand,
                        frame,
                        start: pos,
                    });
                }
                pos += 3;
            }
        }
    }
    peptides
}

fn nucleotide_records(value: &str) -> Vec<(String, String)> {
    let mut records = Vec::new();
    let mut record_id = None::<String>;
    let mut sequence = String::new();
    for line in value.lines() {
        if let Some(header) = line.strip_prefix('>') {
            if let Some(previous_id) = record_id.replace(
                header
                    .split_whitespace()
                    .next()
                    .filter(|text| !text.is_empty())
                    .unwrap_or("record")
                    .to_string(),
            ) {
                records.push((previous_id, normalize_dna(&sequence)));
                sequence.clear();
            }
        } else {
            sequence.push_str(line);
        }
    }
    if let Some(last_id) = record_id {
        records.push((last_id, normalize_dna(&sequence)));
    } else {
        records.push(("record_1".to_string(), normalize_dna(value)));
    }
    records.retain(|(_, sequence)| !sequence.is_empty());
    records
}

fn normalize_dna(value: &str) -> String {
    value
        .chars()
        .filter_map(|ch| match ch.to_ascii_uppercase() {
            'A' | 'C' | 'G' | 'T' | 'N' => Some(ch.to_ascii_uppercase()),
            'U' => Some('T'),
            _ => None,
        })
        .collect()
}

fn reverse_complement(sequence: &str) -> String {
    sequence
        .chars()
        .rev()
        .map(|base| match base {
            'A' => 'T',
            'C' => 'G',
            'G' => 'C',
            'T' => 'A',
            _ => 'N',
        })
        .collect()
}

fn codon_to_amino_acid(codon: &str) -> char {
    match codon {
        "TTT" | "TTC" => 'F',
        "TTA" | "TTG" | "CTT" | "CTC" | "CTA" | "CTG" => 'L',
        "ATT" | "ATC" | "ATA" => 'I',
        "ATG" => 'M',
        "GTT" | "GTC" | "GTA" | "GTG" => 'V',
        "TCT" | "TCC" | "TCA" | "TCG" | "AGT" | "AGC" => 'S',
        "CCT" | "CCC" | "CCA" | "CCG" => 'P',
        "ACT" | "ACC" | "ACA" | "ACG" => 'T',
        "GCT" | "GCC" | "GCA" | "GCG" => 'A',
        "TAT" | "TAC" => 'Y',
        "CAT" | "CAC" => 'H',
        "CAA" | "CAG" => 'Q',
        "AAT" | "AAC" => 'N',
        "AAA" | "AAG" => 'K',
        "GAT" | "GAC" => 'D',
        "GAA" | "GAG" => 'E',
        "TGT" | "TGC" => 'C',
        "TGG" => 'W',
        "CGT" | "CGC" | "CGA" | "CGG" | "AGA" | "AGG" => 'R',
        "GGT" | "GGC" | "GGA" | "GGG" => 'G',
        _ => 'X',
    }
}

fn compact_sequence(value: &str) -> String {
    value
        .lines()
        .filter(|line| !line.starts_with('>'))
        .flat_map(|line| line.chars())
        .filter(|ch| !ch.is_whitespace())
        .collect()
}

fn is_standard_amino_acid(ch: char) -> bool {
    matches!(
        ch.to_ascii_uppercase(),
        'A' | 'C'
            | 'D'
            | 'E'
            | 'F'
            | 'G'
            | 'H'
            | 'I'
            | 'K'
            | 'L'
            | 'M'
            | 'N'
            | 'P'
            | 'Q'
            | 'R'
            | 'S'
            | 'T'
            | 'V'
            | 'W'
            | 'Y'
    )
}

fn file_or_inline(value: &str) -> &'static str {
    if Path::new(value.trim()).exists() {
        "existing_file"
    } else {
        "inline_or_declared_path"
    }
}

fn count_word_markers(value: &str, markers: &[&str]) -> usize {
    markers
        .iter()
        .map(|marker| value.matches(marker).count())
        .sum()
}

fn score_row(input: &ComputedFeatureInput, row: &StoreRow) -> f64 {
    let matched = matched_tokens(input, row);
    if matched.is_empty() {
        0.0
    } else {
        (matched.len() as f64 / input.tokens.len().max(1) as f64).min(1.0)
    }
}

fn matched_tokens(input: &ComputedFeatureInput, row: &StoreRow) -> Vec<String> {
    let mut db_tokens = row.ligand_tokens.clone();
    db_tokens.extend(row.sequence_tokens.iter().cloned());
    db_tokens.extend(row.structure_tokens.iter().cloned());
    input
        .tokens
        .intersection(&db_tokens)
        .cloned()
        .collect::<Vec<_>>()
}

fn initial_candidate_scope(scope: &str, organism: &str) -> String {
    match scope {
        "known_database" if organism.eq_ignore_ascii_case("Homo sapiens") => {
            "known_human_protein".to_string()
        }
        "known_database" => "known_nonhuman_protein".to_string(),
        "metagenome_potential" => "metagenome_potential_protein".to_string(),
        "structure_interaction" => "structure_interaction".to_string(),
        _ => "computed_candidate".to_string(),
    }
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
            candidate_scope: initial_candidate_scope(scope, &cols[4]),
            organism: cols[4].clone(),
            known_score: 0.0,
            metagenome_score: 0.0,
            structure_score: 0.0,
            docking_score: 0.0,
            dynamics_score: 0.0,
            final_score: 0.0,
            evidence: String::new(),
        });
        match scope {
            "known_database" => entry.known_score = entry.known_score.max(score),
            "metagenome_potential" => {
                entry.metagenome_score = entry.metagenome_score.max(score);
                if !entry.candidate_scope.starts_with("known_") {
                    entry.candidate_scope = "metagenome_potential_protein".to_string();
                }
            }
            "structure_interaction" => entry.structure_score = entry.structure_score.max(score),
            _ => {}
        }
        append_candidate_evidence(
            &mut entry.evidence,
            &candidate_evidence_summary(&cols[1], &cols[8]),
        );
    }
    Ok(())
}

fn candidate_evidence_summary(source: &str, evidence: &str) -> String {
    let mut parts = Vec::new();
    for part in evidence
        .split(';')
        .map(str::trim)
        .filter(|part| !part.is_empty())
    {
        let Some((key, value)) = part.split_once('=') else {
            parts.push(part.to_string());
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        if key == "pdb_ids" || key == "target_chain_pdb_ids" {
            let summarized = summarize_pdb_id_cell(value, 5);
            if !summarized.is_empty() {
                parts.push(format!("{key}={summarized}"));
            }
        } else if value.chars().count() <= 80 {
            parts.push(format!("{key}={value}"));
        }
    }
    let summary = if parts.is_empty() {
        evidence.trim().chars().take(180).collect::<String>()
    } else {
        parts.join(";")
    };
    format!("{source}:{summary}")
}

fn summarize_pdb_id_cell(value: &str, max_ids: usize) -> String {
    let ids = pdb_ids_from_text_in_order(value);
    if ids.len() <= max_ids {
        return ids.join(",");
    }
    format!(
        "{}(+{} more)",
        ids[..max_ids].join(","),
        ids.len() - max_ids
    )
}

fn append_candidate_evidence(evidence: &mut String, item: &str) {
    if evidence.contains(item) {
        return;
    }
    if !evidence.is_empty() {
        evidence.push(';');
    }
    evidence.push_str(item);
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

fn tokenize_limited(value: &str, max_chars: usize) -> BTreeSet<String> {
    let mut prefix = String::new();
    let mut char_count = 0usize;
    for ch in value.chars() {
        char_count += 1;
        if char_count <= max_chars {
            prefix.push(ch);
        }
    }
    if char_count <= max_chars {
        return tokenize(value);
    }
    let mut tokens = compact_sequence_tokens(&prefix);
    tokens.insert(format!("len_{char_count}"));
    tokens.insert(format!("hash_{}", stable_hash(value)));
    tokens
}

fn protein_sequence_tokens(value: &str) -> BTreeSet<String> {
    tokenize_limited(value, MAX_SEQUENCE_TOKEN_CHARS)
}

fn compact_sequence_tokens(value: &str) -> BTreeSet<String> {
    let compact = value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(|ch| ch.to_lowercase())
        .collect::<String>();
    let chars = compact.chars().collect::<Vec<_>>();
    let mut tokens = BTreeSet::new();
    if chars.is_empty() {
        return tokens;
    }
    let k = 6usize.min(chars.len());
    let max_tokens = 16usize;
    let step = ((chars.len().saturating_sub(k)) / max_tokens.max(1)).max(1);
    let mut pos = 0usize;
    while pos + k <= chars.len() && tokens.len() < max_tokens {
        tokens.insert(chars[pos..pos + k].iter().collect::<String>());
        pos += step;
    }
    if chars.len() > k {
        tokens.insert(chars[chars.len() - k..].iter().collect::<String>());
    }
    tokens
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

fn encode_tsv_cell(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\r', "\\r")
        .replace('\n', "\\n")
}

fn decode_tsv_cell(value: &str) -> String {
    let mut out = String::new();
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

fn run_python_script(script: &str, args: &[String], context: &str) -> Result<String, String> {
    let mut errors = Vec::new();
    let script_path = temp_python_script_path(context);
    fs::write(&script_path, script)
        .map_err(|err| format!("{context}_python_script_write_failed:{err}"))?;
    for candidate in script_python_command_candidates() {
        let mut command = candidate.clone();
        command.push(python_script_arg(
            &candidate,
            &script_path.display().to_string(),
        ));
        for arg in args {
            command.push(python_script_arg(&candidate, arg));
        }
        match command_output(&command) {
            Ok(output) if output.status.success() => {
                let _ = fs::remove_file(&script_path);
                return Ok(decode_process_bytes(&output.stdout));
            }
            Ok(output) => errors.push(format!(
                "{}:{}",
                candidate.join(" "),
                decode_process_bytes(&output.stderr)
                    .lines()
                    .next()
                    .unwrap_or("script_failed")
            )),
            Err(err) => errors.push(format!("{}:{err}", candidate.join(" "))),
        }
    }
    let _ = fs::remove_file(&script_path);
    Err(format!(
        "{context}_python_failed:{}",
        clean_cell(&errors.join(" | "))
    ))
}

fn temp_python_script_path(context: &str) -> PathBuf {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    let sequence = PYTHON_SCRIPT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    env::temp_dir().join(format!(
        "genoligand_{}_{}_{}_{}.py",
        safe_id(context),
        std::process::id(),
        millis,
        sequence
    ))
}

fn preferred_python_command() -> Vec<String> {
    std::env::var("GENOLIGAND_PYTHON")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(|value| vec![value])
        .unwrap_or_else(|| vec!["python".to_string()])
}

fn python_command_candidates() -> Vec<Vec<String>> {
    let preferred = preferred_python_command();
    if std::env::var("GENOLIGAND_PYTHON")
        .ok()
        .is_some_and(|value| !value.trim().is_empty())
    {
        return vec![preferred];
    }
    python_worker_candidates_from_env(env::var_os("LOCALAPPDATA"))
}

fn python_worker_candidates_from_env(local_appdata: Option<OsString>) -> Vec<Vec<String>> {
    let mut candidates = Vec::new();
    if let Some(local_appdata) = local_appdata {
        let runtime = PathBuf::from(local_appdata)
            .join("GenoLigand")
            .join("runtime")
            .join("python-worker-env");
        for rel in [
            PathBuf::from("genoligand-wsl-python.cmd"),
            PathBuf::from("Scripts").join("python.exe"),
            PathBuf::from("python.exe"),
            PathBuf::from("bin").join("python"),
        ] {
            let candidate = runtime.join(rel);
            if candidate.is_file() {
                candidates.push(vec![candidate.display().to_string()]);
            }
        }
    }
    candidates.extend([
        vec!["python".to_string()],
        vec!["python3".to_string()],
        vec!["py".to_string(), "-3".to_string()],
    ]);
    dedup_command_vecs(candidates)
}

fn dedup_command_vecs(candidates: Vec<Vec<String>>) -> Vec<Vec<String>> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for candidate in candidates {
        let key = candidate.join("\u{1f}");
        if seen.insert(key) {
            out.push(candidate);
        }
    }
    out
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

fn script_python_command_candidates() -> Vec<Vec<String>> {
    script_python_command_candidates_from_env(
        std::env::var("GENOLIGAND_PYTHON").ok(),
        env::var_os("LOCALAPPDATA"),
    )
}

#[cfg(test)]
fn script_python_command_candidates_from(preferred_python: Option<String>) -> Vec<Vec<String>> {
    script_python_command_candidates_from_env(preferred_python, env::var_os("LOCALAPPDATA"))
}

fn script_python_command_candidates_from_env(
    preferred_python: Option<String>,
    local_appdata: Option<OsString>,
) -> Vec<Vec<String>> {
    let mut candidates = Vec::new();
    if let Some(preferred) = preferred_python
        .filter(|value| !value.trim().is_empty())
        .map(|value| vec![value])
    {
        candidates.push(preferred);
    }
    candidates.extend(python_worker_candidates_from_env(local_appdata));
    dedup_command_vecs(candidates)
}

fn is_wsl_python_wrapper(candidate: &[String]) -> bool {
    cfg!(windows)
        && candidate.first().is_some_and(|python| {
            is_windows_script(python) && python.contains("genoligand-wsl-python")
        })
}

fn python_script_arg(candidate: &[String], arg: &str) -> String {
    if is_wsl_python_wrapper(candidate) {
        return windows_path_to_wsl(arg).unwrap_or_else(|| arg.to_string());
    }
    arg.to_string()
}

fn windows_path_to_wsl(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    if bytes.len() < 3 || bytes[1] != b':' || !bytes[0].is_ascii_alphabetic() {
        return None;
    }
    let slash = bytes[2];
    if slash != b'\\' && slash != b'/' {
        return None;
    }
    let drive = (bytes[0] as char).to_ascii_lowercase();
    let rest = value[3..].replace('\\', "/");
    Some(format!("/mnt/{drive}/{rest}"))
}

fn wsl_path_to_windows(value: &str) -> Option<PathBuf> {
    let rest = value.strip_prefix("/mnt/")?;
    let (drive, tail) = rest.split_once('/')?;
    if drive.len() != 1 || !drive.as_bytes()[0].is_ascii_alphabetic() {
        return None;
    }
    let drive = (drive.as_bytes()[0] as char).to_ascii_uppercase();
    Some(PathBuf::from(format!(
        "{drive}:\\{}",
        tail.replace('/', "\\")
    )))
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
mod tests;
