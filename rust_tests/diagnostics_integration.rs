//! Integration Tests for Diagnostics Module
//!
//! These tests verify the pre-flight diagnostic checks work correctly
//! and produce accurate results on various system configurations.

use tach_core::diagnostics::DiagnosticResult;

// =============================================================================
// DiagnosticResult Tests
// =============================================================================

#[test]
fn test_diagnostic_result_pass() {
    let result = DiagnosticResult::pass("Test Check", "All systems go");

    assert!(result.passed);
    assert_eq!(result.name, "Test Check");
    assert_eq!(result.message, "All systems go");
    assert!(result.required);
    assert!(result.details.is_none());
}

#[test]
fn test_diagnostic_result_fail() {
    let result = DiagnosticResult::fail("Test Check", "Something went wrong");

    assert!(!result.passed);
    assert_eq!(result.name, "Test Check");
    assert_eq!(result.message, "Something went wrong");
    assert!(result.required);
}

#[test]
fn test_diagnostic_result_warn() {
    let result = DiagnosticResult::warn("Optional Check", "Not available");

    assert!(!result.passed);
    assert!(!result.required); // warn creates non-required failures
}

#[test]
fn test_diagnostic_result_with_details() {
    let result = DiagnosticResult::pass("Version Check", "Linux 6.6.87")
        .with_details("Full version: 6.6.87-1-default");

    assert!(result.details.is_some());
    assert!(result.details.as_ref().unwrap().contains("6.6.87"));
}

#[test]
fn test_diagnostic_result_optional() {
    let mut result = DiagnosticResult::pass("Optional Feature", "Not required");
    result.required = false;

    assert!(!result.required);
    assert!(result.passed);
}

// =============================================================================
// Simulated Summary Tests (Using Vec<DiagnosticResult>)
// =============================================================================

fn all_passed(results: &[DiagnosticResult]) -> bool {
    results.iter().all(|r| r.passed)
}

fn pass_count(results: &[DiagnosticResult]) -> usize {
    results.iter().filter(|r| r.passed).count()
}

fn fail_count(results: &[DiagnosticResult]) -> usize {
    results.iter().filter(|r| !r.passed).count()
}

fn required_passed(results: &[DiagnosticResult]) -> bool {
    results.iter().filter(|r| r.required).all(|r| r.passed)
}

fn required_fail_count(results: &[DiagnosticResult]) -> usize {
    results.iter().filter(|r| r.required && !r.passed).count()
}

#[allow(dead_code)]
fn optional_fail_count(results: &[DiagnosticResult]) -> usize {
    results.iter().filter(|r| !r.required && !r.passed).count()
}

#[test]
fn test_diagnostic_summary_all_pass() {
    let results = vec![
        DiagnosticResult::pass("Check 1", "OK"),
        DiagnosticResult::pass("Check 2", "OK"),
        DiagnosticResult::pass("Check 3", "OK"),
    ];

    assert!(all_passed(&results));
    assert_eq!(pass_count(&results), 3);
    assert_eq!(fail_count(&results), 0);
    assert_eq!(results.len(), 3);
}

#[test]
fn test_diagnostic_summary_with_failure() {
    let results = vec![
        DiagnosticResult::pass("Check 1", "OK"),
        DiagnosticResult::fail("Check 2", "Failed"),
        DiagnosticResult::pass("Check 3", "OK"),
    ];

    assert!(!all_passed(&results));
    assert_eq!(pass_count(&results), 2);
    assert_eq!(fail_count(&results), 1);
    assert_eq!(results.len(), 3);
}

#[test]
fn test_diagnostic_summary_optional_failure_still_passes() {
    let results = vec![
        DiagnosticResult::pass("Required 1", "OK"),
        DiagnosticResult::warn("Optional Check", "Not available"),
        DiagnosticResult::pass("Required 2", "OK"),
    ];

    // Optional failures don't block overall success
    assert!(required_passed(&results));
    assert!(!all_passed(&results)); // But all_passed is still false
    assert_eq!(required_fail_count(&results), 0);
}

#[test]
fn test_diagnostic_summary_empty() {
    let results: Vec<DiagnosticResult> = vec![];

    assert!(all_passed(&results)); // Vacuously true
    assert_eq!(results.len(), 0);
}

// =============================================================================
// Diagnostic Check Integration Tests
// =============================================================================

#[test]
fn test_kernel_version_check_format() {
    // Test that kernel version parsing handles various formats
    let versions = vec![
        "5.15.0-generic",
        "6.1.0",
        "6.6.87-microsoft-standard-WSL2",
        "5.15.133.1-microsoft-standard-WSL2",
    ];

    for version in versions {
        // Parse major.minor
        let parts: Vec<&str> = version.split('.').collect();
        assert!(
            parts.len() >= 2,
            "Version should have at least major.minor: {}",
            version
        );

        let major: u32 = parts[0].parse().unwrap_or(0);
        let _minor_str = parts[1]
            .split(|c: char| !c.is_numeric())
            .next()
            .unwrap_or("0");

        // Verify parsing worked
        assert!(major >= 5, "Major version should be >= 5 for {}", version);
    }
}

