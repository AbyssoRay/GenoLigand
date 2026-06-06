use gls_cli::stages::{
    ClassifyLociOptions, CooccurScoreOptions, DockOptions, FoldgateOptions, GatePlmOptions,
    GateShortCircuitOptions, OrfExtendOptions, PatchOptions, PhaseGateShortCircuitOptions,
    ReportMarkdownOptions, SearchOptions, SplitDevSmokeOptions, StatsOptions, ValidateKind,
    ValidateOptions, run_classify_loci, run_cooccur_score, run_dock, run_foldgate_realize,
    run_gate_plm, run_gate_short_circuit, run_gate_short_circuit_phase, run_orf_extend,
    run_patch_realize, run_report_markdown, run_search_genome, run_split_dev_smoke,
    run_stats_score, run_validate,
};
use gls_cli::{
    DevSmokeInputs, WorkerCommandSet, run_calibrate_patch_tolerance_json,
    run_dev_smoke_with_worker_commands_and_inputs, run_worker_command,
};
use gls_report::{render_html, render_markdown};
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    match run_cli(&args) {
        Ok(Some(output)) => println!("{output}"),
        Ok(None) => {}
        Err(reason) => {
            eprintln!("{reason}");
            std::process::exit(1);
        }
    }
}

fn run_cli(args: &[String]) -> Result<Option<String>, String> {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") && args.len() == 1 {
        return Ok(Some(GLOBAL_HELP.to_string()));
    }

    let command = args
        .first()
        .cloned()
        .unwrap_or_else(|| "dev-smoke".to_string());
    if command.starts_with("worker-") {
        run_worker_command(&command)?;
        return Ok(None);
    }

    match command.as_str() {
        "dev-smoke" => run_dev_smoke_cli(&args[1..], false).map(Some),
        "dev-smoke-html" => run_dev_smoke_cli(&args[1..], true).map(Some),
        "gls-calibrate-patch-tolerance" | "calibrate-patch-tolerance" => {
            if is_help(&args[1..]) {
                Ok(Some(CALIBRATION_HELP.to_string()))
            } else {
                let options = CalibrationOptions::parse(&args[1..])?;
                run_calibrate_patch_tolerance_json(
                    options.fixture_tsv.as_deref(),
                    options.dataset_id.as_deref(),
                )
                .map(Some)
            }
        }
        "search" => run_search_cli(&args[1..]).map(Some),
        "stats" => run_stats_cli(&args[1..]).map(Some),
        "orf" => run_orf_cli(&args[1..]).map(Some),
        "classify" => run_classify_cli(&args[1..]).map(Some),
        "cooccur" => run_cooccur_cli(&args[1..]).map(Some),
        "gate" => run_gate_cli(&args[1..]).map(Some),
        "foldgate" => run_foldgate_cli(&args[1..]).map(Some),
        "patch" => run_patch_cli(&args[1..]).map(Some),
        "dock" => run_dock_cli(&args[1..]).map(Some),
        "report" => run_report_cli(&args[1..]).map(Some),
        "validate" => run_validate_cli(&args[1..]).map(Some),
        "--help" | "-h" => Ok(Some(GLOBAL_HELP.to_string())),
        _ => Err(format!("unknown_command:{command}")),
    }
}

fn run_dev_smoke_cli(args: &[String], html: bool) -> Result<String, String> {
    if is_help(args) {
        return Ok(DEV_SMOKE_HELP.to_string());
    }
    let options = DevSmokeOptions::parse(args)?;
    if let Some(fragments) = options.fragments {
        let out = options
            .out
            .ok_or_else(|| "missing_required_option:--out".to_string())?;
        let genome = options
            .genome_fasta
            .ok_or_else(|| "missing_required_option:--genome".to_string())?;
        let ligand = options
            .ligand_yaml
            .ok_or_else(|| "missing_required_option:--ligand".to_string())?;
        return run_split_dev_smoke(&SplitDevSmokeOptions {
            genome,
            fragments,
            ligand,
            out,
        });
    }

    let worker_bin =
        std::env::current_exe().map_err(|err| format!("current_exe_unavailable:{err}"))?;
    let inputs = DevSmokeInputs::from_optional_files(
        options.ligand_yaml.as_deref(),
        options.genome_fasta.as_deref(),
    )?;
    let report = run_dev_smoke_with_worker_commands_and_inputs(
        WorkerCommandSet::from_env_or_dev(&worker_bin),
        inputs,
    )?;
    let rendered = if html {
        render_html(&report)
    } else {
        render_markdown(&report)
    };
    if let Some(out) = options.out {
        write_text(&out, &rendered)?;
    }
    Ok(rendered)
}

