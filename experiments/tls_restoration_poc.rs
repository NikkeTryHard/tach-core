//! Phase 2: TLS Restoration Proof-of-Concept
//!
//! This exploration script probes Thread Local Storage (TLS) to understand:
//! 1. Where the fs_base register points (Glibc Thread Control Block)
//! 2. What memory regions constitute TLS
//! 3. How to identify mimalloc TLS structures in Python 3.13
//!
//! # The mimalloc Hazard (Python 3.13)
//!
//! Python 3.13 uses mimalloc which caches allocator state in Thread Local Storage.
//! The `current_free_block` pointer is stored via fs_base. If we restore Heap
//! but leave TLS in "post-execution" state, the next malloc will use stale
//! thread-local bins, causing double-frees or memory corruption.
//!
//! # Running This Exploration
//!
//! ```bash
//! cd /home/louiskaneko/dev/tach-core
//! cargo run --bin tls_exploration
//! ```
//!
//! Or run as a test:
//! ```bash
//! cargo test --test tls_exploration -- --nocapture
//! ```

use std::fs;

// =============================================================================
// arch_prctl Constants (not in libc crate)
// =============================================================================

/// Set the GS segment base address
#[allow(dead_code)]
const ARCH_SET_GS: i32 = 0x1001;

/// Set the FS segment base address (TLS pointer)
#[allow(dead_code)]
const ARCH_SET_FS: i32 = 0x1002;

/// Get the FS segment base address (TLS pointer)
const ARCH_GET_FS: i32 = 0x1003;

/// Get the GS segment base address
#[allow(dead_code)]
const ARCH_GET_GS: i32 = 0x1004;

// =============================================================================
// TLS Segment Information
// =============================================================================

/// Information about a memory region from /proc/self/maps
#[derive(Debug, Clone)]
struct MemoryRegion {
    start: usize,
    end: usize,
    perms: String,
    offset: usize,
    dev: String,
    inode: u64,
    pathname: String,
}

impl MemoryRegion {
    /// Check if an address falls within this region
    fn contains(&self, addr: usize) -> bool {
        addr >= self.start && addr < self.end
    }

    /// Size of the region in bytes
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

        // Parse address range: "7f1234560000-7f1234580000"
        let addr_parts: Vec<&str> = parts[0].split('-').collect();
        if addr_parts.len() != 2 {
            continue;
        }

        let start = usize::from_str_radix(addr_parts[0], 16).unwrap_or(0);
        let end = usize::from_str_radix(addr_parts[1], 16).unwrap_or(0);
        let perms = parts[1].to_string();
        let offset = usize::from_str_radix(parts[2], 16).unwrap_or(0);
        let dev = parts[3].to_string();
        let inode = parts[4].parse::<u64>().unwrap_or(0);
        let pathname = if parts.len() > 5 {
            parts[5..].join(" ")
        } else {
            String::new()
        };

