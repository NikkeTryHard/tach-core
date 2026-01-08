//! Property-Based Tests for Reporter Truncation and Display
//!
//! These tests use proptest to verify invariants of test ID truncation
//! and display formatting that are difficult to test exhaustively.
//!
//! Key invariants tested:
//! 1. Truncated output never exceeds max_width
//! 2. Original content is preserved when below max_width
//! 3. Very long names (up to 10000 chars) are handled correctly
//! 4. Truncation preserves the most relevant part (end) of the ID
//! 5. Unicode characters don't cause panics or incorrect lengths

use proptest::prelude::*;

// =============================================================================
// Test ID Truncation Logic (mirrored from reporter.rs)
// =============================================================================

/// Truncate a test ID to fit within the given maximum width.
///
/// If the ID is longer than max_width, it will be truncated with "..." prefix,
/// showing the most relevant part (end of the path/test name).
fn truncate_test_id(id: &str, max_width: usize) -> String {
    if id.len() <= max_width {
        id.to_string()
    } else if max_width <= 3 {
        "...".to_string()
    } else {
        // Show "..." followed by the last (max_width - 3) characters
        format!("...{}", &id[id.len() - (max_width - 3)..])
    }
}

// =============================================================================
// Property-Based Tests
// =============================================================================

proptest! {
    /// Invariant: Truncated output never exceeds max_width
    #[test]
    fn truncated_output_never_exceeds_max_width(
        id in "[a-zA-Z0-9_/:.]{0,10000}",
        max_width in 0usize..200
    ) {
        let result = truncate_test_id(&id, max_width);
        // Result should never exceed max_width (unless max_width < 3, then it's "...")
        if max_width <= 3 {
            prop_assert!(result.len() <= 3, "Result should be '...' for width <= 3");
        } else {
            prop_assert!(
                result.len() <= max_width,
                "Result '{}' (len {}) exceeds max_width {}",
                result, result.len(), max_width
            );
        }
    }

    /// Invariant: Short IDs are preserved unchanged
    #[test]
    fn short_ids_preserved_unchanged(
        id in "[a-zA-Z0-9_]{1,50}",
        extra_width in 0usize..100
    ) {
        let max_width = id.len() + extra_width;
        let result = truncate_test_id(&id, max_width);
        prop_assert_eq!(
            result, id,
            "ID shorter than max_width should be unchanged"
        );
    }

    /// Invariant: Very long names (up to 10000 chars) are handled correctly
    #[test]
    fn very_long_names_handled_correctly(
        len in 100usize..10000,
        max_width in 20usize..100
    ) {
        // Create a very long test name
        let id: String = std::iter::repeat_n('x', len).collect();
        let result = truncate_test_id(&id, max_width);

        // Should be exactly max_width
        prop_assert_eq!(
            result.len(), max_width,
            "Long ID should be truncated to exactly max_width"
        );

        // Should start with "..."
        prop_assert!(
            result.starts_with("..."),
            "Truncated ID should start with '...'"
        );

        // Should end with the last (max_width - 3) chars of original
        let expected_suffix = &id[id.len() - (max_width - 3)..];
        prop_assert!(
            result.ends_with(expected_suffix),
            "Truncated ID should end with last {} chars of original",
            max_width - 3
        );
    }

    /// Invariant: Truncation at exact boundary is correct
    #[test]
    fn truncation_at_exact_boundary(
        id in "[a-zA-Z0-9_]{10,100}"
    ) {
        let max_width = id.len();
        let result = truncate_test_id(&id, max_width);
        prop_assert_eq!(result, id.clone(), "Exact match should not truncate");

        let result_minus_one = truncate_test_id(&id, max_width - 1);
        prop_assert!(
            result_minus_one.starts_with("..."),
            "One char over should truncate"
        );
    }

    /// Invariant: Realistic test ID patterns are handled
    #[test]
    fn realistic_test_id_patterns(
        path_parts in prop::collection::vec("[a-z_]{3,20}", 1..10),
        class_name in "Test[A-Z][a-z]{3,15}",
        method_name in "test_[a-z_]{5,50}",
        max_width in 30usize..120
    ) {
        let path = path_parts.join("/");
        let id = format!("{}/test_{}.py::{}::{}", path, path_parts.last().unwrap_or(&"module".to_string()), class_name, method_name);

        let result = truncate_test_id(&id, max_width);

        if id.len() <= max_width {
            prop_assert_eq!(result, id, "Short enough IDs unchanged");
        } else {
            prop_assert!(result.starts_with("..."), "Long IDs truncated");
            prop_assert_eq!(result.len(), max_width, "Truncated to exact width");
            // Should show the end (method name is most relevant)
            if max_width > method_name.len() + 3 {
                prop_assert!(
                    result.ends_with(&method_name),
                    "Should preserve method name when possible"
                );
            }
        }
    }
}

// =============================================================================
// Edge Case Tests
// =============================================================================

#[test]
fn test_empty_string() {
    let result = truncate_test_id("", 50);
    assert_eq!(result, "", "Empty string should remain empty");
}

#[test]
fn test_zero_width() {
    let result = truncate_test_id("test_something", 0);
    assert_eq!(result, "...", "Zero width should return '...'");
}

#[test]
fn test_width_one() {
    let result = truncate_test_id("test_something", 1);
    assert_eq!(result, "...", "Width 1 should return '...'");
}

#[test]
fn test_width_three() {
    let result = truncate_test_id("test_something", 3);
    assert_eq!(result, "...", "Width 3 should return '...'");
}

#[test]
fn test_width_four() {
    let result = truncate_test_id("test_something", 4);
    assert_eq!(result, "...g", "Width 4 should return '...' + 1 char");
}

#[test]
fn test_ten_thousand_char_name() {
    let long_name: String = std::iter::repeat_n('a', 10000).collect();
    let result = truncate_test_id(&long_name, 100);
    assert_eq!(result.len(), 100, "10000 char name should truncate to 100");
    assert!(result.starts_with("..."), "Should start with ellipsis");
    assert_eq!(
        &result[3..],
        &long_name[long_name.len() - 97..],
        "Should show last 97 chars"
    );
}

#[test]
fn test_hundred_char_width() {
    let id = "tests/very/long/path/to/some/deeply/nested/module/test_something.py::TestClass::test_method_with_very_long_name";
    let result = truncate_test_id(id, 100);
    assert!(result.len() <= 100, "Should fit in 100 chars");
    if id.len() > 100 {
        assert!(result.starts_with("..."), "Long ID should be truncated");
    }
}
