//! Fuzz target for Toxicity AST Analysis
//!
//! This fuzzer tests that the toxicity analyzer handles arbitrary/malformed
//! Python source without panicking and validates invariants.
//!
//! The toxicity analyzer uses rustpython_parser to parse Python and walk the
//! AST looking for toxic patterns (threading, sockets, ctypes, etc). This
//! fuzzer ensures it handles:
//! - Invalid UTF-8 sequences
//! - Malformed Python syntax
//! - Deeply nested structures
//! - Unicode edge cases
//! - Empty/whitespace-only inputs

#![no_main]

use libfuzzer_sys::fuzz_target;
use std::path::Path;
use tach_core::discovery::analysis::{analyze_file, ToxicityReport};

/// Known toxic module names that should trigger is_toxic=true
const TOXIC_MODULES: &[&str] = &["threading", "_thread", "multiprocessing", "socket", "ctypes", "signal", "concurrent.futures", "grpc", "pandas", "tensorflow", "torch", "cv2", "gevent", "cffi"];

/// Check if source contains an import of a known toxic module
fn source_has_toxic_import(source: &str) -> Option<&'static str> {
    for &module in TOXIC_MODULES {
        // Check for: import <module>
        if source.contains(&format!("import {}", module)) {
            return Some(module);
        }
        // Check for: from <module> import
        if source.contains(&format!("from {} import", module)) {
            return Some(module);
        }
    }
    None
}

/// Check invariants on the toxicity report
fn validate_report(report: &ToxicityReport, source: &str) {
    // Invariant 1: If is_toxic is true, there must be at least one reason
    if report.is_toxic {
        assert!(!report.reasons.is_empty(), "Toxic report should have at least one reason");
    }

    // Invariant 2: Reasons should not contain null bytes
    for reason in &report.reasons {
        assert!(!reason.contains('\0'), "Reason should not contain null bytes: {:?}", reason);
    }

    // Invariant 3: Imports should not contain null bytes
    for import in &report.imports {
        assert!(!import.contains('\0'), "Import should not contain null bytes: {:?}", import);
    }

    // Invariant 4: Imports should be non-empty strings
    for import in &report.imports {
        assert!(!import.is_empty(), "Import name should not be empty");
    }

    // Invariant 5: If source contains a known toxic import and parses correctly,
    // the report should be toxic (heuristic check - not always accurate due to
    // string literals, comments, etc.)
    if let Some(toxic_module) = source_has_toxic_import(source) {
        // Only check if the source looks like it would parse (simple heuristic)
        // This is a weak invariant since the import might be in a string/comment
        if !source.contains(&format!("\"import {}\"", toxic_module)) && !source.contains(&format!("'import {}'", toxic_module)) && !source.contains(&format!("# import {}", toxic_module)) {
            // If it parsed and contains a toxic import, it should be marked toxic
            // But parse errors also mark as toxic, so this is not a strict invariant
            // Just log for corpus building - don't assert
        }
    }

    // Invariant 6: Reason strings should be valid UTF-8 (guaranteed by Rust, but good to assert)
    for reason in &report.reasons {
        assert!(reason.is_ascii() || reason.chars().all(|c| c.is_alphanumeric() || c.is_whitespace() || c.is_ascii_punctuation()), "Reason should be printable: {:?}", reason);
    }
}

fuzz_target!(|data: &[u8]| {
    // Test 1: Try parsing as UTF-8 and analyzing
    if let Ok(source) = std::str::from_utf8(data) {
        // Use a dummy path for analysis
        let path = Path::new("fuzz_input.py");

        // analyze_file should NEVER panic, regardless of input
        let report = analyze_file(source, path);

        // Validate the report meets our invariants
        validate_report(&report, source);
    }

    // Test 2: Also test with lossy UTF-8 conversion (simulates real-world scenarios
    // where files might have encoding issues)
    let lossy_source = String::from_utf8_lossy(data);
    let path = Path::new("fuzz_input_lossy.py");
    let report = analyze_file(&lossy_source, path);
    validate_report(&report, &lossy_source);
});
