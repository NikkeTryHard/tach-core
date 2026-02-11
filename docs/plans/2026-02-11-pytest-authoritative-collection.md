# Pytest-Authoritative Collection — Implementation Plan

> **REQUIRED:** Use `execute-plan` to implement this plan batch by batch.

**Goal:** Make pytest's `perform_collect()` the authoritative source of truth for which tests exist, eliminating the 142-module `tests.py` collection gap (GitHub issue #98)
**Architecture:** Add one IPC message after the existing session effects handshake — the Zygote sends its collected node IDs + metadata back to the Supervisor, which uses them to build/merge the `RunnableTest` list
**Tech Stack:** Rust (bincode IPC, serde), Python (pytest collection), PyO3

---

## Background

tach-core has dual discovery: Rust AST scanning (`scanner.rs`) and Python pytest collection (`tach_harness.py`). Today, only Rust's results drive scheduling. pytest's results sit in `_ITEMS_MAP` but never flow back to the Supervisor. Files like `tests.py` that Rust's `is_test_file()` rejects are silently never scheduled, even though pytest found them.

The fix: after the Zygote runs `perform_collect()`, send the collected test list back to the Supervisor over the existing IPC socket (same pattern as session effects). The Supervisor then uses pytest's list as the authoritative test schedule, enriched with Rust's toxicity/fixture data where available.

### Files Overview

| File | Role |
|------|------|
| `src/core/protocol.rs` | Wire format — new `CollectedTest` struct |
| `src/tach_harness.py` | Python — serialize `_ITEMS_MAP` keys + metadata |
| `src/execution/zygote.rs` | Zygote — call Python, send over socket |
| `src/main.rs` | Supervisor — receive, merge, replace test list |

### Data Flow (after)

```
Zygote: init_session() → perform_collect() → _ITEMS_MAP populated
Zygote: get_collected_tests() → Vec<CollectedTest> serialized
Zygote: cmd_socket.write_all(framed_collected_tests)  ← NEW (after session effects)
Supervisor: read collected tests from socket            ← NEW (after reading session effects)
Supervisor: merge with toxicity graph → final RunnableTest list
Scheduler: dispatches merged list (pytest IDs guaranteed in _ITEMS_MAP)
```

---

### Batch 1: Wire Format + Python Serialization

**Goal:** Define the `CollectedTest` struct on the Rust side and the `get_collected_tests()` function on the Python side.

#### Task 1.1: Add `CollectedTest` struct to protocol.rs

**Files:**
- Modify: `src/core/protocol.rs` (after `TestResult` struct, ~line 110)

**Step 1: Write failing test**
```rust
#[cfg(test)]
mod collected_test_tests {
    use super::*;

    #[test]
    fn test_collected_test_roundtrip() {
        let test = CollectedTest {
            node_id: "tests/test_foo.py::TestBar::test_baz".to_string(),
            file_path: "tests/test_foo.py".to_string(),
            markers: vec!["django_db".to_string(), "slow".to_string()],
            is_async: false,
        };

        let encoded = encode_with_length(&vec![test.clone()]).unwrap();
        let decoded: Vec<CollectedTest> = decode_with_limit(&encoded, MAX_PAYLOAD_SIZE).unwrap();

        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].node_id, "tests/test_foo.py::TestBar::test_baz");
        assert_eq!(decoded[0].file_path, "tests/test_foo.py");
        assert_eq!(decoded[0].markers, vec!["django_db", "slow"]);
        assert!(!decoded[0].is_async);
    }

    #[test]
    fn test_collected_test_empty_list() {
        let tests: Vec<CollectedTest> = vec![];
        let encoded = encode_with_length(&tests).unwrap();
        let decoded: Vec<CollectedTest> = decode_with_limit(&encoded, MAX_PAYLOAD_SIZE).unwrap();
        assert!(decoded.is_empty());
    }

    #[test]
    fn test_collected_test_large_batch() {
        let tests: Vec<CollectedTest> = (0..10000)
            .map(|i| CollectedTest {
                node_id: format!("tests/test_{}.py::test_func_{}", i / 10, i),
                file_path: format!("tests/test_{}.py", i / 10),
                markers: vec![],
                is_async: i % 3 == 0,
            })
            .collect();

        let encoded = encode_with_length(&tests).unwrap();
        let decoded: Vec<CollectedTest> = decode_with_limit(&encoded, MAX_PAYLOAD_SIZE).unwrap();
        assert_eq!(decoded.len(), 10000);
    }
}
```

