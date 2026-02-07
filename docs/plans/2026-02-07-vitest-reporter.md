# Vitest-Style TachReporter Implementation Plan

> **REQUIRED:** Use `execute-plan` to implement this plan batch by batch.

**Goal:** Replace the default interactive reporter with a clean, vitest-inspired output that groups results by file, shows a slim progress spinner during execution, and expands only failing files in the summary.

**Architecture:** New `TachReporter` struct in `src/reporting/reporter.rs` that implements the existing `Reporter` trait. It buffers test results by file during execution (using a `HashMap<String, FileResult>`), shows a minimal spinner, then renders a grouped summary on completion. The "Saved Xm Xs" metric is removed from all reporters. `TachReporter` becomes the new default for interactive terminals; `ProgressReporter` remains available via `--format=progress`.

**Tech Stack:** Rust, `indicatif` (already a dependency), ANSI color codes (existing helpers)

**Worktree:** `/home/nikketryhard/dev/tach-core/.worktrees/feat/vitest-reporter`

---

### Batch 1: Core Data Structures + Basic Rendering

**Goal:** Build the `TachReporter` struct with file-grouped buffering and the summary renderer. No integration yet.

#### Task 1.1: TachReporter struct and file grouping

**Files:**
- Modify: `src/reporting/reporter.rs` (append after `DotsReporter` impl, before `mod tests`)

**Step 1: Write failing test**
```rust
// In mod tests {} at the bottom of reporter.rs

#[test]
fn test_tach_reporter_groups_by_file() {
    let mut reporter = TachReporter::new();
    reporter.on_run_start(4);
    reporter.on_test_start("tests/auth/test_login.py::test_valid", "tests/auth/test_login.py");
    reporter.on_test_finished("tests/auth/test_login.py::test_valid", "pass", 100, None);
    reporter.on_test_start("tests/auth/test_login.py::test_invalid", "tests/auth/test_login.py");
    reporter.on_test_finished("tests/auth/test_login.py::test_invalid", "pass", 50, None);
    reporter.on_test_start("tests/api/test_users.py::test_create", "tests/api/test_users.py");
    reporter.on_test_finished("tests/api/test_users.py::test_create", "fail", 200, Some("AssertionError"));
    reporter.on_test_start("tests/api/test_users.py::test_list", "tests/api/test_users.py");
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
    assert_eq!(api.failures[0].test_id, "tests/api/test_users.py::test_create");
}
```

**Step 2: Verify failure**
Run: `cd /home/nikketryhard/dev/tach-core/.worktrees/feat/vitest-reporter && cargo nextest run -E 'test(test_tach_reporter_groups_by_file)' 2>&1`
Expected: FAIL — `TachReporter` not found

**Step 3: Implement**
Add the following after `DotsReporter` impl block, before `mod tests`:

```rust
// =============================================================================
//  Tach Reporter (Vitest-style)
// =============================================================================

use std::collections::HashMap;

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

/// Vitest-inspired reporter that groups results by file
///
/// During execution: shows a slim spinner with progress count
/// On completion: renders file-grouped summary with smart failure expansion
pub struct TachReporter {
    bar: ProgressBar,
    file_results: HashMap<String, FileResult>,
    /// Maps test ID -> file path (populated by on_test_start)
    test_to_file: HashMap<String, String>,
    /// Insertion counter for stable file ordering
    file_order: usize,
    /// Total tests
    total: usize,
    /// Running counts for spinner display
    passed: usize,
    failed: usize,
    skipped: usize,
    /// Traceback formatting style
    traceback_style: TracebackStyle,
}

impl TachReporter {
    pub fn new() -> Self {
        Self::with_traceback_style(TracebackStyle::Long)
    }

    #[must_use]
    pub fn with_traceback_style(traceback_style: TracebackStyle) -> Self {
        let bar = ProgressBar::new(0);
        bar.set_style(
            ProgressStyle::default_spinner()
                .template("{spinner:.green} {msg}")
                .unwrap_or_else(|_| ProgressStyle::default_spinner()),
        );

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
        }
    }

    /// Extract the short test name from a full node ID
    /// e.g. "tests/foo.py::TestClass::test_method[param]" -> "test_method[param]"
    fn short_test_name(id: &str) -> &str {
        id.rsplit("::").next().unwrap_or(id)
    }
}

impl Default for TachReporter {
    fn default() -> Self {
        Self::new()
    }
}
```

**Step 4: Verify pass**
Run: `cd /home/nikketryhard/dev/tach-core/.worktrees/feat/vitest-reporter && cargo nextest run -E 'test(test_tach_reporter_groups_by_file)' 2>&1`
Expected: PASS

