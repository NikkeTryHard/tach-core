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

use crate::lifecycle::IS_DEBUGGING;
use signal_hook::consts::{SIGINT, SIGQUIT, SIGTERM};
use signal_hook::iterator::Signals;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

/// Global flag to signal shutdown was requested
pub static SHUTDOWN_REQUESTED: AtomicBool = AtomicBool::new(false);

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

// =============================================================================
// Unit Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shutdown_flag() {
        assert!(!SHUTDOWN_REQUESTED.load(Ordering::SeqCst));
        SHUTDOWN_REQUESTED.store(true, Ordering::SeqCst);
        assert!(shutdown_requested());
        SHUTDOWN_REQUESTED.store(false, Ordering::SeqCst);
    }
}
