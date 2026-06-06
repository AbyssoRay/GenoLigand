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

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "positive_holo" => Ok(Self::PositiveHolo),
            "positive_perturbed" => Ok(Self::PositivePerturbed),
            "hard_negative_same_residues" => Ok(Self::HardNegativeSameResidues),
            "pocket_decoy" => Ok(Self::PocketDecoy),
            "predicted_structure_noise" => Ok(Self::PredictedStructureNoise),
            _ => Err(format!("unknown_fixture_type:{value}")),
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
    pub residue_flexibility_bin: String,
    pub pocket_depth_bin: String,
    pub coordinate_noise_level: String,
    pub interaction_kind: InteractionKind,
    pub residue_signature: String,
    pub observed_radius_error: f64,
    pub observed_angle_error_deg: f64,
    pub observed_sasa: f64,
    pub observed_occlusion: f64,
    pub observed_pocket_overlap_score: f64,
    pub observed_rotamer_feasibility_score: f64,
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
        let mut fixture = Self {
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
            residue_flexibility_bin: "medium".to_string(),
            pocket_depth_bin: "shallow".to_string(),
            coordinate_noise_level: "low".to_string(),
            interaction_kind: InteractionKind::HydrogenBond,
            residue_signature,
            observed_radius_error,
            observed_angle_error_deg,
            observed_sasa: 0.5,
            observed_occlusion: 0.2,
            observed_pocket_overlap_score: 0.4,
            observed_rotamer_feasibility_score: 0.8,
            observed_bucket_occupancy: 1,
            observed_seed_triplets: 1,
            runtime_ms: 1.0,
            peak_memory_mb: 1.0,
            hash: String::new(),
        };
        fixture.refresh_hash();
        fixture
    }

    pub fn leakage_key(&self) -> String {
        format!(
            "{}:{}:{}",
            self.source,
            self.residue_signature,
            self.interaction_kind.as_str()
        )
    }

    pub fn refresh_hash(&mut self) {
        let labels = self.interaction_labels.join(",");
        self.hash = stable_hash(&format!(
            "{}:{}:{}:{}:{}:{}:{}:{:.3}:{}:{}:{}:{}:{}:{}:{}:{:.6}:{:.6}:{:.6}:{:.6}:{:.6}:{:.6}:{}:{}:{:.6}:{:.6}",
            self.fixture_id,
            self.fixture_type.as_str(),
            self.split,
            self.source,
            self.license_note,
            self.ligand_id,
            self.pocket_id,
            self.resolution_angstrom,
            self.interaction_kind.as_str(),
            self.ligand_class,
            self.structure_confidence_bin,
            self.residue_flexibility_bin,
            self.pocket_depth_bin,
            self.coordinate_noise_level,
            labels,
            self.observed_radius_error,
            self.observed_angle_error_deg,
            self.observed_sasa,
            self.observed_occlusion,
            self.observed_pocket_overlap_score,
            self.observed_rotamer_feasibility_score,
            self.observed_bucket_occupancy,
            self.observed_seed_triplets,
            self.runtime_ms,
            self.peak_memory_mb
        ));
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
            if fixture.residue_flexibility_bin.is_empty()
                || fixture.pocket_depth_bin.is_empty()
                || fixture.coordinate_noise_level.is_empty()
            {
                return Err(format!(
                    "fixture_missing_calibration_context:{}",
                    fixture.fixture_id
                ));
            }
            for (field, value) in [
                ("observed_sasa", fixture.observed_sasa),
                ("observed_occlusion", fixture.observed_occlusion),
                (
                    "observed_pocket_overlap_score",
                    fixture.observed_pocket_overlap_score,
                ),
                (
                    "observed_rotamer_feasibility_score",
                    fixture.observed_rotamer_feasibility_score,
                ),
            ] {
                if !(0.0..=1.0).contains(&value) {
                    return Err(format!(
                        "fixture_surface_metric_out_of_range:{field}:{}",
                        fixture.fixture_id
                    ));
                }
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
    FixtureManifest {
        dataset_id: "dev-fixtures-v1.6".to_string(),
        fixtures: vec![
            calibration_fixture(FixtureSpec {
                fixture_id: "pos_hbond_1",
                fixture_type: FixtureType::PositiveHolo,
                split: "calibration_train",
                source: "PDBbind:dev-positive",
                license_note: "PDBbind-derived internal calibration fixture",
                ligand_id: "pdbbind_lig_hbond_1",
                pocket_id: "pdbbind_pocket_hbond_1",
                interaction_labels: &["hydrogen_bond:D-H-Q", "holo_complex"],
                resolution_angstrom: 1.8,
                context: FixtureContext {
                    interaction_kind: InteractionKind::HydrogenBond,
                    ligand_class: "generic",
                    structure_confidence_bin: "high",
                    residue_flexibility_bin: "medium",
                    pocket_depth_bin: "shallow",
                    coordinate_noise_level: "low",
                    residue_signature: "D-H-Q",
                },
                observed_errors: (0.35, 8.0),
                metrics: FixtureMetrics {
                    sasa: 0.42,
                    occlusion: 0.22,
                    pocket_overlap_score: 0.48,
                    rotamer_feasibility_score: 0.82,
                    bucket_occupancy: 2,
                    seed_triplets: 4,
                    runtime_ms: 2.0,
                    peak_memory_mb: 8.0,
                },
            }),
            calibration_fixture(FixtureSpec {
                fixture_id: "pos_hbond_perturbed_1",
                fixture_type: FixtureType::PositivePerturbed,
                split: "calibration_train",
                source: "internal curated:apo-holo-positive",
                license_note: "internal curated apo/holo perturbation fixture",
                ligand_id: "internal_lig_hbond_perturbed_1",
                pocket_id: "internal_pocket_hbond_apo_holo_1",
                interaction_labels: &["hydrogen_bond:D-H-Q", "apo_holo_difference"],
                resolution_angstrom: 2.1,
                context: FixtureContext {
                    interaction_kind: InteractionKind::HydrogenBond,
                    ligand_class: "generic",
                    structure_confidence_bin: "high",
                    residue_flexibility_bin: "medium",
                    pocket_depth_bin: "shallow",
                    coordinate_noise_level: "low",
                    residue_signature: "D-H-Q",
                },
                observed_errors: (0.65, 14.0),
                metrics: FixtureMetrics {
                    sasa: 0.31,
                    occlusion: 0.34,
                    pocket_overlap_score: 0.36,
                    rotamer_feasibility_score: 0.70,
                    bucket_occupancy: 3,
                    seed_triplets: 6,
                    runtime_ms: 3.0,
                    peak_memory_mb: 9.0,
                },
            }),
            calibration_fixture(FixtureSpec {
                fixture_id: "hard_neg_hbond_same_residue_1",
                fixture_type: FixtureType::HardNegativeSameResidues,
                split: "calibration_holdout",
                source: "PDBbind:dev-hard-negative",
                license_note: "PDBbind-derived hard negative with matched residues",
                ligand_id: "pdbbind_lig_hbond_hard_neg_1",
                pocket_id: "pdbbind_pocket_hbond_hard_neg_1",
                interaction_labels: &["hydrogen_bond:D-H-Q", "same_residue_hard_negative"],
                resolution_angstrom: 1.9,
                context: FixtureContext {
                    interaction_kind: InteractionKind::HydrogenBond,
                    ligand_class: "generic",
                    structure_confidence_bin: "high",
                    residue_flexibility_bin: "medium",
                    pocket_depth_bin: "shallow",
                    coordinate_noise_level: "low",
                    residue_signature: "D-H-Q",
                },
                observed_errors: (3.50, 70.0),
                metrics: FixtureMetrics {
                    sasa: 0.12,
                    occlusion: 0.82,
                    pocket_overlap_score: 0.08,
                    rotamer_feasibility_score: 0.35,
                    bucket_occupancy: 1,
                    seed_triplets: 4,
                    runtime_ms: 2.0,
                    peak_memory_mb: 7.0,
                },
            }),
            calibration_fixture(FixtureSpec {
                fixture_id: "pocket_decoy_hbond_1",
                fixture_type: FixtureType::PocketDecoy,
                split: "calibration_holdout",
                source: "internal curated:pocket-decoy",
                license_note: "internal curated pocket decoy bucket-control fixture",
                ligand_id: "internal_lig_pocket_decoy_1",
                pocket_id: "decoy_surface_patch_1",
                interaction_labels: &["hydrogen_bond:D-H-Q", "pocket_decoy"],
                resolution_angstrom: 2.0,
                context: FixtureContext {
                    interaction_kind: InteractionKind::HydrogenBond,
                    ligand_class: "generic",
                    structure_confidence_bin: "high",
                    residue_flexibility_bin: "medium",
                    pocket_depth_bin: "shallow",
                    coordinate_noise_level: "low",
                    residue_signature: "D-H-Q",
                },
                observed_errors: (2.25, 48.0),
                metrics: FixtureMetrics {
                    sasa: 0.18,
                    occlusion: 0.74,
                    pocket_overlap_score: 0.12,
                    rotamer_feasibility_score: 0.48,
                    bucket_occupancy: 12,
                    seed_triplets: 64,
                    runtime_ms: 5.0,
                    peak_memory_mb: 14.0,
                },
            }),
            calibration_fixture(FixtureSpec {
                fixture_id: "pos_hydrophobic_deep_1",
                fixture_type: FixtureType::PositiveHolo,
                split: "calibration_train",
                source: "Binding MOAD:dev-positive",
                license_note: "Binding MOAD-derived hydrophobic deep-pocket fixture",
                ligand_id: "moad_lig_hydrophobic_1",
                pocket_id: "moad_deep_pocket_hydrophobic_1",
                interaction_labels: &["hydrophobic:L-I-V", "deep_pocket"],
                resolution_angstrom: 1.7,
                context: FixtureContext {
                    interaction_kind: InteractionKind::Hydrophobic,
                    ligand_class: "generic",
                    structure_confidence_bin: "high",
                    residue_flexibility_bin: "rigid",
                    pocket_depth_bin: "deep",
                    coordinate_noise_level: "low",
                    residue_signature: "L-I-V",
                },
                observed_errors: (0.28, 6.0),
                metrics: FixtureMetrics {
                    sasa: 0.50,
                    occlusion: 0.18,
                    pocket_overlap_score: 0.55,
                    rotamer_feasibility_score: 0.86,
                    bucket_occupancy: 4,
                    seed_triplets: 12,
                    runtime_ms: 3.5,
                    peak_memory_mb: 10.0,
                },
            }),
            calibration_fixture(FixtureSpec {
                fixture_id: "hard_neg_hydrophobic_deep_1",
                fixture_type: FixtureType::HardNegativeSameResidues,
                split: "calibration_holdout",
                source: "Binding MOAD:dev-hard-negative",
                license_note: "Binding MOAD-derived hydrophobic same-residue hard negative",
                ligand_id: "moad_lig_hydrophobic_neg_1",
                pocket_id: "moad_deep_pocket_hydrophobic_neg_1",
                interaction_labels: &["hydrophobic:L-I-V", "same_residue_hard_negative"],
                resolution_angstrom: 1.9,
                context: FixtureContext {
                    interaction_kind: InteractionKind::Hydrophobic,
                    ligand_class: "generic",
                    structure_confidence_bin: "high",
                    residue_flexibility_bin: "rigid",
                    pocket_depth_bin: "deep",
                    coordinate_noise_level: "low",
                    residue_signature: "L-I-V",
                },
                observed_errors: (2.90, 61.0),
                metrics: FixtureMetrics {
                    sasa: 0.16,
                    occlusion: 0.78,
                    pocket_overlap_score: 0.09,
                    rotamer_feasibility_score: 0.40,
                    bucket_occupancy: 3,
                    seed_triplets: 10,
                    runtime_ms: 3.0,
                    peak_memory_mb: 9.0,
                },
            }),
            calibration_fixture(FixtureSpec {
                fixture_id: "pos_aromatic_stack_1",
                fixture_type: FixtureType::PositiveHolo,
                split: "calibration_train",
                source: "BioLiP2:dev-positive",
                license_note: "BioLiP2-derived aromatic stacking fixture",
                ligand_id: "biolip2_lig_aromatic_1",
                pocket_id: "biolip2_pocket_aromatic_1",
                interaction_labels: &["aromatic_stacking:F-Y-W"],
                resolution_angstrom: 1.6,
                context: FixtureContext {
                    interaction_kind: InteractionKind::AromaticStacking,
                    ligand_class: "aromatic",
                    structure_confidence_bin: "high",
                    residue_flexibility_bin: "medium",
                    pocket_depth_bin: "shallow",
                    coordinate_noise_level: "low",
                    residue_signature: "F-Y-W",
                },
                observed_errors: (0.32, 7.0),
                metrics: FixtureMetrics {
                    sasa: 0.45,
                    occlusion: 0.20,
                    pocket_overlap_score: 0.52,
                    rotamer_feasibility_score: 0.80,
                    bucket_occupancy: 5,
                    seed_triplets: 18,
                    runtime_ms: 4.0,
                    peak_memory_mb: 11.0,
                },
            }),
            calibration_fixture(FixtureSpec {
                fixture_id: "hard_neg_aromatic_stack_1",
                fixture_type: FixtureType::HardNegativeSameResidues,
                split: "calibration_holdout",
                source: "BioLiP2:dev-hard-negative",
                license_note: "BioLiP2-derived aromatic same-residue hard negative",
                ligand_id: "biolip2_lig_aromatic_neg_1",
                pocket_id: "biolip2_pocket_aromatic_neg_1",
                interaction_labels: &["aromatic_stacking:F-Y-W", "same_residue_hard_negative"],
                resolution_angstrom: 1.8,
                context: FixtureContext {
                    interaction_kind: InteractionKind::AromaticStacking,
                    ligand_class: "aromatic",
                    structure_confidence_bin: "high",
                    residue_flexibility_bin: "medium",
                    pocket_depth_bin: "shallow",
                    coordinate_noise_level: "low",
                    residue_signature: "F-Y-W",
                },
                observed_errors: (3.10, 66.0),
                metrics: FixtureMetrics {
                    sasa: 0.15,
                    occlusion: 0.80,
                    pocket_overlap_score: 0.07,
                    rotamer_feasibility_score: 0.38,
                    bucket_occupancy: 4,
                    seed_triplets: 16,
                    runtime_ms: 3.5,
                    peak_memory_mb: 10.0,
                },
            }),
            calibration_fixture(FixtureSpec {
                fixture_id: "pos_salt_bridge_flexible_deep_1",
                fixture_type: FixtureType::PositiveHolo,
                split: "calibration_train",
                source: "internal curated:salt-bridge-positive",
                license_note: "internal curated flexible side-chain salt-bridge fixture",
                ligand_id: "internal_lig_salt_bridge_1",
                pocket_id: "internal_deep_pocket_salt_bridge_1",
                interaction_labels: &["salt_bridge:D-K-E", "flexible_side_chain", "deep_pocket"],
                resolution_angstrom: 2.2,
                context: FixtureContext {
                    interaction_kind: InteractionKind::SaltBridge,
                    ligand_class: "charged",
                    structure_confidence_bin: "high",
                    residue_flexibility_bin: "flexible",
                    pocket_depth_bin: "deep",
                    coordinate_noise_level: "low",
                    residue_signature: "D-K-E",
                },
                observed_errors: (0.48, 10.0),
                metrics: FixtureMetrics {
                    sasa: 0.35,
                    occlusion: 0.28,
                    pocket_overlap_score: 0.43,
                    rotamer_feasibility_score: 0.68,
                    bucket_occupancy: 6,
                    seed_triplets: 24,
                    runtime_ms: 4.5,
                    peak_memory_mb: 12.0,
                },
            }),
            calibration_fixture(FixtureSpec {
                fixture_id: "hard_neg_salt_bridge_flexible_deep_1",
                fixture_type: FixtureType::HardNegativeSameResidues,
                split: "calibration_holdout",
                source: "internal curated:salt-bridge-hard-negative",
                license_note: "internal curated flexible salt-bridge hard negative",
                ligand_id: "internal_lig_salt_bridge_neg_1",
                pocket_id: "internal_deep_pocket_salt_bridge_neg_1",
                interaction_labels: &["salt_bridge:D-K-E", "same_residue_hard_negative"],
                resolution_angstrom: 2.2,
                context: FixtureContext {
                    interaction_kind: InteractionKind::SaltBridge,
                    ligand_class: "charged",
                    structure_confidence_bin: "high",
                    residue_flexibility_bin: "flexible",
                    pocket_depth_bin: "deep",
                    coordinate_noise_level: "low",
                    residue_signature: "D-K-E",
                },
                observed_errors: (3.40, 72.0),
                metrics: FixtureMetrics {
                    sasa: 0.10,
                    occlusion: 0.84,
                    pocket_overlap_score: 0.06,
                    rotamer_feasibility_score: 0.32,
                    bucket_occupancy: 4,
                    seed_triplets: 20,
                    runtime_ms: 4.0,
                    peak_memory_mb: 11.0,
                },
            }),
            calibration_fixture(FixtureSpec {
                fixture_id: "pos_metal_coordination_noisy_1",
                fixture_type: FixtureType::PositivePerturbed,
                split: "calibration_train",
                source: "BioLiP2:metal-noisy-positive",
                license_note: "BioLiP2-derived metal coordination predicted-structure fixture",
                ligand_id: "biolip2_lig_metal_1",
                pocket_id: "biolip2_deep_pocket_metal_1",
                interaction_labels: &[
                    "metal_coordination:H-C-D",
                    "coordinate_noise_high",
                    "deep_pocket",
                ],
                resolution_angstrom: 2.4,
                context: FixtureContext {
                    interaction_kind: InteractionKind::MetalCoordination,
                    ligand_class: "metal",
                    structure_confidence_bin: "low",
                    residue_flexibility_bin: "flexible",
                    pocket_depth_bin: "deep",
                    coordinate_noise_level: "high",
                    residue_signature: "H-C-D",
                },
                observed_errors: (0.90, 20.0),
                metrics: FixtureMetrics {
                    sasa: 0.27,
                    occlusion: 0.45,
                    pocket_overlap_score: 0.30,
                    rotamer_feasibility_score: 0.60,
                    bucket_occupancy: 8,
                    seed_triplets: 36,
                    runtime_ms: 5.5,
                    peak_memory_mb: 14.0,
                },
            }),
            calibration_fixture(FixtureSpec {
                fixture_id: "hard_neg_metal_coordination_noisy_1",
                fixture_type: FixtureType::HardNegativeSameResidues,
                split: "calibration_holdout",
                source: "BioLiP2:metal-noisy-hard-negative",
                license_note: "BioLiP2-derived metal coordination noisy hard negative",
                ligand_id: "biolip2_lig_metal_neg_1",
                pocket_id: "biolip2_deep_pocket_metal_neg_1",
                interaction_labels: &[
                    "metal_coordination:H-C-D",
                    "coordinate_noise_high",
                    "same_residue_hard_negative",
                ],
                resolution_angstrom: 2.5,
                context: FixtureContext {
                    interaction_kind: InteractionKind::MetalCoordination,
                    ligand_class: "metal",
                    structure_confidence_bin: "low",
                    residue_flexibility_bin: "flexible",
                    pocket_depth_bin: "deep",
                    coordinate_noise_level: "high",
                    residue_signature: "H-C-D",
                },
                observed_errors: (4.00, 78.0),
                metrics: FixtureMetrics {
                    sasa: 0.08,
                    occlusion: 0.88,
                    pocket_overlap_score: 0.05,
                    rotamer_feasibility_score: 0.25,
                    bucket_occupancy: 5,
                    seed_triplets: 30,
                    runtime_ms: 5.0,
                    peak_memory_mb: 13.0,
                },
            }),
            calibration_fixture(FixtureSpec {
                fixture_id: "predicted_noise_metal_1",
                fixture_type: FixtureType::PredictedStructureNoise,
                split: "calibration_train",
                source: "internal curated:predicted-structure-noise",
                license_note: "internal curated predicted-structure coordinate-noise fixture",
                ligand_id: "internal_lig_metal_noise_1",
                pocket_id: "internal_deep_pocket_metal_noise_1",
                interaction_labels: &["metal_coordination:H-C-D", "predicted_structure_noise"],
                resolution_angstrom: 2.6,
                context: FixtureContext {
                    interaction_kind: InteractionKind::MetalCoordination,
                    ligand_class: "metal",
                    structure_confidence_bin: "low",
                    residue_flexibility_bin: "flexible",
                    pocket_depth_bin: "deep",
                    coordinate_noise_level: "high",
                    residue_signature: "H-C-D",
                },
                observed_errors: (1.15, 24.0),
                metrics: FixtureMetrics {
                    sasa: 0.24,
                    occlusion: 0.50,
                    pocket_overlap_score: 0.26,
                    rotamer_feasibility_score: 0.55,
                    bucket_occupancy: 10,
                    seed_triplets: 48,
                    runtime_ms: 6.0,
                    peak_memory_mb: 15.0,
                },
            }),
        ],
    }
}

struct FixtureSpec<'a> {
    fixture_id: &'a str,
    fixture_type: FixtureType,
    split: &'a str,
    source: &'a str,
    license_note: &'a str,
    ligand_id: &'a str,
    pocket_id: &'a str,
    interaction_labels: &'a [&'a str],
    resolution_angstrom: f64,
    context: FixtureContext<'a>,
    observed_errors: (f64, f64),
    metrics: FixtureMetrics,
}

