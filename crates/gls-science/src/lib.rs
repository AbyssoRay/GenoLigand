use flate2::read::GzDecoder;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use zip::ZipArchive;

const WORKER_SCHEMA_VERSION: &str = "genoligand_worker_current_v1";
const WORKER_PIPELINE_MODE: &str = "genoligand_scientific_pipeline";
const MAX_INDEX_ROWS: usize = 30_000;
const MAX_SOURCE_INDEX_ROWS: usize = 5_000;
const MAX_FASTA_INDEX_ROWS: usize = MAX_SOURCE_INDEX_ROWS;
const MAX_SEQUENCE_TOKEN_CHARS: usize = 96;
const KNOWN_SCORE_WEIGHT: f64 = 0.55;
const METAGENOME_SCORE_WEIGHT: f64 = 0.35;
const STRUCTURE_SCORE_WEIGHT: f64 = 0.10;
const DOCKING_SCORE_WEIGHT: f64 = 0.0;
const MD_SCORE_WEIGHT: f64 = 0.10;

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

#[derive(Clone, Copy, Debug)]
struct WorkerTask {
    job_id: &'static str,
    worker: &'static str,
    module: &'static str,
    stage: &'static str,
}

const WORKER_TASKS: &[WorkerTask] = &[
    WorkerTask {
        job_id: "openff_rdkit_smoke",
        worker: "openff_rdkit",
        module: "genoligand_workers.openff_worker",
        stage: "chemical_primitive",
    },
    WorkerTask {
        job_id: "pdbfixer_smoke",
        worker: "pdbfixer",
        module: "genoligand_workers.pdbfixer_worker",
        stage: "structure_preparation_primitive",
    },
    WorkerTask {
        job_id: "openmm_smoke",
        worker: "openmm",
        module: "genoligand_workers.openmm_worker",
        stage: "dynamics_primitive",
    },
    WorkerTask {
        job_id: "trajectory_features_smoke",
        worker: "trajectory_features",
        module: "genoligand_workers.trajectory_worker",
        stage: "trajectory_feature_primitive",
    },
    WorkerTask {
        job_id: "md_summary_ligand_dynamics",
        worker: "md_summary",
        module: "genoligand_workers.md_summary_worker",
        stage: "dynamics_summary_primitive",
    },
];

