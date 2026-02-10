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
use std::collections::{HashMap, HashSet};
use std::io::IsTerminal;
use std::time::Duration;

// Re-export TracebackStyle for convenience
pub use crate::config::TracebackStyle as TbStyle;

// =============================================================================
// Constants
// =============================================================================

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

/// Check if stdout is connected to a terminal that supports colors.
///
/// Used by TachReporter which outputs to stdout (stderr may be redirected to log file).
fn supports_colors_stdout() -> bool {
    std::io::stdout().is_terminal() && std::env::var("NO_COLOR").is_err()
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
    /// Called before the run starts with per-file test counts.
    /// Used by TachReporter for real-time file streaming.
    fn on_session_setup(&mut self, _file_counts: &HashMap<String, usize>) {}

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

    /// Set the log file path to display in summary (default: no-op).
    fn set_log_path(&mut self, _path: &str) {}
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
    fn on_session_setup(&mut self, file_counts: &HashMap<String, usize>) {
        for r in &mut self.reporters {
            r.on_session_setup(file_counts);
        }
    }

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

    fn set_log_path(&mut self, path: &str) {
        for r in &mut self.reporters {
            r.set_log_path(path);
        }
    }
}

// =============================================================================
//  Progress Bar Reporter
// =============================================================================

use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};

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
        bar.enable_steady_tick(Duration::from_millis(100));

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
        std::io::stdout().is_terminal() && std::env::var("CI").is_err()
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
//  Tach Reporter (Vitest-style)
// =============================================================================

/// Record of a single test failure within a file
struct TachFailureRecord {
    test_id: String,
    short_name: String,
    message: String,
}

/// Aggregated results for a single test file
struct FileResult {
    /// Insertion order index for stable ordering
    order: usize,
    passed: usize,
    failed: usize,
    skipped: usize,
    total_duration_ms: u64,
    failures: Vec<TachFailureRecord>,
}

impl FileResult {
    fn new(order: usize) -> Self {
        Self {
            order,
            passed: 0,
            failed: 0,
            skipped: 0,
            total_duration_ms: 0,
            failures: Vec::new(),
        }
    }

    fn total(&self) -> usize {
        self.passed + self.failed + self.skipped
    }

    fn has_failures(&self) -> bool {
        self.failed > 0
    }
}

/// Vitest-style reporter with file-grouped output
///
/// Groups test results by file and displays them in a compact format:
/// ```text
///  ✓ tests/auth/test_login.py (12)  340ms
///  × tests/api/test_users.py (14 passed | 1 failed)  890ms
///    × test_create_user_invalid_email
/// ```
pub struct TachReporter {
    bar: ProgressBar,
    file_results: HashMap<String, FileResult>,
    test_to_file: HashMap<String, String>,
    file_order: usize,
    total: usize,
    passed: usize,
    failed: usize,
    skipped: usize,
    traceback_style: TracebackStyle,
    /// Per-file expected test counts (set via on_session_setup)
    file_expected: HashMap<String, usize>,
    /// Set of file paths already streamed to terminal
    files_streamed: HashSet<String>,
    /// Count of files already streamed (for testing)
    files_printed: usize,
    /// Optional log file path to display in summary
    log_path: Option<String>,
}

impl TachReporter {
    /// Return the fail icon (×), optionally wrapped in red ANSI codes.
    fn fail_icon(use_colors: bool) -> String {
        if use_colors {
            format!("{}\u{00d7}{}", ANSI_RED, ANSI_RESET)
        } else {
            "\u{00d7}".to_string()
        }
    }

    /// Create a new TachReporter with default settings
    pub fn new() -> Self {
        Self::with_traceback_style(TracebackStyle::Long)
    }

    /// Create a new TachReporter with a specific traceback style
    #[must_use]
    pub fn with_traceback_style(traceback_style: TracebackStyle) -> Self {
        let bar = ProgressBar::new_spinner();
        bar.set_draw_target(ProgressDrawTarget::stdout());
        bar.set_style(
            ProgressStyle::default_spinner()
                .template("{spinner:.green} {msg}")
                .expect("invalid spinner template"),
        );
        bar.enable_steady_tick(Duration::from_millis(100));
        Self {
            bar,
            file_results: HashMap::new(),
            test_to_file: HashMap::new(),
            file_order: 0,
            total: 0,
            passed: 0,
            failed: 0,
            skipped: 0,
            traceback_style,
            file_expected: HashMap::new(),
            files_streamed: HashSet::new(),
            files_printed: 0,
            log_path: None,
        }
    }

