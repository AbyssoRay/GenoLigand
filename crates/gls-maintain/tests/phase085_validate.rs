use gls_maintain::{MaintainOptions, validate_phase_0_8_5};
use std::fs;

#[test]
fn phase085_validator_writes_master_json_even_for_incomplete_artifact() {
    let root = std::env::temp_dir().join("gls_phase085_validate_test");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let workspace = std::env::current_dir().unwrap();
    let result = validate_phase_0_8_5(&MaintainOptions {
        artifact_dir: root.clone(),
        workspace_root: workspace,
        external_tools_root: std::path::PathBuf::from("D:\\user\\codex\\tools"),
        when: "before".to_string(),
    });
    assert!(result.is_ok());
    assert!(
        root.join("metrics/master_validation_phase_0_8_5.json")
            .exists()
    );
    let _ = fs::remove_dir_all(&root);
}
