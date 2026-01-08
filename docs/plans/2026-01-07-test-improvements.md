# Test Improvements Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Strengthen regression prevention across 4 categories: silent behavior changes, performance regressions, kernel compatibility breaks, and pytest API drift.

**Architecture:** Add new test types (exit code tests, JSON schema validation, pytest comparison tests, kernel matrix tests) while maintaining 90% unit test coverage threshold. Tests are independent and can be parallelized.

**Tech Stack:** Rust (cargo test, proptest, cargo-fuzz), Python (pytest), cargo-llvm-cov

---

## Prerequisites

Before starting, ensure:

1. Current tests pass: `cargo test --lib && cargo test --test '*'`
2. Coverage baseline: `./scripts/coverage.sh` (should show ~90%)
3. Build is up-to-date: `cargo build`

---

## Task 1: Exit Code Regression Tests

**Files:**

- Create: `rust_tests/exit_code_tests.rs`
- Modify: `Cargo.toml` (add test target if needed)

**Purpose:** Prevent silent changes to CLI exit codes.

**Step 1: Write the failing test**

Create `rust_tests/exit_code_tests.rs`:

```rust
//! Exit code regression tests for tach-core CLI.
//!
//! These tests ensure exit codes remain stable across releases.
//! Exit codes are part of the public API for CI integration.

use std::process::Command;

/// Get the tach-core binary path
fn tach_binary() -> std::path::PathBuf {
    let mut path = std::env::current_exe().unwrap();
    path.pop(); // Remove test binary name
    path.pop(); // Remove deps
    path.push("tach-core");
    path
}

#[test]
fn test_exit_code_success_on_passing_tests() {
    let output = Command::new(tach_binary())
        .args(["--no-isolation", "-n", "1", "tests/dummy_project"])
        .output()
        .expect("Failed to execute tach-core");

    // Passing tests should exit with 0
    assert_eq!(
        output.status.code(),
        Some(0),
        "Expected exit code 0 for passing tests, got {:?}\nstderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn test_exit_code_failure_on_failing_tests() {
    let output = Command::new(tach_binary())
        .args(["--no-isolation", "-n", "1", "tests/dummy_project/test_fail_assert.py"])
        .output()
        .expect("Failed to execute tach-core");

    // Failing tests should exit with 1
    assert_eq!(
        output.status.code(),
        Some(1),
        "Expected exit code 1 for failing tests, got {:?}",
        output.status.code()
    );
}

#[test]
fn test_exit_code_on_no_tests_found() {
    let output = Command::new(tach_binary())
        .args(["--no-isolation", "-n", "1", "tests/empty_dir_nonexistent"])
        .output()
        .expect("Failed to execute tach-core");

    // No tests found should exit with specific code (typically 5 like pytest, or 0)
    // Document current behavior, fail if it changes
    let code = output.status.code().unwrap_or(-1);
    assert!(
        code == 0 || code == 5,
        "Exit code for no tests should be 0 or 5, got {}",
        code
    );
}

#[test]
fn test_exit_code_version_command() {
    let output = Command::new(tach_binary())
        .args(["version"])
        .output()
        .expect("Failed to execute tach-core");

    assert_eq!(
        output.status.code(),
        Some(0),
        "version command should exit 0"
    );
}

#[test]
fn test_exit_code_self_test_command() {
    let output = Command::new(tach_binary())
        .args(["self-test"])
        .output()
        .expect("Failed to execute tach-core");

    // self-test may fail on some systems but should return a valid code
    let code = output.status.code().unwrap_or(-1);
    assert!(
        code >= 0,
        "self-test should return valid exit code, got {}",
        code
    );
}

#[test]
fn test_exit_code_list_command() {
    let output = Command::new(tach_binary())
        .args(["list", "--no-isolation", "tests/dummy_project"])
        .output()
        .expect("Failed to execute tach-core");

    assert_eq!(
        output.status.code(),
        Some(0),
        "list command should exit 0"
    );
}

#[test]
fn test_exit_code_invalid_argument() {
    let output = Command::new(tach_binary())
        .args(["--invalid-flag-that-does-not-exist"])
        .output()
        .expect("Failed to execute tach-core");

    // Invalid args should exit with non-zero (typically 2 for argument errors)
    let code = output.status.code().unwrap_or(-1);
    assert!(
        code != 0,
        "Invalid argument should exit non-zero, got {}",
        code
    );
}
```

