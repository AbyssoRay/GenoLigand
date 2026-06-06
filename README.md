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

## Phase 0.3 Real-Format Semantics

Phase 0.3 adds a mixed-source fixture under `fixtures/phase_0_3/`: one Ensembl REST SHH public excerpt for provenance plus real-format synthetic scaffold controls for K1/K2/K3/K4/K5/K6 and decoy gates. The synthetic controls are engineering fixtures only and are not biological discoveries.

Run the Phase 0.3 split chain with file inputs:

```powershell
gls-cli search genome --genome fixtures/phase_0_3/genome.fa --fragments fixtures/phase_0_3/fragment_kernel.json --out hits.tsv
gls-cli stats score --hits hits.tsv --out stats.tsv
gls-cli orf extend --genome fixtures/phase_0_3/genome.fa --hits hits.tsv --out orf_extensions.tsv
gls-cli context classify --annotation fixtures/phase_0_3/annotation.gff3 --orfs orf_extensions.tsv --out context.tsv
gls-cli cooccur score --hits hits.tsv --stats stats.tsv --orfs orf_extensions.tsv --context context.tsv --out cooccurrence.tsv
gls-cli gate short-circuit --stats stats.tsv --orfs orf_extensions.tsv --context context.tsv --cooccur cooccurrence.tsv --evidence fixtures/phase_0_3/evidence_table.tsv --out orf_gate.tsv
gls-cli report candidate --hits hits.tsv --stats stats.tsv --orfs orf_extensions.tsv --context context.tsv --cooccur cooccurrence.tsv --gate orf_gate.tsv --evidence fixtures/phase_0_3/evidence_table.tsv --out candidate_report.md
```

Run the reproducible workflow:

```powershell
nextflow run workflows/phase_0_3.nf -profile dev_cpu_smoke
nextflow run workflows/phase_0_3.nf -profile dev_cpu_smoke -resume
```

## Phase 0.4 Sequence Evidence

Phase 0.4 adds a sequence evidence layer for Phase 0.3 candidate ORFs. It exports candidate/control FASTA, computes deterministic sequence features, embeds sequences with the real `esm2_t6_8M_UR50D` fair-esm backend, runs real Swiss-Prot similarity through DIAMOND, scores sequence evidence, and writes a candidate report with explicit caution text. The fixture/mock path remains available only for deterministic workflow checks; final acceptance requires real pLM and real similarity resources.

Local environments, model weights, databases, and acceptance archives stay in ignored project directories such as `.phase_0_4_env/`, `.phase_0_4_tools/`, `data/external_phase_0_4/`, and `acceptance_artifacts_phase_0_4/`.

Useful Phase 0.4 commands:

```powershell
gls-cli sequence export-fasta --orfs orf_extensions.tsv --context context.tsv --cooccur cooccurrence.tsv --gate orf_gate.tsv --out-fasta candidate_orfs.fasta --out-tsv candidate_orfs.tsv
gls-cli sequence features --candidates candidate_orfs.tsv --config configs/phase_0_4.yaml --out sequence_features.tsv
python -m geno_plm.cli diagnose-device --out cuda_env.json
python -m geno_plm.cli embed --input-fasta candidate_orfs.fasta --output-dir plm_embeddings --backend esm2_fair --model-id esm2_t6_8M_UR50D --device cpu --metadata plm_embeddings/embedding_metadata.json
gls-cli sequence score --candidates candidate_orfs.tsv --features sequence_features.tsv --embeddings plm_embeddings/embedding_manifest.tsv --similarity similarity_summary.tsv --weights configs/sequence_evidence_weights.yaml --out sequence_evidence.tsv
gls-cli report phase-0-4 --candidates candidate_orfs.tsv --features sequence_features.tsv --embeddings plm_embeddings/embedding_manifest.tsv --similarity similarity_summary.tsv --evidence sequence_evidence.tsv --out candidate_report_phase_0_4.md
```

Reproducible mock workflow:

```powershell
nextflow run workflows/phase_0_4_sequence_evidence.nf -profile mock
nextflow run workflows/phase_0_4_sequence_evidence.nf -profile mock -resume
```

## Phase 0.5 GPU Strict

Phase 0.5 adds the GPU-only structure evidence layer. It requires CUDA diagnostics, CUDA PyTorch, GPU ESM-2 embeddings, GPU ESMFold structure prediction, structure quality metrics, 3D patch descriptors, a candidate ranking report, and a GPU-labeled Nextflow workflow to pass together. CPU or mock execution is useful for debugging only and cannot satisfy Phase 0.5 acceptance.

Local-only resources stay ignored under `.phase_0_5_env/`, `models/phase_0_5/`, `databases/phase_0_5/`, and `acceptance_artifacts_phase_0_5_gpu_strict/`. The CUDA Toolkit path is configured as `D:\Program Files\CUDA`.

Useful Phase 0.5 commands:

```powershell
python scripts/gpu/check_cuda_env.py --strict --nvcc "D:\Program Files\CUDA\bin\nvcc.exe" --out cuda_env.json
python scripts/gpu/gpu_tensor_smoke_test.py --device cuda:0 --out cuda_tensor_smoke_test.json
python scripts/plm/embed_candidates_gpu.py --input-fasta candidate_orfs.fasta --candidate-tsv candidate_orfs.tsv --output-dir plm_gpu --device cuda:0
python scripts/structure/predict_structures_gpu.py --input-fasta candidate_orfs.fasta --candidate-tsv candidate_orfs.tsv --output-dir structures --device cuda:0
gls-cli report phase-0-5 --candidates candidate_orfs.tsv --sequence-evidence sequence_evidence.tsv --plm-gpu plm_gpu/plm_embeddings.tsv --structure-manifest structures/structure_manifest.tsv --structure-quality structure_quality.tsv --patches patch_descriptors.tsv --out candidate_report_phase_0_5.md --score-out structure_score_decomposition.tsv
nextflow run workflows/phase_0_5_gpu_strict.nf -profile gpu -with-trace -with-report -with-timeline
nextflow run workflows/phase_0_5_gpu_strict.nf -profile gpu -resume -with-trace
```

## License

GenoLigand is distributed under the project EULA in `EULA.txt`.
