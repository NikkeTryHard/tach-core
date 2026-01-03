//! Phase 5.1: Zero-Overhead Coverage via PEP 669 (sys.monitoring)
//!
//! This module implements a high-performance coverage collection system using:
//! - Shared memory ring buffer (memfd_create + mmap)
//! - Lock-free atomic operations for minimal overhead
//! - PEP 669 sys.monitoring callbacks (Python 3.12+)
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                           SHARED MEMORY                                  │
//! │  ┌─────────────────────────────────────────────────────────────────┐   │
//! │  │  RingBufferHeader (64 bytes)                                     │   │
//! │  │  ┌──────────────┬──────────────┬──────────────┬──────────────┐  │   │
//! │  │  │ write_idx    │ read_idx     │ capacity     │ overflow_cnt │  │   │
//! │  │  │ (AtomicU64)  │ (AtomicU64)  │ (u64)        │ (AtomicU64)  │  │   │
//! │  │  └──────────────┴──────────────┴──────────────┴──────────────┘  │   │
//! │  └─────────────────────────────────────────────────────────────────┘   │
//! │  ┌─────────────────────────────────────────────────────────────────┐   │
//! │  │  CoverageEntry[0..capacity] (16 bytes each)                      │   │
//! │  │  ┌──────────────┬──────────────┐                                 │   │
//! │  │  │ code_id (u64)│ lineno (u32) │ flags (u32)                     │   │
//! │  │  └──────────────┴──────────────┘                                 │   │
//! │  └─────────────────────────────────────────────────────────────────┘   │
//! └─────────────────────────────────────────────────────────────────────────┘
//!           ▲                                           │
//!           │ mmap (MAP_SHARED)                         │ mmap (MAP_SHARED)
//!           │                                           ▼
//!    ┌──────────────┐                           ┌──────────────┐
//!    │   WORKER     │                           │  SUPERVISOR  │
//!    │  (Python)    │                           │  (Aggregator)│
//!    │              │                           │              │
//!    │ LINE callback│                           │ Drain thread │
//!    │ writes entry │                           │ reads entries│
//!    └──────────────┘                           └──────────────┘
//! ```
//!
//! # Critical Design Decisions
//!
//! 1. **memfd_create**: Creates anonymous file backed by RAM, appears in
//!    /proc/pid/maps as "memfd:tach_coverage" - MUST be excluded from uffd.
//!
//! 2. **MAP_SHARED**: Both processes see the same memory. No IPC needed.
//!
//! 3. **Lock-free writes**: Worker uses atomic fetch_add for write_idx.
//!    No locks in the hot path.
//!
//! 4. **GIL discipline**: Python callback releases GIL before writing.
//!    This prevents serialization with Supervisor's polling.
//!
//! 5. **Overflow handling**: If buffer is full, increment overflow counter
//!    and drop the entry. Never block the worker.

use anyhow::{anyhow, Context, Result};
use pyo3::prelude::*;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

// =============================================================================
// Constants
// =============================================================================

/// Default ring buffer capacity (number of entries)
/// 4MB total = 262,144 entries at 16 bytes each
pub const DEFAULT_CAPACITY: usize = 262_144;

/// Size of the header in bytes (aligned to 64 bytes for cache line)
pub const HEADER_SIZE: usize = 64;

/// Size of each entry in bytes
pub const ENTRY_SIZE: usize = 16;

/// Name used for memfd_create (appears in /proc/pid/maps)
pub const MEMFD_NAME: &str = "tach_coverage";

// =============================================================================
// Phase 6.1: Mapping Ring Buffer Constants
// =============================================================================

/// Mapping ring buffer capacity (number of entries)
/// 8K entries should be sufficient for most test suites
pub const MAPPING_CAPACITY: usize = 8_192;

/// Size of each mapping entry in bytes
pub const MAPPING_ENTRY_SIZE: usize = 256;

/// Name used for mapping memfd_create
pub const MAPPING_MEMFD_NAME: &str = "tach_mapping";

// =============================================================================
// Data Structures
// =============================================================================

/// Ring buffer header stored at the start of shared memory.
///
/// Layout: 64 bytes total (cache-line aligned)
/// - write_idx: Next position for worker to write (0..capacity)
/// - read_idx: Next position for supervisor to read (0..capacity)
/// - capacity: Total number of entries
/// - overflow_count: Entries dropped due to full buffer
#[repr(C, align(64))]
pub struct RingBufferHeader {
    /// Next write position (worker increments atomically)
    pub write_idx: AtomicU64,
    /// Next read position (supervisor increments)
    pub read_idx: AtomicU64,
    /// Total capacity in entries
    pub capacity: u64,
    /// Number of entries dropped due to overflow
    pub overflow_count: AtomicU64,
    /// Padding to 64 bytes
    _padding: [u8; 32],
}

impl RingBufferHeader {
    /// Check if the buffer is full
    #[inline]
    pub fn is_full(&self) -> bool {
        let write = self.write_idx.load(Ordering::Acquire);
        let read = self.read_idx.load(Ordering::Acquire);
        // Buffer is full when write is one lap ahead of read
        write.wrapping_sub(read) >= self.capacity
    }

    /// Get number of entries available to read
    #[inline]
    pub fn available(&self) -> u64 {
        let write = self.write_idx.load(Ordering::Acquire);
        let read = self.read_idx.load(Ordering::Acquire);
        write.wrapping_sub(read)
    }
}

/// Coverage entry written by the worker for each LINE event.
///
/// Layout: 16 bytes (aligned for efficient access)
/// - code_id: Memory address of the code object (for mapping to filename)
/// - lineno: Line number within the file
/// - flags: Reserved for future use (call/return/exception types)
#[repr(C, align(16))]
#[derive(Clone, Copy, Debug, Default)]
pub struct CoverageEntry {
    /// Memory address of the Python code object
    /// Used to map back to (filename, function_name) via ModuleRegistry
    pub code_id: u64,
    /// Line number within the source file
    pub lineno: u32,
    /// Flags for event type (reserved for future use)
    /// Bit 0: LINE event
    /// Bit 1: CALL event
    /// Bit 2: RETURN event
    pub flags: u32,
}

impl CoverageEntry {
    /// Create a new LINE event entry
    #[inline]
    pub fn line(code_id: u64, lineno: u32) -> Self {
        Self {
            code_id,
            lineno,
            flags: 0x01, // LINE event
        }
    }
}

// =============================================================================
// Phase 6.1: Mapping Entry for code_id -> filename resolution
// =============================================================================

