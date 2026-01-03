//! Fuzz target for MappingEntry
//!
//! This fuzzer tests that MappingEntry::new() never panics with any input
//! and always produces valid UTF-8 output.

#![no_main]

use libfuzzer_sys::fuzz_target;
use tach_core::coverage::MappingEntry;

fuzz_target!(|data: (u64, &str)| {
    let (code_id, filename) = data;

    // Create the entry - should never panic
    let entry = MappingEntry::new(code_id, filename);

    // Invariant: code_id is preserved
    assert_eq!(entry.code_id, code_id);

    // Invariant: filename_len is within bounds
    assert!(entry.filename_len <= 240);

    // Invariant: filename() returns valid UTF-8
    let result = entry.filename();
    assert!(result.len() <= 240);

    // Invariant: UTF-8 validity
    assert!(!result.contains('\u{FFFD}'), "Should not contain replacement character");
});
