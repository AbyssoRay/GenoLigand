use gls_stats::cooccurrence_score;
use gls_types::{
    CandidateId, FragmentHit, LocusClass, OrfCandidate, ShortCircuitDecision,
    ShortCircuitGateResult, machine_reason,
};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
pub struct ShortCircuitConfig {
    pub rule_version: String,
    pub min_len: usize,
    pub max_len: usize,
    pub max_low_complexity_score: f64,
    pub min_entropy_score: f64,
    pub max_repeat_fraction: f64,
    pub min_fragment_info_score: f64,
    pub max_cooccur_q_value: f64,
    pub max_plm_candidates_per_shard: usize,
    pub max_plm_candidates_per_family: usize,
    pub max_plm_candidates_per_locus_class: usize,
    pub simhash_hamming_threshold: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlmBudgetProfile {
    pub profile_name: String,
    pub max_plm_candidates_per_shard: Option<usize>,
    pub max_sequence_length: Option<usize>,
}

impl PlmBudgetProfile {
    pub fn require_bounded_plm_candidates(&self) -> Result<usize, String> {
        self.max_plm_candidates_per_shard
            .ok_or_else(|| format!("plm_budget_profile_requires_bound:{}", self.profile_name))
    }
}

impl ShortCircuitConfig {
    pub fn dev_cpu_smoke() -> Self {
        Self {
            rule_version: "gls-shortcircuit-v1.6".to_string(),
            min_len: 6,
            max_len: 256,
            max_low_complexity_score: 0.82,
            min_entropy_score: 1.8,
            max_repeat_fraction: 0.7,
            min_fragment_info_score: 4.0,
            max_cooccur_q_value: 0.2,
            max_plm_candidates_per_shard: 0,
            max_plm_candidates_per_family: 8,
            max_plm_candidates_per_locus_class: 8,
            simhash_hamming_threshold: 4,
        }
    }

    pub fn workstation() -> Self {
        Self {
            max_plm_candidates_per_shard: 512,
            max_plm_candidates_per_family: 128,
            max_plm_candidates_per_locus_class: 256,
            max_len: 768,
            ..Self::dev_cpu_smoke()
        }
    }

