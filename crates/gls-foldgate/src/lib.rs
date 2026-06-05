use gls_types::{
    CandidateGrade, CandidatePatchGraph, FoldabilityGateResult, InteractionKind, OrfCandidate,
    PatchNode, Point3Data, StructureGateResult,
};

pub trait PersistentFoldWorker {
    fn fold_local_structure(&mut self, candidate: &OrfCandidate) -> Result<String, String>;
}

pub trait PersistentPocketWorker {
    fn detect_pockets(
        &mut self,
        candidate: &OrfCandidate,
        structure_id: &str,
    ) -> Result<CandidatePatchGraph, String>;
}

#[derive(Clone, Debug, Default)]
pub struct MockFoldWorker {
    pub calls: usize,
    pub fail: bool,
}

impl PersistentFoldWorker for MockFoldWorker {
    fn fold_local_structure(&mut self, candidate: &OrfCandidate) -> Result<String, String> {
        self.calls += 1;
        if self.fail {
            Err("fold_worker_failed".to_string())
        } else {
            Ok(format!("structure_{}", candidate.candidate_id))
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct MockPocketWorker {
    pub calls: usize,
    pub fail: bool,
}

impl PersistentPocketWorker for MockPocketWorker {
    fn detect_pockets(
        &mut self,
        candidate: &OrfCandidate,
        _structure_id: &str,
    ) -> Result<CandidatePatchGraph, String> {
        self.calls += 1;
        if self.fail {
            return Err("pocket_worker_failed".to_string());
        }
        let residues = candidate
            .sequence
            .chars()
            .filter(|residue| matches!(residue, 'D' | 'H' | 'Q'))
            .take(3)
            .collect::<Vec<_>>();
        let source_residues = if residues.is_empty() {
            candidate.sequence.chars().take(3).collect::<Vec<_>>()
        } else {
            residues
        };
        let nodes = source_residues
            .into_iter()
            .enumerate()
            .map(|(idx, residue)| PatchNode {
                node_id: format!("{}_pocket_{idx}", candidate.candidate_id),
                residue,
                kind: InteractionKind::HydrogenBond,
                point: Point3Data::new(idx as f64, 0.0, 0.0),
                vector: Point3Data::new(1.0, 0.0, 0.0),
                sasa: 0.5,
                pocket_depth: 2.0,
                confidence: 0.8,
            })
            .collect();
        Ok(CandidatePatchGraph {
            candidate_id: candidate.candidate_id.clone(),
            nodes,
            provenance: candidate.provenance.clone(),
            confidence_bin: "high".to_string(),
            ligand_class: "generic".to_string(),
        })
    }
}

pub fn run_foldgate<F: PersistentFoldWorker, P: PersistentPocketWorker>(
    candidate: &OrfCandidate,
    foldability: &FoldabilityGateResult,
    fold_worker: &mut F,
    pocket_worker: &mut P,
) -> StructureGateResult {
    if !matches!(candidate.grade, CandidateGrade::G2 | CandidateGrade::G3) {
        return StructureGateResult {
            candidate_id: candidate.candidate_id.clone(),
            accepted_for_l15: false,
            candidate_patch_graph: None,
            failure_reason: Some("l15_requires_g2_or_g3_candidate".to_string()),
        };
    }
    if !foldability.pass_structure {
        return StructureGateResult {
            candidate_id: candidate.candidate_id.clone(),
            accepted_for_l15: false,
            candidate_patch_graph: None,
            failure_reason: Some("foldability_gate_failed".to_string()),
        };
    }
    let structure_id = match fold_worker.fold_local_structure(candidate) {
        Ok(value) => value,
        Err(reason) => {
            return StructureGateResult {
                candidate_id: candidate.candidate_id.clone(),
                accepted_for_l15: false,
                candidate_patch_graph: None,
                failure_reason: Some(reason),
            };
        }
    };
    match pocket_worker.detect_pockets(candidate, &structure_id) {
        Ok(cpg) => StructureGateResult {
            candidate_id: candidate.candidate_id.clone(),
            accepted_for_l15: true,
            candidate_patch_graph: Some(cpg),
            failure_reason: None,
        },
        Err(reason) => StructureGateResult {
            candidate_id: candidate.candidate_id.clone(),
            accepted_for_l15: false,
            candidate_patch_graph: None,
            failure_reason: Some(reason),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gls_types::{CandidateId, FoldabilityGateResult, LocusClass, Provenance, stable_hash};

    fn candidate(grade: CandidateGrade) -> OrfCandidate {
        OrfCandidate {
            candidate_id: CandidateId::from("c"),
            shard_id: "s".to_string(),
            sequence: "DHQACDEFG".to_string(),
            nucleotide_range: (0, 27),
            frame: 1,
            locus_class: LocusClass::Unknown,
            stop_to_stop: true,
            fragment_family: "fam".to_string(),
            provenance: Provenance::new("test", "1", "c"),
            grade,
        }
    }

    fn foldability(candidate: &OrfCandidate) -> FoldabilityGateResult {
        FoldabilityGateResult {
            candidate_id: candidate.candidate_id.clone(),
            sequence_hash: stable_hash(&candidate.sequence),
            model_id: "m".to_string(),
            tokenizer_id: "t".to_string(),
            weights_hash: "w".to_string(),
            cache_key: "k".to_string(),
            embedding_from_cache: true,
            pass_structure: true,
            failure_reason: None,
            bucket_len: 64,
        }
    }

    #[test]
    fn g0_g1_candidates_cannot_enter_l15() {
        let c = candidate(CandidateGrade::G1);
        let result = run_foldgate(
            &c,
            &foldability(&c),
            &mut MockFoldWorker::default(),
            &mut MockPocketWorker::default(),
        );

        assert!(!result.accepted_for_l15);
        assert_eq!(
            result.failure_reason.as_deref(),
            Some("l15_requires_g2_or_g3_candidate")
        );
    }

    #[test]
    fn pocket_worker_failure_is_machine_readable() {
        let c = candidate(CandidateGrade::G2);
        let result = run_foldgate(
            &c,
            &foldability(&c),
            &mut MockFoldWorker::default(),
            &mut MockPocketWorker {
                calls: 0,
                fail: true,
            },
        );

        assert_eq!(
            result.failure_reason.as_deref(),
            Some("pocket_worker_failed")
        );
    }
}
