//! Exit Code Regression Tests
//!
//! These tests verify that CLI exit codes are stable and correct.
//! Exit codes are part of the public API for CI integration.
//!
//! Exit Code Contract:
//! - 0: Success (tests passed, command completed successfully)
//! - 1: Failure (tests failed, command failed)
//! - 2: CLI usage error (invalid arguments)
//!
//! Note: Tests that actually run Python tests require pytest to be installed.
//! They will be skipped if pytest is not available.

use std::path::PathBuf;
use std::process::Command;

/// Get the path to the tach-core binary
fn tach_binary() -> PathBuf {
    let mut path = std::env::current_exe().unwrap();
    path.pop(); // Remove test binary name
    path.pop(); // Remove deps
    path.push("tach-core");
    path
}

/// Get the project root directory
fn project_root() -> PathBuf {
    let mut path = std::env::current_exe().unwrap();
    path.pop(); // Remove test binary name
    path.pop(); // Remove deps
    path.pop(); // Remove debug/release
    path.pop(); // Remove target
    path
}

/// Get a test directory that contains only passing tests
fn passing_tests_dir() -> PathBuf {
    // gauntlet_phase1 contains tests that all pass
    project_root().join("tests/gauntlet_phase1")
}

/// Get a test directory that contains failing tests
fn failing_tests_dir() -> PathBuf {
    // pytest_compat/sample_tests contains tests that intentionally fail
    project_root().join("tests/regression/pytest_compat/sample_tests")
}

/// Check if pytest is available in the Python environment
fn pytest_available() -> bool {
    // Check if pytest is importable by running Python directly
    // This is more reliable than trying to parse tach-core output
    let output = Command::new("python3")
        .args(["-c", "import pytest"])
        .output();

    match output {
        Ok(out) => out.status.success(),
        Err(_) => {
            // Also try "python" if "python3" doesn't exist
            match Command::new("python")
                .args(["-c", "import pytest"])
                .output()
            {
                Ok(out) => out.status.success(),
                Err(_) => false,
            }
        }
    }
}

// =============================================================================
// Exit Code Tests
// =============================================================================

#[test]
fn test_exit_code_success_on_passing_tests() {
    // Skip if pytest is not available
    if !pytest_available() {
        eprintln!("Skipping test: pytest is not available in the Python environment");
        return;
    }

    let binary = tach_binary();
    let tests_dir = passing_tests_dir();

    // Skip test if tests directory doesn't exist
    if !tests_dir.exists() {
        eprintln!("Skipping test: {} does not exist", tests_dir.display());
        return;
    }

    let output = Command::new(&binary)
        .arg("--no-isolation")
        .arg("-n")
        .arg("1")
        .arg(&tests_dir)
        .current_dir(project_root())
        .output()
        .expect("Failed to execute tach-core");

    // Print output for debugging if test fails
    if !output.status.success() {
        eprintln!("stdout: {}", String::from_utf8_lossy(&output.stdout));
        eprintln!("stderr: {}", String::from_utf8_lossy(&output.stderr));
    }

    assert!(
        output.status.success(),
        "Expected exit code 0 for passing tests, got {:?}",
        output.status.code()
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "Expected exit code 0 for passing tests"
    );
}

#[test]
fn test_exit_code_failure_on_failing_tests() {
    // Skip if pytest is not available
    if !pytest_available() {
        eprintln!("Skipping test: pytest is not available in the Python environment");
        return;
    }

    let binary = tach_binary();
    let tests_dir = failing_tests_dir();

    // Skip test if tests directory doesn't exist
    if !tests_dir.exists() {
        eprintln!("Skipping test: {} does not exist", tests_dir.display());
        return;
    }

    let output = Command::new(&binary)
        .arg("--no-isolation")
        .arg("--no-fallback")
        .arg("-n")
        .arg("1")
        .arg(&tests_dir)
        .current_dir(project_root())
        .output()
        .expect("Failed to execute tach-core");

    eprintln!("stdout: {}", String::from_utf8_lossy(&output.stdout));
    eprintln!("stderr: {}", String::from_utf8_lossy(&output.stderr));

    assert!(
        !output.status.success(),
        "Expected non-zero exit code for failing tests"
    );
    assert_eq!(
        output.status.code(),
        Some(1),
        "Expected exit code 1 for failing tests"
    );
}

#[test]
fn test_exit_code_on_no_tests_found() {
    let binary = tach_binary();

    // Use a path that exists but has no test files
    let empty_path = project_root().join("docs");

    // Skip test if docs directory doesn't exist
    if !empty_path.exists() {
        eprintln!("Skipping test: {} does not exist", empty_path.display());
        return;
    }

    let output = Command::new(&binary)
        .arg("--no-isolation")
        .arg("-n")
        .arg("1")
        .arg(&empty_path)
        .current_dir(project_root())
        .output()
        .expect("Failed to execute tach-core");

    // When no tests are found, the behavior should be consistent
    eprintln!("stdout: {}", String::from_utf8_lossy(&output.stdout));
    eprintln!("stderr: {}", String::from_utf8_lossy(&output.stderr));

    // Document current behavior: exit 0 when no tests found (like pytest default)
    // This is different from pytest --strict which would fail
    assert_eq!(
        output.status.code(),
        Some(5),
        "Expected exit code 5 when no tests collected (pytest ExitCode.NO_TESTS_COLLECTED)"
    );
}

#[test]
fn test_exit_code_version_command() {
    let binary = tach_binary();

    let output = Command::new(&binary)
        .arg("version")
        .output()
        .expect("Failed to execute tach-core version");

    eprintln!("stdout: {}", String::from_utf8_lossy(&output.stdout));
    eprintln!("stderr: {}", String::from_utf8_lossy(&output.stderr));

    assert!(
        output.status.success(),
        "Expected exit code 0 for version command"
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "Expected exit code 0 for version command"
    );

    // Verify version output contains version info
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("tach") || stderr.contains("0."),
        "Version output should contain version info"
    );
}

