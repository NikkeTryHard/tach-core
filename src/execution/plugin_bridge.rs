//! Plugin Bridge: FD Teleporter for SCM_RIGHTS Handover
//!
//! This module implements the "FD Adoption" pattern for transferring file descriptors
//! between the Supervisor (parent) and Worker processes.
//!
//! # The Fidelity Gap Problem
//!
//! When a pytest fixture returns a socket, DB connection, or file handle, we cannot
//! serialize it via JSON/pickle. The `repr()` degradation in `effects.py` captures
//! metadata but loses the actual file descriptor.
//!
//! # The Solution: SCM_RIGHTS + dup2
//!
//! 1. **Supervisor** captures FD number and metadata in `FileDescriptorEffect`
//! 2. **Supervisor** sends FD to Worker via Unix socket SCM_RIGHTS
//! 3. **Worker** receives FD via `recvmsg` (lands on arbitrary FD number)
//! 4. **Worker** uses `dup2(received_fd, target_fd)` to force it to expected index
//! 5. **Worker** wraps the FD in appropriate Python object (socket, file, etc.)
//!
//! # The "Ghost Close" Prevention
//!
//! After sending an FD via SCM_RIGHTS, the Supervisor must NOT close it until
//! the Worker has confirmed adoption. We use `std::mem::forget()` on the OwnedFd
//! to prevent the Drop impl from closing the FD prematurely.
//!
//! # Architecture
//!
//! ```text
//!                    Supervisor                              Worker
//!                    ---------                              ------
//!                        |                                     |
//!   [fixture returns socket.socket(fd=5)]                      |
//!                        |                                     |
//!   [capture FileDescriptorEffect{fd=5, target=5}]            |
//!                        |                                     |
//!   [sendmsg(sock, SCM_RIGHTS, fd=5)]                         |
//!                        | ---------------------------------> |
//!                        |                        [recvmsg -> received_fd=17]
//!                        |                        [dup2(17, 5) -> fd=5]
//!                        |                        [close(17)]
//!                        |                        [socket.fromfd(5) -> socket obj]
//!                        |                                     |
//!   [forget(fd=5) - don't close!]                             |
//!                        |                                     |
//! ```
//!
//! # Kernel Requirements
//!
//! - Linux 2.6+ (SCM_RIGHTS is POSIX, widely supported)
//! - Unix domain socket between Supervisor and Worker
//!
//! # Safety
//!
//! The `dup2` syscall is safe when:
//! 1. `received_fd` is a valid FD we just received via recvmsg
//! 2. `target_fd` is the FD number the Python object expects
//! 3. If `target_fd` is already open, dup2 atomically closes it first
//!
//! This module handles all edge cases including:
//! - Target FD already in use (dup2 closes it atomically)
//! - Same source and target FD (dup2 is a no-op, returns success)
//! - Multiple FDs in single message (batch teleportation)

use anyhow::{anyhow, Context, Result};
use std::mem::MaybeUninit;
use std::os::fd::{AsRawFd, OwnedFd, RawFd};
use std::os::unix::net::UnixStream;

// =============================================================================
//  FD Teleportation Message Format
// =============================================================================
//
// Message structure for FD handover:
//
// | Bytes  | Field        | Description                              |
// |--------|--------------|------------------------------------------|
// | 0-3    | count        | Number of FDs being sent (u32 LE)        |
// | 4-7    | target_fd[0] | Expected FD number for first FD          |
// | 8-11   | target_fd[1] | Expected FD number for second FD         |
// | ...    | ...          | Additional target FDs                    |
// | CMSG   | SCM_RIGHTS   | Actual FDs attached via control message  |
//
// The `target_fd` array tells the Worker where to `dup2` each received FD.
// This maintains the invariant that the Python object's fileno() returns
// the same value after teleportation.
// =============================================================================

/// Maximum number of FDs that can be transferred in a single message
pub const MAX_FDS_PER_MESSAGE: usize = 16;

/// A request to teleport one or more file descriptors to a Worker
#[derive(Debug, Clone)]
pub struct FdTeleportRequest {
    /// File descriptors to send (from Supervisor's FD table)
    pub fds: Vec<RawFd>,
    /// Target FD numbers the Worker should dup2 to
    pub target_fds: Vec<i32>,
    /// Human-readable names for debugging (e.g., "fixture:db_connection")
    pub names: Vec<String>,
}

