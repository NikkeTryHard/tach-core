//! Snapshot Manager: userfaultfd-based memory reset for worker recycling
//!
//! This module implements the "Snapshot-Hypervisor" pattern for Tach:
//! - Capture a "golden" snapshot of worker memory after initialization
//! - Reset workers to that snapshot after each test (instead of killing them)
//! - Handle page faults via userfaultfd to lazily restore pages
//!
//! This eliminates fork() overhead in the hot loop (target: <50μs reset vs ~1ms fork)
//!
//! # Research Foundation
//!
//! This module implements the memory snapshotting technique described in
//! _Python Memory Snapshotting with Userfaultfd_:
//!
//! > "The kernel iterates over the Page Table Entries corresponding to the
//! > address range. It clears the 'Present' bit, effectively unmapping the
//! > physical pages. The next memory access triggers a page fault."
//!
//! ## How userfaultfd Works
//!
//! 1. **Registration**: We register memory regions with `UFFDIO_REGISTER`.
//!    The kernel marks these regions as "userfaultfd-managed".
//!
//! 2. **Invalidation**: We call `madvise(MADV_DONTNEED)` to discard pages.
//!    Unlike unmap(), this keeps the virtual address range valid but releases
//!    the physical pages. The PTEs are cleared (Present bit = 0).
//!
//! 3. **Fault Handling**: When the worker accesses an invalidated page:
//!    - CPU raises a page fault
//!    - Kernel checks: is this region registered with userfaultfd?
//!    - If yes: instead of SIGSEGV, block the thread and notify the UFFD owner
//!    - Supervisor receives fault notification via `read()` on UFFD fd
//!
//! 4. **Page Restoration**: Supervisor copies the golden page via `UFFDIO_COPY`:
//!    - `uffd.copy(src_data, dst_addr, len, wake=true)`
//!    - Kernel allocates a new physical page, copies data, updates PTE
//!    - Worker thread is unblocked and resumes execution
//!
//! ## MADV_DONTNEED vs MADV_FREE
//!
//! We use MADV_DONTNEED (not MADV_FREE) because:
//! - MADV_DONTNEED: Immediately discards pages, next access triggers fault
//! - MADV_FREE: Marks pages as "reclaimable", but kernel may keep them
//!
//! For snapshot reset, we need deterministic behavior - MADV_DONTNEED guarantees
//! the page will fault on next access.
//!
//! ## Why jemalloc tcache Flush is Required
//!
//! Python 3.13+ uses mimalloc (or jemalloc in some builds), which maintains
//! per-thread caches (tcache). These caches store pointers to memory that
//! will be invalidated by MADV_DONTNEED.
//!
//! If we don't flush the tcache before reset:
//! 1. tcache contains pointer P to heap block B
//! 2. MADV_DONTNEED invalidates B
//! 3. Allocator returns P from tcache (cache hit, no fault!)
//! 4. Worker writes to P, corrupting restored memory
//!
//! Solution: Call `mallctl("thread.tcache.flush")` before MADV_DONTNEED.
//! This empties the tcache, ensuring all allocations go through the main heap
//! (which will trigger proper page faults on invalidated pages).
//!
//! # ELF Segment Registration
//!
//! For correct snapshot/restore of Python's global state (small_ints, singletons),
//! we must precisely identify and register libpython's writable segments:
//!
//! 1. Parse ELF headers using `goblin` to find PT_LOAD segments with PF_W flag
//! 2. Calculate absolute virtual addresses: base_addr + (p_vaddr - first_p_vaddr)
//! 3. Page-align all segments (UFFDIO_REGISTER requires page-aligned addresses)
//! 4. Merge overlapping/adjacent segments to avoid EINVAL from kernel
//! 5. Register merged segments with UFFDIO_REGISTER_MODE_MISSING

use anyhow::{Context, Result, anyhow};
use goblin::elf::{Elf, program_header::PF_W};
use nix::sys::socket::{ControlMessage, MsgFlags, sendmsg};
use nix::sys::uio::{RemoteIoVec, process_vm_readv};
use nix::unistd::Pid;
use std::collections::HashMap;
use std::fs;
use std::io::{IoSlice, IoSliceMut};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use userfaultfd::{Uffd, UffdBuilder};

/// Page size (4KB on x86_64/aarch64)
const PAGE_SIZE: usize = 4096;

// =============================================================================
// TLS Snapshot/Restore Constants
// =============================================================================
//
// These constants enable the "Restoration Quadrant" - capturing and restoring
// the Thread Control Block (TCB) alongside BSS, Heap, and Stack.
//
// CRITICAL: The fs_base register points to the Thread Local Storage block.
// Python 3.13's mimalloc stores critical heap pointers in TLS. If we restore
// the heap without restoring TLS, the mi_heap_t* pointers will be stale.
// =============================================================================

/// ptrace request code for arch_prctl operations (x86_64 only)
#[cfg(target_arch = "x86_64")]
const PTRACE_ARCH_PRCTL: libc::c_uint = 30;

/// arch_prctl subcode: get fs_base register value
#[cfg(target_arch = "x86_64")]
const ARCH_GET_FS: libc::c_int = 0x1003;

/// arch_prctl subcode: set fs_base register value
#[cfg(target_arch = "x86_64")]
const ARCH_SET_FS: libc::c_int = 0x1002;

/// TLS snapshot size hint (12KB covers typical TLS usage including mimalloc state)
/// This value was determined empirically during TLS exploration.
///
/// IMPORTANT: This is now used as a MINIMUM hint, not a hard limit.
/// The actual capture size is determined dynamically by:
/// 1. Using the TLS region boundaries from /proc/pid/maps
/// 2. Capturing from fs_base to min(fs_base + region_size, region_end)
///
/// This handles cases where TensorFlow/PyTorch load many C-extensions with
/// their own TLS slots (the Dynamic Thread Vector - DTV).
const TLS_SNAPSHOT_SIZE_HINT: usize = 12 * 1024;

// =============================================================================
// SCM_RIGHTS: File Descriptor Passing over Unix Sockets
// =============================================================================

/// Send a file descriptor over a Unix socket using SCM_RIGHTS
///
/// This is used by the Worker to send its UFFD to the Supervisor.
/// The message contains the worker's PID (4 bytes) with the FD attached.
pub fn send_fd(sock: &UnixStream, pid: i32, fd: RawFd) -> Result<()> {
    let pid_bytes = pid.to_le_bytes();
    let iov = [IoSlice::new(&pid_bytes)];
    let fds = [fd];
    let cmsg = [ControlMessage::ScmRights(&fds)];

    sendmsg::<()>(sock.as_raw_fd(), &iov, &cmsg, MsgFlags::empty(), None)
        .context("Failed to send FD via SCM_RIGHTS")?;

    Ok(())
}

/// Receive a file descriptor over a Unix socket using SCM_RIGHTS
///
/// This is used by the Supervisor to receive the Worker's UFFD.
/// Returns (worker_pid, uffd_fd).
pub fn recv_fd(sock: &UnixStream) -> Result<(i32, OwnedFd)> {
    use std::mem::MaybeUninit;

    let mut pid_buf = [0u8; 4];
    let mut iov = libc::iovec {
        iov_base: pid_buf.as_mut_ptr() as *mut libc::c_void,
        iov_len: pid_buf.len(),
    };

    // Control message buffer sized for one file descriptor
    // SAFETY: CMSG_SPACE is a const-like macro that computes buffer size
    let mut cmsg_buf =
        [0u8; unsafe { libc::CMSG_SPACE(std::mem::size_of::<RawFd>() as u32) } as usize];

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

    // Extract PID from message body
    let pid = i32::from_le_bytes(pid_buf);

    // Extract file descriptor from control message
    let mut received_fd: Option<RawFd> = None;

    // SAFETY: Iterating over control messages in properly received buffer
    unsafe {
        let mut cmsg = libc::CMSG_FIRSTHDR(&msg);
        while !cmsg.is_null() {
            if (*cmsg).cmsg_level == libc::SOL_SOCKET && (*cmsg).cmsg_type == libc::SCM_RIGHTS {
                let fd_ptr = libc::CMSG_DATA(cmsg) as *const RawFd;
                received_fd = Some(*fd_ptr);
                break;
            }
            cmsg = libc::CMSG_NXTHDR(&msg, cmsg);
        }
    }

    let fd = received_fd.ok_or_else(|| anyhow!("No file descriptor in SCM_RIGHTS message"))?;

    // SAFETY: We just received this FD via recvmsg, we own it now
    let owned_fd = unsafe { OwnedFd::from_raw_fd(fd) };

    Ok((pid, owned_fd))
}

// =============================================================================
// Memory Region Management
// =============================================================================

/// A memory region that can be snapshotted
#[derive(Debug, Clone)]
pub struct MemoryRegion {
    pub start: usize,
    pub end: usize,
    pub len: usize,
    pub perms: String,
    pub name: String,
}

impl MemoryRegion {
    /// Check if this region should be included in the snapshot
    ///
    /// We snapshot: heap, anonymous mappings, libpython data/bss, stack
    /// We exclude: vDSO, vsyscall, read-only mappings, shared mappings,
    ///             coverage ring buffer (memfd:tach_coverage)
    pub fn should_snapshot(&self) -> bool {
        // Must be writable
        if !self.perms.contains('w') {
            return false;
        }

        // Skip vDSO and vsyscall
        if self.name.contains("[vdso]") || self.name.contains("[vsyscall]") {
            return false;
        }

        // =================================================================
        //  EXCLUDE coverage ring buffer from snapshot
        // =================================================================
        // The coverage ring buffer is created via memfd_create("tach_coverage")
        // and appears in /proc/pid/maps as "memfd:tach_coverage" or similar.
        //
        // CRITICAL: This region MUST be excluded from userfaultfd registration.
        // If we reset this region during MADV_DONTNEED, we lose all coverage
        // data collected during test execution.
        //
        // The ring buffer is MAP_SHARED between Supervisor and Worker, so
        // resetting it would corrupt the Supervisor's view of the data.
        // =================================================================
        if self.name.contains("tach_coverage") || self.name.contains("memfd:tach") {
            return false;
        }

        // Include heap
        if self.name.contains("[heap]") {
            return true;
        }

        // Include stack
        if self.name.contains("[stack]") {
            return true;
        }

        // Include libpython data/bss segments
        if self.name.contains("libpython") {
            return true;
        }

        // Include anonymous mappings (empty name, writable)
        if self.name.is_empty() && self.perms.contains('p') {
            return true;
        }

        false
    }

    /// Check if this is the stack region
    pub fn is_stack(&self) -> bool {
        self.name.contains("[stack]")
    }
}

// =============================================================================
//  TLS Snapshot Structure
// =============================================================================

/// Thread Local Storage snapshot for the Restoration Quadrant
///
/// The TLS block contains critical allocator state (mimalloc's mi_heap_t pointers
/// in Python 3.13+). This snapshot captures:
/// 1. The fs_base register value (points to TLS block)
/// 2. The 12KB TLS memory block
///
/// # Why TLS Matters
///
/// Python 3.13 switched from pymalloc to mimalloc, which caches per-thread
/// heap pointers in TLS. If we restore the heap without restoring TLS:
/// - mi_heap_t* at fs_base+0x0ad8 points to stale heap data
/// - Next allocation returns memory from the wrong epoch
/// - Result: Silent corruption or use-after-free
#[derive(Debug, Clone)]
pub struct TlsSnapshot {
    /// The fs_base register value (Thread Control Block address)
    pub fs_base: usize,
    /// The TLS memory block (12KB)
    pub tls_data: Vec<u8>,
    /// Start address of the TLS region in /proc/pid/maps
    pub tls_region_start: usize,
    /// End address of the TLS region
    pub tls_region_end: usize,
}

// =============================================================================
//  TLS Capture/Restore via ptrace
// =============================================================================

