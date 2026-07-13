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
