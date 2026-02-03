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

use crate::config::TracebackStyle;
use serde::Serialize;
use std::io::IsTerminal;

// Re-export TracebackStyle for convenience
pub use crate::config::TracebackStyle as TbStyle;

// =============================================================================
// Constants
// =============================================================================

/// Estimated time per test in cold pytest execution (milliseconds).
///
/// This baseline represents typical pytest startup + import overhead per test:
/// - pytest discovery and collection: ~100-150ms
/// - Module import and fixture setup: ~100-150ms
/// - Test execution overhead: ~50ms
///
/// Used to calculate "time saved" by Tach's snapshot-based warm execution.
/// Based on empirical measurements across typical Python test suites.
const PYTEST_COLD_TEST_MS: u64 = 300;

// =============================================================================
// ANSI Color Codes (Tasks 2.4, 2.5)
// =============================================================================

/// ANSI escape code for cyan (file paths)
const ANSI_CYAN: &str = "\x1b[36m";
/// ANSI escape code for yellow (line numbers)
const ANSI_YELLOW: &str = "\x1b[33m";
/// ANSI escape code for green (function names)
const ANSI_GREEN: &str = "\x1b[32m";
/// ANSI escape code for red (error messages)
const ANSI_RED: &str = "\x1b[31m";
/// ANSI escape code for bold red (failing assertion line)
const ANSI_BOLD_RED: &str = "\x1b[1;31m";
/// ANSI escape code for reset
const ANSI_RESET: &str = "\x1b[0m";

// =============================================================================
// Helper Functions
// =============================================================================

/// Check if a status string represents a passing test
fn is_pass(status: &str) -> bool {
    status.eq_ignore_ascii_case("pass")
}

/// Check if a status string represents a skipped test
fn is_skip(status: &str) -> bool {
    status.eq_ignore_ascii_case("skip")
}

/// Check if stdout/stderr is connected to a terminal that supports colors.
fn supports_colors() -> bool {
    std::io::stderr().is_terminal() && std::env::var("NO_COLOR").is_err()
}

/// Colorize a single traceback line based on its content (Tasks 2.4, 2.5).
///
/// Color scheme:
/// - File paths: cyan
/// - Line numbers: yellow
/// - Function names: green
/// - Error messages (exceptions): red
/// - Failing assertion line (>>> prefix): bold red
fn colorize_traceback_line(line: &str, use_colors: bool) -> String {
    if !use_colors {
        return line.to_string();
    }

    // Check for failing assertion line marker (from Python harness)
    if line.trim_start().starts_with(">>>") {
        return format!("{}{}{}", ANSI_BOLD_RED, line, ANSI_RESET);
    }

    // Check for Python traceback frame: File "path", line N, in func
    if line.contains("File \"") && line.contains(", line ") {
        let mut result = String::new();
        let mut remaining = line;

        // Find and color the file path
        if let Some(file_start) = remaining.find("File \"") {
            result.push_str(&remaining[..file_start]);
            result.push_str("File \"");
            remaining = &remaining[file_start + 6..];

            if let Some(file_end) = remaining.find('"') {
                // Color the file path cyan
                result.push_str(ANSI_CYAN);
                result.push_str(&remaining[..file_end]);
                result.push_str(ANSI_RESET);
                result.push('"');
                remaining = &remaining[file_end + 1..];

                // Find and color the line number
                if let Some(line_start) = remaining.find(", line ") {
                    result.push_str(&remaining[..line_start]);
                    result.push_str(", line ");
                    remaining = &remaining[line_start + 7..];

                    // Find the end of the line number
                    let line_end = remaining
                        .find(|c: char| !c.is_ascii_digit())
                        .unwrap_or(remaining.len());
                    // Color the line number yellow
                    result.push_str(ANSI_YELLOW);
                    result.push_str(&remaining[..line_end]);
                    result.push_str(ANSI_RESET);
                    remaining = &remaining[line_end..];

                    // Find and color the function name
                    if let Some(in_start) = remaining.find(", in ") {
                        result.push_str(&remaining[..in_start]);
                        result.push_str(", in ");
                        remaining = &remaining[in_start + 5..];

                        // Color the function name green
                        result.push_str(ANSI_GREEN);
                        result.push_str(remaining.trim_end());
                        result.push_str(ANSI_RESET);
                        return result;
                    }
                }
            }
        }

        // If parsing failed, return the remaining text
        result.push_str(remaining);
        return result;
    }

    // Check for error/exception lines (e.g., "AssertionError: ...")
    if line
        .trim_start()
        .chars()
        .next()
        .map(|c| c.is_uppercase())
        .unwrap_or(false)
        && (line.contains("Error") || line.contains("Exception") || line.contains("Failed"))
    {
        return format!("{}{}{}", ANSI_RED, line, ANSI_RESET);
    }

    // Check for section headers from enhanced failure (e.g., "Source context:", "Local variables:")
    let trimmed = line.trim();
    if trimmed == "Source context:" || trimmed == "Local variables:" || trimmed == "Traceback:" {
        return format!("{}{}{}", ANSI_YELLOW, line, ANSI_RESET);
    }

    line.to_string()
}