impl FdTeleportRequest {
    /// Create a new teleport request for a single FD
    pub fn single(fd: RawFd, target_fd: i32, name: impl Into<String>) -> Self {
        Self {
            fds: vec![fd],
            target_fds: vec![target_fd],
            names: vec![name.into()],
        }
    }

    /// Create a new teleport request for multiple FDs
    pub fn batch(fds: Vec<RawFd>, target_fds: Vec<i32>, names: Vec<String>) -> Result<Self> {
        if fds.len() != target_fds.len() || fds.len() != names.len() {
            return Err(anyhow!(
                "FD teleport request: mismatched lengths (fds={}, targets={}, names={})",
                fds.len(),
                target_fds.len(),
                names.len()
            ));
        }
        if fds.len() > MAX_FDS_PER_MESSAGE {
            return Err(anyhow!(
                "FD teleport request: too many FDs ({} > {})",
                fds.len(),
                MAX_FDS_PER_MESSAGE
            ));
        }
        Ok(Self {
            fds,
            target_fds,
            names,
        })
    }

    /// Number of FDs in this request
    pub fn len(&self) -> usize {
        self.fds.len()
    }

    /// Check if this request is empty
    pub fn is_empty(&self) -> bool {
        self.fds.is_empty()
    }
}

/// Result of FD adoption on the Worker side
#[derive(Debug)]
pub struct FdAdoptionResult {
    /// Number of FDs successfully adopted
    pub adopted_count: usize,
    /// Final FD numbers after dup2 (should match target_fds)
    pub final_fds: Vec<RawFd>,
    /// Any errors encountered (non-fatal, per-FD)
    pub errors: Vec<String>,
}

// =============================================================================
//  Supervisor Side: Send FDs via SCM_RIGHTS
// =============================================================================

/// Send file descriptors to a Worker via SCM_RIGHTS
///
/// This is the Supervisor-side of the FD Teleporter. After calling this function,
/// the Supervisor MUST NOT close the FDs - they are now owned by the Worker.
///
/// # Arguments
/// * `sock` - Unix socket connected to the Worker
/// * `request` - FDs to send with their target numbers
///
/// # Returns
/// * `Ok(())` on success
/// * `Err` if sendmsg fails
///
/// # Safety
/// After successful send, the caller should use `forget_sent_fds()` to prevent
/// the Drop impl from closing the FDs before the Worker adopts them.
pub fn send_fds(sock: &UnixStream, request: &FdTeleportRequest) -> Result<()> {
    use nix::sys::socket::{sendmsg, ControlMessage, MsgFlags};
    use std::io::IoSlice;

    if request.is_empty() {
        return Ok(());
    }

    if request.len() > MAX_FDS_PER_MESSAGE {
        return Err(anyhow!(
            "Too many FDs to send: {} > {}",
            request.len(),
            MAX_FDS_PER_MESSAGE
        ));
    }

    // Build message body: count (u32) + target_fds (i32 array)
    let count = request.len() as u32;
    let mut msg_body = Vec::with_capacity(4 + request.len() * 4);
    msg_body.extend_from_slice(&count.to_le_bytes());
    for &target_fd in &request.target_fds {
        msg_body.extend_from_slice(&target_fd.to_le_bytes());
    }

    let iov = [IoSlice::new(&msg_body)];
    let cmsg = [ControlMessage::ScmRights(&request.fds)];

    eprintln!(
        "[fd_teleporter] Sending {} FDs: {:?} -> targets {:?}",
        request.len(),
        request.fds,
        request.target_fds
    );

    sendmsg::<()>(sock.as_raw_fd(), &iov, &cmsg, MsgFlags::empty(), None)
        .context("Failed to send FDs via SCM_RIGHTS")?;

    eprintln!("[fd_teleporter] FDs sent successfully");

    Ok(())
}

/// Prevent FDs from being closed after sending
///
/// Call this after `send_fds()` succeeds to prevent the Supervisor from
/// accidentally closing the FDs before the Worker adopts them.
///
/// # The Ghost Close Problem
///
/// If the Supervisor holds an `OwnedFd` for the socket/file, and that OwnedFd
/// goes out of scope after sending, the Drop impl will close the FD. But the
/// kernel still thinks the FD is valid in the Supervisor's FD table (it just
/// sent a copy via SCM_RIGHTS). This can cause the FD to be recycled to a new
/// file before the Worker finishes adopting it.
///
/// # Solution
///
/// Use `std::mem::forget()` on the OwnedFd after sending. This leaks the
/// wrapper but keeps the FD open. The Worker will adopt it via dup2 and
/// become the sole owner.
///
/// # Safety
///
/// This function intentionally leaks the OwnedFd to prevent premature close.
/// The Worker is now responsible for closing the FD when done.
pub fn forget_sent_fd(fd: OwnedFd) {
    let raw_fd = fd.as_raw_fd();
    std::mem::forget(fd);
    eprintln!(
        "[fd_teleporter] Ghost Close Prevention: forgot ownership of FD {}",
        raw_fd
    );
}