**Step 5: Commit**
```bash
git add src/reporting/reporter.rs
git commit -m "feat(reporter): add TachReporter struct with file-grouped buffering"
```

#### Task 1.2: Reporter trait implementation

**Files:**
- Modify: `src/reporting/reporter.rs`

**Step 1: Write failing test**
```rust
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
```

**Step 2: Verify failure**
Run: `cargo nextest run -E 'test(test_tach_reporter_spinner_message_updates)' 2>&1`
Expected: FAIL — Reporter trait not implemented for TachReporter

**Step 3: Implement**
Add `impl Reporter for TachReporter` block:

```rust
impl Reporter for TachReporter {
    fn on_run_start(&mut self, count: usize) {
        self.total = count;
        self.bar.set_message(format!("Running {} tests...", count));
    }

    fn on_test_start(&mut self, id: &str, file: &str) {
        self.test_to_file.insert(id.to_string(), file.to_string());
        // Ensure file entry exists
        if !self.file_results.contains_key(file) {
            let order = self.file_order;
            self.file_order += 1;
            self.file_results.insert(file.to_string(), FileResult::new(order));
        }
    }

    fn on_test_finished(
        &mut self,
        id: &str,
        status: &str,
        duration_ms: u64,
        message: Option<&str>,
    ) {
        let file = self.test_to_file.get(id).cloned().unwrap_or_default();
        let entry = self.file_results.entry(file).or_insert_with(|| {
            let order = self.file_order;
            self.file_order += 1;
            FileResult::new(order)
        });

        entry.total_duration_ms += duration_ms;

        if is_pass(status) {
            entry.passed += 1;
            self.passed += 1;
        } else if is_skip(status) {
            entry.skipped += 1;
            self.skipped += 1;
        } else {
            entry.failed += 1;
            self.failed += 1;
            let use_colors = supports_colors();
            let formatted_msg = message
                .map(|m| {
                    let formatted = format_traceback(m, id, self.traceback_style);
                    colorize_traceback(&formatted, use_colors)
                })
                .unwrap_or_default();
            entry.failures.push(TachFailureRecord {
                test_id: id.to_string(),
                short_name: Self::short_test_name(id).to_string(),
                message: formatted_msg,
            });
        }

        // Update spinner
        let done = self.passed + self.failed + self.skipped;
        self.bar.set_message(format!(
            "Running tests... {}/{} ({}%)",
            done,
            self.total,
            if self.total > 0 { done * 100 / self.total } else { 0 }
        ));
    }

    fn on_run_finished(&mut self, passed: usize, failed: usize, skipped: usize, duration_ms: u64) {
        self.bar.finish_and_clear();

        let use_colors = supports_colors();
        let duration_secs = duration_ms as f64 / 1000.0;

        // Sort files by insertion order
        let mut files: Vec<(&String, &FileResult)> = self.file_results.iter().collect();
        files.sort_by_key(|(_, r)| r.order);

        // Render file results
        eprintln!();
        for (file, result) in &files {
            if result.has_failures() {
                // Failed file: red × with breakdown
                let status_parts = format!(
                    "{} passed | {} failed",
                    result.passed, result.failed
                );
                if use_colors {
                    eprintln!(
                        " {}\u{00d7}{} {} ({})  {}ms",
                        ANSI_RED, ANSI_RESET, file, status_parts, result.total_duration_ms
                    );
                } else {
                    eprintln!(
                        " \u{00d7} {} ({})  {}ms",
                        file, status_parts, result.total_duration_ms
                    );
                }
                // Expand individual failures
                for failure in &result.failures {
                    if use_colors {
                        eprintln!(
                            "   {}\u{00d7}{} {}",
                            ANSI_RED, ANSI_RESET, failure.short_name
                        );
                    } else {
                        eprintln!("   \u{00d7} {}", failure.short_name);
                    }
                }
            } else {
                // Passing file: green checkmark, compact
                if use_colors {
                    eprintln!(
                        " {}\u{2713}{} {} ({})  {}ms",
                        ANSI_GREEN, ANSI_RESET, file, result.total(), result.total_duration_ms
                    );
                } else {
                    eprintln!(
                        " \u{2713} {} ({})  {}ms",
                        file, result.total(), result.total_duration_ms
                    );
                }
            }
        }

        // Render summary block
        let total_files = files.len();
        let failed_files = files.iter().filter(|(_, r)| r.has_failures()).count();
        let passed_files = total_files - failed_files;

        eprintln!();
        // File summary line
        if failed_files > 0 {
            if use_colors {
                eprintln!(
                    " Test Files  {}{} passed{} | {}{} failed{} ({})",
                    ANSI_GREEN, passed_files, ANSI_RESET,
                    ANSI_RED, failed_files, ANSI_RESET,
                    total_files
                );
            } else {
                eprintln!(
                    " Test Files  {} passed | {} failed ({})",
                    passed_files, failed_files, total_files
                );
            }
        } else if use_colors {
            eprintln!(
                " Test Files  {}{} passed{} ({})",
                ANSI_GREEN, passed_files, ANSI_RESET, total_files
            );
        } else {
            eprintln!(" Test Files  {} passed ({})", passed_files, total_files);
        }

        // Tests summary line
        let total_tests = passed + failed + skipped;
        if failed > 0 {
            if use_colors {
                eprintln!(
                    "     Tests  {}{} passed{} | {}{} failed{} | {} skipped ({})",
                    ANSI_GREEN, passed, ANSI_RESET,
                    ANSI_RED, failed, ANSI_RESET,
                    skipped, total_tests
                );
            } else {
                eprintln!(
                    "     Tests  {} passed | {} failed | {} skipped ({})",
                    passed, failed, skipped, total_tests
                );
            }
        } else if use_colors {
            eprintln!(
                "     Tests  {}{} passed{} | {} skipped ({})",
                ANSI_GREEN, passed, ANSI_RESET, skipped, total_tests
            );
        } else {
            eprintln!(
                "     Tests  {} passed | {} skipped ({})",
                passed, skipped, total_tests
            );
        }

        // Duration line
        eprintln!("  Duration  {:.2}s", duration_secs);
        eprintln!();

        // Print detailed failure tracebacks at the end (like vitest)
        if !self.file_results.values().any(|r| r.has_failures()) || self.traceback_style == TracebackStyle::No {
            return;
        }

        eprintln!("{} FAILURES {}", "=".repeat(30), "=".repeat(30));
        for (file, result) in &files {
            if !result.has_failures() {
                continue;
            }
            for failure in &result.failures {
                eprintln!();
                if use_colors {
                    eprintln!(" {}{}{} > {}{}{}", ANSI_RED, "FAIL", ANSI_RESET, ANSI_CYAN, failure.test_id, ANSI_RESET);
                } else {
                    eprintln!(" FAIL > {}", failure.test_id);
                }
                if !failure.message.is_empty() {
                    for line in failure.message.lines().take(20) {
                        eprintln!("  {}", line);
                    }
                }
            }
        }
        eprintln!("{}", "=".repeat(70));
    }

    fn on_error(&mut self, message: &str) {
        self.bar.finish_and_clear();
        let use_colors = supports_colors();
        if use_colors {
            eprintln!("\n {}ERROR{} {}", ANSI_RED, ANSI_RESET, message);
        } else {
            eprintln!("\n ERROR {}", message);
        }
    }
}
```

