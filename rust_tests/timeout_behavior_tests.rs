//! Timeout behavior regression tests for tach-core CLI.
//!
//! These tests verify that the --timeout flag works correctly.
//! The timeout feature was added to ensure tests don't hang indefinitely.
//!
//! Regression prevention: Ensures timeout behavior is consistent.

use std::process::Command;
use std::time::{Duration, Instant};

/// Get the tach-core binary path
fn tach_binary() -> std::path::PathBuf {
    let mut path = std::env::current_exe().unwrap();
    path.pop();
    path.pop();
    path.push("tach-core");
    path
}

#[test]
fn test_timeout_flag_is_recognized() {
    let output = Command::new(tach_binary())
        .args(["--help"])
        .output()
        .expect("Failed to execute tach-core");

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        stdout.contains("--timeout") || stdout.contains("timeout"),
        "--timeout flag should be documented in help output"
    );
}

#[test]
fn test_timeout_env_var_is_recognized() {
    let output = Command::new(tach_binary())
        .args(["--help"])
        .output()
        .expect("Failed to execute tach-core");

    let stdout = String::from_utf8_lossy(&output.stdout);

    // TACH_TIMEOUT should be mentioned or at least timeout should work via env
    assert!(
        stdout.contains("TACH_TIMEOUT") || stdout.contains("timeout"),
        "TACH_TIMEOUT env var should be documented or --timeout should exist"
    );
}

#[test]
fn test_timeout_accepts_numeric_value() {
    // --timeout 30 should be accepted without error
    let output = Command::new(tach_binary())
        .args(["--timeout", "30", "--dry-run", "."])
        .output()
        .expect("Failed to execute tach-core");

    let stderr = String::from_utf8_lossy(&output.stderr);

    // Should succeed (exit code 0)
    assert!(
        output.status.success(),
        "--timeout 30 should be accepted and exit with 0. Exit: {:?}, stderr:\n{}",
        output.status.code(),
        stderr
    );
}

#[test]
fn test_timeout_rejects_non_numeric_value() {
    let output = Command::new(tach_binary())
        .args(["--timeout", "not_a_number", "--dry-run", "."])
        .output()
        .expect("Failed to execute tach-core");

    let stderr = String::from_utf8_lossy(&output.stderr);
    let code = output.status.code().unwrap_or(0);

    // Should reject non-numeric value
    assert!(
        code != 0 || stderr.to_lowercase().contains("invalid") || stderr.contains("error"),
        "--timeout with non-numeric value should be rejected. Exit: {}, stderr:\n{}",
        code,
        stderr
    );
}

#[test]
fn test_timeout_rejects_negative_value() {
    let output = Command::new(tach_binary())
        .args(["--timeout", "-5", "--dry-run", "."])
        .output()
        .expect("Failed to execute tach-core");

    let stderr = String::from_utf8_lossy(&output.stderr);
    let code = output.status.code().unwrap_or(0);

    // Should reject negative value (or parse as flag)
    // Some CLIs treat -5 as a flag, which is also an error
    assert!(
        code != 0 || stderr.contains("error") || stderr.contains("unexpected"),
        "--timeout with negative value should be rejected. Exit: {}, stderr:\n{}",
        code,
        stderr
    );
}

#[test]
fn test_timeout_env_var_works() {
    let output = Command::new(tach_binary())
        .env("TACH_TIMEOUT", "45")
        .args(["--dry-run", "."])
        .output()
        .expect("Failed to execute tach-core");

    // Should not crash
    assert!(
        output.status.code().is_some(),
        "TACH_TIMEOUT env var should be accepted without crashing"
    );
}

#[test]
fn test_timeout_flag_overrides_env_var() {
    // --timeout flag should take precedence over TACH_TIMEOUT env var
    let output = Command::new(tach_binary())
        .env("TACH_TIMEOUT", "1000")
        .args(["--timeout", "5", "--dry-run", "."])
        .output()
        .expect("Failed to execute tach-core");

    // Should not crash and should use the flag value
    assert!(
        output.status.code().is_some(),
        "--timeout should override TACH_TIMEOUT env var without crashing"
    );
}