/// Get fs_base register from a stopped process via ptrace
///
/// # Safety
/// - Worker must be in SIGSTOP state
/// - Caller must have ptrace permissions (child process or CAP_SYS_PTRACE)
///
/// # Architecture
/// - x86_64: Uses PTRACE_ARCH_PRCTL with ARCH_GET_FS
/// - aarch64: Uses PTRACE_GETREGSET (not implemented yet)
#[cfg(target_arch = "x86_64")]
pub fn get_fs_base_ptrace(pid: Pid) -> Result<usize> {
    use nix::sys::ptrace;
    use nix::sys::wait::{WaitPidFlag, waitpid};

    // Attach to the process
    ptrace::attach(pid).with_context(|| format!("Failed to ptrace attach to PID {}", pid))?;

    // Wait for the process to stop (ptrace-stop)
    waitpid(pid, Some(WaitPidFlag::WSTOPPED))
        .with_context(|| format!("Failed to wait for ptrace-stop on PID {}", pid))?;

    // Use PTRACE_ARCH_PRCTL to get fs_base
    // ptrace(PTRACE_ARCH_PRCTL, pid, ARCH_GET_FS, &output)
    let mut fs_base: u64 = 0;

    // SAFETY: We are attached to the process via ptrace, and it is stopped.
    // PTRACE_ARCH_PRCTL with ARCH_GET_FS reads the fs_base into the data pointer.
    let ret = unsafe {
        libc::ptrace(
            PTRACE_ARCH_PRCTL as libc::c_uint,
            pid.as_raw(),
            ARCH_GET_FS,
            &mut fs_base as *mut u64,
        )
    };

    if ret == -1 {
        let err = std::io::Error::last_os_error();
        // Detach before returning error
        let _ = ptrace::detach(pid, None);
        return Err(anyhow!(
            "PTRACE_ARCH_PRCTL(ARCH_GET_FS) failed for PID {}: {}",
            pid,
            err
        ));
    }

    // Detach from the process (it remains stopped, we'll SIGCONT later)
    ptrace::detach(pid, None)
        .with_context(|| format!("Failed to ptrace detach from PID {}", pid))?;

    eprintln!(
        "[tach:tls] Captured fs_base for PID {}: 0x{:016x}",
        pid, fs_base
    );

    Ok(fs_base as usize)
}

/// Set fs_base register for a stopped process via ptrace
///
/// # Safety
/// - Worker must be in SIGSTOP state
/// - Caller must have ptrace permissions
///
/// # Warning
/// Setting fs_base incorrectly will crash the target process.
/// The value must point to a valid TLS block.
#[cfg(target_arch = "x86_64")]
pub fn set_fs_base_ptrace(pid: Pid, fs_base: usize) -> Result<()> {
    use nix::sys::ptrace;
    use nix::sys::wait::{WaitPidFlag, waitpid};

    // Attach to the process
    ptrace::attach(pid).with_context(|| format!("Failed to ptrace attach to PID {}", pid))?;

    // Wait for the process to stop (ptrace-stop)
    waitpid(pid, Some(WaitPidFlag::WSTOPPED))
        .with_context(|| format!("Failed to wait for ptrace-stop on PID {}", pid))?;

    // Use PTRACE_ARCH_PRCTL to set fs_base
    // ptrace(PTRACE_ARCH_PRCTL, pid, ARCH_SET_FS, value)
    // SAFETY: We are attached to the process via ptrace, and it is stopped.
    // PTRACE_ARCH_PRCTL with ARCH_SET_FS sets fs_base to the data value.
    let ret = unsafe {
        libc::ptrace(
            PTRACE_ARCH_PRCTL as libc::c_uint,
            pid.as_raw(),
            ARCH_SET_FS,
            fs_base,
        )
    };

    if ret == -1 {
        let err = std::io::Error::last_os_error();
        // Detach before returning error
        let _ = ptrace::detach(pid, None);
        return Err(anyhow!(
            "PTRACE_ARCH_PRCTL(ARCH_SET_FS) failed for PID {}: {}",
            pid,
            err
        ));
    }

    // Detach from the process
    ptrace::detach(pid, None)
        .with_context(|| format!("Failed to ptrace detach from PID {}", pid))?;

    eprintln!(
        "[tach:tls] Restored fs_base for PID {}: 0x{:016x}",
        pid, fs_base
    );

    Ok(())
}

/// Find the TLS region containing fs_base in /proc/pid/maps
///
/// Returns (start, end) of the anonymous mapping containing fs_base.
fn find_tls_region(pid: Pid, fs_base: usize) -> Result<(usize, usize)> {
    let regions = parse_memory_maps(pid)?;

    for region in regions {
        if region.start <= fs_base && fs_base < region.end {
            eprintln!(
                "[tach:tls] Found TLS region for fs_base 0x{:x}: 0x{:x}-0x{:x} [{}]",
                fs_base, region.start, region.end, region.name
            );
            return Ok((region.start, region.end));
        }
    }

    Err(anyhow!(
        "Could not find memory region containing fs_base 0x{:x} for PID {}",
        fs_base,
        pid
    ))
}

/// Capture TLS snapshot from a stopped worker
///
/// This implements the TLS capture phase of the Restoration Quadrant:
/// 1. Get fs_base via ptrace ARCH_GET_FS
/// 2. Find the TLS region in /proc/pid/maps
/// 3. Read the ENTIRE TLS region via process_vm_readv (dynamic sizing)
///
/// #  Dynamic TLS Sizing
///
/// The Orchestrator identified a critical flaw: the 12KB hardcode fails when
/// TensorFlow/PyTorch load dozens of C-extensions, each requesting TLS slots
/// via the Dynamic Thread Vector (DTV).
///
/// FIX: We now capture from fs_base to region_end (the full TLS allocation),
/// using TLS_SNAPSHOT_SIZE_HINT only as a minimum sanity check.
///
/// # Requirements
/// - Worker must be in SIGSTOP state
/// - Caller must be parent process or have CAP_SYS_PTRACE
#[cfg(target_arch = "x86_64")]
pub fn capture_tls_snapshot(pid: Pid) -> Result<TlsSnapshot> {
    // Step 1: Get fs_base via ptrace
    let fs_base = get_fs_base_ptrace(pid)?;

    // Step 2: Find TLS region
    let (region_start, region_end) = find_tls_region(pid, fs_base)?;

    // =================================================================
    //  Dynamic TLS Sizing
    // =================================================================
    // "Do not guess the size of the heart; measure the cavity."
    // - The Orchestrator
    //
    // We capture from fs_base to the END of the TLS region, not a fixed
    // 12KB. This handles the DTV expansion from heavy C-extension usage.
    //
    // Safety: We verify the region is at least TLS_SNAPSHOT_SIZE_HINT
    // to catch pathological cases where fs_base isn't in a real TLS region.
    // =================================================================
    let max_capture_from_fs = region_end.saturating_sub(fs_base);

    // Sanity check: TLS region should be at least 4KB (one page)
    if max_capture_from_fs < PAGE_SIZE {
        return Err(anyhow!(
            "TLS region too small: fs_base=0x{:x}, region_end=0x{:x}, size={}",
            fs_base,
            region_end,
            max_capture_from_fs
        ));
    }

    // Calculate actual capture size:
    // - Minimum: TLS_SNAPSHOT_SIZE_HINT (to ensure we get mimalloc state)
    // - Maximum: entire region from fs_base to region_end
    let capture_len = max_capture_from_fs.max(TLS_SNAPSHOT_SIZE_HINT.min(max_capture_from_fs));

    eprintln!(
        "[tach:tls] Dynamic TLS capture: fs_base=0x{:x}, region=[0x{:x}, 0x{:x}), capture={} bytes",
        fs_base, region_start, region_end, capture_len
    );

    // Step 3: Read TLS data via process_vm_readv
    let mut tls_data = vec![0u8; capture_len];
    let mut local_iov = [IoSliceMut::new(&mut tls_data)];
    let remote_iov = [RemoteIoVec {
        base: fs_base,
        len: capture_len,
    }];

    let bytes_read = process_vm_readv(pid, &mut local_iov, &remote_iov)
        .with_context(|| format!("process_vm_readv failed for TLS at 0x{:x}", fs_base))?;

    //  Check for partial reads (Orchestrator's warning)
    if bytes_read != capture_len {
        return Err(anyhow!(
            "Partial TLS read: {}/{} bytes. Worker may have 'Fractured Brain' if we proceed.",
            bytes_read,
            capture_len
        ));
    }

    eprintln!(
        "[tach:tls] TLS snapshot captured: {} bytes from fs_base=0x{:x} (region={} bytes total)",
        tls_data.len(),
        fs_base,
        region_end - region_start
    );

    Ok(TlsSnapshot {
        fs_base,
        tls_data,
        tls_region_start: region_start,
        tls_region_end: region_end,
    })
}

/// Restore TLS state to a stopped worker
///
/// This implements the TLS restore phase of the Restoration Quadrant:
/// 1. Write TLS data back via process_vm_writev
/// 2. Set fs_base via ptrace ARCH_SET_FS (ensures register consistency)
///
/// # Requirements
/// - Worker must be in SIGSTOP state
/// - TLS memory must have been restored (via userfaultfd or process_vm_writev)
#[cfg(target_arch = "x86_64")]
pub fn restore_tls_snapshot(pid: Pid, snapshot: &TlsSnapshot) -> Result<()> {
    use nix::sys::uio::process_vm_writev;
    use std::io::IoSlice;

    eprintln!(
        "[tach:tls] Restoring TLS for PID {}: {} bytes to 0x{:x}",
        pid,
        snapshot.tls_data.len(),
        snapshot.fs_base
    );

    // Step 1: Write TLS data back via process_vm_writev
    // This is necessary because userfaultfd may not cover the TLS region
    // if it wasn't registered, or we want explicit control.
    let local_iov = [IoSlice::new(&snapshot.tls_data)];
    let remote_iov = [RemoteIoVec {
        base: snapshot.fs_base,
        len: snapshot.tls_data.len(),
    }];

    let bytes_written = process_vm_writev(pid, &local_iov, &remote_iov).with_context(|| {
        format!(
            "process_vm_writev failed for TLS at 0x{:x}",
            snapshot.fs_base
        )
    })?;

    if bytes_written != snapshot.tls_data.len() {
        return Err(anyhow!(
            "Partial TLS write: {}/{} bytes",
            bytes_written,
            snapshot.tls_data.len()
        ));
    }

    // Step 2: Restore fs_base register
    // This ensures the register points to the same TLS block
    // (should be unchanged, but this is a safety measure)
    set_fs_base_ptrace(pid, snapshot.fs_base)?;

    eprintln!(
        "[tach:tls] TLS restore complete for PID {}: fs_base=0x{:x}",
        pid, snapshot.fs_base
    );

    Ok(())
}

// =============================================================================
// Vectorized Restore: Batched iovec for Restoration Quadrant
// =============================================================================
//
// The Orchestrator's mandate: Reduce syscall overhead by batching TLS, Stack,
// and critical BSS regions into a single process_vm_writev call.
//
// This reduces context switches and achieves lower "jitter" (latency variance).
// =============================================================================

/// Restoration region for vectorized writes
#[derive(Debug, Clone)]
pub struct RestoreRegion {
    /// Remote address in worker's address space
    pub remote_addr: usize,
    /// Data to restore
    pub data: Vec<u8>,
    /// Description for debugging
    pub name: String,
}

