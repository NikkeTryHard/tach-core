//! Ratatui TUI Reporter — Full-screen Vitest-style test result display
//!
//! Shows real-time streaming test results per-test with inline failure
//! details, active worker progress, and aggregate status.
//!
//! ## Layout
//!
//! ```text
//! ┌──────────────────────────────────────────────────┐
//! │ RUN  v0.3.1                            00:03.21  │  ← Header badge
//! │──────────────────────────────────────────────────│
//! │  ✓ tests/test_foo.py::test_a              23ms  │  ← Scrolling results
//! │  ✓ tests/test_foo.py::test_b              11ms  │    (per-test)
//! │  × tests/test_bar.py::test_broken        142ms  │
//! │      AssertionError: expected 1 got 2            │  ← Inline traceback
//! │  ↓ tests/test_baz.py::test_skip            0ms  │
//! │──────────────────────────────────────────────────│
//! │ ❯ tests/test_baz.py              3/8            │  ← Active workers
//! │──────────────────────────────────────────────────│
//! │ Test Files  1 passed | 1 failed (2)              │  ← Status bar
//! │ Tests       5 passed | 1 failed (6/10)           │
//! │ Duration    1.24s                                │
//! └──────────────────────────────────────────────────┘
//! ```

use crate::config::TracebackStyle;
use crate::reporting::reporter::{PhaseDetail, Reporter, format_traceback};

use ratatui::{
    DefaultTerminal, Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem, ListState, Paragraph},
};

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

// =============================================================================
// Style Constants
// =============================================================================

const PASS_STYLE: Style = Style::new().fg(Color::Green);
const FAIL_STYLE: Style = Style::new().fg(Color::Red);
const SKIP_STYLE: Style = Style::new().fg(Color::Yellow);
const DIM_STYLE: Style = Style::new().fg(Color::DarkGray);
const HEADER_STYLE: Style = Style::new().add_modifier(Modifier::BOLD);
const FAIL_BOLD: Style = Style::new().fg(Color::Red).add_modifier(Modifier::BOLD);
const CYAN_STYLE: Style = Style::new().fg(Color::Cyan);
const YELLOW_STYLE: Style = Style::new().fg(Color::Yellow);
const GREEN_STYLE: Style = Style::new().fg(Color::Green);

const SPINNER_CHARS: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const CHECK: &str = "✓";
const CROSS: &str = "×";
const SKIP_ARROW: &str = "↓";
const POINTER: &str = "❯";

const PASS_BOLD: Style = Style::new().fg(Color::Green).add_modifier(Modifier::BOLD);
const SKIP_BOLD: Style = Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD);
const RUN_BADGE: Style = Style::new()
    .fg(Color::White)
    .bg(Color::Green)
    .add_modifier(Modifier::BOLD);

// =============================================================================
// Internal Data Types
// =============================================================================

/// Current lifecycle phase of the test run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Idle,
    Scanning,
    Compiling,
    Resolving,
    Booting,
    Running,
    Finished,
}

/// Result status for a single test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TestStatus {
    Pass,
    Fail,
    Skip,
}

/// A single test result displayed in the scrolling results area.
struct TestResultEntry {
    test_id: String,
    file_path: String,
    status: TestStatus,
    duration_ms: u64,
    traceback: Option<String>,
}

/// Accumulated results for a file still being tested (main-thread only).
struct FileAccumulator {
    _order: usize,
    total_completed: usize,
}

impl FileAccumulator {
    fn new(order: usize) -> Self {
        Self {
            _order: order,
            total_completed: 0,
        }
    }
}

/// A file currently being tested by a worker (shared state).
struct ActiveFile {
    completed: usize,
    total: usize,
}

/// Shared state between main thread and tick thread.
struct SharedState {
    phase: Phase,
    test_results: Vec<TestResultEntry>,
    active_files: Vec<(String, ActiveFile)>,
    passed: usize,
    failed: usize,
    skipped: usize,
    total_tests: usize,
    total_files: usize,
    files_completed: usize,
    spinner_frame: usize,
    start_time: Option<Instant>,
    log_path: Option<String>,
    total_display_lines: usize,
    phase_detail: Option<String>,
}

impl SharedState {
    fn new() -> Self {
        Self {
            phase: Phase::Idle,
            test_results: Vec::new(),
            active_files: Vec::new(),
            passed: 0,
            failed: 0,
            skipped: 0,
            total_tests: 0,
            total_files: 0,
            files_completed: 0,
            spinner_frame: 0,
            start_time: None,
            log_path: None,
            total_display_lines: 0,
            phase_detail: None,
        }
    }

    fn elapsed_str(&self) -> String {
        match self.start_time {
            Some(t) => {
                let elapsed = t.elapsed();
                let secs = elapsed.as_secs();
                let millis = elapsed.subsec_millis();
                format!("{:02}:{:02}.{:02}", secs / 60, secs % 60, millis / 10)
            }
            None => "00:00.00".to_string(),
        }
    }

    fn completed_count(&self) -> usize {
        self.passed + self.failed + self.skipped
    }

    /// Derive file-level stats from test_results.
    fn file_stats(&self) -> (usize, usize, usize) {
        let mut failed_files: HashSet<&str> = HashSet::new();
        let mut has_pass: HashSet<&str> = HashSet::new();
        let mut skip_only: HashSet<&str> = HashSet::new();

        for r in &self.test_results {
            match r.status {
                TestStatus::Fail => {
                    failed_files.insert(&r.file_path);
                }
                TestStatus::Pass => {
                    has_pass.insert(&r.file_path);
                }
                TestStatus::Skip => {
                    skip_only.insert(&r.file_path);
                }
            }
        }

        let failed = failed_files.len();
        // Skip-only files: files that have skips but no passes and no failures
        let skipped = skip_only
            .iter()
            .filter(|f| !has_pass.contains(*f) && !failed_files.contains(*f))
            .count();
        let passed = self
            .files_completed
            .saturating_sub(failed)
            .saturating_sub(skipped);
        (passed, failed, skipped)
    }
}

// =============================================================================
// RatatuiReporter
// =============================================================================

/// Full-screen ratatui TUI reporter implementing the Reporter trait.
pub struct RatatuiReporter {
    state: Arc<Mutex<SharedState>>,
    terminal: Arc<Mutex<Option<DefaultTerminal>>>,
    tick_handle: Option<JoinHandle<()>>,
    tick_stop: Arc<AtomicBool>,

    // Main-thread-only state (not shared with tick thread)
    file_expected: HashMap<String, usize>,
    file_results: HashMap<String, FileAccumulator>,
    test_to_file: HashMap<String, String>,
    /// Set of files already marked complete (to avoid double-counting).
    files_done: HashSet<String>,
    file_order: usize,
    traceback_style: TracebackStyle,
    restored: bool,
    first_test_seen: bool,
    tui_started: bool,
}

impl RatatuiReporter {
    /// Create a new RatatuiReporter with default traceback style.
    pub fn new() -> Self {
        Self::with_traceback_style(TracebackStyle::Long)
    }