struct FixtureContext<'a> {
    interaction_kind: InteractionKind,
    ligand_class: &'a str,
    structure_confidence_bin: &'a str,
    residue_flexibility_bin: &'a str,
    pocket_depth_bin: &'a str,
    coordinate_noise_level: &'a str,
    residue_signature: &'a str,
}

struct FixtureMetrics {
    sasa: f64,
    occlusion: f64,
    pocket_overlap_score: f64,
    rotamer_feasibility_score: f64,
    bucket_occupancy: usize,
    seed_triplets: usize,
    runtime_ms: f64,
    peak_memory_mb: f64,
}

fn calibration_fixture(spec: FixtureSpec<'_>) -> PatchFixture {
    let mut fixture = PatchFixture::new(
        spec.fixture_id,
        spec.fixture_type,
        spec.split,
        spec.source,
        spec.license_note,
        spec.context.residue_signature,
        spec.observed_errors,
    );
    fixture.ligand_id = spec.ligand_id.to_string();
    fixture.pocket_id = spec.pocket_id.to_string();
    fixture.interaction_labels = spec
        .interaction_labels
        .iter()
        .map(|label| (*label).to_string())
        .collect();
    fixture.resolution_angstrom = spec.resolution_angstrom;
    fixture.interaction_kind = spec.context.interaction_kind;
    fixture.ligand_class = spec.context.ligand_class.to_string();
    fixture.structure_confidence_bin = spec.context.structure_confidence_bin.to_string();
    fixture.residue_flexibility_bin = spec.context.residue_flexibility_bin.to_string();
    fixture.pocket_depth_bin = spec.context.pocket_depth_bin.to_string();
    fixture.coordinate_noise_level = spec.context.coordinate_noise_level.to_string();
    fixture.observed_sasa = spec.metrics.sasa;
    fixture.observed_occlusion = spec.metrics.occlusion;
    fixture.observed_pocket_overlap_score = spec.metrics.pocket_overlap_score;
    fixture.observed_rotamer_feasibility_score = spec.metrics.rotamer_feasibility_score;
    fixture.observed_bucket_occupancy = spec.metrics.bucket_occupancy;
    fixture.observed_seed_triplets = spec.metrics.seed_triplets;
    fixture.runtime_ms = spec.metrics.runtime_ms;
    fixture.peak_memory_mb = spec.metrics.peak_memory_mb;
    fixture.refresh_hash();
    fixture
}

