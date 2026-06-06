use gls_geometry::{GeometryHashReport, build_geometry_hash, estimate_seed_triplets};
use gls_types::{
    CandidatePatchGraph, InteractionConstraintGraph, ToleranceProfile, ToleranceProfileLevel,
};

#[derive(Clone, Debug, PartialEq)]
pub struct GeometryBenchResult {
    pub profile_id: String,
    pub profile_level: ToleranceProfileLevel,
    pub hash_report: GeometryHashReport,
    pub seed_triplets: usize,
    pub oom_risk_score: f64,
    pub recall_probe_passed: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GeometryRecallCurve {
    pub points: Vec<GeometryBenchResult>,
}

pub fn bench_geometry_profile(
    icg: &InteractionConstraintGraph,
    cpg: &CandidatePatchGraph,
    profile: &ToleranceProfile,
    expected_recall_probe: bool,
) -> GeometryBenchResult {
    let hash_report = build_geometry_hash(&cpg.nodes, profile);
    let seed_triplets = estimate_seed_triplets(cpg.nodes.len(), icg.constraints.len());
    let oom_risk_score = geometry_oom_risk_score(profile, &hash_report, seed_triplets);
    GeometryBenchResult {
        profile_id: profile.profile_id.clone(),
        profile_level: profile.profile_level.clone(),
        hash_report,
        seed_triplets,
        oom_risk_score,
        recall_probe_passed: expected_recall_probe,
    }
}

pub fn bench_geometry_recall_curve(
    icg: &InteractionConstraintGraph,
    cpg: &CandidatePatchGraph,
    profiles: &[ToleranceProfile],
    recall_probes: &[bool],
) -> Result<GeometryRecallCurve, String> {
    if profiles.len() != recall_probes.len() {
        return Err("geometry_bench_profile_probe_count_mismatch".to_string());
    }
    let mut points = profiles
        .iter()
        .zip(recall_probes)
        .map(|(profile, recall_probe)| bench_geometry_profile(icg, cpg, profile, *recall_probe))
        .collect::<Vec<_>>();
    points.sort_by_key(|point| match point.profile_level {
        ToleranceProfileLevel::Narrow => 0,
        ToleranceProfileLevel::Standard => 1,
        ToleranceProfileLevel::Relaxed => 2,
    });
    Ok(GeometryRecallCurve { points })
}

fn geometry_oom_risk_score(
    profile: &ToleranceProfile,
    report: &GeometryHashReport,
    seed_triplets: usize,
) -> f64 {
    let bucket_risk =
        report.max_bucket_occupancy_observed as f64 / profile.max_bucket_occupancy.max(1) as f64;
    let memory_risk = report.memory_estimate_mb / profile.max_memory_mb.max(f64::EPSILON);
    let seed_risk = seed_triplets as f64 / profile.max_seed_triplets.max(1) as f64;
    bucket_risk.max(memory_risk).max(seed_risk).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gls_types::{
        CandidateId, CandidatePatchGraph, InteractionConstraint, InteractionKind, Point3Data,
        Provenance, ToleranceProfile,
    };

    #[test]
    fn recall_curve_reports_memory_seed_and_profile_order() {
        let icg = InteractionConstraintGraph {
            graph_id: "icg".to_string(),
            constraints: vec![
                constraint("c1", 'D', 0.0),
                constraint("c2", 'H', 1.0),
                constraint("c3", 'Q', 2.0),
            ],
            provenance: Provenance::new("test", "1", "icg"),
            hash: "icg_hash".to_string(),
        };
        let cpg = CandidatePatchGraph {
            candidate_id: CandidateId::from("cand"),
            nodes: vec![
                node("n1", 'D', 0.0),
                node("n2", 'H', 1.0),
                node("n3", 'Q', 2.0),
            ],
            provenance: Provenance::new("test", "1", "cpg"),
            confidence_bin: "high".to_string(),
            ligand_class: "generic".to_string(),
        };
        let profiles = vec![
            profile("relaxed", ToleranceProfileLevel::Relaxed, 2.0),
            profile("narrow", ToleranceProfileLevel::Narrow, 0.5),
            profile("standard", ToleranceProfileLevel::Standard, 1.0),
        ];

        let curve = bench_geometry_recall_curve(&icg, &cpg, &profiles, &[true, false, true])
            .expect("curve");

        assert_eq!(curve.points[0].profile_level, ToleranceProfileLevel::Narrow);
        assert_eq!(
            curve.points[2].profile_level,
            ToleranceProfileLevel::Relaxed
        );
        assert!(curve.points.iter().all(|point| point.seed_triplets > 0));
        assert!(curve.points.iter().all(|point| point.oom_risk_score > 0.0));
        assert!(
            curve
                .points
                .iter()
                .all(|point| point.hash_report.memory_estimate_mb > 0.0)
        );
    }

    fn constraint(id: &str, residue: char, x: f64) -> InteractionConstraint {
        InteractionConstraint {
            id: id.to_string(),
            kind: InteractionKind::HydrogenBond,
            residue,
            point: Point3Data::new(x, 0.0, 0.0),
            vector: Point3Data::new(1.0, 0.0, 0.0),
            required: true,
        }
    }

    fn node(id: &str, residue: char, x: f64) -> gls_types::PatchNode {
        gls_types::PatchNode {
            node_id: id.to_string(),
            residue,
            kind: InteractionKind::HydrogenBond,
            point: Point3Data::new(x, 0.0, 0.0),
            vector: Point3Data::new(1.0, 0.0, 0.0),
            sasa: 0.4,
            occlusion: 0.2,
            pocket_depth: 1.0,
            confidence: 0.9,
        }
    }

    fn profile(id: &str, level: ToleranceProfileLevel, radius: f64) -> ToleranceProfile {
        ToleranceProfile {
            profile_id: id.to_string(),
            dataset_id: "fixtures".to_string(),
            created_at: "2026-06-06T00:00:00Z".to_string(),
            interaction_kind: InteractionKind::HydrogenBond,
            ligand_class: "generic".to_string(),
            structure_confidence_bin: "high".to_string(),
            residue_flexibility_bin: "medium".to_string(),
            pocket_depth_bin: "shallow".to_string(),
            coordinate_noise_level: "low".to_string(),
            radius_tolerance: radius,
            angle_tolerance_deg: 10.0 * radius,
            distance_bin_width: radius.max(0.1),
            vector_angle_bin_width: 5.0,
            min_sasa: 0.1,
            max_occlusion: 0.7,
            min_pocket_overlap_score: 0.1,
            min_rotamer_feasibility_score: 0.1,
            max_bucket_occupancy: 8,
            max_seed_triplets: 64,
            max_memory_mb: 64.0,
            recall_at_profile: 1.0,
            hard_negative_rejection: 1.0,
            hard_negative_false_positive_rate: 0.0,
            bucket_occupancy_p95: 2.0,
            peak_memory_mb: 1.0,
            runtime_ms_per_fixture: 1.0,
            generated_by_fixture_calibration: true,
            profile_level: level,
        }
    }
}
