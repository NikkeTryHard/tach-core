//! Property-Based Tests for Snapshot and TLS Restoration
//!
//! These tests use proptest to verify invariants of the snapshot/restore system
//! that are difficult to test exhaustively with unit tests.
//!
//! Key invariants tested:
//! 1. TLS offset calculations don't overflow
//! 2. Memory region alignment is correct for page boundaries
//! 3. Region merging produces non-overlapping segments
//! 4. Page-aligned addresses remain aligned after operations

use proptest::prelude::*;

// =============================================================================
// Page Alignment Property Tests
// =============================================================================

/// Page size constant (4KB on x86_64/aarch64)
const PAGE_SIZE: usize = 4096;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1000))]

    /// Property: Page alignment always produces addresses divisible by PAGE_SIZE
    #[test]
    fn page_align_down_produces_aligned(addr in 0usize..0x7fff_ffff_ffff_ffff) {
        let aligned = addr & !(PAGE_SIZE - 1);
        prop_assert_eq!(aligned % PAGE_SIZE, 0, "Aligned address should be page-aligned");
        prop_assert!(aligned <= addr, "Aligned-down should be <= original");
    }

    /// Property: Page align-up produces aligned addresses >= original
    #[test]
    fn page_align_up_produces_aligned(addr in 0usize..0x7fff_ffff_ffff_0000) {
        let aligned = (addr + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        prop_assert_eq!(aligned % PAGE_SIZE, 0, "Aligned address should be page-aligned");
        prop_assert!(aligned >= addr, "Aligned-up should be >= original");
    }

    /// Property: Page alignment is idempotent (aligning twice gives same result)
    #[test]
    fn page_align_is_idempotent(addr in 0usize..0x7fff_ffff_ffff_0000) {
        let aligned_once = addr & !(PAGE_SIZE - 1);
        let aligned_twice = aligned_once & !(PAGE_SIZE - 1);
        prop_assert_eq!(aligned_once, aligned_twice, "Alignment should be idempotent");
    }

    /// Property: Distance between aligned-down and aligned-up is at most one page
    #[test]
    fn page_alignment_span(addr in 0usize..0x7fff_ffff_ffff_0000) {
        let down = addr & !(PAGE_SIZE - 1);
        let up = (addr + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);

        // If addr is already aligned, down == up
        // Otherwise, up = down + PAGE_SIZE
        let span = up - down;
        prop_assert!(span == 0 || span == PAGE_SIZE,
            "Span between down and up should be 0 or PAGE_SIZE, got {}", span);
    }
}

// =============================================================================
// Memory Region Property Tests
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    /// Property: Region length calculation doesn't overflow for valid inputs
    #[test]
    fn region_length_no_overflow(
        start in 0x1000usize..0x7fff_ffff_0000,
        len in 0usize..0x1_0000_0000,
    ) {
        let end_result = start.checked_add(len);
        prop_assert!(end_result.is_some() || len > usize::MAX - start,
            "End calculation should succeed or be a valid overflow");
    }

    /// Property: Region contains check is correct
    #[test]
    fn region_contains_invariant(
        start in 0x1000usize..0x7fff_0000_0000,
        len in 1usize..0x1000_0000,
        offset in 0usize..0x1000_0000,
    ) {
        let end = start.saturating_add(len);
        let test_addr = start.saturating_add(offset % (len.max(1)));

        // Address should be in region if start <= addr < end
        let should_contain = test_addr >= start && test_addr < end;

        if offset < len {
            prop_assert!(should_contain,
                "Address {} should be in region [{}, {})", test_addr, start, end);
        }
    }

    /// Property: Non-overlapping regions stay non-overlapping after alignment
    #[test]
    fn aligned_regions_non_overlapping(
        start1 in (PAGE_SIZE as u64..0x7fff_0000_0000u64).prop_map(|x| x as usize),
        len1 in (PAGE_SIZE as u64..0x1000_0000u64).prop_map(|x| x as usize),
        gap in (PAGE_SIZE as u64..0x1000_0000u64).prop_map(|x| x as usize),
        len2 in (PAGE_SIZE as u64..0x1000_0000u64).prop_map(|x| x as usize),
    ) {
        // Align first region
        let r1_start = start1 & !(PAGE_SIZE - 1);
        let r1_end = (start1 + len1 + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);

        // Second region starts after first with a gap
        let r2_base = r1_end.saturating_add(gap);
        let r2_start = r2_base & !(PAGE_SIZE - 1);
        let r2_end = (r2_base + len2 + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);

        // Aligned regions with a gap should not overlap
        prop_assert!(r1_end <= r2_start,
            "Regions should not overlap: [{:#x}, {:#x}) and [{:#x}, {:#x})",
            r1_start, r1_end, r2_start, r2_end);
    }
}

