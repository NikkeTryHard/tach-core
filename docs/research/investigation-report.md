# Investigation Report: Tach-Core Issues and Improvements

> **Status**: Investigation Complete
> **Date**: 2026-01-11
> **Investigators**: Parallel agent analysis

---

## Executive Summary

This report consolidates findings from five parallel investigations into tach-core issues identified during Docker container testing and research documentation. Each issue includes root cause analysis, proposed solutions, and implementation recommendations.

| Issue                       | Priority | Effort | Risk |
| --------------------------- | -------- | ------ | ---- |
| Sandbox test failures       | High     | Medium | Low  |
| `--no-ignore` CLI flag      | High     | Low    | Low  |
| Dangerous pattern detection | Medium   | Low    | Low  |
| Missing edge case tests     | Medium   | High   | Low  |
| CI job for ignored tests    | Low      | Low    | Low  |

---

## Issue 1: Sandbox Test Failures (2 of 5 tests failing)

### Summary

When running `test_fs_destruction.py` through tach-core in Docker, 3 of 5 tests pass but 2 fail due to configuration mismatches.

### Root Cause A: CWD Not Writable (`test_fs_destruction`)

**Location:** `src/isolation/sandbox.rs:208`

```rust
let ruleset = add_path_rule(ruleset, &project_root, read_access)?;
```

The Landlock rule applies **read-only** access to `project_root`, but the OverlayFS expects the directory to be writable. The filesystem mount provides writability, but Landlock's access control overrides this.

**Fix:** Change `read_access` to `all_access`:

```rust
let ruleset = add_path_rule(ruleset, &project_root, all_access)?;
```

**Impact:** Low risk. Project root needs to be writable for test temp files and pytest artifacts.

### Root Cause B: Symlink Escape Readable (`test_symlink_escape_prevention`)

The test creates a symlink to `/etc/shadow` and expects the read to fail. However:

1. Container runs as **root** (uid=0)
2. `/etc` is intentionally allowed for reading (Python needs `/etc/hosts`, SSL certs)
3. Root can read `/etc/shadow` regardless

**Fix Options:**

| Option                               | Pros                 | Cons                |
| ------------------------------------ | -------------------- | ------------------- |
| Target `/root/` instead              | Works in containers  | Needs file to exist |
| Drop privileges post-fork            | Proper security test | More complex        |
| Mark test as container-expected-fail | Simple               | Less coverage       |

**Recommendation:** Update test to target `/root/.bashrc` or similar path not in the Landlock allow-list.

---

## Issue 2: `--no-ignore` CLI Flag

### Summary

The discovery system uses the `ignore` crate which respects `.ignore` and `.gitignore` files. If AI tools add `*.py` to `.ignore`, all Python files are silently excluded from discovery.

### Implementation Plan

**File:** `src/core/config.rs`

Add flag to CLI struct:

```rust
#[arg(long, help = "Ignore .gitignore and .ignore files during discovery")]
pub no_ignore: bool,
```

**File:** `src/discovery/scanner.rs`

Update `WalkBuilder` configuration:

```rust
pub fn discover(root: &Path, no_ignore: bool) -> Result<DiscoveryResult> {
    let canonical_root = root.canonicalize()?;

    let paths: Vec<PathBuf> = WalkBuilder::new(&canonical_root)
        .standard_filters(!no_ignore)  // Disable when flag is set
        .follow_links(true)
        .build()
        .filter_map(|e| e.ok())
        .filter(|e| is_test_file(e.path()))
        .map(|e| e.path().to_path_buf())
        .collect();
    // ...
}
```

**Environment Variable:** Also support `TACH_NO_IGNORE=1` for CI/scripts.

### API Changes

- `discover()` signature changes to accept `no_ignore: bool`
- All callers need updating (main.rs, tests)

---

## Issue 3: Dangerous Pattern Detection

### Summary

When zero tests are discovered, tach-core should warn if `.ignore` contains patterns that block Python files.

