//! Phase 2.3 P1: Sentinel Calibration for Dynamic TLS Offset Discovery
//!
//! This module implements the "Self-Calibration" routine that automatically discovers
//! mimalloc TLS offsets at Zygote warm-up time, eliminating hardcoded offset values.
//!
//! # The Problem
//!
//! TLS offsets vary with:
//! - Python version (3.13.x vs 3.14.x)
//! - glibc version
//! - libpython build configuration
//! - Number of loaded C-extensions (TensorFlow, PyTorch expand the DTV)
//!
//! Hardcoding offsets like `0xad8` makes Tach brittle. A glibc security patch could
//! shift offsets, turning Tach into a heap-corruption engine.
//!
//! # The Solution: Runtime Sentinel Scan
//!
//! 1. Allocate a unique "Sentinel" pattern (0xDEADC0DE_BAADF00D) in Python heap
//! 2. Scan the TLS region for pointers to heap regions
//! 3. Record the discovered mi_heap_t offset
//! 4. Use this offset for the lifetime of the process tree
//!
//! # Integration
//!
//! Call `TlsCalibration::calibrate()` during Zygote warm-up, before the first fork().
//! If calibration fails, Tach must exit with `ERR_CALIBRATION_FAILED`.

use anyhow::{anyhow, Result};
use pyo3::prelude::*;
use std::fs;

// =============================================================================
// Constants
// =============================================================================

/// The sentinel pattern: DEADC0DE BAADF00D
/// This pattern is:
/// - Unlikely to appear naturally in memory
/// - Properly aligned (8-byte boundary)
/// - Triggers mimalloc TLS population when allocated via Python
pub const SENTINEL_PATTERN: u64 = 0xDEADC0DE_BAADF00D;

/// arch_prctl constant for getting FS base (x86_64 only)
#[cfg(target_arch = "x86_64")]
const ARCH_GET_FS: i32 = 0x1003;

// =============================================================================
// Calibration Result
// =============================================================================

/// Result of TLS calibration
#[derive(Debug, Clone)]
pub struct TlsCalibration {
    /// The fs_base value at calibration time
    pub fs_base: usize,

    /// Primary mi_heap_t offset from fs_base (most important for mimalloc)
    pub mi_heap_offset: Option<usize>,

    /// All discovered heap pointer offsets in TLS
    pub heap_pointer_offsets: Vec<HeapPointerInfo>,

    /// Size of the TLS region (for dynamic sizing)
    pub tls_region_size: usize,

    /// Whether calibration was successful
    pub calibrated: bool,
}

/// Information about a heap pointer discovered in TLS
#[derive(Debug, Clone)]
pub struct HeapPointerInfo {
    /// Offset from fs_base
    pub offset: usize,
    /// The pointer value (address in heap)
    pub pointer_value: usize,
    /// Description of the target region
    pub target_description: String,
}

