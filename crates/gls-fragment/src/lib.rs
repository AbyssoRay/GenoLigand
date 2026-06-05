use gls_types::{CandidateId, FragmentGrammar, FragmentHit};

pub fn find_fragment_hits(
    candidate_id: &CandidateId,
    sequence: &str,
    grammar: &FragmentGrammar,
) -> Vec<FragmentHit> {
    let mut hits = Vec::new();
    for fragment in &grammar.exact_fragments {
        hits.extend(find_exact(candidate_id, sequence, fragment, "exact"));
    }
    for category in &grammar.category_fragments {
        hits.extend(find_category(candidate_id, sequence, category));
    }
    hits
}

fn find_exact(
    candidate_id: &CandidateId,
    sequence: &str,
    fragment: &str,
    family: &str,
) -> Vec<FragmentHit> {
    let mut hits = Vec::new();
    let mut offset = 0;
    while let Some(found) = sequence[offset..].find(fragment) {
        let start = offset + found;
        let end = start + fragment.len();
        hits.push(FragmentHit {
            candidate_id: candidate_id.clone(),
            fragment: fragment.to_string(),
            family: family.to_string(),
            start,
            end,
            information_content: information_content(fragment),
            p_value: 1.0,
            q_value: 1.0,
        });
        offset = start + 1;
    }
    hits
}

fn find_category(
    candidate_id: &CandidateId,
    sequence: &str,
    category: &gls_types::CategoryFragment,
) -> Vec<FragmentHit> {
    let chars = sequence.chars().collect::<Vec<_>>();
    let mut hits = Vec::new();
    let mut start = 0;
    while start < chars.len() {
        if !category.residues.contains(&chars[start]) {
            start += 1;
            continue;
        }
        let mut end = start;
        while end < chars.len() && category.residues.contains(&chars[end]) {
            end += 1;
        }
        if end - start >= category.min_len {
            let fragment = chars[start..end].iter().collect::<String>();
            hits.push(FragmentHit {
                candidate_id: candidate_id.clone(),
                fragment: fragment.clone(),
                family: category.family.clone(),
                start,
                end,
                information_content: information_content(&fragment),
                p_value: 1.0,
                q_value: 1.0,
            });
        }
        start = end + 1;
    }
    hits
}

pub fn information_content(fragment: &str) -> f64 {
    if fragment.is_empty() {
        return 0.0;
    }
    let distinct = fragment
        .chars()
        .collect::<std::collections::BTreeSet<_>>()
        .len() as f64;
    (fragment.len() as f64) * (1.0 + distinct.log2())
}

#[cfg(test)]
mod tests {
    use super::*;
    use gls_types::{CategoryFragment, FragmentGrammar, Provenance};

    #[test]
    fn exact_and_category_hits_are_reported() {
        let grammar = FragmentGrammar {
            grammar_id: "g".to_string(),
            exact_fragments: vec!["WQH".to_string()],
            category_fragments: vec![CategoryFragment {
                family: "aromatic".to_string(),
                residues: vec!['F', 'W', 'Y'],
                min_len: 2,
            }],
            provenance: Provenance::new("test", "1", "p"),
            hash: "h".to_string(),
        };
        let hits = find_fragment_hits(&CandidateId::from("c1"), "AAWQHFFP", &grammar);

        assert!(hits.iter().any(|hit| hit.fragment == "WQH"));
        assert!(hits.iter().any(|hit| hit.family == "aromatic"));
    }
}
