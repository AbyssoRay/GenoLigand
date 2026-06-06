use gls_cli::stages::{
    ClassifyLociOptions, CooccurScoreOptions, DockOptions, FoldgateOptions, GatePlmOptions,
    GateShortCircuitOptions, OrfExtendOptions, PatchOptions, PhaseGateShortCircuitOptions,
    ReportMarkdownOptions, SearchOptions, StatsOptions, ValidateKind, ValidateOptions,
    run_classify_loci, run_cooccur_score, run_dock, run_foldgate_realize, run_gate_plm,
    run_gate_short_circuit, run_gate_short_circuit_phase, run_orf_extend, run_patch_realize,
    run_report_markdown, run_search_genome, run_stats_score, run_validate,
};
use gls_genome::{parse_fasta, reverse_complement, six_frame_translations, translate_frame};
use gls_stats::benjamini_hochberg;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

#[test]
fn six_frame_translations_are_total_for_short_and_ambiguous_dna() {
    for dna in [
        "",
        "A",
        "AT",
        "ATG",
        "NNNATGTAA",
        "ATGYHEDLV",
        "CCCCCCCCCCCC",
    ] {
        let frames = six_frame_translations(dna);
        assert_eq!(frames.len(), 6);
        assert_eq!(
            reverse_complement(&reverse_complement(dna)),
            dna.chars()
                .map(|ch| match ch.to_ascii_uppercase() {
                    'A' | 'C' | 'G' | 'T' => ch.to_ascii_uppercase(),
                    _ => 'N',
                })
                .collect::<String>()
        );
        assert!(frames.iter().all(|(frame, _)| *frame != 0));
    }
}

#[test]
fn benjamini_hochberg_q_values_are_monotonic_by_sorted_p_value() {
    for p_values in [
        vec![0.01, 0.02, 0.50, 0.03],
        vec![0.50, 0.001, 0.20, 0.001, 0.90],
        vec![1.0, 1.0, 1.0],
    ] {
        let q_values = benjamini_hochberg(&p_values);
        assert_eq!(q_values.len(), p_values.len());
        assert!(q_values.iter().all(|value| (0.0..=1.0).contains(value)));
        let mut ranked = p_values
            .iter()
            .copied()
            .zip(q_values.iter().copied())
            .collect::<Vec<_>>();
        ranked.sort_by(|left, right| left.0.total_cmp(&right.0));
        assert!(ranked.windows(2).all(|window| window[0].1 <= window[1].1));
    }
}

#[test]
fn split_stage_toy_pipeline_passes_yhedlv_and_filters_ala_rich_decoy() {
    let root = temp_root("split-stage-toy");
    let genome = root.join("toy_genome.fa");
    let fragments = root.join("fragment_kernel.fixture.json");
    let ligand = root.join("ligand.fixture.yaml");
    let hits = root.join("hits.tsv");
    let stats = root.join("stats.tsv");
    let gate = root.join("gate.tsv");
    let plm = root.join("plm.tsv");
    let foldgate = root.join("foldgate.tsv");
    let patches = root.join("patches.tsv");
    let dock = root.join("dock.tsv");

    write_fixture_files(&genome, &fragments, &ligand);

    assert_eq!(
        run_search_genome(&SearchOptions {
            genome: genome.clone(),
            fragments: fragments.clone(),
            out: hits.clone(),
        })
        .expect("search"),
        2
    );
    assert_eq!(
        run_stats_score(&StatsOptions {
            hits: hits.clone(),
            out: stats.clone(),
        })
        .expect("stats"),
        2
    );
    assert_eq!(
        run_gate_short_circuit(&GateShortCircuitOptions {
            stats: stats.clone(),
            out: gate.clone(),
        })
        .expect("gate"),
        2
    );
    assert_eq!(
        run_gate_plm(&GatePlmOptions {
            candidates: gate.clone(),
            profile: "dev_cpu_smoke".to_string(),
            out: plm.clone(),
        })
        .expect("plm"),
        2
    );
    assert_eq!(
        run_foldgate_realize(&FoldgateOptions {
            candidates: plm.clone(),
            out: foldgate.clone(),
        })
        .expect("foldgate"),
        2
    );
    assert_eq!(
        run_patch_realize(&PatchOptions {
            foldgate: foldgate.clone(),
            ligand: ligand.clone(),
            tolerance_profile: root.join("dev_fixture_tolerance.tsv"),
            out: patches.clone(),
        })
        .expect("patch"),
        2
    );
    assert_eq!(
        run_dock(&DockOptions {
            patches,
            mode: "stub".to_string(),
            out: dock.clone(),
        })
        .expect("dock"),
        2
    );

    let plm_text = fs::read_to_string(&plm).expect("read plm");
    assert!(
        plm_text
            .lines()
            .any(|line| line.contains("YHEDLV") && line.contains("\ttrue\ttrue\ttrue\t"))
    );
    let gate_text = fs::read_to_string(&gate).expect("read gate");
    assert!(
        gate_text
            .lines()
            .any(|line| line.contains("AAAAAA") && line.contains("low_complexity_decoy_filtered"))
    );
    let dock_text = fs::read_to_string(&dock).expect("read dock");
    assert!(
        dock_text
            .lines()
            .any(|line| line.contains("YHEDLV") && line.contains("\ttrue\t-7.400000\t"))
    );
}