fn run_search_cli(args: &[String]) -> Result<String, String> {
    if args.is_empty() || is_help(args) {
        return Ok(SEARCH_HELP.to_string());
    }
    match args[0].as_str() {
        "genome" => {
            if is_help(&args[1..]) {
                return Ok(SEARCH_GENOME_HELP.to_string());
            }
            let options = SearchGenomeArgs::parse(&args[1..])?;
            let count = run_search_genome(&SearchOptions {
                genome: options.genome,
                fragments: options.fragments,
                out: options.out.clone(),
            })?;
            Ok(stage_ok("search genome", count, &options.out))
        }
        other => Err(format!("unknown_search_command:{other}")),
    }
}

fn run_stats_cli(args: &[String]) -> Result<String, String> {
    if args.is_empty() || is_help(args) {
        return Ok(STATS_HELP.to_string());
    }
    match args[0].as_str() {
        "score" => {
            if is_help(&args[1..]) {
                return Ok(STATS_SCORE_HELP.to_string());
            }
            let options = StatsScoreArgs::parse(&args[1..])?;
            let count = run_stats_score(&StatsOptions {
                hits: options.hits,
                out: options.out.clone(),
            })?;
            Ok(stage_ok("stats score", count, &options.out))
        }
        other => Err(format!("unknown_stats_command:{other}")),
    }
}

fn run_orf_cli(args: &[String]) -> Result<String, String> {
    if args.is_empty() || is_help(args) {
        return Ok(ORF_HELP.to_string());
    }
    match args[0].as_str() {
        "extend" => {
            if is_help(&args[1..]) {
                return Ok(ORF_EXTEND_HELP.to_string());
            }
            let options = OrfExtendArgs::parse(&args[1..])?;
            let count = run_orf_extend(&OrfExtendOptions {
                genome: options.genome,
                hits: options.hits,
                out: options.out.clone(),
            })?;
            Ok(stage_ok("orf extend", count, &options.out))
        }
        other => Err(format!("unknown_orf_command:{other}")),
    }
}

fn run_classify_cli(args: &[String]) -> Result<String, String> {
    if args.is_empty() || is_help(args) {
        return Ok(CLASSIFY_HELP.to_string());
    }
    match args[0].as_str() {
        "loci" => {
            if is_help(&args[1..]) {
                return Ok(CLASSIFY_LOCI_HELP.to_string());
            }
            let options = ClassifyLociArgs::parse(&args[1..])?;
            let count = run_classify_loci(&ClassifyLociOptions {
                annotation: options.annotation,
                orfs: options.orfs,
                out: options.out.clone(),
            })?;
            Ok(stage_ok("classify loci", count, &options.out))
        }
        other => Err(format!("unknown_classify_command:{other}")),
    }
}

fn run_cooccur_cli(args: &[String]) -> Result<String, String> {
    if args.is_empty() || is_help(args) {
        return Ok(COOCCUR_HELP.to_string());
    }
    match args[0].as_str() {
        "score" => {
            if is_help(&args[1..]) {
                return Ok(COOCCUR_SCORE_HELP.to_string());
            }
            let options = CooccurScoreArgs::parse(&args[1..])?;
            let count = run_cooccur_score(&CooccurScoreOptions {
                hits: options.hits,
                stats: options.stats,
                orfs: options.orfs,
                context: options.context,
                out: options.out.clone(),
            })?;
            Ok(stage_ok("cooccur score", count, &options.out))
        }
        other => Err(format!("unknown_cooccur_command:{other}")),
    }
}

