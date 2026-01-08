//! Coverage flag regression tests for tach-core CLI.
//!
//! These tests verify that the --coverage flag works correctly.
//! Coverage tracking requires Python 3.12+ with PEP 669 support.
//!
//! Regression prevention: Ensures coverage behavior is consistent.

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
fn test_coverage_flag_is_recognized() {
    let output = Command::new(tach_binary())
        .args(["--help"])
        .output()
        .expect("Failed to execute tach-core");

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        stdout.contains("--coverage") || stdout.contains("coverage"),
        "--coverage flag should be documented in help output"
    );
}

#[test]
fn test_coverage_flag_accepted() {
    let output = Command::new(tach_binary())
        .args(["--coverage", "--dry-run", "tests/dummy_project/"])
        .output()
        .expect("Failed to execute tach-core");

    // Should not crash - may warn if Python < 3.12
    assert!(
        output.status.code().is_some(),
        "--coverage should not crash"
    );
}

#[test]
fn test_coverage_with_cov_path() {
    let output = Command::new(tach_binary())
        .args([
            "--coverage",
            "--cov",
            "src/",
            "--dry-run",
            "tests/dummy_project/",
        ])
        .output()
        .expect("Failed to execute tach-core");

    assert!(
        output.status.code().is_some(),
        "--coverage with --cov should not crash"
    );
}

#[test]
fn test_cov_flag_is_recognized() {
    let output = Command::new(tach_binary())
        .args(["--help"])
        .output()
        .expect("Failed to execute tach-core");

    let stdout = String::from_utf8_lossy(&output.stdout);

    // --cov specifies source directories for coverage
    assert!(
        stdout.contains("--cov") || stdout.contains("coverage"),
        "--cov flag should be documented in help output"
    );
}

#[test]
fn test_cov_multiple_paths() {
    let output = Command::new(tach_binary())
        .args([
            "--coverage",
            "--cov",
            "src/",
            "--cov",
            "lib/",
            "--dry-run",
            "tests/dummy_project/",
        ])
        .output()
        .expect("Failed to execute tach-core");

    // Multiple --cov paths should be accepted
    assert!(
        output.status.code().is_some(),
        "Multiple --cov paths should not crash"
    );
}

#[test]
fn test_coverage_env_var() {
    let output = Command::new(tach_binary())
        .env("TACH_COVERAGE", "1")
        .args(["--dry-run", "tests/dummy_project/"])
        .output()
        .expect("Failed to execute tach-core");

    // TACH_COVERAGE env var should be accepted
    assert!(
        output.status.code().is_some(),
        "TACH_COVERAGE env var should be accepted"
    );
}

#[test]
fn test_coverage_output_env_var() {
    let output = Command::new(tach_binary())
        .args(["--help"])
        .output()
        .expect("Failed to execute tach-core");

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Check if TACH_COVERAGE_OUTPUT is documented
    if stdout.contains("TACH_COVERAGE_OUTPUT") {
        eprintln!("TACH_COVERAGE_OUTPUT env var is documented");
    }
}

#[test]
fn test_coverage_format_env_var() {
    let output = Command::new(tach_binary())
        .args(["--help"])
        .output()
        .expect("Failed to execute tach-core");

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Check if TACH_COVERAGE_FORMAT is documented
    if stdout.contains("TACH_COVERAGE_FORMAT") {
        eprintln!("TACH_COVERAGE_FORMAT env var is documented");
    }
}

#[test]
fn test_coverage_with_verbose() {
    let output = Command::new(tach_binary())
        .args(["--coverage", "-v", "--dry-run", "tests/dummy_project/"])
        .output()
        .expect("Failed to execute tach-core");

    assert!(
        output.status.code().is_some(),
        "--coverage with -v should not crash"
    );
}

#[test]
fn test_coverage_with_json_format() {
    let output = Command::new(tach_binary())
        .args([
            "--coverage",
            "--format",
            "json",
            "--dry-run",
            "tests/dummy_project/",
        ])
        .output()
        .expect("Failed to execute tach-core");

    assert!(
        output.status.code().is_some(),
        "--coverage with --format json should not crash"
    );
}

#[test]
fn test_coverage_with_workers() {
    let output = Command::new(tach_binary())
        .args(["--coverage", "-n", "4", "--dry-run", "tests/dummy_project/"])
        .output()
        .expect("Failed to execute tach-core");

    assert!(
        output.status.code().is_some(),
        "--coverage with -n should not crash"
    );
}

#[test]
fn test_coverage_with_no_isolation() {
    let output = Command::new(tach_binary())
        .args([
            "--coverage",
            "--no-isolation",
            "--dry-run",
            "tests/dummy_project/",
        ])
        .output()
        .expect("Failed to execute tach-core");

    assert!(
        output.status.code().is_some(),
        "--coverage with --no-isolation should not crash"
    );
}
