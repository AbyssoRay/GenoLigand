use gls_artifacts::provenance_for_tool;
use gls_geometry::{Point3, build_geometry_hash, estimate_seed_triplets, kabsch_rmsd};
use gls_types::{
    CandidatePatchGraph, GeometryBudgetReport, InteractionConstraintGraph, PatchNode,
    PatchRealizationResult, ToleranceProfile, ToleranceProfileLevel,
};

#[derive(Clone, Debug, PartialEq)]
pub struct PatchRealizerConfig {
    pub min_realization_score: f64,
    pub runtime_ms_per_attempt_budget: f64,
}

impl PatchRealizerConfig {
    pub fn dev() -> Self {
        Self {
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
                    ("profile_dataset", profile.dataset_id.clone()),
                    (
                        "radius_tolerance",
                        format!("{:.6}", profile.radius_tolerance),
                    ),
                    (
                        "angle_tolerance_deg",
                        format!("{:.6}", profile.angle_tolerance_deg),
                    ),
                    ("min_sasa", format!("{:.6}", profile.min_sasa)),
                    ("max_occlusion", format!("{:.6}", profile.max_occlusion)),
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
            ("profile_dataset", fallback_profile.dataset_id.clone()),
            (
                "radius_tolerance",
                format!("{:.6}", fallback_profile.radius_tolerance),
            ),
            (
                "angle_tolerance_deg",
                format!("{:.6}", fallback_profile.angle_tolerance_deg),
            ),
            ("min_sasa", format!("{:.6}", fallback_profile.min_sasa)),
            (
                "max_occlusion",
                format!("{:.6}", fallback_profile.max_occlusion),
            ),
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
    if icg.constraints.is_empty() {
        return None;
    }
    let candidate_sets = icg
        .constraints
        .iter()
        .map(|constraint| {
            seed_nodes
                .iter()
                .copied()
                .filter(|node| node_matches_constraint(node, constraint, profile))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    if candidate_sets.iter().any(Vec::is_empty) {
        return None;
    }

    let mut current = Vec::<&PatchNode>::with_capacity(icg.constraints.len());
    let mut search = HypothesisSearch {
        icg,
        candidate_sets: &candidate_sets,
        profile,
        config,
        evaluated: 0,
        best: None,
    };
    search.enumerate(0, &mut current);
    search.best
}

struct HypothesisSearch<'a, 'b> {
    icg: &'b InteractionConstraintGraph,
    candidate_sets: &'b [Vec<&'a PatchNode>],
    profile: &'b ToleranceProfile,
    config: &'b PatchRealizerConfig,
    evaluated: usize,
    best: Option<MatchResult>,
}

impl<'a> HypothesisSearch<'a, '_> {
    fn enumerate(&mut self, depth: usize, current: &mut Vec<&'a PatchNode>) {
        if self.evaluated >= self.profile.max_seed_triplets {
            return;
        }
        if depth == self.candidate_sets.len() {
            self.evaluated += 1;
            if let Some(match_result) =
                evaluate_hypothesis(self.icg, current, self.profile, self.config)
            {
                let replace = self
                    .best
                    .as_ref()
                    .is_none_or(|existing| match_result.score > existing.score);
                if replace {
                    self.best = Some(match_result);
                }
            }
            return;
        }

        for node in &self.candidate_sets[depth] {
            if current
                .iter()
                .any(|selected| selected.node_id == node.node_id)
            {
                continue;
            }
            current.push(*node);
            self.enumerate(depth + 1, current);
            current.pop();
            if self.evaluated >= self.profile.max_seed_triplets {
                return;
            }
        }
    }
}

fn node_matches_constraint(
    node: &PatchNode,
    constraint: &gls_types::InteractionConstraint,
    profile: &ToleranceProfile,
) -> bool {
    if node.residue != constraint.residue || node.kind != constraint.kind {
        return false;
    }
    let point = Point3::from(node.point);
    let reference = Point3::from(constraint.point);
    let angle = Point3::from(constraint.vector).angle_deg(Point3::from(node.vector));
    point.distance(reference) <= profile.radius_tolerance
        && angle <= profile.angle_tolerance_deg
        && node.sasa >= profile.min_sasa
        && node.occlusion <= profile.max_occlusion
}

fn evaluate_hypothesis(
    icg: &InteractionConstraintGraph,
    nodes: &[&PatchNode],
    profile: &ToleranceProfile,
    config: &PatchRealizerConfig,
) -> Option<MatchResult> {
    let mut reference_points = Vec::new();
    let mut mobile_points = Vec::new();
    let mut angle_penalty = 0.0;
    let mut exposure_score = 0.0;
    let mut occlusion_score_sum = 0.0;

    for (constraint, node) in icg.constraints.iter().zip(nodes) {
        let reference = Point3::from(constraint.point);
        let reference_vector = Point3::from(constraint.vector);
        reference_points.push(reference);
        mobile_points.push(Point3::from(node.point));
        angle_penalty += reference_vector.angle_deg(Point3::from(node.vector))
            / profile.angle_tolerance_deg.max(f64::EPSILON);
        exposure_score +=
            pocket_overlap_score(node, profile) + rotamer_feasibility_score(node, profile);
        occlusion_score_sum += occlusion_score(node, profile);
    }

    let rmsd = kabsch_rmsd(&reference_points, &mobile_points).ok()?;
    let matched_constraints = reference_points.len();
    let normalized_exposure = exposure_score / (matched_constraints.max(1) as f64 * 2.0);
    let normalized_occlusion = occlusion_score_sum / matched_constraints.max(1) as f64;
    let normalized_angle = 1.0 / (1.0 + angle_penalty / matched_constraints.max(1) as f64);
    let score = (1.0 / (1.0 + rmsd)) * 0.45
        + normalized_angle * 0.20
        + normalized_exposure * 0.20
        + normalized_occlusion * 0.15;
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

fn pocket_overlap_score(node: &PatchNode, profile: &ToleranceProfile) -> f64 {
    let depth_component = (node.pocket_depth / (node.pocket_depth + 1.0)).clamp(0.0, 1.0);
    depth_component.max(profile.min_pocket_overlap_score)
}

fn rotamer_feasibility_score(node: &PatchNode, profile: &ToleranceProfile) -> f64 {
    node.confidence
        .clamp(profile.min_rotamer_feasibility_score, 1.0)
}

fn occlusion_score(node: &PatchNode, profile: &ToleranceProfile) -> f64 {
    if node.occlusion > profile.max_occlusion {
        return 0.0;
    }
    (1.0 - node.occlusion / profile.max_occlusion.max(f64::EPSILON)).clamp(0.0, 1.0)
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
                    occlusion: 0.1,
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
            residue_flexibility_bin: "medium".to_string(),
            pocket_depth_bin: "shallow".to_string(),
            coordinate_noise_level: "low".to_string(),
            radius_tolerance: radius,
            angle_tolerance_deg: 20.0,
            distance_bin_width: radius,
            vector_angle_bin_width: 10.0,
            min_sasa: 0.05,
            max_occlusion: 0.65,
            min_pocket_overlap_score: 0.1,
            min_rotamer_feasibility_score: 0.1,
            max_bucket_occupancy: 16,
            max_seed_triplets: 100,
            max_memory_mb: 32768.0,
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
    fn high_occlusion_patch_is_rejected_before_l2() {
        let mut cpg = cpg(0.0);
        for node in &mut cpg.nodes {
            node.occlusion = 0.95;
        }
        let (result, _) = realize_patch(&icg(), &cpg, &[profile(0.5)], &PatchRealizerConfig::dev())
            .expect("realization completes");

        assert!(!result.pass_l2);
        assert_eq!(
            result.failure_reason.as_deref(),
            Some("no_patch_match_under_profile")
        );
    }

    #[test]
    fn occlusion_threshold_is_taken_from_tolerance_profile() {
        let mut cpg = cpg(0.0);
        for node in &mut cpg.nodes {
            node.occlusion = 0.40;
        }
        let mut strict = profile(0.5);
        strict.max_occlusion = 0.20;
        let mut calibrated_relaxed = profile(0.5);
        calibrated_relaxed.max_occlusion = 0.65;

        let (strict_result, _) =
            realize_patch(&icg(), &cpg, &[strict], &PatchRealizerConfig::dev())
                .expect("strict profile completes");
        let (relaxed_result, _) = realize_patch(
            &icg(),
            &cpg,
            &[calibrated_relaxed],
            &PatchRealizerConfig::dev(),
        )
        .expect("relaxed profile completes");

        assert!(!strict_result.pass_l2);
        assert!(relaxed_result.pass_l2);
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
