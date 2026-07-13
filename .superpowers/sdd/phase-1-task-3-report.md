# Phase 1 Task 3 RED/GREEN Report

## Baseline and scope

- Baseline HEAD: `1a52d581d3aa83814028b5d9731ad960869cf28f`
- Branch: `codex/fix-audit-44`
- Version: `0.9.45` -> `0.9.46`
- Scope: field-aware scientific classification, RDKit normalization for inline and mapped molecular structures, streaming local identifier lookup, CAS validation, and the authoritative input inspection API.
- Task 2 contracts retained: source provenance, structure metadata, strict manifests, record identity, multi-record file parsing, and file-content SHA-256.

## RED evidence

The initial test import error for `ConvertedInput` was corrected before RED was accepted. The following failures exercised the missing behavior:

| Command | RED result |
|---|---|
| `cargo test -p gls-science explicit_small_molecule_field_prevents_sequence_reclassification` | Failed: `CCCCCCCC` produced `GenomeOrMetagenome` instead of `SmallMolecule`. |
| `cargo test -p gls-science inline_smiles_and_inchi_are_canonicalized_by_rdkit` | Failed: `C(C)O` remained uncanonicalized instead of becoming `CCO`. |
| `cargo test -p gls-science compound_identifier_lookup_validates_cas_and_canonicalizes_mapped_smiles` | Failed: valid CAS/name rows had no canonical kind because the local index was not consumed. |
| `cargo test -p gls-science compound_identifier_lookup_rejects_invalid_cas_and_unknown_name` | Failed: invalid CAS did not report `invalid_cas_check_digit`. |
| `cargo test -p gls-science declared_sequence_and_complex_fields_determine_scientific_family` | Failed: a short declared protein was reclassified as peptide. |
| `cargo test -p gls-science compound_identifier_lookup_rejects_invalid_mapped_smiles` | Failed: a bad mapped SMILES did not report `invalid_mapped_smiles`. |
| `cargo test -p gls-science input_conversion_prefers_declared_field_over_sequence_like_content` | Failed: declared small-molecule content was classified as sequence. |
| `cargo test -p gls-science input_conversion_does_not_reclassify_declared_small_molecule_as_nucleotide` | Failed: nucleotide-like text under `small_molecule` normalized successfully as genome input. |
| `cargo test -p gls-science input_inspection_reports_every_record_identity_and_failure` | Compile RED `E0425`: `inspect_input_manifest` did not exist. |

During GREEN, an additional failure exposed Windows `.cmd` to WSL shell parsing of parentheses in inline SMILES. The resolver protocol was changed from raw `--text` to UTF-8 `--text-hex`, preserving arbitrary molecular text without shell interpretation.

## Implementation

- Dispatches explicit `small_molecule`/`smiles`/`inchi`/`cas`, peptide, protein, genome/metagenome, and complex fields before untyped content inference.
- Canonicalizes inline SMILES and InChI with RDKit using canonical isomeric SMILES output.
- Validates CAS syntax and check digit before lookup.
- Streams `science_store/indexes/compound_identifiers.tsv` with `BufReader` and enforces the exact five-column header.
- Normalizes names with Unicode lowercase and collapsed whitespace.
- Canonicalizes every mapped SMILES with RDKit and rejects invalid mapped structures.
- Returns explicit `invalid_cas_check_digit`, `unknown_compound_identifier`, and `invalid_mapped_smiles` failures.
- Exports `InputInspectionRow`, `InputInspection`, and `inspect_input_manifest`; inspection reuses the normalization converter and reports every converted or failed record identity.
- Updates legacy content-first assertions and sequence-file fixtures to the declared-field contract.

## GREEN evidence

| Command | Result |
|---|---|
| `cargo test -p gls-science input_conversion` | PASS, 6/6 |
| `cargo test -p gls-science compound_identifier` | PASS, 3/3 |
| `cargo test -p gls-science input_inspection` | PASS, 2/2 |
| `cargo test -p gls-science` | PASS, 93/93 |
| `python -m py_compile python\genoligand_workers\input_resolver.py` | PASS |
| `cargo fmt --all -- --check` | PASS |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | PASS for `gls-cli`, `gls-science`, and `gls-server` at 0.9.46 |
| `cargo test --workspace --all-features` | PASS, 167/167: CLI 3, science 93, server 71; doc-tests PASS |

The Rust molecular tests execute the installed GenoLigand Python worker runtime and exercise the changed Python resolver against real RDKit parsing for SMILES, InChI, SDF, MOL, and MOL2.

## Remaining operational dependency

Production name/CAS coverage depends on the Phase 2 producer populating `science_store/indexes/compound_identifiers.tsv` with the declared header and RDKit-valid mapped SMILES. Unknown identifiers fail explicitly until a matching local row exists.

## Review follow-up: input inspection contracts

### Baseline and scope

- Review baseline HEAD: `ad80c9eb873055b90d990913c543312d79dfd89d`
- Version: `0.9.46` -> `0.9.47`
- Scope: malformed manifest records, field-first file classification, genome nucleotide-or-protein conversion, unsupported file identity, and top-level manifest schema validation.
- Python resolver status: `python/genoligand_workers/input_resolver.py` did not require a review change; its bytecode compilation was still verified locally.

### RED evidence

The review tests were added before implementation and run individually against the review baseline behavior:

