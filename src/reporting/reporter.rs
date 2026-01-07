//! Reporter Module: Trait-based output for Human (CLI) and Machine (JSON) formats
//!
//!  Machine Interface for IDE/CI integration.
//!
//! ## Architecture
//!
//! - `Reporter` trait defines the event callbacks
//! - `JsonReporter` outputs NDJSON to stdout (for --format=json)
//! - `HumanReporter` outputs human-readable text to stderr
//!
//! ## Boss Refinement: Stdout Purity
//!
//! When JsonReporter is active, ONLY valid JSON goes to stdout.
//! All other output (logs, errors, debug) must go to stderr.

use serde::Serialize;

// =============================================================================
// Helper Functions
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

/// Get the current terminal width, with a fallback default.
fn get_terminal_width() -> usize {
    terminal_size::terminal_size()
        .map(|(w, _)| w.0 as usize)
        .unwrap_or(80)
}

/// Machine-readable events for JSON output
#[derive(Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum MachineEvent<'a> {
    /// Emitted at start of test run
    RunStart { count: usize },
    /// Emitted when a test begins execution
    TestStart { id: &'a str, file: &'a str },
    /// Emitted when a test completes
    TestFinished {
        id: &'a str,
        status: &'a str, // "pass", "fail", "skip"
        duration_ms: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        message: Option<&'a str>,
    },
    /// Emitted at end of test run
    RunFinished {
        passed: usize,
        failed: usize,
        skipped: usize,
        duration_ms: u64,
    },
    /// Emitted on fatal error (Boss Refinement #2)
    Error { message: &'a str },
}

/// Reporter trait for output abstraction
pub trait Reporter {
    /// Called at start of test run
    fn on_run_start(&mut self, count: usize);

    /// Called when a test begins execution
    fn on_test_start(&mut self, id: &str, file: &str);

    /// Called when a test completes
    fn on_test_finished(&mut self, id: &str, status: &str, duration_ms: u64, message: Option<&str>);

    /// Called at end of test run
    fn on_run_finished(&mut self, passed: usize, failed: usize, skipped: usize, duration_ms: u64);

    /// Called on fatal error (Boss Refinement #2)
    fn on_error(&mut self, message: &str);
}

/// JSON Reporter - outputs NDJSON to stdout
pub struct JsonReporter;

impl Reporter for JsonReporter {
    fn on_run_start(&mut self, count: usize) {
        let event = MachineEvent::RunStart { count };
        // ONLY JsonReporter touches stdout
        if let Ok(json) = serde_json::to_string(&event) {
            println!("{}", json);
        }
    }

    fn on_test_start(&mut self, id: &str, file: &str) {
        let event = MachineEvent::TestStart { id, file };
        if let Ok(json) = serde_json::to_string(&event) {
            println!("{}", json);
        }
    }

    fn on_test_finished(
        &mut self,
        id: &str,
        status: &str,
        duration_ms: u64,
        message: Option<&str>,
    ) {
        let event = MachineEvent::TestFinished {
            id,
            status,
            duration_ms,
            message,
        };
        if let Ok(json) = serde_json::to_string(&event) {
            println!("{}", json);
        }
    }

    fn on_run_finished(&mut self, passed: usize, failed: usize, skipped: usize, duration_ms: u64) {
        let event = MachineEvent::RunFinished {
            passed,
            failed,
            skipped,
            duration_ms,
        };
        if let Ok(json) = serde_json::to_string(&event) {
            println!("{}", json);
        }
    }

    fn on_error(&mut self, message: &str) {
        let event = MachineEvent::Error { message };
        if let Ok(json) = serde_json::to_string(&event) {
            println!("{}", json);
        }
    }
}

/// Human Reporter - outputs readable text to stderr
pub struct HumanReporter {
    /// Maximum width for test names (based on terminal width)
    max_name_width: usize,
}

