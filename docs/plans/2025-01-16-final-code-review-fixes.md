# Plan: Final Code Review Fixes

**Date:** 2025-01-16
**Branch:** master (direct commits - documentation/CI only)

---

## Background

The comprehensive code review of v0.2.0 Hook Interception Framework identified 1 Important issue and 2 Suggestions remaining after the roadmap status update.

---

## Important Issues

### Issue 1: Roadmap Completed Items List Incomplete

**Location:** `docs/research/roadmap.md`

**Problem:** While the status was updated to ~75%, the completed items list doesn't fully reflect all implemented features. The current list is missing several items that were implemented.

**Current Completed List:**

- Hook registry types with Serde
- 10 builtin hook specs
- hook detection in conftest.py
- marker extraction from decorators (with JSON output)
- autouse fixture detection
- path canonicalization for hook matching
- SysPathAction enum (type-safe)
- session effects IPC bridge
- debug logging for effect application
- pytest_sessionstart in SESSION_HOOKS
- HookEffect enum with all variants
- toxicity integration for global-state-modifying hooks

**Recommended Completed List (add missing items):**

- Hook registry types with Serde
- 10 builtin hook specs
- hook detection in conftest.py
- marker extraction from decorators (with JSON output)
- autouse fixture detection
- path canonicalization for hook matching
- SysPathAction enum (type-safe)
- session effects IPC bridge (Zygote → Supervisor → Workers)
- debug logging for effect application
- pytest_sessionstart in SESSION_HOOKS
- HookEffect enum with all variants
- toxicity integration for global-state-modifying hooks
- conftest inheritance resolution
- effect recording for pytest_configure/sessionstart
- effect replay in workers
- IPC protocol extension
- plugin detection and warning system

**Remaining Items (update):**

- Hook execution (caller, aggregation, wrappers)
- Hook dependency graph

---

## Suggestions (Nice to Have)

### Suggestion 1: Add CI Workflow Entry for gauntlet_hook_effects

**Location:** `.github/workflows/ci.yml`

**Investigation:** Check if `tests/gauntlet_hook_effects/` exists and if it's already in CI.

**Action:** If the directory exists and isn't in CI, add it to the workflow.

### Suggestion 2: Add Integration Test for Zygote-to-Supervisor Effect IPC

**Location:** `rust_tests/` or `tests/`

**Problem:** Current tests verify Python-side effect functions and Rust-side registry separately, but no integration test verifies the full IPC path.

**Action:** Create an integration test that verifies:

1. Zygote sends effects via bincode IPC
2. Supervisor receives and decodes effects
3. Effects are stored in HookRegistry
4. Workers receive and apply effects

---

## Tasks

### Task 1: Update Roadmap Completed Items List

**Goal:** Make the completed items list comprehensive and accurate.

**Implementation:**

1. Update `docs/research/roadmap.md` line 409 with the full list of completed items
2. Update the remaining items to just: Hook execution (caller, aggregation, wrappers), Hook dependency graph

### Task 2: Check and Add CI Workflow Entry

**Goal:** Ensure new test directory is in CI if it exists.

**Investigation:**

1. Check if `tests/gauntlet_hook_effects/` directory exists
2. Check if it's already in `.github/workflows/ci.yml`

**Implementation:**

- If directory exists and not in CI: Add to workflow
- If directory doesn't exist: Skip (no action needed)

### Task 3: Add Effect IPC Integration Test

**Goal:** Create integration test for the full effect IPC path.

**Implementation:**

1. Create `rust_tests/effect_ipc_integration.rs` or add to existing integration test file
2. Test should:
   - Set up a mock Zygote that sends effects
   - Verify Supervisor receives and stores effects
   - Verify effects are available for workers
3. Run tests to verify

---

## Success Criteria

- [ ] Roadmap completed items list is comprehensive
- [ ] Roadmap remaining items are accurate (just 2 items)
- [ ] CI workflow includes gauntlet_hook_effects (if directory exists)
- [ ] Integration test for effect IPC exists and passes
- [ ] All tests pass

---

## Execution Order

1. Task 1: Update roadmap (Important issue)
2. Task 2: Check/add CI entry (Suggestion 1)
3. Task 3: Add integration test (Suggestion 2)

---
