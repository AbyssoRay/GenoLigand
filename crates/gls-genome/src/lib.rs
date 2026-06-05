use gls_artifacts::provenance_for_tool;
use gls_fragment::find_fragment_hits;
use gls_stats::{BackgroundModel, assign_p_values, assign_q_values};
use gls_types::{
    CandidateGrade, CandidateId, FragmentGrammar, FragmentHit, LocusClass, OrfCandidate,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FastaRecord {
    pub id: String,
    pub sequence: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GenomeSearchResult {
    pub candidates: Vec<OrfCandidate>,
    pub hits: Vec<FragmentHit>,
}

pub fn parse_fasta(input: &str) -> Result<Vec<FastaRecord>, String> {
    let mut records = Vec::new();
    let mut current_id: Option<String> = None;
    let mut current_sequence = String::new();
    for raw in input.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(id) = line.strip_prefix('>') {
            if let Some(previous_id) = current_id.replace(id.to_string()) {
                records.push(FastaRecord {
                    id: previous_id,
                    sequence: std::mem::take(&mut current_sequence),
                });
            }
        } else {
            current_sequence.push_str(&line.to_ascii_uppercase());
        }
    }
    if let Some(id) = current_id {
        records.push(FastaRecord {
            id,
            sequence: current_sequence,
        });
    }
    if records.is_empty() {
        return Err("fasta_has_no_records".to_string());
    }
    Ok(records)
}

pub fn reverse_complement(dna: &str) -> String {
    dna.chars()
        .rev()
        .map(|base| match base.to_ascii_uppercase() {
            'A' => 'T',
            'C' => 'G',
            'G' => 'C',
            'T' | 'U' => 'A',
            _ => 'N',
        })
        .collect()
}

pub fn translate_frame(dna: &str, frame: usize) -> String {
    dna.as_bytes()[frame..]
        .chunks(3)
        .filter(|codon| codon.len() == 3)
        .map(translate_codon)
        .collect()
}

pub fn six_frame_translations(dna: &str) -> Vec<(i8, String)> {
    let reverse = reverse_complement(dna);
    let mut frames = Vec::with_capacity(6);
    for frame in 0..3 {
        frames.push((frame as i8 + 1, translate_frame(dna, frame)));
    }
    for frame in 0..3 {
        frames.push((-((frame as i8) + 1), translate_frame(&reverse, frame)));
    }
    frames
}

pub fn stop_to_stop_orfs(record: &FastaRecord, min_aa_len: usize) -> Vec<OrfCandidate> {
    let provenance = provenance_for_tool(
        "gls-genome",
        "v1.6",
        &[
            ("record_id", record.id.clone()),
            ("min_aa_len", min_aa_len.to_string()),
        ],
    );
    let mut candidates = Vec::new();
    for (frame, translation) in six_frame_translations(&record.sequence) {
        let mut aa_start = 0usize;
        let context = OrfBuildContext {
            record,
            frame,
            translation: &translation,
            min_aa_len,
            provenance: &provenance,
        };
        for (aa_index, residue) in translation.chars().enumerate() {
            if residue == '*' {
                push_orf(&context, aa_start, aa_index, &mut candidates);
                aa_start = aa_index + 1;
            }
        }
        push_orf(&context, aa_start, translation.len(), &mut candidates);
    }
    candidates
}

pub fn search_genome(
    fasta: &str,
    grammar: &FragmentGrammar,
    min_aa_len: usize,
) -> Result<GenomeSearchResult, String> {
    let records = parse_fasta(fasta)?;
    let mut candidates = records
        .iter()
        .flat_map(|record| stop_to_stop_orfs(record, min_aa_len))
        .collect::<Vec<_>>();
    candidates.sort_by(|a, b| a.candidate_id.cmp(&b.candidate_id));

    let sequences = candidates
        .iter()
        .map(|candidate| candidate.sequence.as_str())
        .collect::<Vec<_>>();
    let background = BackgroundModel::from_sequences(sequences);
    let mut hits = candidates
        .iter()
        .flat_map(|candidate| {
            find_fragment_hits(&candidate.candidate_id, &candidate.sequence, grammar)
        })
        .collect::<Vec<_>>();
    assign_p_values(&mut hits, &background);
    assign_q_values(&mut hits);

    Ok(GenomeSearchResult { candidates, hits })
}

struct OrfBuildContext<'a> {
    record: &'a FastaRecord,
    frame: i8,
    min_aa_len: usize,
    translation: &'a str,
    provenance: &'a gls_types::Provenance,
}

