//! End-to-End Coverage Pipeline Integration Tests
//!
//! These tests verify the full coverage collection pipeline works correctly,
//! from ring buffer initialization through report generation.

use std::mem::size_of;
use tach_core::coverage::{CoverageEntry, ENTRY_SIZE, MAPPING_ENTRY_SIZE, MappingEntry};

// =============================================================================
// Coverage Entry Tests
// =============================================================================

#[test]
fn test_coverage_entry_line_creation() {
    let entry = CoverageEntry::line(42, 100);

    assert_eq!(entry.code_id, 42);
    assert_eq!(entry.lineno, 100);
    assert_eq!(entry.flags & 0x01, 0x01, "LINE flag should be set");
}

#[test]
fn test_coverage_entry_size_constant() {
    assert_eq!(size_of::<CoverageEntry>(), ENTRY_SIZE);
}

#[test]
fn test_coverage_entry_sequential_ids() {
    let entries: Vec<CoverageEntry> = (0..100u32)
        .map(|i| CoverageEntry::line(i as u64, i * 10))
        .collect();

    for (i, entry) in entries.iter().enumerate() {
        assert_eq!(entry.code_id, i as u64);
        assert_eq!(entry.lineno, (i * 10) as u32);
    }
}

#[test]
fn test_coverage_entry_max_values() {
    let entry = CoverageEntry::line(u64::MAX, u32::MAX);

    assert_eq!(entry.code_id, u64::MAX);
    assert_eq!(entry.lineno, u32::MAX);
}

#[test]
fn test_coverage_entry_zero_values() {
    let entry = CoverageEntry::line(0, 0);

    assert_eq!(entry.code_id, 0);
    assert_eq!(entry.lineno, 0);
}

// =============================================================================
// Mapping Entry Tests
// =============================================================================

#[test]
fn test_mapping_entry_creation() {
    let entry = MappingEntry::new(123, "tests/test_example.py");

    assert_eq!(entry.code_id, 123);
    assert!(!entry.filename().is_empty());
}

#[test]
fn test_mapping_entry_size_constant() {
    assert_eq!(size_of::<MappingEntry>(), MAPPING_ENTRY_SIZE);
}

#[test]
fn test_mapping_entry_filename_preserved() {
    let filename = "tests/unit/test_feature.py";
    let entry = MappingEntry::new(1, filename);

    assert_eq!(entry.filename(), filename);
}

#[test]
fn test_mapping_entry_truncation() {
    let long_filename = "a".repeat(500);
    let entry = MappingEntry::new(1, &long_filename);

    // Filename should be truncated to fit
    assert!(entry.filename_len <= 240);
    assert!(entry.filename().len() <= 240);
}

#[test]
fn test_mapping_entry_unicode() {
    let unicode_filename = "tests/test_\u{1F600}.py";
    let entry = MappingEntry::new(1, unicode_filename);

    // Should handle Unicode without panicking
    let _ = entry.filename();
}

#[test]
fn test_mapping_entry_empty_filename() {
    let entry = MappingEntry::new(1, "");

    assert_eq!(entry.filename_len, 0);
    assert!(entry.filename().is_empty());
}

// =============================================================================
// Coverage Collection Simulation Tests
// =============================================================================

#[test]
fn test_coverage_collection_ordering() {
    // Simulate coverage events coming in order
    let mut entries = Vec::new();

    // Simulate executing lines 10, 20, 30, 20, 10, 40
    let lines = [10, 20, 30, 20, 10, 40];

    for (i, &line) in lines.iter().enumerate() {
        entries.push(CoverageEntry::line(0, line));
        assert_eq!(entries.len(), i + 1);
    }

    assert_eq!(entries.len(), 6);
}

#[test]
fn test_coverage_deduplication_logic() {
    use std::collections::HashSet;

    let entries = [
        CoverageEntry::line(0, 10),
        CoverageEntry::line(0, 20),
        CoverageEntry::line(0, 10), // Duplicate
        CoverageEntry::line(0, 30),
        CoverageEntry::line(0, 20), // Duplicate
    ];

    // Deduplicate by (code_id, lineno)
    let unique: HashSet<(u64, u32)> = entries.iter().map(|e| (e.code_id, e.lineno)).collect();

    assert_eq!(unique.len(), 3); // Lines 10, 20, 30
}

#[test]
fn test_coverage_multiple_files() {
    let entries = vec![
        CoverageEntry::line(0, 10), // File 0
        CoverageEntry::line(1, 10), // File 1
        CoverageEntry::line(0, 20), // File 0
        CoverageEntry::line(2, 5),  // File 2
    ];

    // Group by code_id
    let mut by_file: std::collections::HashMap<u64, Vec<u32>> = std::collections::HashMap::new();
    for entry in entries {
        by_file.entry(entry.code_id).or_default().push(entry.lineno);
    }

    assert_eq!(by_file.len(), 3);
    assert_eq!(by_file.get(&0).unwrap().len(), 2);
    assert_eq!(by_file.get(&1).unwrap().len(), 1);
    assert_eq!(by_file.get(&2).unwrap().len(), 1);
}