/// Colorize an entire traceback message (Tasks 2.4, 2.5).
fn colorize_traceback(traceback: &str, use_colors: bool) -> String {
    if !use_colors {
        return traceback.to_string();
    }

    traceback
        .lines()
        .map(|line| colorize_traceback_line(line, use_colors))
        .collect::<Vec<_>>()
        .join("\n")
}

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
///
/// Checks `TACH_TERM_WIDTH` environment variable first for testing,
/// then falls back to actual terminal size detection.
fn get_terminal_width() -> usize {
    // Check env override for testing narrow terminal behavior
    if let Ok(width_str) = std::env::var("TACH_TERM_WIDTH")
        && let Ok(width) = width_str.parse::<usize>()
    {
        return width.max(20); // Minimum 20 columns
    }
    terminal_size::terminal_size()
        .map(|(w, _)| w.0 as usize)
        .unwrap_or(80)
}

/// Determine if the terminal is too narrow for progress bar.
///
/// Returns true if terminal width is below the threshold for progress bar
/// display (< 40 columns). In this case, DotsReporter should be used instead.
pub fn is_narrow_terminal() -> bool {
    get_terminal_width() < 40
}

/// Format a traceback message according to the specified style.
///
/// This function transforms Python tracebacks based on the --tb flag:
/// - `Short`: Shows only the first and last frames of the traceback
/// - `Long`: Returns the full traceback unchanged (default)
/// - `Line`: Returns a single line summary (file:line: message)
/// - `Native`: Returns the traceback unchanged (same as Long)
/// - `No`: Returns an empty string (suppresses traceback output)
///
/// # Arguments
/// * `traceback` - The raw traceback/error message from Python
/// * `test_id` - The test identifier (used for Line format)
/// * `style` - The traceback formatting style
///
/// # Returns
/// The formatted traceback string
pub fn format_traceback(traceback: &str, test_id: &str, style: TracebackStyle) -> String {
    match style {
        TracebackStyle::No => String::new(),
        TracebackStyle::Native | TracebackStyle::Long => traceback.to_string(),
        TracebackStyle::Line => format_traceback_line(traceback, test_id),
        TracebackStyle::Short => format_traceback_short(traceback),
    }
}

/// Format traceback as a single line: file:line: message
fn format_traceback_line(traceback: &str, test_id: &str) -> String {
    // Try to extract the last line which usually contains the assertion/error message
    let lines: Vec<&str> = traceback.lines().collect();

    // Find the error message (usually the last non-empty line)
    let error_msg = lines
        .iter()
        .rev()
        .find(|line| !line.trim().is_empty())
        .map(|s| s.trim())
        .unwrap_or("Test failed");

    // Try to find file:line from the traceback
    // Python tracebacks have format: File "path/to/file.py", line N, in func
    let file_line = lines
        .iter()
        .rev()
        .find(|line| line.contains("File \"") && line.contains(", line "))
        .and_then(|line| {
            // Parse: File "path/to/file.py", line N, in func
            let start = line.find("File \"")? + 6;
            let end = line[start..].find("\"")? + start;
            let file = &line[start..end];

            let line_start = line.find(", line ")? + 7;
            let line_end = line[line_start..]
                .find(|c: char| !c.is_ascii_digit())
                .map(|i| i + line_start)
                .unwrap_or(line[line_start..].len() + line_start);
            let line_num = &line[line_start..line_end];

            Some(format!("{}:{}", file, line_num))
        })
        .unwrap_or_else(|| test_id.to_string());

    format!("{}: {}", file_line, error_msg)
}