fn run_gate_cli(args: &[String]) -> Result<String, String> {
    if args.is_empty() || is_help(args) {
        return Ok(GATE_HELP.to_string());
    }
    match args[0].as_str() {
        "short-circuit" => {
            if is_help(&args[1..]) {
                return Ok(GATE_SHORT_CIRCUIT_HELP.to_string());
            }
            let options = GateShortCircuitArgs::parse(&args[1..])?;
            let count = if let (Some(orfs), Some(context), Some(cooccur)) = (
                options.orfs.clone(),
                options.context.clone(),
                options.cooccur.clone(),
            ) {
                run_gate_short_circuit_phase(&PhaseGateShortCircuitOptions {
                    stats: options.stats,
                    orfs,
                    context,
                    cooccur,
                    out: options.out.clone(),
                })?
            } else {
                run_gate_short_circuit(&GateShortCircuitOptions {
                    stats: options.stats,
                    out: options.out.clone(),
                })?
            };
            Ok(stage_ok("gate short-circuit", count, &options.out))
        }
        "plm" => {
            if is_help(&args[1..]) {
                return Ok(GATE_PLM_HELP.to_string());
            }
            let options = GatePlmArgs::parse(&args[1..])?;
            let count = run_gate_plm(&GatePlmOptions {
                candidates: options.candidates,
                profile: options.profile,
                out: options.out.clone(),
            })?;
            Ok(stage_ok("gate plm", count, &options.out))
        }
        other => Err(format!("unknown_gate_command:{other}")),
    }
}

fn run_foldgate_cli(args: &[String]) -> Result<String, String> {
    if args.is_empty() || is_help(args) {
        return Ok(FOLDGATE_HELP.to_string());
    }
    match args[0].as_str() {
        "realize" => {
            if is_help(&args[1..]) {
                return Ok(FOLDGATE_REALIZE_HELP.to_string());
            }
            let options = FoldgateRealizeArgs::parse(&args[1..])?;
            let count = run_foldgate_realize(&FoldgateOptions {
                candidates: options.candidates,
                out: options.out.clone(),
            })?;
            Ok(stage_ok("foldgate realize", count, &options.out))
        }
        other => Err(format!("unknown_foldgate_command:{other}")),
    }
}

fn run_patch_cli(args: &[String]) -> Result<String, String> {
    if args.is_empty() || is_help(args) {
        return Ok(PATCH_HELP.to_string());
    }
    match args[0].as_str() {
        "realize" => {
            if is_help(&args[1..]) {
                return Ok(PATCH_REALIZE_HELP.to_string());
            }
            let options = PatchRealizeArgs::parse(&args[1..])?;
            let count = run_patch_realize(&PatchOptions {
                foldgate: options.foldgate,
                ligand: options.ligand,
                tolerance_profile: options.tolerance_profile,
                out: options.out.clone(),
            })?;
            Ok(stage_ok("patch realize", count, &options.out))
        }
        other => Err(format!("unknown_patch_command:{other}")),
    }
}

fn run_dock_cli(args: &[String]) -> Result<String, String> {
    if args.is_empty() || is_help(args) {
        return Ok(DOCK_HELP.to_string());
    }
    match args[0].as_str() {
        "run" => {
            if is_help(&args[1..]) {
                return Ok(DOCK_RUN_HELP.to_string());
            }
            let options = DockRunArgs::parse(&args[1..])?;
            let count = run_dock(&DockOptions {
                patches: options.patches,
                mode: options.mode,
                out: options.out.clone(),
            })?;
            Ok(stage_ok("dock run", count, &options.out))
        }
        other => Err(format!("unknown_dock_command:{other}")),
    }
}

fn run_report_cli(args: &[String]) -> Result<String, String> {
    if args.is_empty() || is_help(args) {
        return Ok(REPORT_HELP.to_string());
    }
    match args[0].as_str() {
        "markdown" => {
            if is_help(&args[1..]) {
                return Ok(REPORT_MARKDOWN_HELP.to_string());
            }
            let options = ReportMarkdownArgs::parse(&args[1..])?;
            let count = run_report_markdown(&ReportMarkdownOptions {
                hits: options.hits,
                stats: options.stats,
                orfs: options.orfs,
                context: options.context,
                cooccur: options.cooccur,
                gate: options.gate,
                plm: options.plm,
                foldgate: options.foldgate,
                patches: options.patches,
                dock: options.dock,
                out: options.out.clone(),
            })?;
            Ok(stage_ok("report markdown", count, &options.out))
        }
        other => Err(format!("unknown_report_command:{other}")),
    }
}

