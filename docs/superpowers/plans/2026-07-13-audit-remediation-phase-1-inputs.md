# Audit Remediation Phase 1 Input Resolution Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace path/string heuristics with one authoritative, content-backed input resolver and make CLI, server, and WebUI obey it.

**Architecture:** `gls-science` owns input parsing and exposes an inspection API. Rust parses structured text and sequence files, an RDKit primitive resolver canonicalizes molecular text/files, and a local identifier index resolves names/CAS. `gls-server` calls this API before saving; WebUI only checks non-empty form state and renders server validation.

**Tech Stack:** Rust 2024, serde_json, sha2, Python 3 worker runtime, RDKit, vanilla JavaScript.

## Global Constraints

- Covers GL-AUDIT-001, GL-AUDIT-002, GL-AUDIT-028, the input half of GL-AUDIT-043, and CLI path handling in GL-AUDIT-044.
- All tests run locally; no GitHub Actions.
- File paths must exist and be readable before conversion succeeds.
- `canonical_value` contains parsed scientific content, never a path string.
- Explicit input fields constrain classification before sequence heuristics run.
- Python performs primitive RDKit parsing; Rust owns classification and acceptance decisions.
- Existing science commands remain source compatible for their callers.
- Phase 2 will build the production `compound_identifiers.tsv`; Phase 1 defines and tests its consumer contract.

---

### Task 1: Strict CLI and manifest parsing

**Files:**
- Modify: `crates/gls-cli/src/main.rs:17-140`
- Modify: `crates/gls-science/src/input_conversion.rs:42-57`
- Test: `crates/gls-cli/src/main.rs`
- Test: `crates/gls-science/src/tests.rs`

**Interfaces:**
- Consumes: `ScienceCommand` and command-line argument slices.
- Produces: `CommonArgs::parse(command, args)` with required science paths and `invalid_input_manifest_json` errors.

- [ ] **Step 1: Add failing CLI tests**

Append a `#[cfg(test)]` module to `crates/gls-cli/src/main.rs` with these cases:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn science_command_requires_input_manifest_and_run_dir() {
        let args = vec!["--science-store".into(), "store".into()];
        let error = CommonArgs::parse(ScienceCommand::InputNormalize, &args)
            .expect_err("science paths must be required");
        assert_eq!(error, "missing_required_option:--input-manifest");
    }

    #[test]
    fn path_options_reject_empty_values() {
        let args = vec!["--science-store".into(), "".into()];
        let error = CommonArgs::parse(ScienceCommand::PrepareScienceStore, &args)
            .expect_err("empty path must fail");
        assert_eq!(error, "empty_value_for:--science-store");
    }

    #[test]
    fn install_command_does_not_require_run_paths() {
        let args = vec!["--science-store".into(), "store".into()];
        let parsed = CommonArgs::parse(ScienceCommand::PrepareScienceStore, &args)
            .expect("install args");
        assert_eq!(parsed.science_store, PathBuf::from("store"));
    }
}
```

- [ ] **Step 2: Add a failing malformed JSON test**

Add to `crates/gls-science/src/tests.rs`:

```rust
#[test]
fn input_conversion_rejects_malformed_manifest_json() {
    let root = temp_root("malformed_input_manifest");
    let input = root.join("input.json");
    write_text(&input, "{not-json").expect("input");
    let error = extract_inputs(&input).expect_err("malformed JSON must fail");
    assert!(error.starts_with("invalid_input_manifest_json:"));
    fs::remove_dir_all(root).expect("cleanup");
}
```

- [ ] **Step 3: Run tests and confirm red state**

Run:

```text
cargo test -p gls-cli science_command_requires_input_manifest_and_run_dir
cargo test -p gls-science input_conversion_rejects_malformed_manifest_json
```

Expected: compile failure because `CommonArgs::parse` lacks the command argument, and malformed JSON test fails because parsing currently returns `Value::Null`.

- [ ] **Step 4: Implement strict parsing**

Change the call in `run_cli` to `CommonArgs::parse(command, &args[2..])`. Change the parser signature and post-parse requirements:

```rust
fn parse(command: ScienceCommand, args: &[String]) -> Result<Self, String> {
    let mut workspace_root = None;
    let mut science_store = None;
    let mut input_manifest = None;
    let mut run_dir = None;
    let mut index = 0usize;
    while index < args.len() {
        match args[index].as_str() {
            "--workspace-root" => {
                workspace_root = Some(take_path(args, &mut index, "--workspace-root")?)
            }
            "--science-store" => {
                science_store = Some(take_path(args, &mut index, "--science-store")?)
            }
            "--input-manifest" => {
                input_manifest = Some(take_path(args, &mut index, "--input-manifest")?)
            }
            "--run-dir" | "--out-dir" => {
                run_dir = Some(take_path(args, &mut index, "--run-dir")?)
            }
            other => return Err(format!("unknown_option:{other}")),
        }
        index += 1;
    }
    let workspace_root = workspace_root
        .unwrap_or(std::env::current_dir().map_err(|err| err.to_string())?);
    let science_store = science_store
        .or_else(|| std::env::var("GENOLIGAND_SCIENCE_STORE").ok().map(PathBuf::from))
        .unwrap_or_else(|| default_science_store(&workspace_root));
    let (input_manifest, run_dir) = if command == ScienceCommand::PrepareScienceStore {
        (PathBuf::new(), PathBuf::new())
    } else {
        (
            input_manifest.ok_or("missing_required_option:--input-manifest")?,
            run_dir.ok_or("missing_required_option:--run-dir")?,
        )
    };
    Ok(Self { workspace_root, science_store, input_manifest, run_dir })
}
```

Change `take_path` to reject whitespace-only values:

```rust
let value = args.get(*index).ok_or_else(|| format!("missing_value_for:{flag}"))?;
if value.trim().is_empty() {
    return Err(format!("empty_value_for:{flag}"));
}
Ok(PathBuf::from(value))
```

Replace the JSON fallback in `extract_raw_inputs` with:

```rust
let value = serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}'))
    .map_err(|err| format!("invalid_input_manifest_json:{err}"))?;
