# Final Cleanup Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Fix all remaining code review issues (1 important + 3 minor).

**Architecture:** Quick cleanup fixes with no functional changes.

**Tech Stack:** Python, TOML

---

## Task 1: Remove Unused tempfile Import

**Files:**

- Modify: `tests/gauntlet_fixtures/conftest.py`

**Step 1: Remove the import**

Remove line 3:

```python
import tempfile
```

**Step 2: Verify syntax**

Run: `python3 -c "import tests.gauntlet_fixtures.conftest"`

**Step 3: Commit**

```bash
git add tests/gauntlet_fixtures/conftest.py
git commit -m "chore: remove unused tempfile import"
```

---

## Task 2: Fix pytest asyncio_mode Warning

**Files:**

- Modify: `pyproject.toml`

**Step 1: Remove asyncio_mode option**

Remove line 38:

```toml
asyncio_mode = "auto"
```

This option is only recognized by pytest-asyncio plugin, which is not consistently installed.

**Step 2: Verify syntax**

Run: `python3 -c "import tomllib; tomllib.load(open('pyproject.toml', 'rb'))"`

**Step 3: Commit**

```bash
git add pyproject.toml
git commit -m "chore: remove asyncio_mode to fix pytest warning"
```

---

## Task 3: Add Proper Type Hint to \_load_hook_function

**Files:**

- Modify: `src/tach_harness.py`

**Step 1: Update return type hint**

Change:

```python
) -> tuple:
```

To:

```python
) -> tuple[object | None, str | None]:
```

**Step 2: Commit**

```bash
git add src/tach_harness.py
git commit -m "chore: add proper type hint to _load_hook_function"
```

---

## Task 4: Fix CRLF Line Endings

**Files:**

- Modify: `src/tach_harness.py`

**Step 1: Convert CRLF to LF**

Run:

```bash
sed -i 's/\r$//' src/tach_harness.py
```

**Step 2: Verify**

Run: `file src/tach_harness.py`
Expected: Should NOT contain "CRLF"

**Step 3: Commit**

```bash
git add src/tach_harness.py
git commit -m "chore: convert CRLF to LF line endings"
```

---

## Summary

| Task | Issue                | Fix                                      |
| ---- | -------------------- | ---------------------------------------- |
| 1    | Unused import        | Remove `import tempfile`                 |
| 2    | asyncio_mode warning | Remove config option                     |
| 3    | Vague type hint      | Add `tuple[object \| None, str \| None]` |
| 4    | CRLF endings         | Convert to LF                            |

**Estimated Time:** 10 minutes
**Commits:** 4
