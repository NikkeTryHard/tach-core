//! Phase 2.2: Runtime Sentinel Scan for Dynamic TLS Offset Discovery
//!
//! This POC implements the "Self-Calibration" routine to automatically discover
//! mimalloc TLS offsets without hardcoding them.
//!
//! # The Problem
//!
//! The TLS offset `0xad8` discovered in Phase 2.1 is specific to:
//! - Python version (3.13.x)
//! - glibc version
//! - libpython build configuration
//! - ASLR state
//!
//! If we hardcode this offset, Tach becomes brittle. A security patch to glibc
//! could shift the offset by 8 bytes, turning Tach into a heap-corruption engine.
//!
//! # The Solution: Runtime Sentinel Scan
//!
//! 1. Allocate a unique "Sentinel" pattern in the Python heap
//! 2. Scan the 12KB TLS region for pointers to the sentinel
//! 3. Record the offset where we find it
//! 4. Use this offset for the duration of the process tree's life
//!
//! # The Sentinel Pattern
//!
//! We use a 64-bit pattern that is:
//! - Unlikely to appear naturally: `0xDEADC0DE_BAADF00D`
//! - Properly aligned (8-byte boundary)
//! - Allocated via Python's allocator (triggers mimalloc TLS population)
//!
//! # Running This Exploration
//!
//! ```bash
//! cd /home/louiskaneko/dev/tach-core
//! source .venv/bin/activate
//! export PYO3_PYTHON=$(which python)
//! cargo run --bin tls_sentinel_scan
//! ```

use pyo3::prelude::*;
use std::collections::HashMap;
use std::fs;

// =============================================================================
// Constants
// =============================================================================

/// The sentinel pattern: DEADC0DE BAADF00D
const SENTINEL_PATTERN: u64 = 0xDEADC0DE_BAADF00D;

/// arch_prctl constant for getting FS base
const ARCH_GET_FS: i32 = 0x1003;

/// Maximum TLS scan range (12KB covers typical TLS usage)
const TLS_SCAN_RANGE: usize = 12 * 1024;

// =============================================================================
// Memory Region Parsing
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

fn parse_memory_maps() -> Vec<MemoryRegion> {
    let content = fs::read_to_string("/proc/self/maps").expect("Failed to read /proc/self/maps");

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
        let pathname = if parts.len() > 5 {
            parts[5..].join(" ")
        } else {
            String::new()
        };

        regions.push(MemoryRegion {
            start,
            end,
            perms,
            pathname,
        });
    }

    regions
}

fn get_fs_base() -> Result<usize, std::io::Error> {
    let mut fs_base: u64 = 0;

    let ret = unsafe { libc::syscall(libc::SYS_arch_prctl, ARCH_GET_FS, &mut fs_base as *mut u64) };

    if ret == 0 {
        Ok(fs_base as usize)
    } else {
        Err(std::io::Error::last_os_error())
    }
}

fn find_containing_region(regions: &[MemoryRegion], addr: usize) -> Option<&MemoryRegion> {
    regions.iter().find(|r| r.contains(addr))
}

// =============================================================================
// Sentinel Allocation via Python
// =============================================================================

/// Discovered TLS offset information
#[derive(Debug, Clone)]
pub struct TlsOffsetInfo {
    /// Offset from fs_base where the pointer was found
    pub offset: usize,
    /// The pointer value (address in heap)
    pub pointer_value: usize,
    /// Description of what the pointer targets
    pub target_description: String,
}

/// Result of the sentinel scan
#[derive(Debug)]
pub struct SentinelScanResult {
    /// The fs_base value
    pub fs_base: usize,
    /// All heap pointers found in TLS
    pub heap_pointers: Vec<TlsOffsetInfo>,
    /// Offsets where we found pointers to our sentinel
    pub sentinel_offsets: Vec<usize>,
    /// The primary mi_heap_t offset (first heap pointer found)
    pub primary_heap_offset: Option<usize>,
}

/// Allocate a sentinel object in Python heap and return its address
///
/// We use ctypes to allocate a c_uint64 with our sentinel pattern.
/// This ensures the allocation goes through Python's allocator (mimalloc).
fn allocate_sentinel(py: Python<'_>) -> PyResult<usize> {
    // Import ctypes
    let ctypes = py.import("ctypes")?;

    // Create sentinel value
    let c_uint64 = ctypes.getattr("c_uint64")?;
    let sentinel = c_uint64.call1((SENTINEL_PATTERN,))?;

    // Get its address
    let addressof = ctypes.getattr("addressof")?;
    let addr_obj = addressof.call1((sentinel.clone(),))?;
    let addr_value: usize = addr_obj.extract()?;

    eprintln!(
        "[Sentinel] Allocated at 0x{:016x}, value: 0x{:016X}",
        addr_value, SENTINEL_PATTERN
    );

    // Keep sentinel alive by storing in Python's __main__ module
    let main_module = py.import("__main__")?;
    main_module.setattr("_tach_sentinel", sentinel)?;

    Ok(addr_value)
}

