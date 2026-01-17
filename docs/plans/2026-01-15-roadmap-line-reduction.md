# Roadmap Line Reduction Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Reduce roadmap.md from 2,388 lines to ~1,800 lines by replacing duplicated content with references to existing documentation.

**Architecture:** Replace inline tables/quotes with doc references, condense completed sections to summaries, collapse future phases to table format.

**Tech Stack:** Markdown editing only - no code changes required.

---

## Pre-Implementation Checklist

- [ ] Current line count: 2,388 lines
- [ ] Target line count: ~1,800 lines (~25% reduction)
- [ ] Backup not needed (git tracked)

---

## Task 1: Replace Container Compatibility Table with Reference

**Files:**

- Modify: `docs/research/roadmap.md` (lines 333-351)

**Step 1: Locate the duplicate table**

Find lines 333-351 containing the Container Compatibility table:

```markdown
### Container Compatibility

Tach uses kernel features that require specific container configurations:

| Container Mode | Landlock | Seccomp | userfaultfd | Recommendation |
...
```

**Step 2: Replace with reference**

Replace the entire table (18 lines) with a 3-line reference:

```markdown
### Container Compatibility

> **Full Matrix:** See [container-compatibility.md](container-compatibility.md) for Docker, Podman, and Kubernetes configurations with capability requirements.
```

**Step 3: Verify link works**

Confirm `container-compatibility.md` exists and contains the full matrix.

**Step 4: Commit**

```bash
git add docs/research/roadmap.md
git commit -m "docs: replace container compatibility table with reference"
```

**Lines saved:** ~15

---

## Task 2: Replace Python Version Compatibility Table with Reference

**Files:**

- Modify: `docs/research/roadmap.md` (lines 353-367)

**Step 1: Locate the duplicate table**

Find lines 353-367 containing Python Version Compatibility:

```markdown
### Python Version Compatibility

| Python Version | Default Allocator | Fork Safety | multiprocessing Default | Tach Support |
...
```

**Step 2: Replace with reference**

Replace with 3-line reference:

```markdown
### Python Version Compatibility

> **Full Matrix:** See [../python-compatibility.md](../python-compatibility.md) for Python 3.10-3.14 support, PyPy status, and free-threading implications.
```

**Step 3: Commit**

```bash
git add docs/research/roadmap.md
git commit -m "docs: replace python compatibility table with reference"
```

**Lines saved:** ~12

---

## Task 3: Replace Kernel Version Requirements Table with Reference

**Files:**

- Modify: `docs/research/roadmap.md` (lines 369-387)

**Step 1: Locate the duplicate table**

Find lines 369-387 containing Kernel Version Requirements.

**Step 2: Replace with reference**

Replace with 3-line reference:

```markdown
### Kernel Version Requirements

> **Full Matrix:** See [isolation-landlock.md](isolation-landlock.md) for Landlock ABI V1-V6 requirements and [isolation-userfaultfd.md](isolation-userfaultfd.md) for userfaultfd kernel requirements.
```

**Step 3: Commit**

```bash
git add docs/research/roadmap.md
git commit -m "docs: replace kernel requirements table with reference"
```

**Lines saved:** ~15

---

## Task 4: Condense 0.1.x Completed Sections

**Files:**

- Modify: `docs/research/roadmap.md` (lines 554-785)

**Step 1: Identify the 0.1.x section**

Lines 554-785 contain detailed task lists for 0.1.1 through 0.1.5, all marked complete with `[x]`.

**Step 2: Replace with condensed summary**

Replace the entire 230-line section with ~25 lines:

```markdown
## 0.1.x - Foundation (Complete)

> **Status:** All 5 milestones delivered. See [CHANGELOG.md](../../CHANGELOG.md) for release details.
>
> **Research Foundation:** Implements the "Kineton" engine from _Python Testing Engine Rust Breakthroughs_.

### Delivered Features

| Version | Focus              | Key Deliverables                                                             |
| ------- | ------------------ | ---------------------------------------------------------------------------- |
| 0.1.1   | Docs & Polish      | Examples directory, quickstart guide, shell completions, `--dry-run`         |
| 0.1.2   | Test Compatibility | `pytest.raises/warns/approx`, traceback formatting, timeout handling         |
| 0.1.3   | Error Handling     | Error categorization (E001-E020), `--diagnose` flag, remediation suggestions |
| 0.1.4   | Dependencies       | PyO3 0.27.2, Rust 2024 Edition, Python 3.14 support                          |
| 0.1.5   | Tooling Research   | `.ignore` conflicts, container compatibility, test discovery analysis        |

> **Implementation Details:** For the complete task breakdown, see git history for v0.1.1-v0.1.5 tags.
```

**Step 3: Commit**

```bash
git add docs/research/roadmap.md
git commit -m "docs: condense completed 0.1.x section to summary table"
```

**Lines saved:** ~205

---

## Task 5: Consolidate Inline Research Quotes

**Files:**

- Modify: `docs/research/roadmap.md` (throughout)

**Step 1: Identify repeated quote patterns**

There are 64 inline `> **Ref**: "quote..."` blocks. Group by source:

- Fork Safety quotes (~8 occurrences) → reference `topic-archive.md`
- userfaultfd quotes (~5 occurrences) → reference `isolation-userfaultfd.md`
- Compatibility Layer quotes (~6 occurrences) → reference `hooks-deep-dive.md`
- Zygote Tree quotes (~7 occurrences) → reference `execution-deep-dive.md`

**Step 2: Add consolidated reference blocks at section headers**

For each major section (0.2.x, 0.3.x, etc.), the header already has a Research Foundation block. Keep those and remove the duplicate inline quotes within the section.

