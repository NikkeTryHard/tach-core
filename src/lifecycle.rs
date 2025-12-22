//! Lifecycle Management for Industrial-Grade Cleanup
//!
//! Implements the "Reaper Architecture" - a defense-in-depth cleanup strategy
//! that guarantees resources are released on any exit path (return, panic, signal).
//!
//! ## Key Features
//!
//! - **CleanupGuard**: RAII struct that cleans up on Drop
//! - **IS_DEBUGGING**: Global flag for signal routing
//! - **Mutex Poison Immunity**: Cleanup works even after panic-while-holding-lock

use nix::sys::signal::{kill, Signal};
use nix::unistd::Pid;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

/// Global flag to track if we're in debugging mode
/// Used by signal handler to decide behavior:
/// - Debug mode: Forward SIGINT to worker (TTY proxy handles)
/// - Normal mode: Initiate graceful shutdown
pub static IS_DEBUGGING: AtomicBool = AtomicBool::new(false);

/// Resource tracker for guaranteed cleanup
///
/// Uses RAII pattern - Drop is called on any exit path
/// (normal return, early return, panic).
///
/// ## Boss Refinement: Mutex Poison Handling
///
/// If the application panics while holding a lock, the mutex becomes
/// "poisoned". During cleanup, we MUST ignore poison status and force
/// access to the PIDs - we're crashing anyway, just kill the workers.
pub struct CleanupGuard {
    /// PIDs of all spawned worker processes
    worker_pids: Mutex<Vec<i32>>,
    /// Socket paths to cleanup (debug server)
    socket_paths: Mutex<Vec<PathBuf>>,
    /// The Zygote PID for explicit cleanup
    zygote_pid: Mutex<Option<i32>>,
}

impl CleanupGuard {
    /// Create a new cleanup guard
    pub fn new() -> Self {
        Self {
            worker_pids: Mutex::new(Vec::new()),
            socket_paths: Mutex::new(Vec::new()),
            zygote_pid: Mutex::new(None),
        }
    }

    /// Track the Zygote PID
    pub fn set_zygote_pid(&self, pid: i32) {
        // BOSS REFINEMENT: Ignore mutex poison
        let mut guard = self.zygote_pid.lock().unwrap_or_else(|e| e.into_inner());
        *guard = Some(pid);
    }

    /// Track a worker PID for cleanup
    pub fn track_worker(&self, pid: i32) {
        // BOSS REFINEMENT: Ignore mutex poison
        let mut pids = self.worker_pids.lock().unwrap_or_else(|e| e.into_inner());
        pids.push(pid);
    }

    /// Remove a worker PID (completed successfully)
    pub fn untrack_worker(&self, pid: i32) {
        // BOSS REFINEMENT: Ignore mutex poison
        let mut pids = self.worker_pids.lock().unwrap_or_else(|e| e.into_inner());
        pids.retain(|&p| p != pid);
    }

    /// Track a socket path for cleanup
    pub fn track_socket(&self, path: PathBuf) {
        // BOSS REFINEMENT: Ignore mutex poison
        let mut sockets = self.socket_paths.lock().unwrap_or_else(|e| e.into_inner());
        sockets.push(path);
    }

    /// Get a clone of worker PIDs (for debug session pausing)
    pub fn get_worker_pids(&self) -> Vec<i32> {
        // BOSS REFINEMENT: Ignore mutex poison
        let pids = self.worker_pids.lock().unwrap_or_else(|e| e.into_inner());
        pids.clone()
    }

    /// Force kill all tracked workers
    fn kill_workers(&self) {
        // BOSS REFINEMENT: Ignore mutex poison - we MUST kill workers even after panic
        let pids = self.worker_pids.lock().unwrap_or_else(|e| e.into_inner());

        for &pid in pids.iter() {
            if pid > 0 {
                // Try to kill entire process group first (catches any children)
                let _ = kill(Pid::from_raw(-pid), Signal::SIGKILL);
                // Also kill the process directly
                let _ = kill(Pid::from_raw(pid), Signal::SIGKILL);
            }
        }

        // Kill the Zygote too
        let zygote = self.zygote_pid.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(pid) = *zygote {
            if pid > 0 {
                let _ = kill(Pid::from_raw(pid), Signal::SIGKILL);
            }
        }
    }

    /// Remove socket files
    fn cleanup_sockets(&self) {
        // BOSS REFINEMENT: Ignore mutex poison
        let sockets = self.socket_paths.lock().unwrap_or_else(|e| e.into_inner());
        for path in sockets.iter() {
            let _ = std::fs::remove_file(path);
        }
    }
}

impl Drop for CleanupGuard {
    fn drop(&mut self) {
        // Order matters: kill processes first, then cleanup files
        // Note: Mount cleanup is NOT needed per boss - worker namespaces auto-destroy

        // 1. Kill all workers (they hold resources)
        self.kill_workers();

        // 2. Remove socket files
        self.cleanup_sockets();

        eprintln!("[tach] Cleanup: Resources released.");
    }
}

impl Default for CleanupGuard {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Unit Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cleanup_guard_creation() {
        let guard = CleanupGuard::new();
        assert_eq!(guard.get_worker_pids().len(), 0);
    }

    #[test]
    fn test_track_untrack_worker() {
        let guard = CleanupGuard::new();
        guard.track_worker(1234);
        guard.track_worker(5678);
        assert_eq!(guard.get_worker_pids().len(), 2);

        guard.untrack_worker(1234);
        assert_eq!(guard.get_worker_pids().len(), 1);
        assert_eq!(guard.get_worker_pids()[0], 5678);
    }

    #[test]
    fn test_is_debugging_flag() {
        assert!(!IS_DEBUGGING.load(Ordering::SeqCst));
        IS_DEBUGGING.store(true, Ordering::SeqCst);
        assert!(IS_DEBUGGING.load(Ordering::SeqCst));
        IS_DEBUGGING.store(false, Ordering::SeqCst);
    }

    #[test]
    fn test_default_trait() {
        let guard = CleanupGuard::default();
        assert_eq!(guard.get_worker_pids().len(), 0);
    }

    #[test]
    fn test_track_socket() {
        let guard = CleanupGuard::new();
        guard.track_socket(PathBuf::from("/tmp/test.sock"));
        guard.track_socket(PathBuf::from("/tmp/test2.sock"));
        // We can't directly access socket_paths, but the operation should not panic
    }

    #[test]
    fn test_set_zygote_pid() {
        let guard = CleanupGuard::new();
        guard.set_zygote_pid(9999);
        // The PID is tracked internally
    }

    #[test]
    fn test_track_multiple_workers() {
        let guard = CleanupGuard::new();
        for i in 0..100 {
            guard.track_worker(i);
        }
        assert_eq!(guard.get_worker_pids().len(), 100);
    }

    #[test]
    fn test_untrack_nonexistent_worker() {
        let guard = CleanupGuard::new();
        guard.track_worker(1);
        guard.untrack_worker(999); // doesn't exist
        assert_eq!(guard.get_worker_pids().len(), 1);
    }
}
