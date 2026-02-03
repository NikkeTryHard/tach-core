//! JUnit XML Reporter for CI Integration
//!
//! Generates JUnit-compatible XML reports for Jenkins, GitLab CI, and GitHub Actions.

use crate::reporter::Reporter;
use serde::Serialize;
use std::fs::File;
use std::io::BufWriter;
use std::path::PathBuf;
use std::time::Instant;

/// Strip ANSI color codes from strings (Boss Refinement #1)
///
/// This function removes ANSI escape sequences (like color codes) from strings
/// to produce clean output for XML reports. It handles:
/// - CSI sequences: `\x1b[...m` (colors, formatting)
/// - OSC sequences: `\x1b]...BEL` or `\x1b]...\x1b\\` (hyperlinks, window titles)
/// - Null bytes: stripped to avoid XML issues
///
/// # Arguments
/// * `s` - The input string potentially containing ANSI escape sequences
///
/// # Returns
/// A new string with all ANSI escape sequences and null bytes removed
#[doc(hidden)] // Public only for fuzz testing, not part of public API
pub fn strip_ansi_codes(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // Skip escape sequence
            if chars.peek() == Some(&'[') {
                // CSI sequence: \x1b[...letter
                chars.next(); // consume '['
                // Skip until we hit a letter
                while let Some(&next) = chars.peek() {
                    chars.next();
                    if next.is_ascii_alphabetic() {
                        break;
                    }
                }
            } else if chars.peek() == Some(&']') {
                // OSC sequence: \x1b]...BEL or \x1b]...\x1b\\
                chars.next(); // consume ']'
                // Skip until BEL (\x07) or ST (\x1b\\)
                while let Some(next) = chars.next() {
                    if next == '\x07' {
                        // BEL terminates OSC
                        break;
                    } else if next == '\x1b' {
                        // Check for ST (\x1b\\)
                        if chars.peek() == Some(&'\\') {
                            chars.next(); // consume '\\'
                            break;
                        }
                    }
                }
            }
            // For other escape sequences (like \x1b= or \x1b>), just skip the ESC byte
        } else if c != '\0' {
            // Skip null bytes
            result.push(c);
        }
    }
    result
}

// =============================================================================
// XML Schema Structs (JUnit Format)
// =============================================================================

#[derive(Serialize)]
#[serde(rename = "testsuites")]
struct TestSuites {
    #[serde(rename = "testsuite")]
    suites: Vec<TestSuite>,
}

#[derive(Serialize)]
struct TestSuite {
    #[serde(rename = "@name")]
    name: String,
    #[serde(rename = "@tests")]
    tests: usize,
    #[serde(rename = "@failures")]
    failures: usize,
    #[serde(rename = "@errors")]
    errors: usize,
    #[serde(rename = "@skipped")]
    skipped: usize,
    #[serde(rename = "@time")]
    time: f64,
    #[serde(rename = "testcase")]
    cases: Vec<TestCase>,
}

#[derive(Serialize)]
struct TestCase {
    #[serde(rename = "@name")]
    name: String,
    #[serde(rename = "@classname")]
    classname: String,
    #[serde(rename = "@time")]
    time: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    failure: Option<Failure>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<Error>,
    #[serde(skip_serializing_if = "Option::is_none")]
    skipped: Option<Skipped>,
}

#[derive(Serialize)]
struct Failure {
    #[serde(rename = "@message")]
    message: String,
    #[serde(rename = "$text")]
    body: String,
}

/// Error struct for error/crash/timeout/harness_error statuses
#[derive(Serialize)]
struct Error {
    #[serde(rename = "@message")]
    message: String,
    #[serde(rename = "$text")]
    body: String,
}