**Step 2: Verify failure**
Run: `cargo nextest run --lib -E 'test(collected_test)'`
Expected: FAIL — `CollectedTest` not defined

**Step 3: Implement**
Add the struct to `src/core/protocol.rs` after the `TestResult` struct (~line 110):

```rust
/// Test metadata sent from Zygote back to Supervisor after pytest collection.
/// This is the authoritative test list — pytest found these, so they WILL exist
/// in _ITEMS_MAP when the worker looks them up.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectedTest {
    /// Full pytest node ID (e.g., "tests/test_foo.py::TestBar::test_baz[param]")
    pub node_id: String,
    /// File path relative to project root
    pub file_path: String,
    /// Marker names (e.g., ["django_db", "slow", "skip"])
    pub markers: Vec<String>,
    /// Whether the test function is async
    pub is_async: bool,
}
```

**Step 4: Verify pass**
Run: `cargo nextest run --lib -E 'test(collected_test)'`
Expected: 3 tests PASS

**Step 5: Commit**
```bash
git add src/core/protocol.rs
git commit -m "feat(protocol): add CollectedTest struct for pytest-authoritative collection"
```

#### Task 1.2: Add `get_collected_tests()` to tach_harness.py

**Files:**
- Modify: `src/tach_harness.py` (after `get_session_hook_effects()` function, ~line 2035)

**Step 1: Write failing test** (manual verification — Python function, no Rust test harness)

We'll verify this works by checking it's callable. The real test is in Batch 3 (integration).

**Step 2: Implement**
Add after the `get_session_hook_effects()` function (~line 2035):

```python
def get_collected_tests():
    """Return the authoritative test list from pytest's collection.

    Called by the Zygote after init_session() to send collected test metadata
    back to the Rust Supervisor. The Supervisor uses this as the source of truth
    for which tests exist, replacing Rust-only AST discovery.

    Returns a list of dicts, each containing:
      - node_id: Full pytest node ID (str)
      - file_path: File path relative to project root (str)
      - markers: List of marker names (list[str])
      - is_async: Whether the test is async (bool)
    """
    import inspect

    result = []
    for node_id, item in _ITEMS_MAP.items():
        # Extract file path relative to project root
        fspath = str(getattr(item, "fspath", ""))
        try:
            file_path = os.path.relpath(fspath, os.getcwd())
        except ValueError:
            file_path = fspath

        # Extract marker names
        markers = [m.name for m in getattr(item, "own_markers", [])]

        # Detect async
        obj = getattr(item, "obj", None)
        func = getattr(obj, "__func__", obj) if obj else None
        is_async = inspect.iscoroutinefunction(func) if func else False

        result.append({
            "node_id": node_id,
            "file_path": file_path,
            "markers": markers,
            "is_async": is_async,
        })

    return result
```

**Step 3: Verify**
No automated test — this is pure Python, verified in integration (Batch 3).

**Step 4: Commit**
```bash
git add src/tach_harness.py
git commit -m "feat(harness): add get_collected_tests() for pytest-authoritative collection"
```

---

### Batch 2: Zygote IPC — Send Collection Results

**Goal:** Have the Zygote call `get_collected_tests()` and send the results to the Supervisor over the existing IPC socket.

#### Task 2.1: Zygote sends collected tests after session effects

**Files:**
- Modify: `src/execution/zygote.rs` (after session effects send, ~line 786)

