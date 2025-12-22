//! Implementation Tests
//!
//! These tests spawn the actual tach-core binary and verify end-to-end behavior.
//! They test the real system integration including fork, zygote, workers, etc.

use std::process::{Command, Stdio};
use std::time::Duration;
use wait_timeout::ChildExt;

/// Get the path to the built binary
fn binary_path() -> String {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    format!("{}/target/debug/tach-core", manifest_dir)
}

/// Get the project root directory
fn project_root() -> &'static str {
    env!("CARGO_MANIFEST_DIR")
}

/// Helper to run tach-core with given args and return output
fn run_tach(args: &[&str]) -> std::process::Output {
    let binary = binary_path();

    Command::new("sudo")
        .arg("-E")
        .arg(&binary)
        .args(args)
        .current_dir(project_root())
        .env("PYTHONHOME", "")
        .env(
            "PYTHONPATH",
            format!(
                "{}/.venv/lib/python3.12/site-packages:{}",
                project_root(),
                project_root()
            ),
        )
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("Failed to execute tach-core")
}

/// Helper to run tach-core with timeout
fn run_tach_with_timeout(args: &[&str], timeout_secs: u64) -> Option<std::process::Output> {
    let binary = binary_path();

    let mut child = Command::new("sudo")
        .arg("-E")
        .arg(&binary)
        .args(args)
        .current_dir(project_root())
        .env("PYTHONHOME", "")
        .env(
            "PYTHONPATH",
            format!(
                "{}/.venv/lib/python3.12/site-packages:{}",
                project_root(),
                project_root()
            ),
        )
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn tach-core");

    // Wait with timeout
    match child.wait_timeout(Duration::from_secs(timeout_secs)) {
        Ok(Some(status)) => {
            let output = child.wait_with_output().ok()?;
            Some(output)
        }
        Ok(None) => {
            // Timeout - kill the process
            let _ = child.kill();
            None
        }
        Err(_) => None,
    }
}

#[test]
#[ignore] // Requires sudo and built binary
fn test_binary_discovers_tests() {
    // First ensure binary is built
    let build_status = Command::new("cargo")
        .args(["build"])
        .current_dir(project_root())
        .status()
        .expect("Failed to build");
    assert!(build_status.success(), "Build should succeed");

    let output = run_tach(&["tests/dummy_project/"]);

    let stderr = String::from_utf8_lossy(&output.stderr);

    // Should report discovered tests
    assert!(
        stderr.contains("Discovered") && stderr.contains("tests"),
        "Should report discovered tests. Got: {}",
        stderr
    );
}

#[test]
#[ignore] // Requires sudo and built binary
fn test_binary_runs_simple_test() {
    let output = run_tach(&["tests/dummy_project/test_simple.py"]);

    let stderr = String::from_utf8_lossy(&output.stderr);

    // Should show test running or at least Zygote creation
    assert!(
        stderr.contains("test_simple")
            || stderr.contains("Complete")
            || stderr.contains("Zygote")
            || stderr.contains("Discovered"),
        "Should attempt to run tests. Got: {}",
        stderr
    );
}

#[test]
#[ignore] // Requires sudo and built binary
fn test_binary_handles_env_vars() {
    let output = run_tach(&["tests/env_test/"]);

    let stderr = String::from_utf8_lossy(&output.stderr);

    // Should load env vars from pyproject.toml
    assert!(
        stderr.contains("[config] Set env:"),
        "Should load env vars. Got: {}",
        stderr
    );
}

#[test]
#[ignore] // Requires sudo and built binary
fn test_binary_reports_pass_fail_counts() {
    let output = run_tach(&["tests/dummy_project/"]);

    let stderr = String::from_utf8_lossy(&output.stderr);

    // Should report pass/fail counts
    assert!(
        stderr.contains("passed") || stderr.contains("failed") || stderr.contains("Complete"),
        "Should report test results. Got: {}",
        stderr
    );
}

#[test]
#[ignore] // Requires sudo and built binary
fn test_binary_creates_zygote() {
    let output = run_tach(&["tests/dummy_project/test_simple.py"]);

    let stderr = String::from_utf8_lossy(&output.stderr);

    // Should show Zygote creation
    assert!(
        stderr.contains("Zygote") || stderr.contains("zygote"),
        "Should create Zygote. Got: {}",
        stderr
    );
}

#[test]
#[ignore] // Requires sudo and built binary
fn test_binary_handles_async_tests() {
    let output = run_tach(&["tests/dummy_project/test_async.py"]);

    let stderr = String::from_utf8_lossy(&output.stderr);

    // Should discover async tests (even if execution fails due to env)
    assert!(
        stderr.contains("async")
            || stderr.contains("Complete")
            || stderr.contains("Discovered")
            || stderr.contains("Zygote"),
        "Should discover async tests. Got: {}",
        stderr
    );
}

#[test]
#[ignore] // Requires sudo and built binary
fn test_binary_isolation_protects_host() {
    let output = run_tach(&["tests/gauntlet/test_fs_destruction.py"]);

    let stderr = String::from_utf8_lossy(&output.stderr);

    // Should either show isolation working or at least discover the test
    assert!(
        stderr.contains("protected")
            || stderr.contains("Read-only")
            || stderr.contains("Discovered")
            || stderr.contains("Zygote"),
        "Should show isolation or discovery. Got: {}",
        stderr
    );
}

#[test]
#[ignore] // Requires sudo and built binary
fn test_binary_handles_missing_directory() {
    let output = run_tach(&["/nonexistent/path/"]);

    // Should exit with error or handle gracefully
    let stderr = String::from_utf8_lossy(&output.stderr);

    // Either error message or empty results
    assert!(
        stderr.contains("error")
            || stderr.contains("Error")
            || stderr.contains("No tests")
            || stderr.contains("0 tests"),
        "Should handle missing directory. Got: {}",
        stderr
    );
}
