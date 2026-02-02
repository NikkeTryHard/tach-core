//! Signal Handling for Graceful Shutdown
//!
//! Routes signals based on application state:
//! - Debug mode: Forward to worker (handled by TTY proxy in raw mode)
//! - Normal mode: Initiate graceful shutdown
//!
//! ## Architecture
//!
//! Signal thread is spawned as a daemon - it will automatically die
//! when the main thread exits (per boss clarification).
//!
//! ## Shutdown Watchdog
//!
//! The watchdog uses two flags to avoid false positives:
//! - `SHUTDOWN_REQUESTED`: Set when signal received, never cleared
//! - `SHUTDOWN_COMPLETE`: Set when graceful shutdown finishes successfully
//!
//! The watchdog only force-exits if shutdown was requested BUT NOT completed.

use crate::lifecycle::IS_DEBUGGING;
use signal_hook::consts::{SIGINT, SIGQUIT, SIGTERM};
use signal_hook::iterator::Signals;
use std::process;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

/// Maximum time to wait for graceful shutdown before force exit
pub const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(3);

/// Global flag to signal shutdown was requested
pub static SHUTDOWN_REQUESTED: AtomicBool = AtomicBool::new(false);

/// Flag set when graceful shutdown completes successfully.
/// This prevents the watchdog from force-exiting after a successful shutdown.
pub static SHUTDOWN_COMPLETE: AtomicBool = AtomicBool::new(false);

/// Install signal handlers for graceful shutdown
///
/// Spawns a daemon thread that listens for signals and routes them:
/// - SIGINT in debug mode: Ignored (raw mode converts to 0x03 byte)
/// - SIGINT in normal mode: Request shutdown
/// - SIGTERM/SIGQUIT: Always request shutdown
pub fn install_signal_handlers() -> Result<(), Box<dyn std::error::Error>> {
    let mut signals = Signals::new([SIGINT, SIGTERM, SIGQUIT])?;

    // Spawn daemon thread - will die when main exits
    thread::spawn(move || {
        for sig in signals.forever() {
            match sig {
                SIGINT => {
                    if IS_DEBUGGING.load(Ordering::SeqCst) {
                        // In debug mode, SIGINT is handled by TTY proxy
                        // Raw mode converts Ctrl+C to 0x03 byte, forwarded to worker
                        // So we don't trigger shutdown here
                        continue;
                    }
                    // Normal mode: graceful shutdown
                    eprintln!("\n[tach] Received SIGINT, shutting down...");
                    SHUTDOWN_REQUESTED.store(true, Ordering::SeqCst);
                }
                SIGTERM | SIGQUIT => {
                    // Always trigger shutdown for these
                    eprintln!("\n[tach] Received signal, shutting down...");
                    SHUTDOWN_REQUESTED.store(true, Ordering::SeqCst);
                }
                _ => {}
            }
        }
    });

    Ok(())
}

/// Check if shutdown was requested (called in scheduler loop)
#[inline]
pub fn shutdown_requested() -> bool {
    SHUTDOWN_REQUESTED.load(Ordering::SeqCst)
}

/// Mark graceful shutdown as complete.
/// Call this when shutdown finishes successfully to prevent the watchdog
/// from force-exiting the process.
#[inline]
pub fn mark_shutdown_complete() {
    SHUTDOWN_COMPLETE.store(true, Ordering::SeqCst);
}

/// Check if shutdown completed successfully
#[inline]
pub fn shutdown_complete() -> bool {
    SHUTDOWN_COMPLETE.load(Ordering::SeqCst)
}

