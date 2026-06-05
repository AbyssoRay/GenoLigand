# GenoLigand-Search V1.6 Pipeline Contract

This contract implements the three files in `goal/` as hard V1.6 requirements.

## Non-Negotiable Gates

- L1 genome candidates must not call pLM directly.
- `gls-plm-gate` accepts only `ShortCircuitGateResult.pass_plm = true`.
- L1 and L1.25 candidates must not enter docking.
- L1.5 accepts only G2/G3 candidates.
- L2 docking accepts only `PatchRealizationResult.pass_l2 = true`.
- `gls-patch-realizer` must reject missing or non-fixture-calibrated `ToleranceProfile`.
- Geometry hash execution must enforce `max_bucket_occupancy`, `max_seed_triplets`, and `max_memory_mb`.

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

`gls-tolerance-calib` generates `narrow`, `standard`, and `relaxed` profiles from positive and hard-negative fixtures. Runtime patch realization tries profiles in that order. Every relaxation is appended to the realization audit and remains bounded by the calibrated bucket, seed, and memory guard fields.

## Reporting

Reports must include candidate counts, retention, failure reasons, CPU time, and peak memory for L1, L1.20, L1.25, L1.5, patch realization, and L2. IDP/IDR outputs are trend only in MVP and must not include macro free energy.