**Step 1: Write failing test**
```rust
// In zygote.rs tests module
#[test]
fn test_collected_tests_framing() {
    // Verify we can encode/decode a Vec<CollectedTest> through the wire format
    use crate::protocol::{CollectedTest, encode_with_length, decode_with_limit, MAX_PAYLOAD_SIZE, HEADER_SIZE};

    let tests = vec![
        CollectedTest {
            node_id: "test_foo.py::test_bar".to_string(),
            file_path: "test_foo.py".to_string(),
            markers: vec!["slow".to_string()],
            is_async: false,
        },
        CollectedTest {
            node_id: "tests.py::TestCache::test_get".to_string(),
            file_path: "tests.py".to_string(),
            markers: vec!["django_db".to_string()],
            is_async: true,
        },
    ];

    let framed = encode_with_length(&tests).unwrap();
    assert!(framed.len() > HEADER_SIZE);

    let decoded: Vec<CollectedTest> = decode_with_limit(&framed, MAX_PAYLOAD_SIZE).unwrap();
    assert_eq!(decoded.len(), 2);
    assert_eq!(decoded[1].node_id, "tests.py::TestCache::test_get");
    assert_eq!(decoded[1].file_path, "tests.py");
    assert!(decoded[1].is_async);
}
```

**Step 2: Verify failure**
Run: `cargo nextest run --lib -E 'test(collected_tests_framing)'`
Expected: FAIL or PASS (struct exists from Batch 1, this mainly validates the pattern)

**Step 3: Implement**
In `src/execution/zygote.rs`, after the session effects send block (~line 786), add:

```rust
    // COLLECTED TESTS IPC (Issue #98): Send pytest's authoritative test list
    // This follows the same framing pattern as session effects above.
    // The Supervisor uses this as the source of truth for which tests exist.
    let collected_tests = Python::attach(|py| -> Result<Vec<crate::protocol::CollectedTest>> {
        let harness = py.import("tach_harness")?;
        let collected_obj = harness.getattr("get_collected_tests")?.call0()?;
        let collected_list: &Bound<'_, pyo3::types::PyList> = collected_obj
            .downcast::<pyo3::types::PyList>()
            .map_err(|e| anyhow::anyhow!("get_collected_tests() didn't return a list: {}", e))?;

        let mut tests = Vec::with_capacity(collected_list.len());
        for item in collected_list.iter() {
            let dict = item
                .downcast::<pyo3::types::PyDict>()
                .map_err(|e| anyhow::anyhow!("collected test item is not a dict: {}", e))?;

            let node_id: String = dict
                .get_item("node_id")?
                .ok_or_else(|| anyhow::anyhow!("missing node_id"))?
                .extract()?;
            let file_path: String = dict
                .get_item("file_path")?
                .ok_or_else(|| anyhow::anyhow!("missing file_path"))?
                .extract()?;
            let markers: Vec<String> = dict
                .get_item("markers")?
                .ok_or_else(|| anyhow::anyhow!("missing markers"))?
                .extract()?;
            let is_async: bool = dict
                .get_item("is_async")?
                .ok_or_else(|| anyhow::anyhow!("missing is_async"))?
                .extract()?;

            tests.push(crate::protocol::CollectedTest {
                node_id,
                file_path,
                markers,
                is_async,
            });
        }
        Ok(tests)
    })?;

    eprintln!(
        "[tach:zygote] Collected {} tests from pytest (authoritative)",
        collected_tests.len()
    );

    let framed_collected = encode_with_length(&collected_tests)
        .map_err(|e| anyhow::anyhow!("Failed to encode collected tests: {}", e))?;
    cmd_socket.write_all(&framed_collected)?;
```

**Important:** This code must go AFTER `sys.modules["tach_harness"]` is registered (line 741) but BEFORE the command loop. The `Python::attach` block needs `tach_harness` importable. The best insertion point is right after `cmd_socket.write_all(&framed_effects)?;` (line 786).

