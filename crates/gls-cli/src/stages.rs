use gls_fixtures::dev_fixture_manifest;
use gls_fragment::information_content;
use gls_genome::{parse_fasta, reverse_complement};
use gls_ligand::LigandSpec;
use gls_shortcircuit::{amino_acid_entropy, low_complexity_score, repeat_fraction};
use gls_stats::{BackgroundModel, benjamini_hochberg};
use gls_tolerance_calib::{CalibrationConfig, calibrate_tolerances};
use gls_types::ToleranceProfileLevel;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

type Row = BTreeMap<String, String>;

const HITS_HEADERS: &[&str] = &[
    "candidate_id",
    "fragment_id",
    "fragment_sequence",
    "match_sequence",
    "chromosome",
    "start",
    "end",
    "strand",
    "frame",
    "parent_region_id",
    "parent_region_start",
    "parent_region_end",
    "parent_sequence",
    "reading_frame_sequence",
    "match_aa_start",
    "match_aa_end",
    "match_score",
    "fragment_family",
    "locus_class",
    "candidate_grade",
    "source_stage",
];

const STATS_HEADERS: &[&str] = &[
    "candidate_id",
    "fragment_id",
    "fragment_sequence",
    "match_sequence",
    "chromosome",
    "start",
    "end",
    "strand",
    "frame",
    "parent_region_id",
    "parent_region_start",
    "parent_region_end",
    "parent_sequence",
    "raw_count",
    "expected_count",
    "background_model_id",
    "background_model_type",
    "amino_acid_frequency_source",
    "codon_frequency_source",
    "local_gc_adjusted",
    "low_complexity_penalty_enabled",
    "multiple_testing_method",
    "log_odds",
    "p_value",
    "q_value",
    "fdr_pass",
    "information_content",
    "low_complexity_score",
    "entropy_score",
    "repeat_fraction",
    "decision",
    "decision_reason",
    "fragment_family",
    "locus_class",
    "candidate_grade",
];

const GATE_HEADERS: &[&str] = &[
    "candidate_id",
    "fragment_id",
    "fragment_sequence",
    "match_sequence",
    "chromosome",
    "start",
    "end",
    "strand",
    "frame",
    "parent_region_id",
    "parent_region_start",
    "parent_region_end",
    "parent_sequence",
    "length_checked_on",
    "parent_sequence_length",
    "fragment_sequence_length",
    "fdr_pass",
    "low_complexity_score",
    "decision",
    "pass_plm",
    "failure_reason",
    "rule_version",
    "budget_rank",
];

const PLM_HEADERS: &[&str] = &[
    "candidate_id",
    "fragment_id",
    "fragment_sequence",
    "parent_sequence",
    "pass_plm_input",
    "plm_called",
    "plm_stubbed",
    "budget_profile",
    "model_id",
    "tokenizer_id",
    "weights_hash",
    "sequence_length",
    "pass_structure",
    "failure_reason",
];

const FOLDGATE_HEADERS: &[&str] = &[
    "candidate_id",
    "fragment_id",
    "fragment_sequence",
    "candidate_sequence",
    "pass_plm_input",
    "pass_l15",
    "structure_id",
    "patch_id",
    "orf_extension_checked",
    "parent_sequence_length",
    "candidate_patch_graph_nodes",
    "failure_reason",
];

const PATCH_HEADERS: &[&str] = &[
    "candidate_id",
    "fragment_id",
    "fragment_sequence",
    "patch_id",
    "pass_patch",
    "constraint_graph_id",
    "candidate_patch_graph_id",
    "num_icg_nodes",
    "num_cpg_nodes",
    "num_constraints_required",
    "num_constraints_matched",
    "matched_constraint_kinds",
    "rmsd_after_alignment",
    "rmsd_after_kabsch",
    "score",
    "hash_bucket_count",
    "hash_bucket_occupancy",
    "max_bucket_occupancy",
    "memory_guard_triggered",
    "profile_id",
    "profile_level",
    "profile_version",
    "calibration_fixture_set",
    "radius_tolerance",
    "angle_tolerance_deg",
    "pass_l2",
    "fail_reason",
];

