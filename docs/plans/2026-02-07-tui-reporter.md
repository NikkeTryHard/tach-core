# TUI Reporter + Log Suppression Implementation Plan

> **REQUIRED:** Use `execute-plan` to implement this plan batch by batch.

**Goal:** Make tach-core output look like vitest's TUI — files scroll up in real-time as they complete, sticky progress bar at bottom, all diagnostic logs go to a log file instead of terminal.
**Architecture:** Two independent changes: (1) redirect all `eprintln!`/stderr diagnostic logs to `/tmp/tach_<uuid>.log`, (2) upgrade TachReporter to stream file results in real-time using `bar.println()` with per-file completion tracking.
**Tech Stack:** indicatif 0.18 (already in use), std::fs::File for log redirect, libc for stderr dup2.

---

### Batch 1: Stderr Log Redirection

**Goal:** Redirect all `[tach:*]` and `[worker:*]` diagnostic logs from stderr to a log file. Terminal only shows reporter output.

#### Task 1.1: Create log redirection module

**Files:**
- Create: `src/reporting/logredirect.rs`
- Modify: `src/reporting/mod.rs` (add module)
- Modify: `src/main.rs` (activate redirect early)

**Step 1: Write failing test**
```rust
// In src/reporting/logredirect.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_file_created() {
        let redirect = LogRedirect::new().unwrap();
        let path = redirect.log_path().to_string();
        assert!(std::path::Path::new(&path).exists());
        drop(redirect);
    }

    #[test]
    fn test_log_path_format() {
        let redirect = LogRedirect::new().unwrap();
        let path = redirect.log_path();
        assert!(path.starts_with("/tmp/tach_"));
        assert!(path.ends_with(".log"));
    }
}
```

**Step 2: Verify failure**
Run: `cargo nextest run --lib -E 'test(logredirect)'`
Expected: FAIL — module doesn't exist

**Step 3: Implement**
```rust
// src/reporting/logredirect.rs
use std::fs::File;
use std::os::unix::io::AsRawFd;
use uuid::Uuid;

/// Redirects stderr to a log file so diagnostic messages don't pollute the terminal.
/// On drop, restores stderr to original.
pub struct LogRedirect {
    log_path: String,
    original_stderr_fd: i32,
}

impl LogRedirect {
    /// Create a new log redirect. Stderr will be redirected to /tmp/tach_<uuid>.log.
    pub fn new() -> std::io::Result<Self> {
        let run_id = Uuid::new_v4();
        let log_path = format!("/tmp/tach_{}.log", run_id);
        let log_file = File::create(&log_path)?;

        // Save original stderr fd
        let original_stderr_fd = unsafe { libc::dup(2) };
        if original_stderr_fd < 0 {
            return Err(std::io::Error::last_os_error());
        }

        // Redirect stderr to log file
        let result = unsafe { libc::dup2(log_file.as_raw_fd(), 2) };
        if result < 0 {
            unsafe { libc::close(original_stderr_fd) };
            return Err(std::io::Error::last_os_error());
        }

        Ok(Self {
            log_path,
            original_stderr_fd,
        })
    }

    /// Get the path to the log file.
    pub fn log_path(&self) -> &str {
        &self.log_path
    }
}

impl Drop for LogRedirect {
    fn drop(&mut self) {
        // Restore original stderr
        unsafe {
            libc::dup2(self.original_stderr_fd, 2);
            libc::close(self.original_stderr_fd);
        }
    }
}
```

**Step 4: Verify pass**
Run: `cargo nextest run --lib -E 'test(logredirect)'`
Expected: PASS

**Step 5: Commit**
```bash
git add src/reporting/logredirect.rs src/reporting/mod.rs
git commit -m "feat(reporting): add stderr log redirect to file"
```

#### Task 1.2: Wire log redirect into main.rs

**Files:**
- Modify: `src/main.rs` — activate LogRedirect early in execute_session, print log path at end

**Implementation:**
- Create `LogRedirect::new()` right after argument parsing, before any `eprintln!` calls
- Store the redirect handle so it lives for the duration of the run
- After the run completes (after reporter output), restore stderr (drop redirect) and print `"  Log file  /tmp/tach_<uuid>.log"` to stderr
- Only activate for non-JSON output formats (JSON mode should stay clean)
- Guard behind `ProgressReporter::should_use_progress_bar()` — if not a TTY, skip redirect

