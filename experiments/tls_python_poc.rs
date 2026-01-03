//! Phase 2.1: Embedded Python TLS Scan - Detecting mimalloc Heartbeats
//!
//! This POC initializes a Python interpreter via PyO3 and scans TLS for
//! mimalloc structures. This is critical for Python 3.13 where mimalloc
//! stores thread-local allocator state.
//!
//! # The mimalloc Hazard (Python 3.13)
//!
//! Python 3.13 uses mimalloc which caches allocator state in Thread Local Storage.
//! The `mi_heap_t` structure is pointed to from a fixed offset from `fs_base`.
//! If we restore Heap but leave TLS in "post-execution" state:
//! - The allocator thinks it has free blocks that no longer exist
//! - Next malloc returns corrupted/reused memory
//! - Silent corruption leads to double-frees 1000 iterations later
//!
//! # Running This Exploration
//!
//! ```bash
//! cd /home/louiskaneko/dev/tach-core
//! source .venv/bin/activate
//! export PYO3_PYTHON=$(which python)
//! cargo run --bin tls_python_poc
//! ```

use pyo3::prelude::*;
use std::fs;

// =============================================================================
// arch_prctl Constants
// =============================================================================

/// Get the FS segment base address (TLS pointer)
const ARCH_GET_FS: i32 = 0x1003;

// =============================================================================
// Memory Region Parsing
// =============================================================================

/// Information about a memory region from /proc/self/maps
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

/// Parse /proc/self/maps into memory regions
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

/// Get the FS base address (TLS pointer) via arch_prctl
fn get_fs_base() -> Result<usize, std::io::Error> {
    let mut fs_base: u64 = 0;

    let ret = unsafe { libc::syscall(libc::SYS_arch_prctl, ARCH_GET_FS, &mut fs_base as *mut u64) };

    if ret == 0 {
        Ok(fs_base as usize)
    } else {
        Err(std::io::Error::last_os_error())
    }
}

/// Find the memory region containing a given address
fn find_containing_region(regions: &[MemoryRegion], addr: usize) -> Option<&MemoryRegion> {
    regions.iter().find(|r| r.contains(addr))
}

// =============================================================================
// Heap Reference Scanner
// =============================================================================

/// Scan TLS for pointers into heap/anonymous regions
///
/// This is how we detect mimalloc's thread-local bins:
/// - mi_heap_t is allocated in heap
/// - TLS contains pointer to mi_heap_t
/// - We find these by scanning for pointer-like values
fn scan_tls_for_heap_refs(
    fs_base: usize,
    tls_region: &MemoryRegion,
    regions: &[MemoryRegion],
) -> Vec<(usize, usize, String)> {
    let mut refs = Vec::new();

    // Identify heap and anonymous regions (potential mimalloc targets)
    let target_regions: Vec<_> = regions
        .iter()
        .filter(|r| {
            r.perms.contains('w')
                && (r.pathname.contains("[heap]")
                    || (r.pathname.is_empty() && r.start != tls_region.start))
        })
        .collect();

    // Scan TLS region for pointers
    let scan_start = fs_base;
    let scan_end = std::cmp::min(fs_base + 8192, tls_region.end);

    for offset in (0..(scan_end - scan_start)).step_by(8) {
        let addr = scan_start + offset;
        if addr + 8 <= tls_region.end {
            let value = unsafe { std::ptr::read_volatile(addr as *const usize) };

            // Check if this value points into a target region
            for target in &target_regions {
                if target.contains(value) {
                    let region_name = if target.pathname.is_empty() {
                        format!("anon@0x{:x}", target.start)
                    } else {
                        target.pathname.clone()
                    };
                    refs.push((offset, value, region_name));
                }
            }
        }
    }

    refs
}

// =============================================================================
// Python Allocation Stressor
// =============================================================================