        regions.push(MemoryRegion {
            start,
            end,
            perms,
            offset,
            dev,
            inode,
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

/// Identify potential TLS-related regions
fn identify_tls_regions(regions: &[MemoryRegion]) -> Vec<&MemoryRegion> {
    regions
        .iter()
        .filter(|r| {
            // TLS regions are typically:
            // 1. Anonymous (no pathname)
            // 2. Writable
            // 3. Private (p in perms)
            // 4. Near the stack or marked with special annotations
            r.perms.contains('w')
                && r.perms.contains('p')
                && (r.pathname.is_empty()
                    || r.pathname.contains("[stack]")
                    || r.pathname.contains("tls")
                    || r.pathname.contains("ld-linux"))
        })
        .collect()
}

// =============================================================================
// Glibc Thread Control Block (TCB) Structure
// =============================================================================

/// The Glibc TCB layout (partial, for exploration)
///
/// The TCB is pointed to by fs_base. Key offsets:
/// - 0x00: self (points to TCB itself)
/// - 0x10: stack_guard (stack canary)
/// - 0x28: errno location
/// - 0x30: multiple_threads flag
/// - ... and many more
///
/// Reference: glibc/nptl/descr.h (struct pthread)
#[repr(C)]
#[allow(dead_code)]
struct GlibcTcbHeader {
    /// Self-pointer (tcb->self == tcb)
    self_ptr: usize,
    /// DTV (Dynamic Thread Vector) pointer
    dtv: usize,
    /// Reserved
    _reserved1: usize,
    /// Stack guard (canary value)
    stack_guard: usize,
    /// Reserved
    _reserved2: usize,
    /// Pointer to errno
    errno_ptr: usize,
    /// Multiple threads flag
    multiple_threads: i32,
}

/// Read the TCB header from fs_base
///
/// # Safety
/// This reads from the memory address pointed to by fs_base.
/// The address must be valid and contain a Glibc TCB.
unsafe fn read_tcb_header(fs_base: usize) -> GlibcTcbHeader {
    let tcb_ptr = fs_base as *const GlibcTcbHeader;
    std::ptr::read_volatile(tcb_ptr)
}

// =============================================================================
// Python 3.13 mimalloc TLS Detection
// =============================================================================

/// Scan for potential mimalloc TLS structures
///
/// mimalloc uses TLS for thread-local heaps. Key patterns:
/// 1. Look for heap metadata structures
/// 2. Look for free-list pointers
/// 3. Look for page allocation markers
fn scan_for_mimalloc_patterns(fs_base: usize, regions: &[MemoryRegion]) {
    eprintln!("\n[TLS/mimalloc] Scanning for mimalloc patterns near fs_base...");

    // Find the region containing fs_base
    if let Some(region) = find_containing_region(regions, fs_base) {
        eprintln!(
            "[TLS/mimalloc] TLS region: 0x{:x}-0x{:x} ({} bytes)",
            region.start,
            region.end,
            region.size()
        );

        // Read the first few KB after fs_base looking for heap structures
        let scan_size = std::cmp::min(4096, region.end.saturating_sub(fs_base));

        eprintln!("[TLS/mimalloc] Scanning {} bytes from fs_base", scan_size);

        // Look for pointer-like values that point into heap regions
        let heap_regions: Vec<_> = regions
            .iter()
            .filter(|r| r.pathname.contains("[heap]"))
            .collect();

        if heap_regions.is_empty() {
            eprintln!("[TLS/mimalloc] WARNING: No [heap] region found. Heap may be anonymous.");
        } else {
            for heap in &heap_regions {
                eprintln!(
                    "[TLS/mimalloc] Heap region: 0x{:x}-0x{:x} ({} KB)",
                    heap.start,
                    heap.end,
                    heap.size() / 1024
                );
            }
        }

        // Scan TLS for pointers into heap
        let mut heap_refs = 0;
        for offset in (0..scan_size).step_by(8) {
            let addr = fs_base + offset;
            if addr + 8 <= region.end {
                let value = unsafe { std::ptr::read_volatile(addr as *const usize) };

                // Check if this looks like a heap pointer
                for heap in &heap_regions {
                    if heap.contains(value) {
                        heap_refs += 1;
                        if heap_refs <= 10 {
                            eprintln!(
                                "[TLS/mimalloc] fs_base+0x{:x}: 0x{:x} -> [heap]",
                                offset, value
                            );
                        }
                    }
                }
            }
        }

        if heap_refs > 10 {
            eprintln!(
                "[TLS/mimalloc] ... and {} more heap references",
                heap_refs - 10
            );
        }

        eprintln!("[TLS/mimalloc] Total heap references in TLS: {}", heap_refs);
    } else {
        eprintln!("[TLS/mimalloc] WARNING: fs_base not in any mapped region!");
    }
}

// =============================================================================
// Main Exploration Function
// =============================================================================

/// Run the TLS exploration
///
/// This function probes the TLS segment and reports findings.
pub fn explore_tls() {
    eprintln!("{}", "=".repeat(70));
    eprintln!("Phase 2: TLS Exploration - Detecting mimalloc Heartbeats");
    eprintln!("{}", "=".repeat(70));

    // Step 1: Get fs_base
    eprintln!("\n[Step 1] Reading fs_base via arch_prctl(ARCH_GET_FS)...");

    let fs_base = match get_fs_base() {
        Ok(addr) => {
            eprintln!("[TLS] fs_base = 0x{:016x}", addr);
            addr
        }
        Err(e) => {
            eprintln!("[TLS] ERROR: arch_prctl failed: {}", e);
            return;
        }
    };

    // Step 2: Parse memory maps
    eprintln!("\n[Step 2] Parsing /proc/self/maps...");
    let regions = parse_memory_maps();
    eprintln!("[TLS] Found {} memory regions", regions.len());

    // Step 3: Find the region containing fs_base
    eprintln!("\n[Step 3] Locating TLS segment...");
    if let Some(region) = find_containing_region(&regions, fs_base) {
        eprintln!("[TLS] fs_base is in region:");
        eprintln!("      Address: 0x{:x}-0x{:x}", region.start, region.end);
        eprintln!(
            "      Size: {} bytes ({} KB)",
            region.size(),
            region.size() / 1024
        );
        eprintln!("      Perms: {}", region.perms);
        eprintln!(
            "      Pathname: {}",
            if region.pathname.is_empty() {
                "(anonymous)"
            } else {
                &region.pathname
            }
        );
    } else {
        eprintln!("[TLS] WARNING: fs_base is not in any mapped region!");
    }

    // Step 4: Read TCB header
    eprintln!("\n[Step 4] Reading Glibc TCB header...");
    let tcb = unsafe { read_tcb_header(fs_base) };
    eprintln!(
        "[TCB] self_ptr: 0x{:016x} (should equal fs_base)",
        tcb.self_ptr
    );
    eprintln!("[TCB] dtv: 0x{:016x}", tcb.dtv);
    eprintln!("[TCB] stack_guard: 0x{:016x}", tcb.stack_guard);
    eprintln!("[TCB] errno_ptr: 0x{:016x}", tcb.errno_ptr);
    eprintln!("[TCB] multiple_threads: {}", tcb.multiple_threads);

    // Validate self-pointer
    if tcb.self_ptr == fs_base {
        eprintln!("[TCB] VALID: self_ptr matches fs_base");
    } else {
        eprintln!("[TCB] WARNING: self_ptr does NOT match fs_base!");
    }

    // Step 5: Identify TLS-related regions
    eprintln!("\n[Step 5] Identifying TLS-related memory regions...");
    let tls_regions = identify_tls_regions(&regions);
    for (i, region) in tls_regions.iter().enumerate() {
        eprintln!(
            "[TLS Region {}] 0x{:x}-0x{:x} {} {}",
            i,
            region.start,
            region.end,
            region.perms,
            if region.pathname.is_empty() {
                "(anon)"
            } else {
                &region.pathname
            }
        );
    }

    // Step 6: Scan for mimalloc patterns
    eprintln!("\n[Step 6] Scanning for mimalloc TLS patterns...");
    scan_for_mimalloc_patterns(fs_base, &regions);

    // Step 7: Summary
    eprintln!("\n[Step 7] TLS Exploration Summary");
    eprintln!("{}", "=".repeat(70));
    eprintln!("fs_base:         0x{:016x}", fs_base);
    eprintln!(
        "TLS region size: {} bytes",
        find_containing_region(&regions, fs_base)
            .map(|r| r.size())
            .unwrap_or(0)
    );
    eprintln!(
        "TCB valid:       {}",
        if tcb.self_ptr == fs_base { "YES" } else { "NO" }
    );
    eprintln!("Total regions:   {}", regions.len());
    eprintln!("TLS regions:     {}", tls_regions.len());
    eprintln!("{}", "=".repeat(70));

    eprintln!("\n[Phase 2] TLS exploration complete.");
    eprintln!("[Phase 2] Next: Capture TLS in golden snapshot, restore on reset.");
}

// =============================================================================
// Entry Points
// =============================================================================

/// Main function for standalone binary execution
fn main() {
    explore_tls();
}

// =============================================================================
// Unit Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_arch_prctl_constants() {
        assert_eq!(ARCH_SET_GS, 0x1001);
        assert_eq!(ARCH_SET_FS, 0x1002);
        assert_eq!(ARCH_GET_FS, 0x1003);
        assert_eq!(ARCH_GET_GS, 0x1004);
    }

    #[test]
    fn test_get_fs_base() {
        let result = get_fs_base();
        assert!(result.is_ok(), "arch_prctl(ARCH_GET_FS) should succeed");

        let fs_base = result.unwrap();
        assert!(fs_base > 0, "fs_base should be non-zero");
        eprintln!("[test] fs_base = 0x{:x}", fs_base);
    }

    #[test]
    fn test_parse_memory_maps() {
        let regions = parse_memory_maps();
        assert!(
            !regions.is_empty(),
            "Should find at least one memory region"
        );

        // Should find stack
        let has_stack = regions.iter().any(|r| r.pathname.contains("[stack]"));
        assert!(has_stack, "Should find [stack] region");
    }

    #[test]
    fn test_fs_base_in_mapped_region() {
        let fs_base = get_fs_base().expect("get_fs_base failed");
        let regions = parse_memory_maps();

        let containing_region = find_containing_region(&regions, fs_base);
        assert!(
            containing_region.is_some(),
            "fs_base should be in a mapped region"
        );
    }

    #[test]
    fn test_tcb_self_pointer() {
        let fs_base = get_fs_base().expect("get_fs_base failed");
        let tcb = unsafe { read_tcb_header(fs_base) };

        assert_eq!(
            tcb.self_ptr, fs_base,
            "TCB self_ptr should equal fs_base (Glibc invariant)"
        );
    }

    /// Full exploration test - run with --nocapture to see output
    #[test]
    #[ignore] // Run explicitly with: cargo test explore_tls_full -- --ignored --nocapture
    fn test_explore_tls_full() {
        explore_tls();
    }
}