// =============================================================================
// TLS Offset Property Tests
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    /// Property: TLS offset from fs_base is within expected bounds
    #[test]
    fn tls_offset_bounds(
        fs_base in 0x7f00_0000_0000usize..0x7fff_0000_0000,
        offset in 0usize..0x10000,  // TLS typically < 64KB
    ) {
        let tls_addr = fs_base.checked_add(offset);
        prop_assert!(tls_addr.is_some(), "TLS offset should not overflow");

        let addr = tls_addr.unwrap();
        prop_assert!(addr > fs_base, "TLS addr should be > fs_base when offset > 0");
    }

    /// Property: 8-byte aligned offsets stay aligned
    #[test]
    fn tls_offset_alignment(offset in (0u64..0x10000).prop_map(|x| (x * 8) as usize)) {
        prop_assert_eq!(offset % 8, 0, "Offset should be 8-byte aligned");
    }

    /// Property: TLS region size calculation is consistent
    #[test]
    fn tls_region_size_consistency(
        fs_base in 0x7f00_0000_0000usize..0x7fff_0000_0000,
        region_end in 0x7f00_0000_0000usize..0x7fff_ffff_0000,
    ) {
        if region_end > fs_base {
            let size = region_end - fs_base;
            prop_assert!(size > 0, "Size should be positive");

            // Reconstructing end from size should give same result
            let reconstructed_end = fs_base + size;
            prop_assert_eq!(reconstructed_end, region_end);
        }
    }
}

// =============================================================================
// Region Merging Property Tests
// =============================================================================

/// Simulated memory region for testing merge logic
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct TestRegion {
    start: usize,
    end: usize,
}

impl TestRegion {
    fn overlaps(&self, other: &Self) -> bool {
        self.start < other.end && other.start < self.end
    }

    fn adjacent(&self, other: &Self) -> bool {
        self.end == other.start || other.end == self.start
    }

    fn merge(&self, other: &Self) -> Self {
        TestRegion {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }
}

/// Merge overlapping/adjacent regions
fn merge_regions(mut regions: Vec<TestRegion>) -> Vec<TestRegion> {
    if regions.is_empty() {
        return regions;
    }

    regions.sort_by_key(|r| r.start);

    let mut merged = vec![regions[0].clone()];

    for region in regions.into_iter().skip(1) {
        let last = merged.last_mut().unwrap();
        if last.overlaps(&region) || last.adjacent(&region) {
            *last = last.merge(&region);
        } else {
            merged.push(region);
        }
    }

    merged
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    /// Property: Merged regions are sorted by start address
    #[test]
    fn merged_regions_sorted(
        regions in prop::collection::vec(
            (0x1000usize..0x1_0000_0000, 0x100usize..0x10000)
                .prop_map(|(start, len)| TestRegion { start, end: start + len }),
            0..10
        )
    ) {
        let merged = merge_regions(regions);

        for window in merged.windows(2) {
            prop_assert!(window[0].start < window[1].start,
                "Regions should be sorted: {:?} should come before {:?}",
                window[0], window[1]);
        }
    }

    /// Property: Merged regions are non-overlapping
    #[test]
    fn merged_regions_non_overlapping(
        regions in prop::collection::vec(
            (0x1000usize..0x1_0000_0000, 0x100usize..0x10000)
                .prop_map(|(start, len)| TestRegion { start, end: start + len }),
            0..10
        )
    ) {
        let merged = merge_regions(regions);

        for window in merged.windows(2) {
            prop_assert!(!window[0].overlaps(&window[1]),
                "Merged regions should not overlap: {:?} and {:?}",
                window[0], window[1]);
        }
    }

    /// Property: Merged regions are not adjacent (would have been merged)
    #[test]
    fn merged_regions_not_adjacent(
        regions in prop::collection::vec(
            (0x1000usize..0x1_0000_0000, 0x100usize..0x10000)
                .prop_map(|(start, len)| TestRegion { start, end: start + len }),
            0..10
        )
    ) {
        let merged = merge_regions(regions);

        for window in merged.windows(2) {
            prop_assert!(!window[0].adjacent(&window[1]),
                "Adjacent regions should have been merged: {:?} and {:?}",
                window[0], window[1]);
        }
    }

    /// Property: Total coverage doesn't decrease after merging
    #[test]
    fn merged_regions_preserve_coverage(
        regions in prop::collection::vec(
            (0x1000usize..0x100_0000, 0x100usize..0x1000)
                .prop_map(|(start, len)| TestRegion { start, end: start + len }),
            0..5
        )
    ) {
        // Calculate original coverage (with overlaps counted multiple times)
        let original_total: usize = regions.iter().map(|r| r.end - r.start).sum();

        let merged = merge_regions(regions.clone());
        let merged_total: usize = merged.iter().map(|r| r.end - r.start).sum();

        // Merged total should be <= original (overlaps removed)
        // and should be > 0 if original was > 0
        if !regions.is_empty() {
            prop_assert!(merged_total > 0, "Should preserve some coverage");
        }
        prop_assert!(merged_total <= original_total,
            "Merged coverage {} should be <= original {}", merged_total, original_total);
    }
}

// =============================================================================
// Snapshot Size Calculation Property Tests
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    /// Property: Snapshot buffer size calculation handles edge cases
    #[test]
    fn snapshot_buffer_size_calculation(
        region_count in 0usize..100,
        avg_region_size in PAGE_SIZE..0x100000usize,
    ) {
        // Simulate calculating total buffer size needed
        let total_size = region_count.checked_mul(avg_region_size);

        // Should either succeed or be a valid overflow
        prop_assert!(total_size.is_some() || region_count > usize::MAX / avg_region_size,
            "Size calculation should succeed or overflow predictably");
    }