```

- [ ] **Step 5: Run focused and crate tests**

Run:

```text
cargo test -p gls-cli
cargo test -p gls-science input_conversion
```

Expected: all tests pass.

- [ ] **Step 6: Commit**

Increment the patch version, stage the four changed files, and commit:

```text
git commit -m "fix: require explicit science command paths [skip ci]"
```

### Task 2: Content-backed file resolution and provenance

**Files:**
- Create: `python/genoligand_workers/input_resolver.py`
- Modify: `crates/gls-science/Cargo.toml`
- Modify: `crates/gls-science/src/input_conversion.rs`
- Modify: `crates/gls-science/src/lib.rs:180-210,2579-2620`
- Test: `crates/gls-science/src/tests.rs`

**Interfaces:**
- Consumes: inline manifest entries and `files[].persisted_path` entries.
- Produces: `ResolvedInputRecord` rows and expanded conversion manifest columns `record_id`, `source_kind`, `source_path`, `content_sha256`.

- [ ] **Step 1: Add failing file-content tests**

Add tests that create a FASTA and nonexistent SDF path:

```rust
fn science_options(root: &Path, input: &Path, run: &Path) -> ScienceOptions {
    ScienceOptions {
        workspace_root: root.to_path_buf(),
        science_store: root.join("science_store"),
        input_manifest: input.to_path_buf(),
        run_dir: run.to_path_buf(),
    }
}