    pub fn apply_plm_budget_profile(&mut self, profile: &PlmBudgetProfile) -> Result<(), String> {
        self.max_plm_candidates_per_shard = profile.require_bounded_plm_candidates()?;
        if let Some(max_sequence_length) = profile.max_sequence_length {
            self.max_len = max_sequence_length;
        }
        Ok(())
    }
}

pub fn parse_plm_budget_profile(
    yaml: &str,
    profile_name: &str,
) -> Result<PlmBudgetProfile, String> {
    let mut in_root = false;
    let mut in_profile = false;
    let mut max_plm_candidates_per_shard = None;
    let mut max_sequence_length = None;

    for raw_line in yaml.lines() {
        let without_comment = raw_line.split('#').next().unwrap_or("").trim_end();
        if without_comment.trim().is_empty() {
            continue;
        }
        let indent = without_comment
            .chars()
            .take_while(|ch| ch.is_ascii_whitespace())
            .count();
        let line = without_comment.trim();
        if indent == 0 {
            in_root = line == "plm_budget:";
            in_profile = false;
            continue;
        }
        if !in_root {
            continue;
        }
        if indent == 2 && line.ends_with(':') {
            in_profile = line.trim_end_matches(':') == profile_name;
            continue;
        }
        if !in_profile || indent < 4 {
            continue;
        }
        let (key, value) = line
            .split_once(':')
            .ok_or_else(|| format!("plm_budget_bad_line:{line}"))?;
        let value = value.trim();
        match key.trim() {
            "max_plm_candidates_per_shard" => {
                max_plm_candidates_per_shard = parse_budget_value(value, key.trim())?;
            }
            "max_sequence_length" => {
                max_sequence_length = parse_budget_value(value, key.trim())?;
            }
            _ => {}
        }
    }

    if !yaml
        .lines()
        .any(|line| line.trim() == format!("{profile_name}:"))
    {
        return Err(format!("plm_budget_profile_not_found:{profile_name}"));
    }
    Ok(PlmBudgetProfile {
        profile_name: profile_name.to_string(),
        max_plm_candidates_per_shard,
        max_sequence_length,
    })
}

fn parse_budget_value(value: &str, key: &str) -> Result<Option<usize>, String> {
    if value == "configurable" {
        return Ok(None);
    }
    value
        .parse::<usize>()
        .map(Some)
        .map_err(|err| format!("plm_budget_bad_value:{key}:{err}"))
}

#[derive(Clone, Debug, PartialEq)]
struct Preliminary {
    result: ShortCircuitGateResult,
    shard_id: String,
    family: String,
    score: f64,
    simhash: u64,
}

pub fn evaluate_short_circuit(
    candidates: &[OrfCandidate],
    hits: &[FragmentHit],
    config: &ShortCircuitConfig,
) -> Vec<ShortCircuitGateResult> {
    let hits_by_candidate = index_hits(hits);
    let mut seen_sequences = BTreeMap::<String, CandidateId>::new();
    let mut cluster_representatives = Vec::<(String, u64)>::new();
    let mut preliminary = Vec::with_capacity(candidates.len());

    for candidate in candidates {
        let candidate_hits = hits_by_candidate
            .get(&candidate.candidate_id)
            .map_or(&[][..], Vec::as_slice);
        let mut result = base_result(candidate, config);
        let mut score = 0.0;

        if let Some(reason) = structural_reject_reason(candidate, config) {
            set_decision(&mut result, ShortCircuitDecision::Reject, reason);
        } else {
            result.low_complexity_score = low_complexity_score(&candidate.sequence);
            result.entropy_score = amino_acid_entropy(&candidate.sequence);
            let repeat_fraction = repeat_fraction(&candidate.sequence);
            if result.low_complexity_score > config.max_low_complexity_score
                || result.entropy_score < config.min_entropy_score
                || repeat_fraction > config.max_repeat_fraction
            {
                set_decision(
                    &mut result,
                    ShortCircuitDecision::HoldNoPlm,
                    "low_complexity_or_repeat_entropy_gate",
                );
            } else {
                result.fragment_info_score = cooccurrence_score(candidate_hits);
                result.cooccur_q_value = min_q_value(candidate_hits);
                score = result.fragment_info_score
                    + result.context_priority_score
                    + result.entropy_score
                    - result.low_complexity_score;
                if result.fragment_info_score < config.min_fragment_info_score
                    || result.cooccur_q_value.unwrap_or(1.0) > config.max_cooccur_q_value
                {
                    if candidate.locus_class == LocusClass::DarkGenome {
                        set_decision(
                            &mut result,
                            ShortCircuitDecision::ForceKeepDarkCandidate,
                            "dark_candidate_held_without_annotation_deletion",
                        );
                    } else {
                        set_decision(
                            &mut result,
                            ShortCircuitDecision::HoldNoPlm,
                            "insufficient_fragment_significance",
                        );
                    }
                } else if let Some(canonical) = seen_sequences.get(&result.sequence_hash) {
                    result.canonical_cluster_id = Some(canonical.to_string());
                    set_decision(
                        &mut result,
                        ShortCircuitDecision::HoldNoPlm,
                        "duplicate_sequence_inherits_canonical_embedding",
                    );
                } else {
                    seen_sequences
                        .insert(result.sequence_hash.clone(), candidate.candidate_id.clone());
                    let simhash = sequence_simhash(&candidate.sequence);
                    if let Some((cluster_id, _)) =
                        cluster_representatives
                            .iter()
                            .find(|(_, representative_hash)| {
                                (representative_hash ^ simhash).count_ones()
                                    <= config.simhash_hamming_threshold
                            })
                    {
                        result.canonical_cluster_id = Some(cluster_id.clone());
                        set_decision(
                            &mut result,
                            ShortCircuitDecision::HoldNoPlm,
                            "near_duplicate_cluster_member",
                        );
                    } else {
                        result.canonical_cluster_id = Some(candidate.candidate_id.to_string());
                        cluster_representatives.push((candidate.candidate_id.to_string(), simhash));
                        result.decision = ShortCircuitDecision::PassPlm;
                    }
                }
            }
        }

        preliminary.push(Preliminary {
            result,
            shard_id: candidate.shard_id.clone(),
            family: candidate.fragment_family.clone(),
            score,
            simhash: sequence_simhash(&candidate.sequence),
        });
    }

    apply_budget(preliminary, config)
}

fn index_hits(hits: &[FragmentHit]) -> BTreeMap<CandidateId, Vec<FragmentHit>> {
    let mut grouped = BTreeMap::<CandidateId, Vec<FragmentHit>>::new();
    for hit in hits {
        grouped
            .entry(hit.candidate_id.clone())
            .or_default()
            .push(hit.clone());
    }
    grouped
}

fn base_result(candidate: &OrfCandidate, config: &ShortCircuitConfig) -> ShortCircuitGateResult {
    ShortCircuitGateResult {
        candidate_id: candidate.candidate_id.clone(),
        decision: ShortCircuitDecision::HoldNoPlm,
        rule_version: config.rule_version.clone(),
        sequence_hash: candidate.sequence_hash(),
        canonical_cluster_id: None,
        aa_len: candidate.sequence.len(),
        low_complexity_score: 0.0,
        entropy_score: 0.0,
        fragment_info_score: 0.0,
        cooccur_q_value: None,
        locus_class: candidate.locus_class.clone(),
        context_priority_score: if candidate.locus_class == LocusClass::DarkGenome {
            0.25
        } else {
            0.0
        },
        budget_rank: 0,
        pass_plm: false,
        failure_reason: None,
    }
}

fn structural_reject_reason(
    candidate: &OrfCandidate,
    config: &ShortCircuitConfig,
) -> Option<&'static str> {
    if candidate.sequence.len() < config.min_len {
        return Some("length_below_minimum");
    }
    if candidate.sequence.len() > config.max_len {
        return Some("length_above_plm_budget");
    }
    if !candidate.stop_to_stop {
        return Some("not_stop_to_stop_orf");
    }
    if candidate
        .sequence
        .chars()
        .any(|residue| !is_valid_residue(residue))
    {
        return Some("invalid_amino_acid_residue");
    }
    None
}

