use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_root(label: &str) -> PathBuf {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "genoligand_cli_{label}_{}_{}",
        std::process::id(),
        suffix
    ))
}

fn fixture_builder() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("tests/e2e/prepare_cli_pipeline_fixture.py")
}

fn python_executable() -> String {
    for candidate in ["python", "python3"] {
        if Command::new(candidate)
            .arg("--version")
            .output()
            .is_ok_and(|output| output.status.success())
        {
            return candidate.to_string();
        }
    }
    panic!("python executable is required for CLI fixture setup");
}

fn prepare_fixture(root: &Path, mode: &str, python: &str) {
    let output = Command::new(python)
        .arg(fixture_builder())
        .args([
            "--root",
            root.to_str().expect("fixture root"),
            "--mode",
            mode,
        ])
        .output()
        .expect("run fixture builder");
    assert!(
        output.status.success(),
        "fixture setup failed: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn run_pipeline(root: &Path, python: &str) -> Output {
    let run = root.join("run");
    Command::new(env!("CARGO_BIN_EXE_gls-cli"))
        .env("GENOLIGAND_PYTHON", python)
        .args([
            "pipeline",
            "run",
            "--workspace-root",
            root.to_str().expect("workspace path"),
            "--science-store",
            root.join("science_store").to_str().expect("store path"),
            "--input-manifest",
            root.join("input_manifest.json")
                .to_str()
                .expect("input path"),
            "--run-dir",
            run.to_str().expect("run path"),
        ])
        .output()
        .expect("run gls-cli")
}

fn assert_no_done_marker(root: &Path) {
    assert!(!root.join("run/pipeline_done.txt").exists());
}

#[test]
fn pipeline_exit_code_and_terminal_state_matrix_uses_real_cli() {
    let python = python_executable();

    let completed_root = temp_root("completed");
    prepare_fixture(&completed_root, "ready", &python);
    let completed = run_pipeline(&completed_root, &python);
    assert_eq!(completed.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&completed.stdout).contains("pipeline_completed"));
    assert_eq!(
        fs::read_to_string(completed_root.join("run/run_state.txt")).expect("completed state"),
        "completed\n"
    );
    assert_no_done_marker(&completed_root);

    let blocked_root = temp_root("blocked");
    prepare_fixture(&blocked_root, "blocked", &python);
    let blocked = run_pipeline(&blocked_root, &python);
    assert_eq!(blocked.status.code(), Some(2));
    let blocked_stderr = String::from_utf8_lossy(&blocked.stderr);
    assert!(blocked_stderr.contains("pipeline_blocked"));
    assert!(blocked_stderr.contains("run_science_store_installer"));
    assert_eq!(
        fs::read_to_string(blocked_root.join("run/run_state.txt")).expect("blocked state"),
        "blocked\n"
    );
    let blocked_reason = fs::read_to_string(blocked_root.join("run/failure_reason.txt"))
        .expect("blocked failure reason");
    assert!(blocked_reason.contains("science_store_readiness_missing"));
    assert!(blocked_reason.contains("science_store_readiness.json is absent"));
    assert!(blocked_reason.contains("run_science_store_installer"));
    assert!(blocked_reason.contains("repair_action="));
    assert_no_done_marker(&blocked_root);

    let failed_root = temp_root("failed");
    prepare_fixture(&failed_root, "failed", &python);
    let failed = run_pipeline(&failed_root, &python);
    assert_eq!(failed.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&failed.stderr).contains("pipeline_failed:preflight"));
    assert_eq!(
        fs::read_to_string(failed_root.join("run/run_state.txt")).expect("failed state"),
        "failed\n"
    );
    let failed_reason = fs::read_to_string(failed_root.join("run/failure_reason.txt"))
        .expect("failed failure reason");
    assert!(failed_reason.contains("state=failed"));
    assert!(failed_reason.contains("stage=preflight"));
    assert!(failed_reason.contains("repair_action="));
    assert_no_done_marker(&failed_root);

    for root in [completed_root, blocked_root, failed_root] {
        fs::remove_dir_all(root).expect("cleanup temp root");
    }
}