**Step 4: Verify pass**
Run: `cargo nextest run -E 'test(test_tach_reporter)' 2>&1`
Expected: Both tests PASS

**Step 5: Commit**
```bash
git add src/reporting/reporter.rs
git commit -m "feat(reporter): implement Reporter trait for TachReporter"
```

---

### Batch 2: Integration + Remove "Saved" Metric

**Goal:** Wire TachReporter as default, add `--format=progress` fallback, remove inaccurate "Saved Xm Xs" metric from all reporters.

#### Task 2.1: Make TachReporter the default interactive reporter

**Files:**
- Modify: `src/main.rs:318-340`

**Step 1: Write failing test**
No unit test needed — this is a wiring change. Verified by running the binary.

**Step 2: Implement**
In `src/main.rs`, change the `OutputFormat::Human` match arm:

Replace:
```rust
        OutputFormat::Human => {
            if ProgressReporter::should_use_progress_bar() && !is_narrow_terminal() {
                reporters.push(Box::new(ProgressReporter::with_traceback_style(
                    cli.traceback,
                )));
            } else {
                reporters.push(Box::new(DotsReporter::with_traceback_style(
                    cli.traceback,
                )));
            }
        }
```

With:
```rust
        OutputFormat::Human => {
            if ProgressReporter::should_use_progress_bar() && !is_narrow_terminal() {
                reporters.push(Box::new(TachReporter::with_traceback_style(
                    cli.traceback,
                )));
            } else {
                reporters.push(Box::new(DotsReporter::with_traceback_style(
                    cli.traceback,
                )));
            }
        }
```

Also add `TachReporter` to the imports at the top of main.rs (wherever `ProgressReporter` is imported).

**Step 3: Verify**
Run: `cargo build 2>&1`
Expected: Compiles with no errors

**Step 4: Commit**
```bash
git add src/main.rs
git commit -m "feat(reporter): make TachReporter the default interactive reporter"
```

#### Task 2.2: Remove "Saved" metric from ProgressReporter and DotsReporter

