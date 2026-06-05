use gls_artifacts::provenance_for_tool;
use gls_types::{
    CategoryFragment, FragmentGrammar, InteractionConstraint, InteractionConstraintGraph,
    InteractionKind, Point3Data, stable_hash,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LigandSpec {
    pub ligand_id: String,
    pub ligand_class: String,
    pub exact_fragments: Vec<String>,
    pub category_fragments: Vec<CategoryFragment>,
    pub residues: Vec<char>,
}

impl LigandSpec {
    pub fn from_yaml(input: &str) -> Result<Self, String> {
        let mut ligand_id = String::from("ligand");
        let mut ligand_class = String::from("generic");
        let mut exact_fragments = Vec::new();
        let mut category_fragments = Vec::new();
        let mut residues = Vec::new();

        for raw in input.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(value) = line.strip_prefix("ligand_id:") {
                ligand_id = value.trim().trim_matches('"').to_string();
            } else if let Some(value) = line.strip_prefix("ligand_class:") {
                ligand_class = value.trim().trim_matches('"').to_string();
            } else if let Some(value) = line.strip_prefix("- exact:") {
                let fragment = value.trim().trim_matches('"').to_ascii_uppercase();
                if !fragment.is_empty() {
                    exact_fragments.push(fragment);
                }
            } else if let Some(value) = line.strip_prefix("- residue:") {
                if let Some(residue) = value.trim().chars().next() {
                    residues.push(residue.to_ascii_uppercase());
                }
            } else if let Some(value) = line.strip_prefix("- category:") {
                category_fragments.push(parse_category(value.trim())?);
            }
        }

        if exact_fragments.is_empty() && residues.is_empty() && category_fragments.is_empty() {
            return Err("ligand_yaml_has_no_interaction_fragments".to_string());
        }
        if residues.is_empty() {
            residues.extend(exact_fragments.iter().flat_map(|fragment| fragment.chars()));
        }
        residues.sort_unstable();
        residues.dedup();

        Ok(Self {
            ligand_id,
            ligand_class,
            exact_fragments,
            category_fragments,
            residues,
        })
    }
}

pub fn build_ligand_kernel(spec: &LigandSpec) -> (InteractionConstraintGraph, FragmentGrammar) {
    let provenance = provenance_for_tool(
        "gls-ligand",
        "v1.6",
        &[
            ("ligand_id", spec.ligand_id.clone()),
            ("ligand_class", spec.ligand_class.clone()),
        ],
    );

    let constraints = spec
        .residues
        .iter()
        .enumerate()
        .map(|(idx, residue)| InteractionConstraint {
            id: format!("icg_{}_{}", spec.ligand_id, idx + 1),
            kind: infer_interaction_kind(*residue),
            residue: *residue,
            point: Point3Data::new(idx as f64 * 1.5, 0.0, 0.0),
            vector: Point3Data::new(1.0, 0.0, 0.0),
            required: true,
        })
        .collect::<Vec<_>>();

    let graph_payload = format!("{:?}{:?}", spec.residues, spec.exact_fragments);
    let graph = InteractionConstraintGraph {
        graph_id: format!("icg_{}", spec.ligand_id),
        constraints,
        provenance: provenance.clone(),
        hash: stable_hash(&graph_payload),
    };

    let grammar_payload = format!("{:?}{:?}", spec.exact_fragments, spec.category_fragments);
    let grammar = FragmentGrammar {
        grammar_id: format!("grammar_{}", spec.ligand_id),
        exact_fragments: spec.exact_fragments.clone(),
        category_fragments: spec.category_fragments.clone(),
        provenance,
        hash: stable_hash(&grammar_payload),
    };

    (graph, grammar)
}

fn parse_category(value: &str) -> Result<CategoryFragment, String> {
    let mut parts = value.split(':');
    let family = parts
        .next()
        .filter(|part| !part.is_empty())
        .ok_or_else(|| "category_fragment_missing_family".to_string())?
        .to_string();
    let residues = parts
        .next()
        .filter(|part| !part.is_empty())
        .ok_or_else(|| "category_fragment_missing_residues".to_string())?
        .chars()
        .map(|ch| ch.to_ascii_uppercase())
        .collect::<Vec<_>>();
    let min_len = parts
        .next()
        .and_then(|part| part.parse::<usize>().ok())
        .unwrap_or(1);
    Ok(CategoryFragment {
        family,
        residues,
        min_len,
    })
}

fn infer_interaction_kind(residue: char) -> InteractionKind {
    match residue {
        'D' | 'E' | 'K' | 'R' | 'H' | 'S' | 'T' | 'N' | 'Q' => InteractionKind::HydrogenBond,
        'F' | 'W' | 'Y' => InteractionKind::AromaticStacking,
        'C' | 'M' | 'L' | 'I' | 'V' | 'A' | 'P' => InteractionKind::Hydrophobic,
        _ => InteractionKind::Generic,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ligand_yaml_generates_stable_kernel_artifacts() {
        let yaml = r#"
ligand_id: demo
ligand_class: alkaloid
fragments:
  - exact: WQH
  - residue: D
  - category: aromatic:FWY:2
"#;
        let spec = LigandSpec::from_yaml(yaml).expect("valid ligand yaml");
        let (icg_a, grammar_a) = build_ligand_kernel(&spec);
        let (icg_b, grammar_b) = build_ligand_kernel(&spec);

        assert_eq!(icg_a.hash, icg_b.hash);
        assert_eq!(grammar_a.hash, grammar_b.hash);
        assert!(
            icg_a
                .constraints
                .iter()
                .any(|constraint| constraint.residue == 'D')
        );
        assert!(grammar_a.provenance.artifact_hash.len() == 16);
    }
}
