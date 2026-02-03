# Fix Toxicity Path Mismatch Implementation Plan

> **REQUIRED:** Use `execute-plan` to implement this plan batch by batch.

**Goal:** Fix path mismatch causing toxic tests to be misclassified as safe, leading to Seccomp blocking socket() syscalls.
**Architecture:** Canonicalize paths in ToxicityGraph lookups to ensure consistent matching regardless of relative/absolute path format.
**Tech Stack:** Rust, std::path::Path::canonicalize()

---

## Background

**Root Cause:** `ToxicityGraph::is_toxic()` performs direct HashMap lookup without canonicalizing paths. When the graph is built with relative paths but queried with absolute paths (or vice versa), the lookup fails and returns `false` (safe) by default.

**Evidence:**
- port_storm tests import `socket` → should be toxic
- Tests fail at `socket.socket()` creation → Seccomp blocking them
- Seccomp only blocks safe tests → tests misclassified as safe
- `is_toxic()` returns `unwrap_or(false)` on lookup miss

---

### Batch 1: Fix ToxicityGraph Path Lookups

**Goal:** Ensure path lookups work regardless of relative/absolute path format.

#### Task 1.1: Add Canonicalization to is_toxic()

**Files:**
- Modify: `src/discovery/graph.rs:209-214`
- Test: `src/discovery/graph.rs` (existing tests)

**Step 1: Write failing test**

Add to `src/discovery/graph.rs` in the `#[cfg(test)]` module:

```rust
#[test]
fn test_is_toxic_with_absolute_path() {
    use std::fs;
    use tempfile::TempDir;

    let temp = TempDir::new().unwrap();
    let toxic_file = temp.path().join("toxic.py");
    fs::write(&toxic_file, "import threading\ndef test_x(): pass").unwrap();

    // Build graph with relative-like path
    let graph = ToxicityGraph::build(&[toxic_file.clone()], &[]);

    // Query with canonicalized absolute path
    let canonical = toxic_file.canonicalize().unwrap();
    assert!(
        graph.is_toxic(&canonical),
        "Should find toxic status using absolute path"
    );

    // Query with the original path
    assert!(
        graph.is_toxic(&toxic_file),
        "Should find toxic status using original path"
    );
}
```

**Step 2: Verify failure**

Run: `cd /home/nikketryhard/dev/tach-core && cargo nextest run --lib -E 'test(is_toxic_with_absolute_path)'`

Expected: FAIL - assertion fails because paths don't match

**Step 3: Implement fix**

Modify `src/discovery/graph.rs` around line 209:

```rust
/// Check if a module at the given path is toxic
pub fn is_toxic(&self, path: &Path) -> bool {
    // Try direct lookup first
    if let Some(&idx) = self.path_to_node.get(path) {
        return self.graph[idx].is_toxic;
    }

    // Try canonicalized path lookup
    if let Ok(canonical) = path.canonicalize() {
        if let Some(&idx) = self.path_to_node.get(&canonical) {
            return self.graph[idx].is_toxic;
        }
    }

    // Try matching by filename as last resort
    if let Some(file_name) = path.file_name() {
        for (stored_path, &idx) in &self.path_to_node {
            if stored_path.file_name() == Some(file_name)
                && stored_path.ends_with(path)
            {
                return self.graph[idx].is_toxic;
            }
        }
    }

    false
}
```

**Step 4: Verify pass**

Run: `cd /home/nikketryhard/dev/tach-core && cargo nextest run --lib -E 'test(is_toxic)'`

Expected: All is_toxic tests PASS

**Step 5: Commit**

```bash
git add src/discovery/graph.rs
git commit -m "fix(toxicity): canonicalize paths in is_toxic() lookup

Fixes path mismatch where graph indexed with relative paths but queried
with absolute paths, causing toxic tests to be misclassified as safe."
```

---

#### Task 1.2: Also Fix is_toxic_by_name()

**Files:**
- Modify: `src/discovery/graph.rs:217-222`

**Step 1: Verify current behavior**

The `is_toxic_by_name()` function uses module name lookup which should work, but verify it handles edge cases.

**Step 2: Review and ensure consistency**

Read the function and confirm it doesn't have the same path issue.

**Step 3: Commit if changes needed**

---

### Batch 2: Add Integration Test for Toxic Test Execution

**Goal:** Verify toxic tests actually skip Seccomp and can create sockets.

#### Task 2.1: Create Socket Toxicity Integration Test

**Files:**
- Create: `rust_tests/toxicity_socket_test.rs`

**Step 1: Write failing test**

```rust
//! Integration test verifying toxic tests can create sockets

use std::process::Command;
use std::fs;
use tempfile::TempDir;

#[test]
fn test_toxic_socket_test_can_create_socket() {
    let temp = TempDir::new().unwrap();

    // Create a test file that imports socket (should be toxic)
    let test_file = temp.path().join("test_socket.py");
    fs::write(&test_file, r#"
import socket

def test_create_socket():
    """This test should be marked toxic and skip Seccomp."""
    sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    sock.close()
    assert True
"#).unwrap();

    // Run tach-core on this test
    let output = Command::new("cargo")
        .args(["run", "--", temp.path().to_str().unwrap(), "-v"])
        .output()
        .expect("Failed to run tach-core");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // Should show test as toxic
    assert!(
        stderr.contains("toxic") || stderr.contains("Toxic"),
        "Test should be marked as toxic. stderr: {}", stderr
    );

    // Test should pass (not fail due to Seccomp blocking socket)
    assert!(
        stdout.contains("1 passed") || stdout.contains("PASSED"),
        "Socket test should pass. stdout: {}\nstderr: {}", stdout, stderr
    );
}
```

**Step 2: Verify failure**

Run: `cd /home/nikketryhard/dev/tach-core && cargo nextest run -E 'test(toxic_socket)'`

Expected: FAIL until Batch 1 fix is applied

**Step 3: Verify pass after Batch 1**

After Batch 1 is complete, this test should pass.

**Step 4: Commit**

```bash
git add rust_tests/toxicity_socket_test.rs
git commit -m "test(integration): add socket toxicity verification test"
```

---

### Batch 3: Verify Fix with Docker Gauntlet Tests

**Goal:** Confirm pytest and tach-core results now match.

#### Task 3.1: Run Comparison Test

**Files:** None (verification only)

**Step 1: Build and deploy to Docker**

```bash
cd /home/nikketryhard/dev/tach-core
cargo build --release
docker cp target/release/tach-core tach-dev:/usr/local/bin/tach-core
```

**Step 2: Run pytest baseline**

```bash
docker exec tach-dev bash -c "cd /workspace && .venv/bin/pytest tests/gauntlet/ -v 2>&1 | tail -5"
```

Expected: `26 passed, 5 failed`

**Step 3: Run tach-core**

```bash
docker exec tach-dev bash -c "cd /workspace && tach-core tests/gauntlet/ -v 2>&1 | grep -E 'passed|failed|toxic'"
```

Expected:
- Should show port_storm tests as toxic
- Should show `26 passed, 5 failed` (matching pytest)

**Step 4: Document results**

If results match, the fix is complete.

**Step 5: Commit verification**

```bash
git add -A
git commit -m "docs: verify toxicity fix with gauntlet tests"
```

---

## Completion Criteria

- [ ] `is_toxic()` handles path format mismatches
- [ ] All existing toxicity tests pass
- [ ] New integration test verifies socket-using tests are toxic
- [ ] Docker gauntlet results match pytest (26 passed, 5 failed)
- [ ] All commits follow conventional commit format