pub fn fixture_manifest_from_tsv(
    dataset_id: impl Into<String>,
    tsv: &str,
) -> Result<FixtureManifest, String> {
    let mut fixtures = Vec::new();
    for (line_index, line) in tsv.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line_index == 0 && line.starts_with("fixture_id\t") {
            continue;
        }
        let fields = line.split('\t').collect::<Vec<_>>();
        if fields.len() != 26 {
            return Err(format!(
                "fixture_tsv_bad_field_count:line{}:expected26:found{}",
                line_index + 1,
                fields.len()
            ));
        }
        let mut fixture = PatchFixture::new(
            fields[0],
            FixtureType::parse(fields[1])?,
            fields[2],
            fields[3],
            fields[4],
            fields[15],
            (
                parse_f64(fields[16], line_index, "observed_radius_error")?,
                parse_f64(fields[17], line_index, "observed_angle_error_deg")?,
            ),
        );
        fixture.ligand_id = required_text(fields[5], line_index, "ligand_id")?.to_string();
        fixture.pocket_id = required_text(fields[6], line_index, "pocket_id")?.to_string();
        fixture.interaction_labels = parse_labels(fields[7], line_index)?;
        fixture.resolution_angstrom = parse_f64(fields[8], line_index, "resolution_angstrom")?;
        fixture.interaction_kind = parse_interaction_kind(fields[9])?;
        fixture.ligand_class = required_text(fields[10], line_index, "ligand_class")?.to_string();
        fixture.structure_confidence_bin =
            required_text(fields[11], line_index, "structure_confidence_bin")?.to_string();
        fixture.residue_flexibility_bin =
            required_text(fields[12], line_index, "residue_flexibility_bin")?.to_string();
        fixture.pocket_depth_bin =
            required_text(fields[13], line_index, "pocket_depth_bin")?.to_string();
        fixture.coordinate_noise_level =
            required_text(fields[14], line_index, "coordinate_noise_level")?.to_string();
        fixture.observed_sasa = parse_f64(fields[18], line_index, "observed_sasa")?;
        fixture.observed_occlusion = parse_f64(fields[19], line_index, "observed_occlusion")?;
        fixture.observed_pocket_overlap_score =
            parse_f64(fields[20], line_index, "observed_pocket_overlap_score")?;
        fixture.observed_rotamer_feasibility_score =
            parse_f64(fields[21], line_index, "observed_rotamer_feasibility_score")?;
        fixture.observed_bucket_occupancy =
            parse_usize(fields[22], line_index, "observed_bucket_occupancy")?;
        fixture.observed_seed_triplets =
            parse_usize(fields[23], line_index, "observed_seed_triplets")?;
        fixture.runtime_ms = parse_f64(fields[24], line_index, "runtime_ms")?;
        fixture.peak_memory_mb = parse_f64(fields[25], line_index, "peak_memory_mb")?;
        fixture.refresh_hash();
        fixtures.push(fixture);
    }
    let manifest = FixtureManifest {
        dataset_id: dataset_id.into(),
        fixtures,
    };
    manifest.validate()?;
    Ok(manifest)
}

