# Test Suite Verification and .ignore Documentation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Run full test suite to get final pass/fail numbers after fixing `.ignore`, then document the `.ignore` file interaction in troubleshooting docs.

**Architecture:** Two independent tasks - one for testing, one for documentation. Testing runs in Docker container. Documentation adds a new subsection to existing troubleshooting.md.

**Tech Stack:** Docker, Cargo, pytest, Markdown

---

## Task 1: Run Full Test Suite in Docker

**Files:**

- None modified (read-only verification)

**Step 1: Run self-test to verify kernel features**

Run:

```bash
docker-compose exec dev bash -c 'cd /workspace && ./target/release/tach-core self-test'
```

Expected: 8/8 PASS for all kernel features

**Step 2: Run Rust unit tests**

Run:

```bash
docker-compose exec dev bash -c 'cd /workspace && cargo test --lib 2>&1 | tail -5'
```

Expected: ~695 tests passed, 0 failed

**Step 3: Run Rust integration tests**

Run:

```bash
docker-compose exec dev bash -c 'cd /workspace && cargo test --test "*" 2>&1 | grep -E "^test result:" | head -20'
```

Expected: Multiple test suites, all passing (including discovery_integration: 6 passed)

**Step 4: Run Python gauntlet tests via pytest**

Run:

```bash
docker-compose exec dev bash -c 'cd /workspace && source .venv/bin/activate && pytest tests/gauntlet/ -v 2>&1 | tail -20'
```

Expected: 26 passed, 5 failed (fs_destruction tests - expected in privileged container)

**Step 5: Record final test counts**

Compile results into a summary table showing before/after fix numbers.

---

## Task 2: Document .ignore File Interaction

**Files:**

- Modify: `docs/troubleshooting.md` (after line 306, in "Test Discovery Issues" section)

**Step 1: Add new subsection for .ignore file issue**

Add the following after the "### Tests Not Found" section (after line 306):

```markdown
### .ignore File Blocking Python Files

**Symptom:**
```

Discovered 0 tests, 0 fixtures

```

Discovery reports zero tests even though test files exist and have valid syntax.

**Cause:**

The `.ignore` file (used by tools like Claude Code for context filtering) contains a pattern that blocks Python files:

```

\*.py

````

Tach uses the `ignore` crate for file discovery, which respects `.ignore` files. This pattern causes ALL Python files to be skipped during test discovery.

**Diagnosis:**

```bash
# Check if .ignore contains *.py
grep '^\*\.py$' .ignore && echo "FOUND: *.py in .ignore is blocking discovery"

# Verify files exist but are being ignored
ls tests/**/*.py  # Files exist
tach-core list .  # But discovery finds nothing
````

**Solution:**

Remove `*.py` from `.ignore`:

```bash
sed -i '/^\*\.py$/d' .ignore
```

Or edit `.ignore` manually and remove the `*.py` line.

**Prevention:**

If you need to exclude Python files from Claude Code's context but not from tach-core:

- Use `.claudeignore` instead (if supported)
- Or add patterns that are more specific (e.g., `src/**/*.py` instead of `*.py`)

> **Note:** The `.ignore` file format is shared between multiple tools. Patterns added for one tool may affect others that use the `ignore` crate.

````

**Step 2: Verify the edit**

Run:
```bash
grep -A 5 "### .ignore File Blocking" docs/troubleshooting.md
````

Expected: Shows the new section header and first few lines

**Step 3: Commit the documentation**

```bash
git add docs/troubleshooting.md
git commit -m "docs: add .ignore file troubleshooting for test discovery

Documents that *.py patterns in .ignore can block tach-core's
test discovery since the ignore crate respects these files.

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Completion Checklist

- [ ] Task 1: Full test suite run with final numbers recorded
- [ ] Task 2: .ignore documentation added to troubleshooting.md
- [ ] Task 2: Changes committed