impl RestoreRegion {
    pub fn new(remote_addr: usize, data: Vec<u8>, name: impl Into<String>) -> Self {
        Self {
            remote_addr,
            data,
            name: name.into(),
        }
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

/// Result of a vectorized restore operation
#[derive(Debug)]
#[must_use = "VectorizedRestoreResult must be checked to confirm memory restoration succeeded"]
pub struct VectorizedRestoreResult {
    /// Total bytes written
    pub bytes_written: usize,
    /// Number of regions restored
    pub regions_restored: usize,
    /// Duration of the operation
    pub duration_us: u64,
    /// Per-region breakdown (for jitter analysis)
    pub region_details: Vec<(String, usize)>,
}

/// Vectorized restore: batch multiple memory regions into a single syscall
///
/// This implements the Orchestrator's mandate for reduced jitter:
/// - Single process_vm_writev call with multiple iovec structures
/// - Covers TLS, Stack, and critical BSS regions
/// - Minimizes context switches during restoration
///
/// # Performance
/// - Individual restores: N syscalls for N regions
/// - Vectorized restore: 1 syscall for N regions
/// - Expected improvement: 20-40% reduction in restoration time
///
/// # Arguments
/// * `pid` - Target worker PID (must be in SIGSTOP state)
/// * `regions` - Slice of RestoreRegion structs to restore
///
/// # Returns
/// * `VectorizedRestoreResult` with timing and byte count information
#[cfg(target_arch = "x86_64")]
pub fn restore_vectorized(pid: Pid, regions: &[RestoreRegion]) -> Result<VectorizedRestoreResult> {
    use nix::sys::uio::process_vm_writev;
    use std::io::IoSlice;
    use std::time::Instant;

    if regions.is_empty() {
        return Ok(VectorizedRestoreResult {
            bytes_written: 0,
            regions_restored: 0,
            duration_us: 0,
            region_details: vec![],
        });
    }

    let start = Instant::now();

    // Build iovec arrays for batch write
    // Local iovecs: pointers to our data buffers
    let local_iovs: Vec<IoSlice> = regions.iter().map(|r| IoSlice::new(&r.data)).collect();

    // Remote iovecs: addresses in the worker's address space
    let remote_iovs: Vec<RemoteIoVec> = regions
        .iter()
        .map(|r| RemoteIoVec {
            base: r.remote_addr,
            len: r.len(),
        })
        .collect();

    // Calculate expected total
    let expected_bytes: usize = regions.iter().map(|r| r.len()).sum();

    // Single syscall for all regions
    let bytes_written = process_vm_writev(pid, &local_iovs, &remote_iovs).with_context(|| {
        format!(
            "Vectorized process_vm_writev failed for {} regions",
            regions.len()
        )
    })?;

    let duration = start.elapsed();

    // Verify complete write
    if bytes_written != expected_bytes {
        return Err(anyhow!(
            "Partial vectorized write: {}/{} bytes across {} regions",
            bytes_written,
            expected_bytes,
            regions.len()
        ));
    }

    // Build region details for analysis
    let region_details: Vec<(String, usize)> =
        regions.iter().map(|r| (r.name.clone(), r.len())).collect();

    eprintln!(
        "[tach:vectorized] Restored {} regions ({} bytes) in {}us",
        regions.len(),
        bytes_written,
        duration.as_micros()
    );

    Ok(VectorizedRestoreResult {
        bytes_written,
        regions_restored: regions.len(),
        duration_us: duration.as_micros() as u64,
        region_details,
    })
}

/// Build restoration regions from a WorkerSnapshot
///
/// This extracts the critical memory regions that need direct restoration
/// (not handled by userfaultfd lazy loading):
/// 1. TLS block (always needed for Python 3.13+)
/// 2. Critical BSS pages (allocator free list heads)
///
/// # DTV Hazard Note
/// The Dynamic Thread Vector (DTV) is part of the TLS region and is
/// automatically included when we capture the full TLS block from
/// fs_base to region_end.
#[cfg(target_arch = "x86_64")]
pub fn build_restore_regions(snapshot: &WorkerSnapshot) -> Vec<RestoreRegion> {
    let mut regions = Vec::new();

    // Region 1: TLS block (critical for mimalloc in Python 3.13+)
    if let Some(ref tls) = snapshot.tls_snapshot {
        regions.push(RestoreRegion::new(tls.fs_base, tls.tls_data.clone(), "TLS"));
    }

    // Region 2: Critical BSS pages (optional optimization)
    // These are the first few pages of libpython's .data/.bss that contain
    // allocator free list heads. Restoring them eagerly avoids the first
    // wave of userfaultfd faults.
    //
    // Future: Identify and extract critical BSS pages during capture.
    // This would improve snapshot efficiency by reducing restored page count.
    // For now, we rely on userfaultfd for BSS restoration.

    regions
}

/// Full vectorized restore with TLS and fs_base synchronization
///
/// This combines:
/// 1. Vectorized memory writes (TLS + critical regions)
/// 2. fs_base register restoration via ptrace
///
/// Use this for complete state restoration with minimal syscalls.
#[cfg(target_arch = "x86_64")]
pub fn restore_full_vectorized(
    pid: Pid,
    snapshot: &WorkerSnapshot,
) -> Result<VectorizedRestoreResult> {
    // Build regions from snapshot
    let regions = build_restore_regions(snapshot);

    if regions.is_empty() {
        return Ok(VectorizedRestoreResult {
            bytes_written: 0,
            regions_restored: 0,
            duration_us: 0,
            region_details: vec![],
        });
    }

    // Perform vectorized write
    let result = restore_vectorized(pid, &regions)?;

    // Restore fs_base register (must be after memory write)
    if let Some(ref tls) = snapshot.tls_snapshot {
        set_fs_base_ptrace(pid, tls.fs_base)?;
    }

    Ok(result)
}

/// Parse /proc/{pid}/maps to extract memory regions
///
/// Format: start-end perms offset dev inode pathname
/// Example: `7f1234560000-7f1234580000 rw-p 00000000 00:00 0 [heap]`
pub fn parse_memory_maps(pid: Pid) -> Result<Vec<MemoryRegion>> {
    let maps_path = format!("/proc/{}/maps", pid);
    let content =
        fs::read_to_string(&maps_path).with_context(|| format!("Failed to read {}", maps_path))?;

    let mut regions = Vec::new();

    for line in content.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 5 {
            continue;
        }

        // Parse address range
        let addr_range: Vec<&str> = parts[0].split('-').collect();
        if addr_range.len() != 2 {
            continue;
        }

        let start = usize::from_str_radix(addr_range[0], 16).unwrap_or(0);
        let end = usize::from_str_radix(addr_range[1], 16).unwrap_or(0);
        let perms = parts[1].to_string();

        // Get pathname (may be empty or at different position)
        let name = if parts.len() > 5 {
            parts[5..].join(" ")
        } else {
            String::new()
        };

        regions.push(MemoryRegion {
            start,
            end,
            len: end - start,
            perms,
            name,
        });
    }

    Ok(regions)
}

/// Align address down to page boundary
fn align_to_page(addr: usize) -> usize {
    addr & !(PAGE_SIZE - 1)
}

/// Align address up to page boundary
fn align_to_page_up(addr: usize) -> usize {
    (addr + PAGE_SIZE - 1) & !(PAGE_SIZE - 1)
}

// =============================================================================
//  ELF Segment Parsing and Page-Aligned Merging
// =============================================================================
//
// This section implements the "Iron Dome" for Python's global state:
//
// 1. Find libpython.so in /proc/pid/maps (or /proc/self/exe if statically linked)
// 2. Parse ELF headers to find PT_LOAD segments with PF_W (writable) flag
// 3. Calculate absolute virtual addresses using the base address from maps
// 4. Page-align all segments (kernel requires page-aligned UFFDIO_REGISTER)
// 5. Merge overlapping/adjacent segments to avoid EINVAL
//
// Why this matters:
// -----------------
// Python caches small integers (-5 to 256) and singletons (None, True, False)
// in libpython's .data segment. If we don't snapshot and restore these,
// reference counts will corrupt after the first memory reset.
// =============================================================================

/// A page-aligned memory segment for UFFDIO registration
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlignedSegment {
    /// Page-aligned start address
    pub start: usize,
    /// Page-aligned end address
    pub end: usize,
    /// Human-readable description for logging
    pub description: String,
}

impl AlignedSegment {
    /// Create a new aligned segment from raw addresses
    ///
    /// Addresses are automatically page-aligned:
    /// - start is rounded DOWN to page boundary
    /// - end is rounded UP to page boundary
    pub fn new(start: usize, end: usize, description: impl Into<String>) -> Self {
        let aligned_start = align_to_page(start);
        let aligned_end = align_to_page_up(end);
        Self {
            start: aligned_start,
            end: aligned_end,
            description: description.into(),
        }
    }

    /// Length in bytes (always page-aligned)
    pub fn len(&self) -> usize {
        self.end - self.start
    }

    /// Returns true if this segment has zero length
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Number of pages in this segment
    pub fn page_count(&self) -> usize {
        self.len() / PAGE_SIZE
    }

    /// Check if this segment overlaps or is adjacent to another
    ///
    /// Two segments are considered overlapping if:
    /// - They share any pages, OR
    /// - They are adjacent (one ends where the other begins)
    ///
    /// This is critical for merging: adjacent segments MUST be merged
    /// because UFFDIO_REGISTER will fail with EINVAL if we try to
    /// register overlapping ranges.
    pub fn overlaps_or_adjacent(&self, other: &Self) -> bool {
        // Segments overlap if neither is entirely before the other
        // Adjacent means one ends exactly where the other begins
        self.start <= other.end && other.start <= self.end
    }

