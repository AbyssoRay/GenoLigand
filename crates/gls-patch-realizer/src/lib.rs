use gls_artifacts::provenance_for_tool;
use gls_geometry::{Point3, build_geometry_hash, estimate_seed_triplets, kabsch_rmsd};
use gls_types::{
    CandidatePatchGraph, GeometryBudgetReport, InteractionConstraintGraph, PatchNode,
    PatchRealizationResult, ToleranceProfile, ToleranceProfileLevel,
};

#[derive(Clone, Debug, PartialEq)]
pub struct PatchRealizerConfig {
    pub min_sasa: f64,
    pub min_pocket_overlap_score: f64,
    pub min_rotamer_feasibility_score: f64,
    pub min_realization_score: f64,
    pub runtime_ms_per_attempt_budget: f64,
}

impl PatchRealizerConfig {
    pub fn dev() -> Self {
        Self {
            min_sasa: 0.05,
            min_pocket_overlap_score: 0.1,
            min_rotamer_feasibility_score: 0.1,
            min_realization_score: 0.5,
            runtime_ms_per_attempt_budget: 100.0,
        }
    }
}

pub fn realize_patch(
    icg: &InteractionConstraintGraph,
    cpg: &CandidatePatchGraph,
    profiles: &[ToleranceProfile],
    config: &PatchRealizerConfig,
) -> Result<(PatchRealizationResult, GeometryBudgetReport), String> {
    if profiles.is_empty() {
        return Err("patch_realizer_requires_tolerance_profile".to_string());
    }
    let mut ordered = profiles.to_vec();
    ordered.sort_by_key(|profile| match profile.profile_level {
        ToleranceProfileLevel::Narrow => 0,
        ToleranceProfileLevel::Standard => 1,
        ToleranceProfileLevel::Relaxed => 2,
    });
    for profile in &ordered {
        profile.validate_for_runtime()?;
    }

    let mut audit = Vec::new();
    let mut relaxed_profiles_tried = Vec::new();
    let mut last_budget = None;
    let mut last_reason = "no_patch_match".to_string();

    for profile in &ordered {
        relaxed_profiles_tried.push(profile.profile_level.as_str().to_string());
        audit.push(format!(
            "try_profile:{}:{}",
            profile.profile_level.as_str(),
            profile.profile_id
        ));

        let hash_report = build_geometry_hash(&cpg.nodes, profile);
        let seed_nodes = seed_nodes(icg, cpg, profile);
        let seed_triplets = estimate_seed_triplets(seed_nodes.len(), icg.constraints.len());
        let mut budget = GeometryBudgetReport {
            candidate_id: cpg.candidate_id.clone(),
            profile_id: profile.profile_id.clone(),
            max_bucket_occupancy_observed: hash_report.max_bucket_occupancy_observed,
            seed_triplets_evaluated: seed_triplets,
            memory_estimate_mb: hash_report.memory_estimate_mb,
            runtime_ms: config
                .runtime_ms_per_attempt_budget
                .min(1.0 + seed_triplets as f64),
            relaxed_profiles_tried: relaxed_profiles_tried.clone(),
            guard_triggered: hash_report.guard_triggered,
            failure_reason: hash_report.failure_reason.clone(),
        };

        if hash_report.guard_triggered {
            last_reason = hash_report
                .failure_reason
                .unwrap_or_else(|| "geometry_hash_guard_triggered".to_string());
            last_budget = Some(budget);
            break;
        }
        if seed_triplets > profile.max_seed_triplets {
            budget.guard_triggered = true;
            budget.failure_reason = Some("max_seed_triplets_guard_triggered".to_string());
            last_reason = "max_seed_triplets_guard_triggered".to_string();
            last_budget = Some(budget);
            break;
        }

        if let Some(match_result) = bounded_ransac_kabsch(icg, &seed_nodes, profile, config) {
            let provenance = provenance_for_tool(
                "gls-patch-realizer",
                "v1.6",
                &[
                    ("icg_hash", icg.hash.clone()),
                    ("profile_id", profile.profile_id.clone()),
                    ("cpg_candidate", cpg.candidate_id.to_string()),
                ],
            );
            let result = PatchRealizationResult {
                candidate_id: cpg.candidate_id.clone(),
                profile_id: profile.profile_id.clone(),
                pass_l2: true,
                matched_constraints: match_result.matched_constraints,
                rmsd: match_result.rmsd,
                score: match_result.score,
                failure_reason: None,
                audit,
                provenance,
            };
            budget.failure_reason = None;
            return Ok((result, budget));
        }

        last_reason = "no_patch_match_under_profile".to_string();
        budget.failure_reason = Some(last_reason.clone());
        last_budget = Some(budget);
    }

    let fallback_profile = ordered
        .last()
        .expect("profiles not empty after earlier check");
    let provenance = provenance_for_tool(
        "gls-patch-realizer",
        "v1.6",
        &[
            ("icg_hash", icg.hash.clone()),
            ("profile_id", fallback_profile.profile_id.clone()),
            ("cpg_candidate", cpg.candidate_id.to_string()),
        ],
    );
    let result = PatchRealizationResult {
        candidate_id: cpg.candidate_id.clone(),
        profile_id: fallback_profile.profile_id.clone(),
        pass_l2: false,
        matched_constraints: 0,
        rmsd: f64::INFINITY,
        score: 0.0,
        failure_reason: Some(last_reason.clone()),
        audit,
        provenance,
    };
    let budget = last_budget.unwrap_or_else(|| GeometryBudgetReport {
        candidate_id: cpg.candidate_id.clone(),
        profile_id: fallback_profile.profile_id.clone(),
        max_bucket_occupancy_observed: 0,
        seed_triplets_evaluated: 0,
        memory_estimate_mb: 0.0,
        runtime_ms: 0.0,
        relaxed_profiles_tried,
        guard_triggered: false,
        failure_reason: Some(last_reason),
    });
    Ok((result, budget))
}