**Step 4: Verify pass**
Run: `cargo nextest run --lib -E 'test(collected_tests_framing)'`
Expected: PASS

**Step 5: Commit**
```bash
git add src/execution/zygote.rs
git commit -m "feat(zygote): send collected tests back to Supervisor after session effects"
```

---

### Batch 3: Supervisor — Receive and Merge

**Goal:** The Supervisor reads the collected test list from the Zygote and uses it as the authoritative test schedule, merging with Rust's toxicity data.

#### Task 3.1: Supervisor receives collected tests

**Files:**
- Modify: `src/main.rs` (after session effects reception, ~line 1165)

**Step 1: Write failing test**

No unit test for this — it's IPC integration. We verify by:
1. Building successfully (`cargo build`)
2. The new `eprintln!` messages appear in output when running tests

**Step 2: Implement**

In `src/main.rs`, after the session effects block (~line 1165), add the receiver:

```rust
            // COLLECTED TESTS IPC (Issue #98): Receive pytest's authoritative test list
            // This replaces Rust-only discovery as the source of truth for which tests exist.
            let mut collected_header = [0u8; tach_core::protocol::HEADER_SIZE];
            cmd_sock_clone.read_exact(&mut collected_header)?;

            let collected_len = u32::from_le_bytes([
                collected_header[4],
                collected_header[5],
                collected_header[6],
                collected_header[7],
            ]) as usize;

            let collected_tests: Vec<tach_core::protocol::CollectedTest> = if collected_len > 0 {
                let mut collected_buf =
                    vec![0u8; tach_core::protocol::HEADER_SIZE + collected_len];
                collected_buf[..tach_core::protocol::HEADER_SIZE]
                    .copy_from_slice(&collected_header);
                cmd_sock_clone.read_exact(
                    &mut collected_buf[tach_core::protocol::HEADER_SIZE..],
                )?;

                tach_core::protocol::decode_with_limit(
                    &collected_buf,
                    tach_core::protocol::MAX_PAYLOAD_SIZE,
                )
                .unwrap_or_else(|e| {
                    eprintln!(
                        "[tach:supervisor] Warning: Failed to decode collected tests: {}",
                        e
                    );
                    vec![]
                })
            } else {
                vec![]
            };

            if !is_json {
                eprintln!(
                    "[tach:supervisor] Received {} collected tests from Zygote (pytest-authoritative)",
                    collected_tests.len()
                );
            }
```

**Step 3: Commit**
```bash
git add src/main.rs
git commit -m "feat(supervisor): receive collected tests from Zygote IPC"
```

#### Task 3.2: Build RunnableTest list from collected tests (the merge)

**Files:**
- Modify: `src/main.rs` (replace the existing `filtered_tests` construction, ~lines 519-541)

**Step 1: Implement**

After receiving collected tests, build the authoritative `RunnableTest` list. This replaces the old path filtering block.

The key logic: for each `CollectedTest` from Python, look up the corresponding Rust-discovered test (by matching `file_path` + deriving `test_name` from `node_id`). If found, use Rust's toxicity/fixture data. If not found (e.g., `tests.py` that Rust's scanner rejected), create a `RunnableTest` with conservative defaults (`is_toxic = true`).