**Step 1: Implement**
In `execute_session()`, right after building the reporter (around where `OutputFormat::Human` is handled):
```rust
// Redirect stderr to log file for interactive mode
let log_redirect = if !is_json && ProgressReporter::should_use_progress_bar() {
    match LogRedirect::new() {
        Ok(redirect) => Some(redirect),
        Err(_) => None, // Fall back to normal stderr if redirect fails
    }
} else {
    None
};
```

At the end of `execute_session()`, after reporter output:
```rust
// Print log file location
if let Some(redirect) = log_redirect {
    let log_path = redirect.log_path().to_string();
    drop(redirect); // Restore stderr
    eprintln!("   Log file  {}", log_path);
}
```

**Step 2: Verify**
Build and run in Docker to confirm:
- No `[tach:*]` or `[worker:*]` messages on terminal
- Log file exists at `/tmp/tach_*.log` with all diagnostic output
- Log path printed at end of run

**Step 3: Commit**
```bash
git add src/main.rs
git commit -m "feat: redirect diagnostic logs to file in interactive mode"
```

---

### Batch 2: Real-time File Streaming in TachReporter

**Goal:** Print file result lines in real-time as files complete, instead of buffering until the end.

#### Task 2.1: Add per-file expected counts to Reporter trait

**Files:**
- Modify: `src/reporting/reporter.rs` — add `on_session_setup` to Reporter trait, implement for all reporters
- Modify: `src/main.rs` or `src/execution/scheduler.rs` — call `on_session_setup` with file counts

**Step 1: Write failing test**
```rust
#[test]
fn test_tach_reporter_streams_file_on_completion() {
    let mut reporter = TachReporter::new();
    let mut counts = HashMap::new();
    counts.insert("tests/test_a.py".to_string(), 2usize);
    counts.insert("tests/test_b.py".to_string(), 1usize);
    reporter.on_session_setup(counts);
    reporter.on_run_start(3);

    reporter.on_test_start("tests/test_a.py::test_1", "tests/test_a.py");
    reporter.on_test_start("tests/test_a.py::test_2", "tests/test_a.py");
    reporter.on_test_start("tests/test_b.py::test_1", "tests/test_b.py");

    reporter.on_test_finished("tests/test_a.py::test_1", "pass", 100, None);
    // File a not complete yet (1/2)
    reporter.on_test_finished("tests/test_a.py::test_2", "pass", 200, None);
    // File a NOW complete (2/2) — should have been printed via bar.println()

    reporter.on_test_finished("tests/test_b.py::test_1", "pass", 50, None);
    // File b NOW complete (1/1)

    // Verify internal state
    assert_eq!(reporter.passed, 3);
    assert_eq!(reporter.files_printed, 2);
}
```

**Step 2: Verify failure**
Run: `cargo nextest run --lib -E 'test(streams_file)'`
Expected: FAIL — no `on_session_setup` method, no `files_printed` field

**Step 3: Implement**

Add to `Reporter` trait:
```rust
fn on_session_setup(&mut self, _file_counts: HashMap<String, usize>) {}
```

Add to `TachReporter`:
```rust
pub struct TachReporter {
    // ... existing fields ...
    file_expected: HashMap<String, usize>,  // file -> expected test count
    files_printed: usize,                    // count of files already streamed
}
```

In `on_session_setup`:
```rust
fn on_session_setup(&mut self, file_counts: HashMap<String, usize>) {
    self.file_expected = file_counts;
}
```

In `on_test_finished`, after updating `file_result`, check if file is complete:
```rust
// Check if this file is now complete
let expected = self.file_expected.get(&file).copied().unwrap_or(0);
let actual = file_result.total();
if expected > 0 && actual == expected {
    // Print this file's result line above the spinner
    let line = self.format_file_line(&file, file_result, supports_colors());
    self.bar.println(line);
    self.files_printed += 1;
}
```

Add `format_file_line` method (extract from `render_file_list`):
```rust
fn format_file_line(&self, file_path: &str, result: &FileResult, use_colors: bool) -> String {
    // Same logic as render_file_list but returns a String instead of printing
}
```

In `on_run_finished`, skip printing files that were already streamed — only print summary + failures:
```rust
fn on_run_finished(&mut self, ...) {
    self.bar.finish_and_clear();
    let use_colors = supports_colors();

    // Only print files that weren't streamed (edge case: files with no expected count)
    // In practice, all files should be streamed, so this is a safety net
    if self.files_printed < self.file_results.len() {
        self.render_file_list(use_colors);
    }

    self.render_summary(duration_ms, use_colors);
    self.render_failures(use_colors);
}
```

