//! Fuzz target for ANSI Escape Sequence Stripper
//!
//! This fuzzer tests that the strip_ansi_codes function handles arbitrary
//! input without panicking and produces valid output.
//!
//! The ANSI stripper is used in JUnit XML report generation to clean up
//! error messages that may contain color codes. It must handle:
//! - Valid CSI sequences: \x1b[...m
//! - Incomplete escape sequences
//! - Non-CSI escapes (OSC, etc.)
//! - Null bytes
//! - Unicode mixed with ANSI
//! - Adversarial/malformed input

#![no_main]

use libfuzzer_sys::fuzz_target;
use tach_core::reporting::junit::strip_ansi_codes;

/// Validate invariants on the output of strip_ansi_codes
fn validate_output(input: &str, output: &str) {
    // Invariant 1: Output should NEVER contain ESC bytes (0x1b)
    assert!(!output.contains('\x1b'), "Output should not contain ESC bytes. Input: {:?}, Output: {:?}", input, output);

    // Invariant 2: Output should NEVER contain null bytes (0x00)
    assert!(!output.contains('\0'), "Output should not contain null bytes. Input: {:?}, Output: {:?}", input, output);

    // Invariant 3: Output length should be <= input length
    // (we only remove characters, never add)
    assert!(output.len() <= input.len(), "Output length {} should be <= input length {}", output.len(), input.len());

    // Invariant 4: Output should be valid UTF-8 (guaranteed by Rust String,
    // but good to verify we didn't break any UTF-8 sequences)
    // This is implicitly verified by String type, but we can check char count
    let output_char_count = output.chars().count();
    assert!(output_char_count <= input.chars().count(), "Output char count {} should be <= input char count {}", output_char_count, input.chars().count());

    // Invariant 5: All printable ASCII chars that aren't part of ANSI sequences
    // should be preserved (heuristic check)
    for c in output.chars() {
        // Output characters should be valid (not control chars except whitespace)
        if c.is_ascii() && !c.is_ascii_whitespace() {
            assert!(c.is_ascii_graphic() || c == ' ', "Unexpected ASCII control character in output: {:?}", c);
        }
    }
}

fuzz_target!(|data: &[u8]| {
    // Test 1: UTF-8 input
    if let Ok(input) = std::str::from_utf8(data) {
        // strip_ansi_codes should NEVER panic
        let output = strip_ansi_codes(input);

        // Validate output invariants
        validate_output(input, &output);

        // Heuristic: if input has no ESC or null bytes, output should match
        if !input.contains('\x1b') && !input.contains('\0') {
            assert_eq!(input, output, "Input without ESC/null should be unchanged");
        }
    }

    // Test 2: Lossy UTF-8 conversion (simulates real-world encoding issues)
    let lossy_input = String::from_utf8_lossy(data);
    let output = strip_ansi_codes(&lossy_input);
    validate_output(&lossy_input, &output);

    // Test 3: Specific adversarial patterns
    if data.len() >= 2 {
        // Create a string with many escape sequences
        let mut adversarial = String::new();
        for chunk in data.chunks(2) {
            adversarial.push('\x1b');
            adversarial.push('[');
            for &b in chunk {
                if b.is_ascii() && b != 0 {
                    adversarial.push(b as char);
                }
            }
            adversarial.push('m');
        }
        adversarial.push_str("text");

        let output = strip_ansi_codes(&adversarial);
        validate_output(&adversarial, &output);

        // The word "text" should be preserved
        assert!(output.contains("text"), "Preserved content should remain: output={:?}", output);
    }

    // Test 4: Incomplete sequences (trailing ESC)
    if let Ok(input) = std::str::from_utf8(data) {
        let with_trailing_esc = format!("{}\x1b", input);
        let output = strip_ansi_codes(&with_trailing_esc);
        validate_output(&with_trailing_esc, &output);

        // Trailing ESC should be stripped
        assert!(!output.ends_with('\x1b'), "Trailing ESC should be stripped");
    }

    // Test 5: Incomplete CSI (ESC [)
    if let Ok(input) = std::str::from_utf8(data) {
        let with_incomplete_csi = format!("{}\x1b[", input);
        let output = strip_ansi_codes(&with_incomplete_csi);
        validate_output(&with_incomplete_csi, &output);
    }
});