fn push_orf(
    context: &OrfBuildContext<'_>,
    aa_start: usize,
    aa_end: usize,
    candidates: &mut Vec<OrfCandidate>,
) {
    if aa_end <= aa_start || aa_end - aa_start < context.min_aa_len {
        return;
    }
    let sequence = context.translation[aa_start..aa_end].to_string();
    if sequence.contains('X') {
        return;
    }
    let positive_frame = context.frame.unsigned_abs() as usize - 1;
    let start = positive_frame + aa_start * 3;
    let end = positive_frame + aa_end * 3;
    let candidate_id = CandidateId::new(format!(
        "{}:{}:{}-{}",
        context.record.id, context.frame, start, end
    ));
    let locus_class = if context.record.id.to_ascii_lowercase().contains("dark") {
        LocusClass::DarkGenome
    } else {
        LocusClass::Unknown
    };
    candidates.push(OrfCandidate {
        candidate_id,
        shard_id: context.record.id.clone(),
        sequence,
        nucleotide_range: (start, end),
        frame: context.frame,
        locus_class,
        stop_to_stop: true,
        fragment_family: "unassigned".to_string(),
        provenance: context.provenance.clone(),
        grade: CandidateGrade::G2,
    });
}

fn translate_codon(codon: &[u8]) -> char {
    match codon {
        b"TTT" | b"TTC" => 'F',
        b"TTA" | b"TTG" | b"CTT" | b"CTC" | b"CTA" | b"CTG" => 'L',
        b"ATT" | b"ATC" | b"ATA" => 'I',
        b"ATG" => 'M',
        b"GTT" | b"GTC" | b"GTA" | b"GTG" => 'V',
        b"TCT" | b"TCC" | b"TCA" | b"TCG" | b"AGT" | b"AGC" => 'S',
        b"CCT" | b"CCC" | b"CCA" | b"CCG" => 'P',
        b"ACT" | b"ACC" | b"ACA" | b"ACG" => 'T',
        b"GCT" | b"GCC" | b"GCA" | b"GCG" => 'A',
        b"TAT" | b"TAC" => 'Y',
        b"TAA" | b"TAG" | b"TGA" => '*',
        b"CAT" | b"CAC" => 'H',
        b"CAA" | b"CAG" => 'Q',
        b"AAT" | b"AAC" => 'N',
        b"AAA" | b"AAG" => 'K',
        b"GAT" | b"GAC" => 'D',
        b"GAA" | b"GAG" => 'E',
        b"TGT" | b"TGC" => 'C',
        b"TGG" => 'W',
        b"CGT" | b"CGC" | b"CGA" | b"CGG" | b"AGA" | b"AGG" => 'R',
        b"GGT" | b"GGC" | b"GGA" | b"GGG" => 'G',
        _ => 'X',
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gls_types::{FragmentGrammar, Provenance};

    #[test]
    fn reverse_complement_round_trips_canonical_bases() {
        let dna = "ATGCCCTAA";
        assert_eq!(reverse_complement(&reverse_complement(dna)), dna);
    }

    #[test]
    fn six_frames_keep_expected_frame_labels() {
        let frames = six_frame_translations("ATGTAA");
        assert_eq!(frames.len(), 6);
        assert_eq!(frames[0], (1, "M*".to_string()));
        assert!(frames.iter().any(|(frame, _)| *frame == -3));
    }

    #[test]
    fn stop_to_stop_orf_boundaries_are_frame_aware() {
        let record = FastaRecord {
            id: "shard".to_string(),
            sequence: "AATGAAATAG".to_string(),
        };
        let candidates = stop_to_stop_orfs(&record, 2);
        assert!(
            candidates
                .iter()
                .any(|candidate| candidate.frame == 2 && candidate.sequence == "MK")
        );
    }

    #[test]
    fn genome_search_returns_hits_with_q_values() {
        let grammar = FragmentGrammar {
            grammar_id: "g".to_string(),
            exact_fragments: vec!["WQH".to_string()],
            category_fragments: Vec::new(),
            provenance: Provenance::new("test", "1", "p"),
            hash: "h".to_string(),
        };
        let result = search_genome(">dark1\nTGGCAACAT", &grammar, 2).expect("search succeeds");
        assert!(!result.candidates.is_empty());
        assert!(result.hits.iter().all(|hit| hit.q_value <= 1.0));
    }
}