### Implementation Plan

**Location:** `src/discovery/scanner.rs` (after `discover()` returns empty)

```rust
fn detect_blocking_patterns(root: &Path) -> Vec<String> {
    let mut dangerous = Vec::new();

    let ignore_path = root.join(".ignore");
    if let Ok(content) = std::fs::read_to_string(&ignore_path) {
        let keywords = ["*.py", "test", "tests/", "conftest"];

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if keywords.iter().any(|k| trimmed.contains(k)) {
                dangerous.push(trimmed.to_string());
            }
        }
    }

    dangerous
}

// In discover() or caller:
if discovery_result.modules.is_empty() {
    let patterns = detect_blocking_patterns(&root);
    if !patterns.is_empty() {
        eprintln!("[tach:discovery] WARNING: No tests discovered!");
        eprintln!("[tach:discovery] The following patterns in .ignore may be blocking Python files:");
        for pattern in &patterns {
            eprintln!("  - {}", pattern);
        }
        eprintln!("[tach:discovery] Try running with --no-ignore or remove these patterns");
    }
}
```

### Warning Format

```
[tach:discovery] WARNING: No tests discovered!
[tach:discovery] The following patterns in .ignore may be blocking Python files:
  - *.py
[tach:discovery] Try running with --no-ignore or remove these patterns
```

---

## Issue 4: Missing Discovery Edge Case Tests

### Summary

The research document identified 11 potential gaps in discovery edge case coverage.

### Priority Matrix

| Edge Case                   | Risk   | Priority   | Notes                                                |
| --------------------------- | ------ | ---------- | ---------------------------------------------------- |
| Dynamic generation          | High   | **High**   | Can't discover `pytest_generate_tests`, need warning |
| Autouse fixtures            | Medium | **High**   | `autouse=True` not tracked in `FixtureDefinition`    |
| conftest.py hierarchy       | Medium | **High**   | Implemented but needs integration tests              |
| Nested TestClasses          | Medium | **Medium** | Scanner only parses first-level classes              |
| `usefixtures` decorator     | Medium | **Medium** | Class-level decorator not captured                   |
| `importorskip`              | Medium | **Low**    | Let pytest handle at runtime                         |
| Unicode in names            | Low    | **Low**    | Verify AST parser handles UTF-8                      |
| Very long names             | Low    | **Low**    | Check buffer/filesystem limits                       |
| Multiple fixture decorators | Low    | **Low**    | Rare, verify no crash                                |
| Yield fixtures              | Low    | **Low**    | Likely works, needs verification                     |
| Skip/Xfail markers          | Low    | **Low**    | Verify tests still discovered                        |

### Implementation Recommendations

**Phase 1: High Priority**

1. Add `autouse` field to `FixtureDefinition` struct
2. Update scanner to detect `autouse=True` in decorator
3. Update resolver to inject autouse fixtures
4. Add detection for `pytest_generate_tests` hook with warning

**Phase 2: Medium Priority**

1. Recursively parse nested `ClassDef` bodies in scanner
2. Parse `@pytest.mark.usefixtures` decorators on classes
3. Add hierarchical conftest integration test

**Phase 3: Low Priority**

1. Property tests for unicode/long names
2. Explicit tests for yield fixtures and skip markers

### Test Locations

| Type        | Location                              | Purpose           |
| ----------- | ------------------------------------- | ----------------- |
| Unit        | `src/discovery/scanner.rs::tests`     | AST parsing logic |
| Integration | `rust_tests/discovery_integration.rs` | File discovery    |
| Resolution  | `rust_tests/resolver_integration.rs`  | Fixture mapping   |

---

## Issue 5: CI Job for Ignored Tests

### Summary

24 tests are marked `#[ignore]` in the codebase. These should be run in CI to prevent silent regressions.

### Current Ignored Tests by Category