// =============================================================================
//  Worker Side: Receive and Adopt FDs via dup2
// =============================================================================

/// Receive and adopt file descriptors from the Supervisor
///
/// This is the Worker-side of the FD Teleporter. It:
/// 1. Receives FDs via recvmsg (they land on arbitrary FD numbers)
/// 2. Reads the target FD numbers from the message
/// 3. Uses dup2 to move each FD to its target number
/// 4. Closes the original received FDs
///
/// # Arguments
/// * `sock` - Unix socket connected to the Supervisor
///
/// # Returns
/// * `Ok(FdAdoptionResult)` with adopted FDs and any errors
/// * `Err` if recvmsg fails completely
///
/// # Safety
/// The dup2 syscall is safe and atomic. If the target FD is already open,
/// dup2 closes it first (atomically).
pub fn receive_and_adopt_fds(sock: &UnixStream) -> Result<FdAdoptionResult> {
    // Maximum message size: 4 bytes count + 16 * 4 bytes targets = 68 bytes
    let mut msg_body = [0u8; 4 + MAX_FDS_PER_MESSAGE * 4];
    let mut iov = libc::iovec {
        iov_base: msg_body.as_mut_ptr() as *mut libc::c_void,
        iov_len: msg_body.len(),
    };

    // Control message buffer for up to MAX_FDS_PER_MESSAGE file descriptors
    // SAFETY: CMSG_SPACE computes the required buffer size
    let cmsg_size =
        unsafe { libc::CMSG_SPACE((MAX_FDS_PER_MESSAGE * std::mem::size_of::<RawFd>()) as u32) }
            as usize;
    let mut cmsg_buf = vec![0u8; cmsg_size];

    let mut msg: libc::msghdr = unsafe { MaybeUninit::zeroed().assume_init() };
    msg.msg_iov = &mut iov;
    msg.msg_iovlen = 1;
    msg.msg_control = cmsg_buf.as_mut_ptr() as *mut libc::c_void;
    msg.msg_controllen = cmsg_buf.len();

    // SAFETY: recvmsg is a safe syscall with properly initialized buffers
    let bytes_received = unsafe { libc::recvmsg(sock.as_raw_fd(), &mut msg, 0) };
    if bytes_received < 0 {
        return Err(anyhow!(
            "recvmsg failed: {}",
            std::io::Error::last_os_error()
        ));
    }

    if bytes_received < 4 {
        return Err(anyhow!("Message too short: {} bytes", bytes_received));
    }

    // Parse message body: count + target_fds
    let count = u32::from_le_bytes([msg_body[0], msg_body[1], msg_body[2], msg_body[3]]) as usize;

    if count == 0 {
        return Ok(FdAdoptionResult {
            adopted_count: 0,
            final_fds: vec![],
            errors: vec![],
        });
    }

    if count > MAX_FDS_PER_MESSAGE {
        return Err(anyhow!(
            "Too many FDs in message: {} > {}",
            count,
            MAX_FDS_PER_MESSAGE
        ));
    }

    // Extract target FDs from message body
    let mut target_fds = Vec::with_capacity(count);
    for i in 0..count {
        let offset = 4 + i * 4;
        if offset + 4 > bytes_received as usize {
            return Err(anyhow!("Message truncated: missing target_fd[{}]", i));
        }
        let target = i32::from_le_bytes([
            msg_body[offset],
            msg_body[offset + 1],
            msg_body[offset + 2],
            msg_body[offset + 3],
        ]);
        target_fds.push(target);
    }

    // Extract received FDs from control message
    let mut received_fds: Vec<RawFd> = Vec::with_capacity(count);

    // SAFETY: Iterating over control messages in properly received buffer
    unsafe {
        let mut cmsg = libc::CMSG_FIRSTHDR(&msg);
        while !cmsg.is_null() {
            if (*cmsg).cmsg_level == libc::SOL_SOCKET && (*cmsg).cmsg_type == libc::SCM_RIGHTS {
                // Calculate number of FDs in this control message
                let cmsg_data_len = (*cmsg).cmsg_len as usize - libc::CMSG_LEN(0) as usize;
                let fd_count = cmsg_data_len / std::mem::size_of::<RawFd>();

                let fd_ptr = libc::CMSG_DATA(cmsg) as *const RawFd;
                for j in 0..fd_count {
                    received_fds.push(*fd_ptr.add(j));
                }
            }
            cmsg = libc::CMSG_NXTHDR(&msg, cmsg);
        }
    }

    if received_fds.len() != count {
        return Err(anyhow!(
            "FD count mismatch: expected {} from message, received {} via SCM_RIGHTS",
            count,
            received_fds.len()
        ));
    }

    eprintln!(
        "[fd_teleporter] Received {} FDs: {:?} -> adopting to {:?}",
        count, received_fds, target_fds
    );

    // =========================================================================
    //  The dup2 Strategy: Force FDs to Expected Indices
    // =========================================================================
    //
    // The Orchestrator's Wisdom:
    //   "In the Matrix, you don't just see the spoon; you become the spoon.
    //    In Tach, the Worker doesn't just see the FD; it IS the Parent's FD."
    //
    // Algorithm:
    // 1. For each (received_fd, target_fd) pair:
    //    a. If received_fd == target_fd: already in position (no-op)
    //    b. Else: dup2(received_fd, target_fd) and close(received_fd)
    // 2. Return the final FD numbers (should all match target_fds)
    // =========================================================================

    let mut final_fds = Vec::with_capacity(count);
    let mut errors = Vec::new();
    let mut adopted_count = 0;

    for i in 0..count {
        let received_fd = received_fds[i];
        let target_fd = target_fds[i];

        if received_fd == target_fd {
            // Already at correct position
            eprintln!(
                "[fd_teleporter] FD {} already at target position",
                received_fd
            );
            final_fds.push(received_fd);
            adopted_count += 1;
            continue;
        }

        // dup2(received_fd, target_fd):
        // - If target_fd is open, it's closed atomically
        // - received_fd is NOT closed by dup2; we must close it manually
        // SAFETY: Both FDs are valid (received_fd from recvmsg, target_fd is where we want it)
        let result = unsafe { libc::dup2(received_fd, target_fd) };

        if result == -1 {
            let err = std::io::Error::last_os_error();
            errors.push(format!(
                "dup2({} -> {}) failed: {}",
                received_fd, target_fd, err
            ));
            // Keep the received_fd as fallback
            final_fds.push(received_fd);
            // Don't close received_fd since we're using it as fallback
        } else {
            eprintln!(
                "[fd_teleporter] dup2({} -> {}) succeeded",
                received_fd, target_fd
            );
            final_fds.push(target_fd);
            adopted_count += 1;

            // Close the original received_fd (now duplicated to target_fd)
            // SAFETY: received_fd is valid and we no longer need it
            unsafe {
                libc::close(received_fd);
            }
        }
    }

    eprintln!(
        "[fd_teleporter] Adoption complete: {}/{} FDs adopted, final_fds={:?}",
        adopted_count, count, final_fds
    );

    Ok(FdAdoptionResult {
        adopted_count,
        final_fds,
        errors,
    })
}