impl HumanReporter {
    /// Create a new human reporter with terminal width detection
    pub fn new() -> Self {
        // Get terminal width, default to 80 if not available
        let term_width = terminal_size::terminal_size()
            .map(|(w, _)| w.0 as usize)
            .unwrap_or(80);
        // Reserve space for "  " prefix, " ... " suffix, and result (20 chars)
        let max_name_width = term_width.saturating_sub(30).max(20);
        Self { max_name_width }
    }

    /// Truncate a test ID if it exceeds the maximum width
    fn truncate_id(&self, id: &str) -> String {
        truncate_test_id(id, self.max_name_width)
    }
}

impl Default for HumanReporter {
    fn default() -> Self {
        Self::new()
    }
}

impl Reporter for HumanReporter {
    fn on_run_start(&mut self, count: usize) {
        eprintln!("[tach] Running {} tests...\n", count);
    }

    fn on_test_start(&mut self, id: &str, _file: &str) {
        let display_id = self.truncate_id(id);
        eprint!("  {} ... ", display_id);
    }

    fn on_test_finished(
        &mut self,
        _id: &str,
        status: &str,
        duration_ms: u64,
        message: Option<&str>,
    ) {
        match status {
            "pass" => eprintln!("ok ({}ms)", duration_ms),
            "fail" => {
                eprintln!("FAILED ({}ms)", duration_ms);
                if let Some(msg) = message {
                    // Indent failure message
                    for line in msg.lines().take(10) {
                        eprintln!("    {}", line);
                    }
                }
            }
            "skip" => eprintln!("skipped"),
            _ => eprintln!("{}", status),
        }
    }

    fn on_run_finished(&mut self, passed: usize, failed: usize, skipped: usize, duration_ms: u64) {
        eprintln!();
        eprintln!(
            "[tach] {} passed, {} failed, {} skipped in {}ms",
            passed, failed, skipped, duration_ms
        );
    }

    fn on_error(&mut self, message: &str) {
        eprintln!("[tach] FATAL ERROR: {}", message);
    }
}

// =============================================================================
// MultiReporter
// =============================================================================

/// MultiReporter - broadcasts events to multiple reporters
pub struct MultiReporter {
    reporters: Vec<Box<dyn Reporter>>,
}

impl MultiReporter {
    pub fn new(reporters: Vec<Box<dyn Reporter>>) -> Self {
        Self { reporters }
    }
}

impl Reporter for MultiReporter {
    fn on_run_start(&mut self, count: usize) {
        for r in &mut self.reporters {
            r.on_run_start(count);
        }
    }

    fn on_test_start(&mut self, id: &str, file: &str) {
        for r in &mut self.reporters {
            r.on_test_start(id, file);
        }
    }

    fn on_test_finished(
        &mut self,
        id: &str,
        status: &str,
        duration_ms: u64,
        message: Option<&str>,
    ) {
        for r in &mut self.reporters {
            r.on_test_finished(id, status, duration_ms, message);
        }
    }

    fn on_run_finished(&mut self, passed: usize, failed: usize, skipped: usize, duration_ms: u64) {
        for r in &mut self.reporters {
            r.on_run_finished(passed, failed, skipped, duration_ms);
        }
    }

    fn on_error(&mut self, message: &str) {
        for r in &mut self.reporters {
            r.on_error(message);
        }
    }
}

// =============================================================================
//  Progress Bar Reporter
// =============================================================================

use indicatif::{ProgressBar, ProgressStyle};

/// Record of a test failure for summary display
struct FailureRecord {
    id: String,
    message: String,
}

/// Progress bar reporter with failure buffering
///
/// Displays an interactive progress bar during test execution.
/// Failures are buffered and displayed in a summary at the end.
pub struct ProgressReporter {
    bar: ProgressBar,
    passed: usize,
    failed: usize,
    skipped: usize,
    failures: Vec<FailureRecord>,
    total: usize,
    /// Maximum width for test IDs in messages
    max_id_width: usize,
}