#[test]
fn validators_accept_minimal_valid_contract_files() {
    let root = temp_root("validators");
    let fasta = root.join("valid.fa");
    let gff = root.join("valid.gff3");
    let fragments = root.join("fragment_kernel.fixture.json");
    let ligand = root.join("ligand.fixture.yaml");
    let genome = root.join("toy_genome.fa");
    write_fixture_files(&genome, &fragments, &ligand);
    fs::write(&fasta, ">valid\nATGTAA\n").expect("write fasta");
    fs::write(
        &gff,
        "##gff-version 3\nchr1\tgenoligand\tCDS\t1\t6\t.\t+\t0\tID=cds1\n",
    )
    .expect("write gff");

    for (kind, input) in [
        (ValidateKind::Fasta, fasta),
        (ValidateKind::Gff, gff),
        (ValidateKind::FragmentKernel, fragments),
        (ValidateKind::Ligand, ligand),
    ] {
        let output = run_validate(&ValidateOptions { kind, input }).expect("validate");
        assert!(output.starts_with("validation_ok"));
    }
}

#[test]
fn phase_0_2_pipeline_preserves_context_ranking_and_stop_boundaries() {
    let root = temp_root("phase-0-2");
    let workspace = workspace_root();
    let fixtures = workspace.join("fixtures").join("phase_0_2");
    let genome = fixtures.join("mini_genome.fa");
    let fragments = fixtures.join("fragment_kernel.json");
    let annotation = fixtures.join("mini_annotation.gff3");
    let ligand = fixtures.join("ligand.fixture.yaml");
    let hits = root.join("hits.tsv");
    let stats = root.join("stats.tsv");
    let orfs = root.join("orfs.tsv");
    let context = root.join("context.tsv");
    let cooccur = root.join("cooccur.tsv");
    let gate = root.join("gate.tsv");
    let plm = root.join("plm.tsv");
    let foldgate = root.join("foldgate.tsv");
    let patches = root.join("patches.tsv");
    let dock = root.join("dock.tsv");
    let report = root.join("GENOLIGAND_CANDIDATE_REPORT.md");
    let report_again = root.join("GENOLIGAND_CANDIDATE_REPORT.again.md");

    run_search_genome(&SearchOptions {
        genome: genome.clone(),
        fragments: fragments.clone(),
        out: hits.clone(),
    })
    .expect("phase search");
    run_stats_score(&StatsOptions {
        hits: hits.clone(),
        out: stats.clone(),
    })
    .expect("phase stats");
    run_orf_extend(&OrfExtendOptions {
        genome: genome.clone(),
        hits: hits.clone(),
        out: orfs.clone(),
    })
    .expect("phase orf extend");
    run_classify_loci(&ClassifyLociOptions {
        annotation,
        orfs: orfs.clone(),
        out: context.clone(),
    })
    .expect("phase classify");
    run_cooccur_score(&CooccurScoreOptions {
        hits: hits.clone(),
        stats: stats.clone(),
        orfs: orfs.clone(),
        context: context.clone(),
        out: cooccur.clone(),
    })
    .expect("phase cooccur");
    run_gate_short_circuit_phase(&PhaseGateShortCircuitOptions {
        stats: stats.clone(),
        orfs: orfs.clone(),
        context: context.clone(),
        cooccur: cooccur.clone(),
        out: gate.clone(),
    })
    .expect("phase gate");
    run_gate_plm(&GatePlmOptions {
        candidates: gate.clone(),
        profile: "dev_cpu_smoke".to_string(),
        out: plm.clone(),
    })
    .expect("phase plm");
    run_foldgate_realize(&FoldgateOptions {
        candidates: plm.clone(),
        out: foldgate.clone(),
    })
    .expect("phase foldgate");
    run_patch_realize(&PatchOptions {
        foldgate: foldgate.clone(),
        ligand: ligand.clone(),
        tolerance_profile: root.join("dev_fixture_tolerance.tsv"),
        out: patches.clone(),
    })
    .expect("phase patch");
    run_dock(&DockOptions {
        patches: patches.clone(),
        mode: "stub".to_string(),
        out: dock.clone(),
    })
    .expect("phase dock");
    for out in [&report, &report_again] {
        run_report_markdown(&ReportMarkdownOptions {
            hits: hits.clone(),
            stats: stats.clone(),
            orfs: orfs.clone(),
            context: context.clone(),
            cooccur: cooccur.clone(),
            gate: gate.clone(),
            plm: plm.clone(),
            foldgate: foldgate.clone(),
            patches: patches.clone(),
            dock: dock.clone(),
            out: out.clone(),
        })
        .expect("phase report");
    }

    assert_hit_coordinates_translate_back_to_matches(&genome, &hits);
    assert_eq!(
        table_value(
            &context,
            "candidate_id",
            "pos_known_cds_001",
            "context_class"
        ),
        "K1_known_cds_same_frame"
    );
    assert_eq!(
        table_value(
            &context,
            "candidate_id",
            "pos_alt_frame_001",
            "context_class"
        ),
        "K2_known_locus_alt_frame"
    );
    assert_eq!(
        table_value(&context, "candidate_id", "pos_lnc_orf_001", "context_class"),
        "K4_lncRNA_or_nc_transcript_orf"
    );
    assert_eq!(
        table_value(
            &context,
            "candidate_id",
            "decoy_low_complexity_001",
            "context_class"
        ),
        "K_LOW_COMPLEXITY_DECOY"
    );
    assert!(
        table_value(
            &gate,
            "candidate_id",
            "decoy_low_complexity_001",
            "failure_reason"
        )
        .contains("low_complexity")
    );
    assert!(
        table_value(
            &gate,
            "candidate_id",
            "decoy_low_complexity_001",
            "pass_plm"
        ) == "false"
    );
    let known_score = table_value(
        &cooccur,
        "candidate_id",
        "pos_known_cds_001",
        "final_cooccur_score",
    )
    .parse::<f64>()
    .expect("known score");
    let single_score = table_value(
        &cooccur,
        "candidate_id",
        "decoy_single_fragment_001",
        "final_cooccur_score",
    )
    .parse::<f64>()
    .expect("single score");
    assert!(known_score > single_score);
    assert!(
        table_value(
            &cooccur,
            "candidate_id",
            "decoy_single_fragment_001",
            "cooccur_reason"
        )
        .contains("single_fragment")
    );
    let boundary_rows = matching_rows(&orfs, "candidate_id", "boundary_stop_001");
    assert!(
        boundary_rows
            .iter()
            .all(|row| !row.contains("pos_yhedlv;pos_kqwc"))
    );
    assert_eq!(
        table_value(&orfs, "candidate_id", "pos_known_cds_001", "parent_orf_id"),
        "pos_known_cds_001:+1:0-42"
    );
    assert_eq!(
        table_value(&orfs, "candidate_id", "pos_alt_frame_001", "parent_orf_id"),
        "pos_alt_frame_001:+2:1-43"
    );
    assert_eq!(
        table_value(&orfs, "candidate_id", "pos_lnc_orf_001", "parent_orf_id"),
        "pos_lnc_orf_001:-1:0-36"
    );
    assert_eq!(
        table_value(&orfs, "candidate_id", "pos_lnc_orf_001", "strand"),
        "-"
    );
    assert_eq!(
        table_value(&orfs, "candidate_id", "pos_lnc_orf_001", "frame"),
        "-1"
    );
    assert_eq!(
        table_value(&orfs, "candidate_id", "pos_alt_frame_001", "frame"),
        "2"
    );
    assert_eq!(
        fs::read_to_string(report).expect("read report"),
        fs::read_to_string(report_again).expect("read report again")
    );
}