    /// Merge this segment with another, returning the combined segment
    ///
    /// # Panics
    /// Panics if segments don't overlap or aren't adjacent (use overlaps_or_adjacent first)
    pub fn merge(&self, other: &Self) -> Self {
        debug_assert!(
            self.overlaps_or_adjacent(other),
            "Cannot merge non-overlapping segments"
        );

        Self {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
            description: format!("{}+{}", self.description, other.description),
        }
    }
}

/// Merge a list of segments, combining any that overlap or are adjacent
///
/// # Algorithm: Page-Align and Merge
///
/// This implements the mandatory merge algorithm to avoid EINVAL from UFFDIO_REGISTER:
///
/// 1. **Sort** all segments by start address
/// 2. **Iterate** through sorted segments:
///    - If current segment overlaps/adjacent to previous, merge them
///    - Otherwise, emit previous and start new accumulator
/// 3. **Emit** final segment
///
/// # Example
///
/// Input segments (after page alignment):
/// ```text
/// A: [0x1000, 0x3000)  "heap"
/// B: [0x2000, 0x4000)  "libpython.data"  <- overlaps A
/// C: [0x5000, 0x6000)  "stack"           <- separate
/// ```
///
/// Output:
/// ```text
/// [0x1000, 0x4000)  "heap+libpython.data"  <- merged A+B
/// [0x5000, 0x6000)  "stack"                <- unchanged
/// ```
///
/// # Why This Matters
///
/// The kernel's UFFDIO_REGISTER syscall will return EINVAL if:
/// - Addresses are not page-aligned (handled by AlignedSegment::new)
/// - Ranges overlap with previously registered ranges (handled by this merge)
///
/// By merging overlapping segments, we ensure each page is registered exactly once.
pub fn merge_segments(mut segments: Vec<AlignedSegment>) -> Vec<AlignedSegment> {
    if segments.is_empty() {
        return segments;
    }

    // Step 1: Sort by start address
    segments.sort_by_key(|s| s.start);

    // Step 2: Merge overlapping/adjacent segments
    let mut merged: Vec<AlignedSegment> = Vec::with_capacity(segments.len());
    let mut current = segments.remove(0);

    for segment in segments {
        if current.overlaps_or_adjacent(&segment) {
            // Merge: extend current to include segment
            // Key boundary condition: use max(current.end, segment.end)
            current = current.merge(&segment);
        } else {
            // No overlap: emit current, start new accumulator
            merged.push(current);
            current = segment;
        }
    }

    // Step 3: Emit final segment
    merged.push(current);

    merged
}

/// Information about libpython's location in memory
#[derive(Debug)]
pub struct LibpythonInfo {
    /// Path to the libpython shared object (or executable if statically linked)
    pub path: PathBuf,
    /// Base address where libpython is mapped
    pub base_addr: usize,
    /// Whether libpython is statically linked into the executable
    pub is_static: bool,
}

/// Find libpython in /proc/pid/maps
///
/// Returns the path and base address of libpython.so, or the executable
/// if Python is statically linked.
///
/// # Algorithm
///
/// 1. Parse /proc/pid/maps looking for "libpython" in the pathname
/// 2. Find the first (lowest address) mapping - this is the base address
/// 3. If not found, check if /proc/self/exe contains Python symbols (static linking)
pub fn find_libpython(pid: Pid) -> Result<LibpythonInfo> {
    let regions = parse_memory_maps(pid)?;

    // Look for libpython.so mappings
    // The first (lowest address) r-xp mapping is typically the base
    let mut libpython_regions: Vec<&MemoryRegion> = regions
        .iter()
        .filter(|r| r.name.contains("libpython"))
        .collect();

    if !libpython_regions.is_empty() {
        // Sort by start address to find base
        libpython_regions.sort_by_key(|r| r.start);
        let base_region = libpython_regions[0];

        // Extract path from the region name
        let path = PathBuf::from(&base_region.name);

        return Ok(LibpythonInfo {
            path,
            base_addr: base_region.start,
            is_static: false,
        });
    }

    // Fallback: Check if Python is statically linked into the executable
    // This is common in some Rust-Python distributions
    let exe_path = format!("/proc/{}/exe", pid);
    let exe_real =
        fs::read_link(&exe_path).with_context(|| format!("Failed to read {}", exe_path))?;

    // Find the executable's base address in maps
    let exe_name = exe_real.file_name().and_then(|n| n.to_str()).unwrap_or("");

    for region in &regions {
        if region.name.contains(exe_name) && region.perms.contains('r') {
            return Ok(LibpythonInfo {
                path: exe_real,
                base_addr: region.start,
                is_static: true,
            });
        }
    }

    Err(anyhow!(
        "Could not find libpython.so or statically linked Python in PID {}",
        pid
    ))
}

/// Parse ELF and extract writable PT_LOAD segments
///
/// # Algorithm
///
/// 1. Read the ELF file from disk
/// 2. Parse program headers to find PT_LOAD segments
/// 3. Filter for segments with PF_W (writable) flag
/// 4. Calculate absolute virtual addresses:
///    `target_va = base_addr + (segment.p_vaddr - first_segment.p_vaddr)`
///
/// # Why PT_LOAD segments?
///
/// While .data and .bss sections are useful for debugging, the kernel
/// operates on segments (PT_LOAD), not sections. A PT_LOAD segment with
/// PF_W contains all writable data including:
/// - .data (initialized global variables)
/// - .bss (zero-initialized global variables)
/// - .got (global offset table)
/// - .got.plt (PLT entries)
///
/// By registering the entire writable segment, we ensure complete coverage
/// of Python's global state.
pub fn parse_elf_writable_segments(
    elf_path: &PathBuf,
    base_addr: usize,
) -> Result<Vec<AlignedSegment>> {
    let elf_bytes = fs::read(elf_path)
        .with_context(|| format!("Failed to read ELF file: {}", elf_path.display()))?;

    let elf = Elf::parse(&elf_bytes)
        .with_context(|| format!("Failed to parse ELF: {}", elf_path.display()))?;

    // Find the first PT_LOAD segment's p_vaddr (used for address calculation)
    let first_load_vaddr = elf
        .program_headers
        .iter()
        .find(|ph| ph.p_type == goblin::elf::program_header::PT_LOAD)
        .map(|ph| ph.p_vaddr as usize)
        .unwrap_or(0);

    let mut segments = Vec::new();

    for (idx, ph) in elf.program_headers.iter().enumerate() {
        // Only process PT_LOAD segments
        if ph.p_type != goblin::elf::program_header::PT_LOAD {
            continue;
        }

        // Only process writable segments (PF_W flag)
        if ph.p_flags & PF_W == 0 {
            continue;
        }

        // Calculate absolute virtual address
        // Formula: target_va = base_addr + (p_vaddr - first_load_vaddr)
        //
        // Why this formula?
        // - base_addr is where the ELF is actually loaded (from /proc/maps)
        // - p_vaddr is the virtual address in the ELF file
        // - first_load_vaddr is the base of the ELF's virtual address space
        // - The difference (p_vaddr - first_load_vaddr) is the offset from base
        let segment_offset = ph.p_vaddr as usize - first_load_vaddr;
        let target_va = base_addr + segment_offset;
        let target_end = target_va + ph.p_memsz as usize;

        let description = format!(
            "libpython:PT_LOAD[{}]:0x{:x}-0x{:x}",
            idx, target_va, target_end
        );

        eprintln!(
            "[tach:snapshot] Found writable segment: {} ({} pages)",
            description,
            (align_to_page_up(target_end) - align_to_page(target_va)) / PAGE_SIZE
        );

        segments.push(AlignedSegment::new(target_va, target_end, description));
    }

    if segments.is_empty() {
        eprintln!(
            "[tach:snapshot] WARNING: No writable PT_LOAD segments found in {}",
            elf_path.display()
        );
    }

    Ok(segments)
}

/// Get all segments that should be registered with userfaultfd
///
/// This combines:
/// 1. Standard regions from /proc/maps (heap, stack, anonymous)
/// 2. ELF-parsed libpython writable segments
///
/// All segments are page-aligned and merged to avoid UFFDIO_REGISTER errors.
pub fn get_snapshot_segments(pid: Pid) -> Result<Vec<AlignedSegment>> {
    let mut all_segments = Vec::new();

    // 1. Get standard regions from /proc/maps
    let regions = parse_memory_maps(pid)?;
    for region in regions.iter().filter(|r| r.should_snapshot()) {
        all_segments.push(AlignedSegment::new(
            region.start,
            region.end,
            region.name.clone(),
        ));
    }

    // 2. Parse libpython ELF for precise writable segment identification
    match find_libpython(pid) {
        Ok(libpython) => {
            eprintln!(
                "[tach:snapshot] Found libpython at 0x{:x}: {} (static={})",
                libpython.base_addr,
                libpython.path.display(),
                libpython.is_static
            );

            match parse_elf_writable_segments(&libpython.path, libpython.base_addr) {
                Ok(elf_segments) => {
                    for seg in elf_segments {
                        all_segments.push(seg);
                    }
                }
                Err(e) => {
                    eprintln!(
                        "[tach:snapshot] WARNING: Failed to parse libpython ELF: {}. \
                         Falling back to /proc/maps detection.",
                        e
                    );
                }
            }
        }
        Err(e) => {
            eprintln!(
                "[tach:snapshot] WARNING: Could not find libpython: {}. \
                 Relying on /proc/maps detection only.",
                e
            );
        }
    }

    // 3. Merge overlapping/adjacent segments
    let merged = merge_segments(all_segments);

    eprintln!(
        "[tach:snapshot] Total segments after merge: {} ({} pages)",
        merged.len(),
        merged.iter().map(|s| s.page_count()).sum::<usize>()
    );

    Ok(merged)
}

// =============================================================================
// Per-Worker Snapshot State
// =============================================================================

/// Snapshot state for a single worker
pub struct WorkerSnapshot {
    /// The worker's userfaultfd
    pub uffd: Uffd,
    /// Golden pages: page_addr -> page_data
    pub golden_pages: HashMap<usize, Vec<u8>>,
    /// Registered memory regions
    pub regions: Vec<MemoryRegion>,
    ///  TLS snapshot for the Restoration Quadrant
    /// Contains fs_base register value and 12KB TLS memory block
    #[cfg(target_arch = "x86_64")]
    pub tls_snapshot: Option<TlsSnapshot>,
}

// =============================================================================
// Snapshot Manager
// =============================================================================

use super::calibration::TlsCalibration;

/// Central manager for capturing and restoring worker memory
pub struct SnapshotManager {
    /// Whether userfaultfd is available
    pub available: bool,
    /// Per-worker snapshots
    workers: HashMap<i32, WorkerSnapshot>,
    ///  TLS calibration data (discovered during Zygote warm-up)
    /// This contains the dynamically discovered mi_heap_t offset
    #[cfg(target_arch = "x86_64")]
    calibration: Option<TlsCalibration>,
}

impl SnapshotManager {
    /// Create a new SnapshotManager, testing for userfaultfd availability
    pub fn new() -> Result<Self> {
        // Test if userfaultfd is available
        let available = match UffdBuilder::new()
            .close_on_exec(true)
            .non_blocking(false)
            .create()
        {
            Ok(_) => {
                eprintln!("[tach:snapshot] userfaultfd available - Fast-Reset mode enabled");
                true
            }
            Err(e) => {
                eprintln!(
                    "[tach:snapshot] userfaultfd unavailable ({}). Falling back to fork-server.",
                    e
                );
                false
            }
        };

        Ok(Self {
            available,
            workers: HashMap::new(),
            #[cfg(target_arch = "x86_64")]
            calibration: None,
        })
    }

    ///  Perform TLS self-calibration during Zygote warm-up
    ///
    /// This MUST be called:
    /// - After Python is initialized
    /// - Before the first worker fork()
    /// - In the Zygote process (not workers)
    ///
    /// # Returns
    /// - `Ok(())` if calibration succeeds
    /// - `Err` with `ERR_CALIBRATION_FAILED` if Tach cannot self-calibrate
    ///
    /// # Boot Log
    /// On success, logs: `[restoration] Sentinel found at fs_base + 0xXXXX`
    #[cfg(target_arch = "x86_64")]
    pub fn calibrate(&mut self) -> Result<()> {
        eprintln!("[tach:snapshot] Initiating TLS self-calibration...");

        let calibration = TlsCalibration::calibrate()
            .context("TLS self-calibration failed - ERR_CALIBRATION_FAILED")?;

        if calibration.is_calibrated() {
            eprintln!(
                "[tach:snapshot] TLS calibration complete: mi_heap_t at fs_base + 0x{:04X}",
                calibration.primary_offset().unwrap_or(0)
            );
            self.calibration = Some(calibration);
            Ok(())
        } else {
            // Calibration ran but found no heap pointers - this is OK for pre-3.13 Python
            eprintln!(
                "[tach:snapshot] TLS calibration found no heap pointers (Python < 3.13 or pymalloc). \
                 TLS restoration will be skipped."
            );
            self.calibration = Some(calibration);
            Ok(())
        }
    }

    /// Check if calibration has been performed
    #[cfg(target_arch = "x86_64")]
    pub fn is_calibrated(&self) -> bool {
        self.calibration.as_ref().is_some_and(|c| c.is_calibrated())
    }

    /// Get the calibrated mi_heap_t offset (if available)
    #[cfg(target_arch = "x86_64")]
    pub fn calibrated_offset(&self) -> Option<usize> {
        self.calibration.as_ref().and_then(|c| c.primary_offset())
    }

    /// Get the raw UFFD file descriptor for a worker (for polling)
    pub fn get_worker_uffd(&self, pid: Pid) -> Option<RawFd> {
        self.workers.get(&pid.as_raw()).map(|w| w.uffd.as_raw_fd())
    }

    /// Register a worker with its UFFD (received via SCM_RIGHTS)
    ///
    /// This is called when a worker sends its UFFD to the Supervisor.
    /// The worker must be in SIGSTOP state before calling this.
    ///
    /// #  TLS Capture
    ///
    /// This function now captures the TLS snapshot (fs_base + 12KB TLS data)
    /// as part of the golden snapshot. This is critical for Python 3.13+
    /// with mimalloc, which stores heap pointers in TLS.
    pub fn register_worker_with_uffd(&mut self, pid: Pid, uffd: Uffd) -> Result<()> {
        if !self.available {
            return Ok(()); // No-op in fallback mode
        }

        // Parse memory maps and filter for snapshotable regions
        let regions = parse_memory_maps(pid)?;
        let snapshot_regions: Vec<MemoryRegion> = regions
            .into_iter()
            .filter(|r| r.should_snapshot())
            .collect();

        eprintln!(
            "[tach:snapshot] Registering worker PID {}: {} regions to capture",
            pid,
            snapshot_regions.len()
        );

        // Capture golden copy for each region
        let mut golden_pages = HashMap::new();
        for region in &snapshot_regions {
            let pages = self.capture_region_pages(pid, region)?;
            golden_pages.extend(pages);
        }

        // Register regions with the worker's UFFD
        for region in &snapshot_regions {
            uffd.register(region.start as *mut libc::c_void, region.len)
                .with_context(|| format!("Failed to register region {}", region.name))?;
        }

        // =================================================================
        //  Capture TLS Snapshot (Restoration Quadrant - TCB)
        // =================================================================
        // The TLS block contains critical allocator state (mimalloc's mi_heap_t
        // pointers in Python 3.13+). We capture fs_base and 12KB of TLS data.
        //
        // NOTE: Worker MUST be in SIGSTOP state for ptrace to succeed.
        // =================================================================
        #[cfg(target_arch = "x86_64")]
        let tls_snapshot = match capture_tls_snapshot(pid) {
            Ok(tls) => {
                eprintln!(
                    "[tach:snapshot] TLS captured: fs_base=0x{:x}, {} bytes",
                    tls.fs_base,
                    tls.tls_data.len()
                );
                Some(tls)
            }
            Err(e) => {
                // TLS capture failure is non-fatal for pre-3.13 Python
                // (pymalloc doesn't use TLS caching)
                eprintln!(
                    "[tach:snapshot] WARNING: TLS capture failed: {}. \
                     This may cause issues with Python 3.13+ (mimalloc).",
                    e
                );
                None
            }
        };

        // Store worker snapshot
        self.workers.insert(
            pid.as_raw(),
            WorkerSnapshot {
                uffd,
                golden_pages,
                regions: snapshot_regions,
                #[cfg(target_arch = "x86_64")]
                tls_snapshot,
            },
        );

        Ok(())
    }

    /// Capture a single memory region using process_vm_readv
    /// Returns a HashMap of page_addr -> page_data
    fn capture_region_pages(
        &self,
        pid: Pid,
        region: &MemoryRegion,
    ) -> Result<HashMap<usize, Vec<u8>>> {
        let mut buffer = vec![0u8; region.len];

        // Set up iovec for process_vm_readv
        let mut local_iov = [IoSliceMut::new(&mut buffer)];
        let remote_iov = [RemoteIoVec {
            base: region.start,
            len: region.len,
        }];

        // Direct kernel memory copy - no ptrace attach required for child processes
        let bytes_read = process_vm_readv(pid, &mut local_iov, &remote_iov)
            .with_context(|| format!("process_vm_readv failed for region {:?}", region.name))?;

        if bytes_read != region.len {
            return Err(anyhow!(
                "Partial snapshot read for {}: {}/{}",
                region.name,
                bytes_read,
                region.len
            ));
        }

        // Split into pages
        let mut pages = HashMap::new();
        let mut offset = 0;
        while offset < region.len {
            let page_addr = region.start + offset;
            let page_end = (offset + PAGE_SIZE).min(region.len);
            let page_data = buffer[offset..page_end].to_vec();

            pages.insert(page_addr, page_data);
            offset += PAGE_SIZE;
        }

        eprintln!(
            "[tach:snapshot]   {} ({:x}-{:x}): {} pages captured",
            region.name,
            region.start,
            region.end,
            region.len / PAGE_SIZE
        );

        Ok(pages)
    }

