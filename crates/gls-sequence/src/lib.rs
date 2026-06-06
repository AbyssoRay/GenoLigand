use std::fmt::Write;

const CANONICAL: &[u8] = b"ACDEFGHIKLMNPQRSTVWY";

const KD_SCALE: &[(u8, f64)] = &[
    (b'A', 1.8),
    (b'C', 2.5),
    (b'D', -3.5),
    (b'E', -3.5),
    (b'F', 2.8),
    (b'G', -0.4),
    (b'H', -3.2),
    (b'I', 4.5),
    (b'K', -3.9),
    (b'L', 3.8),
    (b'M', 1.9),
    (b'N', -3.5),
    (b'P', -1.6),
    (b'Q', -3.5),
    (b'R', -4.5),
    (b'S', -0.8),
    (b'T', -0.7),
    (b'V', 4.2),
    (b'W', -0.9),
    (b'Y', -1.3),
];

const RESIDUE_WEIGHTS: &[(u8, f64)] = &[
    (b'A', 89.09),
    (b'C', 121.15),
    (b'D', 133.10),
    (b'E', 147.13),
    (b'F', 165.19),
    (b'G', 75.07),
    (b'H', 155.16),
    (b'I', 131.17),
    (b'K', 146.19),
    (b'L', 131.17),
    (b'M', 149.21),
    (b'N', 132.12),
    (b'P', 115.13),
    (b'Q', 146.15),
    (b'R', 174.20),
    (b'S', 105.09),
    (b'T', 119.12),
    (b'V', 117.15),
    (b'W', 204.23),
    (b'Y', 181.19),
];

