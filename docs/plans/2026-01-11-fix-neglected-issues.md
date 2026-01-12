# Fix Neglected Issues Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Fix all neglected issues from the documentation cleanup and discovery/sandbox work.

**Architecture:** Fix security vulnerabilities in Landlock rules, improve pattern detection warnings, add missing documentation, and clean up stale files.

**Tech Stack:** Rust, Python, Markdown

---

## Task 1: Delete Completed Plan File

**Files:**

- Delete: `docs/plans/2026-01-11-docs-cleanup.md`

**Step 1:** Delete the completed plan file

```bash
rm docs/plans/2026-01-11-docs-cleanup.md
```

**Step 2:** Commit

```bash
git add -A
git commit -m "chore: delete completed docs-cleanup plan"
```

---

## Task 2: Document --no-ignore Flag

**Files:**

- Modify: `docs/configuration.md`
- Modify: `CLAUDE.md`

**Step 1:** Add `--no-ignore` to `docs/configuration.md` in the "Tach-Specific Options" table (after line 91)

Add this row to the table:

```markdown
| `--no-ignore` | Bypass .ignore/.gitignore files during discovery | false |
```

**Step 2:** Add `--no-ignore` to `CLAUDE.md` CLI Reference section

Find the Options table and add:

```markdown
| `--no-ignore` | `TACH_NO_IGNORE` | Bypass .ignore/.gitignore during discovery | false |
```

**Step 3:** Commit

```bash
git add docs/configuration.md CLAUDE.md
git commit -m "docs: add --no-ignore flag to CLI documentation"
```

---

## Task 3: Fix Volatile Data in test-discovery-analysis.md

**Files:**

- Modify: `docs/research/test-discovery-analysis.md`

**Issue:** Hardcoded test counts like "24 ignored tests", "15 tests", "3 tests"

**Step 1:** Replace specific counts with relative descriptions

Replace:

- "24 ignored tests" → "multiple ignored tests"
- "15 tests" → "most tests" or describe the category
- "3 tests" → "a few tests" or describe the category
- Update the Executive Summary to use descriptive language instead of exact numbers

**Step 2:** Update all "Total: N tests" lines to remove exact counts or note they are approximate

**Step 3:** Commit

```bash
git add docs/research/test-discovery-analysis.md
git commit -m "docs: remove volatile test counts from test-discovery-analysis"
```

---

## Task 4: Fix Landlock Security - Restrict project_root Access

**Files:**

- Modify: `src/isolation/sandbox.rs`

**Issue:** `all_access` for project_root allows `MAKE_CHAR` and `MAKE_BLOCK`, enabling device node creation escape.

**Step 1:** Create a new `write_access` constant that excludes dangerous rights

Find line ~178 where `all_access` is defined. Add a new constant:

```rust
// Safe write access - excludes device node creation
let safe_write_access = AccessFs::READ_FILE
    | AccessFs::WRITE_FILE
    | AccessFs::READ_DIR
    | AccessFs::REMOVE_DIR
    | AccessFs::REMOVE_FILE
    | AccessFs::MAKE_DIR
    | AccessFs::MAKE_REG
    | AccessFs::MAKE_SYM
    | AccessFs::EXECUTE;
```

**Step 2:** Replace `all_access` with `safe_write_access` for project_root

Find line ~209 where project_root is added and change:

```rust
// Before:
let ruleset = add_path_rule(ruleset, &project_root, all_access)?;
// After:
let ruleset = add_path_rule(ruleset, &project_root, safe_write_access)?;
```

**Step 3:** Write test to verify device node creation is blocked

Add test to `rust_tests/sandbox_enforcement.rs`:

```rust
#[test]
fn test_sandbox_blocks_device_node_creation() {
    // This test verifies that MAKE_CHAR/MAKE_BLOCK are blocked
    // in project_root by attempting mknod and expecting EACCES
}
```

**Step 4:** Run tests

```bash
cargo test --test sandbox_enforcement -- test_sandbox_blocks_device_node
```

**Step 5:** Commit

```bash
git add src/isolation/sandbox.rs rust_tests/sandbox_enforcement.rs
git commit -m "security: restrict Landlock access for project_root

Remove MAKE_CHAR and MAKE_BLOCK from project_root permissions
to prevent device node creation escape attacks."
```

---

## Task 5: Fix Landlock Security - Remove Excessive /run Access

**Files:**

- Modify: `src/isolation/sandbox.rs`

**Issue:** Line 235 adds `all_access` to entire `/run` directory, which contains sensitive files like `docker.sock`.

**Step 1:** Remove or restrict the `/run` rule

Find line ~235 and either:

- Remove the line entirely (worker scratch at `/run/tach/worker_{id}` is already added)
- Or change to read-only access

**Step 2:** Run sandbox tests to verify nothing breaks

```bash
cargo test --test sandbox_enforcement
```

**Step 3:** Commit

```bash
git add src/isolation/sandbox.rs
git commit -m "security: remove excessive /run access from Landlock rules"
```

---

## Task 6: Improve Dangerous Pattern Detection - Proactive Warning

**Files:**

- Modify: `src/main.rs`

