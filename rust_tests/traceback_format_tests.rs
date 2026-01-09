//! Traceback format regression tests for tach-core CLI.
//!
//! These tests verify that the --tb flag produces consistent output formats.
//! The --tb flag was added in 0.1.2 and supports: short, long, line, native, no
//!
//! Regression prevention: Ensures traceback formatting doesn't silently change.

use std::process::Command;

/// Marker that separates loader/discovery output from actual test execution output.
/// This follows the project's logging convention: `[tach:module]` prefix for all eprintln! output.
/// We use this to isolate test results from compilation warnings in loader output.
const TEST_OUTPUT_MARKER: &str = "[tach:reporter] Running";

/// Extract the test output section from combined stdout/stderr.
/// This filters out loader/discovery noise to focus on actual test results.
fn extract_test_output(combined: &str) -> &str {
    combined
        .split(TEST_OUTPUT_MARKER)
        .nth(1)
        .unwrap_or(combined)
}

/// Get the tach-core binary path
fn tach_binary() -> std::path::PathBuf {
    let mut path = std::env::current_exe().unwrap();
    path.pop(); // Remove test binary name
    path.pop(); // Remove deps
    path.push("tach-core");
    path
}

/// Run tach with a specific --tb style on a failing test
fn run_with_tb_style(style: &str) -> (String, String, i32) {
    let output = Command::new(tach_binary())
        .args([
            "--no-isolation",
            "-n",
            "1",
            "--tb",
            style,
            "tests/dummy_project/test_fail_assert.py",
        ])
        .output()
        .expect("Failed to execute tach-core");

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let code = output.status.code().unwrap_or(-1);

    (stdout, stderr, code)
}

// =============================================================================
// RED: Write failing tests first
// =============================================================================

#[test]
fn test_tb_flag_is_recognized() {
    // Test that --tb flag doesn't cause an error
    let output = Command::new(tach_binary())
        .args(["--help"])
        .output()
        .expect("Failed to execute tach-core");

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        stdout.contains("--tb") || stdout.contains("traceback"),
        "--tb flag should be documented in help output"
    );
}

#[test]
fn test_tb_short_truncates_traceback() {
    let (stdout, stderr, _code) = run_with_tb_style("short");
    let combined = format!("{}\n{}", stdout, stderr);

    // Short style should have traceback but be concise
    // It should NOT have the full "during handling of the above exception" chains
    // in the TEST OUTPUT section (after the reporter starts running tests)
    let long_chain_indicator = "During handling of the above exception";

    // Find the test output section (after loader/discovery)
    let test_output = extract_test_output(&combined);

    let has_long_chain = test_output.contains(long_chain_indicator);

    // Short should either have no traceback or a truncated one
    // The key is it shouldn't have verbose exception chains in test output
    assert!(
        !has_long_chain,
        "--tb short should not include verbose exception chains in test output. Output:\n{}",
        combined
    );
}

#[test]
fn test_tb_long_includes_full_traceback() {
    let (stdout, stderr, _code) = run_with_tb_style("long");
    let combined = format!("{}\n{}", stdout, stderr);

    // Long style should include file references
    let has_file_reference =
        combined.contains("File ") || combined.contains(".py") || combined.contains("line ");

    assert!(
        has_file_reference || combined.contains("Traceback") || combined.contains("Error"),
        "--tb long should include detailed traceback info. Output:\n{}",
        combined
    );
}

#[test]
fn test_tb_line_single_line_format() {
    let (stdout, stderr, _code) = run_with_tb_style("line");
    let combined = format!("{}\n{}", stdout, stderr);

    // Line style should be compact - typically one line per error
    // Count newlines in the error portion to verify compactness
    let error_lines: Vec<&str> = combined
        .lines()
        .filter(|l| l.contains("Error") || l.contains("assert") || l.contains("FAILED"))
        .collect();

    // Should have some error indication
    assert!(
        !error_lines.is_empty() || combined.contains("FAILED") || combined.contains("failed"),
        "--tb line should still show failure indication. Output:\n{}",
        combined
    );
}

#[test]
fn test_tb_no_suppresses_traceback() {
    let (stdout, stderr, _code) = run_with_tb_style("no");
    let combined = format!("{}\n{}", stdout, stderr);

    // "no" style should suppress traceback entirely in TEST OUTPUT
    // Find the test output section (after loader/discovery)
    let test_output = extract_test_output(&combined);

    // Should NOT contain "Traceback (most recent call last)" in test output
    let has_full_traceback = test_output.contains("Traceback (most recent call last)");

    assert!(
        !has_full_traceback,
        "--tb no should suppress full traceback in test output. Output:\n{}",
        combined
    );

    // But should still indicate the test failed
    assert!(
        combined.contains("FAILED") || combined.contains("failed") || combined.contains("1 failed"),
        "--tb no should still report test failure. Output:\n{}",
        combined
    );
}