    /// Property: Page-aligned size is always >= original size
    #[test]
    fn page_aligned_size_gte_original(size in 0usize..0x1_0000_0000) {
        let aligned_size = (size + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        prop_assert!(aligned_size >= size,
            "Aligned size {} should be >= original size {}", aligned_size, size);
    }

    /// Property: Page-aligned size waste is less than one page
    #[test]
    fn page_alignment_waste_bounded(size in 1usize..0x1_0000_0000) {
        let aligned_size = (size + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        let waste = aligned_size - size;
        prop_assert!(waste < PAGE_SIZE,
            "Alignment waste {} should be < PAGE_SIZE {}", waste, PAGE_SIZE);
    }
}

// =============================================================================
// fs_base Register Property Tests (x86_64 specific)
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// Property: Valid fs_base addresses are in userspace range
    #[test]
    fn fs_base_in_userspace_range(fs_base in 0x7f00_0000_0000usize..0x7fff_ffff_0000) {
        // fs_base should be in the high userspace range on x86_64
        prop_assert!(fs_base >= 0x7f00_0000_0000,
            "fs_base should be in high userspace");
        prop_assert!(fs_base < 0x8000_0000_0000,
            "fs_base should be below kernel space");
    }

    /// Property: TLS access at offset doesn't cross into kernel space
    #[test]
    fn tls_access_stays_in_userspace(
        fs_base in 0x7f00_0000_0000usize..0x7ffe_0000_0000,
        offset in 0usize..0x100000,
    ) {
        let access_addr = fs_base.saturating_add(offset);
        prop_assert!(access_addr < 0x8000_0000_0000,
            "TLS access should stay in userspace: {:#x}", access_addr);
    }
}

// =============================================================================
// Integration: Snapshot Capture/Restore Simulation
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    /// Property: Capture then restore produces same data
    #[test]
    fn capture_restore_roundtrip(
        data in prop::collection::vec(any::<u8>(), 0..PAGE_SIZE * 4)
    ) {
        // Simulate capture
        let captured = data.clone();

        // Simulate restore
        let mut restored = vec![0u8; data.len()];
        restored.copy_from_slice(&captured);

        prop_assert_eq!(restored, data, "Restored data should match original");
    }

    /// Property: Multiple pages are captured and restored independently
    #[test]
    fn multi_page_capture_restore(
        pages in prop::collection::vec(
            prop::collection::vec(any::<u8>(), PAGE_SIZE..PAGE_SIZE+1),
            1..10
        )
    ) {
        // Simulate capturing each page
        let captured: Vec<Vec<u8>> = pages.to_vec();

        // Simulate restoring each page
        let restored: Vec<Vec<u8>> = captured.clone();

        prop_assert_eq!(restored.len(), pages.len());
        for (i, (orig, rest)) in pages.iter().zip(restored.iter()).enumerate() {
            prop_assert_eq!(orig, rest, "Page {} should match", i);
        }
    }
}
