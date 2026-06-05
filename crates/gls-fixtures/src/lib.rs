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
            "{}:{}:{}:{}:{}:{}:{}:{:.3}:{}:{}:{}:{}:{:.6}:{:.6}:{:.6}:{:.6}:{:.6}:{:.6}:{}:{}:{:.6}:{:.6}",
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
    let mut positive = PatchFixture::new(
        "pos_hbond_1",
        FixtureType::PositiveHolo,
        "calibration_train",
        "curated-dev-positive",
        "internal curated fixture; PDBbind or Binding MOAD imports use the same manifest schema",
        "D-H-Q",
        (0.35, 8.0),
    );
    positive.observed_bucket_occupancy = 2;
    positive.observed_seed_triplets = 4;
    positive.observed_sasa = 0.42;
    positive.observed_occlusion = 0.22;
    positive.observed_pocket_overlap_score = 0.48;
    positive.observed_rotamer_feasibility_score = 0.82;
    positive.refresh_hash();

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
    perturbed.observed_sasa = 0.31;
    perturbed.observed_occlusion = 0.34;
    perturbed.observed_pocket_overlap_score = 0.36;
    perturbed.observed_rotamer_feasibility_score = 0.70;
    perturbed.refresh_hash();

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
    hard_negative.observed_sasa = 0.12;
    hard_negative.observed_occlusion = 0.82;
    hard_negative.observed_pocket_overlap_score = 0.08;
    hard_negative.observed_rotamer_feasibility_score = 0.35;
    hard_negative.refresh_hash();

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
    pocket_decoy.observed_sasa = 0.18;
    pocket_decoy.observed_occlusion = 0.74;
    pocket_decoy.observed_pocket_overlap_score = 0.12;
    pocket_decoy.observed_rotamer_feasibility_score = 0.48;
    pocket_decoy.pocket_id = "decoy_surface_patch_1".to_string();
    pocket_decoy.refresh_hash();

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
    predicted_noise.observed_sasa = 0.27;
    predicted_noise.observed_occlusion = 0.45;
    predicted_noise.observed_pocket_overlap_score = 0.28;
    predicted_noise.observed_rotamer_feasibility_score = 0.62;
    predicted_noise.refresh_hash();

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
        if fields.len() != 23 {
            return Err(format!(
                "fixture_tsv_bad_field_count:line{}:expected23:found{}",
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
            fields[12],
            (
                parse_f64(fields[13], line_index, "observed_radius_error")?,
                parse_f64(fields[14], line_index, "observed_angle_error_deg")?,
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
        fixture.observed_sasa = parse_f64(fields[15], line_index, "observed_sasa")?;
        fixture.observed_occlusion = parse_f64(fields[16], line_index, "observed_occlusion")?;
        fixture.observed_pocket_overlap_score =
            parse_f64(fields[17], line_index, "observed_pocket_overlap_score")?;
        fixture.observed_rotamer_feasibility_score =
            parse_f64(fields[18], line_index, "observed_rotamer_feasibility_score")?;
        fixture.observed_bucket_occupancy =
            parse_usize(fields[19], line_index, "observed_bucket_occupancy")?;
        fixture.observed_seed_triplets =
            parse_usize(fields[20], line_index, "observed_seed_triplets")?;
        fixture.runtime_ms = parse_f64(fields[21], line_index, "runtime_ms")?;
        fixture.peak_memory_mb = parse_f64(fields[22], line_index, "peak_memory_mb")?;
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

    #[test]
    fn curated_fixture_tsv_imports_manifest_with_metrics() {
        let tsv = "\
fixture_id\tfixture_type\tsplit\tsource\tlicense_note\tligand_id\tpocket_id\tinteraction_labels\tresolution_angstrom\tinteraction_kind\tligand_class\tstructure_confidence_bin\tresidue_signature\tobserved_radius_error\tobserved_angle_error_deg\tobserved_sasa\tobserved_occlusion\tobserved_pocket_overlap_score\tobserved_rotamer_feasibility_score\tobserved_bucket_occupancy\tobserved_seed_triplets\truntime_ms\tpeak_memory_mb
pos1\tpositive_holo\ttrain\tcurated-positive\tinternal\tlig1\tpocket1\thydrogen_bond:D-H-Q\t1.8\thydrogen_bond\tgeneric\thigh\tD-H-Q\t0.2\t5.0\t0.5\t0.2\t0.4\t0.8\t2\t4\t2.0\t1.0
neg1\thard_negative_same_residues\tholdout\tcurated-negative\tinternal\tlig2\tpocket2\thydrogen_bond:D-H-Q\t2.0\thydrogen_bond\tgeneric\thigh\tD-H-Q\t3.0\t60.0\t0.1\t0.8\t0.1\t0.3\t1\t4\t2.0\t1.0";
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
        assert_eq!(manifest.fixtures[0].observed_seed_triplets, 4);
    }
}
