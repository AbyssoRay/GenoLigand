use gls_md::{MdCommand, MdOptions, run_md_command};
use std::fs;

#[test]
fn phase085_md_generates_three_complete_examples() {
    let root = std::env::temp_dir().join("gls_phase085_md_test");
    let _ = fs::remove_dir_all(&root);
    run_md_command(
        MdCommand::Report,
        &MdOptions {
            artifact_dir: root.clone(),
            workspace_root: std::env::current_dir().unwrap(),
        },
    )
    .unwrap();
    let summary = fs::read_to_string(root.join("e2e/e2e_examples_summary.tsv")).unwrap();
    assert_eq!(summary.lines().skip(1).count(), 3);
    assert!(summary.contains("100%\tPASS"));
    let _ = fs::remove_dir_all(&root);
}