/// Skipped struct with optional message for xfail differentiation
#[derive(Serialize)]
struct Skipped {
    #[serde(rename = "@message", skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

// =============================================================================
// JunitReporter
// =============================================================================

/// Reporter that buffers results and writes JUnit XML on completion
pub struct JunitReporter {
    output_path: PathBuf,
    cases: Vec<TestCase>,
    start_time: Instant,
    error_message: Option<String>,
}

impl JunitReporter {
    pub fn new(path: PathBuf) -> Self {
        Self {
            output_path: path,
            cases: Vec::new(),
            start_time: Instant::now(),
            error_message: None,
        }
    }
}

impl Reporter for JunitReporter {
    fn on_run_start(&mut self, _count: usize) {
        self.start_time = Instant::now();
        self.cases.clear();
        self.error_message = None;
    }

    fn on_test_start(&mut self, _id: &str, _file: &str) {
        // JUnit doesn't have a test_start event - we buffer results
    }

    fn on_test_finished(
        &mut self,
        id: &str,
        status: &str,
        duration_ms: u64,
        message: Option<&str>,
    ) {
        // Parse id "path/to/file.py::test_name" -> classname, name
        let parts: Vec<&str> = id.splitn(2, "::").collect();
        let classname = parts
            .first()
            .unwrap_or(&"unknown")
            .replace('/', ".")
            .replace(".py", "");
        let name = parts.get(1).unwrap_or(&id).to_string();

        // Determine failure, error, and skipped based on status
        let mut failure = None;
        let mut error = None;
        let mut skipped = None;

        match status {
            "pass" => {}
            "skip" => {
                skipped = Some(Skipped { message: None });
            }
            "xfail" => {
                skipped = Some(Skipped {
                    message: Some("Expected Failure (xfail)".to_string()),
                });
            }
            "xpass" => {
                let raw_msg = message.unwrap_or("Test passed unexpectedly");
                let clean_msg = strip_ansi_codes(raw_msg);
                failure = Some(Failure {
                    message: "Unexpected Pass (xpass)".to_string(),
                    body: clean_msg,
                });
            }
            "error" | "crash" | "timeout" | "harness_error" => {
                let raw_msg = message.unwrap_or("Test error");
                let clean_msg = strip_ansi_codes(raw_msg);
                error = Some(Error {
                    message: format!("Test {}", status),
                    body: clean_msg,
                });
            }
            _ => {
                // "fail" or any other status is treated as failure
                let raw_msg = message.unwrap_or("Test failed");
                let clean_msg = strip_ansi_codes(raw_msg);
                failure = Some(Failure {
                    message: "Test failed".to_string(),
                    body: clean_msg,
                });
            }
        }

        self.cases.push(TestCase {
            name,
            classname,
            time: duration_ms as f64 / 1000.0,
            failure,
            error,
            skipped,
        });
    }