fn set_decision(result: &mut ShortCircuitGateResult, decision: ShortCircuitDecision, reason: &str) {
    result.decision = decision;
    result.pass_plm = false;
    result.failure_reason = Some(machine_reason(reason));
}

fn apply_budget(
    mut preliminary: Vec<Preliminary>,
    config: &ShortCircuitConfig,
) -> Vec<ShortCircuitGateResult> {
    preliminary.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then_with(|| a.simhash.cmp(&b.simhash))
            .then_with(|| a.result.candidate_id.cmp(&b.result.candidate_id))
    });

    let mut shard_counts = BTreeMap::<String, usize>::new();
    let mut family_counts = BTreeMap::<String, usize>::new();
    let mut locus_counts = BTreeMap::<String, usize>::new();

    for item in &mut preliminary {
        if item.result.decision != ShortCircuitDecision::PassPlm {
            continue;
        }
        let shard_count = shard_counts.entry(item.shard_id.clone()).or_default();
        let family_count = family_counts.entry(item.family.clone()).or_default();
        let locus_key = item.result.locus_class.as_str().to_string();
        let locus_count = locus_counts.entry(locus_key).or_default();
        if *shard_count >= config.max_plm_candidates_per_shard
            || *family_count >= config.max_plm_candidates_per_family
            || *locus_count >= config.max_plm_candidates_per_locus_class
        {
            set_decision(
                &mut item.result,
                ShortCircuitDecision::HoldNoPlm,
                "plm_budget_exhausted",
            );
        } else {
            *shard_count += 1;
            *family_count += 1;
            *locus_count += 1;
            item.result.budget_rank = *shard_count as u32;
            item.result.pass_plm = true;
            item.result.failure_reason = None;
        }
    }

    preliminary.sort_by(|a, b| a.result.candidate_id.cmp(&b.result.candidate_id));
    preliminary.into_iter().map(|item| item.result).collect()
}