    /// Create a new RatatuiReporter with a specific traceback style.
    #[must_use]
    pub fn with_traceback_style(traceback_style: TracebackStyle) -> Self {
        Self {
            state: Arc::new(Mutex::new(SharedState::new())),
            terminal: Arc::new(Mutex::new(None)),
            tick_handle: None,
            tick_stop: Arc::new(AtomicBool::new(false)),
            file_expected: HashMap::new(),
            file_results: HashMap::new(),
            test_to_file: HashMap::new(),
            files_done: HashSet::new(),
            file_order: 0,
            traceback_style,
            restored: false,
            first_test_seen: false,
            tui_started: false,
        }
    }

    /// Extract the short test name from a fully-qualified test ID.
    #[cfg(test)]
    fn short_test_name(test_id: &str) -> &str {
        test_id.rsplit("::").next().unwrap_or(test_id)
    }

    /// Format duration for display.
    fn format_duration(ms: u64) -> String {
        if ms < 1000 {
            format!("{}ms", ms)
        } else {
            format!("{:.2}s", ms as f64 / 1000.0)
        }
    }

    /// Check if a status string represents a passing test.
    fn is_pass(status: &str) -> bool {
        status.eq_ignore_ascii_case("pass")
    }

    /// Check if a status string represents a skipped test.
    fn is_skip(status: &str) -> bool {
        status.eq_ignore_ascii_case("skip")
    }

    /// Initialize the terminal (enter alternate screen).
    fn init_terminal(&mut self) {
        let term = ratatui::init();
        let mut t = self.terminal.lock().unwrap_or_else(|e| e.into_inner());
        *t = Some(term);
    }

    /// Restore the terminal (leave alternate screen).
    fn restore_terminal(&mut self) {
        if self.restored {
            return;
        }
        self.restored = true;

        // Stop tick thread
        self.tick_stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.tick_handle.take() {
            let _ = handle.join();
        }

        // Drop the terminal (ratatui::restore happens via Drop)
        let mut t = self.terminal.lock().unwrap_or_else(|e| e.into_inner());
        if t.is_some() {
            drop(t.take());
            // Also explicitly restore in case Drop didn't
            ratatui::restore();
        }
    }

    /// Spawn the background tick thread for elapsed time + spinner animation.
    fn spawn_tick_thread(&mut self) {
        let state = Arc::clone(&self.state);
        let terminal = Arc::clone(&self.terminal);
        let stop = Arc::clone(&self.tick_stop);

        self.tick_handle = Some(thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                thread::sleep(Duration::from_millis(100));
                if stop.load(Ordering::SeqCst) {
                    break;
                }

                // Update spinner frame
                {
                    let mut s = state.lock().unwrap_or_else(|e| e.into_inner());
                    s.spinner_frame = (s.spinner_frame + 1) % SPINNER_CHARS.len();
                }

                // Render
                let state_clone = Arc::clone(&state);
                let mut t = terminal.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(ref mut term) = *t {
                    let _ = term.draw(|frame| {
                        let s = state_clone.lock().unwrap_or_else(|e| e.into_inner());
                        render_frame(frame, &s);
                    });
                }
            }
        }));
    }

    /// Trigger a render from the main thread.
    fn render(&self) {
        let state = Arc::clone(&self.state);
        let mut t = self.terminal.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(ref mut term) = *t {
            let _ = term.draw(|frame| {
                let s = state.lock().unwrap_or_else(|e| e.into_inner());
                render_frame(frame, &s);
            });
        }
    }

    /// Print final summary after restoring terminal.
    fn print_final_summary(&self) {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());

        // Print test-level results
        for result in &state.test_results {
            let duration = Self::format_duration(result.duration_ms);
            match result.status {
                TestStatus::Pass => {
                    println!(" \x1b[32m{}\x1b[0m {}  {}", CHECK, result.test_id, duration);
                }
                TestStatus::Fail => {
                    println!(" \x1b[31m{}\x1b[0m {}  {}", CROSS, result.test_id, duration);
                }
                TestStatus::Skip => {
                    println!(
                        " \x1b[33m{}\x1b[0m {}  {}",
                        SKIP_ARROW, result.test_id, duration
                    );
                }
            }
        }

        // Summary block
        let (passed_files, failed_files, skipped_files) = state.file_stats();
        let total_files = state.files_completed;

        println!();

        // Test Files line
        let mut file_parts: Vec<String> = Vec::new();
        if passed_files > 0 {
            file_parts.push(format!("\x1b[32m{} passed\x1b[0m", passed_files));
        }
        if failed_files > 0 {
            file_parts.push(format!("\x1b[31m{} failed\x1b[0m", failed_files));
        }
        if skipped_files > 0 {
            file_parts.push(format!("{} skipped", skipped_files));
        }
        let file_counts = if file_parts.is_empty() {
            "0".to_string()
        } else {
            file_parts.join(" | ")
        };
        println!(" Test Files  {} ({})", file_counts, total_files);

        // Tests line
        let test_total = state.passed + state.failed + state.skipped;
        let mut test_parts: Vec<String> = Vec::new();
        if state.passed > 0 {
            test_parts.push(format!("\x1b[32m{} passed\x1b[0m", state.passed));
        }
        if state.failed > 0 {
            test_parts.push(format!("\x1b[31m{} failed\x1b[0m", state.failed));
        }
        if state.skipped > 0 {
            test_parts.push(format!("{} skipped", state.skipped));
        }
        let test_counts = if test_parts.is_empty() {
            "0".to_string()
        } else {
            test_parts.join(" | ")
        };
        println!("      Tests  {} ({})", test_counts, test_total);

        // Duration line
        println!("   Duration  {}", state.elapsed_str());

        // Log file
        if let Some(ref path) = state.log_path {
            println!("   Log file  {}", path);
        }

        // Failures block
        let all_failures: Vec<&TestResultEntry> = state
            .test_results
            .iter()
            .filter(|r| r.status == TestStatus::Fail && r.traceback.is_some())
            .collect();

        if !all_failures.is_empty() && self.traceback_style != TracebackStyle::No {
            println!();
            println!("{} FAILURES {}", "=".repeat(30), "=".repeat(30));
            for failure in &all_failures {
                println!();
                println!("\x1b[1;31mFAIL\x1b[0m > {}", failure.test_id);
                println!("{}", "-".repeat(failure.test_id.len().min(70) + 7));
                if let Some(ref tb) = failure.traceback {
                    for line in tb.lines().take(20) {
                        println!("{}", line);
                    }
                }
            }
            println!("{}", "=".repeat(70));
        }
    }
}

impl Default for RatatuiReporter {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for RatatuiReporter {
    fn drop(&mut self) {
        if !self.restored {
            self.restore_terminal();
        }
    }
}

// =============================================================================
// Reporter Trait Implementation
// =============================================================================

impl Reporter for RatatuiReporter {
    fn on_session_setup(&mut self, file_counts: &HashMap<String, usize>) {
        self.file_expected = file_counts.clone();
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.total_files = file_counts.len();
    }