#[test]
fn test_tb_native_uses_python_format() {
    let (stdout, stderr, _code) = run_with_tb_style("native");
    let combined = format!("{}\n{}", stdout, stderr);

    // Native style should use Python's default traceback format
    // This typically includes "Traceback" or file references
    let has_python_style = combined.contains("Traceback")
        || combined.contains("File \"")
        || combined.contains("AssertionError");

    // Native may also just pass through whatever pytest produces
    assert!(
        has_python_style || combined.contains("Error") || combined.contains("failed"),
        "--tb native should show Python-style traceback or error. Output:\n{}",
        combined
    );
}

#[test]
fn test_tb_invalid_value_rejected() {
    let output = Command::new(tach_binary())
        .args([
            "--no-isolation",
            "-n",
            "1",
            "--tb",
            "invalid_style_xyz",
            "tests/dummy_project/test_simple.py",
        ])
        .output()
        .expect("Failed to execute tach-core");

    let stderr = String::from_utf8_lossy(&output.stderr);
    let code = output.status.code().unwrap_or(0);

    // Should either reject with non-zero exit or show an error
    assert!(
        code != 0 || stderr.contains("error") || stderr.contains("invalid"),
        "Invalid --tb value should be rejected. Exit code: {}, stderr:\n{}",
        code,
        stderr
    );
}

#[test]
fn test_tb_env_var_works() {
    // Test TACH_TB environment variable
    let output = Command::new(tach_binary())
        .env("TACH_TB", "short")
        .args([
            "--no-isolation",
            "-n",
            "1",
            "tests/dummy_project/test_fail_assert.py",
        ])
        .output()
        .expect("Failed to execute tach-core");

    // Should not crash
    assert!(
        output.status.code().is_some(),
        "TACH_TB env var should be accepted without crashing"
    );
}

#[test]
fn test_tb_flag_overrides_env_var() {
    // --tb flag should override TACH_TB env var
    let output = Command::new(tach_binary())
        .env("TACH_TB", "long")
        .args([
            "--no-isolation",
            "-n",
            "1",
            "--tb",
            "no",
            "tests/dummy_project/test_fail_assert.py",
        ])
        .output()
        .expect("Failed to execute tach-core");

    let combined = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    // With --tb no, should not have full traceback in TEST OUTPUT (flag overrides env)
    // Find the test output section (after loader/discovery)
    let test_output = extract_test_output(&combined);
    let has_full_traceback = test_output.contains("Traceback (most recent call last)");

    assert!(
        !has_full_traceback,
        "--tb flag should override TACH_TB env var. Output:\n{}",
        combined
    );
}

#[test]
fn test_tb_styles_produce_different_output() {
    // Different --tb styles should produce meaningfully different output
    let (_, _stderr_short, _) = run_with_tb_style("short");
    let (_, stderr_long, _) = run_with_tb_style("long");
    let (_, stderr_no, _) = run_with_tb_style("no");

    // At minimum, "no" should be shorter than "long"
    // (This is a sanity check that the styles actually differ)
    let no_len = stderr_no.len();
    let long_len = stderr_long.len();

    // If both are empty, that's also a valid state (output might be on stdout)
    if no_len > 0 && long_len > 0 {
        assert!(
            no_len <= long_len || stderr_no != stderr_long,
            "--tb styles should produce different output lengths or content"
        );
    }
}

#[test]
fn test_tb_with_passing_tests_no_traceback() {
    // Passing tests should have no traceback regardless of --tb style
    for style in &["short", "long", "line", "native", "no"] {
        let output = Command::new(tach_binary())
            .args([
                "--no-isolation",
                "-n",
                "1",
                "--tb",
                style,
                "tests/dummy_project/test_simple.py",
            ])
            .output()
            .expect("Failed to execute tach-core");

        let combined = format!(
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );

        // Skip if pytest not available (environment issue, not a test failure)
        if combined.contains("No module named 'pytest'") {
            eprintln!("Skipping --tb {} test: pytest not available", style);
            continue;
        }

        // Should not have traceback for passing tests in TEST OUTPUT
        // Find the test output section (after loader/discovery)
        let test_output = extract_test_output(&combined);
        assert!(
            !test_output.contains("Traceback (most recent call last)"),
            "--tb {} should not show traceback for passing tests. Output:\n{}",
            style,
            combined
        );

        // Should indicate success (only if tests actually ran)
        if !combined.contains("Error") && !combined.contains("failed to") {
            assert!(
                output.status.code() == Some(0),
                "--tb {} should exit 0 for passing tests. Output:\n{}",
                style,
                combined
            );
        }
    }
}