/// Receive a single file descriptor and adopt it at a specific target FD
///
/// Convenience wrapper for the common case of adopting a single FD.
///
/// # Arguments
/// * `sock` - Unix socket connected to the Supervisor
///
/// # Returns
/// * `Ok(target_fd)` - The FD is now available at target_fd
/// * `Err` if receive or adoption fails
pub fn receive_and_adopt_single_fd(sock: &UnixStream) -> Result<RawFd> {
    let result = receive_and_adopt_fds(sock)?;

    if result.adopted_count == 0 {
        return Err(anyhow!("No FDs received"));
    }

    if !result.errors.is_empty() {
        eprintln!(
            "[fd_teleporter] WARNING: Adoption had errors: {:?}",
            result.errors
        );
    }

    Ok(result.final_fds[0])
}

// =============================================================================
//  Helper: Create Socket Pair for FD Teleportation
// =============================================================================

/// Create a Unix socket pair for FD teleportation between Supervisor and Worker
///
/// # Returns
/// * `Ok((supervisor_end, worker_end))` - Socket endpoints for each process
///
/// # Usage
/// The Supervisor keeps `supervisor_end` and sends FDs via `send_fds()`.
/// The Worker keeps `worker_end` and receives FDs via `receive_and_adopt_fds()`.
///
/// After fork(), each process should close the end it doesn't need.
pub fn create_teleporter_socket_pair() -> Result<(UnixStream, UnixStream)> {
    UnixStream::pair().context("Failed to create Unix socket pair for FD teleportation")
}

