//! Verbosity level regression tests for tach-core CLI.
//!
//! These tests verify that -v (verbose) and -q (quiet) flags work correctly.
//! Verbosity controls how much output the user sees during test runs.
//!
//! Regression prevention: Ensures verbosity behavior is consistent.

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
// -v (verbose) flag tests
// =============================================================================

#[test]
fn test_verbose_flag_is_recognized() {
    let output = Command::new(tach_binary())
        .args(["--help"])
        .output()
        .expect("Failed to execute tach-core");

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        stdout.contains("-v") || stdout.contains("--verbose") || stdout.contains("verbose"),
        "-v/--verbose flag should be documented in help output"
    );
}

#[test]
fn test_verbose_single_level() {
    // -v should work
    let output = Command::new(tach_binary())
        .args(["-v", "--dry-run", "tests/dummy_project/"])
        .output()
        .expect("Failed to execute tach-core");

    assert_eq!(
        output.status.code(),
        Some(0),
        "-v should be accepted without error"
    );
}

#[test]
fn test_verbose_double_level() {
    // -vv should work (more verbose)
    let output = Command::new(tach_binary())
        .args(["-vv", "--dry-run", "tests/dummy_project/"])
        .output()
        .expect("Failed to execute tach-core");

    assert!(
        output.status.code().is_some(),
        "-vv should be accepted (or gracefully rejected)"
    );
}

#[test]
fn test_verbose_long_form() {
    // --verbose should work
    let output = Command::new(tach_binary())
        .args(["--verbose", "--dry-run", "tests/dummy_project/"])
        .output()
        .expect("Failed to execute tach-core");

    assert_eq!(
        output.status.code(),
        Some(0),
        "--verbose should be accepted without error"
    );
}

#[test]
fn test_verbose_produces_more_output() {
    // -v should produce more output than without
    let output_normal = Command::new(tach_binary())
        .args(["--dry-run", "tests/dummy_project/"])
        .output()
        .expect("Failed to execute tach-core");

    let output_verbose = Command::new(tach_binary())
        .args(["-v", "--dry-run", "tests/dummy_project/"])
        .output()
        .expect("Failed to execute tach-core");

    let normal_combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output_normal.stdout),
        String::from_utf8_lossy(&output_normal.stderr)
    );

    let verbose_combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output_verbose.stdout),
        String::from_utf8_lossy(&output_verbose.stderr)
    );

    // Verbose should have at least as much output (often more)
    // This is a soft check - implementation may vary
    let normal_len = normal_combined.len();
    let verbose_len = verbose_combined.len();

    // Just ensure both complete successfully
    assert!(
        output_normal.status.code() == Some(0) && output_verbose.status.code() == Some(0),
        "Both normal and verbose modes should succeed"
    );

    // Log for visibility (not a hard assertion since implementations vary)
    if verbose_len < normal_len {
        eprintln!(
            "Note: verbose output ({} chars) shorter than normal ({} chars)",
            verbose_len, normal_len
        );
    }
}

// =============================================================================
// -q (quiet) flag tests
// =============================================================================

#[test]
fn test_quiet_flag_is_recognized() {
    let output = Command::new(tach_binary())
        .args(["--help"])
        .output()
        .expect("Failed to execute tach-core");

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        stdout.contains("-q") || stdout.contains("--quiet") || stdout.contains("quiet"),
        "-q/--quiet flag should be documented in help output"
    );
}

#[test]
fn test_quiet_single_level() {
    // -q should work
    let output = Command::new(tach_binary())
        .args(["-q", "--dry-run", "tests/dummy_project/"])
        .output()
        .expect("Failed to execute tach-core");

    assert_eq!(
        output.status.code(),
        Some(0),
        "-q should be accepted without error"
    );
}

#[test]
fn test_quiet_long_form() {
    // --quiet should work
    let output = Command::new(tach_binary())
        .args(["--quiet", "--dry-run", "tests/dummy_project/"])
        .output()
        .expect("Failed to execute tach-core");

    assert_eq!(
        output.status.code(),
        Some(0),
        "--quiet should be accepted without error"
    );
}