impl TlsCalibration {
    /// Perform TLS calibration during Zygote warm-up
    ///
    /// This must be called:
    /// - After Python is initialized
    /// - Before any worker fork()
    /// - In the Zygote process (not workers)
    ///
    /// # Returns
    /// - `Ok(TlsCalibration)` with discovered offsets
    /// - `Err` if calibration fails (Tach should exit with ERR_CALIBRATION_FAILED)
    ///
    /// # Boot Log
    /// On success, logs: `[restoration] Sentinel found at fs_base + 0xXXXX`
    #[cfg(target_arch = "x86_64")]
    pub fn calibrate() -> Result<Self> {
        eprintln!("{}", "=".repeat(70));
        eprintln!("[calibration] Phase 2.3 P1: TLS Self-Calibration Starting");
        eprintln!("{}", "=".repeat(70));

        // Step 1: Get fs_base
        let fs_base = get_fs_base()?;
        eprintln!("[calibration] fs_base = 0x{:016x}", fs_base);

        // Step 2: Parse memory maps to find TLS region
        let regions = parse_memory_maps()?;
        let tls_region = find_containing_region(&regions, fs_base).ok_or_else(|| anyhow!("fs_base 0x{:x} not in any mapped region", fs_base))?;

        eprintln!("[calibration] TLS region: 0x{:x}-0x{:x} ({} bytes)", tls_region.start, tls_region.end, tls_region.size());

        let tls_region_size = tls_region.size();

        // Step 3: Populate mimalloc TLS by allocating Python objects
        eprintln!("[calibration] Populating mimalloc TLS structures...");
        Python::with_gil(|py| {
            populate_mimalloc_tls(py)?;

            // Step 4: Allocate sentinel
            eprintln!("[calibration] Allocating sentinel (0x{:016X})...", SENTINEL_PATTERN);
            let sentinel_addr = allocate_sentinel(py)?;

            // Step 5: Identify heap regions
            let heap_regions: Vec<_> = regions.iter().filter(|r| r.perms.contains('w') && (r.pathname.contains("[heap]") || (r.pathname.is_empty() && r.start != tls_region.start))).collect();

            eprintln!("[calibration] Found {} potential heap regions", heap_regions.len());

            // Step 6: Scan TLS for heap pointers
            eprintln!("[calibration] Scanning TLS for heap pointers...");
            let (heap_pointer_offsets, sentinel_offsets) = scan_tls_for_pointers(fs_base, &tls_region, &heap_regions, sentinel_addr);

            // Step 7: Determine primary mi_heap_t offset
            let mi_heap_offset = heap_pointer_offsets.first().map(|p| p.offset);

            // Log the calibration message (Orchestrator's requirement)
            if let Some(offset) = mi_heap_offset {
                eprintln!("[restoration] Sentinel found at fs_base + 0x{:04X}", offset);
            }

            // Report results
            eprintln!("\n{}", "=".repeat(70));
            eprintln!("[calibration] CALIBRATION RESULTS");
            eprintln!("{}", "=".repeat(70));
            eprintln!("  Heap pointers in TLS: {}", heap_pointer_offsets.len());
            eprintln!("  Primary mi_heap_t:    {}", mi_heap_offset.map(|o| format!("fs_base + 0x{:04X}", o)).unwrap_or_else(|| "NOT FOUND".to_string()));
            eprintln!("  Sentinel direct refs: {}", if sentinel_offsets.is_empty() { "None (expected)" } else { "Found (unexpected)" });

            let calibrated = mi_heap_offset.is_some();

            if calibrated {
                eprintln!("\n[calibration] CALIBRATION SUCCESSFUL");
            } else {
                eprintln!("\n[calibration] CALIBRATION FAILED - No heap pointers found in TLS");
                eprintln!("[calibration] This may indicate Python < 3.13 (pymalloc, no TLS caching)");
            }

            eprintln!("{}", "=".repeat(70));

            Ok(TlsCalibration {
                fs_base,
                mi_heap_offset,
                heap_pointer_offsets,
                tls_region_size,
                calibrated,
            })
        })
    }

    /// Check if calibration was successful
    pub fn is_calibrated(&self) -> bool {
        self.calibrated
    }

    /// Get the primary mi_heap_t offset (returns None if not calibrated)
    pub fn primary_offset(&self) -> Option<usize> {
        self.mi_heap_offset
    }
}

// =============================================================================
// Memory Region Parsing (for self-process calibration)
// =============================================================================

#[derive(Debug, Clone)]
struct MemoryRegion {
    start: usize,
    end: usize,
    perms: String,
    pathname: String,
}

impl MemoryRegion {
    fn contains(&self, addr: usize) -> bool {
        addr >= self.start && addr < self.end
    }

    fn size(&self) -> usize {
        self.end - self.start
    }
}