    fn on_phase(&mut self, phase: &str, detail: Option<&PhaseDetail>) {
        let new_phase = match phase {
            "scanning" => Phase::Scanning,
            "compiling" => Phase::Compiling,
            "resolving" => Phase::Resolving,
            "booting" => Phase::Booting,
            _ => return,
        };

        let detail_str = detail.map(|d| {
            if d.total > 0 && d.total != d.current {
                format!("{}/{} {}", d.current, d.total, d.label)
            } else {
                format!("{} {}", d.current, d.label)
            }
        });

        // Init TUI on first phase call
        if !self.tui_started {
            self.tui_started = true;
            {
                let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
                state.start_time = Some(Instant::now());
            }
            self.init_terminal();
            self.spawn_tick_thread();
        }

        {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            if new_phase != state.phase {
                state.phase_detail = None;
            }
            state.phase = new_phase;
            if detail_str.is_some() {
                state.phase_detail = detail_str;
            }
        }
        self.render();
    }

    fn on_run_start(&mut self, count: usize) {
        {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            state.total_tests = count;
            state.phase = Phase::Booting;
            if state.start_time.is_none() {
                state.start_time = Some(Instant::now());
            }
        }
        if !self.tui_started {
            self.tui_started = true;
            self.init_terminal();
            self.spawn_tick_thread();
        }
        self.render();
    }

    fn on_test_start(&mut self, id: &str, file: &str) {
        self.test_to_file.insert(id.to_string(), file.to_string());

        let order = self.file_order;
        self.file_results
            .entry(file.to_string())
            .or_insert_with(|| {
                self.file_order = order + 1;
                FileAccumulator::new(order)
            });

        // Update active files in shared state
        let expected = self.file_expected.get(file).copied().unwrap_or(0);
        let current_completed = self
            .file_results
            .get(file)
            .map(|f| f.total_completed)
            .unwrap_or(0);

        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());

        // Transition to Running on first test
        if !self.first_test_seen {
            self.first_test_seen = true;
            state.phase = Phase::Running;
        }

        // Update or insert active file
        if let Some((_, af)) = state.active_files.iter_mut().find(|(p, _)| p == file) {
            af.completed = current_completed;
            af.total = expected;
        } else {
            state.active_files.push((
                file.to_string(),
                ActiveFile {
                    completed: current_completed,
                    total: expected,
                },
            ));
        }
        drop(state);

        self.render();
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
        let acc = self.file_results.entry(file.clone()).or_insert_with(|| {
            self.file_order = order + 1;
            FileAccumulator::new(order)
        });
        acc.total_completed += 1;

        // Determine status and build TestResultEntry
        let (test_status, traceback) = if Self::is_pass(status) {
            (TestStatus::Pass, None)
        } else if Self::is_skip(status) {
            (TestStatus::Skip, None)
        } else {
            let formatted_msg = message
                .map(|m| format_traceback(m, id, self.traceback_style))
                .unwrap_or_default();
            let tb = if formatted_msg.is_empty() {
                None
            } else {
                Some(formatted_msg)
            };
            (TestStatus::Fail, tb)
        };

        let entry = TestResultEntry {
            test_id: id.to_string(),
            file_path: file.clone(),
            status: test_status,
            duration_ms,
            traceback,
        };

        // Update shared state
        {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            match test_status {
                TestStatus::Pass => state.passed += 1,
                TestStatus::Fail => state.failed += 1,
                TestStatus::Skip => state.skipped += 1,
            }

            // Calculate display lines for this entry
            let mut lines = 1; // test result line
            if let Some(ref tb) = entry.traceback {
                lines += tb.lines().count().min(20);
            }
            state.total_display_lines += lines;

            state.test_results.push(entry);

            // Update active file progress
            let current_completed = self
                .file_results
                .get(&file)
                .map(|f| f.total_completed)
                .unwrap_or(0);
            if let Some((_, af)) = state.active_files.iter_mut().find(|(p, _)| p == &file) {
                af.completed = current_completed;
            }

            // Check if file is complete
            if let Some(&expected) = self.file_expected.get(&file)
                && current_completed == expected
                && !self.files_done.contains(&file)
            {
                self.files_done.insert(file.clone());
                state.files_completed += 1;
                state.active_files.retain(|(p, _)| p != &file);
            }
        }

        self.render();
    }

    fn on_run_finished(
        &mut self,
        _passed: usize,
        _failed: usize,
        _skipped: usize,
        _duration_ms: u64,
    ) {
        // Mark any remaining files as completed
        {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            state.phase = Phase::Finished;
            // Any files that didn't reach their expected count are still "done"
            for file in self.file_results.keys() {
                if !self.files_done.contains(file) {
                    state.files_completed += 1;
                }
            }
            state.active_files.clear();
        }

        self.restore_terminal();
        self.print_final_summary();
    }

    fn on_error(&mut self, message: &str) {
        self.restore_terminal();
        println!("[tach] FATAL ERROR: {}", message);
    }

    fn set_log_path(&mut self, path: &str) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.log_path = Some(path.to_owned());
    }
}

// =============================================================================
// Rendering (free functions to avoid borrow issues with Mutex)
// =============================================================================

/// Main render function — draws the 4-zone layout.
fn render_frame(frame: &mut Frame, state: &SharedState) {
    let area = frame.area();

    let active_count = state.active_files.len().max(1) as u16;

    let [header_area, content_area, workers_area, status_area] = Layout::vertical([
        Constraint::Length(2),                // header + separator
        Constraint::Fill(1),                  // scrollable results
        Constraint::Length(active_count + 1), // active workers + separator
        Constraint::Length(4),                // status bar (3 lines + border)
    ])
    .areas(area);

    render_header(frame, header_area, state);
    render_results(frame, content_area, state);
    render_workers(frame, workers_area, state);
    render_status(frame, status_area, state);
}

/// Render the header: ` RUN ` badge + version + elapsed time.
fn render_header(frame: &mut Frame, area: Rect, state: &SharedState) {
    let version = env!("CARGO_PKG_VERSION");
    let elapsed = state.elapsed_str();

    let badge = match state.phase {
        Phase::Finished => {
            if state.failed > 0 {
                Span::styled(" FAIL ", FAIL_BOLD)
            } else {
                Span::styled(" PASS ", PASS_BOLD)
            }
        }
        _ => Span::styled(" RUN  ", RUN_BADGE),
    };

    let version_str = format!(" v{}", version);
    let badge_len = badge.width();
    let padding = (area.width as usize)
        .saturating_sub(badge_len)
        .saturating_sub(version_str.len())
        .saturating_sub(elapsed.len())
        .saturating_sub(1);

    let line = Line::from(vec![
        badge,
        Span::styled(version_str, HEADER_STYLE),
        Span::raw(" ".repeat(padding)),
        Span::styled(elapsed, DIM_STYLE),
        Span::raw(" "),
    ]);

    let header = Paragraph::new(line);
    frame.render_widget(header, Rect::new(area.x, area.y, area.width, 1));

    let sep = Paragraph::new(Line::from("─".repeat(area.width as usize))).style(DIM_STYLE);
    frame.render_widget(sep, Rect::new(area.x, area.y + 1, area.width, 1));
}

