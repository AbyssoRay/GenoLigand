use gls_types::{FoldabilityGateResult, OrfCandidate, ShortCircuitGateResult};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PlmModelSpec {
    pub model_id: String,
    pub tokenizer_id: String,
    pub weights_hash: String,
}

impl PlmModelSpec {
    pub fn new(
        model_id: impl Into<String>,
        tokenizer_id: impl Into<String>,
        weights_hash: impl Into<String>,
    ) -> Self {
        Self {
            model_id: model_id.into(),
            tokenizer_id: tokenizer_id.into(),
            weights_hash: weights_hash.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlmGateConfig {
    pub primary_model: PlmModelSpec,
    pub upgraded_model: Option<PlmModelSpec>,
    pub max_plm_candidates_per_shard: usize,
    pub max_sequence_length: usize,
    pub length_bucket_width: usize,
    pub max_concurrent_gpu_batches: usize,
    pub upgrade_fragment_info_below: u32,
}

impl PlmGateConfig {
    pub fn dev_gpu_lowmem() -> Self {
        Self {
            primary_model: PlmModelSpec::new("esm2-small-dev", "esm-aa-v1", "weights-dev"),
            upgraded_model: Some(PlmModelSpec::new(
                "esm2-boundary-dev",
                "esm-aa-v1",
                "weights-boundary-dev",
            )),
            max_plm_candidates_per_shard: 32,
            max_sequence_length: 256,
            length_bucket_width: 64,
            max_concurrent_gpu_batches: 1,
            upgrade_fragment_info_below: 10,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct EmbeddingCache {
    vectors: BTreeMap<String, Vec<f32>>,
}

impl EmbeddingCache {
    pub fn contains(&self, key: &str) -> bool {
        self.vectors.contains_key(key)
    }

    pub fn insert(&mut self, key: String, embedding: Vec<f32>) {
        self.vectors.insert(key, embedding);
    }

    pub fn len(&self) -> usize {
        self.vectors.len()
    }

    pub fn is_empty(&self) -> bool {
        self.vectors.is_empty()
    }
}

pub trait PersistentPlmWorker {
    fn embed_batch(
        &mut self,
        model: &PlmModelSpec,
        bucket_len: usize,
        sequences: &[(String, String)],
    ) -> Vec<Vec<f32>>;
    fn invocation_count(&self) -> usize;
}

#[derive(Clone, Debug, Default)]
pub struct MockPlmWorker {
    invocations: usize,
    embedded_sequences: usize,
    model_invocations: Vec<String>,
}

impl MockPlmWorker {
    pub fn embedded_sequences(&self) -> usize {
        self.embedded_sequences
    }

    pub fn model_invocations(&self) -> &[String] {
        &self.model_invocations
    }
}

impl PersistentPlmWorker for MockPlmWorker {
    fn embed_batch(
        &mut self,
        model: &PlmModelSpec,
        bucket_len: usize,
        sequences: &[(String, String)],
    ) -> Vec<Vec<f32>> {
        self.invocations += 1;
        self.embedded_sequences += sequences.len();
        self.model_invocations.push(model.model_id.clone());
        sequences
            .iter()
            .map(|(_, sequence)| mock_embedding(sequence, bucket_len))
            .collect()
    }

    fn invocation_count(&self) -> usize {
        self.invocations
    }
}

pub fn cache_key(
    sequence_hash: &str,
    model_id: &str,
    tokenizer_id: &str,
    weights_hash: &str,
) -> String {
    format!("{sequence_hash}:{model_id}:{tokenizer_id}:{weights_hash}")
}

pub fn run_plm_gate<W: PersistentPlmWorker>(
    candidates: &[OrfCandidate],
    gates: &[ShortCircuitGateResult],
    config: &PlmGateConfig,
    cache: &mut EmbeddingCache,
    worker: &mut W,
) -> Result<Vec<FoldabilityGateResult>, String> {
    if gates.iter().any(|gate| !gate.pass_plm) {
        return Err("gls_plm_gate_input_contains_pass_plm_false".to_string());
    }
    if config.max_concurrent_gpu_batches == 0 {
        return Err("gpu_semaphore_has_zero_permits".to_string());
    }

    let gate_by_id = gates
        .iter()
        .map(|gate| (gate.candidate_id.clone(), gate))
        .collect::<BTreeMap<_, _>>();
    let mut accepted = candidates
        .iter()
        .filter(|candidate| gate_by_id.contains_key(&candidate.candidate_id))
        .collect::<Vec<_>>();
    accepted.sort_by(|a, b| {
        length_bucket(a.sequence.len(), config.length_bucket_width)
            .cmp(&length_bucket(b.sequence.len(), config.length_bucket_width))
            .then_with(|| a.candidate_id.cmp(&b.candidate_id))
    });

    if accepted.len() > config.max_plm_candidates_per_shard {
        return Err("plm_budget_exhausted_before_worker_invocation".to_string());
    }
    if let Some(candidate) = accepted
        .iter()
        .find(|candidate| candidate.sequence.len() > config.max_sequence_length)
    {
        return Err(format!(
            "sequence_length_exceeds_plm_budget:{}",
            candidate.candidate_id
        ));
    }

    let selected_model_by_candidate = accepted
        .iter()
        .map(|candidate| {
            let gate = gate_by_id[&candidate.candidate_id];
            (
                candidate.candidate_id.clone(),
                select_model_for_gate(gate, config).clone(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut unique_keys = BTreeSet::<String>::new();
    let mut preexisting_cache_keys = BTreeSet::<String>::new();
    let mut batches = BTreeMap::<(PlmModelSpec, usize), Vec<(String, String)>>::new();
    for candidate in &accepted {
        let sequence_hash = gate_by_id[&candidate.candidate_id].sequence_hash.as_str();
        let model = &selected_model_by_candidate[&candidate.candidate_id];
        let key = cache_key(
            sequence_hash,
            &model.model_id,
            &model.tokenizer_id,
            &model.weights_hash,
        );
        if cache.contains(&key) {
            preexisting_cache_keys.insert(key);
            continue;
        }
        if !unique_keys.insert(key.clone()) {
            continue;
        }
        let bucket_len = length_bucket(candidate.sequence.len(), config.length_bucket_width);
        batches
            .entry((model.clone(), bucket_len))
            .or_default()
            .push((key, candidate.sequence.clone()));
    }

    let mut gpu_semaphore = GpuSemaphore::new(config.max_concurrent_gpu_batches);
    for ((model, bucket_len), sequences) in batches {
        gpu_semaphore.acquire()?;
        let embeddings = worker.embed_batch(&model, bucket_len, &sequences);
        for ((key, _), embedding) in sequences.into_iter().zip(embeddings) {
            cache.insert(key, embedding);
        }
        gpu_semaphore.release();
    }

    let results = accepted
        .into_iter()
        .map(|candidate| {
            let gate = gate_by_id[&candidate.candidate_id];
            let model = &selected_model_by_candidate[&candidate.candidate_id];
            let key = cache_key(
                &gate.sequence_hash,
                &model.model_id,
                &model.tokenizer_id,
                &model.weights_hash,
            );
            FoldabilityGateResult {
                candidate_id: candidate.candidate_id.clone(),
                sequence_hash: gate.sequence_hash.clone(),
                model_id: model.model_id.clone(),
                tokenizer_id: model.tokenizer_id.clone(),
                weights_hash: model.weights_hash.clone(),
                cache_key: key.clone(),
                embedding_from_cache: preexisting_cache_keys.contains(&key) || cache.contains(&key),
                pass_structure: true,
                failure_reason: None,
                bucket_len: length_bucket(candidate.sequence.len(), config.length_bucket_width),
            }
        })
        .collect();

    Ok(results)
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct GpuSemaphore {
    permits: usize,
    in_use: usize,
}

impl GpuSemaphore {
    fn new(permits: usize) -> Self {
        Self { permits, in_use: 0 }
    }

    fn acquire(&mut self) -> Result<(), String> {
        if self.in_use >= self.permits {
            return Err("gpu_semaphore_permit_exhausted".to_string());
        }
        self.in_use += 1;
        Ok(())
    }

    fn release(&mut self) {
        self.in_use = self.in_use.saturating_sub(1);
    }
}

fn select_model_for_gate<'a>(
    gate: &ShortCircuitGateResult,
    config: &'a PlmGateConfig,
) -> &'a PlmModelSpec {
    let is_boundary_candidate = gate.fragment_info_score
        < f64::from(config.upgrade_fragment_info_below)
        || gate.cooccur_q_value.unwrap_or(0.0) > 0.05;
    if is_boundary_candidate {
        config
            .upgraded_model
            .as_ref()
            .unwrap_or(&config.primary_model)
    } else {
        &config.primary_model
    }
}

fn length_bucket(length: usize, bucket_width: usize) -> usize {
    let width = bucket_width.max(1);
    length.div_ceil(width) * width
}

fn mock_embedding(sequence: &str, bucket_len: usize) -> Vec<f32> {
    let len = sequence.len().max(1) as f32;
    let hydrophobic = sequence
        .chars()
        .filter(|residue| matches!(residue, 'A' | 'V' | 'I' | 'L' | 'M' | 'F' | 'W' | 'Y'))
        .count() as f32
        / len;
    vec![
        len / bucket_len.max(1) as f32,
        hydrophobic,
        1.0 - hydrophobic,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use gls_types::{
        CandidateGrade, CandidateId, LocusClass, Provenance, ShortCircuitDecision,
        ShortCircuitGateResult,
    };

    fn candidate(id: &str, sequence: &str) -> OrfCandidate {
        OrfCandidate {
            candidate_id: CandidateId::from(id),
            shard_id: "s".to_string(),
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

    fn gate(candidate: &OrfCandidate, pass_plm: bool) -> ShortCircuitGateResult {
        ShortCircuitGateResult {
            candidate_id: candidate.candidate_id.clone(),
            decision: if pass_plm {
                ShortCircuitDecision::PassPlm
            } else {
                ShortCircuitDecision::HoldNoPlm
            },
            rule_version: "v1.6".to_string(),
            sequence_hash: candidate.sequence_hash(),
            canonical_cluster_id: Some(candidate.candidate_id.to_string()),
            aa_len: candidate.sequence.len(),
            low_complexity_score: 0.0,
            entropy_score: 2.0,
            fragment_info_score: 8.0,
            cooccur_q_value: Some(0.01),
            locus_class: LocusClass::Unknown,
            context_priority_score: 0.0,
            budget_rank: 1,
            pass_plm,
            failure_reason: None,
        }
    }

    #[test]
    fn rejected_or_hold_candidates_cannot_enter_plm_gate() {
        let c = candidate("c1", "WQHACDEFG");
        let err = run_plm_gate(
            std::slice::from_ref(&c),
            &[gate(&c, false)],
            &PlmGateConfig::dev_gpu_lowmem(),
            &mut EmbeddingCache::default(),
            &mut MockPlmWorker::default(),
        )
        .expect_err("pass_plm=false must fail");

        assert_eq!(err, "gls_plm_gate_input_contains_pass_plm_false");
    }

    #[test]
    fn duplicate_sequence_uses_one_embedding_cache_entry() {
        let c1 = candidate("c1", "WQHACDEFG");
        let c2 = candidate("c2", "WQHACDEFG");
        let mut cache = EmbeddingCache::default();
        let mut worker = MockPlmWorker::default();
        let results = run_plm_gate(
            &[c1.clone(), c2.clone()],
            &[gate(&c1, true), gate(&c2, true)],
            &PlmGateConfig::dev_gpu_lowmem(),
            &mut cache,
            &mut worker,
        )
        .expect("plm gate succeeds");

        assert_eq!(results.len(), 2);
        assert_eq!(cache.len(), 1);
        assert_eq!(worker.embedded_sequences(), 1);
    }

    #[test]
    fn budget_exhaustion_is_stable_failure_before_worker() {
        let c1 = candidate("c1", "WQHACDEFG");
        let c2 = candidate("c2", "WQHACDEFGK");
        let mut config = PlmGateConfig::dev_gpu_lowmem();
        config.max_plm_candidates_per_shard = 1;
        let mut worker = MockPlmWorker::default();
        let err = run_plm_gate(
            &[c1.clone(), c2.clone()],
            &[gate(&c1, true), gate(&c2, true)],
            &config,
            &mut EmbeddingCache::default(),
            &mut worker,
        )
        .expect_err("budget should fail");

        assert_eq!(err, "plm_budget_exhausted_before_worker_invocation");
        assert_eq!(worker.invocation_count(), 0);
    }

    #[test]
    fn gpu_semaphore_zero_permits_fails_before_worker() {
        let c = candidate("c1", "WQHACDEFG");
        let mut config = PlmGateConfig::dev_gpu_lowmem();
        config.max_concurrent_gpu_batches = 0;
        let mut worker = MockPlmWorker::default();
        let err = run_plm_gate(
            std::slice::from_ref(&c),
            &[gate(&c, true)],
            &config,
            &mut EmbeddingCache::default(),
            &mut worker,
        )
        .expect_err("zero GPU semaphore permits should fail");

        assert_eq!(err, "gpu_semaphore_has_zero_permits");
        assert_eq!(worker.invocation_count(), 0);
    }

    #[test]
    fn boundary_candidates_use_upgraded_versioned_cache_key() {
        let c = candidate("c1", "WQHACDEFG");
        let mut gate = gate(&c, true);
        gate.fragment_info_score = 5.0;
        gate.cooccur_q_value = Some(0.06);
        let config = PlmGateConfig::dev_gpu_lowmem();
        let mut worker = MockPlmWorker::default();
        let results = run_plm_gate(
            std::slice::from_ref(&c),
            &[gate],
            &config,
            &mut EmbeddingCache::default(),
            &mut worker,
        )
        .expect("boundary pLM gate succeeds");

        let upgraded = config.upgraded_model.as_ref().expect("upgrade model");
        assert_eq!(results[0].model_id, upgraded.model_id);
        assert!(results[0].cache_key.contains(&upgraded.weights_hash));
        assert_eq!(worker.model_invocations(), &[upgraded.model_id.clone()]);
    }
}