/// Mapping entry for registering code_id -> filename mappings.
///
/// Layout: 256 bytes total
/// - code_id: Memory address of the Python code object (8 bytes)
/// - filename_len: Length of filename in bytes (2 bytes)
/// - _padding: Alignment padding (6 bytes)
/// - filename: UTF-8 filename bytes, truncated from left if > 240 bytes (240 bytes)
///
/// The filename is truncated from the LEFT to preserve the actual filename
/// while dropping long path prefixes.
#[repr(C, align(8))]
#[derive(Clone, Copy)]
pub struct MappingEntry {
    /// Memory address of the Python code object
    pub code_id: u64,
    /// Length of filename in bytes (max 240)
    pub filename_len: u16,
    /// Padding for alignment
    pub _padding: [u8; 6],
    /// Filename bytes (truncated from left if > 240 bytes)
    pub filename: [u8; 240],
}

impl Default for MappingEntry {
    fn default() -> Self {
        Self {
            code_id: 0,
            filename_len: 0,
            _padding: [0u8; 6],
            filename: [0u8; 240],
        }
    }
}

impl MappingEntry {
    /// Create a new mapping entry.
    ///
    /// If the filename is longer than 240 bytes, it is truncated from the LEFT
    /// to preserve the actual filename while dropping long path prefixes.
    ///
    /// # Safety
    /// This function handles UTF-8 boundary correctly by using char_indices.
    pub fn new(code_id: u64, filename: &str) -> Self {
        let mut entry = Self {
            code_id,
            ..Default::default()
        };

        let bytes = filename.as_bytes();
        if bytes.len() <= 240 {
            // Fits entirely
            entry.filename_len = bytes.len() as u16;
            entry.filename[..bytes.len()].copy_from_slice(bytes);
        } else {
            // Truncate from LEFT - find a valid UTF-8 boundary
            // Start from (len - 240) and find the next char boundary
            let start = bytes.len() - 240;
            // Find the next valid UTF-8 start byte
            let mut safe_start = start;
            while safe_start < bytes.len() && (bytes[safe_start] & 0b1100_0000) == 0b1000_0000 {
                safe_start += 1;
            }
            let slice = &bytes[safe_start..];
            entry.filename_len = slice.len() as u16;
            entry.filename[..slice.len()].copy_from_slice(slice);
        }

        entry
    }

    /// Extract the filename as a String.
    pub fn filename(&self) -> String {
        String::from_utf8_lossy(&self.filename[..self.filename_len as usize]).to_string()
    }
}

// =============================================================================
// Ring Buffer Implementation
// =============================================================================

/// Shared memory ring buffer for coverage data.
///
/// Created via memfd_create + mmap for zero-copy IPC between worker and supervisor.
pub struct CoverageRingBuffer {
    /// Pointer to the mmap'd region
    ptr: *mut u8,
    /// Total size of the mmap'd region in bytes
    size: usize,
    /// File descriptor from memfd_create (kept open for sharing)
    fd: i32,
    /// Capacity in number of entries
    capacity: usize,
}

// Safety: The ring buffer uses atomic operations for synchronization
// and is designed for concurrent access from multiple processes.
unsafe impl Send for CoverageRingBuffer {}
unsafe impl Sync for CoverageRingBuffer {}

impl CoverageRingBuffer {
    /// Create a new ring buffer with the specified capacity.
    ///
    /// Uses memfd_create to create anonymous shared memory that:
    /// 1. Appears in /proc/pid/maps as "memfd:tach_coverage"
    /// 2. Can be shared with child processes via fork()
    /// 3. Is automatically cleaned up when all references are closed
    ///
    /// # Arguments
    /// * `capacity` - Number of CoverageEntry slots
    ///
    /// # Returns
    /// * `Ok(CoverageRingBuffer)` - Ready to use ring buffer
    /// * `Err` - If memfd_create or mmap fails
    pub fn new(capacity: usize) -> Result<Self> {
        let total_size = HEADER_SIZE + (capacity * ENTRY_SIZE);

        // Create anonymous file via memfd_create
        // The name appears in /proc/pid/maps for identification
        let fd = unsafe {
            let name = std::ffi::CString::new(MEMFD_NAME)
                .context("Failed to create CString for memfd name")?;
            libc::memfd_create(name.as_ptr(), libc::MFD_CLOEXEC)
        };

        if fd < 0 {
            return Err(anyhow!(
                "memfd_create failed: {}",
                std::io::Error::last_os_error()
            ));
        }

        // Set the size of the file
        let ret = unsafe { libc::ftruncate(fd, total_size as libc::off_t) };
        if ret < 0 {
            unsafe { libc::close(fd) };
            return Err(anyhow!(
                "ftruncate failed: {}",
                std::io::Error::last_os_error()
            ));
        }

        // Map the file into memory
        // MAP_SHARED: Changes are visible to other processes
        let ptr = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                total_size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd,
                0,
            )
        };

        if ptr == libc::MAP_FAILED {
            unsafe { libc::close(fd) };
            return Err(anyhow!("mmap failed: {}", std::io::Error::last_os_error()));
        }

        // Initialize the header
        let header = ptr as *mut RingBufferHeader;
        unsafe {
            (*header).write_idx = AtomicU64::new(0);
            (*header).read_idx = AtomicU64::new(0);
            (*header).capacity = capacity as u64;
            (*header).overflow_count = AtomicU64::new(0);
            (*header)._padding = [0u8; 32];
        }

        eprintln!(
            "[coverage] Created ring buffer: {} entries, {} bytes total",
            capacity, total_size
        );

        Ok(Self {
            ptr: ptr as *mut u8,
            size: total_size,
            fd,
            capacity,
        })
    }

    /// Get the file descriptor for sharing with child processes.
    ///
    /// The child can mmap this fd to access the same shared memory.
    pub fn fd(&self) -> i32 {
        self.fd
    }

    /// Get pointer to the header.
    #[inline]
    pub fn header(&self) -> &RingBufferHeader {
        unsafe { &*(self.ptr as *const RingBufferHeader) }
    }

    /// Get mutable pointer to the header.
    ///
    /// # Safety
    /// This uses interior mutability through shared memory. The caller must ensure
    /// proper synchronization (atomic operations are used in the header fields).
    #[inline]
    #[allow(clippy::mut_from_ref)] // Intentional: shared memory with atomic synchronization
    pub fn header_mut(&self) -> &mut RingBufferHeader {
        unsafe { &mut *(self.ptr as *mut RingBufferHeader) }
    }

    /// Get pointer to the entry array.
    #[inline]
    fn entries_ptr(&self) -> *mut CoverageEntry {
        unsafe { self.ptr.add(HEADER_SIZE) as *mut CoverageEntry }
    }

    /// Write an entry to the ring buffer (worker side).
    ///
    /// This is the HOT PATH - must be as fast as possible.
    /// Uses lock-free atomic operations with CAS loop to prevent TOCTOU race.
    ///
    /// # Returns
    /// * `true` if entry was written
    /// * `false` if buffer was full (entry dropped, overflow counter incremented)
    #[inline]
    pub fn write(&self, entry: CoverageEntry) -> bool {
        let header = self.header();

        loop {
            let write = header.write_idx.load(Ordering::Acquire);
            let read = header.read_idx.load(Ordering::Acquire);

            // Check if buffer is full
            if write.wrapping_sub(read) >= header.capacity {
                header.overflow_count.fetch_add(1, Ordering::Relaxed);
                return false;
            }

            // Try to reserve a slot atomically using CAS
            match header.write_idx.compare_exchange_weak(
                write,
                write.wrapping_add(1),
                Ordering::AcqRel,
                Ordering::Relaxed,
            ) {
                Ok(_) => {
                    // Successfully reserved slot
                    let slot = (write % self.capacity as u64) as usize;

                    // Write the entry
                    unsafe {
                        let entry_ptr = self.entries_ptr().add(slot);
                        std::ptr::write_volatile(entry_ptr, entry);
                    }

                    return true;
                }
                Err(_) => {
                    // CAS failed, another thread got there first - spin and retry
                    std::hint::spin_loop();
                    continue;
                }
            }
        }
    }

    /// Read entries from the ring buffer (supervisor side).
    ///
    /// Drains up to `max_entries` from the buffer into the provided vector.
    ///
    /// # Arguments
    /// * `out` - Vector to append entries to
    /// * `max_entries` - Maximum number of entries to read
    ///
    /// # Returns
    /// Number of entries read
    pub fn drain(&self, out: &mut Vec<CoverageEntry>, max_entries: usize) -> usize {
        let header = self.header();
        let available = header.available() as usize;
        let to_read = available.min(max_entries);

        if to_read == 0 {
            return 0;
        }

        out.reserve(to_read);

        for _ in 0..to_read {
            let idx = header.read_idx.fetch_add(1, Ordering::AcqRel);
            let slot = (idx % self.capacity as u64) as usize;

            let entry = unsafe {
                let entry_ptr = self.entries_ptr().add(slot);
                std::ptr::read_volatile(entry_ptr)
            };

            out.push(entry);
        }

        to_read
    }

    /// Get the overflow count (entries dropped due to full buffer).
    pub fn overflow_count(&self) -> u64 {
        self.header().overflow_count.load(Ordering::Relaxed)
    }

    /// Get the base address of the mmap'd region.
    ///
    /// Used for excluding this region from userfaultfd registration.
    pub fn base_addr(&self) -> usize {
        self.ptr as usize
    }

    /// Get the size of the mmap'd region.
    pub fn region_size(&self) -> usize {
        self.size
    }
}