#[test]
fn file_input_reads_fasta_records_and_hashes_content() {
    let root = temp_root("fasta_content_resolution");
    let fasta = root.join("queries.fasta");
    write_text(&fasta, ">alpha\nMKTAYI\n>beta\nACDEFG\n").expect("fasta");
    let input = root.join("input.json");
    write_text(
        &input,
        &format!(
            r#"{{"inputs":[{{"field":"protein_ligand","value":{}}}],"files":[]}}"#,
            serde_json::to_string(&fasta.display().to_string()).expect("path JSON")
        ),
    ).expect("input");
    let run = root.join("run");
    let options = science_options(&root, &input, &run);
    run_science_command(ScienceCommand::InputNormalize, &options).expect("normalize");
    let rows = read_text(&run.join("input_conversion_manifest.tsv")).expect("rows");
    assert!(rows.contains("input_1\talpha\t"));
    assert!(rows.contains("input_1\tbeta\t"));
    assert!(rows.contains("\tMKTAYI\t"));
    assert!(!rows.contains(&fasta.display().to_string().replace('\\', "\\\\")));
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn nonexistent_declared_file_is_a_failed_conversion() {
    let root = temp_root("missing_declared_file");
    let input = root.join("input.json");
    write_text(
        &input,
        r#"{"inputs":[{"field":"small_molecule","value":"Z:\\missing\\ghost.sdf"}],"files":[]}"#,
    ).expect("input");
    let run = root.join("run");
    let options = science_options(&root, &input, &run);
    let error = run_science_command(ScienceCommand::InputNormalize, &options)
        .expect_err("missing file must fail normalization");
    assert!(error.contains("input_file_not_found"));
    fs::remove_dir_all(root).expect("cleanup");
}
```

Add these concrete SDF and MOL2 fixture tests:

```rust
#[test]
fn sdf_file_is_resolved_to_canonical_smiles() {
    let root = temp_root("sdf_content_resolution");
    let sdf = root.join("ethanol.sdf");
    write_text(
        &sdf,
        "ethanol\n  GenoLigand\n\n  3  2  0  0  0  0  0  0  0  0999 V2000\n    0.0000    0.0000    0.0000 C   0  0  0  0  0  0  0  0  0  0  0  0\n    1.5000    0.0000    0.0000 C   0  0  0  0  0  0  0  0  0  0  0  0\n    2.8000    0.0000    0.0000 O   0  0  0  0  0  0  0  0  0  0  0  0\n  1  2  1  0  0  0  0\n  2  3  1  0  0  0  0\nM  END\n$$$$\n",
    ).expect("sdf");
    let input = root.join("input.json");
    write_text(
        &input,
        &format!(
            r#"{{"inputs":[{{"field":"small_molecule","value":{}}}],"files":[]}}"#,
            serde_json::to_string(&sdf.display().to_string()).expect("path JSON")
        ),
    ).expect("input");
    let run = root.join("run");
    run_science_command(
        ScienceCommand::InputNormalize,
        &science_options(&root, &input, &run),
    ).expect("normalize");
    let rows = read_text(&run.join("input_conversion_manifest.tsv")).expect("rows");
    assert!(rows.contains("\tCCO\tconverted\t"));
    assert!(!rows.contains("ethanol.sdf\tconverted"));
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn mol2_file_is_resolved_to_canonical_smiles() {
    let root = temp_root("mol2_content_resolution");
    let mol2 = root.join("ethanol.mol2");
    write_text(
        &mol2,
        "@<TRIPOS>MOLECULE\nethanol\n 3 2 0 0 0\nSMALL\nNO_CHARGES\n\n@<TRIPOS>ATOM\n      1 C1          0.0000    0.0000    0.0000 C.3       1 ETH         0.0000\n      2 C2          1.5000    0.0000    0.0000 C.3       1 ETH         0.0000\n      3 O1          2.8000    0.0000    0.0000 O.3       1 ETH         0.0000\n@<TRIPOS>BOND\n     1    1    2 1\n     2    2    3 1\n",
    ).expect("mol2");
    let input = root.join("input.json");
    write_text(
        &input,
        &format!(
            r#"{{"inputs":[{{"field":"small_molecule","value":{}}}],"files":[]}}"#,
            serde_json::to_string(&mol2.display().to_string()).expect("path JSON")
        ),
    ).expect("input");
    let run = root.join("run");
    run_science_command(
        ScienceCommand::InputNormalize,
        &science_options(&root, &input, &run),
    ).expect("normalize");
    let rows = read_text(&run.join("input_conversion_manifest.tsv")).expect("rows");
    assert!(rows.contains("\tCCO\tconverted\t"));
    assert!(!rows.contains("ethanol.mol2\tconverted"));
    fs::remove_dir_all(root).expect("cleanup");
}
```

- [ ] **Step 2: Confirm red state**

Run:

```text
cargo test -p gls-science file_input_reads_fasta_records_and_hashes_content
cargo test -p gls-science nonexistent_declared_file_is_a_failed_conversion
```

Expected: current code produces one path-valued row and accepts the missing path.

- [ ] **Step 3: Add the primitive RDKit resolver**

Create `python/genoligand_workers/input_resolver.py` with a JSON-line response contract:

```python
import argparse
import json
from pathlib import Path
from rdkit import Chem


def molecule_from_path(path: Path, fmt: str):
    if fmt in {"sdf", "mol"}:
        supplier = Chem.SDMolSupplier(str(path), removeHs=False)
        return next((mol for mol in supplier if mol is not None), None)
    if fmt == "mol2":
        return Chem.MolFromMol2File(str(path), removeHs=False)
    raise ValueError(f"unsupported_molecule_format:{fmt}")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--path", required=True)
    parser.add_argument("--format", required=True)
    args = parser.parse_args()
    path = Path(args.path)
    mol = molecule_from_path(path, args.format.lower())
    if mol is None:
        raise ValueError("rdkit_structure_parse_failed")
    print(json.dumps({
        "canonical_smiles": Chem.MolToSmiles(mol, canonical=True, isomericSmiles=True),
        "atom_count": mol.GetNumAtoms(),
    }, ensure_ascii=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
```

- [ ] **Step 4: Implement Rust record resolution**

Add `sha2 = "0.10"` to `gls-science`. Define:

```rust
#[derive(Clone, Debug)]
struct ResolvedInputRecord {
    input_id: String,
    record_id: String,
    original_field: String,
    declared_value: String,
    source_kind: &'static str,
    source_path: String,
    content_sha256: String,
    format: String,
    content: String,
    file_count: usize,
}
```

Implement `resolve_raw_input(raw, options) -> Result<Vec<ResolvedInputRecord>, String>` with these exact branches:

- Existing FASTA/FA/FAA/FNA: read UTF-8, split at headers, emit one record per header.
- Existing FASTQ/FQ: emit one record per four-line FASTQ record and validate `@`, `+`, sequence/quality lengths.
- Existing GB/GBK: read the `ORIGIN` section and strip digits/whitespace.
- Existing SDF/MOL/MOL2: invoke `input_resolver.py` through the established worker Python candidate mechanism and parse `canonical_smiles`.
- Existing PDB/CIF/mmCIF/ENT: keep `source_path`, hash bytes, and set `content` to the coordinate text so downstream never tokenizes a path.
- GFF/GTF/VCF without a companion sequence: return `input_file_requires_reference_sequence`.
- Recognized extension with missing path: return `input_file_not_found:<path>`.
- Inline text: hash the UTF-8 bytes and emit one `record_1` row.

Add a `sha256_hex(bytes)` helper using `Sha256::digest`.

Extend `ConvertedInput` and `NormalizedInput` with record/provenance fields and write the new conversion TSV header:

```text
input_id	record_id	original_field	inferred_kind	canonical_kind	canonical_value	conversion_status	conversion_method	format	source_kind	source_path	content_sha256	reason	artifact	risk_flags
```

- [ ] **Step 5: Make normalization fail when any supplied record fails**

In `normalize_input`, collect conversion errors and return:

```rust
if let Some(failed) = conversions.iter().find(|row| row.conversion_status == "failed") {
    return Err(format!(
        "input_conversion_failed:{}:{}",
        failed.input_id,
        clean_cell(&failed.reason)
    ));
}
```

The manifest may still be written for diagnostics before returning.

- [ ] **Step 6: Run file and feature tests**

Run:

```text
cargo test -p gls-science file_input
cargo test -p gls-science feature_compute_emits_type_specific_features_for_all_supported_inputs
python -m py_compile python/genoligand_workers/input_resolver.py
```

Expected: all pass and path strings are absent from canonical feature rows.

- [ ] **Step 7: Commit**

Increment the patch version and commit:

```text
git commit -m "fix: resolve input files from scientific content [skip ci]"
```

### Task 3: Field-aware molecular classification and identifier lookup

**Files:**
- Modify: `crates/gls-science/src/input_conversion.rs`
- Modify: `crates/gls-science/src/lib.rs`
- Test: `crates/gls-science/src/tests.rs`

**Interfaces:**
- Consumes: `ResolvedInputRecord` and `science_store/indexes/compound_identifiers.tsv`.
- Produces: validated isomeric canonical SMILES and `InputInspection` summaries.

- [ ] **Step 1: Add failing classification and identifier tests**

Add:

```rust
fn normalize_fixture_inputs(
    entries: &[(&str, &str)],
    identifiers: &[(&str, &str, &str)],
) -> Vec<ConvertedInput> {
    let root = temp_root("input_identifier_fixture");
    let store = root.join("science_store");
    let mut lookup = String::from(
        "identifier_type\tidentifier\tcanonical_smiles\tdisplay_name\tsource\n",
    );
    for (kind, identifier, smiles) in identifiers {
        lookup.push_str(&format!("{kind}\t{identifier}\t{smiles}\t{identifier}\ttest\n"));
    }
    write_text(&store.join("indexes/compound_identifiers.tsv"), &lookup).expect("lookup");
    let input = root.join("input.json");
    let rows = entries
        .iter()
        .map(|(field, value)| serde_json::json!({"field": field, "value": value}))
        .collect::<Vec<_>>();
    write_text(
        &input,
        &(serde_json::json!({"inputs": rows, "files": []}).to_string() + "\n"),
    ).expect("input");
    let options = ScienceOptions {
        workspace_root: root.clone(),
        science_store: store,
        input_manifest: input,
        run_dir: root.join("run"),
    };
    let converted = convert_inputs_from_manifest(&options).expect("convert");
    fs::remove_dir_all(root).expect("cleanup");
    converted
}

#[test]
fn explicit_small_molecule_field_prevents_sequence_reclassification() {
    let rows = normalize_fixture_inputs(&[
        ("small_molecule", "CCCCCCCC"),
        ("small_molecule", "caffeine"),
    ], &[("name", "caffeine", "Cn1c(=O)n(C)c2ncn(C)c2c1=O")]);
    assert_eq!(rows[0].canonical_kind, Some(InputKind::SmallMolecule));
    assert_eq!(rows[0].canonical_value, "CCCCCCCC");
    assert_eq!(rows[1].canonical_kind, Some(InputKind::SmallMolecule));
    assert_eq!(rows[1].canonical_value, "Cn1c(=O)n(C)c2ncn(C)c2c1=O");
}

#[test]
fn cas_identifier_requires_valid_check_digit_and_local_mapping() {
    let rows = normalize_fixture_inputs(
        &[("small_molecule", "59-66-5")],
        &[("cas", "59-66-5", "CC(=O)NC1=NN=C(S1)S(N)(=O)=O")],
    );
    assert_eq!(rows[0].canonical_kind, Some(InputKind::SmallMolecule));
    assert_eq!(rows[0].conversion_method, "local_identifier_lookup");
}
```

Add a negative test for an unknown name and an invalid CAS check digit.

- [ ] **Step 2: Confirm red state**

Run:

```text
cargo test -p gls-science explicit_small_molecule_field_prevents_sequence_reclassification
cargo test -p gls-science cas_identifier_requires_valid_check_digit_and_local_mapping
```

Expected: `CCCCCCCC` becomes nucleotide, `caffeine` becomes peptide, and CAS is unresolved.

- [ ] **Step 3: Implement field-first classification**

Replace the current ordering with an explicit field dispatch:

```rust
match normalized_field(raw.original_field.as_str()) {
    "small_molecule" => convert_small_molecule(record, options),
    "peptide" => convert_peptide(record),
    "protein_ligand" => convert_protein(record),
    "genome_or_metagenome" => convert_nucleotide_or_protein(record),
    "complex" => convert_complex(record),
    _ => infer_untyped_record(record, options),
}
```

`convert_small_molecule` must try in this order: parsed molecular file result, InChI, RDKit SMILES canonicalization, valid CAS lookup, normalized name lookup. It must never call nucleotide or peptide classification.

- [ ] **Step 4: Implement the identifier index reader**

Define the Phase 2 producer contract:

```text
identifier_type	identifier	canonical_smiles	display_name	source
```

Implement a streaming lookup over `science_store/indexes/compound_identifiers.tsv`. Normalize names with Unicode lowercase and collapsed whitespace; normalize CAS as ASCII digits/hyphens and verify its check digit. Return `unknown_compound_identifier` when no row matches.

- [ ] **Step 5: Expose one authoritative inspection API**

Export from `gls-science`:

```rust
#[derive(Clone, Debug)]
pub struct InputInspectionRow {
    pub input_id: String,
    pub record_id: String,
    pub canonical_kind: String,
    pub conversion_method: String,
}

#[derive(Clone, Debug)]
pub struct InputInspection {
    pub usable: bool,
    pub summary: String,
    pub errors: Vec<String>,
    pub inputs: Vec<InputInspectionRow>,
}

pub fn inspect_input_manifest(options: &ScienceOptions) -> Result<InputInspection, String>;
```

The implementation runs the same resolver/classifier used by normalization and returns `usable=true` only when at least one non-project input exists and every supplied record converts.

- [ ] **Step 6: Run focused tests**

Run:

```text
cargo test -p gls-science input_conversion
cargo test -p gls-science compound_identifier
```

Expected: all pass.

- [ ] **Step 7: Commit**

Increment the patch version and commit:

```text
git commit -m "fix: classify inputs by declared scientific type [skip ci]"
```

### Task 4: Server-owned input validation

**Files:**
- Modify: `crates/gls-server/Cargo.toml`
- Delete: `crates/gls-server/src/input_recognition.rs`
- Modify: `crates/gls-server/src/lib.rs:1-15,141-177,355-376,630-675`
- Test: `crates/gls-server/src/tests.rs`

**Interfaces:**
- Consumes: `gls_science::inspect_input_manifest`.
- Produces: `POST /api/user/inputs/inspect` and save responses with structured validation.

- [ ] **Step 1: Add failing server tests**

Add tests for inspection and saving:

```rust
fn server_options_with_compound_lookup(root: &Path, data_row: &str) -> ServerOptions {
    let options = server_options(root.to_path_buf());
    let store = science_store_path(&options);
    fs::create_dir_all(store.join("indexes")).expect("indexes");
    fs::write(
        store.join("indexes/compound_identifiers.tsv"),
        format!(
            "identifier_type\tidentifier\tcanonical_smiles\tdisplay_name\tsource\n{data_row}\ttest\n"
        ),
    ).expect("lookup");
    options
}

#[test]
fn user_input_inspection_uses_science_resolver() {
    let root = temp_root("user_input_inspection");
    let options = server_options_with_compound_lookup(
        &root,
        "name\tcaffeine\tCn1c(=O)n(C)c2ncn(C)c2c1=O\tCaffeine",
    );
    let response = inspect_user_input_json(
        &options,
        r#"{"inputs":[{"field":"small_molecule","value":"caffeine"}],"files":[]}"#,
    );
    assert_eq!(response["usable"], true);
    assert_eq!(response["inputs"][0]["canonical_kind"], "small_molecule");
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn user_input_save_rejects_missing_structure_path() {
    let root = temp_root("missing_structure_save");
    let options = server_options(root.clone());
    fs::create_dir_all(project_dir(&options, "project-1")).expect("project");
    let response = write_input_manifest(
        &options,
        "project-1",
        r#"{"inputs":[{"field":"small_molecule","value":"Z:\\missing\\ghost.sdf"}],"files":[]}"#,
    );
    assert_eq!(response["status"], "failed");
    assert_eq!(response["error"], "input_file_not_found");
    fs::remove_dir_all(root).expect("cleanup");
}
```

- [ ] **Step 2: Confirm red state**

Run:

```text
cargo test -p gls-server user_input_inspection_uses_science_resolver
cargo test -p gls-server user_input_save_rejects_missing_structure_path
```

Expected: inspection endpoint/helper is absent and extension-only path is accepted.

- [ ] **Step 3: Depend on gls-science and remove duplicated heuristics**

Add:

```toml
gls-science = { path = "../gls-science" }
```

Remove the `input_recognition` module. Implement `inspect_user_input_json` by writing the request body to a uniquely named pending file under `artifact_dir/input-inspection/`, constructing `ScienceOptions`, calling `inspect_input_manifest`, and deleting the pending file in all outcomes.

Return:

```json
{
  "usable": true,
  "summary": "已识别为小分子",
  "errors": [],
  "inputs": [{"input_id":"input_1","canonical_kind":"small_molecule"}]
}
```

Route `POST /api/user/inputs/inspect` to this helper.

- [ ] **Step 4: Make project save atomic and authoritative**

In `write_input_manifest`, parse JSON with an explicit error, write `input_manifest.pending.json`, inspect that file, and rename it to `input_manifest.json` only when inspection is usable. Delete the pending file on all errors. Return the first structured resolver error code and user-facing message.

- [ ] **Step 5: Run server tests**

Run:

```text
cargo test -p gls-server input_validation
cargo test -p gls-server user_input
```

Expected: all pass and the removed duplicate module has no references.

- [ ] **Step 6: Commit**

Increment the patch version and commit:

```text
git commit -m "fix: make science resolver authoritative for input saves [skip ci]"
```

### Task 5: WebUI asynchronous validation and phase acceptance

**Files:**
- Modify: `webui/app/state/app-state.js:1-76`
- Modify: `webui/app/core/selectors.js:1-130`
- Modify: `webui/app/core/actions.js:1-47`
- Modify: `webui/app/pages/input.js:1-124`
- Modify: `tests/webui/check_static_webui_contracts.py`
- Test: `crates/gls-server/src/tests.rs`

**Interfaces:**
- Consumes: `POST /api/user/inputs/inspect`.
- Produces: server-backed per-form validation state and save enablement for names/CAS/existing files.

- [ ] **Step 1: Add failing static WebUI contract checks**

Extend `check_static_webui_contracts.py` to require:

```python
require('/api/user/inputs/inspect' in actions, "input validation must use server inspection")
require('looksLikeSmilesText' not in selectors, "client must not own SMILES acceptance")
require('looksLikeNucleotideSequence' not in selectors, "client must not override declared input type")
require('inputInspection' in app_state, "state must retain authoritative inspection")
```

- [ ] **Step 2: Confirm red state**

Run:

```text
python tests/webui/check_static_webui_contracts.py
```

Expected: new checks fail.

- [ ] **Step 3: Replace client scientific heuristics**

Add to state:

```javascript
inputInspection: null,
inputInspectionPending: false,
inputInspectionError: "",
```

Make `inputsReadyToSave()` require at least one non-empty field and `state.inputInspection?.usable === true`. Remove molecular/sequence scientific regexes from `selectors.js`; retain only `filledInputs()` and presentation helpers.

Add `inspectInputs()` in `actions.js`:

```javascript
function inputPayloadFromForm() {
  return {
    input_kind: "multi_input_workbench",
    inputs: [
      {
        field: "project_name",
        value: state.form.project_name.trim() || "未命名研究项目",
      },
      ...filledInputs().map((item) => ({ field: item.field, value: item.value })),
    ],
    files: [],
  };
}

async function inspectInputs() {
  const payload = inputPayloadFromForm();
  state.inputInspectionPending = true;
  state.inputInspectionError = "";
  render();
  try {
    state.inputInspection = await api("/api/user/inputs/inspect", {
      method: "POST",
      body: JSON.stringify(payload),
    });
  } catch (error) {
    state.inputInspection = null;
    state.inputInspectionError = error.message || "输入检查失败";
  } finally {
    state.inputInspectionPending = false;
    render();
  }
}
```

Use one shared `inputPayloadFromForm()` for inspect and save. Debounce form inspection by 300 ms and cancel the previous timer when the form changes.

- [ ] **Step 4: Render authoritative status**

The input page must display pending, usable, and the first server error. Names, CAS and paths are no longer disabled by client regexes. Save buttons remain disabled while inspection is pending or unusable.

- [ ] **Step 5: Run static, Rust, JS and focused browser checks**

Run:

```text
python tests/webui/check_static_webui_contracts.py
node --check webui/app/state/app-state.js
node --check webui/app/core/selectors.js
node --check webui/app/core/actions.js
node --check webui/app/pages/input.js
cargo test -p gls-server
```

Start the local server and verify in the browser:

- `CCCCCCCC` is accepted as a small molecule.
- fixture-backed `caffeine` and `59-66-5` are accepted.
- an existing SDF path is accepted.
- `Z:\missing\ghost.sdf` is rejected with a file-not-found message.

- [ ] **Step 6: Run Phase 1 regression suite**

Run:

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
python tests/architecture/check_repository_contracts.py
python tests/e2e/check_pipeline_contracts.py
python tests/webui/check_static_webui_contracts.py
```

Expected: all pass. Record GL-AUDIT-001, 002, 028 and CLI portion of 044 as fixed in the progress ledger; keep GL-AUDIT-043 open until Phase 3 database-side stereochemical matching is complete.

- [ ] **Step 7: Commit and push phase**

Increment the patch version and commit:

```text
git commit -m "fix: complete authoritative input resolution [skip ci]"
git push origin codex/fix-audit-44
```
