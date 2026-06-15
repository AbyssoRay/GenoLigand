use flate2::read::GzDecoder;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use zip::ZipArchive;

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

pub fn create_temporary_test_store(root: &Path) -> Result<PathBuf, String> {
    let store = root.join("science_store");
    ensure_dir(&store.join("indexes"))?;
    write_text(
        &store.join("science_store_manifest.tsv"),
        "item\tstatus\tpath\nwarehouse\tready\tindexes/known_targets.tsv\nmetagenome_index\tready\tindexes/metagenome_targets.tsv\ntool_versions\tready\tindexes/tool_versions.tsv\n",
    )?;
    write_text(
        &store.join("indexes/tool_versions.tsv"),
        &temporary_tool_versions_text(),
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
        &store.join("install_status.json"),
        &format!(
            "{}\n",
            json!({
                "status": "ready",
                "fixture": "unit_test_only",
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

fn temporary_tool_versions_text() -> String {
    let mut text = String::from("tool\tstatus\tversion\tlicense\tlabel\tdetail\n");
    for tool in REQUIRED_TOOLS {
        text.push_str(&format!(
            "{}\tready\ttest-bound\t{}\t{}\ttemporary_test_fixture\n",
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
                "required_items": 3,
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
                    "metagenome": build_summary.metagenome_rows
                }
            })
        ),
    )?;
    Ok(format!(
        "science_store_prepared\tready_items={ready}\trequired_items=3\tstatus={}\tmissing_components={}\tstore={}",
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
    write_text(manifest_path, &manifest)?;
    Ok(IndexBuildSummary {
        known_rows: known_rows.len(),
        metagenome_rows: metagenome_rows.len(),
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
) {
    let count = built_rows.len();
    rows.extend(built_rows);
    manifest.push_str(&format!(
        "{index_name}\t{}\t{}\t{count}\t{}\tfasta_sequence_rows\n",
        relative_display(root, path),
        if count > 0 { "used" } else { "empty" },
        index_output_for(index_name)
    ));
}

fn index_output_for(index_name: &str) -> &'static str {
    match index_name {
        "known_targets" => "indexes/known_targets.tsv",
        "metagenome_targets" => "indexes/metagenome_targets.tsv",
        _ => "indexes/unknown.tsv",
    }
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

fn is_chembl_sqlite_source(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    is_sqlite_name(&name) || (name.ends_with(".tar.gz") && name.contains("chembl"))
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
    let text = read_sequence_text(path)?;
    let mut rows = Vec::new();
    let mut header = String::new();
    let mut sequence = String::new();
    for line in text.lines() {
        let line = line.trim_start_matches('\u{feff}');
        if let Some(rest) = line.strip_prefix('>') {
            push_fasta_row(
                source,
                default_organism,
                target_class,
                evidence,
                &header,
                &sequence,
                &mut rows,
            );
            header = rest.trim().to_string();
            sequence.clear();
        } else {
            sequence.push_str(line.trim());
        }
    }
    push_fasta_row(
        source,
        default_organism,
        target_class,
        evidence,
        &header,
        &sequence,
        &mut rows,
    );
    Ok(rows)
}

fn read_sequence_text(path: &Path) -> Result<String, String> {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if name.ends_with(".gz") {
        read_gzip_text(path)
    } else if name.ends_with(".zip") {
        read_zip_text(path, matches_sequence_name)
    } else {
        read_text(path)
    }
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
    let mut sequence_tokens = tokenize(sequence);
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
    for tool in REQUIRED_TOOLS {
        let probe = probe_tool(tool);
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
        "openbabel" => probe_command(tool.id, "obabel", &["-V"]),
        "mmseqs2" => probe_command(tool.id, "mmseqs", &["version"]),
        "foldseek" => probe_command(tool.id, "foldseek", &["version"]),
        "autodock_vina" => probe_command(tool.id, "vina", &["--version"]),
        "openmm" => probe_python_module(tool.id, "openmm", "openmm.version.version"),
        "pdbfixer" => probe_python_module(tool.id, "pdbfixer", "'installed'"),
        "mdtraj" => probe_python_module(tool.id, "mdtraj", "mdtraj.__version__"),
        "mdanalysis" => probe_python_module(tool.id, "MDAnalysis", "MDAnalysis.__version__"),
        _ => ToolProbe {
            ready: false,
            version: "unknown".to_string(),
            detail: format!("no_probe_defined_for:{}", tool.id),
        },
    }
}

fn probe_command(tool_id: &str, program: &str, args: &[&str]) -> ToolProbe {
    match Command::new(program).args(args).output() {
        Ok(output) => {
            let text = format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            ToolProbe {
                ready: output.status.success(),
                version: if output.status.success() {
                    first_nonempty_line(&text).unwrap_or_else(|| "installed".to_string())
                } else {
                    "missing".to_string()
                },
                detail: if output.status.success() {
                    format!("{tool_id}_detected")
                } else {
                    format!("{tool_id}_probe_failed")
                },
            }
        }
        Err(err) => ToolProbe {
            ready: false,
            version: "missing".to_string(),
            detail: format!("{tool_id}_not_found:{err}"),
        },
    }
}

fn probe_python_module(tool_id: &str, module: &str, version_expr: &str) -> ToolProbe {
    let code = format!("import {module}; print({version_expr})");
    match Command::new("python").args(["-c", &code]).output() {
        Ok(output) => {
            let text = format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            ToolProbe {
                ready: output.status.success(),
                version: if output.status.success() {
                    first_nonempty_line(&text).unwrap_or_else(|| "installed".to_string())
                } else {
                    "missing".to_string()
                },
                detail: if output.status.success() {
                    format!("{tool_id}_python_module_detected")
                } else {
                    format!("{tool_id}_python_module_missing")
                },
            }
        }
        Err(err) => ToolProbe {
            ready: false,
            version: "missing".to_string(),
            detail: format!("python_probe_failed_for_{tool_id}:{err}"),
        },
    }
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
    let status = options.science_store.join("install_status.json");
    if status.exists() {
        let value: Value = serde_json::from_str(&read_text(&status)?)
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
    write_candidate_detail(options, &candidates)?;
    Ok(format!(
        "targets_ranked\tcandidate_count={}",
        candidates.len()
    ))
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
        "structure_patch_summary.tsv",
        "docking_summary.tsv",
        "md_summary.tsv",
        "target_candidate_ranking.tsv",
        "score_decomposition.tsv",
        "candidate_detail.json",
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
    write_text(&options.run_dir.join("evidence_ledger.tsv"), &text)?;
    Ok("evidence_built".to_string())
}

fn render_report(options: &ScienceOptions) -> Result<String, String> {
    let candidates = read_candidates(&options.run_dir.join("target_candidate_ranking.tsv"))?;
    let mut text = String::from("# GenoLigand 科研报告\n\n");
    text.push_str(
        "本报告只包含计算预测和证据优先级，不代表实验验证、体内结合、药效或作用机制确认。\n\n",
    );
    text.push_str("## 候选靶点\n\n");
    if candidates.is_empty() {
        text.push_str(
            "当前运行没有生成候选靶点。可能原因是没有匹配证据，或计算在开始前被运行条件阻塞。\n",
        );
    } else {
        text.push_str("| 排序 | 靶点 | 候选范围 | 物种 | 综合分数 | 证据摘要 |\n");
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
            "\n建议把排序靠前的候选作为后续实验验证的优先对象，而不是直接视为已确认靶点。\n",
        );
    }
    write_text(&options.run_dir.join("research_report.md"), &text)?;
    Ok("report_rendered".to_string())
}

fn report_scope_label(value: &str) -> &'static str {
    match value {
        "known_database" | "known_human_protein" => "已知蛋白数据库",
        "metagenome_potential" | "metagenome_potential_protein" => "宏基因组潜在序列",
        "structure_interaction" => "结构互作证据",
        _ => "计算候选",
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
    let rows = candidates
        .iter()
        .map(|candidate| {
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
                "source_artifacts": [
                    "input_normalized_manifest.tsv",
                    "feature_manifest.tsv",
                    "known_database_matches.tsv",
                    "metagenome_potential_matches.tsv",
                    "target_candidate_ranking.tsv",
                    "evidence_ledger.tsv"
                ],
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
            "known_database_matches.tsv;metagenome_potential_matches.tsv",
            "candidate_detail.json;structure_patch_summary.tsv;md_summary.tsv",
        ),
        (
            "structure_patch_summary.tsv",
            "target_candidate_ranking.tsv",
            "evidence_ledger.tsv",
        ),
        (
            "docking_summary.tsv",
            "target_candidate_ranking.tsv",
            "evidence_ledger.tsv",
        ),
        (
            "md_summary.tsv",
            "target_candidate_ranking.tsv",
            "evidence_ledger.tsv",
        ),
        (
            "score_decomposition.tsv",
            "target_candidate_ranking.tsv",
            "evidence_ledger.tsv",
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
        text.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\n",
            clean_cell(&project_id),
            clean_cell(&run_id),
            artifact,
            clean_cell(upstream),
            clean_cell(downstream),
            if options.run_dir.join(artifact).exists() {
                "available"
            } else {
                "not_yet_written"
            }
        ));
    }
    write_text(&options.run_dir.join("artifact_source_map.tsv"), &text)
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
    let text = read_table_text(path).map_err(|err| format!("{label}_missing:{err}"))?;
    let mut lines = text.lines();
    let Some(header_line) = lines.next() else {
        return Ok(Vec::new());
    };
    let headers = split_tsv(header_line.trim_start_matches('\u{feff}'));
    if is_bindingdb_header(&headers) {
        return Ok(parse_bindingdb_rows(&headers, lines));
    }
    let mut rows = Vec::new();
    for line in lines {
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
LIMIT 200000
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
    sequence_tokens.extend(tokenize(&row.sequences));
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
            "Target Chain Sequence",
            "Target Sequence",
        ],
    )
    .is_some()
        && find_header(headers, &["Target Name", "Target", "BindingDB Target Name"]).is_some()
}

fn parse_bindingdb_rows<'a, I>(headers: &[String], lines: I) -> Vec<StoreRow>
where
    I: IntoIterator<Item = &'a str>,
{
    let target_name_idx = find_header(headers, &["Target Name", "Target", "BindingDB Target Name"]);
    let sequence_idx = find_header(
        headers,
        &[
            "BindingDB Target Chain Sequence",
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
            "UniProt SwissProt Primary ID of Target Chain",
            "UniProt Primary ID",
        ],
    );
    let trembl_idx = find_header(
        headers,
        &[
            "UniProt (TrEMBL) Primary ID of Target Chain",
            "UniProt TrEMBL Primary ID of Target Chain",
        ],
    );
    let pdb_idx = find_header(
        headers,
        &[
            "Target Chain PDB ID(s)",
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
        let mut sequence_tokens = tokenize(&sequence);
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
    rows
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

fn run_python_script(script: &str, args: &[String], context: &str) -> Result<String, String> {
    let candidates: [(&str, &[&str]); 3] = [("python", &[]), ("python3", &[]), ("py", &["-3"])];
    let mut errors = Vec::new();
    for (program, prefix_args) in candidates {
        let mut command = Command::new(program);
        for arg in prefix_args {
            command.arg(arg);
        }
        command.arg("-c").arg(script);
        for arg in args {
            command.arg(arg);
        }
        match command.output() {
            Ok(output) if output.status.success() => {
                return Ok(String::from_utf8_lossy(&output.stdout).to_string());
            }
            Ok(output) => errors.push(format!(
                "{program}:{}",
                String::from_utf8_lossy(&output.stderr)
                    .lines()
                    .next()
                    .unwrap_or("script_failed")
            )),
            Err(err) => errors.push(format!("{program}:{err}")),
        }
    }
    Err(format!(
        "{context}_python_failed:{}",
        clean_cell(&errors.join(" | "))
    ))
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
        let context = read_text(&run.join("run_context.json")).expect("run context");
        let source_map = read_text(&run.join("artifact_source_map.tsv")).expect("source map");
        assert!(context.contains("\"state\":\"blocked\""));
        assert!(source_map.contains("target_candidate_ranking.tsv"));
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
        assert!(detail.contains("\"candidates\""));
        assert!(detail.contains("\"candidate_id\""));
        assert!(context.contains("\"state\":\"completed\""));
        assert!(source_map.contains("research_report.md"));
        assert!(evidence.contains("score_decomposition"));
        assert!(report.contains("# GenoLigand 科研报告"));
        assert!(report.contains("计算预测"));
        assert!(report.contains("建议把排序靠前的候选作为后续实验验证的优先对象"));
        assert!(report.contains("已知蛋白数据库") || report.contains("宏基因组潜在序列"));
        assert!(!report.contains("GenoLigand Scientific Report"));
        assert!(!report.contains("No candidate target"));
        assert!(!report.contains("known_database"));
        assert!(!report.contains("metagenome_potential"));
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
            "item\tstatus\tpath\nwarehouse\tready\tindexes/known_targets.tsv\nmetagenome_index\tready\tindexes/metagenome_targets.tsv\ntool_versions\tready\tindexes/tool_versions.tsv\n",
        )
        .expect("manifest");
        write_text(
            &store.join("indexes/tool_versions.tsv"),
            &temporary_tool_versions_text(),
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
