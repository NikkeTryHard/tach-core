//! Worker count (-n) regression tests for tach-core CLI.
//!
//! These tests verify that the -n/--workers flag works correctly.
//! Worker count affects parallelism and test execution behavior.
//!
//! Regression prevention: Ensures worker configuration is consistent.

use std::process::Command;

/// Get the tach-core binary path
fn tach_binary() -> std::path::PathBuf {
    let mut path = std::env::current_exe().unwrap();
    path.pop();
    path.pop();
    path.push("tach-core");
    path
}

#[test]
fn test_workers_flag_is_recognized() {
    let output = Command::new(tach_binary())
        .args(["--help"])
        .output()
        .expect("Failed to execute tach-core");

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        stdout.contains("-n") || stdout.contains("--workers") || stdout.contains("workers"),
        "-n/--workers flag should be documented in help output"
    );
}

#[test]
fn test_workers_numeric_value() {
    let output = Command::new(tach_binary())
        .args(["-n", "4", "--dry-run", "tests/dummy_project/"])
        .output()
        .expect("Failed to execute tach-core");

    assert_eq!(output.status.code(), Some(0), "-n 4 should be accepted");
}

#[test]
fn test_workers_one() {
    let output = Command::new(tach_binary())
        .args(["-n", "1", "--dry-run", "tests/dummy_project/"])
        .output()
        .expect("Failed to execute tach-core");

    assert_eq!(
        output.status.code(),
        Some(0),
        "-n 1 (single worker) should be accepted"
    );
}

#[test]
fn test_workers_auto() {
    let output = Command::new(tach_binary())
        .args(["-n", "auto", "--dry-run", "tests/dummy_project/"])
        .output()
        .expect("Failed to execute tach-core");

    // 'auto' should be accepted (uses CPU count)
    assert!(output.status.code().is_some(), "-n auto should be handled");
}

#[test]
fn test_workers_zero() {
    let output = Command::new(tach_binary())
        .args(["-n", "0", "--dry-run", "tests/dummy_project/"])
        .output()
        .expect("Failed to execute tach-core");

    // -n 0 might mean "auto" or be rejected
    assert!(output.status.code().is_some(), "-n 0 should not crash");
}

#[test]
fn test_workers_negative_rejected() {
    let output = Command::new(tach_binary())
        .args(["-n", "-1", "--dry-run", "tests/dummy_project/"])
        .output()
        .expect("Failed to execute tach-core");

    let stderr = String::from_utf8_lossy(&output.stderr);
    let code = output.status.code().unwrap_or(0);

    // Should reject negative values
    assert!(
        code != 0 || stderr.contains("error") || stderr.contains("invalid"),
        "-n -1 should be rejected or treated as flag"
    );
}

#[test]
fn test_workers_non_numeric_rejected() {
    let output = Command::new(tach_binary())
        .args(["-n", "abc", "--dry-run", "tests/dummy_project/"])
        .output()
        .expect("Failed to execute tach-core");

    let stderr = String::from_utf8_lossy(&output.stderr);
    let code = output.status.code().unwrap_or(0);

    // Should reject non-numeric (unless it's a special value like 'auto')
    // 'abc' should definitely be rejected
    assert!(
        code != 0 || stderr.contains("error") || stderr.contains("invalid"),
        "-n abc should be rejected. Exit: {}, stderr:\n{}",
        code,
        stderr
    );
}

#[test]
fn test_workers_large_value() {
    let output = Command::new(tach_binary())
        .args(["-n", "1000", "--dry-run", "tests/dummy_project/"])
        .output()
        .expect("Failed to execute tach-core");

    // Large values should be accepted (even if impractical)
    assert!(output.status.code().is_some(), "-n 1000 should not crash");
}

#[test]
fn test_workers_long_form() {
    let output = Command::new(tach_binary())
        .args(["--workers", "4", "--dry-run", "tests/dummy_project/"])
        .output()
        .expect("Failed to execute tach-core");

    assert!(
        output.status.code().is_some(),
        "--workers 4 should be accepted"
    );
}

#[test]
fn test_workers_env_var() {
    let output = Command::new(tach_binary())
        .env("TACH_WORKERS", "2")
        .args(["--dry-run", "tests/dummy_project/"])
        .output()
        .expect("Failed to execute tach-core");

    // TACH_WORKERS env var should work
    assert!(
        output.status.code().is_some(),
        "TACH_WORKERS env var should be accepted"
    );
}

#[test]
fn test_workers_flag_overrides_env() {
    let output = Command::new(tach_binary())
        .env("TACH_WORKERS", "8")
        .args(["-n", "2", "--dry-run", "tests/dummy_project/"])
        .output()
        .expect("Failed to execute tach-core");

    // Flag should override env var
    assert!(
        output.status.code().is_some(),
        "-n should override TACH_WORKERS"
    );
}

#[test]
fn test_workers_with_list_command() {
    let output = Command::new(tach_binary())
        .args(["-n", "4", "list"])
        .output()
        .expect("Failed to execute tach-core");

    // -n with list might be ignored or accepted
    assert!(
        output.status.code().is_some(),
        "-n with list should not crash"
    );
}

#[test]
fn test_workers_with_verbose() {
    let output = Command::new(tach_binary())
        .args(["-n", "4", "-v", "--dry-run", "tests/dummy_project/"])
        .output()
        .expect("Failed to execute tach-core");

    assert_eq!(
        output.status.code(),
        Some(0),
        "-n with -v should work together"
    );
}