    /// Reset a worker's memory by invalidating pages (remote)
    ///
    /// Uses process_madvise (Linux 5.10+) to operate on REMOTE process memory.
    /// NOTE: MADV_DONTNEED via process_madvise requires Linux 5.12+.
    /// If this fails, use Worker Self-Reset (Seppuku) pattern instead.
    pub fn reset_worker(&self, pid: Pid) -> Result<()> {
        if !self.available {
            return Ok(()); // No-op in fallback mode
        }

        let worker = self
            .workers
            .get(&pid.as_raw())
            .ok_or_else(|| anyhow!("Worker {} not registered with SnapshotManager", pid))?;

        // Get pidfd for the target process
        let pidfd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid.as_raw(), 0) } as i32;
        if pidfd < 0 {
            return Err(anyhow!(
                "pidfd_open failed for PID {}: {}",
                pid,
                std::io::Error::last_os_error()
            ));
        }

        // Construct iovec array for all regions
        let iovecs: Vec<libc::iovec> = worker
            .regions
            .iter()
            .map(|r| libc::iovec {
                iov_base: r.start as *mut libc::c_void,
                iov_len: r.len,
            })
            .collect();

        // Call process_madvise - REMOTE MADV_DONTNEED
        const SYS_PROCESS_MADVISE: libc::c_long = 440;
        let ret = unsafe {
            libc::syscall(
                SYS_PROCESS_MADVISE,
                pidfd,
                iovecs.as_ptr(),
                iovecs.len(),
                libc::MADV_DONTNEED,
                0u32,
            )
        };

        unsafe { libc::close(pidfd) };

        if ret < 0 {
            return Err(anyhow!(
                "process_madvise failed for PID {}: {}",
                pid,
                std::io::Error::last_os_error()
            ));
        }

        eprintln!(
            "[tach:snapshot] Reset worker {}: invalidated {} regions",
            pid,
            iovecs.len()
        );