fn parse_f64(value: &str, line_index: usize, field: &str) -> Result<f64, String> {
    value
        .parse::<f64>()
        .map_err(|err| format!("fixture_tsv_bad_float:line{}:{field}:{err}", line_index + 1))
}

fn parse_usize(value: &str, line_index: usize, field: &str) -> Result<usize, String> {
    value
        .parse::<usize>()
        .map_err(|err| format!("fixture_tsv_bad_usize:line{}:{field}:{err}", line_index + 1))
}

fn required_text<'a>(value: &'a str, line_index: usize, field: &str) -> Result<&'a str, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(format!(
            "fixture_tsv_missing_text:line{}:{field}",
            line_index + 1
        ));
    }
    Ok(value)
}

fn parse_labels(value: &str, line_index: usize) -> Result<Vec<String>, String> {
    let labels = value
        .split(',')
        .map(str::trim)
        .filter(|label| !label.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    if labels.is_empty() {
        return Err(format!(
            "fixture_tsv_missing_interaction_labels:line{}",
            line_index + 1
        ));
    }
    Ok(labels)
}

fn parse_interaction_kind(value: &str) -> Result<InteractionKind, String> {
    match value {
        "hydrogen_bond" => Ok(InteractionKind::HydrogenBond),
        "hydrophobic" => Ok(InteractionKind::Hydrophobic),
        "aromatic_stacking" => Ok(InteractionKind::AromaticStacking),
        "salt_bridge" => Ok(InteractionKind::SaltBridge),
        "metal_coordination" => Ok(InteractionKind::MetalCoordination),
        "generic" => Ok(InteractionKind::Generic),
        _ => Err(format!("unknown_interaction_kind:{value}")),
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
                && !fixture.residue_flexibility_bin.is_empty()
                && !fixture.pocket_depth_bin.is_empty()
                && !fixture.coordinate_noise_level.is_empty()
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
    fn dev_manifest_covers_required_calibration_sources_and_contexts() {
        let manifest = dev_fixture_manifest();
        let kinds = manifest
            .fixtures
            .iter()
            .map(|fixture| fixture.interaction_kind.as_str())
            .collect::<BTreeSet<_>>();
        for required in [
            "hydrogen_bond",
            "hydrophobic",
            "aromatic_stacking",
            "salt_bridge",
            "metal_coordination",
        ] {
            assert!(
                kinds.contains(required),
                "missing interaction kind {required}"
            );
        }

        for required_source in ["PDBbind", "Binding MOAD", "BioLiP2", "internal curated"] {
            assert!(
                manifest
                    .fixtures
                    .iter()
                    .any(|fixture| fixture.source.contains(required_source)),
                "missing source family {required_source}"
            );
        }

        assert!(
            manifest
                .fixtures
                .iter()
                .any(|fixture| fixture.pocket_depth_bin == "shallow")
        );
        assert!(
            manifest
                .fixtures
                .iter()
                .any(|fixture| fixture.pocket_depth_bin == "deep")
        );
        assert!(
            manifest
                .fixtures
                .iter()
                .any(|fixture| fixture.residue_flexibility_bin == "flexible")
        );
        assert!(manifest.fixtures.iter().any(|fixture| {
            fixture
                .interaction_labels
                .iter()
                .any(|label| label == "apo_holo_difference")
        }));
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

    #[test]
    fn curated_fixture_tsv_imports_manifest_with_metrics() {
        let tsv = "\
fixture_id\tfixture_type\tsplit\tsource\tlicense_note\tligand_id\tpocket_id\tinteraction_labels\tresolution_angstrom\tinteraction_kind\tligand_class\tstructure_confidence_bin\tresidue_flexibility_bin\tpocket_depth_bin\tcoordinate_noise_level\tresidue_signature\tobserved_radius_error\tobserved_angle_error_deg\tobserved_sasa\tobserved_occlusion\tobserved_pocket_overlap_score\tobserved_rotamer_feasibility_score\tobserved_bucket_occupancy\tobserved_seed_triplets\truntime_ms\tpeak_memory_mb
pos1\tpositive_holo\ttrain\tcurated-positive\tinternal\tlig1\tpocket1\thydrogen_bond:D-H-Q\t1.8\thydrogen_bond\tgeneric\thigh\tmedium\tshallow\tlow\tD-H-Q\t0.2\t5.0\t0.5\t0.2\t0.4\t0.8\t2\t4\t2.0\t1.0
neg1\thard_negative_same_residues\tholdout\tcurated-negative\tinternal\tlig2\tpocket2\thydrogen_bond:D-H-Q\t2.0\thydrogen_bond\tgeneric\thigh\tmedium\tshallow\tlow\tD-H-Q\t3.0\t60.0\t0.1\t0.8\t0.1\t0.3\t1\t4\t2.0\t1.0";
        let manifest = fixture_manifest_from_tsv("curated-v1", tsv).expect("fixture tsv");

        assert_eq!(manifest.dataset_id, "curated-v1");
        assert_eq!(manifest.fixtures.len(), 2);
        assert_eq!(manifest.fixtures[0].ligand_id, "lig1");
        assert_eq!(manifest.fixtures[0].pocket_id, "pocket1");
        assert_eq!(
            manifest.fixtures[0].interaction_labels,
            vec!["hydrogen_bond:D-H-Q"]
        );
        assert_eq!(
            manifest.fixtures[0].interaction_kind,
            InteractionKind::HydrogenBond
        );
        assert_eq!(manifest.fixtures[0].residue_flexibility_bin, "medium");
        assert_eq!(manifest.fixtures[0].pocket_depth_bin, "shallow");
        assert_eq!(manifest.fixtures[0].coordinate_noise_level, "low");
        assert_eq!(manifest.fixtures[0].observed_seed_triplets, 4);
    }
}