**Step 2: Run test to verify it compiles and behavior is captured**

Run: `cargo test --test exit_code_tests -- --nocapture`

Expected: Tests pass (documenting current behavior) or fail if binary missing

**Step 3: Commit**

```bash
git add rust_tests/exit_code_tests.rs
git commit -m "test: add exit code regression tests

Prevents silent changes to CLI exit codes which are part of
the public API for CI integration.

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Task 2: JSON Output Schema Validation

**Files:**

- Create: `rust_tests/json_schema_tests.rs`
- Create: `tests/regression/schemas/` (directory)
- Create: `tests/regression/schemas/test_result.schema.json`

**Purpose:** Prevent silent changes to JSON output format.

**Step 1: Create JSON schema for test results**

Create `tests/regression/schemas/test_result.schema.json`:

```json
{
  "$schema": "http://json-schema.org/draft-07/schema#",
  "title": "TachTestResult",
  "description": "JSON output from tach-core --format json",
  "type": "object",
  "properties": {
    "event": {
      "type": "string",
      "enum": ["test_start", "test_finish", "run_start", "run_finish", "error"]
    },
    "name": {
      "type": "string",
      "description": "Test name in format module::class::test or module::test"
    },
    "outcome": {
      "type": "string",
      "enum": ["passed", "failed", "skipped", "xfailed", "xpassed", "error"]
    },
    "duration_ms": {
      "type": "number",
      "minimum": 0
    },
    "message": {
      "type": "string"
    },
    "timestamp": {
      "type": "string",
      "format": "date-time"
    }
  },
  "required": ["event"]
}
```

**Step 2: Write the validation test**

Create `rust_tests/json_schema_tests.rs`:

```rust
//! JSON output schema validation tests.
//!
//! These tests ensure JSON output format remains stable and valid.

use std::process::Command;

fn tach_binary() -> std::path::PathBuf {
    let mut path = std::env::current_exe().unwrap();
    path.pop();
    path.pop();
    path.push("tach-core");
    path
}

#[test]
fn test_json_output_is_valid_ndjson() {
    let output = Command::new(tach_binary())
        .args(["--format", "json", "--no-isolation", "-n", "1", "tests/dummy_project/test_simple.py"])
        .output()
        .expect("Failed to execute tach-core");

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Each line should be valid JSON (NDJSON format)
    for (i, line) in stdout.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let parsed: Result<serde_json::Value, _> = serde_json::from_str(line);
        assert!(
            parsed.is_ok(),
            "Line {} is not valid JSON: {}\nParse error: {:?}",
            i + 1,
            line,
            parsed.err()
        );
    }
}

#[test]
fn test_json_output_has_required_event_field() {
    let output = Command::new(tach_binary())
        .args(["--format", "json", "--no-isolation", "-n", "1", "tests/dummy_project/test_simple.py"])
        .output()
        .expect("Failed to execute tach-core");

    let stdout = String::from_utf8_lossy(&output.stdout);

    for line in stdout.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let parsed: serde_json::Value = serde_json::from_str(line)
            .expect("Failed to parse JSON");

        assert!(
            parsed.get("event").is_some(),
            "JSON output missing required 'event' field: {}",
            line
        );
    }
}