pub fn low_complexity_score(sequence: &str) -> f64 {
    if sequence.is_empty() {
        return 1.0;
    }
    let mut counts = BTreeMap::<char, usize>::new();
    for residue in sequence.chars() {
        *counts.entry(residue).or_default() += 1;
    }
    counts.values().copied().max().unwrap_or(0) as f64 / sequence.len() as f64
}

pub fn amino_acid_entropy(sequence: &str) -> f64 {
    if sequence.is_empty() {
        return 0.0;
    }
    let mut counts = BTreeMap::<char, usize>::new();
    for residue in sequence.chars() {
        *counts.entry(residue).or_default() += 1;
    }
    counts
        .values()
        .map(|count| {
            let p = *count as f64 / sequence.len() as f64;
            -p * p.log2()
        })
        .sum()
}

pub fn repeat_fraction(sequence: &str) -> f64 {
    if sequence.len() < 2 {
        return 0.0;
    }
    let residues = sequence.chars().collect::<Vec<_>>();
    let repeated = residues
        .windows(2)
        .filter(|window| window[0] == window[1])
        .count();
    repeated as f64 / (sequence.len() - 1) as f64
}

fn min_q_value(hits: &[FragmentHit]) -> Option<f64> {
    hits.iter().map(|hit| hit.q_value).min_by(f64::total_cmp)
}

fn sequence_simhash(sequence: &str) -> u64 {
    let mut accum = [0i32; 64];
    let chars = sequence.chars().collect::<Vec<_>>();
    if chars.len() < 3 {
        return gls_types::stable_hash(sequence)
            .chars()
            .fold(0u64, |acc, ch| {
                acc.wrapping_mul(16) + ch.to_digit(16).unwrap_or(0) as u64
            });
    }
    for kmer in chars.windows(3) {
        let token = kmer.iter().collect::<String>();
        let hash = gls_types::stable_hash(&token)
            .chars()
            .fold(0u64, |acc, ch| {
                acc.wrapping_mul(16) + ch.to_digit(16).unwrap_or(0) as u64
            });
        for (bit, value) in accum.iter_mut().enumerate() {
            if (hash >> bit) & 1 == 1 {
                *value += 1;
            } else {
                *value -= 1;
            }
        }
    }
    accum.iter().enumerate().fold(0u64, |hash, (bit, value)| {
        if *value >= 0 {
            hash | (1u64 << bit)
        } else {
            hash
        }
    })
}