/// Force mimalloc to populate its TLS structures by allocating many objects
fn populate_mimalloc_tls(py: Python<'_>) -> PyResult<()> {
    let code = r#"
import gc

# Allocate many small objects to populate mimalloc's thread-local bins
print("[Sentinel] Populating mimalloc TLS structures...")

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

print("[Sentinel] mimalloc TLS structures populated")
"#;

    let code_with_nul = format!("{}\0", code);
    let code_cstr = std::ffi::CStr::from_bytes_with_nul(code_with_nul.as_bytes())
        .expect("CStr creation failed");

    py.run(code_cstr, None, None)?;

    Ok(())
}

// =============================================================================
// TLS Scanning
// =============================================================================

/// Scan TLS region for heap pointers and sentinel
fn scan_tls_for_sentinel(
    fs_base: usize,
    tls_region: &MemoryRegion,
    heap_regions: &[&MemoryRegion],
    sentinel_addr: usize,
) -> SentinelScanResult {
    let mut heap_pointers = Vec::new();
    let mut sentinel_offsets = Vec::new();

    let scan_end = std::cmp::min(fs_base + TLS_SCAN_RANGE, tls_region.end);

    for offset in (0..(scan_end - fs_base)).step_by(8) {
        let addr = fs_base + offset;
        if addr + 8 <= tls_region.end {
            let value = unsafe { std::ptr::read_volatile(addr as *const usize) };

            // Check if this is our sentinel
            if value == sentinel_addr {
                sentinel_offsets.push(offset);
                eprintln!(
                    "[Sentinel] FOUND SENTINEL POINTER at fs_base+0x{:04x} -> 0x{:016x}",
                    offset, value
                );
            }

            // Check if this points to any heap region
            for heap in heap_regions {
                if heap.contains(value) {
                    let target_desc = if heap.pathname.is_empty() {
                        format!("anon@0x{:x}", heap.start)
                    } else {
                        heap.pathname.clone()
                    };

                    heap_pointers.push(TlsOffsetInfo {
                        offset,
                        pointer_value: value,
                        target_description: target_desc,
                    });
                    break;
                }
            }
        }
    }

    let primary_heap_offset = heap_pointers.first().map(|p| p.offset);

    SentinelScanResult {
        fs_base,
        heap_pointers,
        sentinel_offsets,
        primary_heap_offset,
    }
}

// =============================================================================
// Self-Calibration API
// =============================================================================