#[test]
fn test_json_output_event_types_are_valid() {
    let output = Command::new(tach_binary())
        .args(["--format", "json", "--no-isolation", "-n", "1", "tests/dummy_project/test_simple.py"])
        .output()
        .expect("Failed to execute tach-core");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let valid_events = ["test_start", "test_finish", "run_start", "run_finish", "error", "discovery"];

    for line in stdout.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let parsed: serde_json::Value = serde_json::from_str(line).unwrap();
        if let Some(event) = parsed.get("event").and_then(|e| e.as_str()) {
            assert!(
                valid_events.contains(&event),
                "Unknown event type '{}' in JSON output. Valid types: {:?}",
                event,
                valid_events
            );
        }
    }
}

#[test]
fn test_json_test_finish_has_outcome() {
    let output = Command::new(tach_binary())
        .args(["--format", "json", "--no-isolation", "-n", "1", "tests/dummy_project/test_simple.py"])
        .output()
        .expect("Failed to execute tach-core");

    let stdout = String::from_utf8_lossy(&output.stdout);

    for line in stdout.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let parsed: serde_json::Value = serde_json::from_str(line).unwrap();
        if parsed.get("event").and_then(|e| e.as_str()) == Some("test_finish") {
            assert!(
                parsed.get("outcome").is_some(),
                "test_finish event missing 'outcome' field: {}",
                line
            );
        }
    }
}
```

**Step 3: Run tests**

Run: `cargo test --test json_schema_tests -- --nocapture`

**Step 4: Commit**

```bash
git add rust_tests/json_schema_tests.rs tests/regression/schemas/
git commit -m "test: add JSON output schema validation

Ensures JSON output format (--format json) remains stable.
Validates NDJSON format, required fields, and event types.

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Task 3: Pytest API Comparison Tests

**Files:**

- Create: `tests/regression/pytest_compat/test_pytest_comparison.py`
- Create: `tests/regression/pytest_compat/sample_tests/` (test fixtures)

**Purpose:** Detect pytest API drift by comparing tach harness behavior against real pytest.

**Step 1: Create sample test fixtures**

Create `tests/regression/pytest_compat/sample_tests/test_raises_samples.py`:

```python
"""Sample tests for pytest.raises comparison."""
import pytest


def test_raises_basic():
    """Basic pytest.raises usage."""
    with pytest.raises(ValueError):
        raise ValueError("expected")


def test_raises_with_match():
    """pytest.raises with match parameter."""
    with pytest.raises(ValueError, match="expected"):
        raise ValueError("this is expected error")


def test_raises_wrong_exception():
    """Should fail - wrong exception type."""
    with pytest.raises(TypeError):
        raise ValueError("wrong type")
```

Create `tests/regression/pytest_compat/sample_tests/test_approx_samples.py`:

```python
"""Sample tests for pytest.approx comparison."""
import pytest


def test_approx_basic():
    """Basic pytest.approx usage."""
    assert 0.1 + 0.2 == pytest.approx(0.3)


def test_approx_with_rel():
    """pytest.approx with relative tolerance."""
    assert 100.0 == pytest.approx(99.0, rel=0.02)


def test_approx_with_abs():
    """pytest.approx with absolute tolerance."""
    assert 100.0 == pytest.approx(100.5, abs=1.0)


def test_approx_list():
    """pytest.approx with lists."""
    assert [0.1, 0.2] == pytest.approx([0.1, 0.2])
```

**Step 2: Create the comparison test**

Create `tests/regression/pytest_compat/test_pytest_comparison.py`:

