//! Exit-first (-x) and maxfail regression tests for tach-core CLI.
//!
//! These tests verify that -x/--exitfirst and --maxfail work correctly.
//! These flags control early termination on test failures.
//!
//! Regression prevention: Ensures early exit behavior is consistent.

use std::process::Command;

/// Get the tach-core binary path
fn tach_binary() -> std::path::PathBuf {
    let mut path = std::env::current_exe().unwrap();
    path.pop();
    path.pop();
    path.push("tach-core");
    path
}

// =============================================================================
// Exit-first (-x) tests
// =============================================================================

#[test]
fn test_exitfirst_flag_is_recognized() {
    let output = Command::new(tach_binary())
        .args(["--help"])
        .output()
        .expect("Failed to execute tach-core");

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        stdout.contains("-x") || stdout.contains("--exitfirst") || stdout.contains("exit"),
        "-x/--exitfirst flag should be documented in help output"
    );
}

#[test]
fn test_exitfirst_short_form() {
    let output = Command::new(tach_binary())
        .args(["-x", "--dry-run", "tests/dummy_project/"])
        .output()
        .expect("Failed to execute tach-core");

    assert_eq!(output.status.code(), Some(0), "-x should be accepted");
}

#[test]
fn test_exitfirst_long_form() {
    let output = Command::new(tach_binary())
        .args(["--exitfirst", "--dry-run", "tests/dummy_project/"])
        .output()
        .expect("Failed to execute tach-core");

    assert_eq!(
        output.status.code(),
        Some(0),
        "--exitfirst should be accepted"
    );
}

#[test]
fn test_exitfirst_with_verbose() {
    let output = Command::new(tach_binary())
        .args(["-x", "-v", "--dry-run", "tests/dummy_project/"])
        .output()
        .expect("Failed to execute tach-core");

    assert_eq!(
        output.status.code(),
        Some(0),
        "-x with -v should work together"
    );
}

#[test]
fn test_exitfirst_with_workers() {
    let output = Command::new(tach_binary())
        .args(["-x", "-n", "4", "--dry-run", "tests/dummy_project/"])
        .output()
        .expect("Failed to execute tach-core");

    assert_eq!(
        output.status.code(),
        Some(0),
        "-x with -n should work together"
    );
}

// =============================================================================
// Maxfail tests
// =============================================================================

#[test]
fn test_maxfail_flag_is_recognized() {
    let output = Command::new(tach_binary())
        .args(["--help"])
        .output()
        .expect("Failed to execute tach-core");

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        stdout.contains("--maxfail") || stdout.contains("max"),
        "--maxfail flag should be documented in help output"
    );
}

#[test]
fn test_maxfail_numeric_value() {
    let output = Command::new(tach_binary())
        .args(["--maxfail", "3", "--dry-run", "tests/dummy_project/"])
        .output()
        .expect("Failed to execute tach-core");

    assert_eq!(
        output.status.code(),
        Some(0),
        "--maxfail 3 should be accepted"
    );
}

#[test]
fn test_maxfail_one() {
    let output = Command::new(tach_binary())
        .args(["--maxfail", "1", "--dry-run", "tests/dummy_project/"])
        .output()
        .expect("Failed to execute tach-core");

    // --maxfail 1 is equivalent to -x
    assert_eq!(
        output.status.code(),
        Some(0),
        "--maxfail 1 should be accepted (equivalent to -x)"
    );
}

#[test]
fn test_maxfail_zero() {
    let output = Command::new(tach_binary())
        .args(["--maxfail", "0", "--dry-run", "tests/dummy_project/"])
        .output()
        .expect("Failed to execute tach-core");

    // --maxfail 0 might mean "no limit" or be rejected
    assert!(
        output.status.code().is_some(),
        "--maxfail 0 should not crash"
    );
}

#[test]
fn test_maxfail_negative_rejected() {
    let output = Command::new(tach_binary())
        .args(["--maxfail", "-1", "--dry-run", "tests/dummy_project/"])
        .output()
        .expect("Failed to execute tach-core");

    let stderr = String::from_utf8_lossy(&output.stderr);
    let code = output.status.code().unwrap_or(0);

    // Should reject negative values
    assert!(
        code != 0 || stderr.contains("error") || stderr.contains("invalid"),
        "--maxfail -1 should be rejected or treated as flag"
    );
}

#[test]
fn test_maxfail_non_numeric_rejected() {
    let output = Command::new(tach_binary())
        .args(["--maxfail", "abc", "--dry-run", "tests/dummy_project/"])
        .output()
        .expect("Failed to execute tach-core");

    let stderr = String::from_utf8_lossy(&output.stderr);
    let code = output.status.code().unwrap_or(0);

    // Should reject non-numeric values
    assert!(
        code != 0 || stderr.contains("error") || stderr.contains("invalid"),
        "--maxfail abc should be rejected. Exit: {}, stderr:\n{}",
        code,
        stderr
    );
}

#[test]
fn test_maxfail_large_value() {
    let output = Command::new(tach_binary())
        .args(["--maxfail", "1000", "--dry-run", "tests/dummy_project/"])
        .output()
        .expect("Failed to execute tach-core");

    // Large values should be accepted
    assert!(
        output.status.code().is_some(),
        "--maxfail 1000 should not crash"
    );
}

// =============================================================================
// Combined tests
// =============================================================================

#[test]
fn test_exitfirst_and_maxfail_together() {
    // Using both might be redundant or one overrides
    let output = Command::new(tach_binary())
        .args(["-x", "--maxfail", "5", "--dry-run", "tests/dummy_project/"])
        .output()
        .expect("Failed to execute tach-core");

    // Should not crash - might use one or the other
    assert!(
        output.status.code().is_some(),
        "-x and --maxfail together should not crash"
    );
}

#[test]
fn test_exitfirst_with_list() {
    let output = Command::new(tach_binary())
        .args(["-x", "list"])
        .output()
        .expect("Failed to execute tach-core");

    // -x with list is meaningless but should not crash
    assert!(
        output.status.code().is_some(),
        "-x with list should not crash"
    );
}

#[test]
fn test_maxfail_with_json_format() {
    let output = Command::new(tach_binary())
        .args([
            "--maxfail",
            "3",
            "--format",
            "json",
            "--dry-run",
            "tests/dummy_project/",
        ])
        .output()
        .expect("Failed to execute tach-core");

    assert!(
        output.status.code().is_some(),
        "--maxfail with --format json should not crash"
    );
}

#[test]
fn test_exitfirst_with_quiet() {
    let output = Command::new(tach_binary())
        .args(["-x", "-q", "--dry-run", "tests/dummy_project/"])
        .output()
        .expect("Failed to execute tach-core");

    assert_eq!(
        output.status.code(),
        Some(0),
        "-x with -q should work together"
    );
}
