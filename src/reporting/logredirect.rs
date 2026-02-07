//! Log Redirection Module
//!
//! Redirects stderr (fd 2) to a log file so that diagnostic `[tach:*]` and
//! `[worker:*]` messages are captured instead of cluttering the terminal.
//! Reporter output is routed through stdout, so it remains visible.
//!
//! Uses `libc::dup` / `libc::dup2` for fd-level redirection.

use std::fs::File;
use std::os::unix::io::{AsRawFd, RawFd};
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// Redirects stderr to a log file for the duration of its lifetime.
///
/// On creation, saves the original stderr fd and redirects fd 2 to a log file.
/// On drop (or explicit `restore()`), restores the original stderr.
pub struct LogRedirect {
    /// Path to the log file
    log_path: PathBuf,
    /// Saved original stderr fd (via dup)
    original_stderr_fd: RawFd,
    /// Whether stderr has already been restored
    restored: bool,
}

impl LogRedirect {
    /// Create a new LogRedirect that captures stderr to `/tmp/tach_<uuid>.log`.
    ///
    /// # Errors
    /// Returns an error if the log file cannot be created or fd operations fail.
    pub fn new() -> std::io::Result<Self> {
        let log_path = PathBuf::from(format!("/tmp/tach_{}.log", Uuid::new_v4()));
        Self::with_path(log_path)
    }

    /// Create a new LogRedirect with a specific log file path.
    ///
    /// Useful for testing with predictable paths.
    fn with_path(log_path: PathBuf) -> std::io::Result<Self> {
        let log_file = File::create(&log_path)?;

        // Save original stderr fd
        // SAFETY: dup(2) duplicates fd 2 (stderr). This is a standard POSIX
        // operation with no memory safety concerns.
        let original_stderr_fd = unsafe { libc::dup(2) };
        if original_stderr_fd < 0 {
            return Err(std::io::Error::last_os_error());
        }

        // Redirect stderr to log file
        // SAFETY: dup2 atomically redirects fd 2 to point at the log file.
        // The log_file fd is valid because File::create succeeded above.
        let result = unsafe { libc::dup2(log_file.as_raw_fd(), 2) };
        if result < 0 {
            // Clean up saved fd on failure
            unsafe { libc::close(original_stderr_fd) };
            return Err(std::io::Error::last_os_error());
        }

        // log_file is dropped here, closing its fd. stderr (fd 2) now owns
        // the file description via dup2.

        Ok(Self {
            log_path,
            original_stderr_fd,
            restored: false,
        })
    }

    /// Return the path to the log file.
    pub fn log_path(&self) -> &Path {
        &self.log_path
    }

    /// Manually restore stderr to its original destination.
    ///
    /// This is idempotent -- calling it multiple times is safe.
    /// After restore, eprintln! will write to the original terminal again.
    pub fn restore(&mut self) {
        if self.restored {
            return;
        }

        // SAFETY: Restores fd 2 to its original destination. original_stderr_fd
        // was obtained from dup(2) in new() and is valid.
        unsafe {
            libc::dup2(self.original_stderr_fd, 2);
            libc::close(self.original_stderr_fd);
        }

        self.restored = true;
    }
}

impl Drop for LogRedirect {
    fn drop(&mut self) {
        self.restore();
    }
}

// =============================================================================
// Unit Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::os::unix::io::FromRawFd;

    #[test]
    fn test_log_file_created() {
        let path = PathBuf::from(format!("/tmp/tach_test_{}.log", Uuid::new_v4()));
        let redirect = LogRedirect::with_path(path.clone()).expect("should create redirect");
        assert!(path.exists(), "log file should exist after creation");

        // Clean up
        drop(redirect);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_log_path_format() {
        let redirect = LogRedirect::new().expect("should create redirect");
        let path = redirect.log_path();
        let path_str = path.to_string_lossy();

        assert!(
            path_str.starts_with("/tmp/tach_"),
            "path should start with /tmp/tach_"
        );
        assert!(path_str.ends_with(".log"), "path should end with .log");

        // Clean up
        let path_owned = path.to_path_buf();
        drop(redirect);
        let _ = std::fs::remove_file(&path_owned);
    }

    #[test]
    fn test_stderr_captured_to_log_file() {
        let path = PathBuf::from(format!("/tmp/tach_test_{}.log", Uuid::new_v4()));
        let mut redirect = LogRedirect::with_path(path.clone()).expect("should create redirect");

        // Write to stderr (fd 2) -- should go to log file
        // Use raw write to avoid buffering issues with eprintln!
        let msg = b"[tach:test] diagnostic message\n";
        unsafe {
            libc::write(2, msg.as_ptr() as *const libc::c_void, msg.len());
        }

        // Also flush via File to ensure write is committed
        let mut log_file = unsafe { File::from_raw_fd(libc::dup(2)) };
        log_file.flush().ok();

        // Restore stderr
        redirect.restore();

        // Read log file content
        let content = std::fs::read_to_string(&path).expect("should read log file");
        assert!(
            content.contains("[tach:test] diagnostic message"),
            "log file should contain the diagnostic message, got: {:?}",
            content
        );

        // Clean up
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_restore_is_idempotent() {
        let path = PathBuf::from(format!("/tmp/tach_test_{}.log", Uuid::new_v4()));
        let mut redirect = LogRedirect::with_path(path.clone()).expect("should create redirect");

        redirect.restore();
        redirect.restore(); // Should not panic or error

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_drop_restores_stderr() {
        let path = PathBuf::from(format!("/tmp/tach_test_{}.log", Uuid::new_v4()));
        {
            let _redirect = LogRedirect::with_path(path.clone()).expect("should create redirect");
            // redirect dropped here
        }

        // After drop, stderr should be restored -- eprintln should work
        // (We can't easily verify terminal output, but we verify no panic)
        eprintln!("[test] stderr restored after drop");

        let _ = std::fs::remove_file(&path);
    }
}