impl Drop for CoverageRingBuffer {
    fn drop(&mut self) {
        unsafe {
            if !self.ptr.is_null() {
                libc::munmap(self.ptr as *mut libc::c_void, self.size);
            }
            if self.fd >= 0 {
                libc::close(self.fd);
            }
        }
    }
}

// =============================================================================
// Phase 6.1: Mapping Ring Buffer Implementation
// =============================================================================

/// Shared memory ring buffer for code_id -> filename mappings.
///
/// Similar to CoverageRingBuffer but with larger entries (256 bytes each)
/// to accommodate filenames. Used by PY_START callback to register
/// code objects on first encounter.
pub struct MappingRingBuffer {
    /// Pointer to the mmap'd region
    ptr: *mut u8,
    /// Total size of the mmap'd region in bytes
    size: usize,
    /// File descriptor from memfd_create
    fd: i32,
    /// Capacity in number of entries
    capacity: usize,
}

// Safety: Same as CoverageRingBuffer - uses atomic operations
unsafe impl Send for MappingRingBuffer {}
unsafe impl Sync for MappingRingBuffer {}

impl MappingRingBuffer {
    /// Create a new mapping ring buffer with the specified capacity.
    pub fn new(capacity: usize) -> Result<Self> {
        let total_size = HEADER_SIZE + (capacity * MAPPING_ENTRY_SIZE);

        // Create anonymous file via memfd_create
        let fd = unsafe {
            let name = std::ffi::CString::new(MAPPING_MEMFD_NAME)
                .context("Failed to create CString for mapping memfd name")?;
            libc::memfd_create(name.as_ptr(), libc::MFD_CLOEXEC)
        };

        if fd < 0 {
            return Err(anyhow!(
                "memfd_create failed for mapping buffer: {}",
                std::io::Error::last_os_error()
            ));
        }

        // Set the size of the file
        let ret = unsafe { libc::ftruncate(fd, total_size as libc::off_t) };
        if ret < 0 {
            unsafe { libc::close(fd) };
            return Err(anyhow!(
                "ftruncate failed for mapping buffer: {}",
                std::io::Error::last_os_error()
            ));
        }

        // Map the file into memory
        let ptr = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                total_size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd,
                0,
            )
        };

        if ptr == libc::MAP_FAILED {
            unsafe { libc::close(fd) };
            return Err(anyhow!(
                "mmap failed for mapping buffer: {}",
                std::io::Error::last_os_error()
            ));
        }

        // Initialize the header
        let header = ptr as *mut RingBufferHeader;
        unsafe {
            (*header).write_idx = AtomicU64::new(0);
            (*header).read_idx = AtomicU64::new(0);
            (*header).capacity = capacity as u64;
            (*header).overflow_count = AtomicU64::new(0);
            (*header)._padding = [0u8; 32];
        }

        eprintln!(
            "[coverage] Created mapping buffer: {} entries, {} bytes total",
            capacity, total_size
        );

        Ok(Self {
            ptr: ptr as *mut u8,
            size: total_size,
            fd,
            capacity,
        })
    }

    /// Get pointer to the header.
    #[inline]
    pub fn header(&self) -> &RingBufferHeader {
        unsafe { &*(self.ptr as *const RingBufferHeader) }
    }

    /// Get pointer to the entry array.
    #[inline]
    fn entries_ptr(&self) -> *mut MappingEntry {
        unsafe { self.ptr.add(HEADER_SIZE) as *mut MappingEntry }
    }

    /// Write a mapping entry to the ring buffer.
    ///
    /// Called from PY_START callback on first encounter of a code object.
    ///
    /// Uses a CAS (Compare-And-Swap) loop to prevent TOCTOU race:
    /// Multiple threads could pass is_full() check simultaneously,
    /// then all increment write_idx, causing buffer overflow.
    #[inline]
    pub fn write(&self, entry: MappingEntry) -> bool {
        let header = self.header();

        loop {
            let write = header.write_idx.load(Ordering::Acquire);
            let read = header.read_idx.load(Ordering::Acquire);

            // Check if buffer is full
            if write.wrapping_sub(read) >= header.capacity {
                header.overflow_count.fetch_add(1, Ordering::Relaxed);
                return false;
            }

            // Try to reserve a slot atomically using CAS
            match header.write_idx.compare_exchange_weak(
                write,
                write.wrapping_add(1),
                Ordering::AcqRel,
                Ordering::Relaxed,
            ) {
                Ok(_) => {
                    // Successfully reserved slot
                    let slot = (write % self.capacity as u64) as usize;

                    // Write the entry
                    unsafe {
                        let entry_ptr = self.entries_ptr().add(slot);
                        std::ptr::write_volatile(entry_ptr, entry);
                    }

                    return true;
                }
                Err(_) => {
                    // CAS failed, another thread got there first - spin and retry
                    std::hint::spin_loop();
                    continue;
                }
            }
        }
    }

    /// Drain mapping entries from the buffer.
    ///
    /// Called by CoverageAggregator to populate code_map.
    pub fn drain(&self, out: &mut Vec<MappingEntry>, max_entries: usize) -> usize {
        let header = self.header();
        let available = header.available() as usize;
        let to_read = available.min(max_entries);

        if to_read == 0 {
            return 0;
        }

        out.reserve(to_read);

        for _ in 0..to_read {
            let idx = header.read_idx.fetch_add(1, Ordering::AcqRel);
            let slot = (idx % self.capacity as u64) as usize;

            let entry = unsafe {
                let entry_ptr = self.entries_ptr().add(slot);
                std::ptr::read_volatile(entry_ptr)
            };

            out.push(entry);
        }

        to_read
    }

    /// Get the overflow count.
    pub fn overflow_count(&self) -> u64 {
        self.header().overflow_count.load(Ordering::Relaxed)
    }
}

