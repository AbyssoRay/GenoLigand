use gls_types::{
    CandidateGrade, CandidatePatchGraph, FoldabilityGateResult, InteractionKind, OrfCandidate,
    OrfExtensionResult, PatchNode, Point3Data, Provenance, StructureGateResult, stable_hash,
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrfExtensionPolicy {
    pub upstream_aa: usize,
    pub downstream_aa: usize,
}

impl Default for OrfExtensionPolicy {
    fn default() -> Self {
        Self {
            upstream_aa: 15,
            downstream_aa: 15,
        }
    }
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
        let nodes: Vec<PatchNode> = source_residues
            .into_iter()
            .enumerate()
            .map(|(idx, residue)| PatchNode {
                node_id: format!("{}_pocket_{idx}", candidate.candidate_id),
                residue,
                kind: InteractionKind::HydrogenBond,
                point: Point3Data::new(idx as f64, 0.0, 0.0),
                vector: Point3Data::new(1.0, 0.0, 0.0),
                sasa: 0.5,
                occlusion: 0.1,
                pocket_depth: 2.0,
                confidence: 0.8,
            })
            .collect();
        let node_count = nodes.len();
        Ok(CandidatePatchGraph {
            candidate_id: candidate.candidate_id.clone(),
            nodes,
            provenance: Provenance::new(
                "gls-foldgate-pocket-worker",
                "v1.6",
                &format!(
                    "candidate_id={};structure_id={};detector=p2rank_or_fpocket;nodes={}",
                    candidate.candidate_id, _structure_id, node_count
                ),
            ),
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
    let orf_extension = extend_orf_candidate(candidate, &OrfExtensionPolicy::default());
    if !matches!(candidate.grade, CandidateGrade::G2 | CandidateGrade::G3) {
        return StructureGateResult {
            candidate_id: candidate.candidate_id.clone(),
            accepted_for_l15: false,
            orf_extension: Some(orf_extension),
            candidate_patch_graph: None,
            failure_reason: Some("l15_requires_g2_or_g3_candidate".to_string()),
        };
    }
    if !foldability.pass_structure {
        return StructureGateResult {
            candidate_id: candidate.candidate_id.clone(),
            accepted_for_l15: false,
            orf_extension: Some(orf_extension),
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
                orf_extension: Some(orf_extension),
                candidate_patch_graph: None,
                failure_reason: Some(reason),
            };
        }
    };
    match pocket_worker.detect_pockets(candidate, &structure_id) {
        Ok(cpg) => StructureGateResult {
            candidate_id: candidate.candidate_id.clone(),
            accepted_for_l15: true,
            orf_extension: Some(orf_extension),
            candidate_patch_graph: Some(cpg),
            failure_reason: None,
        },
        Err(reason) => StructureGateResult {
            candidate_id: candidate.candidate_id.clone(),
            accepted_for_l15: false,
            orf_extension: Some(orf_extension),
            candidate_patch_graph: None,
            failure_reason: Some(reason),
        },
    }
}

pub fn extend_orf_candidate(
    candidate: &OrfCandidate,
    policy: &OrfExtensionPolicy,
) -> OrfExtensionResult {
    let upstream_nt = policy.upstream_aa.saturating_mul(3);
    let downstream_nt = policy.downstream_aa.saturating_mul(3);
    let start = candidate.nucleotide_range.0.saturating_sub(upstream_nt);
    let end = candidate.nucleotide_range.1.saturating_add(downstream_nt);
    let upstream_aa_added = (candidate.nucleotide_range.0 - start) / 3;
    let downstream_aa_added = (end - candidate.nucleotide_range.1) / 3;
    let extended_aa_len = candidate
        .sequence
        .len()
        .saturating_add(upstream_aa_added)
        .saturating_add(downstream_aa_added);
    let parameters = format!(
        "candidate_id={};original={}-{};extended={}-{};upstream={};downstream={}",
        candidate.candidate_id,
        candidate.nucleotide_range.0,
        candidate.nucleotide_range.1,
        start,
        end,
        policy.upstream_aa,
        policy.downstream_aa
    );
    OrfExtensionResult {
        candidate_id: candidate.candidate_id.clone(),
        original_nucleotide_range: candidate.nucleotide_range,
        extended_nucleotide_range: (start, end),
        original_aa_len: candidate.sequence.len(),
        extended_aa_len,
        upstream_aa_added,
        downstream_aa_added,
        sequence_hash: stable_hash(&format!("{}:{}:{}", candidate.sequence, start, end)),
        provenance: Provenance::new("gls-foldgate-orf-extension", "v1.6", &parameters),
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

    #[test]
    fn l15_records_orf_extension_before_worker_boundary() {
        let c = candidate(CandidateGrade::G2);
        let result = run_foldgate(
            &c,
            &foldability(&c),
            &mut MockFoldWorker::default(),
            &mut MockPocketWorker::default(),
        );
        let extension = result.orf_extension.expect("extension is recorded");

        assert_eq!(extension.original_nucleotide_range, (0, 27));
        assert!(extension.extended_nucleotide_range.1 > extension.original_nucleotide_range.1);
        assert_eq!(extension.provenance.source, "gls-foldgate-orf-extension");
    }
}
