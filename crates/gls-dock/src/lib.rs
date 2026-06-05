use gls_types::{CandidateId, DockingResult, PatchRealizationResult, stable_hash};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DockingManifestEntry {
    pub candidate_id: CandidateId,
    pub receptor_id: String,
    pub ligand_id: String,
    pub patch_profile_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DockingManifest {
    pub manifest_id: String,
    pub worker_pool: String,
    pub vina_args: Vec<String>,
    pub entries: Vec<DockingManifestEntry>,
    pub rejected_candidate_ids: Vec<CandidateId>,
}

impl DockingManifest {
    pub fn is_batch_manifest(&self) -> bool {
        self.entries.len() > 1 || self.rejected_candidate_ids.is_empty()
    }
}

pub trait PersistentDockWorker {
    fn dock_batch(&mut self, candidates: &[PatchRealizationResult]) -> Result<Vec<f64>, String>;
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
    fn dock_batch(&mut self, candidates: &[PatchRealizationResult]) -> Result<Vec<f64>, String> {
        if !self.started {
            self.started = true;
            self.starts += 1;
        }
        self.batches += 1;
        Ok(candidates
            .iter()
            .map(|candidate| -7.0 - candidate.score)
            .collect())
    }

    fn process_start_count(&self) -> usize {
        self.starts
    }
}

pub fn run_docking<W: PersistentDockWorker>(
    candidates: &[PatchRealizationResult],
    worker: &mut W,
) -> Result<Vec<DockingResult>, String> {
    let _manifest =
        build_docking_manifest(candidates, "vina-persistent-pool", &["--exhaustiveness=8"]);
    let accepted = candidates
        .iter()
        .filter(|candidate| candidate.pass_l2)
        .cloned()
        .collect::<Vec<_>>();
    let scores = if accepted.is_empty() {
        Vec::new()
    } else {
        worker.dock_batch(&accepted)?
    };
    if scores.len() != accepted.len() {
        return Err("dock_worker_score_count_mismatch".to_string());
    }
    let mut score_by_candidate = accepted
        .iter()
        .map(|candidate| candidate.candidate_id.clone())
        .zip(scores)
        .collect::<std::collections::BTreeMap<_, _>>();

    Ok(candidates
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
        .collect())
}

pub fn build_docking_manifest(
    candidates: &[PatchRealizationResult],
    worker_pool: impl Into<String>,
    vina_args: &[&str],
) -> DockingManifest {
    let mut entries = Vec::new();
    let mut rejected_candidate_ids = Vec::new();
    for candidate in candidates {
        if candidate.pass_l2 {
            entries.push(DockingManifestEntry {
                candidate_id: candidate.candidate_id.clone(),
                receptor_id: format!("receptor_{}", candidate.candidate_id),
                ligand_id: format!("ligand_{}", candidate.candidate_id),
                patch_profile_id: candidate.profile_id.clone(),
            });
        } else {
            rejected_candidate_ids.push(candidate.candidate_id.clone());
        }
    }
    let manifest_payload = entries
        .iter()
        .map(|entry| {
            format!(
                "{}:{}:{}:{}",
                entry.candidate_id, entry.receptor_id, entry.ligand_id, entry.patch_profile_id
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    DockingManifest {
        manifest_id: format!("dock_manifest_{}", stable_hash(&manifest_payload)),
        worker_pool: worker_pool.into(),
        vina_args: vina_args.iter().map(|arg| (*arg).to_string()).collect(),
        entries,
        rejected_candidate_ids,
    }
}

pub fn parse_vina_result(candidate_id: CandidateId, output: &str) -> Result<DockingResult, String> {
    let score = output
        .lines()
        .find_map(parse_vina_score_line)
        .ok_or_else(|| "vina_result_missing_score".to_string())?;
    Ok(DockingResult {
        candidate_id,
        pass_l2_input: true,
        score: Some(score),
        worker_reused: true,
        failure_reason: None,
    })
}

fn parse_vina_score_line(line: &str) -> Option<f64> {
    let trimmed = line.trim();
    if let Some(rest) = trimmed.strip_prefix("REMARK VINA RESULT:") {
        return rest.split_whitespace().next()?.parse::<f64>().ok();
    }
    let mut columns = trimmed.split_whitespace();
    let mode = columns.next()?;
    if mode.parse::<usize>().ok()? == 1 {
        return columns.next()?.parse::<f64>().ok();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use gls_types::Provenance;

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
        let results =
            run_docking(&[patch("ok", true), patch("no", false)], &mut worker).expect("dock");

        assert!(results.iter().any(|result| result.score.is_some()));
        assert!(results.iter().any(
            |result| result.failure_reason.as_deref() == Some("docking_requires_pass_l2_true")
        ));
    }

    #[test]
    fn docking_worker_is_persistent_across_batch() {
        let mut worker = MockDockWorker::default();
        let _ = run_docking(&[patch("a", true), patch("b", true)], &mut worker).expect("dock");

        assert_eq!(worker.process_start_count(), 1);
        assert_eq!(worker.batches(), 1);
    }

    #[test]
    fn docking_manifest_only_contains_pass_l2_candidates() {
        let manifest = build_docking_manifest(
            &[patch("ok", true), patch("no", false)],
            "pool",
            &["--cpu=4"],
        );

        assert_eq!(manifest.entries.len(), 1);
        assert_eq!(manifest.entries[0].candidate_id, CandidateId::from("ok"));
        assert_eq!(
            manifest.rejected_candidate_ids,
            vec![CandidateId::from("no")]
        );
        assert!(manifest.vina_args.contains(&"--cpu=4".to_string()));
    }

    #[test]
    fn vina_result_parser_extracts_best_affinity() {
        let output =
            "-----+------------+----------+----------\n   1       -8.40      0.000      0.000\n";
        let result = parse_vina_result(CandidateId::from("ok"), output).expect("parse vina");

        assert_eq!(result.score, Some(-8.40));
        assert!(result.failure_reason.is_none());
    }

    #[test]
    fn vina_result_parser_accepts_remark_format() {
        let output = "REMARK VINA RESULT: -7.25 0.0 0.0";
        let result = parse_vina_result(CandidateId::from("ok"), output).expect("parse vina");

        assert_eq!(result.score, Some(-7.25));
    }
}