impl Drop for MappingRingBuffer {
    fn drop(&mut self) {
        unsafe {
            if !self.ptr.is_null() {
                libc::munmap(self.ptr as *mut libc::c_void, self.size);
            }
            if self.fd >= 0 {
                libc::close(self.fd);
            }
        }
    }
}

// =============================================================================
// Global Ring Buffer Instance
// =============================================================================

/// Global ring buffer instance (initialized by Supervisor, shared with Workers)
static RING_BUFFER: OnceLock<CoverageRingBuffer> = OnceLock::new();

/// Initialize the global coverage ring buffer.
///
/// Called by the Supervisor before forking workers.
pub fn init_coverage_buffer(capacity: usize) -> Result<&'static CoverageRingBuffer> {
    if RING_BUFFER.get().is_some() {
        return Err(anyhow!("Coverage ring buffer already initialized"));
    }

    let buffer = CoverageRingBuffer::new(capacity)?;

    RING_BUFFER
        .set(buffer)
        .map_err(|_| anyhow!("Failed to set global ring buffer"))?;

    // SAFETY: We just set the buffer above, so get() must succeed
    RING_BUFFER
        .get()
        .ok_or_else(|| anyhow!("Ring buffer not available after initialization"))
}

/// Get reference to the global coverage ring buffer.
pub fn get_coverage_buffer() -> Option<&'static CoverageRingBuffer> {
    RING_BUFFER.get()
}

/// Check if coverage is enabled (ring buffer initialized).
pub fn is_coverage_enabled() -> bool {
    RING_BUFFER.get().is_some()
}

// =============================================================================
// Phase 6.1: Global Mapping Buffer Instance
// =============================================================================

/// Global mapping buffer instance (initialized by Supervisor, shared with Workers)
static MAPPING_BUFFER: OnceLock<MappingRingBuffer> = OnceLock::new();

/// Initialize the global mapping ring buffer.
///
/// Called by the Supervisor before forking workers.
pub fn init_mapping_buffer(capacity: usize) -> Result<&'static MappingRingBuffer> {
    if MAPPING_BUFFER.get().is_some() {
        return Err(anyhow!("Mapping ring buffer already initialized"));
    }

    let buffer = MappingRingBuffer::new(capacity)?;

    MAPPING_BUFFER
        .set(buffer)
        .map_err(|_| anyhow!("Failed to set global mapping buffer"))?;

    // SAFETY: We just set the buffer above, so get() must succeed
    MAPPING_BUFFER
        .get()
        .ok_or_else(|| anyhow!("Mapping buffer not available after initialization"))
}

/// Get reference to the global mapping ring buffer.
pub fn get_mapping_buffer() -> Option<&'static MappingRingBuffer> {
    MAPPING_BUFFER.get()
}

// =============================================================================
// Phase 6.1: Thread-Local Seen Codes Set
// =============================================================================

use std::cell::RefCell;
use std::collections::HashSet;

thread_local! {
    /// Thread-local set of seen code object IDs.
    ///
    /// Used by PY_START callback to avoid duplicate registrations.
    /// Each thread maintains its own set for lock-free operation.
    ///
    /// Pre-sized to 1024 entries to reduce reallocations during test runs.
    /// A typical test file has 50-200 code objects, so 1024 covers most cases.
    static SEEN_CODES: RefCell<HashSet<u64>> = RefCell::new(HashSet::with_capacity(1024));
}

/// Check if code_id has been seen, mark as seen if not.
///
/// Returns `true` if this is the FIRST time seeing this code_id.
/// Returns `false` if already seen (no registration needed).
///
/// This is called from the PY_START callback for every function entry.
/// The thread-local set ensures O(1) lookup without any locking.
#[inline]
fn mark_code_seen(code_id: u64) -> bool {
    SEEN_CODES.with(|seen| {
        let mut set = seen.borrow_mut();
        if set.contains(&code_id) {
            false
        } else {
            set.insert(code_id);
            true
        }
    })
}

// =============================================================================
// Coverage Aggregator (Supervisor Side)
// =============================================================================

use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

/// Coverage data aggregated by the Supervisor.
///
/// Maps (filename, lineno) to hit count.
pub type CoverageData = HashMap<(String, u32), u64>;

/// Aggregator that drains the ring buffer and accumulates coverage data.
///
/// Runs in a dedicated thread, polling the ring buffer periodically.
pub struct CoverageAggregator {
    /// Accumulated coverage data: (filename, lineno) -> hit_count
    data: Arc<std::sync::Mutex<CoverageData>>,
    /// Code object ID to filename mapping (populated lazily)
    /// Uses RwLock for better read performance - reads are more frequent than writes
    code_map: Arc<std::sync::RwLock<HashMap<u64, String>>>,
    /// Signal to stop the aggregator thread
    stop_flag: Arc<AtomicBool>,
    /// Handle to the aggregator thread
    thread_handle: Option<JoinHandle<()>>,
}

impl CoverageAggregator {
    /// Create a new aggregator (does not start the thread yet).
    pub fn new() -> Self {
        Self {
            data: Arc::new(std::sync::Mutex::new(HashMap::new())),
            code_map: Arc::new(std::sync::RwLock::new(HashMap::new())),
            stop_flag: Arc::new(AtomicBool::new(false)),
            thread_handle: None,
        }
    }