#[test]
fn phase_0_2_nextflow_resume_hash_record_is_consistent_when_present() {
    let workspace = workspace_root();
    let workflow = workspace
        .join("workflows")
        .join("phase_0_2_mini_benchmark.nf");
    let workflow_text = fs::read_to_string(&workflow).expect("read phase workflow");
    for required in [
        "cache 'deep'",
        "sourceFingerprint()",
        "report markdown",
        "GENOLIGAND_CANDIDATE_REPORT.md",
    ] {
        assert!(
            workflow_text.contains(required),
            "workflow missing deterministic resume contract token: {required}"
        );
    }

    let compare = workspace
        .join("acceptance_artifacts_phase_0_2")
        .join("metrics")
        .join("resume_hash_compare.tsv");
    if compare.exists() {
        let rows = table_rows(&compare);
        let row = rows.first().expect("resume hash row");
        let first = row.get("first_hash").expect("first hash");
        let resume = row.get("resume_hash").expect("resume hash");
        assert_eq!(first, resume);
        assert_eq!(row.get("status").expect("resume status"), "PASS");
    }
}

#[test]
fn phase_0_3_fixture_covers_context_evidence_and_decoy_semantics() {
    let root = temp_root("phase-0-3");
    let workspace = workspace_root();
    let fixtures = workspace.join("fixtures").join("phase_0_3");
    let genome = fixtures.join("genome.fa");
    let fragments = fixtures.join("fragment_kernel.json");
    let annotation = fixtures.join("annotation.gff3");
    let evidence = fixtures.join("evidence_table.tsv");
    let hits = root.join("hits.tsv");
    let stats = root.join("stats.tsv");
    let orfs = root.join("orf_extensions.tsv");
    let context = root.join("context.tsv");
    let cooccur = root.join("cooccurrence.tsv");
    let gate = root.join("orf_gate.tsv");
    let report = root.join("candidate_report.md");

    run_search_genome(&SearchOptions {
        genome: genome.clone(),
        fragments,
        out: hits.clone(),
    })
    .expect("phase 0.3 search");
    run_stats_score(&StatsOptions {
        hits: hits.clone(),
        out: stats.clone(),
    })
    .expect("phase 0.3 stats");
    run_orf_extend(&OrfExtendOptions {
        genome: genome.clone(),
        hits: hits.clone(),
        out: orfs.clone(),
    })
    .expect("phase 0.3 orf");
    run_classify_loci(&ClassifyLociOptions {
        annotation,
        orfs: orfs.clone(),
        out: context.clone(),
    })
    .expect("phase 0.3 context");
    run_cooccur_score(&CooccurScoreOptions {
        hits: hits.clone(),
        stats: stats.clone(),
        orfs: orfs.clone(),
        context: context.clone(),
        out: cooccur.clone(),
    })
    .expect("phase 0.3 cooccur");
    gls_cli::stages::run_gate_short_circuit_phase_with_evidence(
        &PhaseGateShortCircuitOptions {
            stats: stats.clone(),
            orfs: orfs.clone(),
            context: context.clone(),
            cooccur: cooccur.clone(),
            out: gate.clone(),
        },
        Some(&evidence),
    )
    .expect("phase 0.3 gate");
    gls_cli::stages::run_report_candidate(&gls_cli::stages::ReportCandidateOptions {
        hits: hits.clone(),
        stats: stats.clone(),
        orfs: orfs.clone(),
        context: context.clone(),
        cooccur: cooccur.clone(),
        gate: gate.clone(),
        evidence: evidence.clone(),
        out: report.clone(),
    })
    .expect("phase 0.3 report");

    assert_hit_coordinates_translate_back_to_matches(&genome, &hits);
    for (candidate, expected) in [
        ("p03_k1_cds_001", "K1_known_cds_same_frame"),
        ("p03_k2_alt_frame_001", "K2_known_locus_alt_frame"),
        ("p03_k3_utr_001", "K3_utr_orf"),
        ("p03_k4_lnc_001", "K4_lncRNA_or_nc_transcript_orf"),
        ("p03_k5_intronic_001", "K5_intronic_orf"),
        ("p03_k6_antisense_001", "K6_antisense_orf"),
        ("p03_d1_low_complexity_001", "K_LOW_COMPLEXITY_DECOY"),
    ] {
        assert_eq!(
            table_value(&context, "candidate_id", candidate, "context_class"),
            expected
        );
    }
    assert_eq!(
        table_value(
            &gate,
            "candidate_id",
            "p03_d1_low_complexity_001",
            "pass_plm"
        ),
        "false"
    );
    assert!(
        table_value(
            &gate,
            "candidate_id",
            "p03_d1_low_complexity_001",
            "decision_reason"
        )
        .contains("low_complexity")
    );
    assert!(
        table_value(&gate, "candidate_id", "p03_d2_background_001", "risk_flags")
            .contains("low_information_background")
    );
    assert_eq!(
        table_value(&gate, "candidate_id", "p03_k5_intronic_001", "decision"),
        "exploration_queue"
    );
    assert!(
        table_value(&gate, "candidate_id", "p03_k5_intronic_001", "risk_flags")
            .contains("low_context_evidence")
    );
    let k1_score = table_value(
        &cooccur,
        "candidate_id",
        "p03_k1_cds_001",
        "cooccurrence_score",
    )
    .parse::<f64>()
    .expect("k1 score");
    let d2_score = table_value(
        &cooccur,
        "candidate_id",
        "p03_d2_background_001",
        "cooccurrence_score",
    )
    .parse::<f64>()
    .expect("d2 score");
    assert!(k1_score > d2_score);
    let report_text = fs::read_to_string(report).expect("read phase 0.3 report");
    for section in [
        "## Input Summary",
        "## Fragment Kernel Summary",
        "## Genome/Annotation Summary",
        "## Evidence Summary",
        "## Candidate Ranking Table",
        "## Context Class Distribution",
        "## Top Candidate Details",
        "## Filtered Decoys",
        "## Interpretation Cautions",
        "## Reproducibility Metadata",
    ] {
        assert!(report_text.contains(section), "missing section {section}");
    }
    assert!(report_text.contains("未注释 ORF 候选不能被解释为已证实蛋白"));
}

