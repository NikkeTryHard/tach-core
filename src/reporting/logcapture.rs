//! Log capture system using memfd for non-blocking stdout/stderr capture
//!
//! Design:
//! 1. Supervisor creates memfd per worker slot BEFORE forking Zygote
//! 2. Zygote inherits these FDs (no MFD_CLOEXEC)
//! 3. Workers inherit when Zygote forks them
//! 4. Worker calls dup2(memfd, STDOUT) to redirect
//! 5. Supervisor reads from memfd after test completes

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::ffi::CString;
use std::fs::File;
use std::io::Read;
use std::os::unix::io::{FromRawFd, RawFd};

/// Size of each log buffer (1MB per worker slot)
pub const LOG_BUFFER_SIZE: usize = 1024 * 1024;

/// Manages memory-mapped log buffers for worker output capture
pub struct LogCapture {
    /// Map of slot_id -> raw fd
    fds: HashMap<usize, RawFd>,
    num_slots: usize,
}

impl LogCapture {
    /// Create log capture system with the specified number of slots
    pub fn new(max_slots: usize) -> Result<Self> {
        let mut fds = HashMap::new();

        for slot in 0..max_slots {
            let fd = create_memfd(&format!("tach_log_{}", slot))?;

            // Resize to buffer size
            unsafe {
                if libc::ftruncate(fd, LOG_BUFFER_SIZE as i64) != 0 {
                    return Err(anyhow::anyhow!("ftruncate failed for slot {}", slot));
                }
            }

            fds.insert(slot, fd);
        }

        Ok(Self {
            fds,
            num_slots: max_slots,
        })
    }

    /// Get the file descriptor for a slot
    pub fn get_fd(&self, slot: usize) -> Option<RawFd> {
        self.fds.get(&slot).copied()
    }

    /// Get number of slots
    pub fn slot_count(&self) -> usize {
        self.num_slots
    }

    /// Read and clear logs from a slot
    pub fn read_and_clear(&self, slot: usize) -> Result<String> {
        let fd = *self.fds.get(&slot).context("Invalid slot")?;

        // Seek to beginning
        unsafe {
            libc::lseek(fd, 0, libc::SEEK_SET);
        }

        // Read content using dup'd fd (to not affect position)
        let dup_fd = unsafe { libc::dup(fd) };
        if dup_fd < 0 {
            return Err(anyhow::anyhow!("dup failed"));
        }

        let mut file = unsafe { File::from_raw_fd(dup_fd) };
        let mut content = String::new();
        let _ = file.read_to_string(&mut content);
        // File will close dup_fd on drop, which is fine

        // Truncate to clear and reset for next use
        unsafe {
            libc::ftruncate(fd, 0);
            libc::ftruncate(fd, LOG_BUFFER_SIZE as i64);
        }

        // Trim null bytes and trailing whitespace
        let content = content.trim_end_matches('\0').trim_end().to_string();
        Ok(content)
    }
}

impl Drop for LogCapture {
    fn drop(&mut self) {
        for fd in self.fds.values() {
            unsafe {
                libc::close(*fd);
            }
        }
    }
}

/// Create an anonymous memory file WITHOUT MFD_CLOEXEC (so it survives fork)
fn create_memfd(name: &str) -> Result<RawFd> {
    let c_name = CString::new(name)?;

    // NO MFD_CLOEXEC - fd must be inherited by forked children
    let fd = unsafe { libc::syscall(libc::SYS_memfd_create, c_name.as_ptr(), 0) as RawFd };

    if fd < 0 {
        Err(anyhow::anyhow!(
            "memfd_create failed: {}",
            std::io::Error::last_os_error()
        ))
    } else {
        Ok(fd)
    }
}

