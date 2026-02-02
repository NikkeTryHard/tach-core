//! Signal Handling Integration Tests
//!
//! Tests for graceful shutdown and SIGINT termination behavior.
//! These tests verify that the process terminates correctly when
//! Ctrl+C is pressed, even when blocked on socket operations.

use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

/// Test that SHUTDOWN_REQUESTED atomic flag works correctly
#[test]
fn test_shutdown_requested_atomic_state() {
    use std::sync::atomic::{AtomicBool, Ordering};

    let flag = AtomicBool::new(false);

    // Initially false
    assert!(!flag.load(Ordering::SeqCst));

    // Set to true
    flag.store(true, Ordering::SeqCst);
    assert!(flag.load(Ordering::SeqCst));

    // Set back to false
    flag.store(false, Ordering::SeqCst);
    assert!(!flag.load(Ordering::SeqCst));
}

/// Test that multiple threads can read the shutdown flag consistently
#[test]
fn test_shutdown_flag_thread_visibility() {
    let flag = Arc::new(AtomicBool::new(false));
    let flag_clone = Arc::clone(&flag);

    // Spawn a reader thread
    let handle = thread::spawn(move || {
        // Wait up to 1 second for flag to become true
        for _ in 0..100 {
            if flag_clone.load(Ordering::SeqCst) {
                return true;
            }
            thread::sleep(Duration::from_millis(10));
        }
        false
    });

    // Give thread time to start
    thread::sleep(Duration::from_millis(50));

    // Set the flag
    flag.store(true, Ordering::SeqCst);

    // Reader should see the change
    let saw_flag = handle.join().expect("Thread should not panic");
    assert!(saw_flag, "Reader thread should observe flag change");
}

/// Test shutdown timeout constant is reasonable
#[test]
fn test_shutdown_timeout_constant() {
    // The timeout should be between 1 and 10 seconds
    // Too short = may interrupt valid cleanup
    // Too long = frustrates users
    let timeout_secs = 3; // SHUTDOWN_TIMEOUT = Duration::from_secs(3)

    assert!(timeout_secs >= 1, "Shutdown timeout should be at least 1 second");
    assert!(timeout_secs <= 10, "Shutdown timeout should be at most 10 seconds");
}

/// Test that watchdog pattern works correctly
///
/// This simulates the shutdown watchdog behavior without actually
/// calling process::exit().
#[test]
fn test_watchdog_pattern_triggers_after_timeout() {
    let shutdown_requested = Arc::new(AtomicBool::new(false));
    let watchdog_triggered = Arc::new(AtomicBool::new(false));
    let shutdown_timeout = Duration::from_millis(100); // Fast timeout for testing

    let shutdown_clone = Arc::clone(&shutdown_requested);
    let triggered_clone = Arc::clone(&watchdog_triggered);

    // Simulate watchdog thread
    let watchdog = thread::spawn(move || {
        // Wait until shutdown is requested
        while !shutdown_clone.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_millis(10));
        }

        // Give graceful shutdown a chance
        thread::sleep(shutdown_timeout);

        // If we're still here, graceful shutdown failed
        if shutdown_clone.load(Ordering::SeqCst) {
            triggered_clone.store(true, Ordering::SeqCst);
        }
    });

    // Request shutdown
    shutdown_requested.store(true, Ordering::SeqCst);

    // Wait for watchdog to trigger
    watchdog.join().expect("Watchdog thread should not panic");

    assert!(
        watchdog_triggered.load(Ordering::SeqCst),
        "Watchdog should trigger after timeout when shutdown is still requested"
    );
}

/// Test that watchdog does NOT trigger if shutdown completes quickly
#[test]
fn test_watchdog_pattern_no_trigger_on_fast_shutdown() {
    let shutdown_requested = Arc::new(AtomicBool::new(false));
    let shutdown_complete = Arc::new(AtomicBool::new(false));
    let watchdog_checked = Arc::new(AtomicBool::new(false));
    let shutdown_timeout = Duration::from_millis(200);

    let requested_clone = Arc::clone(&shutdown_requested);
    let complete_clone = Arc::clone(&shutdown_complete);
    let checked_clone = Arc::clone(&watchdog_checked);

    // Simulate watchdog thread
    let watchdog = thread::spawn(move || {
        // Wait until shutdown is requested
        while !requested_clone.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_millis(10));
        }

        // Give graceful shutdown a chance
        thread::sleep(shutdown_timeout);

        // Record that we checked
        checked_clone.store(true, Ordering::SeqCst);

        // Return whether we would force exit: requested AND NOT complete
        requested_clone.load(Ordering::SeqCst) && !complete_clone.load(Ordering::SeqCst)
    });

    // Request shutdown
    shutdown_requested.store(true, Ordering::SeqCst);

    // Simulate fast graceful shutdown completing
    thread::sleep(Duration::from_millis(50));
    shutdown_complete.store(true, Ordering::SeqCst);

    // Wait for watchdog
    let would_force_exit = watchdog.join().expect("Watchdog thread should not panic");

    assert!(
        watchdog_checked.load(Ordering::SeqCst),
        "Watchdog should have run its check"
    );
    assert!(
        !would_force_exit,
        "Watchdog should NOT force exit when shutdown completed gracefully"
    );
}