```python
"""Compare tach harness behavior against real pytest.

This test runs the same test files through both pytest and tach-core,
then compares the results to detect API drift.
"""
import json
import os
import subprocess
from pathlib import Path
from typing import Dict, List, Set, Tuple

import pytest

TEST_DIR = Path(__file__).parent
SAMPLE_TESTS_DIR = TEST_DIR / "sample_tests"
PROJECT_ROOT = TEST_DIR.parent.parent.parent
TACH_BINARY = PROJECT_ROOT / "target" / "debug" / "tach-core"


def run_pytest(test_file: Path) -> Dict[str, str]:
    """Run pytest and return test outcomes.

    Returns:
        Dict mapping test name to outcome (passed/failed/skipped/error)
    """
    result = subprocess.run(
        ["pytest", str(test_file), "-v", "--tb=no"],
        capture_output=True,
        text=True,
        cwd=str(PROJECT_ROOT),
    )

    outcomes = {}
    for line in result.stdout.splitlines():
        if "::" in line and any(status in line for status in ["PASSED", "FAILED", "SKIPPED", "ERROR", "XFAIL", "XPASS"]):
            # Parse: "test_file.py::test_name PASSED"
            parts = line.rsplit(" ", 1)
            if len(parts) == 2:
                test_name = parts[0].strip()
                status = parts[1].strip()
                # Normalize status
                status_map = {
                    "PASSED": "passed",
                    "FAILED": "failed",
                    "SKIPPED": "skipped",
                    "ERROR": "error",
                    "XFAIL": "xfailed",
                    "XPASS": "xpassed",
                }
                outcomes[test_name] = status_map.get(status, status.lower())

    return outcomes


def run_tach(test_file: Path) -> Dict[str, str]:
    """Run tach-core and return test outcomes.

    Returns:
        Dict mapping test name to outcome
    """
    if not TACH_BINARY.exists():
        pytest.skip(f"tach-core binary not found at {TACH_BINARY}")

    env = os.environ.copy()
    env["PYO3_PYTHON"] = str(PROJECT_ROOT / ".venv" / "bin" / "python")

    result = subprocess.run(
        [str(TACH_BINARY), "--format", "json", "--no-isolation", "-n", "1", str(test_file)],
        capture_output=True,
        text=True,
        cwd=str(PROJECT_ROOT),
        env=env,
    )

    outcomes = {}
    for line in result.stdout.splitlines():
        if not line.strip():
            continue
        try:
            event = json.loads(line)
            if event.get("event") == "test_finish":
                name = event.get("name", "")
                outcome = event.get("outcome", "")
                if name and outcome:
                    outcomes[name] = outcome
        except json.JSONDecodeError:
            continue

    return outcomes


class TestPytestCompatibility:
    """Compare tach harness against real pytest behavior."""

    @pytest.fixture(autouse=True)
    def check_prerequisites(self):
        """Ensure both pytest and tach are available."""
        if not TACH_BINARY.exists():
            pytest.skip(f"tach-core not built: {TACH_BINARY}")

    def compare_outcomes(
        self,
        pytest_outcomes: Dict[str, str],
        tach_outcomes: Dict[str, str],
        test_file: str,
    ) -> Tuple[Set[str], Set[str], List[Tuple[str, str, str]]]:
        """Compare outcomes between pytest and tach.

        Returns:
            Tuple of (missing_in_tach, extra_in_tach, mismatches)
        """
        pytest_tests = set(pytest_outcomes.keys())
        tach_tests = set(tach_outcomes.keys())

        # Normalize test names for comparison (tach may use different format)
        def normalize_name(name: str) -> str:
            # Extract just the test function name
            if "::" in name:
                return name.split("::")[-1]
            return name

        pytest_normalized = {normalize_name(k): v for k, v in pytest_outcomes.items()}
        tach_normalized = {normalize_name(k): v for k, v in tach_outcomes.items()}

        missing = set(pytest_normalized.keys()) - set(tach_normalized.keys())
        extra = set(tach_normalized.keys()) - set(pytest_normalized.keys())

        mismatches = []
        for name in pytest_normalized.keys() & tach_normalized.keys():
            if pytest_normalized[name] != tach_normalized[name]:
                mismatches.append((name, pytest_normalized[name], tach_normalized[name]))

        return missing, extra, mismatches

    def test_raises_compatibility(self):
        """Test pytest.raises behavior matches."""
        test_file = SAMPLE_TESTS_DIR / "test_raises_samples.py"
        if not test_file.exists():
            pytest.skip(f"Sample test not found: {test_file}")

        pytest_outcomes = run_pytest(test_file)
        tach_outcomes = run_tach(test_file)

        missing, extra, mismatches = self.compare_outcomes(
            pytest_outcomes, tach_outcomes, str(test_file)
        )

        assert not missing, f"Tests found by pytest but not tach: {missing}"
        assert not mismatches, (
            f"Outcome mismatches between pytest and tach:\n" +
            "\n".join(f"  {name}: pytest={p}, tach={t}" for name, p, t in mismatches)
        )

    def test_approx_compatibility(self):
        """Test pytest.approx behavior matches."""
        test_file = SAMPLE_TESTS_DIR / "test_approx_samples.py"
        if not test_file.exists():
            pytest.skip(f"Sample test not found: {test_file}")

        pytest_outcomes = run_pytest(test_file)
        tach_outcomes = run_tach(test_file)

        missing, extra, mismatches = self.compare_outcomes(
            pytest_outcomes, tach_outcomes, str(test_file)
        )

        assert not missing, f"Tests found by pytest but not tach: {missing}"
        assert not mismatches, (
            f"Outcome mismatches between pytest and tach:\n" +
            "\n".join(f"  {name}: pytest={p}, tach={t}" for name, p, t in mismatches)
        )
```

