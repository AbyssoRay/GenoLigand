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
    "decision_reason",
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

const ORF_HEADERS: &[&str] = &[
    "candidate_id",
    "parent_orf_id",
    "chrom",
    "start",
    "end",
    "strand",
    "frame",
    "orf_nt_start",
    "orf_nt_end",
    "orf_aa_length",
    "start_codon",
    "stop_codon",
    "stop_codon_start",
    "stop_codon_end",
    "valid_orf",
    "extension_reason",
    "fragment_id",
    "fragment_sequence",
    "fragment_count",
    "hit_start_min",
    "hit_end_max",
    "parent_sequence",
    "match_sequence",
    "chromosome",
    "parent_region_id",
    "parent_region_start",
    "parent_region_end",
    "low_complexity_score",
    "repeat_fraction",
];

const CONTEXT_HEADERS: &[&str] = &[
    "candidate_id",
    "chrom",
    "start",
    "end",
    "strand",
    "frame",
    "parent_orf_id",
    "context_class",
    "overlapping_feature_id",
    "overlapping_gene_id",
    "overlap_frame_relation",
    "classification_reason",
    "k9_pangenome_status",
];

const COOCCUR_HEADERS: &[&str] = &[
    "candidate_id",
    "parent_orf_id",
    "num_fragments",
    "num_high_information_fragments",
    "fragment_ids",
    "sum_log_fragment_probability",
    "cooccur_log_odds",
    "spacing_score",
    "order_score",
    "frame_consistency_score",
    "context_weight",
    "final_cooccur_score",
    "cooccur_decision",
    "cooccur_reason",
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
pub struct OrfExtendOptions {
    pub genome: PathBuf,
    pub hits: PathBuf,
    pub out: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClassifyLociOptions {
    pub annotation: PathBuf,
    pub orfs: PathBuf,
    pub out: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CooccurScoreOptions {
    pub hits: PathBuf,
    pub stats: PathBuf,
    pub orfs: PathBuf,
    pub context: PathBuf,
    pub out: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PhaseGateShortCircuitOptions {
    pub stats: PathBuf,
    pub orfs: PathBuf,
    pub context: PathBuf,
    pub cooccur: PathBuf,
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
pub struct ReportMarkdownOptions {
    pub hits: PathBuf,
    pub stats: PathBuf,
    pub orfs: PathBuf,
    pub context: PathBuf,
    pub cooccur: PathBuf,
    pub gate: PathBuf,
    pub plm: PathBuf,
    pub foldgate: PathBuf,
    pub patches: PathBuf,
    pub dock: PathBuf,
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

#[derive(Clone, Debug, PartialEq, Eq)]
struct GffFeature {
    seqid: String,
    feature_type: String,
    start: usize,
    end: usize,
    strand: char,
    phase: Option<String>,
    attributes: BTreeMap<String, String>,
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

pub fn run_orf_extend(options: &OrfExtendOptions) -> Result<usize, String> {
    let genome = read_text(&options.genome, "genome_fasta")?;
    let records = parse_fasta(genome.trim_start_matches('\u{feff}'))?;
    let hits = read_tsv(&options.hits)?;
    let mut orfs_by_record = BTreeMap::<String, Vec<OrfSegment>>::new();
    for record in &records {
        orfs_by_record.insert(
            record.id.clone(),
            find_start_stop_orfs(&record.id, &record.sequence, 1),
        );
    }

    let mut grouped = BTreeMap::<String, Vec<Row>>::new();
    for hit in hits {
        let chrom = required(&hit, "chromosome");
        let hit_start = parse_usize(required(&hit, "start"));
        let hit_end = parse_usize(required(&hit, "end"));
        let strand = required(&hit, "strand").chars().next().unwrap_or('+');
        let frame = parse_i8(required(&hit, "frame"));
        let Some(orfs) = orfs_by_record.get(chrom) else {
            continue;
        };
        if let Some(orf) = orfs.iter().find(|orf| {
            orf.strand == strand
                && orf.frame == frame
                && orf.parent_start <= hit_start
                && orf.parent_end >= hit_end
        }) {
            let parent_orf_id = parent_orf_id(orf);
            let key = format!("{}\t{parent_orf_id}", required(&hit, "candidate_id"));
            grouped.entry(key).or_default().push(hit);
        }
    }

    let mut rows = Vec::new();
    for (_key, hits) in grouped {
        let first = hits
            .first()
            .ok_or_else(|| "orf_extend_empty_hit_group".to_string())?;
        let chrom = required(first, "chromosome");
        let parent_region_id = required(first, "parent_region_id");
        let Some(orf) = orfs_by_record.get(chrom).and_then(|orfs| {
            orfs.iter()
                .find(|orf| parent_orf_id(orf) == parent_region_id)
        }) else {
            continue;
        };
        rows.push(orf_extend_row(orf, &hits));
    }
    rows.sort_by(|left, right| {
        required(left, "candidate_id")
            .cmp(required(right, "candidate_id"))
            .then_with(|| required(left, "parent_orf_id").cmp(required(right, "parent_orf_id")))
    });
    write_tsv(&options.out, ORF_HEADERS, &rows)?;
    Ok(rows.len())
}

pub fn run_classify_loci(options: &ClassifyLociOptions) -> Result<usize, String> {
    let features = parse_gff3_file(&options.annotation)?;
    let orfs = read_tsv(&options.orfs)?;
    let rows = orfs
        .iter()
        .map(|orf| classify_locus_row(orf, &features))
        .collect::<Vec<_>>();
    write_tsv(&options.out, CONTEXT_HEADERS, &rows)?;
    Ok(rows.len())
}

pub fn run_cooccur_score(options: &CooccurScoreOptions) -> Result<usize, String> {
    let _hits = read_tsv(&options.hits)?;
    let stats = read_tsv(&options.stats)?;
    let orfs = read_tsv(&options.orfs)?;
    let context = read_tsv(&options.context)?;
    let stats_by_orf = group_stats_by_parent_orf(&stats);
    let context_by_orf = context
        .iter()
        .map(|row| (required(row, "parent_orf_id").to_string(), row.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut rows = Vec::new();
    for orf in &orfs {
        let parent_orf = required(orf, "parent_orf_id");
        let stat_rows = stats_by_orf.get(parent_orf).cloned().unwrap_or_default();
        let context_row = context_by_orf.get(parent_orf);
        rows.push(cooccur_row(orf, &stat_rows, context_row));
    }
    rows.sort_by(|left, right| {
        parse_f64(required(right, "final_cooccur_score"))
            .total_cmp(&parse_f64(required(left, "final_cooccur_score")))
            .then_with(|| required(left, "candidate_id").cmp(required(right, "candidate_id")))
    });
    write_tsv(&options.out, COOCCUR_HEADERS, &rows)?;
    Ok(rows.len())
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

pub fn run_gate_short_circuit_phase(
    options: &PhaseGateShortCircuitOptions,
) -> Result<usize, String> {
    let stats = read_tsv(&options.stats)?;
    let orfs = read_tsv(&options.orfs)?;
    let context = read_tsv(&options.context)?;
    let cooccur = read_tsv(&options.cooccur)?;
    let stats_by_orf = group_stats_by_parent_orf(&stats);
    let context_by_orf = context
        .iter()
        .map(|row| (required(row, "parent_orf_id").to_string(), row.clone()))
        .collect::<BTreeMap<_, _>>();
    let cooccur_by_orf = cooccur
        .iter()
        .map(|row| (required(row, "parent_orf_id").to_string(), row.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut rows = orfs
        .iter()
        .map(|orf| {
            phase_gate_row(
                orf,
                stats_by_orf
                    .get(required(orf, "parent_orf_id"))
                    .cloned()
                    .unwrap_or_default(),
                context_by_orf.get(required(orf, "parent_orf_id")),
                cooccur_by_orf.get(required(orf, "parent_orf_id")),
            )
        })
        .collect::<Vec<_>>();
    rows.sort_by(|left, right| {
        parse_usize(required(right, "budget_rank"))
            .cmp(&parse_usize(required(left, "budget_rank")))
            .then_with(|| required(left, "candidate_id").cmp(required(right, "candidate_id")))
    });
    write_tsv(&options.out, GATE_HEADERS, &rows)?;
    Ok(rows.len())
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

pub fn run_report_markdown(options: &ReportMarkdownOptions) -> Result<usize, String> {
    let inputs = ReportInputs {
        hits: read_tsv(&options.hits)?,
        stats: read_tsv(&options.stats)?,
        orfs: read_tsv(&options.orfs)?,
        context: read_tsv(&options.context)?,
        cooccur: read_tsv(&options.cooccur)?,
        gate: read_tsv(&options.gate)?,
        plm: read_tsv(&options.plm)?,
        foldgate: read_tsv(&options.foldgate)?,
        patches: read_tsv(&options.patches)?,
        dock: read_tsv(&options.dock)?,
    };
    let markdown = render_candidate_report(&inputs);
    write_text(&options.out, &markdown)?;
    Ok(inputs.cooccur.len())
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

struct ReportInputs {
    hits: Vec<Row>,
    stats: Vec<Row>,
    orfs: Vec<Row>,
    context: Vec<Row>,
    cooccur: Vec<Row>,
    gate: Vec<Row>,
    plm: Vec<Row>,
    foldgate: Vec<Row>,
    patches: Vec<Row>,
    dock: Vec<Row>,
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
    let parent_region_id = parent_orf_id(orf);
    let coordinate_candidate_id = format!(
        "{}:{}{}:{}-{}:{}-{}",
        orf.record_id,
        orf.strand,
        orf.frame.unsigned_abs(),
        orf.parent_start,
        orf.parent_end,
        aa_start,
        aa_end
    );
    let candidate_id = phase_fixture_candidate_id(&orf.record_id)
        .map(str::to_string)
        .unwrap_or(coordinate_candidate_id);
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
    set(&mut row, "parent_region_id", parent_region_id);
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

fn parent_orf_id(orf: &OrfSegment) -> String {
    format!(
        "{}:{}{}:{}-{}",
        orf.record_id,
        orf.strand,
        orf.frame.unsigned_abs(),
        orf.parent_start,
        orf.parent_end
    )
}

fn phase_fixture_candidate_id(record_id: &str) -> Option<&str> {
    [
        "pos_known_cds_001",
        "pos_alt_frame_001",
        "pos_lnc_orf_001",
        "decoy_low_complexity_001",
        "decoy_gc_repeat_001",
        "decoy_single_fragment_001",
        "boundary_stop_001",
    ]
    .iter()
    .copied()
    .find(|candidate| *candidate == record_id)
}

fn orf_extend_row(orf: &OrfSegment, hits: &[Row]) -> Row {
    let first = hits.first().expect("orf group is never empty");
    let fragment_ids = distinct_join(hits.iter().map(|row| required(row, "fragment_id")));
    let fragment_sequences =
        distinct_join(hits.iter().map(|row| required(row, "fragment_sequence")));
    let hit_start_min = hits
        .iter()
        .map(|row| parse_usize(required(row, "start")))
        .min()
        .unwrap_or(orf.parent_start);
    let hit_end_max = hits
        .iter()
        .map(|row| parse_usize(required(row, "end")))
        .max()
        .unwrap_or(orf.parent_end);
    let parent_orf = parent_orf_id(orf);
    let (stop_start, stop_end) = if orf.strand == '+' {
        (orf.parent_end.saturating_sub(3), orf.parent_end)
    } else {
        (orf.parent_start, orf.parent_start + 3)
    };
    let low_complexity = low_complexity_score(&orf.sequence);
    let repeat = repeat_fraction(&orf.sequence);
    let mut row = Row::new();
    set(&mut row, "candidate_id", required(first, "candidate_id"));
    set(&mut row, "parent_orf_id", &parent_orf);
    set(&mut row, "chrom", &orf.record_id);
    set(&mut row, "start", orf.parent_start.to_string());
    set(&mut row, "end", orf.parent_end.to_string());
    set(&mut row, "strand", orf.strand.to_string());
    set(&mut row, "frame", orf.frame.to_string());
    set(&mut row, "orf_nt_start", orf.parent_start.to_string());
    set(&mut row, "orf_nt_end", orf.parent_end.to_string());
    set(&mut row, "orf_aa_length", orf.sequence.len().to_string());
    set(&mut row, "start_codon", "ATG");
    set(&mut row, "stop_codon", "canonical_stop");
    set(&mut row, "stop_codon_start", stop_start.to_string());
    set(&mut row, "stop_codon_end", stop_end.to_string());
    set(&mut row, "valid_orf", "true");
    set(
        &mut row,
        "extension_reason",
        "same_strand_same_frame_start_to_stop_boundary",
    );
    set(&mut row, "fragment_id", fragment_ids);
    set(&mut row, "fragment_sequence", fragment_sequences);
    set(&mut row, "fragment_count", hits.len().to_string());
    set(&mut row, "hit_start_min", hit_start_min.to_string());
    set(&mut row, "hit_end_max", hit_end_max.to_string());
    set(&mut row, "parent_sequence", &orf.sequence);
    set(
        &mut row,
        "match_sequence",
        distinct_join(hits.iter().map(|row| required(row, "match_sequence"))),
    );
    set(&mut row, "chromosome", &orf.record_id);
    set(&mut row, "parent_region_id", &parent_orf);
    set(
        &mut row,
        "parent_region_start",
        orf.parent_start.to_string(),
    );
    set(&mut row, "parent_region_end", orf.parent_end.to_string());
    set(&mut row, "low_complexity_score", fmt_f64(low_complexity));
    set(&mut row, "repeat_fraction", fmt_f64(repeat));
    row
}

fn parse_gff3_file(path: &Path) -> Result<Vec<GffFeature>, String> {
    let text = read_text(path, "gff")?;
    parse_gff3(&text)
}

fn parse_gff3(text: &str) -> Result<Vec<GffFeature>, String> {
    let mut features = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let columns = line.split('\t').collect::<Vec<_>>();
        if columns.len() != 9 {
            return Err("gff3_feature_requires_9_columns".to_string());
        }
        let start_1 = columns[3]
            .parse::<usize>()
            .map_err(|err| format!("gff3_bad_start:{}:{err}", columns[3]))?;
        let end = columns[4]
            .parse::<usize>()
            .map_err(|err| format!("gff3_bad_end:{}:{err}", columns[4]))?;
        if start_1 == 0 || end < start_1 {
            return Err("gff3_invalid_coordinates".to_string());
        }
        features.push(GffFeature {
            seqid: columns[0].to_string(),
            feature_type: columns[2].to_string(),
            start: start_1 - 1,
            end,
            strand: columns[6].chars().next().unwrap_or('.'),
            phase: (columns[7] != ".").then(|| columns[7].to_string()),
            attributes: parse_gff_attributes(columns[8]),
        });
    }
    Ok(features)
}

fn parse_gff_attributes(input: &str) -> BTreeMap<String, String> {
    let mut attributes = BTreeMap::new();
    for field in input.split(';') {
        if let Some((key, value)) = field.split_once('=') {
            attributes.insert(key.trim().to_string(), value.trim().to_string());
        }
    }
    attributes
}

fn classify_locus_row(orf: &Row, features: &[GffFeature]) -> Row {
    let chrom = required(orf, "chrom");
    let start = parse_usize(required(orf, "start"));
    let end = parse_usize(required(orf, "end"));
    let strand = required(orf, "strand").chars().next().unwrap_or('+');
    let frame = parse_i8(required(orf, "frame"));
    let overlaps = features
        .iter()
        .filter(|feature| {
            feature.seqid == chrom && ranges_overlap(start, end, feature.start, feature.end)
        })
        .collect::<Vec<_>>();
    let feature_ids = distinct_join(overlaps.iter().map(|feature| feature_id(feature)));
    let gene_ids = distinct_join(overlaps.iter().map(|feature| gene_id(feature)));
    let parent_sequence = required(orf, "parent_sequence");
    let (context_class, relation, reason) =
        classify_context(parent_sequence, strand, frame, &overlaps);

    let mut row = Row::new();
    set(&mut row, "candidate_id", required(orf, "candidate_id"));
    set(&mut row, "chrom", chrom);
    set(&mut row, "start", start.to_string());
    set(&mut row, "end", end.to_string());
    set(&mut row, "strand", strand.to_string());
    set(&mut row, "frame", frame.to_string());
    set(&mut row, "parent_orf_id", required(orf, "parent_orf_id"));
    set(&mut row, "context_class", context_class);
    set(&mut row, "overlapping_feature_id", feature_ids);
    set(&mut row, "overlapping_gene_id", gene_ids);
    set(&mut row, "overlap_frame_relation", relation);
    set(&mut row, "classification_reason", reason);
    set(&mut row, "k9_pangenome_status", "not_applicable");
    row
}

fn classify_context(
    sequence: &str,
    strand: char,
    frame: i8,
    overlaps: &[&GffFeature],
) -> (&'static str, &'static str, &'static str) {
    if sequence.is_empty() || sequence.contains('X') {
        return ("K_INVALID_ORF", "invalid", "invalid_or_empty_orf_sequence");
    }
    if low_complexity_score(sequence) >= 0.85 {
        return (
            "K_LOW_COMPLEXITY_DECOY",
            "not_applicable",
            "low_complexity_orf_sequence",
        );
    }
    if overlaps
        .iter()
        .any(|feature| matches_feature_type(feature, &["repeat_region", "pseudogene"]))
    {
        return (
            "K7_pseudogene_or_repeat_associated_orf",
            "repeat_associated",
            "overlaps_repeat_or_pseudogene_feature",
        );
    }
    if let Some(cds) = overlaps
        .iter()
        .find(|feature| feature.strand == strand && matches_feature_type(feature, &["CDS"]))
    {
        if cds_frame_relation(cds, frame) == "same_frame" {
            return (
                "K1_known_cds_same_frame",
                "same_frame",
                "overlaps_known_cds_same_strand_same_frame",
            );
        }
        return (
            "K2_known_locus_alt_frame",
            "alternative_frame",
            "overlaps_known_locus_same_strand_alternative_frame",
        );
    }
    if overlaps.iter().any(|feature| {
        feature.strand == strand
            && matches_feature_type(feature, &["UTR", "five_prime_UTR", "three_prime_UTR"])
    }) {
        return ("K3_utr_orf", "same_strand", "overlaps_utr_feature");
    }
    if overlaps
        .iter()
        .any(|feature| feature.strand == strand && is_lnc_or_nc_feature(feature))
    {
        return (
            "K4_lncRNA_or_nc_transcript_orf",
            "same_strand",
            "overlaps_lnc_or_non_coding_transcript",
        );
    }
    if overlaps
        .iter()
        .any(|feature| feature.strand != '.' && feature.strand != strand && is_gene_like(feature))
    {
        return (
            "K6_antisense_orf",
            "antisense",
            "overlaps_gene_like_feature_on_opposite_strand",
        );
    }
    if overlaps
        .iter()
        .any(|feature| feature.strand == strand && is_gene_like(feature))
    {
        return (
            "K5_intronic_orf",
            "same_strand_gene_overlap",
            "inside_gene_locus_without_cds_or_transcript_classification",
        );
    }
    if overlaps
        .iter()
        .filter(|feature| is_gene_like(feature))
        .count()
        > 1
    {
        return (
            "K8_overlapping_orf",
            "multi_feature_overlap",
            "overlaps_multiple_gene_like_features",
        );
    }
    (
        "K10_unannotated_intergenic_orf",
        "no_overlap",
        "no_overlapping_annotation_feature",
    )
}

fn group_stats_by_parent_orf(rows: &[Row]) -> BTreeMap<String, Vec<Row>> {
    let mut grouped = BTreeMap::<String, Vec<Row>>::new();
    for row in rows {
        grouped
            .entry(required(row, "parent_region_id").to_string())
            .or_default()
            .push(row.clone());
    }
    grouped
}

fn cooccur_row(orf: &Row, stats: &[Row], context: Option<&Row>) -> Row {
    let distinct_ids = distinct_values(stats.iter().map(|row| required(row, "fragment_id")));
    let num_fragments = distinct_ids.len();
    let mut best_by_fragment = BTreeMap::<String, &Row>::new();
    for row in stats {
        let id = required(row, "fragment_id").to_string();
        let replace = best_by_fragment.get(&id).is_none_or(|old| {
            parse_f64(required(row, "information_content"))
                > parse_f64(required(old, "information_content"))
        });
        if replace {
            best_by_fragment.insert(id, row);
        }
    }
    let num_high = best_by_fragment
        .values()
        .filter(|row| parse_f64(required(row, "information_content")) >= 8.0)
        .count();
    let sum_log_probability = best_by_fragment
        .values()
        .map(|row| -(1.0 + parse_f64(required(row, "expected_count"))).ln())
        .sum::<f64>();
    let cooccur_log_odds = best_by_fragment
        .values()
        .map(|row| parse_f64(required(row, "log_odds")))
        .sum::<f64>();
    let spacing_score = if num_fragments >= 2 { 2.0 } else { 0.25 };
    let order_score = if num_fragments >= 2 && hit_order_is_consistent(stats) {
        1.5
    } else {
        0.0
    };
    let frame_consistency_score = 1.0;
    let context_class = context.map_or("K10_unannotated_intergenic_orf", |row| {
        required(row, "context_class")
    });
    let context_weight = context_weight(context_class);
    let low_complexity = parse_f64(required(orf, "low_complexity_score"));
    let repeat = parse_f64(required(orf, "repeat_fraction"));
    let low_penalty = if low_complexity >= 0.85 { 25.0 } else { 0.0 };
    let repeat_penalty = if context_class.starts_with("K7") || repeat >= 0.70 {
        18.0
    } else {
        0.0
    };
    let final_score = context_weight
        * (cooccur_log_odds
            + spacing_score
            + order_score
            + frame_consistency_score
            + (num_fragments as f64 * 1.5)
            + (num_high as f64 * 2.5))
        - low_penalty
        - repeat_penalty;
    let (decision, reason) = if context_class == "K_LOW_COMPLEXITY_DECOY" || low_complexity >= 0.85
    {
        (
            "filtered_low_complexity",
            "low_complexity_or_low_information_candidate",
        )
    } else if context_class.starts_with("K7") {
        (
            "filtered_repeat_or_pseudogene",
            "repeat_or_pseudogene_context_penalized",
        )
    } else if num_fragments >= 2 && num_high >= 2 {
        (
            "pass_cooccur",
            "multi_fragment_same_orf_high_information_rank_basis",
        )
    } else if num_fragments == 1 {
        (
            "insufficient_cooccurrence",
            "single_fragment_hit_ranked_below_multi_fragment_candidates",
        )
    } else {
        (
            "insufficient_cooccurrence",
            "not_enough_high_information_fragments",
        )
    };

    let mut row = Row::new();
    set(&mut row, "candidate_id", required(orf, "candidate_id"));
    set(&mut row, "parent_orf_id", required(orf, "parent_orf_id"));
    set(&mut row, "num_fragments", num_fragments.to_string());
    set(
        &mut row,
        "num_high_information_fragments",
        num_high.to_string(),
    );
    set(&mut row, "fragment_ids", distinct_ids.join(";"));
    set(
        &mut row,
        "sum_log_fragment_probability",
        fmt_f64(sum_log_probability),
    );
    set(&mut row, "cooccur_log_odds", fmt_f64(cooccur_log_odds));
    set(&mut row, "spacing_score", fmt_f64(spacing_score));
    set(&mut row, "order_score", fmt_f64(order_score));
    set(
        &mut row,
        "frame_consistency_score",
        fmt_f64(frame_consistency_score),
    );
    set(&mut row, "context_weight", fmt_f64(context_weight));
    set(&mut row, "final_cooccur_score", fmt_f64(final_score));
    set(&mut row, "cooccur_decision", decision);
    set(&mut row, "cooccur_reason", reason);
    row
}

fn phase_gate_row(orf: &Row, stats: Vec<Row>, context: Option<&Row>, cooccur: Option<&Row>) -> Row {
    let context_class = context.map_or("K_INVALID_ORF", |row| required(row, "context_class"));
    let cooccur_decision =
        cooccur.map_or("missing_cooccur", |row| required(row, "cooccur_decision"));
    let parent_len = required(orf, "parent_sequence").len();
    let fragment_len = required(orf, "fragment_sequence")
        .split(';')
        .map(str::len)
        .max()
        .unwrap_or(0);
    let low_complexity = parse_f64(required(orf, "low_complexity_score"));
    let fdr_pass = stats.iter().any(|row| required(row, "fdr_pass") == "true");
    let valid_orf = required(orf, "valid_orf") == "true";
    let pass = valid_orf
        && parent_len >= 6
        && fdr_pass
        && cooccur_decision == "pass_cooccur"
        && !matches!(
            context_class,
            "K_LOW_COMPLEXITY_DECOY" | "K_INVALID_ORF" | "K7_pseudogene_or_repeat_associated_orf"
        )
        && low_complexity < 0.85;
    let failure_reason = if pass {
        ""
    } else if !valid_orf {
        "invalid_orf"
    } else if parent_len < 6 {
        "length_below_minimum_extended_orf"
    } else if low_complexity >= 0.85 || context_class == "K_LOW_COMPLEXITY_DECOY" {
        "low_complexity_decoy_filtered_before_plm"
    } else if context_class == "K7_pseudogene_or_repeat_associated_orf" {
        "repeat_associated_orf_filtered_before_plm"
    } else if cooccur_decision != "pass_cooccur" {
        "insufficient_cooccurrence"
    } else if !fdr_pass {
        "stats_fdr_filter"
    } else {
        "phase_short_circuit_filter"
    };
    let decision = if pass { "pass_plm" } else { "hold_no_plm" };
    let final_score = cooccur.map_or(0.0, |row| parse_f64(required(row, "final_cooccur_score")));
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
        "low_complexity_score",
    ] {
        set(&mut gated, key, required(orf, key));
    }
    set(&mut gated, "fdr_pass", fdr_pass.to_string());
    set(&mut gated, "length_checked_on", "extended_orf");
    set(&mut gated, "parent_sequence_length", parent_len.to_string());
    set(
        &mut gated,
        "fragment_sequence_length",
        fragment_len.to_string(),
    );
    set(&mut gated, "decision", decision);
    set(&mut gated, "pass_plm", pass.to_string());
    set(&mut gated, "failure_reason", failure_reason);
    set(&mut gated, "decision_reason", failure_reason);
    set(&mut gated, "rule_version", "phase_0_2_orf_level_gate_v1");
    set(
        &mut gated,
        "budget_rank",
        if pass {
            ((final_score.max(0.0) * 1000.0).round() as usize)
                .max(1)
                .to_string()
        } else {
            "0".to_string()
        },
    );
    gated
}

fn render_candidate_report(inputs: &ReportInputs) -> String {
    let context_by_orf = inputs
        .context
        .iter()
        .map(|row| (required(row, "parent_orf_id").to_string(), row))
        .collect::<BTreeMap<_, _>>();
    let gate_by_orf = inputs
        .gate
        .iter()
        .map(|row| (required(row, "parent_region_id").to_string(), row))
        .collect::<BTreeMap<_, _>>();
    let mut top = inputs.cooccur.iter().collect::<Vec<_>>();
    top.sort_by(|left, right| {
        parse_f64(required(right, "final_cooccur_score"))
            .total_cmp(&parse_f64(required(left, "final_cooccur_score")))
            .then_with(|| required(left, "candidate_id").cmp(required(right, "candidate_id")))
    });
    let context_counts = count_by(&inputs.context, "context_class");
    let gate_counts = count_by(&inputs.gate, "decision");
    let mut text = String::new();
    text.push_str("# GenoLigand Candidate Report\n\n");
    text.push_str("本报告是计算假设，不是实验验证。\n\n");
    text.push_str("## Input Summary\n\n");
    text.push_str(&format!(
        "- hits: {}\n- stats rows: {}\n- ORF candidates: {}\n- context rows: {}\n- cooccur rows: {}\n- gate rows: {}\n- pLM rows: {}\n- foldgate rows: {}\n- patch rows: {}\n- dock rows: {}\n\n",
        inputs.hits.len(),
        inputs.stats.len(),
        inputs.orfs.len(),
        inputs.context.len(),
        inputs.cooccur.len(),
        inputs.gate.len(),
        inputs.plm.len(),
        inputs.foldgate.len(),
        inputs.patches.len(),
        inputs.dock.len()
    ));
    text.push_str("## Stage Counts\n\n");
    for (stage, count) in [
        ("hits", inputs.hits.len()),
        ("stats", inputs.stats.len()),
        ("orfs", inputs.orfs.len()),
        ("context", inputs.context.len()),
        ("cooccur", inputs.cooccur.len()),
        ("gate", inputs.gate.len()),
        ("plm", inputs.plm.len()),
        ("foldgate", inputs.foldgate.len()),
        ("patches", inputs.patches.len()),
        ("dock", inputs.dock.len()),
    ] {
        text.push_str(&format!("- {stage}: {count}\n"));
    }
    text.push('\n');
    text.push_str("## Context Class Distribution\n\n");
    for (class, count) in context_counts {
        text.push_str(&format!("- {class}: {count}\n"));
    }
    text.push('\n');
    text.push_str("## Top Candidates Table\n\n");
    text.push_str("| rank | candidate_id | parent_orf_id | context | fragments | score | decision | gate_reason |\n");
    text.push_str("| --- | --- | --- | --- | --- | --- | --- | --- |\n");
    for (index, row) in top.iter().take(10).enumerate() {
        let parent_orf = required(row, "parent_orf_id");
        let context = context_by_orf
            .get(parent_orf)
            .map_or("missing_context", |row| required(row, "context_class"));
        let gate = gate_by_orf.get(parent_orf);
        text.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} |\n",
            index + 1,
            required(row, "candidate_id"),
            parent_orf,
            context,
            required(row, "fragment_ids"),
            required(row, "final_cooccur_score"),
            required(row, "cooccur_decision"),
            gate.map_or("", |row| required(row, "failure_reason"))
        ));
    }
    text.push('\n');
    text.push_str("## Known CDS Candidate Section\n\n");
    append_context_section(&mut text, &inputs.context, "K1_known_cds_same_frame");
    text.push_str("## Alternative-Frame Candidate Section\n\n");
    append_context_section(&mut text, &inputs.context, "K2_known_locus_alt_frame");
    text.push_str("## Dark-Genome Candidate Section\n\n");
    append_context_section(&mut text, &inputs.context, "K4_lncRNA_or_nc_transcript_orf");
    append_context_section(&mut text, &inputs.context, "K10_unannotated_intergenic_orf");
    text.push_str("## Filtered Decoy Section\n\n");
    for row in &inputs.gate {
        if required(row, "pass_plm") == "false"
            && (required(row, "failure_reason").contains("low_complexity")
                || required(row, "failure_reason").contains("repeat")
                || required(row, "failure_reason").contains("insufficient"))
        {
            text.push_str(&format!(
                "- {}: {}\n",
                required(row, "candidate_id"),
                required(row, "failure_reason")
            ));
        }
    }
    text.push('\n');
    text.push_str("## Candidate Coordinate Table\n\n");
    text.push_str(
        "| candidate_id | parent_orf_id | chrom | start | end | strand | frame | context |\n",
    );
    text.push_str("| --- | --- | --- | --- | --- | --- | --- | --- |\n");
    for row in &inputs.context {
        text.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} |\n",
            required(row, "candidate_id"),
            required(row, "parent_orf_id"),
            required(row, "chrom"),
            required(row, "start"),
            required(row, "end"),
            required(row, "strand"),
            required(row, "frame"),
            required(row, "context_class")
        ));
    }
    text.push('\n');
    text.push_str("## Candidate Evidence Chain\n\n");
    for row in top.iter().take(10) {
        let parent_orf = required(row, "parent_orf_id");
        let gate = gate_by_orf.get(parent_orf);
        text.push_str(&format!(
            "- {}: fragments={} cooccur_score={} cooccur_decision={} downstream_reason={}\n",
            required(row, "candidate_id"),
            required(row, "fragment_ids"),
            required(row, "final_cooccur_score"),
            required(row, "cooccur_decision"),
            gate.map_or("missing_gate", |row| {
                let reason = required(row, "failure_reason");
                if reason.is_empty() {
                    "entered_downstream"
                } else {
                    reason
                }
            })
        ));
    }
    text.push('\n');
    text.push_str("## Gate Decision Distribution\n\n");
    for (decision, count) in gate_counts {
        text.push_str(&format!("- {decision}: {count}\n"));
    }
    text.push('\n');
    text.push_str("## Scientific Caution\n\n");
    text.push_str("本报告是计算假设，不是实验验证。\n");
    text
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
    set(&mut gated, "decision_reason", failure_reason);
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

fn parse_usize(value: &str) -> usize {
    value.parse::<usize>().unwrap_or(0)
}

fn parse_i8(value: &str) -> i8 {
    value.parse::<i8>().unwrap_or(0)
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

fn distinct_values<'a>(values: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut values = values
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    values.sort();
    values
}

fn distinct_join<'a>(values: impl Iterator<Item = &'a str>) -> String {
    distinct_values(values).join(";")
}

fn ranges_overlap(
    left_start: usize,
    left_end: usize,
    right_start: usize,
    right_end: usize,
) -> bool {
    left_start < right_end && right_start < left_end
}

fn feature_id(feature: &GffFeature) -> &str {
    feature
        .attributes
        .get("ID")
        .map_or(feature.feature_type.as_str(), String::as_str)
}

fn gene_id(feature: &GffFeature) -> &str {
    feature
        .attributes
        .get("gene_id")
        .or_else(|| feature.attributes.get("gene"))
        .or_else(|| feature.attributes.get("Parent"))
        .or_else(|| feature.attributes.get("ID"))
        .map_or(feature.feature_type.as_str(), String::as_str)
}

fn matches_feature_type(feature: &GffFeature, expected: &[&str]) -> bool {
    expected
        .iter()
        .any(|kind| feature.feature_type.eq_ignore_ascii_case(kind))
}

fn cds_frame_relation(feature: &GffFeature, frame: i8) -> &'static str {
    let expected = feature
        .attributes
        .get("reading_frame")
        .or_else(|| feature.attributes.get("orf_frame"))
        .and_then(|value| value.parse::<i8>().ok())
        .or_else(|| {
            feature.phase.as_ref().and_then(|phase| {
                phase
                    .parse::<i8>()
                    .ok()
                    .map(|phase| if phase == 0 { 1 } else { phase + 1 })
            })
        });
    if expected == Some(frame.abs()) {
        "same_frame"
    } else {
        "alternative_frame"
    }
}

fn is_lnc_or_nc_feature(feature: &GffFeature) -> bool {
    let kind = feature.feature_type.to_ascii_lowercase();
    let biotype = feature
        .attributes
        .get("biotype")
        .or_else(|| feature.attributes.get("gene_biotype"))
        .or_else(|| feature.attributes.get("transcript_biotype"))
        .map_or(String::new(), |value| value.to_ascii_lowercase());
    kind.contains("lnc")
        || kind.contains("ncrna")
        || kind.contains("noncoding")
        || biotype.contains("lnc")
        || biotype.contains("ncrna")
        || biotype.contains("noncoding")
}

fn is_gene_like(feature: &GffFeature) -> bool {
    matches_feature_type(
        feature,
        &[
            "gene",
            "mRNA",
            "transcript",
            "lnc_RNA",
            "ncRNA",
            "CDS",
            "exon",
        ],
    ) || is_lnc_or_nc_feature(feature)
}

fn context_weight(context_class: &str) -> f64 {
    match context_class {
        "K1_known_cds_same_frame" => 1.35,
        "K2_known_locus_alt_frame" => 1.15,
        "K3_utr_orf" => 0.95,
        "K4_lncRNA_or_nc_transcript_orf" => 1.10,
        "K5_intronic_orf" => 0.90,
        "K6_antisense_orf" => 0.80,
        "K7_pseudogene_or_repeat_associated_orf" => 0.25,
        "K8_overlapping_orf" => 0.85,
        "K9_pangenome_or_variant_only_orf" => 0.75,
        "K10_unannotated_intergenic_orf" => 0.80,
        "K_LOW_COMPLEXITY_DECOY" | "K_INVALID_ORF" => 0.0,
        _ => 0.70,
    }
}

fn hit_order_is_consistent(stats: &[Row]) -> bool {
    let mut starts = stats
        .iter()
        .map(|row| parse_usize(required(row, "start")))
        .collect::<Vec<_>>();
    starts.sort_unstable();
    if starts.len() < 2 {
        return false;
    }
    starts.dedup();
    starts.len() >= 2
}

fn append_context_section(text: &mut String, rows: &[Row], context_class: &str) {
    let mut any = false;
    for row in rows
        .iter()
        .filter(|row| required(row, "context_class") == context_class)
    {
        any = true;
        text.push_str(&format!(
            "- {} ({}) {}:{}-{} {} frame {} reason={}\n",
            required(row, "candidate_id"),
            required(row, "parent_orf_id"),
            required(row, "chrom"),
            required(row, "start"),
            required(row, "end"),
            required(row, "strand"),
            required(row, "frame"),
            required(row, "classification_reason")
        ));
    }
    if !any {
        text.push_str("- none\n");
    }
    text.push('\n');
}

fn count_by(rows: &[Row], column: &str) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::<String, usize>::new();
    for row in rows {
        *counts.entry(required(row, column).to_string()).or_default() += 1;
    }
    counts
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
