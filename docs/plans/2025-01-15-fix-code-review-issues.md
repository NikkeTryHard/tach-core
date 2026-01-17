# Fix Code Review Issues Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Fix all 10 issues identified by code review agents (0 critical, 6 important, 4 minor).

**Architecture:** Make targeted edits to docs/research/roadmap.md to fix styling, text consistency, and section header alignment.

**Tech Stack:** Mermaid flowchart syntax, Markdown

---

## Issue Summary

| #   | Severity  | Issue                                            | Fix                                       |
| --- | --------- | ------------------------------------------------ | ----------------------------------------- |
| 1   | Important | Plan file not in worktree                        | Copy plan file to worktree                |
| 2   | Important | Orphaned nodes Phase 5 (P5_0, P5_5, P5_6)        | Intentional - add comment explaining      |
| 3   | Important | Orphaned node Phase 6 (P6_4)                     | Add P6_0 --> P6_4 arrow                   |
| 4   | Important | Orphaned nodes Phase 7 (P7_4, P7_6)              | Intentional - add comment explaining      |
| 5   | Important | Legend color conflict (canStart = milestone)     | Change milestone to purple (#8b5cf6)      |
| 6   | Important | Status says "can start" but nodes styled pending | Change P2_2, P2_3, P2_4 to canStart class |
| 7   | Minor     | Node ID consolidation noted                      | No fix needed - acknowledged              |
| 8   | Minor     | Milestone vs canStart only stroke width          | Fixed by #5 (color change)                |
| 9   | Minor     | Version numbering inconsistent                   | Update section headers to match flowchart |
| 10  | Minor     | Legend emoji don't appear on nodes               | No fix needed - expected behavior         |

---

## Batch 1: Fix Plan File and Add Comments for Orphaned Nodes

### Task 1.1: Copy plan file to worktree

Copy `docs/plans/2025-01-15-roadmap-flowchart-reconciliation.md` from main repo to worktree.

### Task 1.2: Add comment explaining intentionally orphaned "done" nodes

In the flowchart, before the styling section, add a comment:

```mermaid
%% Note: Orphaned nodes (P5_0, P5_6, P7_4, P8_1) are intentionally disconnected
%% They represent completed work with no remaining dependencies
```

---

## Batch 2: Fix Orphaned P6_4 and Color Conflict

### Task 2.1: Add arrow from P6_0 to P6_4

In Phase 6 arrows section, add: `P6_0 --> P6_4`

This makes P6_4 (Scheduler Persistence) depend on P6_0 (pyproject.toml Schema), which is logical.

### Task 2.2: Change milestone color from blue to purple

Change line 248 from:

```
classDef milestone fill:#3b82f6,stroke:#1d4ed8,color:#fff,stroke-width:2px
```

To:

```
classDef milestone fill:#8b5cf6,stroke:#7c3aed,color:#fff,stroke-width:2px
```

### Task 2.3: Update legend emoji for milestone

Change line 267 from:

```
**Legend:** 🟢 Done | 🟠 In Progress | 🔵 Can Start Now | ⚪ Pending | 🔷 Milestone
```

To:

```
**Legend:** 🟢 Done | 🟠 In Progress | 🔵 Can Start Now | ⚪ Pending | 🟣 Milestone
```

---

## Batch 3: Fix Status Text and Node Styling Consistency

### Task 3.1: Change P2_2, P2_3, P2_4 from pending to canStart

These items CAN start in parallel after P2_0 (which is done). Update line 254:

From:

```
class P2_2,P2_3,P2_4,P2_5 pending
```

To:

```
class P2_2,P2_3,P2_4 canStart
class P2_5 pending
```

P2_5 (Plugin Stabilization) stays pending because it depends on P2_1-P2_4 completing.

---

## Batch 4: Fix Section Header Version Numbering

### Task 4.1: Update Landlock section header

Change line 670 from:

```
### 0.2.3.1 - Landlock V4 Network Isolation (Kernel 6.7+)
```

To:

```
### 0.2.4 - Landlock V4-V6 Network Isolation (Kernel 6.7+)
```

### Task 4.2: Update Plugin Stabilization section header

Change line 696 from:

```
### 0.2.4 - Plugin Testing and Stabilization
```

To:

```
### 0.2.5 - Plugin Testing and Stabilization
```

---

## Batch 5: Commit All Fixes

### Task 5.1: Verify Mermaid renders correctly

Open roadmap.md and verify the flowchart renders without errors.

### Task 5.2: Commit the fixes

```bash
git add docs/research/roadmap.md docs/plans/
git commit -m "fix: address code review issues in roadmap flowchart

- Add comment explaining intentionally orphaned done nodes
- Add P6_0 --> P6_4 arrow (Scheduler depends on Schema)
- Change milestone color from blue to purple (distinguish from canStart)
- Change P2_2, P2_3, P2_4 from pending to canStart (matches status text)
- Update section headers: 0.2.3.1 -> 0.2.4, 0.2.4 -> 0.2.5

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Verification Checklist

- [ ] Plan file exists in worktree docs/plans/
- [ ] Orphaned done nodes have explanatory comment
- [ ] P6_4 has incoming arrow from P6_0
- [ ] Milestone class uses purple (#8b5cf6), not blue
- [ ] Legend shows 🟣 for Milestone
- [ ] P2_2, P2_3, P2_4 styled as canStart (blue)
- [ ] Section header 0.2.3.1 renamed to 0.2.4
- [ ] Section header 0.2.4 renamed to 0.2.5
- [ ] Flowchart renders without errors