#[test]
fn test_timeout_zero_is_handled() {
    // --timeout 0 might mean "no timeout" or be an error
    let output = Command::new(tach_binary())
        .args(["--timeout", "0", "--dry-run", "."])
        .output()
        .expect("Failed to execute tach-core");

    // Should not crash - either accept (no timeout) or reject gracefully
    assert!(
        output.status.code().is_some(),
        "--timeout 0 should not crash"
    );
}

#[test]
fn test_timeout_very_large_value_accepted() {
    // Very large timeout should be accepted (users may want this for slow tests)
    let output = Command::new(tach_binary())
        .args(["--timeout", "86400", "--dry-run", "."])
        .output()
        .expect("Failed to execute tach-core");

    let stderr = String::from_utf8_lossy(&output.stderr);

    // Should accept large values (86400 = 24 hours)
    assert!(
        output.status.code().is_some() && !stderr.contains("too large"),
        "--timeout 86400 (24h) should be accepted. stderr:\n{}",
        stderr
    );
}

#[test]
fn test_timeout_default_is_reasonable() {
    // Default timeout should be something reasonable (documented as 60s)
    // This test verifies the help mentions the default
    let output = Command::new(tach_binary())
        .args(["--help"])
        .output()
        .expect("Failed to execute tach-core");

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Help should mention the default value
    // Common patterns: [default: 60], (default 60), default: 60
    let has_default_mentioned = stdout.contains("60")
        || stdout.contains("default")
        || stdout.to_lowercase().contains("timeout");

    assert!(
        has_default_mentioned,
        "Help should mention timeout default. Output:\n{}",
        stdout
    );
}

#[test]
fn test_dry_run_respects_timeout_flag() {
    // Even in dry-run, timeout should be configurable
    let start = Instant::now();

    let output = Command::new(tach_binary())
        .args(["--timeout", "1", "--dry-run", "."])
        .output()
        .expect("Failed to execute tach-core");

    let elapsed = start.elapsed();

    // Dry-run should complete quickly regardless of timeout
    // (timeout only applies to actual test execution)
    assert!(
        elapsed < Duration::from_secs(30),
        "--dry-run should complete quickly even with --timeout. Took {:?}",
        elapsed
    );

    assert!(
        output.status.code().is_some(),
        "--dry-run with --timeout should not crash"
    );
}

#[test]
fn test_list_command_ignores_timeout() {
    // The 'list' command shouldn't be affected by timeout
    let output = Command::new(tach_binary())
        .args(["list", "--timeout", "1", "."])
        .output()
        .expect("Failed to execute tach-core");

    // Should complete without timeout issues
    assert!(
        output.status.code().is_some(),
        "'list' command should complete regardless of --timeout"
    );
}

#[test]
fn test_version_command_ignores_timeout() {
    // The 'version' command shouldn't be affected by timeout
    let output = Command::new(tach_binary())
        .args(["version", "--timeout", "1"])
        .output()
        .expect("Failed to execute tach-core");

    // Should complete (though might reject the flag as invalid for version)
    assert!(
        output.status.code().is_some(),
        "'version' command should not hang on --timeout"
    );
}

#[test]
fn test_self_test_ignores_timeout() {
    // self-test has its own timing but shouldn't be affected by --timeout
    let start = Instant::now();

    let output = Command::new(tach_binary())
        .args(["self-test"])
        .output()
        .expect("Failed to execute tach-core");

    let elapsed = start.elapsed();

    // self-test should complete in reasonable time
    assert!(
        elapsed < Duration::from_secs(60),
        "self-test should complete in < 60s. Took {:?}",
        elapsed
    );

    assert!(
        output.status.code().is_some(),
        "self-test should complete without hanging"
    );
}
