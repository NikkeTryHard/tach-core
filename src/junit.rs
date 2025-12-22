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
fn strip_ansi_codes(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // Skip escape sequence
            if chars.peek() == Some(&'[') {
                chars.next(); // consume '['
                              // Skip until we hit a letter
                while let Some(&next) = chars.peek() {
                    chars.next();
                    if next.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
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
}

#[derive(Serialize)]
struct Failure {
    #[serde(rename = "@message")]
    message: String,
    #[serde(rename = "$text")]
    body: String,
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

        let failure = if status != "pass" {
            let raw_msg = message.unwrap_or("Test failed");
            let clean_msg = strip_ansi_codes(raw_msg);
            Some(Failure {
                message: "Test failed".to_string(),
                body: clean_msg,
            })
        } else {
            None
        };

        self.cases.push(TestCase {
            name,
            classname,
            time: duration_ms as f64 / 1000.0,
            failure,
        });
    }

    fn on_run_finished(&mut self, passed: usize, failed: usize, skipped: usize, duration_ms: u64) {
        let suite = TestSuite {
            name: "tach".to_string(),
            tests: passed + failed + skipped,
            failures: failed,
            errors: 0,
            skipped,
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
                            eprintln!("[tach] Failed to write JUnit report: {}", e);
                        } else {
                            eprintln!(
                                "[tach] JUnit report written to {}",
                                self.output_path.display()
                            );
                        }
                    }
                    Err(e) => {
                        eprintln!("[tach] Failed to serialize JUnit report: {}", e);
                    }
                }
            }
            Err(e) => {
                eprintln!("[tach] Failed to create JUnit report: {}", e);
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
}