/// Render the scrolling test results list with Vitest-style formatting.
fn render_results(frame: &mut Frame, area: Rect, state: &SharedState) {
    let mut items: Vec<ListItem> = Vec::new();

    for result in &state.test_results {
        let duration = RatatuiReporter::format_duration(result.duration_ms);

        let mut spans = vec![Span::raw(" ")];

        match result.status {
            TestStatus::Pass => {
                spans.push(Span::styled(CHECK.to_string(), PASS_STYLE));
                spans.push(Span::raw(" "));
                spans.extend(format_test_id(&result.test_id, Style::default()));
                spans.push(Span::raw("  "));
                spans.push(Span::styled(duration, DIM_STYLE));
                items.push(ListItem::new(Line::from(spans)));
            }
            TestStatus::Fail => {
                spans.push(Span::styled(CROSS.to_string(), FAIL_STYLE));
                spans.push(Span::raw(" "));
                spans.extend(format_test_id(&result.test_id, FAIL_STYLE));
                spans.push(Span::raw("  "));
                spans.push(Span::styled(duration, DIM_STYLE));
                items.push(ListItem::new(Line::from(spans)));

                if let Some(ref tb) = result.traceback {
                    for tb_line in tb.lines().take(20) {
                        let styled = style_traceback_line(tb_line);
                        items.push(ListItem::new(styled));
                    }
                }
            }
            TestStatus::Skip => {
                spans.push(Span::styled(SKIP_ARROW.to_string(), SKIP_STYLE));
                spans.push(Span::raw(" "));
                spans.extend(format_test_id(&result.test_id, DIM_STYLE));
                spans.push(Span::raw("  "));
                spans.push(Span::styled(duration, DIM_STYLE));
                items.push(ListItem::new(Line::from(spans)));
            }
        }
    }

    let total_items = items.len();
    let mut list_state = ListState::default();
    if total_items > 0 {
        list_state.select(Some(total_items.saturating_sub(1)));
    }

    let list = List::new(items);
    frame.render_stateful_widget(list, area, &mut list_state);
}

/// Render active workers with spinner + file path + progress.
/// Shows phase-appropriate messages when no active files.
fn render_workers(frame: &mut Frame, area: Rect, state: &SharedState) {
    // Top separator
    let sep = Paragraph::new(Line::from("─".repeat(area.width as usize))).style(DIM_STYLE);
    frame.render_widget(sep, Rect::new(area.x, area.y, area.width, 1));

    let spinner = SPINNER_CHARS[state.spinner_frame % SPINNER_CHARS.len()];

    if state.active_files.is_empty() {
        let base_msg = match state.phase {
            Phase::Idle => "initializing...",
            Phase::Scanning => "Scanning for tests...",
            Phase::Compiling => "Compiling bytecode...",
            Phase::Resolving => "Resolving fixtures...",
            Phase::Booting => "Booting zygote...",
            Phase::Running => "waiting...",
            Phase::Finished => "done",
        };
        let msg = if let Some(ref detail) = state.phase_detail {
            format!("{} ({})", base_msg, detail)
        } else {
            base_msg.to_string()
        };
        let msg_style = match state.phase {
            Phase::Scanning | Phase::Compiling | Phase::Resolving | Phase::Booting => CYAN_STYLE,
            _ => DIM_STYLE,
        };
        let line = Line::from(vec![
            Span::raw(" "),
            Span::styled(spinner, SKIP_STYLE),
            Span::raw(" "),
            Span::styled(msg, msg_style),
        ]);
        let p = Paragraph::new(line);
        frame.render_widget(p, Rect::new(area.x, area.y + 1, area.width, 1));
    } else {
        for (i, (file_path, active)) in state.active_files.iter().enumerate() {
            if (i as u16 + 1) >= area.height {
                break;
            }
            let progress = if active.total > 0 {
                format!("{}/{}", active.completed, active.total)
            } else {
                format!("{}", active.completed)
            };

            let path_max = (area.width as usize).saturating_sub(progress.len() + 6);
            let display_path = if file_path.len() > path_max {
                format!(
                    "...{}",
                    &file_path[file_path.len().saturating_sub(path_max.saturating_sub(3))..]
                )
            } else {
                file_path.clone()
            };
            let padding = path_max.saturating_sub(display_path.len());

            let line = Line::from(vec![
                Span::raw(" "),
                Span::styled(POINTER, YELLOW_STYLE),
                Span::raw(" "),
                Span::styled(display_path, Style::default()),
                Span::raw(" ".repeat(padding)),
                Span::styled(progress, DIM_STYLE),
                Span::raw(" "),
            ]);
            let p = Paragraph::new(line);
            frame.render_widget(p, Rect::new(area.x, area.y + 1 + i as u16, area.width, 1));
        }
    }
}

/// Render the bottom status bar.
fn render_status(frame: &mut Frame, area: Rect, state: &SharedState) {
    // Top border
    let border = Paragraph::new(Line::from("─".repeat(area.width as usize))).style(DIM_STYLE);
    frame.render_widget(border, Rect::new(area.x, area.y, area.width, 1));

    // Derive file-level stats from test results
    let (passed_files, failed_files, skipped_files) = state.file_stats();
    let total_files_done = state.files_completed;

    // Line 1: Test Files
    let mut file_spans: Vec<Span> = vec![Span::raw(" Test Files  ")];
    let mut first = true;
    if passed_files > 0 {
        file_spans.push(Span::styled(format!("{}", passed_files), PASS_BOLD));
        file_spans.push(Span::styled(" passed", PASS_STYLE));
        first = false;
    }
    if failed_files > 0 {
        if !first {
            file_spans.push(Span::styled(" | ", DIM_STYLE));
        }
        file_spans.push(Span::styled(format!("{}", failed_files), FAIL_BOLD));
        file_spans.push(Span::styled(" failed", FAIL_STYLE));
        first = false;
    }
    if skipped_files > 0 {
        if !first {
            file_spans.push(Span::styled(" | ", DIM_STYLE));
        }
        file_spans.push(Span::styled(format!("{}", skipped_files), SKIP_BOLD));
        file_spans.push(Span::styled(" skipped", SKIP_STYLE));
    }
    let running = state.active_files.len();
    if running > 0 {
        file_spans.push(Span::styled(
            format!(" ({} + {} running)", total_files_done, running),
            DIM_STYLE,
        ));
    } else if state.total_files > 0 {
        file_spans.push(Span::styled(
            format!(" ({}/{})", total_files_done, state.total_files),
            DIM_STYLE,
        ));
    } else {
        file_spans.push(Span::styled(format!(" ({})", total_files_done), DIM_STYLE));
    }
    let file_line = Paragraph::new(Line::from(file_spans));
    frame.render_widget(file_line, Rect::new(area.x, area.y + 1, area.width, 1));

    // Line 2: Tests
    let mut test_spans: Vec<Span> = vec![Span::raw("      Tests  ")];
    first = true;
    if state.passed > 0 {
        test_spans.push(Span::styled(format!("{}", state.passed), PASS_BOLD));
        test_spans.push(Span::styled(" passed", PASS_STYLE));
        first = false;
    }
    if state.failed > 0 {
        if !first {
            test_spans.push(Span::styled(" | ", DIM_STYLE));
        }
        test_spans.push(Span::styled(format!("{}", state.failed), FAIL_BOLD));
        test_spans.push(Span::styled(" failed", FAIL_STYLE));
        first = false;
    }
    if state.skipped > 0 {
        if !first {
            test_spans.push(Span::styled(" | ", DIM_STYLE));
        }
        test_spans.push(Span::styled(format!("{}", state.skipped), SKIP_BOLD));
        test_spans.push(Span::styled(" skipped", SKIP_STYLE));
    }
    test_spans.push(Span::styled(
        format!(" ({}/{})", state.completed_count(), state.total_tests),
        DIM_STYLE,
    ));
    let test_line = Paragraph::new(Line::from(test_spans));
    frame.render_widget(test_line, Rect::new(area.x, area.y + 2, area.width, 1));

    // Line 3: Duration
    let duration_line = Paragraph::new(Line::from(vec![
        Span::raw("   Duration  "),
        Span::styled(state.elapsed_str(), Style::default()),
    ]));
    frame.render_widget(duration_line, Rect::new(area.x, area.y + 3, area.width, 1));
}

