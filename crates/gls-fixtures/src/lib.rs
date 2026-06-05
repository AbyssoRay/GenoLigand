use gls_types::{InteractionKind, stable_hash};
use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FixtureType {
    PositiveHolo,
    PositivePerturbed,
    HardNegativeSameResidues,
    PocketDecoy,
    PredictedStructureNoise,
}

impl FixtureType {
    pub fn is_positive(&self) -> bool {
        matches!(self, Self::PositiveHolo | Self::PositivePerturbed)
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::PositiveHolo => "positive_holo",
            Self::PositivePerturbed => "positive_perturbed",
            Self::HardNegativeSameResidues => "hard_negative_same_residues",
            Self::PocketDecoy => "pocket_decoy",
            Self::PredictedStructureNoise => "predicted_structure_noise",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PatchFixture {
    pub fixture_id: String,
    pub fixture_type: FixtureType,
    pub split: String,
    pub source: String,
    pub license_note: String,
    pub ligand_id: String,
    pub pocket_id: String,
    pub interaction_labels: Vec<String>,
    pub resolution_angstrom: f64,
    pub ligand_class: String,
    pub structure_confidence_bin: String,
    pub interaction_kind: InteractionKind,
    pub residue_signature: String,
    pub observed_radius_error: f64,
    pub observed_angle_error_deg: f64,
    pub observed_bucket_occupancy: usize,
    pub observed_seed_triplets: usize,
    pub runtime_ms: f64,
    pub peak_memory_mb: f64,
    pub hash: String,
}

impl PatchFixture {
    pub fn new(
        fixture_id: impl Into<String>,
        fixture_type: FixtureType,
        split: impl Into<String>,
        source: impl Into<String>,
        license_note: impl Into<String>,
        residue_signature: impl Into<String>,
        observed_errors: (f64, f64),
    ) -> Self {
        let (observed_radius_error, observed_angle_error_deg) = observed_errors;
        let fixture_id = fixture_id.into();
        let split = split.into();
        let source = source.into();
        let license_note = license_note.into();
        let residue_signature = residue_signature.into();
        let ligand_id = format!("ligand_{fixture_id}");
        let pocket_id = format!("pocket_{fixture_id}");
        let interaction_labels = vec![format!(
            "{}:{}",
            InteractionKind::HydrogenBond.as_str(),
            residue_signature
        )];
        let resolution_angstrom = 1.8;
        let payload = format!(
            "{fixture_id}:{split}:{source}:{license_note}:{ligand_id}:{pocket_id}:{residue_signature}:{observed_radius_error:.3}:{observed_angle_error_deg:.3}"
        );
        Self {
            fixture_id,
            fixture_type,
            split,
            source,
            license_note,
            ligand_id,
            pocket_id,
            interaction_labels,
            resolution_angstrom,
            ligand_class: "generic".to_string(),
            structure_confidence_bin: "high".to_string(),
            interaction_kind: InteractionKind::HydrogenBond,
            residue_signature,
            observed_radius_error,
            observed_angle_error_deg,
            observed_bucket_occupancy: 1,
            observed_seed_triplets: 1,
            runtime_ms: 1.0,
            peak_memory_mb: 1.0,
            hash: stable_hash(&payload),
        }
    }

    pub fn leakage_key(&self) -> String {
        format!(
            "{}:{}:{}",
            self.source,
            self.residue_signature,
            self.interaction_kind.as_str()
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct FixtureManifest {
    pub dataset_id: String,
    pub fixtures: Vec<PatchFixture>,
}

impl FixtureManifest {
    pub fn validate(&self) -> Result<(), String> {
        if self.dataset_id.is_empty() {
            return Err("fixture_manifest_missing_dataset_id".to_string());
        }
        let mut positive_keys = BTreeSet::<String>::new();
        let mut hard_negative_keys = BTreeSet::<String>::new();
        for fixture in &self.fixtures {
            if fixture.hash.is_empty() {
                return Err(format!("fixture_missing_hash:{}", fixture.fixture_id));
            }
            if fixture.source.is_empty() {
                return Err(format!("fixture_missing_source:{}", fixture.fixture_id));
            }
            if fixture.license_note.is_empty() {
                return Err(format!(
                    "fixture_missing_license_note:{}",
                    fixture.fixture_id
                ));
            }
            if fixture.split.is_empty() {
                return Err(format!("fixture_missing_split:{}", fixture.fixture_id));
            }
            if fixture.ligand_id.is_empty() {
                return Err(format!("fixture_missing_ligand:{}", fixture.fixture_id));
            }
            if fixture.pocket_id.is_empty() {
                return Err(format!("fixture_missing_pocket:{}", fixture.fixture_id));
            }
            if fixture.interaction_labels.is_empty() {
                return Err(format!(
                    "fixture_missing_interaction_labels:{}",
                    fixture.fixture_id
                ));
            }
            if fixture.resolution_angstrom <= 0.0 {
                return Err(format!("fixture_missing_resolution:{}", fixture.fixture_id));
            }
            if fixture.fixture_type.is_positive() {
                positive_keys.insert(fixture.leakage_key());
            } else if fixture.fixture_type == FixtureType::HardNegativeSameResidues {
                hard_negative_keys.insert(fixture.leakage_key());
            }
        }
        if positive_keys
            .intersection(&hard_negative_keys)
            .next()
            .is_some()
        {
            return Err("hard_negative_positive_split_leakage".to_string());
        }
        Ok(())
    }
}

pub fn dev_fixture_manifest() -> FixtureManifest {
    let mut positive = PatchFixture::new(
        "pos_hbond_1",
        FixtureType::PositiveHolo,
        "calibration_train",
        "curated-dev-positive",
        "internal dev fixture; replace with PDBbind or Binding MOAD before production",
        "D-H-Q",
        (0.35, 8.0),
    );
    positive.observed_bucket_occupancy = 2;
    positive.observed_seed_triplets = 4;

    let mut perturbed = PatchFixture::new(
        "pos_hbond_perturbed_1",
        FixtureType::PositivePerturbed,
        "calibration_train",
        "curated-dev-perturbed",
        "internal dev fixture; coordinate-noise calibration only",
        "D-H-Q",
        (0.65, 14.0),
    );
    perturbed.observed_bucket_occupancy = 3;
    perturbed.observed_seed_triplets = 6;

    let mut hard_negative = PatchFixture::new(
        "hard_neg_same_residue_1",
        FixtureType::HardNegativeSameResidues,
        "calibration_holdout",
        "curated-dev-negative",
        "internal dev fixture; hard negative geometry",
        "D-H-Q",
        (3.50, 70.0),
    );
    hard_negative.observed_bucket_occupancy = 1;
    hard_negative.observed_seed_triplets = 4;

    let mut pocket_decoy = PatchFixture::new(
        "pocket_decoy_1",
        FixtureType::PocketDecoy,
        "calibration_holdout",
        "curated-dev-pocket-decoy",
        "internal dev fixture; pocket decoy bucket-explosion control",
        "D-H-Q",
        (2.25, 48.0),
    );
    pocket_decoy.observed_bucket_occupancy = 12;
    pocket_decoy.observed_seed_triplets = 64;
    pocket_decoy.pocket_id = "decoy_surface_patch_1".to_string();

    let mut predicted_noise = PatchFixture::new(
        "predicted_noise_1",
        FixtureType::PredictedStructureNoise,
        "calibration_train",
        "curated-dev-predicted-noise",
        "internal dev fixture; predicted-structure coordinate noise",
        "D-H-Q",
        (0.95, 22.0),
    );
    predicted_noise.structure_confidence_bin = "low".to_string();
    predicted_noise.observed_bucket_occupancy = 5;
    predicted_noise.observed_seed_triplets = 10;

    FixtureManifest {
        dataset_id: "dev-fixtures-v1.6".to_string(),
        fixtures: vec![
            positive,
            perturbed,
            hard_negative,
            pocket_decoy,
            predicted_noise,
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dev_manifest_has_required_hash_source_license_and_split() {
        let manifest = dev_fixture_manifest();
        manifest.validate().expect("dev fixtures are valid");
        assert!(
            manifest
                .fixtures
                .iter()
                .all(|fixture| !fixture.hash.is_empty())
        );
        assert!(manifest.fixtures.iter().all(|fixture| {
            !fixture.ligand_id.is_empty()
                && !fixture.pocket_id.is_empty()
                && !fixture.interaction_labels.is_empty()
                && fixture.resolution_angstrom > 0.0
        }));
    }

    #[test]
    fn dev_manifest_covers_all_required_fixture_types() {
        let manifest = dev_fixture_manifest();
        let types = manifest
            .fixtures
            .iter()
            .map(|fixture| fixture.fixture_type.as_str())
            .collect::<BTreeSet<_>>();

        for required in [
            "positive_holo",
            "positive_perturbed",
            "hard_negative_same_residues",
            "pocket_decoy",
            "predicted_structure_noise",
        ] {
            assert!(types.contains(required), "missing fixture type {required}");
        }
    }

    #[test]
    fn hard_negative_positive_leakage_is_rejected() {
        let mut positive = PatchFixture::new(
            "p",
            FixtureType::PositiveHolo,
            "train",
            "same",
            "license",
            "A-B-C",
            (0.1, 1.0),
        );
        let mut negative = positive.clone();
        negative.fixture_id = "n".to_string();
        negative.fixture_type = FixtureType::HardNegativeSameResidues;
        negative.split = "holdout".to_string();
        positive.hash = "p".to_string();
        negative.hash = "n".to_string();
        let manifest = FixtureManifest {
            dataset_id: "leaky".to_string(),
            fixtures: vec![positive, negative],
        };

        assert_eq!(
            manifest.validate(),
            Err("hard_negative_positive_split_leakage".to_string())
        );
    }
}
