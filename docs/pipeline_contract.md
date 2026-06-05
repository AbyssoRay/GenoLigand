# GenoLigand-Search V1.6 Pipeline Contract

This contract implements the three files in `goal/` as hard V1.6 requirements.

## Non-Negotiable Gates

- L1 genome candidates must not call pLM directly.
- `gls-plm-gate` accepts only `ShortCircuitGateResult.pass_plm = true`.
- `gls-plm-gate` batches by sequence length, uses a versioned cache key, and requires a GPU batch semaphore before worker invocation.
- L1 and L1.25 candidates must not enter docking.
- L1.5 accepts only G2/G3 candidates.
- L2 docking accepts only `PatchRealizationResult.pass_l2 = true`.
- `gls-patch-realizer` must reject missing or non-fixture-calibrated `ToleranceProfile`.
- Geometry hash execution must enforce `max_bucket_occupancy`, `max_seed_triplets`, and `max_memory_mb`.
- pLM, fold, pocket, and docking science-tool boundaries are persistent stdin/stdout workers in the V1.6 dev profile; production adapters must keep the same no-cold-start process boundary.

## Data Flow

```text
L0 gls-ligand
  -> FragmentGrammar + InteractionConstraintGraph
L1 gls-genome + gls-fragment + gls-stats
  -> FragmentHit + OrfCandidate
L1.20 gls-shortcircuit
  -> ShortCircuitGateResult
L1.25 gls-plm-gate
  -> FoldabilityGateResult
L1.5 gls-foldgate + gls-patch-realizer
  -> StructureGateResult + PatchRealizationResult + GeometryBudgetReport
L2 gls-dock
  -> DockingResult
```

## Short-Circuit Evaluation Order

1. Length, stop-to-stop ORF, and invalid residue checks.
2. Low complexity, repeat, and amino-acid entropy checks.
3. Fragment information and co-occurrence q-value checks.
4. Exact sequence hash deduplication and SimHash-like near clustering.
5. Per-shard, per-fragment-family, and per-locus-class pLM budgets.
6. Lightweight context priority; dark-genome candidates are held rather than deleted for missing annotation.
7. pLM invocation list.

## Tolerance Calibration

`gls-tolerance-calib` generates `narrow`, `standard`, and `relaxed` profiles from positive and hard-negative fixtures. Runtime patch realization tries profiles in that order. Every relaxation is appended to the realization audit and remains bounded by the calibrated bucket, seed, and memory guard fields. The same `ToleranceProfile` also carries calibrated SASA, occlusion, pocket-overlap, and rotamer-feasibility thresholds; `gls-patch-realizer` must not read those thresholds from runtime config.

Fixture manifests are imported through an explicit V1.6 TSV contract rather than inferred filenames. Each row must carry `fixture_id`, `fixture_type`, `split`, `source`, `license_note`, `ligand_id`, `pocket_id`, comma-separated `interaction_labels`, `resolution_angstrom`, `interaction_kind`, `ligand_class`, `structure_confidence_bin`, residue signature, observed geometry/surface metrics, bucket occupancy, seed triplets, runtime, and peak memory. The corresponding JSON contracts are `schemas/fixture_manifest.schema.json` and `schemas/calibration_report.schema.json`.

`gls-geometry-bench` reports a coarse-to-fine recall curve over calibrated profiles. Every point records profile level, geometry hash bucket occupancy, memory estimate, seed triplet count, guard state, OOM risk score, and whether the fixture recall probe passed.

## Reporting

Reports must include candidate counts, retention, failure reasons, CPU time, and peak memory for L1, L1.20, L1.25, L1.5, patch realization, and L2. IDP/IDR outputs are trend only in MVP and must not include macro free energy.

The smoke runner has built-in example ligand/genome inputs for CI and Nextflow, but the same path accepts explicit files through `gls-cli dev-smoke --ligand-yaml PATH --genome-fasta PATH` or the environment variables `GENOLIGAND_LIGAND_YAML_FILE` and `GENOLIGAND_GENOME_FASTA_FILE`.

## L1.25 pLM Versioning

The default pLM path uses a small primary model. Boundary candidates can be upgraded to a larger model only after L1.20 has already selected them. Cache identity is `sequence_hash + model_id + tokenizer_id + weights_hash`, so primary and upgraded embeddings are never conflated.

## L2 Docking Manifest

`gls-dock` builds a batch manifest from `PatchRealizationResult` values. Manifest entries are emitted only for `pass_l2 = true`; rejected candidate ids are recorded separately with a machine-readable reason. Vina-style output parsing accepts both tabular mode lines and `REMARK VINA RESULT:` lines.

## Worker Protocol

The executable `gls-cli` provides the dev worker commands `worker-plm-dev`, `worker-fold-dev`, `worker-pocket-dev`, and `worker-dock-dev`. The default `gls-cli dev-smoke` command launches persistent child processes and communicates over a line protocol. By default those child processes are the built-in dev commands; production adapters can replace them with `GENOLIGAND_PLM_WORKER_BIN`, `GENOLIGAND_FOLD_WORKER_BIN`, `GENOLIGAND_POCKET_WORKER_BIN`, and `GENOLIGAND_DOCK_WORKER_BIN`, plus matching `_ARGS` variables. Missing or failing external workers are reported as machine-readable worker spawn/protocol failures, never silently downgraded to in-process calls.

The repository ships adapter shims under `workers/`: pLM, fold, and pocket wrappers relay the persistent line protocol to configured ESM/ColabFold/P2Rank/fpocket commands, while `workers/dock-worker/vina_worker.py` invokes AutoDock Vina against PDBQT inputs and parses Vina scores back into the L2 protocol.
