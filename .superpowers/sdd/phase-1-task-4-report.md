# Phase 1 Task 4 Report

## Baseline and scope

- Branch: `codex/fix-audit-44`
- Baseline: `3a0159e954d46e54ae3477fda7de0e32914fc560`
- Version: `0.9.48` -> `0.9.49`
- Server scientific validation now calls `gls_science::inspect_input_manifest` directly.
- `crates/gls-server/src/input_recognition.rs` and all module references were removed.
- WebUI files were unchanged.

## RED evidence

| Command | Exit | Observed failure |
| --- | ---: | --- |
| `cargo test -p gls-server user_input_inspection` | 101 | `inspect_user_input_json` and the controlled promotion helper were absent. |
| `cargo test -p gls-server user_input_save` | 101 | The new inspection and promotion interfaces were absent. |
| `cargo test -p gls-server input_validation` | 101 | The new inspection and promotion interfaces were absent. |
| `cargo test --offline -p gls-server user_input_inspection_parses_error_after_colon_record_id` | 101 | Returned `chain` for FASTA record `sample:chain`; expected `invalid_peptide_sequence`. |
| `cargo test --offline -p gls-server user_input_save_reports_cleanup_failure_with_original_context` | 101 | Injectable save operations helper was absent. |
| `cargo test --offline -p gls-server user_input_save_does_not_cleanup_after_successful_promotion` | 101 | Cleanup ran after a successful rename and triggered the injected panic. |
| `cargo test --offline -p gls-server user_input_save_failed_promotion_preserves_formal_state` | 101 | Promotion failure discarded the successful science inspection context. |

## GREEN behavior

- `POST /api/user/inputs/inspect` writes the exact request body to a PID-plus-atomic-counter pending filename and removes it on every return path.
- Inspection responses preserve `input_id`, `record_id`, `canonical_kind`, and `conversion_method` and return structured resolver codes and messages.
- Error extraction uses the exact `input_id:sanitized_record_id:` prefix, including record IDs containing colons.
- Saves parse JSON explicitly, inspect `input_manifest.pending.json`, and publish with same-directory `std::fs::rename` after `usable=true`.
- Failed write, inspection, and promotion paths clean the pending file. Cleanup failures return `input_manifest_cleanup_failed` with the original structured cause.
- Successful promotion is the commit point; no cleanup operation runs after rename consumes the pending file.
- Failed saves preserve the prior formal manifest, project name, and latest run selection. Successful saves update the name and clear the run selection.
- Preflight, project visibility, and run input status use the science inspection result.

## Final verification

| Command | Exit | Result |
| --- | ---: | --- |
| `cargo test --offline -p gls-server user_input_inspection` | 0 | 8 passed |
| `cargo test --offline -p gls-server user_input_save` | 0 | 6 passed |
| `cargo test --offline -p gls-server input_validation` | 0 | 3 passed |
| `cargo test --offline -p gls-server` | 0 | 84 passed |
| `cargo fmt --all -- --check` | 0 | clean |
| `cargo clippy --offline --workspace --all-targets --all-features -- -D warnings` | 0 | clean |
| `cargo test --offline --workspace --all-features` | 0 | CLI 3, science 100, server 84 passed |
| `git diff --check` | 0 | clean |

## Dependency closure

- `gls-server` adds the workspace path dependency `gls-science = { path = "../gls-science" }`.
- No registry dependency was added. `atomic-file-install` is absent from manifests and `Cargo.lock`.
- Final Cargo verification ran with `--offline`; the lockfile changes contain workspace version updates and the direct `gls-science` dependency edge.

## Review follow-up (2026-07-13)

### RED evidence

| Command | Exit | Observed failure |
| --- | ---: | --- |
| `cargo test --offline -p gls-server user_input_inspection_parses_error_after_colon_record_id` | 101 | The shorter `input_1:sample:` prefix won and returned `chain` instead of `invalid_peptide_sequence`. |
| `cargo test --offline -p gls-server input_validation_rejects_malformed_save_without_state_changes` | 101 | A stale `input_manifest.pending.json` survived malformed JSON parsing. |
| `cargo test --offline -p gls-server input_validation_malformed_save_reports_cleanup_failure_with_json_cause` | 101 | Parsing returned `invalid_input_manifest_json` before attempting injected cleanup. |
| `cargo test --offline -p gls-server user_input_inspection_cleanup_failure_preserves_resolver_cause` | 101 | The injectable inspection cleanup helper was absent. |
| `cargo test --offline -p gls-server input_validation_has_no_server_owned_recognizer_module` | 101 | The E2E checker still referenced the deleted server recognizer source path. |

### GREEN evidence

| Command | Exit | Result |
| --- | ---: | --- |
| `cargo test --offline -p gls-server user_input_inspection_parses_error_after_colon_record_id` | 0 | Longest exact record prefix selected; 1 passed. |
| `cargo test --offline -p gls-server input_validation_rejects_malformed_save_without_state_changes` | 0 | Stale pending file removed while formal manifest, project name, and run selection remained unchanged. |
| `cargo test --offline -p gls-server input_validation_malformed_save_reports_cleanup_failure_with_json_cause` | 0 | Cleanup error became top-level and retained the invalid JSON cause. |
| `cargo test --offline -p gls-server user_input_inspection_cleanup_failure_preserves_resolver_cause` | 0 | Cleanup error retained `input_file_not_found` in the structured cause and errors. |
| `cargo test --offline -p gls-server user_input_inspection` | 0 | 9 passed. |
| `cargo test --offline -p gls-server input_validation_has_no_server_owned_recognizer_module` | 0 | 1 passed; the checker no longer references the removed path. |
| `python tests/e2e/check_pipeline_contracts.py` | 0 | `pipeline_contracts PASS`. |
| `python -m py_compile tests/e2e/check_pipeline_contracts.py` | 0 | Compile check passed. |
| `cargo fmt --all -- --check` | 1 then 0 | Rustfmt identified three assertion-layout differences; `cargo fmt --all` corrected them and the final check passed. |
| `git diff --check` | 0 | Clean. |

The review publishes workspace version `0.9.50` in `Cargo.toml` and all three workspace package entries in `Cargo.lock`. It adds no registry dependency. The continuation used the completed focused Rust results above and retained the full Task4 suite evidence in the preceding Final verification section.
