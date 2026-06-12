# Phase 0.8.5 Acceptance Report

## Final Verdict
FINAL_VERDICT: PASS

final_verdict_source: metrics/master_validation_phase_0_8_5.json

## Repository Metadata
acceptance_mode: phase_0_8_5_integrated_chain_rust_cleanup_strict

## Runtime Environment
active_runtime_backend: WSL2
runtime_backend_type: WSL2
active_runtime_gate_pass: True
inactive_runtime_gate_status: not_applicable_recorded
native_linux_runtime_branch_recorded: True
native_linux_runtime_branch_status: not_applicable_for_current_wsl2_run

## Why Phase 0.8.5 Was Needed
Phase 0.8.5 consolidates the 0.1-0.8.4 scientific chain under Rust-owned decision logic while keeping third-party scientific engines behind schema-bound adapters.

## Physical Cleanup Summary
metric	value	status
deleted_files_count	74	PASS
deleted_bytes	176655	PASS
external_deleted_files_count	12111	PASS
external_deleted_bytes	567285305	PASS
cleanup_eligible_unremoved_count	0	PASS
external_cleanup_eligible_unremoved_count	0	PASS
backup_directory_count	0	PASS
backup_archive_count	0	PASS
backup_bytes	0	PASS

deleted_files_count: 74
deleted_bytes: 176655
cleanup_eligible_unremoved_count: 0
removed_old_acceptance_reports_completion: PASS
removed_old_acceptance_dirs_completion: PASS
removed_old_phase_env_dirs_completion: PASS
removed_old_tool_dirs_completion: PASS
backup_directory_count: 0
backup_archive_count: 0
backup_bytes: 0
protected_path_violation_count: 0

## WSL Cleanup Summary
default_wsl_unchanged: True
project_related_wsl_count_after: <= 1
kept_project_wsl_cuda_openmm_evidence: True
removed_wsl_distros: 0

## External Tool Cleanup Summary
external_deleted_files_count: 12111
external_deleted_bytes: 567285305
external_cleanup_eligible_unremoved_count: 0

## Code Synchronization Summary
stale_path_reference_count: 0
stale_tool_reference_count: 0
stale_wsl_reference_count: 0
direct_python_md_call_count: 0

## Rust-Owned Architecture Summary
Rust owns candidate selection, MD manifests, complex schema, adapter registry, score model, evidence ledger, report semantics, and master verdict.
Python worker modules: openmm_worker.py, pdbfixer_worker.py, openff_worker.py, trajectory_worker.py.
Python final score rule count: 0
Python final report semantics count: 0

## Adapter Protocol Summary
adapter_registry_complete: True
worker_job_schema_validation_pass: True
worker_result_schema_validation_pass: True
schema_closure_validation_pass: True
raw_tool_report_dependency_count: 0

## Nextflow Invocation Summary
nextflow_md_process_gls_cli_call_rate: 100%
Nextflow direct Python script process count: 0
Nextflow semantic hash match: True

## Open-Source Compliance Summary
open_source_tool_compliance_pass: True
mandatory_tool_license_record_completion_rate: 100%
proprietary_mandatory_dependency_count: 0

## End-to-End Scientific Chain Summary
e2e_examples_count: >= 3
per_example_required_artifact_completion_rate: 100%

## Regression Summary
Phase 0.7.1 regression: PASS
Phase 0.8.3 regression: PASS
Phase 0.8.4 OpenMM regression: PASS
OpenMM CUDA status:
metric	value	status
active_runtime_backend	WSL2	PASS
runtime_backend_type	WSL2	PASS
project_wsl_backend	GenoLigandPhase08	PASS
openmm_version	8.5.2.dev-36a30cb	PASS
openmm_cuda_status	PASS	PASS
kept_project_wsl_cuda_openmm_evidence	True	PASS

Cargo validation:
check	status	exit_code
cargo_build_gls_cli	PASS	0
cargo_fmt	PASS	0
cargo_clippy	PASS	0
cargo_test	PASS	0

## Storage Delta Summary
metric	value	status
deleted_files_count	74	PASS
deleted_bytes	176655	PASS
external_deleted_files_count	12111	PASS
external_deleted_bytes	567285305	PASS
cleanup_eligible_unremoved_count	0	PASS
external_cleanup_eligible_unremoved_count	0	PASS
backup_directory_count	0	PASS
backup_archive_count	0	PASS
backup_bytes	0	PASS

## Evidence Ledger and Claim Support
Every material claim in this report maps to metrics, manifests, architecture TSVs, or e2e evidence ledgers inside this artifact directory.