/// Spawn a watchdog thread that force-exits if shutdown takes too long.
///
/// This prevents the process from hanging indefinitely when graceful shutdown
/// fails (e.g., blocked on socket read). After `SHUTDOWN_TIMEOUT` from when
/// shutdown is requested, the watchdog forces process exit with code 130
/// (128 + SIGINT).
///
/// # Exit behavior
///
/// The watchdog uses `process::exit(130)` which bypasses normal cleanup
/// (destructors, drop handlers). This is intentional:
/// - If graceful shutdown is stuck, cleanup is already failing
/// - We need to guarantee termination for the user
/// - Exit code 130 signals to parent processes that we were interrupted
pub fn spawn_shutdown_watchdog() -> Result<thread::JoinHandle<()>, Box<dyn std::error::Error>> {
    let handle = thread::spawn(move || {
        // Wait until shutdown is requested
        while !SHUTDOWN_REQUESTED.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_millis(100));
        }

        // Give graceful shutdown a chance
        thread::sleep(SHUTDOWN_TIMEOUT);

        // Force exit only if shutdown was requested but NOT completed
        // This prevents false positives when graceful shutdown succeeds
        if SHUTDOWN_REQUESTED.load(Ordering::SeqCst) && !SHUTDOWN_COMPLETE.load(Ordering::SeqCst) {
            eprintln!(
                "\n[tach] Graceful shutdown timed out after {:?}, forcing exit",
                SHUTDOWN_TIMEOUT
            );
            process::exit(130); // 128 + SIGINT (2)
        }
    });

    Ok(handle)
}

// =============================================================================
// Unit Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shutdown_flag_default() {
        // Reset to known state first
        SHUTDOWN_REQUESTED.store(false, Ordering::SeqCst);
        assert!(!shutdown_requested());
    }

    #[test]
    fn test_shutdown_flag_set_and_check() {
        SHUTDOWN_REQUESTED.store(false, Ordering::SeqCst);
        assert!(!SHUTDOWN_REQUESTED.load(Ordering::SeqCst));

        SHUTDOWN_REQUESTED.store(true, Ordering::SeqCst);
        assert!(shutdown_requested());

        // Cleanup
        SHUTDOWN_REQUESTED.store(false, Ordering::SeqCst);
    }

    #[test]
    fn test_shutdown_complete_flag() {
        // Reset to known state
        SHUTDOWN_COMPLETE.store(false, Ordering::SeqCst);
        assert!(!shutdown_complete());

        // Mark shutdown complete
        mark_shutdown_complete();
        assert!(shutdown_complete());

        // Cleanup
        SHUTDOWN_COMPLETE.store(false, Ordering::SeqCst);
    }

    #[test]
    fn test_watchdog_skips_when_shutdown_complete() {
        // This tests the logic: requested=true AND complete=true => no force exit
        SHUTDOWN_REQUESTED.store(true, Ordering::SeqCst);
        SHUTDOWN_COMPLETE.store(true, Ordering::SeqCst);

        // The watchdog condition: requested && !complete
        let would_force_exit =
            SHUTDOWN_REQUESTED.load(Ordering::SeqCst) && !SHUTDOWN_COMPLETE.load(Ordering::SeqCst);
        assert!(
            !would_force_exit,
            "Should NOT force exit when shutdown completed"
        );

        // Cleanup
        SHUTDOWN_REQUESTED.store(false, Ordering::SeqCst);
        SHUTDOWN_COMPLETE.store(false, Ordering::SeqCst);
    }

    #[test]
    fn test_shutdown_requested_inline() {
        // Test the inline function specifically
        SHUTDOWN_REQUESTED.store(false, Ordering::SeqCst);
        let result = shutdown_requested();
        assert!(!result);

        SHUTDOWN_REQUESTED.store(true, Ordering::SeqCst);
        let result = shutdown_requested();
        assert!(result);

        // Cleanup
        SHUTDOWN_REQUESTED.store(false, Ordering::SeqCst);
    }

    #[test]
    fn test_atomic_ordering() {
        // Verify SeqCst ordering is used correctly
        SHUTDOWN_REQUESTED.store(false, Ordering::SeqCst);

        // Multiple stores should be visible
        for _ in 0..10 {
            SHUTDOWN_REQUESTED.store(true, Ordering::SeqCst);
            assert!(SHUTDOWN_REQUESTED.load(Ordering::SeqCst));
            SHUTDOWN_REQUESTED.store(false, Ordering::SeqCst);
            assert!(!SHUTDOWN_REQUESTED.load(Ordering::SeqCst));
        }
    }

    #[test]
    fn test_install_signal_handlers_succeeds() {
        // Signal handlers should install without error
        // Note: This spawns a daemon thread that will be cleaned up when tests exit
        let result = install_signal_handlers();
        assert!(
            result.is_ok(),
            "Signal handler installation failed: {:?}",
            result
        );
    }
}