    /// Start the aggregator thread.
    ///
    /// The thread polls both ring buffers every `poll_interval`:
    /// 1. Drain mapping buffer FIRST (populates code_map)
    /// 2. Drain coverage buffer (uses code_map for resolution)
    pub fn start(&mut self, poll_interval: Duration) {
        let data = Arc::clone(&self.data);
        let code_map = Arc::clone(&self.code_map);
        let stop_flag = Arc::clone(&self.stop_flag);

        let handle = thread::spawn(move || {
            let mut coverage_batch = Vec::with_capacity(4096);
            let mut mapping_batch = Vec::with_capacity(256);

            while !stop_flag.load(Ordering::Relaxed) {
                // 1. Drain mapping buffer FIRST (populates code_map)
                // This ensures code_id -> filename mappings are available
                // before we try to resolve coverage entries
                if let Some(mapping_buffer) = get_mapping_buffer() {
                    let count = mapping_buffer.drain(&mut mapping_batch, 1024);
                    if count > 0 {
                        let mut code_map_guard =
                            code_map.write().unwrap_or_else(|e| e.into_inner());
                        for entry in mapping_batch.drain(..) {
                            code_map_guard.insert(entry.code_id, entry.filename());
                        }
                    }
                }

                // 2. Drain coverage buffer (uses code_map for resolution)
                if let Some(buffer) = get_coverage_buffer() {
                    let count = buffer.drain(&mut coverage_batch, 4096);

                    if count > 0 {
                        // Process batch
                        let mut data_guard = data.lock().unwrap_or_else(|e| e.into_inner());
                        let code_map_guard = code_map.read().unwrap_or_else(|e| e.into_inner());

                        for entry in coverage_batch.drain(..) {
                            // Try to map code_id to filename
                            let filename = code_map_guard
                                .get(&entry.code_id)
                                .cloned()
                                .unwrap_or_else(|| format!("<code:{:x}>", entry.code_id));

                            // Increment hit count
                            let key = (filename, entry.lineno);
                            *data_guard.entry(key).or_insert(0) += 1;
                        }
                    }
                }

                thread::sleep(poll_interval);
            }

            // Final drain after stop signal
            // Drain mapping first, then coverage
            if let Some(mapping_buffer) = get_mapping_buffer() {
                let mut mapping_batch = Vec::new();
                mapping_buffer.drain(&mut mapping_batch, usize::MAX);
                if !mapping_batch.is_empty() {
                    let mut code_map_guard = code_map.write().unwrap_or_else(|e| e.into_inner());
                    for entry in mapping_batch {
                        code_map_guard.insert(entry.code_id, entry.filename());
                    }
                }
            }

            if let Some(buffer) = get_coverage_buffer() {
                let mut batch = Vec::new();
                buffer.drain(&mut batch, usize::MAX);

                if !batch.is_empty() {
                    let mut data_guard = data.lock().unwrap_or_else(|e| e.into_inner());
                    let code_map_guard = code_map.read().unwrap_or_else(|e| e.into_inner());

                    for entry in batch {
                        let filename = code_map_guard
                            .get(&entry.code_id)
                            .cloned()
                            .unwrap_or_else(|| format!("<code:{:x}>", entry.code_id));

                        let key = (filename, entry.lineno);
                        *data_guard.entry(key).or_insert(0) += 1;
                    }
                }
            }
        });

        self.thread_handle = Some(handle);
    }

    /// Register a code object ID to filename mapping.
    ///
    /// Called by the Supervisor when it learns about new code objects
    /// (e.g., from module loading or Python introspection).
    pub fn register_code(&self, code_id: u64, filename: String) {
        let mut code_map = self.code_map.write().unwrap_or_else(|e| e.into_inner());
        code_map.insert(code_id, filename);
    }

    /// Stop the aggregator thread and wait for it to finish.
    pub fn stop(&mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);

        if let Some(handle) = self.thread_handle.take() {
            let _ = handle.join();
        }
    }

    /// Get the accumulated coverage data.
    ///
    /// Returns a clone of the current coverage data.
    /// For final collection after stop(), prefer `take_data()` to avoid cloning.
    pub fn get_data(&self) -> CoverageData {
        self.data.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Take ownership of the accumulated coverage data.
    ///
    /// This is more efficient than `get_data()` as it avoids cloning.
    /// Should only be called after `stop()` to ensure all data is collected.
    pub fn take_data(&mut self) -> CoverageData {
        std::mem::take(&mut *self.data.lock().unwrap_or_else(|e| e.into_inner()))
    }

    /// Get the number of unique (file, line) pairs covered.
    pub fn covered_lines(&self) -> usize {
        self.data.lock().unwrap_or_else(|e| e.into_inner()).len()
    }

    /// Get the total number of line executions recorded.
    pub fn total_hits(&self) -> u64 {
        self.data
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .sum()
    }
}

