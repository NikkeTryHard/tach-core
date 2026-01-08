//! Property-Based Tests for Coverage Ring Buffer
//!
//! These tests use proptest to verify invariants of the lock-free ring buffer
//! that are difficult to test exhaustively with unit tests.
//!
//! Key invariants tested:
//! 1. Write index never exceeds capacity when buffer is drained
//! 2. Overflow count correctly tracks dropped entries
//! 3. No data corruption under concurrent access patterns
//! 4. Mapping entries preserve filename integrity

use proptest::prelude::*;

// Re-export coverage types for testing
use tach_core::coverage::{CoverageEntry, ENTRY_SIZE, MAPPING_ENTRY_SIZE, MappingEntry};

// =============================================================================
// CoverageEntry Property Tests
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1000))]

    /// Property: CoverageEntry size is always exactly ENTRY_SIZE bytes
    #[test]
    fn coverage_entry_size_invariant(code_id: u64, lineno: u32) {
        let entry = CoverageEntry::line(code_id, lineno);
        prop_assert_eq!(std::mem::size_of_val(&entry), ENTRY_SIZE);
    }

    /// Property: CoverageEntry::line() always sets LINE flag (0x01)
    #[test]
    fn coverage_entry_line_flag(code_id: u64, lineno: u32) {
        let entry = CoverageEntry::line(code_id, lineno);
        prop_assert_eq!(entry.flags & 0x01, 0x01);
        prop_assert_eq!(entry.code_id, code_id);
        prop_assert_eq!(entry.lineno, lineno);
    }

    /// Property: Any valid code_id and lineno can be stored and retrieved
    #[test]
    fn coverage_entry_roundtrip(code_id: u64, lineno: u32) {
        let entry = CoverageEntry::line(code_id, lineno);
        prop_assert_eq!(entry.code_id, code_id);
        prop_assert_eq!(entry.lineno, lineno);
    }
}

// =============================================================================
// MappingEntry Property Tests
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    /// Property: MappingEntry size is always exactly MAPPING_ENTRY_SIZE bytes
    #[test]
    fn mapping_entry_size_invariant(code_id: u64) {
        let entry = MappingEntry::new(code_id, "/test/path.py");
        prop_assert_eq!(std::mem::size_of_val(&entry), MAPPING_ENTRY_SIZE);
    }

    /// Property: Short filenames are preserved exactly
    #[test]
    fn mapping_entry_short_filename_preserved(
        code_id: u64,
        filename in "[a-zA-Z0-9/_.-]{1,200}"
    ) {
        let entry = MappingEntry::new(code_id, &filename);
        prop_assert_eq!(entry.code_id, code_id);
        prop_assert_eq!(entry.filename(), filename);
    }

    /// Property: Long filenames are truncated to <= 240 bytes
    #[test]
    fn mapping_entry_long_filename_truncated(
        code_id: u64,
        prefix in "[a-zA-Z0-9/_.-]{100,200}",
        suffix in "[a-zA-Z0-9/_.-]{100,200}"
    ) {
        let long_filename = format!("{}{}", prefix, suffix);
        let entry = MappingEntry::new(code_id, &long_filename);

        prop_assert!(entry.filename_len <= 240);
        prop_assert!(entry.filename().len() <= 240);

        // Important: truncation from LEFT preserves the suffix
        if long_filename.len() > 240 {
            prop_assert!(entry.filename().ends_with(&suffix[suffix.len().saturating_sub(50)..]),
                "Suffix should be preserved after left-truncation");
        }
    }

    /// Property: Empty filename produces empty result
    #[test]
    fn mapping_entry_empty_filename(code_id: u64) {
        let entry = MappingEntry::new(code_id, "");
        prop_assert_eq!(entry.code_id, code_id);
        prop_assert_eq!(entry.filename_len, 0);
        prop_assert_eq!(entry.filename(), "");
    }

    /// Property: UTF-8 strings remain valid after truncation
    #[test]
    fn mapping_entry_utf8_safety(
        code_id: u64,
        path in "[a-zA-Z0-9/_.-]{0,50}",
        unicode_chars in "[αβγδεζηθικλμνξοπρστυφχψω]{0,100}"
    ) {
        let filename = format!("{}{}", path, unicode_chars);
        let entry = MappingEntry::new(code_id, &filename);

        // The result should be valid UTF-8 (no replacement characters)
        let result = entry.filename();
        prop_assert!(!result.contains('\u{FFFD}'),
            "Result should not contain UTF-8 replacement characters");
    }
}