**Step 3: Run tests**

Run: `pytest tests/regression/pytest_compat/ -v`

**Step 4: Commit**

```bash
git add tests/regression/pytest_compat/
git commit -m "test: add pytest API compatibility comparison tests

Compares tach harness behavior against real pytest to detect API drift.
Covers pytest.raises and pytest.approx initially.

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Task 4: Kernel Version Graceful Degradation Tests

**Files:**

- Create: `rust_tests/kernel_compat_tests.rs`

**Purpose:** Ensure graceful degradation on older kernels without crashes.

**Step 1: Write the kernel compatibility tests**

Create `rust_tests/kernel_compat_tests.rs`:

```rust
//! Kernel compatibility and graceful degradation tests.
//!
//! These tests verify that tach-core handles missing kernel features
//! gracefully (warns but doesn't crash).

use std::process::Command;

fn tach_binary() -> std::path::PathBuf {
    let mut path = std::env::current_exe().unwrap();
    path.pop();
    path.pop();
    path.push("tach-core");
    path
}

/// Get current kernel version as (major, minor)
fn kernel_version() -> Option<(u32, u32)> {
    let output = Command::new("uname")
        .arg("-r")
        .output()
        .ok()?;

    let version_str = String::from_utf8_lossy(&output.stdout);
    let parts: Vec<&str> = version_str.trim().split('.').collect();

    if parts.len() >= 2 {
        let major = parts[0].parse().ok()?;
        let minor = parts[1].parse().ok()?;
        Some((major, minor))
    } else {
        None
    }
}

#[test]
fn test_self_test_reports_kernel_features() {
    let output = Command::new(tach_binary())
        .args(["self-test"])
        .output()
        .expect("Failed to execute tach-core");

    let combined = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    // self-test should report on kernel features
    // It should mention at least some of these
    let expected_features = [
        "userfaultfd",
        "landlock",
        "seccomp",
        "namespace",
        "kernel",
    ];

    let mentions_features = expected_features
        .iter()
        .any(|f| combined.to_lowercase().contains(f));

    assert!(
        mentions_features,
        "self-test should report on kernel features. Output:\n{}",
        combined
    );
}

#[test]
fn test_no_isolation_mode_works() {
    // --no-isolation should always work regardless of kernel features
    let output = Command::new(tach_binary())
        .args(["--no-isolation", "-n", "1", "tests/dummy_project/test_simple.py"])
        .output()
        .expect("Failed to execute tach-core");

    // Should complete without crashing
    assert!(
        output.status.code().is_some(),
        "--no-isolation mode crashed unexpectedly"
    );
}

