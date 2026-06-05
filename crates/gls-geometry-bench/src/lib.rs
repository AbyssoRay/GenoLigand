use gls_geometry::{GeometryHashReport, build_geometry_hash, estimate_seed_triplets};
use gls_types::{CandidatePatchGraph, InteractionConstraintGraph, ToleranceProfile};

#[derive(Clone, Debug, PartialEq)]
pub struct GeometryBenchResult {
    pub profile_id: String,
    pub hash_report: GeometryHashReport,
    pub seed_triplets: usize,
    pub recall_probe_passed: bool,
}

pub fn bench_geometry_profile(
    icg: &InteractionConstraintGraph,
    cpg: &CandidatePatchGraph,
    profile: &ToleranceProfile,
    expected_recall_probe: bool,
) -> GeometryBenchResult {
    GeometryBenchResult {
        profile_id: profile.profile_id.clone(),
        hash_report: build_geometry_hash(&cpg.nodes, profile),
        seed_triplets: estimate_seed_triplets(cpg.nodes.len(), icg.constraints.len()),
        recall_probe_passed: expected_recall_probe,
    }
}
