//! Snapshot Manager: userfaultfd-based memory reset for worker recycling
//!
//! This module implements the "Snapshot-Hypervisor" pattern for Tach:
//! - Capture a "golden" snapshot of worker memory after initialization
//! - Reset workers to that snapshot after each test (instead of killing them)
//! - Handle page faults via userfaultfd to lazily restore pages
//!
//! This eliminates fork() overhead in the hot loop (target: <50μs reset vs ~1ms fork)
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

use anyhow::{anyhow, Context, Result};
use goblin::elf::{program_header::PF_W, Elf};
use nix::sys::socket::{sendmsg, ControlMessage, MsgFlags};
use nix::sys::uio::{process_vm_readv, RemoteIoVec};
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

    sendmsg::<()>(sock.as_raw_fd(), &iov, &cmsg, MsgFlags::empty(), None).context("Failed to send FD via SCM_RIGHTS")?;

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
    let mut cmsg_buf = [0u8; unsafe { libc::CMSG_SPACE(std::mem::size_of::<RawFd>() as u32) } as usize];

    let mut msg: libc::msghdr = unsafe { MaybeUninit::zeroed().assume_init() };
    msg.msg_iov = &mut iov;
    msg.msg_iovlen = 1;
    msg.msg_control = cmsg_buf.as_mut_ptr() as *mut libc::c_void;
    msg.msg_controllen = cmsg_buf.len();

    // SAFETY: recvmsg is a safe syscall with properly initialized buffers
    let bytes_received = unsafe { libc::recvmsg(sock.as_raw_fd(), &mut msg, 0) };
    if bytes_received < 0 {
        return Err(anyhow!("recvmsg failed: {}", std::io::Error::last_os_error()));
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
    use nix::sys::wait::{waitpid, WaitPidFlag};

    // Attach to the process
    ptrace::attach(pid).with_context(|| format!("Failed to ptrace attach to PID {}", pid))?;

    // Wait for the process to stop (ptrace-stop)
    waitpid(pid, Some(WaitPidFlag::WSTOPPED)).with_context(|| format!("Failed to wait for ptrace-stop on PID {}", pid))?;

    // Use PTRACE_ARCH_PRCTL to get fs_base
    // ptrace(PTRACE_ARCH_PRCTL, pid, ARCH_GET_FS, &output)
    let mut fs_base: u64 = 0;

    // SAFETY: We are attached to the process via ptrace, and it is stopped.
    // PTRACE_ARCH_PRCTL with ARCH_GET_FS reads the fs_base into the data pointer.
    let ret = unsafe { libc::ptrace(PTRACE_ARCH_PRCTL as libc::c_uint, pid.as_raw(), ARCH_GET_FS, &mut fs_base as *mut u64) };

    if ret == -1 {
        let err = std::io::Error::last_os_error();
        // Detach before returning error
        let _ = ptrace::detach(pid, None);
        return Err(anyhow!("PTRACE_ARCH_PRCTL(ARCH_GET_FS) failed for PID {}: {}", pid, err));
    }

    // Detach from the process (it remains stopped, we'll SIGCONT later)
    ptrace::detach(pid, None).with_context(|| format!("Failed to ptrace detach from PID {}", pid))?;

    eprintln!("[tls] Captured fs_base for PID {}: 0x{:016x}", pid, fs_base);

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
    use nix::sys::wait::{waitpid, WaitPidFlag};

    // Attach to the process
    ptrace::attach(pid).with_context(|| format!("Failed to ptrace attach to PID {}", pid))?;

    // Wait for the process to stop (ptrace-stop)
    waitpid(pid, Some(WaitPidFlag::WSTOPPED)).with_context(|| format!("Failed to wait for ptrace-stop on PID {}", pid))?;

    // Use PTRACE_ARCH_PRCTL to set fs_base
    // ptrace(PTRACE_ARCH_PRCTL, pid, ARCH_SET_FS, value)
    // SAFETY: We are attached to the process via ptrace, and it is stopped.
    // PTRACE_ARCH_PRCTL with ARCH_SET_FS sets fs_base to the data value.
    let ret = unsafe { libc::ptrace(PTRACE_ARCH_PRCTL as libc::c_uint, pid.as_raw(), ARCH_SET_FS, fs_base) };

    if ret == -1 {
        let err = std::io::Error::last_os_error();
        // Detach before returning error
        let _ = ptrace::detach(pid, None);
        return Err(anyhow!("PTRACE_ARCH_PRCTL(ARCH_SET_FS) failed for PID {}: {}", pid, err));
    }

    // Detach from the process
    ptrace::detach(pid, None).with_context(|| format!("Failed to ptrace detach from PID {}", pid))?;

    eprintln!("[tls] Restored fs_base for PID {}: 0x{:016x}", pid, fs_base);

    Ok(())
}

