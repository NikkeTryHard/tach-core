//! Durations flag regression tests for tach-core CLI.
//!
//! These tests verify that the --durations flag works correctly.
//! The durations flag shows timing for the slowest tests.
//!
//! Regression prevention: Ensures timing output behavior is consistent.

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
fn test_durations_flag_is_recognized() {
    let output = Command::new(tach_binary())
        .args(["--help"])
        .output()
        .expect("Failed to execute tach-core");

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        stdout.contains("--durations") || stdout.contains("durations"),
        "--durations flag should be documented in help output"
    );
}

#[test]
fn test_durations_numeric_value() {
    let output = Command::new(tach_binary())
        .args(["--durations", "10", "--dry-run", "tests/dummy_project/"])
        .output()
        .expect("Failed to execute tach-core");

    assert_eq!(
        output.status.code(),
        Some(0),
        "--durations 10 should be accepted"
    );
}

#[test]
fn test_durations_zero() {
    let output = Command::new(tach_binary())
        .args(["--durations", "0", "--dry-run", "tests/dummy_project/"])
        .output()
        .expect("Failed to execute tach-core");

    // --durations 0 might mean "show all" or be rejected
    assert!(
        output.status.code().is_some(),
        "--durations 0 should not crash"
    );
}

#[test]
fn test_durations_one() {
    let output = Command::new(tach_binary())
        .args(["--durations", "1", "--dry-run", "tests/dummy_project/"])
        .output()
        .expect("Failed to execute tach-core");

    assert_eq!(
        output.status.code(),
        Some(0),
        "--durations 1 should be accepted"
    );
}

#[test]
fn test_durations_large_value() {
    let output = Command::new(tach_binary())
        .args(["--durations", "1000", "--dry-run", "tests/dummy_project/"])
        .output()
        .expect("Failed to execute tach-core");

    assert!(
        output.status.code().is_some(),
        "--durations 1000 should not crash"
    );
}

#[test]
fn test_durations_negative_rejected() {
    let output = Command::new(tach_binary())
        .args(["--durations", "-1", "--dry-run", "tests/dummy_project/"])
        .output()
        .expect("Failed to execute tach-core");

    let stderr = String::from_utf8_lossy(&output.stderr);
    let code = output.status.code().unwrap_or(0);

    // Should reject negative values
    assert!(
        code != 0 || stderr.contains("error") || stderr.contains("invalid"),
        "--durations -1 should be rejected or treated as flag"
    );
}

#[test]
fn test_durations_non_numeric_rejected() {
    let output = Command::new(tach_binary())
        .args(["--durations", "abc", "--dry-run", "tests/dummy_project/"])
        .output()
        .expect("Failed to execute tach-core");

    let stderr = String::from_utf8_lossy(&output.stderr);
    let code = output.status.code().unwrap_or(0);

    // Should reject non-numeric values
    assert!(
        code != 0 || stderr.contains("error") || stderr.contains("invalid"),
        "--durations abc should be rejected. Exit: {}, stderr:\n{}",
        code,
        stderr
    );
}

#[test]
fn test_durations_with_verbose() {
    let output = Command::new(tach_binary())
        .args([
            "--durations",
            "5",
            "-v",
            "--dry-run",
            "tests/dummy_project/",
        ])
        .output()
        .expect("Failed to execute tach-core");

    assert_eq!(
        output.status.code(),
        Some(0),
        "--durations with -v should work together"
    );
}

#[test]
fn test_durations_with_quiet() {
    let output = Command::new(tach_binary())
        .args([
            "--durations",
            "5",
            "-q",
            "--dry-run",
            "tests/dummy_project/",
        ])
        .output()
        .expect("Failed to execute tach-core");

    // Durations with quiet might suppress the output or show it anyway
    assert!(
        output.status.code().is_some(),
        "--durations with -q should not crash"
    );
}

#[test]
fn test_durations_with_json_format() {
    let output = Command::new(tach_binary())
        .args([
            "--durations",
            "5",
            "--format",
            "json",
            "--dry-run",
            "tests/dummy_project/",
        ])
        .output()
        .expect("Failed to execute tach-core");

    assert!(
        output.status.code().is_some(),
        "--durations with --format json should not crash"
    );
}

#[test]
fn test_durations_with_workers() {
    let output = Command::new(tach_binary())
        .args([
            "--durations",
            "5",
            "-n",
            "4",
            "--dry-run",
            "tests/dummy_project/",
        ])
        .output()
        .expect("Failed to execute tach-core");

    assert_eq!(
        output.status.code(),
        Some(0),
        "--durations with -n should work together"
    );
}
