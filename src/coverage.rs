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
    #[inline]
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
    /// Uses lock-free atomic operations.
    ///
    /// # Returns
    /// * `true` if entry was written
    /// * `false` if buffer was full (entry dropped, overflow counter incremented)
    #[inline]
    pub fn write(&self, entry: CoverageEntry) -> bool {
        let header = self.header();

        // Check if buffer is full
        if header.is_full() {
            header.overflow_count.fetch_add(1, Ordering::Relaxed);
            return false;
        }

        // Reserve a slot atomically
        let idx = header.write_idx.fetch_add(1, Ordering::AcqRel);
        let slot = (idx % self.capacity as u64) as usize;

        // Write the entry
        unsafe {
            let entry_ptr = self.entries_ptr().add(slot);
            std::ptr::write_volatile(entry_ptr, entry);
        }

        true
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

    Ok(RING_BUFFER.get().unwrap())
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
    code_map: Arc<std::sync::Mutex<HashMap<u64, String>>>,
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
            code_map: Arc::new(std::sync::Mutex::new(HashMap::new())),
            stop_flag: Arc::new(AtomicBool::new(false)),
            thread_handle: None,
        }
    }

    /// Start the aggregator thread.
    ///
    /// The thread polls the ring buffer every `poll_interval` and drains
    /// entries into the accumulated coverage data.
    pub fn start(&mut self, poll_interval: Duration) {
        let data = Arc::clone(&self.data);
        let code_map = Arc::clone(&self.code_map);
        let stop_flag = Arc::clone(&self.stop_flag);

        let handle = thread::spawn(move || {
            let mut batch = Vec::with_capacity(1024);

            while !stop_flag.load(Ordering::Relaxed) {
                // Drain entries from ring buffer
                if let Some(buffer) = get_coverage_buffer() {
                    let count = buffer.drain(&mut batch, 4096);

                    if count > 0 {
                        // Process batch
                        let mut data_guard = data.lock().unwrap();
                        let code_map_guard = code_map.lock().unwrap();

                        for entry in batch.drain(..) {
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
            if let Some(buffer) = get_coverage_buffer() {
                let mut batch = Vec::new();
                buffer.drain(&mut batch, usize::MAX);

                if !batch.is_empty() {
                    let mut data_guard = data.lock().unwrap();
                    let code_map_guard = code_map.lock().unwrap();

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
        let mut code_map = self.code_map.lock().unwrap();
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
    pub fn get_data(&self) -> CoverageData {
        self.data.lock().unwrap().clone()
    }

    /// Get the number of unique (file, line) pairs covered.
    pub fn covered_lines(&self) -> usize {
        self.data.lock().unwrap().len()
    }

    /// Get the total number of line executions recorded.
    pub fn total_hits(&self) -> u64 {
        self.data.lock().unwrap().values().sum()
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
    py.allow_threads(|| {
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
}