fn run_validate_cli(args: &[String]) -> Result<String, String> {
    if args.is_empty() || is_help(args) {
        return Ok(VALIDATE_HELP.to_string());
    }
    let kind = match args[0].as_str() {
        "fasta" => ValidateKind::Fasta,
        "gff" => ValidateKind::Gff,
        "fragment-kernel" => ValidateKind::FragmentKernel,
        "ligand" => ValidateKind::Ligand,
        other => return Err(format!("unknown_validate_command:{other}")),
    };
    if is_help(&args[1..]) {
        return Ok(validate_leaf_help(kind).to_string());
    }
    let options = ValidateArgs::parse(&args[1..])?;
    run_validate(&ValidateOptions {
        kind,
        input: options.input,
    })
}

#[derive(Default)]
struct CalibrationOptions {
    fixture_tsv: Option<PathBuf>,
    dataset_id: Option<String>,
}

impl CalibrationOptions {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut options = Self::default();
        let mut index = 0;
        while index < args.len() {
            match args[index].as_str() {
                "--fixture-tsv" => {
                    options.fixture_tsv = Some(take_path(args, &mut index, "--fixture-tsv")?);
                }
                "--dataset-id" => {
                    options.dataset_id = Some(take_string(args, &mut index, "--dataset-id")?);
                }
                other => return Err(format!("unknown_calibration_option:{other}")),
            }
            index += 1;
        }
        Ok(options)
    }
}

#[derive(Default)]
struct DevSmokeOptions {
    ligand_yaml: Option<PathBuf>,
    genome_fasta: Option<PathBuf>,
    fragments: Option<PathBuf>,
    out: Option<PathBuf>,
}

impl DevSmokeOptions {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut options = Self::default();
        let mut index = 0;
        while index < args.len() {
            match args[index].as_str() {
                "--ligand-yaml" | "--ligand" => {
                    let flag = args[index].clone();
                    options.ligand_yaml = Some(take_path(args, &mut index, &flag)?);
                }
                "--genome-fasta" | "--genome" => {
                    let flag = args[index].clone();
                    options.genome_fasta = Some(take_path(args, &mut index, &flag)?);
                }
                "--fragments" => {
                    options.fragments = Some(take_path(args, &mut index, "--fragments")?);
                }
                "--out" => {
                    options.out = Some(take_path(args, &mut index, "--out")?);
                }
                other => return Err(format!("unknown_dev_smoke_option:{other}")),
            }
            index += 1;
        }
        Ok(options)
    }
}

struct SearchGenomeArgs {
    genome: PathBuf,
    fragments: PathBuf,
    out: PathBuf,
}

impl SearchGenomeArgs {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut genome = None;
        let mut fragments = None;
        let mut out = None;
        parse_options(args, |flag, args, index| match flag {
            "--genome" | "--genome-fasta" => {
                genome = Some(take_path(args, index, flag)?);
                Ok(())
            }
            "--fragments" => {
                fragments = Some(take_path(args, index, flag)?);
                Ok(())
            }
            "--out" => {
                out = Some(take_path(args, index, flag)?);
                Ok(())
            }
            other => Err(format!("unknown_search_genome_option:{other}")),
        })?;
        Ok(Self {
            genome: genome.ok_or_else(|| "missing_required_option:--genome".to_string())?,
            fragments: fragments
                .ok_or_else(|| "missing_required_option:--fragments".to_string())?,
            out: out.ok_or_else(|| "missing_required_option:--out".to_string())?,
        })
    }
}

struct StatsScoreArgs {
    hits: PathBuf,
    out: PathBuf,
}

impl StatsScoreArgs {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut hits = None;
        let mut out = None;
        parse_options(args, |flag, args, index| match flag {
            "--hits" => {
                hits = Some(take_path(args, index, flag)?);
                Ok(())
            }
            "--out" => {
                out = Some(take_path(args, index, flag)?);
                Ok(())
            }
            other => Err(format!("unknown_stats_score_option:{other}")),
        })?;
        Ok(Self {
            hits: hits.ok_or_else(|| "missing_required_option:--hits".to_string())?,
            out: out.ok_or_else(|| "missing_required_option:--out".to_string())?,
        })
    }
}

struct OrfExtendArgs {
    genome: PathBuf,
    hits: PathBuf,
    out: PathBuf,
}