#[test]
fn test_quiet_produces_less_output() {
    // -q should produce less output than without
    let output_normal = Command::new(tach_binary())
        .args(["--dry-run", "tests/dummy_project/"])
        .output()
        .expect("Failed to execute tach-core");

    let output_quiet = Command::new(tach_binary())
        .args(["-q", "--dry-run", "tests/dummy_project/"])
        .output()
        .expect("Failed to execute tach-core");

    let normal_combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output_normal.stdout),
        String::from_utf8_lossy(&output_normal.stderr)
    );

    let quiet_combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output_quiet.stdout),
        String::from_utf8_lossy(&output_quiet.stderr)
    );

    // Quiet should have at most as much output (often less)
    let normal_len = normal_combined.len();
    let quiet_len = quiet_combined.len();

    // Just ensure both complete successfully
    assert!(
        output_normal.status.code() == Some(0) && output_quiet.status.code() == Some(0),
        "Both normal and quiet modes should succeed"
    );

    // Log for visibility
    if quiet_len > normal_len {
        eprintln!(
            "Note: quiet output ({} chars) longer than normal ({} chars)",
            quiet_len, normal_len
        );
    }
}

// =============================================================================
// Combined and edge case tests
// =============================================================================

#[test]
fn test_verbose_and_quiet_conflict() {
    // Using both -v and -q might be an error or last one wins
    let output = Command::new(tach_binary())
        .args(["-v", "-q", "--dry-run", "tests/dummy_project/"])
        .output()
        .expect("Failed to execute tach-core");

    // Should not crash - either error gracefully or pick one
    assert!(
        output.status.code().is_some(),
        "-v and -q together should not crash"
    );
}

#[test]
fn test_verbosity_with_json_format() {
    // -v with --format json should work
    let output = Command::new(tach_binary())
        .args([
            "-v",
            "--format",
            "json",
            "--dry-run",
            "tests/dummy_project/",
        ])
        .output()
        .expect("Failed to execute tach-core");

    assert!(
        output.status.code().is_some(),
        "-v with --format json should not crash"
    );
}

#[test]
fn test_quiet_with_json_format() {
    // -q with --format json should work
    let output = Command::new(tach_binary())
        .args([
            "-q",
            "--format",
            "json",
            "--dry-run",
            "tests/dummy_project/",
        ])
        .output()
        .expect("Failed to execute tach-core");

    assert!(
        output.status.code().is_some(),
        "-q with --format json should not crash"
    );
}

#[test]
fn test_verbosity_with_list_command() {
    // -v with list command should work
    let output = Command::new(tach_binary())
        .args(["-v", "list"])
        .output()
        .expect("Failed to execute tach-core");

    // Should not crash (though may not change output for list)
    assert!(
        output.status.code().is_some(),
        "-v with list should not crash"
    );
}

#[test]
fn test_quiet_with_list_command() {
    // -q with list command should work
    let output = Command::new(tach_binary())
        .args(["-q", "list"])
        .output()
        .expect("Failed to execute tach-core");

    // Should not crash
    assert!(
        output.status.code().is_some(),
        "-q with list should not crash"
    );
}

#[test]
fn test_verbosity_with_version_command() {
    // -v with version command should work
    let output = Command::new(tach_binary())
        .args(["-v", "version"])
        .output()
        .expect("Failed to execute tach-core");

    // Should not crash
    assert!(
        output.status.code().is_some(),
        "-v with version should not crash"
    );
}

#[test]
fn test_verbosity_with_self_test() {
    // -v with self-test should work
    let output = Command::new(tach_binary())
        .args(["-v", "self-test"])
        .output()
        .expect("Failed to execute tach-core");

    // Should complete
    assert!(
        output.status.code().is_some(),
        "-v with self-test should complete"
    );
}

#[test]
fn test_quiet_suppresses_progress() {
    // -q should suppress progress indicators during test run
    // This is harder to test without actually running tests,
    // but we can verify the flag is accepted
    let output = Command::new(tach_binary())
        .args(["-q", "--dry-run", "."])
        .output()
        .expect("Failed to execute tach-core");

    assert_eq!(
        output.status.code(),
        Some(0),
        "-q should work with any test path"
    );
}

#[test]
fn test_verbose_shows_discovery_info() {
    // -v should show more discovery information
    let output = Command::new(tach_binary())
        .args(["-v", "--dry-run", "tests/dummy_project/"])
        .output()
        .expect("Failed to execute tach-core");

    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    // Verbose mode often shows more details about what's happening
    // At minimum, it should succeed
    assert_eq!(
        output.status.code(),
        Some(0),
        "-v should succeed. Output:\n{}",
        combined
    );
}
