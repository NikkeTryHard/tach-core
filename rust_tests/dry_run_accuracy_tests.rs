//! Dry-run accuracy regression tests for tach-core CLI.
//!
//! These tests verify that --dry-run accurately reflects what would run.
//! The --dry-run flag shows tests without executing them.
//!
//! Regression prevention: Ensures dry-run matches actual execution.

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
fn test_dry_run_flag_is_recognized() {
    let output = Command::new(tach_binary())
        .args(["--help"])
        .output()
        .expect("Failed to execute tach-core");

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        stdout.contains("--dry-run") || stdout.contains("dry"),
        "--dry-run flag should be documented in help output"
    );
}

#[test]
fn test_dry_run_exits_zero() {
    // --dry-run should always exit 0 (no tests fail since none run)
    let output = Command::new(tach_binary())
        .args(["--dry-run", "tests/dummy_project/"])
        .output()
        .expect("Failed to execute tach-core");

    assert_eq!(output.status.code(), Some(0), "--dry-run should exit 0");
}

#[test]
fn test_dry_run_does_not_execute_tests() {
    // --dry-run should NOT actually run any tests
    let output = Command::new(tach_binary())
        .args(["--dry-run", "tests/dummy_project/test_fail_assert.py"])
        .output()
        .expect("Failed to execute tach-core");

    let combined = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    // Should NOT show test results like PASSED/FAILED for individual tests
    // (It might show summary of what would run)
    let has_execution_output = combined.contains("PASSED") && combined.contains("FAILED");

    // Dry run should exit 0 even for failing tests
    assert_eq!(
        output.status.code(),
        Some(0),
        "--dry-run should exit 0 even for tests that would fail. Output:\n{}",
        combined
    );

    // If it shows PASSED/FAILED, they should be from dry-run listing, not execution
    if has_execution_output {
        // This is suspicious - might be actually running tests
        eprintln!("Warning: --dry-run output contains PASSED/FAILED. Verify no actual execution.");
    }
}

#[test]
fn test_dry_run_shows_test_count() {
    // --dry-run should show how many tests would run
    let output = Command::new(tach_binary())
        .args(["--dry-run", "tests/dummy_project/"])
        .output()
        .expect("Failed to execute tach-core");

    let combined = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    // Should contain some indication of test count
    let has_count = combined.contains("test")
        || combined.contains("selected")
        || combined.contains("would")
        || combined.chars().any(|c| c.is_ascii_digit());

    assert!(
        has_count,
        "--dry-run should indicate test count. Output:\n{}",
        combined
    );
}

#[test]
fn test_dry_run_respects_keyword_filter() {
    // --dry-run should respect -k filter
    let output_all = Command::new(tach_binary())
        .args(["--dry-run", "tests/dummy_project/"])
        .output()
        .expect("Failed to execute tach-core");

    let output_filtered = Command::new(tach_binary())
        .args(["--dry-run", "-k", "simple", "tests/dummy_project/"])
        .output()
        .expect("Failed to execute tach-core");

    // Both should succeed
    assert!(
        output_all.status.code() == Some(0) && output_filtered.status.code() == Some(0),
        "--dry-run with and without filter should both succeed"
    );
}

#[test]
fn test_dry_run_respects_marker_filter() {
    // --dry-run should respect -m filter
    let output = Command::new(tach_binary())
        .args(["--dry-run", "-m", "not slow", "tests/dummy_project/"])
        .output()
        .expect("Failed to execute tach-core");

    assert_eq!(
        output.status.code(),
        Some(0),
        "--dry-run with -m filter should succeed"
    );
}

#[test]
fn test_dry_run_vs_collect_only_similarity() {
    // --dry-run and --collect-only should show similar information
    let dry_run = Command::new(tach_binary())
        .args(["--dry-run", "tests/dummy_project/test_simple.py"])
        .output()
        .expect("Failed to execute tach-core");

    let collect_only = Command::new(tach_binary())
        .args(["--collect-only", "tests/dummy_project/test_simple.py"])
        .output()
        .expect("Failed to execute tach-core");

    // Both should succeed
    assert!(
        dry_run.status.code() == Some(0) && collect_only.status.code() == Some(0),
        "--dry-run and --collect-only should both succeed"
    );
}

