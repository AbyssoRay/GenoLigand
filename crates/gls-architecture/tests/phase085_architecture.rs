use gls_architecture::{ArchitectureCommand, ArchitectureOptions, run_architecture_command};
use std::fs;

#[test]
fn phase085_architecture_audit_records_rust_ownership() {
    let root = std::env::temp_dir().join("gls_phase085_architecture_test");
    let _ = fs::remove_dir_all(&root);
    let workspace = std::env::current_dir().unwrap();
    run_architecture_command(
        ArchitectureCommand::AuditMdOwnership,
        &ArchitectureOptions {
            artifact_dir: root.clone(),
            workspace_root: workspace,
        },
    )
    .unwrap();
    let audit =
        fs::read_to_string(root.join("architecture/md_architecture_ownership_audit.tsv")).unwrap();
    assert!(audit.contains("master_verdict\trust"));
    assert!(audit.contains("validation_gate"));
    let _ = fs::remove_dir_all(&root);
}