impl OrfExtendArgs {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut genome = None;
        let mut hits = None;
        let mut out = None;
        parse_options(args, |flag, args, index| match flag {
            "--genome" | "--genome-fasta" => {
                genome = Some(take_path(args, index, flag)?);
                Ok(())
            }
            "--hits" => {
                hits = Some(take_path(args, index, flag)?);
                Ok(())
            }
            "--out" => {
                out = Some(take_path(args, index, flag)?);
                Ok(())
            }
            other => Err(format!("unknown_orf_extend_option:{other}")),
        })?;
        Ok(Self {
            genome: genome.ok_or_else(|| "missing_required_option:--genome".to_string())?,
            hits: hits.ok_or_else(|| "missing_required_option:--hits".to_string())?,
            out: out.ok_or_else(|| "missing_required_option:--out".to_string())?,
        })
    }
}

struct ClassifyLociArgs {
    annotation: PathBuf,
    orfs: PathBuf,
    out: PathBuf,
}

impl ClassifyLociArgs {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut annotation = None;
        let mut orfs = None;
        let mut out = None;
        parse_options(args, |flag, args, index| match flag {
            "--annotation" | "--gff" => {
                annotation = Some(take_path(args, index, flag)?);
                Ok(())
            }
            "--orfs" => {
                orfs = Some(take_path(args, index, flag)?);
                Ok(())
            }
            "--out" => {
                out = Some(take_path(args, index, flag)?);
                Ok(())
            }
            other => Err(format!("unknown_classify_loci_option:{other}")),
        })?;
        Ok(Self {
            annotation: annotation
                .ok_or_else(|| "missing_required_option:--annotation".to_string())?,
            orfs: orfs.ok_or_else(|| "missing_required_option:--orfs".to_string())?,
            out: out.ok_or_else(|| "missing_required_option:--out".to_string())?,
        })
    }
}

struct CooccurScoreArgs {
    hits: PathBuf,
    stats: PathBuf,
    orfs: PathBuf,
    context: PathBuf,
    out: PathBuf,
}

impl CooccurScoreArgs {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut hits = None;
        let mut stats = None;
        let mut orfs = None;
        let mut context = None;
        let mut out = None;
        parse_options(args, |flag, args, index| match flag {
            "--hits" => {
                hits = Some(take_path(args, index, flag)?);
                Ok(())
            }
            "--stats" => {
                stats = Some(take_path(args, index, flag)?);
                Ok(())
            }
            "--orfs" => {
                orfs = Some(take_path(args, index, flag)?);
                Ok(())
            }
            "--context" => {
                context = Some(take_path(args, index, flag)?);
                Ok(())
            }
            "--out" => {
                out = Some(take_path(args, index, flag)?);
                Ok(())
            }
            other => Err(format!("unknown_cooccur_score_option:{other}")),
        })?;
        Ok(Self {
            hits: hits.ok_or_else(|| "missing_required_option:--hits".to_string())?,
            stats: stats.ok_or_else(|| "missing_required_option:--stats".to_string())?,
            orfs: orfs.ok_or_else(|| "missing_required_option:--orfs".to_string())?,
            context: context.ok_or_else(|| "missing_required_option:--context".to_string())?,
            out: out.ok_or_else(|| "missing_required_option:--out".to_string())?,
        })
    }
}

struct GateShortCircuitArgs {
    stats: PathBuf,
    orfs: Option<PathBuf>,
    context: Option<PathBuf>,
    cooccur: Option<PathBuf>,
    out: PathBuf,
}

impl GateShortCircuitArgs {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut stats = None;
        let mut orfs = None;
        let mut context = None;
        let mut cooccur = None;
        let mut out = None;
        parse_options(args, |flag, args, index| match flag {
            "--stats" => {
                stats = Some(take_path(args, index, flag)?);
                Ok(())
            }
            "--orfs" => {
                orfs = Some(take_path(args, index, flag)?);
                Ok(())
            }
            "--context" => {
                context = Some(take_path(args, index, flag)?);
                Ok(())
            }
            "--cooccur" => {
                cooccur = Some(take_path(args, index, flag)?);
                Ok(())
            }
            "--out" => {
                out = Some(take_path(args, index, flag)?);
                Ok(())
            }
            other => Err(format!("unknown_gate_short_circuit_option:{other}")),
        })?;
        Ok(Self {
            stats: stats.ok_or_else(|| "missing_required_option:--stats".to_string())?,
            orfs,
            context,
            cooccur,
            out: out.ok_or_else(|| "missing_required_option:--out".to_string())?,
        })
    }
}

