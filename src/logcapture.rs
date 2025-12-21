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
use std::io::{Read, Seek, SeekFrom};
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
        for (_, fd) in &self.fds {
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
        let stdout_file = libc::fdopen(libc::STDOUT_FILENO, b"w\0".as_ptr() as *const i8);
        if !stdout_file.is_null() {
            libc::setvbuf(stdout_file, std::ptr::null_mut(), libc::_IOLBF, 0);
        }
    }
    Ok(())
}