fn temp_root(label: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "genoligand-{label}-{}-{}",
        std::process::id(),
        std::thread::current().name().unwrap_or("test")
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("create temp root");
    root
}

fn write_fixture_files(genome: &PathBuf, fragments: &PathBuf, ligand: &PathBuf) {
    fs::write(
        genome,
        ">toy_dark\nNNNNNNATGTATCATGAAGATCTTGTTTAAACCATGGCTGCTGCTGCTGCTGCTTAANNN\n",
    )
    .expect("write genome");
    fs::write(
        fragments,
        r#"{
  "schema_version": "1.0",
  "fragments": [
    {"id": "pos_yhedlv", "kind": "exact", "sequence": "YHEDLV", "information_weight": 1.0},
    {"id": "decoy_ala6", "kind": "exact", "sequence": "AAAAAA", "information_weight": 1.0}
  ]
}
"#,
    )
    .expect("write fragments");
    fs::write(
        ligand,
        "\
ligand_id: toy_ligand
ligand_class: generic
fragments:
  - exact: YHEDLV
  - exact: AAAAAA
  - residue: Y
  - residue: H
  - residue: E
  - residue: D
  - residue: L
  - residue: V
",
    )
    .expect("write ligand");
}

fn workspace_root() -> PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("workspace root")
        .to_path_buf()
}

