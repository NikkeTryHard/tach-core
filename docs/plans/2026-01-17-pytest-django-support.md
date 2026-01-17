# 0.2.1 pytest-django Support Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Implement first-class pytest-django support with marker argument parsing, database transaction isolation via SAVEPOINT/ROLLBACK, and core Django fixtures.

**Architecture:** Extend the existing 0.2.0 hook framework to parse `@pytest.mark.django_db` marker arguments (not just names), transmit them via `TestPayload`, and execute database isolation logic in `tach_harness.py`. The current Django isolation code already exists but doesn't respect marker arguments.

**Tech Stack:** Rust (scanner.rs, protocol.rs, registry.rs), Python (tach_harness.py), pytest-django compatibility

---

## Overview

The implementation is divided into 4 batches:

| Batch | Focus                        | Tasks                                                        |
| ----- | ---------------------------- | ------------------------------------------------------------ |
| 1     | Marker Argument Parsing      | Parse `django_db(transaction=True, databases=[...])` in Rust |
| 2     | Protocol & Effect Extensions | Extend TestPayload and HookEffect for Django marker metadata |
| 3     | Python Execution Logic       | Implement marker-aware transaction isolation in harness      |
| 4     | Tests & Integration          | Django gauntlet tests for all marker variants                |

---

## Batch 1: Marker Argument Parsing (Rust)

**Goal:** Extract marker arguments from decorators, not just marker names.

### Files:

- Modify: `src/discovery/scanner.rs`
- Modify: `src/discovery/mod.rs` (export new types)
- Test: `src/discovery/scanner.rs` (inline tests)

### Task 1.1: Define MarkerInfo struct

Add a new struct to represent markers with their arguments:

```rust
// In scanner.rs, after FixtureDefinition struct
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct MarkerInfo {
    pub name: String,
    #[serde(default)]
    pub args: HashMap<String, serde_json::Value>,
}
```

### Task 1.2: Add marker_info field to TestCase

Extend TestCase to include structured marker info alongside existing `markers: Vec<String>`:

```rust
// In TestCase struct, add:
#[serde(default)]
pub marker_info: Vec<MarkerInfo>,
```

### Task 1.3: Implement extract_marker_arguments helper

Create a helper function that extracts keyword arguments from a Call expression:

- Parse `@pytest.mark.django_db(transaction=True)` → `{"transaction": true}`
- Parse `@pytest.mark.django_db(databases=["default", "other"])` → `{"databases": ["default", "other"]}`
- Handle: `Constant` (bool, int, str), `List` (for databases), `Name` (True/False)

### Task 1.4: Modify extract_markers_from_decorators

Change the function signature to return `Vec<MarkerInfo>` instead of `Vec<String>`:

- Keep backward compatibility by also populating the existing `markers: Vec<String>` field
- For `@pytest.mark.name` (bare): return `MarkerInfo { name, args: {} }`
- For `@pytest.mark.name(kwargs)`: return `MarkerInfo { name, args: {parsed kwargs} }`

### Task 1.5: Update analyze_function to use new extraction

Update the call site in `analyze_function()` to populate both `markers` and `marker_info` fields.

### Task 1.6: Write unit tests for marker argument parsing

Add tests in the `#[cfg(test)]` module:

- Test bare marker: `@pytest.mark.django_db` → `MarkerInfo { name: "django_db", args: {} }`
- Test with bool: `@pytest.mark.django_db(transaction=True)` → `args: {"transaction": true}`
- Test with list: `@pytest.mark.django_db(databases=["default"])` → `args: {"databases": [...]}`
- Test multiple args: `@pytest.mark.django_db(transaction=True, reset_sequences=True)`

### Task 1.7: Run tests and commit Batch 1

```bash
cargo test --lib -- scanner
git add src/discovery/
git commit -m "feat(discovery): parse marker arguments from pytest decorators"
```

---

## Batch 2: Protocol & Effect Extensions (Rust)

**Goal:** Extend IPC protocol and hook effects to transmit Django marker metadata.

### Files:

- Modify: `src/core/protocol.rs`
- Modify: `src/hooks/registry.rs`
- Test: Inline tests in both files

### Task 2.1: Add MarkerInfo to TestPayload

Import and add the marker_info field to TestPayload:

```rust
// In protocol.rs
use crate::discovery::MarkerInfo;

// In TestPayload struct, add:
#[serde(default)]
pub marker_info: Vec<MarkerInfo>,
```

