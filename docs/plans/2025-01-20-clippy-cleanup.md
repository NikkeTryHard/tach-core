# Clippy Cleanup Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Resolve 2 clippy warnings to achieve a warning-free build.

**Architecture:** Simple refactoring - replace deprecated type alias and collapse nested if statement.

**Tech Stack:** Rust, pyo3

---

## Task 1: Fix Deprecated PyObject Type Alias

**Files:**

- Modify: `src/execution/zygote.rs`

**Step 1: Run clippy to confirm current warnings**

Run: `cargo clippy 2>&1 | grep -E "(warning|error)"`
Expected: 2 warnings about `PyObject` and collapsible `if`

**Step 2: Update function return type**

In `src/execution/zygote.rs`, find:

```rust
fn json_value_to_py(py: Python<'_>, value: &serde_json::Value) -> PyResult<PyObject> {
```

Replace with:

```rust
fn json_value_to_py(py: Python<'_>, value: &serde_json::Value) -> PyResult<Py<PyAny>> {
```

**Step 3: Add import if needed**

Ensure `Py` and `PyAny` are imported. Check top of file for:

```rust
use pyo3::{Py, PyAny, ...};
```

**Step 4: Run clippy to verify warning resolved**

Run: `cargo clippy 2>&1 | grep "PyObject"`
Expected: No output (warning gone)

**Step 5: Run tests to verify no regressions**

Run: `cargo test --lib`
Expected: All tests pass

**Step 6: Commit**

```bash
git add src/execution/zygote.rs
git commit -m "refactor(zygote): replace deprecated PyObject with Py<PyAny>"
```

---

## Task 2: Collapse Nested If Statement

**Files:**

- Modify: `src/discovery/scanner.rs`

**Step 1: Locate the collapsible if**

In `src/discovery/scanner.rs` around line 651, find:

```rust
if let Some(ref arg_name) = keyword.arg {
    if let Some(value) = expr_to_json_value(&keyword.value) {
        args.insert(arg_name.to_string(), value);
    }
}
```

**Step 2: Collapse into single if-let chain**

Replace with:

```rust
if let (Some(ref arg_name), Some(value)) = (&keyword.arg, expr_to_json_value(&keyword.value)) {
    args.insert(arg_name.to_string(), value);
}
```

**Step 3: Run clippy to verify warning resolved**

Run: `cargo clippy 2>&1 | grep "collapsible"`
Expected: No output (warning gone)

**Step 4: Run tests to verify no regressions**

Run: `cargo test --lib`
Expected: All tests pass

**Step 5: Commit**

```bash
git add src/discovery/scanner.rs
git commit -m "refactor(scanner): collapse nested if-let into single pattern"
```

---

## Task 3: Final Verification

**Step 1: Run full clippy check**

Run: `cargo clippy 2>&1`
Expected: No warnings (only "Finished" message)

**Step 2: Run full test suite**

Run: `cargo test`
Expected: All tests pass

**Step 3: Squash commits (optional)**

If preferred, squash into single commit:

```bash
git rebase -i HEAD~2
# Change second commit to "squash"
# New message: "refactor: resolve clippy warnings"
```

---

## Summary

| Task | File                       | Change                   |
| ---- | -------------------------- | ------------------------ |
| 1    | `src/execution/zygote.rs`  | `PyObject` → `Py<PyAny>` |
| 2    | `src/discovery/scanner.rs` | Collapse nested if-let   |
| 3    | Verification               | Confirm zero warnings    |

**Estimated time:** 5-10 minutes
