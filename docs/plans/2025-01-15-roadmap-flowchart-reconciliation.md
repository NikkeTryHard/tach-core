# Roadmap Flowchart Reconciliation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Fix the roadmap flowchart to accurately reflect true technical dependencies between phases and tasks, enabling parallel development tracks.

**Architecture:** Replace the incorrect sequential phase dependencies with a multi-track dependency graph that shows: (1) which items are already done, (2) which can start immediately, (3) true sequential dependencies, and (4) cross-phase dependencies.

**Tech Stack:** Mermaid flowchart syntax, Markdown

---

## Summary of Changes

| Issue                | Current State                                                      | Correct State                                                       |
| -------------------- | ------------------------------------------------------------------ | ------------------------------------------------------------------- |
| Phase 2 structure    | Sequential: Wrapper Hooks → Effect System → Plugin Shims → plugins | Parallel: pytest-django, pytest-asyncio, pytest-mock all from 0.2.0 |
| Phase 3 structure    | Sequential: Django → SQLAlchemy → Connection → etc                 | Parallel: Django DB and SQLAlchemy can run together                 |
| Phase 4 structure    | All sequential                                                     | 0.4.3 Autouse and 0.4.4 Parametrized are independent                |
| Phase 5-9            | Blocked by prior phases                                            | Many items can start NOW (only need 0.1.x)                          |
| Done items           | Styled as pending                                                  | 0.5.0, 0.5.6, 0.7.4, 0.8.1 should be in done class                  |
| Inter-phase deps     | Phase → Phase (sequential)                                         | Task → Task (accurate dependencies)                                 |
| 0.2.x mini-flowchart | Conflicts with main flowchart                                      | Remove or reconcile                                                 |

---

## Batch 1: Fix Main Flowchart Phase 2 (Plugin Compatibility)

**Files:**

- Modify: `docs/research/roadmap.md` (lines 55-83)

### Task 1.1: Update Phase 2 node labels to match section headers

**What to change:**

- P2_1: "0.2.1 Wrapper Hooks" → "0.2.1 pytest-django"
- P2_2: "0.2.2 Effect System" → "0.2.2 pytest-asyncio"
- P2_3: "0.2.3 Plugin Shims" → "0.2.3 pytest-mock/env/timeout"
- Remove P2_31, P2_32, P2_33 (consolidated into P2_1, P2_2, P2_3)
- P2_34: "0.2.3.4 Landlock V4" → "0.2.4 Landlock V4-V6" (consolidate Landlock versions)
- Remove P2_35, P2_36 (consolidated)
- P2_4: "0.2.4 Plugin Detection" → "0.2.5 Plugin Stabilization"
- Remove P2_5 (consolidated into P2_4)

### Task 1.2: Update Phase 2 dependency arrows

**What to change:**

- Remove: `P2_0 --> P2_1 --> P2_2 --> P2_3` (sequential)
- Add: `P2_0 --> P2_1`, `P2_0 --> P2_2`, `P2_0 --> P2_3`, `P2_0 --> P2_4` (parallel from 0.2.0)
- Add: `P2_1 --> P2_5`, `P2_2 --> P2_5`, `P2_3 --> P2_5` (all plugins feed into stabilization)
- Keep P2_4 (Landlock) as independent track with dotted line to P2_5

### Task 1.3: Verify Phase 2 changes render correctly

**Run:** Open roadmap.md in VS Code or GitHub to verify Mermaid renders

---

## Batch 2: Fix Main Flowchart Phase 3-4 (Database & Fixtures)

**Files:**

- Modify: `docs/research/roadmap.md` (lines 85-115)

### Task 2.1: Update Phase 3 to show parallel Django/SQLAlchemy

**What to change:**