    /// Extract the short test name from a fully-qualified test ID.
    ///
    /// `"tests/foo.py::TestClass::test_method"` -> `"test_method"`
    /// `"tests/foo.py::test_simple"` -> `"test_simple"`
    /// `"just_a_name"` -> `"just_a_name"`
    fn short_test_name(test_id: &str) -> &str {
        test_id.rsplit("::").next().unwrap_or(test_id)
    }

    /// Format a duration in milliseconds for display.
    ///
    /// Returns e.g. `"340ms"` or `"2.14s"` for longer durations.
    fn format_duration(ms: u64) -> String {
        if ms < 1000 {
            format!("{}ms", ms)
        } else {
            format!("{:.2}s", ms as f64 / 1000.0)
        }
    }

    /// Return file results sorted by insertion order.
    fn sorted_files(&self) -> Vec<(&String, &FileResult)> {
        let mut files: Vec<_> = self.file_results.iter().collect();
        files.sort_by_key(|(_, r)| r.order);
        files
    }

    /// Format a single file result line as a String (without trailing newline).
    fn format_file_line(&self, file_path: &str, result: &FileResult, use_colors: bool) -> String {
        let duration = Self::format_duration(result.total_duration_ms);

        if result.has_failures() {
            let counts = if result.passed > 0 {
                if use_colors {
                    format!(
                        "{}{} passed{} | {}{} failed{}",
                        ANSI_GREEN, result.passed, ANSI_RESET, ANSI_RED, result.failed, ANSI_RESET,
                    )
                } else {
                    format!("{} passed | {} failed", result.passed, result.failed)
                }
            } else if use_colors {
                format!("{}{} failed{}", ANSI_RED, result.failed, ANSI_RESET)
            } else {
                format!("{} failed", result.failed)
            };

            let icon = Self::fail_icon(use_colors);

            format!(" {} {} ({})  {}", icon, file_path, counts, duration)
        } else if result.passed == 0 && result.skipped > 0 {
            let icon = if use_colors {
                format!("{}-{}", ANSI_YELLOW, ANSI_RESET)
            } else {
                "-".to_string()
            };
            format!(
                " {} {} ({} skipped)  {}",
                icon, file_path, result.skipped, duration
            )
        } else if result.skipped > 0 {
            let icon = if use_colors {
                format!("{}\u{2713}{}", ANSI_GREEN, ANSI_RESET)
            } else {
                "\u{2713}".to_string()
            };
            format!(
                " {} {} ({} passed | {} skipped)  {}",
                icon, file_path, result.passed, result.skipped, duration
            )
        } else {
            let icon = if use_colors {
                format!("{}\u{2713}{}", ANSI_GREEN, ANSI_RESET)
            } else {
                "\u{2713}".to_string()
            };
            format!(" {} {} ({})  {}", icon, file_path, result.total(), duration)
        }
    }

    /// Render the file-grouped results list to stdout.
    /// Skips files that were already streamed in real-time.
    fn render_file_list(&self, use_colors: bool) {
        let files = self.sorted_files();

        for (file_path, result) in &files {
            // Skip files already streamed during the run
            if self.files_streamed.contains(*file_path) {
                continue;
            }

            println!("{}", self.format_file_line(file_path, result, use_colors));

            // List failed test names under the file
            if result.has_failures() {
                for failure in &result.failures {
                    let fail_icon = Self::fail_icon(use_colors);
                    println!("   {} {}", fail_icon, failure.short_name);
                }
            }
        }
    }