### Task 2.2: Add DjangoDbSetup HookEffect variant

Extend the HookEffect enum for Django-specific effects:

```rust
// In registry.rs HookEffect enum, add:
/// Django database marker configuration
DjangoDbSetup {
    transaction: bool,
    reset_sequences: bool,
    databases: Vec<String>,
},
```

### Task 2.3: Update encode/decode tests

Add roundtrip tests to verify TestPayload with marker_info serializes correctly:

- Create TestPayload with `marker_info: vec![MarkerInfo { name: "django_db", args: {...} }]`
- Encode with `encode_with_length`
- Decode with `decode_with_limit`
- Assert marker_info preserved

### Task 2.4: Add DjangoDbSetup effect tests

Test serialization of the new HookEffect variant:

- Create `HookEffect::DjangoDbSetup { transaction: true, ... }`
- Serialize to JSON and back
- Verify fields preserved

### Task 2.5: Run tests and commit Batch 2

```bash
cargo test --lib -- protocol
cargo test --lib -- registry
git add src/core/protocol.rs src/hooks/registry.rs src/discovery/
git commit -m "feat(protocol): add marker_info to TestPayload and DjangoDbSetup effect"
```

---

## Batch 3: Python Execution Logic

**Goal:** Implement marker-aware Django database isolation in tach_harness.py.

### Files:

- Modify: `src/tach_harness.py`
- Test: `tests/django_project/test_django_markers.py` (new)

### Task 3.1: Add parse_django_db_marker function

Create a function that extracts Django DB settings from marker_info:

```python
def _parse_django_db_marker(marker_info: list) -> dict | None:
    """
    Parse @pytest.mark.django_db marker arguments.

    Returns dict with keys: transaction, reset_sequences, databases
    Returns None if no django_db marker present.
    """
```

### Task 3.2: Add pre_fork_cleanup function

Create a function to close all Django connections before forking:

```python
def _close_django_connections():
    """Close all Django database connections before fork."""
    if "django" not in sys.modules:
        return
    try:
        from django.db import connections
        connections.close_all()
    except Exception:
        pass
```

This should be called in the Zygote before forking workers.

### Task 3.3: Refactor Django isolation in run_test

Modify the existing Django isolation code (lines ~1826-1860) to:

1. Check `marker_info` for `django_db` marker first
2. If `transaction=True` in marker: skip transaction wrapping entirely
3. If `transaction=False` (default): use existing atomic() wrapping
4. Respect `databases` list if provided (default to all aliases)

Current code wraps ALL databases in atomic() unconditionally. New code should:

- Only wrap databases specified in marker (or all if not specified)
- Skip wrapping if `transaction=True`

### Task 3.4: Add SAVEPOINT support for explicit control

Replace atomic() with explicit SAVEPOINT for finer control:

```python
def _apply_django_db_isolation(marker_args: dict) -> list:
    """Apply database isolation based on marker args. Returns cleanup context."""
    from django.db import connections, transaction

    databases = marker_args.get("databases", list(connections))
    use_transaction = not marker_args.get("transaction", False)

    if not use_transaction:
        return []  # No isolation for transaction=True tests

    savepoints = []
    for alias in databases:
        sid = transaction.savepoint(using=alias)
        savepoints.append((alias, sid))
    return savepoints

def _cleanup_django_db_isolation(savepoints: list):
    """Rollback savepoints after test."""
    from django.db import transaction
    for alias, sid in reversed(savepoints):
        transaction.savepoint_rollback(sid, using=alias)
```

### Task 3.5: Update run_test to use new isolation functions

Refactor the Django isolation section in run_test():

1. Parse marker_info to get django_db settings
2. Call `_apply_django_db_isolation()` before test
3. Call `_cleanup_django_db_isolation()` in finally block
4. Remove old atomic() approach

### Task 3.6: Add marker_info parameter to run_test

Update the run_test function signature to accept marker_info:

```python
def run_test(node_id: str, cached_effects: list | None = None, marker_info: list | None = None):
```

### Task 3.7: Write Django marker test file

Create `tests/django_project/test_django_markers.py`:

```python
"""Tests for @pytest.mark.django_db marker argument support."""
import pytest
from tests.django_project.models import TestUser

@pytest.mark.django_db
def test_default_transaction_rollback():
    """Default: transaction=False, should rollback."""
    TestUser.objects.create(name="RollbackTest")
    assert TestUser.objects.count() == 1

@pytest.mark.django_db(transaction=False)
def test_explicit_transaction_false():
    """Explicit transaction=False, should rollback."""
    TestUser.objects.create(name="ExplicitRollback")
    assert TestUser.objects.count() == 1

# Note: transaction=True tests would need special handling
# and are marked as toxic (deferred to 0.3.0)
```