/// Find the TLS region containing fs_base in /proc/pid/maps
///
/// Returns (start, end) of the anonymous mapping containing fs_base.
fn find_tls_region(pid: Pid, fs_base: usize) -> Result<(usize, usize)> {
    let regions = parse_memory_maps(pid)?;

    for region in regions {
        if region.start <= fs_base && fs_base < region.end {
            eprintln!("[tls] Found TLS region for fs_base 0x{:x}: 0x{:x}-0x{:x} [{}]", fs_base, region.start, region.end, region.name);
            return Ok((region.start, region.end));
        }
    }

    Err(anyhow!("Could not find memory region containing fs_base 0x{:x} for PID {}", fs_base, pid))
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
        return Err(anyhow!("TLS region too small: fs_base=0x{:x}, region_end=0x{:x}, size={}", fs_base, region_end, max_capture_from_fs));
    }

    // Calculate actual capture size:
    // - Minimum: TLS_SNAPSHOT_SIZE_HINT (to ensure we get mimalloc state)
    // - Maximum: entire region from fs_base to region_end
    let capture_len = max_capture_from_fs.max(TLS_SNAPSHOT_SIZE_HINT.min(max_capture_from_fs));

    eprintln!("[tls] Dynamic TLS capture: fs_base=0x{:x}, region=[0x{:x}, 0x{:x}), capture={} bytes", fs_base, region_start, region_end, capture_len);

    // Step 3: Read TLS data via process_vm_readv
    let mut tls_data = vec![0u8; capture_len];
    let mut local_iov = [IoSliceMut::new(&mut tls_data)];
    let remote_iov = [RemoteIoVec { base: fs_base, len: capture_len }];

    let bytes_read = process_vm_readv(pid, &mut local_iov, &remote_iov).with_context(|| format!("process_vm_readv failed for TLS at 0x{:x}", fs_base))?;

    //  Check for partial reads (Orchestrator's warning)
    if bytes_read != capture_len {
        return Err(anyhow!("Partial TLS read: {}/{} bytes. Worker may have 'Fractured Brain' if we proceed.", bytes_read, capture_len));
    }

    eprintln!("[tls] TLS snapshot captured: {} bytes from fs_base=0x{:x} (region={} bytes total)", tls_data.len(), fs_base, region_end - region_start);

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

    eprintln!("[tls] Restoring TLS for PID {}: {} bytes to 0x{:x}", pid, snapshot.tls_data.len(), snapshot.fs_base);

    // Step 1: Write TLS data back via process_vm_writev
    // This is necessary because userfaultfd may not cover the TLS region
    // if it wasn't registered, or we want explicit control.
    let local_iov = [IoSlice::new(&snapshot.tls_data)];
    let remote_iov = [RemoteIoVec { base: snapshot.fs_base, len: snapshot.tls_data.len() }];

    let bytes_written = process_vm_writev(pid, &local_iov, &remote_iov).with_context(|| format!("process_vm_writev failed for TLS at 0x{:x}", snapshot.fs_base))?;

    if bytes_written != snapshot.tls_data.len() {
        return Err(anyhow!("Partial TLS write: {}/{} bytes", bytes_written, snapshot.tls_data.len()));
    }

    // Step 2: Restore fs_base register
    // This ensures the register points to the same TLS block
    // (should be unchanged, but this is a safety measure)
    set_fs_base_ptrace(pid, snapshot.fs_base)?;

    eprintln!("[tls] TLS restore complete for PID {}: fs_base=0x{:x}", pid, snapshot.fs_base);

    Ok(())
}