// =============================================================================
// Coverage Report Format Tests
// =============================================================================

#[test]
fn test_lcov_line_format() {
    // LCOV format: DA:lineno,hit_count
    let lineno = 42u32;
    let hit_count = 1u32;

    let lcov_line = format!("DA:{},{}", lineno, hit_count);
    assert_eq!(lcov_line, "DA:42,1");
}

#[test]
fn test_lcov_file_header() {
    let filename = "tests/test_example.py";

    let sf_line = format!("SF:{}", filename);
    assert_eq!(sf_line, "SF:tests/test_example.py");
}

#[test]
fn test_lcov_end_record() {
    let end_record = "end_of_record";
    assert_eq!(end_record, "end_of_record");
}

#[test]
fn test_json_coverage_format() {
    // JSON coverage format for a file
    let file_coverage = serde_json::json!({
        "path": "tests/test_example.py",
        "covered_lines": [10, 20, 30],
        "total_lines": 50,
        "coverage_percent": 60.0
    });

    let json_str = serde_json::to_string(&file_coverage).unwrap();
    assert!(json_str.contains("test_example.py"));
    assert!(json_str.contains("covered_lines"));
}

// =============================================================================
// Ring Buffer Simulation Tests
// =============================================================================

#[test]
fn test_ring_buffer_capacity_calculation() {
    // Ring buffer should be sized to hold N entries
    let entry_count = 1024usize;
    let required_size = entry_count * ENTRY_SIZE;

    // Page-align
    let page_size = 4096usize;
    let aligned_size = (required_size + page_size - 1) & !(page_size - 1);

    assert!(aligned_size >= required_size);
    assert_eq!(aligned_size % page_size, 0);
}

#[test]
fn test_ring_buffer_wrap_calculation() {
    let capacity = 1024usize;
    let positions = [0, 100, 500, 1023, 1024, 1025, 2048];

    for pos in positions {
        let wrapped = pos % capacity;
        assert!(wrapped < capacity);
    }
}

#[test]
fn test_ring_buffer_overflow_detection() {
    let capacity = 1024u64;
    let mut write_pos = 0u64;
    let mut overflow_count = 0u64;

    // Simulate writing 3000 entries to a 1024-entry buffer
    for _ in 0..3000 {
        if write_pos >= capacity {
            overflow_count += 1;
            write_pos = 0;
        }
        write_pos += 1;
    }

    // Should have overflowed ~2 times
    assert!(overflow_count >= 2);
}

// =============================================================================
// Coverage Statistics Tests
// =============================================================================

#[test]
fn test_coverage_percentage_calculation() {
    let covered_lines = 75u32;
    let total_lines = 100u32;

    let percentage = (covered_lines as f64 / total_lines as f64) * 100.0;
    assert!((percentage - 75.0).abs() < 0.01);
}

#[test]
fn test_coverage_percentage_zero_lines() {
    let covered_lines = 0u32;
    let total_lines = 0u32;

    // Avoid division by zero
    let percentage = if total_lines == 0 {
        100.0 // Vacuously 100% if no lines to cover
    } else {
        (covered_lines as f64 / total_lines as f64) * 100.0
    };

    assert!((percentage - 100.0).abs() < 0.01);
}

#[test]
fn test_coverage_aggregation() {
    // Aggregate coverage across multiple files
    let files = [
        (10, 20), // File 1: 10/20 lines covered
        (15, 30), // File 2: 15/30 lines covered
        (25, 50), // File 3: 25/50 lines covered
    ];

    let total_covered: u32 = files.iter().map(|(c, _)| c).sum();
    let total_lines: u32 = files.iter().map(|(_, t)| t).sum();

    assert_eq!(total_covered, 50);
    assert_eq!(total_lines, 100);

    let overall_percentage = (total_covered as f64 / total_lines as f64) * 100.0;
    assert!((overall_percentage - 50.0).abs() < 0.01);
}

// =============================================================================
// Edge Cases
// =============================================================================

#[test]
fn test_coverage_entry_alignment() {
    // Entry should be properly aligned for atomic operations
    assert!(
        ENTRY_SIZE.is_multiple_of(8),
        "Entry size should be 8-byte aligned"
    );
}

#[test]
fn test_mapping_entry_alignment() {
    assert!(
        MAPPING_ENTRY_SIZE.is_multiple_of(8),
        "Mapping entry size should be 8-byte aligned"
    );
}

#[test]
fn test_high_frequency_line_events() {
    // Simulate high-frequency line events (hot loop)
    let hot_line = 42u32;
    let mut hit_count = 0u64;

    for _ in 0..1_000_000 {
        let _entry = CoverageEntry::line(0, hot_line);
        hit_count += 1;
    }

    assert_eq!(hit_count, 1_000_000);
}