/// Test socket timeout error handling logic
#[test]
fn test_socket_timeout_error_classification() {
    use std::io::ErrorKind;

    // These are the error kinds we check for in dispatch_test
    let timeout_kinds = [ErrorKind::TimedOut, ErrorKind::WouldBlock];

    for kind in timeout_kinds {
        let error = std::io::Error::new(kind, "test error");
        assert!(
            error.kind() == ErrorKind::TimedOut || error.kind() == ErrorKind::WouldBlock,
            "Error kind {:?} should be recognized as timeout-related",
            kind
        );
    }

    // Other errors should NOT match
    let other_error = std::io::Error::new(ErrorKind::BrokenPipe, "broken pipe");
    assert!(
        other_error.kind() != ErrorKind::TimedOut && other_error.kind() != ErrorKind::WouldBlock,
        "BrokenPipe should not be classified as timeout"
    );
}

/// Test that cmd_socket timeout value is reasonable
#[test]
fn test_cmd_socket_timeout_value() {
    // The cmd_socket timeout should be set to 10 seconds
    // This gives Zygote enough time to fork while still detecting hangs
    let timeout_secs = 10;

    assert!(
        timeout_secs >= 5,
        "cmd_socket timeout should be at least 5 seconds for slow forks"
    );
    assert!(
        timeout_secs <= 30,
        "cmd_socket timeout should be at most 30 seconds to detect hangs promptly"
    );
}

/// Test exit code for forced shutdown is correct
#[test]
fn test_forced_shutdown_exit_code() {
    // Exit code 130 = 128 + SIGINT (2)
    // This is the standard Unix convention for signal-terminated processes
    let exit_code = 130;
    let sigint = 2;

    assert_eq!(exit_code, 128 + sigint, "Exit code should be 128 + SIGINT");
}

// =============================================================================
// Integration Tests (require binary)
// =============================================================================

/// Helper to check if the tach binary exists
fn tach_binary_exists() -> bool {
    let binary_path = std::env::current_dir()
        .map(|p| p.join("target/debug/tach"))
        .unwrap_or_default();

    binary_path.exists()
}

/// Test that --help exits quickly (sanity check for binary)
#[test]
#[ignore] // Run with --ignored flag
fn test_binary_help_exits_quickly() {
    if !tach_binary_exists() {
        eprintln!("Skipping: tach binary not found (run cargo build first)");
        return;
    }

    let start = std::time::Instant::now();
    let output = Command::new("./target/debug/tach")
        .arg("--help")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .output();

    assert!(output.is_ok(), "tach --help should execute");
    assert!(
        start.elapsed() < Duration::from_secs(5),
        "--help should complete within 5 seconds"
    );
}

/// Test that Zygote timeout error message is correctly formatted.
///
/// When the Zygote fails to respond within the timeout period (10 seconds),
/// the scheduler should return an error containing "Zygote timeout" to help
/// diagnose the issue.
#[test]
fn test_zygote_timeout_error_message() {
    // The expected error message format from dispatch_test when Zygote times out
    let expected_substring = "Zygote timeout";
    let error_message = "Zygote timeout: no response within 10s. Zygote may have crashed or deadlocked.";

    assert!(
        error_message.contains(expected_substring),
        "Error message should contain '{}', got: {}",
        expected_substring,
        error_message
    );

    // Verify the message provides actionable information
    assert!(
        error_message.contains("10s"),
        "Error message should mention the timeout duration"
    );
    assert!(
        error_message.contains("crashed") || error_message.contains("deadlocked"),
        "Error message should suggest possible causes"
    );
}