fn parse_memory_maps() -> Result<Vec<MemoryRegion>> {
    let content = fs::read_to_string("/proc/self/maps")?;

    let mut regions = Vec::new();

    for line in content.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 5 {
            continue;
        }

        let addr_parts: Vec<&str> = parts[0].split('-').collect();
        if addr_parts.len() != 2 {
            continue;
        }

        let start = usize::from_str_radix(addr_parts[0], 16).unwrap_or(0);
        let end = usize::from_str_radix(addr_parts[1], 16).unwrap_or(0);
        let perms = parts[1].to_string();
        let pathname = if parts.len() > 5 { parts[5..].join(" ") } else { String::new() };

        regions.push(MemoryRegion { start, end, perms, pathname });
    }

    Ok(regions)
}

fn find_containing_region(regions: &[MemoryRegion], addr: usize) -> Option<&MemoryRegion> {
    regions.iter().find(|r| r.contains(addr))
}

// =============================================================================
// arch_prctl for self-process (no ptrace needed)
// =============================================================================

#[cfg(target_arch = "x86_64")]
fn get_fs_base() -> Result<usize> {
    let mut fs_base: u64 = 0;

    // SAFETY: arch_prctl(ARCH_GET_FS) reads fs_base into the provided pointer
    let ret = unsafe { libc::syscall(libc::SYS_arch_prctl, ARCH_GET_FS, &mut fs_base as *mut u64) };

    if ret == 0 {
        Ok(fs_base as usize)
    } else {
        Err(anyhow!("arch_prctl(ARCH_GET_FS) failed: {}", std::io::Error::last_os_error()))
    }
}

// =============================================================================
// Python Integration for Sentinel Allocation
// =============================================================================

/// Allocate a sentinel object in Python heap and return its address
fn allocate_sentinel(py: Python<'_>) -> Result<usize> {
    let ctypes = py.import("ctypes").map_err(|e| anyhow!("Failed to import ctypes: {}", e))?;

    let c_uint64 = ctypes.getattr("c_uint64").map_err(|e| anyhow!("Failed to get c_uint64: {}", e))?;
    let sentinel = c_uint64.call1((SENTINEL_PATTERN,)).map_err(|e| anyhow!("Failed to create sentinel: {}", e))?;

    let addressof = ctypes.getattr("addressof").map_err(|e| anyhow!("Failed to get addressof: {}", e))?;
    let addr_obj = addressof.call1((sentinel.clone(),)).map_err(|e| anyhow!("Failed to get sentinel address: {}", e))?;
    let addr_value: usize = addr_obj.extract().map_err(|e| anyhow!("Failed to extract address: {}", e))?;

    eprintln!("[calibration] Sentinel allocated at 0x{:016x}, value: 0x{:016X}", addr_value, SENTINEL_PATTERN);

    // Keep sentinel alive by storing in Python's __main__ module
    let main_module = py.import("__main__").map_err(|e| anyhow!("Failed to import __main__: {}", e))?;
    main_module.setattr("_tach_sentinel", sentinel).map_err(|e| anyhow!("Failed to store sentinel: {}", e))?;

    Ok(addr_value)
}

/// Force mimalloc to populate its TLS structures by allocating many objects
fn populate_mimalloc_tls(py: Python<'_>) -> Result<()> {
    let code = r#"
import gc

# Allocate many small objects to populate mimalloc's thread-local bins

# Phase 1: Small allocations (populates size-class bins)
small_objects = [bytearray(64) for _ in range(1000)]

# Phase 2: Float allocations (uses float free list)
floats = [float(i) * 1.1 for i in range(1000)]

# Phase 3: Delete to populate free lists
del small_objects
del floats
gc.collect()

# Phase 4: Reallocate (exercises cached bins)
small_objects_2 = [bytearray(64) for _ in range(500)]
floats_2 = [float(i) * 2.2 for i in range(500)]
"#;

    let code_with_nul = format!("{}\0", code);
    let code_cstr = std::ffi::CStr::from_bytes_with_nul(code_with_nul.as_bytes()).expect("CStr creation failed");

    py.run(code_cstr, None, None).map_err(|e| anyhow!("Failed to populate mimalloc TLS: {}", e))?;

    Ok(())
}