// =============================================================================
//  Python Integration: Wrap Adopted FDs in Python Objects
// =============================================================================

/// Python-callable wrapper for FD adoption
///
/// This function is designed to be called from Python via PyO3 to complete
/// the FD teleportation by wrapping the adopted FD in an appropriate Python object.
///
/// # Example Python Integration (in tach/effects.py)
///
/// ```python
/// def adopt_fd_as_socket(sock_fd: int) -> socket.socket:
///     """Wrap an adopted FD in a Python socket object."""
///     import socket
///     return socket.fromfd(sock_fd, socket.AF_INET, socket.SOCK_STREAM)
///
/// def adopt_fd_as_file(file_fd: int, mode: str = 'rb') -> IO:
///     """Wrap an adopted FD in a Python file object."""
///     import os
///     return os.fdopen(file_fd, mode)
/// ```
pub mod python {
    use pyo3::prelude::*;

    /// Register the FD teleporter Python module
    pub fn register_module(parent: &Bound<'_, PyModule>) -> PyResult<()> {
        let m = PyModule::new(parent.py(), "fd_teleporter")?;
        // Add Python-callable functions here when needed
        parent.add_submodule(&m)?;
        Ok(())
    }
}

// =============================================================================
//  Unit Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::os::unix::net::UnixStream;

    #[test]
    fn test_fd_teleport_request_single() {
        let req = FdTeleportRequest::single(5, 5, "test_socket");
        assert_eq!(req.len(), 1);
        assert_eq!(req.fds[0], 5);
        assert_eq!(req.target_fds[0], 5);
        assert_eq!(req.names[0], "test_socket");
    }

    #[test]
    fn test_fd_teleport_request_batch() {
        let req = FdTeleportRequest::batch(
            vec![3, 4, 5],
            vec![10, 11, 12],
            vec!["fd_a".into(), "fd_b".into(), "fd_c".into()],
        )
        .unwrap();

        assert_eq!(req.len(), 3);
        assert!(!req.is_empty());
    }

    #[test]
    fn test_fd_teleport_request_batch_mismatched_lengths() {
        let result =
            FdTeleportRequest::batch(vec![3, 4], vec![10], vec!["fd_a".into(), "fd_b".into()]);
        assert!(result.is_err());
    }

    #[test]
    fn test_fd_teleport_request_too_many_fds() {
        let fds: Vec<RawFd> = (0..20).collect();
        let targets: Vec<i32> = (100..120).collect();
        let names: Vec<String> = (0..20).map(|i| format!("fd_{}", i)).collect();

        let result = FdTeleportRequest::batch(fds, targets, names);
        assert!(result.is_err());
    }

    #[test]
    fn test_create_teleporter_socket_pair() {
        let (supervisor, worker) =
            create_teleporter_socket_pair().expect("Failed to create socket pair");

        // Test that sockets are connected
        let mut supervisor = supervisor;
        let mut worker = worker;

        supervisor.write_all(b"hello").expect("Write failed");
        supervisor.flush().expect("Flush failed");

        let mut buf = [0u8; 5];
        worker.read_exact(&mut buf).expect("Read failed");
        assert_eq!(&buf, b"hello");
    }

    #[test]
    fn test_dup2_same_fd_is_noop() {
        // Create a pipe to get a valid FD
        let (read_end, _write_end) = UnixStream::pair().expect("pipe failed");
        let fd = read_end.as_raw_fd();

        // dup2(fd, fd) should be a no-op and return fd
        let result = unsafe { libc::dup2(fd, fd) };
        assert_eq!(result, fd);
    }

    #[test]
    fn test_dup2_moves_fd() {
        // Create a socket pair to get valid FDs
        let (sock_a, _sock_b) = UnixStream::pair().expect("socketpair failed");
        let original_fd = sock_a.as_raw_fd();

        // Find an unused FD number (high number is usually free)
        let target_fd = 200;

        // dup2 to move the FD
        let result = unsafe { libc::dup2(original_fd, target_fd) };
        assert_eq!(result, target_fd);

        // Clean up: close the duplicated FD
        unsafe {
            libc::close(target_fd);
        }
    }

    // Integration test: Full FD teleportation
    // This test requires forking or threading, so it's marked as ignored
    // for regular test runs.
    #[test]
    #[ignore]
    fn test_full_fd_teleportation() {
        // This would test the full flow:
        // 1. Create socket pair
        // 2. Fork (or use threads with careful FD handling)
        // 3. Parent sends FD via send_fds
        // 4. Child receives via receive_and_adopt_fds
        // 5. Verify FD is at expected number
        //
        // Full test requires integration test with actual fork
    }

    // =========================================================================
    // Additional Regression Prevention Tests (Phase 2)
    // =========================================================================

    #[test]
    fn test_fd_teleport_request_empty() {
        let req = FdTeleportRequest::batch(vec![], vec![], vec![]).unwrap();
        assert!(req.is_empty());
        assert_eq!(req.len(), 0);
    }

    #[test]
    fn test_fd_teleport_request_single_same_fd() {
        // Test when source and target FD are the same
        let req = FdTeleportRequest::single(10, 10, "same_fd");
        assert_eq!(req.fds[0], 10);
        assert_eq!(req.target_fds[0], 10);
    }

    #[test]
    fn test_fd_teleport_request_single_different_fd() {
        // Test when source and target FD are different
        let req = FdTeleportRequest::single(5, 100, "moved_fd");
        assert_eq!(req.fds[0], 5);
        assert_eq!(req.target_fds[0], 100);
        assert_eq!(req.names[0], "moved_fd");
    }

    #[test]
    fn test_fd_teleport_request_batch_max_size() {
        // Test batch with exactly MAX_FDS_PER_MESSAGE
        let fds: Vec<RawFd> = (0..MAX_FDS_PER_MESSAGE as i32).collect();
        let targets: Vec<i32> = (100..100 + MAX_FDS_PER_MESSAGE as i32).collect();
        let names: Vec<String> = (0..MAX_FDS_PER_MESSAGE)
            .map(|i| format!("fd_{}", i))
            .collect();

        let result = FdTeleportRequest::batch(fds, targets, names);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), MAX_FDS_PER_MESSAGE);
    }

    #[test]
    fn test_fd_teleport_request_batch_one_over_max() {
        // Test batch with MAX_FDS_PER_MESSAGE + 1 (should fail)
        let count = MAX_FDS_PER_MESSAGE + 1;
        let fds: Vec<RawFd> = (0..count as i32).collect();
        let targets: Vec<i32> = (100..100 + count as i32).collect();
        let names: Vec<String> = (0..count).map(|i| format!("fd_{}", i)).collect();

        let result = FdTeleportRequest::batch(fds, targets, names);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("too many FDs"));
    }

    #[test]
    fn test_fd_teleport_request_negative_fds() {
        // Negative FDs are technically valid in the request structure
        // (validation happens at send/receive time)
        let req = FdTeleportRequest::single(-1, -1, "negative");
        assert_eq!(req.fds[0], -1);
        assert_eq!(req.target_fds[0], -1);
    }

    #[test]
    fn test_fd_teleport_request_names_preserved() {
        // Test that names are properly stored
        let req = FdTeleportRequest::single(5, 5, "special:fixture:db_connection");
        assert_eq!(req.names[0], "special:fixture:db_connection");
    }

    #[test]
    fn test_fd_teleport_request_unicode_name() {
        // Test Unicode names
        let req = FdTeleportRequest::single(5, 5, "套接字连接");
        assert_eq!(req.names[0], "套接字连接");
    }

    #[test]
    fn test_fd_teleport_request_empty_name() {
        let req = FdTeleportRequest::single(5, 5, "");
        assert_eq!(req.names[0], "");
    }

    #[test]
    fn test_max_fds_per_message_constant() {
        // Verify the constant is reasonable
        assert_eq!(MAX_FDS_PER_MESSAGE, 16);
        // Static assertions moved to const block
        const _: () = {
            assert!(MAX_FDS_PER_MESSAGE > 0);
            assert!(MAX_FDS_PER_MESSAGE <= 1024); // Kernel SCM_MAX_FD is typically 253
        };
    }

    #[test]
    fn test_fd_adoption_result_structure() {
        let result = FdAdoptionResult {
            adopted_count: 3,
            final_fds: vec![10, 11, 12],
            errors: vec!["error1".to_string()],
        };

        assert_eq!(result.adopted_count, 3);
        assert_eq!(result.final_fds.len(), 3);
        assert_eq!(result.errors.len(), 1);
    }

    #[test]
    fn test_fd_adoption_result_empty() {
        let result = FdAdoptionResult {
            adopted_count: 0,
            final_fds: vec![],
            errors: vec![],
        };

        assert_eq!(result.adopted_count, 0);
        assert!(result.final_fds.is_empty());
        assert!(result.errors.is_empty());
    }

    #[test]
    fn test_send_fds_empty_request() {
        let (supervisor, _worker) = create_teleporter_socket_pair().unwrap();
        let req = FdTeleportRequest::batch(vec![], vec![], vec![]).unwrap();

        // Empty request should succeed immediately
        let result = send_fds(&supervisor, &req);
        assert!(result.is_ok());
    }

    #[test]
    fn test_send_fds_too_many_fds_error() {
        let (supervisor, _worker) = create_teleporter_socket_pair().unwrap();

        // Create a request that exceeds the limit (bypassing batch validation)
        let mut req = FdTeleportRequest::single(1, 1, "test");
        for i in 2..=20 {
            req.fds.push(i);
            req.target_fds.push(i);
            req.names.push(format!("fd_{}", i));
        }

        // send_fds should reject this
        let result = send_fds(&supervisor, &req);
        assert!(result.is_err());
    }

    #[test]
    fn test_socket_pair_bidirectional() {
        let (mut supervisor, mut worker) = create_teleporter_socket_pair().unwrap();

        // Test supervisor -> worker
        supervisor.write_all(b"to_worker").unwrap();
        let mut buf = [0u8; 9];
        worker.read_exact(&mut buf).unwrap();
        assert_eq!(&buf, b"to_worker");

        // Test worker -> supervisor
        worker.write_all(b"to_super").unwrap();
        let mut buf = [0u8; 8];
        supervisor.read_exact(&mut buf).unwrap();
        assert_eq!(&buf, b"to_super");
    }

    #[test]
    fn test_dup2_to_closed_fd() {
        // Create a valid source FD
        let (sock, _) = UnixStream::pair().expect("socketpair failed");
        let source_fd = sock.as_raw_fd();

        // Use a high target FD that should be closed
        let target_fd = 999;

        // dup2 should work even if target is not open
        let result = unsafe { libc::dup2(source_fd, target_fd) };
        assert_eq!(result, target_fd);

        // Verify the target is now valid
        let flags = unsafe { libc::fcntl(target_fd, libc::F_GETFD) };
        assert!(flags >= 0, "Target FD should be valid after dup2");

        // Clean up
        unsafe {
            libc::close(target_fd);
        }
    }

    #[test]
    fn test_dup2_invalid_source_fails() {
        // Use an invalid source FD
        let invalid_fd = 9999;
        let target_fd = 200;

        // dup2 with invalid source should fail
        let result = unsafe { libc::dup2(invalid_fd, target_fd) };
        assert_eq!(result, -1);

        let errno = std::io::Error::last_os_error().raw_os_error();
        assert_eq!(errno, Some(libc::EBADF));
    }

    #[test]
    fn test_dup2_negative_target() {
        // Create a valid source FD
        let (sock, _) = UnixStream::pair().expect("socketpair failed");
        let source_fd = sock.as_raw_fd();

        // Negative target should fail
        let result = unsafe { libc::dup2(source_fd, -1) };
        assert_eq!(result, -1);

        let errno = std::io::Error::last_os_error().raw_os_error();
        assert_eq!(errno, Some(libc::EBADF));
    }

    #[test]
    fn test_forget_sent_fd() {
        use std::os::fd::FromRawFd;

        // Create an FD via dup
        let (sock, _) = UnixStream::pair().expect("socketpair failed");
        let original_fd = sock.as_raw_fd();

        // dup to create a new FD we can forget
        let duped_fd = unsafe { libc::dup(original_fd) };
        assert!(duped_fd >= 0);

        // Wrap it in OwnedFd
        let owned_fd = unsafe { OwnedFd::from_raw_fd(duped_fd) };

        // Verify FD is valid before forget
        let flags = unsafe { libc::fcntl(duped_fd, libc::F_GETFD) };
        assert!(flags >= 0, "FD should be valid before forget");

        // Call forget_sent_fd (this intentionally leaks)
        forget_sent_fd(owned_fd);

        // FD should STILL be valid (not closed by Drop)
        let flags_after = unsafe { libc::fcntl(duped_fd, libc::F_GETFD) };
        assert!(
            flags_after >= 0,
            "FD should still be valid after forget (not closed)"
        );

        // Clean up manually since we leaked it
        unsafe {
            libc::close(duped_fd);
        }
    }

    #[test]
    fn test_fd_teleport_request_clone() {
        let req = FdTeleportRequest::single(5, 10, "original");
        let cloned = req.clone();

        assert_eq!(cloned.fds, req.fds);
        assert_eq!(cloned.target_fds, req.target_fds);
        assert_eq!(cloned.names, req.names);
    }

    #[test]
    fn test_fd_teleport_request_debug() {
        let req = FdTeleportRequest::single(5, 10, "debug_test");
        let debug_str = format!("{:?}", req);

        assert!(debug_str.contains("FdTeleportRequest"));
        assert!(debug_str.contains("5"));
        assert!(debug_str.contains("10"));
    }

    #[test]
    fn test_fd_adoption_result_debug() {
        let result = FdAdoptionResult {
            adopted_count: 1,
            final_fds: vec![10],
            errors: vec![],
        };

        let debug_str = format!("{:?}", result);
        assert!(debug_str.contains("FdAdoptionResult"));
        assert!(debug_str.contains("adopted_count"));
    }

    #[test]
    fn test_socket_pair_fd_validity() {
        let (supervisor, worker) = create_teleporter_socket_pair().unwrap();

        let sup_fd = supervisor.as_raw_fd();
        let worker_fd = worker.as_raw_fd();

        // Both FDs should be valid
        assert!(sup_fd >= 0);
        assert!(worker_fd >= 0);

        // FDs should be different
        assert_ne!(sup_fd, worker_fd);

        // Verify with fcntl
        let sup_flags = unsafe { libc::fcntl(sup_fd, libc::F_GETFD) };
        let worker_flags = unsafe { libc::fcntl(worker_fd, libc::F_GETFD) };

        assert!(sup_flags >= 0, "Supervisor FD should be valid");
        assert!(worker_flags >= 0, "Worker FD should be valid");
    }

    #[test]
    fn test_multiple_socket_pairs() {
        // Create multiple socket pairs to verify no FD collision
        let mut pairs = Vec::new();
        for _ in 0..10 {
            let pair = create_teleporter_socket_pair().unwrap();
            pairs.push(pair);
        }

        // All FDs should be unique
        let mut all_fds = Vec::new();
        for (sup, worker) in &pairs {
            all_fds.push(sup.as_raw_fd());
            all_fds.push(worker.as_raw_fd());
        }

        let mut unique_fds = all_fds.clone();
        unique_fds.sort();
        unique_fds.dedup();

        assert_eq!(all_fds.len(), unique_fds.len(), "All FDs should be unique");
    }

    #[test]
    fn test_batch_lengths_error_messages() {
        // Test various mismatched length combinations
        let cases = vec![
            (vec![1, 2], vec![1], vec!["a".into()]),
            (vec![1], vec![1, 2], vec!["a".into()]),
            (vec![1], vec![1], vec!["a".into(), "b".into()]),
            (
                vec![1, 2, 3],
                vec![1, 2],
                vec!["a".into(), "b".into(), "c".into()],
            ),
        ];

        for (fds, targets, names) in cases {
            let result = FdTeleportRequest::batch(fds.clone(), targets.clone(), names.clone());
            assert!(
                result.is_err(),
                "Should fail for fds={:?}, targets={:?}, names={:?}",
                fds,
                targets,
                names
            );
            let err_msg = result.unwrap_err().to_string();
            assert!(err_msg.contains("mismatched lengths"));
        }
    }

    #[test]
    fn test_dup2_overwrites_target() {
        // Create two socket pairs to have distinct FDs
        let (sock_a, _) = UnixStream::pair().expect("socketpair failed");
        let (sock_b, _) = UnixStream::pair().expect("socketpair failed");

        let fd_a = sock_a.as_raw_fd();
        let fd_b = sock_b.as_raw_fd();

        // Dup fd_a to a high target
        let target = 500;
        let result = unsafe { libc::dup2(fd_a, target) };
        assert_eq!(result, target);

        // Now dup fd_b to the same target - this should close the previous dup
        let result2 = unsafe { libc::dup2(fd_b, target) };
        assert_eq!(result2, target);

        // Target should still be valid
        let flags = unsafe { libc::fcntl(target, libc::F_GETFD) };
        assert!(flags >= 0);

        // Clean up
        unsafe {
            libc::close(target);
        }
    }
}