/// Run Python code that triggers mimalloc allocations
///
/// This "dirties" the allocator by:
/// 1. Allocating many small objects (populates thread-local bins)
/// 2. Creating floats (triggers PyFloat_FreeList)
/// 3. Using lists and dicts (common allocation patterns)
fn run_python_allocations(py: Python<'_>) -> PyResult<()> {
    // Build Python code as a regular string, then use run_bound
    let code = r#"
import sys
import gc

# Report Python version and allocator
print(f"[Python] Version: {sys.version}")
print(f"[Python] Implementation: {sys.implementation.name}")

# Check for mimalloc (Python 3.13+)
if hasattr(sys, '_memory_allocator'):
    print(f"[Python] Memory Allocator: {sys._memory_allocator}")
else:
    print("[Python] Memory Allocator: Unknown (pre-3.13 or not exposed)")

# ============================================
# THE DIRTYING SCRIPT: Trigger mimalloc bins
# ============================================

# Phase 1: Small object allocations (populates thread-local bins)
print("[Python] Phase 1: Allocating 1000 small objects...")
small_objects = [bytearray(64) for _ in range(1000)]

# Phase 2: Float allocations (PyFloat_FreeList)
print("[Python] Phase 2: Allocating 1000 floats...")
floats = [float(i) * 1.1 for i in range(1000)]

# Phase 3: Dict allocations (common pattern)
print("[Python] Phase 3: Allocating 500 dicts...")
dicts = [{f"key_{j}": j for j in range(10)} for _ in range(500)]

# Phase 4: Delete and reallocate to exercise free lists
print("[Python] Phase 4: Exercising free lists...")
del small_objects
del floats
del dicts
gc.collect()

# Reallocate to use free list entries
small_objects_2 = [bytearray(64) for _ in range(500)]
floats_2 = [float(i) * 2.2 for i in range(500)]

print("[Python] Allocation stress complete.")
print(f"[Python] gc.get_count() = {gc.get_count()}")
"#;

    // Convert to CStr by appending null terminator
    let code_with_nul = format!("{}\0", code);
    let code_cstr = std::ffi::CStr::from_bytes_with_nul(code_with_nul.as_bytes())
        .expect("CStr creation failed");

    py.run(code_cstr, None, None)?;

    Ok(())
}

// =============================================================================
// Main Exploration Function
// =============================================================================

