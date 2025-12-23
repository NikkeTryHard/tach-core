//! Snapshot Manager: userfaultfd-based memory reset for worker recycling
//!
//! This module implements the "Snapshot-Hypervisor" pattern for Tach:
//! - Capture a "golden" snapshot of worker memory after initialization
//! - Reset workers to that snapshot after each test (instead of killing them)
//! - Handle page faults via userfaultfd to lazily restore pages
//!
//! This eliminates fork() overhead in the hot loop (target: <50μs reset vs ~1ms fork)

use anyhow::{anyhow, Context, Result};
use nix::sys::socket::{sendmsg, ControlMessage, MsgFlags};
use nix::sys::uio::{process_vm_readv, RemoteIoVec};
use nix::unistd::Pid;
use std::collections::HashMap;
use std::fs;
use std::io::{IoSlice, IoSliceMut};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::net::UnixStream;
use userfaultfd::{Uffd, UffdBuilder};

/// Page size (4KB on x86_64/aarch64)
const PAGE_SIZE: usize = 4096;

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
    /// We exclude: vDSO, vsyscall, read-only mappings, shared mappings
    pub fn should_snapshot(&self) -> bool {
        // Must be writable
        if !self.perms.contains('w') {
            return false;
        }

        // Skip vDSO and vsyscall
        if self.name.contains("[vdso]") || self.name.contains("[vsyscall]") {
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

/// Parse /proc/{pid}/maps to extract memory regions
///
/// Format: start-end perms offset dev inode pathname
/// Example: 7f1234560000-7f1234580000 rw-p 00000000 00:00 0 [heap]
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
}

// =============================================================================
// Snapshot Manager
// =============================================================================

/// Central manager for capturing and restoring worker memory
pub struct SnapshotManager {
    /// Whether userfaultfd is available
    pub available: bool,
    /// Per-worker snapshots
    workers: HashMap<i32, WorkerSnapshot>,
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
                eprintln!("[snapshot] userfaultfd available - Fast-Reset mode enabled");
                true
            }
            Err(e) => {
                eprintln!(
                    "[snapshot] userfaultfd unavailable ({}). Falling back to fork-server.",
                    e
                );
                false
            }
        };

        Ok(Self {
            available,
            workers: HashMap::new(),
        })
    }

    /// Get the raw UFFD file descriptor for a worker (for polling)
    pub fn get_worker_uffd(&self, pid: Pid) -> Option<RawFd> {
        self.workers.get(&pid.as_raw()).map(|w| w.uffd.as_raw_fd())
    }

    /// Register a worker with its UFFD (received via SCM_RIGHTS)
    ///
    /// This is called when a worker sends its UFFD to the Supervisor.
    /// The worker must be in SIGSTOP state before calling this.
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
            "[snapshot] Registering worker PID {}: {} regions to capture",
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

        // Store worker snapshot
        self.workers.insert(
            pid.as_raw(),
            WorkerSnapshot {
                uffd,
                golden_pages,
                regions: snapshot_regions,
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
            "[snapshot]   {} ({:x}-{:x}): {} pages captured",
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
            "[snapshot] Reset worker {}: invalidated {} regions",
            pid,
            iovecs.len()
        );

        Ok(())
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
                "[snapshot] Restoring page at {:x} ({} bytes) for PID {}",
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
                "[snapshot] Zero-filling page at {:x} for PID {} (not in snapshot)",
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
                    let fault_addr = addr.addr();
                    eprintln!(
                        "[snapshot] UFFD_EVENT_PAGEFAULT at {:x} for PID {}",
                        fault_addr, pid
                    );

                    // Get data and restore
                    let page_start = align_to_page(fault_addr);
                    if let Some(data) = worker.golden_pages.get(&page_start) {
                        eprintln!(
                            "[snapshot] Restoring page {:x} ({} bytes)",
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
                            "[snapshot] Zero-filling page {:x} (not in snapshot)",
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
}