- Keep: P3_0 "Django DB", P3_1 "SQLAlchemy"
- Change arrows: Remove `P3_0 --> P3_1` (they're parallel)
- Add: Both P3_0 and P3_1 feed into P3_2 (Connection Mgmt)
- Consolidate P3_3 and P3_4 into P3_3 "Additional DBs" (one item for all extra DBs)

### Task 2.2: Update Phase 4 to show independent items

**What to change:**

- Keep sequential: P4_0 → P4_1 → P4_2 (fixture scopes build on each other)
- Make independent: P4_3 (Autouse) and P4_4 (Parametrized) - no arrows TO them from P4_2
- Add: P4_3 and P4_4 feed INTO P4_5 (Zygote Warmup)
- Keep: P4_5 → P4_6

### Task 2.3: Verify Phase 3-4 changes render correctly

**Run:** Open roadmap.md in VS Code or GitHub to verify Mermaid renders

---

## Batch 3: Fix Main Flowchart Phase 5-8 (DX, Config, Perf, CI)

**Files:**

- Modify: `docs/research/roadmap.md` (lines 117-191)

### Task 3.1: Update Phase 5 to show parallel items and done status

**What to change:**

- P5_0 and P5_6 are already done - move to done styling class
- Make parallel from 0.1.x: P5_1, P5_3, P5_5 (Debug, Watch, Log)
- Keep sequential: P5_1 → P5_2 (Debug → Interactive Debug)
- Keep sequential: P5_3 → P5_4 (Watch → Smart Watch)

### Task 3.2: Update Phase 6 to show parallel items

**What to change:**

- P6_0 depends on 0.1.x only (can start now)
- P6_2 (Toxicity Config) depends on 0.1.x only (existing feature)
- P6_3 (Plugin Config) depends on 0.2.0 (plugin registry)
- Make P6_1, P6_2, P6_3 parallel after P6_0

### Task 3.3: Update Phase 7 to show parallel items and done status

**What to change:**

- P7_4 is already done - move to done styling class
- Make parallel from 0.1.x: P7_0, P7_1, P7_6 (History, Memory, Lazy)
- Keep sequential: P7_1 → P7_2 → P7_3 → P7_7 (UFFD chain)
- Keep sequential: P7_0 → P7_5 (History → Adaptive)
- Keep sequential: P7_1 → P7_8 → P7_9 (Fork tracking chain)

### Task 3.4: Update Phase 8 to show done status and split experimental

**What to change:**

- P8_1 is already done - move to done styling class
- Make parallel: P8_0, P8_2, P8_3 (CI features)
- Split P8_4-P8_6 (Sub-Interpreters) as separate experimental track

### Task 3.5: Verify Phase 5-8 changes render correctly

**Run:** Open roadmap.md in VS Code or GitHub to verify Mermaid renders

---

## Batch 4: Fix Inter-Phase Dependencies

**Files:**

- Modify: `docs/research/roadmap.md` (lines 247-268)

### Task 4.1: Remove incorrect sequential phase dependencies

**What to change:**

- Remove: `Phase1 --> Phase2 --> Phase3 --> ... --> Phase10 --> Future`
- These imply all of Phase N must complete before Phase N+1 starts

### Task 4.2: Add accurate cross-phase task dependencies

**What to add (replace the phase-to-phase arrows):**

```mermaid
%% Foundation feeds everything
Phase1 --> Phase2
Phase1 --> Phase5
Phase1 --> Phase6
Phase1 --> Phase7
Phase1 --> Phase8
Phase1 --> Phase9

%% True cross-phase task dependencies
P2_1 -.-> P3_0
P3_0 -.-> P4_0
P3_1 -.-> P4_0
P4_2 -.-> P4_5
P7_8 -.-> P9_0
P5_6 -.-> P8_3
```

### Task 4.3: Update existing cross-phase dependency comments

**What to change:**

- Keep: `P2_31 -.->|"Django fixtures"| P3_0` but rename P2_31 to P2_1
- Keep: `P3_0 -.->|"DB transactions"| P4_0`
- Keep: `P7_8 -.->|"fork tracking"| P9_0`
- Keep: `P5_6 -.->|"PEP 669 coverage"| P8_3`
- Remove: `P4_6 -.->|"fixture viz"| P5_1` (weak dependency)
- Remove: References to deleted nodes (P2_31, P2_34, P7_9, P8_6)

### Task 4.4: Verify inter-phase changes render correctly

**Run:** Open roadmap.md in VS Code or GitHub to verify Mermaid renders

---

## Batch 5: Fix Styling Classes

**Files:**

- Modify: `docs/research/roadmap.md` (lines 270-292)

### Task 5.1: Add "canStart" styling class

**What to add:**

```mermaid
classDef canStart fill:#3b82f6,stroke:#1d4ed8,color:#fff
```

### Task 5.2: Update node class assignments

**What to change:**

```mermaid
%% Already done
class P1_1,P1_2,P1_3,P1_4,P1_5 done
class P2_0,P5_0,P5_6,P7_4,P8_1 done

%% In progress
class P2_1 inProgress

%% Can start immediately (only need 0.1.x or 0.2.0)
class P5_1,P5_3,P5_5,P6_0,P6_2,P7_0,P7_1,P7_6,P8_0,P9_2,P9_5,P9_6,P9_7 canStart

%% Pending (has real blockers)
class P2_2,P2_3,P2_4,P2_5 pending
class P3_0,P3_1,P3_2,P3_3 pending
class P4_0,P4_1,P4_2,P4_3,P4_4,P4_5,P4_6 pending
class P5_2,P5_4 pending
class P6_1,P6_3,P6_4,P6_5 pending
class P7_2,P7_3,P7_5,P7_7,P7_8,P7_9 pending
class P8_2,P8_3,P8_4,P8_5,P8_6 pending
class P9_0,P9_1,P9_3,P9_4,P9_8 pending
class P10_0,P10_1,P10_2,P10_3,P10_4 pending
class P10_5 milestone
```

### Task 5.3: Update legend

**What to change:**

- Update: `**Legend:** 🟢 Done | 🟠 In Progress | 🔵 Can Start Now | ⚪ Pending | 🔷 Milestone`

### Task 5.4: Verify styling changes render correctly

**Run:** Open roadmap.md in VS Code or GitHub to verify Mermaid renders

---

## Batch 6: Remove/Reconcile 0.2.x Mini-Flowchart

**Files:**

- Modify: `docs/research/roadmap.md` (lines 511-553)

### Task 6.1: Remove the conflicting 0.2.x Development Flow section

**What to change:**

- Delete the entire "### 0.2.x Development Flow" section (lines 511-553)
- OR update it to match the corrected main flowchart

**Rationale:** This mini-flowchart shows a different structure than the main flowchart. Having two conflicting diagrams causes confusion. The main flowchart should be the single source of truth.

### Task 6.2: Add a note in 0.2.x section referencing main flowchart

**What to add (after line 509):**

```markdown
> **Development Flow:** See the main flowchart at the top of this document for task dependencies. Items 0.2.1-0.2.3 can be developed in parallel after 0.2.0 is complete.
```

### Task 6.3: Verify section changes are consistent

**Run:** Search for any remaining references to deleted node names (P2_31, P2_32, etc.)

---

## Batch 7: Update Current Status Section

**Files:**

- Modify: `docs/research/roadmap.md` (lines 295-301)

### Task 7.1: Update the Current Status text

**What to change:**

```markdown
**Current Status:**

- Phase 1 (0.1.x): Complete
- Phase 2 (0.2.x): In Progress - 0.2.0 done, 0.2.1 in progress, 0.2.2-0.2.4 can start in parallel
- Phases 3-4: Blocked by Phase 2 plugin work
- Phases 5-9: **Many items can start NOW** - see blue "Can Start" nodes in flowchart
- Phase 10: Not started
```

### Task 7.2: Commit all changes

**Run:**

```bash
git add docs/research/roadmap.md
git commit -m "docs: reconcile roadmap flowchart with true technical dependencies

- Fix Phase 2: Show plugins as parallel, not sequential through Wrapper Hooks
- Fix Phase 3: Show Django/SQLAlchemy as parallel
- Fix Phase 4: Show Autouse/Parametrized as independent
- Fix Phase 5-8: Add canStart class for items that can begin now
- Fix inter-phase deps: Replace phase→phase with task→task
- Remove conflicting 0.2.x mini-flowchart
- Update styling classes for done items (0.5.0, 0.5.6, 0.7.4, 0.8.1)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Verification Checklist

After all batches are complete, verify:

- [ ] Main flowchart renders without errors
- [ ] All done items (P2_0, P5_0, P5_6, P7_4, P8_1) show green
- [ ] P2_1 shows orange (in progress)
- [ ] "Can Start" items show blue
- [ ] No orphaned nodes (nodes with no connections)
- [ ] No references to deleted nodes in cross-phase dependencies
- [ ] 0.2.x mini-flowchart is removed or reconciled
- [ ] Legend matches the styling classes used

---

## Appendix: Corrected Phase 2 Mermaid Code

For reference, here is the corrected Phase 2 section:

```mermaid
subgraph Phase2["Phase 2: Plugin Compatibility 🔨 IN PROGRESS"]
    direction TB
    P2_0["0.2.0 Hook Framework ✅"]
    P2_1["0.2.1 pytest-django"]
    P2_2["0.2.2 pytest-asyncio"]
    P2_3["0.2.3 pytest-mock/env/timeout"]
    P2_4["0.2.4 Landlock V4-V6"]
    P2_5["0.2.5 Plugin Stabilization"]

    P2_0 --> P2_1
    P2_0 --> P2_2
    P2_0 --> P2_3
    P2_0 --> P2_4
    P2_1 --> P2_5
    P2_2 --> P2_5
    P2_3 --> P2_5
    P2_4 -.->|optional| P2_5
end
```