**Issue:** Warning only shows when 0 tests found. Should warn even when some tests are found if dangerous patterns exist.

**Step 1:** Modify `warn_if_blocking_patterns` function (line ~659)

Change from:

```rust
fn warn_if_blocking_patterns(cwd: &Path, is_empty: bool, is_json: bool) {
    if is_empty && !is_json {
```

To:

```rust
fn warn_if_blocking_patterns(cwd: &Path, is_empty: bool, is_json: bool) {
    if is_json {
        return;
    }

    let patterns = discovery::detect_blocking_patterns(cwd);
    if patterns.is_empty() {
        return;
    }

    if is_empty {
        eprintln!("[tach:discovery] WARNING: No tests discovered!");
        eprintln!("[tach:discovery] These patterns in .ignore may be blocking Python files:");
    } else {
        eprintln!("[tach:discovery] NOTE: These patterns in .ignore may be blocking some tests:");
    }

    for pattern in &patterns {
        eprintln!("  - {}", pattern);
    }
    eprintln!("[tach:discovery] Try running with --no-ignore to verify.");
}
```

**Step 2:** Update tests in `rust_tests/discovery_integration.rs` if needed

**Step 3:** Run tests

```bash
cargo test --test discovery_integration
```

**Step 4:** Commit

```bash
git add src/main.rs
git commit -m "feat: warn about blocking patterns even when some tests found

Previously only warned when 0 tests discovered. Now shows a NOTE
if dangerous patterns exist in .ignore but some tests were still found."
```

---

## Task 7: Improve Symlink Escape Test

**Files:**

- Modify: `tests/gauntlet/test_fs_destruction.py`

**Issue:** Test uses `/root/.bashrc` which may not exist in all environments, causing test to skip.

**Step 1:** Use a more reliable target with fallback logic

Replace the current test target selection with:

```python
def test_symlink_escape_prevention():
    """Verify symlink traversal cannot escape sandbox."""
    # Try multiple targets that should be protected but readable by root
    targets = [
        "/root/.bashrc",      # Common in containers
        "/etc/hostname",      # Always exists in containers
        "/etc/machine-id",    # Usually exists
    ]

    target = None
    for t in targets:
        if os.path.exists(t):
            target = t
            break

    if target is None:
        pytest.skip("No suitable symlink target found")

    # Rest of test...
```

**Step 2:** Run the test

```bash
./target/release/tach-core tests/gauntlet/test_fs_destruction.py::test_symlink_escape_prevention -v
```

**Step 3:** Commit

```bash
git add tests/gauntlet/test_fs_destruction.py
git commit -m "test: improve symlink escape test with fallback targets"
```

---

## Task 8: Document Known Limitations

**Files:**

- Modify: `docs/troubleshooting.md`

**Issue:** Missing edge case tests (autouse fixtures, nested TestClass, etc.) should be documented as known limitations.

**Step 1:** Add "Known Limitations" section to troubleshooting.md

```markdown
## Known Limitations

### Static Discovery Limitations

Tach uses static AST analysis for test discovery, which cannot detect:

| Feature                 | Limitation                                     | Workaround                             |
| ----------------------- | ---------------------------------------------- | -------------------------------------- |
| `pytest_generate_tests` | Dynamic test generation not visible statically | Use explicit parametrize decorators    |
| Autouse fixtures        | May not be fully detected in all cases         | Document in test or use explicit marks |
| Nested TestClass        | Deeply nested classes may not be discovered    | Flatten test class hierarchy           |
| Plugin-generated tests  | Tests created by plugins at runtime            | Run with `--collect-only` to verify    |

These limitations are inherent to static analysis. If tests are missing, use `--no-ignore` to verify they aren't being filtered, or run `pytest --collect-only` to compare discovery results.
```

**Step 2:** Commit

```bash
git add docs/troubleshooting.md
git commit -m "docs: add Known Limitations section for static discovery"
```

---

## Task 9: Final Verification

**Step 1:** Run all tests

```bash
cargo test --lib
cargo test --test '*'
```

**Step 2:** Verify documentation is complete

```bash
grep -n "no-ignore" docs/configuration.md CLAUDE.md
grep -n "Known Limitations" docs/troubleshooting.md
```

**Step 3:** Check for remaining volatile data

```bash
grep -rn "24 ignored\|15 tests\|3 tests" docs/research/ --include="*.md"
```

Expected: No matches or only legitimate examples.

---

## Summary

| Task | Description                    | Priority            |
| ---- | ------------------------------ | ------------------- |
| 1    | Delete completed plan file     | Low                 |
| 2    | Document --no-ignore flag      | Medium              |
| 3    | Fix volatile test counts       | Low                 |
| 4    | Restrict Landlock project_root | **High (Security)** |
| 5    | Remove excessive /run access   | **High (Security)** |
| 6    | Proactive pattern warnings     | Medium              |
| 7    | Improve symlink test           | Medium              |
| 8    | Document known limitations     | Low                 |
| 9    | Final verification             | Required            |

**Expected Results:**

- 2 security vulnerabilities fixed (Landlock)
- 1 usability improvement (proactive warnings)
- Documentation gaps filled
- Stale files cleaned up