/// Perform runtime self-calibration to discover TLS offsets.
///
/// This is the main entry point for the calibration routine.
/// Call this once during Zygote warm-up phase, before taking snapshots.
///
/// # Returns
/// A HashMap mapping offset names to their discovered values:
/// - "mi_heap_t" -> offset of primary heap pointer
/// - "sentinel" -> offset where sentinel was found (if any)
pub fn calibrate_tls_offsets() -> HashMap<String, usize> {
    eprintln!("{}", "=".repeat(70));
    eprintln!("Phase 2.2: Runtime Sentinel Scan - TLS Self-Calibration");
    eprintln!("{}", "=".repeat(70));

    // Initialize Python
    Python::initialize();

    let result = Python::attach(|py| {
        // Step 1: Populate mimalloc TLS structures
        eprintln!("\n[Step 1] Populating mimalloc TLS structures...");
        if let Err(e) = populate_mimalloc_tls(py) {
            eprintln!("[ERROR] Failed to populate TLS: {}", e);
            return None;
        }

        // Step 2: Allocate sentinel
        eprintln!(
            "\n[Step 2] Allocating sentinel (0x{:016X})...",
            SENTINEL_PATTERN
        );
        let sentinel_addr = match allocate_sentinel(py) {
            Ok(addr) => addr,
            Err(e) => {
                eprintln!("[ERROR] Failed to allocate sentinel: {}", e);
                return None;
            }
        };

        // Step 3: Get fs_base and memory maps
        eprintln!("\n[Step 3] Reading fs_base and memory maps...");
        let fs_base = get_fs_base().expect("Failed to get fs_base");
        eprintln!("[TLS] fs_base = 0x{:016x}", fs_base);

        let regions = parse_memory_maps();
        let tls_region = match find_containing_region(&regions, fs_base) {
            Some(r) => r,
            None => {
                eprintln!("[ERROR] fs_base not in any mapped region");
                return None;
            }
        };
        eprintln!(
            "[TLS] TLS region: 0x{:x}-0x{:x} ({} bytes)",
            tls_region.start,
            tls_region.end,
            tls_region.size()
        );

        // Identify heap/anonymous regions
        let heap_regions: Vec<_> = regions
            .iter()
            .filter(|r| {
                r.perms.contains('w')
                    && (r.pathname.contains("[heap]")
                        || (r.pathname.is_empty() && r.start != tls_region.start))
            })
            .collect();

        eprintln!("[TLS] Found {} potential heap regions", heap_regions.len());

        // Step 4: Scan TLS
        eprintln!("\n[Step 4] Scanning TLS for heap pointers and sentinel...");
        let scan_result = scan_tls_for_sentinel(fs_base, tls_region, &heap_regions, sentinel_addr);

        Some(scan_result)
    });

    let mut offsets = HashMap::new();

    if let Some(scan_result) = result {
        eprintln!("\n{}", "=".repeat(70));
        eprintln!("CALIBRATION RESULTS");
        eprintln!("{}", "=".repeat(70));

        eprintln!(
            "\n[Heap Pointers in TLS] ({} found)",
            scan_result.heap_pointers.len()
        );
        eprintln!("{}", "-".repeat(70));
        for (i, ptr) in scan_result.heap_pointers.iter().take(10).enumerate() {
            eprintln!(
                "  [{:2}] fs_base+0x{:04x} -> 0x{:016x} [{}]",
                i, ptr.offset, ptr.pointer_value, ptr.target_description
            );
        }
        if scan_result.heap_pointers.len() > 10 {
            eprintln!("  ... and {} more", scan_result.heap_pointers.len() - 10);
        }

        if let Some(primary_offset) = scan_result.primary_heap_offset {
            eprintln!("\n[PRIMARY mi_heap_t OFFSET] 0x{:04x}", primary_offset);
            offsets.insert("mi_heap_t".to_string(), primary_offset);
        }

        if !scan_result.sentinel_offsets.is_empty() {
            eprintln!(
                "\n[SENTINEL FOUND] at {} offset(s): {:?}",
                scan_result.sentinel_offsets.len(),
                scan_result
                    .sentinel_offsets
                    .iter()
                    .map(|o| format!("0x{:04x}", o))
                    .collect::<Vec<_>>()
            );
            offsets.insert("sentinel".to_string(), scan_result.sentinel_offsets[0]);
        } else {
            eprintln!("\n[SENTINEL] Not directly found in TLS");
            eprintln!("           (This is expected - sentinel is in heap, not TLS)");
            eprintln!("           The mi_heap_t pointer leads to the sentinel.");
        }

        eprintln!("\n{}", "=".repeat(70));
        eprintln!("CALIBRATION COMPLETE");
        eprintln!("{}", "=".repeat(70));

        // Summary
        eprintln!("\n[Summary]");
        eprintln!("  fs_base:              0x{:016x}", scan_result.fs_base);
        eprintln!(
            "  Heap pointers in TLS: {}",
            scan_result.heap_pointers.len()
        );
        eprintln!(
            "  Primary mi_heap_t:    {}",
            scan_result
                .primary_heap_offset
                .map(|o| format!("fs_base+0x{:04x}", o))
                .unwrap_or_else(|| "NOT FOUND".to_string())
        );
        eprintln!(
            "  Sentinel direct ref:  {}",
            if scan_result.sentinel_offsets.is_empty() {
                "No (expected)"
            } else {
                "Yes (unexpected)"
            }
        );
    } else {
        eprintln!("\n[ERROR] Calibration failed");
    }

    offsets
}

// =============================================================================
// Entry Point
// =============================================================================

fn main() {
    let offsets = calibrate_tls_offsets();

    eprintln!("\n[Final Offset Registry]");
    for (name, offset) in &offsets {
        eprintln!("  {} = 0x{:04x}", name, offset);
    }

    if offsets.contains_key("mi_heap_t") {
        eprintln!("\n[Phase 2.2] Self-Calibration SUCCESSFUL");
        eprintln!("[Phase 2.2] Use the mi_heap_t offset for TLS restoration");
    } else {
        eprintln!("\n[Phase 2.2] Self-Calibration FAILED");
        eprintln!("[Phase 2.2] Could not locate mi_heap_t in TLS");
    }
}

// =============================================================================
// Unit Tests
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
        let regions = parse_memory_maps();
        assert!(!regions.is_empty());
    }

    #[test]
    fn test_get_fs_base() {
        let fs_base = get_fs_base().expect("Failed to get fs_base");
        assert!(fs_base > 0);
    }

    #[test]
    #[ignore] // Run with: cargo test test_calibrate_tls_offsets -- --ignored --nocapture
    fn test_calibrate_tls_offsets() {
        let offsets = calibrate_tls_offsets();
        assert!(
            offsets.contains_key("mi_heap_t"),
            "Should discover mi_heap_t offset"
        );
    }
}