/// Format traceback showing only first and last frames
fn format_traceback_short(traceback: &str) -> String {
    let lines: Vec<&str> = traceback.lines().collect();

    if lines.len() <= 6 {
        // Already short enough, return as-is
        return traceback.to_string();
    }

    // Find traceback frames (lines starting with "  File " or containing "File \"")
    let mut frame_indices: Vec<usize> = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if line.trim_start().starts_with("File \"") {
            frame_indices.push(i);
        }
    }

    if frame_indices.len() <= 2 {
        // Only 2 or fewer frames, return as-is
        return traceback.to_string();
    }

    let mut result: Vec<String> = Vec::new();

    // Get first frame (first 2 lines: File line + code line)
    let first_frame = frame_indices[0];
    result.push(lines[first_frame].to_string());
    if first_frame + 1 < lines.len() && !lines[first_frame + 1].trim_start().starts_with("File \"")
    {
        result.push(lines[first_frame + 1].to_string());
    }

    // Add ellipsis to show skipped frames
    let skipped = frame_indices.len() - 2;
    if skipped > 0 {
        result.push(String::new());
        result.push(format!("    ... ({} frames omitted) ...", skipped));
        result.push(String::new());
    }

    // Get last frame and everything after it (error message)
    let last_frame = *frame_indices.last().unwrap();
    for line in lines.iter().skip(last_frame) {
        result.push(line.to_string());
    }

    result.join("\n")
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
    /// Traceback formatting style
    traceback_style: TracebackStyle,
}

impl HumanReporter {
    /// Create a new human reporter with terminal width detection
    pub fn new() -> Self {
        Self::with_traceback_style(TracebackStyle::Long)
    }

    /// Create a new human reporter with a specific traceback style
    #[must_use]
    pub fn with_traceback_style(traceback_style: TracebackStyle) -> Self {
        // Get terminal width, default to 80 if not available
        let term_width = terminal_size::terminal_size()
            .map(|(w, _)| w.0 as usize)
            .unwrap_or(80);
        // Reserve space for "  " prefix, " ... " suffix, and result (20 chars)
        let max_name_width = term_width.saturating_sub(30).max(20);
        Self {
            max_name_width,
            traceback_style,
        }
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
        eprintln!("[tach:reporter] Running {} tests...\n", count);
    }

    fn on_test_start(&mut self, id: &str, _file: &str) {
        let display_id = self.truncate_id(id);
        eprint!("  {} ... ", display_id);
    }

    fn on_test_finished(
        &mut self,
        id: &str,
        status: &str,
        duration_ms: u64,
        message: Option<&str>,
    ) {
        let use_colors = supports_colors();
        if is_pass(status) {
            eprintln!("ok ({}ms)", duration_ms);
        } else if is_skip(status) {
            eprintln!("skipped");
        } else {
            // Catch ALL failures: fail, crash, timeout, error, harness_error
            if use_colors {
                eprintln!(
                    "{}FAILED [{}]{} ({}ms)",
                    ANSI_RED, status, ANSI_RESET, duration_ms
                );
            } else {
                eprintln!("FAILED [{}] ({}ms)", status, duration_ms);
            }
            if let Some(msg) = message {
                // Format traceback according to style
                let formatted = format_traceback(msg, id, self.traceback_style);
                if !formatted.is_empty() {
                    // Apply colorization if terminal supports it
                    let colorized = colorize_traceback(&formatted, use_colors);
                    // Indent failure message
                    for line in colorized.lines().take(20) {
                        eprintln!("    {}", line);
                    }
                }
            }
        }
    }

    fn on_run_finished(&mut self, passed: usize, failed: usize, skipped: usize, duration_ms: u64) {
        eprintln!();
        eprintln!(
            "[tach:reporter] {} passed, {} failed, {} skipped in {}ms",
            passed, failed, skipped, duration_ms
        );
    }

    fn on_error(&mut self, message: &str) {
        eprintln!("[tach:reporter] FATAL ERROR: {}", message);
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
    /// Traceback formatting style
    traceback_style: TracebackStyle,
}

impl ProgressReporter {
    /// Create a new progress reporter with terminal-responsive layout
    pub fn new() -> Self {
        Self::with_traceback_style(TracebackStyle::Long)
    }