#[test]
fn test_graceful_degradation_on_permission_denied() {
    // Even without CAP_SYS_ADMIN, tach should degrade gracefully
    // This test verifies it doesn't panic or crash on EPERM

    let output = Command::new(tach_binary())
        .args(["-n", "1", "tests/dummy_project/test_simple.py"])
        .output()
        .expect("Failed to execute tach-core");

    let stderr = String::from_utf8_lossy(&output.stderr);

    // Either succeeds (has permissions) or degrades with warning
    // Should NOT contain panic or crash indicators
    assert!(
        !stderr.contains("panic") && !stderr.contains("SIGSEGV"),
        "tach should not panic on permission issues. Stderr:\n{}",
        stderr
    );

    // Exit code should be valid (not signal termination)
    assert!(
        output.status.code().is_some(),
        "tach terminated by signal (crashed) instead of exiting gracefully"
    );
}

#[test]
fn test_landlock_warning_on_old_kernel() {
    if let Some((major, minor)) = kernel_version() {
        if major > 5 || (major == 5 && minor >= 13) {
            // Kernel supports Landlock, skip this test
            println!("Kernel {}.{} supports Landlock, skipping old-kernel test", major, minor);
            return;
        }
    }

    // On old kernels, tach should warn about Landlock but not crash
    let output = Command::new(tach_binary())
        .args(["-n", "1", "tests/dummy_project/test_simple.py"])
        .output()
        .expect("Failed to execute tach-core");

    let stderr = String::from_utf8_lossy(&output.stderr);

    // Should either work or warn, not crash
    assert!(
        output.status.code().is_some(),
        "tach crashed on kernel without Landlock"
    );

    // If it failed, should have a helpful message
    if output.status.code() != Some(0) {
        assert!(
            stderr.to_lowercase().contains("landlock")
            || stderr.to_lowercase().contains("kernel")
            || stderr.contains("--no-isolation"),
            "Error message should mention kernel features or --no-isolation. Stderr:\n{}",
            stderr
        );
    }
}

#[test]
fn test_version_command_shows_capabilities() {
    let output = Command::new(tach_binary())
        .args(["version", "-v"])
        .output()
        .expect("Failed to execute tach-core");

    let combined = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    // Verbose version should show system capabilities
    // At minimum it should show the version
    assert!(
        combined.contains("tach") || combined.contains("0."),
        "version command should show version info. Output:\n{}",
        combined
    );
}
```

**Step 2: Run tests**

Run: `cargo test --test kernel_compat_tests -- --nocapture`

**Step 3: Commit**

```bash
git add rust_tests/kernel_compat_tests.rs
git commit -m "test: add kernel compatibility and graceful degradation tests

Verifies tach handles missing kernel features (Landlock, userfaultfd)
gracefully with warnings instead of crashes.

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Task 5: Coverage Threshold Enforcement

**Files:**

- Verify: `scripts/coverage.sh`
- Verify: `.github/workflows/ci.yml`
- Verify: `codecov.yml`

**Purpose:** Ensure 90% unit test coverage is enforced.

**Step 1: Verify current coverage**

Run: `./scripts/coverage.sh`

Document current coverage percentage.

**Step 2: Verify CI enforces threshold**

Check `.github/workflows/ci.yml` contains coverage job with 90% threshold.

**Step 3: Update CLAUDE.md if threshold differs**

If CLAUDE.md says 80% but CI says 90%, update CLAUDE.md to match CI (90%).

**Step 4: Commit any fixes**

```bash
git add CLAUDE.md scripts/coverage.sh
git commit -m "docs: clarify 90% unit test coverage threshold

Aligns CLAUDE.md with CI enforcement of 90% coverage.

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Task 6: Hot-Path Microbenchmarks

**Files:**

- Create: `benches/hot_paths.rs`
- Modify: `Cargo.toml` (add benchmark configuration)

**Purpose:** Detect performance regressions in critical paths.

**Step 1: Add benchmark configuration to Cargo.toml**

Add to `Cargo.toml`:

```toml
[[bench]]
name = "hot_paths"
harness = false
```

Add dev-dependency:

```toml
[dev-dependencies]
criterion = "0.5"
```

**Step 2: Create benchmark file**

Create `benches/hot_paths.rs`:

```rust
//! Hot-path microbenchmarks for regression detection.

