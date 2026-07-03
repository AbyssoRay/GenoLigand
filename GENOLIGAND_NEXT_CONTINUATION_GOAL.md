# GenoLigand Next Continuation Goal

Date: 2026-07-03
Current version: 0.9.36
Current commit before this handoff: d319e3bb97aa972c66b987257e06b3fa5008462b
Branch: main
Science store: D:\GenoLigandData\science_store

## Direction Correction

The next session must continue from the user's corrected requirement:

- Do not treat a few-thousand-row TSV projection as the database when the downloaded databases are tens or hundreds of GB.
- Do not solve mismatches by changing wording. If the UI/report says onsite search, docking, complex MD, CUDA, or final evidence is present, the feature must actually run and be backed by current-run artifacts.
- Do not let test fixtures, sample outputs, smoke checks, or static fallback outputs enter final candidate ranking, evidence, UI, report, or export.
- The UI should directly show the final scientific results. Export is supplementary, not the only way to inspect results.
- Completion requires a real input run proving onsite search, docking, receptor-ligand complex MD, CUDA monitoring, report, and UI display from the current science store.

## Current Verified State

- Git was clean before writing this file.
- Version is 0.9.36.
- Latest pushed implementation commit is d319e3bb97aa972c66b987257e06b3fa5008462b.
- Ports 19190-19194 were not listening during the check.
- Runtime binaries had been published in C:\Users\BeiChen\AppData\Local\GenoLigand\runtime\bin during the previous 0.9.36 work.
- Science store reports ready, but this ready state is not sufficient proof of full database correctness.

Current local index sizes:

- indexes\known_targets.tsv: 5,125 lines, about 3.0 MB.
- indexes\metagenome_targets.tsv: 5,001 lines, about 2.0 MB.
- indexes\structure_targets.tsv: 26,006 lines, about 6.3 MB.
- indexes\mmseqs\mmseqs_target_databases.tsv: 7 lines.
- indexes\foldseek\foldseek_target_databases.tsv: 2 lines.

Large persistent databases do exist:

- MMseqs metagenome target DB includes files over 100 GB, including target about 122.6 GB and target.lookup about 20.7 GB.
- Foldseek RCSB target DB exists with GB-scale index files.
- Downloads include large sources such as MGnify mgy_clusters.fa.gz about 79.3 GB, ChEMBL sqlite archive about 5.7 GB, BindingDB zip about 591 MB, and PLINDER annotation data about 1.0 GB.

## Newly Confirmed Problems

### P0: TSV projection is still being confused with database completeness

Evidence from D:\GenoLigandData\science_store\index_build_manifest.tsv:

```text
known_targets downloads/bindingdb/bindingdb_all.tsv.zip used 5000 indexes/known_targets.tsv canonical_or_source_table_rows
known_targets downloads/chembl\chembl_sqlite.tar.gz used 5000 indexes/known_targets.tsv chembl_sqlite_archive_rows
known_targets downloads/uniprot\uniprot_human_reviewed.fasta.gz used 5000 indexes/known_targets.tsv fasta_sequence_rows
metagenome_targets downloads/metagenome\mgnify\mgy_clusters.fa.gz used 5000 indexes/metagenome_targets.tsv fasta_sequence_rows
```

Impact:

- The current "ready" status can coexist with legacy 5,000-row projections from large source files.
- This can mislead Diagnostics, UI, preflight, and future review into saying the database is complete when only a small projection is present.
- Persistent MMseqs/Foldseek databases are real and large, but their existence does not fix known-target TSV projection completeness by itself.

Required fix:

- Installer and setup API must classify these 5,000-row projections as summary/projection indexes, not full databases.
- Readiness must distinguish:
  - full searchable source database,
  - persistent MMseqs/Foldseek DB,
  - UI summary/projection TSV,
  - incomplete legacy sampled TSV.
- A large source with a small projection must trigger repair or an explicit "projection only" state, not plain ready.

### P0: Known database onsite search is not complete enough

Current status:

- Small molecule onsite BindingDB zip query exists and worked for acetazolamide.
- ChEMBL sqlite and BindingDB large sources are not yet represented as a complete searchable local known-target service in final readiness semantics.
- The 5,000-row known_targets.tsv must not be treated as full known database coverage.

Required fix:

- Keep BindingDB onsite query for exact/canonical small molecules.
- Add or confirm ChEMBL onsite query against the sqlite archive for current small molecule and target evidence, not a 5,000-row projection.
- Ensure known_database_matches.tsv records source, row provenance, activity value, target id, ligand identity, and no test/static rows.
- Candidate ranking must prefer current-run onsite hits over projection matches.

### P0: Final UI must directly show complete scientific results

Current concern:

- Server and UI have hard display caps in several places, including 5,000-row and 500-row limits.
- This is acceptable only as pagination or viewport protection, not as final data truncation.
- UI should show direct result pages for Candidate Ranking, Candidate Detail, Evidence Ledger, Genome/ORF, Structure/Pocket, Docking, MD Summary, Report, and Export.