**Files:**
- Modify: `src/reporting/reporter.rs`

**Step 1: Write failing test**
No test needed — removing dead code. We'll verify existing tests still pass.

**Step 2: Implement**
1. Delete the `PYTEST_COLD_TEST_MS` constant (line 36)
2. In `ProgressReporter::on_run_finished` (around lines 787-813): Delete the entire "Time Saved metric" block from `let total_tests = passed + failed + skipped;` through the end of the closing brace.
3. In `DotsReporter::on_run_finished` (around lines 949-969): Delete the same "Time Saved metric" block.

**Step 3: Verify pass**
Run: `cargo nextest run --lib 2>&1 | tail -3`
Expected: All tests pass

**Step 4: Commit**
```bash
git add src/reporting/reporter.rs
git commit -m "fix(reporter): remove inaccurate 'Saved' initialization overhead metric"
```

---

### Batch 3: Edge Cases + Final Tests

**Goal:** Handle edge cases (empty runs, all-skip, narrow terminal fallback) and add comprehensive tests.

#### Task 3.1: Edge case tests

**Files:**
- Modify: `src/reporting/reporter.rs` (add tests)

**Step 1: Write tests**
```rust
#[test]
fn test_tach_reporter_empty_run() {
    let mut reporter = TachReporter::new();
    reporter.on_run_start(0);
    // Should not panic
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
    assert_eq!(reporter.failed, 0);
    let file = &reporter.file_results["f.py"];
    assert_eq!(file.skipped, 2);
    assert!(!file.has_failures());
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
    assert_eq!(TachReporter::short_test_name("tests/foo.py::TestClass::test_method"), "test_method");
    assert_eq!(TachReporter::short_test_name("tests/foo.py::test_simple"), "test_simple");
    assert_eq!(TachReporter::short_test_name("tests/foo.py::test_param[1-2-3]"), "test_param[1-2-3]");
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

    // Files should be ordered by first appearance, not alphabetically
    let mut files: Vec<(&String, &FileResult)> = reporter.file_results.iter().collect();
    files.sort_by_key(|(_, r)| r.order);
    let names: Vec<&str> = files.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(names, vec!["b.py", "a.py", "c.py"]);
}
```

**Step 2: Verify all pass**
Run: `cargo nextest run -E 'test(test_tach_reporter)' 2>&1`
Expected: All TachReporter tests PASS

**Step 3: Commit**
```bash
git add src/reporting/reporter.rs
git commit -m "test(reporter): add comprehensive TachReporter edge case tests"
```

#### Task 3.2: Make file_results and structs pub(crate) for testability

If tests in Task 3.1 fail because `file_results`, `FileResult`, `TachFailureRecord` are private, make them `pub(crate)`:

```rust
pub(crate) struct TachFailureRecord { ... }
pub(crate) struct FileResult { ... }
// In TachReporter:
pub(crate) file_results: HashMap<String, FileResult>,
```

This is only needed if the tests can't access them — since they're in the same module, they should be fine with no visibility change.

**Step 1: Verify**
Run: `cargo nextest run --lib 2>&1 | tail -3`
Expected: All 822+ tests pass

**Step 2: Commit (only if changes were needed)**
```bash
git add src/reporting/reporter.rs
git commit -m "refactor(reporter): adjust visibility for TachReporter internals"
```

---

### Batch 4: Full Integration Test

**Goal:** Build the release binary and verify the output looks correct in Docker with the real test suite.

#### Task 4.1: Build and visual verification

**Steps:**
1. Build release:
```bash
rm -f target/release/tach-core && touch src/execution/zygote.rs
cargo build --release 2>&1
```

2. Run unit tests to verify nothing broke:
```bash
cargo nextest run --lib 2>&1 | tail -5
```

3. In Docker (if available), run the full test-aistudio suite and visually verify the output matches the design spec:
```bash
# In Docker container:
export PYO3_PYTHON=/workspace/.venv/bin/python3 PATH=/workspace/.venv/bin:$PATH VIRTUAL_ENV=/workspace/.venv
cd test-aistudio && /workspace/target/release/tach-core --no-isolation
```

Expected output format:
```
 ✓ tests/auth/test_login.py (12)  340ms
 ✓ tests/auth/test_signup.py (8)  120ms
 × tests/api/test_users.py (14 passed | 1 failed)  890ms
   × test_create_user  AssertionError: ...
 ✓ tests/api/test_orders.py (23)  450ms

 Test Files  4 passed | 1 failed (5)
     Tests   57 passed | 1 failed | 2 skipped (60)
  Duration   2.14s
```

4. Commit:
```bash
git add -A
git commit -m "feat(reporter): vitest-style TachReporter as default output"
```