struct GatePlmArgs {
    candidates: PathBuf,
    profile: String,
    out: PathBuf,
}

impl GatePlmArgs {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut candidates = None;
        let mut profile = None;
        let mut out = None;
        parse_options(args, |flag, args, index| match flag {
            "--candidates" => {
                candidates = Some(take_path(args, index, flag)?);
                Ok(())
            }
            "--profile" => {
                profile = Some(take_string(args, index, flag)?);
                Ok(())
            }
            "--out" => {
                out = Some(take_path(args, index, flag)?);
                Ok(())
            }
            other => Err(format!("unknown_gate_plm_option:{other}")),
        })?;
        Ok(Self {
            candidates: candidates
                .ok_or_else(|| "missing_required_option:--candidates".to_string())?,
            profile: profile.unwrap_or_else(|| "dev_gpu_lowmem".to_string()),
            out: out.ok_or_else(|| "missing_required_option:--out".to_string())?,
        })
    }
}

struct FoldgateRealizeArgs {
    candidates: PathBuf,
    out: PathBuf,
}

impl FoldgateRealizeArgs {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut candidates = None;
        let mut out = None;
        parse_options(args, |flag, args, index| match flag {
            "--candidates" => {
                candidates = Some(take_path(args, index, flag)?);
                Ok(())
            }
            "--out" => {
                out = Some(take_path(args, index, flag)?);
                Ok(())
            }
            other => Err(format!("unknown_foldgate_realize_option:{other}")),
        })?;
        Ok(Self {
            candidates: candidates
                .ok_or_else(|| "missing_required_option:--candidates".to_string())?,
            out: out.ok_or_else(|| "missing_required_option:--out".to_string())?,
        })
    }
}

struct PatchRealizeArgs {
    foldgate: PathBuf,
    ligand: PathBuf,
    tolerance_profile: PathBuf,
    out: PathBuf,
}

impl PatchRealizeArgs {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut foldgate = None;
        let mut ligand = None;
        let mut tolerance_profile = None;
        let mut out = None;
        parse_options(args, |flag, args, index| match flag {
            "--foldgate" => {
                foldgate = Some(take_path(args, index, flag)?);
                Ok(())
            }
            "--ligand" => {
                ligand = Some(take_path(args, index, flag)?);
                Ok(())
            }
            "--tolerance-profile" => {
                tolerance_profile = Some(take_path(args, index, flag)?);
                Ok(())
            }
            "--out" => {
                out = Some(take_path(args, index, flag)?);
                Ok(())
            }
            other => Err(format!("unknown_patch_realize_option:{other}")),
        })?;
        Ok(Self {
            foldgate: foldgate.ok_or_else(|| "missing_required_option:--foldgate".to_string())?,
            ligand: ligand.ok_or_else(|| "missing_required_option:--ligand".to_string())?,
            tolerance_profile: tolerance_profile
                .unwrap_or_else(|| PathBuf::from("dev_fixture_tolerance_profile.tsv")),
            out: out.ok_or_else(|| "missing_required_option:--out".to_string())?,
        })
    }
}

struct DockRunArgs {
    patches: PathBuf,
    mode: String,
    out: PathBuf,
}

impl DockRunArgs {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut patches = None;
        let mut mode = None;
        let mut out = None;
        parse_options(args, |flag, args, index| match flag {
            "--patches" => {
                patches = Some(take_path(args, index, flag)?);
                Ok(())
            }
            "--mode" => {
                mode = Some(take_string(args, index, flag)?);
                Ok(())
            }
            "--out" => {
                out = Some(take_path(args, index, flag)?);
                Ok(())
            }
            other => Err(format!("unknown_dock_run_option:{other}")),
        })?;
        Ok(Self {
            patches: patches.ok_or_else(|| "missing_required_option:--patches".to_string())?,
            mode: mode.unwrap_or_else(|| "stub".to_string()),
            out: out.ok_or_else(|| "missing_required_option:--out".to_string())?,
        })
    }
}

struct ReportMarkdownArgs {
    hits: PathBuf,
    stats: PathBuf,
    orfs: PathBuf,
    context: PathBuf,
    cooccur: PathBuf,
    gate: PathBuf,
    plm: PathBuf,
    foldgate: PathBuf,
    patches: PathBuf,
    dock: PathBuf,
    out: PathBuf,
}

