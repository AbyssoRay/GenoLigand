# Phase 0.2 Mini Benchmark Fixture

This fixture is deterministic, offline, and intentionally small. FASTA record IDs are also the expected candidate IDs so stage outputs can be audited without external mapping.

## Regions

- `pos_known_cds_001`: plus-strand protein-coding CDS with `YHEDLV`, `KQWCM`, `QWCMR`, and `KQWC`; expected `K1_known_cds_same_frame` and Top 3 ranking.
- `pos_alt_frame_001`: plus-strand ORF starts at frame 2 inside a protein-coding locus whose reference CDS is annotated as frame 1; expected `K2_known_locus_alt_frame`.
- `pos_lnc_orf_001`: minus-strand ORF inside a lncRNA-like transcript; expected `K4_lncRNA_or_nc_transcript_orf` and retained exploratory status.
- `decoy_low_complexity_001`: Ala-rich ORF with `AAAAAA`; expected low-complexity filtering before pLM.
- `decoy_gc_repeat_001`: GC/repeat-like ORF overlapping repeat and pseudogene features; expected repeat penalty and no positive ranking.
- `decoy_single_fragment_001`: single `YHEDLV` hit only; expected below multi-fragment candidates.
- `boundary_stop_001`: two stop-bounded ORFs in one record; extension must stop at the first stop and must not merge downstream fragments.

All coordinates in stage TSV outputs are zero-based half-open genomic intervals. GFF3 uses standard one-based inclusive coordinates.