fn table_value(path: &PathBuf, key_column: &str, key_value: &str, target_column: &str) -> String {
    let text = fs::read_to_string(path).expect("read table");
    let mut lines = text.lines();
    let headers = lines
        .next()
        .expect("headers")
        .split('\t')
        .collect::<Vec<_>>();
    let key_index = headers
        .iter()
        .position(|header| *header == key_column)
        .expect("key column");
    let target_index = headers
        .iter()
        .position(|header| *header == target_column)
        .expect("target column");
    for line in lines {
        let cells = line.split('\t').collect::<Vec<_>>();
        if cells.get(key_index).copied() == Some(key_value) {
            return cells
                .get(target_index)
                .copied()
                .unwrap_or_default()
                .to_string();
        }
    }
    panic!("missing row {key_column}={key_value} in {}", path.display());
}

fn matching_rows(path: &PathBuf, key_column: &str, key_value: &str) -> Vec<String> {
    let text = fs::read_to_string(path).expect("read table");
    let mut lines = text.lines();
    let headers = lines
        .next()
        .expect("headers")
        .split('\t')
        .collect::<Vec<_>>();
    let key_index = headers
        .iter()
        .position(|header| *header == key_column)
        .expect("key column");
    lines
        .filter(|line| line.split('\t').nth(key_index) == Some(key_value))
        .map(ToOwned::to_owned)
        .collect()
}