| Command | RED result |
|---|---|
| `cargo test -p gls-science input_inspection_reports_each_malformed_non_project_input_value` | Failed: one valid SMILES made inspection usable because numeric, missing, and empty non-project values were omitted. |
| `cargo test -p gls-science input_inspection_reports_each_malformed_non_project_file_value` | Failed: malformed and empty non-project file entries were omitted, leaving two rows instead of four. |
| `cargo test -p gls-science declared_fields_control_coordinate_and_molecule_file_compatibility` | Failed: a PDB declared as peptide was classified as `ProteinLigand`; molecular and coordinate format handling preceded the declared field. |
| `cargo test -p gls-science genome_field_accepts_nucleotide_or_protein_content_as_genome_family` | Failed: an inline standard amino-acid sequence under `genome_or_metagenome` had no canonical family. |
| `cargo test -p gls-science input_inspection_rejects_unsupported_file_paths_without_inline_fallback` | Failed: unknown-extension and extensionless `files[]` values fell through as inline text and lost file source identity. |
| `cargo test -p gls-science input_manifest_rejects_non_array_input_and_file_sections` | Failed: a present non-array `inputs` section returned an empty inspection instead of a schema error. |

### Implementation

- Retains every non-project `inputs[]` and `files[]` item with stable `input_id` and `record_1`, including invalid value types, missing values, and empty values.
- Emits per-record `invalid_input_value_type`, `missing_input_value`, and `empty_input_value` failures while preserving inline or file provenance.
- Rejects present non-array top-level sections with `invalid_input_manifest_schema:inputs_must_be_array` or `invalid_input_manifest_schema:files_must_be_array`; missing sections remain empty.
- Dispatches declared scientific fields before file-format inference. PDB files retain declared peptide, protein, or complex families, while incompatible field/format pairs fail explicitly.
- Restricts declared small molecules to RDKit molecular structure formats and retains actual-format inference for untyped files.
- Converts declared genome/metagenome content as nucleotide first and standard amino acid second, preserving the `GenomeOrMetagenome` canonical family for inline and `.faa` protein sequences.
- Rejects unknown-extension and extensionless `files[]` values as `unsupported_input_file_format` before inline inference, while known missing files continue to report `input_file_not_found`.
- Preserves Task 2 source path, record identity, file SHA-256, structure metadata, and multi-record parsing through the existing RDKit/OpenMM-backed resolution paths.

### GREEN evidence

| Command | Result |
|---|---|
| `cargo test -p gls-science input_inspection` | PASS, 5/5 |
| `cargo test -p gls-science input_manifest_rejects_non_array_input_and_file_sections` | PASS, 1/1 |
| `cargo test -p gls-science declared_fields_control_coordinate_and_molecule_file_compatibility` | PASS, 1/1 |
| `cargo test -p gls-science genome_field_accepts_nucleotide_or_protein_content_as_genome_family` | PASS, 1/1 |
| `cargo test -p gls-science` | PASS, 99/99; doc-tests PASS |
| `python -m py_compile python\genoligand_workers\input_resolver.py` | PASS |
| `cargo fmt --all -- --check` | PASS |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | PASS for all workspace targets and features at 0.9.47 |
| `cargo test --workspace --all-features` | PASS, 173/173: CLI 3, science 99, server 71; doc-tests PASS |

## Second review follow-up: ambiguous genome sequences

### Baseline and scope

- Review baseline HEAD: `7362de731e3f5f1f016a4068a5caeba6a41fcfed`
- Version: `0.9.47` -> `0.9.48`
- Scope: short sequences that belong to both the nucleotide alphabet and the standard amino-acid alphabet under `genome_or_metagenome`.
- Python resolver status: `python/genoligand_workers/input_resolver.py` did not require a second-review change; local bytecode compilation passed.

### RED evidence

| Command | RED result |
|---|---|
| `cargo test -p gls-science genome_field_falls_back_to_amino_acids_for_short_nucleotide_alphabet_sequences` | Failed with exit code 101: inline `ACGTAC` produced no canonical kind instead of `GenomeOrMetagenome`; the nucleotide-alphabet branch returned failure before amino-acid fallback. |

The regression test covers inline identity `record_1` and FASTA identity `ambiguous` from an `.faa` record. The existing `ACGTACGT` assertion continues to require `nucleotide_sequence` for a nucleotide-alphabet sequence with length at least eight.

### Implementation

- Applies nucleotide conversion when `len >= 8` and every symbol is nucleotide-compatible.
- Continues to standard amino-acid conversion when the nucleotide success condition is not met.
- Preserves `GenomeOrMetagenome` as the canonical family and `standard_amino_acid_sequence` as the method for inline and `.faa` `ACGTAC` records.

### GREEN evidence

| Command | Result |
|---|---|
| `cargo test -p gls-science genome_field_falls_back_to_amino_acids_for_short_nucleotide_alphabet_sequences` | PASS, 1/1 |
| `cargo test -p gls-science` | PASS, 100/100; doc-tests PASS |
| `cargo fmt --all -- --check` | PASS |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | PASS for all workspace targets and features at 0.9.48 |
| `cargo test --workspace --all-features` | PASS, 174/174: CLI 3, science 100, server 71; doc-tests PASS |
| `python -m py_compile python\genoligand_workers\input_resolver.py` | PASS |
| `git diff --check` | PASS |