impl ProgressReporter {
    /// Create a new progress reporter with terminal-responsive layout
    pub fn new() -> Self {
        let term_width = get_terminal_width();

        // Calculate responsive bar width:
        // - Spinner + space: 2
        // - [elapsed_precise]: 12 (HH:MM:SS.mmm in brackets)
        // - Space: 1
        // - [bar]: variable
        // - Space: 1
        // - pos/len: ~10
        // - Space: 1
        // - msg: ~20
        // Total overhead: ~47 chars
        let bar_width = if term_width < 60 {
            // Very narrow: minimal bar
            10
        } else if term_width < 80 {
            // Narrow: small bar
            20
        } else if term_width < 120 {
            // Normal: medium bar
            40
        } else {
            // Wide: larger bar
            60
        };

        // Calculate max ID width for truncation (for messages)
        let max_id_width = term_width.saturating_sub(50).max(20);

        let bar = ProgressBar::new(0);
        let template = format!("{{spinner:.green}} [{{elapsed_precise}}] [{{bar:{}.cyan/blue}}] {{pos}}/{{len}} {{msg}}", bar_width);
        bar.set_style(
            ProgressStyle::default_bar()
                .template(&template)
                .unwrap_or_else(|_| ProgressStyle::default_bar())
                .progress_chars("=>-"),
        );

        Self {
            bar,
            passed: 0,
            failed: 0,
            skipped: 0,
            failures: Vec::new(),
            total: 0,
            max_id_width,
        }
    }

    /// Check if we should use progress bar (interactive terminal)
    pub fn should_use_progress_bar() -> bool {
        atty::is(atty::Stream::Stderr) && std::env::var("CI").is_err()
    }
}

impl Default for ProgressReporter {
    fn default() -> Self {
        Self::new()
    }
}

impl Reporter for ProgressReporter {
    fn on_run_start(&mut self, count: usize) {
        self.total = count;
        self.bar.set_length(count as u64);
        self.bar.set_message("Starting...");
    }

    fn on_test_start(&mut self, id: &str, _file: &str) {
        // Truncate long test IDs using instance's max width
        let display_id = truncate_test_id(id, self.max_id_width);
        self.bar.set_message(display_id);
    }

    fn on_test_finished(
        &mut self,
        id: &str,
        status: &str,
        _duration_ms: u64,
        message: Option<&str>,
    ) {
        match status {
            "pass" => self.passed += 1,
            "fail" => {
                self.failed += 1;
                // Buffer failure for summary
                self.failures.push(FailureRecord {
                    id: id.to_string(),
                    message: message.unwrap_or("").to_string(),
                });
            }
            "skip" => self.skipped += 1,
            _ => {}
        }

        self.bar.inc(1);
        self.bar.set_message(format!(
            "P:{} F:{} S:{}",
            self.passed, self.failed, self.skipped
        ));
    }

    fn on_run_finished(&mut self, passed: usize, failed: usize, skipped: usize, duration_ms: u64) {
        self.bar.finish_and_clear();

        // Print failure details
        if !self.failures.is_empty() {
            eprintln!("\n{} FAILURES {}", "=".repeat(30), "=".repeat(30));
            for failure in &self.failures {
                eprintln!("\n{}", failure.id);
                eprintln!("{}", "-".repeat(failure.id.len().min(70)));
                // Limit failure message to 20 lines
                for line in failure.message.lines().take(20) {
                    eprintln!("{}", line);
                }
            }
            eprintln!("{}", "=".repeat(70));
        }

        // Print summary with colors
        let duration_secs = duration_ms as f64 / 1000.0;
        if failed > 0 {
            eprintln!(
                "\n\x1b[31m{} passed, {} failed, {} skipped in {:.2}s\x1b[0m",
                passed, failed, skipped, duration_secs
            );
        } else {
            eprintln!(
                "\n\x1b[32m{} passed, {} failed, {} skipped in {:.2}s\x1b[0m",
                passed, failed, skipped, duration_secs
            );
        }

        // Time Saved metric: compare actual time vs estimated cold pytest time
        // Baseline: 300ms per test (typical pytest startup + import overhead)
        let total_tests = passed + failed + skipped;
        let estimated_cold_ms = total_tests as u64 * 300;
        let time_saved_ms = estimated_cold_ms.saturating_sub(duration_ms);

        if time_saved_ms > 1000 && total_tests > 10 {
            let saved_secs = time_saved_ms as f64 / 1000.0;
            if saved_secs >= 60.0 {
                let mins = (saved_secs / 60.0).floor() as u64;
                let secs = saved_secs % 60.0;
                eprintln!(
                    "\x1b[36m(Saved {}m {:.0}s of initialization overhead)\x1b[0m",
                    mins, secs
                );
            } else {
                eprintln!(
                    "\x1b[36m(Saved {:.1}s of initialization overhead)\x1b[0m",
                    saved_secs
                );
            }
        }
    }