/// Format a file path Vitest-style: `dim(dir/) + bold(basename) + dim(.ext)`
fn format_file_path(path: &str) -> Vec<Span<'static>> {
    let bold_style = Style::new().add_modifier(Modifier::BOLD);

    let (dir, filename) = match path.rfind('/') {
        Some(pos) => (&path[..=pos], &path[pos + 1..]),
        None => ("", path),
    };

    let (stem, ext) = match filename.rfind('.') {
        Some(pos) => (&filename[..pos], &filename[pos..]),
        None => (filename, ""),
    };

    let mut spans = Vec::new();
    if !dir.is_empty() {
        spans.push(Span::styled(dir.to_string(), DIM_STYLE));
    }
    spans.push(Span::styled(stem.to_string(), bold_style));
    if !ext.is_empty() {
        spans.push(Span::styled(ext.to_string(), DIM_STYLE));
    }
    spans
}

fn format_test_id(test_id: &str, test_name_style: Style) -> Vec<Span<'static>> {
    match test_id.find("::") {
        Some(pos) => {
            let file_path = &test_id[..pos];
            let test_name = &test_id[pos..];
            let mut spans = format_file_path(file_path);
            spans.push(Span::styled(test_name.to_string(), test_name_style));
            spans
        }
        None => vec![Span::styled(test_id.to_string(), test_name_style)],
    }
}

/// Style a single traceback line for ratatui display.
fn style_traceback_line(line: &str) -> Line<'static> {
    let owned = line.to_string();
    let trimmed = owned.trim_start();

    // Assertion line (>>> prefix)
    if trimmed.starts_with(">>>") {
        return Line::from(vec![Span::styled(format!("     {}", owned), FAIL_BOLD)]);
    }

    // Python traceback frame: File "path", line N, in func
    if owned.contains("File \"") && owned.contains(", line ") {
        return style_traceback_frame(&owned);
    }

    // Error/Exception lines
    if trimmed
        .chars()
        .next()
        .map(|c| c.is_uppercase())
        .unwrap_or(false)
        && (trimmed.contains("Error")
            || trimmed.contains("Exception")
            || trimmed.contains("Failed"))
    {
        return Line::from(vec![Span::styled(format!("     {}", owned), FAIL_STYLE)]);
    }

    // Section headers
    if trimmed == "Source context:" || trimmed == "Local variables:" || trimmed == "Traceback:" {
        return Line::from(vec![Span::styled(format!("     {}", owned), YELLOW_STYLE)]);
    }

    // Default: dim traceback content
    Line::from(vec![Span::styled(format!("     {}", owned), DIM_STYLE)])
}

/// Style a Python traceback frame line: File "path", line N, in func
fn style_traceback_frame(line: &str) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    spans.push(Span::raw("     "));

    let line = line.to_string();

    // Parse: File "path", line N, in func
    if let Some(file_start) = line.find("File \"") {
        spans.push(Span::styled(line[..file_start].to_string(), DIM_STYLE));
        spans.push(Span::raw("File \""));

        let after_file = &line[file_start + 6..];
        if let Some(file_end) = after_file.find('"') {
            // File path in cyan
            spans.push(Span::styled(after_file[..file_end].to_string(), CYAN_STYLE));
            spans.push(Span::raw("\""));

            let remaining = &after_file[file_end + 1..];
            if let Some(line_start) = remaining.find(", line ") {
                spans.push(Span::raw(remaining[..line_start].to_string()));
                spans.push(Span::raw(", line "));

                let after_line = &remaining[line_start + 7..];
                let line_end = after_line
                    .find(|c: char| !c.is_ascii_digit())
                    .unwrap_or(after_line.len());

                // Line number in yellow
                spans.push(Span::styled(
                    after_line[..line_end].to_string(),
                    YELLOW_STYLE,
                ));

                let rest = &after_line[line_end..];
                if let Some(in_start) = rest.find(", in ") {
                    spans.push(Span::raw(rest[..in_start].to_string()));
                    spans.push(Span::raw(", in "));
                    // Function name in green
                    spans.push(Span::styled(
                        rest[in_start + 5..].trim_end().to_string(),
                        GREEN_STYLE,
                    ));
                } else {
                    spans.push(Span::raw(rest.to_string()));
                }
            } else {
                spans.push(Span::raw(remaining.to_string()));
            }
        } else {
            spans.push(Span::raw(after_file.to_string()));
        }
    } else {
        spans.push(Span::styled(line.to_string(), DIM_STYLE));
    }

    Line::from(spans)
}