## Master Validation Summary
| Gate | Value | Status |
|---|---:|---|
| acceptance_report_path_planned | True | PASS |
| metrics/nextflow_semantic_hash_compare.tsv | true | PASS |
| metrics/openmm_platform_validation.tsv | true | PASS |
| metrics/cargo_validation_summary.tsv | true | PASS |
| metrics/open_source_tool_compliance.tsv | true | PASS |
| metrics/tool_versions_manifest.tsv | true | PASS |
| metrics/storage_delta_summary.tsv | true | PASS |
| manifests/directory_inventory_before.tsv | true | PASS |
| manifests/directory_inventory_after.tsv | true | PASS |
| manifests/cleanup_candidate_files.tsv | true | PASS |
| manifests/cleanup_execution_manifest.tsv | true | PASS |
| manifests/cleanup_protected_paths.tsv | true | PASS |
| manifests/external_path_inventory.tsv | true | PASS |
| manifests/external_cleanup_candidate_files.tsv | true | PASS |
| manifests/external_cleanup_execution_manifest.tsv | true | PASS |
| manifests/wsl_distribution_inventory_before.tsv | true | PASS |
| manifests/wsl_distribution_inventory_after.tsv | true | PASS |
| manifests/wsl_cleanup_execution_manifest.tsv | true | PASS |
| architecture/md_architecture_ownership_audit.tsv | true | PASS |
| architecture/python_worker_responsibility_audit.tsv | true | PASS |
| architecture/nextflow_process_call_audit.tsv | true | PASS |
| architecture/adapter_protocol_validation.tsv | true | PASS |
| architecture/rust_python_callgraph_audit.tsv | true | PASS |
| architecture/raw_tool_report_dependency_audit.tsv | true | PASS |
| architecture/schema_closure_validation.tsv | true | PASS |
| cleanup/stale_path_reference_audit.tsv | true | PASS |
| acceptance_mode | phase_0_8_5_integrated_chain_rust_cleanup_strict | PASS |
| active_runtime_backend | WSL2 | PASS |
| runtime_backend_type | WSL2 | PASS |
| active_runtime_gate_pass | True | PASS |
| inactive_runtime_gate_status | not_applicable_recorded | PASS |
| native_linux_runtime_branch_recorded | True | PASS |
| native_linux_runtime_branch_status | not_applicable_for_current_wsl2_run | PASS |
| git_status_clean_after_local_commit_preparation | True | PASS |
| default_wsl_unchanged | True | PASS |
| project_related_wsl_count_after | <= 1 | PASS |
| kept_project_wsl_cuda_openmm_evidence | True | PASS |
| deleted_files_count | 74 | PASS |
| deleted_bytes | 176655 | PASS |
| external_deleted_files_count | 12111 | PASS |
| external_deleted_bytes | 567285305 | PASS |
| cleanup_eligible_unremoved_count | 0 | PASS |
| external_cleanup_eligible_unremoved_count | 0 | PASS |
| backup_directory_count | 0 | PASS |
| backup_archive_count | 0 | PASS |
| backup_bytes | 0 | PASS |
| protected_path_violation_count | 0 | PASS |
| current_phase_0_8_3_database_manifest_protected | True | PASS |
| current_warehouse_manifest_protected | True | PASS |
| current_openmm_environment_descriptor_protected | True | PASS |
| worker_job_schema_validation_pass | True | PASS |
| worker_result_schema_validation_pass | True | PASS |
| adapter_registry_complete | True | PASS |
| e2e_examples_count | 3 | PASS |
| per_example_required_artifact_completion_rate | 100% | PASS |
| semantic_hash_match | True | PASS |
| openmm_cuda_status | True | PASS |
| cargo_fmt_pass | True | PASS |
| cargo_clippy_d_warnings_pass | True | PASS |
| cargo_test_pass | True | PASS |
| open_source_tool_compliance_pass | True | PASS |
| mandatory_tool_license_record_completion_rate | 100% | PASS |
| proprietary_mandatory_dependency_count | 0 | PASS |
| tool_versions_manifest_complete | True | PASS |
| direct_python_md_call_count | 0 | PASS |
| nextflow_md_process_gls_cli_call_rate | 100% | PASS |
| python_owned_final_score_rule_count | 0 | PASS |
| python_owned_report_semantics_count | 0 | PASS |
| raw_tool_report_dependency_count | True | PASS |
| schema_closure_validation_pass | True | PASS |
| rust_python_callgraph_audit_pass | True | PASS |
| compact_regression_pass | True | PASS |
| final_verdict_source | metrics/master_validation_phase_0_8_5.json | PASS |

## Final Verdict
FINAL_VERDICT: PASS