/// Redirect stdout/stderr to a file descriptor (called in worker after fork)
pub fn redirect_output(fd: RawFd) -> Result<()> {
    if fd < 0 {
        return Ok(());
    }

    unsafe {
        // Seek to beginning of memfd
        libc::lseek(fd, 0, libc::SEEK_SET);

        // Redirect stdout and stderr
        if libc::dup2(fd, libc::STDOUT_FILENO) < 0 {
            return Err(anyhow::anyhow!(
                "dup2 stdout failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        if libc::dup2(fd, libc::STDERR_FILENO) < 0 {
            return Err(anyhow::anyhow!(
                "dup2 stderr failed: {}",
                std::io::Error::last_os_error()
            ));
        }

        // Make stdout line-buffered using setvbuf
        let stdout_file = libc::fdopen(libc::STDOUT_FILENO, c"w".as_ptr());
        if !stdout_file.is_null() {
            libc::setvbuf(stdout_file, std::ptr::null_mut(), libc::_IOLBF, 0);
        }
    }
    Ok(())
}

// =============================================================================
// Unit Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // LogCapture Creation Tests
    // =========================================================================

    #[test]
    fn test_logcapture_new_single_slot() {
        let capture = LogCapture::new(1).expect("Failed to create LogCapture with 1 slot");
        assert_eq!(capture.slot_count(), 1);
        assert!(capture.get_fd(0).is_some());
        assert!(capture.get_fd(1).is_none());
    }

    #[test]
    fn test_logcapture_new_multiple_slots() {
        let capture = LogCapture::new(4).expect("Failed to create LogCapture with 4 slots");
        assert_eq!(capture.slot_count(), 4);

        for slot in 0..4 {
            assert!(
                capture.get_fd(slot).is_some(),
                "Slot {} should have an fd",
                slot
            );
        }
        assert!(capture.get_fd(4).is_none(), "Slot 4 should not exist");
    }

    #[test]
    fn test_logcapture_new_zero_slots() {
        let capture = LogCapture::new(0).expect("Failed to create LogCapture with 0 slots");
        assert_eq!(capture.slot_count(), 0);
        assert!(capture.get_fd(0).is_none());
    }

    // =========================================================================
    // get_fd Tests
    // =========================================================================

    #[test]
    fn test_get_fd_valid_slot() {
        let capture = LogCapture::new(3).unwrap();

        let fd0 = capture.get_fd(0);
        let fd1 = capture.get_fd(1);
        let fd2 = capture.get_fd(2);

        assert!(fd0.is_some());
        assert!(fd1.is_some());
        assert!(fd2.is_some());

        // Each slot should have a unique fd
        assert_ne!(fd0, fd1);
        assert_ne!(fd1, fd2);
        assert_ne!(fd0, fd2);
    }

    #[test]
    fn test_get_fd_invalid_slot() {
        let capture = LogCapture::new(2).unwrap();

        assert!(capture.get_fd(2).is_none());
        assert!(capture.get_fd(100).is_none());
        assert!(capture.get_fd(usize::MAX).is_none());
    }

    #[test]
    fn test_get_fd_returns_valid_fd() {
        let capture = LogCapture::new(1).unwrap();
        let fd = capture.get_fd(0).unwrap();

        // Valid fds are non-negative
        assert!(fd >= 0, "fd should be non-negative");

        // Verify fd is valid by checking with fcntl
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
        assert!(flags >= 0, "fd should be valid (fcntl should succeed)");
    }

    // =========================================================================
    // slot_count Tests
    // =========================================================================

    #[test]
    fn test_slot_count_matches_requested() {
        for count in [1, 2, 4, 8, 16] {
            let capture = LogCapture::new(count).unwrap();
            assert_eq!(
                capture.slot_count(),
                count,
                "slot_count should match requested count"
            );
        }
    }

    // =========================================================================
    // read_and_clear Tests
    // =========================================================================

    #[test]
    fn test_read_and_clear_empty_buffer() {
        let capture = LogCapture::new(1).unwrap();

        // Reading an empty buffer should return empty string
        let content = capture.read_and_clear(0).unwrap();
        assert!(
            content.is_empty(),
            "Empty buffer should return empty string"
        );
    }

    #[test]
    fn test_read_and_clear_with_content() {
        let capture = LogCapture::new(1).unwrap();
        let fd = capture.get_fd(0).unwrap();

        // Write some content to the memfd
        let test_message = "Hello, test log!";
        unsafe {
            libc::lseek(fd, 0, libc::SEEK_SET);
            libc::write(
                fd,
                test_message.as_ptr() as *const libc::c_void,
                test_message.len(),
            );
        }

        // Read it back
        let content = capture.read_and_clear(0).unwrap();
        assert_eq!(content, test_message);

        // After read_and_clear, buffer should be empty
        let content_after = capture.read_and_clear(0).unwrap();
        assert!(
            content_after.is_empty(),
            "Buffer should be cleared after read_and_clear"
        );
    }

    #[test]
    fn test_read_and_clear_trims_null_bytes() {
        let capture = LogCapture::new(1).unwrap();
        let fd = capture.get_fd(0).unwrap();

        // Write content followed by null bytes
        let test_message = b"test\0\0\0";
        unsafe {
            libc::lseek(fd, 0, libc::SEEK_SET);
            libc::write(
                fd,
                test_message.as_ptr() as *const libc::c_void,
                test_message.len(),
            );
        }

        let content = capture.read_and_clear(0).unwrap();
        assert_eq!(content, "test", "Null bytes should be trimmed");
    }

    #[test]
    fn test_read_and_clear_trims_whitespace() {
        let capture = LogCapture::new(1).unwrap();
        let fd = capture.get_fd(0).unwrap();

        // Write content with trailing whitespace
        let test_message = "test message  \n\n";
        unsafe {
            libc::lseek(fd, 0, libc::SEEK_SET);
            libc::write(
                fd,
                test_message.as_ptr() as *const libc::c_void,
                test_message.len(),
            );
        }

        let content = capture.read_and_clear(0).unwrap();
        assert_eq!(
            content, "test message",
            "Trailing whitespace should be trimmed"
        );
    }

    #[test]
    fn test_read_and_clear_invalid_slot() {
        let capture = LogCapture::new(1).unwrap();

        let result = capture.read_and_clear(999);
        assert!(result.is_err(), "Invalid slot should return error");
    }

    #[test]
    fn test_read_and_clear_multiple_times() {
        let capture = LogCapture::new(1).unwrap();
        let fd = capture.get_fd(0).unwrap();

        // First write and read
        let msg1 = "first message";
        unsafe {
            libc::lseek(fd, 0, libc::SEEK_SET);
            libc::write(fd, msg1.as_ptr() as *const libc::c_void, msg1.len());
        }
        let content1 = capture.read_and_clear(0).unwrap();
        assert_eq!(content1, msg1);

        // Second write and read (buffer was cleared)
        let msg2 = "second message";
        unsafe {
            libc::lseek(fd, 0, libc::SEEK_SET);
            libc::write(fd, msg2.as_ptr() as *const libc::c_void, msg2.len());
        }
        let content2 = capture.read_and_clear(0).unwrap();
        assert_eq!(content2, msg2);
    }

    // =========================================================================
    // redirect_output Tests
    // =========================================================================

    #[test]
    fn test_redirect_output_negative_fd_returns_ok() {
        // Negative fd should return Ok without doing anything
        let result = redirect_output(-1);
        assert!(result.is_ok(), "Negative fd should return Ok");

        let result = redirect_output(-100);
        assert!(result.is_ok(), "Any negative fd should return Ok");
    }

    // =========================================================================
    // create_memfd Tests
    // =========================================================================

    #[test]
    fn test_create_memfd_success() {
        let fd = create_memfd("test_memfd").expect("create_memfd should succeed");
        assert!(fd >= 0, "memfd should have non-negative fd");

        // Verify it's a valid fd
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
        assert!(flags >= 0, "fd should be valid");

        // Clean up
        unsafe {
            libc::close(fd);
        }
    }

    #[test]
    fn test_create_memfd_unique_fds() {
        let fd1 = create_memfd("test1").unwrap();
        let fd2 = create_memfd("test2").unwrap();

        assert_ne!(fd1, fd2, "Each memfd should have a unique fd");

        // Clean up
        unsafe {
            libc::close(fd1);
            libc::close(fd2);
        }
    }

    #[test]
    fn test_create_memfd_inheritable() {
        // memfd should NOT have FD_CLOEXEC set (so it survives fork)
        let fd = create_memfd("test_inherit").unwrap();

        let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
        assert!(flags >= 0, "fcntl should succeed");
        assert_eq!(flags & libc::FD_CLOEXEC, 0, "FD_CLOEXEC should NOT be set");

        unsafe {
            libc::close(fd);
        }
    }

    #[test]
    fn test_create_memfd_writable() {
        let fd = create_memfd("test_write").unwrap();

        // Resize the memfd
        let result = unsafe { libc::ftruncate(fd, 1024) };
        assert_eq!(result, 0, "ftruncate should succeed");

        // Write to it
        let data = b"test data";
        let written = unsafe { libc::write(fd, data.as_ptr() as *const libc::c_void, data.len()) };
        assert_eq!(written as usize, data.len(), "write should succeed");

        unsafe {
            libc::close(fd);
        }
    }

    // =========================================================================
    // LOG_BUFFER_SIZE Constant Test
    // =========================================================================

    #[test]
    fn test_log_buffer_size_is_1mb() {
        assert_eq!(
            LOG_BUFFER_SIZE,
            1024 * 1024,
            "LOG_BUFFER_SIZE should be 1MB"
        );
    }

    // =========================================================================
    // Drop Behavior Test
    // =========================================================================

    #[test]
    fn test_drop_closes_fds() {
        let fd_to_check;

        {
            let capture = LogCapture::new(1).unwrap();
            fd_to_check = capture.get_fd(0).unwrap();

            // Verify fd is valid before drop
            let flags = unsafe { libc::fcntl(fd_to_check, libc::F_GETFD) };
            assert!(flags >= 0, "fd should be valid before drop");
        } // capture is dropped here

        // After drop, fd should be closed (fcntl should fail)
        let flags = unsafe { libc::fcntl(fd_to_check, libc::F_GETFD) };
        assert_eq!(flags, -1, "fd should be closed after drop");
    }

    // =========================================================================
    // Additional Edge Case Tests (Phase 2 Regression Prevention)
    // =========================================================================

    #[test]
    fn test_logcapture_large_slot_count() {
        // Test creating many slots
        let capture = LogCapture::new(32).expect("Failed to create LogCapture with 32 slots");
        assert_eq!(capture.slot_count(), 32);

        // Verify all slots have valid fds
        for slot in 0..32 {
            let fd = capture.get_fd(slot);
            assert!(fd.is_some(), "Slot {} should exist", slot);
            assert!(fd.unwrap() >= 0, "Slot {} should have valid fd", slot);
        }
    }

    #[test]
    fn test_read_and_clear_large_content() {
        let capture = LogCapture::new(1).unwrap();
        let fd = capture.get_fd(0).unwrap();

        // Write a large message (approaching buffer limit but not exceeding)
        let test_message: String = "X".repeat(65536); // 64KB
        unsafe {
            libc::lseek(fd, 0, libc::SEEK_SET);
            libc::write(
                fd,
                test_message.as_ptr() as *const libc::c_void,
                test_message.len(),
            );
        }

        let content = capture.read_and_clear(0).unwrap();
        assert_eq!(content.len(), test_message.len());
        assert_eq!(content, test_message);
    }

    #[test]
    fn test_read_and_clear_binary_content() {
        let capture = LogCapture::new(1).unwrap();
        let fd = capture.get_fd(0).unwrap();

        // Write binary content (non-UTF8)
        let binary_data: [u8; 8] = [0xFF, 0xFE, 0x00, 0x01, 0x80, 0x90, 0xA0, 0xB0];
        unsafe {
            libc::lseek(fd, 0, libc::SEEK_SET);
            libc::write(
                fd,
                binary_data.as_ptr() as *const libc::c_void,
                binary_data.len(),
            );
        }

        // read_to_string may produce replacement characters for invalid UTF-8
        let result = capture.read_and_clear(0);
        assert!(result.is_ok(), "Should handle binary content gracefully");
    }

    #[test]
    fn test_create_memfd_with_special_chars_in_name() {
        // Test that special characters in name work
        let fd = create_memfd("test-with-dashes_and_underscores").unwrap();
        assert!(fd >= 0);
        unsafe {
            libc::close(fd);
        }
    }

    #[test]
    fn test_create_memfd_with_numbers_in_name() {
        let fd = create_memfd("log_12345_test").unwrap();
        assert!(fd >= 0);
        unsafe {
            libc::close(fd);
        }
    }

    #[test]
    fn test_memfd_seek_operations() {
        let capture = LogCapture::new(1).unwrap();
        let fd = capture.get_fd(0).unwrap();

        // Write at position 0
        let msg = "hello";
        unsafe {
            libc::lseek(fd, 0, libc::SEEK_SET);
            libc::write(fd, msg.as_ptr() as *const libc::c_void, msg.len());
        }

        // Verify position after write
        let pos = unsafe { libc::lseek(fd, 0, libc::SEEK_CUR) };
        assert_eq!(pos as usize, msg.len());

        // Seek back to start
        let pos = unsafe { libc::lseek(fd, 0, libc::SEEK_SET) };
        assert_eq!(pos, 0);
    }

    #[test]
    fn test_multiple_slots_independent() {
        let capture = LogCapture::new(3).unwrap();

        // Write different content to each slot
        let messages = ["slot0 content", "slot1 content", "slot2 content"];
        for (slot, msg) in messages.iter().enumerate() {
            let fd = capture.get_fd(slot).unwrap();
            unsafe {
                libc::lseek(fd, 0, libc::SEEK_SET);
                libc::write(fd, msg.as_ptr() as *const libc::c_void, msg.len());
            }
        }

        // Verify each slot has its own content
        for (slot, expected) in messages.iter().enumerate() {
            let content = capture.read_and_clear(slot).unwrap();
            assert_eq!(
                &content, *expected,
                "Slot {} should have correct content",
                slot
            );
        }
    }

    #[test]
    fn test_read_and_clear_clears_buffer_completely() {
        let capture = LogCapture::new(1).unwrap();
        let fd = capture.get_fd(0).unwrap();

        // Write content
        let msg = "test content";
        unsafe {
            libc::lseek(fd, 0, libc::SEEK_SET);
            libc::write(fd, msg.as_ptr() as *const libc::c_void, msg.len());
        }

        // Read and clear
        let _ = capture.read_and_clear(0).unwrap();

        // Write new content at position 0
        let new_msg = "new";
        unsafe {
            libc::lseek(fd, 0, libc::SEEK_SET);
            libc::write(fd, new_msg.as_ptr() as *const libc::c_void, new_msg.len());
        }

        // Should only see new content, not old content remnants
        let content = capture.read_and_clear(0).unwrap();
        assert_eq!(content, new_msg, "Buffer should be completely cleared");
        assert!(
            !content.contains("test"),
            "Old content should not be present"
        );
    }
}