#[derive(Clone, Debug)]
struct MatchResult {
    matched_constraints: usize,
    rmsd: f64,
    score: f64,
}

fn seed_nodes<'a>(
    icg: &InteractionConstraintGraph,
    cpg: &'a CandidatePatchGraph,
    profile: &ToleranceProfile,
) -> Vec<&'a PatchNode> {
    cpg.nodes
        .iter()
        .filter(|node| {
            node.kind == profile.interaction_kind
                && icg.constraints.iter().any(|constraint| {
                    constraint.kind == node.kind && constraint.residue == node.residue
                })
        })
        .collect()
}

fn bounded_ransac_kabsch(
    icg: &InteractionConstraintGraph,
    seed_nodes: &[&PatchNode],
    profile: &ToleranceProfile,
    config: &PatchRealizerConfig,
) -> Option<MatchResult> {
    let mut reference_points = Vec::new();
    let mut mobile_points = Vec::new();
    let mut angle_penalty = 0.0;
    let mut exposure_score = 0.0;

    for constraint in &icg.constraints {
        let reference = Point3::from(constraint.point);
        let reference_vector = Point3::from(constraint.vector);
        let best = seed_nodes
            .iter()
            .filter(|node| node.residue == constraint.residue && node.kind == constraint.kind)
            .filter_map(|node| {
                let point = Point3::from(node.point);
                let distance = reference.distance(point);
                let angle = reference_vector.angle_deg(Point3::from(node.vector));
                let sasa_ok = node.sasa >= config.min_sasa;
                if distance <= profile.radius_tolerance
                    && angle <= profile.angle_tolerance_deg
                    && sasa_ok
                {
                    Some((
                        *node,
                        distance + angle / profile.angle_tolerance_deg.max(f64::EPSILON),
                    ))
                } else {
                    None
                }
            })
            .min_by(|a, b| a.1.total_cmp(&b.1));
        let (node, _) = best?;
        reference_points.push(reference);
        mobile_points.push(Point3::from(node.point));
        angle_penalty += reference_vector.angle_deg(Point3::from(node.vector))
            / profile.angle_tolerance_deg.max(f64::EPSILON);
        exposure_score +=
            pocket_overlap_score(node, config) + rotamer_feasibility_score(node, config);
    }

    let rmsd = kabsch_rmsd(&reference_points, &mobile_points).ok()?;
    let matched_constraints = reference_points.len();
    let normalized_exposure = exposure_score / (matched_constraints.max(1) as f64 * 2.0);
    let normalized_angle = 1.0 / (1.0 + angle_penalty / matched_constraints.max(1) as f64);
    let score = (1.0 / (1.0 + rmsd)) * 0.5 + normalized_angle * 0.25 + normalized_exposure * 0.25;
    if rmsd <= profile.radius_tolerance && score >= config.min_realization_score {
        Some(MatchResult {
            matched_constraints,
            rmsd,
            score,
        })
    } else {
        None
    }
}

fn pocket_overlap_score(node: &PatchNode, config: &PatchRealizerConfig) -> f64 {
    let depth_component = (node.pocket_depth / (node.pocket_depth + 1.0)).clamp(0.0, 1.0);
    depth_component.max(config.min_pocket_overlap_score)
}

