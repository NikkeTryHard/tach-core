# Fix Parametrized Test Discovery - Implementation Plan

> **REQUIRED:** Use `execute-plan` to implement this plan batch by batch.

**Goal:** Fix the 8 test failures caused by parametrized tests not being expanded into multiple node_ids
**Architecture:** Extend scanner.rs to parse @pytest.mark.parametrize values and generate multiple TestCase entries
**Tech Stack:** Rust, rustpython_parser AST

---

## Background

The Rust scanner discovers `test_name` but pytest collects `test_name[param0]`, `test_name[param1]`, etc.

Example:
```python
@pytest.mark.parametrize("exc,msg", [
    (ValueError, "Validation failed"),
    (RuntimeError, "Runtime issue"),
])
def test_foo(exc, msg): ...
```

Rust discovers: `test_foo` (1 test)
Pytest collects: `test_foo[ValueError-Validation failed]`, `test_foo[RuntimeError-Runtime issue]` (2 tests)

This causes "Test not found in Zygote session" errors.

---

### Batch 1: Extract Parametrize Values

**Goal:** Parse the parameter values from @pytest.mark.parametrize decorators.

#### Task 1.1: Add ParametrizeValue struct

**Files:**
- Modify: `src/discovery/scanner.rs`

**Step 1: Write failing test**
```rust
#[test]
fn test_extract_parametrize_values_simple() {
    let code = r#"
import pytest

@pytest.mark.parametrize("name", ["alice", "bob"])
def test_greet(name):
    pass
"#;
    let result = parse_test_file(code, Path::new("test.py")).unwrap();

    // Should have 2 tests: test_greet[alice], test_greet[bob]
    assert_eq!(result.tests.len(), 2);
    assert!(result.tests.iter().any(|t| t.name == "test_greet[alice]"));
    assert!(result.tests.iter().any(|t| t.name == "test_greet[bob]"));
}
```

**Step 2: Verify failure**
Run: `cargo nextest run -E 'test(test_extract_parametrize_values)' --lib`
Expected: FAIL - only 1 test discovered

**Step 3: Implement**

Add helper to extract parameter values:
```rust
/// Extract parameter IDs from @pytest.mark.parametrize
/// Returns Vec of parameter ID strings like ["alice", "bob"] or ["0", "1"] for complex values
fn extract_parametrize_ids(decorator: &Decorator) -> Vec<String> {
    // Parse the second argument of parametrize (the values list)
    // For simple values: use repr
    // For tuples: join with "-"
    // For complex objects: use index
}
```

Update `analyze_function` to expand tests:
```rust
// Instead of pushing single TestCase, expand parametrized tests
let param_ids = extract_parametrize_ids(&decorator);
if param_ids.is_empty() {
    tests.push(single_test);
} else {
    for id in param_ids {
        let mut test = single_test.clone();
        test.name = format!("{}[{}]", test.name, id);
        tests.push(test);
    }
}
```

**Step 4: Verify pass**
Run: `cargo nextest run -E 'test(test_extract_parametrize_values)' --lib`
Expected: PASS

**Step 5: Commit**
```bash
git add src/discovery/scanner.rs
git commit -m "$(cat <<'EOF'
feat(discovery): extract parametrize values for test expansion

Parse @pytest.mark.parametrize decorator values and generate
parameter IDs matching pytest's collection format.
EOF
)"
```

---

#### Task 1.2: Handle tuple parameters

**Files:**
- Modify: `src/discovery/scanner.rs`

**Step 1: Write failing test**
```rust
#[test]
fn test_extract_parametrize_values_tuples() {
    let code = r#"
import pytest

@pytest.mark.parametrize("exc,msg", [
    (ValueError, "Validation failed"),
    (RuntimeError, "Runtime issue"),
])
def test_errors(exc, msg):
    pass
"#;
    let result = parse_test_file(code, Path::new("test.py")).unwrap();

    // Should have 2 tests with tuple-style IDs
    assert_eq!(result.tests.len(), 2);
    assert!(result.tests.iter().any(|t| t.name.contains("ValueError")));
    assert!(result.tests.iter().any(|t| t.name.contains("RuntimeError")));
}
```

**Step 2: Verify failure**
Run: `cargo nextest run -E 'test(test_extract_parametrize_values_tuples)' --lib`
Expected: FAIL

**Step 3: Implement**
Extend `extract_parametrize_ids` to handle tuples:
```rust
// For tuples like (ValueError, "msg"), join first elements with "-"
// Match pytest's default ID generation
```

**Step 4: Verify pass**
Run: `cargo nextest run -E 'test(test_extract_parametrize_values_tuples)' --lib`
Expected: PASS

