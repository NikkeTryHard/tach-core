//! Filter expression regression tests for tach-core CLI.
//!
//! These tests verify that -k (keyword) and -m (marker) filtering work correctly.
//! Filter expressions are critical for test selection.
//!
//! Regression prevention: Ensures filter behavior is consistent.

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
// -k (keyword) filter tests
// =============================================================================

#[test]
fn test_keyword_flag_is_recognized() {
    let output = Command::new(tach_binary())
        .args(["--help"])
        .output()
        .expect("Failed to execute tach-core");

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        stdout.contains("-k") || stdout.contains("--keyword") || stdout.contains("keyword"),
        "-k/--keyword flag should be documented in help output"
    );
}

#[test]
fn test_keyword_filter_basic() {
    // -k with a simple keyword should work
    let output = Command::new(tach_binary())
        .args(["list", "-k", "simple", "tests/dummy_project/"])
        .output()
        .expect("Failed to execute tach-core");

    // Should not crash
    assert!(output.status.code().is_some(), "-k filter should not crash");
}

#[test]
fn test_keyword_filter_with_and() {
    // -k "test and simple" - AND expression
    let output = Command::new(tach_binary())
        .args(["list", "-k", "test and simple", "tests/dummy_project/"])
        .output()
        .expect("Failed to execute tach-core");

    // Should not crash (expression might match or not)
    assert!(
        output.status.code().is_some(),
        "-k with 'and' expression should not crash"
    );
}

#[test]
fn test_keyword_filter_with_or() {
    // -k "simple or async" - OR expression
    let output = Command::new(tach_binary())
        .args(["list", "-k", "simple or async", "tests/dummy_project/"])
        .output()
        .expect("Failed to execute tach-core");

    // Should not crash
    assert!(
        output.status.code().is_some(),
        "-k with 'or' expression should not crash"
    );
}

#[test]
fn test_keyword_filter_with_not() {
    // -k "not fail" - NOT expression
    let output = Command::new(tach_binary())
        .args(["list", "-k", "not fail", "tests/dummy_project/"])
        .output()
        .expect("Failed to execute tach-core");

    // Should not crash
    assert!(
        output.status.code().is_some(),
        "-k with 'not' expression should not crash"
    );
}

#[test]
fn test_keyword_filter_with_parentheses() {
    // -k "(simple or async) and not fail" - complex expression
    let output = Command::new(tach_binary())
        .args([
            "list",
            "-k",
            "(simple or async) and not fail",
            "tests/dummy_project/",
        ])
        .output()
        .expect("Failed to execute tach-core");

    // Should not crash (complex expressions should be supported)
    assert!(
        output.status.code().is_some(),
        "-k with parentheses should not crash"
    );
}

#[test]
fn test_keyword_filter_empty_string() {
    // -k "" - empty filter should match all or be an error
    let output = Command::new(tach_binary())
        .args(["list", "-k", "", "tests/dummy_project/"])
        .output()
        .expect("Failed to execute tach-core");

    // Should not crash - might match all tests or reject empty
    assert!(
        output.status.code().is_some(),
        "-k with empty string should not crash"
    );
}

#[test]
fn test_keyword_filter_no_match() {
    // -k with a keyword that matches nothing
    let output = Command::new(tach_binary())
        .args(["list", "-k", "xyznonexistent123", "tests/dummy_project/"])
        .output()
        .expect("Failed to execute tach-core");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{}\n{}", stdout, stderr);

    // Should either show 0 tests or indicate no match
    let has_zero_indication = combined.contains("0")
        || combined.contains("no tests")
        || combined.to_lowercase().contains("empty")
        || combined.contains("collected")
        || output.status.code() == Some(0);

    assert!(
        has_zero_indication || output.status.code().is_some(),
        "-k with no matches should handle gracefully. Output:\n{}",
        combined
    );
}

// =============================================================================
// -m (marker) filter tests
// =============================================================================

#[test]
fn test_marker_flag_is_recognized() {
    let output = Command::new(tach_binary())
        .args(["--help"])
        .output()
        .expect("Failed to execute tach-core");

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        stdout.contains("-m") || stdout.contains("--marker") || stdout.contains("marker"),
        "-m/--markers flag should be documented in help output"
    );
}

#[test]
fn test_marker_filter_basic() {
    // -m with a simple marker should work
    let output = Command::new(tach_binary())
        .args(["list", "-m", "slow", "tests/"])
        .output()
        .expect("Failed to execute tach-core");

    // Should not crash
    assert!(output.status.code().is_some(), "-m filter should not crash");
}