    /// Render the summary block to stdout.
    fn render_summary(&self, duration_ms: u64, use_colors: bool) {
        // Count file-level pass/fail/skip
        let files = self.sorted_files();
        let total_files = files.len();
        let failed_files = files.iter().filter(|(_, r)| r.has_failures()).count();
        let skipped_files = files
            .iter()
            .filter(|(_, r)| !r.has_failures() && r.passed == 0)
            .count();
        let passed_files = total_files - failed_files - skipped_files;

        // File summary line
        let mut file_parts: Vec<String> = Vec::new();

        if passed_files > 0 {
            if use_colors {
                file_parts.push(format!(
                    "{}{} passed{}",
                    ANSI_GREEN, passed_files, ANSI_RESET
                ));
            } else {
                file_parts.push(format!("{} passed", passed_files));
            }
        }
        if failed_files > 0 {
            if use_colors {
                file_parts.push(format!("{}{} failed{}", ANSI_RED, failed_files, ANSI_RESET));
            } else {
                file_parts.push(format!("{} failed", failed_files));
            }
        }
        if skipped_files > 0 {
            file_parts.push(format!("{} skipped", skipped_files));
        }

        let file_counts = if file_parts.is_empty() {
            "0".to_string()
        } else {
            file_parts.join(" | ")
        };

        println!();
        println!(" Test Files  {} ({})", file_counts, total_files);

        // Test counts line
        let test_total = self.passed + self.failed + self.skipped;
        let mut test_parts: Vec<String> = Vec::new();

        if self.passed > 0 {
            if use_colors {
                test_parts.push(format!(
                    "{}{} passed{}",
                    ANSI_GREEN, self.passed, ANSI_RESET
                ));
            } else {
                test_parts.push(format!("{} passed", self.passed));
            }
        }
        if self.failed > 0 {
            if use_colors {
                test_parts.push(format!("{}{} failed{}", ANSI_RED, self.failed, ANSI_RESET));
            } else {
                test_parts.push(format!("{} failed", self.failed));
            }
        }
        if self.skipped > 0 {
            test_parts.push(format!("{} skipped", self.skipped));
        }

        let test_counts = if test_parts.is_empty() {
            "0".to_string()
        } else {
            test_parts.join(" | ")
        };

        println!("      Tests  {} ({})", test_counts, test_total);

        // Duration line
        let duration_str = Self::format_duration(duration_ms);
        println!("   Duration  {}", duration_str);

        // Log file path (if set)
        if let Some(ref path) = self.log_path {
            println!("   Log file  {}", path);
        }
    }

    /// Render failure details at the end.
    fn render_failures(&self, use_colors: bool) {
        if self.traceback_style == TracebackStyle::No {
            return;
        }

        let all_failures: Vec<&TachFailureRecord> = self
            .sorted_files()
            .iter()
            .flat_map(|(_, r)| r.failures.iter())
            .collect();

        if all_failures.is_empty() {
            return;
        }

        println!();
        println!("{} FAILURES {}", "=".repeat(30), "=".repeat(30));

        for failure in &all_failures {
            let header = if use_colors {
                format!("{}FAIL{} > {}", ANSI_BOLD_RED, ANSI_RESET, failure.test_id)
            } else {
                format!("FAIL > {}", failure.test_id)
            };
            println!();
            println!("{}", header);
            println!("{}", "-".repeat(failure.test_id.len().min(70) + 7));
            if !failure.message.is_empty() {
                for line in failure.message.lines().take(20) {
                    println!("{}", line);
                }
            }
        }
        println!("{}", "=".repeat(70));
    }
}

impl Default for TachReporter {
    fn default() -> Self {
        Self::new()
    }
}

impl Reporter for TachReporter {
    fn on_session_setup(&mut self, file_counts: &HashMap<String, usize>) {
        self.file_expected = file_counts.clone();
    }

    fn on_run_start(&mut self, count: usize) {
        self.total = count;
        self.bar.set_message(format!("Running {} tests", count));
    }

    fn on_test_start(&mut self, id: &str, file: &str) {
        // Map test ID to file
        self.test_to_file.insert(id.to_string(), file.to_string());

        // Ensure file entry exists
        let order = self.file_order;
        self.file_results
            .entry(file.to_string())
            .or_insert_with(|| {
                self.file_order = order + 1;
                FileResult::new(order)
            });
    }