**Step 4: Verify pass**
Run: `cargo nextest run --lib -E 'test(tach_reporter)'`
Expected: PASS

**Step 5: Commit**
```bash
git add src/reporting/reporter.rs
git commit -m "feat(reporter): stream file results in real-time via bar.println()"
```

#### Task 2.2: Wire file counts from scheduler to reporter

**Files:**
- Modify: `src/main.rs` — compute per-file counts from `filtered_tests` and call `reporter.on_session_setup()`

**Implementation:**
After `filtered_tests` is built (around line 461 in main.rs), compute file counts:
```rust
// Compute per-file test counts for real-time streaming
let mut file_counts: HashMap<String, usize> = HashMap::new();
for test in &filtered_tests {
    *file_counts.entry(test.file_path.clone()).or_insert(0) += 1;
}
reporter.on_session_setup(file_counts);
```

This goes before `reporter.on_run_start(filtered_tests.len())`.

**Step 1: Implement** the wiring in main.rs
**Step 2: Build** and run in Docker to verify real-time streaming
**Step 3: Commit**
```bash
git add src/main.rs
git commit -m "feat: wire per-file test counts to reporter for real-time streaming"
```

---

### Batch 3: Polish and Integration Testing

**Goal:** Verify end-to-end behavior, clean up edge cases, update existing tests.

#### Task 3.1: Handle edge cases

**Files:**
- Modify: `src/reporting/reporter.rs`

**Edge cases to handle:**
1. Worker crash/timeout — file may never reach expected count. In `on_run_finished`, print any remaining unstreamed files.
2. Files with 0 expected tests (shouldn't happen but safety net)
3. `render_file_list` should skip already-printed files to avoid duplicates

**Step 1: Write tests**
```rust
#[test]
fn test_tach_reporter_crash_still_prints_file() {
    let mut reporter = TachReporter::new();
    let mut counts = HashMap::new();
    counts.insert("tests/test_a.py".to_string(), 3usize);
    reporter.on_session_setup(counts);
    reporter.on_run_start(3);

    reporter.on_test_start("tests/test_a.py::test_1", "tests/test_a.py");
    reporter.on_test_start("tests/test_a.py::test_2", "tests/test_a.py");
    reporter.on_test_start("tests/test_a.py::test_3", "tests/test_a.py");

    reporter.on_test_finished("tests/test_a.py::test_1", "pass", 100, None);
    reporter.on_test_finished("tests/test_a.py::test_2", "crash", 200, Some("worker died"));
    // test_3 never finishes (worker crash took it out)

    // on_run_finished should still print the file
    reporter.on_run_finished(1, 1, 0, 300);
}
```

**Step 2: Implement** — in `on_run_finished`, collect unstreamed files and print them
**Step 3: Verify** — all tests pass
**Step 4: Commit**
```bash
git add src/reporting/reporter.rs
git commit -m "fix(reporter): handle unstreamed files from crashes in final output"
```

#### Task 3.2: Update spinner message format

**Files:**
- Modify: `src/reporting/reporter.rs`

**Change spinner to show more useful info:**
```
Current: "Running tests... 42/1996 (2%)"
New:     "Running 1996 tests · 42 passed · 0 failed"
```

Update the `on_test_finished` spinner message and `on_run_start` initial message.

**Step 1: Implement** the format change
**Step 2: Verify** existing tests still pass
**Step 3: Commit**
```bash
git add src/reporting/reporter.rs
git commit -m "refactor(reporter): improve spinner message format"
```

#### Task 3.3: Add log path to summary output

**Files:**
- Modify: `src/reporting/reporter.rs`

**Add log path as 4th line in summary block:**
```
 Test Files  98 passed | 1 failed (99)
      Tests  1985 passed | 1 failed | 9 skipped (1995)
   Duration  20.53s
   Log file  /tmp/tach_abc123.log
```

Add `log_path: Option<String>` field to TachReporter, set it from main.rs before the run starts.

**Step 1: Implement**
**Step 2: Verify** — tests pass, Docker run shows log path in summary
**Step 3: Commit**
```bash
git add src/reporting/reporter.rs src/main.rs
git commit -m "feat(reporter): show log file path in summary output"
```