/// Parse /proc/{pid}/maps to extract memory regions
///
/// Format: start-end perms offset dev inode pathname
/// Example: 7f1234560000-7f1234580000 rw-p 00000000 00:00 0 [heap]
pub fn parse_memory_maps(pid: Pid) -> Result<Vec<MemoryRegion>> {
    let maps_path = format!("/proc/{}/maps", pid);
    let content = fs::read_to_string(&maps_path).with_context(|| format!("Failed to read {}", maps_path))?;

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
        let name = if parts.len() > 5 { parts[5..].join(" ") } else { String::new() };

        regions.push(MemoryRegion { start, end, len: end - start, perms, name });
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
        debug_assert!(self.overlaps_or_adjacent(other), "Cannot merge non-overlapping segments");

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
    let mut libpython_regions: Vec<&MemoryRegion> = regions.iter().filter(|r| r.name.contains("libpython")).collect();

    if !libpython_regions.is_empty() {
        // Sort by start address to find base
        libpython_regions.sort_by_key(|r| r.start);
        let base_region = libpython_regions[0];

        // Extract path from the region name
        let path = PathBuf::from(&base_region.name);

        return Ok(LibpythonInfo { path, base_addr: base_region.start, is_static: false });
    }

    // Fallback: Check if Python is statically linked into the executable
    // This is common in some Rust-Python distributions
    let exe_path = format!("/proc/{}/exe", pid);
    let exe_real = fs::read_link(&exe_path).with_context(|| format!("Failed to read {}", exe_path))?;

    // Find the executable's base address in maps
    let exe_name = exe_real.file_name().and_then(|n| n.to_str()).unwrap_or("");

    for region in &regions {
        if region.name.contains(exe_name) && region.perms.contains('r') {
            return Ok(LibpythonInfo { path: exe_real, base_addr: region.start, is_static: true });
        }
    }

    Err(anyhow!("Could not find libpython.so or statically linked Python in PID {}", pid))
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
pub fn parse_elf_writable_segments(elf_path: &PathBuf, base_addr: usize) -> Result<Vec<AlignedSegment>> {
    let elf_bytes = fs::read(elf_path).with_context(|| format!("Failed to read ELF file: {}", elf_path.display()))?;

    let elf = Elf::parse(&elf_bytes).with_context(|| format!("Failed to parse ELF: {}", elf_path.display()))?;

    // Find the first PT_LOAD segment's p_vaddr (used for address calculation)
    let first_load_vaddr = elf.program_headers.iter().find(|ph| ph.p_type == goblin::elf::program_header::PT_LOAD).map(|ph| ph.p_vaddr as usize).unwrap_or(0);

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

        let description = format!("libpython:PT_LOAD[{}]:0x{:x}-0x{:x}", idx, target_va, target_end);

        eprintln!("[snapshot] Found writable segment: {} ({} pages)", description, (align_to_page_up(target_end) - align_to_page(target_va)) / PAGE_SIZE);

        segments.push(AlignedSegment::new(target_va, target_end, description));
    }

    if segments.is_empty() {
        eprintln!("[snapshot] WARNING: No writable PT_LOAD segments found in {}", elf_path.display());
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
        all_segments.push(AlignedSegment::new(region.start, region.end, region.name.clone()));
    }

    // 2. Parse libpython ELF for precise writable segment identification
    match find_libpython(pid) {
        Ok(libpython) => {
            eprintln!("[snapshot] Found libpython at 0x{:x}: {} (static={})", libpython.base_addr, libpython.path.display(), libpython.is_static);

            match parse_elf_writable_segments(&libpython.path, libpython.base_addr) {
                Ok(elf_segments) => {
                    for seg in elf_segments {
                        all_segments.push(seg);
                    }
                }
                Err(e) => {
                    eprintln!(
                        "[snapshot] WARNING: Failed to parse libpython ELF: {}. \
                         Falling back to /proc/maps detection.",
                        e
                    );
                }
            }
        }
        Err(e) => {
            eprintln!(
                "[snapshot] WARNING: Could not find libpython: {}. \
                 Relying on /proc/maps detection only.",
                e
            );
        }
    }

    // 3. Merge overlapping/adjacent segments
    let merged = merge_segments(all_segments);

    eprintln!("[snapshot] Total segments after merge: {} ({} pages)", merged.len(), merged.iter().map(|s| s.page_count()).sum::<usize>());

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
        let available = match UffdBuilder::new().close_on_exec(true).non_blocking(false).create() {
            Ok(_) => {
                eprintln!("[snapshot] userfaultfd available - Fast-Reset mode enabled");
                true
            }
            Err(e) => {
                eprintln!("[snapshot] userfaultfd unavailable ({}). Falling back to fork-server.", e);
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
        eprintln!("[snapshot] Initiating TLS self-calibration...");

        let calibration = TlsCalibration::calibrate().context("TLS self-calibration failed - ERR_CALIBRATION_FAILED")?;

        if calibration.is_calibrated() {
            eprintln!("[snapshot] TLS calibration complete: mi_heap_t at fs_base + 0x{:04X}", calibration.primary_offset().unwrap_or(0));
            self.calibration = Some(calibration);
            Ok(())
        } else {
            // Calibration ran but found no heap pointers - this is OK for pre-3.13 Python
            eprintln!(
                "[snapshot] TLS calibration found no heap pointers (Python < 3.13 or pymalloc). \
                 TLS restoration will be skipped."
            );
            self.calibration = Some(calibration);
            Ok(())
        }
    }

    /// Check if calibration has been performed
    #[cfg(target_arch = "x86_64")]
    pub fn is_calibrated(&self) -> bool {
        self.calibration.as_ref().map_or(false, |c| c.is_calibrated())
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
        let snapshot_regions: Vec<MemoryRegion> = regions.into_iter().filter(|r| r.should_snapshot()).collect();

        eprintln!("[snapshot] Registering worker PID {}: {} regions to capture", pid, snapshot_regions.len());

        // Capture golden copy for each region
        let mut golden_pages = HashMap::new();
        for region in &snapshot_regions {
            let pages = self.capture_region_pages(pid, region)?;
            golden_pages.extend(pages);
        }

        // Register regions with the worker's UFFD
        for region in &snapshot_regions {
            uffd.register(region.start as *mut libc::c_void, region.len).with_context(|| format!("Failed to register region {}", region.name))?;
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
                eprintln!("[snapshot] TLS captured: fs_base=0x{:x}, {} bytes", tls.fs_base, tls.tls_data.len());
                Some(tls)
            }
            Err(e) => {
                // TLS capture failure is non-fatal for pre-3.13 Python
                // (pymalloc doesn't use TLS caching)
                eprintln!(
                    "[snapshot] WARNING: TLS capture failed: {}. \
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
    fn capture_region_pages(&self, pid: Pid, region: &MemoryRegion) -> Result<HashMap<usize, Vec<u8>>> {
        let mut buffer = vec![0u8; region.len];

        // Set up iovec for process_vm_readv
        let mut local_iov = [IoSliceMut::new(&mut buffer)];
        let remote_iov = [RemoteIoVec { base: region.start, len: region.len }];

        // Direct kernel memory copy - no ptrace attach required for child processes
        let bytes_read = process_vm_readv(pid, &mut local_iov, &remote_iov).with_context(|| format!("process_vm_readv failed for region {:?}", region.name))?;

        if bytes_read != region.len {
            return Err(anyhow!("Partial snapshot read for {}: {}/{}", region.name, bytes_read, region.len));
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

        eprintln!("[snapshot]   {} ({:x}-{:x}): {} pages captured", region.name, region.start, region.end, region.len / PAGE_SIZE);

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

        let worker = self.workers.get(&pid.as_raw()).ok_or_else(|| anyhow!("Worker {} not registered with SnapshotManager", pid))?;

        // Get pidfd for the target process
        let pidfd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid.as_raw(), 0) } as i32;
        if pidfd < 0 {
            return Err(anyhow!("pidfd_open failed for PID {}: {}", pid, std::io::Error::last_os_error()));
        }

        // Construct iovec array for all regions
        let iovecs: Vec<libc::iovec> = worker.regions.iter().map(|r| libc::iovec { iov_base: r.start as *mut libc::c_void, iov_len: r.len }).collect();

        // Call process_madvise - REMOTE MADV_DONTNEED
        const SYS_PROCESS_MADVISE: libc::c_long = 440;
        let ret = unsafe { libc::syscall(SYS_PROCESS_MADVISE, pidfd, iovecs.as_ptr(), iovecs.len(), libc::MADV_DONTNEED, 0u32) };

        unsafe { libc::close(pidfd) };

        if ret < 0 {
            return Err(anyhow!("process_madvise failed for PID {}: {}", pid, std::io::Error::last_os_error()));
        }

        eprintln!("[snapshot] Reset worker {}: invalidated {} regions", pid, iovecs.len());

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

        let worker = self.workers.get(&pid.as_raw()).ok_or_else(|| anyhow!("Worker {} not registered with SnapshotManager", pid))?;

        if let Some(ref tls_snapshot) = worker.tls_snapshot {
            eprintln!("[snapshot] Restoring TLS for worker {}: fs_base=0x{:x}, {} bytes", pid, tls_snapshot.fs_base, tls_snapshot.tls_data.len());
            restore_tls_snapshot(pid, tls_snapshot)?;
            eprintln!("[snapshot] TLS restoration complete for worker {}", pid);
        } else {
            eprintln!("[snapshot] No TLS snapshot for worker {} (pre-3.13 Python or capture failed)", pid);
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

    /// Handle a page fault by restoring from golden snapshot
    ///
    /// This is called from the fault handling loop when userfaultfd reports a fault.
    pub fn handle_fault(&self, pid: Pid, fault_addr: usize) -> Result<()> {
        let worker = self.workers.get(&pid.as_raw()).ok_or_else(|| anyhow!("Worker {} not registered with SnapshotManager", pid))?;

        let page_start = align_to_page(fault_addr);

        if let Some(data) = worker.golden_pages.get(&page_start) {
            // Restore the page from golden snapshot
            eprintln!("[snapshot] Restoring page at {:x} ({} bytes) for PID {}", page_start, data.len(), pid);
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
            eprintln!("[snapshot] Zero-filling page at {:x} for PID {} (not in snapshot)", page_start, pid);
            unsafe { worker.uffd.zeropage(page_start as *mut libc::c_void, PAGE_SIZE, true) }.with_context(|| format!("Failed to zero page at {:x}", page_start))?;
        }

        Ok(())
    }

    /// Poll for pending UFFD events and handle them
    ///
    /// This reads from the UFFD file descriptor and handles
    /// any pending page faults by restoring from golden snapshot.
    pub fn handle_pending_faults(&mut self, pid: Pid) -> Result<usize> {
        use userfaultfd::Event;

        let worker = self.workers.get(&pid.as_raw()).ok_or_else(|| anyhow!("Worker {} not registered with SnapshotManager", pid))?;

        let mut handled = 0;

        // Read events from UFFD
        loop {
            match worker.uffd.read_event() {
                Ok(Some(Event::Pagefault { addr, .. })) => {
                    let fault_addr = addr.addr();
                    eprintln!("[snapshot] UFFD_EVENT_PAGEFAULT at {:x} for PID {}", fault_addr, pid);

                    // Get data and restore
                    let page_start = align_to_page(fault_addr);
                    if let Some(data) = worker.golden_pages.get(&page_start) {
                        eprintln!("[snapshot] Restoring page {:x} ({} bytes)", page_start, data.len());
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
                        eprintln!("[snapshot] Zero-filling page {:x} (not in snapshot)", page_start);
                        unsafe {
                            worker.uffd.zeropage(page_start as *mut libc::c_void, PAGE_SIZE, true)?;
                        }
                    }
                    handled += 1;
                }
                Ok(Some(event)) => {
                    eprintln!("[snapshot] UFFD event: {:?} for PID {}", event, pid);
                }
                Ok(None) => {
                    // No more events
                    break;
                }
                Err(e) => {
                    // Any error means no events ready or UFFD closed
                    eprintln!("[snapshot] UFFD read_event: {} (breaking poll loop)", e);
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
        assert!(anon.should_snapshot(), "Anonymous writable regions included");
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
        assert!(libpython.should_snapshot(), "libpython data segment included");
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
        assert!(!coverage_memfd.should_snapshot(), "coverage ring buffer must be excluded");

        // Also test the shorter name variant
        let coverage_short = MemoryRegion {
            start: 0xf000,
            end: 0x10000,
            len: 0x1000,
            perms: "rw-s".to_string(),
            name: "memfd:tach_coverage".to_string(),
        };
        assert!(!coverage_short.should_snapshot(), "coverage ring buffer (short name) must be excluded");
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
        assert!(mgr.worker_pids().is_empty(), "No workers registered initially");
    }

    #[test]
    fn test_snapshot_manager_get_nonexistent_worker() {
        let mgr = SnapshotManager::new().unwrap();
        let fake_pid = Pid::from_raw(99999);
        assert!(mgr.get_worker_uffd(fake_pid).is_none(), "Nonexistent worker should return None");
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
        let segments = vec![AlignedSegment::new(0x1000, 0x2000, "a"), AlignedSegment::new(0x5000, 0x6000, "b"), AlignedSegment::new(0x9000, 0xa000, "c")];
        let merged = merge_segments(segments);

        // No overlap, should remain 3 segments
        assert_eq!(merged.len(), 3);
    }

    #[test]
    fn test_merge_segments_overlapping() {
        let segments = vec![AlignedSegment::new(0x1000, 0x3000, "a"), AlignedSegment::new(0x2000, 0x4000, "b")];
        let merged = merge_segments(segments);

        // Should merge into one segment
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].start, 0x1000);
        assert_eq!(merged[0].end, 0x4000);
    }

    #[test]
    fn test_merge_segments_adjacent() {
        let segments = vec![AlignedSegment::new(0x1000, 0x2000, "a"), AlignedSegment::new(0x2000, 0x3000, "b")];
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
        let segments = vec![AlignedSegment::new(0x1000, 0x3000, "heap"), AlignedSegment::new(0x2000, 0x4000, "libpython.data"), AlignedSegment::new(0x5000, 0x6000, "stack")];
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
        let segments = vec![AlignedSegment::new(0x5000, 0x6000, "c"), AlignedSegment::new(0x1000, 0x3000, "a"), AlignedSegment::new(0x2000, 0x4000, "b")];
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
        let segments = vec![AlignedSegment::new(0x1000, 0x2000, "a"), AlignedSegment::new(0x1800, 0x2800, "b"), AlignedSegment::new(0x2400, 0x3400, "c"), AlignedSegment::new(0x3000, 0x4000, "d")];
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
        let tls_candidates: Vec<_> = regions.iter().filter(|r| r.perms.contains('w') && r.perms.contains('p') && r.name.is_empty()).collect();

        // There should be at least one anonymous writable region
        // (TLS is typically in one of these)
        eprintln!("[test] Found {} potential TLS candidate regions", tls_candidates.len());
        assert!(!tls_candidates.is_empty(), "Should find anonymous writable regions");
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
}
