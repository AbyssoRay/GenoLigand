use std::collections::BTreeMap;
use std::fmt::{self, Display};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CandidateId(String);

impl CandidateId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for CandidateId {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for CandidateId {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl Display for CandidateId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Provenance {
    pub source: String,
    pub version: String,
    pub parameters_hash: String,
    pub artifact_hash: String,
}

impl Provenance {
    pub fn new(source: impl Into<String>, version: impl Into<String>, parameters: &str) -> Self {
        let source = source.into();
        let version = version.into();
        let parameters_hash = stable_hash(parameters);
        let artifact_hash = stable_hash(&format!("{source}:{version}:{parameters_hash}"));
        Self {
            source,
            version,
            parameters_hash,
            artifact_hash,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum LocusClass {
    AnnotatedProtein,
    DarkGenome,
    Pseudogene,
    NonCoding,
    #[default]
    Unknown,
}

impl LocusClass {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::AnnotatedProtein => "annotated_protein",
            Self::DarkGenome => "dark_genome",
            Self::Pseudogene => "pseudogene",
            Self::NonCoding => "non_coding",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InteractionKind {
    HydrogenBond,
    Hydrophobic,
    AromaticStacking,
    SaltBridge,
    MetalCoordination,
    Generic,
}

impl InteractionKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::HydrogenBond => "hydrogen_bond",
            Self::Hydrophobic => "hydrophobic",
            Self::AromaticStacking => "aromatic_stacking",
            Self::SaltBridge => "salt_bridge",
            Self::MetalCoordination => "metal_coordination",
            Self::Generic => "generic",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShortCircuitDecision {
    Reject,
    HoldNoPlm,
    PassPlm,
    ForceKeepDarkCandidate,
}

impl ShortCircuitDecision {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Reject => "reject",
            Self::HoldNoPlm => "hold_no_plm",
            Self::PassPlm => "pass_plm",
            Self::ForceKeepDarkCandidate => "force_keep_dark_candidate",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CandidateGrade {
    G0,
    G1,
    G2,
    G3,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OrfCandidate {
    pub candidate_id: CandidateId,
    pub shard_id: String,
    pub sequence: String,
    pub nucleotide_range: (usize, usize),
    pub frame: i8,
    pub locus_class: LocusClass,
    pub stop_to_stop: bool,
    pub fragment_family: String,
    pub provenance: Provenance,
    pub grade: CandidateGrade,
}

impl OrfCandidate {
    pub fn sequence_hash(&self) -> String {
        stable_hash(&self.sequence)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct FragmentHit {
    pub candidate_id: CandidateId,
    pub fragment: String,
    pub family: String,
    pub start: usize,
    pub end: usize,
    pub information_content: f64,
    pub p_value: f64,
    pub q_value: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point3Data {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Point3Data {
    pub fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct InteractionConstraint {
    pub id: String,
    pub kind: InteractionKind,
    pub residue: char,
    pub point: Point3Data,
    pub vector: Point3Data,
    pub required: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct InteractionConstraintGraph {
    pub graph_id: String,
    pub constraints: Vec<InteractionConstraint>,
    pub provenance: Provenance,
    pub hash: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CategoryFragment {
    pub family: String,
    pub residues: Vec<char>,
    pub min_len: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FragmentGrammar {
    pub grammar_id: String,
    pub exact_fragments: Vec<String>,
    pub category_fragments: Vec<CategoryFragment>,
    pub provenance: Provenance,
    pub hash: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PatchNode {
    pub node_id: String,
    pub residue: char,
    pub kind: InteractionKind,
    pub point: Point3Data,
    pub vector: Point3Data,
    pub sasa: f64,
    pub occlusion: f64,
    pub pocket_depth: f64,
    pub confidence: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CandidatePatchGraph {
    pub candidate_id: CandidateId,
    pub nodes: Vec<PatchNode>,
    pub provenance: Provenance,
    pub confidence_bin: String,
    pub ligand_class: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToleranceProfileLevel {
    Narrow,
    Standard,
    Relaxed,
}

impl ToleranceProfileLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Narrow => "narrow",
            Self::Standard => "standard",
            Self::Relaxed => "relaxed",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ToleranceProfile {
    pub profile_id: String,
    pub dataset_id: String,
    pub created_at: String,
    pub interaction_kind: InteractionKind,
    pub ligand_class: String,
    pub structure_confidence_bin: String,
    pub radius_tolerance: f64,
    pub angle_tolerance_deg: f64,
    pub distance_bin_width: f64,
    pub vector_angle_bin_width: f64,
    pub min_sasa: f64,
    pub max_occlusion: f64,
    pub min_pocket_overlap_score: f64,
    pub min_rotamer_feasibility_score: f64,
    pub max_bucket_occupancy: usize,
    pub max_seed_triplets: usize,
    pub max_memory_mb: f64,
    pub recall_at_profile: f64,
    pub hard_negative_rejection: f64,
    pub hard_negative_false_positive_rate: f64,
    pub bucket_occupancy_p95: f64,
    pub peak_memory_mb: f64,
    pub runtime_ms_per_fixture: f64,
    pub generated_by_fixture_calibration: bool,
    pub profile_level: ToleranceProfileLevel,
}

impl ToleranceProfile {
    pub fn validate_for_runtime(&self) -> Result<(), String> {
        if !self.generated_by_fixture_calibration {
            return Err("tolerance_profile_not_fixture_calibrated".to_string());
        }
        if self.radius_tolerance <= 0.0
            || self.angle_tolerance_deg <= 0.0
            || self.distance_bin_width <= 0.0
            || self.vector_angle_bin_width <= 0.0
        {
            return Err("tolerance_profile_has_non_positive_geometry_budget".to_string());
        }
        if !(0.0..=1.0).contains(&self.min_sasa)
            || !(0.0..=1.0).contains(&self.max_occlusion)
            || !(0.0..=1.0).contains(&self.min_pocket_overlap_score)
            || !(0.0..=1.0).contains(&self.min_rotamer_feasibility_score)
        {
            return Err("tolerance_profile_has_invalid_surface_or_rotamer_budget".to_string());
        }
        if self.max_bucket_occupancy == 0 || self.max_seed_triplets == 0 {
            return Err("tolerance_profile_has_zero_guard".to_string());
        }
        if self.max_memory_mb <= 0.0 {
            return Err("tolerance_profile_has_zero_memory_guard".to_string());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ShortCircuitGateResult {
    pub candidate_id: CandidateId,
    pub decision: ShortCircuitDecision,
    pub rule_version: String,
    pub sequence_hash: String,
    pub canonical_cluster_id: Option<String>,
    pub aa_len: usize,
    pub low_complexity_score: f64,
    pub entropy_score: f64,
    pub fragment_info_score: f64,
    pub cooccur_q_value: Option<f64>,
    pub locus_class: LocusClass,
    pub context_priority_score: f64,
    pub budget_rank: u32,
    pub pass_plm: bool,
    pub failure_reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FoldabilityGateResult {
    pub candidate_id: CandidateId,
    pub sequence_hash: String,
    pub model_id: String,
    pub tokenizer_id: String,
    pub weights_hash: String,
    pub cache_key: String,
    pub embedding_from_cache: bool,
    pub pass_structure: bool,
    pub failure_reason: Option<String>,
    pub bucket_len: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StructureGateResult {
    pub candidate_id: CandidateId,
    pub accepted_for_l15: bool,
    pub orf_extension: Option<OrfExtensionResult>,
    pub candidate_patch_graph: Option<CandidatePatchGraph>,
    pub failure_reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OrfExtensionResult {
    pub candidate_id: CandidateId,
    pub original_nucleotide_range: (usize, usize),
    pub extended_nucleotide_range: (usize, usize),
    pub original_aa_len: usize,
    pub extended_aa_len: usize,
    pub upstream_aa_added: usize,
    pub downstream_aa_added: usize,
    pub sequence_hash: String,
    pub provenance: Provenance,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PatchRealizationResult {
    pub candidate_id: CandidateId,
    pub profile_id: String,
    pub pass_l2: bool,
    pub matched_constraints: usize,
    pub rmsd: f64,
    pub score: f64,
    pub failure_reason: Option<String>,
    pub audit: Vec<String>,
    pub provenance: Provenance,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GeometryBudgetReport {
    pub candidate_id: CandidateId,
    pub profile_id: String,
    pub max_bucket_occupancy_observed: usize,
    pub seed_triplets_evaluated: usize,
    pub memory_estimate_mb: f64,
    pub runtime_ms: f64,
    pub relaxed_profiles_tried: Vec<String>,
    pub guard_triggered: bool,
    pub failure_reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DockingResult {
    pub candidate_id: CandidateId,
    pub pass_l2_input: bool,
    pub score: Option<f64>,
    pub worker_reused: bool,
    pub failure_reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct IdpTrendResult {
    pub candidate_id: CandidateId,
    pub ensemble_hit_rate: f64,
    pub cluster_enrichment: f64,
    pub contact_consistency: f64,
    pub trend_label: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct StageResourceUsage {
    pub stage: String,
    pub candidates_in: usize,
    pub candidates_out: usize,
    pub failures: BTreeMap<String, usize>,
    pub cpu_ms: f64,
    pub peak_memory_mb: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct EndToEndReport {
    pub stages: Vec<StageResourceUsage>,
    pub audit_records: Vec<CandidateAuditRecord>,
    pub notes: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CandidateAuditRecord {
    pub candidate_id: CandidateId,
    pub stage: String,
    pub tool_name: String,
    pub tool_version: String,
    pub parameters_hash: String,
    pub artifact_hash: String,
}

impl CandidateAuditRecord {
    pub fn from_provenance(
        candidate_id: CandidateId,
        stage: impl Into<String>,
        provenance: &Provenance,
    ) -> Self {
        Self {
            candidate_id,
            stage: stage.into(),
            tool_name: provenance.source.clone(),
            tool_version: provenance.version.clone(),
            parameters_hash: provenance.parameters_hash.clone(),
            artifact_hash: provenance.artifact_hash.clone(),
        }
    }
}

pub fn stable_hash(input: &str) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in input.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

pub fn machine_reason(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>()
        .split('_')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("_")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_are_stable_hex() {
        assert_eq!(stable_hash("GenoLigand"), stable_hash("GenoLigand"));
        assert_eq!(stable_hash("GenoLigand").len(), 16);
    }

    #[test]
    fn uncalibrated_profile_is_rejected_for_runtime() {
        let profile = ToleranceProfile {
            profile_id: "manual".to_string(),
            dataset_id: "none".to_string(),
            created_at: "2026-06-06T00:00:00Z".to_string(),
            interaction_kind: InteractionKind::HydrogenBond,
            ligand_class: "generic".to_string(),
            structure_confidence_bin: "high".to_string(),
            radius_tolerance: 1.0,
            angle_tolerance_deg: 20.0,
            distance_bin_width: 0.5,
            vector_angle_bin_width: 10.0,
            min_sasa: 0.05,
            max_occlusion: 0.65,
            min_pocket_overlap_score: 0.1,
            min_rotamer_feasibility_score: 0.1,
            max_bucket_occupancy: 8,
            max_seed_triplets: 64,
            max_memory_mb: 64.0,
            recall_at_profile: 0.9,
            hard_negative_rejection: 0.95,
            hard_negative_false_positive_rate: 0.05,
            bucket_occupancy_p95: 3.0,
            peak_memory_mb: 8.0,
            runtime_ms_per_fixture: 1.0,
            generated_by_fixture_calibration: false,
            profile_level: ToleranceProfileLevel::Standard,
        };

        assert_eq!(
            profile.validate_for_runtime(),
            Err("tolerance_profile_not_fixture_calibrated".to_string())
        );
    }
}
