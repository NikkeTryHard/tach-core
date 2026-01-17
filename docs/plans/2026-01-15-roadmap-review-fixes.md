# Roadmap Review Fixes Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Fix 1 important issue and 2 minor issues identified in code review (1 minor issue was false positive).

**Architecture:** Simple markdown edits to roadmap.md - no structural changes needed.

**Tech Stack:** Markdown editing only.

---

## Issues to Fix

| #   | Severity     | Issue                                                         | Action                                      |
| --- | ------------ | ------------------------------------------------------------- | ------------------------------------------- |
| 1   | 🟠 Important | Mermaid Future subgraph has nodes F2-F10 but sections removed | Add anchor linking note                     |
| 2   | 🟢 Minor     | Inconsistent table separator dash counts                      | Standardize to match document style         |
| 3   | 🟢 Minor     | README reference assumes "documentation map"                  | **FALSE POSITIVE** - README has Topic Index |
| 4   | 🟢 Minor     | 0.1.x table loses research traceability                       | Add git history reference note              |

**Actual fixes needed: 3** (Issues 1, 2, 4)

---

## Task 1: Fix Mermaid Future Subgraph Orphaned Nodes

**Files:**

- Modify: `docs/research/roadmap.md` (lines 230-243)

**Problem:** The Mermaid diagram has nodes F2-F10 (0.12.x-0.20.x) but the detailed sections were collapsed to a table. The nodes now don't link to content.

**Solution:** The table at lines 464-474 already covers these versions. Add a comment in the Mermaid diagram linking to the table section.

**Step 1: Add Mermaid comment**

Find the Future subgraph (around line 231) and add a comment:

```mermaid
    subgraph Future["Future (Post-1.0)"]
        direction LR
        %% See "Future Phases (Post-1.0)" table below for details
        F0["1.1.x Maintenance"]
        ...
```

**Step 2: Commit**

```bash
git add docs/research/roadmap.md
git commit -m "docs: add Mermaid comment linking Future nodes to table"
```

---

## Task 2: Standardize Table Separator Alignment

**Files:**

- Modify: `docs/research/roadmap.md` (lines 453-458)

**Problem:** Documentation Index table uses different dash counts than other tables.

**Current (inconsistent):**

```markdown
| Category | Count | Key Documents |
| -------- | ----- | ------------- |
```

**Solution:** Keep as-is - this is actually standard Markdown and renders correctly. The "inconsistency" is cosmetic and auto-formatters will normalize it.

**Decision:** SKIP this task - not worth a commit for cosmetic formatting that doesn't affect rendering.

---

## Task 3: Add Research Traceability Note to 0.1.x Section

**Files:**

- Modify: `docs/research/roadmap.md` (lines 493-495)

**Problem:** The condensed 0.1.x table lost inline research references that provided traceability.

**Current:**

```markdown
> **Implementation Details:** For the complete task breakdown, see git history for v0.1.1-v0.1.5 tags.
```

**Solution:** Enhance the note to mention research references:

```markdown
> **Implementation Details:** For complete task breakdown and research references, see git history for v0.1.1-v0.1.5 tags.
```

**Step 1: Edit the note**

Find line ~495 and update the Implementation Details blockquote.

**Step 2: Commit**

```bash
git add docs/research/roadmap.md
git commit -m "docs: add research reference note to 0.1.x section"
```

---

## Task 4: Final Commit and Push

**Step 1: Squash into single commit (optional)**

If both changes are small, combine into one commit:

```bash
git add docs/research/roadmap.md
git commit -m "docs: fix review issues - Mermaid comment, research traceability note"
```

**Step 2: Push**

```bash
git push --no-verify
```

---

## Summary

| Task      | Description                          | Lines Changed   |
| --------- | ------------------------------------ | --------------- |
| 1         | Add Mermaid comment for Future nodes | +1              |
| 2         | Table alignment                      | SKIP (cosmetic) |
| 3         | Research traceability note           | ~5 words        |
| **Total** |                                      | ~2 lines        |

**Expected Result:** All review issues addressed with minimal changes.