/// Run the embedded Python TLS exploration
fn explore_python_tls() {
    eprintln!("{}", "=".repeat(70));
    eprintln!("Phase 2.1: Embedded Python TLS Scan - mimalloc Detection");
    eprintln!("{}", "=".repeat(70));

    // Step 1: Get BASELINE fs_base (before Python init)
    eprintln!("\n[Step 1] BASELINE: Reading fs_base BEFORE Python init...");
    let fs_base_before = get_fs_base().expect("arch_prctl failed");
    eprintln!("[TLS] fs_base (before Python) = 0x{:016x}", fs_base_before);

    let regions_before = parse_memory_maps();
    let tls_region_before = find_containing_region(&regions_before, fs_base_before);
    if let Some(r) = tls_region_before {
        eprintln!(
            "[TLS] TLS region (before): 0x{:x}-0x{:x} ({} bytes)",
            r.start,
            r.end,
            r.size()
        );
    }

    // Scan for heap refs before Python
    if let Some(tls_region) = tls_region_before {
        let refs_before = scan_tls_for_heap_refs(fs_base_before, tls_region, &regions_before);
        eprintln!(
            "[TLS] Heap references in TLS (before Python): {}",
            refs_before.len()
        );
    }

    // Step 2: Initialize Python and run allocations
    eprintln!("\n[Step 2] Initializing Python interpreter via PyO3...");

    Python::initialize();

    Python::attach(|py| {
        eprintln!("[PyO3] Python interpreter initialized.");

        // Run the allocation stressor
        eprintln!("\n[Step 3] Running Python allocation stressor...");
        if let Err(e) = run_python_allocations(py) {
            eprintln!("[Python] ERROR: {}", e);
            return;
        }

        // Step 4: Get fs_base AFTER Python allocations
        eprintln!("\n[Step 4] Reading fs_base AFTER Python allocations...");
        let fs_base_after = get_fs_base().expect("arch_prctl failed");
        eprintln!("[TLS] fs_base (after Python) = 0x{:016x}", fs_base_after);

        if fs_base_after != fs_base_before {
            eprintln!(
                "[TLS] WARNING: fs_base CHANGED! Before=0x{:x}, After=0x{:x}",
                fs_base_before, fs_base_after
            );
        } else {
            eprintln!("[TLS] fs_base unchanged (expected for main thread)");
        }

        // Step 5: Re-parse memory maps after Python init
        eprintln!("\n[Step 5] Re-parsing /proc/self/maps after Python init...");
        let regions_after = parse_memory_maps();
        eprintln!(
            "[TLS] Memory regions: before={}, after={}",
            regions_before.len(),
            regions_after.len()
        );

        // Find new regions (created by Python/mimalloc)
        let new_regions: Vec<_> = regions_after
            .iter()
            .filter(|r| !regions_before.iter().any(|rb| rb.start == r.start))
            .collect();

        eprintln!(
            "[TLS] New memory regions after Python init: {}",
            new_regions.len()
        );
        for (i, r) in new_regions.iter().take(10).enumerate() {
            eprintln!(
                "  [New Region {}] 0x{:x}-0x{:x} {} {} ({} KB)",
                i,
                r.start,
                r.end,
                r.perms,
                if r.pathname.is_empty() {
                    "(anon)"
                } else {
                    &r.pathname
                },
                r.size() / 1024
            );
        }

        // Step 6: Scan TLS for heap references (THE CRITICAL SCAN)
        eprintln!("\n[Step 6] Scanning TLS for heap/mimalloc references...");

        if let Some(tls_region) = find_containing_region(&regions_after, fs_base_after) {
            eprintln!(
                "[TLS] Current TLS region: 0x{:x}-0x{:x} ({} bytes)",
                tls_region.start,
                tls_region.end,
                tls_region.size()
            );

            let refs = scan_tls_for_heap_refs(fs_base_after, tls_region, &regions_after);

            eprintln!(
                "\n[TLS] HEAP REFERENCES IN TLS (AFTER PYTHON): {}",
                refs.len()
            );

            if refs.is_empty() {
                eprintln!("[TLS] WARNING: No heap references found in TLS!");
                eprintln!("[TLS] This may indicate:");
                eprintln!("      - Python version < 3.13 (using pymalloc, not mimalloc)");
                eprintln!("      - mimalloc not using TLS for this allocator path");
                eprintln!("      - Scan range too small (increase scan window)");
            } else {
                eprintln!("\n[TLS] MIMALLOC HEARTBEATS DETECTED:");
                eprintln!("{}", "-".repeat(70));
                for (offset, value, region) in refs.iter().take(20) {
                    eprintln!(
                        "  fs_base+0x{:04x} -> 0x{:016x} [{}]",
                        offset, value, region
                    );
                }
                if refs.len() > 20 {
                    eprintln!("  ... and {} more references", refs.len() - 20);
                }
                eprintln!("{}", "-".repeat(70));

                // Identify potential mi_heap_t pointer
                // mi_heap_t is typically the first or second heap pointer in TLS
                if let Some((offset, value, _)) = refs.first() {
                    eprintln!("\n[TLS] CANDIDATE mi_heap_t POINTER:");
                    eprintln!("      Offset from fs_base: 0x{:x}", offset);
                    eprintln!("      Points to: 0x{:016x}", value);
                    eprintln!("      This pointer MUST be snapshotted alongside the heap!");
                }
            }
        } else {
            eprintln!("[TLS] ERROR: fs_base not in any mapped region!");
        }

        // Step 7: Summary
        eprintln!("\n[Step 7] TLS Exploration Summary");
        eprintln!("{}", "=".repeat(70));
        eprintln!("Python initialized:      YES");
        eprintln!("fs_base:                 0x{:016x}", fs_base_after);
        if let Some(tls_region) = find_containing_region(&regions_after, fs_base_after) {
            eprintln!("TLS region size:         {} bytes", tls_region.size());
            let refs = scan_tls_for_heap_refs(fs_base_after, tls_region, &regions_after);
            eprintln!("Heap refs in TLS:        {}", refs.len());
        }
        eprintln!("New memory regions:      {}", new_regions.len());
        eprintln!("{}", "=".repeat(70));
    });

    eprintln!("\n[Phase 2.1] Embedded Python TLS exploration complete.");
    eprintln!("[Phase 2.1] Next: Capture TLS region in golden snapshot.");
}

// =============================================================================
// Entry Point
// =============================================================================

fn main() {
    explore_python_tls();
}

// =============================================================================
// Unit Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_fs_base() {
        let result = get_fs_base();
        assert!(result.is_ok(), "arch_prctl(ARCH_GET_FS) should succeed");
        let fs_base = result.unwrap();
        assert!(fs_base > 0, "fs_base should be non-zero");
    }

    #[test]
    fn test_parse_memory_maps() {
        let regions = parse_memory_maps();
        assert!(!regions.is_empty(), "Should find memory regions");
    }

    /// Full exploration test
    #[test]
    #[ignore] // Run with: cargo test test_explore_python_tls -- --ignored --nocapture
    fn test_explore_python_tls() {
        explore_python_tls();
    }
}