#[derive(Clone, Debug)]
struct ComputedFeatureInput {
    input_id: String,
    tokens: BTreeSet<String>,
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
    source: String,
    target_id: String,
    target_name: String,
    organism: String,
    target_class: String,
    match_score: f64,
    matched_features: String,
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
    if !known_rows.is_empty() {
        write_store_rows(
            &options.science_store.join("indexes/known_targets.tsv"),
            &known_rows,
        )?;
    }
    if !metagenome_rows.is_empty() {
        write_store_rows(
            &options.science_store.join("indexes/metagenome_targets.tsv"),
            &metagenome_rows,
        )?;
    }
    let mut structure_rows = build_structure_index_rows(options, &mut manifest)?;
    structure_rows = dedupe_store_rows(structure_rows);
    if !structure_rows.is_empty() {
        write_store_rows(
            &options.science_store.join("indexes/structure_targets.tsv"),
            &structure_rows,
        )?;
    }
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
    if rows.len() >= MAX_INDEX_ROWS {
        manifest.push_str(&format!(
            "{index_name}\t{}\tskipped\t0\t{}\tindex_row_limit_reached\n",
            relative_display(root, path),
            index_output_for(index_name)
        ));
        return Ok(());
    }
    if !path.exists() {
        manifest.push_str(&format!(
            "{index_name}\t{}\tmissing\t0\t{}\tnot_present\n",
            relative_display(root, path),
            index_output_for(index_name)
        ));
        return Ok(());
    }
    let (mut parsed, notes) = read_source_rows(path, index_name)?;
    let available = MAX_INDEX_ROWS
        .saturating_sub(rows.len())
        .min(MAX_SOURCE_INDEX_ROWS);
    let parsed_total = parsed.len();
    if parsed.len() > available {
        parsed.truncate(available);
    }
    let count = parsed.len();
    rows.extend(parsed);
    let note_suffix = if parsed_total > count {
        ";index_row_limit_reached"
    } else {
        ""
    };
    manifest.push_str(&format!(
        "{index_name}\t{}\t{}\t{count}\t{}\t{notes}{note_suffix}\n",
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
    let available = MAX_INDEX_ROWS
        .saturating_sub(rows.len())
        .min(MAX_SOURCE_INDEX_ROWS);
    let total = built_rows.len();
    let count = total.min(available);
    rows.extend(built_rows.into_iter().take(count));
    let note_suffix = if total > count {
        ";index_row_limit_reached"
    } else {
        ""
    };
    manifest.push_str(&format!(
        "{index_name}\t{}\t{}\t{count}\t{}\t{notes}{note_suffix}\n",
        relative_display(root, path),
        if count > 0 { "used" } else { "empty" },
        index_output_for(index_name)
    ));
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
    for path in discover_plinder_tables(&options.science_store.join("downloads/plinder"))? {
        let built = structure_file_row("PLINDER", &path);
        append_built_rows(
            "structure_targets",
            &options.science_store,
            &path,
            vec![built],
            &mut rows,
            manifest,
            "plinder_annotation_rows",
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
        rows.len() >= MAX_FASTA_INDEX_ROWS
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
    let mut sequence_tokens = tokenize_limited(sequence, MAX_SEQUENCE_TOKEN_CHARS);
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
    normalize_input(options)?;
    compute_features(options)?;
    match_known(options)?;
    match_metagenome(options)?;
    rank_targets(options)?;
    evaluate_structure(options)?;
    run_schema_bound_workers(options)?;
    screen_dynamics(options)?;
    rank_targets(options)?;
    build_evidence(options)?;
    render_report(options)?;
    write_artifact_source_map(options)?;
    write_text(&options.run_dir.join("run_state.txt"), "completed\n")?;
    write_run_context(options, "completed", &[])?;
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
    Ok(blockers)
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
            encode_tsv_cell(&input.primary_value),
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
        "input_id\tinput_kind\tfeature_family\tfeature_name\tfeature_value\tfeature_source\tfeature_token\n",
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
                "{}\t{}\t{family}\t{name}\t{value}\tGenoLigand\t\n",
                input.input_id,
                input.kind.as_str()
            ));
        }
        for row in typed_feature_rows(&input) {
            text.push_str(&format!(
                "{}\t{}\t{}\t{}\t{}\tGenoLigand\t{}\n",
                input.input_id,
                input.kind.as_str(),
                clean_cell(&row.family),
                clean_cell(&row.name),
                clean_cell(&row.value),
                clean_cell(&row.token)
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
    let inputs = read_computed_feature_inputs(&options.run_dir.join("feature_manifest.tsv"))?;
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
    let docking = options.run_dir.join("docking_summary.tsv");
    let structure_ready =
        if copy_tool_output_if_available(options, "structure_patch_summary.tsv", &structure)? {
            true
        } else {
            write_structure_summary_from_matches(&structure, &structure_matches)?;
            !structure_matches.is_empty()
        };
    let docking_ready = copy_tool_output_or_header(
        options,
        "docking_summary.tsv",
        "candidate_id\tdocking_status\tdocking_score\tdocking_engine\tnotes\n",
        &docking,
    )?;
    let viewer_ready = copy_structure_viewer_artifact_if_available(options)?
        || write_structure_viewer_from_matches(options, &structure_matches)?;
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
        "input_id\tsource\ttarget_id\ttarget_name\torganism\ttarget_class\tmatch_score\tmatched_features\tevidence\n",
    )
}

fn run_schema_bound_workers(options: &ScienceOptions) -> Result<String, String> {
    let job_dir = options.run_dir.join("worker_jobs");
    let result_dir = options.run_dir.join("worker_results");
    ensure_dir(&job_dir)?;
    ensure_dir(&result_dir)?;
    let mut summary = String::from(
        "job_id\tworker\tstage\tjob_schema_valid\tlaunch_status\tresult_schema_valid\tresult_status\tengine_version\tprimitive_execution_only\tpython_owns_final_score\tpython_owns_report_semantics\n",
    );
    for task in WORKER_TASKS {
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
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            task.job_id,
            task.worker,
            task.stage,
            job_schema_valid,
            clean_cell(&launch_status),
            result_schema_valid,
            clean_cell(result_status),
            clean_cell(engine_version),
            primitive_execution_only,
            python_owns_final_score,
            python_owns_report_semantics
        ));
    }
    write_text(
        &options.run_dir.join("worker_runtime_summary.tsv"),
        &summary,
    )?;
    Ok(format!(
        "schema_bound_workers_checked\tworker_count={}",
        WORKER_TASKS.len()
    ))
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
    let value = json!({
        "schema_version": WORKER_SCHEMA_VERSION,
        "job_id": task.job_id,
        "worker": task.worker,
        "status": "FAIL",
        "engine_version": format!("worker unavailable: {}", clean_cell(reason)),
        "python_owns_final_score": false,
        "python_owns_report_semantics": false,
        "primitive_execution_only": true
    });
    write_text(path, &(value.to_string() + "\n"))
}

fn validate_worker_result(value: &Value, task: &WorkerTask) -> bool {
    value.get("schema_version").and_then(Value::as_str) == Some(WORKER_SCHEMA_VERSION)
        && value.get("job_id").and_then(Value::as_str) == Some(task.job_id)
        && value.get("worker").and_then(Value::as_str) == Some(task.worker)
        && matches!(
            value.get("status").and_then(Value::as_str),
            Some("PASS") | Some("FAIL")
        )
        && value
            .get("engine_version")
            .and_then(Value::as_str)
            .is_some()
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

fn match_structure_interactions(options: &ScienceOptions) -> Result<Vec<IndexMatch>, String> {
    let inputs = read_computed_feature_inputs(&options.run_dir.join("feature_manifest.tsv"))?;
    let rows = read_store_rows(
        &options.science_store.join("indexes/structure_targets.tsv"),
        "structure_interaction_index",
    )?;
    let mut matches = Vec::new();
    let mut out = String::from(
        "input_id\tsource\ttarget_id\ttarget_name\torganism\ttarget_class\tmatch_score\tmatched_features\tevidence\n",
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
                "{}\t{}\t{}\t{}\t{}\t{}\t{:.4}\t{}\t{}\n",
                item.input_id,
                clean_cell(&item.source),
                clean_cell(&item.target_id),
                clean_cell(&item.target_name),
                clean_cell(&item.organism),
                clean_cell(&item.target_class),
                item.match_score,
                clean_cell(&item.matched_features),
                clean_cell(&item.evidence)
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
    let mut ids = BTreeSet::new();
    for value in [
        item.target_id.as_str(),
        item.target_name.as_str(),
        item.matched_features.as_str(),
        item.evidence.as_str(),
    ] {
        for token in value.split(|ch: char| !ch.is_ascii_alphanumeric()) {
            let upper = token.trim().to_ascii_uppercase();
            if is_pdb_id(&upper) {
                ids.insert(upper);
            }
        }
    }
    ids.into_iter().collect()
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
    merge_match_file(
        &options.run_dir.join("structure_interaction_matches.tsv"),
        "structure_interaction",
        &mut by_target,
    )?;
    let mut candidates = by_target.into_values().collect::<Vec<_>>();
    apply_md_scores(&mut candidates, &options.run_dir.join("md_summary.tsv"))?;
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

fn apply_md_scores(candidates: &mut [Candidate], path: &Path) -> Result<(), String> {
    let md_scores = read_md_scores(path)?;
    if md_scores.is_empty() {
        return Ok(());
    }
    for candidate in candidates {
        if let Some(score) = md_scores.get(&candidate.candidate_id) {
            candidate.dynamics_score = *score;
            append_evidence_once(&mut candidate.evidence, "MD:md_summary_stability_score");
        }
    }
    Ok(())
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
    write_score_decomposition(options)?;
    write_artifact_source_map(options)?;
    let files = [
        "run_context.json",
        "artifact_source_map.tsv",
        "input_normalized_manifest.tsv",
        "feature_manifest.tsv",
        "known_database_matches.tsv",
        "metagenome_potential_matches.tsv",
        "structure_interaction_matches.tsv",
        "worker_runtime_summary.tsv",
        "structure_patch_summary.tsv",
        "docking_summary.tsv",
        "md_summary.tsv",
        "tool_outputs/md_summary.tsv",
        "tool_outputs/md_rmsd.tsv",
        "tool_outputs/md_rmsf.tsv",
        "tool_outputs/md_ligand_drift.tsv",
        "tool_outputs/md_contacts.tsv",
        "tool_outputs/md_hbonds.tsv",
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
        "worker_runtime_summary.tsv" => {
            if rows > 0 {
                let text = read_text(path)?;
                let pass_count = text
                    .lines()
                    .skip(1)
                    .filter(|line| line.contains("\tPASS\t"))
                    .count();
                (
                    "available",
                    format!("{rows} worker checks recorded; {pass_count} passed"),
                )
            } else {
                ("not_generated", "no worker runtime rows".to_string())
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
        "md_summary.tsv" => {
            if rows > 0 {
                (
                    "available",
                    format!(
                        "{rows} short dynamics screening rows from current run metrics; md_stability_score feeds md_score and final_score with weight {MD_SCORE_WEIGHT:.2}; restricted receptor model screening evidence"
                    ),
                )
            } else {
                ("not_generated", "no dynamics rows".to_string())
            }
        }
        file if file.starts_with("tool_outputs/md_") && file.ends_with(".tsv") => {
            if rows > 0 {
                (
                    "available",
                    format!("{rows} current-run MD metric artifact rows for screening summary"),
                )
            } else {
                ("not_generated", "no MD metric artifact rows".to_string())
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
        for (index, candidate) in candidates.iter().enumerate() {
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
    Ok(format!(
        "## \u{52a8}\u{529b}\u{5b66}\u{8bc4}\u{5206}\n\n- \u{6765}\u{6e90}\u{ff1a}md_summary.tsv \u{4e2d}\u{7684} md_stability_score\u{ff0c}\u{7531} tool_outputs/md_rmsd.tsv\u{3001}tool_outputs/md_rmsf.tsv\u{3001}tool_outputs/md_ligand_drift.tsv\u{3001}tool_outputs/md_contacts.tsv \u{548c} tool_outputs/md_hbonds.tsv \u{7b49}\u{5f53}\u{524d}\u{8fd0}\u{884c}\u{8f68}\u{8ff9}\u{6307}\u{6807}\u{6c47}\u{603b}\u{5f97}\u{5230}\u{3002}\n- \u{7528}\u{9014}\u{ff1a}\u{5199}\u{5165} Candidate Ranking \u{7684} md_score\u{ff0c}\u{5e76}\u{4ee5} {MD_SCORE_WEIGHT:.2} \u{6743}\u{91cd}\u{53c2}\u{4e0e} final_score\u{ff1b}\u{672c}\u{8fd0}\u{884c} {ranked_md} \u{4e2a}\u{5019}\u{9009}\u{6709} MD \u{5206}\u{6570}\u{3002}\n- \u{4f7f}\u{7528}\u{8bf4}\u{660e}\u{ff1a}\u{5f53}\u{524d} MD \u{4f5c}\u{4e3a}\u{77ed}\u{7a0b}\u{7a33}\u{5b9a}\u{6027}\u{7b5b}\u{9009}\u{8bc1}\u{636e}\u{ff0c}\u{4f7f}\u{7528}\u{5f53}\u{524d}\u{8fd0}\u{884c}\u{7684}\u{914d}\u{4f53}\u{653e}\u{7f6e}\u{548c}\u{53d7}\u{9650} receptor \u{6a21}\u{578b}\u{ff1b}\u{9002}\u{5408}\u{5019}\u{9009}\u{4f18}\u{5148}\u{7ea7}\u{5224}\u{65ad}\u{ff0c}\u{5b8c}\u{6574}\u{751f}\u{4ea7}\u{7ea7} MD \u{548c}\u{5b9e}\u{9a8c}\u{7ed3}\u{679c}\u{7531}\u{540e}\u{7eed}\u{5b9e}\u{9a8c}\u{4e0e}\u{957f}\u{7a0b}\u{6a21}\u{62df}\u{8865}\u{5145}\u{3002}\n- OpenMM \u{5e73}\u{53f0}\u{ff1a}{platform}\u{ff1b}\u{8f68}\u{8ff9}\u{4ea7}\u{7269}\u{ff1a}{trajectory}\u{3002}\n\n"
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
                "input_normalized_manifest.tsv",
                "feature_manifest.tsv",
                "known_database_matches.tsv",
                "metagenome_potential_matches.tsv",
                "structure_interaction_matches.tsv",
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
            "input_manifest.json",
            "feature_manifest.tsv",
        ),
        (
            "feature_manifest.tsv",
            "input_normalized_manifest.tsv",
            "known_database_matches.tsv;metagenome_potential_matches.tsv",
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
            "indexes/structure_targets.tsv;feature_manifest.tsv",
            "target_candidate_ranking.tsv;structure_patch_summary.tsv",
        ),
        (
            "worker_runtime_summary.tsv",
            "Rust-generated worker job specs;schema-bound primitive worker results",
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
            "evidence_ledger.tsv",
        ),
        (
            "md_summary.tsv",
            "tool_outputs/md_run_manifest.json;tool_outputs/md_summary.tsv;tool_outputs/md_rmsd.tsv;tool_outputs/md_rmsf.tsv;tool_outputs/md_ligand_drift.tsv;tool_outputs/md_contacts.tsv;tool_outputs/md_hbonds.tsv",
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
            "tool_outputs/ligand_placement.tsv",
            "current run ligand or peptide coordinates from worker calculation",
            "md_summary.tsv;tool_outputs/complex_md_manifest.json;tool_outputs/md_run_manifest.json",
        ),
        (
            "tool_outputs/md_trajectory.tsv",
            "current run MD or peptide conformer trajectory coordinates",
            "md_summary.tsv",
        ),
        (
            "tool_outputs/complex_md_manifest.json",
            "current run receptor coordinate, ligand coordinate, OpenMM platform, trajectory and metric artifacts",
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

fn extract_inputs(path: &Path) -> Result<Vec<NormalizedInput>, String> {
    let text = read_text(path)?;
    let value =
        serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}')).unwrap_or(Value::Null);
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
        InputKind::SmallMolecule => &[
            "small_molecule",
            "small molecule",
            "smiles",
            "sdf",
            "mol2",
            "compound",
            "cas",
        ],
        InputKind::Peptide => &["peptide"],
        InputKind::ProteinLigand => &[
            "protein_ligand",
            "protein ligand",
            "protein",
            "fasta_sequence",
        ],
        InputKind::Complex => &["complex"],
        InputKind::GenomeOrMetagenome => &[
            "genome_or_metagenome",
            "genome",
            "metagenome",
            "transcript",
            "gff",
            "gtf",
            "vcf",
        ],
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
            primary_value: decode_tsv_cell(&cols[2]),
            file_count: cols[3].parse().unwrap_or(0),
            tokens: cols[4].split_whitespace().map(ToOwned::to_owned).collect(),
        });
    }
    Ok(out)
}

fn read_computed_feature_inputs(path: &Path) -> Result<Vec<ComputedFeatureInput>, String> {
    let text = read_text(path)?;
    let mut by_input = BTreeMap::<String, ComputedFeatureInput>::new();
    for line in text.lines().skip(1) {
        let cols = split_tsv(line);
        if cols.len() < 7 {
            continue;
        }
        if !matches!(
            cols[1].as_str(),
            "small_molecule" | "peptide" | "protein_ligand" | "complex" | "genome_or_metagenome"
        ) {
            continue;
        }
        let token = cols[6].trim();
        let entry = by_input
            .entry(cols[0].clone())
            .or_insert_with(|| ComputedFeatureInput {
                input_id: cols[0].clone(),
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
        if rows.len() >= MAX_SOURCE_INDEX_ROWS {
            break;
        }
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
LIMIT 5000
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
    sequence_tokens.extend(tokenize_limited(&row.sequences, MAX_SEQUENCE_TOKEN_CHARS));
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
        if rows.len() >= MAX_SOURCE_INDEX_ROWS {
            break;
        }
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
        let mut sequence_tokens = tokenize_limited(&sequence, MAX_SEQUENCE_TOKEN_CHARS);
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
        tokenize(&sequence),
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
    let sequence = compact_sequence(value);
    let nucleotide_count = sequence
        .chars()
        .filter(|ch| matches!(ch.to_ascii_uppercase(), 'A' | 'C' | 'G' | 'T' | 'U' | 'N'))
        .count();
    let gc_count = sequence
        .chars()
        .filter(|ch| matches!(ch.to_ascii_uppercase(), 'G' | 'C'))
        .count();
    let gc_fraction = if nucleotide_count > 0 {
        format!("{:.4}", gc_count as f64 / nucleotide_count as f64)
    } else {
        "0.0000".to_string()
    };
    let translated_orfs = translate_orfs(&sequence);
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
            sequence
                .to_ascii_uppercase()
                .matches("ATG")
                .count()
                .to_string(),
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
    for peptide in translated_orfs {
        rows.extend(token_feature_rows(
            "genome_or_metagenome",
            "translated_orf_feature_token",
            tokenize(&peptide),
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
            tokenize(&sequence),
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

fn translate_orfs(sequence: &str) -> Vec<String> {
    let upper = sequence.to_ascii_uppercase();
    let bases = upper.as_bytes();
    let mut peptides = Vec::new();
    for frame in 0..3 {
        let mut pos = frame;
        while pos + 3 <= bases.len() {
            if &upper[pos..pos + 3] != "ATG" {
                pos += 3;
                continue;
            }
            let mut peptide = String::new();
            let mut codon_pos = pos;
            while codon_pos + 3 <= bases.len() {
                let codon = &upper[codon_pos..codon_pos + 3];
                if matches!(codon, "TAA" | "TAG" | "TGA") {
                    break;
                }
                peptide.push(codon_to_amino_acid(codon));
                codon_pos += 3;
            }
            if peptide.len() >= 3 {
                peptides.push(peptide);
            }
            pos += 3;
        }
    }
    peptides
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
    for candidate in script_python_command_candidates() {
        let mut command = candidate.clone();
        command.push("-c".to_string());
        command.push(script.to_string());
        for arg in args {
            command.push(python_script_arg(&candidate, arg));
        }
        match command_output(&command) {
            Ok(output) if output.status.success() => {
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
    Err(format!(
        "{context}_python_failed:{}",
        clean_cell(&errors.join(" | "))
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
    let mut candidates = vec![
        vec!["python".to_string()],
        vec!["python3".to_string()],
        vec!["py".to_string(), "-3".to_string()],
    ];
    if let Some(preferred) = std::env::var("GENOLIGAND_PYTHON")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(|value| vec![value])
    {
        candidates.push(preferred);
    }
    candidates
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
    fn openmm_cuda_ready_follows_probe_detail_flag() {
        let cuda_probe = ToolProbe {
            ready: true,
            version: "8.2.0".to_string(),
            detail: "openmm_python_module_detected;platforms=Reference,CPU,CUDA;cuda=ready"
                .to_string(),
        };
        let cpu_probe = ToolProbe {
            ready: true,
            version: "8.2.0".to_string(),
            detail: "openmm_python_module_detected;platforms=Reference,CPU;cuda=missing"
                .to_string(),
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

        let dirs = command_wrapper_dirs_from_env(
            Some(local.into_os_string()),
            Some(python.into_os_string()),
        );

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
        let detail = read_text(&run.join("candidate_detail.json")).expect("candidate detail");
        let context = read_text(&run.join("run_context.json")).expect("run context");
        let source_map = read_text(&run.join("artifact_source_map.tsv")).expect("source map");
        let evidence = read_text(&run.join("evidence_ledger.tsv")).expect("evidence ledger");
        let report = read_text(&run.join("research_report.md")).expect("research report");
        let structure =
            read_text(&run.join("structure_patch_summary.tsv")).expect("structure summary");
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
        assert!(
            evidence.contains("docking_summary.tsv\tGenoLigand\tnot_generated\tno docking rows")
        );
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
            "{\"inputs\":[{\"field\":\"small_molecule\",\"value\":\"CCO kinase\"}],\"files\":[]}\n",
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
        let result =
            read_text(&run.join("worker_results/openmm_smoke.json")).expect("worker result");
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
{"field":"protein_ligand","value":">protein\nMKTAYIAKQRQISFVKSHFSRQDILDLWQ"},
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
        let with_features =
            read_text(&run.join("known_database_matches.tsv")).expect("known matches");
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
{"field":"protein_ligand","value":"MKTAYIAKQ"},
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
        let dynamics =
            run_science_command(ScienceCommand::DynamicsScreen, &options).expect("dynamics");
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

        let out =
            run_science_command(ScienceCommand::StructureEvaluate, &options).expect("structure");
        let viewer = read_text(&run.join("structure_viewer.cif")).expect("viewer cif");
        let status = read_text(&run.join("structure_viewer_status.tsv")).expect("viewer status");

        assert!(out.contains("viewer_artifact=ready"));
        assert!(viewer.contains("ATOM 1"));
        assert!(status.contains("ready\t10GS\t10GS\tstructure_viewer.cif"));
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
        let out = run_store_command(ScienceCommand::PrepareScienceStore, &options)
            .expect("prepare store");
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
        let _ = run_store_command(ScienceCommand::PrepareScienceStore, &options)
            .expect("prepare store");
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
        let _ = run_store_command(ScienceCommand::PrepareScienceStore, &options)
            .expect("prepare store");
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
        let _ = run_store_command(ScienceCommand::PrepareScienceStore, &options)
            .expect("prepare store");
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
        let _ = run_store_command(ScienceCommand::PrepareScienceStore, &options)
            .expect("prepare store");
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
        let _ = run_store_command(ScienceCommand::PrepareScienceStore, &options)
            .expect("prepare store");
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
        let _ = run_store_command(ScienceCommand::PrepareScienceStore, &options)
            .expect("prepare store");
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
        let _ = run_store_command(ScienceCommand::PrepareScienceStore, &options)
            .expect("prepare store");
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
        let _ = run_store_command(ScienceCommand::PrepareScienceStore, &options)
            .expect("prepare store");
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
        let _ = run_store_command(ScienceCommand::PrepareScienceStore, &options)
            .expect("prepare store");
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
        let file = fs::File::create(path)
            .map_err(|err| format!("create_failed:{}:{err}", path.display()))?;
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
        let file = fs::File::create(path)
            .map_err(|err| format!("create_failed:{}:{err}", path.display()))?;
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

    fn write_tar_gzip_file(
        path: &Path,
        member_name: &str,
        source_path: &Path,
    ) -> Result<(), String> {
        use flate2::Compression;
        use flate2::write::GzEncoder;

        if let Some(parent) = path.parent() {
            ensure_dir(parent)?;
        }
        let file = fs::File::create(path)
            .map_err(|err| format!("create_failed:{}:{err}", path.display()))?;
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
}