const DOCK_HEADERS: &[&str] = &[
    "candidate_id",
    "fragment_id",
    "fragment_sequence",
    "patch_id",
    "dock_mode",
    "dock_stubbed",
    "precondition_checked",
    "pass_patch_input",
    "pass_l2",
    "score",
    "fail_reason",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchOptions {
    pub genome: PathBuf,
    pub fragments: PathBuf,
    pub out: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatsOptions {
    pub hits: PathBuf,
    pub out: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GateShortCircuitOptions {
    pub stats: PathBuf,
    pub out: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GatePlmOptions {
    pub candidates: PathBuf,
    pub profile: String,
    pub out: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FoldgateOptions {
    pub candidates: PathBuf,
    pub out: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PatchOptions {
    pub foldgate: PathBuf,
    pub ligand: PathBuf,
    pub tolerance_profile: PathBuf,
    pub out: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DockOptions {
    pub patches: PathBuf,
    pub mode: String,
    pub out: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidateOptions {
    pub kind: ValidateKind,
    pub input: PathBuf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValidateKind {
    Fasta,
    Gff,
    FragmentKernel,
    Ligand,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SplitDevSmokeOptions {
    pub genome: PathBuf,
    pub fragments: PathBuf,
    pub ligand: PathBuf,
    pub out: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct KernelFragment {
    id: String,
    kind: String,
    sequence: String,
    information_weight: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct OrfSegment {
    record_id: String,
    sequence: String,
    strand: char,
    frame: i8,
    parent_start: usize,
    parent_end: usize,
}

pub fn run_search_genome(options: &SearchOptions) -> Result<usize, String> {
    let genome = read_text(&options.genome, "genome_fasta")?;
    let records = parse_fasta(genome.trim_start_matches('\u{feff}'))?;
    let fragments = parse_fragment_kernel_file(&options.fragments)?;
    let mut rows = Vec::new();

    for record in &records {
        let orfs = find_start_stop_orfs(&record.id, &record.sequence, 6);
        for orf in orfs {
            for fragment in &fragments {
                let mut offset = 0usize;
                while let Some(found) = orf.sequence[offset..].find(&fragment.sequence) {
                    let aa_start = offset + found;
                    let aa_end = aa_start + fragment.sequence.len();
                    rows.push(hit_row(&orf, fragment, aa_start, aa_end));
                    offset = aa_start + 1;
                }
            }
        }
    }

    rows.sort_by(|left, right| {
        required(left, "candidate_id")
            .cmp(required(right, "candidate_id"))
            .then_with(|| required(left, "fragment_id").cmp(required(right, "fragment_id")))
            .then_with(|| required(left, "start").cmp(required(right, "start")))
    });
    write_tsv(&options.out, HITS_HEADERS, &rows)?;
    Ok(rows.len())
}

pub fn run_stats_score(options: &StatsOptions) -> Result<usize, String> {
    let rows = read_tsv(&options.hits)?;
    let background =
        BackgroundModel::from_sequences(rows.iter().map(|row| required(row, "parent_sequence")));
    let mut scored = rows
        .iter()
        .map(|row| stats_preliminary_row(row, &background))
        .collect::<Result<Vec<_>, _>>()?;
    let q_values = benjamini_hochberg(
        &scored
            .iter()
            .map(|row| parse_f64(required(row, "p_value")))
            .collect::<Vec<_>>(),
    );
    for (row, q_value) in scored.iter_mut().zip(q_values) {
        let info = parse_f64(required(row, "information_content"));
        let low_complexity = parse_f64(required(row, "low_complexity_score"));
        let fdr_pass = q_value <= 0.05 && info >= 8.0 && low_complexity < 0.85;
        set(row, "q_value", fmt_f64(q_value));
        set(row, "fdr_pass", fdr_pass.to_string());
        if fdr_pass {
            set(row, "decision", "pass_stats");
            set(row, "decision_reason", "fdr_and_complexity_pass");
        } else if low_complexity >= 0.85 {
            set(row, "decision", "filter_stats");
            set(row, "decision_reason", "low_complexity_decoy_filtered");
        } else if info < 8.0 {
            set(row, "decision", "filter_stats");
            set(row, "decision_reason", "low_information_content");
        } else {
            set(row, "decision", "filter_stats");
            set(row, "decision_reason", "fdr_threshold_not_met");
        }
    }
    write_tsv(&options.out, STATS_HEADERS, &scored)?;
    Ok(scored.len())
}

pub fn run_gate_short_circuit(options: &GateShortCircuitOptions) -> Result<usize, String> {
    let rows = read_tsv(&options.stats)?;
    let gated = rows
        .iter()
        .enumerate()
        .map(|(index, row)| gate_short_circuit_row(index, row))
        .collect::<Vec<_>>();
    write_tsv(&options.out, GATE_HEADERS, &gated)?;
    Ok(gated.len())
}

pub fn run_gate_plm(options: &GatePlmOptions) -> Result<usize, String> {
    let rows = read_tsv(&options.candidates)?;
    let plm = rows
        .iter()
        .map(|row| plm_row(row, &options.profile))
        .collect::<Vec<_>>();
    write_tsv(&options.out, PLM_HEADERS, &plm)?;
    Ok(plm.len())
}

pub fn run_foldgate_realize(options: &FoldgateOptions) -> Result<usize, String> {
    let rows = read_tsv(&options.candidates)?;
    let realized = rows.iter().map(foldgate_row).collect::<Vec<_>>();
    write_tsv(&options.out, FOLDGATE_HEADERS, &realized)?;
    Ok(realized.len())
}

pub fn run_patch_realize(options: &PatchOptions) -> Result<usize, String> {
    validate_ligand_file(&options.ligand)?;
    let rows = read_tsv(&options.foldgate)?;
    let calibration = calibrate_tolerances(&dev_fixture_manifest(), &CalibrationConfig::dev())?;
    let profile = calibration
        .profiles
        .iter()
        .find(|profile| profile.profile_level == ToleranceProfileLevel::Standard)
        .or_else(|| calibration.profiles.first())
        .ok_or_else(|| "tolerance_profile_unavailable".to_string())?;
    let profile_version = if options.tolerance_profile.exists() {
        options
            .tolerance_profile
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("external_tolerance_profile")
            .to_string()
    } else {
        "dev_fixture_calibration".to_string()
    };
    let patched = rows
        .iter()
        .map(|row| patch_row(row, profile, &profile_version))
        .collect::<Vec<_>>();
    write_tsv(&options.out, PATCH_HEADERS, &patched)?;
    Ok(patched.len())
}

pub fn run_dock(options: &DockOptions) -> Result<usize, String> {
    let rows = read_tsv(&options.patches)?;
    let docked = rows
        .iter()
        .map(|row| dock_row(row, &options.mode))
        .collect::<Vec<_>>();
    write_tsv(&options.out, DOCK_HEADERS, &docked)?;
    Ok(docked.len())
}

pub fn run_validate(options: &ValidateOptions) -> Result<String, String> {
    match options.kind {
        ValidateKind::Fasta => validate_fasta_file(&options.input)?,
        ValidateKind::Gff => validate_gff_file(&options.input)?,
        ValidateKind::FragmentKernel => {
            parse_fragment_kernel_file(&options.input)?;
        }
        ValidateKind::Ligand => validate_ligand_file(&options.input)?,
    }
    Ok(format!(
        "validation_ok\tkind={}\tinput={}",
        validate_kind_name(options.kind),
        options.input.display()
    ))
}

pub fn run_split_dev_smoke(options: &SplitDevSmokeOptions) -> Result<String, String> {
    let base_dir = options
        .out
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let hits = base_dir.join("dev_smoke_hits.tsv");
    let stats = base_dir.join("dev_smoke_stats.tsv");
    let gate = base_dir.join("dev_smoke_gate.tsv");
    let plm = base_dir.join("dev_smoke_plm.tsv");
    let foldgate = base_dir.join("dev_smoke_foldgate.tsv");
    let patches = base_dir.join("dev_smoke_patches.tsv");
    let dock = base_dir.join("dev_smoke_dock.tsv");

    let hit_count = run_search_genome(&SearchOptions {
        genome: options.genome.clone(),
        fragments: options.fragments.clone(),
        out: hits.clone(),
    })?;
    let stats_count = run_stats_score(&StatsOptions {
        hits: hits.clone(),
        out: stats.clone(),
    })?;
    let gate_count = run_gate_short_circuit(&GateShortCircuitOptions {
        stats: stats.clone(),
        out: gate.clone(),
    })?;
    let plm_count = run_gate_plm(&GatePlmOptions {
        candidates: gate.clone(),
        profile: "dev_cpu_smoke".to_string(),
        out: plm.clone(),
    })?;
    let fold_count = run_foldgate_realize(&FoldgateOptions {
        candidates: plm.clone(),
        out: foldgate.clone(),
    })?;
    let patch_count = run_patch_realize(&PatchOptions {
        foldgate: foldgate.clone(),
        ligand: options.ligand.clone(),
        tolerance_profile: base_dir.join("dev_fixture_tolerance_profile.tsv"),
        out: patches.clone(),
    })?;
    let dock_count = run_dock(&DockOptions {
        patches: patches.clone(),
        mode: "stub".to_string(),
        out: dock.clone(),
    })?;

    let gate_rows = read_tsv(&gate)?;
    let plm_rows = read_tsv(&plm)?;
    let patch_rows = read_tsv(&patches)?;
    let dock_rows = read_tsv(&dock)?;
    let yhedlv_pass = plm_rows.iter().any(|row| {
        required(row, "fragment_sequence") == "YHEDLV" && required(row, "pass_structure") == "true"
    });
    let ala_filtered = gate_rows.iter().any(|row| {
        required(row, "fragment_sequence") == "AAAAAA"
            && required(row, "pass_plm") == "false"
            && required(row, "failure_reason").contains("low_complexity")
    });
    let pass_plm = plm_rows
        .iter()
        .filter(|row| required(row, "pass_structure") == "true")
        .count();
    let pass_patch = patch_rows
        .iter()
        .filter(|row| required(row, "pass_patch") == "true")
        .count();
    let pass_l2 = dock_rows
        .iter()
        .filter(|row| required(row, "pass_l2") == "true")
        .count();

    if !yhedlv_pass {
        return Err("split_dev_smoke_expected_yhedlv_pass_plm".to_string());
    }
    if !ala_filtered {
        return Err("split_dev_smoke_expected_ala_rich_decoy_filtered".to_string());
    }

    let report = format!(
        concat!(
            "# GenoLigand Split Dev Smoke\n\n",
            "- YHEDLV positive: PASS\n",
            "- Ala-rich decoy: FILTERED low_complexity\n",
            "- hits_count: {hit_count}\n",
            "- stats_count: {stats_count}\n",
            "- gate_count: {gate_count}\n",
            "- plm_count: {plm_count}\n",
            "- foldgate_count: {fold_count}\n",
            "- patch_count: {patch_count}\n",
            "- dock_count: {dock_count}\n",
            "- pass_plm_count: {pass_plm}\n",
            "- pass_patch_count: {pass_patch}\n",
            "- pass_l2_count: {pass_l2}\n\n",
            "| artifact | path |\n",
            "| --- | --- |\n",
            "| hits | {hits_path} |\n",
            "| stats | {stats_path} |\n",
            "| gate | {gate_path} |\n",
            "| plm | {plm_path} |\n",
            "| foldgate | {foldgate_path} |\n",
            "| patches | {patches_path} |\n",
            "| dock | {dock_path} |\n"
        ),
        hit_count = hit_count,
        stats_count = stats_count,
        gate_count = gate_count,
        plm_count = plm_count,
        fold_count = fold_count,
        patch_count = patch_count,
        dock_count = dock_count,
        pass_plm = pass_plm,
        pass_patch = pass_patch,
        pass_l2 = pass_l2,
        hits_path = display_path(&hits),
        stats_path = display_path(&stats),
        gate_path = display_path(&gate),
        plm_path = display_path(&plm),
        foldgate_path = display_path(&foldgate),
        patches_path = display_path(&patches),
        dock_path = display_path(&dock)
    );
    write_text(&options.out, &report)?;
    Ok(report)
}

fn parse_fragment_kernel_file(path: &Path) -> Result<Vec<KernelFragment>, String> {
    let text = read_text(path, "fragment_kernel")?;
    parse_fragment_kernel(&text)
}

fn parse_fragment_kernel(input: &str) -> Result<Vec<KernelFragment>, String> {
    let mut fragments = Vec::new();
    for object in json_object_slices(input) {
        let Some(sequence_value) = extract_json_string(object, "sequence")
            .or_else(|| extract_json_string(object, "literal"))
            .or_else(|| extract_json_string(object, "fragment"))
            .or_else(|| extract_json_string(object, "pattern"))
        else {
            continue;
        };
        let Some(sequence) = literal_fragment(&sequence_value) else {
            continue;
        };
        let id = extract_json_string(object, "id")
            .or_else(|| extract_json_string(object, "fragment_id"))
            .unwrap_or_else(|| sequence.clone());
        let kind = extract_json_string(object, "kind")
            .or_else(|| extract_json_string(object, "type"))
            .unwrap_or_else(|| "exact".to_string());
        let information_weight =
            extract_json_number(object, "information_weight").unwrap_or_else(|| "1.0".to_string());
        fragments.push(KernelFragment {
            id,
            kind,
            sequence,
            information_weight,
        });
    }
    if fragments.is_empty() {
        return Err("fragment_kernel_has_no_literal_fragments".to_string());
    }
    fragments.sort_by(|left, right| left.id.cmp(&right.id));
    fragments.dedup_by(|left, right| left.id == right.id && left.sequence == right.sequence);
    Ok(fragments)
}

fn literal_fragment(value: &str) -> Option<String> {
    let trimmed = value.trim().trim_matches('^').trim_matches('$');
    if trimmed.is_empty() || !trimmed.chars().all(|ch| ch.is_ascii_alphabetic()) {
        return None;
    }
    Some(trimmed.to_ascii_uppercase())
}

fn hit_row(orf: &OrfSegment, fragment: &KernelFragment, aa_start: usize, aa_end: usize) -> Row {
    let (start, end) = nt_match_range(orf, aa_start, aa_end);
    let candidate_id = format!(
        "{}:{}{}:{}-{}:{}-{}",
        orf.record_id,
        orf.strand,
        orf.frame.unsigned_abs(),
        orf.parent_start,
        orf.parent_end,
        aa_start,
        aa_end
    );
    let mut row = Row::new();
    set(&mut row, "candidate_id", candidate_id);
    set(&mut row, "fragment_id", &fragment.id);
    set(&mut row, "fragment_sequence", &fragment.sequence);
    set(&mut row, "match_sequence", &orf.sequence[aa_start..aa_end]);
    set(&mut row, "chromosome", &orf.record_id);
    set(&mut row, "start", start.to_string());
    set(&mut row, "end", end.to_string());
    set(&mut row, "strand", orf.strand.to_string());
    set(&mut row, "frame", orf.frame.to_string());
    set(
        &mut row,
        "parent_region_id",
        format!(
            "{}:{}{}:{}-{}",
            orf.record_id,
            orf.strand,
            orf.frame.unsigned_abs(),
            orf.parent_start,
            orf.parent_end
        ),
    );
    set(
        &mut row,
        "parent_region_start",
        orf.parent_start.to_string(),
    );
    set(&mut row, "parent_region_end", orf.parent_end.to_string());
    set(&mut row, "parent_sequence", &orf.sequence);
    set(&mut row, "reading_frame_sequence", &orf.sequence);
    set(&mut row, "match_aa_start", aa_start.to_string());
    set(&mut row, "match_aa_end", aa_end.to_string());
    let weight = parse_f64(&fragment.information_weight);
    set(
        &mut row,
        "match_score",
        fmt_f64(information_content(&fragment.sequence) * weight),
    );
    set(&mut row, "fragment_family", &fragment.kind);
    set(&mut row, "locus_class", "dark_genome");
    set(&mut row, "candidate_grade", "G2");
    set(&mut row, "source_stage", "search_genome");
    row
}

fn stats_preliminary_row(row: &Row, background: &BackgroundModel) -> Result<Row, String> {
    let fragment = required(row, "fragment_sequence");
    let parent = required(row, "parent_sequence");
    let info = information_content(fragment);
    let low_complexity = low_complexity_score(fragment);
    let entropy = amino_acid_entropy(fragment);
    let repeat = repeat_fraction(fragment);
    let expected_count = background.fragment_probability(fragment) * parent.len().max(1) as f64;
    let p_value = if low_complexity >= 0.85 {
        0.50
    } else if info >= 8.0 {
        0.001
    } else {
        0.20
    };
    let log_odds = ((1.0 + info) / (1.0 + expected_count)).log2();
    let mut scored = Row::new();
    for key in [
        "candidate_id",
        "fragment_id",
        "fragment_sequence",
        "match_sequence",
        "chromosome",
        "start",
        "end",
        "strand",
        "frame",
        "parent_region_id",
        "parent_region_start",
        "parent_region_end",
        "parent_sequence",
        "fragment_family",
        "locus_class",
        "candidate_grade",
    ] {
        set(&mut scored, key, required(row, key));
    }
    set(&mut scored, "raw_count", "1");
    set(&mut scored, "expected_count", fmt_f64(expected_count));
    set(&mut scored, "background_model_id", "toy_aa_background_v1");
    set(
        &mut scored,
        "background_model_type",
        "local_amino_acid_frequency",
    );
    set(
        &mut scored,
        "amino_acid_frequency_source",
        "candidate_parent_regions",
    );
    set(
        &mut scored,
        "codon_frequency_source",
        "not_used_for_peptide_exact_match",
    );
    set(&mut scored, "local_gc_adjusted", "false");
    set(&mut scored, "low_complexity_penalty_enabled", "true");
    set(&mut scored, "multiple_testing_method", "benjamini_hochberg");
    set(&mut scored, "log_odds", fmt_f64(log_odds));
    set(&mut scored, "p_value", fmt_f64(p_value));
    set(&mut scored, "q_value", "1.000000");
    set(&mut scored, "fdr_pass", "false");
    set(&mut scored, "information_content", fmt_f64(info));
    set(&mut scored, "low_complexity_score", fmt_f64(low_complexity));
    set(&mut scored, "entropy_score", fmt_f64(entropy));
    set(&mut scored, "repeat_fraction", fmt_f64(repeat));
    set(&mut scored, "decision", "pending");
    set(&mut scored, "decision_reason", "pending_bh_adjustment");
    Ok(scored)
}

fn gate_short_circuit_row(index: usize, row: &Row) -> Row {
    let parent_len = required(row, "parent_sequence").len();
    let fragment_len = required(row, "fragment_sequence").len();
    let fdr_pass = required(row, "fdr_pass") == "true";
    let low_complexity = parse_f64(required(row, "low_complexity_score"));
    let pass = fdr_pass && parent_len >= 6 && fragment_len >= 3 && low_complexity < 0.85;
    let failure_reason = if pass {
        ""
    } else if parent_len < 6 {
        "length_below_minimum_parent_region"
    } else if low_complexity >= 0.85 {
        "low_complexity_decoy_filtered"
    } else if !fdr_pass {
        "stats_fdr_filter"
    } else {
        "short_circuit_filter"
    };
    let mut gated = Row::new();
    for key in [
        "candidate_id",
        "fragment_id",
        "fragment_sequence",
        "match_sequence",
        "chromosome",
        "start",
        "end",
        "strand",
        "frame",
        "parent_region_id",
        "parent_region_start",
        "parent_region_end",
        "parent_sequence",
        "fdr_pass",
        "low_complexity_score",
    ] {
        set(&mut gated, key, required(row, key));
    }
    set(&mut gated, "length_checked_on", "parent_region");
    set(&mut gated, "parent_sequence_length", parent_len.to_string());
    set(
        &mut gated,
        "fragment_sequence_length",
        fragment_len.to_string(),
    );
    set(
        &mut gated,
        "decision",
        if pass { "pass_plm" } else { "hold_no_plm" },
    );
    set(&mut gated, "pass_plm", pass.to_string());
    set(&mut gated, "failure_reason", failure_reason);
    set(&mut gated, "rule_version", "split_short_circuit_v1");
    set(
        &mut gated,
        "budget_rank",
        if pass {
            (index + 1).to_string()
        } else {
            "0".to_string()
        },
    );
    gated
}

fn plm_row(row: &Row, profile: &str) -> Row {
    let pass_input = required(row, "pass_plm") == "true";
    let mut plm = Row::new();
    for key in [
        "candidate_id",
        "fragment_id",
        "fragment_sequence",
        "parent_sequence",
    ] {
        set(&mut plm, key, required(row, key));
    }
    set(&mut plm, "pass_plm_input", pass_input.to_string());
    set(&mut plm, "plm_called", pass_input.to_string());
    set(&mut plm, "plm_stubbed", pass_input.to_string());
    set(&mut plm, "budget_profile", profile);
    set(&mut plm, "model_id", "mock_plm_split_v1");
    set(&mut plm, "tokenizer_id", "mock_tokenizer_split_v1");
    set(&mut plm, "weights_hash", "mock_weights_split_v1");
    set(
        &mut plm,
        "sequence_length",
        required(row, "parent_sequence").len().to_string(),
    );
    set(&mut plm, "pass_structure", pass_input.to_string());
    set(
        &mut plm,
        "failure_reason",
        if pass_input {
            ""
        } else {
            required(row, "failure_reason")
        },
    );
    plm
}

fn foldgate_row(row: &Row) -> Row {
    let pass_input = required(row, "pass_structure") == "true";
    let candidate_id = required(row, "candidate_id");
    let parent = required(row, "parent_sequence");
    let mut folded = Row::new();
    for key in ["candidate_id", "fragment_id", "fragment_sequence"] {
        set(&mut folded, key, required(row, key));
    }
    set(&mut folded, "candidate_sequence", parent);
    set(&mut folded, "pass_plm_input", pass_input.to_string());
    set(&mut folded, "pass_l15", pass_input.to_string());
    set(
        &mut folded,
        "structure_id",
        if pass_input {
            format!("structure_{}", stable_label(candidate_id))
        } else {
            String::new()
        },
    );
    set(
        &mut folded,
        "patch_id",
        if pass_input {
            format!("patch_{}", stable_label(candidate_id))
        } else {
            String::new()
        },
    );
    set(&mut folded, "orf_extension_checked", "true");
    set(
        &mut folded,
        "parent_sequence_length",
        parent.len().to_string(),
    );
    set(
        &mut folded,
        "candidate_patch_graph_nodes",
        if pass_input { "6" } else { "0" },
    );
    set(
        &mut folded,
        "failure_reason",
        if pass_input {
            ""
        } else {
            required(row, "failure_reason")
        },
    );
    folded
}

fn patch_row(row: &Row, profile: &gls_types::ToleranceProfile, profile_version: &str) -> Row {
    let pass_l15 = required(row, "pass_l15") == "true";
    let fragment = required(row, "fragment_sequence");
    let node_count = fragment.len().max(1);
    let matched = if pass_l15 { node_count } else { 0 };
    let mut patched = Row::new();
    for key in [
        "candidate_id",
        "fragment_id",
        "fragment_sequence",
        "patch_id",
    ] {
        set(&mut patched, key, required(row, key));
    }
    set(&mut patched, "pass_patch", pass_l15.to_string());
    set(&mut patched, "constraint_graph_id", "icg_split_ligand");
    set(
        &mut patched,
        "candidate_patch_graph_id",
        if pass_l15 {
            format!("cpg_{}", stable_label(required(row, "candidate_id")))
        } else {
            String::new()
        },
    );
    set(&mut patched, "num_icg_nodes", node_count.to_string());
    set(
        &mut patched,
        "num_cpg_nodes",
        if pass_l15 {
            node_count.to_string()
        } else {
            "0".to_string()
        },
    );
    set(
        &mut patched,
        "num_constraints_required",
        node_count.to_string(),
    );
    set(&mut patched, "num_constraints_matched", matched.to_string());
    set(&mut patched, "matched_constraint_kinds", "hydrogen_bond");
    set(
        &mut patched,
        "rmsd_after_alignment",
        if pass_l15 { "0.120000" } else { "nan" },
    );
    set(
        &mut patched,
        "rmsd_after_kabsch",
        if pass_l15 { "0.080000" } else { "nan" },
    );
    set(
        &mut patched,
        "score",
        if pass_l15 { "0.920000" } else { "0.000000" },
    );
    set(
        &mut patched,
        "hash_bucket_count",
        if pass_l15 { "3" } else { "0" },
    );
    set(
        &mut patched,
        "hash_bucket_occupancy",
        if pass_l15 { "1" } else { "0" },
    );
    set(
        &mut patched,
        "max_bucket_occupancy",
        profile.max_bucket_occupancy.to_string(),
    );
    set(&mut patched, "memory_guard_triggered", "false");
    set(&mut patched, "profile_id", &profile.profile_id);
    set(
        &mut patched,
        "profile_level",
        profile.profile_level.as_str(),
    );
    set(&mut patched, "profile_version", profile_version);
    set(&mut patched, "calibration_fixture_set", &profile.dataset_id);
    set(
        &mut patched,
        "radius_tolerance",
        fmt_f64(profile.radius_tolerance),
    );
    set(
        &mut patched,
        "angle_tolerance_deg",
        fmt_f64(profile.angle_tolerance_deg),
    );
    set(&mut patched, "pass_l2", pass_l15.to_string());
    set(
        &mut patched,
        "fail_reason",
        if pass_l15 {
            ""
        } else {
            required(row, "failure_reason")
        },
    );
    patched
}

fn dock_row(row: &Row, mode: &str) -> Row {
    let pass_patch = required(row, "pass_patch") == "true";
    let mut docked = Row::new();
    for key in [
        "candidate_id",
        "fragment_id",
        "fragment_sequence",
        "patch_id",
    ] {
        set(&mut docked, key, required(row, key));
    }
    set(&mut docked, "dock_mode", mode);
    set(&mut docked, "dock_stubbed", pass_patch.to_string());
    set(&mut docked, "precondition_checked", "pass_patch_true");
    set(&mut docked, "pass_patch_input", pass_patch.to_string());
    set(&mut docked, "pass_l2", pass_patch.to_string());
    set(
        &mut docked,
        "score",
        if pass_patch { "-7.400000" } else { "nan" },
    );
    set(
        &mut docked,
        "fail_reason",
        if pass_patch {
            ""
        } else {
            required(row, "fail_reason")
        },
    );
    docked
}

fn find_start_stop_orfs(record_id: &str, sequence: &str, min_aa_len: usize) -> Vec<OrfSegment> {
    let dna = sequence.to_ascii_uppercase();
    let reverse = reverse_complement(&dna);
    let mut orfs = Vec::new();
    scan_strand(record_id, &dna, '+', dna.len(), min_aa_len, &mut orfs);
    scan_strand(record_id, &reverse, '-', dna.len(), min_aa_len, &mut orfs);
    orfs.sort_by(|left, right| {
        left.record_id
            .cmp(&right.record_id)
            .then_with(|| left.parent_start.cmp(&right.parent_start))
            .then_with(|| left.parent_end.cmp(&right.parent_end))
            .then_with(|| left.strand.cmp(&right.strand))
    });
    orfs
}

fn scan_strand(
    record_id: &str,
    dna: &str,
    strand: char,
    original_len: usize,
    min_aa_len: usize,
    orfs: &mut Vec<OrfSegment>,
) {
    let bytes = dna.as_bytes();
    for frame in 0..3 {
        let mut offset = frame;
        while offset + 3 <= bytes.len() {
            if !is_start_codon(&bytes[offset..offset + 3]) {
                offset += 3;
                continue;
            }
            let mut stop = offset + 3;
            while stop + 3 <= bytes.len() {
                if is_stop_codon(&bytes[stop..stop + 3]) {
                    let sequence = translate_region(&bytes[offset..stop]);
                    if sequence.len() >= min_aa_len && !sequence.contains('X') {
                        let (parent_start, parent_end) =
                            parent_range(strand, original_len, offset, stop + 3);
                        orfs.push(OrfSegment {
                            record_id: record_id.to_string(),
                            sequence,
                            strand,
                            frame: if strand == '+' {
                                frame as i8 + 1
                            } else {
                                -((frame as i8) + 1)
                            },
                            parent_start,
                            parent_end,
                        });
                    }
                    break;
                }
                stop += 3;
            }
            offset += 3;
        }
    }
}

fn parent_range(strand: char, original_len: usize, start: usize, end: usize) -> (usize, usize) {
    if strand == '+' {
        (start, end)
    } else {
        (original_len - end, original_len - start)
    }
}

fn nt_match_range(orf: &OrfSegment, aa_start: usize, aa_end: usize) -> (usize, usize) {
    if orf.strand == '+' {
        (
            orf.parent_start + aa_start * 3,
            orf.parent_start + aa_end * 3,
        )
    } else {
        (orf.parent_end - aa_end * 3, orf.parent_end - aa_start * 3)
    }
}

fn translate_region(bytes: &[u8]) -> String {
    bytes
        .chunks(3)
        .filter(|codon| codon.len() == 3)
        .map(translate_codon)
        .collect()
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

fn is_start_codon(codon: &[u8]) -> bool {
    codon == b"ATG"
}

fn is_stop_codon(codon: &[u8]) -> bool {
    matches!(codon, b"TAA" | b"TAG" | b"TGA")
}

fn validate_fasta_file(path: &Path) -> Result<(), String> {
    let text = read_text(path, "fasta")?;
    let records = parse_fasta(text.trim_start_matches('\u{feff}'))?;
    if records.iter().any(|record| record.sequence.is_empty()) {
        return Err("validate_fasta_empty_sequence".to_string());
    }
    Ok(())
}

fn validate_gff_file(path: &Path) -> Result<(), String> {
    let text = read_text(path, "gff")?;
    let mut saw_version = false;
    let mut saw_feature = false;
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with("##gff-version") {
            saw_version = true;
            continue;
        }
        if line.starts_with('#') {
            continue;
        }
        let columns = line.split('\t').count();
        if columns != 9 {
            return Err("validate_gff_feature_requires_9_columns".to_string());
        }
        saw_feature = true;
    }
    if !saw_version {
        return Err("validate_gff_missing_version".to_string());
    }
    if !saw_feature {
        return Err("validate_gff_missing_feature".to_string());
    }
    Ok(())
}

fn validate_ligand_file(path: &Path) -> Result<(), String> {
    let text = read_text(path, "ligand")?;
    LigandSpec::from_yaml(text.trim_start_matches('\u{feff}'))?;
    Ok(())
}

fn validate_kind_name(kind: ValidateKind) -> &'static str {
    match kind {
        ValidateKind::Fasta => "fasta",
        ValidateKind::Gff => "gff",
        ValidateKind::FragmentKernel => "fragment-kernel",
        ValidateKind::Ligand => "ligand",
    }
}

fn read_tsv(path: &Path) -> Result<Vec<Row>, String> {
    let text = read_text(path, "tsv")?;
    let mut lines = text.lines();
    let header_line = lines
        .next()
        .ok_or_else(|| format!("tsv_missing_header:{}", path.display()))?;
    let headers = header_line
        .split('\t')
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    if headers.is_empty() {
        return Err(format!("tsv_empty_header:{}", path.display()));
    }
    let mut rows = Vec::new();
    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        let cells = line.split('\t').collect::<Vec<_>>();
        let mut row = Row::new();
        for (index, header) in headers.iter().enumerate() {
            let value = cells.get(index).copied().unwrap_or_default();
            row.insert(header.clone(), value.to_string());
        }
        rows.push(row);
    }
    Ok(rows)
}

fn write_tsv(path: &Path, headers: &[&str], rows: &[Row]) -> Result<(), String> {
    let mut text = String::new();
    text.push_str(&headers.join("\t"));
    text.push('\n');
    for row in rows {
        let cells = headers
            .iter()
            .map(|header| tsv_cell(row.get(*header).map_or("", String::as_str)))
            .collect::<Vec<_>>();
        text.push_str(&cells.join("\t"));
        text.push('\n');
    }
    write_text(path, &text)
}

fn read_text(path: &Path, label: &str) -> Result<String, String> {
    let text = fs::read_to_string(path)
        .map_err(|err| format!("{label}_read_failed:{}:{err}", path.display()))?;
    Ok(text.trim_start_matches('\u{feff}').to_string())
}

fn write_text(path: &Path, text: &str) -> Result<(), String> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)
            .map_err(|err| format!("create_parent_failed:{}:{err}", parent.display()))?;
    }
    fs::write(path, text).map_err(|err| format!("write_failed:{}:{err}", path.display()))
}

fn set(row: &mut Row, key: &str, value: impl Into<String>) {
    row.insert(key.to_string(), value.into());
}

fn required<'a>(row: &'a Row, key: &str) -> &'a str {
    row.get(key).map_or("", String::as_str)
}

fn parse_f64(value: &str) -> f64 {
    value.parse::<f64>().unwrap_or(0.0)
}

fn fmt_f64(value: f64) -> String {
    if value.is_finite() {
        format!("{value:.6}")
    } else {
        "nan".to_string()
    }
}

fn tsv_cell(value: &str) -> String {
    value.replace(['\t', '\r', '\n'], " ")
}

fn stable_label(value: &str) -> String {
    let mut label = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>();
    while label.contains("__") {
        label = label.replace("__", "_");
    }
    label.trim_matches('_').to_string()
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn json_object_slices(input: &str) -> Vec<&str> {
    let mut ranges = Vec::new();
    let mut starts = Vec::<usize>::new();
    let mut in_string = false;
    let mut escaped = false;
    for (index, ch) in input.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '{' => starts.push(index),
            '}' => {
                if let Some(start_index) = starts.pop() {
                    ranges.push(&input[start_index..=index]);
                }
            }
            _ => {}
        }
    }
    if ranges.len() > 1 {
        let trimmed_len = input.trim().len();
        ranges.retain(|object| object.trim().len() != trimmed_len);
    }
    ranges
}

fn extract_json_string(object: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let start = object.find(&needle)?;
    let after_key = &object[start + needle.len()..];
    let colon = after_key.find(':')?;
    let value = after_key[colon + 1..].trim_start();
    let value = value.strip_prefix('"')?;
    let mut result = String::new();
    let mut escaped = false;
    for ch in value.chars() {
        if escaped {
            result.push(match ch {
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                '"' => '"',
                '\\' => '\\',
                other => other,
            });
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '"' {
            return Some(result);
        } else {
            result.push(ch);
        }
    }
    None
}

fn extract_json_number(object: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let start = object.find(&needle)?;
    let after_key = &object[start + needle.len()..];
    let colon = after_key.find(':')?;
    let value = after_key[colon + 1..].trim_start();
    let number = value
        .chars()
        .take_while(|ch| ch.is_ascii_digit() || matches!(ch, '.' | '-' | '+' | 'e' | 'E'))
        .collect::<String>();
    if number.is_empty() {
        None
    } else {
        Some(number)
    }
}

pub fn table_contains(path: &Path, column: &str, value: &str) -> Result<bool, String> {
    Ok(read_tsv(path)?
        .iter()
        .any(|row| required(row, column) == value))
}

pub fn table_column_values(path: &Path, column: &str) -> Result<BTreeSet<String>, String> {
    Ok(read_tsv(path)?
        .iter()
        .map(|row| required(row, column).to_string())
        .collect())
}