#[test]
fn test_sysctl_value_parsing() {
    // Test parsing of sysctl values
    let valid_values = vec!["1", "1\n", " 1 ", "1 ", " 1\n"];

    for val in valid_values {
        let parsed: u32 = val.trim().parse().unwrap_or(0);
        assert_eq!(parsed, 1, "Should parse '{}' as 1", val);
    }

    let invalid_values = vec!["0", "2", "abc", ""];
    for val in invalid_values {
        let parsed: u32 = val.trim().parse().unwrap_or(99);
        assert_ne!(parsed, 1, "'{}' should not parse as 1", val);
    }
}

#[test]
fn test_capability_string_parsing() {
    // Test parsing capability bitmask strings
    // CAP_SYS_PTRACE is bit 19, so 1 << 19 = 0x80000
    let cap_strings = vec![
        ("0000003fffffffff", true),  // Full capabilities
        ("0000000000000000", false), // No capabilities
        ("0000000000080000", true),  // CAP_SYS_PTRACE only (bit 19 = 0x80000)
    ];

    for (cap_str, expected_ptrace) in cap_strings {
        let caps = u64::from_str_radix(cap_str, 16).unwrap();
        let has_ptrace = (caps >> 19) & 1 == 1;

        assert_eq!(
            has_ptrace, expected_ptrace,
            "CAP_SYS_PTRACE check failed for {}: expected {}, got {}",
            cap_str, expected_ptrace, has_ptrace
        );
    }
}

// =============================================================================
// Result Chain Tests
// =============================================================================

#[test]
fn test_diagnostic_result_clone() {
    let original = DiagnosticResult::pass("Clone Test", "Testing clone");
    let cloned = original.clone();

    assert_eq!(original.name, cloned.name);
    assert_eq!(original.message, cloned.message);
    assert_eq!(original.passed, cloned.passed);
}

#[test]
fn test_diagnostic_result_debug() {
    let result = DiagnosticResult::fail("Debug Test", "For debugging");
    let debug_str = format!("{:?}", result);

    assert!(debug_str.contains("Debug Test"));
    assert!(debug_str.contains("For debugging"));
    assert!(debug_str.contains("passed: false"));
}

// =============================================================================
// Edge Cases
// =============================================================================

#[test]
fn test_diagnostic_with_unicode() {
    let result = DiagnosticResult::pass("Unicode Test", "Passed \u{2713}");
    assert!(result.message.contains('\u{2713}'));
}

#[test]
fn test_diagnostic_with_long_message() {
    let long_message = "A".repeat(10000);
    let result = DiagnosticResult::pass("Long Message", &long_message);

    assert_eq!(result.message.len(), 10000);
}

#[test]
fn test_diagnostic_with_empty_strings() {
    let result = DiagnosticResult::pass("", "");

    assert!(result.name.is_empty());
    assert!(result.message.is_empty());
    assert!(result.passed);
}

// =============================================================================
// Summary Statistics Tests
// =============================================================================

#[test]
fn test_diagnostic_summary_statistics() {
    let results = vec![
        DiagnosticResult::pass("P1", "OK"),
        DiagnosticResult::pass("P2", "OK"),
        DiagnosticResult::fail("F1", "Failed"),
        DiagnosticResult::warn("O1", "Optional Failed"),
    ];

    assert_eq!(pass_count(&results), 2);
    assert_eq!(fail_count(&results), 2);
    assert_eq!(required_fail_count(&results), 1);
    assert_eq!(optional_fail_count(&results), 1);
}

#[test]
fn test_diagnostic_result_chaining() {
    let result = DiagnosticResult::pass("Chain Test", "OK").with_details("Additional info");

    assert!(result.passed);
    assert!(result.details.is_some());
}

#[test]
fn test_multiple_failures() {
    let results = vec![
        DiagnosticResult::fail("F1", "Failed 1"),
        DiagnosticResult::fail("F2", "Failed 2"),
        DiagnosticResult::fail("F3", "Failed 3"),
    ];

    assert_eq!(fail_count(&results), 3);
    assert!(!required_passed(&results));
}

#[test]
fn test_mixed_results() {
    let results = vec![
        DiagnosticResult::pass("Kernel", "6.6.87"),
        DiagnosticResult::fail("Landlock", "Not supported"),
        DiagnosticResult::warn("Seccomp", "Optional"),
        DiagnosticResult::pass("UFFD", "Enabled"),
    ];

    assert_eq!(pass_count(&results), 2);
    assert_eq!(fail_count(&results), 2);
    assert!(!required_passed(&results));
    assert_eq!(required_fail_count(&results), 1);
}