```rust
            // --- PYTEST-AUTHORITATIVE TEST LIST (Issue #98) ---
            // Build RunnableTest list from pytest's collection, enriched with Rust metadata.
            // Python is the authority on WHAT tests exist.
            // Rust provides toxicity, fixtures, and hooks.
            let target = std::path::Path::new(target_path);
            let target_canonical = target
                .canonicalize()
                .unwrap_or_else(|_| target.to_path_buf());

            // Index Rust-discovered tests by node_id for O(1) lookup
            let mut rust_test_index: std::collections::HashMap<String, &resolver::RunnableTest> =
                std::collections::HashMap::new();
            for test in &runnable_tests {
                let node_id = format!(
                    "{}::{}",
                    test.file_path.to_string_lossy(),
                    test.test_name
                );
                rust_test_index.insert(node_id, test);
            }

            let mut filtered_tests: Vec<resolver::RunnableTest> = Vec::new();
            let mut python_only_count = 0usize;

            for ct in &collected_tests {
                // Path filter: only include tests under the target path
                let test_path = std::path::Path::new(&ct.file_path);
                let test_canonical = test_path
                    .canonicalize()
                    .unwrap_or_else(|_| test_path.to_path_buf());

                let in_target = test_canonical.starts_with(&target_canonical)
                    || test_canonical == target_canonical
                    || test_path.starts_with(target);

                if !in_target {
                    continue;
                }

                // Look up Rust metadata
                if let Some(rust_test) = rust_test_index.get(&ct.node_id) {
                    // Rust found this test too — use its rich metadata
                    filtered_tests.push((*rust_test).clone());
                } else {
                    // Python found this test but Rust didn't (e.g., tests.py)
                    // Create a RunnableTest with conservative defaults
                    python_only_count += 1;

                    // Extract test_name from node_id: "path/file.py::Class::method" → "Class::method"
                    let test_name = ct
                        .node_id
                        .split_once("::")
                        .map(|(_, name)| name.to_string())
                        .unwrap_or_else(|| ct.node_id.clone());

                    // Check toxicity graph for the file (if Rust scanned it at all)
                    let file_path = std::path::PathBuf::from(&ct.file_path);
                    let is_toxic = toxicity_graph.is_toxic(&file_path)
                        || ct.markers.iter().any(|m| m == "django_db");

                    filtered_tests.push(resolver::RunnableTest {
                        file_path,
                        test_name,
                        is_async: ct.is_async,
                        fixtures: vec![], // No static resolution available
                        is_toxic,
                        timeout_secs: None,
                        markers: ct.markers.clone(),
                        marker_info: vec![],
                    });
                }
            }

            if !is_json {
                eprintln!(
                    "[tach:supervisor] Selected {} tests to run ({} from Rust+Python, {} Python-only, filtered by path: {})",
                    filtered_tests.len(),
                    filtered_tests.len() - python_only_count,
                    python_only_count,
                    target_path
                );
            }
```

**IMPORTANT:** This block REPLACES the old `filtered_tests` construction at lines 519-541. The old code:
```rust
let filtered_tests: Vec<resolver::RunnableTest> = runnable_tests
    .into_iter()
    .filter(|test| { ... })
    .collect();
```
...is replaced by the new merge logic above.

However, the old `filtered_tests` block is used in **two places** in `main.rs`:
1. `execute_session()` (line 526) — the main test run path
2. `handle_dry_run_command()` (line 873) — the `--dry-run` path

