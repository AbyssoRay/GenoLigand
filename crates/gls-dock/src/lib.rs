use gls_types::{DockingResult, PatchRealizationResult};

pub trait PersistentDockWorker {
    fn dock_batch(&mut self, candidates: &[PatchRealizationResult]) -> Vec<f64>;
    fn process_start_count(&self) -> usize;
}

#[derive(Clone, Debug, Default)]
pub struct MockDockWorker {
    started: bool,
    starts: usize,
    batches: usize,
}

impl MockDockWorker {
    pub fn batches(&self) -> usize {
        self.batches
    }
}

impl PersistentDockWorker for MockDockWorker {
    fn dock_batch(&mut self, candidates: &[PatchRealizationResult]) -> Vec<f64> {
        if !self.started {
            self.started = true;
            self.starts += 1;
        }
        self.batches += 1;
        candidates
            .iter()
            .map(|candidate| -7.0 - candidate.score)
            .collect()
    }

    fn process_start_count(&self) -> usize {
        self.starts
    }
}

pub fn run_docking<W: PersistentDockWorker>(
    candidates: &[PatchRealizationResult],
    worker: &mut W,
) -> Vec<DockingResult> {
    let accepted = candidates
        .iter()
        .filter(|candidate| candidate.pass_l2)
        .cloned()
        .collect::<Vec<_>>();
    let scores = if accepted.is_empty() {
        Vec::new()
    } else {
        worker.dock_batch(&accepted)
    };
    let mut score_by_candidate = accepted
        .iter()
        .map(|candidate| candidate.candidate_id.clone())
        .zip(scores)
        .collect::<std::collections::BTreeMap<_, _>>();

    candidates
        .iter()
        .map(|candidate| {
            if !candidate.pass_l2 {
                return DockingResult {
                    candidate_id: candidate.candidate_id.clone(),
                    pass_l2_input: false,
                    score: None,
                    worker_reused: worker.process_start_count() <= 1,
                    failure_reason: Some("docking_requires_pass_l2_true".to_string()),
                };
            }
            DockingResult {
                candidate_id: candidate.candidate_id.clone(),
                pass_l2_input: true,
                score: score_by_candidate.remove(&candidate.candidate_id),
                worker_reused: worker.process_start_count() <= 1,
                failure_reason: None,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gls_types::{CandidateId, Provenance};

    fn patch(id: &str, pass_l2: bool) -> PatchRealizationResult {
        PatchRealizationResult {
            candidate_id: CandidateId::from(id),
            profile_id: "p".to_string(),
            pass_l2,
            matched_constraints: if pass_l2 { 3 } else { 0 },
            rmsd: 0.1,
            score: 0.9,
            failure_reason: None,
            audit: Vec::new(),
            provenance: Provenance::new("test", "1", id),
        }
    }

    #[test]
    fn docking_only_processes_pass_l2_candidates() {
        let mut worker = MockDockWorker::default();
        let results = run_docking(&[patch("ok", true), patch("no", false)], &mut worker);

        assert!(results.iter().any(|result| result.score.is_some()));
        assert!(results.iter().any(
            |result| result.failure_reason.as_deref() == Some("docking_requires_pass_l2_true")
        ));
    }

    #[test]
    fn docking_worker_is_persistent_across_batch() {
        let mut worker = MockDockWorker::default();
        let _ = run_docking(&[patch("a", true), patch("b", true)], &mut worker);

        assert_eq!(worker.process_start_count(), 1);
        assert_eq!(worker.batches(), 1);
    }
}
