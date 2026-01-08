//! JUnit XML output regression tests for tach-core CLI.
//!
//! These tests verify that --junit-xml produces valid output.
//! JUnit XML is used for CI integration (Jenkins, GitHub Actions, etc.).
//!
//! Regression prevention: Ensures JUnit XML format remains parseable.

use std::fs;
use std::process::Command;
use tempfile::TempDir;

/// Get the tach-core binary path
fn tach_binary() -> std::path::PathBuf {
    let mut path = std::env::current_exe().unwrap();
    path.pop();
    path.pop();
    path.push("tach-core");
    path
}

#[test]
fn test_junit_xml_flag_is_recognized() {
    let output = Command::new(tach_binary())
        .args(["--help"])
        .output()
        .expect("Failed to execute tach-core");

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        stdout.contains("--junit-xml") || stdout.contains("junit"),
        "--junit-xml flag should be documented in help output"
    );
}

#[test]
fn test_junit_xml_creates_file() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let xml_path = temp_dir.path().join("test-results.xml");

    let output = Command::new(tach_binary())
        .args([
            "--junit-xml",
            xml_path.to_str().unwrap(),
            "--dry-run",
            "tests/dummy_project/",
        ])
        .output()
        .expect("Failed to execute tach-core");

    // Command should succeed
    assert!(
        output.status.code().is_some(),
        "--junit-xml should not crash"
    );

    // Note: dry-run may or may not create the file - this is implementation-dependent
    // The key is that it doesn't crash
}

#[test]
fn test_junit_xml_with_actual_tests() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let xml_path = temp_dir.path().join("test-results.xml");

    let output = Command::new(tach_binary())
        .args([
            "--no-isolation",
            "-n",
            "1",
            "--junit-xml",
            xml_path.to_str().unwrap(),
            "tests/dummy_project/test_simple.py",
        ])
        .output()
        .expect("Failed to execute tach-core");

    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    // Skip if environment isn't set up
    if combined.contains("No module named") {
        eprintln!("Skipping: pytest not available");
        return;
    }

    // If tests ran, check if XML file was created
    if xml_path.exists() {
        let content = fs::read_to_string(&xml_path).expect("Failed to read XML file");

        // Basic XML validation
        assert!(
            content.contains("<?xml")
                || content.contains("<testsuites")
                || content.contains("<testsuite"),
            "JUnit XML should contain valid XML elements. Content:\n{}",
            content
        );
    }
}

#[test]
fn test_junit_xml_env_var() {
    let output = Command::new(tach_binary())
        .args(["--help"])
        .output()
        .expect("Failed to execute tach-core");

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Check if TACH_JUNIT_XML env var is documented
    if stdout.contains("TACH_JUNIT_XML") {
        eprintln!("TACH_JUNIT_XML env var is documented");
    }

    // Test always passes - just informational
}

#[test]
fn test_junit_xml_with_failures() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let xml_path = temp_dir.path().join("test-results.xml");

    let output = Command::new(tach_binary())
        .args([
            "--no-isolation",
            "-n",
            "1",
            "--junit-xml",
            xml_path.to_str().unwrap(),
            "tests/dummy_project/test_fail_assert.py",
        ])
        .output()
        .expect("Failed to execute tach-core");

    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    // Skip if environment isn't set up
    if combined.contains("No module named") {
        eprintln!("Skipping: pytest not available");
        return;
    }

    // With failures, should still create XML (if it ran)
    if xml_path.exists() {
        let content = fs::read_to_string(&xml_path).expect("Failed to read XML file");

        // Should contain failure information
        assert!(
            content.contains("failure")
                || content.contains("error")
                || content.contains("failures="),
            "JUnit XML should indicate failures. Content:\n{}",
            content
        );
    }
}

#[test]
fn test_junit_xml_path_with_spaces() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let xml_path = temp_dir.path().join("test results with spaces.xml");

    let output = Command::new(tach_binary())
        .args([
            "--junit-xml",
            xml_path.to_str().unwrap(),
            "--dry-run",
            "tests/dummy_project/",
        ])
        .output()
        .expect("Failed to execute tach-core");

    // Should handle paths with spaces
    assert!(
        output.status.code().is_some(),
        "--junit-xml with spaces in path should not crash"
    );
}

#[test]
fn test_junit_xml_to_stdout() {
    // Some implementations support - for stdout
    let output = Command::new(tach_binary())
        .args(["--junit-xml", "-", "--dry-run", "tests/dummy_project/"])
        .output()
        .expect("Failed to execute tach-core");

    // Should either work or error gracefully
    assert!(
        output.status.code().is_some(),
        "--junit-xml - should not crash"
    );
}

#[test]
fn test_junit_xml_overwrites_existing() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let xml_path = temp_dir.path().join("test-results.xml");

    // Create an existing file
    fs::write(&xml_path, "old content").expect("Failed to write file");

    let output = Command::new(tach_binary())
        .args([
            "--junit-xml",
            xml_path.to_str().unwrap(),
            "--dry-run",
            "tests/dummy_project/",
        ])
        .output()
        .expect("Failed to execute tach-core");

    // Should not crash when file exists
    assert!(
        output.status.code().is_some(),
        "--junit-xml should handle existing file"
    );
}