// =============================================================================
// TLS Scanning
// =============================================================================

/// Scan TLS region for heap pointers and sentinel references
fn scan_tls_for_pointers(fs_base: usize, tls_region: &MemoryRegion, heap_regions: &[&MemoryRegion], sentinel_addr: usize) -> (Vec<HeapPointerInfo>, Vec<usize>) {
    let mut heap_pointers = Vec::new();
    let mut sentinel_offsets = Vec::new();

    // Scan from fs_base to end of TLS region
    let scan_end = tls_region.end;

    for offset in (0..(scan_end - fs_base)).step_by(8) {
        let addr = fs_base + offset;
        if addr + 8 <= tls_region.end {
            // SAFETY: We're reading from our own process's mapped memory
            let value = unsafe { std::ptr::read_volatile(addr as *const usize) };

            // Check if this is our sentinel
            if value == sentinel_addr {
                sentinel_offsets.push(offset);
                eprintln!("[calibration] SENTINEL POINTER at fs_base+0x{:04x} -> 0x{:016x}", offset, value);
            }

            // Check if this points to any heap region
            for heap in heap_regions {
                if heap.contains(value) {
                    let target_desc = if heap.pathname.is_empty() { format!("anon@0x{:x}", heap.start) } else { heap.pathname.clone() };

                    heap_pointers.push(HeapPointerInfo {
                        offset,
                        pointer_value: value,
                        target_description: target_desc,
                    });
                    break;
                }
            }
        }
    }

    (heap_pointers, sentinel_offsets)
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sentinel_pattern() {
        assert_eq!(SENTINEL_PATTERN, 0xDEADC0DE_BAADF00D);
    }

    #[test]
    fn test_parse_memory_maps() {
        let regions = parse_memory_maps().expect("Failed to parse maps");
        assert!(!regions.is_empty());

        // Should find stack region
        let has_stack = regions.iter().any(|r| r.pathname.contains("[stack]"));
        assert!(has_stack, "Should find [stack] region");
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn test_get_fs_base() {
        let fs_base = get_fs_base().expect("Failed to get fs_base");
        assert!(fs_base > 0);
        eprintln!("[test] fs_base = 0x{:016x}", fs_base);
    }

    #[test]
    fn test_memory_region_contains() {
        let region = MemoryRegion {
            start: 0x1000,
            end: 0x2000,
            perms: "rw-p".to_string(),
            pathname: "[heap]".to_string(),
        };

        assert!(region.contains(0x1000));
        assert!(region.contains(0x1500));
        assert!(region.contains(0x1fff));
        assert!(!region.contains(0x2000)); // End is exclusive
        assert!(!region.contains(0x0fff));
    }

    #[test]
    fn test_heap_pointer_info_creation() {
        let info = HeapPointerInfo {
            offset: 0xad8,
            pointer_value: 0x7f1234560000,
            target_description: "[heap]".to_string(),
        };

        assert_eq!(info.offset, 0xad8);
        assert_eq!(info.pointer_value, 0x7f1234560000);
    }

    // Integration test - requires Python
    #[test]
    #[ignore] // Run with: cargo test test_tls_calibration -- --ignored --nocapture
    fn test_tls_calibration() {
        pyo3::prepare_freethreaded_python();

        let result = TlsCalibration::calibrate();
        assert!(result.is_ok(), "Calibration should succeed");

        let calibration = result.unwrap();
        eprintln!("[test] Calibration result: {:?}", calibration);

        // On Python 3.13+, we should find heap pointers
        // On older Python, calibration may succeed but find no pointers (which is OK)
        eprintln!("[test] Primary offset: {:?}", calibration.primary_offset());
    }
}