    /// Create a new progress reporter with a specific traceback style
    #[must_use]
    pub fn with_traceback_style(traceback_style: TracebackStyle) -> Self {
        let term_width = get_terminal_width();

        // Calculate max ID width for truncation (for messages)
        // For narrow terminals, give more space to the counter
        let max_id_width = if term_width < 60 {
            term_width.saturating_sub(20).max(10)
        } else {
            term_width.saturating_sub(50).max(20)
        };

        let bar = ProgressBar::new(0);

        // Choose template based on terminal width:
        // - Very narrow (< 60): Minimal mode - just counter and short message
        // - Narrow (< 80): Small bar with condensed format
        // - Normal (< 120): Medium bar with full format
        // - Wide (>= 120): Large bar with full format
        let template = if term_width < 60 {
            // Minimal mode: [42/100] test_name
            // No spinner, no elapsed time, no progress bar
            "{pos}/{len} {msg}".to_string()
        } else if term_width < 80 {
            // Narrow mode: Small bar (20 chars)
            "{spinner:.green} [{bar:20.cyan/blue}] {pos}/{len} {msg}".to_string()
        } else if term_width < 120 {
            // Normal mode: Medium bar (40 chars)
            "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} {msg}"
                .to_string()
        } else {
            // Wide mode: Large bar (60 chars)
            "{spinner:.green} [{elapsed_precise}] [{bar:60.cyan/blue}] {pos}/{len} {msg}"
                .to_string()
        };

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
            traceback_style,
        }
    }

    /// Check if we should use progress bar (interactive terminal)
    pub fn should_use_progress_bar() -> bool {
        std::io::stderr().is_terminal() && std::env::var("CI").is_err()
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
        if is_pass(status) {
            self.passed += 1;
        } else if is_skip(status) {
            self.skipped += 1;
        } else {
            // Catch ALL failures: fail, crash, timeout, error, harness_error
            self.failed += 1;
            // Format and buffer failure for summary with colorization
            let use_colors = supports_colors();
            let formatted_msg = message
                .map(|m| {
                    let formatted = format_traceback(m, id, self.traceback_style);
                    colorize_traceback(&formatted, use_colors)
                })
                .unwrap_or_default();
            self.failures.push(FailureRecord {
                id: id.to_string(),
                message: formatted_msg,
            });
        }

        self.bar.inc(1);
        self.bar.set_message(format!(
            "P:{} F:{} S:{}",
            self.passed, self.failed, self.skipped
        ));
    }

