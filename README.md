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

## Phase 0.6 Structure Patch Strict

Phase 0.6 adds a real structure-similarity and binding-patch evidence layer on top of the Phase 0.5 GPU structures. It uses Foldseek against an external RCSB-derived structure subset, confirms top hits with US-align, derives real BioLiP2 binding-patch references, matches candidate patches, calibrates against D1-D5 decoys, and writes a target-prioritization report. The report is explicitly a prioritization artifact: it does not claim target engagement, binding affinity, mechanism, biological activity, docking success, or experimental validation.

Local-only tools, databases, and acceptance archives stay ignored under `.phase_0_6_tools/`, `data/external_phase_0_6/`, `databases/phase_0_6/`, and `acceptance_artifacts_phase_0_6_structure_patch_strict/`.

Useful Phase 0.6 commands:

```powershell
python scripts/phase06/check_phase05_inputs.py
python scripts/phase06/prepare_query_structures.py
python scripts/phase06/prepare_biolip2_subset.py
python scripts/phase06/fetch_rcsb_reference_subset.py
python scripts/phase06/build_reference_patch_db.py
python scripts/phase06/build_foldseek_databases.py --foldseek-bin /opt/foldseek/foldseek/bin/foldseek --wsl-distro GenoLigandPhase06
python scripts/phase06/run_foldseek_search.py --foldseek-bin /opt/foldseek/foldseek/bin/foldseek --wsl-distro GenoLigandPhase06
python scripts/phase06/run_usalign_confirmation.py --usalign-bin .phase_0_6_tools/USalign/USalign.exe
python scripts/phase06/match_candidate_patches.py
python scripts/phase06/calibrate_decoy_margins.py
python scripts/phase06/score_target_priors.py
python scripts/phase06/render_candidate_report_phase_0_6.py
python scripts/validation/check_phase_0_6_structure_patch_strict.py
nextflow run workflows/phase_0_6_structure_patch_strict.nf -profile strict_local -with-trace acceptance_artifacts_phase_0_6_structure_patch_strict/metrics/nextflow_trace_first.tsv
nextflow run workflows/phase_0_6_structure_patch_strict.nf -profile strict_local -resume -with-trace acceptance_artifacts_phase_0_6_structure_patch_strict/metrics/nextflow_trace_resume.tsv
```

## Phase 0.7 Anti-Hallucination Docking Strict

Phase 0.7 adds a ligand-aware docking prefilter under strict anti-hallucination gates. The first accepted mode is `acceptance_control_only`: it uses a real co-crystal positive control and a documented negative ligand to verify ligand ingestion, conformer generation, receptor preparation, docking boxes, real Vina docking, redocking RMSD, interaction fingerprints, decoy calibration, evidence ledger, report linting, and Nextflow first/resume reproducibility. Control-only outputs are engineering evidence only and do not authorize production target-prior claims.

Local-only tools, databases, and acceptance archives stay ignored under `acceptance_artifacts_phase_0_7_anti_hallucination_strict/`, `.phase_0_7_tools/`, `.phase_0_7_env/`, `data/external_phase_0_7/`, `databases/phase_0_7/`, and `models/phase_0_7/`. The preferred shared tool root for RDKit, Meeko, AutoDock Vina, and Open Babel is `D:\user\codex\tools\genoligand-phase07`; set `GENOLIGAND_PHASE07_VINA`, `GENOLIGAND_PHASE07_LIGAND_PREP`, `GENOLIGAND_PHASE07_RECEPTOR_PREP`, and `GENOLIGAND_PHASE07_OBABEL` when using tools outside PATH.

Useful Phase 0.7 commands:

```powershell
python scripts/phase07/run_phase_0_7_anti_hallucination_strict_acceptance.py
python scripts/validation/check_phase_0_7_master_acceptance.py --artifact-dir acceptance_artifacts_phase_0_7_anti_hallucination_strict
nextflow run workflows/phase_0_7_anti_hallucination_docking.nf -profile strict -with-trace -with-report -with-timeline
nextflow run workflows/phase_0_7_anti_hallucination_docking.nf -profile strict -resume -with-trace -with-report -with-timeline
```

## Phase 0.7.1 Audit-Fix Strict

Phase 0.7.1 is an audit-fix strict milestone for the Phase 0.7 control-only docking chain. It repairs metadata and report defensibility only: real AutoDock Vina/RDKit/Meeko/Open Babel/Nextflow versions, first/resume workflow evidence, direct semantic hash comparison, expanded decoy-superiority warnings, confidence-downgrade evidence, claim-to-evidence mapping, and placeholder/claim linting. It does not introduce production query-ligand target prioritization.

Local-only Phase 0.7.1 acceptance outputs stay ignored under `acceptance_artifacts_phase_0_7_1_audit_fix_strict/`, `.phase_0_7_1_tools/`, `.phase_0_7_1_env/`, `data/external_phase_0_7_1/`, `databases/phase_0_7_1/`, and `models/phase_0_7_1/`.

Useful Phase 0.7.1 commands:

```powershell
python scripts/phase07/run_phase_0_7_1_audit_fix_strict_acceptance.py
python scripts/validation/check_phase_0_7_1_audit_fix_strict.py --artifact-dir acceptance_artifacts_phase_0_7_1_audit_fix_strict --phase07-dir acceptance_artifacts_phase_0_7_anti_hallucination_strict --strict
nextflow run workflows/phase_0_7_anti_hallucination_strict.nf -profile control_only -with-trace acceptance_artifacts_phase_0_7_1_audit_fix_strict/outputs/workflow/nextflow_trace_first.tsv
nextflow run workflows/phase_0_7_anti_hallucination_strict.nf -profile control_only -resume -with-trace acceptance_artifacts_phase_0_7_1_audit_fix_strict/outputs/workflow/nextflow_trace_resume.tsv
```

## License

GenoLigand is distributed under the project EULA in `EULA.txt`.