fn rotamer_feasibility_score(node: &PatchNode, config: &PatchRealizerConfig) -> f64 {
    node.confidence
        .clamp(config.min_rotamer_feasibility_score, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gls_types::{
        CandidateId, InteractionConstraint, InteractionKind, Point3Data, Provenance,
        ToleranceProfile,
    };

    fn icg() -> InteractionConstraintGraph {
        let constraints = vec![
            InteractionConstraint {
                id: "c1".to_string(),
                kind: InteractionKind::HydrogenBond,
                residue: 'D',
                point: Point3Data::new(0.0, 0.0, 0.0),
                vector: Point3Data::new(1.0, 0.0, 0.0),
                required: true,
            },
            InteractionConstraint {
                id: "c2".to_string(),
                kind: InteractionKind::HydrogenBond,
                residue: 'H',
                point: Point3Data::new(1.0, 0.0, 0.0),
                vector: Point3Data::new(1.0, 0.0, 0.0),
                required: true,
            },
            InteractionConstraint {
                id: "c3".to_string(),
                kind: InteractionKind::HydrogenBond,
                residue: 'Q',
                point: Point3Data::new(0.0, 1.0, 0.0),
                vector: Point3Data::new(1.0, 0.0, 0.0),
                required: true,
            },
        ];
        InteractionConstraintGraph {
            graph_id: "g".to_string(),
            hash: "h".to_string(),
            constraints,
            provenance: Provenance::new("test", "1", "icg"),
        }
    }

    fn cpg(offset: f64) -> CandidatePatchGraph {
        let residues = [('D', 0.0, 0.0), ('H', 1.0, 0.0), ('Q', 0.0, 1.0)];
        CandidatePatchGraph {
            candidate_id: CandidateId::from("cand"),
            nodes: residues
                .iter()
                .enumerate()
                .map(|(idx, (residue, x, y))| PatchNode {
                    node_id: format!("n{idx}"),
                    residue: *residue,
                    kind: InteractionKind::HydrogenBond,
                    point: Point3Data::new(x + offset, *y, 0.0),
                    vector: Point3Data::new(1.0, 0.0, 0.0),
                    sasa: 0.5,
                    pocket_depth: 2.0,
                    confidence: 0.9,
                })
                .collect(),
            provenance: Provenance::new("test", "1", "cpg"),
            confidence_bin: "high".to_string(),
            ligand_class: "generic".to_string(),
        }
    }

    fn profile(radius: f64) -> ToleranceProfile {
        ToleranceProfile {
            profile_id: format!("p{radius}"),
            dataset_id: "d".to_string(),
            created_at: "t".to_string(),
            interaction_kind: InteractionKind::HydrogenBond,
            ligand_class: "generic".to_string(),
            structure_confidence_bin: "high".to_string(),
            radius_tolerance: radius,
            angle_tolerance_deg: 20.0,
            distance_bin_width: radius,
            vector_angle_bin_width: 10.0,
            max_bucket_occupancy: 16,
            max_seed_triplets: 100,
            max_memory_mb: 100.0,
            recall_at_profile: 1.0,
            hard_negative_rejection: 1.0,
            hard_negative_false_positive_rate: 0.0,
            bucket_occupancy_p95: 2.0,
            peak_memory_mb: 1.0,
            runtime_ms_per_fixture: 1.0,
            generated_by_fixture_calibration: true,
            profile_level: ToleranceProfileLevel::Standard,
        }
    }

    #[test]
    fn missing_tolerance_profile_is_rejected() {
        let err = realize_patch(&icg(), &cpg(0.0), &[], &PatchRealizerConfig::dev())
            .expect_err("profile required");

        assert_eq!(err, "patch_realizer_requires_tolerance_profile");
    }

    #[test]
    fn uncalibrated_profile_is_rejected() {
        let mut profile = profile(1.0);
        profile.generated_by_fixture_calibration = false;
        let err = realize_patch(&icg(), &cpg(0.0), &[profile], &PatchRealizerConfig::dev())
            .expect_err("calibration required");

        assert_eq!(err, "tolerance_profile_not_fixture_calibrated");
    }

    #[test]
    fn positive_fixture_like_patch_passes_l2() {
        let (result, budget) = realize_patch(
            &icg(),
            &cpg(0.1),
            &[profile(0.5)],
            &PatchRealizerConfig::dev(),
        )
        .expect("realization succeeds");

        assert!(result.pass_l2);
        assert_eq!(result.matched_constraints, 3);
        assert!(!budget.guard_triggered);
    }

    #[test]
    fn hard_negative_same_residue_geometry_is_rejected() {
        let (result, _) = realize_patch(
            &icg(),
            &cpg(10.0),
            &[profile(0.5)],
            &PatchRealizerConfig::dev(),
        )
        .expect("realization completes");

        assert!(!result.pass_l2);
        assert_eq!(
            result.failure_reason.as_deref(),
            Some("no_patch_match_under_profile")
        );
    }

    #[test]
    fn coarse_to_fine_relaxation_is_audited() {
        let mut narrow = profile(0.1);
        narrow.profile_level = ToleranceProfileLevel::Narrow;
        let mut relaxed = profile(0.5);
        relaxed.profile_level = ToleranceProfileLevel::Relaxed;
        let (result, budget) = realize_patch(
            &icg(),
            &cpg(0.2),
            &[relaxed, narrow],
            &PatchRealizerConfig::dev(),
        )
        .expect("realization succeeds");

        assert!(result.pass_l2);
        assert_eq!(
            budget.relaxed_profiles_tried,
            vec!["narrow".to_string(), "relaxed".to_string()]
        );
        assert!(
            result
                .audit
                .iter()
                .any(|entry| entry.starts_with("try_profile:narrow"))
        );
    }
}