impl Default for CoverageAggregator {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for CoverageAggregator {
    fn drop(&mut self) {
        self.stop();
    }
}

// =============================================================================
// PyO3 FFI Exports
// =============================================================================

/// Record a LINE event from Python's sys.monitoring callback.
///
/// This is called from Python for every LINE event. It MUST be fast.
///
/// # GIL Discipline
///
/// This function is called WITH the GIL held. We extract the necessary
/// data (code_id, lineno) while holding the GIL, then release it before
/// writing to the ring buffer.
///
/// # Arguments
/// * `code_id` - Memory address of the code object (id(code) in Python)
/// * `lineno` - Line number
///
/// # Returns
/// * `true` if entry was recorded
/// * `false` if coverage not enabled or buffer full
#[pyfunction]
#[pyo3(name = "record_line")]
pub fn py_record_line(py: Python<'_>, code_id: u64, lineno: u32) -> bool {
    // Release GIL before writing to ring buffer
    // This prevents serialization with Supervisor's aggregator thread
    py.detach(|| {
        if let Some(buffer) = get_coverage_buffer() {
            buffer.write(CoverageEntry::line(code_id, lineno))
        } else {
            false
        }
    })
}

/// Check if coverage collection is enabled.
#[pyfunction]
#[pyo3(name = "is_coverage_enabled")]
pub fn py_is_coverage_enabled() -> bool {
    is_coverage_enabled()
}

/// Get the current overflow count.
#[pyfunction]
#[pyo3(name = "get_coverage_overflow")]
pub fn py_get_coverage_overflow() -> u64 {
    get_coverage_buffer()
        .map(|b| b.overflow_count())
        .unwrap_or(0)
}

// =============================================================================
// Phase 6.1: PY_START Registration Callback
// =============================================================================

/// Record a PY_START event (function entry) for code_id -> filename registration.
///
/// This is the REGISTRATION PATH - called for every function entry.
/// Uses thread-local caching to ensure each code object is only registered once.
///
/// # Flow
/// 1. Check thread-local SEEN_CODES set (O(1) lookup)
/// 2. If new: write mapping to MappingRingBuffer, add to SEEN_CODES
/// 3. If seen: return immediately (no work)
///
/// # GIL Discipline
/// This function is called WITH the GIL held. We release the GIL before
/// writing to the ring buffer to avoid serialization with the aggregator.
///
/// # Arguments
/// * `code_id` - Memory address of the code object (id(code) in Python)
/// * `filename` - The co_filename attribute of the code object
#[pyfunction]
#[pyo3(name = "record_py_start")]
pub fn py_record_py_start(py: Python<'_>, code_id: u64, filename: String) {
    // Release GIL before doing any work
    py.detach(|| {
        // Check thread-local set (fast path for repeated calls)
        if mark_code_seen(code_id) {
            // First time seeing this code object - register mapping
            if let Some(buffer) = get_mapping_buffer() {
                let entry = MappingEntry::new(code_id, &filename);
                buffer.write(entry);
            }
        }
    });
}

/// Get the current mapping buffer overflow count.
#[pyfunction]
#[pyo3(name = "get_mapping_overflow")]
pub fn py_get_mapping_overflow() -> u64 {
    get_mapping_buffer()
        .map(|b| b.overflow_count())
        .unwrap_or(0)
}

// =============================================================================
// Coverage Output Writers
// =============================================================================

/// Write coverage data to LCOV format file.
///
/// LCOV format is widely supported by coverage visualization tools like
/// Codecov, Coveralls, and IDE plugins.
///
/// # Format
/// ```text
/// SF:/path/to/file.py
/// DA:10,5
/// DA:11,3
/// DA:15,0
/// LF:3
/// LH:2
/// end_of_record
/// ```
///
/// - SF: Source file path
/// - DA:line,hits: Line data (line number, hit count)
/// - LF: Lines found (total lines instrumented)
/// - LH: Lines hit (lines with hits > 0)
/// - end_of_record: End marker
pub fn write_lcov(data: &CoverageData, path: &std::path::Path) -> Result<()> {
    use std::collections::BTreeMap;
    use std::io::Write;

    // Group by filename, sorted for deterministic output
    let mut by_file: BTreeMap<&str, Vec<(u32, u64)>> = BTreeMap::new();
    for ((filename, lineno), hits) in data {
        by_file
            .entry(filename.as_str())
            .or_default()
            .push((*lineno, *hits));
    }

    let mut output = std::fs::File::create(path)
        .with_context(|| format!("Failed to create LCOV file: {}", path.display()))?;

    for (filename, mut lines) in by_file {
        // Sort lines by line number
        lines.sort_by_key(|(lineno, _)| *lineno);

        // SF: Source file
        writeln!(output, "SF:{}", filename)?;

        // DA: Line data
        for (lineno, hits) in &lines {
            writeln!(output, "DA:{},{}", lineno, hits)?;
        }

        // LF: Lines found (total instrumented)
        writeln!(output, "LF:{}", lines.len())?;

        // LH: Lines hit (with hits > 0)
        let lines_hit = lines.iter().filter(|(_, hits)| *hits > 0).count();
        writeln!(output, "LH:{}", lines_hit)?;

        writeln!(output, "end_of_record")?;
    }

    eprintln!("[coverage] Wrote LCOV report to {}", path.display());
    Ok(())
}

/// Write coverage data to JSON format file.
///
/// JSON format is useful for programmatic processing and integration
/// with custom tools.
///
/// # Format
/// ```json
/// {
///   "files": {
///     "/path/to/file.py": {
///       "lines": { "10": 5, "11": 3 },
///       "lines_found": 2,
///       "lines_hit": 2
///     }
///   },
///   "totals": {
///     "lines_found": 100,
///     "lines_hit": 80,
///     "line_coverage": 0.8
///   }
/// }
/// ```
pub fn write_json(data: &CoverageData, path: &std::path::Path) -> Result<()> {
    use std::collections::BTreeMap;

    // Group by filename
    let mut by_file: BTreeMap<&str, BTreeMap<u32, u64>> = BTreeMap::new();
    for ((filename, lineno), hits) in data {
        by_file
            .entry(filename.as_str())
            .or_default()
            .insert(*lineno, *hits);
    }

    // Build JSON structure
    let mut files_json = serde_json::Map::new();
    let mut total_found = 0usize;
    let mut total_hit = 0usize;

    for (filename, lines) in by_file {
        let lines_found = lines.len();
        let lines_hit = lines.values().filter(|&&h| h > 0).count();

        total_found += lines_found;
        total_hit += lines_hit;

        let lines_obj: serde_json::Map<String, serde_json::Value> = lines
            .into_iter()
            .map(|(lineno, hits)| (lineno.to_string(), serde_json::Value::from(hits)))
            .collect();

        let file_obj = serde_json::json!({
            "lines": lines_obj,
            "lines_found": lines_found,
            "lines_hit": lines_hit
        });

        files_json.insert(filename.to_string(), file_obj);
    }

    let coverage_pct = if total_found > 0 {
        (total_hit as f64) / (total_found as f64)
    } else {
        0.0
    };

    let output = serde_json::json!({
        "files": files_json,
        "totals": {
            "lines_found": total_found,
            "lines_hit": total_hit,
            "line_coverage": coverage_pct
        }
    });

    let file = std::fs::File::create(path)
        .with_context(|| format!("Failed to create JSON coverage file: {}", path.display()))?;

    serde_json::to_writer_pretty(file, &output)
        .with_context(|| format!("Failed to write JSON coverage: {}", path.display()))?;

    eprintln!("[coverage] Wrote JSON report to {}", path.display());
    Ok(())
}

/// Write coverage data to the specified output file.
///
/// The format is determined by the file extension:
/// - `.lcov` or `.info` -> LCOV format
/// - `.json` -> JSON format
/// - Other -> LCOV format (default)
pub fn write_coverage_report(
    data: &CoverageData,
    path: &std::path::Path,
    format: Option<&str>,
) -> Result<()> {
    let format = format.unwrap_or_else(|| {
        path.extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("lcov")
    });

    match format.to_lowercase().as_str() {
        "json" => write_json(data, path),
        "lcov" | "info" => write_lcov(data, path),
        _ => write_lcov(data, path),
    }
}

// =============================================================================
// Unit Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_entry_size() {
        assert_eq!(std::mem::size_of::<CoverageEntry>(), ENTRY_SIZE);
    }

    #[test]
    fn test_header_size() {
        assert_eq!(std::mem::size_of::<RingBufferHeader>(), HEADER_SIZE);
    }

    #[test]
    fn test_entry_alignment() {
        assert_eq!(std::mem::align_of::<CoverageEntry>(), 16);
    }

    #[test]
    fn test_header_alignment() {
        assert_eq!(std::mem::align_of::<RingBufferHeader>(), 64);
    }

    #[test]
    fn test_ring_buffer_creation() {
        let buffer = CoverageRingBuffer::new(1024).expect("Failed to create buffer");
        assert_eq!(buffer.capacity, 1024);
        assert!(buffer.fd >= 0);
        assert!(!buffer.ptr.is_null());
    }

    #[test]
    fn test_ring_buffer_write_read() {
        let buffer = CoverageRingBuffer::new(16).expect("Failed to create buffer");

        // Write some entries
        for i in 0..10 {
            let entry = CoverageEntry::line(0x1000 + i, i as u32);
            assert!(buffer.write(entry));
        }

        // Read them back
        let mut entries = Vec::new();
        let count = buffer.drain(&mut entries, 100);
        assert_eq!(count, 10);
        assert_eq!(entries.len(), 10);

        for (i, entry) in entries.iter().enumerate() {
            assert_eq!(entry.code_id, 0x1000 + i as u64);
            assert_eq!(entry.lineno, i as u32);
        }
    }

    #[test]
    fn test_ring_buffer_overflow() {
        let buffer = CoverageRingBuffer::new(4).expect("Failed to create buffer");

        // Fill the buffer
        for i in 0..4 {
            assert!(buffer.write(CoverageEntry::line(i, 0)));
        }

        // Next write should fail (overflow)
        assert!(!buffer.write(CoverageEntry::line(100, 0)));
        assert_eq!(buffer.overflow_count(), 1);

        // Drain and try again
        let mut entries = Vec::new();
        buffer.drain(&mut entries, 4);
        assert!(buffer.write(CoverageEntry::line(200, 0)));
    }

    #[test]
    fn test_ring_buffer_wrap_around() {
        let buffer = CoverageRingBuffer::new(4).expect("Failed to create buffer");

        // Write and drain multiple times to test wrap-around
        for round in 0..3 {
            for i in 0..4 {
                let entry = CoverageEntry::line((round * 4 + i) as u64, 0);
                assert!(buffer.write(entry));
            }

            let mut entries = Vec::new();
            let count = buffer.drain(&mut entries, 4);
            assert_eq!(count, 4);

            for (i, entry) in entries.iter().enumerate() {
                assert_eq!(entry.code_id, (round * 4 + i) as u64);
            }
        }
    }

    // =========================================================================
    // Phase 6.1: Mapping Entry Tests
    // =========================================================================

    #[test]
    fn test_mapping_entry_size() {
        assert_eq!(std::mem::size_of::<MappingEntry>(), MAPPING_ENTRY_SIZE);
    }

    #[test]
    fn test_mapping_entry_alignment() {
        assert_eq!(std::mem::align_of::<MappingEntry>(), 8);
    }

    #[test]
    fn test_mapping_entry_short_filename() {
        let entry = MappingEntry::new(0x12345678, "/home/user/project/test.py");
        assert_eq!(entry.code_id, 0x12345678);
        assert_eq!(entry.filename(), "/home/user/project/test.py");
    }

    #[test]
    fn test_mapping_entry_exact_240_bytes() {
        // Create a filename that is exactly 240 bytes
        let filename = "a".repeat(240);
        let entry = MappingEntry::new(0xABCD, &filename);
        assert_eq!(entry.code_id, 0xABCD);
        assert_eq!(entry.filename_len, 240);
        assert_eq!(entry.filename(), filename);
    }

    #[test]
    fn test_mapping_entry_truncation_from_left() {
        // Create a filename longer than 240 bytes
        let prefix = "/very/long/path/that/will/be/truncated/";
        let suffix = "important_filename.py";
        let middle = "x".repeat(300 - prefix.len() - suffix.len());
        let long_filename = format!("{}{}{}", prefix, middle, suffix);

        assert!(long_filename.len() > 240);

        let entry = MappingEntry::new(0x9999, &long_filename);
        assert_eq!(entry.code_id, 0x9999);

        // The filename should be truncated from the LEFT
        // So the suffix (important_filename.py) should be preserved
        let result = entry.filename();
        assert!(result.len() <= 240);
        assert!(result.ends_with(suffix));
    }

    #[test]
    fn test_mapping_entry_utf8_boundary_handling() {
        // Create a filename with multi-byte UTF-8 characters
        // Each emoji is 4 bytes, so we need to test boundary handling
        let prefix = "🔥".repeat(60); // 240 bytes of emojis
        let suffix = "/test.py";
        let long_filename = format!("{}{}", prefix, suffix);

        let entry = MappingEntry::new(0x1111, &long_filename);

        // The result should be valid UTF-8
        let result = entry.filename();
        assert!(result.len() <= 240);
        // Verify it's valid UTF-8 (filename() uses from_utf8_lossy)
        assert!(!result.contains('\u{FFFD}')); // No replacement characters
    }

    #[test]
    fn test_mapping_entry_empty_filename() {
        let entry = MappingEntry::new(0x0, "");
        assert_eq!(entry.code_id, 0);
        assert_eq!(entry.filename_len, 0);
        assert_eq!(entry.filename(), "");
    }

    // =========================================================================
    // Phase 6.1: Mapping Ring Buffer Tests
    // =========================================================================

    #[test]
    fn test_mapping_buffer_creation() {
        let buffer = MappingRingBuffer::new(64).expect("Failed to create mapping buffer");
        assert_eq!(buffer.capacity, 64);
        assert!(buffer.fd >= 0);
        assert!(!buffer.ptr.is_null());
    }

    #[test]
    fn test_mapping_buffer_write_read() {
        let buffer = MappingRingBuffer::new(16).expect("Failed to create mapping buffer");

        // Write some entries
        for i in 0..10 {
            let filename = format!("/path/to/file_{}.py", i);
            let entry = MappingEntry::new(0x1000 + i, &filename);
            assert!(buffer.write(entry));
        }

        // Read them back
        let mut entries = Vec::new();
        let count = buffer.drain(&mut entries, 100);
        assert_eq!(count, 10);
        assert_eq!(entries.len(), 10);

        for (i, entry) in entries.iter().enumerate() {
            assert_eq!(entry.code_id, 0x1000 + i as u64);
            let expected = format!("/path/to/file_{}.py", i);
            assert_eq!(entry.filename(), expected);
        }
    }

    #[test]
    fn test_mapping_buffer_overflow() {
        let buffer = MappingRingBuffer::new(4).expect("Failed to create mapping buffer");

        // Fill the buffer
        for i in 0..4 {
            let entry = MappingEntry::new(i, &format!("file_{}.py", i));
            assert!(buffer.write(entry));
        }

        // Next write should fail (overflow)
        let overflow_entry = MappingEntry::new(100, "overflow.py");
        assert!(!buffer.write(overflow_entry));
        assert_eq!(buffer.overflow_count(), 1);

        // Drain and try again
        let mut entries = Vec::new();
        buffer.drain(&mut entries, 4);
        let new_entry = MappingEntry::new(200, "new.py");
        assert!(buffer.write(new_entry));
    }

    #[test]
    fn test_mapping_buffer_wrap_around() {
        let buffer = MappingRingBuffer::new(4).expect("Failed to create mapping buffer");

        // Write and drain multiple times to test wrap-around
        for round in 0..3 {
            for i in 0..4 {
                let code_id = (round * 4 + i) as u64;
                let entry = MappingEntry::new(code_id, &format!("file_{}.py", code_id));
                assert!(buffer.write(entry));
            }

            let mut entries = Vec::new();
            let count = buffer.drain(&mut entries, 4);
            assert_eq!(count, 4);

            for (i, entry) in entries.iter().enumerate() {
                let expected_id = (round * 4 + i) as u64;
                assert_eq!(entry.code_id, expected_id);
            }
        }
    }

    // =========================================================================
    // Phase 6.1: Thread-Local SEEN_CODES Tests
    // =========================================================================

    #[test]
    fn test_mark_code_seen_first_time() {
        // First time seeing a code_id should return true
        let code_id = 0xDEADBEEF_u64;
        assert!(mark_code_seen(code_id));
    }

    #[test]
    fn test_mark_code_seen_second_time() {
        // Use a unique code_id for this test
        let code_id = 0xCAFEBABE_u64;

        // First time should return true
        assert!(mark_code_seen(code_id));

        // Second time should return false
        assert!(!mark_code_seen(code_id));

        // Third time should also return false
        assert!(!mark_code_seen(code_id));
    }

    #[test]
    fn test_mark_code_seen_multiple_codes() {
        // Each unique code_id should return true on first encounter
        let codes = [0x1111_u64, 0x2222_u64, 0x3333_u64, 0x4444_u64];

        for &code_id in &codes {
            assert!(mark_code_seen(code_id));
        }

        // Second encounter should return false for all
        for &code_id in &codes {
            assert!(!mark_code_seen(code_id));
        }
    }

    // =========================================================================
    // Phase 1.3: CAS Loop Regression Tests (TOCTOU Race Prevention)
    // =========================================================================

    #[test]
    fn test_coverage_ring_buffer_concurrent_writes() {
        use std::sync::Arc;
        use std::thread;

        // Create a buffer with enough capacity for concurrent writes
        let buffer = Arc::new(CoverageRingBuffer::new(1024).expect("Failed to create buffer"));
        let num_threads = 8;
        let writes_per_thread = 100;

        let handles: Vec<_> = (0..num_threads)
            .map(|thread_id| {
                let buffer = Arc::clone(&buffer);
                thread::spawn(move || {
                    let mut success_count = 0;
                    for i in 0..writes_per_thread {
                        let code_id = (thread_id * 1000 + i) as u64;
                        let entry = CoverageEntry::line(code_id, i as u32);
                        if buffer.write(entry) {
                            success_count += 1;
                        }
                    }
                    success_count
                })
            })
            .collect();

        // Wait for all threads and sum successful writes
        let total_writes: usize = handles.into_iter().map(|h| h.join().unwrap()).sum();

        // All writes should succeed (buffer has capacity 1024, we write 800)
        assert_eq!(total_writes, num_threads * writes_per_thread);

        // Verify we can drain all entries
        let mut entries = Vec::new();
        let drained = buffer.drain(&mut entries, 2000);
        assert_eq!(drained, total_writes);

        // Verify no overflow occurred
        assert_eq!(buffer.overflow_count(), 0);
    }

    #[test]
    fn test_coverage_ring_buffer_concurrent_overflow() {
        use std::sync::Arc;
        use std::thread;

        // Small buffer to force overflow contention
        let buffer = Arc::new(CoverageRingBuffer::new(16).expect("Failed to create buffer"));
        let num_threads = 4;
        let writes_per_thread = 100;

        let handles: Vec<_> = (0..num_threads)
            .map(|thread_id| {
                let buffer = Arc::clone(&buffer);
                thread::spawn(move || {
                    let mut success_count = 0;
                    for i in 0..writes_per_thread {
                        let code_id = (thread_id * 1000 + i) as u64;
                        let entry = CoverageEntry::line(code_id, i as u32);
                        if buffer.write(entry) {
                            success_count += 1;
                        }
                    }
                    success_count
                })
            })
            .collect();

        // Wait for all threads
        let total_success: usize = handles.into_iter().map(|h| h.join().unwrap()).sum();

        // Buffer capacity is 16, so most writes should overflow
        // The key invariant: success_count + overflow_count >= total_attempts
        // (some overflows may be counted multiple times due to CAS retry)
        assert!(total_success <= 16);
        assert!(buffer.overflow_count() > 0);

        // Drain and verify we got exactly what succeeded
        let mut entries = Vec::new();
        let drained = buffer.drain(&mut entries, 100);
        assert_eq!(drained, total_success);
    }

    #[test]
    fn test_mapping_ring_buffer_concurrent_writes() {
        use std::sync::Arc;
        use std::thread;

        // Create a buffer with enough capacity for concurrent writes
        let buffer = Arc::new(MappingRingBuffer::new(256).expect("Failed to create buffer"));
        let num_threads = 4;
        let writes_per_thread = 50;

        let handles: Vec<_> = (0..num_threads)
            .map(|thread_id| {
                let buffer = Arc::clone(&buffer);
                thread::spawn(move || {
                    let mut success_count = 0;
                    for i in 0..writes_per_thread {
                        let code_id = (thread_id * 1000 + i) as u64;
                        let filename = format!("/path/thread_{}/file_{}.py", thread_id, i);
                        let entry = MappingEntry::new(code_id, &filename);
                        if buffer.write(entry) {
                            success_count += 1;
                        }
                    }
                    success_count
                })
            })
            .collect();

        // Wait for all threads and sum successful writes
        let total_writes: usize = handles.into_iter().map(|h| h.join().unwrap()).sum();

        // All writes should succeed (buffer has capacity 256, we write 200)
        assert_eq!(total_writes, num_threads * writes_per_thread);

        // Verify we can drain all entries
        let mut entries = Vec::new();
        let drained = buffer.drain(&mut entries, 500);
        assert_eq!(drained, total_writes);

        // Verify no overflow occurred
        assert_eq!(buffer.overflow_count(), 0);
    }

    #[test]
    fn test_ring_buffer_cas_loop_stress() {
        use std::sync::Arc;
        use std::thread;

        // Stress test with many threads competing for few slots
        let buffer = Arc::new(CoverageRingBuffer::new(8).expect("Failed to create buffer"));
        let num_threads = 16;
        let writes_per_thread = 50;

        let handles: Vec<_> = (0..num_threads)
            .map(|thread_id| {
                let buffer = Arc::clone(&buffer);
                thread::spawn(move || {
                    for i in 0..writes_per_thread {
                        let code_id = (thread_id * 1000 + i) as u64;
                        let entry = CoverageEntry::line(code_id, i as u32);
                        // We don't care about success/failure, just that it doesn't crash
                        let _ = buffer.write(entry);
                    }
                })
            })
            .collect();

        // All threads should complete without panic
        for handle in handles {
            handle.join().expect("Thread should not panic");
        }

        // Buffer should be in a consistent state
        let header = buffer.header();
        let write_idx = header.write_idx.load(Ordering::Acquire);
        let read_idx = header.read_idx.load(Ordering::Acquire);

        // write_idx should never be less than read_idx
        assert!(write_idx >= read_idx);

        // Available entries should be <= capacity
        let available = write_idx.wrapping_sub(read_idx);
        assert!(available <= buffer.capacity as u64);
    }
}
