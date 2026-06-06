# GenoLigand

GenoLigand is a Rust workspace for genome-scale ligand-aware target discovery. It keeps the pipeline split into explicit stages for genome search, statistical scoring, short-circuit gating, pLM gating, fold/patch realization, docking, validation, and reproducible smoke workflows.

The current developer release is `v0.1.0`. It is intentionally conservative: toy fixtures, schema checks, property tests, and Nextflow smoke runs are used to keep stage contracts visible while the heavier scientific workers remain replaceable behind stable CLI boundaries.

## Developer Commands

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo test --workspace --all-features --test property_tests
```

Useful CLI entry points:

```powershell
gls-cli search genome --genome <genome.fa> --fragments <fragment_kernel.json> --out hits.tsv
gls-cli stats score --hits hits.tsv --out stats.tsv
gls-cli orf extend --genome <genome.fa> --hits hits.tsv --out orfs.tsv
gls-cli classify loci --annotation <annotation.gff3> --orfs orfs.tsv --out context.tsv
gls-cli cooccur score --hits hits.tsv --stats stats.tsv --orfs orfs.tsv --context context.tsv --out cooccur.tsv
gls-cli gate short-circuit --stats stats.tsv --orfs orfs.tsv --context context.tsv --cooccur cooccur.tsv --out gate.tsv
gls-cli gate plm --candidates gate.tsv --profile dev_cpu_smoke --out plm.tsv
gls-cli foldgate realize --candidates plm.tsv --out foldgate.tsv
gls-cli patch realize --foldgate foldgate.tsv --ligand ligand.yaml --out patches.tsv
gls-cli dock run --patches patches.tsv --mode stub --out dock.tsv
gls-cli report markdown --hits hits.tsv --stats stats.tsv --orfs orfs.tsv --context context.tsv --cooccur cooccur.tsv --gate gate.tsv --plm plm.tsv --foldgate foldgate.tsv --patches patches.tsv --dock dock.tsv --out GENOLIGAND_CANDIDATE_REPORT.md
```

## Phase 0.2 Mini Benchmark

Phase 0.2 upgrades the toy split pipeline into a real-format, offline mini benchmark under `fixtures/phase_0_2/`. The fixture includes known-CDS, alternative-frame, lncRNA-like, low-complexity, repeat-associated, single-fragment, and stop-boundary cases. Generated acceptance archives are local-only and ignored by Git.

Run the split CLI chain directly with the commands above, or run the reproducible Nextflow smoke:

```powershell
nextflow run workflows/phase_0_2_mini_benchmark.nf -profile dev_cpu_smoke
nextflow run workflows/phase_0_2_mini_benchmark.nf -profile dev_cpu_smoke -resume
```

## License

GenoLigand is distributed under the project EULA in `EULA.txt`.
