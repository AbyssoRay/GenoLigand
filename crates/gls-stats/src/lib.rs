use gls_types::FragmentHit;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct BackgroundModel {
    pub residue_probabilities: BTreeMap<char, f64>,
}

impl BackgroundModel {
    pub fn from_sequences<'a>(sequences: impl IntoIterator<Item = &'a str>) -> Self {
        let mut counts = BTreeMap::<char, usize>::new();
        let mut total = 0usize;
        for sequence in sequences {
            for residue in sequence.chars().filter(|ch| ch.is_ascii_uppercase()) {
                *counts.entry(residue).or_default() += 1;
                total += 1;
            }
        }
        if total == 0 {
            return Self::default();
        }
        let residue_probabilities = counts
            .into_iter()
            .map(|(residue, count)| (residue, count as f64 / total as f64))
            .collect();
        Self {
            residue_probabilities,
        }
    }

    pub fn fragment_probability(&self, fragment: &str) -> f64 {
        fragment
            .chars()
            .map(|residue| {
                self.residue_probabilities
                    .get(&residue)
                    .copied()
                    .unwrap_or(1.0 / 20.0)
            })
            .product::<f64>()
    }
}

pub fn assign_p_values(hits: &mut [FragmentHit], background: &BackgroundModel) {
    for hit in hits {
        let probability = background.fragment_probability(&hit.fragment);
        let length_penalty = 1.0 / (hit.fragment.len().max(1) as f64);
        hit.p_value = (probability / hit.information_content.max(length_penalty)).clamp(0.0, 1.0);
    }
}

pub fn benjamini_hochberg(p_values: &[f64]) -> Vec<f64> {
    let n = p_values.len();
    if n == 0 {
        return Vec::new();
    }
    let mut indexed = p_values
        .iter()
        .copied()
        .enumerate()
        .collect::<Vec<(usize, f64)>>();
    indexed.sort_by(|a, b| a.1.total_cmp(&b.1));

    let mut adjusted = vec![1.0; n];
    let mut running_min = 1.0;
    for (rank_from_zero, (original_index, p_value)) in indexed.into_iter().enumerate().rev() {
        let rank = rank_from_zero + 1;
        let q_value = (p_value * n as f64 / rank as f64).min(running_min);
        running_min = q_value;
        adjusted[original_index] = q_value.clamp(0.0, 1.0);
    }
    adjusted
}

pub fn assign_q_values(hits: &mut [FragmentHit]) {
    let q_values = benjamini_hochberg(&hits.iter().map(|hit| hit.p_value).collect::<Vec<_>>());
    for (hit, q_value) in hits.iter_mut().zip(q_values) {
        hit.q_value = q_value;
    }
}

pub fn cooccurrence_score(hits: &[FragmentHit]) -> f64 {
    let families = hits
        .iter()
        .map(|hit| hit.family.as_str())
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    let info = hits.iter().map(|hit| hit.information_content).sum::<f64>();
    info * families.max(1) as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn high_frequency_fragments_receive_weaker_scores() {
        let background = BackgroundModel::from_sequences(["AAAAAAAAAA", "AAAAAWQH"]);
        let rare = background.fragment_probability("WQH");
        let common = background.fragment_probability("AAA");

        assert!(rare < common);
    }

    #[test]
    fn bh_q_values_are_monotonic_by_rank() {
        let q = benjamini_hochberg(&[0.01, 0.02, 0.50, 0.03]);
        assert!(q[0] <= q[2]);
        assert!(q.iter().all(|value| (0.0..=1.0).contains(value)));
    }
}