| Category                   | Count | Requirement                   |
| -------------------------- | ----- | ----------------------------- |
| Environment (built binary) | 15    | `cargo build --release` first |
| Slow (1000+ iterations)    | 3     | ~60s each                     |
| WIP/Experimental           | 3     | `--features experiments`      |
| Python environment         | 2     | Python interpreter + fork     |

### Proposed CI Job

**File:** `.github/workflows/ci.yml`

```yaml
ignored-tests:
  name: Ignored & Integration Tests
  runs-on: ubuntu-latest
  needs: [check]
  continue-on-error: true # Don't block PRs if these fail

  steps:
    - uses: actions/checkout@v4

    - name: Install Rust toolchain
      uses: dtolnay/rust-action@stable

    - name: Set up kernel features
      run: sudo sysctl -w vm.unprivileged_userfaultfd=1

    - name: Build release binary
      run: cargo build --release

    - name: Set up Python
      uses: actions/setup-python@v5
      with:
        python-version: "3.12"

    - name: Create venv
      run: |
        python -m venv .venv
        source .venv/bin/activate
        pip install pytest

    - name: Run ignored implementation tests
      run: |
        source .venv/bin/activate
        sudo -E cargo test --test implementation_tests -- --ignored

    - name: Run ignored memory tests
      run: cargo test --test memory_invariant -- --ignored --nocapture
```

### Considerations

- Use `continue-on-error: true` so these don't block PRs
- Requires `sudo` for kernel sysctl and privileged operations
- GitHub-hosted runners have limited kernel features
- Consider self-hosted runner for full sandbox testing

---

## Recommended Implementation Order

### Phase 1: Quick Wins (Low Effort, High Value)

1. **`--no-ignore` flag** - Simple CLI addition, fixes critical UX issue
2. **Dangerous pattern detection** - Proactive user warning
3. **CI job for ignored tests** - Visibility into test coverage

### Phase 2: Bug Fixes (Medium Effort)

1. **Sandbox test fix A** - Change Landlock rule for project_root
2. **Sandbox test fix B** - Update symlink test target

### Phase 3: Edge Cases (High Effort)

1. **Autouse fixtures** - Struct + scanner + resolver changes
2. **Nested TestClasses** - Recursive parsing
3. **Dynamic test detection** - Hook detection + warning
4. **Remaining edge cases** - Per priority matrix

---

## Files Modified by This Investigation

| File                                    | Changes Needed                       |
| --------------------------------------- | ------------------------------------ |
| `src/core/config.rs`                    | Add `--no-ignore` flag               |
| `src/discovery/scanner.rs`              | `no_ignore` param, pattern detection |
| `src/isolation/sandbox.rs`              | Change Landlock rule at line 208     |
| `tests/gauntlet/test_fs_destruction.py` | Update symlink test target           |
| `.github/workflows/ci.yml`              | Add `ignored-tests` job              |
| `rust_tests/discovery_integration.rs`   | Add edge case tests                  |

---

## Appendix: Agent Investigation Details

### Agent ac01462: Sandbox Test Failures

- Analyzed `src/isolation/sandbox.rs` Landlock rules
- Identified `read_access` vs `all_access` mismatch at line 208
- Traced OverlayFS mount expectations

### Agent ab103e3: `--no-ignore` Flag

- Identified `WalkBuilder.standard_filters(true)` as control point
- Mapped API signature changes needed
- Verified `ignore` crate supports per-call bypass

### Agent a7ebd52: Dangerous Pattern Detection

- Listed critical patterns: `*.py`, `test*.py`, `tests/`, `conftest.py`
- Designed detection algorithm
- Defined warning message format

### Agent aa3b5b5: Missing Edge Case Tests

- Reviewed 11 gaps from research document
- Analyzed scanner.rs and resolver.rs coverage
- Created priority matrix and test location recommendations

### Agent a4b9d4b: CI Configuration

- Analyzed current workflow structure
- Identified environment requirements for ignored tests
- Proposed `ignored-tests` job with continue-on-error