fn table_rows(path: &PathBuf) -> Vec<HashMap<String, String>> {
    let text = fs::read_to_string(path).expect("read table");
    let mut lines = text.lines();
    let headers = lines
        .next()
        .expect("headers")
        .split('\t')
        .map(clean_tsv_cell)
        .collect::<Vec<_>>();
    lines
        .map(|line| {
            headers
                .iter()
                .cloned()
                .zip(line.split('\t').map(clean_tsv_cell))
                .collect::<HashMap<_, _>>()
        })
        .collect()
}

fn clean_tsv_cell(cell: &str) -> String {
    cell.trim_matches('"').to_string()
}

fn assert_hit_coordinates_translate_back_to_matches(genome: &PathBuf, hits: &PathBuf) {
    let fasta = fs::read_to_string(genome).expect("read genome");
    let records = parse_fasta(&fasta)
        .expect("parse genome")
        .into_iter()
        .map(|record| (record.id, record.sequence))
        .collect::<HashMap<_, _>>();
    for row in table_rows(hits) {
        let chromosome = row.get("chromosome").expect("chromosome");
        let sequence = records.get(chromosome).expect("record sequence");
        let start = row
            .get("start")
            .expect("start")
            .parse::<usize>()
            .expect("start usize");
        let end = row
            .get("end")
            .expect("end")
            .parse::<usize>()
            .expect("end usize");
        assert!(start < end, "hit must have positive width: {row:?}");
        assert!(
            end <= sequence.len(),
            "hit must stay inside record: {row:?}"
        );
        assert_eq!((end - start) % 3, 0, "hit must cover full codons: {row:?}");
        let dna = &sequence[start..end];
        let coding_dna = if row.get("strand").expect("strand") == "-" {
            reverse_complement(dna)
        } else {
            dna.to_string()
        };
        assert_eq!(
            translate_frame(&coding_dna, 0),
            *row.get("match_sequence").expect("match sequence"),
            "DNA coordinate span must translate back to the matched peptide"
        );
    }
}
