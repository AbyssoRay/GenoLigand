# Phase 0.3 Fixture

This fixture is a mixed-source, deterministic Phase 0.3 validation set.

- `real_ensembl_shh_excerpt_001` is a public Ensembl REST excerpt from human GRCh38 chromosome 7 around `SHH` (`7:155799900..155800080`). It is included as a real public sequence/provenance control.
- `p03_*` contigs are real-format synthetic scaffold controls used to force the required K1/K2/K3/K4/K5/K6 and D1/D2 semantic gates in a tiny offline test set.

Source status: `real_public_excerpt` for the SHH excerpt, and `real_format_synthetic_sequence` for the engineered semantic controls.

The synthetic controls are not biological discoveries. They exist only to verify engineering behavior: ORF extension, annotation overlap semantics, evidence summarization, co-occurrence ranking, decoy filtering, and candidate reporting.

Expected classes are recorded in `expected_semantics.yml`. Evidence rows are intentionally small and fixture-like; they prove the system can read and summarize evidence layers, not that any candidate is experimentally validated.
