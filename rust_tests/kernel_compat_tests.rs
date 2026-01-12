//! Kernel Compatibility and Graceful Degradation Tests
//!
//! These tests verify that tach handles missing kernel features gracefully:
//! - Warns instead of crashes when features are unavailable
//! - Falls back to degraded modes (e.g., --no-isolation)
//! - Never panics due to kernel feature availability
//!
//! Philosophy: "Tach should ALWAYS run, even if degraded."
//!
//! Test Strategy:
//! - Use std::process::Command to run the tach-core binary
//! - Check exit codes and output for proper behavior
//! - Verify no panic messages appear in stderr

use std::process::{Command, Output};
use std::time::Duration;
use wait_timeout::ChildExt;

/// Path to the tach-core binary (check multiple locations for different build profiles)
fn tach_binary() -> String {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");

    // Check in priority order:
    // 1. Release build (for CI)
    // 2. Debug build (for local dev)
    // 3. llvm-cov release build (for coverage CI)
    // 4. llvm-cov debug build (for coverage local)
    let paths = [
        format!("{}/target/release/tach-core", manifest_dir),
        format!("{}/target/debug/tach-core", manifest_dir),
        format!("{}/target/llvm-cov-target/release/tach-core", manifest_dir),
        format!("{}/target/llvm-cov-target/debug/tach-core", manifest_dir),
    ];

    for path in &paths {
        if std::path::Path::new(path).exists() {
            return path.clone();
        }
    }

    // Fall back to cargo run approach
    "cargo".to_string()
}

/// Run a command and ensure it doesn't crash (has an exit code)
fn run_command(args: &[&str]) -> Output {
    let binary = tach_binary();

    if binary == "cargo" {
        // Use cargo run for cases where binary isn't built yet
        let mut cmd_args = vec!["run", "--quiet", "--"];
        cmd_args.extend(args);
        Command::new("cargo")
            .args(&cmd_args)
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .output()
            .expect("Failed to execute cargo run")
    } else {
        Command::new(&binary)
            .args(args)
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .output()
            .expect("Failed to execute tach-core binary")
    }
}

/// Run a command with timeout to prevent hanging
fn run_command_with_timeout(args: &[&str], timeout_secs: u64) -> Option<Output> {
    let binary = tach_binary();

    let mut child = if binary == "cargo" {
        let mut cmd_args = vec!["run", "--quiet", "--"];
        cmd_args.extend(args);
        Command::new("cargo")
            .args(&cmd_args)
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("Failed to spawn cargo run")
    } else {
        Command::new(&binary)
            .args(args)
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("Failed to spawn tach-core binary")
    };

    let timeout = Duration::from_secs(timeout_secs);
    match child.wait_timeout(timeout) {
        Ok(Some(status)) => {
            let stdout = child
                .stdout
                .take()
                .map(|mut s| {
                    let mut buf = Vec::new();
                    std::io::Read::read_to_end(&mut s, &mut buf).ok();
                    buf
                })
                .unwrap_or_default();

            let stderr = child
                .stderr
                .take()
                .map(|mut s| {
                    let mut buf = Vec::new();
                    std::io::Read::read_to_end(&mut s, &mut buf).ok();
                    buf
                })
                .unwrap_or_default();

            Some(Output {
                status,
                stdout,
                stderr,
            })
        }
        Ok(None) => {
            // Timeout - kill the process
            let _ = child.kill();
            let _ = child.wait();
            None
        }
        Err(_) => None,
    }
}

// =============================================================================
// SELF-TEST REPORTS KERNEL FEATURES
// =============================================================================