    fn on_test_finished(
        &mut self,
        id: &str,
        status: &str,
        duration_ms: u64,
        message: Option<&str>,
    ) {
        let file = self
            .test_to_file
            .get(id)
            .cloned()
            .unwrap_or_else(|| "unknown".to_string());

        let order = self.file_order;
        let file_result = self.file_results.entry(file.clone()).or_insert_with(|| {
            self.file_order = order + 1;
            FileResult::new(order)
        });

        file_result.total_duration_ms += duration_ms;

        if is_pass(status) {
            self.passed += 1;
            file_result.passed += 1;
        } else if is_skip(status) {
            self.skipped += 1;
            file_result.skipped += 1;
        } else {
            // fail, crash, timeout, error, harness_error
            self.failed += 1;
            file_result.failed += 1;

            let use_colors = supports_colors_stdout();
            let formatted_msg = message
                .map(|m| {
                    let formatted = format_traceback(m, id, self.traceback_style);
                    colorize_traceback(&formatted, use_colors)
                })
                .unwrap_or_default();

            file_result.failures.push(TachFailureRecord {
                test_id: id.to_string(),
                short_name: Self::short_test_name(id).to_string(),
                message: formatted_msg,
            });
        }

        // Update spinner
        let mut parts = vec![format!("Running {} tests", self.total)];
        if self.passed > 0 {
            parts.push(format!("{} passed", self.passed));
        }
        if self.failed > 0 {
            parts.push(format!("{} failed", self.failed));
        }
        self.bar.set_message(parts.join(" \u{00b7} "));

        // --- Real-time file streaming ---
        // Check if this file is now complete (all expected tests finished)
        if let Some(&expected) = self.file_expected.get(&file) {
            // Re-borrow file_result immutably after the mutable borrow above has ended
            if let Some(file_result) = self.file_results.get(&file) {
                let actual = file_result.total();
                if actual == expected {
                    let use_colors = supports_colors_stdout();
                    let line = self.format_file_line(&file, file_result, use_colors);
                    self.bar.println(&line);

                    // For failed files, also print the failed test names
                    if file_result.has_failures() {
                        for failure in &file_result.failures {
                            let fail_icon = Self::fail_icon(use_colors);
                            self.bar
                                .println(format!("   {} {}", fail_icon, failure.short_name));
                        }
                    }

                    self.files_streamed.insert(file);
                    self.files_printed += 1;
                }
            }
        }
    }

    fn on_run_finished(
        &mut self,
        _passed: usize,
        _failed: usize,
        _skipped: usize,
        duration_ms: u64,
    ) {
        // Clear spinner
        self.bar.finish_and_clear();

        let use_colors = supports_colors_stdout();

        // Render file-grouped list (only files not already streamed)
        let has_unstreamed = self
            .file_results
            .keys()
            .any(|path| !self.files_streamed.contains(path));
        if has_unstreamed {
            println!();
            self.render_file_list(use_colors);
        }

        // Render summary block
        self.render_summary(duration_ms, use_colors);

        // Render failure details
        self.render_failures(use_colors);
    }

    fn on_error(&mut self, message: &str) {
        self.bar.finish_and_clear();
        // NOTE: Uses println! (stdout) intentionally. TachReporter and JsonReporter
        // are mutually exclusive (see OutputFormat match in main.rs), so this won't
        // pollute JSON output. Using stdout ensures the error is visible even when
        // stderr is redirected to the log file by LogRedirect.
        println!("[tach:reporter] FATAL ERROR: {}", message);
    }