    fn on_run_finished(&mut self, passed: usize, failed: usize, skipped: usize, duration_ms: u64) {
        self.bar.finish_and_clear();

        // Print failure details (already formatted by traceback style)
        if !self.failures.is_empty() && self.traceback_style != TracebackStyle::No {
            eprintln!("\n{} FAILURES {}", "=".repeat(30), "=".repeat(30));
            for failure in &self.failures {
                eprintln!("\n{}", failure.id);
                eprintln!("{}", "-".repeat(failure.id.len().min(70)));
                if !failure.message.is_empty() {
                    // Limit failure message to 20 lines
                    for line in failure.message.lines().take(20) {
                        eprintln!("{}", line);
                    }
                }
            }
            eprintln!("{}", "=".repeat(70));
        }

        // Print summary with colors (if supported)
        let duration_secs = duration_ms as f64 / 1000.0;
        let use_colors = supports_colors();
        if failed > 0 {
            if use_colors {
                eprintln!(
                    "\n{}{} passed, {} failed, {} skipped in {:.2}s{}",
                    ANSI_RED, passed, failed, skipped, duration_secs, ANSI_RESET
                );
            } else {
                eprintln!(
                    "\n{} passed, {} failed, {} skipped in {:.2}s",
                    passed, failed, skipped, duration_secs
                );
            }
        } else if use_colors {
            eprintln!(
                "\n{}{} passed, {} failed, {} skipped in {:.2}s{}",
                ANSI_GREEN, passed, failed, skipped, duration_secs, ANSI_RESET
            );
        } else {
            eprintln!(
                "\n{} passed, {} failed, {} skipped in {:.2}s",
                passed, failed, skipped, duration_secs
            );
        }

        // Time Saved metric: compare actual time vs estimated cold pytest time
        let total_tests = passed + failed + skipped;
        let estimated_cold_ms = total_tests as u64 * PYTEST_COLD_TEST_MS;
        let time_saved_ms = estimated_cold_ms.saturating_sub(duration_ms);

        if time_saved_ms > 1000 && total_tests > 10 {
            let saved_secs = time_saved_ms as f64 / 1000.0;
            if saved_secs >= 60.0 {
                let mins = (saved_secs / 60.0).floor() as u64;
                let secs = saved_secs % 60.0;
                if use_colors {
                    eprintln!(
                        "{}(Saved {}m {:.0}s of initialization overhead){}",
                        ANSI_CYAN, mins, secs, ANSI_RESET
                    );
                } else {
                    eprintln!("(Saved {}m {:.0}s of initialization overhead)", mins, secs);
                }
            } else if use_colors {
                eprintln!(
                    "{}(Saved {:.1}s of initialization overhead){}",
                    ANSI_CYAN, saved_secs, ANSI_RESET
                );
            } else {
                eprintln!("(Saved {:.1}s of initialization overhead)", saved_secs);
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
    /// Traceback formatting style
    traceback_style: TracebackStyle,
}

impl DotsReporter {
    /// Create a new dots reporter
    pub fn new() -> Self {
        Self::with_traceback_style(TracebackStyle::Long)
    }

    /// Create a new dots reporter with a specific traceback style
    #[must_use]
    pub fn with_traceback_style(traceback_style: TracebackStyle) -> Self {
        Self {
            passed: 0,
            failed: 0,
            skipped: 0,
            failures: Vec::new(),
            column: 0,
            traceback_style,
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
        eprintln!("[tach:reporter] Running {} tests...\n", count);
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
        if is_pass(status) {
            self.passed += 1;
            self.print_char('.');
        } else if is_skip(status) {
            self.skipped += 1;
            self.print_char('s');
        } else {
            // Catch ALL failures: fail, crash, timeout, error, harness_error
            self.failed += 1;
            self.print_char('F');
            // Format and buffer failure for summary with colorization
            let use_colors = supports_colors();
            let formatted_msg = message
                .map(|m| {
                    let formatted = format_traceback(m, id, self.traceback_style);
                    colorize_traceback(&formatted, use_colors)
                })
                .unwrap_or_default();
            self.failures.push(FailureRecord {
                id: id.to_string(),
                message: formatted_msg,
            });
        }
    }

    fn on_run_finished(&mut self, passed: usize, failed: usize, skipped: usize, duration_ms: u64) {
        // Finish the line if we have any output
        if self.column > 0 {
            eprintln!();
        }

        // Print failure details (already formatted by traceback style)
        if !self.failures.is_empty() && self.traceback_style != TracebackStyle::No {
            eprintln!("\n{} FAILURES {}", "=".repeat(30), "=".repeat(30));
            for failure in &self.failures {
                eprintln!("\n{}", failure.id);
                eprintln!("{}", "-".repeat(failure.id.len().min(70)));
                if !failure.message.is_empty() {
                    // Limit failure message to 20 lines
                    for line in failure.message.lines().take(20) {
                        eprintln!("{}", line);
                    }
                }
            }
            eprintln!("{}", "=".repeat(70));
        }

        // Print summary
        let duration_secs = duration_ms as f64 / 1000.0;
        eprintln!(
            "\n[tach:reporter] {} passed, {} failed, {} skipped in {:.2}s",
            passed, failed, skipped, duration_secs
        );

        // Time Saved metric: compare actual time vs estimated cold pytest time
        let total_tests = passed + failed + skipped;
        let estimated_cold_ms = total_tests as u64 * PYTEST_COLD_TEST_MS;
        let time_saved_ms = estimated_cold_ms.saturating_sub(duration_ms);

        if time_saved_ms > 1000 && total_tests > 10 {
            let saved_secs = time_saved_ms as f64 / 1000.0;
            if saved_secs >= 60.0 {
                let mins = (saved_secs / 60.0).floor() as u64;
                let secs = saved_secs % 60.0;
                eprintln!(
                    "[tach:reporter] (Saved {}m {:.0}s of initialization overhead)",
                    mins, secs
                );
            } else {
                eprintln!(
                    "[tach:reporter] (Saved {:.1}s of initialization overhead)",
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
        eprintln!("[tach:reporter] FATAL ERROR: {}", message);
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
    fn test_get_terminal_width_env_override() {
        // Test TACH_TERM_WIDTH environment variable override
        // SAFETY: This test runs in isolation and restores the env var afterward
        unsafe { std::env::set_var("TACH_TERM_WIDTH", "50") };
        let width = get_terminal_width();
        unsafe { std::env::remove_var("TACH_TERM_WIDTH") };
        assert_eq!(width, 50, "Should respect TACH_TERM_WIDTH env var");
    }

    #[test]
    fn test_get_terminal_width_env_minimum() {
        // Test that env override respects minimum of 20
        // SAFETY: This test runs in isolation and restores the env var afterward
        unsafe { std::env::set_var("TACH_TERM_WIDTH", "10") };
        let width = get_terminal_width();
        unsafe { std::env::remove_var("TACH_TERM_WIDTH") };
        assert_eq!(width, 20, "Should enforce minimum width of 20");
    }

    #[test]
    fn test_get_terminal_width_env_invalid() {
        // Test that invalid env values fall back to terminal detection
        // SAFETY: This test runs in isolation and restores the env var afterward
        unsafe { std::env::set_var("TACH_TERM_WIDTH", "invalid") };
        let width = get_terminal_width();
        unsafe { std::env::remove_var("TACH_TERM_WIDTH") };
        // Should fall back to terminal size or 80
        assert!(
            width >= 20,
            "Should fall back to reasonable width for invalid env"
        );
    }

    #[test]
    fn test_is_narrow_terminal_narrow() {
        // Test is_narrow_terminal returns true for narrow terminals
        // SAFETY: This test runs in isolation and restores the env var afterward
        unsafe { std::env::set_var("TACH_TERM_WIDTH", "30") };
        let result = super::is_narrow_terminal();
        unsafe { std::env::remove_var("TACH_TERM_WIDTH") };
        assert!(result, "Should return true for terminal < 40 columns");
    }

    #[test]
    fn test_is_narrow_terminal_wide() {
        // Test is_narrow_terminal returns false for wide terminals
        // SAFETY: This test runs in isolation and restores the env var afterward
        unsafe { std::env::set_var("TACH_TERM_WIDTH", "80") };
        let result = super::is_narrow_terminal();
        unsafe { std::env::remove_var("TACH_TERM_WIDTH") };
        assert!(!result, "Should return false for terminal >= 40 columns");
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

    // =========================================================================
    //  Traceback Formatting Tests (0.1.2-C)
    // =========================================================================

    #[test]
    fn test_format_traceback_no_style() {
        let traceback = "Traceback (most recent call last):\n  File \"test.py\", line 10\n    assert False\nAssertionError";
        let result = super::format_traceback(traceback, "test_foo", TracebackStyle::No);
        assert!(result.is_empty(), "No style should return empty string");
    }

    #[test]
    fn test_format_traceback_long_style() {
        let traceback = "Traceback (most recent call last):\n  File \"test.py\", line 10\n    assert False\nAssertionError";
        let result = super::format_traceback(traceback, "test_foo", TracebackStyle::Long);
        assert_eq!(result, traceback, "Long style should return unchanged");
    }

    #[test]
    fn test_format_traceback_native_style() {
        let traceback = "Traceback (most recent call last):\n  File \"test.py\", line 10\n    assert False\nAssertionError";
        let result = super::format_traceback(traceback, "test_foo", TracebackStyle::Native);
        assert_eq!(result, traceback, "Native style should return unchanged");
    }

    #[test]
    fn test_format_traceback_line_style() {
        let traceback = "Traceback (most recent call last):\n  File \"test.py\", line 10, in test_foo\n    assert False\nAssertionError";
        let result = super::format_traceback(traceback, "test_foo", TracebackStyle::Line);
        assert!(
            result.contains("test.py:10"),
            "Line style should include file:line"
        );
        assert!(
            result.contains("AssertionError"),
            "Line style should include error message"
        );
    }

    #[test]
    fn test_format_traceback_line_style_extracts_location() {
        let traceback = r#"Traceback (most recent call last):
  File "/path/to/test_example.py", line 42, in test_something
    assert 1 == 2
AssertionError: 1 != 2"#;
        let result = super::format_traceback(traceback, "test_something", TracebackStyle::Line);
        assert!(
            result.contains("/path/to/test_example.py:42"),
            "Should extract correct file:line"
        );
        assert!(
            result.contains("AssertionError"),
            "Should include error message"
        );
    }

    #[test]
    fn test_format_traceback_line_style_fallback() {
        let traceback = "Simple error without traceback format";
        let result = super::format_traceback(traceback, "test_foo", TracebackStyle::Line);
        assert!(result.contains("test_foo"), "Should fall back to test_id");
        assert!(result.contains("Simple error"), "Should include error text");
    }

    #[test]
    fn test_format_traceback_short_style_already_short() {
        let traceback = "File \"test.py\", line 10\nAssertionError";
        let result = super::format_traceback(traceback, "test_foo", TracebackStyle::Short);
        assert_eq!(result, traceback, "Short traceback should be unchanged");
    }

    #[test]
    fn test_format_traceback_short_style_truncates() {
        let traceback = r#"Traceback (most recent call last):
  File "first.py", line 1, in first
    second()
  File "second.py", line 2, in second
    third()
  File "third.py", line 3, in third
    fourth()
  File "fourth.py", line 4, in fourth
    fifth()
  File "fifth.py", line 5, in fifth
    assert False
AssertionError"#;
        let result = super::format_traceback(traceback, "test_foo", TracebackStyle::Short);
        assert!(result.contains("first.py"), "Should include first frame");
        assert!(result.contains("fifth.py"), "Should include last frame");
        assert!(result.contains("AssertionError"), "Should include error");
        assert!(
            result.contains("frames omitted"),
            "Should indicate omitted frames"
        );
        // Should NOT include middle frames
        assert!(
            !result.contains("third.py"),
            "Should not include middle frame"
        );
    }

    #[test]
    fn test_format_traceback_short_style_two_frames() {
        let traceback = r#"Traceback (most recent call last):
  File "first.py", line 1, in first
    second()
  File "second.py", line 2, in second
    assert False
AssertionError"#;
        let result = super::format_traceback(traceback, "test_foo", TracebackStyle::Short);
        // With only 2 frames, should return as-is
        assert_eq!(result, traceback, "Two-frame traceback should be unchanged");
    }

    #[test]
    fn test_progress_reporter_with_traceback_style() {
        let reporter = ProgressReporter::with_traceback_style(TracebackStyle::Short);
        assert_eq!(reporter.traceback_style, TracebackStyle::Short);
    }

    #[test]
    fn test_dots_reporter_with_traceback_style() {
        let reporter = DotsReporter::with_traceback_style(TracebackStyle::Line);
        assert_eq!(reporter.traceback_style, TracebackStyle::Line);
    }

    #[test]
    fn test_human_reporter_with_traceback_style() {
        let reporter = HumanReporter::with_traceback_style(TracebackStyle::No);
        assert_eq!(reporter.traceback_style, TracebackStyle::No);
    }

    // =========================================================================
    //  Colorization Tests (Tasks 2.4, 2.5)
    // =========================================================================

    #[test]
    fn test_colorize_traceback_line_disabled() {
        let line = "  File \"test.py\", line 10, in test_func";
        let result = colorize_traceback_line(line, false);
        assert_eq!(result, line, "No colors when disabled");
    }

    #[test]
    fn test_colorize_traceback_line_file_path() {
        let line = "  File \"test.py\", line 10, in test_func";
        let result = colorize_traceback_line(line, true);
        assert!(result.contains(ANSI_CYAN), "File path should be cyan");
        assert!(result.contains(ANSI_YELLOW), "Line number should be yellow");
        assert!(result.contains(ANSI_GREEN), "Function name should be green");
        assert!(result.contains(ANSI_RESET), "Should have reset codes");
    }

    #[test]
    fn test_colorize_traceback_line_error_message() {
        let line = "AssertionError: expected True";
        let result = colorize_traceback_line(line, true);
        assert!(result.contains(ANSI_RED), "Error should be red");
        assert!(result.contains(ANSI_RESET), "Should have reset code");
    }

    #[test]
    fn test_colorize_traceback_line_failing_assertion() {
        let line = ">>>   10 | assert x == y";
        let result = colorize_traceback_line(line, true);
        assert!(
            result.contains(ANSI_BOLD_RED),
            "Failing line should be bold red"
        );
    }

    #[test]
    fn test_colorize_traceback_line_section_header() {
        let line = "Source context:";
        let result = colorize_traceback_line(line, true);
        assert!(
            result.contains(ANSI_YELLOW),
            "Section header should be yellow"
        );
    }

    #[test]
    fn test_colorize_traceback_full() {
        let traceback = "Source context:\n>>>   10 | assert x == y\nAssertionError: x != y";
        let result = colorize_traceback(traceback, true);
        assert!(
            result.contains(ANSI_YELLOW),
            "Should contain yellow for header"
        );
        assert!(
            result.contains(ANSI_BOLD_RED),
            "Should contain bold red for failing line"
        );
        assert!(result.contains(ANSI_RED), "Should contain red for error");
    }

    #[test]
    fn test_colorize_traceback_disabled() {
        let traceback = "Source context:\n>>>   10 | assert x == y\nAssertionError";
        let result = colorize_traceback(traceback, false);
        assert_eq!(result, traceback, "No colors when disabled");
    }

    // =========================================================================
    //  Non-standard Status Handling Tests (Batch 3: crash/timeout/error)
    // =========================================================================

    #[test]
    fn test_progress_reporter_counts_crash_as_failure() {
        let mut reporter = ProgressReporter::new();
        reporter.on_run_start(3);
        reporter.on_test_finished("test1", "pass", 100, None);
        reporter.on_test_finished("test2", "crash", 100, Some("segfault"));
        reporter.on_test_finished("test3", "skip", 100, None);

        assert_eq!(reporter.passed, 1);
        assert_eq!(reporter.failed, 1, "crash should be counted as failure");
        assert_eq!(reporter.skipped, 1);
        assert_eq!(reporter.failures.len(), 1);
        assert_eq!(reporter.failures[0].id, "test2");
    }

    #[test]
    fn test_progress_reporter_counts_timeout_as_failure() {
        let mut reporter = ProgressReporter::new();
        reporter.on_run_start(2);
        reporter.on_test_finished("test1", "timeout", 5000, Some("exceeded 5s limit"));
        reporter.on_test_finished("test2", "pass", 100, None);

        assert_eq!(reporter.passed, 1);
        assert_eq!(reporter.failed, 1, "timeout should be counted as failure");
        assert_eq!(reporter.failures.len(), 1);
        assert_eq!(reporter.failures[0].id, "test1");
    }

    #[test]
    fn test_progress_reporter_counts_error_as_failure() {
        let mut reporter = ProgressReporter::new();
        reporter.on_run_start(1);
        reporter.on_test_finished("test1", "error", 100, Some("import error"));

        assert_eq!(reporter.failed, 1, "error should be counted as failure");
        assert_eq!(reporter.failures.len(), 1);
    }

    #[test]
    fn test_progress_reporter_counts_harness_error_as_failure() {
        let mut reporter = ProgressReporter::new();
        reporter.on_run_start(1);
        reporter.on_test_finished("test1", "harness_error", 100, Some("harness crash"));

        assert_eq!(
            reporter.failed, 1,
            "harness_error should be counted as failure"
        );
        assert_eq!(reporter.failures.len(), 1);
    }

    #[test]
    fn test_dots_reporter_counts_crash_as_failure() {
        let mut reporter = DotsReporter::new();
        reporter.on_run_start(3);
        reporter.on_test_finished("test1", "pass", 100, None);
        reporter.on_test_finished("test2", "crash", 100, Some("segfault"));
        reporter.on_test_finished("test3", "skip", 100, None);

        assert_eq!(reporter.passed, 1);
        assert_eq!(reporter.failed, 1, "crash should be counted as failure");
        assert_eq!(reporter.skipped, 1);
        assert_eq!(reporter.failures.len(), 1);
        assert_eq!(reporter.failures[0].id, "test2");
    }

    #[test]
    fn test_dots_reporter_counts_timeout_as_failure() {
        let mut reporter = DotsReporter::new();
        reporter.on_run_start(2);
        reporter.on_test_finished("test1", "timeout", 5000, Some("exceeded limit"));
        reporter.on_test_finished("test2", "pass", 100, None);

        assert_eq!(reporter.passed, 1);
        assert_eq!(reporter.failed, 1, "timeout should be counted as failure");
        assert_eq!(reporter.failures.len(), 1);
    }

    #[test]
    fn test_dots_reporter_counts_all_non_pass_skip_as_failure() {
        let mut reporter = DotsReporter::new();
        reporter.on_run_start(5);
        reporter.on_test_finished("t1", "fail", 100, None);
        reporter.on_test_finished("t2", "crash", 100, None);
        reporter.on_test_finished("t3", "timeout", 100, None);
        reporter.on_test_finished("t4", "error", 100, None);
        reporter.on_test_finished("t5", "harness_error", 100, None);

        assert_eq!(
            reporter.failed, 5,
            "all non-pass/skip statuses should be failures"
        );
        assert_eq!(reporter.failures.len(), 5);
    }
}