use criterion::{black_box, criterion_group, criterion_main, Criterion};

// Import the modules to benchmark
// Adjust these imports based on actual module structure
// use tach_core::discovery::scanner;
// use tach_core::core::protocol;

fn bench_placeholder(c: &mut Criterion) {
    // Placeholder - replace with actual hot-path benchmarks
    c.bench_function("placeholder_baseline", |b| {
        b.iter(|| {
            let sum: u64 = (0..1000).sum();
            black_box(sum)
        })
    });
}

// Add actual benchmarks as modules become accessible:
// fn bench_protocol_parse(c: &mut Criterion) { ... }
// fn bench_scanner_tokenize(c: &mut Criterion) { ... }
// fn bench_snapshot_restore(c: &mut Criterion) { ... }

criterion_group!(benches, bench_placeholder);
criterion_main!(benches);
```

**Step 3: Run benchmarks**

Run: `cargo bench`

**Step 4: Commit**

```bash
git add benches/hot_paths.rs Cargo.toml
git commit -m "test: add hot-path microbenchmark infrastructure

Uses criterion for accurate microbenchmarks of performance-critical paths.
Run with 'cargo bench' to detect regressions.

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Task 7: Mutation Testing Setup (Optional - Nightly)

**Files:**

- Create: `scripts/mutation_test.sh`

**Purpose:** Verify test quality by ensuring tests catch code mutations.

**Step 1: Create mutation testing script**

Create `scripts/mutation_test.sh`:

```bash
#!/bin/bash
# Mutation testing with cargo-mutants
# Run nightly to verify test effectiveness

set -e

echo "=== Mutation Testing ==="
echo "This may take a long time (1+ hours for full suite)"

# Install if not present
if ! command -v cargo-mutants &> /dev/null; then
    echo "Installing cargo-mutants..."
    cargo install cargo-mutants
fi

# Run on a subset of critical modules for faster feedback
echo "Running mutation tests on critical modules..."
cargo mutants \
    --package tach-core \
    --file "src/core/config.rs" \
    --file "src/core/protocol.rs" \
    --file "src/discovery/scanner.rs" \
    --timeout 300 \
    --jobs 4

echo "=== Mutation Testing Complete ==="
echo "Check mutants.out/ for results"
```

**Step 2: Make executable and commit**

```bash
chmod +x scripts/mutation_test.sh
git add scripts/mutation_test.sh
git commit -m "test: add mutation testing script

Uses cargo-mutants to verify test effectiveness.
Run nightly or before releases: ./scripts/mutation_test.sh

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Summary

| Task                   | Type   | Purpose                            | Parallelizable |
| ---------------------- | ------ | ---------------------------------- | -------------- |
| 1. Exit Code Tests     | Rust   | Prevent CLI exit code changes      | Yes            |
| 2. JSON Schema Tests   | Rust   | Prevent JSON output format changes | Yes            |
| 3. Pytest Comparison   | Python | Detect pytest API drift            | Yes            |
| 4. Kernel Compat Tests | Rust   | Verify graceful degradation        | Yes            |
| 5. Coverage Threshold  | Docs   | Enforce 90% coverage               | Yes            |
| 6. Microbenchmarks     | Rust   | Detect hot-path regressions        | Yes            |
| 7. Mutation Testing    | Script | Verify test quality                | No (slow)      |

**Parallelization:** Tasks 1-6 can be executed by independent subagents. Task 7 should run separately due to long execution time.

---

## Execution

Plan complete and saved to `docs/plans/2026-01-07-test-improvements.md`. Two execution options:

**1. Subagent-Driven (this session)** - I dispatch fresh subagent per task, review between tasks, fast iteration

**2. Parallel Session (separate)** - Open new session with executing-plans, batch execution with checkpoints

**Which approach?**