### Task 3.8: Run tests and commit Batch 3

```bash
# Run Python tests
pytest tests/django_project/ -v

git add src/tach_harness.py tests/django_project/
git commit -m "feat(harness): implement marker-aware Django database isolation"
```

---

## Batch 4: Integration Tests & Documentation

**Goal:** Comprehensive integration tests and documentation updates.

### Files:

- Create: `tests/gauntlet_django/test_marker_isolation.py`
- Create: `tests/gauntlet_django/conftest.py`
- Modify: `docs/research/roadmap.md` (mark tasks complete)
- Modify: `.github/workflows/ci.yml` (add gauntlet_django)

### Task 4.1: Create gauntlet_django test directory

Create test directory structure:

```
tests/gauntlet_django/
├── __init__.py
├── conftest.py
├── test_marker_isolation.py
└── test_parallel_isolation.py
```

### Task 4.2: Create conftest.py for Django setup

```python
"""Django test configuration for gauntlet tests."""
import os
import django
import pytest

def pytest_configure(config):
    os.environ.setdefault("DJANGO_SETTINGS_MODULE", "tests.django_project.settings")
    django.setup()
```

### Task 4.3: Create parallel isolation test

Create `test_parallel_isolation.py` that proves parallel tests are isolated:

- 5+ tests that each create a unique record
- Each test asserts count == 1
- Run with `tach -n 4` to prove parallel isolation

### Task 4.4: Create marker variant tests

Create `test_marker_isolation.py`:

- Test `@pytest.mark.django_db` (bare)
- Test `@pytest.mark.django_db(transaction=False)`
- Test `@pytest.mark.django_db(databases=["default"])`
- Test without marker (should error or skip)

### Task 4.5: Add gauntlet_django to CI

Update `.github/workflows/ci.yml` to include the new test directory:

```yaml
- name: Run Django gauntlet tests
  run: |
    ./target/release/tach-core tests/gauntlet_django/ -v
```

### Task 4.6: Update roadmap.md

Mark completed items in `docs/research/roadmap.md`:

- [x] Marker argument parsing
- [x] Database transaction wrapping
- [x] SAVEPOINT isolation

Update the mermaid diagram to show P2_1 as done.

### Task 4.7: Run full test suite and commit Batch 4

```bash
# Build release
cargo build --release

# Run all tests
cargo test --lib
cargo test --test '*'
pytest tests/gauntlet_django/ -v

# Run tach self-test
./target/release/tach-core self-test

git add tests/gauntlet_django/ docs/research/roadmap.md .github/workflows/ci.yml
git commit -m "test(django): add comprehensive pytest-django marker isolation tests"
```

---

## Verification Checklist

Before marking 0.2.1 complete:

- [ ] `cargo test --lib` passes
- [ ] `cargo test --test '*'` passes
- [ ] `pytest tests/django_project/` passes
- [ ] `pytest tests/gauntlet_django/` passes
- [ ] `./target/release/tach-core tests/gauntlet_django/ -n 4` passes (parallel)
- [ ] No regressions in existing gauntlet tests

---

## Deferred to 0.3.0

The following are explicitly OUT OF SCOPE for 0.2.1:

| Feature                                          | Reason                                   |
| ------------------------------------------------ | ---------------------------------------- |
| `transaction=True` support                       | Requires table truncation, mark as toxic |
| `reset_sequences=True`                           | Requires sequence manipulation           |
| Django fixtures (`client`, `rf`, `admin_client`) | Separate fixture injection work          |
| `@pytest.mark.urls`                              | URL override is separate feature         |
| FD teleportation for connections                 | Complex, 0.3.2 scope                     |
| `live_server` fixture                            | Requires subprocess management           |

---

## Success Criteria

1. **Marker Parsing**: `@pytest.mark.django_db(transaction=False, databases=["default"])` correctly parsed
2. **Isolation**: Each test sees isolated database state (count == 1 pattern passes)
3. **Parallel Safety**: 4+ workers running Django tests don't cross-contaminate
4. **Backward Compatible**: Existing Django tests continue to work
5. **No Performance Regression**: SAVEPOINT approach is faster than current atomic() usage
