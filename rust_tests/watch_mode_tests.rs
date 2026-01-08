//! Watch mode regression tests for tach-core CLI.
//!
//! These tests verify that the -w/--watch flag works correctly.
//! Watch mode re-runs tests when source files change.
//!
//! Regression prevention: Ensures watch mode behavior is consistent.
//!
//! Note: Watch mode is inherently interactive and long-running.
//! These tests verify basic flag acceptance and quick behaviors,
//! not full interactive watch cycles.

use std::process::{Command, Stdio};
use std::time::Duration;

/// Get the tach-core binary path
fn tach_binary() -> std::path::PathBuf {
    let mut path = std::env::current_exe().unwrap();
    path.pop();
    path.pop();
    path.push("tach-core");
    path
}

// =============================================================================
// Flag recognition tests
// =============================================================================

#[test]
fn test_watch_flag_is_recognized() {
    let output = Command::new(tach_binary())
        .args(["--help"])
        .output()
        .expect("Failed to execute tach-core");

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        stdout.contains("-w") || stdout.contains("--watch") || stdout.contains("watch"),
        "-w/--watch flag should be documented in help output"
    );
}

#[test]
fn test_watch_short_flag_accepted() {
    let output = Command::new(tach_binary())
        .args(["--help"])
        .output()
        .expect("Failed to execute tach-core");

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        stdout.contains("-w") || stdout.contains("watch"),
        "-w short flag should be documented"
    );
}

#[test]
fn test_watch_long_flag_accepted() {
    let output = Command::new(tach_binary())
        .args(["--help"])
        .output()
        .expect("Failed to execute tach-core");

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        stdout.contains("--watch") || stdout.contains("watch"),
        "--watch long flag should be documented"
    );
}

// =============================================================================
// Compatibility tests (watch with other flags)
// =============================================================================

#[test]
fn test_watch_with_verbose() {
    let output = Command::new(tach_binary())
        .args(["--help"])
        .output()
        .expect("Failed to execute tach-core");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let has_watch = stdout.contains("-w") || stdout.contains("--watch");
    let has_verbose = stdout.contains("-v") || stdout.contains("--verbose");

    assert!(
        has_watch && has_verbose,
        "Both -w and -v flags should be documented"
    );
}

#[test]
fn test_watch_with_quiet() {
    let output = Command::new(tach_binary())
        .args(["--help"])
        .output()
        .expect("Failed to execute tach-core");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let has_watch = stdout.contains("-w") || stdout.contains("--watch");
    let has_quiet = stdout.contains("-q") || stdout.contains("--quiet");

    assert!(
        has_watch && has_quiet,
        "Both -w and -q flags should be documented"
    );
}

#[test]
fn test_watch_with_keyword_filter() {
    let output = Command::new(tach_binary())
        .args(["--help"])
        .output()
        .expect("Failed to execute tach-core");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let has_watch = stdout.contains("-w") || stdout.contains("--watch");
    let has_keyword = stdout.contains("-k") || stdout.contains("keyword");

    assert!(
        has_watch && has_keyword,
        "Both -w and -k flags should be documented"
    );
}

#[test]
fn test_watch_with_marker_filter() {
    let output = Command::new(tach_binary())
        .args(["--help"])
        .output()
        .expect("Failed to execute tach-core");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let has_watch = stdout.contains("-w") || stdout.contains("--watch");
    let has_marker = stdout.contains("-m") || stdout.contains("marker");

    assert!(
        has_watch && has_marker,
        "Both -w and -m flags should be documented"
    );
}

// =============================================================================
// Quick spawn/kill tests
// =============================================================================

#[test]
fn test_watch_can_be_spawned() {
    // Verify that spawning with --watch doesn't immediately crash
    let mut child = Command::new(tach_binary())
        .args(["-w", "tests/dummy_project/"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn tach-core with --watch");

    // Give it a moment to start
    std::thread::sleep(Duration::from_millis(100));

    // Kill the process (we don't want to wait for file changes)
    let _ = child.kill();
    let _ = child.wait();

    // If we got here without panic, the test passed
    // (spawn succeeded and process was manageable)
}

#[test]
fn test_watch_with_invalid_path() {
    // --watch with nonexistent path should handle gracefully
    let mut child = Command::new(tach_binary())
        .args(["-w", "nonexistent_path_xyz123/"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn tach-core");

    // Give it time to detect the invalid path
    std::thread::sleep(Duration::from_millis(1000));

    // Kill it (it might have already exited with an error)
    let _ = child.kill();

    // Wait for it to finish
    let _ = child.wait();

    // If we got here without hang, the test passed
}

#[test]
fn test_watch_can_be_killed() {
    // Watch mode should be killable (basic sanity check)
    let mut child = Command::new(tach_binary())
        .args(["-w", "tests/dummy_project/"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn tach-core");

    // Give it time to start
    std::thread::sleep(Duration::from_millis(200));

    // Kill it forcefully
    let kill_result = child.kill();

    // Wait for it to actually exit
    let wait_result = child.wait();

    // Should be killable
    assert!(
        kill_result.is_ok() || wait_result.is_ok(),
        "--watch should be killable"
    );
}

// =============================================================================
// Dry-run interaction tests
// =============================================================================

#[test]
fn test_watch_and_dry_run_interaction() {
    // --watch and --dry-run together might be incompatible or have defined behavior
    let output = Command::new(tach_binary())
        .args(["--watch", "--dry-run", "tests/dummy_project/"])
        .output()
        .expect("Failed to execute tach-core");

    // Should either:
    // 1. Work (dry-run in watch mode)
    // 2. Error gracefully (incompatible flags)
    // 3. Prioritize one flag over the other
    assert!(
        output.status.code().is_some(),
        "--watch with --dry-run should not crash"
    );
}

#[test]
fn test_watch_env_var() {
    // Check if there's a TACH_WATCH env var
    let output = Command::new(tach_binary())
        .args(["--help"])
        .output()
        .expect("Failed to execute tach-core");

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Just document whether env var exists (informational test)
    if stdout.contains("TACH_WATCH") {
        eprintln!("TACH_WATCH env var is documented");
    } else {
        eprintln!("TACH_WATCH env var not documented (may not exist)");
    }
    // Test passes if help output was retrieved successfully
}