    fn set_log_path(&mut self, path: &str) {
        self.log_path = Some(path.to_owned());
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

    // =========================================================================
    // TachReporter tests
    // =========================================================================

    #[test]
    fn test_tach_reporter_groups_by_file() {
        let mut reporter = TachReporter::new();
        reporter.on_run_start(4);
        reporter.on_test_start(
            "tests/auth/test_login.py::test_valid",
            "tests/auth/test_login.py",
        );
        reporter.on_test_finished("tests/auth/test_login.py::test_valid", "pass", 100, None);
        reporter.on_test_start(
            "tests/auth/test_login.py::test_invalid",
            "tests/auth/test_login.py",
        );
        reporter.on_test_finished("tests/auth/test_login.py::test_invalid", "pass", 50, None);
        reporter.on_test_start(
            "tests/api/test_users.py::test_create",
            "tests/api/test_users.py",
        );
        reporter.on_test_finished(
            "tests/api/test_users.py::test_create",
            "fail",
            200,
            Some("AssertionError"),
        );
        reporter.on_test_start(
            "tests/api/test_users.py::test_list",
            "tests/api/test_users.py",
        );
        reporter.on_test_finished("tests/api/test_users.py::test_list", "pass", 80, None);

        assert_eq!(reporter.file_results.len(), 2);
        let auth = &reporter.file_results["tests/auth/test_login.py"];
        assert_eq!(auth.passed, 2);
        assert_eq!(auth.failed, 0);
        assert_eq!(auth.total_duration_ms, 150);
        let api = &reporter.file_results["tests/api/test_users.py"];
        assert_eq!(api.passed, 1);
        assert_eq!(api.failed, 1);
        assert_eq!(api.failures.len(), 1);
        assert_eq!(
            api.failures[0].test_id,
            "tests/api/test_users.py::test_create"
        );
    }

    #[test]
    fn test_tach_reporter_spinner_message_updates() {
        let mut reporter = TachReporter::new();
        reporter.on_run_start(10);
        assert_eq!(reporter.total, 10);
        reporter.on_test_start("f.py::t1", "f.py");
        reporter.on_test_finished("f.py::t1", "pass", 100, None);
        assert_eq!(reporter.passed, 1);
        reporter.on_test_start("f.py::t2", "f.py");
        reporter.on_test_finished("f.py::t2", "fail", 50, Some("boom"));
        assert_eq!(reporter.failed, 1);
        reporter.on_test_start("f.py::t3", "f.py");
        reporter.on_test_finished("f.py::t3", "skip", 10, None);
        assert_eq!(reporter.skipped, 1);
    }

    #[test]
    fn test_tach_reporter_empty_run() {
        let mut reporter = TachReporter::new();
        reporter.on_run_start(0);
        reporter.on_run_finished(0, 0, 0, 100);
    }

    #[test]
    fn test_tach_reporter_all_skipped() {
        let mut reporter = TachReporter::new();
        reporter.on_run_start(2);
        reporter.on_test_start("f.py::t1", "f.py");
        reporter.on_test_finished("f.py::t1", "skip", 10, None);
        reporter.on_test_start("f.py::t2", "f.py");
        reporter.on_test_finished("f.py::t2", "skip", 10, None);
        assert_eq!(reporter.skipped, 2);
        assert_eq!(reporter.passed, 0);
        let file = &reporter.file_results["f.py"];
        assert_eq!(file.skipped, 2);
        assert!(!file.has_failures());

        // File with only skips should NOT count as "passed"
        let files = reporter.sorted_files();
        let failed_files = files.iter().filter(|(_, r)| r.has_failures()).count();
        let skipped_files = files
            .iter()
            .filter(|(_, r)| !r.has_failures() && r.passed == 0)
            .count();
        let passed_files = files.len() - failed_files - skipped_files;
        assert_eq!(passed_files, 0, "all-skip file should not count as passed");
        assert_eq!(skipped_files, 1, "all-skip file should count as skipped");
    }

    #[test]
    fn test_tach_reporter_mixed_pass_and_skip() {
        let mut reporter = TachReporter::new();
        reporter.on_run_start(4);
        // File with mixed pass + skip
        reporter.on_test_start("mix.py::t1", "mix.py");
        reporter.on_test_finished("mix.py::t1", "pass", 100, None);
        reporter.on_test_start("mix.py::t2", "mix.py");
        reporter.on_test_finished("mix.py::t2", "skip", 10, None);
        // File with only passes
        reporter.on_test_start("pass.py::t1", "pass.py");
        reporter.on_test_finished("pass.py::t1", "pass", 50, None);
        // File with only skips
        reporter.on_test_start("skip.py::t1", "skip.py");
        reporter.on_test_finished("skip.py::t1", "skip", 10, None);

        // Verify file-level counts
        let mix = &reporter.file_results["mix.py"];
        assert_eq!(mix.passed, 1);
        assert_eq!(mix.skipped, 1);

        let pass = &reporter.file_results["pass.py"];
        assert_eq!(pass.passed, 1);
        assert_eq!(pass.skipped, 0);

        let skip = &reporter.file_results["skip.py"];
        assert_eq!(skip.passed, 0);
        assert_eq!(skip.skipped, 1);

        // Verify summary-level file classification
        let files = reporter.sorted_files();
        let failed_files = files.iter().filter(|(_, r)| r.has_failures()).count();
        let skipped_files = files
            .iter()
            .filter(|(_, r)| !r.has_failures() && r.passed == 0)
            .count();
        let passed_files = files.len() - failed_files - skipped_files;

        assert_eq!(failed_files, 0);
        assert_eq!(
            passed_files, 2,
            "mix.py and pass.py should count as passed files"
        );
        assert_eq!(skipped_files, 1, "skip.py should count as a skipped file");
    }

    #[test]
    fn test_tach_reporter_crash_timeout_counted_as_failure() {
        let mut reporter = TachReporter::new();
        reporter.on_run_start(3);
        reporter.on_test_start("f.py::t1", "f.py");
        reporter.on_test_finished("f.py::t1", "crash", 100, Some("segfault"));
        reporter.on_test_start("f.py::t2", "f.py");
        reporter.on_test_finished("f.py::t2", "timeout", 5000, Some("exceeded limit"));
        reporter.on_test_start("f.py::t3", "f.py");
        reporter.on_test_finished("f.py::t3", "harness_error", 100, Some("import error"));
        assert_eq!(reporter.failed, 3);
        let file = &reporter.file_results["f.py"];
        assert_eq!(file.failed, 3);
        assert_eq!(file.failures.len(), 3);
    }

    #[test]
    fn test_tach_reporter_short_test_name() {
        assert_eq!(
            TachReporter::short_test_name("tests/foo.py::TestClass::test_method"),
            "test_method"
        );
        assert_eq!(
            TachReporter::short_test_name("tests/foo.py::test_simple"),
            "test_simple"
        );
        assert_eq!(
            TachReporter::short_test_name("tests/foo.py::test_param[1-2-3]"),
            "test_param[1-2-3]"
        );
        assert_eq!(TachReporter::short_test_name("just_a_name"), "just_a_name");
    }

    #[test]
    fn test_tach_reporter_with_traceback_style() {
        let reporter = TachReporter::with_traceback_style(TracebackStyle::Short);
        assert_eq!(reporter.traceback_style, TracebackStyle::Short);
    }

    #[test]
    fn test_tach_reporter_default() {
        let reporter = TachReporter::default();
        assert_eq!(reporter.passed, 0);
        assert_eq!(reporter.failed, 0);
        assert_eq!(reporter.skipped, 0);
        assert!(reporter.file_results.is_empty());
    }

    #[test]
    fn test_tach_reporter_file_ordering_preserved() {
        let mut reporter = TachReporter::new();
        reporter.on_run_start(3);
        reporter.on_test_start("b.py::t1", "b.py");
        reporter.on_test_finished("b.py::t1", "pass", 100, None);
        reporter.on_test_start("a.py::t1", "a.py");
        reporter.on_test_finished("a.py::t1", "pass", 50, None);
        reporter.on_test_start("c.py::t1", "c.py");
        reporter.on_test_finished("c.py::t1", "pass", 75, None);

        let mut files: Vec<(&String, &FileResult)> = reporter.file_results.iter().collect();
        files.sort_by_key(|(_, r)| r.order);
        let names: Vec<&str> = files.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(names, vec!["b.py", "a.py", "c.py"]);
    }

    // =========================================================================
    //  Real-time File Streaming Tests (Batch 2)
    // =========================================================================

    #[test]
    fn test_tach_reporter_streams_file_on_completion() {
        let mut reporter = TachReporter::new();
        let mut counts = HashMap::new();
        counts.insert("tests/test_a.py".to_string(), 2usize);
        counts.insert("tests/test_b.py".to_string(), 1usize);
        reporter.on_session_setup(&counts);
        reporter.on_run_start(3);

        reporter.on_test_start("tests/test_a.py::test_1", "tests/test_a.py");
        reporter.on_test_start("tests/test_a.py::test_2", "tests/test_a.py");
        reporter.on_test_start("tests/test_b.py::test_1", "tests/test_b.py");

        reporter.on_test_finished("tests/test_a.py::test_1", "pass", 100, None);
        assert_eq!(reporter.files_printed, 0); // Not complete yet

        reporter.on_test_finished("tests/test_a.py::test_2", "pass", 200, None);
        assert_eq!(reporter.files_printed, 1); // File a complete

        reporter.on_test_finished("tests/test_b.py::test_1", "pass", 50, None);
        assert_eq!(reporter.files_printed, 2); // File b complete

        assert_eq!(reporter.passed, 3);
    }

    #[test]
    fn test_tach_reporter_no_session_setup_still_works() {
        // If on_session_setup is never called, nothing streams (backward compat)
        let mut reporter = TachReporter::new();
        reporter.on_run_start(2);
        reporter.on_test_start("tests/test_a.py::test_1", "tests/test_a.py");
        reporter.on_test_finished("tests/test_a.py::test_1", "pass", 100, None);
        assert_eq!(reporter.files_printed, 0); // No streaming without setup
    }

    #[test]
    fn test_tach_reporter_streams_failed_file() {
        let mut reporter = TachReporter::new();
        let mut counts = HashMap::new();
        counts.insert("tests/test_c.py".to_string(), 2usize);
        reporter.on_session_setup(&counts);
        reporter.on_run_start(2);

        reporter.on_test_start("tests/test_c.py::test_1", "tests/test_c.py");
        reporter.on_test_start("tests/test_c.py::test_2", "tests/test_c.py");
        reporter.on_test_finished("tests/test_c.py::test_1", "pass", 100, None);
        reporter.on_test_finished(
            "tests/test_c.py::test_2",
            "fail",
            200,
            Some("AssertionError"),
        );
        assert_eq!(reporter.files_printed, 1);
        assert!(reporter.files_streamed.contains("tests/test_c.py"));
    }

    #[test]
    fn test_tach_reporter_unfinished_file_not_streamed() {
        // If a file never reaches expected count (e.g., worker crash), it stays unstreamed
        let mut reporter = TachReporter::new();
        let mut counts = HashMap::new();
        counts.insert("tests/test_d.py".to_string(), 3usize);
        reporter.on_session_setup(&counts);
        reporter.on_run_start(3);

        reporter.on_test_start("tests/test_d.py::test_1", "tests/test_d.py");
        reporter.on_test_start("tests/test_d.py::test_2", "tests/test_d.py");
        reporter.on_test_finished("tests/test_d.py::test_1", "pass", 100, None);
        reporter.on_test_finished("tests/test_d.py::test_2", "pass", 200, None);
        // test_3 never finishes (crash scenario)
        assert_eq!(reporter.files_printed, 0); // Not complete yet
        assert!(!reporter.files_streamed.contains("tests/test_d.py"));
    }

    #[test]
    fn test_tach_reporter_format_file_line_all_pass() {
        let mut reporter = TachReporter::new();
        reporter.on_run_start(2);
        reporter.on_test_start("f.py::t1", "f.py");
        reporter.on_test_finished("f.py::t1", "pass", 100, None);
        reporter.on_test_start("f.py::t2", "f.py");
        reporter.on_test_finished("f.py::t2", "pass", 200, None);

        let result = &reporter.file_results["f.py"];
        let line = reporter.format_file_line("f.py", result, false);
        assert!(line.contains("\u{2713}"), "Should contain check mark");
        assert!(line.contains("f.py"), "Should contain file path");
        assert!(line.contains("(2)"), "Should contain total count");
    }

    #[test]
    fn test_tach_reporter_crash_prints_unstreamed_files() {
        let mut reporter = TachReporter::new();
        let mut counts = HashMap::new();
        counts.insert("tests/test_a.py".to_string(), 3usize);
        reporter.on_session_setup(&counts);
        reporter.on_run_start(3);

        reporter.on_test_start("tests/test_a.py::test_1", "tests/test_a.py");
        reporter.on_test_start("tests/test_a.py::test_2", "tests/test_a.py");
        reporter.on_test_start("tests/test_a.py::test_3", "tests/test_a.py");

        reporter.on_test_finished("tests/test_a.py::test_1", "pass", 100, None);
        reporter.on_test_finished("tests/test_a.py::test_2", "crash", 200, Some("worker died"));
        // test_3 never finishes — worker crash took it out

        assert_eq!(reporter.files_printed, 0); // File NOT streamed (only 2/3 done)

        // on_run_finished should print the file in the final output (via render_file_list)
        reporter.on_run_finished(1, 1, 0, 300);
        // No assertion needed — just verify it doesn't panic
        // The file should appear in final output because it's NOT in files_streamed
        assert!(
            !reporter.files_streamed.contains("tests/test_a.py"),
            "crashed file should not be in files_streamed"
        );
    }

    #[test]
    fn test_tach_reporter_format_file_line_with_failures() {
        let mut reporter = TachReporter::new();
        reporter.on_run_start(2);
        reporter.on_test_start("f.py::t1", "f.py");
        reporter.on_test_finished("f.py::t1", "pass", 100, None);
        reporter.on_test_start("f.py::t2", "f.py");
        reporter.on_test_finished("f.py::t2", "fail", 200, Some("boom"));

        let result = &reporter.file_results["f.py"];
        let line = reporter.format_file_line("f.py", result, false);
        assert!(line.contains("\u{00d7}"), "Should contain x mark");
        assert!(
            line.contains("1 passed | 1 failed"),
            "Should show pass/fail counts"
        );
    }
}
