use flate2::read::GzDecoder;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};
use zip::ZipArchive;

mod input_conversion;

#[cfg(test)]
use input_conversion::extract_inputs;
use input_conversion::{
    INPUT_CONVERSION_HEADER, convert_inputs_from_manifest, for_each_converted_input,
    input_manifest_has_declared_input, write_input_conversion_row,
};

const WORKER_SCHEMA_VERSION: &str = "genoligand_worker_current_v2";
const WORKER_PIPELINE_MODE: &str = "genoligand_scientific_pipeline";
const MD_PROVENANCE_SCHEMA_VERSION: &str = "genoligand_md_artifact_provenance_v1";
const MAX_SEQUENCE_TOKEN_CHARS: usize = 96;
const MAX_STRUCTURE_METADATA_CHARS: usize = 16_384;
const LARGE_METAGENOME_FASTA_TSV_MAX_BYTES: u64 = 1_000_000_000;
const ALIGNMENT_DATABASE_MANIFEST_SCHEMA_VERSION: &str =
    "genoligand_alignment_database_manifest_v2";
const SOURCE_ROW_BATCH_SIZE: usize = 512;
const RANKING_POLICY_VERSION: &str = "genoligand_target_ranking_v3";
const SCIENCE_READINESS_SCHEMA_VERSION: &str = "genoligand_science_store_readiness_v1";
const CHEMBL_QUERY_MANIFEST_SCHEMA_VERSION: &str = "genoligand_chembl_query_database_v1";
const CHEMBL_QUERY_MANIFEST_REL_PATH: &str = "downloads/chembl/chembl_query.manifest.json";
const SOURCE_RETENTION_SCHEMA_VERSION: &str = "genoligand_source_retention_v1";
const BINDINGDB_QUERY_MANIFEST_SCHEMA_VERSION: &str = "genoligand_bindingdb_query_database_v1";
const BINDINGDB_QUERY_DATABASE_REL_PATH: &str = "indexes/bindingdb/bindingdb_query.sqlite";
const BINDINGDB_QUERY_MANIFEST_REL_PATH: &str = "indexes/bindingdb/bindingdb_query.manifest.json";
const LIGAND_SIMILARITY_DATABASE_REL_PATH: &str =
    "indexes/ligand_similarity/ligand_similarity.sqlite";
const LIGAND_SIMILARITY_MANIFEST_REL_PATH: &str =
    "indexes/ligand_similarity/ligand_similarity.manifest.json";
const LIGAND_SIMILARITY_SCHEMA_VERSION: &str = "genoligand_ligand_similarity_index_v2";
const STRUCTURE_TARGETS_MANIFEST_REL_PATH: &str = "indexes/structure_targets.manifest.json";
const STRUCTURE_TARGETS_MANIFEST_SCHEMA_VERSION: &str =
    "genoligand_structure_targets_publication_v2";
const STRUCTURE_PUBLICATION_TRANSACTION_REL_PATH: &str =
    "indexes/structure_publication.transaction.json";
const STRUCTURE_PUBLICATION_TRANSACTION_SCHEMA_VERSION: &str =
    "genoligand_structure_publication_transaction_v2";
const PLINDER_CANONICAL_MANIFEST_SCHEMA_VERSION: &str = "genoligand_plinder_canonical_v1";
const LIGAND_SIMILARITY_STANDARDIZATION: &str =
    "rdkit_cleanup_fragment_parent_reionize_canonical_tautomer_v1";
const BINDINGDB_SOURCE_REL_PATH: &str = "downloads/bindingdb/bindingdb_all.tsv.zip";
const CHEMBL_QUERY_HASH_CACHE_SCHEMA_VERSION: &str = "genoligand_chembl_query_sha256_cache_v1";
const CHEMBL_QUERY_HASH_CACHE_DIR: &str = ".chembl_query.sha256_cache";
const RCSB_INDEXED_PDB_IDS_REL_PATH: &str = "indexes/foldseek/rcsb_mmcif/indexed_pdb_ids.tsv";
const SCIENCE_READINESS_EVIDENCE_PATHS: &[&str] = &[
    "source_requirements.tsv",
    "manifests/source_file_manifest.tsv",
    "external_sync_execution_manifest.tsv",
    "install_status.json",
    "index_build_manifest.tsv",
    "index_readiness_manifest.tsv",
    "science_index_readiness.tsv",
    "indexes/known_targets.tsv",
    "indexes/metagenome_targets.tsv",
    "indexes/structure_targets.tsv",
    STRUCTURE_TARGETS_MANIFEST_REL_PATH,
    "indexes/tool_versions.tsv",
    "indexes/mmseqs/mmseqs_target_databases.tsv",
    "indexes/foldseek/foldseek_target_databases.tsv",
    RCSB_INDEXED_PDB_IDS_REL_PATH,
    LIGAND_SIMILARITY_MANIFEST_REL_PATH,
    "manifests/source_retention_manifest.tsv",
    "downloads/rcsb/ligand_bound_pdb_scope.json",
    "downloads/rcsb/ligand_bound_pdb_scope.tsv",
    "downloads/rcsb/on_demand_policy.json",
];
static PYTHON_SCRIPT_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static WORKER_INVOCATION_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static PIPELINE_INVOCATION_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static CHEMBL_QUERY_HASH_CACHE_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static CHEMBL_QUERY_PROCESS_HASH_CACHE: OnceLock<Mutex<BTreeMap<String, (String, String)>>> =
    OnceLock::new();
static LIGAND_SIMILARITY_PUBLICATION_HASH_CACHE: OnceLock<Mutex<BTreeMap<String, String>>> =
    OnceLock::new();
const LIGAND_SIMILARITY_PUBLICATION_HASH_CACHE_LIMIT: usize = 8;
#[cfg(test)]
static CHEMBL_QUERY_TEST_HASH_COUNTS: OnceLock<Mutex<BTreeMap<String, u64>>> = OnceLock::new();
const WORKER_INVOCATION_FILE: &str = "worker_invocation_id.txt";
const PIPELINE_INVOCATION_FILE: &str = "pipeline_invocation_id.txt";

#[derive(Clone, Debug)]
pub struct ScienceOptions {
    pub workspace_root: PathBuf,
    pub science_store: PathBuf,
    pub input_manifest: PathBuf,
    pub run_dir: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScienceStoreReadinessBlocker {
    pub code: String,
    pub component: String,
    pub evidence: String,
    pub repair_action: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScienceStoreReadinessAdvisory {
    pub code: String,
    pub component: String,
    pub evidence: String,
    pub action: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScienceStoreReadinessEvidence {
    pub path: String,
    pub sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScienceStoreReadiness {
    pub status: String,
    pub ready: bool,
    pub blockers: Vec<String>,
    pub blocker_details: Vec<ScienceStoreReadinessBlocker>,
    pub advisories: Vec<String>,
    pub advisory_details: Vec<ScienceStoreReadinessAdvisory>,
    pub repair_actions: Vec<String>,
    pub evidence_manifest: Vec<ScienceStoreReadinessEvidence>,
    pub evidence_digest: String,
    pub sources: Vec<Value>,
    pub indexes: Vec<Value>,
}

impl ScienceStoreReadiness {
    pub fn reasons(&self) -> &[String] {
        &self.blockers
    }
}

pub fn evaluate_science_store_readiness(
    science_store: &Path,
) -> Result<ScienceStoreReadiness, String> {
    let serialized = science_store.join("science_store_readiness.json");
    if !serialized.is_file() {
        return Ok(readiness_failure(
            "science_store_readiness_missing",
            "science_store_readiness.json is absent",
            "run_science_store_installer",
        ));
    }
    let value = match read_text(&serialized)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}')).ok())
    {
        Some(Value::Object(value)) => Value::Object(value),
        _ => {
            return Ok(readiness_failure(
                "science_store_readiness_invalid",
                "science_store_readiness.json is not valid JSON object",
                "rerun_science_store_installer",
            ));
        }
    };
    if value.get("schema_version").and_then(Value::as_str) != Some(SCIENCE_READINESS_SCHEMA_VERSION)
    {
        return Ok(readiness_failure(
            "science_store_readiness_invalid",
            "schema_version mismatch",
            "rerun_science_store_installer",
        ));
    }
    let Some(expected_evidence) = parse_readiness_evidence(value.get("evidence_manifest")) else {
        return Ok(readiness_failure(
            "science_store_readiness_invalid",
            "evidence_manifest is invalid",
            "rerun_science_store_installer",
        ));
    };
    let expected_digest = readiness_evidence_digest(&expected_evidence);
    if value.get("evidence_digest").and_then(Value::as_str) != Some(&expected_digest) {
        return Ok(readiness_failure(
            "science_store_readiness_invalid",
            "evidence_digest does not bind evidence_manifest",
            "rerun_science_store_installer",
        ));
    }
    let Ok((current_evidence, current_digest)) = science_readiness_evidence(science_store) else {
        return Ok(readiness_failure(
            "science_store_readiness_stale",
            "current readiness evidence cannot be read",
            "rerun_science_store_installer",
        ));
    };
    if expected_evidence != current_evidence || expected_digest != current_digest {
        return Ok(readiness_failure(
            "science_store_readiness_stale",
            "authoritative readiness evidence changed after generation",
            "rerun_science_store_installer",
        ));
    }
    let Some(blocker_values) = value.get("blockers").and_then(Value::as_array) else {
        return Ok(readiness_failure(
            "science_store_readiness_invalid",
            "blockers is not an array",
            "rerun_science_store_installer",
        ));
    };
    let mut blocker_details = Vec::with_capacity(blocker_values.len());
    for blocker in blocker_values {
        let Some(code) = blocker.get("code").and_then(Value::as_str) else {
            return Ok(readiness_failure(
                "science_store_readiness_invalid",
                "blocker code is missing",
                "rerun_science_store_installer",
            ));
        };
        let Some(component) = blocker.get("component").and_then(Value::as_str) else {
            return Ok(readiness_failure(
                "science_store_readiness_invalid",
                "blocker component is missing",
                "rerun_science_store_installer",
            ));
        };
        let Some(evidence) = blocker.get("evidence").and_then(Value::as_str) else {
            return Ok(readiness_failure(
                "science_store_readiness_invalid",
                "blocker evidence is missing",
                "rerun_science_store_installer",
            ));
        };
        let Some(repair_action) = blocker.get("repair_action").and_then(Value::as_str) else {
            return Ok(readiness_failure(
                "science_store_readiness_invalid",
                "blocker repair_action is missing",
                "rerun_science_store_installer",
            ));
        };
        if code.trim().is_empty() {
            return Ok(readiness_failure(
                "science_store_readiness_invalid",
                "blocker code is empty",
                "rerun_science_store_installer",
            ));
        }
        blocker_details.push(ScienceStoreReadinessBlocker {
            code: code.to_string(),
            component: component.to_string(),
            evidence: evidence.to_string(),
            repair_action: repair_action.to_string(),
        });
    }
    let Some(repair_actions) = value
        .get("repair_actions")
        .and_then(Value::as_array)
        .and_then(|values| {
            values
                .iter()
                .map(|value| value.as_str().map(ToOwned::to_owned))
                .collect::<Option<Vec<_>>>()
        })
    else {
        return Ok(readiness_failure(
            "science_store_readiness_invalid",
            "repair_actions is not a string array",
            "rerun_science_store_installer",
        ));
    };
    let Some(mut advisory_details) = parse_readiness_advisories(value.get("advisories")) else {
        return Ok(readiness_failure(
            "science_store_readiness_invalid",
            "advisories is not a valid advisory array",
            "rerun_science_store_installer",
        ));
    };
    if let Some(advisory) = foldseek_refresh_advisory(science_store)
        && !advisory_details
            .iter()
            .any(|item| item.code == advisory.code)
    {
        advisory_details.push(advisory);
    }
    let advisories = advisory_details
        .iter()
        .map(|advisory| advisory.code.clone())
        .collect();
    let ready = value.get("ready").and_then(Value::as_bool);
    let status = value.get("status").and_then(Value::as_str);
    if !matches!(
        (ready, status),
        (Some(true), Some("ready")) | (Some(false), Some("needs_repair"))
    ) || ready != Some(blocker_details.is_empty())
        || blocker_details.iter().any(|blocker| {
            !blocker.repair_action.is_empty() && !repair_actions.contains(&blocker.repair_action)
        })
    {
        return Ok(readiness_failure(
            "science_store_readiness_invalid",
            "status, blockers, and repair_actions are inconsistent",
            "rerun_science_store_installer",
        ));
    }
    let blockers = blocker_details
        .iter()
        .map(|blocker| blocker.code.clone())
        .collect();
    let Some(sources) = value.get("sources").and_then(Value::as_array).cloned() else {
        return Ok(readiness_failure(
            "science_store_readiness_invalid",
            "sources is not an array",
            "rerun_science_store_installer",
        ));
    };
    let Some(indexes) = value.get("indexes").and_then(Value::as_array).cloned() else {
        return Ok(readiness_failure(
            "science_store_readiness_invalid",
            "indexes is not an array",
            "rerun_science_store_installer",
        ));
    };
    Ok(ScienceStoreReadiness {
        status: status.expect("validated status").to_string(),
        ready: ready.expect("validated ready flag"),
        blockers,
        blocker_details,
        advisories,
        advisory_details,
        repair_actions,
        evidence_manifest: current_evidence,
        evidence_digest: current_digest,
        sources,
        indexes,
    })
}

fn readiness_failure(code: &str, evidence: &str, repair_action: &str) -> ScienceStoreReadiness {
    ScienceStoreReadiness {
        status: "needs_repair".to_string(),
        ready: false,
        blockers: vec![code.to_string()],
        blocker_details: vec![ScienceStoreReadinessBlocker {
            code: code.to_string(),
            component: "science_store_readiness".to_string(),
            evidence: evidence.to_string(),
            repair_action: repair_action.to_string(),
        }],
        advisories: Vec::new(),
        advisory_details: Vec::new(),
        repair_actions: vec![repair_action.to_string()],
        evidence_manifest: Vec::new(),
        evidence_digest: String::new(),
        sources: Vec::new(),
        indexes: Vec::new(),
    }
}

fn parse_readiness_advisories(value: Option<&Value>) -> Option<Vec<ScienceStoreReadinessAdvisory>> {
    let Some(value) = value else {
        return Some(Vec::new());
    };
    let rows = value.as_array()?;
    let mut seen = BTreeSet::new();
    rows.iter()
        .map(|row| {
            let code = row.get("code")?.as_str()?.trim();
            let component = row.get("component")?.as_str()?.trim();
            let evidence = row.get("evidence")?.as_str()?.trim();
            let action = row.get("action")?.as_str()?.trim();
            if code.is_empty()
                || component.is_empty()
                || evidence.is_empty()
                || action.is_empty()
                || !seen.insert(code.to_string())
            {
                return None;
            }
            Some(ScienceStoreReadinessAdvisory {
                code: code.to_string(),
                component: component.to_string(),
                evidence: evidence.to_string(),
                action: action.to_string(),
            })
        })
        .collect()
}

fn foldseek_refresh_advisory(science_store: &Path) -> Option<ScienceStoreReadinessAdvisory> {
    let manifest = science_store.join("indexes/foldseek/foldseek_target_databases.tsv");
    let row = read_simple_tsv_rows(&manifest)
        .ok()?
        .into_iter()
        .find(|row| {
            row.get("source_id").map(String::as_str) == Some("rcsb_mmcif")
                && row.get("scope_mode").map(String::as_str) == Some("on_demand")
                && row.get("status").map(String::as_str) == Some("ready")
        })?;
    let indexed_ids = read_simple_tsv_rows(&science_store.join(RCSB_INDEXED_PDB_IDS_REL_PATH))
        .ok()?
        .into_iter()
        .filter_map(|row| {
            row.get("pdb_id")
                .map(|value| value.trim().to_ascii_uppercase())
        })
        .filter(|pdb_id| is_pdb_id(pdb_id))
        .collect::<BTreeSet<_>>();
    let cached_ids = current_valid_rcsb_cache_ids(science_store)?;
    let unindexed_ids = cached_ids
        .difference(&indexed_ids)
        .cloned()
        .collect::<Vec<_>>();
    if unindexed_ids.is_empty() {
        return None;
    }
    let coverage = row
        .get("scope_coverage")
        .filter(|value| !value.trim().is_empty())
        .cloned()
        .unwrap_or_else(|| {
            format!(
                "{}/{}",
                row.get("indexed_pdb_count")
                    .map(String::as_str)
                    .unwrap_or("unknown"),
                row.get("scope_count")
                    .map(String::as_str)
                    .unwrap_or("unknown")
            )
        });
    Some(ScienceStoreReadinessAdvisory {
        code: "rcsb_foldseek_refresh_needed".to_string(),
        component: "rcsb".to_string(),
        evidence: format!(
            "scope_coverage={coverage};cached_unindexed_count={};cached_unindexed_sample={}",
            unindexed_ids.len(),
            unindexed_ids
                .iter()
                .take(5)
                .cloned()
                .collect::<Vec<_>>()
                .join(",")
        ),
        action: "refresh_foldseek_when_convenient".to_string(),
    })
}

fn current_valid_rcsb_cache_ids(science_store: &Path) -> Option<BTreeSet<String>> {
    let cache_root = science_store.join(RCSB_ON_DEMAND_CACHE_ROOT);
    if !cache_root.is_dir() {
        return Some(BTreeSet::new());
    }
    let canonical_store = fs::canonicalize(science_store).ok()?;
    let canonical_root = fs::canonicalize(&cache_root).ok()?;
    if !canonical_root.starts_with(&canonical_store) {
        return None;
    }
    let mut entries = Vec::new();
    collect_rcsb_cache_entries(&cache_root, &mut entries).ok()?;
    let mut ids = BTreeSet::new();
    for entry in entries {
        let Ok(candidate) = fs::canonicalize(&entry.path) else {
            continue;
        };
        if !candidate.starts_with(&canonical_root) {
            continue;
        }
        let Some(pdb_id) = lower_file_name(&candidate)
            .strip_suffix(".cif.gz")
            .map(str::to_ascii_uppercase)
            .filter(|value| is_pdb_id(value))
        else {
            continue;
        };
        let Ok(expected) = fs::canonicalize(rcsb_coordinate_cache_path(science_store, &pdb_id))
        else {
            continue;
        };
        if candidate == expected && coordinate_file_identity_matches_pdb(&candidate, &pdb_id) {
            ids.insert(pdb_id);
        }
    }
    Some(ids)
}

fn parse_readiness_evidence(value: Option<&Value>) -> Option<Vec<ScienceStoreReadinessEvidence>> {
    let rows = value?.as_array()?;
    let mut seen = BTreeSet::new();
    rows.iter()
        .map(|row| {
            let path = row.get("path")?.as_str()?;
            let sha256 = row.get("sha256")?.as_str()?;
            if !SCIENCE_READINESS_EVIDENCE_PATHS.contains(&path)
                || !seen.insert(path.to_string())
                || !valid_sha256_hex(sha256)
            {
                return None;
            }
            Some(ScienceStoreReadinessEvidence {
                path: path.to_string(),
                sha256: sha256.to_ascii_lowercase(),
            })
        })
        .collect()
}

fn science_readiness_evidence(
    science_store: &Path,
) -> Result<(Vec<ScienceStoreReadinessEvidence>, String), String> {
    let mut evidence = Vec::new();
    for rel_path in SCIENCE_READINESS_EVIDENCE_PATHS {
        let path = science_store.join(rel_path);
        if !path.is_file() {
            continue;
        }
        let sha256 = sha256_file_hex(&path)
            .ok_or_else(|| format!("science_readiness_evidence_read_failed:{rel_path}"))?;
        evidence.push(ScienceStoreReadinessEvidence {
            path: (*rel_path).to_string(),
            sha256,
        });
    }
    let digest = readiness_evidence_digest(&evidence);
    Ok((evidence, digest))
}

fn readiness_evidence_digest(evidence: &[ScienceStoreReadinessEvidence]) -> String {
    let mut hasher = Sha256::new();
    for row in evidence {
        hasher.update(row.path.as_bytes());
        hasher.update(b"\t");
        hasher.update(row.sha256.as_bytes());
        hasher.update(b"\n");
    }
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
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
    CandidateCoarseRank,
    DynamicsScreen,
    TargetRank,
    EvidenceBuild,
    ReportRender,
    PipelineFinalize,
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
            ("candidate", "coarse-rank") => Ok(Self::CandidateCoarseRank),
            ("dynamics", "screen") => Ok(Self::DynamicsScreen),
            ("target", "rank") => Ok(Self::TargetRank),
            ("evidence", "build") => Ok(Self::EvidenceBuild),
            ("report", "render") => Ok(Self::ReportRender),
            ("pipeline", "finalize") => Ok(Self::PipelineFinalize),
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
    tokens: BTreeSet<String>,
    source_path: String,
    structure_atom_count: usize,
    structure_chain_count: usize,
    structure_residue_count: usize,
}

const NORMALIZED_INPUT_HEADER: &str = "input_id\tinput_kind\tprimary_value\tfile_count\tfeature_tokens\trecord_id\tsource_kind\tsource_path\tcontent_sha256\tstatus\tstructure_atom_count\tstructure_chain_count\tstructure_residue_count";
const FEATURE_MANIFEST_HEADER: &str = "input_id\tinput_kind\tfeature_family\tfeature_name\tfeature_value\tfeature_source\tfeature_token\trecord_id";
const QUERY_STRUCTURE_MANIFEST_HEADER: &str = "input_id\trecord_id\tinput_kind\tinput_digest\tmethod\tfeatures_json\tcoordinate_artifact\tfeature_digest\tstatus";
const QUERY_STRUCTURE_ARTIFACT_SCHEMA_VERSION: &str = "genoligand_query_structure_artifact_v1";
const QUERY_STRUCTURE_FEATURE_SCHEMA_VERSION: &str = "genoligand_query_structure_features_v1";
const MAX_QUERY_STRUCTURE_FEATURE_BYTES: u64 = 256 * 1024;
const MAX_QUERY_STRUCTURE_MANIFEST_BYTES: u64 = 64 * 1024;
const MAX_QUERY_STRUCTURE_FEATURE_ROWS: usize = 96;
const TARGET_CANDIDATE_RANKING_HEADER: &str = "rank\tcandidate_id\ttarget_id\ttarget_name\tcandidate_scope\torganism\tfinal_score\tknown_database_score\tmetagenome_score\tstructure_score\tdocking_score\tmd_score\tevidence_summary\tstatus\tinput_id\trecord_id\tidentity_namespace\tinput_kind\tranking_policy\tknown_database_weight\tmetagenome_weight\tstructure_weight\tdocking_weight\tmd_weight";
const INPUT_SEQUENCE_SPOOL_FILE: &str = "input_sequence_spool.bin";
const INPUT_SEQUENCE_SPOOL_PENDING_FILE: &str = "input_sequence_spool.bin.pending";

#[derive(Clone, Copy, Debug)]
struct WorkerTask<'a> {
    job_id: &'a str,
    worker: &'static str,
    module: &'static str,
    stage: &'static str,
}

const SCIENTIFIC_WORKER_TASKS: &[WorkerTask<'static>] = &[WorkerTask {
    job_id: "md_summary_ligand_dynamics",
    worker: "md_summary",
    module: "genoligand_workers.md_summary_worker",
    stage: "dynamics_summary_primitive",
}];

const SEQUENCE_ALIGNMENT_WORKER_TASK: WorkerTask<'static> = WorkerTask {
    job_id: "mmseqs_onsite_sequence_alignment",
    worker: "mmseqs_alignment",
    module: "genoligand_workers.alignment_worker",
    stage: "onsite_sequence_alignment",
};

const STRUCTURE_ALIGNMENT_WORKER_TASK: WorkerTask<'static> = WorkerTask {
    job_id: "foldseek_onsite_structure_alignment",
    worker: "foldseek_alignment",
    module: "genoligand_workers.alignment_worker",
    stage: "onsite_structure_alignment",
};

const DOCKING_WORKER_TASK: WorkerTask<'static> = WorkerTask {
    job_id: "autodock_vina_complex_docking",
    worker: "docking",
    module: "genoligand_workers.docking_worker",
    stage: "docking_primitive",
};

const REPORT_CANDIDATE_LIMIT: usize = 50;
const COMPLEX_CANDIDATE_TOP_N: usize = 3;
const STRUCTURE_MATCH_TOP_K_PER_INPUT_RECORD: usize = 256;
const MIN_STRUCTURE_MATCH_TOKEN_CHARS: usize = 3;
const RCSB_ON_DEMAND_POLICY_SCHEMA_VERSION: &str = "genoligand_rcsb_on_demand_policy_v1";
const RCSB_OFFICIAL_DOWNLOAD_BASE_URL: &str = "https://files.rcsb.org/download";
const RCSB_OFFICIAL_COORDINATE_TEMPLATE: &str = "https://files.rcsb.org/download/{pdb_id}.cif.gz";
const RCSB_ON_DEMAND_CACHE_ROOT: &str = "downloads/rcsb/mmcif";
const RCSB_ON_DEMAND_VALIDATION: &str = "gzip_mmcif_entry_and_atom_site";
const RCSB_ON_DEMAND_FETCH_LIMIT: usize = 12;
const RCSB_ON_DEMAND_CACHE_MAX_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const RCSB_ON_DEMAND_CACHE_MAX_FILES: usize = 4096;
const RCSB_ON_DEMAND_FETCH_MANIFEST: &str = "rcsb_on_demand_fetch.tsv";

#[derive(Clone, Debug)]
struct ComputedFeatureInput {
    input_id: String,
    record_id: String,
    input_kind: String,
    tokens: BTreeSet<String>,
}

#[derive(Clone, Debug)]
struct QueryStructureArtifact {
    method: String,
    coordinate_artifact: String,
    feature_digest: String,
    status: String,
    feature_rows: Vec<FeatureRow>,
}

#[derive(Clone, Debug)]
struct QueryStructureIndexRow {
    input_id: String,
    record_id: String,
    input_kind: String,
    input_digest: String,
    method: String,
    features_json: String,
    coordinate_artifact: String,
    feature_digest: String,
    status: String,
}

#[derive(Clone, Debug)]
struct ConversionProvenance {
    canonical_kind: String,
    source_kind: String,
    source_path: String,
    content_sha256: String,
    artifact: String,
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
struct CanonicalSourceBinding {
    source_id: String,
    source_path: String,
    source_digest: String,
}

#[derive(Clone, Debug)]
struct Candidate {
    candidate_id: String,
    input_id: String,
    record_id: String,
    input_kind: String,
    ranking_policy: String,
    identity_namespace: String,
    target_id: String,
    target_name: String,
    target_class: String,
    candidate_scope: String,
    organism: String,
    sources: BTreeSet<String>,
    pdb_ids: BTreeSet<String>,
    metadata_quality: u16,
    known_score: f64,
    metagenome_score: f64,
    structure_score: f64,
    docking_score: f64,
    dynamics_score: f64,
    final_score: f64,
    evidence: String,
}

#[derive(Clone, Copy, Debug)]
struct RankingRoute {
    key: &'static str,
    known: f64,
    metagenome: f64,
    structure: f64,
    docking: f64,
    md: f64,
}

impl RankingRoute {
    fn score(self, candidate: &Candidate) -> f64 {
        self.score_components(
            candidate.known_score,
            candidate.metagenome_score,
            candidate.structure_score,
            candidate.docking_score,
            candidate.dynamics_score,
        )
    }

    fn score_components(
        self,
        known: f64,
        metagenome: f64,
        structure: f64,
        docking: f64,
        md: f64,
    ) -> f64 {
        debug_assert!((self.weight_sum() - 1.0).abs() < 1e-9);
        (known * self.known
            + metagenome * self.metagenome
            + structure * self.structure
            + docking * self.docking
            + md * self.md)
            .clamp(0.0, 1.0)
    }

    fn weight_sum(self) -> f64 {
        self.known + self.metagenome + self.structure + self.docking + self.md
    }

    fn preliminary_weights(self) -> (f64, f64, f64) {
        let sum = self.known + self.metagenome + self.structure;
        debug_assert!(sum > 0.0);
        (
            self.known / sum,
            self.metagenome / sum,
            self.structure / sum,
        )
    }

    fn preliminary_score(self, candidate: &Candidate) -> f64 {
        let (known, metagenome, structure) = self.preliminary_weights();
        (candidate.known_score * known
            + candidate.metagenome_score * metagenome
            + candidate.structure_score * structure)
            .clamp(0.0, 1.0)
    }
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
    record_id: String,
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
    identity_namespace: String,
    input_id: String,
    record_id: String,
    target_id: String,
    target_name: String,
    organism: String,
    target_class: String,
    source: String,
    pdb_id: String,
    evidence: String,
}

#[derive(Clone, Debug)]
struct CoordinateCandidate {
    selected: SelectedComplexCandidate,
    coordinate_path: Option<PathBuf>,
    priority: i64,
    score: f64,
}

#[derive(Clone, Debug)]
struct RcsbFetchResult {
    status: String,
    path: PathBuf,
    sha256: String,
    reason: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RcsbOnDemandPolicy {
    max_cache_bytes: u64,
    max_cache_files: usize,
}

#[derive(Debug)]
struct RcsbCacheEntry {
    path: PathBuf,
    bytes: u64,
    modified: SystemTime,
}

#[derive(Clone, Debug)]
struct RcsbFetchAuditRow {
    pdb_id: String,
    status: String,
    url: String,
    path: String,
    bytes: u64,
    sha256: String,
    reason: String,
}

#[derive(Clone, Debug)]
struct CandidateWorkerOutcome {
    candidate_id: String,
    input_id: String,
    record_id: String,
    job_id: String,
    invocation_id: String,
    status: String,
    reason: String,
    manifest: String,
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
        local_rel_path: "downloads/plinder/index",
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
        ScienceCommand::InputNormalize => {
            start_pipeline_invocation(options)?;
            start_worker_invocation(options)?;
            normalize_input(options)
        }
        ScienceCommand::FeaturesCompute => {
            start_pipeline_invocation(options)?;
            start_worker_invocation(options)?;
            prepare_query_structures(options)?;
            compute_features(options)
        }
        ScienceCommand::DatabaseMatchKnown => match_known(options),
        ScienceCommand::MetagenomeMatchPotential => match_metagenome(options),
        ScienceCommand::StructureEvaluate => evaluate_structure(options),
        ScienceCommand::CandidateCoarseRank => coarse_rank_candidates(options),
        ScienceCommand::DynamicsScreen => execute_dynamics_screen(options),
        ScienceCommand::TargetRank => rank_targets(options),
        ScienceCommand::EvidenceBuild => build_evidence(options),
        ScienceCommand::ReportRender => render_report(options),
        ScienceCommand::PipelineFinalize => finalize_pipeline(options),
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
        "source\ttarget_id\ttarget_name\torganism\ttarget_class\tligand_tokens\tsequence_tokens\tstructure_tokens\tevidence\nMGnify\tMGYP000000001\tTest metagenome protein\tmetagenome\tmetagenome_potential\t\tmetagenome test sequence\t\tunit_test_record\n",
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
    write_text(
        &store.join("index_build_manifest.tsv"),
        "index\tinput_path\tstatus\trows\toutput_path\tnotes\nknown_targets\tunit_test/known_targets.tsv\tused\t1\tindexes/known_targets.tsv\tfull_test_index\nmetagenome_targets\tunit_test/metagenome_targets.tsv\tused\t1\tindexes/metagenome_targets.tsv\tfull_test_index\nstructure_targets\tunit_test/structure_targets.tsv\tused\t1\tindexes/structure_targets.tsv\tfull_test_index\n",
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
    ensure_dir(&store.join("manifests"))?;
    let mut source_manifest = String::from(
        "source_id\tsource_label\tsource_class\tlocal_path\tstatus\tstorage_state\tfile_count\tbytes\tsha256\tprovenance\trelease_identity\ttrust_source\n",
    );
    for source in REQUIRED_SOURCES {
        source_manifest.push_str(&format!(
            "{}\t{}\tunit_test\t{}\tready\tunit_test\t1\t1\t{}\tunit_test\tunit_test_release\tunit_test\n",
            source.id,
            source.label,
            source.local_rel_path,
            "a".repeat(64)
        ));
    }
    write_text(
        &store.join("manifests/source_file_manifest.tsv"),
        &source_manifest,
    )?;
    for (tool, db_rel) in [
        ("mmseqs", "indexes/mmseqs/unit_test/target"),
        ("foldseek", "indexes/foldseek/unit_test/target"),
    ] {
        let prefix = store.join(db_rel);
        ensure_dir(prefix.parent().expect("unit test database parent"))?;
        write_text(&prefix, "unit-test-database\n")?;
        write_text(
            &PathBuf::from(format!("{}.dbtype", prefix.display())),
            "1234",
        )?;
        write_text(
            &PathBuf::from(format!("{}.index", prefix.display())),
            "index\n",
        )?;
        write_text(
            &PathBuf::from(format!("{}.lookup", prefix.display())),
            "lookup\n",
        )?;
        let manifest = store.join(format!("indexes/{tool}/{tool}_target_databases.tsv"));
        write_text(
            &manifest,
            &format!(
                "source_id\tmanifest_schema_version\tsource_namespace\tsource_class\tsource_digest\tsource_files\tsource_bytes\tbuilder_version\tdb_path\tstatus\nunit_test\tgenoligand_alignment_database_manifest_v2\tunit_test\tunit_test\t{}\t1\t1\tunit_test_builder\t{db_rel}\tready\n",
                "a".repeat(64)
            ),
        )?;
    }
    write_test_science_store_readiness(&store, &[])?;
    Ok(store)
}

#[cfg(test)]
fn write_test_science_store_readiness(
    store: &Path,
    blockers: &[(&str, &str, &str, &str)],
) -> Result<(), String> {
    let (evidence_manifest, evidence_digest) = science_readiness_evidence(store)?;
    let blocker_values = blockers
        .iter()
        .map(|(code, component, evidence, repair_action)| {
            json!({
                "code": code,
                "component": component,
                "evidence": evidence,
                "repair_action": repair_action,
            })
        })
        .collect::<Vec<_>>();
    let repair_actions = blockers
        .iter()
        .map(|(_, _, _, repair_action)| *repair_action)
        .filter(|action| !action.is_empty())
        .collect::<BTreeSet<_>>();
    let evidence_values = evidence_manifest
        .iter()
        .map(|row| json!({"path": row.path, "sha256": row.sha256}))
        .collect::<Vec<_>>();
    write_text(
        &store.join("science_store_readiness.json"),
        &format!(
            "{}\n",
            json!({
                "schema_version": SCIENCE_READINESS_SCHEMA_VERSION,
                "status": if blockers.is_empty() {"ready"} else {"needs_repair"},
                "ready": blockers.is_empty(),
                "generated_at": 1,
                "evidence_manifest": evidence_values,
                "evidence_digest": evidence_digest,
                "sources": [],
                "indexes": [],
                "blockers": blocker_values,
                "advisories": [],
                "repair_actions": repair_actions,
            })
        ),
    )
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
    reconcile_structure_publication_transaction(&options.science_store)?;
    if let Some(status) = reusable_published_science_store(options) {
        cleanup_superseded_ligand_query_artifacts(&options.science_store)?;
        return Ok(format!(
            "science_store_prepared\tstatus={status}\treused_published_indexes=true\tstore={}",
            options.science_store.display()
        ));
    }
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
    let ligand_similarity_manifest = options
        .science_store
        .join(LIGAND_SIMILARITY_MANIFEST_REL_PATH);
    write_tool_versions(&tools)?;
    let ligand_similarity_error = prepare_ligand_similarity_authority(options).err();
    let ligand_similarity_ready =
        ligand_similarity_error.is_none() && ligand_similarity_authority_ready(options).is_ok();
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
        (
            "ligand_similarity_index",
            ligand_similarity_ready,
            &ligand_similarity_manifest,
            "build_compact_ligand_similarity_index",
        ),
    ];
    let required_items = rows.len();
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
                "required_items": required_items,
                "missing_components": missing,
                "ligand_similarity_detail": ligand_similarity_error
                    .as_deref()
                    .unwrap_or("ready"),
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
        "science_store_prepared\tready_items={ready}\trequired_items={required_items}\tstatus={}\tmissing_components={}\tstore={}",
        if missing.is_empty() {
            "ready"
        } else {
            "needs_repair"
        },
        missing.len(),
        options.science_store.display()
    ))
}

fn reusable_published_science_store(options: &StoreOptions) -> Option<&'static str> {
    let readiness = evaluate_science_store_readiness(&options.science_store).ok()?;
    let structure_publication_ready =
        validate_structure_targets_publication(&options.science_store).is_ok();
    if readiness.blockers.iter().any(|blocker| {
        matches!(
            blocker.as_str(),
            "science_store_readiness_missing"
                | "science_store_readiness_invalid"
                | "science_store_readiness_stale"
        )
    }) || validate_retained_ligand_similarity_publication(&options.science_store).is_err()
        || !published_indexes_have_pruned_retention(&options.science_store)
        || !structure_publication_ready
        || ![
            "indexes/known_targets.tsv",
            "indexes/metagenome_targets.tsv",
            "indexes/structure_targets.tsv",
            "index_build_manifest.tsv",
        ]
        .iter()
        .all(|relative| options.science_store.join(relative).is_file())
    {
        return None;
    }
    if !readiness.ready && structure_rebuild_authority_available(&options.science_store) {
        return None;
    }
    Some(if readiness.ready {
        "ready"
    } else {
        "needs_repair"
    })
}

fn structure_rebuild_authority_available(store: &Path) -> bool {
    let mut available = BTreeSet::new();
    if validate_plinder_canonical_publication(store).is_ok()
        && structure_source_identity(store, "PLINDER").is_ok()
    {
        available.insert("PLINDER".to_string());
    }
    if discover_biolip_tables(&store.join("downloads/biolip2")).is_ok_and(|paths| {
        structure_source_identity(store, "BioLiP2").is_ok()
            && source_files_have_minimum_bytes(&paths, 10 * 1024 * 1024)
    }) {
        available.insert("BioLiP2".to_string());
    }
    if discover_moad_tables(&store.join("downloads/binding_moad")).is_ok_and(|paths| {
        structure_source_identity(store, "Binding MOAD").is_ok()
            && source_files_have_minimum_bytes(&paths, 100 * 1024)
    }) {
        available.insert("Binding MOAD".to_string());
    }
    if current_valid_rcsb_cache_ids(store).is_some_and(|ids| !ids.is_empty())
        && structure_source_identity(store, "RCSB").is_ok()
    {
        available.insert("RCSB".to_string());
    }
    let required = required_structure_contributions(store);
    !available.is_empty() && required.is_subset(&available)
}

fn source_files_have_minimum_bytes(paths: &[PathBuf], minimum: u64) -> bool {
    !paths.is_empty()
        && paths
            .iter()
            .try_fold(0_u64, |total, path| {
                fs::metadata(path)
                    .ok()
                    .filter(|meta| meta.is_file() && meta.len() > 0)
                    .and_then(|meta| total.checked_add(meta.len()))
            })
            .is_some_and(|bytes| bytes >= minimum)
}

fn published_indexes_have_pruned_retention(store: &Path) -> bool {
    let Ok(rows) = read_simple_tsv_rows(&store.join("manifests/source_retention_manifest.tsv"))
    else {
        return false;
    };
    let retained = |source_id: &str, required: &str| {
        rows.iter().any(|row| {
            row.get("schema_version").map(String::as_str) == Some(SOURCE_RETENTION_SCHEMA_VERSION)
                && row.get("state").map(String::as_str) == Some("pruned")
                && row.get("source_id").map(String::as_str) == Some(source_id)
                && row.get("derived_paths").is_some_and(|paths| {
                    paths
                        .split(';')
                        .map(|path| path.trim().replace('\\', "/"))
                        .any(|path| path == required)
                })
        })
    };
    published_index_is_header_only(&store.join("indexes/known_targets.tsv"))
        && published_index_is_header_only(&store.join("indexes/metagenome_targets.tsv"))
        && retained(
            "uniprot_human_reviewed",
            "indexes/mmseqs/mmseqs_target_databases.tsv",
        )
        && retained("biolip2", "indexes/mmseqs/mmseqs_target_databases.tsv")
        && retained(
            "metagenome_potential",
            "indexes/mmseqs/mmseqs_target_databases.tsv",
        )
        && ready_alignment_namespaces(store, "known", &["UniProt", "BioLiP2"])
        && ready_alignment_namespaces(store, "metagenome", &["MGnify"])
}

fn published_index_is_header_only(path: &Path) -> bool {
    let expected = store_row_header();
    fs::metadata(path).is_ok_and(|meta| meta.is_file() && meta.len() == expected.len() as u64)
        && fs::read(path).is_ok_and(|bytes| bytes == expected.as_bytes())
}

fn ready_alignment_namespaces(store: &Path, source_class: &str, expected: &[&str]) -> bool {
    let Ok(rows) = read_simple_tsv_rows(&store.join("indexes/mmseqs/mmseqs_target_databases.tsv"))
    else {
        return false;
    };
    expected.iter().all(|expected_namespace| {
        retained_alignment_namespace_ready(store, &rows, source_class, expected_namespace)
    })
}

fn retained_alignment_namespace_ready(
    store: &Path,
    rows: &[BTreeMap<String, String>],
    source_class: &str,
    namespace: &str,
) -> bool {
    let (source_id, source_root, expected_sources): (&str, &str, &[&str]) = match namespace {
        "UniProt" => (
            "uniprot_human_reviewed",
            "downloads/uniprot/uniprot_human_reviewed.fasta.gz",
            &["downloads/uniprot/uniprot_human_reviewed.fasta.gz"],
        ),
        "BioLiP2" => (
            "biolip2",
            "downloads/biolip2",
            &[
                "downloads/biolip2/peptide.fasta.gz",
                "downloads/biolip2/peptide_nr.fasta.gz",
                "downloads/biolip2/protein.fasta.gz",
                "downloads/biolip2/protein_nr.fasta.gz",
            ],
        ),
        "MGnify" => (
            "metagenome_potential",
            "downloads/metagenome/mgnify",
            &["downloads/metagenome/mgnify/mgy_clusters.fa.gz"],
        ),
        _ => return false,
    };
    let Ok(source_rows) = read_simple_tsv_rows(&store.join("manifests/source_file_manifest.tsv"))
    else {
        return false;
    };
    let Ok(retention_rows) =
        read_simple_tsv_rows(&store.join("manifests/source_retention_manifest.tsv"))
    else {
        return false;
    };
    let mut source_matches = source_rows
        .iter()
        .filter(|row| row.get("source_id").map(String::as_str) == Some(source_id));
    let Some(source) = source_matches.next() else {
        return false;
    };
    let mut retention_matches = retention_rows
        .iter()
        .filter(|row| row.get("source_id").map(String::as_str) == Some(source_id));
    let Some(retention) = retention_matches.next() else {
        return false;
    };
    let source_digest = source.get("sha256").map(String::as_str).unwrap_or("");
    let source_files = source
        .get("file_count")
        .and_then(|value| value.parse::<u64>().ok());
    let source_bytes = source
        .get("bytes")
        .and_then(|value| value.parse::<u64>().ok());
    if source_matches.next().is_some()
        || retention_matches.next().is_some()
        || source.get("status").map(String::as_str) != Some("ready")
        || source.get("storage_state").map(String::as_str) != Some("indexed_source_pruned")
        || source
            .get("local_path")
            .map(|path| path.replace('\\', "/"))
            .as_deref()
            != Some(source_root)
        || !valid_sha256_hex(source_digest)
        || source_files.is_none_or(|value| value == 0)
        || source_bytes.is_none_or(|value| value == 0)
        || source
            .get("release_identity")
            .is_none_or(|value| value.trim().is_empty())
        || source
            .get("trust_source")
            .is_none_or(|value| value.trim().is_empty())
        || retention.get("schema_version").map(String::as_str)
            != Some(SOURCE_RETENTION_SCHEMA_VERSION)
        || retention.get("state").map(String::as_str) != Some("pruned")
        || retention
            .get("source_local_path")
            .map(|path| path.replace('\\', "/"))
            .as_deref()
            != Some(source_root)
        || retention.get("source_manifest_sha256").map(String::as_str) != Some(source_digest)
        || retention.get("release_identity") != source.get("release_identity")
        || retention.get("trust_source") != source.get("trust_source")
        || retention
            .get("source_files")
            .and_then(|value| value.parse::<u64>().ok())
            != source_files
        || retention
            .get("source_bytes")
            .and_then(|value| value.parse::<u64>().ok())
            != source_bytes
        || !retention.get("derived_paths").is_some_and(|paths| {
            paths.split(';').any(|path| {
                path.trim().replace('\\', "/") == "indexes/mmseqs/mmseqs_target_databases.tsv"
            })
        })
    {
        return false;
    }

    let namespace_rows = rows
        .iter()
        .filter(|row| row.get("source_namespace").map(String::as_str) == Some(namespace))
        .collect::<Vec<_>>();
    if namespace_rows.len() != expected_sources.len() {
        return false;
    }
    let mut ready_sources = BTreeMap::new();
    let mut database_paths = BTreeSet::new();
    for row in namespace_rows {
        let source_rel = row
            .get("source_path")
            .map(|path| path.trim().replace('\\', "/"))
            .unwrap_or_default();
        let digest = row.get("source_digest").map(String::as_str).unwrap_or("");
        let bytes = row
            .get("source_bytes")
            .and_then(|value| value.parse::<u64>().ok());
        let database_rel = row
            .get("db_path")
            .map(|path| path.trim().replace('\\', "/"))
            .unwrap_or_default();
        let relative_database = Path::new(&database_rel);
        if row.get("manifest_schema_version").map(String::as_str)
            != Some(ALIGNMENT_DATABASE_MANIFEST_SCHEMA_VERSION)
            || row.get("source_class").map(String::as_str) != Some(source_class)
            || row.get("status").map(String::as_str) != Some("ready")
            || !expected_sources.contains(&source_rel.as_str())
            || !valid_sha256_hex(digest)
            || row
                .get("source_files")
                .and_then(|value| value.parse::<u64>().ok())
                != Some(1)
            || bytes.is_none_or(|value| value == 0)
            || row
                .get("builder_version")
                .is_none_or(|version| version.trim().is_empty())
            || database_rel.is_empty()
            || relative_database.is_absolute()
            || relative_database.components().any(|component| {
                matches!(
                    component,
                    std::path::Component::ParentDir
                        | std::path::Component::RootDir
                        | std::path::Component::Prefix(_)
                )
            })
            || !database_rel.starts_with("indexes/mmseqs/")
            || !database_paths.insert(database_rel.clone())
        {
            return false;
        }
        let database = store.join(relative_database);
        if ![
            database.clone(),
            PathBuf::from(format!("{}.dbtype", database.display())),
            PathBuf::from(format!("{}.index", database.display())),
            PathBuf::from(format!("{}.lookup", database.display())),
        ]
        .iter()
        .all(|path| fs::metadata(path).is_ok_and(|meta| meta.is_file() && meta.len() > 0))
            || ready_sources
                .insert(
                    source_rel,
                    (digest.to_ascii_lowercase(), bytes.unwrap_or(0)),
                )
                .is_some()
        {
            return false;
        }
    }

    if namespace == "UniProt" {
        return source_files == Some(1)
            && ready_sources.get(expected_sources[0])
                == Some(&(
                    source_digest.to_ascii_lowercase(),
                    source_bytes.unwrap_or(0),
                ));
    }
    if namespace == "MGnify" {
        let Ok(payloads) = serde_json::from_str::<Vec<Value>>(
            retention
                .get("payload_records_json")
                .map(String::as_str)
                .unwrap_or(""),
        ) else {
            return false;
        };
        let matches = payloads
            .iter()
            .filter(|payload| {
                payload
                    .get("path")
                    .and_then(Value::as_str)
                    .map(|path| path.replace('\\', "/"))
                    .as_deref()
                    == Some(expected_sources[0])
            })
            .collect::<Vec<_>>();
        return matches.len() == 1
            && matches[0].get("kind").and_then(Value::as_str) == Some("file")
            && matches[0].get("file_count").and_then(Value::as_u64) == Some(1)
            && matches[0]
                .get("sha256")
                .and_then(Value::as_str)
                .is_some_and(valid_sha256_hex)
            && ready_sources.get(expected_sources[0])
                == matches[0]
                    .get("sha256")
                    .and_then(Value::as_str)
                    .zip(matches[0].get("bytes").and_then(Value::as_u64))
                    .map(|(digest, bytes)| (digest.to_ascii_lowercase(), bytes))
                    .as_ref();
    }
    expected_sources
        .iter()
        .all(|source| ready_sources.contains_key(*source))
}

fn prepare_ligand_similarity_authority(options: &StoreOptions) -> Result<(), String> {
    if validate_ligand_similarity_runtime_contract(options).is_ok() {
        cleanup_superseded_ligand_query_artifacts(&options.science_store)?;
        return ligand_similarity_authority_ready(options);
    }

    prepare_bindingdb_query_database(options)?;
    let chembl_database = options
        .science_store
        .join("downloads/chembl/chembl_query.sqlite");
    let chembl_manifest = options.science_store.join(CHEMBL_QUERY_MANIFEST_REL_PATH);
    let bindingdb_database = options
        .science_store
        .join(BINDINGDB_QUERY_DATABASE_REL_PATH);
    let bindingdb_manifest = options
        .science_store
        .join(BINDINGDB_QUERY_MANIFEST_REL_PATH);
    if !chembl_database.is_file() || !chembl_manifest.is_file() {
        return Err("ligand_similarity_chembl_staging_missing".to_string());
    }
    if !bindingdb_database.is_file() || !bindingdb_manifest.is_file() {
        return Err("ligand_similarity_bindingdb_staging_missing".to_string());
    }
    validate_chembl_query_runtime_contract(
        &options.science_store,
        &chembl_database,
        &chembl_manifest,
    )?;
    validate_bindingdb_query_runtime_contract(
        &options.science_store,
        &bindingdb_database,
        &bindingdb_manifest,
    )?;

    let database = options
        .science_store
        .join(LIGAND_SIMILARITY_DATABASE_REL_PATH);
    let manifest = options
        .science_store
        .join(LIGAND_SIMILARITY_MANIFEST_REL_PATH);
    let arguments = vec![
        "build".to_string(),
        "--chembl-db".to_string(),
        chembl_database.display().to_string(),
        "--chembl-manifest".to_string(),
        chembl_manifest.display().to_string(),
        "--bindingdb-db".to_string(),
        bindingdb_database.display().to_string(),
        "--bindingdb-manifest".to_string(),
        bindingdb_manifest.display().to_string(),
        "--output-db".to_string(),
        database.display().to_string(),
        "--manifest".to_string(),
        manifest.display().to_string(),
    ];
    run_ligand_similarity_module(options, &arguments, "ligand_similarity_build")?;
    validate_ligand_similarity_runtime_contract(options)?;
    cleanup_superseded_ligand_query_artifacts(&options.science_store)?;
    ligand_similarity_authority_ready(options)
}

fn ligand_similarity_authority_ready(options: &StoreOptions) -> Result<(), String> {
    validate_ligand_similarity_runtime_contract(options)?;
    if superseded_ligand_query_artifacts_present(&options.science_store)? {
        return Err("superseded_ligand_query_artifacts_present".to_string());
    }
    Ok(())
}

fn validate_ligand_similarity_runtime_contract(options: &StoreOptions) -> Result<(), String> {
    let database = options
        .science_store
        .join(LIGAND_SIMILARITY_DATABASE_REL_PATH);
    let manifest = options
        .science_store
        .join(LIGAND_SIMILARITY_MANIFEST_REL_PATH);
    if !database.exists() && !manifest.exists() {
        return Err("ligand_similarity_index_missing".to_string());
    }
    if !database.is_file() || !manifest.is_file() {
        return Err("ligand_similarity_index_contract_incomplete".to_string());
    }
    let text =
        read_text(&manifest).map_err(|err| format!("ligand_similarity_manifest_invalid:{err}"))?;
    let value = serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}'))
        .map_err(|err| format!("ligand_similarity_manifest_invalid:json:{err}"))?;
    if value.get("schema_version").and_then(Value::as_str) != Some(LIGAND_SIMILARITY_SCHEMA_VERSION)
        || value.get("status").and_then(Value::as_str) != Some("ready")
    {
        return Err("ligand_similarity_manifest_invalid:fixed_fields".to_string());
    }
    let database_meta = value
        .get("database")
        .and_then(Value::as_object)
        .ok_or_else(|| "ligand_similarity_manifest_invalid:database".to_string())?;
    let database_bytes = fs::metadata(&database)
        .map_err(|err| format!("ligand_similarity_database_metadata_failed:{err}"))?
        .len();
    if database_bytes == 0
        || database_meta.get("bytes").and_then(Value::as_u64) != Some(database_bytes)
        || !database_meta
            .get("sha256")
            .and_then(Value::as_str)
            .is_some_and(valid_sha256_hex)
        || !ligand_similarity_declared_path_matches(
            &database,
            database_meta
                .get("path")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        )
    {
        return Err("ligand_similarity_manifest_invalid:database_identity".to_string());
    }

    let counts = value
        .get("counts")
        .and_then(Value::as_object)
        .ok_or_else(|| "ligand_similarity_manifest_invalid:counts".to_string())?;
    if ["molecules", "targets", "evidence"]
        .iter()
        .any(|name| counts.get(*name).and_then(Value::as_u64).unwrap_or(0) == 0)
        || counts.get("hints").and_then(Value::as_u64).is_none()
    {
        return Err("ligand_similarity_manifest_invalid:empty_counts".to_string());
    }

    let inputs = value
        .get("inputs")
        .and_then(Value::as_object)
        .ok_or_else(|| "ligand_similarity_manifest_invalid:inputs".to_string())?;
    let mut input_bytes = 0_u64;
    let mut source_identity = serde_json::Map::new();
    for source_id in ["chembl", "bindingdb"] {
        let input = inputs
            .get(source_id)
            .and_then(Value::as_object)
            .ok_or_else(|| format!("ligand_similarity_manifest_invalid:input:{source_id}"))?;
        let bytes = input.get("bytes").and_then(Value::as_u64).unwrap_or(0);
        let sha256 = input
            .get("sha256")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let source_digest = input
            .get("source_digest")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let source_release = input
            .get("source_release")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let (expected_digest, expected_release) =
            science_store_source_identity(&options.science_store, source_id)?;
        if bytes == 0
            || !valid_sha256_hex(sha256)
            || source_digest != expected_digest
            || source_release != expected_release
            || input
                .get("path")
                .and_then(Value::as_str)
                .is_none_or(str::is_empty)
            || input
                .get("manifest_path")
                .and_then(Value::as_str)
                .is_none_or(str::is_empty)
        {
            return Err(format!(
                "ligand_similarity_manifest_invalid:source_identity:{source_id}"
            ));
        }
        input_bytes = input_bytes
            .checked_add(bytes)
            .ok_or_else(|| "ligand_similarity_input_bytes_overflow".to_string())?;
        source_identity.insert(
            source_id.to_string(),
            json!({
                "source_digest": source_digest,
                "source_release": source_release,
            }),
        );
    }
    if database_bytes >= input_bytes
        || value
            .get("reclaimed_candidate_bytes")
            .and_then(Value::as_u64)
            != Some(input_bytes - database_bytes)
    {
        return Err("ligand_similarity_manifest_invalid:size_reduction".to_string());
    }

    let fingerprint = value
        .get("fingerprint")
        .and_then(Value::as_object)
        .ok_or_else(|| "ligand_similarity_manifest_invalid:fingerprint".to_string())?;
    let fingerprint_valid = fingerprint.get("type").and_then(Value::as_str) == Some("morgan_ecfp4")
        && fingerprint.get("radius").and_then(Value::as_u64) == Some(2)
        && fingerprint.get("bits").and_then(Value::as_u64) == Some(2048)
        && fingerprint
            .get("include_chirality")
            .and_then(Value::as_bool)
            == Some(true)
        && fingerprint.get("standardization").and_then(Value::as_str)
            == Some(LIGAND_SIMILARITY_STANDARDIZATION)
        && fingerprint
            .get("tautomer_max_tautomers")
            .and_then(Value::as_u64)
            == Some(100)
        && fingerprint
            .get("tautomer_max_transforms")
            .and_then(Value::as_u64)
            == Some(100)
        && fingerprint.get("lsh_bands").and_then(Value::as_u64) == Some(0)
        && fingerprint.get("search_algorithm").and_then(Value::as_str)
            == Some("popcount_bitbound_exact_tanimoto")
        && fingerprint.get("bitbound_formula").and_then(Value::as_str)
            == Some("ceil(t*q_popcount) <= db_popcount <= floor(q_popcount/t)")
        && fingerprint.get("reference")
            == Some(&json!({"name":"FPSim2","license":"MIT","commit":"e03e669"}))
        && matches!(
            fingerprint.get("encoding").and_then(Value::as_str),
            Some("rdkit_binary_text" | "zlib1_rdkit_binary_text")
        );
    if !fingerprint_valid {
        return Err("ligand_similarity_manifest_invalid:fingerprint".to_string());
    }
    let stable_fingerprint = json!({
        "type": fingerprint.get("type"),
        "radius": fingerprint.get("radius"),
        "bits": fingerprint.get("bits"),
        "include_chirality": fingerprint.get("include_chirality"),
        "standardization": fingerprint.get("standardization"),
        "tautomer_max_tautomers": fingerprint.get("tautomer_max_tautomers"),
        "tautomer_max_transforms": fingerprint.get("tautomer_max_transforms"),
        "lsh_bands": fingerprint.get("lsh_bands"),
        "search_algorithm": fingerprint.get("search_algorithm"),
        "bitbound_formula": fingerprint.get("bitbound_formula"),
        "reference": fingerprint.get("reference"),
        "encoding": fingerprint.get("encoding"),
    });
    let expected = json!({
        "counts": Value::Object(counts.clone()),
        "fingerprint": stable_fingerprint,
        "source_identity": Value::Object(source_identity),
    });
    run_python_script(
        ligand_similarity_sqlite_contract_script(),
        &[database.display().to_string(), expected.to_string()],
        "ligand_similarity_contract",
    )?;
    Ok(())
}

fn ligand_similarity_declared_path_matches(database: &Path, declared: &str) -> bool {
    let declared = wsl_path_to_windows(declared).unwrap_or_else(|| PathBuf::from(declared));
    if let (Ok(database), Ok(declared)) = (database.canonicalize(), declared.canonicalize()) {
        return declared == database;
    }
    if !database.is_absolute()
        || !declared.is_absolute()
        || database
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
        || declared
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return false;
    }
    let expected = database.to_string_lossy().replace('\\', "/");
    let declared = declared.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        declared.eq_ignore_ascii_case(&expected)
    } else {
        declared == expected
    }
}

fn ligand_similarity_sqlite_contract_script() -> &'static str {
    r#"
import json
import sqlite3
import sys
from pathlib import Path

database, expected_json = sys.argv[1:3]
expected = json.loads(expected_json)
uri = Path(database).resolve().as_uri() + "?mode=ro&immutable=1"
connection = sqlite3.connect(uri, uri=True)
try:
    objects = {
        (str(row[0]), str(row[1]))
        for row in connection.execute(
            "SELECT type, name FROM sqlite_master WHERE type IN ('table', 'index')"
        )
    }
    required = {
        ("table", "molecule"), ("table", "target"),
        ("table", "evidence"), ("table", "structure_hint"),
        ("table", "metadata"),
        ("index", "idx_molecule_popcount"),
    }
    if not required.issubset(objects):
        raise RuntimeError("ligand_similarity_database_objects_missing")
    if any(kind == "table" and name.startswith("_build_") for kind, name in objects):
        raise RuntimeError("ligand_similarity_build_tables_retained")
    required_columns = {
        "molecule": {"id", "canonical_smiles", "fingerprint", "popcount"},
        "target": {
            "id", "source", "target_id", "source_target_id", "target_name",
            "organism", "target_class", "source_release", "source_digest",
        },
        "evidence": {
            "molecule_id", "target_fk", "activity_score", "activity",
            "relation_rank", "numeric_rank",
        },
        "structure_hint": {
            "molecule_id", "target_fk", "ligand_code", "pdb_ids",
            "target_chain_pdb_ids",
        },
        "metadata": {"key", "value"},
    }
    for table, columns in required_columns.items():
        actual = {str(row[1]) for row in connection.execute(f"PRAGMA table_info({table})")}
        if not columns.issubset(actual):
            raise RuntimeError(f"ligand_similarity_columns_missing:{table}")
    metadata = dict(connection.execute("SELECT key, value FROM metadata"))
    if metadata.get("schema_version") != "genoligand_ligand_similarity_index_v2":
        raise RuntimeError("ligand_similarity_metadata_schema_invalid")
    if metadata.get("status") != "ready":
        raise RuntimeError("ligand_similarity_metadata_status_invalid")
    for key in ("counts", "fingerprint", "source_identity"):
        if json.loads(metadata.get(key, "{}")) != expected[key]:
            raise RuntimeError(f"ligand_similarity_metadata_mismatch:{key}")
    sample = connection.execute(
        "SELECT m.fingerprint, m.popcount, t.source_digest, e.activity_score "
        "FROM evidence e JOIN molecule m ON m.id=e.molecule_id "
        "JOIN target t ON t.id=e.target_fk LIMIT 1"
    ).fetchone()
    if (
        sample is None or not sample[0] or not 1 <= int(sample[1]) <= 2048
        or len(str(sample[2])) != 64 or not 0.0 <= float(sample[3]) <= 1.0
    ):
        raise RuntimeError("ligand_similarity_sample_invalid")
    if int(expected["counts"]["hints"]) > 0:
        hint = connection.execute(
            "SELECT ligand_code, pdb_ids, target_chain_pdb_ids FROM structure_hint LIMIT 1"
        ).fetchone()
        if hint is None or not any(str(value).strip() for value in hint):
            raise RuntimeError("ligand_similarity_structure_hint_sample_invalid")
    plan = connection.execute(
        "EXPLAIN QUERY PLAN SELECT id FROM molecule INDEXED BY idx_molecule_popcount "
        "WHERE popcount BETWEEN ? AND ?", (1, 2048)
    ).fetchall()
    if not any("IDX_MOLECULE_POPCOUNT" in str(row[-1]).upper() for row in plan):
        raise RuntimeError("ligand_similarity_popcount_index_unused")
finally:
    connection.close()
print("ready")
"#
}

fn run_ligand_similarity_module(
    options: &StoreOptions,
    arguments: &[String],
    context: &str,
) -> Result<String, String> {
    let python_root = options.workspace_root.join("python");
    let mut errors = Vec::new();
    for candidate in python_command_candidates() {
        let mut command = Command::new(&candidate[0]);
        command.args(&candidate[1..]);
        command
            .arg("-m")
            .arg("genoligand_workers.ligand_similarity_index");
        for argument in arguments {
            command.arg(python_script_arg(&candidate, argument));
        }
        if python_root.exists()
            && let Some(joined) = worker_pythonpath_env(&candidate, &python_root)
        {
            command.env("PYTHONPATH", joined);
            if is_wsl_python_wrapper(&candidate) {
                command.env("WSLENV", wsl_env_with_pythonpath());
            }
        }
        match command.output() {
            Ok(output) if output.status.success() => {
                return Ok(decode_process_bytes(&output.stdout));
            }
            Ok(output) => {
                let stderr = decode_process_bytes(&output.stderr);
                let stdout = decode_process_bytes(&output.stdout);
                let detail = stderr
                    .lines()
                    .find(|line| !line.trim().is_empty())
                    .or_else(|| stdout.lines().find(|line| !line.trim().is_empty()))
                    .unwrap_or("ligand_similarity_module_failed");
                errors.push(format!("{}:{detail}", candidate.join(" ")));
            }
            Err(err) => errors.push(format!("{}:{err}", candidate.join(" "))),
        }
    }
    Err(format!(
        "{context}_python_failed:{}",
        clean_cell(&errors.join(" | "))
    ))
}

fn cleanup_superseded_ligand_query_artifacts(science_store: &Path) -> Result<(), String> {
    for path in superseded_ligand_query_fixed_paths(science_store) {
        remove_controlled_store_artifact(science_store, &path)?;
    }
    for (directory, prefixes) in superseded_ligand_query_temp_roots(science_store) {
        if !directory.is_dir() {
            continue;
        }
        for entry in fs::read_dir(&directory)
            .map_err(|err| format!("ligand_cleanup_read_failed:{}:{err}", directory.display()))?
        {
            let entry = entry.map_err(|err| {
                format!("ligand_cleanup_entry_failed:{}:{err}", directory.display())
            })?;
            let name = entry.file_name().to_string_lossy().to_string();
            if prefixes.iter().any(|prefix| name.starts_with(prefix)) {
                remove_controlled_store_artifact(science_store, &entry.path())?;
            }
        }
    }
    Ok(())
}

fn superseded_ligand_query_artifacts_present(science_store: &Path) -> Result<bool, String> {
    if superseded_ligand_query_fixed_paths(science_store)
        .iter()
        .any(|path| fs::symlink_metadata(path).is_ok())
    {
        return Ok(true);
    }
    for (directory, prefixes) in superseded_ligand_query_temp_roots(science_store) {
        if !directory.is_dir() {
            continue;
        }
        for entry in fs::read_dir(&directory)
            .map_err(|err| format!("ligand_cleanup_read_failed:{}:{err}", directory.display()))?
        {
            let entry = entry.map_err(|err| {
                format!("ligand_cleanup_entry_failed:{}:{err}", directory.display())
            })?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if prefixes.iter().any(|prefix| name.starts_with(prefix)) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn superseded_ligand_query_fixed_paths(science_store: &Path) -> Vec<PathBuf> {
    let databases = [
        science_store.join("downloads/chembl/chembl_query.sqlite"),
        science_store.join(BINDINGDB_QUERY_DATABASE_REL_PATH),
    ];
    let manifests = [
        science_store.join(CHEMBL_QUERY_MANIFEST_REL_PATH),
        science_store.join(BINDINGDB_QUERY_MANIFEST_REL_PATH),
    ];
    let mut paths = Vec::new();
    for database in databases {
        paths.push(database.clone());
        for suffix in ["-wal", "-shm", "-journal"] {
            paths.push(PathBuf::from(format!("{}{suffix}", database.display())));
        }
    }
    for manifest in manifests {
        paths.push(manifest.clone());
        paths.push(PathBuf::from(format!("{}.pending", manifest.display())));
        paths.push(PathBuf::from(format!("{}.previous", manifest.display())));
    }
    paths.push(
        science_store
            .join("downloads/chembl")
            .join(CHEMBL_QUERY_HASH_CACHE_DIR),
    );
    paths
}

fn superseded_ligand_query_temp_roots(science_store: &Path) -> [(PathBuf, [&'static str; 2]); 2] {
    [
        (
            science_store.join("downloads/chembl"),
            [".chembl_query_", ".chembl_query_manifest_"],
        ),
        (
            science_store.join("indexes/bindingdb"),
            [".bindingdb_query_", ".bindingdb_query_manifest_"],
        ),
    ]
}

fn remove_controlled_store_artifact(science_store: &Path, path: &Path) -> Result<(), String> {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return Ok(());
    };
    let root = science_store.canonicalize().map_err(|err| {
        format!(
            "ligand_cleanup_store_canonicalize_failed:{}:{err}",
            science_store.display()
        )
    })?;
    if metadata.file_type().is_symlink() {
        return fs::remove_file(path)
            .or_else(|_| fs::remove_dir(path))
            .map_err(|err| format!("ligand_cleanup_symlink_failed:{}:{err}", path.display()));
    }
    let resolved = path
        .canonicalize()
        .map_err(|err| format!("ligand_cleanup_path_invalid:{}:{err}", path.display()))?;
    if resolved == root || !resolved.starts_with(&root) {
        return Err(format!(
            "ligand_cleanup_path_outside_store:{}",
            path.display()
        ));
    }
    if metadata.is_file() {
        #[cfg(windows)]
        if metadata.permissions().readonly() {
            let mut permissions = metadata.permissions();
            permissions.set_readonly(false);
            fs::set_permissions(&resolved, permissions).map_err(|err| {
                format!(
                    "ligand_cleanup_permissions_failed:{}:{err}",
                    resolved.display()
                )
            })?;
        }
        fs::remove_file(&resolved)
            .map_err(|err| format!("ligand_cleanup_remove_failed:{}:{err}", resolved.display()))
    } else if metadata.is_dir() {
        fs::remove_dir_all(&resolved)
            .map_err(|err| format!("ligand_cleanup_remove_failed:{}:{err}", resolved.display()))
    } else {
        Err(format!(
            "ligand_cleanup_unsupported_artifact:{}",
            path.display()
        ))
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct IndexBuildSummary {
    known_rows: usize,
    metagenome_rows: usize,
    structure_rows: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct StructureSourceIdentity {
    source_id: String,
    source_digest: String,
    release_identity: String,
    trust_source: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct StructureInputIdentity {
    bytes: u64,
    digest: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SourcePayloadIdentity {
    file_count: u64,
    bytes: u64,
    digest: String,
}

struct StoreRowSpool {
    path: PathBuf,
    writer: Option<BufWriter<fs::File>>,
    rows: usize,
}

impl StoreRowSpool {
    fn create(root: &Path, label: &str) -> Result<Self, String> {
        ensure_dir(root)?;
        let path = temporary_data_path(root, label, "tsv");
        let file = fs::File::create(&path)
            .map_err(|err| format!("create_failed:{}:{err}", path.display()))?;
        let mut writer = BufWriter::new(file);
        writer
            .write_all(store_row_header().as_bytes())
            .map_err(|err| format!("write_failed:{}:{err}", path.display()))?;
        Ok(Self {
            path,
            writer: Some(writer),
            rows: 0,
        })
    }

    fn append(&mut self, rows: &[StoreRow]) -> Result<(), String> {
        let writer = self
            .writer
            .as_mut()
            .ok_or_else(|| "store_row_spool_already_finished".to_string())?;
        for row in rows {
            write_store_row(writer, row)
                .map_err(|err| format!("write_failed:{}:{err}", self.path.display()))?;
        }
        self.rows += rows.len();
        Ok(())
    }

    fn finish(mut self, output: &Path) -> Result<usize, String> {
        let mut writer = self
            .writer
            .take()
            .ok_or_else(|| "store_row_spool_already_finished".to_string())?;
        writer
            .flush()
            .map_err(|err| format!("write_failed:{}:{err}", self.path.display()))?;
        let row_count = self.rows;
        drop(writer);
        if row_count == 0 {
            let parent = output
                .parent()
                .ok_or_else(|| format!("index_output_parent_missing:{}", output.display()))?;
            let pending = temporary_data_path(parent, "empty_store_rows", "tsv");
            write_store_rows(&pending, &[])?;
            publish_store_row_index(&pending, output)?;
            return Ok(0);
        }
        dedupe_store_row_spool(&self.path, output)
    }

    fn finish_preserving_published(
        self,
        output: &Path,
        published_rows: Option<usize>,
    ) -> Result<(usize, bool), String> {
        if let Some(rows) = published_rows {
            return Ok((rows, true));
        }
        self.finish(output).map(|rows| (rows, false))
    }
}

fn published_store_index_has_rows(path: &Path) -> Result<bool, String> {
    if !path.is_file() {
        return Ok(false);
    }
    let file = fs::File::open(path)
        .map_err(|err| format!("published_index_open_failed:{}:{err}", path.display()))?;
    let mut lines = BufReader::new(file).lines();
    let header = lines
        .next()
        .transpose()
        .map_err(|err| format!("published_index_read_failed:{}:{err}", path.display()))?
        .unwrap_or_default();
    if header.trim_start_matches('\u{feff}') != store_row_header().trim_end() {
        return Ok(false);
    }
    for line in lines {
        let line =
            line.map_err(|err| format!("published_index_read_failed:{}:{err}", path.display()))?;
        if line.trim().is_empty() {
            continue;
        }
        let cols = split_tsv(line.trim_start_matches('\u{feff}'));
        return Ok(cols.len() >= 9 && !cols[0].trim().is_empty() && !cols[1].trim().is_empty());
    }
    Ok(false)
}

fn store_row_count(path: &Path, context: &str) -> Result<u64, String> {
    let file = fs::File::open(path).map_err(|err| format!("{context}_open_failed:{err}"))?;
    let mut lines = BufReader::new(file).lines();
    let header = lines
        .next()
        .transpose()
        .map_err(|err| format!("{context}_read_failed:{err}"))?
        .unwrap_or_default();
    if header.trim_start_matches('\u{feff}') != store_row_header().trim_end() {
        return Err(format!("{context}_header_invalid"));
    }
    let mut rows = 0_u64;
    for line in lines {
        if !line
            .map_err(|err| format!("{context}_read_failed:{err}"))?
            .trim()
            .is_empty()
        {
            rows = rows
                .checked_add(1)
                .ok_or_else(|| format!("{context}_row_count_overflow"))?;
        }
    }
    Ok(rows)
}

fn validate_plinder_canonical_publication(store: &Path) -> Result<u64, String> {
    let canonical = store.join("indexes/plinder_targets.tsv");
    let manifest_path = store.join("indexes/plinder_targets.manifest.json");
    let manifest = read_json_value_if_file(&manifest_path)
        .ok_or_else(|| "plinder_canonical_manifest_invalid".to_string())?;
    let publication = manifest
        .get("canonical")
        .and_then(Value::as_object)
        .ok_or_else(|| "plinder_canonical_identity_missing".to_string())?;
    let source = manifest
        .get("source")
        .and_then(Value::as_object)
        .ok_or_else(|| "plinder_canonical_source_identity_missing".to_string())?;
    let rows = publication
        .get("row_count")
        .and_then(Value::as_u64)
        .filter(|rows| *rows > 0)
        .ok_or_else(|| "plinder_canonical_row_count_invalid".to_string())?;
    let bytes = fs::metadata(&canonical)
        .map_err(|err| format!("plinder_canonical_metadata_failed:{err}"))?
        .len();
    let declared_path = publication
        .get("path")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let declared_sha = publication
        .get("sha256")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let source_path = source
        .get("path")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let source_name = source_path
        .replace('\\', "/")
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let expected_source = store.join("downloads/plinder/index").join(&source_name);
    let source_bytes = source
        .get("size_bytes")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let source_sha = source
        .get("sha256")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let parquet_rows = source.get("parquet_rows").and_then(Value::as_u64);
    let filtered_rows = manifest
        .get("scan")
        .and_then(|scan| scan.get("filtered_rows"))
        .and_then(Value::as_u64);
    let scan_rows_conserved = match (parquet_rows, filtered_rows) {
        (Some(parquet), Some(filtered)) => rows.checked_add(filtered) == Some(parquet),
        (Some(parquet), None) => {
            manifest.get("schema_version").and_then(Value::as_str)
                == Some(PLINDER_CANONICAL_MANIFEST_SCHEMA_VERSION)
                && rows == parquet
        }
        _ => false,
    };
    let source_identity_ready = if expected_source.is_file() {
        fs::metadata(&expected_source).is_ok_and(|meta| meta.len() == source_bytes)
            && sha256_file_hex(&expected_source).as_deref() == Some(source_sha)
    } else {
        read_simple_tsv_rows(&store.join("manifests/source_retention_manifest.tsv")).is_ok_and(
            |rows| {
                rows.iter().any(|row| {
                    row.get("schema_version").map(String::as_str)
                        == Some(SOURCE_RETENTION_SCHEMA_VERSION)
                        && row.get("source_id").map(String::as_str) == Some("plinder")
                        && row.get("state").map(String::as_str) == Some("pruned")
                        && row.get("source_local_path").is_some_and(|path| {
                            path.replace('\\', "/") == "downloads/plinder/index"
                        })
                        && row
                            .get("source_bytes")
                            .and_then(|value| value.parse::<u64>().ok())
                            == Some(source_bytes)
                        && row
                            .get("source_manifest_sha256")
                            .is_some_and(|digest| valid_sha256_hex(digest))
                        && row
                            .get("release_identity")
                            .is_some_and(|value| !value.trim().is_empty())
                        && row
                            .get("trust_source")
                            .is_some_and(|value| !value.trim().is_empty())
                })
            },
        )
    };
    if manifest.get("schema_version").and_then(Value::as_str)
        != Some(PLINDER_CANONICAL_MANIFEST_SCHEMA_VERSION)
        || manifest.get("status").and_then(Value::as_str) != Some("ready")
        || !matches!(
            source_name.as_str(),
            "annotation_table.parquet" | "annotation_table_nonredundant.parquet"
        )
        || !ligand_similarity_declared_path_matches(&expected_source, source_path)
        || source_bytes == 0
        || !valid_sha256_hex(source_sha)
        || !scan_rows_conserved
        || source
            .get("row_groups")
            .and_then(Value::as_u64)
            .is_none_or(|value| value == 0)
        || source
            .get("schema_field_count")
            .and_then(Value::as_u64)
            .is_none_or(|value| value == 0)
        || !source
            .get("schema_sha256")
            .and_then(Value::as_str)
            .is_some_and(valid_sha256_hex)
        || source.get("schema_mapping") != manifest.get("schema_mapping")
        || !source_identity_ready
        || !ligand_similarity_declared_path_matches(&canonical, declared_path)
        || publication.get("size_bytes").and_then(Value::as_u64) != Some(bytes)
        || !valid_sha256_hex(declared_sha)
        || sha256_file_hex(&canonical).as_deref() != Some(declared_sha)
        || store_row_count(&canonical, "plinder_canonical")? != rows
    {
        return Err("plinder_canonical_publication_mismatch".to_string());
    }
    Ok(rows)
}

fn structure_contribution_source(notes: &str) -> Option<&'static str> {
    if notes.contains("plinder_canonical_rows") {
        Some("PLINDER")
    } else if notes.contains("biolip2_interaction_rows") {
        Some("BioLiP2")
    } else if notes.contains("binding_moad_complex_rows") {
        Some("Binding MOAD")
    } else if notes.contains("rcsb_structure_file_rows") {
        Some("RCSB")
    } else {
        None
    }
}

fn structure_contributions(
    manifest_path: &Path,
) -> Result<BTreeMap<String, (u64, BTreeSet<String>)>, String> {
    let mut contributions = BTreeMap::<String, (u64, BTreeSet<String>)>::new();
    for row in read_simple_tsv_rows(manifest_path)? {
        if row.get("index").map(String::as_str) != Some("structure_targets")
            || row.get("status").map(String::as_str) != Some("used")
            || row
                .get("output_path")
                .map(|path| path.replace('\\', "/"))
                .as_deref()
                != Some("indexes/structure_targets.tsv")
        {
            continue;
        }
        let Some(source) = row
            .get("notes")
            .and_then(|notes| structure_contribution_source(notes))
        else {
            continue;
        };
        let rows = row
            .get("rows")
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|rows| *rows > 0)
            .ok_or_else(|| format!("structure_contribution_rows_invalid:{source}"))?;
        let path = row
            .get("input_path")
            .map(|path| path.replace('\\', "/"))
            .filter(|path| !path.trim().is_empty())
            .ok_or_else(|| format!("structure_contribution_path_missing:{source}"))?;
        let entry = contributions
            .entry(source.to_string())
            .or_insert_with(|| (0, BTreeSet::new()));
        entry.0 = entry
            .0
            .checked_add(rows)
            .ok_or_else(|| format!("structure_contribution_rows_overflow:{source}"))?;
        entry.1.insert(path);
    }
    Ok(contributions)
}

fn required_structure_contributions(store: &Path) -> BTreeSet<String> {
    let mut required = BTreeSet::new();
    if let Some(manifest) =
        read_json_value_if_file(&store.join(STRUCTURE_TARGETS_MANIFEST_REL_PATH))
        && let Some(contributions) = manifest.get("contributions").and_then(Value::as_array)
    {
        for contribution in contributions {
            if let Some(source) = contribution.get("source").and_then(Value::as_str)
                && structure_source_id(source).is_some()
            {
                required.insert(source.to_string());
            }
        }
    }
    if let Ok(rows) = read_simple_tsv_rows(&store.join("manifests/source_file_manifest.tsv")) {
        for row in rows {
            if row.get("status").map(String::as_str) != Some("ready") {
                continue;
            }
            if let Some(source) = match row.get("source_id").map(String::as_str) {
                Some("plinder") => Some("PLINDER"),
                Some("biolip2") => Some("BioLiP2"),
                Some("binding_moad") => Some("Binding MOAD"),
                Some("rcsb_ligand_bound") => Some("RCSB"),
                _ => None,
            } {
                required.insert(source.to_string());
            }
        }
    }
    if let Ok(rows) = read_simple_tsv_rows(&store.join("manifests/source_retention_manifest.tsv")) {
        for row in rows {
            if row.get("schema_version").map(String::as_str)
                != Some(SOURCE_RETENTION_SCHEMA_VERSION)
                || row.get("state").map(String::as_str) != Some("pruned")
                || !row.get("derived_paths").is_some_and(|paths| {
                    paths.split(';').any(|path| {
                        path.trim().replace('\\', "/") == "indexes/structure_targets.tsv"
                    })
                })
            {
                continue;
            }
            if let Some(source) = match row.get("source_id").map(String::as_str) {
                Some("plinder") => Some("PLINDER"),
                Some("biolip2") => Some("BioLiP2"),
                Some("binding_moad") => Some("Binding MOAD"),
                _ => None,
            } {
                required.insert(source.to_string());
            }
        }
    }
    if current_valid_rcsb_cache_ids(store).is_some_and(|ids| !ids.is_empty()) {
        required.insert("RCSB".to_string());
    }
    required
}

fn require_structure_contributions(
    store: &Path,
    contributions: &BTreeMap<String, (u64, BTreeSet<String>)>,
    unique_rows: &BTreeMap<String, u64>,
    verified_trust_sources: Option<&BTreeMap<String, String>>,
) -> Result<(), String> {
    for source in required_structure_contributions(store) {
        let Some((rows, paths)) = contributions.get(&source) else {
            return Err(format!("structure_contribution_missing:{source}"));
        };
        if *rows == 0 || paths.is_empty() {
            return Err(format!("structure_contribution_missing:{source}"));
        }
        let trust_source = if let Some(verified) = verified_trust_sources {
            verified
                .get(&source)
                .ok_or_else(|| format!("structure_contribution_identity_missing:{source}"))?
                .clone()
        } else {
            structure_source_identity(store, &source)?.trust_source
        };
        let minimum = if trust_source == "unit_test" {
            1
        } else {
            match source.as_str() {
                "PLINDER" | "BioLiP2" => 100_000,
                "Binding MOAD" => 1_000,
                "RCSB" => 1,
                _ => 1,
            }
        };
        let unique = unique_rows.get(&source).copied().unwrap_or(0);
        if unique < minimum {
            return Err(format!(
                "structure_contribution_incomplete:{source}:{unique}:{minimum}"
            ));
        }
    }
    Ok(())
}

fn structure_output_counts(path: &Path) -> Result<(u64, BTreeMap<String, u64>), String> {
    let file = fs::File::open(path)
        .map_err(|err| format!("structure_targets_open_failed:{}:{err}", path.display()))?;
    let mut lines = BufReader::new(file).lines();
    let header = lines
        .next()
        .transpose()
        .map_err(|err| format!("structure_targets_read_failed:{err}"))?
        .unwrap_or_default();
    if header.trim_start_matches('\u{feff}') != store_row_header().trim_end() {
        return Err("structure_targets_header_invalid".to_string());
    }
    let mut counts = BTreeMap::new();
    let mut total = 0_u64;
    for line in lines {
        let line = line.map_err(|err| format!("structure_targets_read_failed:{err}"))?;
        if line.trim().is_empty() {
            continue;
        }
        total = total
            .checked_add(1)
            .ok_or_else(|| "structure_targets_row_count_overflow".to_string())?;
        let source = line.split('\t').next().unwrap_or_default().trim();
        if structure_source_id(source).is_some() {
            let count = counts.entry(source.to_string()).or_insert(0_u64);
            *count = count
                .checked_add(1)
                .ok_or_else(|| format!("structure_targets_source_count_overflow:{source}"))?;
        }
    }
    Ok((total, counts))
}

fn structure_source_id(source: &str) -> Option<&'static str> {
    match source {
        "PLINDER" => Some("plinder"),
        "BioLiP2" => Some("biolip2"),
        "Binding MOAD" => Some("binding_moad"),
        "RCSB" => Some("rcsb_ligand_bound"),
        _ => None,
    }
}

fn source_payload_file_included(relative: &str) -> bool {
    let normalized = relative.replace('\\', "/").to_ascii_lowercase();
    let name = normalized.rsplit('/').next().unwrap_or_default();
    !(name.ends_with(".part")
        || name.contains(".part.quarantine-")
        || normalized.contains(".rebuild-")
        || normalized.contains(".previous-")
        || name.ends_with(".attempt.json")
        || name.ends_with(".identity.json")
        || name.ends_with(".tmp"))
}

fn collect_source_payload_files(
    root: &Path,
    current: &Path,
    out: &mut Vec<(String, PathBuf, u64)>,
) -> Result<(), String> {
    let metadata = fs::symlink_metadata(current)
        .map_err(|err| format!("source_payload_metadata_failed:{}:{err}", current.display()))?;
    if metadata.file_type().is_symlink() {
        return Err(format!(
            "source_payload_symlink_rejected:{}",
            current.display()
        ));
    }
    if metadata.is_dir() {
        let mut entries = fs::read_dir(current)
            .map_err(|err| format!("source_payload_read_dir_failed:{}:{err}", current.display()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| format!("source_payload_read_dir_entry_failed:{err}"))?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            collect_source_payload_files(root, &entry.path(), out)?;
        }
        return Ok(());
    }
    if !metadata.is_file() || metadata.len() == 0 {
        return Ok(());
    }
    let relative = if root.is_file() {
        current
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_string()
    } else {
        current
            .strip_prefix(root)
            .map_err(|err| format!("source_payload_relative_path_failed:{err}"))?
            .to_string_lossy()
            .replace('\\', "/")
    };
    if source_payload_file_included(&relative) {
        out.push((relative, current.to_path_buf(), metadata.len()));
    }
    Ok(())
}

fn source_payload_identity(path: &Path) -> Result<SourcePayloadIdentity, String> {
    let mut files = Vec::new();
    collect_source_payload_files(path, path, &mut files)?;
    files.sort_by(|left, right| left.0.cmp(&right.0));
    let mut bytes = 0_u64;
    let mut hasher = Sha256::new();
    for (relative, file, size) in &files {
        let digest = sha256_file_hex(file)
            .ok_or_else(|| format!("source_payload_sha256_failed:{}", file.display()))?;
        bytes = bytes
            .checked_add(*size)
            .ok_or_else(|| "source_payload_bytes_overflow".to_string())?;
        hasher.update(relative.as_bytes());
        hasher.update(size.to_string().as_bytes());
        hasher.update(digest.as_bytes());
    }
    if files.is_empty() || bytes == 0 {
        return Err(format!("source_payload_empty:{}", path.display()));
    }
    Ok(SourcePayloadIdentity {
        file_count: files.len() as u64,
        bytes,
        digest: format!("{:x}", hasher.finalize()),
    })
}

fn structure_source_identity(
    store: &Path,
    source: &str,
) -> Result<StructureSourceIdentity, String> {
    let source_id = structure_source_id(source)
        .ok_or_else(|| format!("structure_source_id_unknown:{source}"))?;
    let rows = read_simple_tsv_rows(&store.join("manifests/source_file_manifest.tsv"))?;
    let mut matches = rows.iter().filter(|row| {
        row.get("source_id").map(String::as_str) == Some(source_id)
            && row.get("status").map(String::as_str) == Some("ready")
    });
    let row = matches
        .next()
        .ok_or_else(|| format!("structure_source_identity_missing:{source_id}"))?;
    if matches.next().is_some() {
        return Err(format!("structure_source_identity_duplicate:{source_id}"));
    }
    let source_digest = row
        .get("sha256")
        .filter(|value| valid_sha256_hex(value))
        .cloned()
        .ok_or_else(|| format!("structure_source_digest_invalid:{source_id}"))?;
    let release_identity = row
        .get("release_identity")
        .filter(|value| !value.trim().is_empty())
        .cloned()
        .ok_or_else(|| format!("structure_source_release_invalid:{source_id}"))?;
    let trust_source = row
        .get("trust_source")
        .filter(|value| !value.trim().is_empty())
        .cloned()
        .ok_or_else(|| format!("structure_source_trust_invalid:{source_id}"))?;
    let storage_state = row.get("storage_state").map(String::as_str).unwrap_or("");
    if storage_state == "indexed_source_pruned" {
        let retention_rows =
            read_simple_tsv_rows(&store.join("manifests/source_retention_manifest.tsv"))?;
        let mut retention_matches = retention_rows
            .iter()
            .filter(|entry| entry.get("source_id").map(String::as_str) == Some(source_id));
        let retention = retention_matches
            .next()
            .ok_or_else(|| format!("structure_source_retention_missing:{source_id}"))?;
        if retention_matches.next().is_some()
            || retention.get("schema_version").map(String::as_str)
                != Some(SOURCE_RETENTION_SCHEMA_VERSION)
            || retention.get("state").map(String::as_str) != Some("pruned")
            || retention.get("source_manifest_sha256").map(String::as_str)
                != Some(source_digest.as_str())
            || retention.get("release_identity").map(String::as_str)
                != Some(release_identity.as_str())
            || retention.get("trust_source").map(String::as_str) != Some(trust_source.as_str())
        {
            return Err(format!("structure_source_retention_mismatch:{source_id}"));
        }
    } else if matches!(
        storage_state,
        "materialized" | "runtime_cache_bounded" | "bounded_runtime_cache"
    ) {
        let local_path = row
            .get("local_path")
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| format!("structure_source_local_path_missing:{source_id}"))?;
        let relative = Path::new(local_path);
        if relative.is_absolute()
            || relative.components().any(|component| {
                matches!(
                    component,
                    std::path::Component::ParentDir
                        | std::path::Component::RootDir
                        | std::path::Component::Prefix(_)
                )
            })
        {
            return Err(format!("structure_source_local_path_invalid:{source_id}"));
        }
        let actual = source_payload_identity(&store.join(relative))?;
        let expected_files = row
            .get("file_count")
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|value| *value > 0)
            .ok_or_else(|| format!("structure_source_file_count_invalid:{source_id}"))?;
        let expected_bytes = row
            .get("bytes")
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|value| *value > 0)
            .ok_or_else(|| format!("structure_source_bytes_invalid:{source_id}"))?;
        if actual.file_count != expected_files
            || actual.bytes != expected_bytes
            || !actual.digest.eq_ignore_ascii_case(&source_digest)
        {
            return Err(format!(
                "structure_source_materialized_identity_mismatch:{source_id}"
            ));
        }
    } else if storage_state == "unit_test" {
        #[cfg(not(test))]
        return Err(format!(
            "structure_source_storage_state_invalid:{source_id}"
        ));
    } else {
        return Err(format!(
            "structure_source_storage_state_invalid:{source_id}"
        ));
    }
    Ok(StructureSourceIdentity {
        source_id: source_id.to_string(),
        source_digest: source_digest.to_ascii_lowercase(),
        release_identity,
        trust_source,
    })
}

fn retained_rcsb_structure_identity_matches(
    store: &Path,
    source_digest: &str,
    release_identity: &str,
    trust_source: &str,
    input_bytes: u64,
    input_digest: &str,
) -> bool {
    let Ok(rows) = read_simple_tsv_rows(&store.join("manifests/source_retention_manifest.tsv"))
    else {
        return false;
    };
    let mut matches = rows
        .iter()
        .filter(|row| row.get("source_id").map(String::as_str) == Some("rcsb_ligand_bound"));
    let Some(row) = matches.next() else {
        return false;
    };
    matches.next().is_none()
        && row.get("schema_version").map(String::as_str) == Some(SOURCE_RETENTION_SCHEMA_VERSION)
        && row.get("state").map(String::as_str) == Some("runtime_cache_bounded")
        && row.get("source_manifest_sha256").map(String::as_str) == Some(source_digest)
        && row.get("release_identity").map(String::as_str) == Some(release_identity)
        && row.get("trust_source").map(String::as_str) == Some(trust_source)
        && row
            .get("structure_input_bytes")
            .and_then(|value| value.parse::<u64>().ok())
            == Some(input_bytes)
        && row.get("structure_input_digest").map(String::as_str) == Some(input_digest)
}

fn structure_input_identity(
    store: &Path,
    source_paths: &BTreeSet<String>,
) -> Result<StructureInputIdentity, String> {
    let mut bytes = 0_u64;
    let mut hasher = Sha256::new();
    for relative in source_paths {
        let normalized = relative.replace('\\', "/");
        let relative_path = Path::new(&normalized);
        if relative_path.is_absolute()
            || relative_path.components().any(|component| {
                matches!(
                    component,
                    std::path::Component::ParentDir
                        | std::path::Component::RootDir
                        | std::path::Component::Prefix(_)
                )
            })
        {
            return Err(format!("structure_input_path_invalid:{relative}"));
        }
        let path = store.join(relative_path);
        let size = fs::metadata(&path)
            .map_err(|err| format!("structure_input_metadata_failed:{}:{err}", path.display()))?
            .len();
        if size == 0 || !path.is_file() {
            return Err(format!("structure_input_file_invalid:{}", path.display()));
        }
        let digest = sha256_file_hex(&path)
            .ok_or_else(|| format!("structure_input_sha256_failed:{}", path.display()))?;
        bytes = bytes
            .checked_add(size)
            .ok_or_else(|| "structure_input_bytes_overflow".to_string())?;
        hasher.update(normalized.as_bytes());
        hasher.update(size.to_string().as_bytes());
        hasher.update(digest.as_bytes());
    }
    if source_paths.is_empty() || bytes == 0 {
        return Err("structure_input_identity_empty".to_string());
    }
    Ok(StructureInputIdentity {
        bytes,
        digest: format!("{:x}", hasher.finalize()),
    })
}

fn write_structure_targets_manifest(store: &Path, index_manifest: &Path) -> Result<u64, String> {
    let structure = store.join("indexes/structure_targets.tsv");
    let (rows, source_counts) = structure_output_counts(&structure)?;
    let contributions = structure_contributions(index_manifest)?;
    require_structure_contributions(store, &contributions, &source_counts, None)?;
    let bytes = fs::metadata(&structure)
        .map_err(|err| format!("structure_targets_metadata_failed:{err}"))?
        .len();
    let sha256 =
        sha256_file_hex(&structure).ok_or_else(|| "structure_targets_sha256_failed".to_string())?;
    let contribution_values = contributions
        .into_iter()
        .map(|(source, (rows, source_paths))| {
            let source_identity = structure_source_identity(store, &source)?;
            let input_identity = structure_input_identity(store, &source_paths)?;
            Ok(json!({
                "source": source,
                "source_id": source_identity.source_id,
                "source_digest": source_identity.source_digest,
                "release_identity": source_identity.release_identity,
                "trust_source": source_identity.trust_source,
                "rows": rows,
                "source_paths": source_paths.into_iter().collect::<Vec<_>>(),
                "input_bytes": input_identity.bytes,
                "input_digest": input_identity.digest,
            }))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let value = json!({
        "schema_version": STRUCTURE_TARGETS_MANIFEST_SCHEMA_VERSION,
        "status": if rows > 0 { "ready" } else { "empty" },
        "path": "indexes/structure_targets.tsv",
        "bytes": bytes,
        "sha256": sha256,
        "rows": rows,
        "contributions": contribution_values,
    });
    let output = store.join(STRUCTURE_TARGETS_MANIFEST_REL_PATH);
    let pending = temporary_data_path(
        output.parent().unwrap_or(store),
        "structure_targets_manifest",
        "json",
    );
    write_text(
        &pending,
        &(serde_json::to_string_pretty(&value)
            .map_err(|err| format!("structure_targets_manifest_serialize_failed:{err}"))?
            + "\n"),
    )?;
    publish_store_row_index(&pending, &output)?;
    Ok(rows)
}

fn validate_structure_targets_publication(store: &Path) -> Result<u64, String> {
    let structure = store.join("indexes/structure_targets.tsv");
    let manifest = read_json_value_if_file(&store.join(STRUCTURE_TARGETS_MANIFEST_REL_PATH))
        .ok_or_else(|| "structure_targets_manifest_invalid".to_string())?;
    let rows = manifest
        .get("rows")
        .and_then(Value::as_u64)
        .filter(|rows| *rows > 0)
        .ok_or_else(|| "structure_targets_rows_invalid".to_string())?;
    let bytes = fs::metadata(&structure)
        .map_err(|err| format!("structure_targets_metadata_failed:{err}"))?
        .len();
    let digest = manifest
        .get("sha256")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let (actual_rows, source_counts) = structure_output_counts(&structure)?;
    if manifest.get("schema_version").and_then(Value::as_str)
        != Some(STRUCTURE_TARGETS_MANIFEST_SCHEMA_VERSION)
        || manifest.get("status").and_then(Value::as_str) != Some("ready")
        || manifest.get("path").and_then(Value::as_str) != Some("indexes/structure_targets.tsv")
        || manifest.get("bytes").and_then(Value::as_u64) != Some(bytes)
        || !valid_sha256_hex(digest)
        || sha256_file_hex(&structure).as_deref() != Some(digest)
        || actual_rows != rows
    {
        return Err("structure_targets_publication_mismatch".to_string());
    }
    let mut contributions = BTreeMap::new();
    let mut verified_trust_sources = BTreeMap::new();
    for contribution in manifest
        .get("contributions")
        .and_then(Value::as_array)
        .ok_or_else(|| "structure_targets_contributions_invalid".to_string())?
    {
        let source = contribution
            .get("source")
            .and_then(Value::as_str)
            .filter(|source| matches!(*source, "PLINDER" | "BioLiP2" | "Binding MOAD" | "RCSB"))
            .ok_or_else(|| "structure_targets_contribution_source_invalid".to_string())?;
        let contribution_rows = contribution
            .get("rows")
            .and_then(Value::as_u64)
            .filter(|rows| *rows > 0)
            .ok_or_else(|| format!("structure_targets_contribution_rows_invalid:{source}"))?;
        let paths = contribution
            .get("source_paths")
            .and_then(Value::as_array)
            .and_then(|paths| {
                paths
                    .iter()
                    .map(|path| path.as_str().map(ToOwned::to_owned))
                    .collect::<Option<BTreeSet<_>>>()
            })
            .filter(|paths| !paths.is_empty())
            .ok_or_else(|| format!("structure_targets_contribution_paths_invalid:{source}"))?;
        let source_id = contribution
            .get("source_id")
            .and_then(Value::as_str)
            .filter(|value| structure_source_id(source) == Some(*value))
            .ok_or_else(|| format!("structure_targets_source_id_invalid:{source}"))?;
        let source_digest = contribution
            .get("source_digest")
            .and_then(Value::as_str)
            .filter(|digest| valid_sha256_hex(digest))
            .ok_or_else(|| format!("structure_targets_source_digest_invalid:{source}"))?;
        let release_identity = contribution
            .get("release_identity")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| format!("structure_targets_source_release_invalid:{source}"))?;
        let trust_source = contribution
            .get("trust_source")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| format!("structure_targets_source_trust_invalid:{source}"))?;
        let input_bytes = contribution
            .get("input_bytes")
            .and_then(Value::as_u64)
            .filter(|bytes| *bytes > 0)
            .ok_or_else(|| format!("structure_targets_input_bytes_invalid:{source}"))?;
        let input_digest = contribution
            .get("input_digest")
            .and_then(Value::as_str)
            .filter(|digest| valid_sha256_hex(digest))
            .ok_or_else(|| format!("structure_targets_input_digest_invalid:{source}"))?;
        let retained_rcsb_identity = source == "RCSB"
            && retained_rcsb_structure_identity_matches(
                store,
                source_digest,
                release_identity,
                trust_source,
                input_bytes,
                input_digest,
            );
        let source_identity_matches = if retained_rcsb_identity {
            true
        } else {
            let current = structure_source_identity(store, source)?;
            source_id == current.source_id
                && source_digest.eq_ignore_ascii_case(&current.source_digest)
                && release_identity == current.release_identity
                && trust_source == current.trust_source
        };
        if !source_identity_matches {
            return Err(format!(
                "structure_targets_source_identity_mismatch:{source}"
            ));
        }
        verified_trust_sources.insert(source.to_string(), trust_source.to_string());
        let existing_paths = paths
            .iter()
            .filter(|relative| store.join(relative.replace('\\', "/")).is_file())
            .count();
        if existing_paths > 0 && !retained_rcsb_identity {
            if existing_paths != paths.len() {
                return Err(format!("structure_targets_input_set_partial:{source}"));
            }
            let actual = structure_input_identity(store, &paths)?;
            if actual.bytes != input_bytes || actual.digest != input_digest {
                return Err(format!(
                    "structure_targets_input_identity_mismatch:{source}"
                ));
            }
        }
        if contributions
            .insert(source.to_string(), (contribution_rows, paths))
            .is_some()
        {
            return Err(format!("structure_targets_contribution_duplicate:{source}"));
        }
    }
    if structure_contributions(&store.join("index_build_manifest.tsv"))? != contributions {
        return Err("structure_targets_contributions_mismatch".to_string());
    }
    require_structure_contributions(
        store,
        &contributions,
        &source_counts,
        Some(&verified_trust_sources),
    )?;
    Ok(rows)
}

fn published_store_index_identity(
    output: &Path,
    manifest_path: &Path,
    index_name: &str,
) -> Result<Option<usize>, String> {
    if !published_store_index_has_rows(output)? {
        return Ok(None);
    }
    let rows = read_simple_tsv_rows(manifest_path)?;
    let mut matches = rows.iter().filter(|row| {
        row.get("index").map(String::as_str) == Some(index_name)
            && row.get("status").map(String::as_str) == Some("publication_ready")
            && row
                .get("output_path")
                .map(|path| path.replace('\\', "/"))
                .as_deref()
                == Some(index_output_for(index_name))
    });
    let row = matches
        .next()
        .ok_or_else(|| format!("{index_name}_publication_identity_missing"))?;
    if matches.next().is_some() {
        return Err(format!("{index_name}_publication_identity_duplicate"));
    }
    let expected_rows = row
        .get("rows")
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|rows| *rows > 0)
        .ok_or_else(|| format!("{index_name}_publication_rows_invalid"))?;
    let notes = row.get("notes").map(String::as_str).unwrap_or_default();
    let fields = notes
        .split(';')
        .filter_map(|field| field.split_once('='))
        .collect::<BTreeMap<_, _>>();
    let expected_bytes = fields
        .get("publication_bytes")
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or_else(|| format!("{index_name}_publication_bytes_invalid"))?;
    let expected_digest = fields
        .get("publication_sha256")
        .filter(|digest| valid_sha256_hex(digest))
        .ok_or_else(|| format!("{index_name}_publication_sha256_invalid"))?;
    if fs::metadata(output)
        .map_err(|err| format!("{index_name}_publication_metadata_failed:{err}"))?
        .len()
        != expected_bytes
        || sha256_file_hex(output).as_deref() != Some(*expected_digest)
        || store_row_count(output, index_name)? != expected_rows as u64
    {
        return Err(format!("{index_name}_publication_identity_mismatch"));
    }
    Ok(Some(expected_rows))
}

fn append_store_index_publication_identity(
    store: &Path,
    index_name: &str,
    output: &Path,
    rows: usize,
    manifest: &mut String,
) -> Result<(), String> {
    if rows == 0 {
        return Ok(());
    }
    let bytes = fs::metadata(output)
        .map_err(|err| format!("{index_name}_publication_metadata_failed:{err}"))?
        .len();
    let digest =
        sha256_file_hex(output).ok_or_else(|| format!("{index_name}_publication_sha256_failed"))?;
    manifest.push_str(&format!(
        "{index_name}\t{}\tpublication_ready\t{rows}\t{}\tpublication_bytes={bytes};publication_sha256={digest}\n",
        relative_display(store, output),
        index_output_for(index_name),
    ));
    Ok(())
}

fn append_published_manifest_rows(
    manifest_path: &Path,
    index_name: &str,
    manifest: &mut String,
) -> Result<bool, String> {
    if !manifest_path.is_file() {
        return Ok(false);
    }
    let text = read_text(manifest_path)?;
    let mut appended = false;
    for line in text.lines().skip(1) {
        let cols = split_tsv(line);
        if cols.first().map(String::as_str) == Some(index_name)
            || (index_name == "known_targets"
                && cols.first().map(String::as_str) == Some("ligand_similarity"))
        {
            manifest.push_str(line);
            manifest.push('\n');
            appended = true;
        }
    }
    Ok(appended)
}

impl Drop for StoreRowSpool {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn dedupe_store_row_spool(raw_path: &Path, output_path: &Path) -> Result<usize, String> {
    let parent = output_path
        .parent()
        .ok_or_else(|| format!("index_output_parent_missing:{}", output_path.display()))?;
    ensure_dir(parent)?;
    let pending_path = temporary_data_path(parent, "deduped_store_rows", "tsv");
    let database_path = temporary_data_path(parent, "store_row_dedupe", "sqlite");
    let result = run_python_script(
        store_row_dedupe_script(),
        &[
            raw_path.display().to_string(),
            pending_path.display().to_string(),
            database_path.display().to_string(),
        ],
        "store_row_dedupe",
    );
    let _ = fs::remove_file(&database_path);
    let output = match result {
        Ok(output) => output,
        Err(err) => {
            let _ = fs::remove_file(&pending_path);
            return Err(err);
        }
    };
    let Some(row_count) = output
        .lines()
        .find_map(|line| line.strip_prefix("rows="))
        .and_then(|value| value.trim().parse::<usize>().ok())
    else {
        let _ = fs::remove_file(&pending_path);
        return Err("store_row_dedupe_missing_row_count".to_string());
    };
    publish_store_row_index(&pending_path, output_path)?;
    Ok(row_count)
}

fn publish_store_row_index(pending: &Path, output: &Path) -> Result<(), String> {
    if !output.exists() {
        return fs::rename(pending, output).map_err(|err| {
            format!(
                "rename_failed:{}:{}:{err}",
                pending.display(),
                output.display()
            )
        });
    }
    let parent = output
        .parent()
        .ok_or_else(|| format!("index_output_parent_missing:{}", output.display()))?;
    let backup = temporary_data_path(parent, "published_store_rows_backup", "tsv");
    fs::rename(output, &backup)
        .map_err(|err| format!("published_index_backup_failed:{}:{err}", output.display()))?;
    if let Err(err) = fs::rename(pending, output) {
        let rollback = fs::rename(&backup, output);
        let _ = fs::remove_file(pending);
        return match rollback {
            Ok(()) => Err(format!(
                "published_index_replace_failed:{}:{}:{err}",
                pending.display(),
                output.display()
            )),
            Err(rollback_err) => Err(format!(
                "published_index_replace_and_rollback_failed:{}:{}:{err}:{rollback_err}",
                pending.display(),
                output.display()
            )),
        };
    }
    fs::remove_file(&backup).map_err(|err| {
        format!(
            "published_index_backup_cleanup_failed:{}:{err}",
            backup.display()
        )
    })
}

fn publish_text_file(path: &Path, text: &str) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("publish_text_parent_missing:{}", path.display()))?;
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("tmp");
    let pending = temporary_data_path(parent, "published_text", extension);
    if let Err(err) = write_text(&pending, text) {
        let _ = fs::remove_file(&pending);
        return Err(err);
    }
    publish_store_row_index(&pending, path)
}

fn structure_publication_bundle_paths(store: &Path) -> [PathBuf; 5] {
    [
        store.join("indexes/known_targets.tsv"),
        store.join("indexes/metagenome_targets.tsv"),
        store.join("indexes/structure_targets.tsv"),
        store.join("index_build_manifest.tsv"),
        store.join(STRUCTURE_TARGETS_MANIFEST_REL_PATH),
    ]
}

fn write_structure_publication_transaction(
    store: &Path,
    backups: &[PublicationBackup],
) -> Result<(), String> {
    let entries = backups
        .iter()
        .map(|entry| {
            json!({
                "original": entry.original.display().to_string(),
                "pending": entry.pending.display().to_string(),
                "backup": entry.backup.display().to_string(),
                "bytes": entry.bytes,
                "sha256": entry.sha256,
            })
        })
        .collect::<Vec<_>>();
    let value = json!({
        "schema_version": STRUCTURE_PUBLICATION_TRANSACTION_SCHEMA_VERSION,
        "phase": "publishing",
        "entries": entries,
    });
    publish_text_file(
        &store.join(STRUCTURE_PUBLICATION_TRANSACTION_REL_PATH),
        &(serde_json::to_string_pretty(&value)
            .map_err(|err| format!("structure_publication_transaction_serialize_failed:{err}"))?
            + "\n"),
    )
}

#[derive(Debug)]
struct PublicationBackup {
    original: PathBuf,
    pending: PathBuf,
    backup: PathBuf,
    bytes: u64,
    sha256: String,
}

fn publication_backup_matches(entry: &PublicationBackup) -> bool {
    fs::metadata(&entry.backup).is_ok_and(|metadata| {
        metadata.is_file()
            && metadata.len() == entry.bytes
            && sha256_file_hex(&entry.backup).as_deref() == Some(entry.sha256.as_str())
    })
}

fn copy_publication_backup(entry: &PublicationBackup) -> Result<(), String> {
    let result = (|| {
        fs::copy(&entry.original, &entry.pending).map_err(|err| {
            format!(
                "publication_bundle_backup_copy_failed:{}:{}:{err}",
                entry.original.display(),
                entry.pending.display()
            )
        })?;
        OpenOptions::new()
            .read(true)
            .write(true)
            .open(&entry.pending)
            .and_then(|file| file.sync_all())
            .map_err(|err| {
                format!(
                    "publication_bundle_backup_sync_failed:{}:{err}",
                    entry.pending.display()
                )
            })?;
        let pending_matches = fs::metadata(&entry.pending).is_ok_and(|metadata| {
            metadata.is_file()
                && metadata.len() == entry.bytes
                && sha256_file_hex(&entry.pending).as_deref() == Some(entry.sha256.as_str())
        });
        if !pending_matches {
            return Err(format!(
                "publication_bundle_backup_identity_mismatch:{}",
                entry.original.display()
            ));
        }
        fs::rename(&entry.pending, &entry.backup).map_err(|err| {
            format!(
                "publication_bundle_backup_publish_failed:{}:{}:{err}",
                entry.pending.display(),
                entry.backup.display()
            )
        })?;
        if !publication_backup_matches(entry) {
            return Err(format!(
                "publication_bundle_backup_identity_mismatch:{}",
                entry.backup.display()
            ));
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&entry.pending);
    }
    result
}

fn backup_publication_bundle(
    store: &Path,
    paths: &[&Path],
) -> Result<Vec<PublicationBackup>, String> {
    let mut backups = Vec::<PublicationBackup>::new();
    for path in paths {
        if !path.exists() {
            continue;
        }
        if !path.is_file() {
            return Err(format!(
                "publication_bundle_file_required:{}",
                path.display()
            ));
        }
        let parent = path
            .parent()
            .ok_or_else(|| format!("publication_bundle_parent_missing:{}", path.display()))?;
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("tmp");
        let metadata = fs::metadata(path).map_err(|err| {
            format!(
                "publication_bundle_backup_metadata_failed:{}:{err}",
                path.display()
            )
        })?;
        let sha256 = sha256_file_hex(path)
            .ok_or_else(|| format!("publication_bundle_backup_sha256_failed:{}", path.display()))?;
        let pending = temporary_data_path(parent, "publication_bundle_backup_pending", extension);
        let backup = temporary_data_path(parent, "publication_bundle_backup", extension);
        backups.push(PublicationBackup {
            original: path.to_path_buf(),
            pending,
            backup,
            bytes: metadata.len(),
            sha256,
        });
    }
    write_structure_publication_transaction(store, &backups)?;
    for entry in &backups {
        if let Err(err) = copy_publication_backup(entry) {
            let cleanup = discard_publication_backups(store, &backups);
            return match cleanup {
                Ok(()) => Err(err),
                Err(cleanup_err) => Err(format!(
                    "{err};publication_bundle_backup_cleanup_failed:{cleanup_err}"
                )),
            };
        }
    }
    Ok(backups)
}

fn restore_publication_bundle(
    store: &Path,
    paths: &[&Path],
    backups: &[PublicationBackup],
) -> Result<(), String> {
    if let Some(entry) = backups
        .iter()
        .find(|entry| !publication_backup_matches(entry))
    {
        return Err(format!(
            "publication_bundle_backup_identity_invalid:{}",
            entry.backup.display()
        ));
    }
    for entry in backups {
        let parent = entry.original.parent().ok_or_else(|| {
            format!(
                "publication_bundle_rollback_parent_missing:{}",
                entry.original.display()
            )
        })?;
        let extension = entry
            .original
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("tmp");
        let pending = temporary_data_path(parent, "publication_bundle_restore", extension);
        let restore_result = (|| {
            fs::copy(&entry.backup, &pending).map_err(|err| {
                format!(
                    "publication_bundle_rollback_copy_failed:{}:{}:{err}",
                    entry.backup.display(),
                    pending.display()
                )
            })?;
            OpenOptions::new()
                .read(true)
                .write(true)
                .open(&pending)
                .and_then(|file| file.sync_all())
                .map_err(|err| {
                    format!(
                        "publication_bundle_rollback_sync_failed:{}:{err}",
                        pending.display()
                    )
                })?;
            let pending_matches = fs::metadata(&pending).is_ok_and(|metadata| {
                metadata.is_file()
                    && metadata.len() == entry.bytes
                    && sha256_file_hex(&pending).as_deref() == Some(entry.sha256.as_str())
            });
            if !pending_matches {
                return Err(format!(
                    "publication_bundle_rollback_identity_mismatch:{}",
                    pending.display()
                ));
            }
            publish_store_row_index(&pending, &entry.original)?;
            Ok(())
        })();
        if let Err(err) = restore_result {
            let _ = fs::remove_file(&pending);
            return Err(err);
        }
    }
    for path in paths {
        if backups
            .iter()
            .any(|entry| entry.original.as_path() == *path)
        {
            continue;
        }
        if path.exists() {
            fs::remove_file(path).map_err(|err| {
                format!(
                    "publication_bundle_rollback_remove_failed:{}:{err}",
                    path.display()
                )
            })?;
        }
    }
    discard_publication_backups(store, backups)
}

fn discard_publication_backups(store: &Path, backups: &[PublicationBackup]) -> Result<(), String> {
    let transaction = store.join(STRUCTURE_PUBLICATION_TRANSACTION_REL_PATH);
    if transaction.exists() {
        fs::remove_file(&transaction).map_err(|err| {
            format!(
                "publication_bundle_transaction_cleanup_failed:{}:{err}",
                transaction.display()
            )
        })?;
    }
    for entry in backups {
        for path in [&entry.pending, &entry.backup] {
            if !path.exists() {
                continue;
            }
            fs::remove_file(path).map_err(|err| {
                format!(
                    "publication_bundle_backup_cleanup_failed:{}:{err}",
                    path.display()
                )
            })?;
        }
    }
    Ok(())
}

struct PublicationBundleTransaction {
    store: PathBuf,
    paths: Vec<PathBuf>,
    backups: Vec<PublicationBackup>,
    committed: bool,
}

impl PublicationBundleTransaction {
    fn begin(store: &Path, paths: &[&Path]) -> Result<Self, String> {
        Ok(Self {
            store: store.to_path_buf(),
            paths: paths.iter().map(|path| (*path).to_path_buf()).collect(),
            backups: backup_publication_bundle(store, paths)?,
            committed: false,
        })
    }

    fn commit(&mut self) -> Result<(), String> {
        self.committed = true;
        discard_publication_backups(&self.store, &self.backups)
    }
}

impl Drop for PublicationBundleTransaction {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        let paths = self.paths.iter().map(PathBuf::as_path).collect::<Vec<_>>();
        let _ = restore_publication_bundle(&self.store, &paths, &self.backups);
    }
}

fn reconcile_structure_publication_transaction(store: &Path) -> Result<(), String> {
    let transaction_path = store.join(STRUCTURE_PUBLICATION_TRANSACTION_REL_PATH);
    if !transaction_path.is_file() {
        return Ok(());
    }
    let value = read_json_value_if_file(&transaction_path)
        .ok_or_else(|| "structure_publication_transaction_invalid".to_string())?;
    if value.get("schema_version").and_then(Value::as_str)
        != Some(STRUCTURE_PUBLICATION_TRANSACTION_SCHEMA_VERSION)
        || value.get("phase").and_then(Value::as_str) != Some("publishing")
    {
        return Err("structure_publication_transaction_schema_invalid".to_string());
    }
    let bundle = structure_publication_bundle_paths(store);
    let allowed = bundle.iter().cloned().collect::<BTreeSet<_>>();
    let mut backups = Vec::new();
    for entry in value
        .get("entries")
        .and_then(Value::as_array)
        .ok_or_else(|| "structure_publication_transaction_entries_invalid".to_string())?
    {
        let original = PathBuf::from(
            entry
                .get("original")
                .and_then(Value::as_str)
                .ok_or_else(|| "structure_publication_transaction_original_invalid".to_string())?,
        );
        let pending = PathBuf::from(
            entry
                .get("pending")
                .and_then(Value::as_str)
                .ok_or_else(|| "structure_publication_transaction_pending_invalid".to_string())?,
        );
        let backup = PathBuf::from(
            entry
                .get("backup")
                .and_then(Value::as_str)
                .ok_or_else(|| "structure_publication_transaction_backup_invalid".to_string())?,
        );
        let bytes = entry
            .get("bytes")
            .and_then(Value::as_u64)
            .ok_or_else(|| "structure_publication_transaction_bytes_invalid".to_string())?;
        let sha256 = entry
            .get("sha256")
            .and_then(Value::as_str)
            .filter(|value| valid_sha256_hex(value))
            .ok_or_else(|| "structure_publication_transaction_sha256_invalid".to_string())?
            .to_string();
        let pending_name = pending
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        let backup_name = backup
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        if !allowed.contains(&original)
            || pending.parent() != original.parent()
            || backup.parent() != original.parent()
            || !pending_name.contains("publication_bundle_backup_pending")
            || !backup_name.contains("publication_bundle_backup")
            || backup_name.contains("publication_bundle_backup_pending")
            || pending == backup
            || backups
                .iter()
                .any(|known: &PublicationBackup| known.original == original)
        {
            return Err("structure_publication_transaction_path_invalid".to_string());
        }
        backups.push(PublicationBackup {
            original,
            pending,
            backup,
            bytes,
            sha256,
        });
    }
    let known = store.join("indexes/known_targets.tsv");
    let metagenome = store.join("indexes/metagenome_targets.tsv");
    let build_manifest = store.join("index_build_manifest.tsv");
    let complete_bundle = bundle.iter().all(|path| path.is_file());
    let compatibility_indexes_ready = matches!(
        published_store_index_identity(&known, &build_manifest, "known_targets"),
        Ok(Some(_))
    ) && matches!(
        published_store_index_identity(&metagenome, &build_manifest, "metagenome_targets"),
        Ok(Some(_))
    );
    if complete_bundle
        && compatibility_indexes_ready
        && validate_structure_targets_publication(store).is_ok()
    {
        return discard_publication_backups(store, &backups);
    }
    let refs = bundle.iter().map(PathBuf::as_path).collect::<Vec<_>>();
    restore_publication_bundle(store, &refs, &backups)
}

fn store_row_dedupe_script() -> &'static str {
    r#"
import csv
import sqlite3
import sys

raw_path, output_path, database_path = sys.argv[1:4]

def merge_tokens(left, right):
    return " ".join(sorted(set(str(left or "").split()) | set(str(right or "").split())))

def merge_evidence(left, right):
    values = []
    seen = set()
    for value in (str(left or "") + ";" + str(right or "")).split(";"):
        value = value.strip()
        if value and value not in seen:
            seen.add(value)
            values.append(value)
    return ";".join(values)

connection = sqlite3.connect(database_path)
try:
    connection.execute("PRAGMA journal_mode=OFF")
    connection.execute("PRAGMA synchronous=OFF")
    connection.execute("PRAGMA temp_store=FILE")
    connection.create_function("merge_tokens", 2, merge_tokens)
    connection.create_function("merge_evidence", 2, merge_evidence)
    connection.execute("""
        CREATE TABLE rows (
            source TEXT NOT NULL,
            target_id TEXT NOT NULL,
            target_name TEXT NOT NULL,
            organism TEXT NOT NULL,
            target_class TEXT NOT NULL,
            ligand_tokens TEXT NOT NULL,
            sequence_tokens TEXT NOT NULL,
            structure_tokens TEXT NOT NULL,
            evidence TEXT NOT NULL,
            PRIMARY KEY (source, target_id)
        )
    """)
    upsert = """
        INSERT INTO rows VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(source, target_id) DO UPDATE SET
            target_name=CASE WHEN rows.target_name = 'unknown' AND excluded.target_name != 'unknown' THEN excluded.target_name ELSE rows.target_name END,
            organism=CASE WHEN rows.organism = 'unknown' AND excluded.organism != 'unknown' THEN excluded.organism ELSE rows.organism END,
            target_class=CASE WHEN rows.target_class != 'known_human_protein' AND excluded.target_class = 'known_human_protein' THEN excluded.target_class ELSE rows.target_class END,
            ligand_tokens=merge_tokens(rows.ligand_tokens, excluded.ligand_tokens),
            sequence_tokens=merge_tokens(rows.sequence_tokens, excluded.sequence_tokens),
            structure_tokens=merge_tokens(rows.structure_tokens, excluded.structure_tokens),
            evidence=merge_evidence(rows.evidence, excluded.evidence)
    """
    batch = []
    with open(raw_path, "r", encoding="utf-8-sig", newline="") as handle:
        reader = csv.DictReader(handle, delimiter="\t")
        for row in reader:
            batch.append((
                row.get("source", ""),
                row.get("target_id", ""),
                row.get("target_name", ""),
                row.get("organism", ""),
                row.get("target_class", ""),
                row.get("ligand_tokens", ""),
                row.get("sequence_tokens", ""),
                row.get("structure_tokens", ""),
                row.get("evidence", ""),
            ))
            if len(batch) >= 1000:
                connection.executemany(upsert, batch)
                connection.commit()
                batch.clear()
    if batch:
        connection.executemany(upsert, batch)
        connection.commit()
    with open(output_path, "w", encoding="utf-8", newline="") as handle:
        writer = csv.writer(handle, delimiter="\t", lineterminator="\n")
        writer.writerow([
            "source", "target_id", "target_name", "organism", "target_class",
            "ligand_tokens", "sequence_tokens", "structure_tokens", "evidence",
        ])
        for row in connection.execute("""
            SELECT source, target_id, target_name, organism, target_class,
                   ligand_tokens, sequence_tokens, structure_tokens, evidence
            FROM rows ORDER BY source, target_id
        """):
            writer.writerow(row)
    print(f"rows={connection.execute('SELECT COUNT(*) FROM rows').fetchone()[0]}")
finally:
    connection.close()
"#
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
    let indexes_dir = options.science_store.join("indexes");
    let ligand_similarity_ready = validate_ligand_similarity_runtime_contract(options).is_ok();
    let known_output = indexes_dir.join("known_targets.tsv");
    let metagenome_output = indexes_dir.join("metagenome_targets.tsv");
    let structure_output = indexes_dir.join("structure_targets.tsv");
    let known_inputs_materialized = known_sources
        .iter()
        .any(|relative| path_has_any_file(&options.science_store.join(relative)))
        || discover_sequence_files(&options.science_store.join("downloads/uniprot"))
            .is_ok_and(|paths| !paths.is_empty());
    let metagenome_inputs_materialized = metagenome_sources
        .iter()
        .any(|relative| path_has_any_file(&options.science_store.join(relative)))
        || discover_sequence_files(&options.science_store.join("downloads/metagenome"))
            .is_ok_and(|paths| !paths.is_empty());
    let known_published = if known_inputs_materialized {
        None
    } else {
        published_store_index_identity(&known_output, manifest_path, "known_targets")?
    };
    let metagenome_published = if metagenome_inputs_materialized {
        None
    } else {
        published_store_index_identity(&metagenome_output, manifest_path, "metagenome_targets")?
    };
    let structure_published = validate_structure_targets_publication(&options.science_store)
        .ok()
        .and_then(|rows| usize::try_from(rows).ok());
    if published_store_index_has_rows(&structure_output)?
        && structure_published.is_none()
        && !structure_rebuild_authority_available(&options.science_store)
    {
        return Err("structure_targets_publication_invalid_without_rebuild_authority".to_string());
    }
    let mut known_rows = StoreRowSpool::create(&indexes_dir, "known_targets_raw")?;
    if known_published.is_some() {
        if !append_published_manifest_rows(manifest_path, "known_targets", &mut manifest)? {
            manifest.push_str(
                "known_targets\tindexes/known_targets.tsv\tpreserved\t1\tindexes/known_targets.tsv\tpublished_nonempty_index_preserved\n",
            );
        }
    } else {
        for rel in known_sources {
            let path = options.science_store.join(rel);
            if ligand_similarity_ready && rel.starts_with("downloads/bindingdb/bindingdb_all") {
                manifest.push_str(&format!(
                "known_targets\t{}\tsuperseded\t0\t{}\tcompact_ligand_similarity_is_authoritative\n",
                relative_display(&options.science_store, &path),
                LIGAND_SIMILARITY_DATABASE_REL_PATH,
            ));
                continue;
            }
            append_canonical_rows(
                "known_targets",
                &options.science_store,
                &path,
                &mut known_rows,
                &mut manifest,
            )?;
        }
        let chembl_dir = options.science_store.join("downloads/chembl");
        let published_chembl_query = chembl_dir.join("chembl_query.sqlite");
        if ligand_similarity_ready {
            let ligand_manifest = options
                .science_store
                .join(LIGAND_SIMILARITY_MANIFEST_REL_PATH);
            let target_count = read_text(&ligand_manifest)
                .ok()
                .and_then(|text| {
                    serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}')).ok()
                })
                .and_then(|value| {
                    value
                        .get("counts")
                        .and_then(|counts| counts.get("targets"))
                        .and_then(Value::as_u64)
                })
                .unwrap_or(0);
            manifest.push_str(&format!(
            "ligand_similarity\t{}\tready\t{target_count}\t{}\tcompact_ligand_similarity_used_by_onsite_matcher\n",
            LIGAND_SIMILARITY_MANIFEST_REL_PATH,
            LIGAND_SIMILARITY_DATABASE_REL_PATH,
        ));
        } else {
            for path in discover_chembl_sqlite_sources(&chembl_dir)? {
                if path == published_chembl_query {
                    continue;
                }
                append_canonical_rows(
                    "known_targets",
                    &options.science_store,
                    &path,
                    &mut known_rows,
                    &mut manifest,
                )?;
            }
        }
        let uniprot_dir = options.science_store.join("downloads/uniprot");
        for path in discover_sequence_files(&uniprot_dir)? {
            append_fasta_source_rows(
                "known_targets",
                &options.science_store,
                &path,
                "UniProt",
                "Homo sapiens",
                "known_human_protein",
                "protein_sequence_record",
                &mut known_rows,
                &mut manifest,
                "fasta_sequence_rows",
            )?;
        }
    }

    let mut metagenome_rows = StoreRowSpool::create(&indexes_dir, "metagenome_targets_raw")?;
    if metagenome_published.is_some() {
        if !append_published_manifest_rows(manifest_path, "metagenome_targets", &mut manifest)? {
            manifest.push_str(
                "metagenome_targets\tindexes/metagenome_targets.tsv\tpreserved\t1\tindexes/metagenome_targets.tsv\tpublished_nonempty_index_preserved\n",
            );
        }
    } else {
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
                if !append_ready_metagenome_alignment_database(
                    &options.science_store,
                    &path,
                    &mut manifest,
                )? {
                    append_alignment_database_required(
                        "metagenome_targets",
                        &options.science_store,
                        &path,
                        &mut manifest,
                    );
                }
                continue;
            }
            append_fasta_source_rows(
                "metagenome_targets",
                &options.science_store,
                &path,
                "metagenome",
                "metagenome_or_custom_source",
                "potential_metagenome_sequence",
                "sequence_record",
                &mut metagenome_rows,
                &mut manifest,
                "fasta_sequence_rows",
            )?;
        }
    }

    if let (Some(known_rows), Some(metagenome_rows), Some(structure_rows)) =
        (known_published, metagenome_published, structure_published)
    {
        return Ok(IndexBuildSummary {
            known_rows,
            metagenome_rows,
            structure_rows,
        });
    }
    let bundle = structure_publication_bundle_paths(&options.science_store);
    let bundle_paths = bundle.iter().map(PathBuf::as_path).collect::<Vec<_>>();
    let mut publication_transaction =
        PublicationBundleTransaction::begin(&options.science_store, &bundle_paths)?;
    let (known_count, known_preserved) =
        known_rows.finish_preserving_published(&known_output, known_published)?;
    let (metagenome_count, metagenome_preserved) =
        metagenome_rows.finish_preserving_published(&metagenome_output, metagenome_published)?;
    if !known_preserved {
        append_store_index_publication_identity(
            &options.science_store,
            "known_targets",
            &known_output,
            known_count,
            &mut manifest,
        )?;
    }
    if !metagenome_preserved {
        append_store_index_publication_identity(
            &options.science_store,
            "metagenome_targets",
            &metagenome_output,
            metagenome_count,
            &mut manifest,
        )?;
    }
    let mut structure_rows = StoreRowSpool::create(&indexes_dir, "structure_targets_raw")?;
    if structure_published.is_some() {
        if !append_published_manifest_rows(manifest_path, "structure_targets", &mut manifest)? {
            manifest.push_str(
                "structure_targets\tindexes/structure_targets.tsv\tpreserved\t1\tindexes/structure_targets.tsv\tpublished_nonempty_index_preserved\n",
            );
        }
    } else {
        build_structure_index_rows(options, &mut manifest, &mut structure_rows)?;
    }
    let (structure_count, _structure_preserved) = if structure_published.is_some() {
        let result =
            structure_rows.finish_preserving_published(&structure_output, structure_published)?;
        publish_text_file(manifest_path, &manifest)?;
        validate_structure_targets_publication(&options.science_store)?;
        result
    } else {
        let result = structure_rows.finish_preserving_published(&structure_output, None)?;
        publish_text_file(manifest_path, &manifest)?;
        let published_rows =
            write_structure_targets_manifest(&options.science_store, manifest_path)?;
        if published_rows != result.0 as u64 {
            return Err("structure_targets_published_row_count_mismatch".to_string());
        }
        if published_rows > 0 {
            validate_structure_targets_publication(&options.science_store)?;
        }
        result
    };
    publication_transaction.commit()?;
    Ok(IndexBuildSummary {
        known_rows: known_count,
        metagenome_rows: metagenome_count,
        structure_rows: structure_count,
    })
}

fn append_canonical_rows(
    index_name: &str,
    root: &Path,
    path: &Path,
    rows: &mut StoreRowSpool,
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
    let (count, notes) = for_each_source_row_batch(path, index_name, |batch| rows.append(batch))?;
    manifest.push_str(&format!(
        "{index_name}\t{}\t{}\t{count}\t{}\t{notes};complete_source_scan\n",
        relative_display(root, path),
        if count > 0 { "used" } else { "empty" },
        index_output_for(index_name)
    ));
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn append_fasta_source_rows(
    index_name: &str,
    root: &Path,
    path: &Path,
    source: &str,
    default_organism: &str,
    target_class: &str,
    evidence: &str,
    rows: &mut StoreRowSpool,
    manifest: &mut String,
    notes: &str,
) -> Result<(), String> {
    let count = for_each_fasta_row_batch(
        path,
        source,
        default_organism,
        target_class,
        evidence,
        |batch| rows.append(batch),
    )?;
    manifest.push_str(&format!(
        "{index_name}\t{}\t{}\t{count}\t{}\t{notes};complete_source_scan\n",
        relative_display(root, path),
        if count > 0 { "used" } else { "empty" },
        index_output_for(index_name)
    ));
    Ok(())
}

fn append_built_row_count(
    index_name: &str,
    root: &Path,
    path: &Path,
    total: usize,
    manifest: &mut String,
    notes: &str,
) {
    manifest.push_str(&format!(
        "{index_name}\t{}\t{}\t{total}\t{}\t{notes};complete_source_scan\n",
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

fn append_ready_metagenome_alignment_database(
    root: &Path,
    source: &Path,
    manifest: &mut String,
) -> Result<bool, String> {
    let database_manifest_rel = "indexes/mmseqs/mmseqs_target_databases.tsv";
    let database_manifest = root.join(database_manifest_rel);
    if !database_manifest.is_file() {
        return Ok(false);
    }
    let text = read_text(&database_manifest)?;
    let mut lines = text.lines();
    let Some(header) = lines.next() else {
        return Ok(false);
    };
    let headers = split_tsv(header);
    let required = |name: &str| find_header(&headers, &[name]);
    let Some(schema_idx) = required("manifest_schema_version") else {
        return Ok(false);
    };
    let Some(namespace_idx) = required("source_namespace") else {
        return Ok(false);
    };
    let Some(class_idx) = required("source_class") else {
        return Ok(false);
    };
    let Some(source_idx) = required("source_path") else {
        return Ok(false);
    };
    let Some(digest_idx) = required("source_digest") else {
        return Ok(false);
    };
    let Some(builder_idx) = required("builder_version") else {
        return Ok(false);
    };
    let Some(database_idx) = required("db_path") else {
        return Ok(false);
    };
    let Some(status_idx) = required("status") else {
        return Ok(false);
    };
    let expected_source = relative_display(root, source).replace('\\', "/");
    for line in lines.filter(|line| !line.trim().is_empty()) {
        let cols = split_tsv(line);
        let source_path = cell(&cols, Some(source_idx)).replace('\\', "/");
        let digest = cell(&cols, Some(digest_idx)).to_ascii_lowercase();
        let database_rel = cell(&cols, Some(database_idx));
        let database = root.join(&database_rel);
        let companions = [
            database.clone(),
            PathBuf::from(format!("{}.dbtype", database.display())),
            PathBuf::from(format!("{}.index", database.display())),
            PathBuf::from(format!("{}.lookup", database.display())),
        ];
        let artifacts_ready = companions.iter().all(|path| {
            fs::metadata(path)
                .map(|metadata| metadata.is_file() && metadata.len() > 0)
                .unwrap_or(false)
        });
        if cell(&cols, Some(schema_idx)) != ALIGNMENT_DATABASE_MANIFEST_SCHEMA_VERSION
            || !cell(&cols, Some(namespace_idx)).eq_ignore_ascii_case("MGnify")
            || !cell(&cols, Some(class_idx)).eq_ignore_ascii_case("metagenome")
            || source_path != expected_source
            || digest.len() != 64
            || !digest.bytes().all(|byte| byte.is_ascii_hexdigit())
            || cell(&cols, Some(builder_idx)).is_empty()
            || cell(&cols, Some(status_idx)) != "ready"
            || database_rel.is_empty()
            || !artifacts_ready
        {
            continue;
        }
        manifest.push_str(&format!(
            "metagenome_targets\t{}\talignment_database_used\t0\t{}\tfull_searchable_alignment_database;consumer=mmseqs_alignment_worker;source_namespace=MGnify;source_digest={digest};database_manifest={database_manifest_rel}\n",
            relative_display(root, source),
            relative_display(root, &database),
        ));
        return Ok(true);
    }
    Ok(false)
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
    rows: &mut StoreRowSpool,
) -> Result<(), String> {
    for path in discover_structure_files(&options.science_store.join("downloads/rcsb/mmcif"))? {
        let pdb_id = path
            .file_name()
            .and_then(|value| value.to_str())
            .map(structure_id_from_file_name)
            .unwrap_or_default();
        if !is_pdb_id(&pdb_id) || !coordinate_file_identity_matches_pdb(&path, &pdb_id) {
            continue;
        }
        let built = structure_file_row("RCSB", &path);
        rows.append(std::slice::from_ref(&built))?;
        append_built_row_count(
            "structure_targets",
            &options.science_store,
            &path,
            1,
            manifest,
            "rcsb_structure_file_rows",
        );
    }
    for path in discover_biolip_tables(&options.science_store.join("downloads/biolip2"))? {
        let count = for_each_biolip_structure_row_batch(&path, |batch| rows.append(batch))?;
        append_built_row_count(
            "structure_targets",
            &options.science_store,
            &path,
            count,
            manifest,
            "biolip2_interaction_rows",
        );
    }
    let plinder_sources =
        discover_plinder_tables(&options.science_store.join("downloads/plinder"))?;
    let canonical = options.science_store.join("indexes/plinder_targets.tsv");
    if canonical.is_file() {
        let published_rows = validate_plinder_canonical_publication(&options.science_store)?;
        let count = for_each_store_row_batch(&canonical, "plinder_canonical_index", |batch| {
            rows.append(batch)
        })?;
        if count == 0 || count as u64 != published_rows {
            return Err(format!(
                "plinder_canonical_index_row_count_mismatch:{}",
                canonical.display()
            ));
        }
        append_built_row_count(
            "structure_targets",
            &options.science_store,
            &canonical,
            count,
            manifest,
            "plinder_canonical_rows",
        );
    } else if !plinder_sources.is_empty() {
        return Err(format!(
            "plinder_canonical_index_missing:{}",
            canonical.display()
        ));
    }
    for path in discover_moad_tables(&options.science_store.join("downloads/binding_moad"))? {
        let count = for_each_moad_structure_row_batch(&path, |batch| rows.append(batch))?;
        append_built_row_count(
            "structure_targets",
            &options.science_store,
            &path,
            count,
            manifest,
            "binding_moad_complex_rows",
        );
    }
    Ok(())
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
            || name.ends_with(".zip")
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
        name.ends_with(".csv")
            || name.ends_with(".tsv")
            || name.ends_with(".txt")
            || name.ends_with(".csv.gz")
            || name.ends_with(".tsv.gz")
            || name.ends_with(".txt.gz")
            || name.ends_with(".zip")
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

fn for_each_fasta_row_batch<F>(
    path: &Path,
    source: &str,
    default_organism: &str,
    target_class: &str,
    evidence: &str,
    mut emit: F,
) -> Result<usize, String>
where
    F: FnMut(&[StoreRow]) -> Result<(), String>,
{
    let name = lower_file_name(path);
    if name.ends_with(".gz") {
        let file =
            fs::File::open(path).map_err(|err| format!("open_failed:{}:{err}", path.display()))?;
        let mut reader = BufReader::new(GzDecoder::new(file));
        return parse_fasta_reader_batched(
            &mut reader,
            source,
            default_organism,
            target_class,
            evidence,
            &mut emit,
        )
        .map_err(|err| format!("gzip_read_failed:{}:{err}", path.display()));
    }
    if name.ends_with(".zip") {
        let mut count = 0usize;
        for_each_supported_zip_member(path, matches_sequence_name, |_, reader| {
            count += parse_fasta_reader_batched(
                reader,
                source,
                default_organism,
                target_class,
                evidence,
                &mut emit,
            )?;
            Ok(())
        })?;
        return Ok(count);
    }
    let file =
        fs::File::open(path).map_err(|err| format!("open_failed:{}:{err}", path.display()))?;
    let mut reader = BufReader::new(file);
    parse_fasta_reader_batched(
        &mut reader,
        source,
        default_organism,
        target_class,
        evidence,
        &mut emit,
    )
    .map_err(|err| format!("read_failed:{}:{err}", path.display()))
}

fn parse_fasta_reader_batched<R, F>(
    reader: &mut R,
    source: &str,
    default_organism: &str,
    target_class: &str,
    evidence: &str,
    emit: &mut F,
) -> Result<usize, String>
where
    R: BufRead + ?Sized,
    F: FnMut(&[StoreRow]) -> Result<(), String>,
{
    let context = FastaParseContext {
        source,
        default_organism,
        target_class,
        evidence,
    };
    let mut rows = Vec::with_capacity(SOURCE_ROW_BATCH_SIZE);
    let mut count = 0usize;
    let mut header = String::new();
    let mut sequence = String::new();
    for line in reader.lines() {
        let line = line.map_err(|err| format!("fasta_read_failed:{err}"))?;
        parse_fasta_line(&line, &context, &mut header, &mut sequence, &mut rows);
        count += flush_full_store_row_batch(&mut rows, emit)?;
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
    count += flush_store_row_batch(&mut rows, emit)?;
    Ok(count)
}

fn flush_full_store_row_batch<F>(rows: &mut Vec<StoreRow>, emit: &mut F) -> Result<usize, String>
where
    F: FnMut(&[StoreRow]) -> Result<(), String>,
{
    if rows.len() < SOURCE_ROW_BATCH_SIZE {
        return Ok(0);
    }
    flush_store_row_batch(rows, emit)
}

fn flush_store_row_batch<F>(rows: &mut Vec<StoreRow>, emit: &mut F) -> Result<usize, String>
where
    F: FnMut(&[StoreRow]) -> Result<(), String>,
{
    if rows.is_empty() {
        return Ok(0);
    }
    let count = rows.len();
    emit(rows)?;
    rows.clear();
    Ok(count)
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
) {
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
    } else {
        sequence.push_str(line.trim());
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
    read_structure_metadata_best_effort(path)
        .map(|text| tokenize(&text))
        .unwrap_or_default()
}

fn read_structure_metadata_best_effort(path: &Path) -> Result<String, String> {
    let name = lower_file_name(path);
    if name.ends_with(".gz") {
        let compressed = (|| {
            let file = fs::File::open(path)
                .map_err(|err| format!("open_failed:{}:{err}", path.display()))?;
            let mut reader = BufReader::new(GzDecoder::new(file));
            read_structure_metadata(&mut reader)
                .map_err(|err| format!("gzip_read_failed:{}:{err}", path.display()))
        })();
        return compressed.or_else(|_| {
            let file = fs::File::open(path)
                .map_err(|err| format!("open_failed:{}:{err}", path.display()))?;
            let mut reader = BufReader::new(file);
            read_structure_metadata(&mut reader)
                .map_err(|err| format!("read_failed:{}:{err}", path.display()))
        });
    }
    let file =
        fs::File::open(path).map_err(|err| format!("open_failed:{}:{err}", path.display()))?;
    let mut reader = BufReader::new(file);
    read_structure_metadata(&mut reader)
        .map_err(|err| format!("read_failed:{}:{err}", path.display()))
}

fn read_structure_metadata(reader: &mut dyn BufRead) -> Result<String, std::io::Error> {
    let mut out = String::new();
    let mut capture_loop = false;
    let mut line = String::new();
    while reader.read_line(&mut line)? != 0 {
        let trimmed = line.trim();
        if trimmed.starts_with("_struct.title") || trimmed.starts_with("_citation.title") {
            push_limited_metadata(&mut out, &line);
        }
        if trimmed.starts_with("_chem_comp.") || trimmed.starts_with("_pdbx_entity_nonpoly.") {
            capture_loop = true;
            push_limited_metadata(&mut out, &line);
        } else if capture_loop {
            if trimmed == "#" {
                capture_loop = false;
            } else {
                push_limited_metadata(&mut out, &line);
            }
        }
        if out.len() >= MAX_STRUCTURE_METADATA_CHARS {
            break;
        }
        line.clear();
    }
    Ok(out)
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
    if out.len() < MAX_STRUCTURE_METADATA_CHARS {
        out.push('\n');
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

fn for_each_biolip_structure_row_batch<F>(path: &Path, mut emit: F) -> Result<usize, String>
where
    F: FnMut(&[StoreRow]) -> Result<(), String>,
{
    let mut count = 0usize;
    for_each_table_reader(path, |reader| {
        let mut batch = Vec::with_capacity(SOURCE_ROW_BATCH_SIZE);
        for line in reader.lines() {
            let line = line.map_err(|err| format!("biolip2_read_failed:{err}"))?;
            let line = line.trim_start_matches('\u{feff}').trim();
            let Some(row) = biolip_structure_row(line) else {
                continue;
            };
            batch.push(row);
            if batch.len() >= SOURCE_ROW_BATCH_SIZE {
                count += batch.len();
                emit(&batch)?;
                batch.clear();
            }
        }
        if !batch.is_empty() {
            count += batch.len();
            emit(&batch)?;
        }
        Ok(())
    })?;
    Ok(count)
}

fn biolip_structure_row(line: &str) -> Option<StoreRow> {
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let cols = split_loose_columns(line);
    if cols.is_empty() || cols[0].eq_ignore_ascii_case("pdb_id") {
        return None;
    }
    let pdb = cols.first().map(|value| value.trim()).unwrap_or_default();
    if pdb.is_empty() {
        return None;
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
    Some(StoreRow {
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
    })
}

fn for_each_moad_structure_row_batch<F>(path: &Path, mut emit: F) -> Result<usize, String>
where
    F: FnMut(&[StoreRow]) -> Result<(), String>,
{
    let mut count = 0usize;
    for_each_table_reader(path, |reader| {
        count += parse_moad_structure_reader_batched(reader, &mut emit)?;
        Ok(())
    })?;
    Ok(count)
}

fn parse_moad_structure_reader_batched<F>(
    reader: &mut dyn BufRead,
    emit: &mut F,
) -> Result<usize, String>
where
    F: FnMut(&[StoreRow]) -> Result<(), String>,
{
    let mut lines = reader.lines();
    let first_line = loop {
        let Some(line) = lines.next() else {
            return Ok(0);
        };
        let line = line.map_err(|err| format!("binding_moad_read_failed:{err}"))?;
        let line = line.trim_start_matches('\u{feff}').trim();
        if !line.is_empty() && !line.starts_with('#') {
            break line.to_string();
        }
    };
    let delimiter = if first_line.contains('\t') {
        Some('\t')
    } else if first_line.contains(',') {
        Some(',')
    } else {
        None
    };
    let first_cols = split_optional_delimited(&first_line, delimiter);
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
    let mut batch = Vec::with_capacity(SOURCE_ROW_BATCH_SIZE);
    let mut count = 0usize;
    if !first_is_header
        && let Some(row) = moad_structure_row(
            &first_line,
            delimiter,
            false,
            pdb_idx,
            ligand_idx,
            affinity_idx,
        )
    {
        batch.push(row);
    }
    for line in lines {
        let line = line.map_err(|err| format!("binding_moad_read_failed:{err}"))?;
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(row) = moad_structure_row(
            line,
            delimiter,
            first_is_header,
            pdb_idx,
            ligand_idx,
            affinity_idx,
        ) {
            batch.push(row);
        }
        if batch.len() >= SOURCE_ROW_BATCH_SIZE {
            count += batch.len();
            emit(&batch)?;
            batch.clear();
        }
    }
    if !batch.is_empty() {
        count += batch.len();
        emit(&batch)?;
    }
    Ok(count)
}

fn moad_structure_row(
    line: &str,
    delimiter: Option<char>,
    has_header: bool,
    pdb_idx: Option<usize>,
    ligand_idx: Option<usize>,
    affinity_idx: Option<usize>,
) -> Option<StoreRow> {
    let cols = split_optional_delimited(line, delimiter);
    let pdb = if has_header {
        cell(&cols, pdb_idx)
    } else {
        cols.first()
            .map(|value| value.trim().to_string())
            .unwrap_or_default()
    };
    if pdb.is_empty() {
        return None;
    }
    let ligand = if has_header {
        cell(&cols, ligand_idx)
    } else {
        cols.get(1)
            .map(|value| value.trim().to_string())
            .unwrap_or_default()
    };
    let affinity = if has_header {
        cell(&cols, affinity_idx)
    } else {
        cols.get(2)
            .map(|value| value.trim().to_string())
            .unwrap_or_default()
    };
    let mut ligand_tokens = tokenize(&ligand);
    ligand_tokens.extend(tokenize(&affinity));
    Some(StoreRow {
        source: "Binding MOAD".to_string(),
        target_id: safe_id(&pdb.to_ascii_uppercase()),
        target_name: format!("Binding MOAD complex {}", pdb.to_ascii_uppercase()),
        organism: "unknown".to_string(),
        target_class: "structure_interaction_record".to_string(),
        ligand_tokens,
        sequence_tokens: tokenize(&pdb),
        structure_tokens: tokenize_limited(line, MAX_SEQUENCE_TOKEN_CHARS),
        evidence: "binding_moad_complex_row".to_string(),
    })
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

fn matches_table_name(name: &str) -> bool {
    name.ends_with(".tsv") || name.ends_with(".txt")
}

fn matches_structure_table_name(name: &str) -> bool {
    matches_table_name(name) || name.ends_with(".csv")
}

fn for_each_table_reader<F>(path: &Path, mut visit: F) -> Result<usize, String>
where
    F: FnMut(&mut dyn BufRead) -> Result<(), String>,
{
    let name = lower_file_name(path);
    if name.ends_with(".zip") {
        return for_each_supported_zip_member(path, matches_structure_table_name, |_, reader| {
            visit(reader)
        });
    }
    let file =
        fs::File::open(path).map_err(|err| format!("open_failed:{}:{err}", path.display()))?;
    if name.ends_with(".gz") {
        let mut reader = BufReader::new(GzDecoder::new(file));
        visit(&mut reader).map_err(|err| format!("gzip_read_failed:{}:{err}", path.display()))?;
    } else {
        let mut reader = BufReader::new(file);
        visit(&mut reader).map_err(|err| format!("read_failed:{}:{err}", path.display()))?;
    }
    Ok(1)
}

fn for_each_supported_zip_member<F>(
    path: &Path,
    accept: fn(&str) -> bool,
    mut visit: F,
) -> Result<usize, String>
where
    F: FnMut(&str, &mut dyn BufRead) -> Result<(), String>,
{
    let file =
        fs::File::open(path).map_err(|err| format!("open_failed:{}:{err}", path.display()))?;
    let mut archive =
        ZipArchive::new(file).map_err(|err| format!("zip_open_failed:{}:{err}", path.display()))?;
    let mut members = archive
        .file_names()
        .enumerate()
        .filter_map(|(index, name)| {
            let lower = name.to_ascii_lowercase();
            (!name.ends_with('/') && accept(&lower)).then(|| (lower, name.to_string(), index))
        })
        .collect::<Vec<_>>();
    members.sort();
    if members.is_empty() {
        return Err(format!("zip_no_supported_text_member:{}", path.display()));
    }
    let count = members.len();
    for (_, name, index) in members {
        let member = archive
            .by_index(index)
            .map_err(|err| format!("zip_member_failed:{}:{err}", path.display()))?;
        let mut reader = BufReader::new(member);
        visit(&name, &mut reader)
            .map_err(|err| format!("zip_read_failed:{}:{name}:{err}", path.display()))?;
    }
    Ok(count)
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

fn write_store_rows(path: &Path, rows: &[StoreRow]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        ensure_dir(parent)?;
    }
    let file =
        fs::File::create(path).map_err(|err| format!("create_failed:{}:{err}", path.display()))?;
    let mut writer = BufWriter::new(file);
    writer
        .write_all(store_row_header().as_bytes())
        .map_err(|err| format!("write_failed:{}:{err}", path.display()))?;
    for row in rows {
        write_store_row(&mut writer, row)
            .map_err(|err| format!("write_failed:{}:{err}", path.display()))?;
    }
    writer
        .flush()
        .map_err(|err| format!("write_failed:{}:{err}", path.display()))
}

fn write_store_row<W: Write>(writer: &mut W, row: &StoreRow) -> std::io::Result<()> {
    writeln!(
        writer,
        "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
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
    )
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
    let alternate_payload_available = match source.id {
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
                .map(|files| {
                    let derived_query = store.join("downloads/chembl/chembl_query.sqlite");
                    files.iter().any(|path| path != &derived_query)
                })
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
    };
    alternate_payload_available || retained_source_available(store, source)
}

fn retained_source_available(store: &Path, source: &RequiredSource) -> bool {
    let Ok(retention_rows) =
        read_simple_tsv_rows(&store.join("manifests/source_retention_manifest.tsv"))
    else {
        return false;
    };
    let mut retention_matches = retention_rows
        .iter()
        .filter(|row| row.get("source_id").map(String::as_str) == Some(source.id));
    let Some(retention) = retention_matches.next() else {
        return false;
    };
    if retention_matches.next().is_some()
        || retention.get("schema_version").map(String::as_str)
            != Some(SOURCE_RETENTION_SCHEMA_VERSION)
        || retention.get("state").map(String::as_str) != Some("pruned")
        || !source_path_matches(
            retention.get("source_local_path").map(String::as_str),
            source.local_rel_path,
        )
    {
        return false;
    }

    let Ok(source_rows) = read_simple_tsv_rows(&store.join("manifests/source_file_manifest.tsv"))
    else {
        return false;
    };
    let mut source_matches = source_rows
        .iter()
        .filter(|row| row.get("source_id").map(String::as_str) == Some(source.id));
    let Some(source_row) = source_matches.next() else {
        return false;
    };
    if source_matches.next().is_some()
        || source_row.get("status").map(String::as_str) != Some("ready")
        || source_row.get("storage_state").map(String::as_str) != Some("indexed_source_pruned")
        || !source_path_matches(
            source_row.get("local_path").map(String::as_str),
            source.local_rel_path,
        )
    {
        return false;
    }

    let source_digest = source_row.get("sha256").map(String::as_str).unwrap_or("");
    let release = source_row
        .get("release_identity")
        .map(String::as_str)
        .unwrap_or("");
    let trust_source = source_row
        .get("trust_source")
        .map(String::as_str)
        .unwrap_or("");
    if !valid_sha256_hex(source_digest)
        || release.trim().is_empty()
        || trust_source.trim().is_empty()
        || retention.get("source_manifest_sha256").map(String::as_str) != Some(source_digest)
        || retention.get("release_identity").map(String::as_str) != Some(release)
        || retention.get("trust_source").map(String::as_str) != Some(trust_source)
    {
        return false;
    }

    let Some(derived_paths) = retention.get("derived_paths") else {
        return false;
    };
    let derived_paths = derived_paths
        .split(';')
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .collect::<Vec<_>>();
    if matches!(source.id, "chembl" | "bindingdb") {
        let expected = BTreeSet::from([
            LIGAND_SIMILARITY_DATABASE_REL_PATH,
            LIGAND_SIMILARITY_MANIFEST_REL_PATH,
        ]);
        let actual = derived_paths.iter().copied().collect::<BTreeSet<_>>();
        if actual != expected || derived_paths.len() != expected.len() {
            return false;
        }
        return validate_retained_ligand_similarity_publication(store).is_ok();
    }
    !derived_paths.is_empty()
        && derived_paths
            .iter()
            .all(|relative| retained_derived_path_has_payload(store, relative))
}

fn source_path_matches(actual: Option<&str>, expected: &str) -> bool {
    actual.is_some_and(|actual| actual.replace('\\', "/") == expected.replace('\\', "/"))
}

fn retained_derived_path_has_payload(store: &Path, relative: &str) -> bool {
    let relative = Path::new(relative);
    if relative.is_absolute()
        || relative.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        return false;
    }
    let path = store.join(relative);
    if path.is_file() {
        return path.metadata().is_ok_and(|metadata| metadata.len() > 0);
    }
    if !path.is_dir() {
        return false;
    }
    let Ok(entries) = fs::read_dir(path) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let path = entry.path();
        if path.is_file() {
            path.metadata().is_ok_and(|metadata| metadata.len() > 0)
        } else {
            retained_derived_path_has_payload(store, &relative_display(store, &path))
        }
    })
}

fn any_source_path_exists(store: &Path, rel_paths: &[&str]) -> bool {
    rel_paths.iter().any(|rel| store.join(rel).exists())
}

fn path_has_any_file(path: &Path) -> bool {
    if path.is_file() {
        return true;
    }
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
    let invocation_id = start_pipeline_invocation(options)?;
    start_worker_invocation(options)?;
    write_text(&options.run_dir.join("last_successful_stage.txt"), "none\n")?;
    write_run_context(options, "starting", &[])?;
    let blocked = pipeline_blockers(options).map_err(|err| {
        record_pipeline_failure(options, &invocation_id, "preflight", &err);
        format!("pipeline_failed:preflight:{err}")
    })?;
    if !blocked.is_empty() {
        let readiness = evaluate_science_store_readiness(&options.science_store)?;
        write_blocked_outputs(options, &invocation_id, &blocked, &readiness)?;
        write_run_context(options, "blocked", &blocked)?;
        return Err(format!(
            "pipeline_blocked\treasons={}\trepair_actions={}\trun_dir={}",
            blocked.join(","),
            readiness.repair_actions.join(","),
            options.run_dir.display()
        ));
    }
    run_pipeline_stage(options, &invocation_id, "input_normalize", || {
        normalize_input(options)
    })?;
    run_pipeline_stage(options, &invocation_id, "query_structure_prepare", || {
        prepare_query_structures(options)
    })?;
    run_pipeline_stage(options, &invocation_id, "features_compute", || {
        compute_features(options)
    })?;
    run_pipeline_stage(options, &invocation_id, "onsite_sequence_alignment", || {
        run_single_worker_task(options, &SEQUENCE_ALIGNMENT_WORKER_TASK)?;
        Ok("onsite_sequence_alignment_checked".to_string())
    })?;
    run_pipeline_stage(options, &invocation_id, "database_match_known", || {
        match_known(options)
    })?;
    run_pipeline_stage(
        options,
        &invocation_id,
        "metagenome_match_potential",
        || match_metagenome(options),
    )?;
    run_pipeline_stage(options, &invocation_id, "structure_evaluate", || {
        evaluate_structure(options)
    })?;
    run_pipeline_stage(options, &invocation_id, "candidate_coarse_rank", || {
        coarse_rank_candidates(options)
    })?;
    run_pipeline_stage(options, &invocation_id, "dynamics_screen", || {
        execute_dynamics_screen(options)
    })?;
    run_pipeline_stage(options, &invocation_id, "target_rank", || {
        rank_targets(options)
    })?;
    run_pipeline_stage(options, &invocation_id, "evidence_build", || {
        build_evidence(options)
    })?;
    run_pipeline_stage(options, &invocation_id, "report_render", || {
        render_report(options)
    })?;
    write_artifact_source_map(options)?;
    if !pipeline_invocation_is_current(options, &invocation_id) {
        return Err("pipeline_invocation_superseded".to_string());
    }
    write_text(&options.run_dir.join("run_state.txt"), "completed\n")?;
    write_run_context(options, "completed", &[])?;
    Ok(format!(
        "pipeline_completed\trun_dir={}",
        options.run_dir.display()
    ))
}

fn finalize_pipeline(options: &ScienceOptions) -> Result<String, String> {
    ensure_dir(&options.run_dir)?;
    let invocation_id = current_pipeline_invocation_id(options)?;
    let state = read_text(&options.run_dir.join("run_state.txt"))?
        .trim()
        .to_string();
    if !matches!(state.as_str(), "failed" | "resumable") {
        return Err(format!("pipeline_finalize_state_invalid:{state}"));
    }
    let last_successful = read_text(&options.run_dir.join("last_successful_stage.txt"))?
        .trim()
        .to_string();
    if last_successful != "dynamics_screen" {
        return Err(format!(
            "pipeline_finalize_last_successful_stage_invalid:{last_successful}"
        ));
    }
    let current_stage = read_text(&options.run_dir.join("current_stage.txt"))?
        .trim()
        .to_string();
    if current_stage != "target_rank" {
        return Err(format!(
            "pipeline_finalize_current_stage_invalid:{current_stage}"
        ));
    }
    let failure = read_text(&options.run_dir.join("failure_reason.txt"))?;
    if !failure.lines().any(|line| line == "stage=target_rank")
        || !failure
            .lines()
            .any(|line| line == "error_code=authenticity_gate_failed")
        || !failure
            .lines()
            .any(|line| line == format!("invocation_id={invocation_id}"))
    {
        return Err("pipeline_finalize_failure_identity_invalid".to_string());
    }
    let migrated = normalize_legacy_structure_match_schema(options)?;
    run_pipeline_stage(options, &invocation_id, "target_rank", || {
        rank_targets(options)
    })?;
    run_pipeline_stage(options, &invocation_id, "evidence_build", || {
        build_evidence(options)
    })?;
    run_pipeline_stage(options, &invocation_id, "report_render", || {
        render_report(options)
    })?;
    write_artifact_source_map(options)?;
    if !pipeline_invocation_is_current(options, &invocation_id) {
        return Err("pipeline_invocation_superseded".to_string());
    }
    let failure_path = options.run_dir.join("failure_reason.txt");
    if failure_path.is_file() {
        fs::remove_file(&failure_path)
            .map_err(|err| format!("remove_file_failed:{}:{err}", failure_path.display()))?;
    }
    write_text(&options.run_dir.join("run_state.txt"), "completed\n")?;
    write_run_context(options, "completed", &[])?;
    Ok(format!(
        "pipeline_finalized\trun_dir={}\tpipeline_invocation_id={invocation_id}\tlegacy_structure_schema_migrated={migrated}",
        options.run_dir.display()
    ))
}

fn normalize_legacy_structure_match_schema(options: &ScienceOptions) -> Result<bool, String> {
    let path = options.run_dir.join("structure_interaction_matches.tsv");
    let text = read_text(&path)?;
    let mut lines = text.lines();
    let header = lines.next().unwrap_or_default();
    let legacy_header = "input_id\tsource\ttarget_id\ttarget_name\torganism\ttarget_class\tmatch_score\tmatched_features\tevidence\trecord_id";
    let current_header = "input_id\tsource\ttarget_id\ttarget_name\torganism\ttarget_class\tmatch_score\tmatched_features\tevidence\tsource_database_id\tcurrent_run_query_id\trecord_id";
    if header != legacy_header {
        let headers = split_tsv(header);
        if !["input_id", "source", "target_id", "evidence", "record_id"]
            .into_iter()
            .all(|required| headers.iter().any(|header| header == required))
        {
            return Err("structure_match_schema_header_invalid".to_string());
        }
        for (offset, line) in lines.enumerate() {
            if !line.trim().is_empty() && split_tsv(line).len() != headers.len() {
                return Err(format!(
                    "structure_match_schema_width_invalid:{}",
                    offset + 2
                ));
            }
        }
        return Ok(false);
    }
    let mut normalized = String::from(current_header);
    normalized.push('\n');
    for (offset, line) in lines.enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let cols = split_tsv(line);
        let row = match cols.len() {
            10 => {
                let mut row = cols[..9].to_vec();
                row.push(cols[1].clone());
                row.push(cols[0].clone());
                row.push(cols[9].clone());
                row
            }
            12 => cols,
            _ => {
                return Err(format!(
                    "structure_match_schema_width_invalid:{}",
                    offset + 2
                ));
            }
        };
        normalized.push_str(&row.join("\t"));
        normalized.push('\n');
    }
    write_text(&path, &normalized)?;
    Ok(true)
}

fn run_pipeline_stage<F>(
    options: &ScienceOptions,
    invocation_id: &str,
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
    if !pipeline_invocation_is_current(options, invocation_id) {
        return Err("pipeline_invocation_superseded".to_string());
    }
    match operation() {
        Ok(result) => {
            write_text(
                &options.run_dir.join("last_successful_stage.txt"),
                &(stage.to_string() + "\n"),
            )?;
            Ok(result)
        }
        Err(err) => {
            record_pipeline_failure(options, invocation_id, stage, &err);
            Err(format!("pipeline_failed:{stage}:{err}"))
        }
    }
}

fn start_pipeline_invocation(options: &ScienceOptions) -> Result<String, String> {
    let sequence = PIPELINE_INVOCATION_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    let invocation_id = format!("pipeline_{millis}_{sequence}");
    write_text(
        &options.run_dir.join(PIPELINE_INVOCATION_FILE),
        &(invocation_id.clone() + "\n"),
    )?;
    Ok(invocation_id)
}

fn pipeline_invocation_is_current(options: &ScienceOptions, invocation_id: &str) -> bool {
    read_text(&options.run_dir.join(PIPELINE_INVOCATION_FILE))
        .is_ok_and(|value| value.trim() == invocation_id)
}

fn repair_action_for_stage(stage: &str) -> &'static str {
    match stage {
        "preflight" => "repair_science_store_or_input_then_rerun",
        "input_normalize" => "correct_the_reported_input_record_then_rerun",
        "onsite_sequence_alignment" | "database_match_known" | "metagenome_match_potential" => {
            "repair_the_reported_database_or_worker_then_rerun"
        }
        "structure_evaluate" | "candidate_coarse_rank" | "dynamics_screen" => {
            "repair_the_reported_candidate_artifact_or_worker_then_rerun"
        }
        _ => "inspect_failure_reason_then_rerun",
    }
}

fn stable_pipeline_error_code(reason: &str) -> String {
    reason
        .split([':', '\t', '\n'])
        .next()
        .map(clean_cell)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "pipeline_stage_failed".to_string())
}

fn record_pipeline_failure(
    options: &ScienceOptions,
    invocation_id: &str,
    stage: &str,
    reason: &str,
) {
    if !pipeline_invocation_is_current(options, invocation_id) {
        return;
    }
    let last_successful_stage = read_text(&options.run_dir.join("last_successful_stage.txt"))
        .unwrap_or_else(|_| "none".to_string());
    let failure = format!(
        "schema_version=genoligand_pipeline_failure_v1\nstate=failed\nerror_code={}\nstage={}\ninvocation_id={}\nlast_successful_stage={}\nrepair_action={}\nreason={}\n",
        stable_pipeline_error_code(reason),
        clean_cell(stage),
        clean_cell(invocation_id),
        clean_cell(last_successful_stage.trim()),
        repair_action_for_stage(stage),
        clean_cell(reason),
    );
    let _ = write_text(&options.run_dir.join("run_state.txt"), "failed\n");
    let _ = write_text(&options.run_dir.join("failure_reason.txt"), &failure);
    let _ = write_run_context(
        options,
        "failed",
        &[format!("{stage}:{}", clean_cell(reason))],
    );
}

fn pipeline_blockers(options: &ScienceOptions) -> Result<Vec<String>, String> {
    let mut blockers = Vec::new();
    if !options.input_manifest.exists() {
        blockers.push("missing_input_manifest".to_string());
    } else if !input_manifest_has_declared_input(options)? {
        blockers.push("missing_input".to_string());
    }
    let readiness = evaluate_science_store_readiness(&options.science_store)?;
    blockers.extend(readiness.blockers);
    blockers.sort();
    blockers.dedup();
    Ok(blockers)
}

fn write_blocked_outputs(
    options: &ScienceOptions,
    invocation_id: &str,
    blockers: &[String],
    readiness: &ScienceStoreReadiness,
) -> Result<(), String> {
    ensure_dir(&options.run_dir)?;
    if !pipeline_invocation_is_current(options, invocation_id) {
        return Err("pipeline_invocation_superseded".to_string());
    }
    write_text(&options.run_dir.join("run_state.txt"), "blocked\n")?;
    write_text(
        &options.run_dir.join("failure_reason.txt"),
        &format!(
            "schema_version=genoligand_pipeline_failure_v1\nstate=blocked\nerror_code=pipeline_preflight_blocked\nstage=preflight\ninvocation_id={}\nlast_successful_stage=none\nrepair_action=repair_science_store_or_input_then_rerun\nblockers={}\nreadiness_evidence={}\nreadiness_repair_actions={}\n",
            clean_cell(invocation_id),
            clean_cell(&blockers.join(",")),
            clean_cell(&readiness_blocker_details(readiness)),
            clean_cell(&readiness.repair_actions.join(",")),
        ),
    )?;
    write_text(
        &options.run_dir.join("target_candidate_ranking.tsv"),
        &(TARGET_CANDIDATE_RANKING_HEADER.to_string() + "\n"),
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

fn readiness_blocker_details(readiness: &ScienceStoreReadiness) -> String {
    readiness
        .blocker_details
        .iter()
        .map(|blocker| {
            format!(
                "{}|component={}|evidence={}|repair_action={}",
                clean_cell(&blocker.code),
                clean_cell(&blocker.component),
                clean_cell(&blocker.evidence),
                clean_cell(&blocker.repair_action),
            )
        })
        .collect::<Vec<_>>()
        .join(";")
}

fn readiness_advisory_details(readiness: &ScienceStoreReadiness) -> String {
    readiness
        .advisory_details
        .iter()
        .map(|advisory| {
            format!(
                "{}|component={}|evidence={}|action={}",
                clean_cell(&advisory.code),
                clean_cell(&advisory.component),
                clean_cell(&advisory.evidence),
                clean_cell(&advisory.action),
            )
        })
        .collect::<Vec<_>>()
        .join(";")
}

fn normalization_tokens(kind: InputKind, value: &str) -> BTreeSet<String> {
    match kind {
        InputKind::Peptide | InputKind::ProteinLigand | InputKind::GenomeOrMetagenome => {
            compact_sequence_tokens(value)
        }
        InputKind::SmallMolecule | InputKind::Complex => tokenize(value),
    }
}

fn is_file_sequence_conversion(input: &input_conversion::ConvertedInput) -> bool {
    input.source_kind == "file"
        && input.conversion_status == "converted"
        && matches!(
            input.canonical_kind,
            Some(InputKind::Peptide | InputKind::ProteinLigand | InputKind::GenomeOrMetagenome)
        )
        && matches!(
            input.format.as_str(),
            "fasta" | "fa" | "faa" | "fna" | "fastq" | "fq" | "gb" | "gbk"
        )
}

fn spool_sequence_conversion<W: Write>(
    writer: &mut W,
    offset: &mut u64,
    input: &mut input_conversion::ConvertedInput,
) -> Result<(), String> {
    let length = u64::try_from(input.canonical_value.len())
        .map_err(|_| "input_sequence_spool_length_overflow".to_string())?;
    writer
        .write_all(input.canonical_value.as_bytes())
        .map_err(|err| format!("input_sequence_spool_write_failed:{err}"))?;
    input.artifact = format!(
        "{INPUT_SEQUENCE_SPOOL_FILE}#offset={}&length={length}",
        *offset
    );
    *offset = offset
        .checked_add(length)
        .ok_or_else(|| "input_sequence_spool_offset_overflow".to_string())?;
    input.canonical_value.clear();
    Ok(())
}

fn normalize_input(options: &ScienceOptions) -> Result<String, String> {
    ensure_dir(&options.run_dir)?;
    let conversion_path = options.run_dir.join("input_conversion_manifest.tsv");
    let normalized_path = options.run_dir.join("input_normalized_manifest.tsv");
    let sequence_spool_path = options.run_dir.join(INPUT_SEQUENCE_SPOOL_FILE);
    let sequence_spool_pending = options.run_dir.join(INPUT_SEQUENCE_SPOOL_PENDING_FILE);
    let conversion_pending = options
        .run_dir
        .join("input_conversion_manifest.tsv.pending");
    let normalized_pending = options
        .run_dir
        .join("input_normalized_manifest.tsv.pending");
    if sequence_spool_pending.exists() {
        fs::remove_file(&sequence_spool_pending).map_err(|err| {
            format!(
                "input_sequence_spool_cleanup_failed:{}:{err}",
                sequence_spool_pending.display()
            )
        })?;
    }
    let mut sequence_spool_writer = BufWriter::new(
        fs::File::create(&sequence_spool_pending)
            .map_err(|err| format!("input_sequence_spool_write_failed:{err}"))?,
    );
    let mut conversion_writer = BufWriter::new(
        fs::File::create(&conversion_pending)
            .map_err(|err| format!("input_conversion_manifest_write_failed:{err}"))?,
    );
    let mut normalized_writer = BufWriter::new(
        fs::File::create(&normalized_pending)
            .map_err(|err| format!("input_normalized_manifest_write_failed:{err}"))?,
    );
    writeln!(conversion_writer, "{INPUT_CONVERSION_HEADER}")
        .map_err(|err| format!("input_conversion_manifest_write_failed:{err}"))?;
    writeln!(normalized_writer, "{NORMALIZED_INPUT_HEADER}")
        .map_err(|err| format!("input_normalized_manifest_write_failed:{err}"))?;
    let mut failed = None::<(String, String)>;
    let mut normalized_rows = 0usize;
    let mut row_count = 0usize;
    let mut spooled_rows = 0usize;
    let mut spool_offset = 0u64;
    let conversion_result = for_each_converted_input(options, |mut input| {
        let normalized = input
            .canonical_kind
            .map(|kind| (kind, normalization_tokens(kind, &input.canonical_value)));
        if is_file_sequence_conversion(&input) {
            spooled_rows += 1;
            spool_sequence_conversion(&mut sequence_spool_writer, &mut spool_offset, &mut input)?;
        }
        write_input_conversion_row(&mut conversion_writer, &input)
            .map_err(|err| format!("input_conversion_manifest_write_failed:{err}"))?;
        row_count += 1;
        if input.conversion_status == "failed" {
            if failed.is_none() {
                failed = Some((input.input_id.clone(), clean_cell(&input.reason)));
            }
        } else if let Some((kind, tokens)) = normalized {
            let tokens = tokens.into_iter().collect::<Vec<_>>().join(" ");
            writeln!(
                normalized_writer,
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\tnormalized\t{}\t{}\t{}",
                input.input_id,
                kind.as_str(),
                encode_tsv_cell(&input.canonical_value),
                input.file_count,
                tokens,
                clean_cell(&input.record_id),
                input.source_kind,
                clean_cell(&input.source_path),
                input.content_sha256,
                input.structure_atom_count,
                input.structure_chain_count,
                input.structure_residue_count,
            )
            .map_err(|err| format!("input_normalized_manifest_write_failed:{err}"))?;
            normalized_rows += 1;
        }
        if row_count.is_multiple_of(input_conversion::INPUT_MANIFEST_WRITE_BATCH_SIZE) {
            conversion_writer
                .flush()
                .map_err(|err| format!("input_conversion_manifest_write_failed:{err}"))?;
            normalized_writer
                .flush()
                .map_err(|err| format!("input_normalized_manifest_write_failed:{err}"))?;
            sequence_spool_writer
                .flush()
                .map_err(|err| format!("input_sequence_spool_write_failed:{err}"))?;
        }
        Ok(())
    });
    if let Err(err) = conversion_result {
        drop(conversion_writer);
        drop(normalized_writer);
        drop(sequence_spool_writer);
        let _ = fs::remove_file(&conversion_pending);
        let _ = fs::remove_file(&normalized_pending);
        let _ = fs::remove_file(&normalized_path);
        let _ = fs::remove_file(&sequence_spool_pending);
        let _ = fs::remove_file(&sequence_spool_path);
        return Err(err);
    }
    conversion_writer
        .flush()
        .map_err(|err| format!("input_conversion_manifest_write_failed:{err}"))?;
    normalized_writer
        .flush()
        .map_err(|err| format!("input_normalized_manifest_write_failed:{err}"))?;
    sequence_spool_writer
        .flush()
        .map_err(|err| format!("input_sequence_spool_write_failed:{err}"))?;
    drop(conversion_writer);
    drop(normalized_writer);
    drop(sequence_spool_writer);
    publish_pending_file(&conversion_pending, &conversion_path)?;
    write_json_manifest(
        &options.run_dir.join("input_conversion_manifest.json"),
        "input_conversion_manifest.tsv",
        "converted",
    )?;
    if let Some((input_id, reason)) = failed {
        let _ = fs::remove_file(&normalized_pending);
        let _ = fs::remove_file(&normalized_path);
        let _ = fs::remove_file(&sequence_spool_pending);
        let _ = fs::remove_file(&sequence_spool_path);
        write_artifact_source_map(options)?;
        return Err(format!("input_conversion_failed:{input_id}:{reason}"));
    }
    if normalized_rows == 0 {
        let _ = fs::remove_file(&normalized_pending);
        let _ = fs::remove_file(&normalized_path);
        let _ = fs::remove_file(&sequence_spool_pending);
        let _ = fs::remove_file(&sequence_spool_path);
        write_artifact_source_map(options)?;
        return Err("missing_input".to_string());
    }
    if spooled_rows == 0 {
        let _ = fs::remove_file(&sequence_spool_pending);
        let _ = fs::remove_file(&sequence_spool_path);
    } else if let Err(err) = publish_pending_file(&sequence_spool_pending, &sequence_spool_path) {
        let _ = fs::remove_file(&normalized_pending);
        return Err(err);
    }
    if let Err(err) = publish_pending_file(&normalized_pending, &normalized_path) {
        let _ = fs::remove_file(&sequence_spool_path);
        return Err(err);
    }
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

fn prepare_query_structures(options: &ScienceOptions) -> Result<String, String> {
    let normalized_path = options.run_dir.join("input_normalized_manifest.tsv");
    let inputs = read_normalized_inputs(&normalized_path)?;
    if inputs.is_empty() {
        return Err("query_structure_normalized_input_missing".to_string());
    }
    let provenance = read_conversion_provenance(&normalized_path)?;
    remove_run_subdir_if_exists(&options.run_dir, "tool_outputs/query_structure")?;
    remove_file_if_exists(&options.run_dir.join("query_structure_manifest.tsv"))?;
    clear_candidate_worker_files(options, "query_structure_prepare")?;

    let job_dir = options.run_dir.join("worker_jobs");
    let result_dir = options.run_dir.join("worker_results");
    ensure_dir(&job_dir)?;
    ensure_dir(&result_dir)?;
    let invocation_id = current_worker_invocation(options)?;
    for input in &inputs {
        let identity = (input.input_id.clone(), input.record_id.clone());
        let source = provenance.get(&identity).ok_or_else(|| {
            format!(
                "query_structure_conversion_provenance_missing:{}:{}",
                clean_cell(&input.input_id),
                clean_cell(&input.record_id)
            )
        })?;
        let input_digest = query_structure_input_digest(input, source);
        let job_id = query_structure_job_id(&input.input_id, &input.record_id, &invocation_id);
        let task = WorkerTask {
            job_id: &job_id,
            worker: "query_structure_prepare",
            module: "genoligand_workers.query_structure_worker",
            stage: "query_structure_prepare",
        };
        let job_path = job_dir.join(format!("{job_id}.json"));
        let result_path = result_dir.join(format!("{job_id}.json"));
        remove_file_if_exists(&result_path)?;
        let mut job = worker_job_value(options, &task)?;
        job["input_id"] = json!(&input.input_id);
        job["record_id"] = json!(&input.record_id);
        job["input_kind"] = json!(input.kind.as_str());
        job["input_digest"] = json!(&input_digest);
        job["source_digest"] = json!(&source.content_sha256);
        job["content_sha256"] = json!(&source.content_sha256);
        if !validate_worker_job(&job, &task) {
            return Err(format!("query_structure_job_invalid:{job_id}"));
        }
        write_text(&job_path, &(job.to_string() + "\n"))?;
        run_worker_task(options, &task, &job_path, &result_path)?;
        let result = read_worker_result_or_failure(&result_path, &task, &job)?;
        if let Some(error) = worker_result_stage_error_bound(&result, &job, options, &task) {
            return Err(error);
        }
    }

    let manifest_path = options.run_dir.join("query_structure_manifest.tsv");
    let artifacts =
        load_validated_query_structure_artifacts(options, &inputs, &provenance, &invocation_id)?;
    Ok(format!(
        "query_structures_prepared\trows={}\tout={}",
        artifacts.len(),
        manifest_path.display()
    ))
}

fn query_structure_input_digest(input: &NormalizedInput, source: &ConversionProvenance) -> String {
    canonical_json_sha256(&json!({
        "input_id": &input.input_id,
        "record_id": &input.record_id,
        "canonical_kind": input.kind.as_str(),
        "canonical_value": &input.primary_value,
        "source_content_sha256": &source.content_sha256,
    }))
}

fn load_validated_query_structure_artifacts(
    options: &ScienceOptions,
    inputs: &[NormalizedInput],
    provenance: &BTreeMap<(String, String), ConversionProvenance>,
    invocation_id: &str,
) -> Result<BTreeMap<(String, String), QueryStructureArtifact>, String> {
    let index_rows =
        read_query_structure_index(&options.run_dir.join("query_structure_manifest.tsv"))?;
    if index_rows.len() != inputs.len() {
        return Err("query_structure_manifest_row_count_mismatch".to_string());
    }
    let mut indexed = BTreeMap::new();
    for row in index_rows {
        let key = (row.input_id.clone(), row.record_id.clone());
        if row.input_id.trim().is_empty()
            || row.record_id.trim().is_empty()
            || row.method.trim().is_empty()
            || row
                .method
                .chars()
                .any(|ch| matches!(ch, '\t' | '\r' | '\n'))
            || !matches!(row.status.as_str(), "ready" | "not_applicable")
            || indexed.insert(key, row).is_some()
        {
            return Err("query_structure_manifest_binding_invalid".to_string());
        }
    }

    let run_id = infer_run_id(&options.run_dir);
    let conversion_digest = sha256_file_hex(&options.run_dir.join("input_conversion_manifest.tsv"))
        .ok_or_else(|| "query_structure_conversion_manifest_digest_unavailable".to_string())?;
    let mut artifacts = BTreeMap::new();
    for input in inputs {
        let identity = (input.input_id.clone(), input.record_id.clone());
        let source = provenance.get(&identity).ok_or_else(|| {
            format!(
                "query_structure_conversion_provenance_missing:{}:{}",
                clean_cell(&input.input_id),
                clean_cell(&input.record_id)
            )
        })?;
        let input_digest = query_structure_input_digest(input, source);
        let row = indexed
            .get(&identity)
            .ok_or_else(|| "query_structure_manifest_identity_missing".to_string())?;
        if row.input_kind != input.kind.as_str()
            || row.input_digest != input_digest
            || !valid_sha256_hex(&row.input_digest)
        {
            return Err("query_structure_manifest_binding_invalid".to_string());
        }

        let job_id = query_structure_job_id(&input.input_id, &input.record_id, invocation_id);
        let task = WorkerTask {
            job_id: &job_id,
            worker: "query_structure_prepare",
            module: "genoligand_workers.query_structure_worker",
            stage: "query_structure_prepare",
        };
        let job = read_json_value_if_file(
            &options
                .run_dir
                .join("worker_jobs")
                .join(format!("{job_id}.json")),
        )
        .ok_or_else(|| format!("query_structure_job_missing:{job_id}"))?;
        if !validate_worker_job(&job, &task)
            || !query_structure_job_binding_matches(
                &job,
                &run_id,
                invocation_id,
                input,
                &input_digest,
                &source.content_sha256,
            )
        {
            return Err(format!("query_structure_job_binding_invalid:{job_id}"));
        }
        let result = read_json_value_if_file(
            &options
                .run_dir
                .join("worker_results")
                .join(format!("{job_id}.json")),
        )
        .ok_or_else(|| format!("query_structure_result_missing:{job_id}"))?;
        if let Some(error) = worker_result_stage_error_bound(&result, &job, options, &task) {
            return Err(error);
        }
        if !query_structure_result_binding_matches(
            &result,
            &run_id,
            invocation_id,
            input,
            &input_digest,
            &source.content_sha256,
            &row.status,
        ) {
            return Err(format!("query_structure_result_binding_invalid:{job_id}"));
        }

        let manifest_reference = result
            .get("manifest")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| format!("query_structure_result_manifest_missing:{job_id}"))?;
        let (manifest_relative, manifest_path) = current_query_artifact(
            &options.run_dir,
            manifest_reference,
            "query_structure_result_manifest",
        )?;
        if fs::metadata(&manifest_path)
            .map_err(|err| format!("query_structure_manifest_metadata_failed:{err}"))?
            .len()
            > MAX_QUERY_STRUCTURE_MANIFEST_BYTES
        {
            return Err("query_structure_artifact_manifest_too_large".to_string());
        }
        let manifest = read_current_run_json_artifact(&options.run_dir, &manifest_relative)?;
        if !query_structure_json_binding_matches(
            &manifest,
            QUERY_STRUCTURE_ARTIFACT_SCHEMA_VERSION,
            &row.status,
            &run_id,
            &job_id,
            invocation_id,
            input,
            &input_digest,
            &source.content_sha256,
            &row.method,
        ) {
            return Err(format!("query_structure_artifact_binding_invalid:{job_id}"));
        }

        let artifact = if row.status == "ready" {
            validate_ready_query_structure_artifact(
                options,
                input,
                row,
                &result,
                &manifest,
                &run_id,
                &job_id,
                invocation_id,
                &input_digest,
                source,
                &conversion_digest,
            )?
        } else {
            validate_not_applicable_query_structure_artifact(row, &result, &manifest)?
        };
        artifacts.insert(identity, artifact);
    }
    if artifacts.len() != indexed.len() {
        return Err("query_structure_manifest_unknown_identity".to_string());
    }
    Ok(artifacts)
}

fn read_query_structure_index(path: &Path) -> Result<Vec<QueryStructureIndexRow>, String> {
    let text = read_text(path)?;
    let mut lines = text.lines();
    let header = lines
        .next()
        .map(|line| line.trim_start_matches('\u{feff}'))
        .unwrap_or_default();
    if header != QUERY_STRUCTURE_MANIFEST_HEADER {
        return Err("query_structure_manifest_schema_mismatch:header".to_string());
    }
    let mut rows = Vec::new();
    for (index, line) in lines.enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let cols = split_tsv(line);
        if cols.len() != 9 {
            return Err(format!(
                "query_structure_manifest_schema_mismatch:line_{}:columns_{}",
                index + 2,
                cols.len()
            ));
        }
        rows.push(QueryStructureIndexRow {
            input_id: cols[0].clone(),
            record_id: cols[1].clone(),
            input_kind: cols[2].clone(),
            input_digest: cols[3].clone(),
            method: cols[4].clone(),
            features_json: cols[5].clone(),
            coordinate_artifact: cols[6].clone(),
            feature_digest: cols[7].clone(),
            status: cols[8].clone(),
        });
    }
    Ok(rows)
}

fn query_structure_job_binding_matches(
    value: &Value,
    run_id: &str,
    invocation_id: &str,
    input: &NormalizedInput,
    input_digest: &str,
    source_digest: &str,
) -> bool {
    [
        ("run_id", run_id),
        ("invocation_id", invocation_id),
        ("input_id", input.input_id.as_str()),
        ("record_id", input.record_id.as_str()),
        ("input_kind", input.kind.as_str()),
        ("input_digest", input_digest),
        ("source_digest", source_digest),
        ("content_sha256", source_digest),
    ]
    .into_iter()
    .all(|(field, expected)| value.get(field).and_then(Value::as_str) == Some(expected))
}

fn query_structure_result_binding_matches(
    value: &Value,
    run_id: &str,
    invocation_id: &str,
    input: &NormalizedInput,
    input_digest: &str,
    source_digest: &str,
    artifact_status: &str,
) -> bool {
    let status_matches = match artifact_status {
        "ready" => {
            value.get("status").and_then(Value::as_str) == Some("PASS")
                && value.get("execution_status").and_then(Value::as_str) == Some("completed")
        }
        "not_applicable" => {
            value.get("status").and_then(Value::as_str) == Some("NOT_APPLICABLE")
                && value.get("execution_status").and_then(Value::as_str) == Some("not_applicable")
        }
        _ => false,
    };
    let expected_rows = usize::from(artifact_status == "ready") as u64;
    let outcome_matches = value
        .get("outcomes")
        .and_then(Value::as_array)
        .is_some_and(|outcomes| {
            outcomes.len() == 1
                && outcomes[0].get("database_id").and_then(Value::as_str)
                    == Some("query_structure_prepare")
                && outcomes[0].get("row_count").and_then(Value::as_u64) == Some(expected_rows)
        });
    let optional_kind_matches = value
        .get("input_kind")
        .is_none_or(|kind| kind.as_str() == Some(input.kind.as_str()));
    status_matches
        && outcome_matches
        && optional_kind_matches
        && [
            ("run_id", run_id),
            ("invocation_id", invocation_id),
            ("input_id", input.input_id.as_str()),
            ("record_id", input.record_id.as_str()),
            ("input_digest", input_digest),
            ("source_digest", source_digest),
        ]
        .into_iter()
        .all(|(field, expected)| value.get(field).and_then(Value::as_str) == Some(expected))
}

#[allow(clippy::too_many_arguments)]
fn query_structure_json_binding_matches(
    value: &Value,
    schema_version: &str,
    status: &str,
    run_id: &str,
    job_id: &str,
    invocation_id: &str,
    input: &NormalizedInput,
    input_digest: &str,
    source_digest: &str,
    method: &str,
) -> bool {
    [
        ("schema_version", schema_version),
        ("status", status),
        ("run_id", run_id),
        ("job_id", job_id),
        ("invocation_id", invocation_id),
        ("input_id", input.input_id.as_str()),
        ("record_id", input.record_id.as_str()),
        ("input_kind", input.kind.as_str()),
        ("input_digest", input_digest),
        ("source_content_sha256", source_digest),
        ("method_version", method),
    ]
    .into_iter()
    .all(|(field, expected)| value.get(field).and_then(Value::as_str) == Some(expected))
}

#[allow(clippy::too_many_arguments)]
fn validate_ready_query_structure_artifact(
    options: &ScienceOptions,
    input: &NormalizedInput,
    row: &QueryStructureIndexRow,
    result: &Value,
    manifest: &Value,
    run_id: &str,
    job_id: &str,
    invocation_id: &str,
    input_digest: &str,
    source: &ConversionProvenance,
    conversion_digest: &str,
) -> Result<QueryStructureArtifact, String> {
    if row.features_json.trim().is_empty()
        || !valid_sha256_hex(&row.feature_digest)
        || row.feature_digest != row.feature_digest.to_ascii_lowercase()
        || manifest.get("features_json").and_then(Value::as_str) != Some(row.features_json.as_str())
        || manifest.get("feature_digest").and_then(Value::as_str)
            != Some(row.feature_digest.as_str())
        || manifest
            .get("conversion_manifest_sha256")
            .and_then(Value::as_str)
            != Some(conversion_digest)
    {
        return Err("query_structure_ready_manifest_incomplete".to_string());
    }
    let result_features = result
        .get("artifact_path")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "query_structure_result_features_missing".to_string())?;
    let (feature_relative, feature_path) = current_query_artifact(
        &options.run_dir,
        &row.features_json,
        "query_structure_features",
    )?;
    let (result_feature_relative, _) = current_query_artifact(
        &options.run_dir,
        result_features,
        "query_structure_result_features",
    )?;
    if feature_relative != result_feature_relative {
        return Err("query_structure_result_features_mismatch".to_string());
    }
    let feature_size = fs::metadata(&feature_path)
        .map_err(|err| format!("query_structure_features_metadata_failed:{err}"))?
        .len();
    if feature_size == 0 || feature_size > MAX_QUERY_STRUCTURE_FEATURE_BYTES {
        return Err("query_structure_features_size_invalid".to_string());
    }
    if sha256_file_hex(&feature_path).as_deref() != Some(row.feature_digest.as_str()) {
        return Err("query_structure_features_digest_mismatch".to_string());
    }
    let features = read_current_run_json_artifact(&options.run_dir, &feature_relative)?;
    if !query_structure_json_binding_matches(
        &features,
        QUERY_STRUCTURE_FEATURE_SCHEMA_VERSION,
        "ready",
        run_id,
        job_id,
        invocation_id,
        input,
        input_digest,
        &source.content_sha256,
        &row.method,
    ) || features
        .get("conversion_manifest_sha256")
        .and_then(Value::as_str)
        != Some(conversion_digest)
    {
        return Err("query_structure_features_binding_invalid".to_string());
    }

    let manifest_coordinate = manifest
        .get("coordinate_artifact")
        .and_then(Value::as_str)
        .ok_or_else(|| "query_structure_coordinate_binding_missing".to_string())?;
    let coordinate_digest = manifest
        .get("coordinate_sha256")
        .and_then(Value::as_str)
        .ok_or_else(|| "query_structure_coordinate_digest_missing".to_string())?;
    let coordinate_size = manifest
        .get("coordinate_size_bytes")
        .and_then(Value::as_u64)
        .ok_or_else(|| "query_structure_coordinate_size_missing".to_string())?;
    let feature_coordinate = features
        .get("coordinate_artifact")
        .and_then(Value::as_object)
        .ok_or_else(|| "query_structure_features_coordinate_missing".to_string())?;
    if manifest_coordinate != row.coordinate_artifact
        || feature_coordinate.get("path").and_then(Value::as_str) != Some(manifest_coordinate)
        || feature_coordinate.get("sha256").and_then(Value::as_str) != Some(coordinate_digest)
        || feature_coordinate.get("size_bytes").and_then(Value::as_u64) != Some(coordinate_size)
        || feature_coordinate
            .get("retention")
            .and_then(Value::as_str)
            .is_none_or(|value| value.trim().is_empty())
        || manifest
            .get("retention")
            .and_then(Value::as_str)
            .is_none_or(|value| value.trim().is_empty())
    {
        return Err("query_structure_coordinate_binding_invalid".to_string());
    }
    if manifest_coordinate.is_empty() {
        if !coordinate_digest.is_empty() || coordinate_size != 0 {
            return Err("query_structure_empty_coordinate_binding_invalid".to_string());
        }
    } else {
        if !valid_sha256_hex(coordinate_digest)
            || coordinate_digest != coordinate_digest.to_ascii_lowercase()
        {
            return Err("query_structure_coordinate_digest_invalid".to_string());
        }
        let coordinate_path =
            resolve_query_coordinate_artifact(&options.run_dir, manifest_coordinate, source)?;
        let actual_size = fs::metadata(&coordinate_path)
            .map_err(|err| format!("query_structure_coordinate_metadata_failed:{err}"))?
            .len();
        if actual_size != coordinate_size
            || sha256_file_hex(&coordinate_path).as_deref() != Some(coordinate_digest)
        {
            return Err("query_structure_coordinate_content_mismatch".to_string());
        }
    }

    let feature_rows = query_structure_feature_rows(&features, input.kind.as_str())?;
    if feature_rows.is_empty() || feature_rows.len() > MAX_QUERY_STRUCTURE_FEATURE_ROWS {
        return Err("query_structure_compact_feature_count_invalid".to_string());
    }
    Ok(QueryStructureArtifact {
        method: row.method.clone(),
        coordinate_artifact: row.coordinate_artifact.clone(),
        feature_digest: row.feature_digest.clone(),
        status: row.status.clone(),
        feature_rows,
    })
}

fn validate_not_applicable_query_structure_artifact(
    row: &QueryStructureIndexRow,
    result: &Value,
    manifest: &Value,
) -> Result<QueryStructureArtifact, String> {
    if !row.features_json.is_empty()
        || !row.coordinate_artifact.is_empty()
        || !row.feature_digest.is_empty()
        || result
            .get("artifact_path")
            .and_then(Value::as_str)
            .is_some_and(|value| !value.trim().is_empty())
        || [
            "features_json",
            "feature_digest",
            "coordinate_artifact",
            "coordinate_sha256",
            "coordinate_size_bytes",
        ]
        .into_iter()
        .any(|field| manifest.get(field).is_some())
        || manifest
            .get("reason")
            .and_then(Value::as_str)
            .is_none_or(|value| value.trim().is_empty())
        || manifest.get("reason").and_then(Value::as_str)
            != result.get("reason").and_then(Value::as_str)
    {
        return Err("query_structure_not_applicable_binding_invalid".to_string());
    }
    Ok(QueryStructureArtifact {
        method: row.method.clone(),
        coordinate_artifact: String::new(),
        feature_digest: String::new(),
        status: row.status.clone(),
        feature_rows: Vec::new(),
    })
}

fn current_query_artifact(
    run_dir: &Path,
    reference: &str,
    label: &str,
) -> Result<(String, PathBuf), String> {
    let relative = current_run_artifact_relative(run_dir, reference)
        .ok_or_else(|| format!("{label}_outside_run:{}", clean_cell(reference)))?;
    Ok((relative.clone(), run_dir.join(relative)))
}

fn resolve_query_coordinate_artifact(
    run_dir: &Path,
    reference: &str,
    source: &ConversionProvenance,
) -> Result<PathBuf, String> {
    if let Some(relative) = current_run_artifact_relative(run_dir, reference) {
        return Ok(run_dir.join(relative));
    }
    if source.source_kind != "file" || source.source_path.trim().is_empty() {
        return Err("query_structure_coordinate_outside_run".to_string());
    }
    let resolve = |value: &str| {
        let path = if cfg!(windows) {
            wsl_path_to_windows(value).unwrap_or_else(|| PathBuf::from(value))
        } else {
            PathBuf::from(value)
        };
        path.canonicalize().ok()
    };
    let coordinate = resolve(reference)
        .filter(|path| path.is_file())
        .ok_or_else(|| "query_structure_coordinate_missing".to_string())?;
    let source_path = resolve(&source.source_path)
        .filter(|path| path.is_file())
        .ok_or_else(|| "query_structure_coordinate_source_missing".to_string())?;
    if coordinate != source_path {
        return Err("query_structure_coordinate_source_path_mismatch".to_string());
    }
    Ok(coordinate)
}

fn query_structure_feature_rows(
    value: &Value,
    input_kind: &str,
) -> Result<Vec<FeatureRow>, String> {
    if let Some(structure) = value.get("structure").and_then(Value::as_object) {
        return query_coordinate_feature_rows(structure, value);
    }
    match input_kind {
        "small_molecule" => query_small_molecule_feature_rows(value),
        "peptide" | "protein_ligand" => query_sequence_feature_rows(value),
        "complex" => Err("query_structure_complex_ready_without_coordinates".to_string()),
        "genome_or_metagenome" => Err("query_structure_genome_ready_status_invalid".to_string()),
        _ => Err("query_structure_feature_kind_invalid".to_string()),
    }
}

fn query_small_molecule_feature_rows(value: &Value) -> Result<Vec<FeatureRow>, String> {
    let physicochemical = value
        .get("physicochemical")
        .and_then(Value::as_object)
        .ok_or_else(|| "query_structure_physicochemical_missing".to_string())?;
    let mut rows = Vec::new();
    for (name, width, minimum, maximum) in [
        ("molecular_weight", 50.0, 0.0, 100_000.0),
        ("logp", 0.5, -100.0, 100.0),
        ("topological_polar_surface_area", 20.0, 0.0, 100_000.0),
        ("hydrogen_bond_donors", 1.0, 0.0, 10_000.0),
        ("hydrogen_bond_acceptors", 1.0, 0.0, 10_000.0),
        ("rotatable_bonds", 2.0, 0.0, 10_000.0),
        ("ring_count", 1.0, 0.0, 10_000.0),
        ("aromatic_ring_count", 1.0, 0.0, 10_000.0),
        ("fraction_csp3", 0.1, 0.0, 1.0),
        ("formal_charge", 1.0, -1_000.0, 1_000.0),
        ("heavy_atom_count", 5.0, 0.0, 100_000.0),
        ("qed", 0.1, 0.0, 1.0),
    ] {
        let number = query_json_number(physicochemical.get(name), name, minimum, maximum)?;
        rows.push(query_binned_feature_row(
            "query_structure_physicochemical",
            name,
            number,
            width,
        ));
    }

    let fingerprint = value
        .get("fingerprint")
        .and_then(Value::as_object)
        .ok_or_else(|| "query_structure_fingerprint_missing".to_string())?;
    let packed = fingerprint
        .get("packed_hex")
        .and_then(Value::as_str)
        .and_then(decode_lower_hex)
        .filter(|bytes| bytes.len() == 256)
        .ok_or_else(|| "query_structure_fingerprint_encoding_invalid".to_string())?;
    let population = fingerprint
        .get("population_count")
        .and_then(Value::as_u64)
        .filter(|count| *count <= 2048)
        .ok_or_else(|| "query_structure_fingerprint_population_invalid".to_string())?;
    let observed_population = packed
        .iter()
        .map(|byte| byte.count_ones() as u64)
        .sum::<u64>();
    if fingerprint.get("algorithm").and_then(Value::as_str) != Some("morgan_ecfp4")
        || fingerprint.get("radius").and_then(Value::as_u64) != Some(2)
        || fingerprint.get("n_bits").and_then(Value::as_u64) != Some(2048)
        || fingerprint
            .get("include_chirality")
            .and_then(Value::as_bool)
            != Some(true)
        || fingerprint.get("encoding").and_then(Value::as_str) != Some("rdkit_binary_text_hex_v1")
        || fingerprint.get("packed_sha256").and_then(Value::as_str)
            != Some(sha256_bytes_hex(&packed).as_str())
        || observed_population != population
    {
        return Err("query_structure_fingerprint_contract_invalid".to_string());
    }
    rows.push(query_binned_feature_row(
        "query_structure_fingerprint",
        "population_count",
        population as f64,
        16.0,
    ));
    rows.extend(query_pharmacophore_feature_rows(value)?);
    Ok(rows)
}

fn query_pharmacophore_feature_rows(value: &Value) -> Result<Vec<FeatureRow>, String> {
    let pharmacophore = value
        .get("pharmacophore")
        .and_then(Value::as_object)
        .ok_or_else(|| "query_structure_pharmacophore_missing".to_string())?;
    let counts = pharmacophore
        .get("counts")
        .and_then(Value::as_object)
        .filter(|counts| counts.len() <= 32)
        .ok_or_else(|| "query_structure_pharmacophore_counts_invalid".to_string())?;
    let point_count = pharmacophore
        .get("point_count")
        .and_then(Value::as_u64)
        .filter(|count| *count <= 100_000)
        .ok_or_else(|| "query_structure_pharmacophore_point_count_invalid".to_string())?;
    let mut total = 0_u64;
    let mut rows = Vec::new();
    for (family, count) in counts {
        let count = count
            .as_u64()
            .filter(|count| *count > 0 && *count <= point_count)
            .ok_or_else(|| "query_structure_pharmacophore_count_invalid".to_string())?;
        total = total
            .checked_add(count)
            .ok_or_else(|| "query_structure_pharmacophore_count_invalid".to_string())?;
        let family = query_token_component(family)
            .ok_or_else(|| "query_structure_pharmacophore_family_invalid".to_string())?;
        rows.push(query_binned_feature_row(
            "query_structure_pharmacophore",
            &format!("{family}_count"),
            count as f64,
            1.0,
        ));
    }
    if total != point_count {
        return Err("query_structure_pharmacophore_total_mismatch".to_string());
    }
    rows.push(query_binned_feature_row(
        "query_structure_pharmacophore",
        "total_point_count",
        point_count as f64,
        4.0,
    ));
    Ok(rows)
}

fn query_sequence_feature_rows(value: &Value) -> Result<Vec<FeatureRow>, String> {
    let sequence = value
        .get("sequence_features")
        .and_then(Value::as_object)
        .ok_or_else(|| "query_structure_sequence_features_missing".to_string())?;
    let length = sequence
        .get("length")
        .and_then(Value::as_u64)
        .filter(|length| *length >= 2 && *length <= 100_000_000)
        .ok_or_else(|| "query_structure_sequence_length_invalid".to_string())?;
    let composition = sequence
        .get("composition")
        .and_then(Value::as_object)
        .filter(|composition| composition.len() <= 20)
        .ok_or_else(|| "query_structure_sequence_composition_invalid".to_string())?;
    let mut counts = BTreeMap::new();
    let mut total = 0_u64;
    for (residue, count) in composition {
        if residue.len() != 1
            || !residue
                .bytes()
                .all(|byte| b"ACDEFGHIKLMNPQRSTVWY".contains(&byte))
        {
            return Err("query_structure_sequence_residue_invalid".to_string());
        }
        let count = count
            .as_u64()
            .filter(|count| *count > 0 && *count <= length)
            .ok_or_else(|| "query_structure_sequence_composition_invalid".to_string())?;
        total = total
            .checked_add(count)
            .ok_or_else(|| "query_structure_sequence_composition_invalid".to_string())?;
        counts.insert(residue.as_bytes()[0] as char, count);
    }
    if total != length {
        return Err("query_structure_sequence_composition_total_mismatch".to_string());
    }

    let mut rows = vec![query_binned_feature_row(
        "query_structure_sequence_prior",
        "length",
        length as f64,
        25.0,
    )];
    for (name, width, minimum, maximum) in [
        (
            "estimated_net_charge_ph7",
            1.0,
            -100_000_000.0,
            100_000_000.0,
        ),
        ("mean_kyte_doolittle_hydropathy", 0.5, -5.0, 5.0),
        ("hydrophobic_fraction", 0.1, 0.0, 1.0),
    ] {
        let number = query_json_number(sequence.get(name), name, minimum, maximum)?;
        rows.push(query_binned_feature_row(
            "query_structure_sequence_prior",
            name,
            number,
            width,
        ));
    }
    if let Some(weight) = sequence.get("molecular_weight") {
        rows.push(query_binned_feature_row(
            "query_structure_sequence_prior",
            "molecular_weight",
            query_json_number(Some(weight), "molecular_weight", 0.0, 1_000_000_000.0)?,
            250.0,
        ));
    }
    for (name, residues) in [
        ("hydrophobic_fraction_from_composition", "AILMFWVY"),
        ("charged_fraction_from_composition", "DEKRH"),
        ("polar_fraction_from_composition", "STNQCY"),
        ("aromatic_fraction_from_composition", "FWY"),
    ] {
        let count = residues
            .chars()
            .map(|residue| counts.get(&residue).copied().unwrap_or(0))
            .sum::<u64>();
        rows.push(query_binned_feature_row(
            "query_structure_sequence_prior",
            name,
            count as f64 / length as f64,
            0.1,
        ));
    }
    if let Some(propensity) = sequence
        .get("secondary_structure_propensity")
        .and_then(Value::as_object)
    {
        for name in ["alpha_helix", "beta_sheet", "turn"] {
            rows.push(query_binned_feature_row(
                "query_structure_sequence_prior",
                name,
                query_json_number(propensity.get(name), name, 0.0, 5.0)?,
                0.1,
            ));
        }
    }
    if value.get("pharmacophore").is_some() {
        rows.extend(query_pharmacophore_feature_rows(value)?);
    }
    Ok(rows)
}

fn query_coordinate_feature_rows(
    structure: &serde_json::Map<String, Value>,
    root: &Value,
) -> Result<Vec<FeatureRow>, String> {
    let atom_count = query_json_u64(structure.get("atom_count"), "atom_count", 1, 500_000)?;
    let heavy_atom_count = query_json_u64(
        structure.get("heavy_atom_count"),
        "heavy_atom_count",
        1,
        atom_count,
    )?;
    let chain_count = query_json_u64(structure.get("chain_count"), "chain_count", 1, 100_000)?;
    let residue_count =
        query_json_u64(structure.get("residue_count"), "residue_count", 1, 200_000)?;
    let radius = query_json_number(
        structure.get("radius_of_gyration_angstrom"),
        "radius_of_gyration_angstrom",
        0.0,
        1_000_000.0,
    )?;
    if structure.get("coordinate_sha256").and_then(Value::as_str)
        != root.get("source_content_sha256").and_then(Value::as_str)
    {
        return Err("query_structure_coordinate_source_digest_mismatch".to_string());
    }
    let minimum = query_json_xyz(
        structure.get("bounding_box_min_angstrom"),
        "bounding_box_min",
    )?;
    let maximum = query_json_xyz(
        structure.get("bounding_box_max_angstrom"),
        "bounding_box_max",
    )?;
    let mut extents = [0.0; 3];
    for index in 0..3 {
        if maximum[index] < minimum[index] {
            return Err("query_structure_bounding_box_invalid".to_string());
        }
        extents[index] = maximum[index] - minimum[index];
    }
    extents.sort_by(f64::total_cmp);

    let mut rows = vec![
        query_binned_feature_row(
            "query_structure_coordinate",
            "atom_count",
            atom_count as f64,
            250.0,
        ),
        query_binned_feature_row(
            "query_structure_coordinate",
            "heavy_atom_count",
            heavy_atom_count as f64,
            250.0,
        ),
        query_binned_feature_row(
            "query_structure_coordinate",
            "chain_count",
            chain_count as f64,
            1.0,
        ),
        query_binned_feature_row(
            "query_structure_coordinate",
            "residue_count",
            residue_count as f64,
            25.0,
        ),
        query_binned_feature_row(
            "query_structure_coordinate",
            "radius_of_gyration_angstrom",
            radius,
            5.0,
        ),
    ];
    for (index, extent) in extents.into_iter().enumerate() {
        rows.push(query_binned_feature_row(
            "query_structure_coordinate",
            &format!("sorted_extent_{}_angstrom", index + 1),
            extent,
            5.0,
        ));
    }

    let element_counts = query_count_map(
        structure.get("element_counts"),
        "element_counts",
        atom_count,
        64,
    )?;
    if element_counts.iter().map(|(_, count)| *count).sum::<u64>() != atom_count {
        return Err("query_structure_element_count_total_mismatch".to_string());
    }
    rows.extend(query_top_composition_rows(
        "query_structure_element_composition",
        element_counts,
        atom_count,
        5,
    )?);
    let residue_counts = query_count_map(
        structure.get("residue_counts"),
        "residue_counts",
        residue_count,
        512,
    )?;
    if residue_counts.iter().map(|(_, count)| *count).sum::<u64>() != residue_count {
        return Err("query_structure_residue_count_total_mismatch".to_string());
    }
    rows.extend(query_top_composition_rows(
        "query_structure_residue_composition",
        residue_counts,
        residue_count,
        8,
    )?);
    Ok(rows)
}

fn query_count_map(
    value: Option<&Value>,
    label: &str,
    maximum: u64,
    max_keys: usize,
) -> Result<Vec<(String, u64)>, String> {
    let object = value
        .and_then(Value::as_object)
        .filter(|object| !object.is_empty() && object.len() <= max_keys)
        .ok_or_else(|| format!("query_structure_{label}_invalid"))?;
    object
        .iter()
        .map(|(name, count)| {
            let name = query_token_component(name)
                .ok_or_else(|| format!("query_structure_{label}_name_invalid"))?;
            let count = count
                .as_u64()
                .filter(|count| *count > 0 && *count <= maximum)
                .ok_or_else(|| format!("query_structure_{label}_count_invalid"))?;
            Ok((name, count))
        })
        .collect()
}

fn query_top_composition_rows(
    family: &str,
    mut counts: Vec<(String, u64)>,
    total: u64,
    limit: usize,
) -> Result<Vec<FeatureRow>, String> {
    if total == 0 {
        return Err("query_structure_composition_total_invalid".to_string());
    }
    counts.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    Ok(counts
        .into_iter()
        .take(limit)
        .map(|(name, count)| {
            query_binned_feature_row(
                family,
                &format!("{name}_fraction"),
                count as f64 / total as f64,
                0.1,
            )
        })
        .collect())
}

fn query_json_number(
    value: Option<&Value>,
    label: &str,
    minimum: f64,
    maximum: f64,
) -> Result<f64, String> {
    value
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite() && *value >= minimum && *value <= maximum)
        .ok_or_else(|| format!("query_structure_{label}_invalid"))
}

fn query_json_u64(
    value: Option<&Value>,
    label: &str,
    minimum: u64,
    maximum: u64,
) -> Result<u64, String> {
    value
        .and_then(Value::as_u64)
        .filter(|value| *value >= minimum && *value <= maximum)
        .ok_or_else(|| format!("query_structure_{label}_invalid"))
}

fn query_json_xyz(value: Option<&Value>, label: &str) -> Result<[f64; 3], String> {
    let values = value
        .and_then(Value::as_array)
        .filter(|values| values.len() == 3)
        .ok_or_else(|| format!("query_structure_{label}_invalid"))?;
    let mut result = [0.0; 3];
    for (index, value) in values.iter().enumerate() {
        result[index] = value
            .as_f64()
            .filter(|value| value.is_finite() && value.abs() <= 1_000_000.0)
            .ok_or_else(|| format!("query_structure_{label}_invalid"))?;
    }
    Ok(result)
}

fn query_binned_feature_row(family: &str, name: &str, value: f64, width: f64) -> FeatureRow {
    let bin = (value / width).floor() as i64;
    let bin = if bin < 0 {
        format!("neg{}", bin.unsigned_abs())
    } else {
        format!("pos{bin}")
    };
    feature_row(
        family,
        name,
        format!("{value:.6}"),
        &format!(
            "qs_{}_{}_bin_{bin}",
            clean_token_component(family),
            clean_token_component(name)
        ),
    )
}

fn clean_token_component(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect()
}

fn query_token_component(value: &str) -> Option<String> {
    let token = clean_token_component(value).trim_matches('_').to_string();
    (!token.is_empty() && token.len() <= 48).then_some(token)
}

fn decode_lower_hex(value: &str) -> Option<Vec<u8>> {
    if value.len() % 2 != 0
        || value
            .bytes()
            .any(|byte| !byte.is_ascii_hexdigit() || byte.is_ascii_uppercase())
    {
        return None;
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let text = std::str::from_utf8(pair).ok()?;
            u8::from_str_radix(text, 16).ok()
        })
        .collect()
}

fn query_structure_job_id(input_id: &str, record_id: &str, invocation_id: &str) -> String {
    let identity_digest = canonical_json_sha256(&json!({
        "input_id": input_id,
        "record_id": record_id,
        "invocation_id": invocation_id,
    }));
    format!("query_structure_prepare__{}", &identity_digest[..16])
}

fn publish_pending_file(pending: &Path, output: &Path) -> Result<(), String> {
    if output.exists() {
        fs::remove_file(output)
            .map_err(|err| format!("remove_failed:{}:{err}", output.display()))?;
    }
    fs::rename(pending, output).map_err(|err| {
        format!(
            "rename_failed:{}:{}:{err}",
            pending.display(),
            output.display()
        )
    })
}

fn compute_features(options: &ScienceOptions) -> Result<String, String> {
    let rows = read_normalized_inputs(&options.run_dir.join("input_normalized_manifest.tsv"))?;
    if rows.is_empty() {
        return Err("normalized_input_missing_or_empty".to_string());
    }
    let provenance =
        read_conversion_provenance(&options.run_dir.join("input_normalized_manifest.tsv"))?;
    let invocation_id = current_worker_invocation(options)?;
    let query_structures =
        load_validated_query_structure_artifacts(options, &rows, &provenance, &invocation_id)?;
    let mut text = format!("{FEATURE_MANIFEST_HEADER}\n");
    for input in rows {
        let query_structure = query_structures
            .get(&(input.input_id.clone(), input.record_id.clone()))
            .ok_or_else(|| "query_structure_features_missing".to_string())?;
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
        for row in &query_structure.feature_rows {
            text.push_str(&format!(
                "{}\t{}\t{}\t{}\t{}\tquery_structure_worker\t{}\t{}\n",
                input.input_id,
                input.kind.as_str(),
                clean_cell(&row.family),
                clean_cell(&row.name),
                clean_cell(&row.value),
                clean_cell(&row.token),
                clean_cell(&input.record_id),
            ));
        }
        for (name, value) in [
            ("preparation_method", query_structure.method.clone()),
            ("feature_digest", query_structure.feature_digest.clone()),
            (
                "coordinate_available",
                (!query_structure.coordinate_artifact.trim().is_empty()).to_string(),
            ),
            ("preparation_status", query_structure.status.clone()),
        ] {
            text.push_str(&format!(
                "{}\t{}\tquery_structure\t{name}\t{}\tquery_structure_worker\t\t{}\n",
                input.input_id,
                input.kind.as_str(),
                clean_cell(&value),
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
    let output_path = options.run_dir.join("known_database_matches.tsv");
    if let Some(parent) = output_path.parent() {
        ensure_dir(parent)?;
    }
    let file = fs::File::create(&output_path)
        .map_err(|err| format!("create_failed:{}:{err}", output_path.display()))?;
    let mut out = BufWriter::new(file);
    out.write_all(
        b"input_id\tsource\ttarget_id\ttarget_name\torganism\ttarget_class\tmatch_score\tmatched_features\tevidence\tsource_row_provenance\tactivity_value\tligand_identity\tcurrent_run_query_id\trecord_id\n",
    )
    .map_err(|err| format!("write_failed:{}:{err}", output_path.display()))?;
    let known_index = options.science_store.join("indexes/known_targets.tsv");
    for_each_store_row_batch(&known_index, "known_database_index", |rows| {
        for input in &inputs {
            for row in rows {
                let score = known_database_match_score(input, row);
                if score > 0.0 {
                    let matched = matched_tokens(input, row).join(" ");
                    let evidence =
                        bind_database_evidence(options, &row.source, &clean_cell(&row.evidence))?;
                    let source_digest = evidence_field(&evidence, &["source_digest"]);
                    let query_digest = canonical_json_sha256(&json!({
                        "input_id": &input.input_id,
                        "record_id": &input.record_id,
                        "target_id": &row.target_id,
                        "source": &row.source,
                        "source_digest": &source_digest,
                    }));
                    let evidence = format!("{evidence};query_digest={query_digest}");
                    let source_provenance =
                        bind_database_evidence(options, &row.source, "known_database_index")?;
                    writeln!(
                        out,
                        "{}\t{}\t{}\t{}\t{}\t{}\t{score:.4}\t{}\t{}\t{}\t\t{}\t{}\t{}",
                        input.input_id,
                        row.source,
                        row.target_id,
                        clean_cell(&row.target_name),
                        clean_cell(&row.organism),
                        row.target_class,
                        clean_cell(&matched),
                        evidence,
                        source_provenance,
                        clean_cell(&matched),
                        query_digest,
                        clean_cell(&input.record_id),
                    )
                    .map_err(|err| format!("write_failed:{}:{err}", output_path.display()))?;
                }
            }
        }
        Ok(())
    })?;
    if !append_ligand_similarity_matches(options, &mut out)? {
        append_chembl_onsite_matches(options, &normalized, &mut out)?;
        append_bindingdb_onsite_matches(options, &normalized, &mut out)?;
    }
    let mut mmseqs = String::new();
    append_mmseqs_match_rows(options, "known", &mut mmseqs, true)?;
    out.write_all(mmseqs.as_bytes())
        .and_then(|_| out.flush())
        .map_err(|err| format!("write_failed:{}:{err}", output_path.display()))?;
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

fn readiness_requires_chembl_query(science_store: &Path) -> bool {
    read_text(&science_store.join("science_store_readiness.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}')).ok())
        .and_then(|value| {
            value
                .get("evidence_manifest")
                .and_then(Value::as_array)
                .cloned()
        })
        .is_some_and(|rows| {
            rows.iter().any(|row| {
                row.get("path").and_then(Value::as_str) == Some(CHEMBL_QUERY_MANIFEST_REL_PATH)
            })
        })
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ChemblQueryHashIdentity {
    manifest_sha256: String,
    database_path: String,
    database_bytes: u64,
    database_modified_identity: String,
    cache_key: String,
}

fn sha256_bytes_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn chembl_query_modified_identity(metadata: &fs::Metadata) -> Result<String, String> {
    let modified = metadata
        .modified()
        .map_err(|err| format!("chembl_query_database_modified_time_unavailable:{err}"))?;
    Ok(match modified.duration_since(UNIX_EPOCH) {
        Ok(duration) => format!(
            "after_epoch:{}:{:09}",
            duration.as_secs(),
            duration.subsec_nanos()
        ),
        Err(error) => {
            let duration = error.duration();
            format!(
                "before_epoch:{}:{:09}",
                duration.as_secs(),
                duration.subsec_nanos()
            )
        }
    })
}

fn chembl_query_hash_identity(
    database: &Path,
    manifest_sha256: &str,
) -> Result<ChemblQueryHashIdentity, String> {
    let canonical = database
        .canonicalize()
        .map_err(|err| format!("chembl_query_database_canonicalize_failed:{err}"))?;
    let metadata = fs::metadata(&canonical)
        .map_err(|err| format!("chembl_query_database_metadata_failed:{err}"))?;
    let database_path = canonical.to_string_lossy().to_string();
    let database_bytes = metadata.len();
    let database_modified_identity = chembl_query_modified_identity(&metadata)?;
    let cache_key = canonical_json_sha256(&json!({
        "schema_version": CHEMBL_QUERY_HASH_CACHE_SCHEMA_VERSION,
        "manifest_sha256": manifest_sha256,
        "database_path": database_path,
        "database_bytes": database_bytes,
        "database_modified_identity": database_modified_identity,
    }));
    Ok(ChemblQueryHashIdentity {
        manifest_sha256: manifest_sha256.to_string(),
        database_path,
        database_bytes,
        database_modified_identity,
        cache_key,
    })
}

fn chembl_query_hash_cache_path(
    database: &Path,
    identity: &ChemblQueryHashIdentity,
) -> Result<PathBuf, String> {
    let parent = database
        .parent()
        .ok_or_else(|| "chembl_query_database_parent_missing".to_string())?;
    Ok(parent
        .join(CHEMBL_QUERY_HASH_CACHE_DIR)
        .join(format!("{}.json", identity.cache_key)))
}

fn read_chembl_query_hash_cache(
    database: &Path,
    identity: &ChemblQueryHashIdentity,
) -> Option<String> {
    let path = chembl_query_hash_cache_path(database, identity).ok()?;
    let text = fs::read_to_string(path).ok()?;
    let value = serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}')).ok()?;
    let digest = value.get("database_sha256")?.as_str()?;
    if value.get("schema_version").and_then(Value::as_str)
        != Some(CHEMBL_QUERY_HASH_CACHE_SCHEMA_VERSION)
        || value.get("cache_key").and_then(Value::as_str) != Some(identity.cache_key.as_str())
        || value.get("manifest_sha256").and_then(Value::as_str)
            != Some(identity.manifest_sha256.as_str())
        || value.get("database_path").and_then(Value::as_str)
            != Some(identity.database_path.as_str())
        || value.get("database_bytes").and_then(Value::as_u64) != Some(identity.database_bytes)
        || value
            .get("database_modified_identity")
            .and_then(Value::as_str)
            != Some(identity.database_modified_identity.as_str())
        || !valid_sha256_hex(digest)
    {
        return None;
    }
    Some(digest.to_ascii_lowercase())
}

fn write_chembl_query_hash_cache(
    database: &Path,
    identity: &ChemblQueryHashIdentity,
    database_sha256: &str,
) -> Result<(), String> {
    if read_chembl_query_hash_cache(database, identity).as_deref() == Some(database_sha256) {
        return Ok(());
    }
    let path = chembl_query_hash_cache_path(database, identity)?;
    let parent = path
        .parent()
        .ok_or_else(|| "chembl_query_hash_cache_parent_missing".to_string())?;
    ensure_dir(parent)?;
    let sequence = CHEMBL_QUERY_HASH_CACHE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let pending = parent.join(format!(
        ".{}.{}.{}.pending",
        identity.cache_key,
        std::process::id(),
        sequence
    ));
    let payload = json!({
        "schema_version": CHEMBL_QUERY_HASH_CACHE_SCHEMA_VERSION,
        "cache_key": identity.cache_key,
        "manifest_sha256": identity.manifest_sha256,
        "database_path": identity.database_path,
        "database_bytes": identity.database_bytes,
        "database_modified_identity": identity.database_modified_identity,
        "database_sha256": database_sha256,
    })
    .to_string()
        + "\n";
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&pending)
        .map_err(|err| format!("chembl_query_hash_cache_create_failed:{err}"))?;
    if let Err(error) = file
        .write_all(payload.as_bytes())
        .and_then(|_| file.sync_all())
    {
        let _ = fs::remove_file(&pending);
        return Err(format!("chembl_query_hash_cache_write_failed:{error}"));
    }
    drop(file);
    if path.exists() {
        if read_chembl_query_hash_cache(database, identity).as_deref() == Some(database_sha256) {
            let _ = fs::remove_file(&pending);
            return Ok(());
        }
        fs::remove_file(&path)
            .map_err(|err| format!("chembl_query_hash_cache_remove_invalid_failed:{err}"))?;
    }
    if let Err(error) = fs::rename(&pending, &path) {
        if read_chembl_query_hash_cache(database, identity).as_deref() == Some(database_sha256) {
            let _ = fs::remove_file(&pending);
            return Ok(());
        }
        let _ = fs::remove_file(&pending);
        return Err(format!("chembl_query_hash_cache_publish_failed:{error}"));
    }
    Ok(())
}

fn chembl_query_process_hash_cache() -> &'static Mutex<BTreeMap<String, (String, String)>> {
    CHEMBL_QUERY_PROCESS_HASH_CACHE.get_or_init(|| Mutex::new(BTreeMap::new()))
}

#[cfg(test)]
fn record_chembl_query_database_hash(database_path: &str) {
    let counts = CHEMBL_QUERY_TEST_HASH_COUNTS.get_or_init(|| Mutex::new(BTreeMap::new()));
    let mut counts = counts
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *counts.entry(database_path.to_string()).or_default() += 1;
}

#[cfg(test)]
fn reset_chembl_query_database_hash_count(database: &Path) {
    let database_path = database
        .canonicalize()
        .unwrap_or_else(|_| database.to_path_buf())
        .to_string_lossy()
        .to_string();
    let counts = CHEMBL_QUERY_TEST_HASH_COUNTS.get_or_init(|| Mutex::new(BTreeMap::new()));
    counts
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .remove(&database_path);
}

#[cfg(test)]
fn chembl_query_database_hash_count(database: &Path) -> u64 {
    let database_path = database
        .canonicalize()
        .unwrap_or_else(|_| database.to_path_buf())
        .to_string_lossy()
        .to_string();
    let counts = CHEMBL_QUERY_TEST_HASH_COUNTS.get_or_init(|| Mutex::new(BTreeMap::new()));
    counts
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(&database_path)
        .copied()
        .unwrap_or_default()
}

#[cfg(test)]
fn clear_chembl_query_process_hash_cache(database: &Path) {
    let database_path = database
        .canonicalize()
        .unwrap_or_else(|_| database.to_path_buf())
        .to_string_lossy()
        .to_string();
    chembl_query_process_hash_cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .remove(&database_path);
}

fn chembl_query_database_sha256(database: &Path, manifest_sha256: &str) -> Result<String, String> {
    for _ in 0..3 {
        let identity = chembl_query_hash_identity(database, manifest_sha256)?;
        let process_hit = chembl_query_process_hash_cache()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&identity.database_path)
            .filter(|(cache_key, _)| cache_key == &identity.cache_key)
            .map(|(_, digest)| digest.clone());
        if let Some(digest) = process_hit {
            if chembl_query_hash_identity(database, manifest_sha256)? == identity {
                return Ok(digest);
            }
            continue;
        }
        if let Some(digest) = read_chembl_query_hash_cache(database, &identity) {
            if chembl_query_hash_identity(database, manifest_sha256)? != identity {
                continue;
            }
            chembl_query_process_hash_cache()
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .insert(
                    identity.database_path.clone(),
                    (identity.cache_key.clone(), digest.clone()),
                );
            return Ok(digest);
        }
        #[cfg(test)]
        record_chembl_query_database_hash(&identity.database_path);
        let digest = sha256_file_hex(database)
            .ok_or_else(|| "chembl_query_database_sha256_read_failed".to_string())?;
        if chembl_query_hash_identity(database, manifest_sha256)? != identity {
            continue;
        }
        let _ = write_chembl_query_hash_cache(database, &identity, &digest);
        chembl_query_process_hash_cache()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(
                identity.database_path.clone(),
                (identity.cache_key.clone(), digest.clone()),
            );
        return Ok(digest);
    }
    Err("chembl_query_database_changed_during_sha256".to_string())
}

fn validate_chembl_query_runtime_contract(
    science_store: &Path,
    database: &Path,
    manifest: &Path,
) -> Result<(), String> {
    let text = read_text(manifest)
        .map_err(|err| format!("chembl_query_manifest_contract_invalid:read_failed:{err}"))?;
    let value = serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}'))
        .map_err(|err| format!("chembl_query_manifest_contract_invalid:json:{err}"))?;
    let (source_digest, source_release) = science_store_source_identity(science_store, "chembl")?;
    let database_bytes = fs::metadata(database)
        .map_err(|err| format!("chembl_query_manifest_contract_invalid:database_metadata:{err}"))?
        .len();
    let expected_database_sha256 = value
        .get("database_sha256")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let valid = value.get("schema_version").and_then(Value::as_str)
        == Some(CHEMBL_QUERY_MANIFEST_SCHEMA_VERSION)
        && value.get("status").and_then(Value::as_str) == Some("ready")
        && value.get("table").and_then(Value::as_str) == Some("genoligand_onsite_activity")
        && value
            .get("rows")
            .and_then(Value::as_u64)
            .is_some_and(|rows| rows > 0)
        && value.get("database_bytes").and_then(Value::as_u64) == Some(database_bytes)
        && valid_sha256_hex(expected_database_sha256)
        && value.get("source_digest").and_then(Value::as_str) == Some(source_digest.as_str())
        && value.get("source_release").and_then(Value::as_str) == Some(source_release.as_str());
    if !valid {
        return Err("chembl_query_manifest_contract_invalid:fixed_fields_or_identity".to_string());
    }
    let manifest_sha256 = sha256_bytes_hex(text.as_bytes());
    let actual_database_sha256 = chembl_query_database_sha256(database, &manifest_sha256)?;
    if !actual_database_sha256.eq_ignore_ascii_case(expected_database_sha256) {
        return Err("database_sha256_mismatch".to_string());
    }
    Ok(())
}

fn append_chembl_onsite_matches<W: Write>(
    options: &ScienceOptions,
    inputs: &[NormalizedInput],
    out: &mut W,
) -> Result<(), String> {
    let small_molecules = inputs
        .iter()
        .filter(|input| input.kind == InputKind::SmallMolecule)
        .collect::<Vec<_>>();
    if small_molecules.is_empty() {
        return Ok(());
    }
    let source = options
        .science_store
        .join("downloads/chembl/chembl_query.sqlite");
    let manifest = options.science_store.join(CHEMBL_QUERY_MANIFEST_REL_PATH);
    if !source.is_file() || !manifest.is_file() {
        if source.exists()
            || manifest.exists()
            || readiness_requires_chembl_query(&options.science_store)
        {
            return Err(format!(
                "chembl_query_contract_missing:database={}:manifest={}",
                source.display(),
                manifest.display()
            ));
        }
        return Ok(());
    }
    validate_chembl_query_runtime_contract(&options.science_store, &source, &manifest)?;
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
    let result_path = temporary_data_path(&options.run_dir, "chembl_onsite_matches", "tsv");
    let run_id = infer_run_id(&options.run_dir);
    let query_invocation = format!(
        "chembl_query_{}",
        stable_hash(&format!("{run_id}:{}", stable_hash(&queries)))
    );
    let args = vec![
        source.display().to_string(),
        query_path.display().to_string(),
        result_path.display().to_string(),
        run_id,
        query_invocation,
    ];
    let result = run_python_script(chembl_onsite_query_script(), &args, "chembl_onsite_query");
    if let Err(err) = result {
        let _ = fs::remove_file(&result_path);
        return Err(err);
    }
    let copy_result = append_bound_known_rows(options, &result_path, out);
    let _ = fs::remove_file(&result_path);
    copy_result
}

fn chembl_onsite_query_script() -> &'static str {
    r#"
import csv
import hashlib
import sqlite3
import sys
from pathlib import Path

database_path, query_path, result_path, run_id, invocation_id = sys.argv[1:6]

if database_path.lower().endswith((".tar.gz", ".tgz", ".zip")):
    raise RuntimeError("chembl_query_requires_published_sqlite")

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
        out.append({"t": "\t", "n": "\n", "r": "\r", "\\": "\\"}.get(nxt, nxt))
        index += 2
    return "".join(out)

try:
    from rdkit import Chem
except Exception as exc:
    raise RuntimeError("rdkit_required_for_exact_smiles_matching") from exc

def canonical_smiles(value):
    molecule = Chem.MolFromSmiles(clean(value))
    if molecule is None:
        return ""
    return Chem.MolToSmiles(molecule, isomericSmiles=True)

def target_class(organism):
    low = organism.lower()
    return "known_human_protein" if "homo sapiens" in low or "human" in low else "known_protein"

def numeric_activity(value):
    try:
        return float(clean(value).replace(",", ""))
    except ValueError:
        return float("inf")

def activity_score(value, relation):
    if clean(relation) != "=":
        return 0.0
    numeric = numeric_activity(value)
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
    return 0.40 if numeric != float("inf") else 0.0

def format_activity_number(value):
    text = clean(value)
    try:
        numeric = float(text)
    except ValueError:
        return text
    return str(int(numeric)) if numeric.is_integer() else text

queries = []
with open(query_path, "r", encoding="utf-8-sig", newline="") as handle:
    for row in csv.DictReader(handle, delimiter="\t"):
        canonical = canonical_smiles(decode(row.get("query", "")).strip())
        if canonical:
            queries.append((
                clean(row.get("input_id", "input_1")) or "input_1",
                clean(row.get("record_id", "record_1")) or "record_1",
                canonical,
            ))

if not queries:
    Path(result_path).write_text("", encoding="utf-8")
    raise SystemExit(0)

query_by_smiles = {}
for input_id, record_id, canonical in queries:
    query_by_smiles.setdefault(canonical, []).append((input_id, record_id))

placeholders = ",".join("?" for _ in query_by_smiles)
sql = f"""
SELECT canonical_isomeric_smiles, target_id, source_target_id, target_name, organism, compound_name,
       standard_type, standard_relation, standard_value, standard_units,
       source_release, source_digest
FROM genoligand_onsite_activity
WHERE canonical_isomeric_smiles IN ({placeholders})
  AND standard_type IN ('Ki', 'Kd', 'IC50', 'EC50')
  AND (standard_units = '' OR standard_units = 'nM')
"""

uri = Path(database_path).resolve().as_uri() + "?mode=ro&immutable=1"
connection = sqlite3.connect(uri, uri=True)
best = {}
try:
    columns = {
        row[1]
        for row in connection.execute("PRAGMA table_info(genoligand_onsite_activity)")
    }
    legacy_required = {
        "canonical_isomeric_smiles", "target_id", "target_name", "organism",
        "compound_name", "standard_type", "standard_relation", "standard_value",
        "standard_units", "source_release", "source_digest",
    }
    if "source_target_id" not in columns and columns == legacy_required:
        print("chembl_query_legacy_11_column_schema_rejected", file=sys.stderr)
        raise SystemExit(1)
    required = legacy_required | {"source_target_id"}
    if not required.issubset(columns):
        raise RuntimeError("chembl_query_table_schema_invalid")
    indexed = False
    for index_row in connection.execute("PRAGMA index_list(genoligand_onsite_activity)"):
        index_name = index_row[1]
        index_columns = [
            item[2] for item in connection.execute(f"PRAGMA index_info('{index_name}')")
        ]
        if index_columns and index_columns[0] == "canonical_isomeric_smiles":
            indexed = True
            break
    if not indexed:
        raise RuntimeError("chembl_query_canonical_index_missing")
    plan = connection.execute(
        "EXPLAIN QUERY PLAN " + sql, list(query_by_smiles.keys())
    ).fetchall()
    if any(
        "SCAN GENOLIGAND_ONSITE_ACTIVITY" in clean(row[-1]).upper()
        for row in plan
    ):
        raise RuntimeError("chembl_query_plan_full_scan")

    for row in connection.execute(sql, list(query_by_smiles.keys())):
        (
            canonical, target_id, source_target_id, target_name, organism, compound_name,
            standard_type, relation, value, units, source_release, source_digest,
        ) = [clean(item) for item in row]
        if (
            not canonical
            or not target_id
            or not source_target_id
            or not source_release
            or len(source_digest) != 64
            or any(ch not in "0123456789abcdefABCDEF" for ch in source_digest)
        ):
            raise RuntimeError("chembl_query_row_provenance_invalid")
        activity = f"{standard_type}{relation or '?'}{format_activity_number(value)} {units}".strip()
        relation_rank = 0 if relation == "=" else 1
        numeric_rank = numeric_activity(value) if relation_rank == 0 else 1e300
        for input_id, record_id in query_by_smiles.get(canonical, []):
            query_digest = hashlib.sha256(
                "\0".join([
                    run_id, invocation_id, input_id, record_id, canonical,
                    target_id, source_target_id, source_release, source_digest,
                ]).encode("utf-8")
            ).hexdigest()
            matched = (
                f"canonical_smiles={canonical};compound_name={compound_name};"
                f"source_target_id={source_target_id};"
                f"source_release={source_release};source_digest={source_digest}"
            )
            evidence = (
                f"chembl_onsite_activity_row;{activity};relation={relation};"
                f"quantitative={'true' if relation == '=' else 'false'};"
                f"source_target_id={source_target_id};"
                f"source_release={source_release};source_digest={source_digest};"
                f"query_digest={query_digest};run_id={run_id};invocation_id={invocation_id}"
            )
            out_row = [
                input_id,
                "ChEMBL",
                target_id,
                target_name or target_id,
                organism or "unknown",
                target_class(organism),
                f"{activity_score(value, relation):.4f}",
                matched,
                evidence,
                (
                    "chembl_onsite_activity_row;"
                    f"source_database_id={source_target_id};"
                    f"source_target_id={source_target_id};"
                    f"source_release={source_release};source_digest={source_digest}"
                ),
                activity,
                matched,
                query_digest,
                record_id,
            ]
            key = (input_id, record_id, target_id)
            rank = (
                relation_rank,
                numeric_rank,
                target_name.lower(),
                organism.lower(),
                activity,
            )
            current = best.get(key)
            if current is None or rank < current[0]:
                best[key] = (rank, out_row)
finally:
    connection.close()

with open(result_path, "w", encoding="utf-8", newline="") as handle:
    for key in sorted(best):
        handle.write("\t".join(best[key][1]))
        handle.write("\n")
"#
}

fn prepare_bindingdb_query_database(options: &StoreOptions) -> Result<(), String> {
    let source = options.science_store.join(BINDINGDB_SOURCE_REL_PATH);
    let database = options
        .science_store
        .join(BINDINGDB_QUERY_DATABASE_REL_PATH);
    let manifest = options
        .science_store
        .join(BINDINGDB_QUERY_MANIFEST_REL_PATH);
    if !source.is_file() {
        if database.is_file() && manifest.is_file() {
            validate_bindingdb_query_runtime_contract(
                &options.science_store,
                &database,
                &manifest,
            )?;
        } else if database.exists() || manifest.exists() {
            return Err("bindingdb_query_contract_incomplete".to_string());
        }
        return Ok(());
    }

    let (source_digest, source_release) =
        science_store_source_identity(&options.science_store, "bindingdb")?;
    let source_archive_sha256 = sha256_file_hex(&source)
        .ok_or_else(|| "bindingdb_source_sha256_read_failed".to_string())?;
    if database.is_file()
        && manifest.is_file()
        && validate_bindingdb_query_runtime_contract(&options.science_store, &database, &manifest)
            .is_ok()
        && bindingdb_manifest_source_archive_sha256(&manifest).as_deref()
            == Some(source_archive_sha256.as_str())
    {
        return Ok(());
    }
    build_bindingdb_query_database(
        &source,
        &database,
        &manifest,
        &source_digest,
        &source_release,
        &source_archive_sha256,
    )?;
    validate_bindingdb_query_runtime_contract(&options.science_store, &database, &manifest)
}

fn bindingdb_manifest_source_archive_sha256(manifest: &Path) -> Option<String> {
    let text = read_text(manifest).ok()?;
    serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}'))
        .ok()?
        .get("source_archive_sha256")?
        .as_str()
        .map(str::to_ascii_lowercase)
}

fn build_bindingdb_query_database(
    source: &Path,
    database: &Path,
    manifest: &Path,
    source_digest: &str,
    source_release: &str,
    source_archive_sha256: &str,
) -> Result<usize, String> {
    let parent = database
        .parent()
        .ok_or_else(|| "bindingdb_query_database_parent_missing".to_string())?;
    ensure_dir(parent)?;
    let pending_database = temporary_data_path(parent, "bindingdb_query", "sqlite");
    let pending_manifest = temporary_data_path(parent, "bindingdb_query_manifest", "json");
    let cleanup = || {
        let _ = fs::remove_file(&pending_database);
        let _ = fs::remove_file(&pending_manifest);
    };
    let source_bytes = fs::metadata(source)
        .map_err(|err| format!("bindingdb_source_metadata_failed:{err}"))?
        .len();
    let output = match run_python_script(
        bindingdb_query_build_script(),
        &[
            source.display().to_string(),
            pending_database.display().to_string(),
            source_digest.to_string(),
            source_release.to_string(),
            source_archive_sha256.to_string(),
        ],
        "bindingdb_query_build",
    ) {
        Ok(output) => output,
        Err(error) => {
            cleanup();
            return Err(error);
        }
    };
    let summary = output
        .lines()
        .rev()
        .find_map(|line| serde_json::from_str::<Value>(line).ok())
        .ok_or_else(|| {
            cleanup();
            "bindingdb_query_build_result_invalid".to_string()
        })?;
    let rows = summary
        .get("rows")
        .and_then(Value::as_u64)
        .filter(|rows| *rows > 0)
        .ok_or_else(|| {
            cleanup();
            "bindingdb_query_build_empty".to_string()
        })?;
    if sha256_file_hex(source).as_deref() != Some(source_archive_sha256) {
        cleanup();
        return Err("bindingdb_source_changed_during_query_build".to_string());
    }
    OpenOptions::new()
        .write(true)
        .open(&pending_database)
        .and_then(|file| file.sync_all())
        .map_err(|err| {
            cleanup();
            format!("bindingdb_query_database_sync_failed:{err}")
        })?;
    let database_bytes = fs::metadata(&pending_database)
        .map_err(|err| {
            cleanup();
            format!("bindingdb_query_database_metadata_failed:{err}")
        })?
        .len();
    let database_sha256 = sha256_file_hex(&pending_database).ok_or_else(|| {
        cleanup();
        "bindingdb_query_database_sha256_read_failed".to_string()
    })?;
    let payload = serde_json::to_string_pretty(&json!({
        "schema_version": BINDINGDB_QUERY_MANIFEST_SCHEMA_VERSION,
        "status": "ready",
        "table": "genoligand_bindingdb_activity",
        "rows": rows,
        "database_bytes": database_bytes,
        "database_sha256": database_sha256,
        "source_path": BINDINGDB_SOURCE_REL_PATH,
        "source_bytes": source_bytes,
        "source_digest": source_digest,
        "source_release": source_release,
        "source_archive_sha256": source_archive_sha256,
        "builder_version": env!("CARGO_PKG_VERSION"),
        "indexes": [
            "idx_bindingdb_activity_canonical_smiles",
            "idx_bindingdb_activity_compact_smiles"
        ],
    }))
    .map_err(|err| format!("bindingdb_query_manifest_serialize_failed:{err}"))?
        + "\n";
    if let Err(error) = write_text(&pending_manifest, &payload) {
        cleanup();
        return Err(error);
    }
    if let Err(error) = OpenOptions::new()
        .write(true)
        .open(&pending_manifest)
        .and_then(|file| file.sync_all())
    {
        cleanup();
        return Err(format!("bindingdb_query_manifest_sync_failed:{error}"));
    }
    if let Err(error) = publish_pending_file(&pending_database, database) {
        cleanup();
        return Err(error);
    }
    if let Err(error) = publish_pending_file(&pending_manifest, manifest) {
        cleanup();
        return Err(error);
    }
    Ok(rows as usize)
}

fn validate_bindingdb_query_runtime_contract(
    science_store: &Path,
    database: &Path,
    manifest: &Path,
) -> Result<(), String> {
    let text = read_text(manifest)
        .map_err(|err| format!("bindingdb_query_manifest_contract_invalid:read_failed:{err}"))?;
    let value = serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}'))
        .map_err(|err| format!("bindingdb_query_manifest_contract_invalid:json:{err}"))?;
    let (source_digest, source_release) =
        science_store_source_identity(science_store, "bindingdb")?;
    let database_bytes = fs::metadata(database)
        .map_err(|err| {
            format!("bindingdb_query_manifest_contract_invalid:database_metadata:{err}")
        })?
        .len();
    let expected_database_sha256 = value
        .get("database_sha256")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let expected_indexes = [
        "idx_bindingdb_activity_canonical_smiles",
        "idx_bindingdb_activity_compact_smiles",
    ];
    let valid = value.get("schema_version").and_then(Value::as_str)
        == Some(BINDINGDB_QUERY_MANIFEST_SCHEMA_VERSION)
        && value.get("status").and_then(Value::as_str) == Some("ready")
        && value.get("table").and_then(Value::as_str) == Some("genoligand_bindingdb_activity")
        && value
            .get("rows")
            .and_then(Value::as_u64)
            .is_some_and(|rows| rows > 0)
        && value.get("database_bytes").and_then(Value::as_u64) == Some(database_bytes)
        && valid_sha256_hex(expected_database_sha256)
        && value.get("source_path").and_then(Value::as_str) == Some(BINDINGDB_SOURCE_REL_PATH)
        && value
            .get("source_bytes")
            .and_then(Value::as_u64)
            .is_some_and(|bytes| bytes > 0)
        && value.get("source_digest").and_then(Value::as_str) == Some(source_digest.as_str())
        && value.get("source_release").and_then(Value::as_str) == Some(source_release.as_str())
        && value
            .get("source_archive_sha256")
            .and_then(Value::as_str)
            .is_some_and(valid_sha256_hex)
        && value
            .get("builder_version")
            .and_then(Value::as_str)
            .is_some_and(|version| !version.is_empty())
        && value
            .get("indexes")
            .and_then(Value::as_array)
            .is_some_and(|indexes| {
                expected_indexes.iter().all(|expected| {
                    indexes
                        .iter()
                        .any(|value| value.as_str() == Some(*expected))
                })
            });
    if !valid {
        return Err(
            "bindingdb_query_manifest_contract_invalid:fixed_fields_or_identity".to_string(),
        );
    }
    let manifest_sha256 = sha256_bytes_hex(text.as_bytes());
    let actual_database_sha256 = chembl_query_database_sha256(database, &manifest_sha256)
        .map_err(|err| err.replace("chembl_query", "bindingdb_query"))?;
    if !actual_database_sha256.eq_ignore_ascii_case(expected_database_sha256) {
        return Err("bindingdb_query_database_sha256_mismatch".to_string());
    }
    Ok(())
}

fn append_bindingdb_onsite_matches<W: Write>(
    options: &ScienceOptions,
    inputs: &[NormalizedInput],
    out: &mut W,
) -> Result<(), String> {
    let small_molecules = inputs
        .iter()
        .filter(|input| input.kind == InputKind::SmallMolecule)
        .collect::<Vec<_>>();
    if small_molecules.is_empty() {
        return Ok(());
    }
    let bindingdb = options
        .science_store
        .join(BINDINGDB_QUERY_DATABASE_REL_PATH);
    let manifest = options
        .science_store
        .join(BINDINGDB_QUERY_MANIFEST_REL_PATH);
    if !bindingdb.is_file() || !manifest.is_file() {
        if bindingdb.exists()
            || manifest.exists()
            || science_store_source_identity(&options.science_store, "bindingdb").is_ok()
        {
            return Err(format!(
                "bindingdb_query_contract_missing:database={}:manifest={}",
                bindingdb.display(),
                manifest.display()
            ));
        }
        return Ok(());
    }
    validate_bindingdb_query_runtime_contract(&options.science_store, &bindingdb, &manifest)?;
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
    let result_path = temporary_data_path(&options.run_dir, "bindingdb_onsite_matches", "tsv");
    let (source_digest, source_release) =
        science_store_source_identity(&options.science_store, "bindingdb")?;
    let run_id = infer_run_id(&options.run_dir);
    let invocation_id = format!(
        "bindingdb_query_{}",
        stable_hash(&format!("{run_id}:{}", stable_hash(&queries)))
    );
    let result = run_python_script(
        bindingdb_onsite_query_script(),
        &[
            bindingdb.display().to_string(),
            query_path.display().to_string(),
            result_path.display().to_string(),
            source_digest,
            source_release,
            run_id,
            invocation_id,
        ],
        "bindingdb_onsite_query",
    );
    if let Err(err) = result {
        let _ = fs::remove_file(&result_path);
        return Err(err);
    }
    let copy_result = append_bound_known_rows(options, &result_path, out);
    let _ = fs::remove_file(&result_path);
    copy_result
}

fn science_store_source_identity(
    science_store: &Path,
    source_id: &str,
) -> Result<(String, String), String> {
    let rows = read_simple_tsv_rows(&science_store.join("manifests/source_file_manifest.tsv"))?;
    let row = rows
        .iter()
        .find(|row| {
            row.get("source_id").map(String::as_str) == Some(source_id)
                && row.get("status").map(String::as_str) == Some("ready")
        })
        .ok_or_else(|| format!("source_manifest_identity_missing:{source_id}"))?;
    let digest = row
        .get("sha256")
        .filter(|value| valid_sha256_hex(value))
        .cloned()
        .ok_or_else(|| format!("source_manifest_digest_invalid:{source_id}"))?;
    let release = row
        .get("release_identity")
        .filter(|value| !value.trim().is_empty())
        .cloned()
        .ok_or_else(|| format!("source_manifest_release_missing:{source_id}"))?;
    Ok((digest, release))
}

fn canonical_source_id(source: &str) -> Option<&'static str> {
    let normalized = source
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect::<String>();
    match normalized.as_str() {
        "chembl" => Some("chembl"),
        "bindingdb" => Some("bindingdb"),
        "uniprot" | "uniprothumanreviewed" | "uniprotreviewedhumanproteome" => {
            Some("uniprot_human_reviewed")
        }
        "rcsb" | "foldseek" | "rcsbligandbound" | "rcsbligandboundstructures" => {
            Some("rcsb_ligand_bound")
        }
        "biolip" | "biolip2" => Some("biolip2"),
        "plinder" => Some("plinder"),
        "bindingmoad" => Some("binding_moad"),
        "mgnify" | "metagenome" | "metagenomepotential" | "mmseqs" => Some("metagenome_potential"),
        _ => None,
    }
}

fn canonical_source_binding(
    science_store: &Path,
    source_hint: &str,
    digest_hint: &str,
) -> Result<CanonicalSourceBinding, String> {
    let rows = read_simple_tsv_rows(&science_store.join("manifests/source_file_manifest.tsv"))?;
    let canonical_id = canonical_source_id(source_hint);
    let row = rows
        .iter()
        .find(|row| {
            row.get("status").map(String::as_str) == Some("ready")
                && canonical_id.is_some_and(|source_id| {
                    row.get("source_id").map(String::as_str) == Some(source_id)
                })
        })
        .or_else(|| {
            (!digest_hint.is_empty()).then_some(())?;
            let mut matching = rows.iter().filter(|row| {
                row.get("status").map(String::as_str) == Some("ready")
                    && row.get("sha256").map(String::as_str) == Some(digest_hint)
            });
            let row = matching.next()?;
            matching.next().is_none().then_some(row)
        })
        .ok_or_else(|| format!("canonical_source_manifest_binding_missing:{source_hint}"))?;
    let source_id = row
        .get("source_id")
        .filter(|value| !value.trim().is_empty())
        .cloned()
        .ok_or_else(|| "canonical_source_manifest_source_id_missing".to_string())?;
    let source_path = row
        .get("local_path")
        .filter(|value| !value.trim().is_empty())
        .cloned()
        .ok_or_else(|| format!("canonical_source_manifest_path_missing:{source_id}"))?;
    let source_digest = row
        .get("sha256")
        .filter(|value| valid_sha256_hex(value))
        .cloned()
        .ok_or_else(|| format!("canonical_source_manifest_digest_invalid:{source_id}"))?;
    if !digest_hint.is_empty() && digest_hint != source_digest {
        return Err(format!(
            "canonical_source_manifest_digest_mismatch:{source_id}"
        ));
    }
    Ok(CanonicalSourceBinding {
        source_id,
        source_path,
        source_digest,
    })
}

fn current_pipeline_invocation_id(options: &ScienceOptions) -> Result<String, String> {
    let value = read_text(&options.run_dir.join(PIPELINE_INVOCATION_FILE))?
        .trim()
        .to_string();
    if value.is_empty() {
        Err("pipeline_invocation_id_missing".to_string())
    } else {
        Ok(value)
    }
}

fn current_or_start_pipeline_invocation_id(options: &ScienceOptions) -> Result<String, String> {
    current_pipeline_invocation_id(options).or_else(|_| start_pipeline_invocation(options))
}

fn bind_database_evidence(
    options: &ScienceOptions,
    source_hint: &str,
    evidence: &str,
) -> Result<String, String> {
    let existing_digest = evidence_field(evidence, &["source_digest"]);
    let binding = canonical_source_binding(&options.science_store, source_hint, &existing_digest)?;
    let run_id = infer_run_id(&options.run_dir);
    let pipeline_invocation_id = current_or_start_pipeline_invocation_id(options)?;
    let existing_run = evidence_field(evidence, &["run_id"]);
    if !existing_run.is_empty() && existing_run != run_id {
        return Err("database_evidence_run_id_mismatch".to_string());
    }
    let existing_pipeline = evidence_field(evidence, &["pipeline_invocation_id"]);
    if !existing_pipeline.is_empty() && existing_pipeline != pipeline_invocation_id {
        return Err("database_evidence_pipeline_invocation_mismatch".to_string());
    }
    let binding_keys = [
        "source_id",
        "source_path",
        "source_digest",
        "run_id",
        "pipeline_invocation_id",
    ];
    let mut parts = evidence
        .split(';')
        .map(str::trim)
        .filter(|part| {
            part.split_once('=')
                .is_none_or(|(key, _)| !binding_keys.contains(&key.trim()))
        })
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    parts.extend([
        format!("source_id={}", binding.source_id),
        format!("source_path={}", binding.source_path),
        format!("source_digest={}", binding.source_digest),
        format!("run_id={run_id}"),
        format!("pipeline_invocation_id={pipeline_invocation_id}"),
    ]);
    Ok(parts.join(";"))
}

fn append_bound_known_rows<W: Write>(
    options: &ScienceOptions,
    path: &Path,
    out: &mut W,
) -> Result<(), String> {
    let text = read_text(path)?;
    let compact_publication = text
        .lines()
        .any(|line| {
            let cols = split_tsv(line);
            cols.get(8)
                .is_some_and(|cell| compact_publication_marker(cell))
                || cols
                    .get(9)
                    .is_some_and(|cell| compact_publication_marker(cell))
        })
        .then(|| current_ligand_similarity_publication(options))
        .transpose()?;
    let mut bound = String::new();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        if line.starts_with("input_id\tsource\ttarget_id\t") {
            continue;
        }
        let mut cols = split_tsv(line);
        if cols.len() != 14 {
            return Err("database_evidence_row_invalid".to_string());
        }
        if compact_publication_marker(&cols[8]) || compact_publication_marker(&cols[9]) {
            validate_compact_known_row_publication(
                &cols[8],
                &cols[9],
                compact_publication
                    .as_ref()
                    .ok_or_else(|| "ligand_similarity_publication_binding_missing".to_string())?,
            )?;
        }
        cols[8] = bind_database_evidence(options, &cols[1], &cols[8])?;
        cols[9] = bind_database_evidence(options, &cols[1], &cols[9])?;
        bound.push_str(&cols.join("\t"));
        bound.push('\n');
    }
    out.write_all(bound.as_bytes())
        .map_err(|err| format!("database_evidence_append_failed:{err}"))
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct LigandSimilarityPublication {
    database_sha256: String,
    manifest_sha256: String,
    schema_version: String,
    build_identity: String,
}

fn compact_publication_marker(value: &str) -> bool {
    value
        .split(';')
        .map(str::trim)
        .any(|part| part == "ligand_similarity_index")
}

fn validate_compact_known_row_publication(
    evidence: &str,
    provenance: &str,
    expected: &LigandSimilarityPublication,
) -> Result<(), String> {
    let evidence_fields = compact_known_row_publication_fields(evidence)?;
    let provenance_fields = compact_known_row_publication_fields(provenance)?;
    if evidence_fields != provenance_fields {
        return Err("ligand_similarity_publication_binding_mismatch:duplicate_channel".to_string());
    }
    for (name, expected_value) in [
        ("compact_database_sha256", expected.database_sha256.as_str()),
        ("compact_manifest_sha256", expected.manifest_sha256.as_str()),
        ("compact_schema_version", expected.schema_version.as_str()),
        ("compact_build_identity", expected.build_identity.as_str()),
    ] {
        if evidence_fields.get(name).map(String::as_str) != Some(expected_value) {
            return Err(format!(
                "ligand_similarity_publication_binding_mismatch:{name}"
            ));
        }
    }
    Ok(())
}

fn compact_known_row_publication_fields(value: &str) -> Result<BTreeMap<String, String>, String> {
    if !compact_publication_marker(value) {
        return Err("ligand_similarity_publication_marker_missing".to_string());
    }
    let names = [
        "compact_database_sha256",
        "compact_manifest_sha256",
        "compact_schema_version",
        "compact_build_identity",
    ];
    let mut fields = BTreeMap::new();
    for part in value.split(';').map(str::trim) {
        let Some((name, field_value)) = part.split_once('=') else {
            continue;
        };
        let name = name.trim();
        if names.contains(&name) {
            let field_value = field_value.trim();
            if field_value.is_empty()
                || fields
                    .insert(name.to_string(), field_value.to_string())
                    .is_some()
            {
                return Err(format!(
                    "ligand_similarity_publication_binding_invalid:{name}"
                ));
            }
        }
    }
    if fields.len() != names.len() {
        return Err("ligand_similarity_publication_binding_incomplete".to_string());
    }
    Ok(fields)
}

fn current_ligand_similarity_publication(
    options: &ScienceOptions,
) -> Result<LigandSimilarityPublication, String> {
    ligand_similarity_publication(&options.science_store)
}

fn ligand_similarity_publication(
    science_store: &Path,
) -> Result<LigandSimilarityPublication, String> {
    let database = science_store.join(LIGAND_SIMILARITY_DATABASE_REL_PATH);
    let manifest = science_store.join(LIGAND_SIMILARITY_MANIFEST_REL_PATH);
    let manifest_identity = ligand_similarity_publication_file_identity(&manifest)?;
    let manifest_bytes = fs::read(&manifest)
        .map_err(|err| format!("ligand_similarity_publication_manifest_read_failed:{err}"))?;
    if ligand_similarity_publication_file_identity(&manifest)? != manifest_identity {
        return Err("ligand_similarity_publication_manifest_changed".to_string());
    }
    let manifest_value = serde_json::from_slice::<Value>(
        manifest_bytes
            .strip_prefix(&[0xef, 0xbb, 0xbf])
            .unwrap_or(&manifest_bytes),
    )
    .map_err(|err| format!("ligand_similarity_publication_manifest_invalid:{err}"))?;
    let schema_version = manifest_value
        .get("schema_version")
        .and_then(Value::as_str)
        .filter(|value| *value == LIGAND_SIMILARITY_SCHEMA_VERSION)
        .ok_or_else(|| "ligand_similarity_publication_schema_invalid".to_string())?
        .to_string();
    if manifest_value.get("status").and_then(Value::as_str) != Some("ready") {
        return Err("ligand_similarity_publication_status_invalid".to_string());
    }
    let database_sha256 = cached_ligand_similarity_publication_sha256(&database)?;
    let declared_database_sha256 = manifest_value
        .pointer("/database/sha256")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if declared_database_sha256 != database_sha256 {
        return Err("ligand_similarity_publication_database_digest_mismatch".to_string());
    }
    if ligand_similarity_publication_file_identity(&manifest)? != manifest_identity {
        return Err("ligand_similarity_publication_manifest_changed".to_string());
    }
    Ok(LigandSimilarityPublication {
        database_sha256,
        manifest_sha256: sha256_bytes_hex(&manifest_bytes),
        schema_version,
        build_identity: ligand_similarity_compact_build_identity(&manifest_value)?,
    })
}

fn validate_retained_ligand_similarity_publication(science_store: &Path) -> Result<(), String> {
    ligand_similarity_publication(science_store)?;
    let database = science_store.join(LIGAND_SIMILARITY_DATABASE_REL_PATH);
    let manifest = science_store.join(LIGAND_SIMILARITY_MANIFEST_REL_PATH);
    let manifest_bytes = fs::read(&manifest)
        .map_err(|err| format!("ligand_similarity_retained_manifest_read_failed:{err}"))?;
    let manifest_value = serde_json::from_slice::<Value>(
        manifest_bytes
            .strip_prefix(&[0xef, 0xbb, 0xbf])
            .unwrap_or(&manifest_bytes),
    )
    .map_err(|err| format!("ligand_similarity_retained_manifest_invalid:{err}"))?;
    let database_meta = manifest_value
        .get("database")
        .and_then(Value::as_object)
        .ok_or_else(|| "ligand_similarity_retained_database_missing".to_string())?;
    let database_bytes = fs::metadata(&database)
        .map_err(|err| format!("ligand_similarity_retained_database_metadata_failed:{err}"))?
        .len();
    if database_bytes == 0
        || database_meta.get("bytes").and_then(Value::as_u64) != Some(database_bytes)
        || !ligand_similarity_declared_path_matches(
            &database,
            database_meta
                .get("path")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        )
    {
        return Err("ligand_similarity_retained_database_identity_invalid".to_string());
    }
    let inputs = manifest_value
        .get("inputs")
        .and_then(Value::as_object)
        .ok_or_else(|| "ligand_similarity_retained_inputs_missing".to_string())?;
    for source_id in ["chembl", "bindingdb"] {
        let input = inputs
            .get(source_id)
            .and_then(Value::as_object)
            .ok_or_else(|| format!("ligand_similarity_retained_input_missing:{source_id}"))?;
        let (expected_digest, expected_release) =
            science_store_source_identity(science_store, source_id)?;
        if input.get("source_digest").and_then(Value::as_str) != Some(expected_digest.as_str())
            || input.get("source_release").and_then(Value::as_str)
                != Some(expected_release.as_str())
        {
            return Err(format!(
                "ligand_similarity_retained_source_identity_invalid:{source_id}"
            ));
        }
    }
    Ok(())
}

fn ligand_similarity_compact_build_identity(manifest: &Value) -> Result<String, String> {
    let fingerprint = manifest
        .get("fingerprint")
        .and_then(Value::as_object)
        .ok_or_else(|| "ligand_similarity_publication_fingerprint_missing".to_string())?;
    let mut stable_fingerprint = serde_json::Map::new();
    for name in [
        "type",
        "radius",
        "bits",
        "include_chirality",
        "standardization",
        "tautomer_max_tautomers",
        "tautomer_max_transforms",
        "lsh_bands",
        "search_algorithm",
        "bitbound_formula",
        "reference",
        "encoding",
    ] {
        stable_fingerprint.insert(
            name.to_string(),
            fingerprint.get(name).cloned().ok_or_else(|| {
                format!("ligand_similarity_publication_fingerprint_field_missing:{name}")
            })?,
        );
    }
    let inputs = manifest
        .get("inputs")
        .and_then(Value::as_object)
        .ok_or_else(|| "ligand_similarity_publication_inputs_missing".to_string())?;
    let mut sources = serde_json::Map::new();
    for source in ["chembl", "bindingdb"] {
        let input = inputs
            .get(source)
            .and_then(Value::as_object)
            .ok_or_else(|| format!("ligand_similarity_publication_input_missing:{source}"))?;
        let source_digest = input
            .get("source_digest")
            .and_then(Value::as_str)
            .filter(|value| valid_sha256_hex(value))
            .ok_or_else(|| format!("ligand_similarity_publication_digest_invalid:{source}"))?;
        let source_release = input
            .get("source_release")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| format!("ligand_similarity_publication_release_missing:{source}"))?;
        sources.insert(
            source.to_string(),
            json!({
                "source_digest": source_digest,
                "source_release": source_release,
            }),
        );
    }
    let identity = json!({
        "schema_version": manifest.get("schema_version"),
        "fingerprint": Value::Object(stable_fingerprint),
        "sources": Value::Object(sources),
    });
    Ok(canonical_json_sha256(&identity))
}

fn ligand_similarity_publication_file_identity(path: &Path) -> Result<String, String> {
    let canonical = path
        .canonicalize()
        .map_err(|err| format!("ligand_similarity_publication_path_invalid:{err}"))?;
    let metadata = fs::metadata(&canonical)
        .map_err(|err| format!("ligand_similarity_publication_metadata_failed:{err}"))?;
    if !metadata.is_file() {
        return Err("ligand_similarity_publication_file_required".to_string());
    }
    let modified_ns = metadata
        .modified()
        .map_err(|err| format!("ligand_similarity_publication_modified_failed:{err}"))?
        .duration_since(UNIX_EPOCH)
        .map_err(|err| format!("ligand_similarity_publication_modified_invalid:{err}"))?
        .as_nanos();
    Ok(format!(
        "{}\0{}\0{}",
        canonical.to_string_lossy(),
        metadata.len(),
        modified_ns
    ))
}

fn cached_ligand_similarity_publication_sha256(path: &Path) -> Result<String, String> {
    let cache =
        LIGAND_SIMILARITY_PUBLICATION_HASH_CACHE.get_or_init(|| Mutex::new(BTreeMap::new()));
    for _ in 0..3 {
        let identity = ligand_similarity_publication_file_identity(path)?;
        let cached = cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&identity)
            .cloned();
        if let Some(digest) = cached {
            if ligand_similarity_publication_file_identity(path)? == identity {
                return Ok(digest);
            }
            continue;
        }
        let digest = sha256_file_hex(path)
            .ok_or_else(|| "ligand_similarity_publication_hash_failed".to_string())?;
        if ligand_similarity_publication_file_identity(path)? != identity {
            continue;
        }
        let mut entries = cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        while entries.len() >= LIGAND_SIMILARITY_PUBLICATION_HASH_CACHE_LIMIT {
            let Some(first) = entries.keys().next().cloned() else {
                break;
            };
            entries.remove(&first);
        }
        entries.insert(identity, digest.clone());
        return Ok(digest);
    }
    Err("ligand_similarity_publication_changed_during_hash".to_string())
}

fn append_ligand_similarity_matches<W: Write>(
    options: &ScienceOptions,
    out: &mut W,
) -> Result<bool, String> {
    let database = options
        .science_store
        .join(LIGAND_SIMILARITY_DATABASE_REL_PATH);
    let manifest = options
        .science_store
        .join(LIGAND_SIMILARITY_MANIFEST_REL_PATH);
    if !database.exists() && !manifest.exists() {
        return Ok(false);
    }
    if !database.is_file() || !manifest.is_file() {
        return Err(format!(
            "ligand_similarity_index_contract_missing:database={}:manifest={}",
            database.display(),
            manifest.display()
        ));
    }
    let query_manifest = options.run_dir.join("query_structure_manifest.tsv");
    if !query_manifest.is_file() {
        return Err("ligand_similarity_query_structure_manifest_missing".to_string());
    }
    let result_path = temporary_data_path(&options.run_dir, "ligand_similarity_matches", "tsv");
    let python_root = options.workspace_root.join("python");
    let mut errors = Vec::new();
    let mut completed = false;
    for candidate in python_command_candidates() {
        let mut command = Command::new(&candidate[0]);
        command.args(&candidate[1..]);
        command
            .arg("-m")
            .arg("genoligand_workers.ligand_similarity_index")
            .arg("query-targets")
            .arg("--database")
            .arg(python_script_arg(
                &candidate,
                &database.display().to_string(),
            ))
            .arg("--manifest")
            .arg(python_script_arg(
                &candidate,
                &manifest.display().to_string(),
            ))
            .arg("--query-manifest")
            .arg(python_script_arg(
                &candidate,
                &query_manifest.display().to_string(),
            ))
            .arg("--output-tsv")
            .arg(python_script_arg(
                &candidate,
                &result_path.display().to_string(),
            ))
            .arg("--top-k")
            .arg("256")
            .arg("--threshold")
            .arg("0.65");
        if python_root.exists()
            && let Some(joined) = worker_pythonpath_env(&candidate, &python_root)
        {
            command.env("PYTHONPATH", joined);
            if is_wsl_python_wrapper(&candidate) {
                command.env("WSLENV", wsl_env_with_pythonpath());
            }
        }
        match command.output() {
            Ok(output) if output.status.success() && result_path.is_file() => {
                completed = true;
                break;
            }
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let stdout = String::from_utf8_lossy(&output.stdout);
                let detail = stderr
                    .lines()
                    .find(|line| !line.trim().is_empty())
                    .or_else(|| stdout.lines().find(|line| !line.trim().is_empty()))
                    .unwrap_or("ligand_similarity_query_failed");
                errors.push(format!("{}:{detail}", candidate.join(" ")));
            }
            Err(err) => errors.push(format!("{}:{err}", candidate.join(" "))),
        }
    }
    if !completed {
        let _ = fs::remove_file(&result_path);
        return Err(format!(
            "ligand_similarity_query_launch_failed:{}",
            clean_cell(&errors.join(" | "))
        ));
    }
    let result = append_bound_known_rows(options, &result_path, out);
    let _ = fs::remove_file(&result_path);
    result.map(|()| true)
}

fn bindingdb_query_build_script() -> &'static str {
    r#"
import csv
import json
import os
import sqlite3
import sys
import zipfile
from functools import lru_cache

archive_path, database_path, source_digest, source_release, source_archive_sha256 = sys.argv[1:6]

def clean(value):
    return str(value or "").replace("\t", " ").replace("\r", " ").replace("\n", " ").strip()

def first(row, names):
    for name in names:
        value = clean(row.get(name, ""))
        if value:
            return value
    return ""

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
        return float("".join(number)) if number else 1e300
    except ValueError:
        return 1e300

def activity_relation(value):
    text = clean(value).lstrip()
    for relation in ("<=", ">=", "<", ">", "~"):
        if text.startswith(relation):
            return relation
    return "="

def activity_score(value):
    if activity_relation(value) != "=":
        return 0.0
    numeric = numeric_activity(value)
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
    return 0.40 if numeric < 1e300 else 0.0

try:
    from rdkit import Chem
except Exception as exc:
    raise RuntimeError("rdkit_required_for_bindingdb_query_index") from exc

@lru_cache(maxsize=100000)
def canonical_smiles(value):
    molecule = Chem.MolFromSmiles(clean(value))
    if molecule is None:
        return ""
    return Chem.MolToSmiles(molecule, isomericSmiles=True)

if os.path.exists(database_path):
    os.remove(database_path)
connection = sqlite3.connect(database_path)
try:
    connection.execute("PRAGMA journal_mode=OFF")
    connection.execute("PRAGMA synchronous=OFF")
    connection.execute("PRAGMA temp_store=FILE")
    connection.execute("""
        CREATE TABLE genoligand_bindingdb_activity (
            source_row_id INTEGER PRIMARY KEY,
            canonical_smiles TEXT NOT NULL,
            compact_smiles TEXT NOT NULL,
            ligand_smiles TEXT NOT NULL,
            ligand_name TEXT NOT NULL,
            ligand_name_lower TEXT NOT NULL,
            target_id TEXT NOT NULL,
            target_name TEXT NOT NULL,
            organism TEXT NOT NULL,
            ligand_code TEXT NOT NULL,
            pdb_ids TEXT NOT NULL,
            target_chain_pdb_ids TEXT NOT NULL,
            activity TEXT NOT NULL,
            activity_type TEXT NOT NULL,
            relation TEXT NOT NULL,
            numeric_rank REAL NOT NULL,
            relation_rank INTEGER NOT NULL,
            activity_score REAL NOT NULL,
            pubchem_cid TEXT NOT NULL
        )
    """)
    connection.execute("""
        CREATE TABLE genoligand_bindingdb_metadata (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        )
    """)
    insert = """
        INSERT INTO genoligand_bindingdb_activity (
            canonical_smiles, compact_smiles, ligand_smiles, ligand_name,
            ligand_name_lower, target_id, target_name, organism, ligand_code,
            pdb_ids, target_chain_pdb_ids, activity, activity_type, relation,
            numeric_rank, relation_rank, activity_score, pubchem_cid
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
    """
    pending = []
    rows = 0
    with zipfile.ZipFile(archive_path) as archive:
        members = sorted(
            (name for name in archive.namelist() if name.lower().endswith((".tsv", ".txt"))),
            key=str.lower,
        )
        if not members:
            raise RuntimeError("bindingdb_query_source_members_missing")
        for member_name in members:
            with archive.open(member_name) as raw:
                text = (line.decode("utf-8", "replace") for line in raw)
                for row in csv.DictReader(text, delimiter="\t"):
                    smiles = first(row, ["Ligand SMILES", "SMILES"])
                    ligand_name = first(row, ["BindingDB Ligand Name", "Ligand Name", "Compound Name"])
                    target_name = first(row, ["Target Name", "UniProt (SwissProt) Recommended Name of Target Chain 1"])
                    organism = first(row, ["Target Source Organism According to Curator or DataSource", "Organism"])
                    target_id = first(row, [
                        "UniProt (SwissProt) Primary ID of Target Chain 1",
                        "UniProt (TrEMBL) Primary ID of Target Chain 1",
                        "BindingDB MonomerID",
                        "Target Name",
                    ])
                    if not target_id:
                        continue
                    if not target_name:
                        target_name = target_id
                    activity_names = ["Ki (nM)", "IC50 (nM)", "Kd (nM)", "EC50 (nM)"]
                    activity = first(row, activity_names)
                    activity_type = next(
                        (name for name in activity_names if clean(row.get(name, ""))),
                        "activity",
                    )
                    relation = activity_relation(activity)
                    pending.append((
                        canonical_smiles(smiles),
                        "".join(smiles.split()).lower(),
                        smiles,
                        ligand_name,
                        ligand_name.lower(),
                        target_id,
                        target_name,
                        organism,
                        first(row, ["Ligand HET ID in PDB"]),
                        first(row, ["PDB ID(s) for Ligand-Target Complex"]),
                        first(row, ["PDB ID(s) of Target Chain 1"]),
                        activity,
                        activity_type,
                        relation,
                        numeric_activity(activity) if relation == "=" else 1e300,
                        0 if relation == "=" else 1,
                        activity_score(activity),
                        first(row, ["PubChem CID"]),
                    ))
                    rows += 1
                    if len(pending) >= 5000:
                        connection.executemany(insert, pending)
                        connection.commit()
                        pending.clear()
    if pending:
        connection.executemany(insert, pending)
    if rows == 0:
        raise RuntimeError("bindingdb_query_index_empty")
    connection.execute(
        "CREATE INDEX idx_bindingdb_activity_canonical_smiles "
        "ON genoligand_bindingdb_activity(canonical_smiles)"
    )
    connection.execute(
        "CREATE INDEX idx_bindingdb_activity_compact_smiles "
        "ON genoligand_bindingdb_activity(compact_smiles)"
    )
    metadata = {
        "schema_version": "genoligand_bindingdb_query_database_v1",
        "source_digest": source_digest,
        "source_release": source_release,
        "source_archive_sha256": source_archive_sha256,
        "rows": str(rows),
    }
    connection.executemany(
        "INSERT INTO genoligand_bindingdb_metadata(key, value) VALUES (?, ?)",
        sorted(metadata.items()),
    )
    connection.commit()
    if connection.execute("PRAGMA quick_check").fetchone()[0] != "ok":
        raise RuntimeError("bindingdb_query_database_integrity_failed")
finally:
    connection.close()

print(json.dumps({"rows": rows}, sort_keys=True))
"#
}

fn bindingdb_onsite_query_script() -> &'static str {
    r#"
import csv
import hashlib
import os
import sqlite3
import sys
import tempfile
from pathlib import Path

database_path, query_path, result_path, source_digest, source_release, run_id, invocation_id = sys.argv[1:8]

if database_path.lower().endswith((".zip", ".gz", ".tsv", ".txt")):
    raise RuntimeError("bindingdb_query_requires_published_sqlite")

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

def target_class(organism):
    low = organism.lower()
    if "homo sapiens" in low or "human" in low:
        return "known_human_protein"
    return "known_protein"

try:
    from rdkit import Chem
except Exception as exc:
    raise RuntimeError("rdkit_required_for_exact_smiles_matching") from exc

def canonical_smiles(value):
    text = clean(value)
    if not text:
        return ""
    if text in canonical_cache:
        return canonical_cache[text]
    mol = Chem.MolFromSmiles(text)
    if mol is None:
        canonical_cache[text] = ""
    else:
        canonical_cache[text] = Chem.MolToSmiles(mol, isomericSmiles=True)
    return canonical_cache[text]

canonical_cache = {}

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
            ))

if not queries:
    open(result_path, "w", encoding="utf-8").close()
    raise SystemExit(0)

tmp_dir = tempfile.TemporaryDirectory(prefix="genoligand_bindingdb_onsite_")
best_connection = sqlite3.connect(os.path.join(tmp_dir.name, "best.sqlite"))
try:
    best_connection.execute("""
        CREATE TABLE best (
            input_id TEXT NOT NULL,
            record_id TEXT NOT NULL,
            target_id TEXT NOT NULL,
            relation_rank INTEGER NOT NULL,
            numeric_rank REAL NOT NULL,
            tie_rank TEXT NOT NULL,
            row_text TEXT NOT NULL,
            PRIMARY KEY (input_id, record_id, target_id)
        )
    """)
    upsert = """
        INSERT INTO best VALUES (?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(input_id, record_id, target_id) DO UPDATE SET
            relation_rank=excluded.relation_rank,
            numeric_rank=excluded.numeric_rank,
            tie_rank=excluded.tie_rank,
            row_text=excluded.row_text
        WHERE excluded.relation_rank < best.relation_rank
           OR (excluded.relation_rank = best.relation_rank AND excluded.numeric_rank < best.numeric_rank)
           OR (excluded.relation_rank = best.relation_rank AND excluded.numeric_rank = best.numeric_rank AND excluded.tie_rank < best.tie_rank)
    """
    columns_sql = """
        SELECT ligand_smiles, ligand_name, target_id, target_name, organism,
               ligand_code, pdb_ids, target_chain_pdb_ids, activity,
               activity_type, relation, numeric_rank, relation_rank,
               activity_score, pubchem_cid
        FROM genoligand_bindingdb_activity
    """
    canonical_sql = columns_sql + " WHERE canonical_smiles = ? ORDER BY source_row_id"
    fallback_sql = columns_sql + (
        " WHERE compact_smiles = ? OR instr(ligand_name_lower, ?) > 0"
        " ORDER BY source_row_id"
    )
    uri = Path(database_path).resolve().as_uri() + "?mode=ro&immutable=1"
    source_connection = sqlite3.connect(uri, uri=True)
    try:
        metadata = dict(source_connection.execute(
            "SELECT key, value FROM genoligand_bindingdb_metadata"
        ))
        if (
            metadata.get("schema_version") != "genoligand_bindingdb_query_database_v1"
            or metadata.get("source_digest") != source_digest
            or metadata.get("source_release") != source_release
        ):
            raise RuntimeError("bindingdb_query_metadata_invalid")
        indexes = {
            row[1]
            for row in source_connection.execute(
                "PRAGMA index_list(genoligand_bindingdb_activity)"
            )
        }
        required_indexes = {
            "idx_bindingdb_activity_canonical_smiles",
            "idx_bindingdb_activity_compact_smiles",
        }
        if not required_indexes.issubset(indexes):
            raise RuntimeError("bindingdb_query_indexes_missing")
        for query_info in queries:
            input_id, record_id, query, query_canonical = query_info
            query_value = query_canonical or query
            compact = "".join(query.split()).lower()
            if query_canonical:
                plan = source_connection.execute(
                    "EXPLAIN QUERY PLAN " + canonical_sql,
                    (query_canonical,),
                ).fetchall()
                if any(
                    "SCAN GENOLIGAND_BINDINGDB_ACTIVITY" in clean(row[-1]).upper()
                    for row in plan
                ):
                    raise RuntimeError("bindingdb_query_plan_full_scan")
                source_rows = source_connection.execute(canonical_sql, (query_canonical,))
            else:
                source_rows = source_connection.execute(
                    fallback_sql,
                    (compact, query.lower()),
                )
            for source_row in source_rows:
                (
                    smiles, ligand_name, target_id, target_name, organism,
                    ligand_code, ligand_complex_pdb_ids, target_chain_pdb_ids,
                    activity, activity_type, relation, numeric_rank,
                    relation_rank, score, pubchem,
                ) = source_row
                values = [
                    smiles, ligand_name, target_id, target_name, organism,
                    ligand_code, ligand_complex_pdb_ids, target_chain_pdb_ids,
                    activity, activity_type, relation, pubchem,
                ]
                (
                    smiles, ligand_name, target_id, target_name, organism,
                    ligand_code, ligand_complex_pdb_ids, target_chain_pdb_ids,
                    activity, activity_type, relation, pubchem,
                ) = [clean(value) for value in values]
                matched = f"ligand_name={ligand_name};ligand_smiles={smiles};ligand_code={ligand_code};pdb_ids={ligand_complex_pdb_ids};target_chain_pdb_ids={target_chain_pdb_ids}"
                activity_value = f"{activity_type}={activity}".strip()
                ligand_identity = f"ligand_smiles={smiles};ligand_name={ligand_name};ligand_code={ligand_code}"
                query_digest = hashlib.sha256(
                    "\0".join([
                        run_id, invocation_id, input_id, record_id, query_value,
                        source_release, source_digest,
                    ]).encode("utf-8")
                ).hexdigest()
                evidence = f"bindingdb_onsite_activity_row;{activity_value};relation={relation};quantitative={'true' if relation == '=' else 'false'};ligand_code={ligand_code};pdb_ids={ligand_complex_pdb_ids};target_chain_pdb_ids={target_chain_pdb_ids};pubchem_cid={pubchem};source_release={source_release};source_digest={source_digest};query_digest={query_digest};run_id={run_id};invocation_id={invocation_id}"
                tie_rank = f"{target_name.lower()}\x1f{organism.lower()}\x1f{activity_value}\x1f{record_id}"
                out_row = [
                    input_id,
                    "BindingDB",
                    target_id,
                    target_name,
                    organism or "unknown",
                    target_class(organism),
                    f"{float(score):.4f}",
                    clean(matched),
                    clean(evidence),
                    f"bindingdb_onsite_activity_row;source_release={source_release};source_digest={source_digest}",
                    clean(activity_value),
                    clean(ligand_identity),
                    query_digest,
                    record_id,
                ]
                best_connection.execute(
                    upsert,
                    (
                        input_id,
                        record_id,
                        target_id,
                        int(relation_rank),
                        float(numeric_rank),
                        tie_rank,
                        "\t".join(out_row),
                    ),
                )
        best_connection.commit()
    finally:
        source_connection.close()
    with open(result_path, "w", encoding="utf-8", newline="") as handle:
        for (row_text,) in best_connection.execute(
            "SELECT row_text FROM best ORDER BY input_id, record_id, relation_rank, numeric_rank, target_id, tie_rank"
        ):
            handle.write(row_text)
            handle.write("\n")
finally:
    best_connection.close()
    tmp_dir.cleanup()
"#
}

fn match_metagenome(options: &ScienceOptions) -> Result<String, String> {
    let inputs = read_computed_feature_inputs(&options.run_dir.join("feature_manifest.tsv"))?;
    let rows = Vec::<StoreRow>::new();
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
    current_worker_invocation(options)?;
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
    let copied_structure =
        copy_tool_output_if_available(options, "structure_patch_summary.tsv", &structure)?;
    let mut structure_ready = if copied_structure {
        true
    } else {
        write_structure_summary_from_matches(&structure, &structure_matches)?;
        !structure_matches.is_empty()
    };
    run_single_worker_task(options, &STRUCTURE_ALIGNMENT_WORKER_TASK)?;
    if append_tool_match_rows_to_file(
        options,
        &options.run_dir.join("structure_interaction_matches.tsv"),
        "foldseek_structure_matches.tsv",
    )? > 0
    {
        structure_ready = true;
    }
    let final_structure_matches =
        read_index_matches(&options.run_dir.join("structure_interaction_matches.tsv"))?;
    if !copied_structure {
        write_structure_summary_from_matches(&structure, &final_structure_matches)?;
    }
    structure_ready |= !final_structure_matches.is_empty();
    Ok(format!(
        "structure_evaluated\tstructure_artifact={}\tmatch_count={}",
        if structure_ready {
            "ready"
        } else {
            "not_ready"
        },
        final_structure_matches.len(),
    ))
}

const CANDIDATE_SHORTLIST_HEADER: &str = "shortlist_rank\tcandidate_id\tidentity_namespace\tinput_id\trecord_id\ttarget_id\ttarget_name\torganism\ttarget_class\tcandidate_scope\tsource\tpreliminary_score\tknown_database_score\tmetagenome_score\tstructure_score\tshortlist_reason\tpdb_ids\tcoordinate_status\tselection_rank\treceptor_artifact\tcoordinate_sha256\tpdb_id\tinput_kind\tranking_policy\tpreliminary_known_database_weight\tpreliminary_metagenome_weight\tpreliminary_structure_weight\tknown_database_weight\tmetagenome_weight\tstructure_weight\tdocking_weight\tmd_weight";

fn ligand_similarity_query_id(
    input_id: &str,
    record_id: &str,
    feature_digest: &str,
    source: &str,
    target_id: &str,
    canonical_smiles: &str,
) -> String {
    let mut hasher = Sha256::new();
    for (index, value) in [
        input_id,
        record_id,
        feature_digest,
        source,
        target_id,
        canonical_smiles,
    ]
    .iter()
    .enumerate()
    {
        if index > 0 {
            hasher.update(b"\0");
        }
        hasher.update(value.as_bytes());
    }
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn validate_ligand_similarity_query_bindings(options: &ScienceOptions) -> Result<(), String> {
    let path = options.run_dir.join("known_database_matches.tsv");
    if !path.is_file() {
        return Ok(());
    }
    let rows = read_simple_tsv_rows(&path)?;
    let compact_rows = rows
        .iter()
        .enumerate()
        .filter(|(_, row)| {
            ["evidence", "source_row_provenance"]
                .iter()
                .filter_map(|field| row.get(*field))
                .flat_map(|value| value.split(';'))
                .any(|marker| marker.trim() == "ligand_similarity_index")
        })
        .collect::<Vec<_>>();
    if compact_rows.is_empty() {
        return Ok(());
    }
    let current_publication = current_ligand_similarity_publication(options)?;

    let normalized_path = options.run_dir.join("input_normalized_manifest.tsv");
    let inputs = read_normalized_inputs(&normalized_path)?;
    let provenance = read_conversion_provenance(&normalized_path)?;
    let invocation_path = options.run_dir.join(WORKER_INVOCATION_FILE);
    if !invocation_path.is_file() {
        return Err("ligand_similarity_query_invocation_missing".to_string());
    }
    let invocation_id = read_text(&invocation_path)?.trim().to_string();
    if invocation_id.is_empty() {
        return Err("ligand_similarity_query_invocation_missing".to_string());
    }
    let artifacts =
        load_validated_query_structure_artifacts(options, &inputs, &provenance, &invocation_id)?;

    for (line_index, row) in compact_rows {
        let value = |field: &str| row.get(field).map(String::as_str).unwrap_or("").trim();
        validate_compact_known_row_publication(
            value("evidence"),
            value("source_row_provenance"),
            &current_publication,
        )
        .map_err(|err| {
            format!(
                "ligand_similarity_query_binding_mismatch:line={}:publication:{err}",
                line_index + 2
            )
        })?;
        let input_id = value("input_id");
        let record_id = value("record_id");
        let artifact = artifacts
            .get(&(input_id.to_string(), record_id.to_string()))
            .ok_or_else(|| {
                format!(
                    "ligand_similarity_query_binding_mismatch:line={}:identity",
                    line_index + 2
                )
            })?;
        let evidence = value("evidence");
        let feature_digest = evidence_field(evidence, &["query_feature_digest"]);
        if artifact.status != "ready"
            || !valid_sha256_hex(&feature_digest)
            || !feature_digest.eq_ignore_ascii_case(&artifact.feature_digest)
        {
            return Err(format!(
                "ligand_similarity_query_binding_mismatch:line={}:feature_digest",
                line_index + 2
            ));
        }
        let canonical_smiles = evidence_field(value("matched_features"), &["canonical_smiles"]);
        let source = value("source");
        let target_id = value("target_id");
        let query_id = value("current_run_query_id");
        if canonical_smiles.is_empty() || source.is_empty() || target_id.is_empty() {
            return Err(format!(
                "ligand_similarity_query_binding_mismatch:line={}:query_identity",
                line_index + 2
            ));
        }
        let expected_query_id = ligand_similarity_query_id(
            input_id,
            record_id,
            &artifact.feature_digest,
            source,
            target_id,
            &canonical_smiles,
        );
        if query_id != expected_query_id {
            return Err(format!(
                "ligand_similarity_query_binding_mismatch:line={}:query_id",
                line_index + 2
            ));
        }
    }
    Ok(())
}

fn coarse_rank_candidates(options: &ScienceOptions) -> Result<String, String> {
    let candidate_count = write_candidate_shortlist(options)?;
    let structure_matches =
        read_index_matches(&options.run_dir.join("structure_interaction_matches.tsv"))?;
    let viewer_ready = write_coordinate_backed_candidate_selection(options, &structure_matches)?
        || copy_structure_viewer_artifact_if_available(options)?;
    let selected_count = selected_candidate_rows(options)?.len();
    Ok(format!(
        "candidates_coarse_ranked\tcandidate_count={candidate_count}\tcoordinate_candidate_count={selected_count}\tviewer_artifact={}",
        if viewer_ready { "ready" } else { "not_ready" },
    ))
}

fn write_candidate_shortlist(options: &ScienceOptions) -> Result<usize, String> {
    validate_ligand_similarity_query_bindings(options)?;
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
    assign_collision_free_candidate_ids(&mut candidates);
    bind_candidate_ranking_routes(options, &mut candidates)?;
    candidates.sort_by(|left, right| {
        left.input_id
            .cmp(&right.input_id)
            .then_with(|| left.record_id.cmp(&right.record_id))
            .then_with(|| preliminary_score(right).total_cmp(&preliminary_score(left)))
            .then_with(|| right.known_score.total_cmp(&left.known_score))
            .then_with(|| right.metagenome_score.total_cmp(&left.metagenome_score))
            .then_with(|| right.structure_score.total_cmp(&left.structure_score))
            .then_with(|| right.metadata_quality.cmp(&left.metadata_quality))
            .then_with(|| left.identity_namespace.cmp(&right.identity_namespace))
            .then_with(|| left.target_id.cmp(&right.target_id))
            .then_with(|| left.candidate_id.cmp(&right.candidate_id))
    });

    let mut text = format!("{CANDIDATE_SHORTLIST_HEADER}\n");
    let mut ranks = BTreeMap::<(String, String), usize>::new();
    for candidate in &candidates {
        let route = ranking_route(&candidate.input_kind);
        let (preliminary_known, preliminary_metagenome, preliminary_structure) =
            route.preliminary_weights();
        let rank = ranks
            .entry((candidate.input_id.clone(), candidate.record_id.clone()))
            .and_modify(|value| *value += 1)
            .or_insert(1);
        let reason = preliminary_reason(candidate);
        text.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{:.4}\t{:.4}\t{:.4}\t{:.4}\t{}\t{}\tnot_prepared\t\t\t\t\t{}\t{}\t{:.4}\t{:.4}\t{:.4}\t{:.4}\t{:.4}\t{:.4}\t{:.4}\t{:.4}\n",
            *rank,
            clean_cell(&candidate.candidate_id),
            clean_cell(&candidate.identity_namespace),
            clean_cell(&candidate.input_id),
            clean_cell(&candidate.record_id),
            clean_cell(&candidate.target_id),
            clean_cell(&candidate.target_name),
            clean_cell(&candidate.organism),
            clean_cell(&candidate.target_class),
            clean_cell(&candidate.candidate_scope),
            clean_cell(&candidate.sources.iter().cloned().collect::<Vec<_>>().join(",")),
            preliminary_score(candidate),
            candidate.known_score,
            candidate.metagenome_score,
            candidate.structure_score,
            clean_cell(&reason),
            clean_cell(&candidate.pdb_ids.iter().cloned().collect::<Vec<_>>().join(",")),
            clean_cell(&candidate.input_kind),
            clean_cell(&candidate.ranking_policy),
            preliminary_known,
            preliminary_metagenome,
            preliminary_structure,
            route.known,
            route.metagenome,
            route.structure,
            route.docking,
            route.md,
        ));
    }
    write_text(&options.run_dir.join("candidate_shortlist.tsv"), &text)?;
    Ok(candidates.len())
}

fn preliminary_score(candidate: &Candidate) -> f64 {
    ranking_route(&candidate.input_kind).preliminary_score(candidate)
}

fn preliminary_reason(candidate: &Candidate) -> String {
    let mut evidence = Vec::new();
    if candidate.known_score > 0.0 {
        evidence.push(format!("known_database={:.4}", candidate.known_score));
    }
    if candidate.metagenome_score > 0.0 {
        evidence.push(format!("metagenome={:.4}", candidate.metagenome_score));
    }
    if candidate.structure_score > 0.0 {
        evidence.push(format!("structure={:.4}", candidate.structure_score));
    }
    if evidence.is_empty() {
        "no_positive_preliminary_evidence".to_string()
    } else {
        evidence.join(";")
    }
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
    if tool_output_name == "foldseek_structure_matches.tsv" {
        return append_foldseek_match_rows(options, out);
    }
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

fn append_foldseek_match_rows(options: &ScienceOptions, out: &mut String) -> Result<usize, String> {
    let path = options
        .run_dir
        .join("tool_outputs/foldseek_structure_matches.tsv");
    if !path.is_file() {
        return Ok(0);
    }
    let text = read_text(&path)?;
    let mut lines = text.lines();
    let header = lines
        .next()
        .ok_or_else(|| "foldseek_provenance_header_missing".to_string())?;
    let headers = split_tsv(header);
    let required = |name: &str| {
        find_header(&headers, &[name])
            .ok_or_else(|| format!("foldseek_provenance_header_missing:{name}"))
    };
    let input_idx = required("input_id")?;
    let source_idx = required("source")?;
    let target_idx = required("target_id")?;
    let target_name_idx = required("target_name")?;
    let organism_idx = required("organism")?;
    let class_idx = required("target_class")?;
    let score_idx = required("match_score")?;
    let matched_idx = required("matched_features")?;
    let evidence_idx = required("evidence")?;
    let record_idx = required("record_id")?;
    let database_idx = required("source_database_id")?;
    let source_digest_idx = required("source_digest")?;
    let builder_idx = required("builder_version")?;
    let version_idx = required("foldseek_version")?;
    let query_idx = required("current_run_query_id")?;
    let query_digest_idx = required("query_digest")?;
    let tm_idx = required("tm_score")?;
    let current_inputs = current_run_input_identities(options)?;
    let mut count = 0usize;
    for (line_index, line) in lines.enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let cols = split_tsv(line);
        let value = |index: usize| cols.get(index).map(String::as_str).unwrap_or("").trim();
        let input_id = value(input_idx);
        let record_id = value(record_idx);
        let evidence = value(evidence_idx);
        let source_digest = value(source_digest_idx);
        let query_digest = value(query_digest_idx);
        if !current_inputs.contains(&(input_id.to_string(), record_id.to_string()))
            || value(database_idx).is_empty()
            || !valid_sha256_hex(source_digest)
            || value(builder_idx).is_empty()
            || value(version_idx).is_empty()
            || value(query_idx).is_empty()
            || !valid_sha256_hex(query_digest)
        {
            return Err(format!(
                "foldseek_provenance_invalid:line={}",
                line_index + 2
            ));
        }
        let parse_finite = |name: &str, raw: &str| {
            raw.parse::<f64>()
                .ok()
                .filter(|number| number.is_finite())
                .ok_or_else(|| format!("foldseek_{name}_invalid:line={}", line_index + 2))
        };
        let tm_score = parse_finite("tm_score", value(tm_idx))?;
        let match_score = parse_finite("match_score", value(score_idx))?;
        let pident = parse_finite("pident", &evidence_field(evidence, &["pident"]))?;
        let bits = parse_finite("bits", &evidence_field(evidence, &["bits"]))?;
        if !(0.0..=1.0).contains(&tm_score) || !(0.0..=1.0).contains(&match_score) {
            return Err(format!(
                "foldseek_score_out_of_range:line={}",
                line_index + 2
            ));
        }
        let alignment_score = (0.7 * (pident / 100.0).clamp(0.0, 1.0)
            + 0.3 * (bits / 200.0).clamp(0.0, 1.0))
        .clamp(0.01, 1.0);
        let expected_score = (0.4 * alignment_score + 0.6 * tm_score).clamp(0.0, 1.0);
        if (match_score - expected_score).abs() > 0.0002 {
            return Err(format!(
                "foldseek_tm_score_not_reflected:line={}",
                line_index + 2
            ));
        }
        let evidence = bind_database_evidence(options, value(source_idx), evidence)?;
        out.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{:.4}\t{}\t{}\t{}\t{}\t{}\n",
            clean_cell(input_id),
            clean_cell(value(source_idx)),
            clean_cell(value(target_idx)),
            clean_cell(value(target_name_idx)),
            clean_cell(value(organism_idx)),
            clean_cell(value(class_idx)),
            match_score,
            clean_cell(value(matched_idx)),
            clean_cell(&evidence),
            clean_cell(value(database_idx)),
            clean_cell(value(query_idx)),
            clean_cell(record_id),
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
        let evidence = bind_database_evidence(
            options,
            &source_namespace,
            &format!(
                "{};source_digest={};builder_version={};mmseqs_version={}",
                clean_cell(&cols[8]),
                clean_cell(&source_digest),
                clean_cell(&builder_version),
                clean_cell(&mmseqs_version)
            ),
        )?;
        if known_output {
            let provenance = bind_database_evidence(
                options,
                &source_namespace,
                &format!(
                    "mmseqs_database={};source_namespace={};source_class={};source_digest={};builder_version={};mmseqs_version={}",
                    clean_cell(&source_database_id),
                    clean_cell(&source_namespace),
                    source_class,
                    clean_cell(&source_digest),
                    clean_cell(&builder_version),
                    clean_cell(&mmseqs_version)
                ),
            )?;
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

fn start_worker_invocation(options: &ScienceOptions) -> Result<String, String> {
    ensure_dir(&options.run_dir)?;
    let sequence = WORKER_INVOCATION_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(options.run_dir.display().to_string().as_bytes());
    hasher.update(std::process::id().to_le_bytes());
    hasher.update(timestamp.to_le_bytes());
    hasher.update(sequence.to_le_bytes());
    let suffix = hasher
        .finalize()
        .iter()
        .take(12)
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let invocation_id = format!("invocation_{suffix}");
    write_text(
        &options.run_dir.join(WORKER_INVOCATION_FILE),
        &(invocation_id.clone() + "\n"),
    )?;
    Ok(invocation_id)
}

fn current_worker_invocation(options: &ScienceOptions) -> Result<String, String> {
    let path = options.run_dir.join(WORKER_INVOCATION_FILE);
    if path.is_file() {
        let value = read_text(&path)?.trim().to_string();
        if !value.is_empty() {
            return Ok(value);
        }
    }
    start_worker_invocation(options)
}

fn clear_worker_task_outputs(
    options: &ScienceOptions,
    task: &WorkerTask<'_>,
) -> Result<(), String> {
    let paths: &[&str] = match task.worker {
        "docking" => &[
            "docking_summary.tsv",
            "tool_outputs/docking_summary.tsv",
            "tool_outputs/docking_manifest.json",
            "tool_outputs/docking_manifest_index.json",
            "tool_outputs/docking_outcomes.tsv",
            "tool_outputs/docking_not_ready.tsv",
            "tool_outputs/docking_receptor.pdb",
            "tool_outputs/docking_receptor.pdbqt",
            "tool_outputs/docking_ligand.sdf",
            "tool_outputs/docking_ligand.pdbqt",
            "tool_outputs/docking_pose.pdbqt",
            "tool_outputs/docking_pose.sdf",
            "tool_outputs/docking_vina.log",
        ],
        "md_summary" => &[
            "md_summary.tsv",
            "md_summary_status.tsv",
            "tool_outputs/md_summary.tsv",
            "tool_outputs/md_not_ready.tsv",
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
            "tool_outputs/md_artifact_provenance.json",
            "tool_outputs/md_manifest_index.json",
            "tool_outputs/md_outcomes.tsv",
        ],
        _ => &[],
    };
    for relative in paths {
        remove_file_if_exists(&options.run_dir.join(relative))?;
    }
    Ok(())
}

fn remove_run_subdir_if_exists(run_dir: &Path, relative: &str) -> Result<(), String> {
    let path = run_dir.join(relative);
    if !path.exists() {
        return Ok(());
    }
    let root = run_dir
        .canonicalize()
        .map_err(|err| format!("run_dir_canonicalize_failed:{}:{err}", run_dir.display()))?;
    let resolved = path
        .canonicalize()
        .map_err(|err| format!("worker_dir_canonicalize_failed:{}:{err}", path.display()))?;
    if resolved == root || !resolved.starts_with(&root) || !resolved.is_dir() {
        return Err(format!("worker_dir_cleanup_outside_run:{}", path.display()));
    }
    fs::remove_dir_all(&resolved)
        .map_err(|err| format!("worker_dir_cleanup_failed:{}:{err}", resolved.display()))
}

fn clear_candidate_worker_files(options: &ScienceOptions, job_prefix: &str) -> Result<(), String> {
    let prefix = format!("{job_prefix}__");
    for relative in ["worker_jobs", "worker_results"] {
        let directory = options.run_dir.join(relative);
        if !directory.is_dir() {
            continue;
        }
        let entries = fs::read_dir(&directory)
            .map_err(|err| format!("worker_dir_read_failed:{}:{err}", directory.display()))?;
        for entry in entries {
            let entry = entry
                .map_err(|err| format!("worker_dir_entry_failed:{}:{err}", directory.display()))?;
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if path.is_file() && name.starts_with(&prefix) && name.ends_with(".json") {
                remove_file_if_exists(&path)?;
            }
        }
    }
    Ok(())
}

fn remove_file_if_exists(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    if !path.is_file() {
        return Err(format!("worker_output_not_file:{}", path.display()));
    }
    fs::remove_file(path).map_err(|err| format!("remove_failed:{}:{err}", path.display()))
}

fn run_artifact_path_is_current(run_dir: &Path, artifact: &str) -> bool {
    if artifact.trim().is_empty() {
        return false;
    }
    let Ok(root) = run_dir.canonicalize() else {
        return false;
    };
    let path = if cfg!(windows) {
        wsl_path_to_windows(artifact).unwrap_or_else(|| PathBuf::from(artifact))
    } else {
        PathBuf::from(artifact)
    };
    let candidate = if path.is_absolute() {
        path
    } else {
        run_dir.join(path)
    };
    candidate
        .canonicalize()
        .ok()
        .is_some_and(|path| path.is_file() && path.starts_with(root))
}

fn read_current_run_json_artifact(run_dir: &Path, artifact: &str) -> Result<Value, String> {
    let root = run_dir
        .canonicalize()
        .map_err(|err| format!("run_artifact_root_invalid:{}:{err}", run_dir.display()))?;
    let path = if cfg!(windows) {
        wsl_path_to_windows(artifact).unwrap_or_else(|| PathBuf::from(artifact))
    } else {
        PathBuf::from(artifact)
    };
    let candidate = if path.is_absolute() {
        path
    } else {
        run_dir.join(path)
    };
    let resolved = candidate
        .canonicalize()
        .map_err(|err| format!("run_artifact_missing:{}:{err}", candidate.display()))?;
    if !resolved.is_file() || !resolved.starts_with(&root) {
        return Err(format!("run_artifact_outside_run:{}", resolved.display()));
    }
    let text = read_text(&resolved)?;
    serde_json::from_str(text.trim_start_matches('\u{feff}'))
        .map_err(|err| format!("run_artifact_json_invalid:{}:{err}", resolved.display()))
}

fn run_scientific_workers(options: &ScienceOptions) -> Result<String, String> {
    if SCIENTIFIC_WORKER_TASKS.len() == 1 && SCIENTIFIC_WORKER_TASKS[0].worker == "md_summary" {
        return run_md_candidate_workers(options);
    }
    let job_dir = options.run_dir.join("worker_jobs");
    let result_dir = options.run_dir.join("worker_results");
    ensure_dir(&job_dir)?;
    ensure_dir(&result_dir)?;
    let mut summary = String::from(
        "job_id\tworker\tstage\tjob_schema_valid\tlaunch_status\tresult_schema_valid\tresult_status\texecution_status\treason\toutcome_count\tengine_version\tprimitive_execution_only\tpython_owns_final_score\tpython_owns_report_semantics\n",
    );
    let mut worker_errors = Vec::new();
    for task in SCIENTIFIC_WORKER_TASKS {
        clear_worker_task_outputs(options, task)?;
        let job_path = job_dir.join(format!("{}.json", task.job_id));
        let result_path = result_dir.join(format!("{}.json", task.job_id));
        remove_file_if_exists(&result_path)?;
        let job = worker_job_value(options, task)?;
        write_text(&job_path, &(job.to_string() + "\n"))?;
        let job_schema_valid = validate_worker_job(&job, task);
        let launch_status = match run_worker_task(options, task, &job_path, &result_path) {
            Ok(()) => "ok".to_string(),
            Err(err) => {
                if !result_path.exists() {
                    write_worker_failure_result(&result_path, task, &job, &err)?;
                }
                format!("failed:{}", clean_cell(&err))
            }
        };
        if !result_path.exists() {
            write_worker_failure_result(&result_path, task, &job, "worker_result_missing")?;
        }
        let result = read_worker_result_or_failure(&result_path, task, &job)?;
        let result_schema_valid = validate_worker_result_binding(&result, &job, options, task);
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
        if let Some(error) = worker_result_stage_error_bound(&result, &job, options, task) {
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

fn run_single_worker_task(options: &ScienceOptions, task: &WorkerTask<'_>) -> Result<(), String> {
    let job_dir = options.run_dir.join("worker_jobs");
    let result_dir = options.run_dir.join("worker_results");
    ensure_dir(&job_dir)?;
    ensure_dir(&result_dir)?;
    clear_worker_task_outputs(options, task)?;
    let job_path = job_dir.join(format!("{}.json", task.job_id));
    let result_path = result_dir.join(format!("{}.json", task.job_id));
    remove_file_if_exists(&result_path)?;
    let job = worker_job_value(options, task)?;
    write_text(&job_path, &(job.to_string() + "\n"))?;
    if let Err(err) = run_worker_task(options, task, &job_path, &result_path)
        && !result_path.exists()
    {
        write_worker_failure_result(&result_path, task, &job, &err)?;
    }
    let result = read_worker_result_or_failure(&result_path, task, &job)?;
    match worker_result_stage_error_bound(&result, &job, options, task) {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

fn selected_candidate_rows(
    options: &ScienceOptions,
) -> Result<Vec<BTreeMap<String, String>>, String> {
    let shortlist_path = options.run_dir.join("candidate_shortlist.tsv");
    let path = if shortlist_path.is_file() {
        shortlist_path
    } else {
        options.run_dir.join("complex_candidate_selection.tsv")
    };
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let mut rows = read_simple_tsv_rows(&path)?;
    if path.file_name().and_then(|value| value.to_str()) == Some("candidate_shortlist.tsv") {
        rows.retain(|row| {
            row.get("coordinate_status").map(String::as_str) == Some("ready")
                && row
                    .get("receptor_artifact")
                    .is_some_and(|value| !value.trim().is_empty())
                && row
                    .get("coordinate_sha256")
                    .is_some_and(|value| valid_sha256_hex(value))
        });
    }
    rows.sort_by(|left, right| {
        left.get("input_id")
            .cmp(&right.get("input_id"))
            .then_with(|| left.get("record_id").cmp(&right.get("record_id")))
            .then_with(|| candidate_selection_rank(left).cmp(&candidate_selection_rank(right)))
            .then_with(|| left.get("candidate_id").cmp(&right.get("candidate_id")))
    });
    let mut counts = BTreeMap::<(String, String), usize>::new();
    Ok(rows
        .into_iter()
        .filter(|row| {
            let key = (
                row.get("input_id").cloned().unwrap_or_default(),
                row.get("record_id").cloned().unwrap_or_default(),
            );
            let count = counts.entry(key).or_default();
            if *count == COMPLEX_CANDIDATE_TOP_N {
                return false;
            }
            *count += 1;
            true
        })
        .collect())
}

fn selected_dynamics_candidate_rows(
    options: &ScienceOptions,
) -> Result<(Vec<BTreeMap<String, String>>, Vec<CandidateWorkerOutcome>), String> {
    let input_kinds = ranking_input_kinds(options)?;
    let invocation_id = current_worker_invocation(options)?;
    let mut runnable = Vec::new();
    let mut not_applicable = Vec::new();
    for mut row in selected_candidate_rows(options)? {
        let input_id = row.get("input_id").cloned().unwrap_or_default();
        let record_id = row.get("record_id").cloned().unwrap_or_default();
        let input_kind = row
            .get("input_kind")
            .filter(|value| !value.trim().is_empty())
            .cloned()
            .or_else(|| {
                input_kinds
                    .get(&(input_id.clone(), record_id.clone()))
                    .cloned()
            })
            .ok_or_else(|| format!("ranking_input_kind_missing:{input_id}:{record_id}"))?;
        row.insert("input_kind".to_string(), input_kind.clone());
        if input_kind == "small_molecule" {
            runnable.push(row);
        } else {
            let candidate_id = row.get("candidate_id").cloned().unwrap_or_default();
            not_applicable.push(CandidateWorkerOutcome {
                job_id: candidate_worker_job_id("dynamics_capability", &candidate_id),
                candidate_id,
                input_id,
                record_id,
                invocation_id: invocation_id.clone(),
                status: "not_applicable".to_string(),
                reason: format!("input_kind_not_supported_for_docking_md:{input_kind}"),
                manifest: String::new(),
            });
        }
    }
    Ok((runnable, not_applicable))
}

fn candidate_selection_rank(row: &BTreeMap<String, String>) -> usize {
    row.get("shortlist_rank")
        .or_else(|| row.get("selection_rank"))
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(usize::MAX)
}

fn candidate_worker_job_id(worker: &str, candidate_id: &str) -> String {
    format!("{worker}__{}", safe_id(candidate_id))
}

fn candidate_worker_job_value(
    options: &ScienceOptions,
    task: &WorkerTask<'_>,
    candidate: &BTreeMap<String, String>,
) -> Result<(Value, CurrentMdRunFacts), String> {
    let facts = selected_candidate_run_facts(options, candidate)?;
    let mut job = worker_job_value(options, task)?;
    job["run_id"] = json!(facts.run_id);
    job["candidate_id"] = json!(facts.candidate_id);
    job["candidate_digest"] = json!(facts.candidate_digest);
    job["input_id"] = json!(facts.input_id);
    job["record_id"] = json!(facts.record_id);
    job["input_digest"] = json!(facts.input_digest);
    job["source_digest"] = json!(facts.source_digest);
    if let Some(namespace) = candidate.get("identity_namespace") {
        job["identity_namespace"] = json!(namespace);
    }
    job["candidate_selection"] = json!(
        if options.run_dir.join("candidate_shortlist.tsv").is_file() {
            "candidate_shortlist.tsv"
        } else {
            "complex_candidate_selection.tsv"
        }
    );
    if let Some(receptor_artifact) = candidate.get("receptor_artifact") {
        job["receptor_artifact"] = json!(receptor_artifact);
    }
    if let Some(receptor_digest) = candidate.get("coordinate_sha256") {
        job["receptor_digest"] = json!(receptor_digest);
    }
    if task.worker == "md_summary" {
        if let Some(entry) = candidate_index_entry(options, "docking", &facts.candidate_id)
            && entry.get("status").and_then(Value::as_str) == Some("completed")
            && let Some(manifest) = entry
                .get("manifest")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
        {
            let manifest_relative = current_run_artifact_relative(&options.run_dir, manifest)
                .ok_or_else(|| "docking_manifest_not_in_current_run".to_string())?;
            let manifest_digest = sha256_file_hex(&options.run_dir.join(&manifest_relative))
                .ok_or_else(|| "docking_manifest_digest_unavailable".to_string())?;
            job["docking_manifest"] = json!(manifest_relative);
            job["docking_manifest_digest"] = json!(manifest_digest);
            job["docking_job_id"] = json!(
                entry
                    .get("job_id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
            );
            job["docking_invocation_id"] = json!(
                entry
                    .get("invocation_id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
            );
        }
        job["platform_policy"] = json!(md_platform_policy(options)?);
    }
    Ok((job, facts))
}

fn md_platform_policy(options: &ScienceOptions) -> Result<String, String> {
    let configured = std::env::var("GENOLIGAND_MD_PLATFORM_POLICY")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            read_json_value_if_file(&options.input_manifest).and_then(|value| {
                value
                    .get("md_platform_policy")
                    .or_else(|| value.get("run_config")?.get("md_platform_policy"))
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
        })
        .unwrap_or_else(|| "disabled".to_string())
        .to_ascii_lowercase();
    if matches!(configured.as_str(), "required" | "preferred" | "disabled") {
        Ok(configured)
    } else {
        Err(format!("md_platform_policy_invalid:{configured}"))
    }
}

fn candidate_manifest_from_index(
    options: &ScienceOptions,
    kind: &str,
    candidate_id: &str,
) -> Option<String> {
    candidate_index_entry(options, kind, candidate_id)?
        .get("manifest")?
        .as_str()
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
}

fn candidate_index_entry(
    options: &ScienceOptions,
    kind: &str,
    candidate_id: &str,
) -> Option<Value> {
    let path = options
        .run_dir
        .join(format!("tool_outputs/{kind}_manifest_index.json"));
    read_json_value_if_file(&path)?
        .get("entries")?
        .as_array()?
        .iter()
        .find(|entry| entry.get("candidate_id").and_then(Value::as_str) == Some(candidate_id))
        .cloned()
}

fn run_candidate_worker_task(
    options: &ScienceOptions,
    task: &WorkerTask<'_>,
    job: &Value,
) -> Result<Value, String> {
    if !validate_candidate_worker_job(job, task, options) {
        return Err(format!("{}_candidate_job_binding_invalid", task.worker));
    }
    let job_dir = options.run_dir.join("worker_jobs");
    let result_dir = options.run_dir.join("worker_results");
    ensure_dir(&job_dir)?;
    ensure_dir(&result_dir)?;
    let job_path = job_dir.join(format!("{}.json", task.job_id));
    let result_path = result_dir.join(format!("{}.json", task.job_id));
    remove_file_if_exists(&result_path)?;
    write_text(&job_path, &(job.to_string() + "\n"))?;
    if let Err(error) = run_worker_task(options, task, &job_path, &result_path)
        && !result_path.is_file()
    {
        write_worker_failure_result(&result_path, task, job, &error)?;
    }
    let mut result = read_worker_result_or_failure(&result_path, task, job)?;
    if !validate_candidate_worker_result(&result, job, task) {
        let reason = format!(
            "{}_candidate_result_binding_invalid:{}",
            task.worker,
            job.get("candidate_id")
                .and_then(Value::as_str)
                .unwrap_or("missing")
        );
        write_worker_failure_result(&result_path, task, job, &reason)?;
        result = read_worker_result_or_failure(&result_path, task, job)?;
        if !validate_candidate_worker_result(&result, job, task) {
            return Err(reason);
        }
    }
    Ok(result)
}

fn validate_candidate_worker_job(
    job: &Value,
    task: &WorkerTask<'_>,
    options: &ScienceOptions,
) -> bool {
    validate_worker_job(job, task)
        && ["run_id", "candidate_id", "input_id", "record_id"]
            .into_iter()
            .all(|field| {
                job.get(field)
                    .and_then(Value::as_str)
                    .is_some_and(|value| !value.trim().is_empty())
            })
        && ["candidate_digest", "input_digest", "source_digest"]
            .into_iter()
            .all(|field| {
                job.get(field)
                    .and_then(Value::as_str)
                    .is_some_and(valid_sha256_hex)
            })
        && (task.worker != "docking"
            || (job
                .get("receptor_artifact")
                .and_then(Value::as_str)
                .is_some_and(|artifact| run_artifact_path_is_current(&options.run_dir, artifact))
                && job
                    .get("receptor_digest")
                    .and_then(Value::as_str)
                    .is_some_and(valid_sha256_hex)))
}

fn validate_candidate_worker_result(result: &Value, job: &Value, task: &WorkerTask<'_>) -> bool {
    let run_dir = Path::new(job.get("run_dir").and_then(Value::as_str).unwrap_or(""));
    let status = result.get("status").and_then(Value::as_str);
    let artifacts_bound = if matches!(status, Some("PASS") | Some("NOT_READY")) {
        result
            .get("manifest")
            .and_then(Value::as_str)
            .is_some_and(|path| run_artifact_path_is_current(run_dir, path))
            && (task.worker != "md_summary"
                || result
                    .get("artifact_provenance")
                    .and_then(Value::as_str)
                    .is_some_and(|path| run_artifact_path_is_current(run_dir, path)))
    } else {
        true
    };
    validate_worker_result(result, task)
        && artifacts_bound
        && [
            "run_id",
            "invocation_id",
            "candidate_id",
            "candidate_digest",
            "input_id",
            "record_id",
            "input_digest",
            "source_digest",
        ]
        .into_iter()
        .all(|field| {
            let expected = job.get(field).and_then(Value::as_str);
            expected.is_some_and(|value| !value.trim().is_empty())
                && result.get(field).and_then(Value::as_str) == expected
        })
        && (task.worker != "docking"
            || result.get("receptor_digest").and_then(Value::as_str)
                == job.get("receptor_digest").and_then(Value::as_str))
}

fn candidate_worker_summary_row(task: &WorkerTask<'_>, result: &Value, job_valid: bool) -> String {
    format!(
        "{}\t{}\t{}\t{}\tok\ttrue\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
        task.job_id,
        task.worker,
        task.stage,
        job_valid,
        clean_cell(
            result
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("FAIL")
        ),
        clean_cell(
            result
                .get("execution_status")
                .and_then(Value::as_str)
                .unwrap_or("failed")
        ),
        clean_cell(
            result
                .get("reason")
                .and_then(Value::as_str)
                .unwrap_or("missing_reason")
        ),
        result
            .get("outcomes")
            .and_then(Value::as_array)
            .map(Vec::len)
            .unwrap_or_default(),
        clean_cell(
            result
                .get("engine_version")
                .and_then(Value::as_str)
                .unwrap_or("missing")
        ),
        result
            .get("primitive_execution_only")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        result
            .get("python_owns_final_score")
            .and_then(Value::as_bool)
            .unwrap_or(true),
        result
            .get("python_owns_report_semantics")
            .and_then(Value::as_bool)
            .unwrap_or(true),
    )
}

fn run_docking_candidate_workers(options: &ScienceOptions) -> Result<(), String> {
    remove_run_subdir_if_exists(&options.run_dir, "tool_outputs/docking")?;
    clear_candidate_worker_files(options, DOCKING_WORKER_TASK.job_id)?;
    reset_candidate_manifest_index(options, "docking")?;
    let (rows, mut worker_outcomes) = selected_dynamics_candidate_rows(options)?;
    let mut outcomes = String::from(
        "candidate_id\tinput_id\trecord_id\tjob_id\tinvocation_id\tstatus\treason\tmanifest\n",
    );
    for outcome in &worker_outcomes {
        outcomes.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t\n",
            clean_cell(&outcome.candidate_id),
            clean_cell(&outcome.input_id),
            clean_cell(&outcome.record_id),
            clean_cell(&outcome.job_id),
            clean_cell(&outcome.invocation_id),
            clean_cell(&outcome.status),
            clean_cell(&outcome.reason),
        ));
    }
    for candidate in rows {
        let candidate_id = candidate
            .get("candidate_id")
            .map(String::as_str)
            .unwrap_or_default();
        let job_id = candidate_worker_job_id(DOCKING_WORKER_TASK.job_id, candidate_id);
        let task = WorkerTask {
            job_id: &job_id,
            worker: DOCKING_WORKER_TASK.worker,
            module: DOCKING_WORKER_TASK.module,
            stage: DOCKING_WORKER_TASK.stage,
        };
        let (job, facts) = candidate_worker_job_value(options, &task, &candidate)?;
        let result = run_candidate_worker_task(options, &task, &job)?;
        ensure_manifest_index_entry(options, "docking", &facts, &task, &result)?;
        let invocation_id = job
            .get("invocation_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let status = result
            .get("execution_status")
            .and_then(Value::as_str)
            .unwrap_or("failed")
            .to_string();
        let reason = result
            .get("reason")
            .and_then(Value::as_str)
            .unwrap_or("missing_reason")
            .to_string();
        let manifest = candidate_manifest_from_index(options, "docking", &facts.candidate_id)
            .unwrap_or_default();
        outcomes.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            clean_cell(&facts.candidate_id),
            clean_cell(&facts.input_id),
            clean_cell(&facts.record_id),
            clean_cell(task.job_id),
            clean_cell(&invocation_id),
            clean_cell(&status),
            clean_cell(&reason),
            clean_cell(&manifest),
        ));
        worker_outcomes.push(CandidateWorkerOutcome {
            candidate_id: facts.candidate_id,
            input_id: facts.input_id,
            record_id: facts.record_id,
            job_id,
            invocation_id,
            status,
            reason,
            manifest,
        });
    }
    write_text(
        &options.run_dir.join("tool_outputs/docking_outcomes.tsv"),
        &outcomes,
    )?;
    ensure_docking_summary_outcomes(options, &worker_outcomes)
}

fn ensure_docking_summary_outcomes(
    options: &ScienceOptions,
    outcomes: &[CandidateWorkerOutcome],
) -> Result<(), String> {
    let path = options.run_dir.join("tool_outputs/docking_summary.tsv");
    let mut text = if path.is_file() {
        read_text(&path)?
    } else {
        String::from("candidate_id\tdocking_status\tdocking_score\tdocking_engine\tnotes\n")
    };
    let present = if path.is_file() {
        read_simple_tsv_rows(&path)?
            .into_iter()
            .filter_map(|row| row.get("candidate_id").cloned())
            .collect::<BTreeSet<_>>()
    } else {
        BTreeSet::new()
    };
    for outcome in outcomes {
        if present.contains(&outcome.candidate_id) {
            continue;
        }
        text.push_str(&format!(
            "{}\t{}\t\tvina\t{}\n",
            clean_cell(&outcome.candidate_id),
            clean_cell(&outcome.status),
            clean_cell(&format!(
                "reason={};input_id={};record_id={};job_id={};invocation_id={};manifest={}",
                outcome.reason,
                outcome.input_id,
                outcome.record_id,
                outcome.job_id,
                outcome.invocation_id,
                outcome.manifest,
            )),
        ));
    }
    write_text(&path, &text)
}

fn run_md_candidate_workers(options: &ScienceOptions) -> Result<String, String> {
    let base = SCIENTIFIC_WORKER_TASKS[0];
    clear_worker_task_outputs(options, &base)?;
    remove_run_subdir_if_exists(&options.run_dir, "tool_outputs/md")?;
    clear_candidate_worker_files(options, base.job_id)?;
    reset_candidate_manifest_index(options, "md")?;
    let (rows, not_applicable) = selected_dynamics_candidate_rows(options)?;
    let mut summary = String::from(
        "job_id\tworker\tstage\tjob_schema_valid\tlaunch_status\tresult_schema_valid\tresult_status\texecution_status\treason\toutcome_count\tengine_version\tprimitive_execution_only\tpython_owns_final_score\tpython_owns_report_semantics\n",
    );
    let mut outcomes = String::from(
        "candidate_id\tinput_id\trecord_id\tjob_id\tinvocation_id\tstatus\treason\tmanifest\n",
    );
    for outcome in &not_applicable {
        summary.push_str(&format!(
            "{}\t{}\t{}\tfalse\tnot_applicable\ttrue\tNOT_APPLICABLE\tnot_applicable\t{}\t0\tGenoLigand capability gate\ttrue\tfalse\tfalse\n",
            clean_cell(&outcome.job_id),
            base.worker,
            base.stage,
            clean_cell(&outcome.reason),
        ));
        outcomes.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t\n",
            clean_cell(&outcome.candidate_id),
            clean_cell(&outcome.input_id),
            clean_cell(&outcome.record_id),
            clean_cell(&outcome.job_id),
            clean_cell(&outcome.invocation_id),
            clean_cell(&outcome.status),
            clean_cell(&outcome.reason),
        ));
    }
    for candidate in rows {
        let candidate_id = candidate
            .get("candidate_id")
            .map(String::as_str)
            .unwrap_or_default();
        let job_id = candidate_worker_job_id(base.job_id, candidate_id);
        let task = WorkerTask {
            job_id: &job_id,
            worker: base.worker,
            module: base.module,
            stage: base.stage,
        };
        let (job, facts) = candidate_worker_job_value(options, &task, &candidate)?;
        let job_valid = validate_candidate_worker_job(&job, &task, options);
        let result = run_candidate_worker_task(options, &task, &job)?;
        ensure_manifest_index_entry(options, "md", &facts, &task, &result)?;
        summary.push_str(&candidate_worker_summary_row(&task, &result, job_valid));
        outcomes.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            clean_cell(&facts.candidate_id),
            clean_cell(&facts.input_id),
            clean_cell(&facts.record_id),
            clean_cell(task.job_id),
            clean_cell(
                job.get("invocation_id")
                    .and_then(Value::as_str)
                    .unwrap_or("")
            ),
            clean_cell(
                result
                    .get("execution_status")
                    .and_then(Value::as_str)
                    .unwrap_or("failed")
            ),
            clean_cell(
                result
                    .get("reason")
                    .and_then(Value::as_str)
                    .unwrap_or("missing_reason")
            ),
            clean_cell(
                &candidate_manifest_from_index(options, "md", &facts.candidate_id)
                    .unwrap_or_default()
            ),
        ));
    }
    write_text(
        &options.run_dir.join("tool_outputs/md_outcomes.tsv"),
        &outcomes,
    )?;
    write_text(
        &options.run_dir.join("scientific_runtime_summary.tsv"),
        &summary,
    )?;
    Ok(format!(
        "scientific_workers_checked\tworker_count={}",
        summary.lines().count().saturating_sub(1)
    ))
}

fn ensure_manifest_index_entry(
    options: &ScienceOptions,
    kind: &str,
    facts: &CurrentMdRunFacts,
    task: &WorkerTask<'_>,
    result: &Value,
) -> Result<(), String> {
    let (schema_version, relative) = match kind {
        "docking" => (
            "genoligand_docking_manifest_index_v1",
            "tool_outputs/docking_manifest_index.json",
        ),
        "md" => (
            "genoligand_md_manifest_index_v1",
            "tool_outputs/md_manifest_index.json",
        ),
        _ => return Err(format!("worker_manifest_index_kind_invalid:{kind}")),
    };
    let path = options.run_dir.join(relative);
    let mut index = read_json_value_if_file(&path).unwrap_or_else(|| {
        json!({
            "schema_version": schema_version,
            "run_id": facts.run_id,
            "entries": []
        })
    });
    if index.get("schema_version").and_then(Value::as_str) != Some(schema_version)
        || index.get("run_id").and_then(Value::as_str) != Some(facts.run_id.as_str())
    {
        return Err(format!("{kind}_manifest_index_identity_mismatch"));
    }
    let entries = index
        .get_mut("entries")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| format!("{kind}_manifest_index_entries_invalid"))?;
    let existing = entries
        .iter()
        .position(|entry| {
            entry.get("candidate_id").and_then(Value::as_str) == Some(facts.candidate_id.as_str())
        })
        .map(|position| entries.remove(position));
    let execution_status = result
        .get("execution_status")
        .and_then(Value::as_str)
        .unwrap_or("failed");
    let completed = execution_status == "completed";
    let index_status = if execution_status == "not_applicable" {
        "not_ready"
    } else {
        execution_status
    };
    let existing_field = |field: &str| {
        existing
            .as_ref()
            .and_then(|entry| entry.get(field))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    let manifest = result
        .get("manifest")
        .and_then(Value::as_str)
        .or_else(|| result.get("artifact_path").and_then(Value::as_str))
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| existing_field("manifest"));
    let artifact_provenance = result
        .get("artifact_provenance")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| existing_field("artifact_provenance"));
    if completed && (manifest.is_empty() || (kind == "md" && artifact_provenance.is_empty())) {
        return Err(format!("{kind}_completed_manifest_binding_missing"));
    }
    entries.push(json!({
        "candidate_id": facts.candidate_id,
        "input_id": facts.input_id,
        "record_id": facts.record_id,
        "candidate_digest": facts.candidate_digest,
        "input_digest": facts.input_digest,
        "source_digest": facts.source_digest,
        "job_id": task.job_id,
        "invocation_id": result.get("invocation_id").and_then(Value::as_str).unwrap_or(""),
        "status": index_status,
        "reason": result.get("reason").and_then(Value::as_str).unwrap_or("missing_reason"),
        "manifest": manifest,
        "artifact_provenance": artifact_provenance,
    }));
    entries.sort_by(|left, right| {
        left.get("candidate_id")
            .and_then(Value::as_str)
            .cmp(&right.get("candidate_id").and_then(Value::as_str))
    });
    write_text(&path, &(index.to_string() + "\n"))
}

fn reset_candidate_manifest_index(options: &ScienceOptions, kind: &str) -> Result<(), String> {
    let schema_version = match kind {
        "docking" => "genoligand_docking_manifest_index_v1",
        "md" => "genoligand_md_manifest_index_v1",
        _ => return Err(format!("worker_manifest_index_kind_invalid:{kind}")),
    };
    write_text(
        &options
            .run_dir
            .join(format!("tool_outputs/{kind}_manifest_index.json")),
        &(json!({
            "schema_version": schema_version,
            "run_id": infer_run_id(&options.run_dir),
            "entries": [],
        })
        .to_string()
            + "\n"),
    )
}

fn worker_job_value(options: &ScienceOptions, task: &WorkerTask<'_>) -> Result<Value, String> {
    let invocation_id = current_worker_invocation(options)?;
    let mut job = json!({
        "schema_version": WORKER_SCHEMA_VERSION,
        "pipeline_mode": WORKER_PIPELINE_MODE,
        "job_id": task.job_id,
        "worker": task.worker,
        "stage": task.stage,
        "run_id": options.run_dir.file_name().and_then(|name| name.to_str()).unwrap_or("run"),
        "invocation_id": invocation_id,
        "rust_owned_decision_logic": true,
        "decision_owner": "GenoLigand Rust core",
        "input_manifest": options.input_manifest.display().to_string(),
        "run_dir": options.run_dir.display().to_string(),
        "science_store": options.science_store.display().to_string(),
        "expected_result_schema": WORKER_SCHEMA_VERSION
    });
    if task.worker == "md_summary"
        && let Ok(facts) = current_md_run_facts(options)
    {
        job["run_id"] = json!(facts.run_id);
        job["candidate_id"] = json!(facts.candidate_id);
        job["candidate_digest"] = json!(facts.candidate_digest);
        job["input_id"] = json!(facts.input_id);
        job["record_id"] = json!(facts.record_id);
        job["input_digest"] = json!(facts.input_digest);
        job["source_digest"] = json!(facts.source_digest);
    }
    Ok(job)
}

fn validate_worker_job(value: &Value, task: &WorkerTask<'_>) -> bool {
    value.get("schema_version").and_then(Value::as_str) == Some(WORKER_SCHEMA_VERSION)
        && value.get("pipeline_mode").and_then(Value::as_str) == Some(WORKER_PIPELINE_MODE)
        && value.get("job_id").and_then(Value::as_str) == Some(task.job_id)
        && value.get("worker").and_then(Value::as_str) == Some(task.worker)
        && value.get("stage").and_then(Value::as_str) == Some(task.stage)
        && value
            .get("invocation_id")
            .and_then(Value::as_str)
            .is_some_and(|value| !value.trim().is_empty())
        && value
            .get("rust_owned_decision_logic")
            .and_then(Value::as_bool)
            == Some(true)
        && value.get("expected_result_schema").and_then(Value::as_str)
            == Some(WORKER_SCHEMA_VERSION)
}

fn run_worker_task(
    options: &ScienceOptions,
    task: &WorkerTask<'_>,
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

fn worker_result_file_is_valid(path: &Path, task: &WorkerTask<'_>) -> bool {
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

fn read_worker_result_or_failure(
    path: &Path,
    task: &WorkerTask<'_>,
    job: &Value,
) -> Result<Value, String> {
    match read_text(path)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}')).ok())
    {
        Some(value) => Ok(value),
        None => {
            write_worker_failure_result(path, task, job, "worker_result_parse_failed")?;
            serde_json::from_str::<Value>(&read_text(path)?)
                .map_err(|err| format!("worker_result_reparse_failed:{}:{err}", path.display()))
        }
    }
}

fn write_worker_failure_result(
    path: &Path,
    task: &WorkerTask<'_>,
    job: &Value,
    reason: &str,
) -> Result<(), String> {
    let reason = clean_cell(reason);
    let mut value = json!({
        "schema_version": WORKER_SCHEMA_VERSION,
        "job_id": task.job_id,
        "worker": task.worker,
        "run_id": job.get("run_id").and_then(Value::as_str).unwrap_or(""),
        "invocation_id": job.get("invocation_id").and_then(Value::as_str).unwrap_or("missing"),
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
    for field in [
        "candidate_id",
        "candidate_digest",
        "input_id",
        "record_id",
        "input_digest",
        "source_digest",
        "receptor_digest",
    ] {
        if let Some(binding) = job
            .get(field)
            .and_then(Value::as_str)
            .filter(|binding| !binding.trim().is_empty())
        {
            value[field] = json!(binding);
        }
    }
    write_text(path, &(value.to_string() + "\n"))
}

fn validate_worker_result(value: &Value, task: &WorkerTask<'_>) -> bool {
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

fn validate_worker_result_binding(
    value: &Value,
    job: &Value,
    options: &ScienceOptions,
    task: &WorkerTask<'_>,
) -> bool {
    if !validate_worker_result(value, task) {
        return false;
    }
    if value.get("run_id").and_then(Value::as_str) != job.get("run_id").and_then(Value::as_str)
        || value.get("invocation_id").and_then(Value::as_str)
            != job.get("invocation_id").and_then(Value::as_str)
    {
        return false;
    }
    if task.worker != "md_summary" {
        return true;
    }
    ["artifact_path", "artifact_provenance"]
        .into_iter()
        .filter_map(|field| value.get(field).and_then(Value::as_str))
        .all(|artifact| run_artifact_path_is_current(&options.run_dir, artifact))
}

fn worker_result_stage_error_bound(
    value: &Value,
    job: &Value,
    options: &ScienceOptions,
    task: &WorkerTask<'_>,
) -> Option<String> {
    if !validate_worker_result_binding(value, job, options, task) {
        return Some(format!("{}_result_binding_invalid", task.worker));
    }
    worker_result_stage_error(value, task)
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

fn worker_result_stage_error(value: &Value, task: &WorkerTask<'_>) -> Option<String> {
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
    let mut matches_by_input = inputs
        .iter()
        .map(|input| {
            (
                (input.input_id.clone(), input.record_id.clone()),
                Vec::<IndexMatch>::new(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    for_each_store_row_batch(
        &options.science_store.join("indexes/structure_targets.tsv"),
        "structure_interaction_index",
        |rows| {
            for input in &inputs {
                let informative_token_count = input
                    .tokens
                    .iter()
                    .filter(|token| structure_match_token_is_informative(token))
                    .count();
                if informative_token_count == 0 {
                    continue;
                }
                let matches = matches_by_input
                    .get_mut(&(input.input_id.clone(), input.record_id.clone()))
                    .expect("structure match input initialized");
                for row in rows {
                    let matched = structure_matched_tokens(input, row);
                    if matched.is_empty() {
                        continue;
                    }
                    matches.push(IndexMatch {
                        input_id: input.input_id.clone(),
                        record_id: input.record_id.clone(),
                        source: row.source.clone(),
                        target_id: row.target_id.clone(),
                        target_name: row.target_name.clone(),
                        organism: row.organism.clone(),
                        target_class: row.target_class.clone(),
                        match_score: (matched.len() as f64 / informative_token_count as f64)
                            .min(1.0),
                        matched_features: matched.join(" "),
                        evidence: row.evidence.clone(),
                    });
                }
                retain_top_structure_matches(matches);
            }
            Ok(())
        },
    )?;
    let mut matches = Vec::new();
    for mut input_matches in matches_by_input.into_values() {
        retain_top_structure_matches(&mut input_matches);
        for item in &mut input_matches {
            item.evidence = bind_database_evidence(options, &item.source, &item.evidence)?;
        }
        matches.append(&mut input_matches);
    }
    let mut out = String::from(
        "input_id\tsource\ttarget_id\ttarget_name\torganism\ttarget_class\tmatch_score\tmatched_features\tevidence\tsource_database_id\tcurrent_run_query_id\trecord_id\n",
    );
    for item in &matches {
        out.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{:.4}\t{}\t{}\t{}\t{}\t{}\n",
            item.input_id,
            clean_cell(&item.source),
            clean_cell(&item.target_id),
            clean_cell(&item.target_name),
            clean_cell(&item.organism),
            clean_cell(&item.target_class),
            item.match_score,
            clean_cell(&item.matched_features),
            clean_cell(&item.evidence),
            clean_cell(&item.source),
            clean_cell(&item.input_id),
            clean_cell(&item.record_id),
        ));
    }
    write_text(
        &options.run_dir.join("structure_interaction_matches.tsv"),
        &out,
    )?;
    Ok(matches)
}

fn structure_match_token_is_informative(token: &str) -> bool {
    !token.starts_with("qs_") && token.chars().count() >= MIN_STRUCTURE_MATCH_TOKEN_CHARS
}

fn structure_matched_tokens(input: &ComputedFeatureInput, row: &StoreRow) -> Vec<String> {
    input
        .tokens
        .iter()
        .filter(|token| {
            structure_match_token_is_informative(token)
                && (row.ligand_tokens.contains(*token)
                    || row.sequence_tokens.contains(*token)
                    || row.structure_tokens.contains(*token))
        })
        .cloned()
        .collect()
}

fn retain_top_structure_matches(matches: &mut Vec<IndexMatch>) {
    matches.sort_by(|left, right| {
        right
            .match_score
            .total_cmp(&left.match_score)
            .then_with(|| left.source.cmp(&right.source))
            .then_with(|| left.target_id.cmp(&right.target_id))
            .then_with(|| left.target_name.cmp(&right.target_name))
            .then_with(|| left.organism.cmp(&right.organism))
            .then_with(|| left.target_class.cmp(&right.target_class))
            .then_with(|| left.matched_features.cmp(&right.matched_features))
            .then_with(|| left.evidence.cmp(&right.evidence))
    });
    matches.truncate(STRUCTURE_MATCH_TOP_K_PER_INPUT_RECORD);
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
    let file =
        fs::File::open(path).map_err(|err| format!("read_failed:{}:{err}", path.display()))?;
    let mut lines = BufReader::new(file).lines();
    let headers = lines
        .next()
        .transpose()
        .map_err(|err| format!("read_failed:{}:{err}", path.display()))?
        .map(|line| split_tsv(line.trim_start_matches('\u{feff}')))
        .unwrap_or_default();
    let Some(input_idx) = find_header(&headers, &["input_id"]) else {
        return Ok(());
    };
    let Some(source_idx) = find_header(&headers, &["source"]) else {
        return Ok(());
    };
    let matched_idx = find_header(&headers, &["matched_features"]);
    let evidence_idx = find_header(&headers, &["evidence"]);
    let provenance_idx = find_header(&headers, &["source_row_provenance"]);
    let ligand_identity_idx = find_header(&headers, &["ligand_identity"]);
    let record_idx = find_header(&headers, &["record_id"]);
    for line in lines {
        let line = line.map_err(|err| format!("read_failed:{}:{err}", path.display()))?;
        let cols = split_tsv(&line);
        if !cell(&cols, Some(source_idx)).eq_ignore_ascii_case("BindingDB") {
            continue;
        }
        let evidence = cell(&cols, evidence_idx);
        let provenance = cell(&cols, provenance_idx);
        if ![evidence.as_str(), provenance.as_str()]
            .iter()
            .any(|value| {
                value.split(';').any(|part| {
                    part.trim()
                        .eq_ignore_ascii_case("bindingdb_onsite_activity_row")
                })
            })
        {
            continue;
        }
        let key = (
            cell(&cols, Some(input_idx)),
            first_nonempty(&[cell(&cols, record_idx), "record_1".to_string()]),
        );
        if let Some(input) = by_input.get_mut(&key) {
            for value in [
                cell(&cols, matched_idx),
                evidence,
                cell(&cols, ligand_identity_idx),
            ] {
                append_bindingdb_structure_identity_tokens(&mut input.tokens, &value);
            }
        }
    }
    Ok(())
}

fn append_bindingdb_structure_identity_tokens(tokens: &mut BTreeSet<String>, value: &str) {
    for part in value.split(';') {
        let Some((key, raw_value)) = part.split_once('=') else {
            continue;
        };
        match key.trim().to_ascii_lowercase().as_str() {
            "ligand_code" => tokens.extend(
                tokenize(raw_value)
                    .into_iter()
                    .filter(|token| structure_match_token_is_informative(token)),
            ),
            "pdb_ids" => {
                for pdb_id in pdb_ids_from_text(raw_value) {
                    tokens.extend(
                        tokenize(&pdb_id)
                            .into_iter()
                            .filter(|token| structure_match_token_is_informative(token)),
                    );
                }
            }
            _ => {}
        }
    }
}

fn write_structure_summary_from_matches(path: &Path, matches: &[IndexMatch]) -> Result<(), String> {
    let mut text = String::from(
        "candidate_id\tstructure_score\tstructure_evidence\tbinding_patch\tpatch_evidence\tkey_residues\tdocking_score\tdocking_pose\tligand_position\tinteraction_residues\n",
    );
    let mut by_target = BTreeMap::<String, &IndexMatch>::new();
    for item in matches {
        let namespace = canonical_target_namespace(&item.source, &item.source, &item.target_id);
        let key = format!(
            "{}\u{1f}{}\u{1f}{}\u{1f}{}",
            item.input_id, item.record_id, namespace, item.target_id
        );
        match by_target.get(&key) {
            Some(existing) if existing.match_score >= item.match_score => {}
            _ => {
                by_target.insert(key, item);
            }
        }
    }
    for item in by_target.values() {
        let namespace = canonical_target_namespace(&item.source, &item.source, &item.target_id);
        text.push_str(&format!(
            "{}\t{:.4}\t{}\t{}\t{}\t{}\t\t\t\t{}\n",
            candidate_id_for(&item.input_id, &item.record_id, &namespace, &item.target_id),
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
            write_text(
                &options.run_dir.join("structure_viewer_status.tsv"),
                &format!(
                    "status\ttarget_id\tpdb_id\tartifact\texpected_path\treason\tcandidate_id\tinput_id\trecord_id\tselection_rank\tcoordinate_sha256\nready\t\t\t{}\t{}\tworker coordinate artifact copied from current run output\t\t\t\t\t{}\n",
                    clean_cell(&artifact),
                    clean_cell(&relative_display(&options.run_dir, &source)),
                    sha256_file_hex(&options.run_dir.join(&artifact)).unwrap_or_default(),
                ),
            )?;
            return Ok(true);
        }
    }
    Ok(false)
}

fn write_coordinate_backed_candidate_selection(
    options: &ScienceOptions,
    matches: &[IndexMatch],
) -> Result<bool, String> {
    write_coordinate_backed_candidate_selection_impl(
        options,
        matches,
        RCSB_OFFICIAL_DOWNLOAD_BASE_URL,
        None,
    )
}

#[cfg(test)]
fn write_coordinate_backed_candidate_selection_with_base_url(
    options: &ScienceOptions,
    matches: &[IndexMatch],
    fetch_base_url: &str,
) -> Result<bool, String> {
    let python = vec!["python".to_string()];
    write_coordinate_backed_candidate_selection_impl(
        options,
        matches,
        fetch_base_url,
        Some(&python),
    )
}

fn write_coordinate_backed_candidate_selection_impl(
    options: &ScienceOptions,
    matches: &[IndexMatch],
    fetch_base_url: &str,
    python_command: Option<&[String]>,
) -> Result<bool, String> {
    let shortlist_path = options.run_dir.join("candidate_shortlist.tsv");
    if !shortlist_path.is_file() {
        write_candidate_shortlist(options)?;
    }
    let shortlist_rows = read_simple_tsv_rows(&shortlist_path)?;
    let shortlist_order = shortlist_rows
        .iter()
        .filter_map(|row| {
            let candidate_id = row.get("candidate_id")?.clone();
            Some((
                candidate_id,
                (
                    row.get("input_id").cloned().unwrap_or_default(),
                    row.get("record_id").cloned().unwrap_or_default(),
                    candidate_selection_rank(row),
                ),
            ))
        })
        .collect::<BTreeMap<_, _>>();
    remove_run_subdir_if_exists(&options.run_dir, "structure_candidates")?;
    for relative in [
        "structure_viewer.json",
        "structure_viewer.pdb",
        "structure_viewer.cif",
        "structure_viewer.mmcif",
    ] {
        remove_file_if_exists(&options.run_dir.join(relative))?;
    }
    let mut candidates = Vec::<CoordinateCandidate>::new();
    for hint in known_structure_hints(&options.run_dir.join("known_database_matches.tsv"))? {
        for pdb_id in &hint.pdb_ids {
            let selected = SelectedComplexCandidate {
                identity_namespace: canonical_target_namespace(
                    "BindingDB",
                    "BindingDB",
                    &hint.target_id,
                ),
                candidate_id: candidate_id_for(
                    &hint.input_id,
                    &hint.record_id,
                    &canonical_target_namespace("BindingDB", "BindingDB", &hint.target_id),
                    &hint.target_id,
                ),
                input_id: hint.input_id.clone(),
                record_id: hint.record_id.clone(),
                target_id: hint.target_id.clone(),
                target_name: hint.target_name.clone(),
                organism: hint.organism.clone(),
                target_class: hint.target_class.clone(),
                source: "BindingDB".to_string(),
                pdb_id: pdb_id.clone(),
                evidence: hint.evidence.clone(),
            };
            let priority = known_hint_rank(&hint);
            let coordinate_path = find_structure_coordinate_file(&options.science_store, pdb_id);
            let score = coordinate_path
                .as_ref()
                .map(|path| coordinate_hint_score(path, &hint))
                .unwrap_or(hint.match_score);
            candidates.push(CoordinateCandidate {
                selected,
                coordinate_path,
                priority,
                score,
            });
        }
    }
    for item in matches {
        for pdb_id in structure_coordinate_ids(item) {
            let selected = SelectedComplexCandidate {
                identity_namespace: canonical_target_namespace(
                    &item.source,
                    &item.source,
                    &item.target_id,
                ),
                candidate_id: candidate_id_for(
                    &item.input_id,
                    &item.record_id,
                    &canonical_target_namespace(&item.source, &item.source, &item.target_id),
                    &item.target_id,
                ),
                input_id: item.input_id.clone(),
                record_id: item.record_id.clone(),
                target_id: item.target_id.clone(),
                target_name: item.target_name.clone(),
                organism: item.organism.clone(),
                target_class: item.target_class.clone(),
                source: item.source.clone(),
                pdb_id: pdb_id.clone(),
                evidence: item.evidence.clone(),
            };
            candidates.push(CoordinateCandidate {
                selected,
                coordinate_path: find_structure_coordinate_file(&options.science_store, &pdb_id),
                priority: 0,
                score: item.match_score,
            });
        }
    }
    candidates.retain(|candidate| shortlist_order.contains_key(&candidate.selected.candidate_id));
    candidates.sort_by(|left, right| {
        shortlist_order[&left.selected.candidate_id]
            .cmp(&shortlist_order[&right.selected.candidate_id])
            .then_with(|| right.priority.cmp(&left.priority))
            .then_with(|| right.score.total_cmp(&left.score))
            .then_with(|| left.selected.candidate_id.cmp(&right.selected.candidate_id))
            .then_with(|| left.selected.pdb_id.cmp(&right.selected.pdb_id))
            .then_with(|| {
                right
                    .coordinate_path
                    .is_some()
                    .cmp(&left.coordinate_path.is_some())
            })
    });

    let mut selected_rows = Vec::new();
    let mut selected_by_input = BTreeMap::<(String, String), usize>::new();
    let mut seen = BTreeSet::new();
    let mut audited_pdb_ids = BTreeSet::new();
    let mut audit_rows = Vec::new();
    let mut fetch_results = BTreeMap::<String, RcsbFetchResult>::new();
    let mut fetch_count = 0usize;
    let mut policy_validated = false;
    for candidate in candidates {
        let input_key = (
            candidate.selected.input_id.clone(),
            candidate.selected.record_id.clone(),
        );
        if selected_by_input
            .get(&input_key)
            .copied()
            .unwrap_or_default()
            == COMPLEX_CANDIDATE_TOP_N
        {
            continue;
        }
        if seen.contains(&candidate.selected.candidate_id) {
            continue;
        }
        let pdb_id = candidate.selected.pdb_id.clone();
        let initially_local = candidate.coordinate_path.is_some();
        let result = if let Some(path) = candidate.coordinate_path {
            RcsbFetchResult {
                status: "reused".to_string(),
                sha256: sha256_file_hex(&path).unwrap_or_default(),
                reason: "valid_cached_coordinate_reused".to_string(),
                path,
            }
        } else if let Some(path) = find_structure_coordinate_file(&options.science_store, &pdb_id) {
            RcsbFetchResult {
                status: "reused".to_string(),
                sha256: sha256_file_hex(&path).unwrap_or_default(),
                reason: "valid_coordinate_published_by_concurrent_run".to_string(),
                path,
            }
        } else if let Some(result) = fetch_results.get(&pdb_id) {
            result.clone()
        } else if fetch_count == RCSB_ON_DEMAND_FETCH_LIMIT {
            RcsbFetchResult {
                status: "skipped".to_string(),
                path: rcsb_coordinate_cache_path(&options.science_store, &pdb_id),
                sha256: String::new(),
                reason: "max_fetches_per_run_reached".to_string(),
            }
        } else {
            if !policy_validated {
                if let Err(error) = validate_rcsb_on_demand_policy(&options.science_store) {
                    audit_rows.push(rcsb_fetch_audit_row(
                        &options.science_store,
                        &pdb_id,
                        "failed",
                        fetch_base_url,
                        &rcsb_coordinate_cache_path(&options.science_store, &pdb_id),
                        "",
                        &error,
                    ));
                    write_rcsb_fetch_audit(options, &audit_rows)?;
                    return Err(error);
                }
                policy_validated = true;
            }
            fetch_count += 1;
            let result = fetch_rcsb_coordinate_with_python(
                &options.science_store,
                &pdb_id,
                fetch_base_url,
                python_command,
            );
            fetch_results.insert(pdb_id.clone(), result.clone());
            result
        };
        let selected = if matches!(result.status.as_str(), "fetched" | "reused") {
            select_coordinate_backed_candidate(
                options,
                candidate.selected,
                &result.path,
                &result.status,
                &result.sha256,
                &mut selected_rows,
                &mut seen,
            )?
        } else {
            false
        };
        if selected {
            *selected_by_input.entry(input_key).or_default() += 1;
        }
        if (!initially_local || selected) && audited_pdb_ids.insert(pdb_id.clone()) {
            audit_rows.push(rcsb_fetch_audit_row(
                &options.science_store,
                &pdb_id,
                &result.status,
                fetch_base_url,
                &result.path,
                &result.sha256,
                &result.reason,
            ));
        }
    }
    write_rcsb_fetch_audit(options, &audit_rows)?;
    write_selected_complex_candidates(options, &selected_rows)?;
    write_candidate_shortlist_coordinates(options, &selected_rows)?;
    write_structure_viewer_status_rows(options, &selected_rows)?;
    Ok(!selected_rows.is_empty())
}

fn select_coordinate_backed_candidate(
    options: &ScienceOptions,
    mut selected: SelectedComplexCandidate,
    coordinate_path: &Path,
    fetch_status: &str,
    source_sha256: &str,
    selected_rows: &mut Vec<(SelectedComplexCandidate, String, String)>,
    seen: &mut BTreeSet<String>,
) -> Result<bool, String> {
    if !coordinate_file_matches_pdb(coordinate_path, &selected.pdb_id) {
        return Ok(false);
    }
    let Some(artifact) = write_candidate_coordinate_artifact(
        &options.run_dir,
        &selected.candidate_id,
        coordinate_path,
    ) else {
        return Ok(false);
    };
    let Some(coordinate_sha256) = sha256_file_hex(&options.run_dir.join(&artifact)) else {
        return Ok(false);
    };
    if selected_rows.is_empty()
        && write_coordinate_viewer_artifact(&options.run_dir, coordinate_path).is_none()
    {
        return Err("primary_structure_viewer_artifact_write_failed".to_string());
    }
    let binding = format!(
        "rcsb_fetch_manifest={RCSB_ON_DEMAND_FETCH_MANIFEST};rcsb_fetch_pdb_id={};rcsb_fetch_status={};rcsb_source_sha256={}",
        selected.pdb_id, fetch_status, source_sha256,
    );
    if !selected.evidence.is_empty() {
        selected.evidence.push(';');
    }
    selected.evidence.push_str(&binding);
    seen.insert(selected.candidate_id.clone());
    selected_rows.push((selected, artifact, coordinate_sha256));
    Ok(true)
}

fn rcsb_coordinate_url(base_url: &str, pdb_id: &str) -> String {
    format!(
        "{}/{}.cif.gz",
        base_url.trim_end_matches('/'),
        pdb_id.to_ascii_uppercase()
    )
}

fn rcsb_coordinate_cache_path(science_store: &Path, pdb_id: &str) -> PathBuf {
    let lower = pdb_id.to_ascii_lowercase();
    let split = if lower.len() >= 3 { &lower[1..3] } else { "" };
    science_store
        .join(RCSB_ON_DEMAND_CACHE_ROOT)
        .join(split)
        .join(format!("{lower}.cif.gz"))
}

fn rcsb_fetch_audit_row(
    science_store: &Path,
    pdb_id: &str,
    status: &str,
    base_url: &str,
    path: &Path,
    sha256: &str,
    reason: &str,
) -> RcsbFetchAuditRow {
    RcsbFetchAuditRow {
        pdb_id: pdb_id.to_ascii_uppercase(),
        status: status.to_string(),
        url: rcsb_coordinate_url(base_url, pdb_id),
        path: relative_display(science_store, path),
        bytes: fs::metadata(path)
            .map(|value| value.len())
            .unwrap_or_default(),
        sha256: sha256.to_string(),
        reason: reason.to_string(),
    }
}

fn write_rcsb_fetch_audit(
    options: &ScienceOptions,
    rows: &[RcsbFetchAuditRow],
) -> Result<(), String> {
    let mut text = String::from("pdb_id\tstatus\turl\tpath\tbytes\tsha256\treason\n");
    for row in rows {
        text.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            clean_cell(&row.pdb_id),
            clean_cell(&row.status),
            clean_cell(&row.url),
            clean_cell(&row.path),
            row.bytes,
            clean_cell(&row.sha256),
            clean_cell(&row.reason),
        ));
    }
    write_text(&options.run_dir.join(RCSB_ON_DEMAND_FETCH_MANIFEST), &text)
}

fn validate_rcsb_on_demand_policy(science_store: &Path) -> Result<RcsbOnDemandPolicy, String> {
    let path = science_store.join("downloads/rcsb/on_demand_policy.json");
    let value = read_text(&path)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}')).ok())
        .ok_or_else(|| "rcsb_on_demand_policy_missing_or_invalid_json".to_string())?;
    let expected_strings = [
        ("schema_version", RCSB_ON_DEMAND_POLICY_SCHEMA_VERSION),
        ("status", "ready"),
        ("mode", "on_demand"),
        ("official_base_url", RCSB_OFFICIAL_DOWNLOAD_BASE_URL),
        ("coordinate_template", RCSB_OFFICIAL_COORDINATE_TEMPLATE),
        ("cache_root", RCSB_ON_DEMAND_CACHE_ROOT),
        ("validation", RCSB_ON_DEMAND_VALIDATION),
    ];
    for (field, expected) in expected_strings {
        if value.get(field).and_then(Value::as_str) != Some(expected) {
            return Err(format!("rcsb_on_demand_policy_invalid:{field}"));
        }
    }
    if value.get("max_fetches_per_run").and_then(Value::as_u64)
        != Some(RCSB_ON_DEMAND_FETCH_LIMIT as u64)
    {
        return Err("rcsb_on_demand_policy_invalid:max_fetches_per_run".to_string());
    }
    let max_cache_bytes = value
        .get("max_cache_bytes")
        .and_then(Value::as_u64)
        .ok_or_else(|| "rcsb_on_demand_policy_invalid:max_cache_bytes".to_string())?;
    if max_cache_bytes != RCSB_ON_DEMAND_CACHE_MAX_BYTES {
        return Err("rcsb_on_demand_policy_invalid:max_cache_bytes".to_string());
    }
    let max_cache_files = value
        .get("max_cache_files")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .ok_or_else(|| "rcsb_on_demand_policy_invalid:max_cache_files".to_string())?;
    if max_cache_files != RCSB_ON_DEMAND_CACHE_MAX_FILES {
        return Err("rcsb_on_demand_policy_invalid:max_cache_files".to_string());
    }
    Ok(RcsbOnDemandPolicy {
        max_cache_bytes,
        max_cache_files,
    })
}

fn enforce_rcsb_on_demand_cache_limit(
    science_store: &Path,
    protected_path: &Path,
    policy: RcsbOnDemandPolicy,
) -> Result<(), String> {
    let cache_root = science_store.join(RCSB_ON_DEMAND_CACHE_ROOT);
    let canonical_root = fs::canonicalize(&cache_root)
        .map_err(|error| format!("rcsb_cache_root_invalid:{error}"))?;
    let canonical_protected = fs::canonicalize(protected_path)
        .map_err(|error| format!("rcsb_cache_protected_file_invalid:{error}"))?;
    if !canonical_protected.starts_with(&canonical_root)
        || !rcsb_cache_coordinate_path(&canonical_protected)
    {
        return Err("rcsb_cache_protected_file_outside_cache".to_string());
    }

    let mut entries = Vec::new();
    collect_rcsb_cache_entries(&cache_root, &mut entries)?;
    let mut total_files = entries.len();
    let mut total_bytes = entries.iter().map(|entry| entry.bytes).sum::<u64>();
    entries.sort_by(|left, right| {
        left.modified
            .cmp(&right.modified)
            .then_with(|| left.path.cmp(&right.path))
    });

    for entry in entries {
        if total_files <= policy.max_cache_files && total_bytes <= policy.max_cache_bytes {
            break;
        }
        let candidate = match fs::canonicalize(&entry.path) {
            Ok(candidate) => candidate,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                total_files = total_files.saturating_sub(1);
                total_bytes = total_bytes.saturating_sub(entry.bytes);
                continue;
            }
            Err(error) => return Err(format!("rcsb_cache_candidate_invalid:{error}")),
        };
        if candidate == canonical_protected {
            continue;
        }
        if !candidate.starts_with(&canonical_root) || !rcsb_cache_coordinate_path(&candidate) {
            return Err("rcsb_cache_candidate_outside_cache".to_string());
        }
        match fs::remove_file(&candidate) {
            Ok(()) => {
                total_files = total_files.saturating_sub(1);
                total_bytes = total_bytes.saturating_sub(entry.bytes);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                total_files = total_files.saturating_sub(1);
                total_bytes = total_bytes.saturating_sub(entry.bytes);
            }
            Err(error) => return Err(format!("rcsb_cache_eviction_failed:{error}")),
        }
    }
    if total_files > policy.max_cache_files || total_bytes > policy.max_cache_bytes {
        return Err("rcsb_cache_limit_requires_protected_file_eviction".to_string());
    }
    Ok(())
}

fn collect_rcsb_cache_entries(
    directory: &Path,
    entries: &mut Vec<RcsbCacheEntry>,
) -> Result<(), String> {
    let children = fs::read_dir(directory)
        .map_err(|error| format!("rcsb_cache_directory_unreadable:{error}"))?;
    for child in children {
        let child = child.map_err(|error| format!("rcsb_cache_entry_unreadable:{error}"))?;
        let file_type = child
            .file_type()
            .map_err(|error| format!("rcsb_cache_entry_type_unreadable:{error}"))?;
        if file_type.is_symlink() {
            continue;
        }
        let path = child.path();
        if file_type.is_dir() {
            collect_rcsb_cache_entries(&path, entries)?;
            continue;
        }
        if !file_type.is_file() || !rcsb_cache_coordinate_path(&path) {
            continue;
        }
        let metadata = child
            .metadata()
            .map_err(|error| format!("rcsb_cache_entry_metadata_unreadable:{error}"))?;
        entries.push(RcsbCacheEntry {
            path,
            bytes: metadata.len(),
            modified: metadata.modified().unwrap_or(UNIX_EPOCH),
        });
    }
    Ok(())
}

fn rcsb_cache_coordinate_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.to_ascii_lowercase().ends_with(".cif.gz"))
}

fn fetch_rcsb_coordinate_with_python(
    science_store: &Path,
    pdb_id: &str,
    base_url: &str,
    python_command: Option<&[String]>,
) -> RcsbFetchResult {
    let path = rcsb_coordinate_cache_path(science_store, pdb_id);
    if !is_pdb_id(&pdb_id.to_ascii_uppercase()) {
        return RcsbFetchResult {
            status: "failed".to_string(),
            path,
            sha256: String::new(),
            reason: "invalid_pdb_id".to_string(),
        };
    }
    let url = rcsb_coordinate_url(base_url, pdb_id);
    let args = [pdb_id.to_ascii_uppercase(), url, path.display().to_string()];
    let output = match run_python_script_with_candidates(
        rcsb_on_demand_fetch_script(),
        &args,
        "rcsb_on_demand_fetch",
        python_command
            .map(|command| vec![command.to_vec()])
            .unwrap_or_else(script_python_command_candidates),
    ) {
        Ok(output) => output,
        Err(error) => {
            return RcsbFetchResult {
                status: "failed".to_string(),
                path,
                sha256: String::new(),
                reason: format!("fetch_execution_failed:{error}"),
            };
        }
    };
    let value = output
        .lines()
        .rev()
        .find_map(|line| serde_json::from_str::<Value>(line).ok());
    let Some(value) = value else {
        return RcsbFetchResult {
            status: "failed".to_string(),
            path,
            sha256: String::new(),
            reason: "fetch_result_invalid_json".to_string(),
        };
    };
    let status = value
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("failed")
        .to_string();
    let mut sha256 = value
        .get("sha256")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_ascii_lowercase();
    let mut reason = value
        .get("reason")
        .and_then(Value::as_str)
        .unwrap_or("fetch_result_missing_reason")
        .to_string();
    if matches!(status.as_str(), "fetched" | "reused")
        && (!coordinate_file_matches_pdb(&path, pdb_id)
            || !valid_sha256_hex(&sha256)
            || sha256_file_hex(&path).as_deref() != Some(sha256.as_str()))
    {
        sha256.clear();
        reason = "post_fetch_coordinate_validation_failed".to_string();
        return RcsbFetchResult {
            status: "failed".to_string(),
            path,
            sha256,
            reason,
        };
    }
    if matches!(status.as_str(), "fetched" | "reused") {
        let cache_result = validate_rcsb_on_demand_policy(science_store)
            .and_then(|policy| enforce_rcsb_on_demand_cache_limit(science_store, &path, policy));
        if let Err(error) = cache_result {
            return RcsbFetchResult {
                status: "failed".to_string(),
                path,
                sha256: String::new(),
                reason: format!("post_fetch_cache_enforcement_failed:{error}"),
            };
        }
    }
    RcsbFetchResult {
        status,
        path,
        sha256,
        reason,
    }
}

fn rcsb_on_demand_fetch_script() -> &'static str {
    r##"import glob
import gzip
import hashlib
import json
import os
import re
import sys
import time
import urllib.error
import urllib.request
import uuid

pdb_id = sys.argv[1].upper()
url = sys.argv[2]
target = os.path.abspath(sys.argv[3])
directory = os.path.dirname(target)
lower = pdb_id.lower()
lock_path = target + ".fetch.lock"
token = f"{os.getpid()}-{uuid.uuid4().hex}"
staging = os.path.join(directory, f".{lower}.{token}.cif.gz.part")
identity = staging + ".identity.json"
lock_owned = False

def result(status, reason, sha256=""):
    print(json.dumps({
        "status": status,
        "reason": reason,
        "sha256": sha256,
    }, sort_keys=True))

def validate(path):
    entry_match = False
    has_atom_site = False
    has_x = False
    has_y = False
    has_z = False
    has_atoms = False
    pending_entry = False
    total = 0
    try:
        with gzip.open(path, "rt", encoding="utf-8", errors="replace") as handle:
            for line in handle:
                total += len(line)
                if total > 2_147_483_648:
                    return False, "uncompressed_size_limit_exceeded"
                stripped = line.strip()
                lowered = stripped.lower()
                if total < 8192 and ("<html" in lowered or "<!doctype html" in lowered):
                    return False, "html_payload_rejected"
                if lowered == f"data_{lower}":
                    entry_match = True
                if pending_entry and stripped and not stripped.startswith("#"):
                    entry_match = stripped.strip("'\"").upper() == pdb_id
                    pending_entry = False
                if lowered == "_entry.id":
                    pending_entry = True
                elif lowered.startswith("_entry.id "):
                    entry_match = stripped.split(None, 1)[1].strip("'\"").upper() == pdb_id
                if lowered.startswith("_atom_site."):
                    has_atom_site = True
                if lowered == "_atom_site.cartn_x":
                    has_x = True
                elif lowered == "_atom_site.cartn_y":
                    has_y = True
                elif lowered == "_atom_site.cartn_z":
                    has_z = True
                if stripped.startswith("ATOM ") or stripped.startswith("HETATM "):
                    has_atoms = True
    except Exception as exc:
        return False, f"gzip_validation_failed:{type(exc).__name__}:{exc}"
    if not entry_match:
        return False, "pdb_entry_id_mismatch"
    if not (has_atom_site and has_x and has_y and has_z and has_atoms):
        return False, "atom_site_coordinates_missing"
    return True, "validated"

def digest(path):
    hasher = hashlib.sha256()
    with open(path, "rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            hasher.update(chunk)
    return hasher.hexdigest()

def cleanup_own():
    for path in (staging, identity):
        try:
            os.remove(path)
        except FileNotFoundError:
            pass

def cleanup_old_identity_files():
    patterns = [
        os.path.join(directory, f".{lower}.*.cif.gz.part"),
        os.path.join(directory, f".{lower}.*.cif.gz.part.identity.json"),
        os.path.join(directory, f"{lower}.cif.gz.part*"),
        os.path.join(directory, f"{lower}.cif.gz.identity*"),
    ]
    for pattern in patterns:
        for path in glob.glob(pattern):
            if path not in (staging, identity):
                try:
                    os.remove(path)
                except FileNotFoundError:
                    pass

try:
    os.makedirs(directory, exist_ok=True)
    if os.path.isfile(target):
        valid, _ = validate(target)
        if valid:
            result("reused", "valid_cached_coordinate_reused", digest(target))
            raise SystemExit(0)

    deadline = time.monotonic() + 120.0
    while not lock_owned:
        try:
            descriptor = os.open(lock_path, os.O_CREAT | os.O_EXCL | os.O_WRONLY)
            with os.fdopen(descriptor, "w", encoding="utf-8") as handle:
                handle.write(token)
            lock_owned = True
        except FileExistsError:
            if os.path.isfile(target):
                valid, _ = validate(target)
                if valid:
                    result("reused", "valid_coordinate_published_by_concurrent_run", digest(target))
                    raise SystemExit(0)
            try:
                if time.time() - os.path.getmtime(lock_path) > 300:
                    os.remove(lock_path)
                    continue
            except FileNotFoundError:
                continue
            if time.monotonic() >= deadline:
                result("failed", "coordinate_fetch_lock_timeout")
                raise SystemExit(0)
            time.sleep(0.05)

    if os.path.isfile(target):
        valid, _ = validate(target)
        if valid:
            result("reused", "valid_coordinate_published_by_concurrent_run", digest(target))
            raise SystemExit(0)

    with open(identity, "w", encoding="utf-8") as handle:
        json.dump({"pdb_id": pdb_id, "url": url, "target": target, "staging": staging}, handle)
    request = urllib.request.Request(url, headers={"User-Agent": "GenoLigand/rcsb-on-demand"})
    try:
        with urllib.request.urlopen(request, timeout=30) as response, open(staging, "wb") as output:
            content_type = (response.headers.get("Content-Type") or "").lower()
            if "text/html" in content_type:
                result("failed", "html_content_type_rejected")
                raise SystemExit(0)
            total = 0
            while True:
                chunk = response.read(1024 * 1024)
                if not chunk:
                    break
                total += len(chunk)
                if total > 1_073_741_824:
                    result("failed", "compressed_size_limit_exceeded")
                    raise SystemExit(0)
                output.write(chunk)
            output.flush()
            os.fsync(output.fileno())
    except urllib.error.HTTPError as exc:
        if exc.code == 404:
            result("not_found", "http_404")
        else:
            result("failed", f"http_error:{exc.code}")
        raise SystemExit(0)
    except (urllib.error.URLError, TimeoutError, OSError) as exc:
        result("failed", f"network_error:{type(exc).__name__}:{exc}")
        raise SystemExit(0)

    valid, reason = validate(staging)
    if not valid:
        result("failed", f"validation_failed:{reason}")
        raise SystemExit(0)
    sha256 = digest(staging)
    os.replace(staging, target)
    cleanup_old_identity_files()
    result("fetched", "validated_and_atomically_published", sha256)
finally:
    cleanup_own()
    if lock_owned:
        try:
            with open(lock_path, "r", encoding="utf-8") as handle:
                owned = handle.read() == token
            if owned:
                os.remove(lock_path)
        except FileNotFoundError:
            pass
"##
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
            record_id: cols
                .get(13)
                .filter(|value| !value.trim().is_empty())
                .cloned()
                .unwrap_or_else(|| "record_1".to_string()),
            target_id: cols[2].clone(),
            target_name: cols[3].clone(),
            organism: cols[4].clone(),
            target_class: cols[5].clone(),
            match_score: cols[6].parse().unwrap_or(0.0),
            evidence: cols[8].clone(),
            pdb_ids,
        };
        let key = format!("{}\t{}\t{}", hint.input_id, hint.record_id, hint.target_id);
        match by_target.get(&key) {
            Some(existing) if known_hint_rank(existing) >= known_hint_rank(&hint) => {}
            _ => {
                by_target.insert(key, hint);
            }
        }
    }
    Ok(by_target.into_values().collect())
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
    let declared_relation = value
        .split(';')
        .find_map(|part| part.trim().strip_prefix("relation="))
        .map(str::trim);
    if declared_relation.is_some_and(|relation| relation != "=") {
        return None;
    }
    for part in value.split(';').map(str::trim) {
        for activity in ["Ki", "IC50", "Kd", "EC50"] {
            let Some(mut tail) = part.strip_prefix(activity).map(str::trim_start) else {
                continue;
            };
            let mut unit = "nM";
            if let Some(without_open) = tail.strip_prefix('(') {
                let Some(close) = without_open.find(')') else {
                    continue;
                };
                unit = without_open[..close].trim();
                tail = without_open[close + 1..].trim_start();
            }
            let Some(without_equals) = tail.strip_prefix('=') else {
                continue;
            };
            tail = without_equals.trim_start();
            if tail
                .chars()
                .next()
                .is_some_and(|ch| matches!(ch, '=' | '<' | '>' | '~'))
            {
                return None;
            }
            let number = tail
                .chars()
                .take_while(|ch| ch.is_ascii_digit() || matches!(*ch, '.' | 'e' | 'E' | '+' | '-'))
                .collect::<String>();
            let parsed = number.parse::<f64>().ok()?;
            let suffix = tail[number.len()..].trim_start();
            if !suffix.is_empty() {
                unit = suffix
                    .split_whitespace()
                    .next()
                    .unwrap_or(unit)
                    .trim_matches(|ch: char| !ch.is_alphabetic() && ch != 'µ' && ch != 'μ');
            }
            let multiplier = match unit.to_ascii_lowercase().as_str() {
                "pm" => 0.001,
                "nm" | "" => 1.0,
                "um" => 1_000.0,
                "mm" => 1_000_000.0,
                "m" => 1_000_000_000.0,
                _ if matches!(unit, "µM" | "μM") => 1_000.0,
                _ => return None,
            };
            let normalized = parsed * multiplier;
            return normalized.is_finite().then_some(normalized);
        }
    }
    None
}

fn coordinate_hint_score(path: &Path, hint: &KnownStructureHint) -> f64 {
    let mut score = hint.match_score;
    if let Ok(metadata) = read_structure_metadata_best_effort(path) {
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

fn write_selected_complex_candidates(
    options: &ScienceOptions,
    rows: &[(SelectedComplexCandidate, String, String)],
) -> Result<(), String> {
    let mut text = String::from(
        "candidate_id\tidentity_namespace\tinput_id\trecord_id\ttarget_id\ttarget_name\torganism\ttarget_class\tsource\tpdb_id\tevidence\tselection_rank\treceptor_artifact\tcoordinate_sha256\n",
    );
    let mut ranks = BTreeMap::<(String, String), usize>::new();
    for (selected, artifact, coordinate_sha256) in rows {
        let rank = ranks
            .entry((selected.input_id.clone(), selected.record_id.clone()))
            .and_modify(|value| *value += 1)
            .or_insert(1);
        text.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            clean_cell(&selected.candidate_id),
            clean_cell(&selected.identity_namespace),
            clean_cell(&selected.input_id),
            clean_cell(&selected.record_id),
            clean_cell(&selected.target_id),
            clean_cell(&selected.target_name),
            clean_cell(&selected.organism),
            clean_cell(&selected.target_class),
            clean_cell(&selected.source),
            clean_cell(&selected.pdb_id),
            clean_cell(&selected.evidence),
            *rank,
            clean_cell(artifact),
            coordinate_sha256,
        ));
    }
    write_text(
        &options.run_dir.join("complex_candidate_selection.tsv"),
        &text,
    )
}

fn write_candidate_shortlist_coordinates(
    options: &ScienceOptions,
    selected_rows: &[(SelectedComplexCandidate, String, String)],
) -> Result<(), String> {
    let path = options.run_dir.join("candidate_shortlist.tsv");
    let mut rows = read_simple_tsv_rows(&path)?;
    let mut ranks = BTreeMap::<(String, String), usize>::new();
    let mut selected =
        BTreeMap::<String, (&SelectedComplexCandidate, &String, &String, usize)>::new();
    for (candidate, artifact, coordinate_sha256) in selected_rows {
        let rank = ranks
            .entry((candidate.input_id.clone(), candidate.record_id.clone()))
            .and_modify(|value| *value += 1)
            .or_insert(1);
        selected.insert(
            candidate.candidate_id.clone(),
            (candidate, artifact, coordinate_sha256, *rank),
        );
    }
    for row in &mut rows {
        let candidate_id = row.get("candidate_id").cloned().unwrap_or_default();
        if let Some((candidate, artifact, coordinate_sha256, selection_rank)) =
            selected.get(&candidate_id)
        {
            row.insert("coordinate_status".to_string(), "ready".to_string());
            row.insert("selection_rank".to_string(), selection_rank.to_string());
            row.insert("receptor_artifact".to_string(), (*artifact).clone());
            row.insert(
                "coordinate_sha256".to_string(),
                (*coordinate_sha256).clone(),
            );
            row.insert("pdb_id".to_string(), candidate.pdb_id.clone());
        } else {
            let status = if row
                .get("pdb_ids")
                .is_some_and(|value| !value.trim().is_empty())
            {
                "not_selected_or_unavailable"
            } else {
                "not_available"
            };
            row.insert("coordinate_status".to_string(), status.to_string());
            for field in [
                "selection_rank",
                "receptor_artifact",
                "coordinate_sha256",
                "pdb_id",
            ] {
                row.insert(field.to_string(), String::new());
            }
        }
    }
    let headers = CANDIDATE_SHORTLIST_HEADER.split('\t').collect::<Vec<_>>();
    let mut text = format!("{CANDIDATE_SHORTLIST_HEADER}\n");
    for row in rows {
        text.push_str(
            &headers
                .iter()
                .map(|header| clean_cell(row.get(*header).map(String::as_str).unwrap_or("")))
                .collect::<Vec<_>>()
                .join("\t"),
        );
        text.push('\n');
    }
    write_text(&path, &text)
}

fn write_structure_viewer_status_rows(
    options: &ScienceOptions,
    rows: &[(SelectedComplexCandidate, String, String)],
) -> Result<(), String> {
    let mut text = String::from(
        "status\ttarget_id\tpdb_id\tartifact\texpected_path\treason\tcandidate_id\tinput_id\trecord_id\tselection_rank\tcoordinate_sha256\n",
    );
    if rows.is_empty() {
        text.push_str("not_ready\t\t\t\t\tno coordinate-backed candidate for this run\t\t\t\t\t\n");
    }
    let mut ranks = BTreeMap::<(String, String), usize>::new();
    for (selected, artifact, coordinate_sha256) in rows {
        let rank = ranks
            .entry((selected.input_id.clone(), selected.record_id.clone()))
            .and_modify(|value| *value += 1)
            .or_insert(1);
        text.push_str(&format!(
            "ready\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            clean_cell(&selected.target_id),
            clean_cell(&selected.pdb_id),
            clean_cell(artifact),
            clean_cell(artifact),
            "coordinate artifact bound to selected candidate",
            clean_cell(&selected.candidate_id),
            clean_cell(&selected.input_id),
            clean_cell(&selected.record_id),
            *rank,
            coordinate_sha256,
        ));
    }
    write_text(&options.run_dir.join("structure_viewer_status.tsv"), &text)
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
        .find(|path| path.is_file() && coordinate_file_matches_pdb(path, pdb_id))
}

fn coordinate_file_matches_pdb(path: &Path, pdb_id: &str) -> bool {
    let Ok(text) = read_maybe_gz_text(path) else {
        return false;
    };
    let prefix = text
        .chars()
        .take(8192)
        .collect::<String>()
        .to_ascii_lowercase();
    if prefix.contains("<html") || prefix.contains("<!doctype html") {
        return false;
    }
    if !coordinate_text_has_atoms(&text) {
        return false;
    }
    coordinate_text_identity_matches(path, &text, pdb_id)
}

fn coordinate_file_identity_matches_pdb(path: &Path, pdb_id: &str) -> bool {
    let Ok(text) = read_maybe_gz_prefix(path, 64 * 1024) else {
        return false;
    };
    let prefix = text.to_ascii_lowercase();
    if prefix.contains("<html") || prefix.contains("<!doctype html") {
        return false;
    }
    coordinate_text_identity_matches(path, &text, pdb_id)
}

fn coordinate_text_identity_matches(path: &Path, text: &str, pdb_id: &str) -> bool {
    let name = lower_file_name(path);
    if name.contains(".pdb") || name.contains(".ent") {
        coordinate_pdb_header_matches(text, pdb_id)
    } else {
        coordinate_mmcif_entry_matches(text, pdb_id)
    }
}

fn coordinate_mmcif_entry_matches(text: &str, pdb_id: &str) -> bool {
    let expected = pdb_id.to_ascii_uppercase();
    let mut declared = false;
    let mut pending_entry_id = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if pending_entry_id && !trimmed.is_empty() && !trimmed.starts_with('#') {
            declared = true;
            let value = trimmed
                .split_whitespace()
                .next()
                .unwrap_or_default()
                .trim_matches(['\'', '"']);
            if !value.eq_ignore_ascii_case(&expected) {
                return false;
            }
            pending_entry_id = false;
        }
        if let Some(value) = trimmed
            .get(..5)
            .filter(|prefix| prefix.eq_ignore_ascii_case("data_"))
            .and_then(|_| trimmed.get(5..))
        {
            declared = true;
            if !value.trim().eq_ignore_ascii_case(&expected) {
                return false;
            }
        }
        let mut fields = trimmed.split_whitespace();
        if fields
            .next()
            .is_some_and(|field| field.eq_ignore_ascii_case("_entry.id"))
        {
            if let Some(value) = fields.next() {
                declared = true;
                if !value
                    .trim_matches(['\'', '"'])
                    .eq_ignore_ascii_case(&expected)
                {
                    return false;
                }
            } else {
                pending_entry_id = true;
            }
        }
    }
    declared && !pending_entry_id
}

fn coordinate_pdb_header_matches(text: &str, pdb_id: &str) -> bool {
    let expected = pdb_id.to_ascii_uppercase();
    let mut declared = false;
    for line in text.lines() {
        if !line
            .get(..6)
            .is_some_and(|record| record.eq_ignore_ascii_case("HEADER"))
        {
            continue;
        }
        declared = true;
        let Some(value) = line.get(62..66).map(str::trim) else {
            return false;
        };
        if !value.eq_ignore_ascii_case(&expected) {
            return false;
        }
    }
    declared
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

fn write_candidate_coordinate_artifact(
    run_dir: &Path,
    candidate_id: &str,
    source: &Path,
) -> Option<String> {
    let text = read_maybe_gz_text(source).ok()?;
    let lower = lower_file_name(source);
    let extension = if lower.contains(".json") {
        if !text.contains("\"atoms\"") {
            return None;
        }
        "json"
    } else if lower.contains(".pdb") || lower.contains(".ent") {
        if !coordinate_text_has_atoms(&text) {
            return None;
        }
        "pdb"
    } else {
        if !coordinate_text_has_atoms(&text) {
            return None;
        }
        "cif"
    };
    let relative = PathBuf::from("structure_candidates")
        .join(safe_id(candidate_id))
        .join(format!("receptor.{extension}"));
    let path = run_dir.join(&relative);
    ensure_dir(path.parent()?).ok()?;
    write_text(&path, &text).ok()?;
    Some(relative_display(run_dir, &path))
}

fn read_index_matches(path: &Path) -> Result<Vec<IndexMatch>, String> {
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let rows = read_simple_tsv_rows(path)?;
    Ok(rows
        .into_iter()
        .filter_map(|row| {
            let input_id = row.get("input_id")?.trim().to_string();
            let target_id = row.get("target_id")?.trim().to_string();
            if input_id.is_empty() || target_id.is_empty() {
                return None;
            }
            Some(IndexMatch {
                input_id,
                record_id: row
                    .get("record_id")
                    .filter(|value| !value.trim().is_empty())
                    .cloned()
                    .unwrap_or_else(|| "record_1".to_string()),
                source: row.get("source").cloned().unwrap_or_default(),
                target_id,
                target_name: row.get("target_name").cloned().unwrap_or_default(),
                organism: row.get("organism").cloned().unwrap_or_default(),
                target_class: row.get("target_class").cloned().unwrap_or_default(),
                match_score: row
                    .get("match_score")
                    .and_then(|value| value.parse::<f64>().ok())
                    .unwrap_or_default(),
                matched_features: row.get("matched_features").cloned().unwrap_or_default(),
                evidence: row.get("evidence").cloned().unwrap_or_default(),
            })
        })
        .collect())
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

fn read_maybe_gz_prefix(path: &Path, max_bytes: u64) -> Result<String, String> {
    let file =
        fs::File::open(path).map_err(|err| format!("open_failed:{}:{err}", path.display()))?;
    let mut bytes = Vec::new();
    if lower_file_name(path).ends_with(".gz") {
        GzDecoder::new(file)
            .take(max_bytes)
            .read_to_end(&mut bytes)
            .map_err(|err| format!("gzip_read_failed:{}:{err}", path.display()))?;
    } else {
        file.take(max_bytes)
            .read_to_end(&mut bytes)
            .map_err(|err| format!("read_failed:{}:{err}", path.display()))?;
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn coordinate_text_has_atoms(text: &str) -> bool {
    text.lines()
        .any(|line| line.starts_with("ATOM") || line.starts_with("HETATM"))
}

fn screen_dynamics(options: &ScienceOptions) -> Result<String, String> {
    let summary = materialize_current_md_summary(options)?;
    Ok(format!(
        "dynamics_screened\tmd_artifact={}",
        if summary.ready { "ready" } else { "not_ready" }
    ))
}

fn execute_dynamics_screen(options: &ScienceOptions) -> Result<String, String> {
    let (runnable, not_applicable) = selected_dynamics_candidate_rows(options)?;
    clear_worker_task_outputs(options, &DOCKING_WORKER_TASK)?;
    run_docking_candidate_workers(options)?;
    copy_tool_output_or_header(
        options,
        "docking_summary.tsv",
        "candidate_id\tdocking_status\tdocking_score\tdocking_engine\tnotes\n",
        &options.run_dir.join("docking_summary.tsv"),
    )?;
    run_scientific_workers(options)?;
    validate_dynamics_not_applicable_outcomes(options)?;
    let result = screen_dynamics(options)?;
    let status = if runnable.is_empty() && !not_applicable.is_empty() {
        "not_applicable"
    } else if !runnable.is_empty() {
        "executed"
    } else {
        "no_candidate"
    };
    Ok(format!(
        "{result}\tdynamics_status={status}\tnot_applicable_count={}",
        not_applicable.len()
    ))
}

fn validate_dynamics_not_applicable_outcomes(options: &ScienceOptions) -> Result<(), String> {
    let (_, not_applicable) = selected_dynamics_candidate_rows(options)?;
    let expected = not_applicable
        .into_iter()
        .map(|outcome| outcome.candidate_id)
        .collect::<BTreeSet<_>>();
    for kind in ["docking", "md"] {
        let path = options
            .run_dir
            .join(format!("tool_outputs/{kind}_outcomes.tsv"));
        let rows = read_simple_tsv_rows(&path)?;
        let actual = rows
            .into_iter()
            .filter(|row| {
                row.get("status").map(String::as_str) == Some("not_applicable")
                    && row.get("reason").is_some_and(|reason| {
                        reason.starts_with("input_kind_not_supported_for_docking_md:")
                    })
            })
            .filter_map(|row| row.get("candidate_id").cloned())
            .collect::<BTreeSet<_>>();
        if actual != expected {
            return Err(format!("{kind}_not_applicable_outcomes_incomplete"));
        }
    }
    Ok(())
}

fn md_summary_header() -> &'static str {
    "candidate_id\trmsd\trmsf\tligand_drift\thydrogen_bond_occupancy\tcontact_occupancy\tinteraction_persistence\tmd_risk_flags\tmd_score_decomposition\tmd_stability_score\n"
}

fn write_md_summary_from_metric_artifacts(
    options: &ScienceOptions,
    out_path: &Path,
    current_artifacts: &BTreeSet<String>,
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
        for (candidate, value) in read_metric_values(options, files, current_artifacts)? {
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
        let required = [
            "rmsd",
            "rmsf",
            "ligand_drift",
            "hydrogen_bond_occupancy",
            "contact_occupancy",
        ];
        if required.iter().any(|field| {
            metrics
                .get(*field)
                .and_then(|value| value.trim().parse::<f64>().ok())
                .is_none_or(|value| !value.is_finite())
        }) {
            return Err(format!("md_metric_set_incomplete_or_invalid:{candidate}"));
        }
        let rmsd = metrics.get("rmsd").expect("validated metric");
        let rmsf = metrics.get("rmsf").expect("validated metric");
        let drift = metrics.get("ligand_drift").expect("validated metric");
        let hbond = metrics
            .get("hydrogen_bond_occupancy")
            .expect("validated metric");
        let contact = metrics.get("contact_occupancy").expect("validated metric");
        text.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t\t\tmetric_artifacts\t\n",
            clean_cell(&candidate),
            clean_cell(rmsd),
            clean_cell(rmsf),
            clean_cell(drift),
            clean_cell(hbond),
            clean_cell(contact)
        ));
    }
    write_text(out_path, &text)?;
    Ok(true)
}

fn read_metric_values(
    options: &ScienceOptions,
    file_names: &[&str],
    current_artifacts: &BTreeSet<String>,
) -> Result<BTreeMap<String, String>, String> {
    let mut values = BTreeMap::new();
    for file_name in file_names {
        if !current_artifacts.contains(&format!("tool_outputs/{file_name}")) {
            continue;
        }
        let path = options.run_dir.join("tool_outputs").join(file_name);
        if !path.exists() {
            continue;
        }
        for row in read_simple_tsv_rows(&path)? {
            let candidate =
                first_metric_value(&row, &["candidate_id", "candidate", "target_id", "id"])
                    .ok_or_else(|| format!("md_metric_candidate_missing:{file_name}"))?;
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

struct CurrentMdWorkerArtifacts {
    artifacts: BTreeSet<String>,
    candidate_id: Option<String>,
    status: String,
    reason: String,
}

#[derive(Clone, Debug)]
struct CurrentMdRunFacts {
    run_id: String,
    candidate_id: String,
    candidate_digest: String,
    input_id: String,
    record_id: String,
    input_digest: String,
    source_digest: String,
}

struct CurrentMdSummary {
    ready: bool,
    candidate_id: Option<String>,
}

#[derive(Clone, Debug)]
struct VerifiedMdFacts {
    candidate_id: String,
    platform: String,
    protocol: String,
    state: String,
    trajectory: String,
    telemetry: Option<String>,
    gpu_observed: bool,
    force_field: Option<String>,
    solvent_model: Option<String>,
    nonbonded_method: Option<String>,
}

#[derive(Clone, Debug)]
struct VerifiedPeptideConformerFacts {
    candidate_id: String,
    conformer_count: usize,
    manifest: String,
}

#[derive(Clone, Debug, Default)]
struct VerifiedIndexedMdComputations {
    complex: Vec<VerifiedMdFacts>,
    peptide: Vec<VerifiedPeptideConformerFacts>,
    not_ready_reasons: Vec<String>,
}

#[derive(Clone, Debug)]
struct ValidatedCandidateIndexEntry {
    facts: CurrentMdRunFacts,
    job_id: String,
    invocation_id: String,
    status: String,
    reason: String,
    manifest: String,
    artifact_provenance: String,
}

fn materialize_current_md_summary(options: &ScienceOptions) -> Result<CurrentMdSummary, String> {
    if options
        .run_dir
        .join("tool_outputs/md_manifest_index.json")
        .is_file()
    {
        return materialize_indexed_md_summary(options);
    }
    let md_path = options.run_dir.join("md_summary.tsv");
    let binding = current_md_worker_artifacts(options)?;
    if binding.status == "PASS"
        && current_md_method(options).as_deref() != Some("openmm_receptor_ligand_complex_dynamics")
    {
        write_text(&md_path, md_summary_header())?;
        write_md_summary_status(
            options,
            "not_applicable",
            "worker method is not receptor-ligand complex molecular dynamics",
        )?;
        return Ok(CurrentMdSummary {
            ready: false,
            candidate_id: None,
        });
    }
    let produced = binding.status == "PASS"
        && ((binding.artifacts.contains("tool_outputs/md_summary.tsv")
            && copy_tool_output_if_available(options, "md_summary.tsv", &md_path)?)
            || write_md_summary_from_metric_artifacts(options, &md_path, &binding.artifacts)?);
    let ready = if produced {
        let candidate_id = binding
            .candidate_id
            .as_deref()
            .ok_or_else(|| "md_summary_candidate_missing".to_string())?;
        validate_md_summary_semantics(options, &md_path, candidate_id, &binding.artifacts)?;
        true
    } else {
        false
    };
    if ready {
        write_md_summary_status(
            options,
            "ready",
            "real MD summary artifact available for this run",
        )?;
    } else {
        write_text(&md_path, md_summary_header())?;
        let status = match binding.status.as_str() {
            "FAIL" => "fail",
            "NOT_READY" => "not_ready",
            "NOT_APPLICABLE" => "not_applicable",
            _ => "not_ready",
        };
        let reason = if matches!(
            binding.status.as_str(),
            "FAIL" | "NOT_READY" | "NOT_APPLICABLE"
        ) {
            binding.reason
        } else {
            let missing = md_not_ready_reason(&binding.artifacts);
            match (missing.is_empty(), binding.reason.is_empty()) {
                (true, _) => binding.reason,
                (_, true) => missing,
                _ => format!("{missing};{}", binding.reason),
            }
        };
        write_md_summary_status(options, status, &reason)?;
    }
    Ok(CurrentMdSummary {
        ready,
        candidate_id: ready.then_some(binding.candidate_id).flatten(),
    })
}

fn materialize_indexed_md_summary(options: &ScienceOptions) -> Result<CurrentMdSummary, String> {
    let verified = verified_indexed_md_computations(options)?;
    let completed_ids = verified
        .complex
        .iter()
        .map(|facts| facts.candidate_id.clone())
        .collect::<BTreeSet<_>>();
    let md_path = options.run_dir.join("md_summary.tsv");
    if completed_ids.is_empty() {
        write_text(&md_path, md_summary_header())?;
        let (status, reason) = if verified.peptide.is_empty() {
            ("not_ready", verified.not_ready_reasons.join(";"))
        } else {
            (
                "not_applicable",
                format!(
                    "{} peptide conformer ensemble(s) completed without receptor-ligand MD",
                    verified.peptide.len()
                ),
            )
        };
        write_md_summary_status(options, status, &reason)?;
        return Ok(CurrentMdSummary {
            ready: false,
            candidate_id: None,
        });
    }
    let source = options.run_dir.join("tool_outputs/md_summary.tsv");
    if !source.is_file() {
        return Err("md_summary_missing_for_completed_candidates".to_string());
    }
    let summary_rows = read_simple_tsv_rows(&source)?;
    let summary_ids = summary_rows
        .iter()
        .filter_map(|row| row.get("candidate_id").cloned())
        .collect::<BTreeSet<_>>();
    if summary_rows.len() != summary_ids.len() || summary_ids != completed_ids {
        return Err("md_summary_completed_candidate_mismatch".to_string());
    }
    let scores = read_md_scores(&source)?;
    if scores.keys().cloned().collect::<BTreeSet<_>>() != completed_ids {
        return Err("md_summary_score_candidate_mismatch".to_string());
    }
    write_text(&md_path, &read_text(&source)?)?;
    write_md_summary_status(
        options,
        "ready",
        "candidate-bound MD summaries verified through manifest index",
    )?;
    Ok(CurrentMdSummary {
        ready: true,
        candidate_id: (completed_ids.len() == 1)
            .then(|| completed_ids.into_iter().next())
            .flatten(),
    })
}

fn validated_candidate_manifest_index(
    options: &ScienceOptions,
    kind: &str,
    base_task: &WorkerTask<'_>,
) -> Result<Vec<ValidatedCandidateIndexEntry>, String> {
    let schema_version = format!("genoligand_{kind}_manifest_index_v1");
    let index_path = options
        .run_dir
        .join(format!("tool_outputs/{kind}_manifest_index.json"));
    let index = read_json_value_if_file(&index_path)
        .ok_or_else(|| format!("{kind}_manifest_index_invalid"))?;
    if index.get("schema_version").and_then(Value::as_str) != Some(&schema_version)
        || index.get("run_id").and_then(Value::as_str)
            != Some(infer_run_id(&options.run_dir).as_str())
    {
        return Err(format!("{kind}_manifest_index_identity_mismatch"));
    }
    let (selected_rows, _) = selected_dynamics_candidate_rows(options)?;
    let mut selected = BTreeMap::new();
    for row in selected_rows {
        let facts = selected_candidate_run_facts(options, &row)?;
        if selected
            .insert(facts.candidate_id.clone(), (facts, row))
            .is_some()
        {
            return Err("selected_candidate_identity_duplicate".to_string());
        }
    }
    let entries = index
        .get("entries")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("{kind}_manifest_index_entries_invalid"))?;
    let mut seen = BTreeSet::new();
    let mut validated = Vec::new();
    for entry in entries {
        let candidate_id = entry
            .get("candidate_id")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| format!("{kind}_manifest_index_candidate_missing"))?;
        let (facts, selected_row) = selected
            .get(candidate_id)
            .ok_or_else(|| format!("{kind}_manifest_index_candidate_mismatch:{candidate_id}"))?;
        if !seen.insert(candidate_id.to_string()) {
            return Err(format!(
                "{kind}_manifest_index_candidate_duplicate:{candidate_id}"
            ));
        }
        for (field, expected) in [
            ("input_id", facts.input_id.as_str()),
            ("record_id", facts.record_id.as_str()),
            ("candidate_digest", facts.candidate_digest.as_str()),
            ("input_digest", facts.input_digest.as_str()),
            ("source_digest", facts.source_digest.as_str()),
        ] {
            if entry.get(field).and_then(Value::as_str) != Some(expected) {
                return Err(format!(
                    "{kind}_manifest_index_selection_mismatch:{candidate_id}:{field}"
                ));
            }
        }
        let job_id = entry
            .get("job_id")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| format!("{kind}_manifest_index_job_missing:{candidate_id}"))?
            .to_string();
        if job_id != candidate_worker_job_id(base_task.job_id, candidate_id) {
            return Err(format!("{kind}_manifest_index_job_mismatch:{candidate_id}"));
        }
        let invocation_id = entry
            .get("invocation_id")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| format!("{kind}_manifest_index_invocation_missing:{candidate_id}"))?
            .to_string();
        let status = entry
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if !matches!(status.as_str(), "completed" | "not_ready" | "failed") {
            return Err(format!(
                "{kind}_manifest_index_status_invalid:{candidate_id}:{status}"
            ));
        }
        let reason = entry
            .get("reason")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| format!("{kind}_manifest_index_reason_missing:{candidate_id}"))?
            .to_string();
        let task = WorkerTask {
            job_id: &job_id,
            worker: base_task.worker,
            module: base_task.module,
            stage: base_task.stage,
        };
        let job_path = options
            .run_dir
            .join("worker_jobs")
            .join(format!("{job_id}.json"));
        let result_path = options
            .run_dir
            .join("worker_results")
            .join(format!("{job_id}.json"));
        let job = read_json_value_if_file(&job_path)
            .ok_or_else(|| format!("{kind}_candidate_job_invalid:{candidate_id}"))?;
        if !validate_candidate_worker_job(&job, &task, options)
            || !job_run_identity_matches(&job, options)
            || job.get("invocation_id").and_then(Value::as_str) != Some(invocation_id.as_str())
        {
            return Err(format!(
                "{kind}_candidate_job_binding_invalid:{candidate_id}"
            ));
        }
        for (field, expected) in [
            ("run_id", facts.run_id.as_str()),
            ("candidate_id", facts.candidate_id.as_str()),
            ("candidate_digest", facts.candidate_digest.as_str()),
            ("input_id", facts.input_id.as_str()),
            ("record_id", facts.record_id.as_str()),
            ("input_digest", facts.input_digest.as_str()),
            ("source_digest", facts.source_digest.as_str()),
        ] {
            if job.get(field).and_then(Value::as_str) != Some(expected) {
                return Err(format!(
                    "{kind}_candidate_job_identity_mismatch:{candidate_id}:{field}"
                ));
            }
        }
        if kind == "docking" {
            for (field, selected_field) in [
                ("receptor_artifact", "receptor_artifact"),
                ("receptor_digest", "coordinate_sha256"),
            ] {
                if job.get(field).and_then(Value::as_str)
                    != selected_row.get(selected_field).map(String::as_str)
                {
                    return Err(format!(
                        "docking_candidate_receptor_mismatch:{candidate_id}:{field}"
                    ));
                }
            }
        }
        let result = read_json_value_if_file(&result_path)
            .ok_or_else(|| format!("{kind}_candidate_result_invalid:{candidate_id}"))?;
        if !validate_candidate_worker_result(&result, &job, &task) {
            return Err(format!(
                "{kind}_candidate_result_binding_invalid:{candidate_id}"
            ));
        }
        let result_status = result
            .get("execution_status")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let normalized_result_status = if result_status == "not_applicable" {
            "not_ready"
        } else {
            result_status
        };
        if normalized_result_status != status
            || result.get("reason").and_then(Value::as_str) != Some(reason.as_str())
        {
            return Err(format!(
                "{kind}_candidate_result_status_mismatch:{candidate_id}"
            ));
        }
        let manifest = entry
            .get("manifest")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let artifact_provenance = entry
            .get("artifact_provenance")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let result_manifest = result
            .get("manifest")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let result_provenance = result
            .get("artifact_provenance")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if (!result_manifest.is_empty() && result_manifest != manifest)
            || (!result_provenance.is_empty() && result_provenance != artifact_provenance)
            || (status == "completed" && manifest.is_empty())
            || (kind == "md"
                && matches!(result_status, "completed" | "not_ready")
                && artifact_provenance.is_empty())
        {
            return Err(format!(
                "{kind}_candidate_artifact_index_mismatch:{candidate_id}"
            ));
        }
        validated.push(ValidatedCandidateIndexEntry {
            facts: facts.clone(),
            job_id,
            invocation_id,
            status,
            reason,
            manifest,
            artifact_provenance,
        });
    }
    if seen != selected.keys().cloned().collect::<BTreeSet<_>>() {
        return Err(format!("{kind}_manifest_index_incomplete"));
    }
    Ok(validated)
}

fn validated_md_artifact_provenance(
    options: &ScienceOptions,
    entry: &ValidatedCandidateIndexEntry,
) -> Result<BTreeSet<String>, String> {
    let provenance_relative =
        current_run_artifact_relative(&options.run_dir, &entry.artifact_provenance)
            .ok_or_else(|| format!("md_provenance_invalid:{}", entry.facts.candidate_id))?;
    let provenance = read_json_value_if_file(&options.run_dir.join(&provenance_relative))
        .ok_or_else(|| format!("md_provenance_invalid:{}", entry.facts.candidate_id))?;
    if provenance.get("schema_version").and_then(Value::as_str)
        != Some(MD_PROVENANCE_SCHEMA_VERSION)
    {
        return Err(format!(
            "md_provenance_schema_mismatch:{}",
            entry.facts.candidate_id
        ));
    }
    for (field, expected) in [
        ("run_id", entry.facts.run_id.as_str()),
        ("job_id", entry.job_id.as_str()),
        ("invocation_id", entry.invocation_id.as_str()),
        ("candidate_id", entry.facts.candidate_id.as_str()),
        ("candidate_digest", entry.facts.candidate_digest.as_str()),
        ("input_id", entry.facts.input_id.as_str()),
        ("record_id", entry.facts.record_id.as_str()),
        ("input_digest", entry.facts.input_digest.as_str()),
        ("source_digest", entry.facts.source_digest.as_str()),
    ] {
        if provenance.get(field).and_then(Value::as_str) != Some(expected) {
            return Err(format!(
                "md_provenance_identity_mismatch:{}:{field}",
                entry.facts.candidate_id
            ));
        }
    }
    let parameters_digest = provenance
        .get("parameters_digest")
        .and_then(Value::as_str)
        .filter(|value| valid_sha256_hex(value))
        .ok_or_else(|| {
            format!(
                "md_provenance_parameters_invalid:{}",
                entry.facts.candidate_id
            )
        })?;
    let tool_name = provenance
        .get("tool_name")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("md_provenance_tool_invalid:{}", entry.facts.candidate_id))?;
    let tool_version = provenance
        .get("tool_version")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("md_provenance_tool_invalid:{}", entry.facts.candidate_id))?;
    let rows = provenance
        .get("artifacts")
        .and_then(Value::as_array)
        .filter(|rows| !rows.is_empty())
        .ok_or_else(|| {
            format!(
                "md_provenance_artifacts_missing:{}",
                entry.facts.candidate_id
            )
        })?;
    let mut artifacts = BTreeSet::new();
    for row in rows {
        for (field, expected) in [
            ("run_id", entry.facts.run_id.as_str()),
            ("job_id", entry.job_id.as_str()),
            ("invocation_id", entry.invocation_id.as_str()),
            ("candidate_id", entry.facts.candidate_id.as_str()),
            ("candidate_digest", entry.facts.candidate_digest.as_str()),
            ("input_id", entry.facts.input_id.as_str()),
            ("record_id", entry.facts.record_id.as_str()),
            ("input_digest", entry.facts.input_digest.as_str()),
            ("source_digest", entry.facts.source_digest.as_str()),
            ("parameters_digest", parameters_digest),
            ("tool_name", tool_name),
            ("tool_version", tool_version),
        ] {
            if row.get(field).and_then(Value::as_str) != Some(expected) {
                return Err(format!(
                    "md_provenance_artifact_identity_mismatch:{}:{field}",
                    entry.facts.candidate_id
                ));
            }
        }
        let artifact = row
            .get("artifact_path")
            .and_then(Value::as_str)
            .and_then(|path| current_run_artifact_relative(&options.run_dir, path))
            .ok_or_else(|| {
                format!(
                    "md_provenance_artifact_invalid:{}",
                    entry.facts.candidate_id
                )
            })?;
        let expected_digest = row
            .get("artifact_sha256")
            .and_then(Value::as_str)
            .filter(|value| valid_sha256_hex(value))
            .ok_or_else(|| {
                format!(
                    "md_provenance_artifact_digest_missing:{}",
                    entry.facts.candidate_id
                )
            })?;
        let actual_digest = sha256_file_hex(&options.run_dir.join(&artifact)).ok_or_else(|| {
            format!(
                "md_provenance_artifact_unreadable:{}:{artifact}",
                entry.facts.candidate_id
            )
        })?;
        if !actual_digest.eq_ignore_ascii_case(expected_digest) {
            return Err(format!(
                "md_provenance_artifact_digest_mismatch:{}:{artifact}",
                entry.facts.candidate_id
            ));
        }
        if !artifacts.insert(artifact.clone()) {
            return Err(format!(
                "md_provenance_artifact_duplicate:{}:{artifact}",
                entry.facts.candidate_id
            ));
        }
    }
    Ok(artifacts)
}

fn artifact_named(
    artifacts: &BTreeSet<String>,
    name: &str,
    candidate_id: &str,
) -> Result<String, String> {
    let matches = artifacts
        .iter()
        .filter(|path| Path::new(path).file_name().and_then(|value| value.to_str()) == Some(name))
        .cloned()
        .collect::<Vec<_>>();
    if matches.len() != 1 {
        return Err(format!(
            "md_artifact_name_binding_invalid:{candidate_id}:{name}"
        ));
    }
    Ok(matches[0].clone())
}

fn finite_md_value(
    row: &BTreeMap<String, String>,
    field: &str,
    candidate_id: &str,
) -> Result<f64, String> {
    row.get(field)
        .and_then(|value| value.trim().parse::<f64>().ok())
        .filter(|value| value.is_finite())
        .ok_or_else(|| format!("md_metric_value_invalid:{candidate_id}:{field}"))
}

fn verify_complex_md_entry(
    options: &ScienceOptions,
    entry: &ValidatedCandidateIndexEntry,
    manifest: &Value,
    artifacts: &BTreeSet<String>,
) -> Result<VerifiedMdFacts, String> {
    let candidate_id = entry.facts.candidate_id.as_str();
    let required_names = [
        "md_summary.tsv",
        "md_rmsd.tsv",
        "md_rmsf.tsv",
        "md_ligand_drift.tsv",
        "md_contacts.tsv",
        "md_hbonds.tsv",
        "ligand_placement.tsv",
        "md_trajectory.tsv",
        "md_state.tsv",
        "md_protocol.tsv",
        "gpu_telemetry.tsv",
        "gpu_telemetry_status.tsv",
        "complex_md_manifest.json",
        "md_run_manifest.json",
    ];
    let mut paths = BTreeMap::new();
    for name in required_names {
        paths.insert(name, artifact_named(artifacts, name, candidate_id)?);
    }
    let manifest_relative = current_run_artifact_relative(&options.run_dir, &entry.manifest)
        .ok_or_else(|| format!("md_manifest_invalid:{candidate_id}"))?;
    if paths.get("md_run_manifest.json") != Some(&manifest_relative) {
        return Err(format!("md_manifest_provenance_unbound:{candidate_id}"));
    }
    let output_parent = Path::new(&manifest_relative).parent();
    if artifacts
        .iter()
        .any(|path| Path::new(path).parent() != output_parent)
    {
        return Err(format!(
            "md_artifact_invocation_directory_mismatch:{candidate_id}"
        ));
    }
    for (field, name) in [
        ("trajectory", "md_trajectory.tsv"),
        ("state_artifact", "md_state.tsv"),
        ("protocol_artifact", "md_protocol.tsv"),
        ("ligand_coordinate", "ligand_placement.tsv"),
        ("complex_manifest", "complex_md_manifest.json"),
        ("gpu_telemetry_artifact", "gpu_telemetry.tsv"),
        ("gpu_telemetry_status_artifact", "gpu_telemetry_status.tsv"),
    ] {
        if manifest.get(field).and_then(Value::as_str) != paths.get(name).map(String::as_str) {
            return Err(format!(
                "md_manifest_artifact_mismatch:{candidate_id}:{field}"
            ));
        }
    }
    let expected_metrics = [
        "md_summary.tsv",
        "md_rmsd.tsv",
        "md_rmsf.tsv",
        "md_ligand_drift.tsv",
        "md_contacts.tsv",
        "md_hbonds.tsv",
    ]
    .into_iter()
    .map(|name| paths[name].clone())
    .collect::<BTreeSet<_>>();
    let manifest_metrics = manifest
        .get("metric_artifacts")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
    if manifest_metrics != expected_metrics {
        return Err(format!("md_metric_artifact_set_mismatch:{candidate_id}"));
    }
    let summary_rows = read_simple_tsv_rows(&options.run_dir.join(&paths["md_summary.tsv"]))?;
    if summary_rows.len() != 1
        || summary_rows[0].get("candidate_id").map(String::as_str) != Some(candidate_id)
    {
        return Err(format!("md_summary_candidate_mismatch:{candidate_id}"));
    }
    let summary = &summary_rows[0];
    let mut values = BTreeMap::new();
    for field in [
        "rmsd",
        "rmsf",
        "ligand_drift",
        "hydrogen_bond_occupancy",
        "contact_occupancy",
        "interaction_persistence",
        "md_stability_score",
    ] {
        let value = finite_md_value(summary, field, candidate_id)?;
        if value < 0.0
            || (matches!(
                field,
                "hydrogen_bond_occupancy"
                    | "contact_occupancy"
                    | "interaction_persistence"
                    | "md_stability_score"
            ) && value > 1.0)
        {
            return Err(format!(
                "md_metric_value_out_of_range:{candidate_id}:{field}"
            ));
        }
        values.insert(field, value);
    }
    for (name, metric, unit) in [
        ("md_rmsd.tsv", "rmsd", "angstrom"),
        ("md_rmsf.tsv", "rmsf", "angstrom"),
        ("md_ligand_drift.tsv", "ligand_drift", "angstrom"),
        ("md_contacts.tsv", "contact_occupancy", "fraction"),
        ("md_hbonds.tsv", "hydrogen_bond_occupancy", "fraction"),
    ] {
        let rows = read_simple_tsv_rows(&options.run_dir.join(&paths[name]))?;
        if rows.len() != 1
            || rows[0].get("candidate_id").map(String::as_str) != Some(candidate_id)
            || rows[0].get("input_id").map(String::as_str) != Some(entry.facts.input_id.as_str())
            || rows[0].get("metric").map(String::as_str) != Some(metric)
            || rows[0].get("unit").map(String::as_str) != Some(unit)
        {
            return Err(format!("md_metric_binding_invalid:{candidate_id}:{name}"));
        }
        let artifact_value = finite_md_value(&rows[0], "value", candidate_id)?;
        if (artifact_value - values[metric]).abs() > 1e-6 {
            return Err(format!(
                "md_metric_summary_mismatch:{candidate_id}:{metric}"
            ));
        }
    }
    let recomputed = clamp_score(
        clamp_score(1.0 - values["rmsd"] / 4.0) * 0.30
            + clamp_score(1.0 - values["ligand_drift"] / 3.0) * 0.25
            + values["contact_occupancy"] * 0.20
            + values["hydrogen_bond_occupancy"] * 0.10
            + values["interaction_persistence"] * 0.15,
    );
    if (recomputed - values["md_stability_score"]).abs() > 1e-4 {
        return Err(format!("md_stability_score_mismatch:{candidate_id}"));
    }
    let protocol_rows = read_simple_tsv_rows(&options.run_dir.join(&paths["md_protocol.tsv"]))?;
    let mut protocol = BTreeMap::new();
    for row in protocol_rows {
        if row.get("candidate_id").map(String::as_str) != Some(candidate_id)
            || row.get("input_id").map(String::as_str) != Some(entry.facts.input_id.as_str())
        {
            return Err(format!("md_protocol_binding_invalid:{candidate_id}"));
        }
        let key = format!(
            "{}:{}",
            row.get("stage").map(String::as_str).unwrap_or_default(),
            row.get("parameter").map(String::as_str).unwrap_or_default()
        );
        if protocol.insert(key, row).is_some() {
            return Err(format!("md_protocol_parameter_duplicate:{candidate_id}"));
        }
    }
    for (key, unit) in [
        ("platform:openmm_platform", ""),
        ("integrator:time_step", "ps"),
        ("production:total_steps", "steps"),
        ("output:frame_count", "frames"),
        ("output:state_rows", "rows"),
    ] {
        if protocol
            .get(key)
            .and_then(|row| row.get("unit"))
            .map(String::as_str)
            != Some(unit)
        {
            return Err(format!(
                "md_protocol_parameter_invalid:{candidate_id}:{key}"
            ));
        }
    }
    let total_steps = protocol["production:total_steps"]
        .get("value")
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .ok_or_else(|| format!("md_protocol_steps_invalid:{candidate_id}"))?;
    let frame_count = protocol["output:frame_count"]
        .get("value")
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value >= 2)
        .ok_or_else(|| format!("md_protocol_frame_count_invalid:{candidate_id}"))?;
    let state_count = protocol["output:state_rows"]
        .get("value")
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value >= 2)
        .ok_or_else(|| format!("md_protocol_state_count_invalid:{candidate_id}"))?;
    if manifest.get("frame_count").and_then(Value::as_u64) != Some(frame_count as u64)
        || manifest.get("state_row_count").and_then(Value::as_u64) != Some(state_count as u64)
    {
        return Err(format!(
            "md_manifest_sampling_count_mismatch:{candidate_id}"
        ));
    }
    let state_rows = read_simple_tsv_rows(&options.run_dir.join(&paths["md_state.tsv"]))?;
    if state_rows.len() != state_count {
        return Err(format!("md_state_row_count_mismatch:{candidate_id}"));
    }
    let max_step = state_rows
        .iter()
        .map(|row| {
            if row.get("candidate_id").map(String::as_str) != Some(candidate_id)
                || row.get("input_id").map(String::as_str) != Some(entry.facts.input_id.as_str())
            {
                return Err(format!("md_state_binding_invalid:{candidate_id}"));
            }
            for field in [
                "time_ps",
                "potential_energy_kj_mol",
                "kinetic_energy_kj_mol",
                "total_energy_kj_mol",
                "temperature_estimate_K",
            ] {
                finite_md_value(row, field, candidate_id)?;
            }
            row.get("step")
                .and_then(|value| value.parse::<u64>().ok())
                .ok_or_else(|| format!("md_state_step_invalid:{candidate_id}"))
        })
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .max()
        .unwrap_or_default();
    if max_step < total_steps {
        return Err(format!("md_state_steps_incomplete:{candidate_id}"));
    }
    let trajectory_rows = read_simple_tsv_rows(&options.run_dir.join(&paths["md_trajectory.tsv"]))?;
    let mut frames = BTreeSet::new();
    for row in trajectory_rows {
        if row.get("candidate_id").map(String::as_str) != Some(candidate_id) {
            return Err(format!("md_trajectory_binding_invalid:{candidate_id}"));
        }
        let frame = row
            .get("frame")
            .and_then(|value| value.parse::<usize>().ok())
            .ok_or_else(|| format!("md_trajectory_frame_invalid:{candidate_id}"))?;
        frames.insert(frame);
    }
    if frames.len() != frame_count {
        return Err(format!("md_trajectory_frame_count_mismatch:{candidate_id}"));
    }
    let platform = manifest
        .get("platform")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("verified_md_platform_missing:{candidate_id}"))?
        .to_string();
    let system = manifest.get("system").and_then(Value::as_object);
    let force_field = system.and_then(|system| {
        let fields = ["protein_forcefield", "small_molecule_forcefield"]
            .into_iter()
            .filter_map(|key| {
                system
                    .get(key)
                    .and_then(Value::as_str)
                    .filter(|value| !value.trim().is_empty())
                    .map(|value| format!("{key}={value}"))
            })
            .collect::<Vec<_>>();
        (!fields.is_empty()).then(|| fields.join(","))
    });
    Ok(VerifiedMdFacts {
        candidate_id: candidate_id.to_string(),
        platform,
        protocol: paths["md_protocol.tsv"].clone(),
        state: paths["md_state.tsv"].clone(),
        trajectory: paths["md_trajectory.tsv"].clone(),
        telemetry: Some(paths["gpu_telemetry.tsv"].clone()),
        gpu_observed: manifest
            .get("gpu_observed")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        force_field,
        solvent_model: system
            .and_then(|value| value.get("solvent_mode"))
            .and_then(Value::as_str)
            .map(str::to_string),
        nonbonded_method: system
            .and_then(|value| value.get("nonbonded_method"))
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

fn verify_peptide_conformer_entry(
    options: &ScienceOptions,
    entry: &ValidatedCandidateIndexEntry,
    manifest: &Value,
    artifacts: &BTreeSet<String>,
) -> Result<VerifiedPeptideConformerFacts, String> {
    let candidate_id = entry.facts.candidate_id.as_str();
    if manifest
        .get("artifact_schema_version")
        .and_then(Value::as_str)
        != Some("genoligand_peptide_conformer_v1")
        || manifest.get("method_class").and_then(Value::as_str) != Some("conformer_ensemble")
        || manifest.get("has_receptor").and_then(Value::as_bool) != Some(false)
        || manifest.get("uses_openmm").and_then(Value::as_bool) != Some(false)
        || manifest.get("md_score").and_then(Value::as_f64) != Some(0.0)
    {
        return Err(format!("peptide_conformer_method_invalid:{candidate_id}"));
    }
    let summary = artifact_named(artifacts, "peptide_conformer_summary.tsv", candidate_id)?;
    let metrics = artifact_named(artifacts, "peptide_conformer_metrics.tsv", candidate_id)?;
    let coordinates = artifact_named(artifacts, "peptide_conformer_coordinates.tsv", candidate_id)?;
    let ensemble = artifact_named(artifacts, "peptide_conformer_ensemble.tsv", candidate_id)?;
    let run_manifest = artifact_named(artifacts, "md_run_manifest.json", candidate_id)?;
    if current_run_artifact_relative(&options.run_dir, &entry.manifest).as_deref()
        != Some(run_manifest.as_str())
        || manifest.get("coordinate_artifact").and_then(Value::as_str) != Some(coordinates.as_str())
        || manifest.get("ensemble_artifact").and_then(Value::as_str) != Some(ensemble.as_str())
    {
        return Err(format!("peptide_conformer_manifest_unbound:{candidate_id}"));
    }
    let metric_artifacts = manifest
        .get("metric_artifacts")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
    if metric_artifacts != BTreeSet::from([summary.clone(), metrics.clone()])
        || artifacts.iter().any(|path| {
            Path::new(path).file_name().and_then(|value| value.to_str()) == Some("md_summary.tsv")
        })
    {
        return Err(format!(
            "peptide_conformer_artifact_set_invalid:{candidate_id}"
        ));
    }
    let summary_rows = read_simple_tsv_rows(&options.run_dir.join(&summary))?;
    if summary_rows.len() != 1
        || summary_rows[0].get("candidate_id").map(String::as_str) != Some(candidate_id)
        || summary_rows[0].get("input_id").map(String::as_str)
            != Some(entry.facts.input_id.as_str())
        || summary_rows[0].get("method").map(String::as_str)
            != Some("rdkit_peptide_conformer_ensemble")
        || summary_rows[0].get("has_receptor").map(String::as_str) != Some("false")
        || summary_rows[0].get("uses_openmm").map(String::as_str) != Some("false")
    {
        return Err(format!("peptide_conformer_summary_invalid:{candidate_id}"));
    }
    let conformer_count = summary_rows[0]
        .get("conformer_count")
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .ok_or_else(|| format!("peptide_conformer_count_invalid:{candidate_id}"))?;
    if manifest.get("conformer_count").and_then(Value::as_u64) != Some(conformer_count as u64) {
        return Err(format!("peptide_conformer_count_mismatch:{candidate_id}"));
    }
    let metric_rows = read_simple_tsv_rows(&options.run_dir.join(metrics))?;
    let metric_names = metric_rows
        .iter()
        .map(|row| {
            if row.get("candidate_id").map(String::as_str) != Some(candidate_id)
                || row.get("input_id").map(String::as_str) != Some(entry.facts.input_id.as_str())
                || row.get("unit").map(String::as_str) != Some("angstrom")
                || finite_md_value(row, "value", candidate_id)? < 0.0
            {
                return Err(format!("peptide_conformer_metric_invalid:{candidate_id}"));
            }
            Ok(row.get("metric").cloned().unwrap_or_default())
        })
        .collect::<Result<BTreeSet<_>, String>>()?;
    if metric_names
        != BTreeSet::from([
            "mean_centroid_drift_angstrom".to_string(),
            "mean_heavy_atom_rmsd_angstrom".to_string(),
            "mean_heavy_atom_rmsf_angstrom".to_string(),
        ])
    {
        return Err(format!(
            "peptide_conformer_metric_set_invalid:{candidate_id}"
        ));
    }
    let frames = read_simple_tsv_rows(&options.run_dir.join(ensemble))?
        .into_iter()
        .filter(|row| row.get("candidate_id").map(String::as_str) == Some(candidate_id))
        .filter_map(|row| row.get("frame").cloned())
        .collect::<BTreeSet<_>>();
    if frames.len() != conformer_count {
        return Err(format!(
            "peptide_conformer_frame_count_mismatch:{candidate_id}"
        ));
    }
    Ok(VerifiedPeptideConformerFacts {
        candidate_id: candidate_id.to_string(),
        conformer_count,
        manifest: run_manifest,
    })
}

fn verified_indexed_md_computations(
    options: &ScienceOptions,
) -> Result<VerifiedIndexedMdComputations, String> {
    let entries = validated_candidate_manifest_index(options, "md", &SCIENTIFIC_WORKER_TASKS[0])?;
    let mut verified = VerifiedIndexedMdComputations::default();
    for entry in entries {
        if entry.status != "completed" {
            verified
                .not_ready_reasons
                .push(format!("{}:{}", entry.facts.candidate_id, entry.reason));
            if !entry.artifact_provenance.is_empty() {
                validated_md_artifact_provenance(options, &entry)?;
            }
            continue;
        }
        let artifacts = validated_md_artifact_provenance(options, &entry)?;
        let manifest_relative = current_run_artifact_relative(&options.run_dir, &entry.manifest)
            .ok_or_else(|| format!("md_manifest_invalid:{}", entry.facts.candidate_id))?;
        if !artifacts.contains(&manifest_relative) {
            return Err(format!(
                "md_manifest_not_bound_by_provenance:{}",
                entry.facts.candidate_id
            ));
        }
        let manifest = read_json_value_if_file(&options.run_dir.join(&manifest_relative))
            .ok_or_else(|| format!("md_manifest_invalid:{}", entry.facts.candidate_id))?;
        for (field, expected) in [
            ("run_id", entry.facts.run_id.as_str()),
            ("job_id", entry.job_id.as_str()),
            ("invocation_id", entry.invocation_id.as_str()),
            ("candidate_id", entry.facts.candidate_id.as_str()),
            ("candidate_digest", entry.facts.candidate_digest.as_str()),
            ("input_id", entry.facts.input_id.as_str()),
            ("record_id", entry.facts.record_id.as_str()),
            ("input_digest", entry.facts.input_digest.as_str()),
            ("source_digest", entry.facts.source_digest.as_str()),
        ] {
            if manifest.get(field).and_then(Value::as_str) != Some(expected) {
                return Err(format!(
                    "md_manifest_identity_mismatch:{}:{field}",
                    entry.facts.candidate_id
                ));
            }
        }
        if manifest.get("artifact_provenance").and_then(Value::as_str)
            != Some(entry.artifact_provenance.as_str())
        {
            return Err(format!(
                "md_manifest_provenance_mismatch:{}",
                entry.facts.candidate_id
            ));
        }
        match manifest.get("method").and_then(Value::as_str) {
            Some("openmm_receptor_ligand_complex_dynamics") => verified.complex.push(
                verify_complex_md_entry(options, &entry, &manifest, &artifacts)?,
            ),
            Some("rdkit_peptide_conformer_ensemble") => verified.peptide.push(
                verify_peptide_conformer_entry(options, &entry, &manifest, &artifacts)?,
            ),
            _ => {
                return Err(format!(
                    "md_manifest_method_invalid:{}",
                    entry.facts.candidate_id
                ));
            }
        }
    }
    verified
        .complex
        .sort_by(|left, right| left.candidate_id.cmp(&right.candidate_id));
    verified
        .peptide
        .sort_by(|left, right| left.candidate_id.cmp(&right.candidate_id));
    Ok(verified)
}

fn current_md_method(options: &ScienceOptions) -> Option<String> {
    read_json_value_if_file(&options.run_dir.join("tool_outputs/md_run_manifest.json")).and_then(
        |value| {
            value
                .get("method")
                .and_then(Value::as_str)
                .map(str::to_string)
        },
    )
}

fn verified_current_md_facts(options: &ScienceOptions) -> Result<Vec<VerifiedMdFacts>, String> {
    if options
        .run_dir
        .join("tool_outputs/md_manifest_index.json")
        .is_file()
    {
        return Ok(verified_indexed_md_computations(options)?.complex);
    }
    let binding = current_md_worker_artifacts(options)?;
    if binding.status != "PASS" {
        return Ok(Vec::new());
    }
    if current_md_method(options).as_deref() != Some("openmm_receptor_ligand_complex_dynamics") {
        return Ok(Vec::new());
    }
    let candidate_id = binding
        .candidate_id
        .as_deref()
        .ok_or_else(|| "verified_md_candidate_missing".to_string())?;
    let summary_path = options.run_dir.join("md_summary.tsv");
    validate_md_summary_semantics(options, &summary_path, candidate_id, &binding.artifacts)?;
    let manifest_path = options.run_dir.join("tool_outputs/md_run_manifest.json");
    let manifest = read_json_value_if_file(&manifest_path)
        .ok_or_else(|| "verified_md_manifest_missing".to_string())?;
    if manifest.get("candidate_id").and_then(Value::as_str) != Some(candidate_id) {
        return Err("verified_md_manifest_candidate_mismatch".to_string());
    }
    let required_artifact = |field: &str| -> Result<String, String> {
        let path = manifest
            .get(field)
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| format!("verified_md_manifest_field_missing:{field}"))?;
        let relative = current_run_artifact_relative(&options.run_dir, path)
            .ok_or_else(|| format!("verified_md_manifest_artifact_invalid:{field}"))?;
        if !binding.artifacts.contains(&relative) {
            return Err(format!("verified_md_manifest_artifact_unbound:{field}"));
        }
        Ok(relative)
    };
    let platform = manifest
        .get("platform")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "verified_md_platform_missing".to_string())?
        .to_string();
    let telemetry = manifest
        .get("gpu_telemetry_artifact")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(|path| {
            current_run_artifact_relative(&options.run_dir, path)
                .ok_or_else(|| "verified_md_telemetry_invalid".to_string())
        })
        .transpose()?;
    if telemetry
        .as_ref()
        .is_some_and(|relative| !binding.artifacts.contains(relative))
    {
        return Err("verified_md_telemetry_unbound".to_string());
    }
    Ok(vec![VerifiedMdFacts {
        candidate_id: candidate_id.to_string(),
        platform,
        protocol: required_artifact("protocol_artifact")?,
        state: required_artifact("state_artifact")?,
        trajectory: required_artifact("trajectory")?,
        telemetry,
        gpu_observed: manifest
            .get("gpu_observed")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        force_field: manifest
            .get("force_field")
            .and_then(Value::as_str)
            .map(str::to_string),
        solvent_model: manifest
            .get("solvent_model")
            .and_then(Value::as_str)
            .map(str::to_string),
        nonbonded_method: manifest
            .get("nonbonded_method")
            .and_then(Value::as_str)
            .map(str::to_string),
    }])
}

fn validate_md_summary_semantics(
    options: &ScienceOptions,
    summary_path: &Path,
    candidate_id: &str,
    artifacts: &BTreeSet<String>,
) -> Result<(), String> {
    for required in [
        "tool_outputs/md_rmsd.tsv",
        "tool_outputs/md_rmsf.tsv",
        "tool_outputs/md_ligand_drift.tsv",
        "tool_outputs/md_contacts.tsv",
        "tool_outputs/md_hbonds.tsv",
        "tool_outputs/md_protocol.tsv",
        "tool_outputs/md_state.tsv",
        "tool_outputs/md_trajectory.tsv",
    ] {
        if !artifacts.contains(required) {
            return Err(format!("md_required_artifact_missing:{required}"));
        }
    }
    let rows = read_simple_tsv_rows(summary_path)?;
    if rows.len() != 1 || rows[0].get("candidate_id").map(String::as_str) != Some(candidate_id) {
        return Err("md_summary_candidate_mismatch".to_string());
    }
    for field in [
        "rmsd",
        "rmsf",
        "ligand_drift",
        "hydrogen_bond_occupancy",
        "contact_occupancy",
        "interaction_persistence",
        "md_stability_score",
    ] {
        let value = rows[0]
            .get(field)
            .and_then(|value| value.trim().parse::<f64>().ok())
            .filter(|value| value.is_finite())
            .ok_or_else(|| format!("md_summary_metric_invalid:{field}"))?;
        if value < 0.0
            || (matches!(
                field,
                "hydrogen_bond_occupancy"
                    | "contact_occupancy"
                    | "interaction_persistence"
                    | "md_stability_score"
            ) && value > 1.0)
        {
            return Err(format!("md_summary_metric_out_of_range:{field}"));
        }
    }
    for relative in [
        "tool_outputs/md_rmsd.tsv",
        "tool_outputs/md_rmsf.tsv",
        "tool_outputs/md_ligand_drift.tsv",
        "tool_outputs/md_contacts.tsv",
        "tool_outputs/md_hbonds.tsv",
    ] {
        let rows = read_simple_tsv_rows(&options.run_dir.join(relative))?;
        if rows.is_empty()
            || rows
                .iter()
                .any(|row| row.get("candidate_id").map(String::as_str) != Some(candidate_id))
        {
            return Err(format!("md_metric_candidate_mismatch:{relative}"));
        }
    }
    let protocol = read_simple_tsv_rows(&options.run_dir.join("tool_outputs/md_protocol.tsv"))?;
    let total_steps = protocol
        .iter()
        .filter(|row| {
            matches!(
                row.get("parameter").map(String::as_str),
                Some("total_steps") | Some("steps")
            )
        })
        .filter_map(|row| row.get("value"))
        .filter_map(|value| value.trim().parse::<u64>().ok())
        .max()
        .unwrap_or_default();
    if total_steps == 0 {
        return Err("md_protocol_zero_steps".to_string());
    }
    for relative in [
        "tool_outputs/md_state.tsv",
        "tool_outputs/md_trajectory.tsv",
    ] {
        if rows_in_tsv(&options.run_dir.join(relative))? < 2 {
            return Err(format!("md_artifact_insufficient_frames:{relative}"));
        }
    }
    Ok(())
}

fn current_md_worker_artifacts(
    options: &ScienceOptions,
) -> Result<CurrentMdWorkerArtifacts, String> {
    let invalid = |reason: &str| {
        Ok(CurrentMdWorkerArtifacts {
            artifacts: BTreeSet::new(),
            candidate_id: None,
            status: "INVALID".to_string(),
            reason: format!("md_artifact_binding_invalid:{reason}"),
        })
    };
    let invocation_path = options.run_dir.join(WORKER_INVOCATION_FILE);
    if !invocation_path.is_file() {
        return invalid("invocation_missing");
    }
    let invocation = read_text(&invocation_path)?.trim().to_string();
    if invocation.is_empty() {
        return invalid("invocation_missing");
    }

    let task = &SCIENTIFIC_WORKER_TASKS[0];
    let job_path = options
        .run_dir
        .join("worker_jobs")
        .join(format!("{}.json", task.job_id));
    let result_path = options
        .run_dir
        .join("worker_results")
        .join(format!("{}.json", task.job_id));
    let Some(job) = read_json_value_if_file(&job_path) else {
        return invalid("job_missing_or_invalid");
    };
    if !validate_worker_job(&job, task)
        || job.get("invocation_id").and_then(Value::as_str) != Some(invocation.as_str())
    {
        return invalid("job_invocation_mismatch");
    }
    if !job_run_identity_matches(&job, options) {
        return invalid("job_run_identity_mismatch");
    }
    let Some(result) = read_json_value_if_file(&result_path) else {
        return invalid("result_missing_or_invalid");
    };
    if !validate_worker_result_binding(&result, &job, options, task) {
        return invalid("result_binding_mismatch");
    }
    if result.get("status").and_then(Value::as_str) != Some("PASS") {
        return Ok(CurrentMdWorkerArtifacts {
            artifacts: BTreeSet::new(),
            candidate_id: None,
            status: result
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("INVALID")
                .to_string(),
            reason: result
                .get("reason")
                .and_then(Value::as_str)
                .unwrap_or("md_worker_result_reason_missing")
                .to_string(),
        });
    }
    let facts = match current_md_run_facts(options) {
        Ok(facts) => facts,
        Err(reason) => return invalid(&reason),
    };
    for (field, expected) in [
        ("candidate_id", facts.candidate_id.as_str()),
        ("candidate_digest", facts.candidate_digest.as_str()),
    ] {
        if job.get(field).and_then(Value::as_str) != Some(expected) {
            return invalid("candidate_manifest_mismatch");
        }
        let Some(actual) = result.get(field).and_then(Value::as_str) else {
            return invalid("result_identity_missing");
        };
        if actual != expected {
            return invalid("candidate_manifest_mismatch");
        }
    }
    for (field, expected) in [
        ("input_id", facts.input_id.as_str()),
        ("input_digest", facts.input_digest.as_str()),
        ("source_digest", facts.source_digest.as_str()),
    ] {
        if job.get(field).and_then(Value::as_str) != Some(expected) {
            return invalid("input_manifest_mismatch");
        }
        let Some(actual) = result.get(field).and_then(Value::as_str) else {
            return invalid("result_identity_missing");
        };
        if actual != expected {
            return invalid("input_manifest_mismatch");
        }
    }
    let Some(provenance_path) = result.get("artifact_provenance").and_then(Value::as_str) else {
        return invalid("provenance_missing");
    };
    let Some(provenance_relative) =
        current_run_artifact_relative(&options.run_dir, provenance_path)
    else {
        return invalid("provenance_outside_run");
    };
    let Some(provenance) = read_json_value_if_file(&options.run_dir.join(&provenance_relative))
    else {
        return invalid("provenance_invalid");
    };
    if provenance.get("schema_version").and_then(Value::as_str)
        != Some(MD_PROVENANCE_SCHEMA_VERSION)
        || provenance.get("job_id").and_then(Value::as_str) != Some(task.job_id)
        || provenance.get("invocation_id").and_then(Value::as_str) != Some(invocation.as_str())
    {
        return invalid("provenance_binding_mismatch");
    }
    for field in ["run_id", "candidate_id", "input_id"] {
        let Some(expected) = result
            .get(field)
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
        else {
            return invalid("result_identity_missing");
        };
        if provenance.get(field).and_then(Value::as_str) != Some(expected) {
            return invalid("provenance_identity_mismatch");
        }
    }
    for field in ["candidate_digest", "input_digest", "source_digest"] {
        let Some(expected) = result.get(field).and_then(Value::as_str) else {
            return invalid("result_digest_missing");
        };
        if !valid_sha256_hex(expected)
            || provenance.get(field).and_then(Value::as_str) != Some(expected)
        {
            return invalid("provenance_digest_mismatch");
        }
    }
    let parameters_digest = provenance
        .get("parameters_digest")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let tool_name = provenance
        .get("tool_name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let tool_version = provenance
        .get("tool_version")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !valid_sha256_hex(parameters_digest)
        || tool_name.trim().is_empty()
        || tool_version.trim().is_empty()
    {
        return invalid("provenance_tool_identity_invalid");
    }
    let Some(rows) = provenance.get("artifacts").and_then(Value::as_array) else {
        return invalid("provenance_artifacts_missing");
    };
    if rows.is_empty() {
        return invalid("provenance_artifacts_empty");
    }
    let mut artifacts = BTreeSet::new();
    for row in rows {
        if row.get("job_id").and_then(Value::as_str) != Some(task.job_id)
            || row.get("invocation_id").and_then(Value::as_str) != Some(invocation.as_str())
        {
            return invalid("artifact_binding_mismatch");
        }
        for field in [
            "run_id",
            "candidate_id",
            "candidate_digest",
            "input_id",
            "input_digest",
            "source_digest",
        ] {
            if row.get(field).and_then(Value::as_str)
                != provenance.get(field).and_then(Value::as_str)
            {
                return invalid("artifact_identity_mismatch");
            }
        }
        if row.get("parameters_digest").and_then(Value::as_str) != Some(parameters_digest)
            || row.get("tool_name").and_then(Value::as_str) != Some(tool_name)
            || row.get("tool_version").and_then(Value::as_str) != Some(tool_version)
        {
            return invalid("artifact_tool_identity_mismatch");
        }
        let Some(artifact) = row.get("artifact_path").and_then(Value::as_str) else {
            return invalid("artifact_path_missing");
        };
        let Some(relative) = current_run_artifact_relative(&options.run_dir, artifact) else {
            return invalid("artifact_outside_run");
        };
        let expected_digest = row
            .get("artifact_sha256")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if !valid_sha256_hex(expected_digest)
            || sha256_file_hex(&options.run_dir.join(&relative)).as_deref() != Some(expected_digest)
        {
            return invalid("artifact_digest_mismatch");
        }
        if !artifacts.insert(relative) {
            return invalid("artifact_duplicate");
        }
    }
    Ok(CurrentMdWorkerArtifacts {
        artifacts,
        candidate_id: Some(facts.candidate_id),
        status: "PASS".to_string(),
        reason: String::new(),
    })
}

fn job_run_identity_matches(job: &Value, options: &ScienceOptions) -> bool {
    if job.get("run_id").and_then(Value::as_str) != Some(infer_run_id(&options.run_dir).as_str()) {
        return false;
    }
    let Some(job_run_dir) = job.get("run_dir").and_then(Value::as_str) else {
        return false;
    };
    let Ok(expected) = options.run_dir.canonicalize() else {
        return false;
    };
    PathBuf::from(job_run_dir)
        .canonicalize()
        .is_ok_and(|actual| actual == expected)
}

fn current_md_run_facts(options: &ScienceOptions) -> Result<CurrentMdRunFacts, String> {
    let candidate_rows = selected_candidate_rows(options)?;
    if candidate_rows.len() != 1 {
        return Err("candidate_manifest_mismatch".to_string());
    }
    selected_candidate_run_facts(options, &candidate_rows[0])
}

fn selected_candidate_run_facts(
    options: &ScienceOptions,
    candidate_row: &BTreeMap<String, String>,
) -> Result<CurrentMdRunFacts, String> {
    let candidate = candidate_row
        .iter()
        .map(|(key, value)| (key.clone(), value.trim().to_string()))
        .collect::<BTreeMap<_, _>>();
    let candidate_id = candidate
        .get("candidate_id")
        .map(String::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "candidate_manifest_mismatch".to_string())?
        .to_string();
    let input_id = candidate
        .get("input_id")
        .map(String::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "candidate_manifest_mismatch".to_string())?
        .to_string();
    let record_id = candidate
        .get("record_id")
        .map(String::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "candidate_manifest_mismatch".to_string())?
        .to_string();
    let candidate_digest = canonical_json_sha256(&json!({
        "schema_version": "genoligand_candidate_selection_digest_v1",
        "candidate_id": &candidate_id,
        "identity_namespace": candidate.get("identity_namespace").map(String::as_str).unwrap_or(""),
        "input_id": &input_id,
        "record_id": &record_id,
        "target_id": candidate.get("target_id").map(String::as_str).unwrap_or(""),
        "pdb_id": candidate.get("pdb_id").map(String::as_str).unwrap_or(""),
        "selection_rank": candidate.get("selection_rank").map(String::as_str).unwrap_or(""),
        "receptor_artifact": candidate.get("receptor_artifact").map(String::as_str).unwrap_or(""),
        "coordinate_sha256": candidate.get("coordinate_sha256").map(String::as_str).unwrap_or(""),
    }));

    let input_rows = read_simple_tsv_rows(&options.run_dir.join("input_conversion_manifest.tsv"))?;
    let selected = input_rows
        .iter()
        .filter(|row| {
            row.get("input_id").map(String::as_str) == Some(input_id.as_str())
                && row.get("record_id").map(String::as_str) == Some(record_id.as_str())
                && row.get("conversion_status").map(String::as_str) == Some("converted")
                && matches!(
                    row.get("canonical_kind").map(String::as_str),
                    Some("small_molecule") | Some("peptide")
                )
        })
        .collect::<Vec<_>>();
    if selected.len() != 1 {
        return Err("input_manifest_mismatch".to_string());
    }
    let input = selected[0]
        .iter()
        .map(|(key, value)| (key.clone(), value.trim().to_string()))
        .collect::<BTreeMap<_, _>>();
    let canonical_value = input
        .get("canonical_value")
        .map(|value| decode_tsv_cell(value).trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "input_manifest_mismatch".to_string())?;
    let source_digest = input
        .get("content_sha256")
        .map(|value| value.to_ascii_lowercase())
        .filter(|value| valid_sha256_hex(value))
        .ok_or_else(|| "input_manifest_mismatch".to_string())?;
    let input_digest = canonical_json_sha256(&json!({
        "input_id": input_id,
        "record_id": record_id,
        "canonical_kind": input.get("canonical_kind").map(String::as_str).unwrap_or(""),
        "canonical_value": canonical_value,
        "source_content_sha256": source_digest,
    }));
    Ok(CurrentMdRunFacts {
        run_id: infer_run_id(&options.run_dir),
        candidate_id,
        candidate_digest,
        input_id,
        record_id,
        input_digest,
        source_digest,
    })
}

fn canonical_json_sha256(value: &Value) -> String {
    let mut payload = String::new();
    write_canonical_ascii_json(value, &mut payload);
    let mut hasher = Sha256::new();
    hasher.update(payload.as_bytes());
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn write_canonical_ascii_json(value: &Value, out: &mut String) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
        Value::Number(value) => out.push_str(&value.to_string()),
        Value::String(value) => {
            let encoded =
                serde_json::to_string(value).expect("JSON string serialization cannot fail");
            for ch in encoded.chars() {
                if ch.is_ascii() {
                    out.push(ch);
                } else {
                    let code = ch as u32;
                    if code <= 0xffff {
                        out.push_str(&format!("\\u{code:04x}"));
                    } else {
                        let scalar = code - 0x1_0000;
                        let high = 0xd800 + (scalar >> 10);
                        let low = 0xdc00 + (scalar & 0x3ff);
                        out.push_str(&format!("\\u{high:04x}\\u{low:04x}"));
                    }
                }
            }
        }
        Value::Array(values) => {
            out.push('[');
            for (index, value) in values.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_canonical_ascii_json(value, out);
            }
            out.push(']');
        }
        Value::Object(values) => {
            out.push('{');
            let mut entries = values.iter().collect::<Vec<_>>();
            entries.sort_by(|left, right| left.0.cmp(right.0));
            for (index, (key, value)) in entries.into_iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_canonical_ascii_json(&Value::String(key.clone()), out);
                out.push(':');
                write_canonical_ascii_json(value, out);
            }
            out.push('}');
        }
    }
}

fn read_json_value_if_file(path: &Path) -> Option<Value> {
    read_text(path)
        .ok()
        .and_then(|text| serde_json::from_str(text.trim_start_matches('\u{feff}')).ok())
}

fn current_run_artifact_relative(run_dir: &Path, artifact: &str) -> Option<String> {
    if artifact.trim().is_empty() {
        return None;
    }
    let artifact = artifact.replace('\\', "/");
    let root = run_dir.canonicalize().ok()?;
    let path = if cfg!(windows) {
        wsl_path_to_windows(&artifact).unwrap_or_else(|| PathBuf::from(&artifact))
    } else {
        PathBuf::from(&artifact)
    };
    let candidate = if path.is_absolute() {
        path
    } else {
        run_dir.join(path)
    };
    let resolved = candidate.canonicalize().ok()?;
    if !resolved.is_file() || !resolved.starts_with(&root) {
        return None;
    }
    Some(
        resolved
            .strip_prefix(root)
            .ok()?
            .to_string_lossy()
            .replace('\\', "/"),
    )
}

fn valid_sha256_hex(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn sha256_file_hex(path: &Path) -> Option<String> {
    let mut file = fs::File::open(path).ok()?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).ok()?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Some(
        hasher
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
    )
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

fn md_not_ready_reason(current_artifacts: &BTreeSet<String>) -> String {
    let mut reasons = Vec::new();
    if !current_artifacts.contains("tool_outputs/md_summary.tsv") {
        reasons.push("no md_summary.tsv in current run tool_outputs".to_string());
    }
    if !current_artifacts.contains("tool_outputs/md_rmsd.tsv")
        && !current_artifacts.contains("tool_outputs/rmsd.tsv")
    {
        reasons.push("no RMSD metric artifact".to_string());
    }
    if !current_artifacts.contains("tool_outputs/md_rmsf.tsv")
        && !current_artifacts.contains("tool_outputs/rmsf.tsv")
    {
        reasons.push("no RMSF metric artifact".to_string());
    }
    if !current_artifacts.contains("tool_outputs/md_ligand_drift.tsv")
        && !current_artifacts.contains("tool_outputs/ligand_drift.tsv")
    {
        reasons.push("no ligand drift metric artifact".to_string());
    }
    if !current_artifacts.contains("tool_outputs/md_contacts.tsv")
        && !current_artifacts.contains("tool_outputs/contacts.tsv")
    {
        reasons.push("no contact metric artifact".to_string());
    }
    if !current_artifacts.contains("tool_outputs/md_hbonds.tsv")
        && !current_artifacts.contains("tool_outputs/hydrogen_bonds.tsv")
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
    marker: String,
    reason: String,
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
    let current_inputs = current_run_input_identities(options)?;
    for artifact in artifacts {
        let path = options.run_dir.join(artifact);
        if !path.exists() || !path.is_file() {
            violations.push(AuthenticityViolation {
                artifact: (*artifact).to_string(),
                line_number: 0,
                marker: "missing_required_artifact".to_string(),
                reason: "required_current_run_artifact_missing".to_string(),
            });
            continue;
        }
        let text = read_text(&path)?;
        if matches!(
            *artifact,
            "known_database_matches.tsv"
                | "metagenome_potential_matches.tsv"
                | "structure_interaction_matches.tsv"
                | "tool_outputs/mmseqs_alignment_matches.tsv"
                | "tool_outputs/foldseek_structure_matches.tsv"
                | "target_candidate_ranking.tsv"
        ) {
            validate_current_run_tsv_provenance(
                options,
                artifact,
                &text,
                &current_inputs,
                &mut violations,
            );
        } else if *artifact == "candidate_detail.json" {
            validate_candidate_detail_provenance(artifact, &text, &current_inputs, &mut violations);
        } else if *artifact == "docking_summary.tsv" {
            if let Err(reason) = read_docking_scores(options, &path) {
                violations.push(AuthenticityViolation {
                    artifact: (*artifact).to_string(),
                    line_number: 0,
                    marker: "invalid_docking_provenance".to_string(),
                    reason,
                });
            }
        } else if *artifact == "md_summary.tsv" {
            match verified_current_md_facts(options) {
                Ok(facts) => {
                    let summary_ids = read_simple_tsv_rows(&path)
                        .unwrap_or_default()
                        .into_iter()
                        .filter_map(|row| row.get("candidate_id").cloned())
                        .collect::<BTreeSet<_>>();
                    let verified_ids = facts
                        .into_iter()
                        .map(|facts| facts.candidate_id)
                        .collect::<BTreeSet<_>>();
                    if summary_ids != verified_ids {
                        violations.push(AuthenticityViolation {
                            artifact: (*artifact).to_string(),
                            line_number: 0,
                            marker: "invalid_md_provenance".to_string(),
                            reason: "md_summary_verified_candidate_set_mismatch".to_string(),
                        });
                    }
                }
                Err(reason) => violations.push(AuthenticityViolation {
                    artifact: (*artifact).to_string(),
                    line_number: 0,
                    marker: "invalid_md_provenance".to_string(),
                    reason,
                }),
            }
        }
    }
    Ok(violations)
}

fn current_run_input_identities(
    options: &ScienceOptions,
) -> Result<BTreeSet<(String, String)>, String> {
    let rows = read_simple_tsv_rows(&options.run_dir.join("input_normalized_manifest.tsv"))?;
    Ok(rows
        .into_iter()
        .filter_map(|row| {
            let input_id = row.get("input_id")?.trim();
            let record_id = row
                .get("record_id")
                .map(String::as_str)
                .unwrap_or("record_1")
                .trim();
            (!input_id.is_empty() && !record_id.is_empty())
                .then(|| (input_id.to_string(), record_id.to_string()))
        })
        .collect())
}

fn validate_current_run_tsv_provenance(
    options: &ScienceOptions,
    artifact: &str,
    text: &str,
    current_inputs: &BTreeSet<(String, String)>,
    violations: &mut Vec<AuthenticityViolation>,
) {
    let mut lines = text.lines();
    let headers = split_tsv(lines.next().unwrap_or_default());
    let input_idx = find_header(&headers, &["input_id"]);
    let record_idx = find_header(&headers, &["record_id"]);
    let target_idx = find_header(&headers, &["target_id"]);
    let source_idx = find_header(&headers, &["source", "source_database_id"]);
    let evidence_idx = find_header(&headers, &["evidence", "evidence_summary"]);
    let candidate_idx = find_header(&headers, &["candidate_id"]);
    let query_idx = find_header(&headers, &["current_run_query_id", "query_digest"]);
    let source_provenance_idx = find_header(
        &headers,
        &[
            "source_digest",
            "source_row_provenance",
            "source_database_id",
        ],
    );
    let expected_run_id = infer_run_id(&options.run_dir);
    let expected_pipeline_invocation = current_pipeline_invocation_id(options).unwrap_or_default();
    let mut missing_headers = Vec::new();
    if input_idx.is_none() {
        missing_headers.push("input_id");
    }
    if record_idx.is_none() {
        missing_headers.push("record_id");
    }
    if target_idx.is_none() {
        missing_headers.push("target_id");
    }
    if artifact == "target_candidate_ranking.tsv" && candidate_idx.is_none() {
        missing_headers.push("candidate_id");
    }
    if !missing_headers.is_empty() {
        violations.push(AuthenticityViolation {
            artifact: artifact.to_string(),
            line_number: 1,
            marker: "provenance_schema_missing".to_string(),
            reason: missing_headers.join(","),
        });
        return;
    }
    for (offset, line) in lines.enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let cols = split_tsv(line);
        let input_id = cell(&cols, input_idx);
        let record_id = {
            let value = cell(&cols, record_idx);
            if value.is_empty() {
                "record_1".to_string()
            } else {
                value
            }
        };
        let mut reasons = Vec::new();
        if !current_inputs.contains(&(input_id.clone(), record_id.clone())) {
            reasons.push("input_or_record_not_in_current_run");
        }
        if cell(&cols, target_idx).trim().is_empty() {
            reasons.push("missing_target_id");
        }
        if artifact != "target_candidate_ranking.tsv" && cell(&cols, source_idx).trim().is_empty() {
            reasons.push("missing_source_identity");
        }
        if cell(&cols, evidence_idx).trim().is_empty() {
            reasons.push("missing_evidence_identity");
        }
        if artifact == "target_candidate_ranking.tsv"
            && cell(&cols, candidate_idx).trim().is_empty()
        {
            reasons.push("missing_candidate_identity");
        }
        if matches!(
            artifact,
            "known_database_matches.tsv"
                | "metagenome_potential_matches.tsv"
                | "tool_outputs/mmseqs_alignment_matches.tsv"
                | "tool_outputs/foldseek_structure_matches.tsv"
        ) {
            let query = cell(&cols, query_idx);
            let digest = query.strip_prefix("query_").unwrap_or(&query);
            if !valid_sha256_hex(digest) {
                reasons.push("invalid_query_identity");
            }
        }
        if matches!(
            artifact,
            "known_database_matches.tsv"
                | "metagenome_potential_matches.tsv"
                | "structure_interaction_matches.tsv"
        ) {
            let provenance = cell(&cols, source_provenance_idx);
            let evidence = cell(&cols, evidence_idx);
            let bound_field = |name: &str| {
                first_nonempty(&[
                    evidence_field(&evidence, &[name]),
                    evidence_field(&provenance, &[name]),
                ])
            };
            let source_id = bound_field("source_id");
            let source_path = bound_field("source_path");
            let source_digest = bound_field("source_digest");
            let run_id = bound_field("run_id");
            let pipeline_invocation_id = bound_field("pipeline_invocation_id");
            match canonical_source_binding(&options.science_store, &source_id, &source_digest) {
                Ok(binding)
                    if binding.source_id == source_id
                        && binding.source_path == source_path
                        && binding.source_digest == source_digest => {}
                Ok(_) => reasons.push("source_manifest_binding_mismatch"),
                Err(_) => reasons.push("source_manifest_binding_missing"),
            }
            if run_id != expected_run_id {
                reasons.push("run_id_not_current");
            }
            if expected_pipeline_invocation.is_empty()
                || pipeline_invocation_id != expected_pipeline_invocation
            {
                reasons.push("pipeline_invocation_not_current");
            }
        }
        if !reasons.is_empty() {
            violations.push(AuthenticityViolation {
                artifact: artifact.to_string(),
                line_number: offset + 2,
                marker: "missing_positive_provenance".to_string(),
                reason: reasons.join(";"),
            });
        }
    }
}

fn validate_candidate_detail_provenance(
    artifact: &str,
    text: &str,
    current_inputs: &BTreeSet<(String, String)>,
    violations: &mut Vec<AuthenticityViolation>,
) {
    let value: Value = match serde_json::from_str(text.trim_start_matches('\u{feff}')) {
        Ok(value) => value,
        Err(err) => {
            violations.push(AuthenticityViolation {
                artifact: artifact.to_string(),
                line_number: 0,
                marker: "invalid_json".to_string(),
                reason: err.to_string(),
            });
            return;
        }
    };
    for (index, candidate) in value
        .get("candidates")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
    {
        let input_id = candidate
            .get("input_id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let record_id = candidate
            .get("record_id")
            .and_then(Value::as_str)
            .unwrap_or("record_1");
        let target_id = candidate
            .get("target_id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let namespace = candidate
            .get("identity_namespace")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let candidate_id = candidate
            .get("candidate_id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let expected_id = candidate_id_for(input_id, record_id, namespace, target_id);
        if !current_inputs.contains(&(input_id.to_string(), record_id.to_string()))
            || input_id.is_empty()
            || record_id.is_empty()
            || target_id.is_empty()
            || namespace.is_empty()
            || candidate_id != expected_id
        {
            violations.push(AuthenticityViolation {
                artifact: artifact.to_string(),
                line_number: index + 1,
                marker: "missing_positive_provenance".to_string(),
                reason: "candidate_identity_not_bound_to_current_input_record_target".to_string(),
            });
        }
    }
}

fn write_authenticity_gate_report(
    options: &ScienceOptions,
    violations: &[AuthenticityViolation],
) -> Result<(), String> {
    let mut text = String::from("artifact\tstatus\tline_number\tmarker\treason\n");
    if violations.is_empty() {
        text.push_str(
            "final_run_artifacts\tpassed\t0\tpositive_provenance\tall final rows bind to current run input, record, source and validated scientific artifacts\n",
        );
    } else {
        for violation in violations {
            text.push_str(&format!(
                "{}\tblocked\t{}\t{}\t{}\n",
                clean_cell(&violation.artifact),
                violation.line_number,
                clean_cell(&violation.marker),
                clean_cell(&violation.reason)
            ));
        }
    }
    write_text(&options.run_dir.join("authenticity_gate.tsv"), &text)
}

fn ranking_route(input_kind: &str) -> RankingRoute {
    match input_kind {
        "small_molecule" => RankingRoute {
            key: "small_molecule_route_v1",
            known: 0.45,
            metagenome: 0.0,
            structure: 0.20,
            docking: 0.15,
            md: 0.20,
        },
        "peptide" => RankingRoute {
            key: "peptide_route_v2",
            known: 0.30,
            metagenome: 0.20,
            structure: 0.50,
            docking: 0.0,
            md: 0.0,
        },
        "protein_ligand" => RankingRoute {
            key: "protein_ligand_route_v2",
            known: 0.20,
            metagenome: 0.10,
            structure: 0.70,
            docking: 0.0,
            md: 0.0,
        },
        "complex" => RankingRoute {
            key: "complex_route_v2",
            known: 0.15,
            metagenome: 0.05,
            structure: 0.80,
            docking: 0.0,
            md: 0.0,
        },
        "genome_or_metagenome" => RankingRoute {
            key: "genome_or_metagenome_route_v1",
            known: 0.45,
            metagenome: 0.55,
            structure: 0.0,
            docking: 0.0,
            md: 0.0,
        },
        _ => RankingRoute {
            key: "fallback_route_v2",
            known: 0.50,
            metagenome: 0.30,
            structure: 0.20,
            docking: 0.0,
            md: 0.0,
        },
    }
}

fn ranking_input_kinds(
    options: &ScienceOptions,
) -> Result<BTreeMap<(String, String), String>, String> {
    let path = options.run_dir.join("input_conversion_manifest.tsv");
    if !path.is_file() {
        return Ok(BTreeMap::new());
    }
    let mut kinds = BTreeMap::new();
    for row in read_simple_tsv_rows(&path)? {
        if row.get("conversion_status").map(String::as_str) != Some("converted") {
            continue;
        }
        let input_id = row.get("input_id").cloned().unwrap_or_default();
        let record_id = row.get("record_id").cloned().unwrap_or_default();
        let input_kind = row.get("canonical_kind").cloned().unwrap_or_default();
        if input_id.is_empty() || record_id.is_empty() || input_kind.is_empty() {
            return Err("ranking_input_identity_incomplete".to_string());
        }
        if let Some(existing) = kinds.insert((input_id, record_id), input_kind.clone())
            && existing != input_kind
        {
            return Err("ranking_input_kind_conflict".to_string());
        }
    }
    Ok(kinds)
}

fn bind_candidate_ranking_routes(
    options: &ScienceOptions,
    candidates: &mut [Candidate],
) -> Result<(), String> {
    let input_kinds = ranking_input_kinds(options)?;
    let conversion_manifest_exists = options
        .run_dir
        .join("input_conversion_manifest.tsv")
        .is_file();
    for candidate in candidates {
        candidate.input_kind =
            match input_kinds.get(&(candidate.input_id.clone(), candidate.record_id.clone())) {
                Some(input_kind) => input_kind.clone(),
                None if !conversion_manifest_exists => "unknown".to_string(),
                None => {
                    return Err(format!(
                        "ranking_input_kind_missing:{}:{}",
                        candidate.input_id, candidate.record_id
                    ));
                }
            };
        candidate.ranking_policy = ranking_route(&candidate.input_kind).key.to_string();
    }
    Ok(())
}

fn sort_ranked_candidates(candidates: &mut [Candidate]) {
    candidates.sort_by(|left, right| {
        left.input_id
            .cmp(&right.input_id)
            .then_with(|| left.record_id.cmp(&right.record_id))
            .then_with(|| right.final_score.total_cmp(&left.final_score))
            .then_with(|| left.identity_namespace.cmp(&right.identity_namespace))
            .then_with(|| left.target_id.cmp(&right.target_id))
            .then_with(|| left.candidate_id.cmp(&right.candidate_id))
    });
}

fn candidate_group_ranks(candidates: &[Candidate]) -> Vec<usize> {
    let mut ranks = BTreeMap::<(String, String), usize>::new();
    candidates
        .iter()
        .map(|candidate| {
            *ranks
                .entry((candidate.input_id.clone(), candidate.record_id.clone()))
                .and_modify(|value| *value += 1)
                .or_insert(1)
        })
        .collect()
}

fn rank_targets(options: &ScienceOptions) -> Result<String, String> {
    validate_ligand_similarity_query_bindings(options)?;
    ensure_final_artifacts_authentic(
        options,
        &[
            "known_database_matches.tsv",
            "metagenome_potential_matches.tsv",
            "structure_interaction_matches.tsv",
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
    assign_collision_free_candidate_ids(&mut candidates);
    apply_docking_scores(&mut candidates, options)?;
    apply_md_scores(&mut candidates, options)?;
    bind_candidate_ranking_routes(options, &mut candidates)?;
    for candidate in &mut candidates {
        let route = ranking_route(&candidate.input_kind);
        candidate.final_score = route.score(candidate);
    }
    sort_ranked_candidates(&mut candidates);
    let mut text = format!("{TARGET_CANDIDATE_RANKING_HEADER}\n");
    let ranks = candidate_group_ranks(&candidates);
    for (candidate, rank) in candidates.iter().zip(ranks) {
        let route = ranking_route(&candidate.input_kind);
        text.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{:.4}\t{:.4}\t{:.4}\t{:.4}\t{:.4}\t{:.4}\t{}\tcomputed_prediction\t{}\t{}\t{}\t{}\t{}\t{:.4}\t{:.4}\t{:.4}\t{:.4}\t{:.4}\n",
            rank,
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
            clean_cell(&candidate.evidence),
            clean_cell(&candidate.input_id),
            clean_cell(&candidate.record_id),
            clean_cell(&candidate.identity_namespace),
            clean_cell(&candidate.input_kind),
            clean_cell(&candidate.ranking_policy),
            route.known,
            route.metagenome,
            route.structure,
            route.docking,
            route.md,
        ));
    }
    write_text(&options.run_dir.join("target_candidate_ranking.tsv"), &text)?;
    write_candidate_detail(options, &candidates)?;
    Ok(format!(
        "targets_ranked\tcandidate_count={}",
        candidates.len()
    ))
}

fn apply_docking_scores(
    candidates: &mut [Candidate],
    options: &ScienceOptions,
) -> Result<(), String> {
    let docking_scores =
        read_docking_scores(options, &options.run_dir.join("docking_summary.tsv"))?;
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

fn read_docking_scores(
    options: &ScienceOptions,
    path: &Path,
) -> Result<BTreeMap<String, f64>, String> {
    if !path.exists() {
        return Ok(BTreeMap::new());
    }
    let manifests = validated_docking_manifests(options)?;
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
    let mut completed_rows = BTreeSet::new();
    for line in lines {
        let cols = split_tsv(line);
        let candidate_id = cell(&cols, candidate_idx);
        let status = cell(&cols, status_idx).to_ascii_lowercase();
        let engine = cell(&cols, engine_idx).to_ascii_lowercase();
        if candidate_id.is_empty() || status != "completed" || !engine.contains("vina") {
            continue;
        }
        if !completed_rows.insert(candidate_id.clone()) {
            return Err(format!(
                "docking_summary_candidate_duplicate:{candidate_id}"
            ));
        }
        let manifest = manifests
            .get(&candidate_id)
            .ok_or_else(|| format!("docking_manifest_missing_for_candidate:{candidate_id}"))?;
        let pose = docking_pose_from_notes(&cell(&cols, notes_idx))
            .ok_or_else(|| format!("docking_pose_missing_for_candidate:{candidate_id}"))?;
        let pose_relative = current_run_artifact_relative(&options.run_dir, &pose)
            .ok_or_else(|| format!("docking_pose_outside_current_run:{candidate_id}"))?;
        if !manifest.contains(&pose_relative) {
            return Err(format!(
                "docking_pose_not_bound_by_manifest:{candidate_id}:{pose_relative}"
            ));
        }
        if let Some(score) = docking_score_from_affinity(&cell(&cols, score_idx)) {
            scores.insert(candidate_id, score);
        } else {
            return Err(format!("docking_score_invalid:{candidate_id}"));
        }
    }
    if completed_rows != manifests.keys().cloned().collect::<BTreeSet<_>>() {
        return Err("docking_summary_manifest_candidate_mismatch".to_string());
    }
    Ok(scores)
}

fn docking_pose_from_notes(notes: &str) -> Option<String> {
    for key in ["pose", "pose_sdf", "docking_pose", "docking_pose_sdf"] {
        let value = evidence_field(notes, &[key]);
        if !value.trim().is_empty() {
            return Some(value);
        }
    }
    None
}

fn validated_docking_manifests(
    options: &ScienceOptions,
) -> Result<BTreeMap<String, BTreeSet<String>>, String> {
    if options
        .run_dir
        .join("tool_outputs/docking_manifest_index.json")
        .is_file()
    {
        return validated_indexed_docking_manifests(options);
    }
    let manifest_path = options.run_dir.join("tool_outputs/docking_manifest.json");
    if !manifest_path.is_file() {
        return Ok(BTreeMap::new());
    }
    let text = read_text(&manifest_path)?;
    let manifest: Value = serde_json::from_str(text.trim_start_matches('\u{feff}'))
        .map_err(|err| format!("docking_manifest_parse_failed:{err}"))?;
    if manifest.get("status").and_then(Value::as_str) != Some("completed") {
        return Err("docking_manifest_not_completed".to_string());
    }
    let candidate_id = manifest
        .get("candidate_id")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "docking_manifest_candidate_missing".to_string())?;
    let input_id = manifest
        .get("input_id")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "docking_manifest_input_missing".to_string())?;
    let invocation_id = manifest
        .get("invocation_id")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "docking_manifest_invocation_missing".to_string())?;
    let expected_invocation = read_text(&options.run_dir.join(WORKER_INVOCATION_FILE))?
        .trim()
        .to_string();
    if invocation_id != expected_invocation {
        return Err("docking_manifest_invocation_mismatch".to_string());
    }
    let current_inputs = current_run_input_identities(options)?;
    if !current_inputs
        .iter()
        .any(|(current, _)| current == input_id)
    {
        return Err("docking_manifest_input_not_in_current_run".to_string());
    }
    let artifacts = manifest
        .get("artifacts")
        .and_then(Value::as_array)
        .filter(|rows| !rows.is_empty())
        .ok_or_else(|| "docking_manifest_artifacts_missing".to_string())?;
    let mut verified = BTreeSet::new();
    for artifact in artifacts {
        let path = artifact
            .get("path")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| "docking_manifest_artifact_path_missing".to_string())?;
        let expected_digest = artifact
            .get("sha256")
            .and_then(Value::as_str)
            .filter(|value| valid_sha256_hex(value))
            .ok_or_else(|| "docking_manifest_artifact_digest_missing".to_string())?;
        let relative = current_run_artifact_relative(&options.run_dir, path)
            .ok_or_else(|| format!("docking_manifest_artifact_outside_run:{path}"))?;
        let actual = sha256_file_hex(&options.run_dir.join(&relative))
            .ok_or_else(|| format!("docking_manifest_artifact_unreadable:{relative}"))?;
        if !actual.eq_ignore_ascii_case(expected_digest) {
            return Err(format!(
                "docking_manifest_artifact_digest_mismatch:{relative}"
            ));
        }
        if !verified.insert(relative.clone()) {
            return Err(format!("docking_manifest_duplicate_artifact:{relative}"));
        }
    }
    Ok(BTreeMap::from([(candidate_id.to_string(), verified)]))
}

fn validated_indexed_docking_manifests(
    options: &ScienceOptions,
) -> Result<BTreeMap<String, BTreeSet<String>>, String> {
    let entries = validated_candidate_manifest_index(options, "docking", &DOCKING_WORKER_TASK)?;
    let selected = selected_candidate_rows(options)?
        .into_iter()
        .filter_map(|row| row.get("candidate_id").cloned().map(|id| (id, row)))
        .collect::<BTreeMap<_, _>>();
    let expected_artifact_names = BTreeSet::from([
        "ligand_pdbqt",
        "ligand_sdf",
        "log",
        "pose",
        "pose_sdf",
        "receptor_coordinate",
        "receptor_pdb",
        "receptor_pdbqt",
    ]);
    let mut manifests = BTreeMap::new();
    for entry in entries {
        if entry.status != "completed" {
            continue;
        }
        let candidate_id = entry.facts.candidate_id.as_str();
        let manifest_relative = current_run_artifact_relative(&options.run_dir, &entry.manifest)
            .ok_or_else(|| format!("docking_manifest_invalid:{candidate_id}"))?;
        let manifest = read_json_value_if_file(&options.run_dir.join(&manifest_relative))
            .ok_or_else(|| format!("docking_manifest_invalid:{candidate_id}"))?;
        if manifest
            .get("artifact_schema_version")
            .and_then(Value::as_str)
            != Some("genoligand_docking_artifacts_v1")
            || manifest.get("status").and_then(Value::as_str) != Some("completed")
            || manifest.get("method").and_then(Value::as_str) != Some("autodock_vina")
        {
            return Err(format!("docking_manifest_schema_invalid:{candidate_id}"));
        }
        for (field, expected) in [
            ("run_id", entry.facts.run_id.as_str()),
            ("job_id", entry.job_id.as_str()),
            ("invocation_id", entry.invocation_id.as_str()),
            ("candidate_id", entry.facts.candidate_id.as_str()),
            ("candidate_digest", entry.facts.candidate_digest.as_str()),
            ("input_id", entry.facts.input_id.as_str()),
            ("record_id", entry.facts.record_id.as_str()),
            ("input_digest", entry.facts.input_digest.as_str()),
            ("source_content_sha256", entry.facts.source_digest.as_str()),
        ] {
            if manifest.get(field).and_then(Value::as_str) != Some(expected) {
                return Err(format!(
                    "docking_manifest_identity_mismatch:{candidate_id}:{field}"
                ));
            }
        }
        if manifest.get("ligand_record_id").and_then(Value::as_str)
            != Some(entry.facts.record_id.as_str())
        {
            return Err(format!("docking_manifest_record_mismatch:{candidate_id}"));
        }
        let selected_row = selected
            .get(candidate_id)
            .ok_or_else(|| format!("docking_selection_missing:{candidate_id}"))?;
        let receptor_artifact = selected_row
            .get("receptor_artifact")
            .map(|value| value.replace('\\', "/"))
            .unwrap_or_default();
        let receptor_digest = selected_row
            .get("coordinate_sha256")
            .map(String::as_str)
            .unwrap_or_default();
        if !valid_sha256_hex(receptor_digest)
            || manifest.get("receptor_digest").and_then(Value::as_str) != Some(receptor_digest)
            || manifest.get("receptor_sha256").and_then(Value::as_str) != Some(receptor_digest)
            || manifest
                .get("receptor_coordinate")
                .and_then(Value::as_str)
                .and_then(|path| current_run_artifact_relative(&options.run_dir, path))
                .as_deref()
                != Some(receptor_artifact.as_str())
        {
            return Err(format!("docking_manifest_receptor_mismatch:{candidate_id}"));
        }
        let rows = manifest
            .get("artifacts")
            .and_then(Value::as_array)
            .filter(|rows| !rows.is_empty())
            .ok_or_else(|| format!("docking_manifest_artifacts_missing:{candidate_id}"))?;
        let invocation_parent = Path::new(&manifest_relative)
            .parent()
            .ok_or_else(|| format!("docking_manifest_parent_missing:{candidate_id}"))?;
        let mut artifact_names = BTreeSet::new();
        let mut artifacts = BTreeSet::new();
        let mut named_paths = BTreeMap::new();
        for row in rows {
            let artifact_name = row
                .get("artifact_name")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| format!("docking_artifact_name_missing:{candidate_id}"))?;
            if !artifact_names.insert(artifact_name) {
                return Err(format!(
                    "docking_artifact_name_duplicate:{candidate_id}:{artifact_name}"
                ));
            }
            for (field, expected) in [
                ("run_id", entry.facts.run_id.as_str()),
                ("job_id", entry.job_id.as_str()),
                ("invocation_id", entry.invocation_id.as_str()),
                ("candidate_id", entry.facts.candidate_id.as_str()),
                ("candidate_digest", entry.facts.candidate_digest.as_str()),
                ("input_id", entry.facts.input_id.as_str()),
                ("record_id", entry.facts.record_id.as_str()),
                ("input_digest", entry.facts.input_digest.as_str()),
                ("source_digest", entry.facts.source_digest.as_str()),
            ] {
                if row.get(field).and_then(Value::as_str) != Some(expected) {
                    return Err(format!(
                        "docking_artifact_identity_mismatch:{candidate_id}:{artifact_name}:{field}"
                    ));
                }
            }
            let relative = row
                .get("artifact_path")
                .and_then(Value::as_str)
                .and_then(|path| current_run_artifact_relative(&options.run_dir, path))
                .ok_or_else(|| {
                    format!("docking_artifact_path_invalid:{candidate_id}:{artifact_name}")
                })?;
            let expected_digest = row
                .get("artifact_sha256")
                .and_then(Value::as_str)
                .filter(|value| valid_sha256_hex(value))
                .ok_or_else(|| {
                    format!("docking_artifact_digest_missing:{candidate_id}:{artifact_name}")
                })?;
            let actual_digest =
                sha256_file_hex(&options.run_dir.join(&relative)).ok_or_else(|| {
                    format!("docking_artifact_unreadable:{candidate_id}:{artifact_name}")
                })?;
            if !actual_digest.eq_ignore_ascii_case(expected_digest)
                || (artifact_name == "receptor_coordinate"
                    && (relative != receptor_artifact
                        || row.get("receptor_digest").and_then(Value::as_str)
                            != Some(receptor_digest)))
                || (artifact_name != "receptor_coordinate"
                    && Path::new(&relative).parent() != Some(invocation_parent))
                || !artifacts.insert(relative.clone())
            {
                return Err(format!(
                    "docking_artifact_binding_invalid:{candidate_id}:{artifact_name}"
                ));
            }
            named_paths.insert(artifact_name, relative);
        }
        if artifact_names != expected_artifact_names {
            return Err(format!("docking_artifact_set_mismatch:{candidate_id}"));
        }
        for field in [
            "receptor_pdb",
            "receptor_pdbqt",
            "ligand_sdf",
            "ligand_pdbqt",
            "pose",
            "pose_sdf",
            "log",
        ] {
            if manifest.get(field).and_then(Value::as_str)
                != named_paths.get(field).map(String::as_str)
            {
                return Err(format!(
                    "docking_manifest_artifact_mismatch:{candidate_id}:{field}"
                ));
            }
        }
        manifests.insert(candidate_id.to_string(), artifacts);
    }
    Ok(manifests)
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
    let current_summary = materialize_current_md_summary(options)?;
    if !current_summary.ready {
        return Ok(());
    }
    if options
        .run_dir
        .join("tool_outputs/md_manifest_index.json")
        .is_file()
    {
        let completed =
            candidate_index_statuses(&options.run_dir.join("tool_outputs/md_manifest_index.json"))
                .into_iter()
                .filter_map(|(candidate_id, status)| {
                    (status == "completed").then_some(candidate_id)
                })
                .collect::<BTreeSet<_>>();
        let md_scores = read_md_scores(&options.run_dir.join("md_summary.tsv"))?;
        for candidate in candidates {
            if completed.contains(&candidate.candidate_id)
                && let Some(score) = md_scores.get(&candidate.candidate_id)
            {
                candidate.dynamics_score = *score;
                append_evidence_once(&mut candidate.evidence, "MD:manifest_bound_stability_score");
            }
        }
        return Ok(());
    }
    let Some(bound_candidate_id) = current_summary.candidate_id else {
        return Ok(());
    };
    let complex_candidates = complex_md_candidate_ids(options)?;
    if !complex_candidates.contains(&bound_candidate_id) {
        return Ok(());
    }
    let md_scores = read_md_scores(&options.run_dir.join("md_summary.tsv"))?;
    if md_scores.is_empty() {
        return Ok(());
    }
    for candidate in candidates {
        if candidate.candidate_id != bound_candidate_id {
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
    let mut files = vec![
        "run_context.json",
        "authenticity_gate.tsv",
        "artifact_source_map.tsv",
        "input_conversion_manifest.tsv",
        "input_normalized_manifest.tsv",
        "query_structure_manifest.tsv",
        "feature_manifest.tsv",
        "tool_outputs/mmseqs_alignment_matches.tsv",
        "tool_outputs/mmseqs_alignment_status.tsv",
        "known_database_matches.tsv",
        "metagenome_potential_matches.tsv",
        "structure_interaction_matches.tsv",
        "tool_outputs/foldseek_structure_matches.tsv",
        "tool_outputs/foldseek_alignment_status.tsv",
        "candidate_shortlist.tsv",
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
    ]
    .into_iter()
    .map(str::to_string)
    .collect::<Vec<_>>();
    if options
        .run_dir
        .join("tool_outputs/docking_manifest_index.json")
        .is_file()
    {
        files.push("tool_outputs/docking_manifest_index.json".to_string());
        let manifests = validated_indexed_docking_manifests(options)?;
        for artifacts in manifests.values() {
            for artifact in artifacts {
                files.push(artifact.clone());
            }
        }
        for entry in validated_candidate_manifest_index(options, "docking", &DOCKING_WORKER_TASK)? {
            if !entry.manifest.is_empty() {
                files.push(entry.manifest);
            }
        }
    }
    if options
        .run_dir
        .join("tool_outputs/md_manifest_index.json")
        .is_file()
    {
        files.push("tool_outputs/md_manifest_index.json".to_string());
        for entry in validated_candidate_manifest_index(options, "md", &SCIENTIFIC_WORKER_TASKS[0])?
        {
            if !entry.manifest.is_empty() {
                files.push(entry.manifest.clone());
            }
            if !entry.artifact_provenance.is_empty() {
                for artifact in validated_md_artifact_provenance(options, &entry)? {
                    files.push(artifact);
                }
                files.push(entry.artifact_provenance);
            }
        }
    }
    files.sort_unstable();
    files.dedup();
    let mut text = String::from("claim_id\tevidence_artifact\towner\tstatus\tnotes\n");
    for file in files {
        let path = options.run_dir.join(&file);
        if path.exists() {
            let status = artifact_evidence_status(options, &file, &path)?;
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

fn artifact_evidence_status(
    options: &ScienceOptions,
    file: &str,
    path: &Path,
) -> Result<ArtifactEvidenceStatus, String> {
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
        "query_structure_manifest.tsv" => {
            if rows > 0 {
                (
                    "available",
                    format!("{rows} query-side structure preparation rows"),
                )
            } else {
                (
                    "not_generated",
                    "no query-side structure preparation rows".to_string(),
                )
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
        "candidate_shortlist.tsv" => {
            if rows > 0 {
                (
                    "available",
                    format!("{rows} candidates ranked before docking and dynamics"),
                )
            } else {
                (
                    "no_match",
                    "no candidates passed preliminary retrieval".to_string(),
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
            let verified = rows > 0 && !read_docking_scores(options, path)?.is_empty();
            if verified {
                ("available", format!("{rows} verified docking rows"))
            } else {
                (
                    "not_generated",
                    "no current-run docking rows with verified pose provenance".to_string(),
                )
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
            let facts = verified_current_md_facts(options)?;
            if !facts.is_empty() {
                let candidates = facts
                    .iter()
                    .map(|facts| facts.candidate_id.as_str())
                    .collect::<Vec<_>>()
                    .join(",");
                let platforms = facts
                    .iter()
                    .map(|facts| facts.platform.as_str())
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect::<Vec<_>>()
                    .join(",");
                (
                    "available",
                    format!(
                        "{rows} verified OpenMM complex MD rows for {candidates}; platform {platforms}; final-score contribution follows each candidate ranking_policy md_weight"
                    ),
                )
            } else {
                (
                    "not_generated",
                    "no current-run receptor-ligand MD artifacts with complete provenance"
                        .to_string(),
                )
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
        text.push_str("| \u{8f93}\u{5165} | \u{8bb0}\u{5f55} | \u{6392}\u{5e8f} | \u{9776}\u{70b9} | \u{5019}\u{9009}\u{8303}\u{56f4} | \u{7269}\u{79cd} | \u{7efc}\u{5408}\u{5206}\u{6570} | \u{8bc1}\u{636e}\u{6458}\u{8981} |\n");
        text.push_str("| --- | --- | --- | --- | --- | --- | --- | --- |\n");
        let mut group_counts = BTreeMap::<(String, String), usize>::new();
        let mut displayed = 0usize;
        for candidate in &candidates {
            let rank = group_counts
                .entry((candidate.input_id.clone(), candidate.record_id.clone()))
                .or_default();
            if *rank == REPORT_CANDIDATE_LIMIT {
                continue;
            }
            *rank += 1;
            displayed += 1;
            text.push_str(&format!(
                "| {} | {} | {} | {} ({}) | {} | {} | {:.4} | {} |\n",
                clean_cell(&candidate.input_id),
                clean_cell(&candidate.record_id),
                *rank,
                clean_cell(&candidate.target_name),
                clean_cell(&candidate.target_id),
                report_scope_label(&candidate.candidate_scope),
                clean_cell(&candidate.organism),
                candidate.final_score,
                clean_cell(&candidate.evidence)
            ));
        }
        if displayed < candidates.len() {
            text.push_str(&format!(
                "\n\u{5171}\u{751f}\u{6210} {} \u{4e2a}\u{5019}\u{9009}\u{9776}\u{70b9}\u{ff1b}\u{62a5}\u{544a}\u{5bf9}\u{6bcf}\u{4e2a}\u{8f93}\u{5165}\u{8bb0}\u{5f55}\u{5c55}\u{793a}\u{524d} {} \u{4e2a}\u{ff0c}\u{5b8c}\u{6574}\u{6392}\u{540d}\u{53ef}\u{5728}\u{5bfc}\u{51fa}\u{7684}\u{5019}\u{9009}\u{6392}\u{5e8f}\u{8868}\u{4e2d}\u{67e5}\u{770b}\u{3002}\n",
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
    let complex = verified_current_md_facts(options)?;
    let peptide = if options
        .run_dir
        .join("tool_outputs/md_manifest_index.json")
        .is_file()
    {
        verified_indexed_md_computations(options)?.peptide
    } else {
        Vec::new()
    };
    let mut text = String::from("## \u{52a8}\u{529b}\u{5b66}\u{8bc4}\u{5206}\n\n");
    if complex.is_empty() {
        text.push_str("\u{5f53}\u{524d}\u{8fd0}\u{884c}\u{6ca1}\u{6709}\u{901a}\u{8fc7}\u{5019}\u{9009}\u{3001}\u{8f93}\u{5165}\u{548c}\u{4ea7}\u{7269}\u{7ed1}\u{5b9a}\u{6821}\u{9a8c}\u{7684} receptor-ligand complex MD \u{8bc1}\u{636e}\u{ff0c}md_score \u{4fdd}\u{6301} 0.0000\u{ff0c}\u{5bf9} final_score \u{8d21}\u{732e}\u{4e3a} 0\u{3002}\n\n");
    } else {
        let verified_ids = complex
            .iter()
            .map(|facts| facts.candidate_id.as_str())
            .collect::<BTreeSet<_>>();
        let ranked_md = candidates
            .iter()
            .filter(|candidate| {
                verified_ids.contains(candidate.candidate_id.as_str())
                    && candidate.dynamics_score > 0.0
            })
            .count();
        text.push_str(&format!(
            "md_summary.tsv \u{4e2d}\u{7684} md_stability_score \u{5df2}\u{901a}\u{8fc7}\u{9010}\u{5019}\u{9009}\u{4ea7}\u{7269}\u{6821}\u{9a8c}\u{ff0c}\u{5171} {} \u{4e2a} receptor-ligand complex MD\u{ff1b}{ranked_md} \u{4e2a}\u{5019}\u{9009}\u{6309}\u{5404}\u{81ea} input_kind \u{8def}\u{7ebf}\u{7684} md_weight \u{53c2}\u{4e0e} final_score\u{3002}\n\n",
            complex.len()
        ));
        for facts in &complex {
            let gpu_observed = if facts.gpu_observed {
                "\u{5df2}\u{89c2}\u{6d4b}"
            } else {
                "\u{672a}\u{89c2}\u{6d4b}"
            };
            let telemetry = facts
                .telemetry
                .as_deref()
                .unwrap_or("\u{8fd0}\u{884c}\u{6e05}\u{5355}\u{672a}\u{8bb0}\u{5f55}");
            let model_fields = [
                facts
                    .force_field
                    .as_deref()
                    .map(|value| format!("force_field={value}")),
                facts
                    .solvent_model
                    .as_deref()
                    .map(|value| format!("solvent_model={value}")),
                facts
                    .nonbonded_method
                    .as_deref()
                    .map(|value| format!("nonbonded_method={value}")),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(", ");
            text.push_str(&format!(
                "- \u{5019}\u{9009} {}\u{ff1a}OpenMM \u{5e73}\u{53f0} {}\u{ff0c}GPU {}\u{ff0c}\u{6a21}\u{578b} {}\u{ff0c}\u{534f}\u{8bae} {}\u{ff0c}\u{72b6}\u{6001} {}\u{ff0c}\u{76d1}\u{6d4b} {}\u{ff0c}\u{8f68}\u{8ff9} {}\u{3002}\n",
                facts.candidate_id,
                facts.platform,
                gpu_observed,
                if model_fields.is_empty() {"\u{6e05}\u{5355}\u{672a}\u{8bb0}\u{5f55}"} else {&model_fields},
                facts.protocol,
                facts.state,
                telemetry,
                facts.trajectory,
            ));
        }
        text.push('\n');
    }
    if !peptide.is_empty() {
        text.push_str("### \u{591a}\u{80bd}\u{6784}\u{8c61}\u{96c6}\n\n");
        for facts in peptide {
            text.push_str(&format!(
                "- \u{5019}\u{9009} {}\u{ff1a}RDKit peptide conformer ensemble\u{ff0c}\u{6784}\u{8c61}\u{6570} {}\u{ff0c}has_receptor=false\u{ff0c}uses_openmm=false\u{ff0c}md_score=0.0000\u{ff0c}\u{4ea7}\u{7269}\u{6e05}\u{5355} {}\u{3002}\n",
                facts.candidate_id, facts.conformer_count, facts.manifest
            ));
        }
        text.push('\n');
    }
    Ok(text)
}

fn report_source_summary(options: &ScienceOptions, candidate_count: usize) -> String {
    let project_id = clean_cell(&infer_project_id(&options.run_dir));
    let run_id = clean_cell(&infer_run_id(&options.run_dir));
    let policy_digest = ranking_policy_digest();
    let input_status = if options.input_manifest.exists() {
        "\u{5df2}\u{8bfb}\u{53d6}\u{5f53}\u{524d}\u{9879}\u{76ee}\u{8f93}\u{5165}"
    } else {
        "\u{672a}\u{8bfb}\u{53d6}\u{5230}\u{5f53}\u{524d}\u{9879}\u{76ee}\u{8f93}\u{5165}"
    };
    let database_status = match evaluate_science_store_readiness(&options.science_store) {
        Ok(readiness) if readiness.ready && readiness.advisories.is_empty() => {
            "\u{79d1}\u{5b66}\u{8d44}\u{6e90}\u{5df2}\u{5c31}\u{7eea}".to_string()
        }
        Ok(readiness) if readiness.ready => format!(
            "\u{79d1}\u{5b66}\u{8d44}\u{6e90}\u{5df2}\u{5c31}\u{7eea}\u{ff0c}\u{5efa}\u{8bae}\u{7ef4}\u{62a4}\u{ff1a}{}",
            readiness_advisory_details(&readiness)
        ),
        Ok(readiness) => format!(
            "\u{79d1}\u{5b66}\u{8d44}\u{6e90}\u{9700}\u{8981}\u{4fee}\u{590d}\u{ff1a}{}",
            readiness_blocker_details(&readiness)
        ),
        Err(reason) => format!(
            "\u{79d1}\u{5b66}\u{8d44}\u{6e90}\u{5c31}\u{7eea}\u{5224}\u{5b9a}\u{5931}\u{8d25}\u{ff1a}{}",
            clean_cell(&reason)
        ),
    };
    let output_status = if candidate_count > 0 {
        format!(
            "\u{5f53}\u{524d}\u{8fd0}\u{884c}\u{751f}\u{6210} {candidate_count} \u{4e2a}\u{5019}\u{9009}\u{5047}\u{8bbe}\u{ff0c}\u{6392}\u{5e8f}\u{3001}\u{8be6}\u{60c5}\u{548c}\u{8bc1}\u{636e}\u{6458}\u{8981}\u{5747}\u{6765}\u{81ea}\u{5f53}\u{524d}\u{8fd0}\u{884c}"
        )
    } else {
        "\u{5f53}\u{524d}\u{8fd0}\u{884c}\u{6ca1}\u{6709}\u{53ef}\u{5c55}\u{793a}\u{7684}\u{5019}\u{9009}\u{5047}\u{8bbe}".to_string()
    };
    format!(
        "## \u{7ed3}\u{679c}\u{6765}\u{6e90}\n\n- \u{9879}\u{76ee}\u{7f16}\u{53f7}\u{ff1a}{project_id}\n- \u{8fd0}\u{884c}\u{7f16}\u{53f7}\u{ff1a}{run_id}\n- \u{8f93}\u{5165}\u{ff1a}{input_status}\n- \u{6570}\u{636e}\u{5e93}\u{ff1a}{database_status}\n- \u{6392}\u{5e8f}\u{7b56}\u{7565}\u{ff1a}{RANKING_POLICY_VERSION}\u{ff0c}digest={policy_digest}\n- \u{8f93}\u{51fa}\u{6750}\u{6599}\u{ff1a}{output_status}\n\n",
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

fn ranking_policy_value() -> Value {
    let route_value = |input_kind: &str| {
        let route = ranking_route(input_kind);
        let (preliminary_known, preliminary_metagenome, preliminary_structure) =
            route.preliminary_weights();
        json!({
            "ranking_policy": route.key,
            "known_database_weight": route.known,
            "metagenome_weight": route.metagenome,
            "structure_weight": route.structure,
            "docking_weight": route.docking,
            "md_weight": route.md,
            "preliminary_known_database_weight": preliminary_known,
            "preliminary_metagenome_weight": preliminary_metagenome,
            "preliminary_structure_weight": preliminary_structure,
        })
    };
    json!({
        "schema_version": RANKING_POLICY_VERSION,
        "score_semantics": "input_kind_route_weighted_sum",
        "shortlist_score_semantics": "route_weights_renormalized_over_known_metagenome_structure",
        "routes": {
            "small_molecule": route_value("small_molecule"),
            "peptide": route_value("peptide"),
            "protein_ligand": route_value("protein_ligand"),
            "complex": route_value("complex"),
            "genome_or_metagenome": route_value("genome_or_metagenome"),
            "fallback": route_value("unknown"),
        },
    })
}

fn ranking_policy_digest() -> String {
    canonical_json_sha256(&ranking_policy_value())
}

fn write_run_context(
    options: &ScienceOptions,
    state: &str,
    blockers: &[String],
) -> Result<(), String> {
    let project_id = infer_project_id(&options.run_dir);
    let run_id = infer_run_id(&options.run_dir);
    let pipeline_invocation_id = read_text(&options.run_dir.join(PIPELINE_INVOCATION_FILE))
        .unwrap_or_default()
        .trim()
        .to_string();
    let input_status = file_context(&options.input_manifest);
    let known_index = options.science_store.join("indexes/known_targets.tsv");
    let metagenome_index = options.science_store.join("indexes/metagenome_targets.tsv");
    let structure_index = options.science_store.join("indexes/structure_targets.tsv");
    let structure_context = structure_file_context(&options.science_store, &structure_index);
    let tool_versions = options.science_store.join("indexes/tool_versions.tsv");
    let ranking_policy = ranking_policy_value();
    let ranking_policy_digest = ranking_policy_digest();
    let context = json!({
        "state": state,
        "project_id": project_id,
        "run_id": run_id,
        "pipeline_invocation_id": pipeline_invocation_id,
        "workspace_root": options.workspace_root.display().to_string(),
        "science_store": options.science_store.display().to_string(),
        "run_dir": options.run_dir.display().to_string(),
        "input_manifest": input_status,
        "database_indexes": {
            "known_targets": file_context(&known_index),
            "metagenome_targets": file_context(&metagenome_index),
            "structure_targets": structure_context,
            "tool_versions": file_context(&tool_versions)
        },
        "ranking_policy": ranking_policy,
        "ranking_policy_digest": &ranking_policy_digest,
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
        ("pipeline_invocation_id", pipeline_invocation_id),
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
        ("ranking_policy_version", RANKING_POLICY_VERSION.to_string()),
        ("ranking_policy_digest", ranking_policy_digest),
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

fn structure_file_context(store: &Path, path: &Path) -> Value {
    let exists = path.is_file();
    let bytes = fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
    let manifest_path = store.join(STRUCTURE_TARGETS_MANIFEST_REL_PATH);
    let content_id = read_json_value_if_file(&manifest_path).and_then(|manifest| {
        let digest = manifest.get("sha256").and_then(Value::as_str)?;
        (exists
            && manifest.get("schema_version").and_then(Value::as_str)
                == Some(STRUCTURE_TARGETS_MANIFEST_SCHEMA_VERSION)
            && manifest.get("status").and_then(Value::as_str) == Some("ready")
            && manifest.get("path").and_then(Value::as_str)
                == Some("indexes/structure_targets.tsv")
            && manifest.get("bytes").and_then(Value::as_u64) == Some(bytes)
            && valid_sha256_hex(digest))
        .then(|| digest.to_ascii_lowercase())
    });
    json!({
        "path": path.display().to_string(),
        "exists": exists,
        "bytes": bytes,
        "content_id": content_id,
        "publication_manifest": manifest_path.display().to_string(),
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
    let mut computation_artifacts = BTreeMap::<String, Vec<String>>::new();
    for facts in verified_current_md_facts(options)? {
        let parent = Path::new(&facts.protocol)
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        let mut artifacts = vec![
            "md_summary.tsv".to_string(),
            "tool_outputs/md_manifest_index.json".to_string(),
        ];
        for name in [
            "complex_md_manifest.json",
            "ligand_placement.tsv",
            "md_run_manifest.json",
            "md_protocol.tsv",
            "md_state.tsv",
            "gpu_telemetry.tsv",
            "gpu_telemetry_status.tsv",
            "md_trajectory.tsv",
            "md_summary.tsv",
            "md_rmsd.tsv",
            "md_rmsf.tsv",
            "md_ligand_drift.tsv",
            "md_contacts.tsv",
            "md_hbonds.tsv",
        ] {
            artifacts.push(parent.join(name).to_string_lossy().replace('\\', "/"));
        }
        computation_artifacts.insert(facts.candidate_id, artifacts);
    }
    if options
        .run_dir
        .join("tool_outputs/md_manifest_index.json")
        .is_file()
    {
        for facts in verified_indexed_md_computations(options)?.peptide {
            let parent = Path::new(&facts.manifest)
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_default();
            let mut artifacts = vec!["tool_outputs/md_manifest_index.json".to_string()];
            for name in [
                "md_run_manifest.json",
                "peptide_conformer_summary.tsv",
                "peptide_conformer_metrics.tsv",
                "peptide_conformer_coordinates.tsv",
                "peptide_conformer_ensemble.tsv",
            ] {
                artifacts.push(parent.join(name).to_string_lossy().replace('\\', "/"));
            }
            computation_artifacts.insert(facts.candidate_id, artifacts);
        }
    }
    let rows = candidates
        .iter()
        .map(|candidate| {
            let route = ranking_route(&candidate.input_kind);
            let mut source_artifacts = vec![
                "input_conversion_manifest.tsv".to_string(),
                "input_normalized_manifest.tsv".to_string(),
                "query_structure_manifest.tsv".to_string(),
                "feature_manifest.tsv".to_string(),
                "known_database_matches.tsv".to_string(),
                "metagenome_potential_matches.tsv".to_string(),
                "structure_interaction_matches.tsv".to_string(),
                "candidate_shortlist.tsv".to_string(),
                "complex_candidate_selection.tsv".to_string(),
                "target_candidate_ranking.tsv".to_string(),
                "score_decomposition.tsv".to_string(),
                "evidence_ledger.tsv".to_string(),
            ];
            if let Some(artifacts) = computation_artifacts.get(&candidate.candidate_id) {
                source_artifacts.extend(artifacts.iter().cloned());
            }
            json!({
                "candidate_id": candidate.candidate_id,
                "input_id": candidate.input_id,
                "record_id": candidate.record_id,
                "input_kind": candidate.input_kind,
                "ranking_policy": candidate.ranking_policy,
                "ranking_weights": {
                    "known_database": route.known,
                    "metagenome": route.metagenome,
                    "structure": route.structure,
                    "docking": route.docking,
                    "md": route.md,
                },
                "identity_namespace": candidate.identity_namespace,
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
            "query_structure_manifest.tsv;feature_manifest.tsv",
        ),
        (
            "query_structure_manifest.tsv",
            "input_conversion_manifest.tsv;query-side RDKit or coordinate structure preparation",
            "feature_manifest.tsv;known_database_matches.tsv;evidence_ledger.tsv",
        ),
        (
            "input_conversion_manifest.tsv",
            "input_manifest.json",
            "input_normalized_manifest.tsv;feature_manifest.tsv;evidence_ledger.tsv",
        ),
        (
            "feature_manifest.tsv",
            "input_conversion_manifest.tsv;input_normalized_manifest.tsv;query_structure_manifest.tsv",
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
            "indexes/known_targets.tsv;indexes/ligand_similarity/ligand_similarity.sqlite;query_structure_manifest.tsv",
            "target_candidate_ranking.tsv",
        ),
        (
            "metagenome_potential_matches.tsv",
            "indexes/metagenome_targets.tsv",
            "target_candidate_ranking.tsv",
        ),
        (
            "target_candidate_ranking.tsv",
            "input_conversion_manifest.tsv;known_database_matches.tsv;metagenome_potential_matches.tsv;structure_interaction_matches.tsv;docking_summary.tsv;md_summary.tsv",
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
            "candidate_shortlist.tsv",
            "input_conversion_manifest.tsv;known_database_matches.tsv;metagenome_potential_matches.tsv;structure_interaction_matches.tsv",
            "complex_candidate_selection.tsv;docking_summary.tsv;md_summary.tsv;target_candidate_ranking.tsv",
        ),
        (
            "complex_candidate_selection.tsv",
            "candidate_shortlist.tsv;local coordinate files",
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
    let coordinate_candidates = selected_candidate_rows(options)?
        .into_iter()
        .filter_map(|row| row.get("candidate_id").cloned())
        .collect::<BTreeSet<_>>();
    let docking_outcomes = candidate_index_statuses(
        &options
            .run_dir
            .join("tool_outputs/docking_manifest_index.json"),
    );
    let md_outcomes =
        candidate_index_statuses(&options.run_dir.join("tool_outputs/md_manifest_index.json"));
    let policy_digest = ranking_policy_digest();
    let mut text = String::from(
        "candidate_id\tcomponent\tvalue\towner\tpolicy_version\tpolicy_digest\tinput_kind\tranking_policy\n",
    );
    for candidate in candidates {
        let route = ranking_route(&candidate.input_kind);
        let ranking_policy = if candidate.ranking_policy.is_empty() {
            route.key
        } else {
            &candidate.ranking_policy
        };
        let mut components = vec![
            ("known_database_score", candidate.known_score),
            ("known_database_score_weight", route.known),
            (
                "known_database_score_contribution",
                candidate.known_score * route.known,
            ),
            ("metagenome_score", candidate.metagenome_score),
            ("metagenome_score_weight", route.metagenome),
            (
                "metagenome_score_contribution",
                candidate.metagenome_score * route.metagenome,
            ),
            ("structure_score", candidate.structure_score),
            ("structure_score_weight", route.structure),
            (
                "structure_score_contribution",
                candidate.structure_score * route.structure,
            ),
            ("docking_score", candidate.docking_score),
            ("docking_score_weight", route.docking),
            (
                "docking_score_contribution",
                candidate.docking_score * route.docking,
            ),
            ("md_score", candidate.dynamics_score),
            ("md_score_weight", route.md),
            ("md_score_contribution", candidate.dynamics_score * route.md),
            ("final_score", candidate.final_score),
        ];
        components.extend([
            (
                "coordinate_backed_candidate",
                if coordinate_candidates.contains(&candidate.candidate_id) {
                    1.0
                } else {
                    0.0
                },
            ),
            (
                "docking_outcome_present",
                if docking_outcomes.contains_key(&candidate.candidate_id) {
                    1.0
                } else {
                    0.0
                },
            ),
            (
                "docking_completed",
                if docking_outcomes
                    .get(&candidate.candidate_id)
                    .map(String::as_str)
                    == Some("completed")
                {
                    1.0
                } else {
                    0.0
                },
            ),
            (
                "md_outcome_present",
                if md_outcomes.contains_key(&candidate.candidate_id) {
                    1.0
                } else {
                    0.0
                },
            ),
            (
                "md_completed",
                if md_outcomes.get(&candidate.candidate_id).map(String::as_str) == Some("completed")
                {
                    1.0
                } else {
                    0.0
                },
            ),
        ]);
        for (component, value) in components {
            text.push_str(&format!(
                "{}\t{component}\t{value:.4}\tGenoLigand\t{}\t{}\t{}\t{}\n",
                candidate.candidate_id,
                RANKING_POLICY_VERSION,
                policy_digest,
                clean_cell(&candidate.input_kind),
                clean_cell(ranking_policy),
            ));
        }
    }
    write_text(&options.run_dir.join("score_decomposition.tsv"), &text)
}

fn candidate_index_statuses(path: &Path) -> BTreeMap<String, String> {
    let Some(index) = read_json_value_if_file(path) else {
        return BTreeMap::new();
    };
    index
        .get("entries")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let candidate_id = entry.get("candidate_id")?.as_str()?.trim();
            let status = entry.get("status")?.as_str()?.trim();
            if candidate_id.is_empty() || status.is_empty() {
                return None;
            }
            Some((candidate_id.to_string(), status.to_string()))
        })
        .collect()
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
            artifact: cols[13].clone(),
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

fn read_sequence_spool_value(
    run_dir: &Path,
    artifact: &str,
    expected_sha256: &str,
) -> Result<String, String> {
    let (relative, location) = artifact
        .split_once('#')
        .ok_or_else(|| format!("input_sequence_spool_reference_invalid:{artifact}"))?;
    if relative != INPUT_SEQUENCE_SPOOL_FILE {
        return Err(format!("input_sequence_spool_path_invalid:{relative}"));
    }
    let mut offset = None;
    let mut length = None;
    for field in location.split('&') {
        let (name, value) = field
            .split_once('=')
            .ok_or_else(|| format!("input_sequence_spool_reference_invalid:{artifact}"))?;
        let parsed = value
            .parse::<u64>()
            .map_err(|_| format!("input_sequence_spool_reference_invalid:{artifact}"))?;
        match name {
            "offset" if offset.replace(parsed).is_none() => {}
            "length" if length.replace(parsed).is_none() => {}
            _ => return Err(format!("input_sequence_spool_reference_invalid:{artifact}")),
        }
    }
    let offset = offset.ok_or_else(|| format!("input_sequence_spool_offset_missing:{artifact}"))?;
    let length = length.ok_or_else(|| format!("input_sequence_spool_length_missing:{artifact}"))?;
    let path = run_dir.join(relative);
    let resolved_root = run_dir.canonicalize().map_err(|err| {
        format!(
            "input_sequence_spool_root_invalid:{}:{err}",
            run_dir.display()
        )
    })?;
    let resolved = path
        .canonicalize()
        .map_err(|err| format!("input_sequence_spool_missing:{}:{err}", path.display()))?;
    if !resolved.starts_with(&resolved_root) {
        return Err(format!(
            "input_sequence_spool_outside_run:{}",
            resolved.display()
        ));
    }
    let mut file = fs::File::open(&resolved).map_err(|err| {
        format!(
            "input_sequence_spool_read_failed:{}:{err}",
            resolved.display()
        )
    })?;
    file.seek(SeekFrom::Start(offset))
        .map_err(|err| format!("input_sequence_spool_seek_failed:{artifact}:{err}"))?;
    let mut value = String::new();
    file.take(length)
        .read_to_string(&mut value)
        .map_err(|err| format!("input_sequence_spool_read_failed:{artifact}:{err}"))?;
    if value.len() as u64 != length {
        return Err(format!("input_sequence_spool_truncated:{artifact}"));
    }
    let observed_sha256 = format!("{:x}", Sha256::digest(value.as_bytes()));
    if observed_sha256 != expected_sha256 {
        return Err(format!("input_sequence_spool_digest_mismatch:{artifact}"));
    }
    Ok(value)
}

fn read_normalized_inputs(path: &Path) -> Result<Vec<NormalizedInput>, String> {
    let run_dir = path
        .parent()
        .ok_or_else(|| "normalized_input_manifest_missing_parent".to_string())?;
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
        match cols[6].as_str() {
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
            "file" | "inline" => {}
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
        let primary_value = if cols[2].is_empty() && !expected.artifact.is_empty() {
            read_sequence_spool_value(run_dir, &expected.artifact, &expected.content_sha256)?
        } else {
            decode_tsv_cell(&cols[2])
        };
        parse_usize(3, "file_count")?;
        out.push(NormalizedInput {
            input_id: cols[0].clone(),
            record_id: cols[5].clone(),
            kind,
            primary_value,
            tokens: cols[4].split_whitespace().map(ToOwned::to_owned).collect(),
            source_path: cols[7].clone(),
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

fn for_each_source_row_batch<F>(
    path: &Path,
    label: &str,
    emit: F,
) -> Result<(usize, &'static str), String>
where
    F: FnMut(&[StoreRow]) -> Result<(), String>,
{
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if is_sqlite_name(&name) {
        return for_each_chembl_sqlite_row_batch(path, emit)
            .map(|count| (count, "chembl_sqlite_rows"));
    }
    if name.ends_with(".tar.gz") && name.contains("chembl") {
        return for_each_chembl_sqlite_archive_row_batch(path, emit)
            .map(|count| (count, "chembl_sqlite_archive_rows"));
    }
    for_each_store_row_batch(path, label, emit)
        .map(|count| (count, "canonical_or_source_table_rows"))
}

fn for_each_store_row_batch<F>(path: &Path, label: &str, mut emit: F) -> Result<usize, String>
where
    F: FnMut(&[StoreRow]) -> Result<(), String>,
{
    let name = lower_file_name(path);
    if name.ends_with(".zip") {
        let mut count = 0usize;
        for_each_supported_zip_member(path, matches_table_name, |_, reader| {
            count += parse_store_rows_from_lines_batched(reader.lines(), label, &mut emit)?;
            Ok(())
        })
        .map_err(|err| format!("{label}_missing:{err}"))?;
        return Ok(count);
    }
    if name.ends_with(".gz") {
        let file = fs::File::open(path)
            .map_err(|err| format!("{label}_missing:{}:{err}", path.display()))?;
        let mut reader = BufReader::new(GzDecoder::new(file));
        return parse_store_rows_from_lines_batched(reader.by_ref().lines(), label, &mut emit)
            .map_err(|err| format!("{label}_missing:{err}"));
    }
    let file =
        fs::File::open(path).map_err(|err| format!("{label}_missing:{}:{err}", path.display()))?;
    let mut reader = BufReader::new(file);
    parse_store_rows_from_lines_batched(reader.by_ref().lines(), label, &mut emit)
        .map_err(|err| format!("{label}_missing:{err}"))
}

fn parse_store_rows_from_lines_batched<I, F>(
    mut lines: I,
    label: &str,
    emit: &mut F,
) -> Result<usize, String>
where
    I: Iterator<Item = Result<String, std::io::Error>>,
    F: FnMut(&[StoreRow]) -> Result<(), String>,
{
    let Some(header_line) = lines.next() else {
        return Ok(0);
    };
    let header_line = header_line.map_err(|err| format!("{label}_read_failed:{err}"))?;
    let headers = split_tsv(header_line.trim_start_matches('\u{feff}'));
    if is_bindingdb_header(&headers) {
        return parse_bindingdb_rows_batched(&headers, lines, label, emit);
    }
    let mut rows = Vec::with_capacity(SOURCE_ROW_BATCH_SIZE);
    let mut count = 0usize;
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
        count += flush_full_store_row_batch(&mut rows, emit)?;
    }
    count += flush_store_row_batch(&mut rows, emit)?;
    Ok(count)
}

fn for_each_chembl_sqlite_archive_row_batch<F>(path: &Path, mut emit: F) -> Result<usize, String>
where
    F: FnMut(&[StoreRow]) -> Result<(), String>,
{
    let file =
        fs::File::open(path).map_err(|err| format!("open_failed:{}:{err}", path.display()))?;
    let decoder = GzDecoder::new(file);
    let mut archive = tar::Archive::new(decoder);
    let entries = archive
        .entries()
        .map_err(|err| format!("tar_entries_failed:{}:{err}", path.display()))?;
    let mut extracted = Vec::new();
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
        let member_sort_name = member_path.to_string_lossy().to_ascii_lowercase();
        if !is_sqlite_name(&member_name) {
            continue;
        }
        let tmp_path = unique_temp_file_path("chembl_sqlite", ".sqlite");
        entry
            .unpack(&tmp_path)
            .map_err(|err| format!("tar_unpack_failed:{}:{err}", path.display()))?;
        extracted.push((member_sort_name, tmp_path));
    }
    if extracted.is_empty() {
        return Err(format!("chembl_sqlite_member_missing:{}", path.display()));
    }
    extracted.sort_by(|left, right| left.0.cmp(&right.0));
    let result = (|| {
        let mut count = 0usize;
        for (_, tmp_path) in &extracted {
            count += for_each_chembl_sqlite_row_batch(tmp_path, &mut emit)?;
        }
        Ok(count)
    })();
    for (_, tmp_path) in extracted {
        let _ = fs::remove_file(tmp_path);
    }
    result
}

#[cfg(test)]
fn parse_chembl_sqlite_archive(path: &Path) -> Result<Vec<StoreRow>, String> {
    let mut rows = Vec::new();
    for_each_chembl_sqlite_archive_row_batch(path, |batch| {
        rows.extend_from_slice(batch);
        Ok(())
    })?;
    Ok(rows)
}

fn for_each_chembl_sqlite_row_batch<F>(path: &Path, mut emit: F) -> Result<usize, String>
where
    F: FnMut(&[StoreRow]) -> Result<(), String>,
{
    let output_path = unique_temp_file_path("chembl_sqlite_rows", ".tsv");
    let result = run_python_script(
        chembl_sqlite_query_script(),
        &[
            path.display().to_string(),
            output_path.display().to_string(),
        ],
        "chembl_sqlite_query",
    );
    if let Err(err) = result {
        let _ = fs::remove_file(&output_path);
        return Err(err);
    }
    let parse_result = (|| {
        let file = fs::File::open(&output_path)
            .map_err(|err| format!("chembl_sqlite_rows_open_failed:{err}"))?;
        let mut rows = Vec::with_capacity(SOURCE_ROW_BATCH_SIZE);
        let mut count = 0usize;
        for line in BufReader::new(file).lines() {
            let line = line.map_err(|err| format!("chembl_sqlite_rows_read_failed:{err}"))?;
            let cols = split_tsv(&line);
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
            count += flush_full_store_row_batch(&mut rows, &mut emit)?;
        }
        count += flush_store_row_batch(&mut rows, &mut emit)?;
        Ok(count)
    })();
    let _ = fs::remove_file(&output_path);
    parse_result
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
with open(sys.argv[2], "w", encoding="utf-8", newline="") as handle:
    writer = csv.writer(handle, delimiter="\t", lineterminator="\n")
    for row in connection.execute(query):
        writer.writerow(["" if value is None else value for value in row])
connection.close()
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

fn parse_bindingdb_rows_batched<I, F>(
    headers: &[String],
    lines: I,
    label: &str,
    emit: &mut F,
) -> Result<usize, String>
where
    I: Iterator<Item = Result<String, std::io::Error>>,
    F: FnMut(&[StoreRow]) -> Result<(), String>,
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

    let mut rows = Vec::with_capacity(SOURCE_ROW_BATCH_SIZE);
    let mut count = 0usize;
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
        count += flush_full_store_row_batch(&mut rows, emit)?;
    }
    count += flush_store_row_batch(&mut rows, emit)?;
    Ok(count)
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
        InputKind::GenomeOrMetagenome => {
            let mut rows = genome_or_metagenome_features(&input.primary_value);
            for row in &mut rows {
                if row.name == "translated_orf_identity" {
                    row.value = row.value.replacen(
                        "record=record_1;",
                        &format!("record={};", input.record_id),
                        1,
                    );
                }
            }
            rows
        }
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
        let eligible_count = input
            .tokens
            .iter()
            .filter(|token| legacy_match_token_is_eligible(token))
            .count();
        (matched.len() as f64 / eligible_count.max(1) as f64).min(1.0)
    }
}

fn matched_tokens(input: &ComputedFeatureInput, row: &StoreRow) -> Vec<String> {
    let mut db_tokens = row.ligand_tokens.clone();
    db_tokens.extend(row.sequence_tokens.iter().cloned());
    db_tokens.extend(row.structure_tokens.iter().cloned());
    input
        .tokens
        .intersection(&db_tokens)
        .filter(|token| legacy_match_token_is_eligible(token))
        .cloned()
        .collect::<Vec<_>>()
}

fn legacy_match_token_is_eligible(token: &str) -> bool {
    !token.starts_with("qs_")
}

fn initial_candidate_scope(scope: &str, organism: &str) -> String {
    match scope {
        "known_database" if is_human_organism(organism) => "known_human_protein".to_string(),
        "known_database" => "known_nonhuman_protein".to_string(),
        "metagenome_potential" => "metagenome_potential_protein".to_string(),
        "structure_interaction" => "structure_interaction".to_string(),
        _ => "computed_candidate".to_string(),
    }
}

fn is_human_organism(organism: &str) -> bool {
    let lower = organism.to_ascii_lowercase();
    if lower.contains("homo sapiens") {
        return true;
    }
    lower
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .any(|token| token == "human")
}

fn candidate_evidence_field(evidence: &str, key: &str) -> Option<String> {
    evidence
        .split(';')
        .filter_map(|part| part.split_once('='))
        .find_map(|(candidate, value)| {
            (candidate.trim().eq_ignore_ascii_case(key) && !value.trim().is_empty())
                .then(|| value.trim().to_string())
        })
}

fn match_namespace(
    source: &str,
    evidence: &str,
    cols: &[String],
    source_database_idx: Option<usize>,
) -> String {
    let source_database = cell(cols, source_database_idx);
    if !source_database.is_empty() {
        return source_database;
    }
    candidate_evidence_field(evidence, "source_namespace")
        .or_else(|| candidate_evidence_field(evidence, "source_database_id"))
        .or_else(|| candidate_evidence_field(evidence, "source_db"))
        .unwrap_or_else(|| source.trim().to_string())
}

fn candidate_metadata_quality(
    target_id: &str,
    target_name: &str,
    organism: &str,
    target_class: &str,
    evidence: &str,
) -> u16 {
    let mut quality = 0u16;
    let name = target_name.trim();
    if !name.is_empty() && !name.eq_ignore_ascii_case(target_id) && name != "unknown" {
        quality += 20;
    }
    let organism = organism.trim();
    if is_human_organism(organism) {
        quality += 30;
    } else if !organism.is_empty() && !organism.eq_ignore_ascii_case("unknown") {
        quality += 10;
    }
    if target_class.eq_ignore_ascii_case("known_human_protein") {
        quality += 20;
    } else if !target_class.trim().is_empty() && !target_class.eq_ignore_ascii_case("unknown") {
        quality += 5;
    }
    let evidence = evidence.to_ascii_lowercase();
    if evidence.contains("onsite_activity_row") || evidence.contains("alignment") {
        quality += 15;
    } else if evidence.contains("activity_row") || evidence.contains("structure") {
        quality += 8;
    }
    quality
}

fn looks_like_uniprot_accession(target_id: &str) -> bool {
    let value = target_id.trim();
    matches!(value.len(), 6 | 10)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
        && value
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_alphabetic())
        && value.bytes().any(|byte| byte.is_ascii_digit())
}

fn canonical_target_namespace(raw_namespace: &str, source: &str, target_id: &str) -> String {
    if looks_like_uniprot_accession(target_id) {
        return "uniprot".to_string();
    }
    let raw = raw_namespace.trim();
    if !raw.is_empty() {
        return raw.to_ascii_lowercase();
    }
    source.trim().to_ascii_lowercase()
}

fn candidate_identity_suffix(
    input_id: &str,
    record_id: &str,
    namespace: &str,
    target_id: &str,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input_id.as_bytes());
    hasher.update([0]);
    hasher.update(record_id.as_bytes());
    hasher.update([0]);
    hasher.update(namespace.as_bytes());
    hasher.update([0]);
    hasher.update(target_id.as_bytes());
    hasher
        .finalize()
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn candidate_id_for(input_id: &str, record_id: &str, namespace: &str, target_id: &str) -> String {
    format!(
        "target_{}_{}",
        safe_id(target_id),
        candidate_identity_suffix(input_id, record_id, namespace, target_id)
    )
}

fn assign_collision_free_candidate_ids(candidates: &mut [Candidate]) {
    for candidate in candidates {
        candidate.candidate_id = candidate_id_for(
            &candidate.input_id,
            &candidate.record_id,
            &candidate.identity_namespace,
            &candidate.target_id,
        );
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
    let mut lines = text.lines();
    let headers = split_tsv(lines.next().unwrap_or_default());
    let source_database_idx = find_header(&headers, &["source_database_id"]);
    let input_idx = find_header(&headers, &["input_id"]);
    let record_idx = find_header(&headers, &["record_id"]);
    for line in lines {
        let cols = split_tsv(line);
        if cols.len() < 9 {
            continue;
        }
        let target_id = cols[2].clone();
        if target_id.trim().is_empty() {
            continue;
        }
        let source = cols[1].clone();
        let evidence = cols[8].clone();
        let raw_namespace = match_namespace(&source, &evidence, &cols, source_database_idx);
        let namespace = canonical_target_namespace(&raw_namespace, &source, &target_id);
        let input_id = cell(&cols, input_idx);
        let record_id = {
            let value = cell(&cols, record_idx);
            if value.trim().is_empty() {
                "record_1".to_string()
            } else {
                value
            }
        };
        let identity_key = format!(
            "{}\u{1f}{}\u{1f}{}\u{1f}{}",
            input_id,
            record_id,
            namespace.trim().to_ascii_lowercase(),
            target_id
        );
        let score = cols[6].parse::<f64>().unwrap_or(0.0);
        let metadata_quality =
            candidate_metadata_quality(&target_id, &cols[3], &cols[4], &cols[5], &evidence);
        let entry = by_target.entry(identity_key).or_insert(Candidate {
            candidate_id: candidate_id_for(&input_id, &record_id, &namespace, &target_id),
            input_id,
            record_id,
            input_kind: String::new(),
            ranking_policy: String::new(),
            identity_namespace: namespace,
            target_id: target_id.clone(),
            target_name: cols[3].clone(),
            target_class: cols[5].clone(),
            candidate_scope: initial_candidate_scope(scope, &cols[4]),
            organism: cols[4].clone(),
            sources: BTreeSet::new(),
            pdb_ids: BTreeSet::new(),
            metadata_quality,
            known_score: 0.0,
            metagenome_score: 0.0,
            structure_score: 0.0,
            docking_score: 0.0,
            dynamics_score: 0.0,
            final_score: 0.0,
            evidence: String::new(),
        });
        if metadata_quality > entry.metadata_quality
            || (metadata_quality == entry.metadata_quality
                && (&cols[3], &cols[4]) < (&entry.target_name, &entry.organism))
        {
            entry.target_name = cols[3].clone();
            entry.target_class = cols[5].clone();
            entry.organism = cols[4].clone();
            entry.candidate_scope = initial_candidate_scope(scope, &cols[4]);
            entry.metadata_quality = metadata_quality;
        }
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
        entry.sources.insert(source.clone());
        entry
            .pdb_ids
            .extend(candidate_pdb_ids(&target_id, &cols[7], &evidence));
        append_candidate_evidence(
            &mut entry.evidence,
            &candidate_evidence_summary(&source, &evidence),
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

fn candidate_pdb_ids(target_id: &str, matched_features: &str, evidence: &str) -> Vec<String> {
    let mut ids = BTreeSet::new();
    if is_pdb_id(&target_id.to_ascii_uppercase()) {
        ids.insert(target_id.to_ascii_uppercase());
    }
    for value in [matched_features, evidence] {
        for part in value.split(';') {
            let Some((key, raw_value)) = part.split_once('=') else {
                continue;
            };
            if matches!(
                key.trim().to_ascii_lowercase().as_str(),
                "pdb_id" | "pdb_ids" | "target_chain_pdb_ids"
            ) {
                ids.extend(pdb_ids_from_text(raw_value));
            }
        }
    }
    ids.into_iter().collect()
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
            input_id: cols.get(14).cloned().unwrap_or_default(),
            record_id: cols.get(15).cloned().unwrap_or_default(),
            input_kind: cols.get(17).cloned().unwrap_or_default(),
            ranking_policy: cols.get(18).cloned().unwrap_or_default(),
            identity_namespace: cols.get(16).cloned().unwrap_or_default(),
            target_id: cols[2].clone(),
            target_name: cols[3].clone(),
            target_class: String::new(),
            candidate_scope: cols[4].clone(),
            organism: cols[5].clone(),
            sources: BTreeSet::new(),
            pdb_ids: BTreeSet::new(),
            metadata_quality: 0,
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
    run_python_script_with_candidates(script, args, context, script_python_command_candidates())
}

fn run_python_script_with_candidates(
    script: &str,
    args: &[String],
    context: &str,
    candidates: Vec<Vec<String>>,
) -> Result<String, String> {
    let mut errors = Vec::new();
    let script_path = temp_python_script_path(context);
    fs::write(&script_path, script)
        .map_err(|err| format!("{context}_python_script_write_failed:{err}"))?;
    for candidate in candidates {
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
    unique_temp_file_path(context, ".py")
}

fn unique_temp_file_path(context: &str, suffix: &str) -> PathBuf {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    let sequence = PYTHON_SCRIPT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    env::temp_dir().join(format!(
        "genoligand_{}_{}_{}_{}{}",
        safe_id(context),
        std::process::id(),
        millis,
        sequence,
        suffix
    ))
}

fn temporary_data_path(root: &Path, label: &str, extension: &str) -> PathBuf {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    let sequence = PYTHON_SCRIPT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    root.join(format!(
        ".{}_{}_{}_{}.{}",
        safe_id(label),
        std::process::id(),
        millis,
        sequence,
        safe_id(extension)
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