    fn on_error(&mut self, message: &str) {
        self.bar.abandon_with_message(format!("ERROR: {}", message));
    }
}

// =============================================================================
//  Dots Reporter (CI Fallback)
// =============================================================================

/// Simple dots reporter for CI environments
///
/// Outputs a single character per test:
/// - `.` for pass
/// - `F` for fail
/// - `s` for skip
///
/// Failures are buffered and displayed in a summary at the end.
pub struct DotsReporter {
    passed: usize,
    failed: usize,
    skipped: usize,
    failures: Vec<FailureRecord>,
    column: usize,
}

impl DotsReporter {
    /// Create a new dots reporter
    pub fn new() -> Self {
        Self {
            passed: 0,
            failed: 0,
            skipped: 0,
            failures: Vec::new(),
            column: 0,
        }
    }

    /// Print a character and wrap at 80 columns
    fn print_char(&mut self, c: char) {
        eprint!("{}", c);
        self.column += 1;
        if self.column >= 80 {
            eprintln!();
            self.column = 0;
        }
    }
}

impl Default for DotsReporter {
    fn default() -> Self {
        Self::new()
    }
}

impl Reporter for DotsReporter {
    fn on_run_start(&mut self, count: usize) {
        eprintln!("[tach] Running {} tests...\n", count);
    }

    fn on_test_start(&mut self, _id: &str, _file: &str) {
        // No output on test start
    }

    fn on_test_finished(
        &mut self,
        id: &str,
        status: &str,
        _duration_ms: u64,
        message: Option<&str>,
    ) {
        match status {
            "pass" => {
                self.passed += 1;
                self.print_char('.');
            }
            "fail" => {
                self.failed += 1;
                self.print_char('F');
                // Buffer failure for summary
                self.failures.push(FailureRecord {
                    id: id.to_string(),
                    message: message.unwrap_or("").to_string(),
                });
            }
            "skip" => {
                self.skipped += 1;
                self.print_char('s');
            }
            _ => {
                self.print_char('?');
            }
        }
    }

    fn on_run_finished(&mut self, passed: usize, failed: usize, skipped: usize, duration_ms: u64) {
        // Finish the line if we have any output
        if self.column > 0 {
            eprintln!();
        }

        // Print failure details
        if !self.failures.is_empty() {
            eprintln!("\n{} FAILURES {}", "=".repeat(30), "=".repeat(30));
            for failure in &self.failures {
                eprintln!("\n{}", failure.id);
                eprintln!("{}", "-".repeat(failure.id.len().min(70)));
                // Limit failure message to 20 lines
                for line in failure.message.lines().take(20) {
                    eprintln!("{}", line);
                }
            }
            eprintln!("{}", "=".repeat(70));
        }

        // Print summary
        let duration_secs = duration_ms as f64 / 1000.0;
        eprintln!(
            "\n[tach] {} passed, {} failed, {} skipped in {:.2}s",
            passed, failed, skipped, duration_secs
        );

        // Time Saved metric: compare actual time vs estimated cold pytest time
        // Baseline: 300ms per test (typical pytest startup + import overhead)
        let total_tests = passed + failed + skipped;
        let estimated_cold_ms = total_tests as u64 * 300;
        let time_saved_ms = estimated_cold_ms.saturating_sub(duration_ms);

        if time_saved_ms > 1000 && total_tests > 10 {
            let saved_secs = time_saved_ms as f64 / 1000.0;
            if saved_secs >= 60.0 {
                let mins = (saved_secs / 60.0).floor() as u64;
                let secs = saved_secs % 60.0;
                eprintln!(
                    "[tach] (Saved {}m {:.0}s of initialization overhead)",
                    mins, secs
                );
            } else {
                eprintln!(
                    "[tach] (Saved {:.1}s of initialization overhead)",
                    saved_secs
                );
            }
        }
    }