#[derive(Clone, Debug, PartialEq)]
pub struct SequenceFeatures {
    pub aa_length: usize,
    pub aa_valid: bool,
    pub noncanonical_aa_count: usize,
    pub noncanonical_aa_positions: String,
    pub stop_count: usize,
    pub molecular_weight: f64,
    pub aa_entropy: f64,
    pub low_complexity_score: f64,
    pub max_homopolymer_run: usize,
    pub acidic_fraction: f64,
    pub basic_fraction: f64,
    pub charged_fraction: f64,
    pub polar_fraction: f64,
    pub hydrophobic_fraction: f64,
    pub aromatic_fraction: f64,
    pub glycine_fraction: f64,
    pub proline_fraction: f64,
    pub cysteine_count: usize,
    pub net_charge_approx_ph7: f64,
    pub mean_kd_hydropathy: f64,
    pub max_kd_window_9: f64,
    pub max_kd_window_19: f64,
    pub tm_like_hydrophobic_run_flag: bool,
    pub signal_peptide_heuristic_flag: bool,
    pub disorder_heuristic_score: f64,
    pub sequence_risk_flags: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SequenceScoreComponents {
    pub protein_like_score: f64,
    pub complexity_score: f64,
    pub length_suitability_score: f64,
    pub similarity_support_score: f64,
    pub novelty_balance_score: f64,
    pub plm_embedding_quality_score: f64,
    pub risk_penalty: f64,
    pub sequence_evidence_score: f64,
    pub flags: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SequenceEvidenceWeights {
    pub context_prior: f64,
    pub protein_like: f64,
    pub complexity: f64,
    pub length_suitability: f64,
    pub similarity_support: f64,
    pub novelty_balance: f64,
    pub plm_embedding_quality: f64,
    pub risk_penalty: f64,
}

impl Default for SequenceEvidenceWeights {
    fn default() -> Self {
        Self {
            context_prior: 0.20,
            protein_like: 0.20,
            complexity: 0.15,
            length_suitability: 0.10,
            similarity_support: 0.20,
            novelty_balance: 0.10,
            plm_embedding_quality: 0.05,
            risk_penalty: 1.00,
        }
    }
}

pub fn canonical_sequence(sequence: &str) -> String {
    sequence
        .trim()
        .to_ascii_uppercase()
        .trim_end_matches('*')
        .to_string()
}

pub fn candidate_fasta_header(
    candidate_id: &str,
    context_class: &str,
    aa_length: usize,
    source: &str,
) -> String {
    format!(">{candidate_id}|context={context_class}|orf_len={aa_length}|source={source}|phase=0.4")
}

pub fn parse_candidate_fasta_header(
    header: &str,
) -> Result<(String, String, usize, String), String> {
    let clean = header.trim().trim_start_matches('>');
    let mut parts = clean.split('|');
    let candidate_id = parts
        .next()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "fasta_header_missing_candidate_id".to_string())?
        .to_string();
    let mut context = None;
    let mut length = None;
    let mut source = None;
    for part in parts {
        if let Some(value) = part.strip_prefix("context=") {
            context = Some(value.to_string());
        } else if let Some(value) = part.strip_prefix("orf_len=") {
            length = Some(
                value
                    .parse::<usize>()
                    .map_err(|_| "fasta_header_bad_orf_len".to_string())?,
            );
        } else if let Some(value) = part.strip_prefix("source=") {
            source = Some(value.to_string());
        }
    }
    Ok((
        candidate_id,
        context.ok_or_else(|| "fasta_header_missing_context".to_string())?,
        length.ok_or_else(|| "fasta_header_missing_orf_len".to_string())?,
        source.ok_or_else(|| "fasta_header_missing_source".to_string())?,
    ))
}

pub fn sequence_features(
    sequence: &str,
    context_class: &str,
    export_status: &str,
) -> SequenceFeatures {
    let seq = canonical_sequence(sequence);
    let bytes = seq.as_bytes();
    let aa_length = bytes.len();
    let mut noncanonical = Vec::new();
    let mut stop_count = 0usize;
    for (index, aa) in bytes.iter().enumerate() {
        if *aa == b'*' {
            stop_count += 1;
        }
        if !is_canonical(*aa) {
            noncanonical.push((index + 1).to_string());
        }
    }
    let aa_valid = aa_length > 0 && noncanonical.is_empty() && stop_count == 0;
    let low_complexity_score = low_complexity_score(&seq, &[6, 8, 12]);
    let max_homopolymer_run = max_homopolymer_run(&seq);
    let mean_kd_hydropathy = mean_kd_hydropathy(&seq);
    let max_kd_window_9 = max_kd_window(&seq, 9);
    let max_kd_window_19 = max_kd_window(&seq, 19);
    let mut risk_flags = Vec::new();
    if !aa_valid {
        risk_flags.push("invalid_sequence".to_string());
    }
    if low_complexity_score >= 0.70 {
        risk_flags.push("low_complexity".to_string());
    }
    if max_homopolymer_run >= 6 {
        risk_flags.push("homopolymer_run".to_string());
    }
    if context_class.contains("DECOY") || export_status.contains("control") {
        risk_flags.push("synthetic_or_decoy_control".to_string());
    }
    if context_class == "high_background_low_information_decoy" {
        risk_flags.push("low_information_background".to_string());
    }
    if aa_length < 30 {
        risk_flags.push("short_sequence_alignment_warning".to_string());
    }
    SequenceFeatures {
        aa_length,
        aa_valid,
        noncanonical_aa_count: noncanonical.len(),
        noncanonical_aa_positions: noncanonical.join(";"),
        stop_count,
        molecular_weight: molecular_weight(&seq),
        aa_entropy: aa_entropy(&seq),
        low_complexity_score,
        max_homopolymer_run,
        acidic_fraction: fraction(&seq, b"DE"),
        basic_fraction: fraction(&seq, b"KRH"),
        charged_fraction: fraction(&seq, b"DEKRH"),
        polar_fraction: fraction(&seq, b"STNQCY"),
        hydrophobic_fraction: fraction(&seq, b"AILMFWV"),
        aromatic_fraction: fraction(&seq, b"FWY"),
        glycine_fraction: fraction(&seq, b"G"),
        proline_fraction: fraction(&seq, b"P"),
        cysteine_count: count(&seq, b"C"),
        net_charge_approx_ph7: net_charge_approx_ph7(&seq),
        mean_kd_hydropathy,
        max_kd_window_9,
        max_kd_window_19,
        tm_like_hydrophobic_run_flag: max_kd_window_19 >= 1.6 && hydrophobic_run(&seq) >= 14,
        signal_peptide_heuristic_flag: signal_peptide_heuristic(&seq),
        disorder_heuristic_score: disorder_heuristic_score(&seq),
        sequence_risk_flags: risk_flags,
    }
}

pub fn score_sequence_evidence(
    context_class: &str,
    phase_0_3_score: f64,
    features: &SequenceFeatures,
    similarity_support_score: f64,
    novelty_balance_score: f64,
    plm_embedding_quality_score: f64,
    weights: &SequenceEvidenceWeights,
) -> SequenceScoreComponents {
    let context_prior_score = context_prior_score(context_class, phase_0_3_score);
    let protein_like_score = protein_like_score(features);
    let complexity_score = clamp01(1.0 - features.low_complexity_score);
    let length_suitability_score = length_suitability_score(features.aa_length);
    let risk_penalty = risk_penalty(features);
    let raw = weights.context_prior * context_prior_score
        + weights.protein_like * protein_like_score
        + weights.complexity * complexity_score
        + weights.length_suitability * length_suitability_score
        + weights.similarity_support * similarity_support_score
        + weights.novelty_balance * novelty_balance_score
        + weights.plm_embedding_quality * plm_embedding_quality_score
        - weights.risk_penalty * risk_penalty;
    SequenceScoreComponents {
        protein_like_score,
        complexity_score,
        length_suitability_score,
        similarity_support_score,
        novelty_balance_score,
        plm_embedding_quality_score,
        risk_penalty,
        sequence_evidence_score: clamp01(raw),
        flags: features.sequence_risk_flags.clone(),
    }
}

pub fn sha256_hex(input: &[u8]) -> String {
    const H0: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut data = input.to_vec();
    let bit_len = (data.len() as u64) * 8;
    data.push(0x80);
    while data.len() % 64 != 56 {
        data.push(0);
    }
    data.extend_from_slice(&bit_len.to_be_bytes());
    let mut h = H0;
    for chunk in data.chunks(64) {
        let mut w = [0u32; 64];
        for (index, word) in w.iter_mut().take(16).enumerate() {
            let start = index * 4;
            *word = u32::from_be_bytes([
                chunk[start],
                chunk[start + 1],
                chunk[start + 2],
                chunk[start + 3],
            ]);
        }
        for index in 16..64 {
            let s0 = w[index - 15].rotate_right(7)
                ^ w[index - 15].rotate_right(18)
                ^ (w[index - 15] >> 3);
            let s1 = w[index - 2].rotate_right(17)
                ^ w[index - 2].rotate_right(19)
                ^ (w[index - 2] >> 10);
            w[index] = w[index - 16]
                .wrapping_add(s0)
                .wrapping_add(w[index - 7])
                .wrapping_add(s1);
        }
        let mut a = h[0];
        let mut b = h[1];
        let mut c = h[2];
        let mut d = h[3];
        let mut e = h[4];
        let mut f = h[5];
        let mut g = h[6];
        let mut hh = h[7];
        for index in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[index])
                .wrapping_add(w[index]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }
    let mut output = String::with_capacity(64);
    for word in h {
        write!(&mut output, "{word:08x}").expect("write sha");
    }
    output
}

pub fn fmt_f64(value: f64) -> String {
    format!("{:.6}", finite_or_zero(value))
}

fn is_canonical(aa: u8) -> bool {
    CANONICAL.contains(&aa)
}

fn aa_entropy(sequence: &str) -> f64 {
    let len = sequence.bytes().filter(|aa| is_canonical(*aa)).count();
    if len == 0 {
        return 0.0;
    }
    let mut entropy = 0.0;
    for aa in CANONICAL {
        let count = sequence.bytes().filter(|value| value == aa).count();
        if count > 0 {
            let p = count as f64 / len as f64;
            entropy -= p * p.log2();
        }
    }
    entropy / 20.0_f64.log2()
}

fn low_complexity_score(sequence: &str, windows: &[usize]) -> f64 {
    if sequence.is_empty() {
        return 1.0;
    }
    let bytes = sequence.as_bytes();
    let mut max_score = 1.0 - aa_entropy(sequence);
    for window in windows.iter().copied().filter(|window| *window > 0) {
        if bytes.len() < window {
            continue;
        }
        for slice in bytes.windows(window) {
            let text = std::str::from_utf8(slice).unwrap_or("");
            max_score = f64::max(max_score, 1.0 - aa_entropy(text));
        }
    }
    clamp01(max_score)
}

fn max_homopolymer_run(sequence: &str) -> usize {
    let mut previous = None;
    let mut current = 0usize;
    let mut best = 0usize;
    for aa in sequence.bytes() {
        if Some(aa) == previous {
            current += 1;
        } else {
            previous = Some(aa);
            current = 1;
        }
        best = best.max(current);
    }
    best
}

fn molecular_weight(sequence: &str) -> f64 {
    let total = sequence
        .bytes()
        .map(|aa| lookup(RESIDUE_WEIGHTS, aa).unwrap_or(0.0))
        .sum::<f64>();
    if sequence.len() > 1 {
        total - ((sequence.len() - 1) as f64 * 18.015)
    } else {
        total
    }
}

fn fraction(sequence: &str, set: &[u8]) -> f64 {
    if sequence.is_empty() {
        return 0.0;
    }
    sequence.bytes().filter(|aa| set.contains(aa)).count() as f64 / sequence.len() as f64
}

fn count(sequence: &str, set: &[u8]) -> usize {
    sequence.bytes().filter(|aa| set.contains(aa)).count()
}

fn net_charge_approx_ph7(sequence: &str) -> f64 {
    count(sequence, b"KR") as f64 + count(sequence, b"H") as f64 * 0.1
        - count(sequence, b"DE") as f64
}

fn mean_kd_hydropathy(sequence: &str) -> f64 {
    let mut total = 0.0;
    let mut count = 0usize;
    for aa in sequence.bytes() {
        if let Some(value) = lookup(KD_SCALE, aa) {
            total += value;
            count += 1;
        }
    }
    if count == 0 {
        0.0
    } else {
        total / count as f64
    }
}

fn max_kd_window(sequence: &str, window: usize) -> f64 {
    if window == 0 || sequence.len() < window {
        return mean_kd_hydropathy(sequence);
    }
    sequence
        .as_bytes()
        .windows(window)
        .map(|slice| mean_kd_hydropathy(std::str::from_utf8(slice).unwrap_or("")))
        .fold(f64::NEG_INFINITY, f64::max)
}

fn hydrophobic_run(sequence: &str) -> usize {
    let mut current = 0usize;
    let mut best = 0usize;
    for aa in sequence.bytes() {
        if b"AILMFWV".contains(&aa) {
            current += 1;
        } else {
            current = 0;
        }
        best = best.max(current);
    }
    best
}

fn signal_peptide_heuristic(sequence: &str) -> bool {
    let n_term = sequence.bytes().take(30).collect::<Vec<_>>();
    if n_term.len() < 18 {
        return false;
    }
    let basic_n_region = n_term.iter().take(8).any(|aa| b"KR".contains(aa));
    let hydrophobic_h_region = n_term
        .windows(8)
        .any(|window| window.iter().filter(|aa| b"AILMFWV".contains(aa)).count() >= 6);
    basic_n_region && hydrophobic_h_region
}

fn disorder_heuristic_score(sequence: &str) -> f64 {
    clamp01(fraction(sequence, b"PGSDEQKRN") * 1.15 - fraction(sequence, b"WYFILV") * 0.30)
}

fn context_prior_score(context_class: &str, phase_0_3_score: f64) -> f64 {
    let base: f64 = if context_class.starts_with("K1") {
        1.0
    } else if context_class.starts_with("K2") || context_class.starts_with("K3") {
        0.68
    } else if context_class.starts_with("K4")
        || context_class.starts_with("K5")
        || context_class.starts_with("K6")
    {
        0.45
    } else if context_class.contains("DECOY") || context_class.contains("decoy") {
        0.0
    } else {
        0.25
    };
    clamp01(base * 0.75 + phase_0_3_score.min(50.0) / 50.0 * 0.25)
}

fn protein_like_score(features: &SequenceFeatures) -> f64 {
    if !features.aa_valid {
        return 0.0;
    }
    let composition_penalty = if features.max_homopolymer_run >= 6 {
        0.25
    } else {
        0.0
    };
    clamp01(1.0 - composition_penalty - features.low_complexity_score * 0.35)
}

fn length_suitability_score(length: usize) -> f64 {
    match length {
        0..=5 => 0.0,
        6..=14 => 0.35,
        15..=29 => 0.60,
        30..=350 => 1.0,
        351..=800 => 0.75,
        _ => 0.45,
    }
}

fn risk_penalty(features: &SequenceFeatures) -> f64 {
    let mut penalty: f64 = 0.0;
    if !features.aa_valid {
        penalty += 0.65;
    }
    if features.low_complexity_score >= 0.70 {
        penalty += 0.35;
    }
    if features.max_homopolymer_run >= 6 {
        penalty += 0.20;
    }
    if features
        .sequence_risk_flags
        .iter()
        .any(|flag| flag.contains("control") || flag.contains("decoy"))
    {
        penalty += 0.55;
    }
    clamp01(penalty)
}

fn lookup(table: &[(u8, f64)], key: u8) -> Option<f64> {
    table
        .iter()
        .find(|(aa, _)| *aa == key)
        .map(|(_, value)| *value)
}

fn finite_or_zero(value: f64) -> f64 {
    if value.is_finite() { value } else { 0.0 }
}

fn clamp01(value: f64) -> f64 {
    finite_or_zero(value).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fasta_header_round_trip_preserves_fields() {
        let header = candidate_fasta_header("cand1", "K1_known_cds_same_frame", 42, "fixture");
        assert_eq!(
            parse_candidate_fasta_header(&header).expect("parse"),
            (
                "cand1".to_string(),
                "K1_known_cds_same_frame".to_string(),
                42,
                "fixture".to_string()
            )
        );
    }

    #[test]
    fn sha256_known_vector_matches_standard() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn low_complexity_and_homopolymer_are_flagged() {
        let features =
            sequence_features("AAAAAAAAAA", "K_LOW_COMPLEXITY_DECOY", "control_sequence");
        assert!(features.low_complexity_score > 0.9);
        assert_eq!(features.max_homopolymer_run, 10);
        assert!(
            features
                .sequence_risk_flags
                .iter()
                .any(|flag| flag == "low_complexity")
        );
    }

    #[test]
    fn scoring_penalizes_controls() {
        let candidate = sequence_features("MPEPTIDEKQWCLV", "K1_known_cds_same_frame", "exported");
        let control = sequence_features(
            "AAAAAAAAAAAAAA",
            "K_LOW_COMPLEXITY_DECOY",
            "control_sequence",
        );
        let weights = SequenceEvidenceWeights::default();
        let candidate_score = score_sequence_evidence(
            "K1_known_cds_same_frame",
            30.0,
            &candidate,
            0.8,
            0.4,
            1.0,
            &weights,
        );
        let control_score = score_sequence_evidence(
            "K_LOW_COMPLEXITY_DECOY",
            0.0,
            &control,
            0.0,
            0.0,
            1.0,
            &weights,
        );
        assert!(candidate_score.sequence_evidence_score > control_score.sequence_evidence_score);
    }
}