Required fix:

- Add server-side pagination/filter metadata for large tables.
- UI must display totals, page controls, active filters, and direct detail panels.
- Export remains available for full files, but the normal page must show the scientific outputs directly.

### P0: Docking and complex MD must be mandatory current-run artifacts for claims

Current status from previous 0.9.36 run:

- A real acetazolamide run produced BindingDB onsite hit P00918, structure coordinate, Vina docking, OpenMM CUDA MD, trajectory, protocol, state, and GPU telemetry.
- That was good evidence for one path, but next completion cannot rely on a single previous run or on changed wording.

Required fix:

- Pipeline should select top candidates with real receptor coordinates and run docking per selected candidate.
- Complex MD must start from the real receptor coordinate and docking pose.
- MD score must be computed only when trajectory metrics and complex manifest are current-run artifacts.
- If docking/MD fails for a candidate, the candidate detail must show a concrete failure reason and the ranking must not pretend MD support exists.

### P1: Installer repair path still needs stricter behavior for legacy projections

Current observation:

- A short installer repair run returned ready and showed debug stdout "science_indexes_reused_for_alignment_repair" even though index_build_manifest.tsv still listed 5,000-row projections for large known/metagenome sources.
- A temporary test was started to catch BindingDB 600 MB with only 5,000 rows, but it was not completed or committed. The half-finished code was removed before this handoff.

Required fix:

- Write a failing install contract test:
  - create a large BindingDB-like source over the default large-source threshold,
  - set index_build_manifest.tsv to used 5000,
  - assert existing_science_indexes_ready returns false.
- Implement a robust threshold and per-source completeness rule.
- Re-run installer against the real science store and verify the old projection rows are rebuilt or explicitly marked projection_only/incomplete.

### P1: Report wording should follow function, not replace it

Current correction:

- Do not "fix" claims by weakening wording when the intended software standard is higher.
- The right direction is to make onsite search, docking, complex MD, CUDA, and UI result display actually satisfy the current product language.

Required fix:

- Keep report language ambitious only where the artifacts prove it.
- Use explicit "not ready" or "not run" only when a real feature cannot run for a concrete current-run reason.
- Do not hide failures under vague language.

### P1: Need a stricter no-test-artifact gate

Required fix:

- Add an authenticity gate that scans final run artifacts for fixture/sample/smoke/test-only source tags.
- Block final ranking/report/export from consuming:
  - unit-test fixture rows,
  - smoke outputs,
  - synthetic placeholder rows,
  - static fallback candidate rows,
  - demo/sample data.
- Permit tests to create fixtures only under test temp directories.

## Concrete Next Plan

1. Start with the install/readiness contract.
   - Add a failing test for large BindingDB/ChEMBL/MGnify sources with small projection rows.
   - Make installer readiness distinguish full database, persistent DB, projection TSV, and incomplete legacy sample.
   - Re-run install repair and verify real index_build_manifest no longer claims 5,000-row large-source projections as ready.

2. Strengthen known onsite search.
   - Add ChEMBL sqlite onsite query for current small molecule.
   - Keep BindingDB onsite exact/canonical query.
   - Make known_database_matches.tsv source rows come from current-run queries or declared projection rows with explicit scope.

3. Strengthen onsite sequence/structure search.
   - Verify MMseqs2 searches all ready target DB rows in indexes\mmseqs\mmseqs_target_databases.tsv.
   - Verify Foldseek searches the ready RCSB target DB with -a and convertalis output.
   - Ensure output rows enter candidate ranking and evidence with source DB identity and current-run query ID.

4. Strengthen docking and complex MD.
   - Run docking for selected top coordinate-backed candidates, not a static or prior pose.
   - Run OpenMM receptor-ligand complex MD from the docking pose.
   - Require md_protocol.tsv, md_state.tsv, trajectory, complex manifest, and GPU telemetry for MD support.

5. Make UI directly inspect large results.
   - Add paginated endpoints or pagination metadata for ranking/evidence/structure/MD.
   - UI should show totals, page navigation, filters, and detail panels.
   - Do not rely on export-only inspection for final results.

6. Run a real end-to-end validation.
   - Use a small, known-target drug input selected from public sources.
   - Verify onsite search, docking, complex MD, CUDA telemetry, report, UI pages, and export.
   - Confirm no test/smoke/static fallback artifacts are consumed.

7. Finish with normal release hygiene.
   - Increment patch version.
   - Run fmt, clippy, workspace tests, Python compile, install/webui/authenticity contracts, JS checks.
   - Clean target, __pycache__, temporary validation runs.
   - Commit and push.

## Current Stop State

- No implementation changes from the unfinished strict-readiness test are retained.
- This handoff file is the only intended new file from the final convergence step.
- The active work should resume from the problems above, not from the earlier wording-only fixes.
