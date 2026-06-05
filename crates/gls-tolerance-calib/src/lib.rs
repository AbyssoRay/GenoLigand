use gls_fixtures::{FixtureManifest, FixtureType, PatchFixture};
use gls_types::{InteractionKind, ToleranceProfile, ToleranceProfileLevel, stable_hash};

#[derive(Clone, Debug, PartialEq)]
pub struct CalibrationConfig {
    pub created_at: String,
    pub target_positive_recall: f64,
    pub max_hard_negative_false_positive_rate: f64,
    pub max_bucket_occupancy_p95: f64,
    pub max_peak_memory_mb: f64,
    pub max_runtime_ms_per_fixture: f64,
    pub max_seed_triplets: usize,
    pub max_bucket_occupancy: usize,
    pub level_scales: [(ToleranceProfileLevel, f64); 3],
}

impl CalibrationConfig {
    pub fn dev() -> Self {
        Self {
            created_at: "2026-06-06T00:00:00Z".to_string(),
            target_positive_recall: 0.95,
            max_hard_negative_false_positive_rate: 0.05,
            max_bucket_occupancy_p95: 64.0,
            max_peak_memory_mb: 256.0,
            max_runtime_ms_per_fixture: 1000.0,
            max_seed_triplets: 4096,
            max_bucket_occupancy: 128,
            level_scales: [
                (ToleranceProfileLevel::Narrow, 1.0),
                (ToleranceProfileLevel::Standard, 1.5),
                (ToleranceProfileLevel::Relaxed, 2.0),
            ],
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CalibrationReport {
    pub dataset_id: String,
    pub profiles: Vec<ToleranceProfile>,
    pub positive_recall: f64,
    pub hard_negative_rejection: f64,
    pub bucket_occupancy_p95: f64,
    pub peak_memory_mb: f64,
    pub runtime_ms_per_fixture: f64,
}

pub fn calibrate_tolerances(
    manifest: &FixtureManifest,
    config: &CalibrationConfig,
) -> Result<CalibrationReport, String> {
    manifest.validate()?;
    let positives = manifest
        .fixtures
        .iter()
        .filter(|fixture| fixture.fixture_type.is_positive())
        .collect::<Vec<_>>();
    let hard_negatives = manifest
        .fixtures
        .iter()
        .filter(|fixture| fixture.fixture_type == FixtureType::HardNegativeSameResidues)
        .collect::<Vec<_>>();
    if positives.is_empty() {
        return Err("calibration_requires_positive_fixtures".to_string());
    }
    if hard_negatives.is_empty() {
        return Err("calibration_requires_hard_negative_fixtures".to_string());
    }

    let base_radius = percentile(
        &positives
            .iter()
            .map(|fixture| fixture.observed_radius_error)
            .collect::<Vec<_>>(),
        config.target_positive_recall,
    );
    let base_angle = percentile(
        &positives
            .iter()
            .map(|fixture| fixture.observed_angle_error_deg)
            .collect::<Vec<_>>(),
        config.target_positive_recall,
    );
    let bucket_p95 = percentile(
        &manifest
            .fixtures
            .iter()
            .map(|fixture| fixture.observed_bucket_occupancy as f64)
            .collect::<Vec<_>>(),
        0.95,
    );
    let peak_memory = manifest
        .fixtures
        .iter()
        .map(|fixture| fixture.peak_memory_mb)
        .fold(0.0, f64::max);
    let runtime = manifest
        .fixtures
        .iter()
        .map(|fixture| fixture.runtime_ms)
        .sum::<f64>()
        / manifest.fixtures.len() as f64;

    if bucket_p95 > config.max_bucket_occupancy_p95 {
        return Err("calibration_bucket_occupancy_exceeds_threshold".to_string());
    }
    if peak_memory > config.max_peak_memory_mb {
        return Err("calibration_peak_memory_exceeds_threshold".to_string());
    }
    if runtime > config.max_runtime_ms_per_fixture {
        return Err("calibration_runtime_exceeds_threshold".to_string());
    }

    let mut profiles = Vec::new();
    for (level, scale) in &config.level_scales {
        let radius = base_radius * scale;
        let angle = base_angle * scale;
        let positive_recall = recall(&positives, radius, angle);
        let hard_negative_false_positive_rate = recall(&hard_negatives, radius, angle);
        if hard_negative_false_positive_rate > config.max_hard_negative_false_positive_rate
            && *level == ToleranceProfileLevel::Standard
        {
            return Err("calibration_hard_negative_false_positive_exceeds_threshold".to_string());
        }
        let hard_negative_rejection = 1.0 - hard_negative_false_positive_rate;
        profiles.push(profile_from_metrics(
            manifest,
            config,
            level.clone(),
            ProfileMetrics {
                radius,
                angle,
                positive_recall,
                hard_negative_rejection,
                hard_negative_false_positive_rate,
                bucket_p95,
                peak_memory,
                runtime,
            },
        ));
    }

    let standard_metrics = profiles
        .iter()
        .find(|profile| profile.profile_level == ToleranceProfileLevel::Standard)
        .map(|profile| (profile.recall_at_profile, profile.hard_negative_rejection))
        .expect("standard profile exists");
    Ok(CalibrationReport {
        dataset_id: manifest.dataset_id.clone(),
        profiles,
        positive_recall: standard_metrics.0,
        hard_negative_rejection: standard_metrics.1,
        bucket_occupancy_p95: bucket_p95,
        peak_memory_mb: peak_memory,
        runtime_ms_per_fixture: runtime,
    })
}

fn profile_from_metrics(
    manifest: &FixtureManifest,
    config: &CalibrationConfig,
    level: ToleranceProfileLevel,
    metrics: ProfileMetrics,
) -> ToleranceProfile {
    let first = &manifest.fixtures[0];
    let profile_payload = format!(
        "{}:{}:{}:{:.3}:{:.3}",
        manifest.dataset_id,
        first.interaction_kind.as_str(),
        level.as_str(),
        metrics.radius,
        metrics.angle
    );
    ToleranceProfile {
        profile_id: format!("tol_{}_{}", level.as_str(), stable_hash(&profile_payload)),
        dataset_id: manifest.dataset_id.clone(),
        created_at: config.created_at.clone(),
        interaction_kind: first.interaction_kind.clone(),
        ligand_class: first.ligand_class.clone(),
        structure_confidence_bin: first.structure_confidence_bin.clone(),
        radius_tolerance: metrics.radius,
        angle_tolerance_deg: metrics.angle,
        distance_bin_width: (metrics.radius / 2.0).max(f64::EPSILON),
        vector_angle_bin_width: (metrics.angle / 2.0).max(f64::EPSILON),
        max_bucket_occupancy: config.max_bucket_occupancy,
        max_seed_triplets: config.max_seed_triplets,
        max_memory_mb: config.max_peak_memory_mb,
        recall_at_profile: metrics.positive_recall,
        hard_negative_rejection: metrics.hard_negative_rejection,
        hard_negative_false_positive_rate: metrics.hard_negative_false_positive_rate,
        bucket_occupancy_p95: metrics.bucket_p95,
        peak_memory_mb: metrics.peak_memory,
        runtime_ms_per_fixture: metrics.runtime,
        generated_by_fixture_calibration: true,
        profile_level: level,
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct ProfileMetrics {
    radius: f64,
    angle: f64,
    positive_recall: f64,
    hard_negative_rejection: f64,
    hard_negative_false_positive_rate: f64,
    bucket_p95: f64,
    peak_memory: f64,
    runtime: f64,
}

fn recall(fixtures: &[&PatchFixture], radius: f64, angle: f64) -> f64 {
    let passing = fixtures
        .iter()
        .filter(|fixture| {
            fixture.observed_radius_error <= radius && fixture.observed_angle_error_deg <= angle
        })
        .count();
    passing as f64 / fixtures.len().max(1) as f64
}

fn percentile(values: &[f64], quantile: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let index = ((sorted.len() - 1) as f64 * quantile.clamp(0.0, 1.0)).ceil() as usize;
    sorted[index]
}

pub fn select_profiles_for_runtime(
    profiles: &[ToleranceProfile],
    interaction_kind: &InteractionKind,
    ligand_class: &str,
    confidence_bin: &str,
) -> Vec<ToleranceProfile> {
    let mut selected = profiles
        .iter()
        .filter(|profile| {
            &profile.interaction_kind == interaction_kind
                && profile.ligand_class == ligand_class
                && profile.structure_confidence_bin == confidence_bin
        })
        .cloned()
        .collect::<Vec<_>>();
    selected.sort_by_key(|profile| match profile.profile_level {
        ToleranceProfileLevel::Narrow => 0,
        ToleranceProfileLevel::Standard => 1,
        ToleranceProfileLevel::Relaxed => 2,
    });
    selected
}

#[cfg(test)]
mod tests {
    use super::*;
    use gls_fixtures::dev_fixture_manifest;

    #[test]
    fn calibration_generates_three_fixture_backed_profiles() {
        let manifest = dev_fixture_manifest();
        let report =
            calibrate_tolerances(&manifest, &CalibrationConfig::dev()).expect("calibration ok");

        assert_eq!(report.profiles.len(), 3);
        assert!(
            report
                .profiles
                .iter()
                .all(|profile| profile.validate_for_runtime().is_ok())
        );
        assert!(
            report
                .profiles
                .iter()
                .any(|profile| profile.profile_level == ToleranceProfileLevel::Narrow)
        );
    }

    #[test]
    fn runtime_selection_is_coarse_to_fine() {
        let manifest = dev_fixture_manifest();
        let report =
            calibrate_tolerances(&manifest, &CalibrationConfig::dev()).expect("calibration ok");
        let selected = select_profiles_for_runtime(
            &report.profiles,
            &InteractionKind::HydrogenBond,
            "generic",
            "high",
        );

        assert_eq!(selected[0].profile_level, ToleranceProfileLevel::Narrow);
        assert_eq!(selected[2].profile_level, ToleranceProfileLevel::Relaxed);
    }
}
