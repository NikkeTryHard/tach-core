//! Fuzz target for CoverageEntry
//!
//! This fuzzer tests that CoverageEntry::line() never panics with any input
//! and always produces correctly flagged entries.

#![no_main]

use libfuzzer_sys::fuzz_target;
use tach_core::coverage::{CoverageEntry, ENTRY_SIZE};

fuzz_target!(|data: (u64, u32)| {
    let (code_id, lineno) = data;

    // Create the entry - should never panic
    let entry = CoverageEntry::line(code_id, lineno);

    // Invariant: code_id is preserved
    assert_eq!(entry.code_id, code_id);

    // Invariant: lineno is preserved
    assert_eq!(entry.lineno, lineno);

    // Invariant: LINE flag is set
    assert_eq!(entry.flags & 0x01, 0x01);

    // Invariant: entry size is exactly ENTRY_SIZE
    assert_eq!(std::mem::size_of_val(&entry), ENTRY_SIZE);
});