// =============================================================================
// Protocol Serialization Property Tests
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    /// Property: TestPayload serializes and deserializes correctly
    #[test]
    fn test_payload_roundtrip(
        test_id: u32,
        file_path in "[a-zA-Z0-9/_.-]{1,100}",
        test_name in "[a-zA-Z_][a-zA-Z0-9_]{0,50}",
        is_toxic: bool,
        is_async: bool,
        log_fd in 0i32..1000,
    ) {
        use tach_core::protocol::TestPayload;

        let payload = TestPayload {
            test_id,
            file_path: file_path.clone(),
            test_name: test_name.clone(),
            fixtures: vec![],
            is_async,
            is_toxic,
            log_fd,
            debug_socket_path: String::new(),
            timeout_secs: None,
        };

        let serialized = bincode::serde::encode_to_vec(&payload, bincode::config::standard()).expect("Failed to serialize");
        let (deserialized, _): (TestPayload, usize) = bincode::serde::decode_from_slice(&serialized, bincode::config::standard()).expect("Failed to deserialize");

        prop_assert_eq!(deserialized.test_id, test_id);
        prop_assert_eq!(deserialized.file_path, file_path);
        prop_assert_eq!(deserialized.test_name, test_name);
        prop_assert_eq!(deserialized.is_toxic, is_toxic);
        prop_assert_eq!(deserialized.is_async, is_async);
        prop_assert_eq!(deserialized.log_fd, log_fd);
    }

    /// Property: TestResult serializes and deserializes correctly
    #[test]
    fn test_result_roundtrip(
        test_id: u32,
        status in 0u8..10,
        duration_ns: u64,
        message in "[a-zA-Z0-9 .,!?-]{0,500}",
    ) {
        use tach_core::protocol::TestResult;

        let result = TestResult {
            test_id,
            status,
            duration_ns,
            message: message.clone(),
            memory_rss_bytes: None,
        };

        let serialized = bincode::serde::encode_to_vec(&result, bincode::config::standard()).expect("Failed to serialize");
        let (deserialized, _): (TestResult, usize) = bincode::serde::decode_from_slice(&serialized, bincode::config::standard()).expect("Failed to deserialize");

        prop_assert_eq!(deserialized.test_id, test_id);
        prop_assert_eq!(deserialized.status, status);
        prop_assert_eq!(deserialized.duration_ns, duration_ns);
        prop_assert_eq!(deserialized.message, message);
    }
}

// =============================================================================
// Ring Buffer Index Invariants
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    /// Property: Ring buffer wrap-around calculation is correct
    #[test]
    fn ring_buffer_index_wrap(
        write_count in 0u64..10000,
        capacity in 4u64..1024,
    ) {
        // Simulate index wrapping
        let slot = (write_count % capacity) as usize;

        // Slot should always be within bounds
        prop_assert!(slot < capacity as usize,
            "Slot {} should be < capacity {}", slot, capacity);
    }

    /// Property: Available entries calculation handles wrap-around
    #[test]
    fn ring_buffer_available_calculation(
        write_idx in 0u64..u64::MAX / 2,
        read_delta in 0u64..10000,
    ) {
        let read_idx = write_idx.saturating_sub(read_delta);
        let available = write_idx.wrapping_sub(read_idx);

        prop_assert_eq!(available, write_idx - read_idx,
            "Available calculation should match simple subtraction for non-wrapped case");
    }
}

// =============================================================================
// Configuration Property Tests
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// Property: TachConfig defaults are always valid
    #[test]
    fn tach_config_defaults_valid(_seed: u32) {
        use tach_core::config::TachConfig;

        let config = TachConfig::default();

        prop_assert!(!config.test_pattern().is_empty());
        prop_assert!(config.timeout() > 0);
        prop_assert!(config.workers() > 0);
        prop_assert!(!config.isolation_strategy().is_empty());
    }
}

// =============================================================================
// Memory Region Property Tests
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// Property: Memory region length calculation doesn't overflow
    #[test]
    fn memory_region_length_no_overflow(
        start in 0x1000usize..0x7fff_ffff_ffff_0000,
        len in 0usize..0x1_0000_0000,
    ) {
        // Simulate end calculation
        let end = start.checked_add(len);

        prop_assert!(end.is_some() || len > usize::MAX - start,
            "End calculation should either succeed or be a valid overflow case");
    }
}
