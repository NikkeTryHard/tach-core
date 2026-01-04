//! Fuzz target for TLS Snapshot Operations
//!
//! This fuzzer tests TLS offset calculations and memory region handling
//! to ensure they don't overflow or produce invalid addresses.

#![no_main]

use libfuzzer_sys::fuzz_target;

/// Page size constant (4KB on x86_64)
const PAGE_SIZE: usize = 4096;

/// Simulates TLS offset calculation from fs_base
fn calculate_tls_address(fs_base: usize, offset: usize) -> Option<usize> {
    fs_base.checked_add(offset)
}

/// Simulates page alignment (align down)
fn page_align_down(addr: usize) -> usize {
    addr & !(PAGE_SIZE - 1)
}

/// Simulates page alignment (align up)
fn page_align_up(addr: usize) -> Option<usize> {
    addr.checked_add(PAGE_SIZE - 1).map(|a| a & !(PAGE_SIZE - 1))
}

/// Simulates memory region length calculation
fn region_length(start: usize, end: usize) -> Option<usize> {
    if end >= start {
        Some(end - start)
    } else {
        None
    }
}

/// Simulates snapshot buffer size calculation
fn snapshot_buffer_size(region_count: usize, avg_region_size: usize) -> Option<usize> {
    region_count.checked_mul(avg_region_size)
}

/// Check if address is in userspace (x86_64)
fn is_userspace_address(addr: usize) -> bool {
    addr < 0x8000_0000_0000
}

/// Check if address is in typical TLS region (high userspace)
fn is_tls_region(addr: usize) -> bool {
    addr >= 0x7f00_0000_0000 && addr < 0x8000_0000_0000
}

fuzz_target!(|data: (u64, u64, u32, u32)| {
    let (fs_base_raw, offset_raw, region_count, avg_size) = data;

    // Constrain to reasonable ranges
    let fs_base = (fs_base_raw as usize) % 0x8000_0000_0000; // Keep in userspace
    let offset = (offset_raw as usize) % 0x100000; // TLS typically < 1MB
    let region_count = (region_count as usize) % 1000;
    let avg_size = (avg_size as usize) % 0x100000;

    // Test 1: TLS address calculation
    if let Some(tls_addr) = calculate_tls_address(fs_base, offset) {
        // Invariant: Result should be >= fs_base when offset > 0
        if offset > 0 {
            assert!(tls_addr > fs_base, "TLS address should increase with positive offset");
        }

        // Invariant: Result should be in userspace
        if is_tls_region(fs_base) && offset < 0x10000 {
            assert!(is_userspace_address(tls_addr), "TLS access should stay in userspace");
        }
    }

    // Test 2: Page alignment operations
    let aligned_down = page_align_down(fs_base);

    // Invariant: Aligned-down should be <= original
    assert!(aligned_down <= fs_base, "Aligned-down should be <= original");

    // Invariant: Aligned-down should be page-aligned
    assert_eq!(aligned_down % PAGE_SIZE, 0, "Should be page-aligned");

    // Test 3: Page align-up
    if let Some(aligned_up) = page_align_up(fs_base) {
        // Invariant: Aligned-up should be >= original
        assert!(aligned_up >= fs_base, "Aligned-up should be >= original");

        // Invariant: Aligned-up should be page-aligned
        assert_eq!(aligned_up % PAGE_SIZE, 0, "Should be page-aligned");

        // Invariant: Difference should be < PAGE_SIZE
        assert!(aligned_up - fs_base < PAGE_SIZE, "Alignment waste should be < PAGE_SIZE");
    }

    // Test 4: Region length calculation
    let start = page_align_down(fs_base);
    if let Some(end) = page_align_up(fs_base.saturating_add(offset)) {
        if let Some(length) = region_length(start, end) {
            // Invariant: Length should be page-aligned
            assert_eq!(length % PAGE_SIZE, 0, "Region length should be page-aligned");

            // Invariant: Length should be >= offset
            assert!(length >= offset, "Region should cover the requested range");
        }
    }

    // Test 5: Snapshot buffer size
    if let Some(total) = snapshot_buffer_size(region_count, avg_size) {
        // Invariant: Total should be >= each component (unless zero)
        if region_count > 0 && avg_size > 0 {
            assert!(total >= avg_size, "Total should be >= avg_size");
        }
    }
});