#[test]
fn test_exit_code_self_test_command() {
    let binary = tach_binary();

    let output = Command::new(&binary)
        .arg("self-test")
        .output()
        .expect("Failed to execute tach-core self-test");

    eprintln!("stdout: {}", String::from_utf8_lossy(&output.stdout));
    eprintln!("stderr: {}", String::from_utf8_lossy(&output.stderr));

    // self-test may return 0 or 1 depending on system capabilities
    // We just verify it completes and returns a valid exit code
    let exit_code = output.status.code();
    assert!(
        exit_code == Some(0) || exit_code == Some(1),
        "Expected exit code 0 or 1 for self-test command, got {:?}",
        exit_code
    );
}

#[test]
fn test_exit_code_list_command() {
    let binary = tach_binary();
    let tests_dir = passing_tests_dir();

    // Skip test if tests directory doesn't exist
    if !tests_dir.exists() {
        eprintln!("Skipping test: {} does not exist", tests_dir.display());
        return;
    }

    let output = Command::new(&binary)
        .arg("list")
        .current_dir(&tests_dir)
        .output()
        .expect("Failed to execute tach-core list");

    eprintln!("stdout: {}", String::from_utf8_lossy(&output.stdout));
    eprintln!("stderr: {}", String::from_utf8_lossy(&output.stderr));

    assert!(
        output.status.success(),
        "Expected exit code 0 for list command"
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "Expected exit code 0 for list command"
    );

    // Verify list output contains test names (from gauntlet_phase1)
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("test_"),
        "List output should contain discovered test names"
    );
}

#[test]
fn test_exit_code_invalid_argument() {
    let binary = tach_binary();

    let output = Command::new(&binary)
        .arg("--this-flag-does-not-exist")
        .output()
        .expect("Failed to execute tach-core with invalid argument");

    eprintln!("stdout: {}", String::from_utf8_lossy(&output.stdout));
    eprintln!("stderr: {}", String::from_utf8_lossy(&output.stderr));

    assert!(
        !output.status.success(),
        "Expected non-zero exit code for invalid argument"
    );

    // clap typically returns exit code 2 for usage errors
    let exit_code = output.status.code();
    assert!(
        exit_code == Some(2) || exit_code == Some(1),
        "Expected exit code 1 or 2 for invalid argument, got {:?}",
        exit_code
    );
}

// =============================================================================
// Additional Edge Case Tests
// =============================================================================

#[test]
fn test_exit_code_help_flag() {
    let binary = tach_binary();

    let output = Command::new(&binary)
        .arg("--help")
        .output()
        .expect("Failed to execute tach-core --help");

    assert!(
        output.status.success(),
        "Expected exit code 0 for --help flag"
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "Expected exit code 0 for --help flag"
    );

    // Verify help output contains usage info
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Usage") || stdout.contains("tach"),
        "Help output should contain usage information"
    );
}

#[test]
fn test_exit_code_dry_run() {
    let binary = tach_binary();
    let tests_dir = passing_tests_dir();

    // Skip test if tests directory doesn't exist
    if !tests_dir.exists() {
        eprintln!("Skipping test: {} does not exist", tests_dir.display());
        return;
    }

    let output = Command::new(&binary)
        .arg("--dry-run")
        .arg("--no-isolation")
        .arg(&tests_dir)
        .current_dir(project_root())
        .output()
        .expect("Failed to execute tach-core --dry-run");

    eprintln!("stdout: {}", String::from_utf8_lossy(&output.stdout));
    eprintln!("stderr: {}", String::from_utf8_lossy(&output.stderr));

    assert!(
        output.status.success(),
        "Expected exit code 0 for dry-run command"
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "Expected exit code 0 for dry-run command"
    );
}

#[test]
fn test_exit_code_collect_only() {
    let binary = tach_binary();
    let tests_dir = passing_tests_dir();

    // Skip test if tests directory doesn't exist
    if !tests_dir.exists() {
        eprintln!("Skipping test: {} does not exist", tests_dir.display());
        return;
    }

    let output = Command::new(&binary)
        .arg("--collect-only")
        .arg("--no-isolation")
        .current_dir(&tests_dir)
        .output()
        .expect("Failed to execute tach-core --collect-only");

    eprintln!("stdout: {}", String::from_utf8_lossy(&output.stdout));
    eprintln!("stderr: {}", String::from_utf8_lossy(&output.stderr));

    assert!(
        output.status.success(),
        "Expected exit code 0 for --collect-only flag"
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "Expected exit code 0 for --collect-only flag"
    );
}

#[test]
fn test_exit_code_nonexistent_path() {
    let binary = tach_binary();

    let output = Command::new(&binary)
        .arg("--no-isolation")
        .arg("-n")
        .arg("1")
        .arg("/nonexistent/path/that/does/not/exist")
        .current_dir(project_root())
        .output()
        .expect("Failed to execute tach-core with nonexistent path");

    eprintln!("stdout: {}", String::from_utf8_lossy(&output.stdout));
    eprintln!("stderr: {}", String::from_utf8_lossy(&output.stderr));

    // Behavior for nonexistent path: either error (1) or no tests found (0)
    // This documents the current behavior
    let exit_code = output.status.code();
    assert!(
        exit_code == Some(5) || exit_code == Some(1),
        "Expected exit code 5 (no tests) or 1 (error) for nonexistent path, got {:?}",
        exit_code
    );
}