impl ReportMarkdownArgs {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut hits = None;
        let mut stats = None;
        let mut orfs = None;
        let mut context = None;
        let mut cooccur = None;
        let mut gate = None;
        let mut plm = None;
        let mut foldgate = None;
        let mut patches = None;
        let mut dock = None;
        let mut out = None;
        parse_options(args, |flag, args, index| match flag {
            "--hits" => {
                hits = Some(take_path(args, index, flag)?);
                Ok(())
            }
            "--stats" => {
                stats = Some(take_path(args, index, flag)?);
                Ok(())
            }
            "--orfs" => {
                orfs = Some(take_path(args, index, flag)?);
                Ok(())
            }
            "--context" => {
                context = Some(take_path(args, index, flag)?);
                Ok(())
            }
            "--cooccur" => {
                cooccur = Some(take_path(args, index, flag)?);
                Ok(())
            }
            "--gate" => {
                gate = Some(take_path(args, index, flag)?);
                Ok(())
            }
            "--plm" => {
                plm = Some(take_path(args, index, flag)?);
                Ok(())
            }
            "--foldgate" => {
                foldgate = Some(take_path(args, index, flag)?);
                Ok(())
            }
            "--patches" => {
                patches = Some(take_path(args, index, flag)?);
                Ok(())
            }
            "--dock" => {
                dock = Some(take_path(args, index, flag)?);
                Ok(())
            }
            "--out" => {
                out = Some(take_path(args, index, flag)?);
                Ok(())
            }
            other => Err(format!("unknown_report_markdown_option:{other}")),
        })?;
        Ok(Self {
            hits: hits.ok_or_else(|| "missing_required_option:--hits".to_string())?,
            stats: stats.ok_or_else(|| "missing_required_option:--stats".to_string())?,
            orfs: orfs.ok_or_else(|| "missing_required_option:--orfs".to_string())?,
            context: context.ok_or_else(|| "missing_required_option:--context".to_string())?,
            cooccur: cooccur.ok_or_else(|| "missing_required_option:--cooccur".to_string())?,
            gate: gate.ok_or_else(|| "missing_required_option:--gate".to_string())?,
            plm: plm.ok_or_else(|| "missing_required_option:--plm".to_string())?,
            foldgate: foldgate.ok_or_else(|| "missing_required_option:--foldgate".to_string())?,
            patches: patches.ok_or_else(|| "missing_required_option:--patches".to_string())?,
            dock: dock.ok_or_else(|| "missing_required_option:--dock".to_string())?,
            out: out.ok_or_else(|| "missing_required_option:--out".to_string())?,
        })
    }
}

struct ValidateArgs {
    input: PathBuf,
}

impl ValidateArgs {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut input = None;
        parse_options(args, |flag, args, index| match flag {
            "--input" => {
                input = Some(take_path(args, index, flag)?);
                Ok(())
            }
            other => Err(format!("unknown_validate_option:{other}")),
        })?;
        Ok(Self {
            input: input.ok_or_else(|| "missing_required_option:--input".to_string())?,
        })
    }
}

fn parse_options<F>(args: &[String], mut handle: F) -> Result<(), String>
where
    F: FnMut(&str, &[String], &mut usize) -> Result<(), String>,
{
    let mut index = 0;
    while index < args.len() {
        let flag = args[index].as_str();
        handle(flag, args, &mut index)?;
        index += 1;
    }
    Ok(())
}

fn take_path(args: &[String], index: &mut usize, flag: &str) -> Result<PathBuf, String> {
    Ok(PathBuf::from(take_string(args, index, flag)?))
}

fn take_string(args: &[String], index: &mut usize, flag: &str) -> Result<String, String> {
    *index += 1;
    args.get(*index)
        .cloned()
        .ok_or_else(|| format!("missing_value_for:{flag}"))
}

fn is_help(args: &[String]) -> bool {
    args.iter().any(|arg| arg == "--help" || arg == "-h")
}

fn stage_ok(stage: &str, count: usize, out: &Path) -> String {
    format!(
        "stage_ok\tstage={stage}\trows={count}\tout={}",
        out.display()
    )
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

fn validate_leaf_help(kind: ValidateKind) -> &'static str {
    match kind {
        ValidateKind::Fasta => VALIDATE_FASTA_HELP,
        ValidateKind::Gff => VALIDATE_GFF_HELP,
        ValidateKind::FragmentKernel => VALIDATE_FRAGMENT_KERNEL_HELP,
        ValidateKind::Ligand => VALIDATE_LIGAND_HELP,
    }
}