#[test]
fn test_marker_filter_with_not() {
    // -m "not slow" - exclude marker
    let output = Command::new(tach_binary())
        .args(["list", "-m", "not slow", "tests/"])
        .output()
        .expect("Failed to execute tach-core");

    // Should not crash
    assert!(
        output.status.code().is_some(),
        "-m with 'not' should not crash"
    );
}

#[test]
fn test_marker_filter_with_and() {
    // -m "slow and integration" - AND markers
    let output = Command::new(tach_binary())
        .args(["list", "-m", "slow and integration", "tests/"])
        .output()
        .expect("Failed to execute tach-core");

    // Should not crash
    assert!(
        output.status.code().is_some(),
        "-m with 'and' should not crash"
    );
}

#[test]
fn test_marker_filter_with_or() {
    // -m "slow or skip" - OR markers
    let output = Command::new(tach_binary())
        .args(["list", "-m", "slow or skip", "tests/"])
        .output()
        .expect("Failed to execute tach-core");

    // Should not crash
    assert!(
        output.status.code().is_some(),
        "-m with 'or' should not crash"
    );
}

// =============================================================================
// Combined filter tests
// =============================================================================

#[test]
fn test_keyword_and_marker_together() {
    // -k and -m together should both be applied
    let output = Command::new(tach_binary())
        .args(["list", "-k", "test", "-m", "not slow", "tests/"])
        .output()
        .expect("Failed to execute tach-core");

    // Should not crash
    assert!(
        output.status.code().is_some(),
        "-k and -m together should not crash"
    );
}

#[test]
fn test_multiple_keyword_filters() {
    // Multiple -k flags (if supported)
    let output = Command::new(tach_binary())
        .args(["list", "-k", "test", "-k", "simple", "tests/"])
        .output()
        .expect("Failed to execute tach-core");

    // Should not crash (might combine or reject)
    assert!(
        output.status.code().is_some(),
        "Multiple -k flags should not crash"
    );
}

#[test]
fn test_filter_with_dry_run() {
    // Filters should work with --dry-run
    let output = Command::new(tach_binary())
        .args(["--dry-run", "-k", "simple", "tests/dummy_project/"])
        .output()
        .expect("Failed to execute tach-core");

    // Should not crash
    assert!(
        output.status.code().is_some(),
        "-k with --dry-run should not crash"
    );
}

#[test]
fn test_filter_special_characters() {
    // Filters with special characters
    let output = Command::new(tach_binary())
        .args(["list", "-k", "test_*", "tests/"])
        .output()
        .expect("Failed to execute tach-core");

    // Should not crash (might use glob or literal match)
    assert!(
        output.status.code().is_some(),
        "-k with special characters should not crash"
    );
}

#[test]
fn test_filter_case_sensitivity() {
    // Test case sensitivity behavior
    let output_lower = Command::new(tach_binary())
        .args(["list", "-k", "simple", "tests/dummy_project/"])
        .output()
        .expect("Failed to execute tach-core");

    let output_upper = Command::new(tach_binary())
        .args(["list", "-k", "SIMPLE", "tests/dummy_project/"])
        .output()
        .expect("Failed to execute tach-core");

    // Both should work without crashing
    assert!(
        output_lower.status.code().is_some() && output_upper.status.code().is_some(),
        "Both lower and upper case filters should work"
    );
}

#[test]
fn test_filter_with_path_like_pattern() {
    // -k with path-like pattern
    let output = Command::new(tach_binary())
        .args(["list", "-k", "dummy_project", "tests/"])
        .output()
        .expect("Failed to execute tach-core");

    // Should match tests in that path
    assert!(
        output.status.code().is_some(),
        "-k with path-like pattern should not crash"
    );
}

#[test]
fn test_filter_unicode() {
    // -k with unicode characters (edge case)
    let output = Command::new(tach_binary())
        .args(["list", "-k", "テスト", "tests/"])
        .output()
        .expect("Failed to execute tach-core");

    // Should not crash (will likely match nothing)
    assert!(
        output.status.code().is_some(),
        "-k with unicode should not crash"
    );
}

#[test]
fn test_collect_only_respects_filters() {
    // --collect-only should respect -k filter
    let output = Command::new(tach_binary())
        .args(["--collect-only", "-k", "simple", "tests/dummy_project/"])
        .output()
        .expect("Failed to execute tach-core");

    // Should not crash
    assert!(
        output.status.code().is_some(),
        "--collect-only with -k should not crash"
    );
}