**Step 5: Commit**
```bash
git add src/discovery/scanner.rs
git commit -m "$(cat <<'EOF'
feat(discovery): handle tuple parameters in parametrize

Generate IDs for tuple parameters matching pytest's format:
(ValueError, "msg") -> "ValueError-msg"
EOF
)"
```

---

### Batch 2: Handle Edge Cases

**Goal:** Handle pytest.param, ids=, and nested parametrize.

#### Task 2.1: Support pytest.param with id

**Files:**
- Modify: `src/discovery/scanner.rs`

**Step 1: Write failing test**
```rust
#[test]
fn test_parametrize_with_explicit_ids() {
    let code = r#"
import pytest

@pytest.mark.parametrize("val", [
    pytest.param(1, id="one"),
    pytest.param(2, id="two"),
])
def test_numbers(val):
    pass
"#;
    let result = parse_test_file(code, Path::new("test.py")).unwrap();

    assert_eq!(result.tests.len(), 2);
    assert!(result.tests.iter().any(|t| t.name == "test_numbers[one]"));
    assert!(result.tests.iter().any(|t| t.name == "test_numbers[two]"));
}
```

**Step 2: Verify failure**
Run: `cargo nextest run -E 'test(test_parametrize_with_explicit_ids)' --lib`
Expected: FAIL

**Step 3: Implement**
Handle `pytest.param(value, id="name")` syntax in the value extraction.

**Step 4: Verify pass**
Run: `cargo nextest run -E 'test(test_parametrize_with_explicit_ids)' --lib`
Expected: PASS

**Step 5: Commit**
```bash
git add src/discovery/scanner.rs
git commit -m "$(cat <<'EOF'
feat(discovery): support pytest.param with explicit id

Extract id= keyword argument from pytest.param() calls
for accurate test name generation.
EOF
)"
```

---

#### Task 2.2: Support ids= argument

**Files:**
- Modify: `src/discovery/scanner.rs`

**Step 1: Write failing test**
```rust
#[test]
fn test_parametrize_with_ids_list() {
    let code = r#"
import pytest

@pytest.mark.parametrize("val", [1, 2, 3], ids=["one", "two", "three"])
def test_with_ids(val):
    pass
"#;
    let result = parse_test_file(code, Path::new("test.py")).unwrap();

    assert_eq!(result.tests.len(), 3);
    assert!(result.tests.iter().any(|t| t.name == "test_with_ids[one]"));
    assert!(result.tests.iter().any(|t| t.name == "test_with_ids[two]"));
    assert!(result.tests.iter().any(|t| t.name == "test_with_ids[three]"));
}
```

**Step 2: Verify failure**
Run: `cargo nextest run -E 'test(test_parametrize_with_ids_list)' --lib`
Expected: FAIL

**Step 3: Implement**
Check for `ids=` keyword argument in parametrize call.

**Step 4: Verify pass**
Run: `cargo nextest run -E 'test(test_parametrize_with_ids_list)' --lib`
Expected: PASS

**Step 5: Commit**
```bash
git add src/discovery/scanner.rs
git commit -m "$(cat <<'EOF'
feat(discovery): support ids= argument in parametrize

Use explicit ids list when provided instead of generating
from parameter values.
EOF
)"
```

---

### Batch 3: Integration Testing

**Goal:** Verify fix works on AIstudioProxyAPI.

#### Task 3.1: Run tach-core on AIstudioProxyAPI

**Files:**
- No code changes

**Step 1: Build and test**
```bash
# In Docker
cd /workspace && cargo build --release
cd /workspace/test-aistudio && source .venv/bin/activate
/workspace/target/release/tach-core tests/api_utils/ 2>&1 | tail -20
```

**Step 2: Verify**
Expected: 0 "Test not found" errors, 677/677 tests discovered and run

**Step 3: Commit**
```bash
git commit --allow-empty -m "$(cat <<'EOF'
test: verify parametrize fix on AIstudioProxyAPI

All 677 tests now discovered and executed correctly.
No more "Test not found in Zygote session" errors.
EOF
)"
```

---

## Summary

| Batch | Tasks | Purpose |
|-------|-------|---------|
| 1 | 2 | Extract parametrize values and expand tests |
| 2 | 2 | Handle edge cases (pytest.param, ids=) |
| 3 | 1 | Integration testing |

**Total: 5 tasks across 3 batches**

After implementation:
- All 8 previously failing tests will pass
- Parametrized tests correctly expanded to match pytest collection
- No changes to toxicity detection (too risky without FD hooks)