// =============================================================================
// Unit Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ratatui_reporter_creation() {
        let r = RatatuiReporter::new();
        let state = r.state.lock().unwrap();
        assert_eq!(state.passed, 0);
        assert_eq!(state.failed, 0);
        assert_eq!(state.skipped, 0);
        assert_eq!(state.total_tests, 0);
        assert_eq!(state.phase, Phase::Idle);
    }

    #[test]
    fn test_ratatui_reporter_with_traceback_style() {
        let r = RatatuiReporter::with_traceback_style(TracebackStyle::Short);
        assert_eq!(r.traceback_style, TracebackStyle::Short);
    }

    #[test]
    fn test_ratatui_reporter_default() {
        let r = RatatuiReporter::default();
        assert_eq!(r.traceback_style, TracebackStyle::Long);
    }

    #[test]
    fn test_restored_flag_prevents_double_restore() {
        let mut r = RatatuiReporter::new();
        r.restored = true;
        // Should not panic on drop
        drop(r);
    }

    #[test]
    fn test_short_test_name() {
        assert_eq!(
            RatatuiReporter::short_test_name("tests/foo.py::TestClass::test_method"),
            "test_method"
        );
        assert_eq!(
            RatatuiReporter::short_test_name("tests/foo.py::test_simple"),
            "test_simple"
        );
        assert_eq!(
            RatatuiReporter::short_test_name("just_a_name"),
            "just_a_name"
        );
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(RatatuiReporter::format_duration(42), "42ms");
        assert_eq!(RatatuiReporter::format_duration(999), "999ms");
        assert_eq!(RatatuiReporter::format_duration(1000), "1.00s");
        assert_eq!(RatatuiReporter::format_duration(1234), "1.23s");
    }

    #[test]
    fn test_is_pass_skip() {
        assert!(RatatuiReporter::is_pass("pass"));
        assert!(RatatuiReporter::is_pass("Pass"));
        assert!(!RatatuiReporter::is_pass("fail"));
        assert!(RatatuiReporter::is_skip("skip"));
        assert!(RatatuiReporter::is_skip("Skip"));
        assert!(!RatatuiReporter::is_skip("pass"));
    }

    #[test]
    fn test_session_setup_stores_file_counts() {
        let mut r = RatatuiReporter::new();
        let mut counts = HashMap::new();
        counts.insert("test_foo.py".to_string(), 5);
        counts.insert("test_bar.py".to_string(), 3);
        r.on_session_setup(&counts);
        assert_eq!(r.file_expected.len(), 2);
        assert_eq!(r.file_expected["test_foo.py"], 5);
        let state = r.state.lock().unwrap();
        assert_eq!(state.total_files, 2);
    }

    #[test]
    fn test_test_start_creates_file_accumulator() {
        let mut r = RatatuiReporter::new();
        r.restored = true;
        r.on_test_start("test_foo.py::test_a", "test_foo.py");
        assert!(r.file_results.contains_key("test_foo.py"));
        assert!(r.test_to_file.contains_key("test_foo.py::test_a"));
    }

    #[test]
    fn test_test_finished_pass_increments_counters() {
        let mut r = RatatuiReporter::new();
        r.restored = true;
        r.on_test_start("test_foo.py::test_a", "test_foo.py");
        r.on_test_finished("test_foo.py::test_a", "pass", 42, None);

        let state = r.state.lock().unwrap();
        assert_eq!(state.passed, 1);
        assert_eq!(state.failed, 0);
        assert_eq!(state.test_results.len(), 1);
        assert_eq!(state.test_results[0].test_id, "test_foo.py::test_a");
        assert_eq!(state.test_results[0].status, TestStatus::Pass);
        assert_eq!(state.test_results[0].duration_ms, 42);
    }

    #[test]
    fn test_test_finished_fail_records_failure() {
        let mut r = RatatuiReporter::new();
        r.restored = true;
        r.on_test_start("test_foo.py::test_a", "test_foo.py");
        r.on_test_finished(
            "test_foo.py::test_a",
            "fail",
            42,
            Some("AssertionError: 1 != 2"),
        );

        let state = r.state.lock().unwrap();
        assert_eq!(state.failed, 1);
        assert_eq!(state.test_results.len(), 1);
        assert_eq!(state.test_results[0].status, TestStatus::Fail);
        assert!(state.test_results[0].traceback.is_some());
    }

    #[test]
    fn test_test_finished_skip_increments_counters() {
        let mut r = RatatuiReporter::new();
        r.restored = true;
        r.on_test_start("test_foo.py::test_a", "test_foo.py");
        r.on_test_finished("test_foo.py::test_a", "skip", 0, None);

        let state = r.state.lock().unwrap();
        assert_eq!(state.skipped, 1);
        assert_eq!(state.test_results.len(), 1);
        assert_eq!(state.test_results[0].status, TestStatus::Skip);
    }

    #[test]
    fn test_crash_timeout_error_counted_as_failure() {
        let mut r = RatatuiReporter::new();
        r.restored = true;

        for status in &["crash", "timeout", "error", "harness_error"] {
            r.on_test_start(&format!("test.py::test_{}", status), "test.py");
            r.on_test_finished(&format!("test.py::test_{}", status), status, 100, None);
        }

        let state = r.state.lock().unwrap();
        assert_eq!(state.failed, 4);
        assert_eq!(state.test_results.len(), 4);
        for result in &state.test_results {
            assert_eq!(result.status, TestStatus::Fail);
        }
    }

    #[test]
    fn test_file_completion_increments_files_completed() {
        let mut r = RatatuiReporter::new();
        r.restored = true;
        let mut counts = HashMap::new();
        counts.insert("test_foo.py".to_string(), 2);
        r.on_session_setup(&counts);

        r.on_test_start("test_foo.py::test_a", "test_foo.py");
        r.on_test_finished("test_foo.py::test_a", "pass", 10, None);
        r.on_test_start("test_foo.py::test_b", "test_foo.py");
        r.on_test_finished("test_foo.py::test_b", "pass", 20, None);

        let state = r.state.lock().unwrap();
        assert_eq!(state.files_completed, 1);
        assert_eq!(state.test_results.len(), 2);
        assert_eq!(state.test_results[0].test_id, "test_foo.py::test_a");
        assert_eq!(state.test_results[1].test_id, "test_foo.py::test_b");
    }

    #[test]
    fn test_file_completion_removes_from_active() {
        let mut r = RatatuiReporter::new();
        r.restored = true;
        let mut counts = HashMap::new();
        counts.insert("test_foo.py".to_string(), 1);
        r.on_session_setup(&counts);

        r.on_test_start("test_foo.py::test_a", "test_foo.py");
        {
            let state = r.state.lock().unwrap();
            assert_eq!(state.active_files.len(), 1);
        }

        r.on_test_finished("test_foo.py::test_a", "pass", 10, None);
        {
            let state = r.state.lock().unwrap();
            assert_eq!(state.active_files.len(), 0);
            assert_eq!(state.files_completed, 1);
        }
    }

    #[test]
    fn test_no_session_setup_no_file_completion() {
        let mut r = RatatuiReporter::new();
        r.restored = true;
        // No on_session_setup call
        r.on_test_start("test_foo.py::test_a", "test_foo.py");
        r.on_test_finished("test_foo.py::test_a", "pass", 10, None);

        let state = r.state.lock().unwrap();
        // Without expected counts, files never auto-complete during run
        assert_eq!(state.files_completed, 0);
        // But test results are still recorded
        assert_eq!(state.test_results.len(), 1);
    }

    #[test]
    fn test_test_ordering_preserved() {
        let mut r = RatatuiReporter::new();
        r.restored = true;
        let mut counts = HashMap::new();
        counts.insert("a.py".to_string(), 1);
        counts.insert("b.py".to_string(), 1);
        counts.insert("c.py".to_string(), 1);
        r.on_session_setup(&counts);

        // Complete in order: b, a, c
        r.on_test_start("b.py::test_1", "b.py");
        r.on_test_finished("b.py::test_1", "pass", 10, None);
        r.on_test_start("a.py::test_1", "a.py");
        r.on_test_finished("a.py::test_1", "pass", 20, None);
        r.on_test_start("c.py::test_1", "c.py");
        r.on_test_finished("c.py::test_1", "pass", 30, None);

        let state = r.state.lock().unwrap();
        // Test results are in chronological order
        assert_eq!(state.test_results[0].test_id, "b.py::test_1");
        assert_eq!(state.test_results[1].test_id, "a.py::test_1");
        assert_eq!(state.test_results[2].test_id, "c.py::test_1");
        assert_eq!(state.files_completed, 3);
    }

    #[test]
    fn test_set_log_path() {
        let mut r = RatatuiReporter::new();
        r.set_log_path("/tmp/tach.log");
        let state = r.state.lock().unwrap();
        assert_eq!(state.log_path.as_deref(), Some("/tmp/tach.log"));
    }

    #[test]
    fn test_elapsed_str_format() {
        let state = SharedState::new();
        assert_eq!(state.elapsed_str(), "00:00.00");
    }

    #[test]
    fn test_style_traceback_line_error() {
        let line = style_traceback_line("AssertionError: expected 1 got 2");
        assert!(!line.spans.is_empty());
    }

    #[test]
    fn test_style_traceback_line_assertion() {
        let line = style_traceback_line(">>> assert x == 1");
        assert!(!line.spans.is_empty());
    }

    #[test]
    fn test_style_traceback_line_file_frame() {
        let line = style_traceback_line(r#"  File "test_foo.py", line 42, in test_bar"#);
        assert!(line.spans.len() >= 3);
    }

    #[test]
    fn test_style_traceback_line_section_header() {
        let line = style_traceback_line("Source context:");
        assert!(!line.spans.is_empty());
    }

    #[test]
    fn test_phase_transitions() {
        let mut r = RatatuiReporter::new();
        r.restored = true;

        {
            let state = r.state.lock().unwrap();
            assert_eq!(state.phase, Phase::Idle);
        }

        let mut counts = HashMap::new();
        counts.insert("test_foo.py".to_string(), 1);
        r.on_session_setup(&counts);
        {
            let state = r.state.lock().unwrap();
            assert_eq!(state.phase, Phase::Idle);
        }

        // Simulate on_phase("scanning") without terminal init
        {
            let mut state = r.state.lock().unwrap();
            state.phase = Phase::Scanning;
            state.start_time = Some(Instant::now());
        }
        {
            let state = r.state.lock().unwrap();
            assert_eq!(state.phase, Phase::Scanning);
        }

        // Simulate on_run_start phase transition
        {
            let mut state = r.state.lock().unwrap();
            state.phase = Phase::Booting;
        }
        {
            let state = r.state.lock().unwrap();
            assert_eq!(state.phase, Phase::Booting);
        }

        // on_test_start transitions to Running
        r.on_test_start("test_foo.py::test_a", "test_foo.py");
        {
            let state = r.state.lock().unwrap();
            assert_eq!(state.phase, Phase::Running);
        }

        // on_run_finished transitions to Finished
        r.on_test_finished("test_foo.py::test_a", "pass", 10, None);
        r.on_run_finished(1, 0, 0, 10);
        {
            let state = r.state.lock().unwrap();
            assert_eq!(state.phase, Phase::Finished);
        }
    }

    #[test]
    fn test_render_with_test_backend() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut state = SharedState::new();
        state.phase = Phase::Running;
        state.total_tests = 10;
        state.passed = 3;
        state.start_time = Some(Instant::now());
        state.test_results.push(TestResultEntry {
            test_id: "test_foo.py::test_a".to_string(),
            file_path: "test_foo.py".to_string(),
            status: TestStatus::Pass,
            duration_ms: 42,
            traceback: None,
        });
        state.test_results.push(TestResultEntry {
            test_id: "test_foo.py::test_b".to_string(),
            file_path: "test_foo.py".to_string(),
            status: TestStatus::Pass,
            duration_ms: 15,
            traceback: None,
        });
        state.test_results.push(TestResultEntry {
            test_id: "test_foo.py::test_c".to_string(),
            file_path: "test_foo.py".to_string(),
            status: TestStatus::Pass,
            duration_ms: 8,
            traceback: None,
        });

        terminal
            .draw(|frame| {
                render_frame(frame, &state);
            })
            .unwrap();

        let buffer = terminal.backend().buffer().clone();
        let content: String = buffer.content.iter().map(|cell| cell.symbol()).collect();
        assert!(content.contains("RUN"));
        assert!(content.contains("test_foo.py::test_a"));
        assert!(content.contains("Tests"));
    }

    #[test]
    fn test_render_empty_state() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let state = SharedState::new();

        // Should not panic
        terminal
            .draw(|frame| {
                render_frame(frame, &state);
            })
            .unwrap();
    }

    #[test]
    fn test_render_with_failures() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut state = SharedState::new();
        state.phase = Phase::Running;
        state.total_tests = 5;
        state.passed = 3;
        state.failed = 1;
        state.start_time = Some(Instant::now());
        state.test_results.push(TestResultEntry {
            test_id: "test_bar.py::test_broken".to_string(),
            file_path: "test_bar.py".to_string(),
            status: TestStatus::Fail,
            duration_ms: 142,
            traceback: Some("AssertionError: expected 1 got 2".to_string()),
        });
        state.active_files.push((
            "test_baz.py".to_string(),
            ActiveFile {
                completed: 3,
                total: 8,
            },
        ));

        terminal
            .draw(|frame| {
                render_frame(frame, &state);
            })
            .unwrap();

        let buffer = terminal.backend().buffer().clone();
        let content: String = buffer.content.iter().map(|cell| cell.symbol()).collect();
        assert!(content.contains("test_bar.py::test_broken"));
        assert!(content.contains("test_baz.py"));
        assert!(content.contains("3/8"));
    }

    #[test]
    fn test_render_with_active_workers() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut state = SharedState::new();
        state.phase = Phase::Running;
        state.total_tests = 20;
        state.start_time = Some(Instant::now());
        state.active_files.push((
            "test_encoding.py".to_string(),
            ActiveFile {
                completed: 3,
                total: 8,
            },
        ));
        state.active_files.push((
            "test_feedparser.py".to_string(),
            ActiveFile {
                completed: 1,
                total: 4,
            },
        ));

        terminal
            .draw(|frame| {
                render_frame(frame, &state);
            })
            .unwrap();

        let buffer = terminal.backend().buffer().clone();
        let content: String = buffer.content.iter().map(|cell| cell.symbol()).collect();
        assert!(content.contains("test_encoding.py"));
        assert!(content.contains("3/8"));
        assert!(content.contains("test_feedparser.py"));
        assert!(content.contains("1/4"));
    }

    #[test]
    fn test_render_booting_phase() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut state = SharedState::new();
        state.phase = Phase::Booting;
        state.total_tests = 10;
        state.start_time = Some(Instant::now());

        terminal
            .draw(|frame| {
                render_frame(frame, &state);
            })
            .unwrap();

        let buffer = terminal.backend().buffer().clone();
        let content: String = buffer.content.iter().map(|cell| cell.symbol()).collect();
        assert!(content.contains("Booting zygote..."));
    }

    #[test]
    fn test_render_scanning_phase() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut state = SharedState::new();
        state.phase = Phase::Scanning;
        state.start_time = Some(Instant::now());

        terminal
            .draw(|frame| {
                render_frame(frame, &state);
            })
            .unwrap();

        let buffer = terminal.backend().buffer().clone();
        let content: String = buffer.content.iter().map(|cell| cell.symbol()).collect();
        assert!(content.contains("Scanning for tests..."));
    }

    #[test]
    fn test_render_skip_results() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut state = SharedState::new();
        state.phase = Phase::Running;
        state.total_tests = 2;
        state.skipped = 1;
        state.passed = 1;
        state.start_time = Some(Instant::now());
        state.test_results.push(TestResultEntry {
            test_id: "test_foo.py::test_a".to_string(),
            file_path: "test_foo.py".to_string(),
            status: TestStatus::Pass,
            duration_ms: 10,
            traceback: None,
        });
        state.test_results.push(TestResultEntry {
            test_id: "test_foo.py::test_b".to_string(),
            file_path: "test_foo.py".to_string(),
            status: TestStatus::Skip,
            duration_ms: 0,
            traceback: None,
        });

        terminal
            .draw(|frame| {
                render_frame(frame, &state);
            })
            .unwrap();

        let buffer = terminal.backend().buffer().clone();
        let content: String = buffer.content.iter().map(|cell| cell.symbol()).collect();
        assert!(content.contains("test_foo.py::test_a"));
        assert!(content.contains("test_foo.py::test_b"));
    }

    #[test]
    fn test_file_stats_derivation() {
        let mut state = SharedState::new();
        state.files_completed = 3;
        state.test_results.push(TestResultEntry {
            test_id: "a.py::test_1".to_string(),
            file_path: "a.py".to_string(),
            status: TestStatus::Pass,
            duration_ms: 10,
            traceback: None,
        });
        state.test_results.push(TestResultEntry {
            test_id: "b.py::test_1".to_string(),
            file_path: "b.py".to_string(),
            status: TestStatus::Fail,
            duration_ms: 20,
            traceback: Some("error".to_string()),
        });
        state.test_results.push(TestResultEntry {
            test_id: "c.py::test_1".to_string(),
            file_path: "c.py".to_string(),
            status: TestStatus::Skip,
            duration_ms: 0,
            traceback: None,
        });

        let (passed, failed, skipped) = state.file_stats();
        assert_eq!(passed, 1); // a.py
        assert_eq!(failed, 1); // b.py
        assert_eq!(skipped, 1); // c.py (skip-only)
    }

    #[test]
    fn test_first_test_start_transitions_to_running() {
        let mut r = RatatuiReporter::new();
        r.restored = true;

        // Simulate booting phase
        {
            let mut state = r.state.lock().unwrap();
            state.phase = Phase::Booting;
        }

        assert!(!r.first_test_seen);
        r.on_test_start("test_foo.py::test_a", "test_foo.py");
        assert!(r.first_test_seen);

        let state = r.state.lock().unwrap();
        assert_eq!(state.phase, Phase::Running);
    }

    #[test]
    fn test_render_scanning_with_phase_detail() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut state = SharedState::new();
        state.phase = Phase::Scanning;
        state.start_time = Some(Instant::now());
        state.phase_detail = Some("142 tests".to_string());

        terminal
            .draw(|frame| {
                render_frame(frame, &state);
            })
            .unwrap();

        let buffer = terminal.backend().buffer().clone();
        let content: String = buffer.content.iter().map(|cell| cell.symbol()).collect();
        assert!(content.contains("Scanning for tests..."));
        assert!(content.contains("142 tests"));
    }

    #[test]
    fn test_render_compiling_with_phase_detail() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut state = SharedState::new();
        state.phase = Phase::Compiling;
        state.start_time = Some(Instant::now());
        state.phase_detail = Some("38/142 files".to_string());

        terminal
            .draw(|frame| {
                render_frame(frame, &state);
            })
            .unwrap();

        let buffer = terminal.backend().buffer().clone();
        let content: String = buffer.content.iter().map(|cell| cell.symbol()).collect();
        assert!(content.contains("Compiling bytecode..."));
        assert!(content.contains("38/142 files"));
    }

    #[test]
    fn test_phase_detail_persists_within_same_phase() {
        let mut state = SharedState::new();
        state.phase = Phase::Scanning;
        state.phase_detail = Some("142 tests".to_string());

        // Same phase, no new detail — detail persists (on_phase skips None updates)
        assert_eq!(state.phase, Phase::Scanning);
        assert_eq!(state.phase_detail.as_deref(), Some("142 tests"));
    }

    #[test]
    fn test_phase_detail_clears_on_phase_transition() {
        let mut state = SharedState::new();
        state.phase = Phase::Scanning;
        state.phase_detail = Some("142 tests".to_string());

        // Simulate on_phase("compiling", None): phase changes → detail clears
        let new_phase = Phase::Compiling;
        if new_phase != state.phase {
            state.phase_detail = None;
        }
        state.phase = new_phase;

        assert_eq!(state.phase, Phase::Compiling);
        assert!(state.phase_detail.is_none());
    }

    #[test]
    fn test_phase_detail_updates_on_same_phase_with_detail() {
        let mut state = SharedState::new();
        state.phase = Phase::Compiling;
        state.phase_detail = None;

        // Simulate on_phase("compiling", Some(detail))
        let detail_str = Some("38/142 files".to_string());
        let new_phase = Phase::Compiling;
        if new_phase != state.phase {
            state.phase_detail = None;
        }
        state.phase = new_phase;
        if detail_str.is_some() {
            state.phase_detail = detail_str;
        }

        assert_eq!(state.phase_detail.as_deref(), Some("38/142 files"));
    }

    #[test]
    fn test_format_file_path_with_directory() {
        let spans = format_file_path("tests/unit/test_foo.py");
        assert_eq!(spans.len(), 3);
        assert_eq!(spans[0].content, "tests/unit/");
        assert_eq!(spans[1].content, "test_foo");
        assert_eq!(spans[2].content, ".py");
    }

    #[test]
    fn test_format_file_path_no_directory() {
        let spans = format_file_path("test_foo.py");
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].content, "test_foo");
        assert_eq!(spans[1].content, ".py");
    }

    #[test]
    fn test_format_file_path_no_extension() {
        let spans = format_file_path("tests/conftest");
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].content, "tests/");
        assert_eq!(spans[1].content, "conftest");
    }

    #[test]
    fn test_format_test_id_with_separator() {
        let spans = format_test_id("tests/test_foo.py::test_bar", Style::default());
        assert!(spans.len() >= 3);
        let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text, "tests/test_foo.py::test_bar");
    }

    #[test]
    fn test_format_test_id_without_separator() {
        let spans = format_test_id("test_bar", Style::default());
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].content, "test_bar");
    }

    #[test]
    fn test_render_header_shows_run_badge() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut state = SharedState::new();
        state.phase = Phase::Running;
        state.start_time = Some(Instant::now());

        terminal
            .draw(|frame| {
                render_frame(frame, &state);
            })
            .unwrap();

        let buffer = terminal.backend().buffer().clone();
        let content: String = buffer.content.iter().map(|cell| cell.symbol()).collect();
        assert!(content.contains("RUN"));
    }

    #[test]
    fn test_render_header_shows_pass_when_finished() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut state = SharedState::new();
        state.phase = Phase::Finished;
        state.start_time = Some(Instant::now());

        terminal
            .draw(|frame| {
                render_frame(frame, &state);
            })
            .unwrap();

        let buffer = terminal.backend().buffer().clone();
        let content: String = buffer.content.iter().map(|cell| cell.symbol()).collect();
        assert!(content.contains("PASS"));
    }

    #[test]
    fn test_render_header_shows_fail_when_finished_with_failures() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut state = SharedState::new();
        state.phase = Phase::Finished;
        state.failed = 1;
        state.start_time = Some(Instant::now());

        terminal
            .draw(|frame| {
                render_frame(frame, &state);
            })
            .unwrap();

        let buffer = terminal.backend().buffer().clone();
        let content: String = buffer.content.iter().map(|cell| cell.symbol()).collect();
        assert!(content.contains("FAIL"));
    }
}