    fn on_error(&mut self, message: &str) {
        if self.column > 0 {
            eprintln!();
            self.column = 0;
        }
        eprintln!("[tach] FATAL ERROR: {}", message);
    }
}

// =============================================================================
// Unit Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_json_event_serialization() {
        let event = MachineEvent::TestFinished {
            id: "test_foo",
            status: "pass",
            duration_ms: 42,
            message: None,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"event\":\"test_finished\""));
        assert!(json.contains("\"id\":\"test_foo\""));
        assert!(json.contains("\"status\":\"pass\""));
        assert!(!json.contains("message")); // skip_serializing_if = None
    }

    #[test]
    fn test_json_event_with_message() {
        let event = MachineEvent::TestFinished {
            id: "test_bar",
            status: "fail",
            duration_ms: 100,
            message: Some("assertion failed"),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"message\":\"assertion failed\""));
    }

    #[test]
    fn test_error_event() {
        let event = MachineEvent::Error {
            message: "Zygote died unexpectedly",
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"event\":\"error\""));
    }

    #[test]
    fn test_run_start_event() {
        let event = MachineEvent::RunStart { count: 42 };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"event\":\"run_start\""));
        assert!(json.contains("\"count\":42"));
    }

    #[test]
    fn test_run_finished_event() {
        let event = MachineEvent::RunFinished {
            passed: 10,
            failed: 2,
            skipped: 1,
            duration_ms: 5000,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"event\":\"run_finished\""));
        assert!(json.contains("\"passed\":10"));
        assert!(json.contains("\"failed\":2"));
        assert!(json.contains("\"skipped\":1"));
    }

    #[test]
    fn test_test_start_event() {
        let event = MachineEvent::TestStart {
            id: "test_foo.py::test_bar",
            file: "test_foo.py",
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"event\":\"test_start\""));
        assert!(json.contains("\"file\":\"test_foo.py\""));
    }

    #[test]
    fn test_multi_reporter_creation() {
        let reporters: Vec<Box<dyn Reporter>> =
            vec![Box::new(HumanReporter::new()), Box::new(JsonReporter)];
        let _multi = MultiReporter::new(reporters);
        // Should compile and not panic
    }

    #[test]
    fn test_multi_reporter_empty() {
        let reporters: Vec<Box<dyn Reporter>> = vec![];
        let mut multi = MultiReporter::new(reporters);
        // Operations should not panic on empty reporter list
        multi.on_run_start(10);
        multi.on_test_start("test", "file.py");
        multi.on_test_finished("test", "pass", 100, None);
        multi.on_run_finished(1, 0, 0, 100);
        multi.on_error("error");
    }

    #[test]
    fn test_status_strings() {
        // Ensure common status strings work
        for status in &["pass", "fail", "skip"] {
            let event = MachineEvent::TestFinished {
                id: "test",
                status,
                duration_ms: 1,
                message: None,
            };
            let json = serde_json::to_string(&event).unwrap();
            assert!(json.contains(status));
        }
    }

    // =========================================================================
    //  Progress Reporter Tests
    // =========================================================================

    #[test]
    fn test_progress_reporter_creation() {
        let reporter = ProgressReporter::new();
        assert_eq!(reporter.passed, 0);
        assert_eq!(reporter.failed, 0);
        assert_eq!(reporter.skipped, 0);
        assert!(reporter.failures.is_empty());
    }

    #[test]
    fn test_progress_reporter_default() {
        let reporter = ProgressReporter::default();
        assert_eq!(reporter.passed, 0);
    }

    #[test]
    fn test_progress_reporter_should_use_progress_bar() {
        // This test just ensures the function doesn't panic
        // The actual result depends on the environment
        let _ = ProgressReporter::should_use_progress_bar();
    }

    // =========================================================================
    //  Dots Reporter Tests
    // =========================================================================

    #[test]
    fn test_dots_reporter_creation() {
        let reporter = DotsReporter::new();
        assert_eq!(reporter.passed, 0);
        assert_eq!(reporter.failed, 0);
        assert_eq!(reporter.skipped, 0);
        assert!(reporter.failures.is_empty());
        assert_eq!(reporter.column, 0);
    }

    #[test]
    fn test_dots_reporter_default() {
        let reporter = DotsReporter::default();
        assert_eq!(reporter.passed, 0);
    }

    #[test]
    fn test_dots_reporter_tracks_failures() {
        let mut reporter = DotsReporter::new();
        reporter.on_run_start(3);
        reporter.on_test_finished("test1", "pass", 100, None);
        reporter.on_test_finished("test2", "fail", 100, Some("assertion failed"));
        reporter.on_test_finished("test3", "skip", 100, None);

        assert_eq!(reporter.passed, 1);
        assert_eq!(reporter.failed, 1);
        assert_eq!(reporter.skipped, 1);
        assert_eq!(reporter.failures.len(), 1);
        assert_eq!(reporter.failures[0].id, "test2");
    }

    // =========================================================================
    // Truncation and Terminal Width Tests (Bug Fix 0.1.1-C)
    // =========================================================================

    #[test]
    fn test_truncate_test_id_short() {
        // Short IDs should not be truncated
        let id = "test_simple";
        assert_eq!(truncate_test_id(id, 50), "test_simple");
    }

    #[test]
    fn test_truncate_test_id_long() {
        // Long IDs should be truncated with "..." prefix
        let id = "tests/very/long/path/to/test_module.py::TestClass::test_method";
        let result = truncate_test_id(id, 30);
        assert!(result.starts_with("..."), "Should start with ellipsis");
        assert_eq!(result.len(), 30, "Should be exactly max_width");
        assert!(result.ends_with("test_method"), "Should show end of ID");
    }

    #[test]
    fn test_truncate_test_id_very_narrow() {
        // Very narrow width should still work
        let id = "test_something_very_long";
        let result = truncate_test_id(id, 5);
        assert_eq!(result, "...ng", "Should show only last 2 chars after ...");
    }

    #[test]
    fn test_truncate_test_id_min_width() {
        // Width <= 3 should just return "..."
        let id = "test";
        assert_eq!(truncate_test_id(id, 3), "...");
        assert_eq!(truncate_test_id(id, 2), "...");
    }

    #[test]
    fn test_truncate_test_id_exact_fit() {
        // ID exactly at max_width should not be truncated
        let id = "exactly_twenty_chars";
        assert_eq!(id.len(), 20);
        assert_eq!(truncate_test_id(id, 20), id);
    }

    #[test]
    fn test_get_terminal_width() {
        // Should return a reasonable value (or fallback)
        let width = get_terminal_width();
        assert!(width >= 20, "Terminal width should be at least 20");
    }

    #[test]
    fn test_human_reporter_creation() {
        // HumanReporter should create without panicking
        let reporter = HumanReporter::new();
        assert!(
            reporter.max_name_width >= 20,
            "Max name width should be reasonable"
        );
    }
}