    fn on_run_finished(&mut self, passed: usize, failed: usize, skipped: usize, duration_ms: u64) {
        // Calculate errors and failures from buffered cases for accurate counts
        let errors = self.cases.iter().filter(|c| c.error.is_some()).count();
        let failures = self.cases.iter().filter(|c| c.failure.is_some()).count();
        let skipped_count = self.cases.iter().filter(|c| c.skipped.is_some()).count();

        let suite = TestSuite {
            name: "tach".to_string(),
            tests: self.cases.len(),
            failures,
            errors,
            skipped: skipped_count,
            time: duration_ms as f64 / 1000.0,
            cases: std::mem::take(&mut self.cases),
        };

        let root = TestSuites {
            suites: vec![suite],
        };

        // Write to file
        match File::create(&self.output_path) {
            Ok(file) => {
                let mut writer = BufWriter::new(file);
                // Write XML declaration
                use std::io::Write;
                let _ = writer.write_all(b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");

                // Serialize to string first, then write
                match quick_xml::se::to_string(&root) {
                    Ok(xml) => {
                        if let Err(e) = writer.write_all(xml.as_bytes()) {
                            eprintln!("[tach:junit] Failed to write JUnit report: {}", e);
                        } else {
                            eprintln!(
                                "[tach:junit] JUnit report written to {}",
                                self.output_path.display()
                            );
                        }
                    }
                    Err(e) => {
                        eprintln!("[tach:junit] Failed to serialize JUnit report: {}", e);
                    }
                }
            }
            Err(e) => {
                eprintln!("[tach:junit] Failed to create JUnit report: {}", e);
            }
        }
    }

    fn on_error(&mut self, message: &str) {
        self.error_message = Some(message.to_string());
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_ansi_codes() {
        // Color code: "\x1b[31mRed text\x1b[0m"
        let input = "\x1b[31mRed text\x1b[0m";
        let output = strip_ansi_codes(input);
        assert_eq!(output, "Red text");

        // No ANSI codes
        assert_eq!(strip_ansi_codes("plain text"), "plain text");

        // Multiple codes
        let input = "\x1b[1m\x1b[31mBold Red\x1b[0m";
        let output = strip_ansi_codes(input);
        assert_eq!(output, "Bold Red");
    }

    #[test]
    fn test_strip_null_bytes() {
        let input = "text\0with\0nulls";
        let output = strip_ansi_codes(input);
        assert_eq!(output, "textwithnulls");
    }

    #[test]
    fn test_strip_ansi_complex_escape_sequences() {
        // Bold + color + reset
        let input = "\x1b[1;31;40mColored\x1b[0m normal";
        let output = strip_ansi_codes(input);
        assert_eq!(output, "Colored normal");
    }

    #[test]
    fn test_strip_ansi_cursor_movement() {
        // Cursor movement codes
        let input = "\x1b[2Jcleared\x1b[1;1H";
        let output = strip_ansi_codes(input);
        assert_eq!(output, "cleared");
    }

    #[test]
    fn test_junit_reporter_creation() {
        let reporter = JunitReporter::new(PathBuf::from("/tmp/test.xml"));
        assert!(reporter.cases.is_empty());
    }

    #[test]
    fn test_junit_reporter_buffers_tests() {
        let mut reporter = JunitReporter::new(PathBuf::from("/tmp/test.xml"));

        reporter.on_run_start(2);
        reporter.on_test_start("test.py::test_foo", "test.py");
        reporter.on_test_finished("test.py::test_foo", "pass", 42, None);
        reporter.on_test_start("test.py::test_bar", "test.py");
        reporter.on_test_finished("test.py::test_bar", "fail", 100, Some("assertion failed"));

        assert_eq!(reporter.cases.len(), 2);
        assert_eq!(reporter.cases[0].name, "test_foo");
        assert_eq!(reporter.cases[1].name, "test_bar");
        assert!(reporter.cases[0].failure.is_none());
        assert!(reporter.cases[1].failure.is_some());
    }

    #[test]
    fn test_junit_classname_parsing() {
        let mut reporter = JunitReporter::new(PathBuf::from("/tmp/test.xml"));
        reporter.on_run_start(1);
        reporter.on_test_finished("path/to/test_module.py::test_func", "pass", 10, None);

        // path/to/test_module.py -> path.to.test_module
        assert_eq!(reporter.cases[0].classname, "path.to.test_module");
        assert_eq!(reporter.cases[0].name, "test_func");
    }

    #[test]
    fn test_junit_time_conversion() {
        let mut reporter = JunitReporter::new(PathBuf::from("/tmp/test.xml"));
        reporter.on_run_start(1);
        reporter.on_test_finished("test.py::test_a", "pass", 1500, None); // 1500ms = 1.5s

        assert!((reporter.cases[0].time - 1.5).abs() < 0.001);
    }

    #[test]
    fn test_junit_failure_strips_ansi() {
        let mut reporter = JunitReporter::new(PathBuf::from("/tmp/test.xml"));
        reporter.on_run_start(1);
        reporter.on_test_finished(
            "test.py::test_fail",
            "fail",
            50,
            Some("\x1b[31mAssertionError\x1b[0m: expected True"),
        );

        let failure = reporter.cases[0].failure.as_ref().unwrap();
        assert_eq!(failure.body, "AssertionError: expected True");
        assert!(!failure.body.contains("\x1b"));
    }

    #[test]
    fn test_junit_on_error_stores_message() {
        let mut reporter = JunitReporter::new(PathBuf::from("/tmp/test.xml"));
        reporter.on_error("Zygote crashed");
        assert_eq!(reporter.error_message, Some("Zygote crashed".to_string()));
    }

    #[test]
    fn test_junit_run_start_clears_state() {
        let mut reporter = JunitReporter::new(PathBuf::from("/tmp/test.xml"));
        reporter.on_test_finished("test.py::test_a", "pass", 10, None);
        reporter.on_error("some error");

        // Start new run should clear
        reporter.on_run_start(0);
        assert!(reporter.cases.is_empty());
        assert!(reporter.error_message.is_none());
    }

    #[test]
    fn test_junit_skipped_test() {
        let mut reporter = JunitReporter::new(PathBuf::from("/tmp/test.xml"));
        reporter.on_run_start(1);
        reporter.on_test_finished("test.py::test_skip", "skip", 5, None);

        assert_eq!(reporter.cases.len(), 1);
        assert!(reporter.cases[0].failure.is_none());
        assert!(reporter.cases[0].skipped.is_some());
    }

    #[test]
    fn test_junit_all_status_types() {
        let mut reporter = JunitReporter::new(PathBuf::from("/tmp/test.xml"));
        reporter.on_run_start(3);
        reporter.on_test_finished("test.py::test_pass", "pass", 10, None);
        reporter.on_test_finished("test.py::test_fail", "fail", 20, Some("assertion error"));
        reporter.on_test_finished("test.py::test_skip", "skip", 5, None);

        assert_eq!(reporter.cases.len(), 3);

        // Pass: no failure, no error, no skipped
        assert!(reporter.cases[0].failure.is_none());
        assert!(reporter.cases[0].error.is_none());
        assert!(reporter.cases[0].skipped.is_none());

        // Fail: failure present, no error, no skipped
        assert!(reporter.cases[1].failure.is_some());
        assert!(reporter.cases[1].error.is_none());
        assert!(reporter.cases[1].skipped.is_none());

        // Skip: no failure, no error, skipped present (no message)
        assert!(reporter.cases[2].failure.is_none());
        assert!(reporter.cases[2].error.is_none());
        assert!(reporter.cases[2].skipped.is_some());
        assert!(
            reporter.cases[2]
                .skipped
                .as_ref()
                .unwrap()
                .message
                .is_none()
        );
    }

    #[test]
    fn test_junit_xfail_status() {
        let mut reporter = JunitReporter::new(PathBuf::from("/tmp/test.xml"));
        reporter.on_run_start(1);
        reporter.on_test_finished("test.py::test_xfail", "xfail", 10, None);

        assert_eq!(reporter.cases.len(), 1);
        assert!(reporter.cases[0].failure.is_none());
        assert!(reporter.cases[0].error.is_none());
        assert!(reporter.cases[0].skipped.is_some());

        let skipped = reporter.cases[0].skipped.as_ref().unwrap();
        assert_eq!(
            skipped.message,
            Some("Expected Failure (xfail)".to_string())
        );
    }

    #[test]
    fn test_junit_xpass_status() {
        let mut reporter = JunitReporter::new(PathBuf::from("/tmp/test.xml"));
        reporter.on_run_start(1);
        reporter.on_test_finished(
            "test.py::test_xpass",
            "xpass",
            10,
            Some("Test passed unexpectedly"),
        );

        assert_eq!(reporter.cases.len(), 1);
        assert!(reporter.cases[0].skipped.is_none());
        assert!(reporter.cases[0].error.is_none());
        assert!(reporter.cases[0].failure.is_some());

        let failure = reporter.cases[0].failure.as_ref().unwrap();
        assert_eq!(failure.message, "Unexpected Pass (xpass)");
        assert_eq!(failure.body, "Test passed unexpectedly");
    }

    #[test]
    fn test_junit_error_statuses() {
        let mut reporter = JunitReporter::new(PathBuf::from("/tmp/test.xml"));
        reporter.on_run_start(4);
        reporter.on_test_finished("test.py::test_error", "error", 10, Some("RuntimeError"));
        reporter.on_test_finished("test.py::test_crash", "crash", 20, Some("Segfault"));
        reporter.on_test_finished("test.py::test_timeout", "timeout", 30000, Some("Timed out"));
        reporter.on_test_finished(
            "test.py::test_harness",
            "harness_error",
            5,
            Some("Harness failed"),
        );

        assert_eq!(reporter.cases.len(), 4);

        // All should have error, no failure, no skipped
        for case in &reporter.cases {
            assert!(case.failure.is_none());
            assert!(case.skipped.is_none());
            assert!(case.error.is_some());
        }

        // Verify error messages
        assert_eq!(
            reporter.cases[0].error.as_ref().unwrap().message,
            "Test error"
        );
        assert_eq!(
            reporter.cases[0].error.as_ref().unwrap().body,
            "RuntimeError"
        );

        assert_eq!(
            reporter.cases[1].error.as_ref().unwrap().message,
            "Test crash"
        );
        assert_eq!(reporter.cases[1].error.as_ref().unwrap().body, "Segfault");

        assert_eq!(
            reporter.cases[2].error.as_ref().unwrap().message,
            "Test timeout"
        );
        assert_eq!(reporter.cases[2].error.as_ref().unwrap().body, "Timed out");

        assert_eq!(
            reporter.cases[3].error.as_ref().unwrap().message,
            "Test harness_error"
        );
        assert_eq!(
            reporter.cases[3].error.as_ref().unwrap().body,
            "Harness failed"
        );
    }

    #[test]
    fn test_junit_error_count_calculation() {
        let mut reporter = JunitReporter::new(PathBuf::from("/tmp/test.xml"));
        reporter.on_run_start(5);
        reporter.on_test_finished("test.py::test_pass", "pass", 10, None);
        reporter.on_test_finished("test.py::test_fail", "fail", 20, Some("failed"));
        reporter.on_test_finished("test.py::test_error", "error", 30, Some("error"));
        reporter.on_test_finished("test.py::test_timeout", "timeout", 40, Some("timeout"));
        reporter.on_test_finished("test.py::test_skip", "skip", 5, None);

        // Verify counts from buffered cases
        let errors = reporter.cases.iter().filter(|c| c.error.is_some()).count();
        let failures = reporter
            .cases
            .iter()
            .filter(|c| c.failure.is_some())
            .count();
        let skipped = reporter
            .cases
            .iter()
            .filter(|c| c.skipped.is_some())
            .count();

        assert_eq!(errors, 2); // error + timeout
        assert_eq!(failures, 1); // fail
        assert_eq!(skipped, 1); // skip
    }

    // =========================================================================
    // ANSI Stripper Edge Case Tests (Regression Prevention)
    // =========================================================================

    #[test]
    fn test_strip_ansi_incomplete_escape() {
        // Incomplete escape sequence: \x1b[ without a terminating letter
        // This should NOT cause an infinite loop or panic
        let input = "\x1b[";
        let output = strip_ansi_codes(input);
        assert_eq!(
            output, "",
            "Incomplete CSI sequence should produce empty output"
        );

        // Incomplete escape with text before
        let input = "before\x1b[";
        let output = strip_ansi_codes(input);
        assert_eq!(
            output, "before",
            "Text before incomplete sequence should be preserved"
        );

        // Incomplete escape with partial parameters
        let input = "text\x1b[31";
        let output = strip_ansi_codes(input);
        assert_eq!(output, "text", "Partial CSI with digits but no terminator");

        // Incomplete escape with semicolons but no terminator
        let input = "text\x1b[1;2;3";
        let output = strip_ansi_codes(input);
        assert_eq!(output, "text", "CSI with parameters but no terminator");
    }

    #[test]
    fn test_strip_ansi_very_long_sequence() {
        // Very long parameter list (stress test)
        let params = (1..=20)
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join(";");
        let input = format!("\x1b[{}mColored Text\x1b[0m", params);
        let output = strip_ansi_codes(&input);
        assert_eq!(
            output, "Colored Text",
            "Very long CSI sequence should be stripped"
        );

        // Extremely long sequence (100 parameters)
        let long_params = (1..=100)
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join(";");
        let input = format!("prefix\x1b[{}msuffix", long_params);
        let output = strip_ansi_codes(&input);
        assert_eq!(
            output, "prefixsuffix",
            "Extremely long CSI sequence should be handled"
        );
    }

    #[test]
    fn test_strip_ansi_osc_sequences() {
        // OSC (Operating System Command) sequences use \x1b] instead of \x1b[
        // These are used for terminal hyperlinks, window titles, etc.

        // Lone escape without [ or ] - should just strip the escape
        let input = "\x1bhello";
        let output = strip_ansi_codes(input);
        assert_eq!(
            output, "hello",
            "Non-CSI/OSC escape should strip only the ESC byte"
        );

        // OSC sequence terminated by BEL (\x07)
        let input = "\x1b]0;window title\x07normal";
        let output = strip_ansi_codes(input);
        assert_eq!(
            output, "normal",
            "OSC sequence with BEL terminator should be fully stripped"
        );

        // OSC sequence terminated by ST (\x1b\\)
        let input = "\x1b]0;window title\x1b\\normal";
        let output = strip_ansi_codes(input);
        assert_eq!(
            output, "normal",
            "OSC sequence with ST terminator should be fully stripped"
        );

        // Terminal hyperlink (OSC 8)
        let input = "\x1b]8;;https://example.com\x07Click here\x1b]8;;\x07";
        let output = strip_ansi_codes(input);
        assert_eq!(
            output, "Click here",
            "Terminal hyperlink OSC sequences should be stripped"
        );

        // Multiple non-CSI escapes
        let input = "\x1b=\x1b>";
        let output = strip_ansi_codes(input);
        assert_eq!(
            output, "=>",
            "Non-CSI escapes strip only ESC, leave next char"
        );

        // OSC mixed with CSI
        let input = "\x1b]0;title\x07\x1b[31mRed\x1b[0m text";
        let output = strip_ansi_codes(input);
        assert_eq!(
            output, "Red text",
            "Mixed OSC and CSI sequences should both be stripped"
        );

        // Incomplete OSC (no terminator) - consumes to end of string
        let input = "before\x1b]0;incomplete";
        let output = strip_ansi_codes(input);
        assert_eq!(
            output, "before",
            "Incomplete OSC sequence should be stripped to end"
        );
    }

    #[test]
    fn test_strip_ansi_mixed_with_unicode() {
        // Unicode emoji mixed with ANSI codes
        let input = "\x1b[32m✓\x1b[0m passed";
        let output = strip_ansi_codes(input);
        assert_eq!(output, "✓ passed", "Unicode emoji should be preserved");

        // Multi-byte unicode characters
        let input = "\x1b[31m日本語\x1b[0m text";
        let output = strip_ansi_codes(input);
        assert_eq!(output, "日本語 text", "CJK characters should be preserved");

        // Emoji with skin tone modifiers (multi-codepoint)
        let input = "\x1b[33m👋🏽\x1b[0m hello";
        let output = strip_ansi_codes(input);
        assert_eq!(output, "👋🏽 hello", "Complex emoji should be preserved");

        // Mix of unicode and multiple ANSI sequences
        let input = "\x1b[1m\x1b[31mError:\x1b[0m 文件 'test.py' 不存在 ❌";
        let output = strip_ansi_codes(input);
        assert_eq!(
            output, "Error: 文件 'test.py' 不存在 ❌",
            "Mixed unicode and multiple ANSI should work"
        );
    }

    #[test]
    fn test_strip_ansi_escape_at_end() {
        // Trailing escape character (lone \x1b at end)
        let input = "text\x1b";
        let output = strip_ansi_codes(input);
        assert_eq!(output, "text", "Trailing lone ESC should be stripped");

        // Trailing CSI start (\x1b[ at end)
        let input = "text\x1b[";
        let output = strip_ansi_codes(input);
        assert_eq!(output, "text", "Trailing CSI start should be stripped");

        // Trailing partial CSI with digits
        let input = "text\x1b[31";
        let output = strip_ansi_codes(input);
        assert_eq!(output, "text", "Trailing partial CSI should be stripped");
    }

    #[test]
    fn test_strip_ansi_nested_or_overlapping() {
        // Back-to-back CSI sequences (not truly nested, but adjacent)
        let input = "\x1b[1m\x1b[31m\x1b[40mtext\x1b[0m\x1b[0m\x1b[0m";
        let output = strip_ansi_codes(input);
        assert_eq!(
            output, "text",
            "Multiple adjacent sequences should be stripped"
        );

        // Malformed: ESC inside CSI parameters (adversarial input)
        // \x1b[31\x1b[32mtext - second ESC appears in middle of first sequence
        let input = "\x1b[31\x1b[32mtext\x1b[0m";
        let output = strip_ansi_codes(input);
        // The first CSI starts, consumes '3', '1', then hits \x1b
        // \x1b is not alphabetic, so loop continues, consuming \x1b
        // Then '[', '3', '2', 'm' - 'm' is alphabetic, breaks
        // Result: text is preserved
        assert_eq!(
            output, "text",
            "Overlapping CSI sequences should be handled"
        );

        // Empty CSI (no parameters)
        let input = "\x1b[mtext";
        let output = strip_ansi_codes(input);
        assert_eq!(
            output, "text",
            "Empty CSI (just ESC [ m) should be stripped"
        );
    }

    #[test]
    fn test_strip_ansi_adversarial_inputs() {
        // Very long string of just escapes (DoS prevention)
        let input = "\x1b[31m".repeat(1000);
        let output = strip_ansi_codes(&input);
        assert_eq!(
            output, "",
            "Many consecutive ANSI codes should produce empty output"
        );

        // Alternating escape and text
        let input = "a\x1b[1mb\x1b[0mc\x1b[31md\x1b[0m";
        let output = strip_ansi_codes(input);
        assert_eq!(
            output, "abcd",
            "Alternating text and ANSI should preserve text"
        );

        // String of only escape bytes
        let input = "\x1b\x1b\x1b\x1b\x1b";
        let output = strip_ansi_codes(input);
        assert_eq!(output, "", "Multiple lone ESC bytes should all be stripped");

        // Mix of null bytes and ANSI
        let input = "\x00\x1b[31m\x00text\x00\x1b[0m\x00";
        let output = strip_ansi_codes(input);
        assert_eq!(
            output, "text",
            "Null bytes and ANSI should both be stripped"
        );
    }

    #[test]
    fn test_strip_ansi_empty_and_whitespace() {
        // Empty string
        assert_eq!(strip_ansi_codes(""), "", "Empty string should return empty");

        // Only whitespace
        assert_eq!(
            strip_ansi_codes("   "),
            "   ",
            "Whitespace should be preserved"
        );

        // Whitespace with ANSI
        let input = "  \x1b[31m  \x1b[0m  ";
        let output = strip_ansi_codes(input);
        assert_eq!(
            output, "      ",
            "Whitespace around ANSI should be preserved"
        );

        // Newlines and tabs with ANSI
        let input = "\n\t\x1b[32mtext\x1b[0m\n\t";
        let output = strip_ansi_codes(input);
        assert_eq!(
            output, "\n\ttext\n\t",
            "Newlines and tabs should be preserved"
        );
    }
}