Example - in 0.3.x Database Integration, the header says:

```markdown
> **Research Foundation**: Addresses the "Fork-Safety Paradox" from _Fork Safety of Python C-Extensions_...
```

Remove the 8+ inline `> **Ref**: "Fork Safety..."` quotes within that section since they repeat the header reference.

**Step 3: Remove redundant inline quotes**

For each section, remove inline quotes that duplicate the section header's Research Foundation reference. Keep only quotes that add NEW information not in the header.

**Step 4: Commit**

```bash
git add docs/research/roadmap.md
git commit -m "docs: consolidate inline research quotes to section headers"
```

**Lines saved:** ~50

---

## Task 6: Collapse Future Phases to Table Format

**Files:**

- Modify: `docs/research/roadmap.md` (lines 2117-2350)

**Step 1: Identify future phases**

Lines 2117-2350 contain 9 detailed future phase sections (0.12.x - 0.20.x), each with full task breakdowns.

**Step 2: Replace with consolidated table**

Replace the 234 lines with ~30 lines:

```markdown
## Future Phases (Post-1.0)

> **Detailed Specs:** See [external-research.md](external-research.md) for competitive analysis and [topic-archive.md](topic-archive.md) for technical deep-dives.

| Version | Feature          | Description                                | Learn From               |
| ------- | ---------------- | ------------------------------------------ | ------------------------ |
| 0.12.x  | Remote Execution | Distributed test execution across machines | Maelstrom broker/worker  |
| 0.13.x  | Test Sharding    | Intelligent test partitioning for CI       | nextest `--shard N/M`    |
| 0.14.x  | Visual Testing   | Screenshot and visual regression           | Playwright snapshots     |
| 0.15.x  | AI-Powered       | ML for test selection and flaky detection  | -                        |
| 0.16.x  | Mutation Testing | Validate test quality via mutations        | pymute patterns          |
| 0.17.x  | Property-Based   | Hypothesis integration                     | Hypothesis shrinking     |
| 0.18.x  | Contract Testing | API contract validation                    | OpenAPI, Pact            |
| 0.19.x  | Benchmarking     | Built-in performance testing               | `@pytest.mark.benchmark` |
| 0.20.x  | Observability    | OpenTelemetry and Prometheus integration   | OTEL SDK                 |

### Implementation Notes

- **Remote Execution (0.12.x):** Broker manages test queue, content-addressable artifact storage, mDNS discovery
- **Sharding (0.13.x):** `--shard N/M` syntax, deterministic balancing, shard-aware coverage merging
- **AI-Powered (0.15.x):** Track code-to-test relationships, predict failures, quarantine flaky tests
```

**Step 3: Commit**

```bash
git add docs/research/roadmap.md
git commit -m "docs: collapse future phases to table format"
```

**Lines saved:** ~200

---

## Task 7: Trim Documentation Index

**Files:**

- Modify: `docs/research/roadmap.md` (lines 492-534)

**Step 1: Identify verbose index**

The Documentation Index contains 3 tables listing all 24 docs with descriptions.

**Step 2: Simplify to single reference**

Replace with compact version:

```markdown
### Documentation Index

> **Complete Index:** See [README.md](README.md) for the full documentation map.

| Category            | Count | Key Documents                                                                |
| ------------------- | ----- | ---------------------------------------------------------------------------- |
| Deep Dives          | 7     | `isolation-deep-dive.md`, `discovery-deep-dive.md`, `execution-deep-dive.md` |
| Isolation Modules   | 4     | `isolation-landlock.md`, `isolation-seccomp.md`, `isolation-userfaultfd.md`  |
| Research & Analysis | 6     | `external-research.md`, `topic-archive.md`, `container-compatibility.md`     |
| User Documentation  | 7     | `../quickstart.md`, `../configuration.md`, `../troubleshooting.md`           |
```

**Step 3: Commit**

```bash
git add docs/research/roadmap.md
git commit -m "docs: simplify documentation index to compact table"
```

**Lines saved:** ~25

---

## Task 8: Final Verification and Line Count

**Files:**

- Verify: `docs/research/roadmap.md`

**Step 1: Count final lines**

```bash
wc -l docs/research/roadmap.md
```

Expected: ~1,850-1,900 lines (down from 2,388)

**Step 2: Verify all links work**

```bash
# Check that referenced files exist
ls docs/research/container-compatibility.md
ls docs/python-compatibility.md
ls docs/research/isolation-landlock.md
ls docs/research/isolation-userfaultfd.md
ls docs/research/README.md
ls docs/research/external-research.md
ls docs/research/topic-archive.md
```

**Step 3: Verify Mermaid diagrams still render**

Open `docs/research/roadmap.md` in GitHub/GitLab preview to verify Mermaid diagrams render correctly.

**Step 4: Final commit**

```bash
git add docs/research/roadmap.md
git commit -m "docs: complete roadmap line reduction (~25% smaller)"
```

**Step 5: Push**

```bash
git push --no-verify
```

---

## Summary

| Task      | Description                         | Lines Saved    |
| --------- | ----------------------------------- | -------------- |
| 1         | Container compatibility → reference | ~15            |
| 2         | Python compatibility → reference    | ~12            |
| 3         | Kernel requirements → reference     | ~15            |
| 4         | Condense 0.1.x completed sections   | ~205           |
| 5         | Consolidate inline research quotes  | ~50            |
| 6         | Collapse future phases to table     | ~200           |
| 7         | Trim documentation index            | ~25            |
| **Total** |                                     | **~520 lines** |

**Final Result:** ~2,388 → ~1,868 lines (22% reduction)