const GLOBAL_HELP: &str = "\
usage: gls-cli <command> [options]

commands:
  dev-smoke
  search genome
  stats score
  orf extend
  classify loci
  cooccur score
  gate short-circuit
  gate plm
  foldgate realize
  patch realize
  dock run
  report markdown
  validate fasta|gff|fragment-kernel|ligand
  gls-calibrate-patch-tolerance
";

const DEV_SMOKE_HELP: &str = "\
usage: gls-cli dev-smoke [--ligand-yaml PATH] [--genome-fasta PATH] [--out PATH]
       gls-cli dev-smoke --genome PATH --fragments PATH --ligand PATH --out PATH
";

const CALIBRATION_HELP: &str =
    "usage: gls-cli gls-calibrate-patch-tolerance [--fixture-tsv PATH] [--dataset-id ID]";
const SEARCH_HELP: &str = "usage: gls-cli search <genome>\ncommands:\n  genome";
const SEARCH_GENOME_HELP: &str =
    "usage: gls-cli search genome --genome PATH --fragments PATH --out PATH";
const STATS_HELP: &str = "usage: gls-cli stats <score>\ncommands:\n  score";
const STATS_SCORE_HELP: &str = "usage: gls-cli stats score --hits PATH --out PATH";
const ORF_HELP: &str = "usage: gls-cli orf <extend>\ncommands:\n  extend";
const ORF_EXTEND_HELP: &str = "usage: gls-cli orf extend --genome PATH --hits PATH --out PATH";
const CLASSIFY_HELP: &str = "usage: gls-cli classify <loci>\ncommands:\n  loci";
const CLASSIFY_LOCI_HELP: &str =
    "usage: gls-cli classify loci --annotation PATH --orfs PATH --out PATH";
const COOCCUR_HELP: &str = "usage: gls-cli cooccur <score>\ncommands:\n  score";
const COOCCUR_SCORE_HELP: &str =
    "usage: gls-cli cooccur score --hits PATH --stats PATH --orfs PATH --context PATH --out PATH";
const GATE_HELP: &str =
    "usage: gls-cli gate <short-circuit|plm>\ncommands:\n  short-circuit\n  plm";
const GATE_SHORT_CIRCUIT_HELP: &str = "usage: gls-cli gate short-circuit --stats PATH [--orfs PATH --context PATH --cooccur PATH] --out PATH";
const GATE_PLM_HELP: &str = "usage: gls-cli gate plm --candidates PATH [--profile NAME] --out PATH";
const FOLDGATE_HELP: &str = "usage: gls-cli foldgate <realize>\ncommands:\n  realize";
const FOLDGATE_REALIZE_HELP: &str = "usage: gls-cli foldgate realize --candidates PATH --out PATH";
const PATCH_HELP: &str = "usage: gls-cli patch <realize>\ncommands:\n  realize";
const PATCH_REALIZE_HELP: &str = "usage: gls-cli patch realize --foldgate PATH --ligand PATH [--tolerance-profile PATH] --out PATH";
const DOCK_HELP: &str = "usage: gls-cli dock <run>\ncommands:\n  run";
const DOCK_RUN_HELP: &str = "usage: gls-cli dock run --patches PATH [--mode MODE] --out PATH";
const REPORT_HELP: &str = "usage: gls-cli report <markdown>\ncommands:\n  markdown";
const REPORT_MARKDOWN_HELP: &str = "usage: gls-cli report markdown --hits PATH --stats PATH --orfs PATH --context PATH --cooccur PATH --gate PATH --plm PATH --foldgate PATH --patches PATH --dock PATH --out PATH";
const VALIDATE_HELP: &str =
    "usage: gls-cli validate <fasta|gff|fragment-kernel|ligand> --input PATH";
const VALIDATE_FASTA_HELP: &str = "usage: gls-cli validate fasta --input PATH";
const VALIDATE_GFF_HELP: &str = "usage: gls-cli validate gff --input PATH";
const VALIDATE_FRAGMENT_KERNEL_HELP: &str = "usage: gls-cli validate fragment-kernel --input PATH";
const VALIDATE_LIGAND_HELP: &str = "usage: gls-cli validate ligand --input PATH";