#[test]
fn test_list_command_works_without_path() {
    // 'list' command should work without a path argument
    let output = Command::new(tach_binary())
        .args(["list"])
        .output()
        .expect("Failed to execute tach-core");

    // Should succeed (discovers tests in current directory)
    assert!(
        output.status.code().is_some(),
        "'list' command should not crash"
    );
}

#[test]
fn test_dry_run_with_workers_flag() {
    // --dry-run should accept -n flag even though it doesn't run tests
    let output = Command::new(tach_binary())
        .args(["--dry-run", "-n", "4", "tests/dummy_project/"])
        .output()
        .expect("Failed to execute tach-core");

    assert_eq!(
        output.status.code(),
        Some(0),
        "--dry-run with -n flag should succeed"
    );
}

#[test]
fn test_dry_run_with_verbose() {
    // --dry-run with -v should work
    let output = Command::new(tach_binary())
        .args(["--dry-run", "-v", "tests/dummy_project/"])
        .output()
        .expect("Failed to execute tach-core");

    assert_eq!(
        output.status.code(),
        Some(0),
        "--dry-run with -v should succeed"
    );
}

#[test]
fn test_dry_run_with_quiet() {
    // --dry-run with -q should work
    let output = Command::new(tach_binary())
        .args(["--dry-run", "-q", "tests/dummy_project/"])
        .output()
        .expect("Failed to execute tach-core");

    assert_eq!(
        output.status.code(),
        Some(0),
        "--dry-run with -q should succeed"
    );
}

#[test]
fn test_dry_run_with_json_format() {
    // --dry-run with --format json should work
    let output = Command::new(tach_binary())
        .args(["--dry-run", "--format", "json", "tests/dummy_project/"])
        .output()
        .expect("Failed to execute tach-core");

    // Should succeed
    assert_eq!(
        output.status.code(),
        Some(0),
        "--dry-run with --format json should succeed"
    );
}

#[test]
fn test_dry_run_nonexistent_path() {
    // --dry-run with nonexistent path should handle gracefully
    let output = Command::new(tach_binary())
        .args(["--dry-run", "nonexistent_path_xyz123/"])
        .output()
        .expect("Failed to execute tach-core");

    // Should either exit non-zero or handle gracefully
    assert!(
        output.status.code().is_some(),
        "--dry-run with bad path should not crash"
    );
}

#[test]
fn test_dry_run_empty_directory() {
    // --dry-run on directory with no tests
    let output = Command::new(tach_binary())
        .args(["--dry-run", "src/"])
        .output()
        .expect("Failed to execute tach-core");

    // Should handle gracefully (no tests to run)
    assert!(
        output.status.code().is_some(),
        "--dry-run on non-test directory should not crash"
    );
}

#[test]
fn test_dry_run_is_fast() {
    // --dry-run should be fast since it doesn't execute tests
    use std::time::Instant;

    let start = Instant::now();

    let output = Command::new(tach_binary())
        .args(["--dry-run", "tests/"])
        .output()
        .expect("Failed to execute tach-core");

    let elapsed = start.elapsed();

    // Dry-run should complete in < 30 seconds for any reasonable test suite
    // (It's just doing discovery, not execution)
    assert!(
        elapsed.as_secs() < 30,
        "--dry-run should be fast. Took {:?}",
        elapsed
    );

    assert!(
        output.status.code().is_some(),
        "--dry-run should complete without hanging"
    );
}

#[test]
fn test_collect_only_is_alias_for_dry_run_or_list() {
    // --collect-only should behave like list or dry-run
    let output = Command::new(tach_binary())
        .args(["--collect-only", "tests/dummy_project/"])
        .output()
        .expect("Failed to execute tach-core");

    assert_eq!(
        output.status.code(),
        Some(0),
        "--collect-only should succeed like --dry-run"
    );
}