fn is_valid_residue(residue: char) -> bool {
    matches!(
        residue,
        'A' | 'C'
            | 'D'
            | 'E'
            | 'F'
            | 'G'
            | 'H'
            | 'I'
            | 'K'
            | 'L'
            | 'M'
            | 'N'
            | 'P'
            | 'Q'
            | 'R'
            | 'S'
            | 'T'
            | 'V'
            | 'W'
            | 'Y'
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use gls_types::{CandidateGrade, Provenance};

    fn candidate(id: &str, sequence: &str) -> OrfCandidate {
        OrfCandidate {
            candidate_id: CandidateId::from(id),
            shard_id: "shard1".to_string(),
            sequence: sequence.to_string(),
            nucleotide_range: (0, sequence.len() * 3),
            frame: 1,
            locus_class: LocusClass::Unknown,
            stop_to_stop: true,
            fragment_family: "fam".to_string(),
            provenance: Provenance::new("test", "1", id),
            grade: CandidateGrade::G2,
        }
    }

    fn hit(id: &str) -> FragmentHit {
        FragmentHit {
            candidate_id: CandidateId::from(id),
            fragment: "WQH".to_string(),
            family: "fam".to_string(),
            start: 0,
            end: 3,
            information_content: 8.0,
            p_value: 0.001,
            q_value: 0.01,
        }
    }

    #[test]
    fn low_complexity_candidate_never_passes_plm() {
        let mut config = ShortCircuitConfig::workstation();
        config.max_plm_candidates_per_shard = 4;
        let results =
            evaluate_short_circuit(&[candidate("c1", "AAAAAAAAAAAA")], &[hit("c1")], &config);

        assert!(!results[0].pass_plm);
        assert_eq!(
            results[0].failure_reason.as_deref(),
            Some("low_complexity_or_repeat_entropy_gate")
        );
    }

    #[test]
    fn duplicate_sequence_only_allows_canonical_representative() {
        let mut config = ShortCircuitConfig::workstation();
        config.max_plm_candidates_per_shard = 4;
        let candidates = vec![candidate("c1", "WQHACDEFG"), candidate("c2", "WQHACDEFG")];
        let hits = vec![hit("c1"), hit("c2")];
        let results = evaluate_short_circuit(&candidates, &hits, &config);

        assert_eq!(results.iter().filter(|result| result.pass_plm).count(), 1);
        assert!(results.iter().any(|result| result.failure_reason.as_deref()
            == Some("duplicate_sequence_inherits_canonical_embedding")));
    }

    #[test]
    fn plm_budget_zero_is_stable_and_machine_readable() {
        let config = ShortCircuitConfig::dev_cpu_smoke();
        let results =
            evaluate_short_circuit(&[candidate("c1", "WQHACDEFG")], &[hit("c1")], &config);

        assert!(!results[0].pass_plm);
        assert_eq!(
            results[0].failure_reason.as_deref(),
            Some("plm_budget_exhausted")
        );
    }

    #[test]
    fn plm_budget_yaml_profile_updates_runtime_config() {
        let yaml = "\
plm_budget:
  dev_gpu_lowmem:
    max_plm_candidates_per_shard: 32
    max_sequence_length: 256
  hpc:
    max_plm_candidates_per_shard: configurable
";
        let profile = parse_plm_budget_profile(yaml, "dev_gpu_lowmem").expect("profile");
        let mut config = ShortCircuitConfig::workstation();
        config
            .apply_plm_budget_profile(&profile)
            .expect("apply profile");

        assert_eq!(config.max_plm_candidates_per_shard, 32);
        assert_eq!(config.max_len, 256);
        assert_eq!(
            parse_plm_budget_profile(yaml, "hpc")
                .expect("hpc")
                .require_bounded_plm_candidates(),
            Err("plm_budget_profile_requires_bound:hpc".to_string())
        );
    }

    #[test]
    fn dark_candidates_are_held_not_deleted_when_annotation_is_absent() {
        let mut dark = candidate("dark1", "ACDEFGHIK");
        dark.locus_class = LocusClass::DarkGenome;
        let config = ShortCircuitConfig::workstation();
        let results = evaluate_short_circuit(&[dark], &[], &config);

        assert_eq!(
            results[0].decision,
            ShortCircuitDecision::ForceKeepDarkCandidate
        );
        assert!(!results[0].pass_plm);
    }

    #[test]
    fn one_hundred_thousand_candidates_are_budgeted_on_cpu() {
        let mut candidates = Vec::with_capacity(100_000);
        let mut hits = Vec::with_capacity(100_000);
        for idx in 0..100_000 {
            let sequence = format!("WQHACDEFG{idx:05}");
            let mut c = candidate(&format!("c{idx:05}"), &sequence.replace('0', "A"));
            c.sequence = "WQHACDEFGKLMNP".to_string();
            c.sequence.push(char::from(b'A' + (idx % 20) as u8 % 20));
            c.candidate_id = CandidateId::new(format!("c{idx:05}"));
            candidates.push(c);
            hits.push(hit(&format!("c{idx:05}")));
        }
        let mut config = ShortCircuitConfig::workstation();
        config.max_plm_candidates_per_shard = 32;
        let results = evaluate_short_circuit(&candidates, &hits, &config);

        assert_eq!(results.len(), 100_000);
        assert!(results.iter().filter(|result| result.pass_plm).count() <= 32);
    }
}