Only the `execute_session()` path goes through the Zygote, so only that path gets the new merge logic. The `--dry-run` and `--collect-only` paths keep using Rust-only discovery (they don't fork a Zygote).

**Step 2: Verify**
Run: `cargo build`
Expected: Compiles with no errors

Run: `cargo nextest run --lib`
Expected: All 944+ existing tests pass

**Step 3: Commit**
```bash
git add src/main.rs
git commit -m "feat(supervisor): build test list from pytest collection, merge with Rust toxicity data"
```

---

### Batch 4: Handle Edge Cases + Cleanup

**Goal:** Handle empty collection, fix the `collected_tests` block placement in the fork parent, and ensure backward compatibility.

#### Task 4.1: Handle empty pytest collection gracefully

**Files:**
- Modify: `src/main.rs`

**Step 1: Implement**

After the merge block, add:

```rust
            // If pytest collected nothing, fall back to Rust-only discovery
            // This handles: broken conftest, pytest configuration errors, etc.
            if collected_tests.is_empty() && !runnable_tests.is_empty() {
                if !is_json {
                    eprintln!(
                        "[tach:supervisor] Warning: pytest collected 0 tests but Rust found {}. \
                         Falling back to Rust-only discovery.",
                        runnable_tests.len()
                    );
                }
                // Re-apply the old path filtering on Rust-discovered tests
                filtered_tests = runnable_tests
                    .into_iter()
                    .filter(|test| {
                        let test_path = std::path::Path::new(&test.file_path);
                        let test_canonical = test_path
                            .canonicalize()
                            .unwrap_or_else(|_| test_path.to_path_buf());
                        test_canonical.starts_with(&target_canonical)
                            || test_canonical == target_canonical
                            || test_path.starts_with(target)
                    })
                    .collect();
            }
```

**Step 2: Commit**
```bash
git add src/main.rs
git commit -m "feat(supervisor): fallback to Rust discovery when pytest collection is empty"
```

#### Task 4.2: Add `is_test_file` relaxation for toxicity scanning

**Files:**
- Modify: `src/discovery/scanner.rs` (~line 247)

**Step 1: Write failing test**
```rust
#[test]
fn test_is_test_file_accepts_tests_py() {
    assert!(is_test_name("tests.py"));
}
```

**Step 2: Verify failure**
Run: `cargo nextest run --lib -E 'test(is_test_file_accepts_tests_py)'`
Expected: FAIL — `tests.py` doesn't match current patterns

**Step 3: Implement**

In `src/discovery/scanner.rs`, modify `is_test_file()` (or the internal `is_test_name()` it calls) to also accept `tests.py`:

```rust
// Add to the pattern check (line ~247):
name.starts_with("test_") || name.ends_with("_test.py") || name == "conftest.py" || name == "tests.py"
```

This is NOT the zero-gap fix (that's the IPC change above). This is an optimization so Rust's toxicity graph also scans `tests.py` files, allowing accurate toxicity tagging instead of the conservative `is_toxic = true` default for Python-only tests.

**Step 4: Verify pass**
Run: `cargo nextest run --lib -E 'test(is_test_file_accepts_tests_py)'`
Expected: PASS

**Step 5: Commit**
```bash
git add src/discovery/scanner.rs
git commit -m "feat(scanner): accept tests.py in is_test_file for toxicity scanning"
```

#### Task 4.3: Export `CollectedTest` from lib.rs

**Files:**
- Modify: `src/lib.rs` (wherever protocol types are re-exported)

**Step 1: Verify**
Check if `protocol::CollectedTest` is already accessible via `tach_core::protocol::CollectedTest` in `main.rs`. If `protocol` module is already public, this task is a no-op.

Run: `cargo build`

If it compiles, this task is done. If not, add the re-export.

**Step 2: Commit** (if needed)
```bash
git add src/lib.rs
git commit -m "chore: export CollectedTest from protocol module"
```

---

### Batch 5: Verification

**Goal:** Verify the full pipeline works end-to-end.

#### Task 5.1: Build and run unit tests

**Step 1:**
```bash
cargo fmt && cargo clippy -- -D warnings
cargo nextest run --lib
```
Expected: All tests pass, no warnings

#### Task 5.2: Verify with a real test suite (if Docker available)

**Step 1:** Run tach-core against a project with `tests.py` files
```bash
# Inside Docker container with Django test suite
./target/debug/tach-core utils_tests/
```

Expected output should show:
- `[tach:zygote] Collected N tests from pytest (authoritative)`
- `[tach:supervisor] Received N collected tests from Zygote (pytest-authoritative)`
- `[tach:supervisor] Selected N tests to run (X from Rust+Python, Y Python-only, ...)`
- Python-only count > 0 for modules using `tests.py`

#### Task 5.3: Final commit

```bash
git add -A
git commit -m "feat: pytest-authoritative collection (closes #98)

Make pytest's perform_collect() the single source of truth for which
tests exist. The Zygote sends collected node IDs back to the Supervisor
via IPC, which merges them with Rust's toxicity/fixture data.

This fixes 142 Django modules using tests.py that were silently never
scheduled because Rust's is_test_file() rejected them."
```