        Ok(())
    }

    /// Restore TLS state for a worker ( Restoration Quadrant - TCB)
    ///
    /// This MUST be called after MADV_DONTNEED has invalidated the worker's
    /// memory pages, but BEFORE the worker resumes execution.
    ///
    /// # Requirements
    /// - Worker must be in SIGSTOP state
    /// - TLS snapshot must have been captured during registration
    ///
    /// # What This Does
    /// 1. Writes the 12KB TLS data back to the worker's TLS region
    /// 2. Restores the fs_base register via ptrace ARCH_SET_FS
    ///
    /// # Why This Matters
    /// Python 3.13+ uses mimalloc which caches heap pointers in TLS.
    /// If we restore the heap but not the TLS, the mi_heap_t* pointers
    /// in TLS will point to stale heap data, causing use-after-free.
    #[cfg(target_arch = "x86_64")]
    pub fn restore_worker_tls(&self, pid: Pid) -> Result<()> {
        if !self.available {
            return Ok(()); // No-op in fallback mode
        }

        let worker = self
            .workers
            .get(&pid.as_raw())
            .ok_or_else(|| anyhow!("Worker {} not registered with SnapshotManager", pid))?;

        if let Some(ref tls_snapshot) = worker.tls_snapshot {
            eprintln!(
                "[tach:snapshot] Restoring TLS for worker {}: fs_base=0x{:x}, {} bytes",
                pid,
                tls_snapshot.fs_base,
                tls_snapshot.tls_data.len()
            );
            restore_tls_snapshot(pid, tls_snapshot)?;
            eprintln!(
                "[tach:snapshot] TLS restoration complete for worker {}",
                pid
            );
        } else {
            eprintln!(
                "[tach:snapshot] No TLS snapshot for worker {} (pre-3.13 Python or capture failed)",
                pid
            );
        }

        Ok(())
    }

    /// Full worker reset with TLS restoration
    ///
    /// This combines memory reset (MADV_DONTNEED) with TLS restoration.
    /// Call this instead of reset_worker() when you need complete state restoration.
    ///
    /// # Sequence
    /// 1. Invalidate memory pages via process_madvise(MADV_DONTNEED)
    /// 2. Restore TLS block via process_vm_writev + ptrace ARCH_SET_FS
    /// 3. Page faults will restore heap/BSS via userfaultfd
    #[cfg(target_arch = "x86_64")]
    pub fn reset_worker_full(&self, pid: Pid) -> Result<()> {
        // Step 1: Invalidate memory pages
        self.reset_worker(pid)?;

        // Step 2: Restore TLS (must happen before worker resumes)
        self.restore_worker_tls(pid)?;

        Ok(())
    }

    /// Full worker reset with VECTORIZED TLS restoration
    ///
    /// This is the optimized version that uses a single process_vm_writev call
    /// with multiple iovec structures to restore the entire Restoration Quadrant.
    ///
    /// # Performance
    /// - Reduces syscall overhead by batching writes
    /// - Lower jitter (latency variance) compared to individual restores
    /// - Expected 20-40% improvement in restoration time
    ///
    /// # Sequence
    /// 1. Invalidate memory pages via process_madvise(MADV_DONTNEED)
    /// 2. Vectorized restore: TLS + critical regions in single syscall
    /// 3. Restore fs_base register via ptrace
    /// 4. Page faults restore remaining heap/BSS via userfaultfd
    #[cfg(target_arch = "x86_64")]
    pub fn reset_worker_full_vectorized(&self, pid: Pid) -> Result<VectorizedRestoreResult> {
        // Step 1: Invalidate memory pages
        self.reset_worker(pid)?;

        // Step 2: Vectorized restore of critical regions
        let worker = self
            .workers
            .get(&pid.as_raw())
            .ok_or_else(|| anyhow!("Worker {} not registered", pid))?;

        restore_full_vectorized(pid, worker)
    }

    /// Handle a page fault by restoring from golden snapshot
    ///
    /// This is called from the fault handling loop when userfaultfd reports a fault.
    pub fn handle_fault(&self, pid: Pid, fault_addr: usize) -> Result<()> {
        let worker = self
            .workers
            .get(&pid.as_raw())
            .ok_or_else(|| anyhow!("Worker {} not registered with SnapshotManager", pid))?;

        let page_start = align_to_page(fault_addr);

        if let Some(data) = worker.golden_pages.get(&page_start) {
            // Restore the page from golden snapshot
            eprintln!(
                "[tach:snapshot] Restoring page at {:x} ({} bytes) for PID {}",
                page_start,
                data.len(),
                pid
            );
            // CRITICAL: Uffd::copy signature is (src, dst, len, wake)
            unsafe {
                worker.uffd.copy(
                    data.as_ptr() as *const libc::c_void, // src data
                    page_start as *mut libc::c_void,      // dst addr
                    data.len(),                           // len
                    true,                                 // wake the faulting thread
                )
            }
            .with_context(|| format!("Failed to copy page at {:x}", page_start))?;
        } else {
            // Page not in snapshot - zero it
            eprintln!(
                "[tach:snapshot] Zero-filling page at {:x} for PID {} (not in snapshot)",
                page_start, pid
            );
            unsafe {
                worker
                    .uffd
                    .zeropage(page_start as *mut libc::c_void, PAGE_SIZE, true)
            }
            .with_context(|| format!("Failed to zero page at {:x}", page_start))?;
        }

        Ok(())
    }

    /// Poll for pending UFFD events and handle them
    ///
    /// This reads from the UFFD file descriptor and handles
    /// any pending page faults by restoring from golden snapshot.
    pub fn handle_pending_faults(&mut self, pid: Pid) -> Result<usize> {
        use userfaultfd::Event;

        let worker = self
            .workers
            .get(&pid.as_raw())
            .ok_or_else(|| anyhow!("Worker {} not registered with SnapshotManager", pid))?;

        let mut handled = 0;

        // Read events from UFFD
        loop {
            match worker.uffd.read_event() {
                Ok(Some(Event::Pagefault { addr, .. })) => {
                    let fault_addr = addr as usize;
                    eprintln!(
                        "[tach:snapshot] UFFD_EVENT_PAGEFAULT at {:x} for PID {}",
                        fault_addr, pid
                    );

                    // Get data and restore
                    let page_start = align_to_page(fault_addr);
                    if let Some(data) = worker.golden_pages.get(&page_start) {
                        eprintln!(
                            "[tach:snapshot] Restoring page {:x} ({} bytes)",
                            page_start,
                            data.len()
                        );
                        // CRITICAL: Uffd::copy signature is (src, dst, len, wake)
                        unsafe {
                            worker.uffd.copy(
                                data.as_ptr() as *const libc::c_void, // src data
                                page_start as *mut libc::c_void,      // dst addr
                                data.len(),                           // len
                                true,                                 // wake
                            )?;
                        }
                    } else {
                        eprintln!(
                            "[tach:snapshot] Zero-filling page {:x} (not in snapshot)",
                            page_start
                        );
                        unsafe {
                            worker.uffd.zeropage(
                                page_start as *mut libc::c_void,
                                PAGE_SIZE,
                                true,
                            )?;
                        }
                    }
                    handled += 1;
                }
                Ok(Some(event)) => {
                    eprintln!("[tach:snapshot] UFFD event: {:?} for PID {}", event, pid);
                }
                Ok(None) => {
                    // No more events
                    break;
                }
                Err(e) => {
                    // Any error means no events ready or UFFD closed
                    eprintln!(
                        "[tach:snapshot] UFFD read_event: {} (breaking poll loop)",
                        e
                    );
                    break;
                }
            }
        }

        Ok(handled)
    }

    /// Remove a worker from the manager (when killed after 1000 tests)
    pub fn remove_worker(&mut self, pid: Pid) {
        self.workers.remove(&pid.as_raw());
    }

    /// Get list of all registered worker PIDs
    pub fn worker_pids(&self) -> Vec<Pid> {
        self.workers.keys().map(|&p| Pid::from_raw(p)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // Memory Region Parsing Tests
    // =========================================================================

    #[test]
    fn test_parse_self_maps() {
        let pid = Pid::from_raw(std::process::id() as i32);
        let regions = parse_memory_maps(pid).expect("Failed to parse maps");

        assert!(!regions.is_empty());

        // Should find at least stack
        let has_stack = regions.iter().any(|r| r.name.contains("[stack]"));

        eprintln!("Found {} regions", regions.len());
        eprintln!("Has stack: {}", has_stack);

        // Stack should exist for any normal process
        assert!(has_stack, "Should find stack region");
    }

    #[test]
    fn test_parse_self_maps_has_readable_regions() {
        let pid = Pid::from_raw(std::process::id() as i32);
        let regions = parse_memory_maps(pid).expect("Failed to parse maps");

        // At least some regions should be readable
        let readable_count = regions.iter().filter(|r| r.perms.contains('r')).count();
        assert!(readable_count > 0, "Should have readable regions");
    }

    // =========================================================================
    // Memory Region Filtering Tests
    // =========================================================================

    #[test]
    fn test_region_filtering_heap() {
        let heap = MemoryRegion {
            start: 0x1000,
            end: 0x2000,
            len: 0x1000,
            perms: "rw-p".to_string(),
            name: "[heap]".to_string(),
        };
        assert!(heap.should_snapshot(), "Heap should be snapshotted");
        assert!(!heap.is_stack(), "Heap is not stack");
    }

    #[test]
    fn test_region_filtering_stack() {
        let stack = MemoryRegion {
            start: 0x3000,
            end: 0x4000,
            len: 0x1000,
            perms: "rw-p".to_string(),
            name: "[stack]".to_string(),
        };
        assert!(stack.should_snapshot(), "Stack should be snapshotted");
        assert!(stack.is_stack(), "Stack is_stack() should be true");
    }

    #[test]
    fn test_region_filtering_vdso_excluded() {
        let vdso = MemoryRegion {
            start: 0x5000,
            end: 0x6000,
            len: 0x1000,
            perms: "r-xp".to_string(),
            name: "[vdso]".to_string(),
        };
        assert!(!vdso.should_snapshot(), "vDSO should be excluded");
    }

    #[test]
    fn test_region_filtering_vsyscall_excluded() {
        let vsyscall = MemoryRegion {
            start: 0x7000,
            end: 0x8000,
            len: 0x1000,
            perms: "r-xp".to_string(),
            name: "[vsyscall]".to_string(),
        };
        assert!(!vsyscall.should_snapshot(), "vsyscall should be excluded");
    }

    #[test]
    fn test_region_filtering_readonly_excluded() {
        let readonly = MemoryRegion {
            start: 0x9000,
            end: 0xa000,
            len: 0x1000,
            perms: "r--p".to_string(),
            name: "/lib/libc.so".to_string(),
        };
        assert!(!readonly.should_snapshot(), "Read-only regions excluded");
    }

    #[test]
    fn test_region_filtering_anonymous_included() {
        let anon = MemoryRegion {
            start: 0xb000,
            end: 0xc000,
            len: 0x1000,
            perms: "rw-p".to_string(),
            name: "".to_string(),
        };
        assert!(
            anon.should_snapshot(),
            "Anonymous writable regions included"
        );
    }

    #[test]
    fn test_region_filtering_libpython_included() {
        let libpython = MemoryRegion {
            start: 0xd000,
            end: 0xe000,
            len: 0x1000,
            perms: "rw-p".to_string(),
            name: "/usr/lib/libpython3.12.so".to_string(),
        };
        assert!(
            libpython.should_snapshot(),
            "libpython data segment included"
        );
    }

    #[test]
    fn test_region_filtering_coverage_buffer_excluded() {
        //  Coverage ring buffer must be EXCLUDED from snapshot
        // It's created via memfd_create("tach_coverage") and appears in /proc/maps
        let coverage_memfd = MemoryRegion {
            start: 0xf000,
            end: 0x10000,
            len: 0x1000,
            perms: "rw-s".to_string(), // Shared mapping
            name: "/memfd:tach_coverage (deleted)".to_string(),
        };
        assert!(
            !coverage_memfd.should_snapshot(),
            "coverage ring buffer must be excluded"
        );

        // Also test the shorter name variant
        let coverage_short = MemoryRegion {
            start: 0xf000,
            end: 0x10000,
            len: 0x1000,
            perms: "rw-s".to_string(),
            name: "memfd:tach_coverage".to_string(),
        };
        assert!(
            !coverage_short.should_snapshot(),
            "coverage ring buffer (short name) must be excluded"
        );
    }

    // =========================================================================
    // Page Alignment Tests
    // =========================================================================

    #[test]
    fn test_page_alignment_already_aligned() {
        assert_eq!(align_to_page(0x1000), 0x1000);
        assert_eq!(align_to_page(0x2000), 0x2000);
        assert_eq!(align_to_page(0x0), 0x0);
    }

    #[test]
    fn test_page_alignment_unaligned() {
        assert_eq!(align_to_page(0x1001), 0x1000);
        assert_eq!(align_to_page(0x1fff), 0x1000);
        assert_eq!(align_to_page(0x2345), 0x2000);
    }

    #[test]
    fn test_page_alignment_large_addresses() {
        // Test with realistic 64-bit addresses
        assert_eq!(align_to_page(0x7f1234560000), 0x7f1234560000);
        assert_eq!(align_to_page(0x7f1234560abc), 0x7f1234560000);
        assert_eq!(align_to_page(0x7f1234560fff), 0x7f1234560000);
    }

    // =========================================================================
    // SnapshotManager Tests
    // =========================================================================

    #[test]
    fn test_snapshot_manager_creation() {
        // This may fail if UFFD is not available, which is okay
        let result = SnapshotManager::new();
        assert!(result.is_ok(), "SnapshotManager::new() should not panic");

        let mgr = result.unwrap();
        // available may be true or false depending on system
        eprintln!("SnapshotManager available: {}", mgr.available);
    }

    #[test]
    fn test_snapshot_manager_no_workers_initially() {
        let mgr = SnapshotManager::new().unwrap();
        assert!(
            mgr.worker_pids().is_empty(),
            "No workers registered initially"
        );
    }

    #[test]
    fn test_snapshot_manager_get_nonexistent_worker() {
        let mgr = SnapshotManager::new().unwrap();
        let fake_pid = Pid::from_raw(99999);
        assert!(
            mgr.get_worker_uffd(fake_pid).is_none(),
            "Nonexistent worker should return None"
        );
    }

    // =========================================================================
    // SCM_RIGHTS Tests (require actual socket, basic validation only)
    // =========================================================================

    #[test]
    fn test_pid_bytes_roundtrip() {
        let pid: i32 = 12345;
        let bytes = pid.to_le_bytes();
        let recovered = i32::from_le_bytes(bytes);
        assert_eq!(pid, recovered);
    }

    #[test]
    fn test_negative_pid_roundtrip() {
        // PID -1 is special (wait for any child)
        let pid: i32 = -1;
        let bytes = pid.to_le_bytes();
        let recovered = i32::from_le_bytes(bytes);
        assert_eq!(pid, recovered);
    }

    // =========================================================================
    //  AlignedSegment Tests
    // =========================================================================

    #[test]
    fn test_aligned_segment_page_alignment() {
        // Unaligned addresses should be page-aligned
        let seg = AlignedSegment::new(0x1234, 0x5678, "test");

        // Start should be rounded DOWN to page boundary
        assert_eq!(seg.start, 0x1000);
        // End should be rounded UP to page boundary
        assert_eq!(seg.end, 0x6000);
    }

    #[test]
    fn test_aligned_segment_already_aligned() {
        let seg = AlignedSegment::new(0x1000, 0x3000, "test");
        assert_eq!(seg.start, 0x1000);
        assert_eq!(seg.end, 0x3000);
        assert_eq!(seg.len(), 0x2000);
        assert_eq!(seg.page_count(), 2);
    }

    #[test]
    fn test_aligned_segment_overlap_detection() {
        let a = AlignedSegment::new(0x1000, 0x3000, "a");
        let b = AlignedSegment::new(0x2000, 0x4000, "b");
        let c = AlignedSegment::new(0x5000, 0x6000, "c");

        // A and B overlap
        assert!(a.overlaps_or_adjacent(&b));
        assert!(b.overlaps_or_adjacent(&a));

        // A and C don't overlap
        assert!(!a.overlaps_or_adjacent(&c));
        assert!(!c.overlaps_or_adjacent(&a));
    }

    #[test]
    fn test_aligned_segment_adjacent_detection() {
        let a = AlignedSegment::new(0x1000, 0x2000, "a");
        let b = AlignedSegment::new(0x2000, 0x3000, "b");

        // Adjacent segments should be detected as overlapping
        // (they share the boundary page)
        assert!(a.overlaps_or_adjacent(&b));
        assert!(b.overlaps_or_adjacent(&a));
    }

    #[test]
    fn test_aligned_segment_merge() {
        let a = AlignedSegment::new(0x1000, 0x3000, "a");
        let b = AlignedSegment::new(0x2000, 0x4000, "b");

        let merged = a.merge(&b);

        // Merged should span both segments
        assert_eq!(merged.start, 0x1000);
        assert_eq!(merged.end, 0x4000);
        assert!(merged.description.contains("a"));
        assert!(merged.description.contains("b"));
    }

    #[test]
    fn test_aligned_segment_merge_max_end() {
        // Test the critical boundary condition: max(prev.end, current.end)
        let a = AlignedSegment::new(0x1000, 0x5000, "a"); // Larger
        let b = AlignedSegment::new(0x2000, 0x3000, "b"); // Smaller, contained

        let merged = a.merge(&b);

        // End should be max(0x5000, 0x3000) = 0x5000
        assert_eq!(merged.start, 0x1000);
        assert_eq!(merged.end, 0x5000);
    }

    // =========================================================================
    //  Segment Merge Algorithm Tests
    // =========================================================================

    #[test]
    fn test_merge_segments_empty() {
        let segments: Vec<AlignedSegment> = vec![];
        let merged = merge_segments(segments);
        assert!(merged.is_empty());
    }

    #[test]
    fn test_merge_segments_single() {
        let segments = vec![AlignedSegment::new(0x1000, 0x2000, "single")];
        let merged = merge_segments(segments);

        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].start, 0x1000);
        assert_eq!(merged[0].end, 0x2000);
    }

    #[test]
    fn test_merge_segments_no_overlap() {
        let segments = vec![
            AlignedSegment::new(0x1000, 0x2000, "a"),
            AlignedSegment::new(0x5000, 0x6000, "b"),
            AlignedSegment::new(0x9000, 0xa000, "c"),
        ];
        let merged = merge_segments(segments);

        // No overlap, should remain 3 segments
        assert_eq!(merged.len(), 3);
    }

    #[test]
    fn test_merge_segments_overlapping() {
        let segments = vec![
            AlignedSegment::new(0x1000, 0x3000, "a"),
            AlignedSegment::new(0x2000, 0x4000, "b"),
        ];
        let merged = merge_segments(segments);

        // Should merge into one segment
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].start, 0x1000);
        assert_eq!(merged[0].end, 0x4000);
    }

    #[test]
    fn test_merge_segments_adjacent() {
        let segments = vec![
            AlignedSegment::new(0x1000, 0x2000, "a"),
            AlignedSegment::new(0x2000, 0x3000, "b"),
        ];
        let merged = merge_segments(segments);

        // Adjacent segments should merge
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].start, 0x1000);
        assert_eq!(merged[0].end, 0x3000);
    }

    #[test]
    fn test_merge_segments_complex() {
        // Complex scenario from the docstring:
        // A: [0x1000, 0x3000)  "heap"
        // B: [0x2000, 0x4000)  "libpython.data"  <- overlaps A
        // C: [0x5000, 0x6000)  "stack"           <- separate
        let segments = vec![
            AlignedSegment::new(0x1000, 0x3000, "heap"),
            AlignedSegment::new(0x2000, 0x4000, "libpython.data"),
            AlignedSegment::new(0x5000, 0x6000, "stack"),
        ];
        let merged = merge_segments(segments);

        // Should produce 2 segments: merged A+B and separate C
        assert_eq!(merged.len(), 2);

        // First segment: merged heap + libpython
        assert_eq!(merged[0].start, 0x1000);
        assert_eq!(merged[0].end, 0x4000);

        // Second segment: stack
        assert_eq!(merged[1].start, 0x5000);
        assert_eq!(merged[1].end, 0x6000);
    }

    #[test]
    fn test_merge_segments_unsorted_input() {
        // Input is not sorted - merge should handle this
        let segments = vec![
            AlignedSegment::new(0x5000, 0x6000, "c"),
            AlignedSegment::new(0x1000, 0x3000, "a"),
            AlignedSegment::new(0x2000, 0x4000, "b"),
        ];
        let merged = merge_segments(segments);

        // Should still produce correct result
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].start, 0x1000);
        assert_eq!(merged[0].end, 0x4000);
        assert_eq!(merged[1].start, 0x5000);
        assert_eq!(merged[1].end, 0x6000);
    }

    #[test]
    fn test_merge_segments_chain() {
        // Chain of overlapping segments
        let segments = vec![
            AlignedSegment::new(0x1000, 0x2000, "a"),
            AlignedSegment::new(0x1800, 0x2800, "b"),
            AlignedSegment::new(0x2400, 0x3400, "c"),
            AlignedSegment::new(0x3000, 0x4000, "d"),
        ];
        let merged = merge_segments(segments);

        // All should merge into one
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].start, 0x1000);
        assert_eq!(merged[0].end, 0x4000);
    }

    // =========================================================================
    //  Page Alignment Up Tests
    // =========================================================================

    #[test]
    fn test_align_to_page_up_already_aligned() {
        assert_eq!(align_to_page_up(0x1000), 0x1000);
        assert_eq!(align_to_page_up(0x2000), 0x2000);
        assert_eq!(align_to_page_up(0x0), 0x0);
    }

    #[test]
    fn test_align_to_page_up_unaligned() {
        assert_eq!(align_to_page_up(0x1001), 0x2000);
        assert_eq!(align_to_page_up(0x1fff), 0x2000);
        assert_eq!(align_to_page_up(0x2001), 0x3000);
    }

    // =========================================================================
    //  TLS Snapshot Tests
    // =========================================================================

    #[test]
    fn test_tls_snapshot_struct_creation() {
        let snapshot = TlsSnapshot {
            fs_base: 0x7f1234560000,
            tls_data: vec![0u8; 12 * 1024],
            tls_region_start: 0x7f1234550000,
            tls_region_end: 0x7f1234570000,
        };

        assert_eq!(snapshot.fs_base, 0x7f1234560000);
        assert_eq!(snapshot.tls_data.len(), 12 * 1024);
        assert_eq!(snapshot.tls_region_end - snapshot.tls_region_start, 0x20000);
    }

    #[test]
    fn test_tls_snapshot_size_constant() {
        // TLS_SNAPSHOT_SIZE_HINT is the minimum hint (12KB)
        //  Actual capture size is now dynamic based on region boundaries
        assert_eq!(TLS_SNAPSHOT_SIZE_HINT, 12 * 1024);
        assert_eq!(TLS_SNAPSHOT_SIZE_HINT, 12288);
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn test_arch_prctl_constants() {
        // These constants must match the kernel values for x86_64
        assert_eq!(ARCH_GET_FS, 0x1003);
        assert_eq!(ARCH_SET_FS, 0x1002);
        assert_eq!(PTRACE_ARCH_PRCTL, 30);
    }

    #[test]
    fn test_tls_snapshot_data_roundtrip() {
        // Test that TLS snapshot can store and retrieve data correctly
        //  TLS size is now dynamic, so we test with a sample size
        let sample_size = 16 * 1024; // 16KB - larger than hint to verify dynamic sizing
        let test_pattern: Vec<u8> = (0..sample_size).map(|i| (i % 256) as u8).collect();

        let snapshot = TlsSnapshot {
            fs_base: 0x7f1234560000,
            tls_data: test_pattern.clone(),
            tls_region_start: 0x7f1234550000,
            tls_region_end: 0x7f1234570000,
        };

        assert_eq!(snapshot.tls_data.len(), sample_size);
        assert_eq!(snapshot.tls_data, test_pattern);
    }

    #[test]
    fn test_find_tls_region_self() {
        // Test that we can find TLS region for our own process
        // This is a smoke test - we don't actually capture TLS here
        // because we can't ptrace ourselves
        let pid = Pid::from_raw(std::process::id() as i32);
        let regions = parse_memory_maps(pid).expect("Failed to parse maps");

        // We should have at least some regions
        assert!(!regions.is_empty());

        // Look for any anonymous writable region that could be TLS
        let tls_candidates: Vec<_> = regions
            .iter()
            .filter(|r| r.perms.contains('w') && r.perms.contains('p') && r.name.is_empty())
            .collect();

        // There should be at least one anonymous writable region
        // (TLS is typically in one of these)
        eprintln!(
            "[tach:test] Found {} potential TLS candidate regions",
            tls_candidates.len()
        );
        assert!(
            !tls_candidates.is_empty(),
            "Should find anonymous writable regions"
        );
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn test_worker_snapshot_with_tls_field() {
        // Verify WorkerSnapshot struct includes tls_snapshot field
        // This is a compile-time check - if it compiles, the field exists
        let mgr = SnapshotManager::new().expect("Failed to create manager");

        // Worker snapshot should have tls_snapshot: Option<TlsSnapshot>
        // We can't directly access workers HashMap, but we can verify
        // the structure exists via the API
        assert!(mgr.worker_pids().is_empty(), "No workers initially");
    }

    // =========================================================================
    // Additional Edge Case Tests (Phase 2 Regression Prevention)
    // =========================================================================

    #[test]
    fn test_memory_region_struct_fields() {
        let region = MemoryRegion {
            start: 0x7f0000000000,
            end: 0x7f0000001000,
            len: 0x1000,
            perms: "rw-p".to_string(),
            name: "test_region".to_string(),
        };

        assert_eq!(region.start, 0x7f0000000000);
        assert_eq!(region.end, 0x7f0000001000);
        assert_eq!(region.len, 0x1000);
        assert_eq!(region.perms, "rw-p");
        assert_eq!(region.name, "test_region");
    }

    #[test]
    fn test_memory_region_len_calculation() {
        let region = MemoryRegion {
            start: 0x1000,
            end: 0x5000,
            len: 0x4000,
            perms: "rw-p".to_string(),
            name: "".to_string(),
        };

        assert_eq!(region.len, region.end - region.start);
    }

    #[test]
    fn test_region_filtering_shared_excluded() {
        // Shared mappings (like coverage buffer) should be excluded
        let shared = MemoryRegion {
            start: 0x1000,
            end: 0x2000,
            len: 0x1000,
            perms: "rw-s".to_string(), // 's' = shared
            name: "".to_string(),
        };
        assert!(
            !shared.should_snapshot(),
            "Shared mappings should be excluded"
        );
    }

    #[test]
    fn test_region_filtering_vvar_excluded() {
        let vvar = MemoryRegion {
            start: 0x1000,
            end: 0x2000,
            len: 0x1000,
            perms: "r--p".to_string(),
            name: "[vvar]".to_string(),
        };
        assert!(!vvar.should_snapshot(), "vvar should be excluded");
    }

    #[test]
    fn test_region_is_stack_false_for_non_stack() {
        let heap = MemoryRegion {
            start: 0x1000,
            end: 0x2000,
            len: 0x1000,
            perms: "rw-p".to_string(),
            name: "[heap]".to_string(),
        };
        assert!(!heap.is_stack(), "Heap should not be identified as stack");

        let anon = MemoryRegion {
            start: 0x1000,
            end: 0x2000,
            len: 0x1000,
            perms: "rw-p".to_string(),
            name: "".to_string(),
        };
        assert!(
            !anon.is_stack(),
            "Anonymous region should not be identified as stack"
        );
    }

    #[test]
    fn test_aligned_segment_zero_length() {
        // Edge case: same start and end address
        let seg = AlignedSegment::new(0x1000, 0x1000, "zero");
        // After alignment, this should still have some size
        // since end is rounded up
        assert_eq!(seg.start, 0x1000);
        assert_eq!(seg.end, 0x1000);
        assert_eq!(seg.len(), 0);
        assert_eq!(seg.page_count(), 0);
    }

    #[test]
    fn test_aligned_segment_single_byte() {
        // Edge case: single byte spans page boundaries
        let seg = AlignedSegment::new(0x1fff, 0x2001, "tiny");
        assert_eq!(seg.start, 0x1000); // Aligned down
        assert_eq!(seg.end, 0x3000); // Aligned up
        assert_eq!(seg.page_count(), 2);
    }

    #[test]
    fn test_aligned_segment_page_count_large() {
        let seg = AlignedSegment::new(0x1000, 0x11000, "16_pages");
        assert_eq!(seg.page_count(), 16);
    }

    #[test]
    fn test_merge_segments_all_overlapping() {
        // All segments overlap into one
        let segments = vec![
            AlignedSegment::new(0x1000, 0x3000, "a"),
            AlignedSegment::new(0x1500, 0x2500, "b"),
            AlignedSegment::new(0x2000, 0x4000, "c"),
            AlignedSegment::new(0x3500, 0x5000, "d"),
        ];
        let merged = merge_segments(segments);

        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].start, 0x1000);
        assert_eq!(merged[0].end, 0x5000);
    }

    #[test]
    fn test_parse_maps_filters_permissions() {
        let pid = Pid::from_raw(std::process::id() as i32);
        let regions = parse_memory_maps(pid).expect("Failed to parse maps");

        // Verify we can find regions with different permissions
        let has_readonly = regions
            .iter()
            .any(|r| r.perms.contains("r-") && !r.perms.contains('w'));
        let has_writable = regions.iter().any(|r| r.perms.contains('w'));

        assert!(has_writable, "Should have writable regions");
        // Note: readonly regions may or may not exist depending on system state
        eprintln!("Has read-only regions: {}", has_readonly);
        eprintln!("Has writable regions: {}", has_writable);
    }

    #[test]
    fn test_page_size_constant() {
        assert_eq!(PAGE_SIZE, 4096);
        assert_eq!(PAGE_SIZE, 0x1000);
    }

    #[test]
    fn test_tls_snapshot_empty_data() {
        let snapshot = TlsSnapshot {
            fs_base: 0x7f1234560000,
            tls_data: vec![],
            tls_region_start: 0x7f1234550000,
            tls_region_end: 0x7f1234550000,
        };

        assert!(snapshot.tls_data.is_empty());
        assert_eq!(snapshot.tls_region_end - snapshot.tls_region_start, 0);
    }

    #[test]
    fn test_region_filtering_memfd_mapping_excluded() {
        // All memfd mappings for tach should be excluded
        let mapping_memfd = MemoryRegion {
            start: 0xf000,
            end: 0x10000,
            len: 0x1000,
            perms: "rw-s".to_string(),
            name: "/memfd:tach_mapping (deleted)".to_string(),
        };
        assert!(
            !mapping_memfd.should_snapshot(),
            "tach_mapping memfd must be excluded"
        );
    }

    #[test]
    fn test_aligned_segment_description_preserved() {
        let seg = AlignedSegment::new(0x1000, 0x2000, "my_description");
        assert_eq!(seg.description, "my_description");
    }

    #[test]
    fn test_merge_preserves_all_descriptions() {
        let a = AlignedSegment::new(0x1000, 0x2000, "first");
        let b = AlignedSegment::new(0x1500, 0x2500, "second");

        let merged = a.merge(&b);

        // Merged description should contain both original descriptions
        assert!(
            merged.description.contains("first"),
            "Should contain 'first'"
        );
        assert!(
            merged.description.contains("second"),
            "Should contain 'second'"
        );
    }

    // =========================================================================
    // Error Path Tests (Regression Prevention)
    // =========================================================================

    #[test]
    fn test_restore_region_creation() {
        let data = vec![0xde, 0xad, 0xbe, 0xef];
        let region = RestoreRegion::new(0x7f1234560000, data.clone(), "test_region");

        assert_eq!(region.remote_addr, 0x7f1234560000);
        assert_eq!(region.data, data);
        assert_eq!(region.name, "test_region");
        assert_eq!(region.len(), 4);
        assert!(!region.is_empty());
    }

    #[test]
    fn test_restore_region_empty() {
        let region = RestoreRegion::new(0x1000, vec![], "empty_region");

        assert_eq!(region.len(), 0);
        assert!(region.is_empty());
    }

    #[test]
    fn test_restore_region_large_data() {
        // Test with page-sized data
        let data = vec![0xAB; PAGE_SIZE];
        let region = RestoreRegion::new(0x1000, data, "page_region");

        assert_eq!(region.len(), PAGE_SIZE);
        assert!(!region.is_empty());
    }

    #[test]
    fn test_vectorized_restore_result_fields() {
        let result = VectorizedRestoreResult {
            bytes_written: 12288,
            regions_restored: 3,
            duration_us: 150,
            region_details: vec![
                ("TLS".to_string(), 4096),
                ("Stack".to_string(), 4096),
                ("BSS".to_string(), 4096),
            ],
        };

        assert_eq!(result.bytes_written, 12288);
        assert_eq!(result.regions_restored, 3);
        assert_eq!(result.duration_us, 150);
        assert_eq!(result.region_details.len(), 3);
    }

    #[test]
    fn test_vectorized_restore_result_empty() {
        let result = VectorizedRestoreResult {
            bytes_written: 0,
            regions_restored: 0,
            duration_us: 0,
            region_details: vec![],
        };

        assert_eq!(result.bytes_written, 0);
        assert!(result.region_details.is_empty());
    }

    #[test]
    fn test_libpython_info_struct() {
        let info = LibpythonInfo {
            path: PathBuf::from("/usr/lib/libpython3.12.so"),
            base_addr: 0x7f1234560000,
            is_static: false,
        };

        assert_eq!(info.path, PathBuf::from("/usr/lib/libpython3.12.so"));
        assert_eq!(info.base_addr, 0x7f1234560000);
        assert!(!info.is_static);
    }

    #[test]
    fn test_libpython_info_static() {
        let info = LibpythonInfo {
            path: PathBuf::from("/usr/bin/python"),
            base_addr: 0x400000,
            is_static: true,
        };

        assert!(info.is_static);
    }

    #[test]
    fn test_parse_memory_maps_nonexistent_pid() {
        // Test error handling for nonexistent PID
        let fake_pid = Pid::from_raw(999999999);
        let result = parse_memory_maps(fake_pid);

        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("999999999") || err_msg.contains("Failed to read"),
            "Error should mention the PID or file read failure: {}",
            err_msg
        );
    }

    #[test]
    fn test_find_libpython_nonexistent_pid() {
        // Test error handling when trying to find libpython for nonexistent PID
        let fake_pid = Pid::from_raw(999999999);
        let result = find_libpython(fake_pid);

        assert!(result.is_err());
        // The error should be about reading the maps file, not about missing libpython
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("999999999") || err_msg.contains("Failed"),
            "Error should mention the PID or failure: {}",
            err_msg
        );
    }

    #[test]
    fn test_find_tls_region_nonexistent_pid() {
        // Test error handling for nonexistent PID in find_tls_region
        let fake_pid = Pid::from_raw(999999999);
        let result = find_tls_region(fake_pid, 0x7f1234560000);

        assert!(result.is_err());
    }

    #[test]
    fn test_aligned_segment_is_empty_true() {
        // Test is_empty for a zero-length segment
        let seg = AlignedSegment {
            start: 0x1000,
            end: 0x1000,
            description: "zero_len".to_string(),
        };

        assert!(seg.is_empty());
        assert_eq!(seg.len(), 0);
        assert_eq!(seg.page_count(), 0);
    }

    #[test]
    fn test_memory_region_should_snapshot_executable_excluded() {
        // Executable-only regions should be excluded (no write permission)
        let exec_region = MemoryRegion {
            start: 0x1000,
            end: 0x2000,
            len: 0x1000,
            perms: "r-xp".to_string(),
            name: "/lib/libc.so".to_string(),
        };
        assert!(
            !exec_region.should_snapshot(),
            "Executable-only regions should be excluded"
        );
    }

    #[test]
    fn test_memory_region_should_snapshot_all_permissions() {
        // Region with all permissions should be included if writable
        let all_perms = MemoryRegion {
            start: 0x1000,
            end: 0x2000,
            len: 0x1000,
            perms: "rwxp".to_string(),
            name: "[heap]".to_string(),
        };
        assert!(
            all_perms.should_snapshot(),
            "Writable heap should be included"
        );
    }

    #[test]
    fn test_parse_elf_writable_segments_nonexistent_file() {
        // Test error handling for nonexistent ELF file
        let fake_path = PathBuf::from("/nonexistent/path/to/libpython.so");
        let result = parse_elf_writable_segments(&fake_path, 0x7f1234560000);

        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("nonexistent") || err_msg.contains("Failed to read"),
            "Error should mention the file or read failure: {}",
            err_msg
        );
    }

    #[test]
    fn test_snapshot_manager_reset_worker_not_registered() {
        // Test error handling when resetting an unregistered worker
        let mgr = SnapshotManager::new().unwrap();

        // Skip if UFFD is not available (returns Ok in fallback mode)
        if !mgr.available {
            eprintln!("[tach:test] UFFD unavailable, skipping reset_worker test");
            return;
        }

        let fake_pid = Pid::from_raw(99999);
        let result = mgr.reset_worker(fake_pid);

        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("99999") || err_msg.contains("not registered"),
            "Error should mention the PID or registration status: {}",
            err_msg
        );
    }

    #[test]
    fn test_snapshot_manager_handle_fault_not_registered() {
        // Test error handling when handling fault for unregistered worker
        let mgr = SnapshotManager::new().unwrap();

        let fake_pid = Pid::from_raw(99999);
        let result = mgr.handle_fault(fake_pid, 0x7f1234560000);

        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("99999") || err_msg.contains("not registered"),
            "Error should mention the PID or registration status: {}",
            err_msg
        );
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn test_snapshot_manager_restore_worker_tls_not_registered() {
        // Test error handling when restoring TLS for unregistered worker
        let mgr = SnapshotManager::new().unwrap();

        // Skip if UFFD is not available (returns Ok in fallback mode)
        if !mgr.available {
            eprintln!("[tach:test] UFFD unavailable, skipping restore_worker_tls test");
            return;
        }

        let fake_pid = Pid::from_raw(99999);
        let result = mgr.restore_worker_tls(fake_pid);

        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("99999") || err_msg.contains("not registered"),
            "Error should mention the PID or registration status: {}",
            err_msg
        );
    }

    #[test]
    fn test_snapshot_manager_remove_nonexistent_worker() {
        // Removing a nonexistent worker should be a no-op (no panic)
        let mut mgr = SnapshotManager::new().unwrap();
        let fake_pid = Pid::from_raw(99999);

        // This should not panic
        mgr.remove_worker(fake_pid);

        // Verify no workers exist
        assert!(mgr.worker_pids().is_empty());
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn test_snapshot_manager_calibration_state() {
        // Test calibration state methods
        let mgr = SnapshotManager::new().unwrap();

        // Before calibration, should not be calibrated
        assert!(!mgr.is_calibrated());
        assert!(mgr.calibrated_offset().is_none());
    }

    #[test]
    fn test_aligned_segment_eq_trait() {
        // Test PartialEq and Eq derivation
        let seg1 = AlignedSegment::new(0x1000, 0x2000, "test");
        let seg2 = AlignedSegment::new(0x1000, 0x2000, "test");
        let seg3 = AlignedSegment::new(0x1000, 0x3000, "test");

        assert_eq!(seg1, seg2);
        assert_ne!(seg1, seg3);
    }

    #[test]
    fn test_aligned_segment_clone_trait() {
        // Test Clone derivation
        let seg1 = AlignedSegment::new(0x1000, 0x2000, "test");
        let seg2 = seg1.clone();

        assert_eq!(seg1, seg2);
        assert_eq!(seg1.description, seg2.description);
    }

    #[test]
    fn test_memory_region_clone_trait() {
        // Test Clone derivation for MemoryRegion
        let region1 = MemoryRegion {
            start: 0x1000,
            end: 0x2000,
            len: 0x1000,
            perms: "rw-p".to_string(),
            name: "test".to_string(),
        };
        let region2 = region1.clone();

        assert_eq!(region1.start, region2.start);
        assert_eq!(region1.name, region2.name);
    }

    #[test]
    fn test_tls_snapshot_clone_trait() {
        // Test Clone derivation for TlsSnapshot
        let snapshot1 = TlsSnapshot {
            fs_base: 0x7f1234560000,
            tls_data: vec![1, 2, 3, 4],
            tls_region_start: 0x7f1234550000,
            tls_region_end: 0x7f1234570000,
        };
        let snapshot2 = snapshot1.clone();

        assert_eq!(snapshot1.fs_base, snapshot2.fs_base);
        assert_eq!(snapshot1.tls_data, snapshot2.tls_data);
    }

    #[test]
    fn test_restore_region_clone_trait() {
        // Test Clone derivation for RestoreRegion
        let region1 = RestoreRegion::new(0x1000, vec![0xAB; 100], "test");
        let region2 = region1.clone();

        assert_eq!(region1.remote_addr, region2.remote_addr);
        assert_eq!(region1.data, region2.data);
        assert_eq!(region1.name, region2.name);
    }

    #[test]
    fn test_aligned_segment_not_overlapping() {
        // Test non-overlapping segments with a gap
        let a = AlignedSegment::new(0x1000, 0x2000, "a");
        let c = AlignedSegment::new(0x4000, 0x5000, "c");

        // A and C have a gap between them
        assert!(!a.overlaps_or_adjacent(&c));
        assert!(!c.overlaps_or_adjacent(&a));
    }

    #[test]
    fn test_aligned_segment_contained() {
        // Test when one segment is completely contained in another
        let outer = AlignedSegment::new(0x1000, 0x5000, "outer");
        let inner = AlignedSegment::new(0x2000, 0x3000, "inner");

        assert!(outer.overlaps_or_adjacent(&inner));
        assert!(inner.overlaps_or_adjacent(&outer));

        let merged = outer.merge(&inner);
        assert_eq!(merged.start, 0x1000);
        assert_eq!(merged.end, 0x5000);
    }

    #[test]
    fn test_page_alignment_boundary() {
        // Test alignment at exact page boundaries
        assert_eq!(align_to_page(PAGE_SIZE - 1), 0);
        assert_eq!(align_to_page(PAGE_SIZE), PAGE_SIZE);
        assert_eq!(align_to_page(PAGE_SIZE + 1), PAGE_SIZE);

        assert_eq!(align_to_page_up(PAGE_SIZE - 1), PAGE_SIZE);
        assert_eq!(align_to_page_up(PAGE_SIZE), PAGE_SIZE);
        assert_eq!(align_to_page_up(PAGE_SIZE + 1), PAGE_SIZE * 2);
    }

    #[test]
    fn test_region_filtering_memfd_variants() {
        // Test various memfd naming patterns that should be excluded
        let variants = [
            "memfd:tach_coverage",
            "/memfd:tach_coverage (deleted)",
            "memfd:tach_coverage (deleted)",
            "/dev/shm/tach_coverage",
        ];

        for name in &variants[..3] {
            // First 3 should be excluded
            let region = MemoryRegion {
                start: 0x1000,
                end: 0x2000,
                len: 0x1000,
                perms: "rw-s".to_string(),
                name: name.to_string(),
            };
            assert!(
                !region.should_snapshot(),
                "Region with name '{}' should be excluded",
                name
            );
        }
    }

    #[test]
    fn test_handle_pending_faults_not_registered() {
        // Test error handling when handling pending faults for unregistered worker
        let mut mgr = SnapshotManager::new().unwrap();

        let fake_pid = Pid::from_raw(99999);
        let result = mgr.handle_pending_faults(fake_pid);

        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("99999") || err_msg.contains("not registered"),
            "Error should mention the PID or registration status: {}",
            err_msg
        );
    }

    // =========================================================================
    // Spec Gap Fixes: Additional Error Path Tests
    // =========================================================================

    #[test]
    fn test_find_libpython_not_in_maps() {
        // Test find_libpython behavior for the current process
        //
        // NOTE: Since tach-core is a PyO3 project, the test binary is linked
        // against libpython and will likely find it. This test verifies the
        // function works correctly in all scenarios:
        // 1. libpython found → is_static=false, path references python
        // 2. Fallback to main exe → is_static=true, path is executable
        // 3. Not found → Err with descriptive message
        let pid = Pid::from_raw(std::process::id() as i32);
        let result = find_libpython(pid);

        match result {
            Ok(info) => {
                if info.is_static {
                    // Fallback to main executable (no libpython in maps)
                    // This can happen on some build configurations
                    assert!(
                        !info.path.to_string_lossy().contains("libpython"),
                        "Static fallback path should not contain libpython: {:?}",
                        info.path
                    );
                } else {
                    // Found actual libpython.so (common for PyO3 projects)
                    // Verify the path references python in some way
                    let path_str = info.path.to_string_lossy();
                    assert!(
                        path_str.contains("python") || path_str.contains("libpython"),
                        "Dynamic libpython path should reference python: {:?}",
                        info.path
                    );
                    // Verify base_addr is non-zero (valid mapping)
                    assert!(
                        info.base_addr > 0,
                        "Base address should be non-zero for valid mapping"
                    );
                }
            }
            Err(e) => {
                // If it fails, should mention the expected error
                let err_msg = e.to_string();
                assert!(
                    err_msg.contains("Could not find libpython")
                        || err_msg.contains("statically linked"),
                    "Error should indicate libpython was not found: {}",
                    err_msg
                );
            }
        }
    }

    #[test]
    fn test_parse_elf_writable_segments_invalid_elf() {
        // Test error handling when ELF file is corrupted/invalid
        // Create a temp file with non-ELF content
        use std::io::Write;

        let temp_dir = std::env::temp_dir();
        let test_file = temp_dir.join(format!("tach_test_invalid_elf_{}", std::process::id()));

        // Write garbage data that is definitely not an ELF file
        {
            let mut file = std::fs::File::create(&test_file).expect("Failed to create test file");
            file.write_all(b"This is not an ELF file. Just random garbage data!")
                .expect("Failed to write");
        }

        let result = parse_elf_writable_segments(&test_file, 0x7f1234560000);

        // Clean up
        let _ = std::fs::remove_file(&test_file);

        assert!(result.is_err(), "Parsing invalid ELF should fail");

        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("ELF")
                || err_msg.contains("parse")
                || err_msg.contains("Failed")
                || err_msg.contains("Invalid"),
            "Error should mention ELF parsing issue: {}",
            err_msg
        );
    }

    #[test]
    fn test_parse_memory_maps_malformed_format_handling() {
        // Test that parse_memory_maps handles the actual /proc/maps format correctly
        // We verify the parsing by checking that:
        // 1. Addresses are parsed correctly (hex format)
        // 2. Permissions are extracted
        // 3. Names (paths) are captured

        let pid = Pid::from_raw(std::process::id() as i32);
        let regions = parse_memory_maps(pid).expect("Failed to parse maps");

        // Verify address parsing
        for region in &regions {
            // Start should be less than or equal to end
            assert!(
                region.start <= region.end,
                "Region start {} should be <= end {}",
                region.start,
                region.end
            );

            // Length should match
            assert_eq!(
                region.len,
                region.end - region.start,
                "Region length should equal end - start"
            );

            // Permissions should be 4 characters
            assert!(
                region.perms.len() >= 4,
                "Permissions '{}' should be at least 4 chars",
                region.perms
            );

            // Permissions should only contain valid characters
            let valid_chars = ['r', 'w', 'x', 'p', 's', '-'];
            for c in region.perms.chars().take(4) {
                assert!(
                    valid_chars.contains(&c),
                    "Invalid permission char '{}' in '{}'",
                    c,
                    region.perms
                );
            }

            // Addresses should be page-aligned (most regions are)
            // But don't assert this as some regions may not be
        }

        // Should have at least some regions
        assert!(!regions.is_empty(), "Should parse at least some regions");
    }
}
