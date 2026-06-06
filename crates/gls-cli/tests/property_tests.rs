use gls_cli::stages::{
    DockOptions, FoldgateOptions, GatePlmOptions, GateShortCircuitOptions, PatchOptions,
    SearchOptions, StatsOptions, ValidateKind, ValidateOptions, run_dock, run_foldgate_realize,
    run_gate_plm, run_gate_short_circuit, run_patch_realize, run_search_genome, run_stats_score,
    run_validate,
};
use gls_genome::{reverse_complement, six_frame_translations};
use gls_stats::benjamini_hochberg;
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