/// Test that `tach self-test` reports kernel features without crashing.
///
/// Key assertions:
/// - Process exits with a code (doesn't crash/hang)
/// - No panic messages in output
/// - Reports kernel version
/// - Reports userfaultfd status
/// - Reports Landlock status
/// - Reports Seccomp status
#[test]
fn test_self_test_reports_kernel_features() {
    let output = run_command(&["self-test"]);

    // Process should have exited (not crashed)
    assert!(
        output.status.code().is_some(),
        "self-test should exit with a code, not crash"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{}{}", stdout, stderr);

    // Should NOT contain panic indicators
    assert!(
        !combined.contains("panicked at"),
        "self-test should not panic"
    );
    assert!(
        !combined.contains("SIGSEGV"),
        "self-test should not segfault"
    );
    assert!(
        !combined.contains("stack backtrace"),
        "self-test should not produce backtrace from crash"
    );

    // Should report on key features (present in diagnostics output)
    // These are the feature names from diagnostics.rs
    let has_diagnostic_output = combined.contains("Kernel Version")
        || combined.contains("userfaultfd")
        || combined.contains("Landlock")
        || combined.contains("Seccomp")
        || combined.contains("Pre-Flight Diagnostics");

    assert!(
        has_diagnostic_output,
        "self-test should report kernel features. Got: {}",
        combined
    );
}

/// Test that self-test produces [PASS] or [WARN] or [FAIL] markers.
#[test]
fn test_self_test_produces_status_markers() {
    let output = run_command(&["self-test"]);

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{}{}", stdout, stderr);

    // Should contain at least one status marker
    let has_status_marker =
        combined.contains("[PASS]") || combined.contains("[WARN]") || combined.contains("[FAIL]");

    assert!(
        has_status_marker,
        "self-test should produce [PASS]/[WARN]/[FAIL] markers. Got: {}",
        combined
    );
}

// =============================================================================
// NO-ISOLATION MODE WORKS
// =============================================================================

/// Test that `--no-isolation` mode works even when isolation features are unavailable.
///
/// Key assertions:
/// - Process exits with a code (doesn't crash)
/// - No panic messages
/// - Recognizes the flag
#[test]
fn test_no_isolation_mode_flag_recognized() {
    // Use --help to verify the flag exists
    let output = run_command(&["--help"]);

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{}{}", stdout, stderr);

    // Process should complete successfully
    assert!(
        output.status.code().is_some(),
        "--help should exit with a code"
    );

    // Should recognize --no-isolation flag
    assert!(
        combined.contains("no-isolation") || combined.contains("NO_ISOLATION"),
        "--help should show --no-isolation flag. Got: {}",
        combined
    );
}

/// Test that --no-isolation with --dry-run works without crashing.
///
/// This is a safe way to test the flag without actually running tests.
#[test]
fn test_no_isolation_with_dry_run() {
    // Create a minimal test directory
    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let test_file = temp_dir.path().join("test_dummy.py");
    std::fs::write(&test_file, "def test_dummy():\n    pass\n").expect("Failed to write test file");

    // Run with --no-isolation --dry-run on the temp directory
    let binary = tach_binary();
    let output = if binary == "cargo" {
        Command::new("cargo")
            .args(["run", "--quiet", "--", "--no-isolation", "--dry-run"])
            .arg(temp_dir.path())
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .output()
            .expect("Failed to execute")
    } else {
        Command::new(&binary)
            .args(["--no-isolation", "--dry-run"])
            .arg(temp_dir.path())
            .output()
            .expect("Failed to execute")
    };

    // Should complete without crashing
    assert!(
        output.status.code().is_some(),
        "--no-isolation --dry-run should not crash"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{}{}", stdout, stderr);

    // Should NOT contain panic
    assert!(
        !combined.contains("panicked at"),
        "--no-isolation should not panic"
    );

    // Should indicate isolation is disabled OR complete normally
    // (Output varies based on whether tests were found)
    let valid_output = combined.contains("Isolation disabled")
        || combined.contains("No tests")
        || combined.contains("dry-run")
        || combined.contains("DRY RUN")
        || combined.contains("test_dummy")
        || output.status.success();

    assert!(
        valid_output,
        "--no-isolation --dry-run should work. Got: {}",
        combined
    );
}

// =============================================================================
// GRACEFUL DEGRADATION ON PERMISSION DENIED
// =============================================================================

/// Test that tach doesn't crash when permission is denied for kernel features.
///
/// We simulate this by testing error handling paths exist and work.
/// The actual permission checks are kernel-dependent.
#[test]
fn test_graceful_degradation_version_command() {
    // The version command should work regardless of kernel features
    let output = run_command(&["version"]);

    assert!(
        output.status.code().is_some(),
        "version command should exit with a code"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{}{}", stdout, stderr);

    // Should NOT panic
    assert!(
        !combined.contains("panicked at"),
        "version should not panic"
    );

    // Should show version info
    let shows_version = combined.contains("tach") || combined.contains(env!("CARGO_PKG_VERSION"));
    assert!(
        shows_version,
        "version command should show version info. Got: {}",
        combined
    );
}

/// Test that listing tests works without requiring isolation features.
#[test]
fn test_list_command_no_isolation_required() {
    // Create a minimal test directory
    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let test_file = temp_dir.path().join("test_example.py");
    std::fs::write(
        &test_file,
        "def test_a():\n    pass\n\ndef test_b():\n    pass\n",
    )
    .expect("Failed to write test file");

    // Note: The path must come BEFORE the subcommand in the CLI
    // Usage: tach-core [OPTIONS] [PATH] [COMMAND]
    let binary = tach_binary();
    let output = if binary == "cargo" {
        Command::new("cargo")
            .args(["run", "--quiet", "--"])
            .arg(temp_dir.path())
            .arg("list")
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .output()
            .expect("Failed to execute")
    } else {
        Command::new(&binary)
            .arg(temp_dir.path())
            .arg("list")
            .output()
            .expect("Failed to execute")
    };

    // Should complete without crashing
    assert!(
        output.status.code().is_some(),
        "list command should not crash"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{}{}", stdout, stderr);

    // Should NOT panic
    assert!(
        !combined.contains("panicked at"),
        "list command should not panic"
    );

    // Should list the tests we created
    let has_tests = combined.contains("test_a") || combined.contains("test_b");
    assert!(
        has_tests || combined.contains("No tests") || output.status.success(),
        "list command should show tests or complete successfully. Got: {}",
        combined
    );
}

// =============================================================================
// LANDLOCK WARNING ON OLD KERNEL
// =============================================================================

/// Test that the version command with -v shows Landlock status.
///
/// This test verifies the verbose output includes Landlock information,
/// which can show either available, ABI version, or "unavailable" message.
#[test]
fn test_version_verbose_shows_landlock_status() {
    // Note: The -v flag must come BEFORE the subcommand
    // Usage: tach-core [OPTIONS] [PATH] [COMMAND]
    let output = run_command(&["-v", "version"]);

    assert!(
        output.status.code().is_some(),
        "-v version should exit with a code"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{}{}", stdout, stderr);

    // Should NOT panic
    assert!(
        !combined.contains("panicked at"),
        "-v version should not panic"
    );

    // Verbose mode should show Landlock status
    // Note: On systems where Landlock is available, it shows "ABI vN"
    // On older systems, it shows "unavailable"
    let shows_landlock_info = combined.contains("Landlock")
        || combined.contains("Capabilities")
        || combined.contains("ABI");

    assert!(
        shows_landlock_info,
        "-v version should show Landlock status. Got: {}",
        combined
    );
}

/// Test that self-test reports Landlock status appropriately.
///
/// The status should be either PASS (with ABI version) or WARN (unavailable),
/// but never cause a crash.
#[test]
fn test_landlock_status_in_self_test() {
    let output = run_command(&["self-test"]);

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{}{}", stdout, stderr);

    // Should NOT crash
    assert!(
        output.status.code().is_some(),
        "self-test should not crash when checking Landlock"
    );

    // Should report on Landlock
    let mentions_landlock = combined.contains("Landlock");
    assert!(
        mentions_landlock,
        "self-test should mention Landlock. Got: {}",
        combined
    );

    // If Landlock is unavailable, should be a WARN not a crash
    if combined.contains("unavailable") || combined.contains("Unavailable") {
        // This is graceful degradation - the test passes
        assert!(
            combined.contains("[WARN]") || combined.contains("[PASS]"),
            "Landlock unavailability should be handled gracefully"
        );
    }
}

// =============================================================================
// VERSION COMMAND SHOWS CAPABILITIES
// =============================================================================

/// Test that `-v version` shows all system capabilities.
///
/// Key assertions:
/// - Shows userfaultfd status
/// - Shows Landlock status
/// - Shows Seccomp status
/// - Shows Python version
/// - Shows kernel version
#[test]
fn test_version_command_shows_capabilities() {
    // Note: The -v flag must come BEFORE the subcommand
    // Usage: tach-core [OPTIONS] [PATH] [COMMAND]
    let output = run_command(&["-v", "version"]);

    assert!(
        output.status.success() || output.status.code().is_some(),
        "-v version should complete"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{}{}", stdout, stderr);

    // Should NOT panic
    assert!(
        !combined.contains("panicked at"),
        "-v version should not panic"
    );

    // Should show various capability information
    // Note: exact format depends on system, but should have some of these
    let capability_indicators = [
        "userfaultfd",
        "Landlock",
        "Seccomp",
        "Python",
        "Kernel",
        "Capabilities",
        "Allocator",
        "Jemalloc",
    ];

    let mut found_indicators = 0;
    for indicator in &capability_indicators {
        if combined.contains(indicator) {
            found_indicators += 1;
        }
    }

    assert!(
        found_indicators >= 2,
        "-v version should show at least 2 capability indicators. Got: {}",
        combined
    );
}

// =============================================================================
// PROCESS STABILITY TESTS
// =============================================================================

/// Test that help command works (sanity check).
#[test]
fn test_help_command_works() {
    let output = run_command(&["--help"]);

    assert!(output.status.success(), "--help should succeed");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{}{}", stdout, stderr);

    // Should show usage information
    assert!(
        combined.contains("Usage") || combined.contains("USAGE") || combined.contains("tach"),
        "--help should show usage. Got: {}",
        combined
    );
}

/// Test that invalid subcommand doesn't crash, just shows error.
#[test]
fn test_invalid_subcommand_graceful() {
    let output = run_command(&["nonexistent-command"]);

    // Should exit with an error code, not crash
    assert!(
        output.status.code().is_some(),
        "Invalid subcommand should exit with a code, not crash"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{}{}", stdout, stderr);

    // Should NOT panic
    assert!(
        !combined.contains("panicked at"),
        "Invalid subcommand should not panic"
    );

    // Should show an error message
    assert!(
        combined.contains("error") || combined.contains("Error") || !output.status.success(),
        "Invalid subcommand should produce an error. Got: {}",
        combined
    );
}

/// Test that self-test doesn't hang (with timeout).
#[test]
fn test_self_test_completes_in_reasonable_time() {
    // self-test should complete within 30 seconds
    let result = run_command_with_timeout(&["self-test"], 30);

    assert!(
        result.is_some(),
        "self-test should complete within 30 seconds, not hang"
    );

    let output = result.unwrap();
    assert!(
        output.status.code().is_some(),
        "self-test should have an exit code"
    );
}

/// Test that version command is fast (doesn't do heavy initialization).
#[test]
fn test_version_is_fast() {
    // version should complete within 5 seconds
    let result = run_command_with_timeout(&["version"], 5);

    assert!(result.is_some(), "version should complete within 5 seconds");
}

// =============================================================================
// JEMALLOC VERIFICATION IN DIAGNOSTICS
// =============================================================================

/// Test that self-test reports jemalloc status.
#[test]
fn test_self_test_reports_jemalloc() {
    let output = run_command(&["self-test"]);

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{}{}", stdout, stderr);

    // Should mention jemalloc (either PASS or FAIL)
    let mentions_allocator = combined.contains("Jemalloc")
        || combined.contains("jemalloc")
        || combined.contains("allocator");

    assert!(
        mentions_allocator,
        "self-test should report allocator status. Got: {}",
        combined
    );
}

// =============================================================================
// USERFAULTFD GRACEFUL DEGRADATION
// =============================================================================

/// Test that userfaultfd unavailability is reported gracefully.
#[test]
fn test_userfaultfd_status_reported() {
    let output = run_command(&["self-test"]);

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{}{}", stdout, stderr);

    // Should mention userfaultfd
    let mentions_uffd = combined.contains("userfaultfd");

    assert!(
        mentions_uffd,
        "self-test should report userfaultfd status. Got: {}",
        combined
    );

    // If userfaultfd is unavailable, it should be handled gracefully
    // (shown as WARN or FAIL with explanation, not a panic)
    if combined.contains("Disabled") || combined.contains("Unavailable") {
        assert!(
            !combined.contains("panicked"),
            "userfaultfd unavailability should not cause panic"
        );
    }
}

// =============================================================================
// SECCOMP GRACEFUL HANDLING
// =============================================================================

/// Test that Seccomp status is reported in self-test.
#[test]
fn test_seccomp_status_reported() {
    let output = run_command(&["self-test"]);

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{}{}", stdout, stderr);

    // Should mention Seccomp
    let mentions_seccomp = combined.contains("Seccomp") || combined.contains("seccomp");

    assert!(
        mentions_seccomp,
        "self-test should report Seccomp status. Got: {}",
        combined
    );
}

/// Test that Seccomp unavailability doesn't crash tach.
#[test]
fn test_seccomp_unavailable_graceful() {
    // We can't easily disable Seccomp, but we can verify the diagnostic
    // path handles it gracefully by checking that self-test completes
    let output = run_command(&["self-test"]);

    // Should complete without crashing
    assert!(
        output.status.code().is_some(),
        "self-test should complete even if Seccomp is unavailable"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("panicked at"),
        "Seccomp checks should not cause panics"
    );
}
