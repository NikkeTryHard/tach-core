# Documentation Cleanup Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Clean up, consolidate, and correct all project documentation to eliminate volatile data, reduce redundancy, fix inaccuracies, and improve maintainability.

**Architecture:** This is a pure documentation refactor. No code changes except fixing Cargo.toml version. Work proceeds in batches: deletions first, then corrections, then consolidations, then CLAUDE.md sync.

**Tech Stack:** Markdown, Git

---

## Batch 1: Deletions and Quick Fixes

### Task 1: Delete Stale Plan File

**Files:**

- Delete: `docs/plans/2026-01-14-0.1.x-cleanup-and-0.2.x-foundation.md`

**Step 1: Delete the file**

```bash
rm docs/plans/2026-01-14-0.1.x-cleanup-and-0.2.x-foundation.md
```

**Step 2: Verify deletion**

```bash
ls docs/plans/
```

Expected: Only `2026-01-15-v0.2.0-hook-interception-completion.md` and this plan remain.

**Step 3: Commit**

```bash
git add -A && git commit -m "chore(docs): delete completed 0.1.x cleanup plan"
```

---

### Task 2: Fix Version Mismatch in Cargo.toml

**Files:**

- Modify: `Cargo.toml` (version field)

**Step 1: Update version to match CHANGELOG**

Change `version = "0.1.4"` to `version = "0.1.5"` in Cargo.toml.

**Step 2: Verify**

```bash
grep '^version' Cargo.toml
```

Expected: `version = "0.1.5"`

**Step 3: Commit**

```bash
git add Cargo.toml && git commit -m "chore: bump version to 0.1.5 to match CHANGELOG"
```

---

## Batch 2: Fix Security Documentation Drift

### Task 3: Update sandbox.md with Correct Syscall List

**Files:**

- Modify: `docs/architecture/sandbox.md`
- Reference: `src/isolation/sandbox.rs` (for actual syscall list)

**Step 1: Read actual blocked syscalls from code**

Find the seccomp blocklist in `src/isolation/sandbox.rs` and extract all 22 syscalls.

**Step 2: Update sandbox.md**

Replace the current 15-syscall list with the complete 22-syscall list. Add the missing syscalls:

- `chown`, `fchown`, `lchown`
- `chmod`, `fchmod`, `fchmodat`
- `rename`

Format as a table with syscall name and reason for blocking.

**Step 3: Verify accuracy**

Cross-reference the updated doc with the code to ensure 1:1 match.

**Step 4: Commit**

```bash
git add docs/architecture/sandbox.md && git commit -m "docs(architecture): update seccomp blocklist to match implementation (22 syscalls)"
```

---

### Task 4: Update discovery.md Function Signature

**Files:**

- Modify: `docs/architecture/discovery.md`

**Step 1: Fix function signature**

Change documented signature from:

```rust
analyze_function(source: &str) -> Vec<Marker>
```

To actual signature:

```rust
analyze_function(stmt: &ast::StmtFunctionDef) -> Vec<Marker>
```

**Step 2: Commit**

```bash
git add docs/architecture/discovery.md && git commit -m "docs(architecture): fix analyze_function signature drift"
```

---

## Batch 3: Remove Volatile Data from Research Files

### Task 5: Clean container-compatibility.md

**Files:**

- Modify: `docs/research/container-compatibility.md`

**Step 1: Remove volatile test counts**

Find and replace these patterns:

- Line ~14: `"only 3 of 5 tests pass"` → `"partial test success"`
- Lines ~38-53: Replace specific PASS/FAIL counts with qualitative status
- Lines ~74-82: Replace summary table counts with status indicators (✅/⚠️/❌)
- Line ~265: `"3 PASS, 2 FAIL"` → `"mixed results"`

**Step 2: Verify no hardcoded counts remain**

```bash
grep -nE '[0-9]+ (PASS|FAIL|tests? pass)' docs/research/container-compatibility.md
```

Expected: No matches.

**Step 3: Commit**

```bash
git add docs/research/container-compatibility.md && git commit -m "docs(research): remove volatile test counts from container-compatibility"
```

---

### Task 6: Clean external-research.md

**Files:**

- Modify: `docs/research/external-research.md`

**Step 1: Remove volatile timing data**

Find and remove/generalize:

- Line ~1429-1430: Comparative timing data (`<1ms`, `50-100ms`) → describe as "significantly faster"
- Line ~1459: `"34891 tests collected in 197.54s"` → `"tens of thousands of tests"` (this is a quote, add note that it's historical)

**Step 2: Commit**

```bash
git add docs/research/external-research.md && git commit -m "docs(research): remove volatile timing data from external-research"
```

---

### Task 7: Clean research-investigation.md

**Files:**

- Modify: `docs/research/research-investigation.md`

**Step 1: Remove ASCII file tree**

Find and delete the 12-line ASCII file tree dump. Replace with reference to actual directory.

**Step 2: Commit**

```bash
git add docs/research/research-investigation.md && git commit -m "docs(research): remove volatile file tree from research-investigation"
```

---

### Task 8: Clean test-discovery-analysis.md

**Files:**

- Modify: `docs/research/test-discovery-analysis.md`

**Step 1: Remove specific counts**

- Lines ~113-115: Replace exact durations (`~30-60s`, `~10-30s`) with relative comparisons
- Lines ~168-175: Replace exact PropTest cases (`200`, `300`, `100`) with "configurable" or remove

**Step 2: Commit**

```bash
git add docs/research/test-discovery-analysis.md && git commit -m "docs(research): remove volatile counts from test-discovery-analysis"
```

---

## Batch 4: Consolidate Architecture Docs

### Task 9: Merge internal-architecture.md into snapshot.md

**Files:**

- Modify: `docs/architecture/snapshot.md`
- Delete: `docs/architecture/internal-architecture.md`

**Step 1: Read both files**

Identify unique content in `internal-architecture.md` (Physics of Restoration, TCB/BSS/Heap/Stack, Restoration Quadrant diagram).

**Step 2: Append to snapshot.md**

Add a new section "## Restoration Physics" to `snapshot.md` containing the unique content from `internal-architecture.md`. Preserve the Restoration Quadrant diagram.

**Step 3: Delete internal-architecture.md**

```bash
rm docs/architecture/internal-architecture.md
```

**Step 4: Update any references**

Search for links to `internal-architecture.md` and update to `snapshot.md#restoration-physics`.

```bash
grep -r "internal-architecture" docs/
```

**Step 5: Commit**

```bash
git add -A && git commit -m "docs(architecture): merge internal-architecture into snapshot.md"
```

---

### Task 10: Merge protocol.md into overview.md

**Files:**

- Modify: `docs/architecture/overview.md`
- Delete: `docs/architecture/protocol.md`

**Step 1: Read protocol.md**

Extract the framing protocol content (8-byte header, `TA` magic).

**Step 2: Add to overview.md**

Add a section "## Communication Protocol" to `overview.md` with the protocol content.

**Step 3: Delete protocol.md**

```bash
rm docs/architecture/protocol.md
```

**Step 4: Update references**

```bash
grep -r "protocol.md" docs/
```

**Step 5: Commit**

```bash
git add -A && git commit -m "docs(architecture): merge protocol into overview.md"
```

---

## Batch 5: Reduce Redundancy

### Task 11: De-duplicate development.md

**Files:**

- Modify: `docs/development.md`
- Reference: `README.md`

**Step 1: Identify duplicated sections**

These sections in `development.md` are duplicated from README:

- "Project Structure" table
- "Quick Start" / Docker commands
- Basic build commands

**Step 2: Remove duplicates, add references**

Replace duplicated content with:

```markdown
> See [README.md](../README.md) for project structure, Docker setup, and quick start.
```

Keep only development-specific content:

- Detailed testing commands
- Native (non-Docker) setup
- Contribution guidelines

**Step 3: Commit**

```bash
git add docs/development.md && git commit -m "docs: de-duplicate development.md, reference README"
```

---

### Task 12: Merge research-investigation.md and research-reference.md

**Files:**

- Modify: `docs/research/research-reference.md` → rename to `docs/research/README.md`
- Delete: `docs/research/research-investigation.md`

**Step 1: Create docs/research/README.md**

Combine the table of contents from both files into a single `README.md` for the research directory.

**Step 2: Delete old files**

```bash
rm docs/research/research-investigation.md docs/research/research-reference.md
```

**Step 3: Commit**

```bash
git add -A && git commit -m "docs(research): consolidate index files into README.md"
```

---

## Batch 6: Archive Research Papers

### Task 13: Move papers-very-verbose to archive

**Files:**

- Move: `docs/research/papers-very-verbose/` → `docs/archive/research-papers/`

**Step 1: Create archive directory**

```bash
mkdir -p docs/archive
```

**Step 2: Move papers**

```bash
mv docs/research/papers-very-verbose docs/archive/research-papers
```

**Step 3: Update any references**

```bash
grep -r "papers-very-verbose" docs/
```

**Step 4: Commit**

```bash
git add -A && git commit -m "docs: archive verbose research papers"
```

---

## Batch 7: Sync CLAUDE.md

### Task 14: Update CLAUDE.md Test Hierarchy

**Files:**

- Modify: `CLAUDE.md`

**Step 1: Remove volatile file counts**

In the "Test Hierarchy" section, remove specific counts like:

- "24 files" for integration tests
- "5 files" for proptest
- "10 fuzzers"

Replace with general descriptions or remove counts entirely. Use patterns instead:

```markdown
rust*tests/ # Rust integration tests
\*\_integration.rs # Component integration
proptest*\*.rs # Property-based testing
```

**Step 2: Fix test naming convention**

Update or remove the `test_<letter>_<description>` convention if it's not consistently followed.

**Step 3: Commit (if git-tracked) or save**

Note: CLAUDE.md is gitignored, so just save. No commit needed.

---

### Task 15: Fix CLAUDE.md Logging Convention

**Files:**

- Modify: `CLAUDE.md`

**Step 1: Update logging rule**

Change the rule from "NEVER use bare `[tach]` prefixes" to acknowledge reality:

```markdown
| **Prefixes** | Use `[tach:module]` for component-specific output. Bare `[tach]` acceptable for lifecycle events. |
```

**Step 2: Save**

Note: CLAUDE.md is gitignored.

---

### Task 16: Remove CLI Duplication Note in CLAUDE.md

**Files:**

- Modify: `CLAUDE.md`

**Step 1: Add reference to configuration.md**

In the CLI Reference section, add:

```markdown
> For complete CLI reference, see [docs/configuration.md](docs/configuration.md).
```

Keep the quick reference table but note it may drift.

**Step 2: Save**

---

## Batch 8: Final Verification

### Task 17: Verify All Changes

**Step 1: Check for remaining volatile data**

```bash
grep -rn "tests pass\|tests fail\|[0-9]\+ files\|[0-9]\+ms\|[0-9]\+s collected" docs/ --include="*.md" | grep -v "CHANGELOG\|plans/"
```

**Step 2: Check for broken internal links**

```bash
grep -roh '\[.*\](.*\.md)' docs/ | grep -v http | sort -u
```

Verify each linked file exists.

**Step 3: Run documentation linter (if available)**

```bash
# Optional: markdownlint if installed
npx markdownlint docs/**/*.md || true
```

---

### Task 18: Final Commit and Summary

**Step 1: Check git status**

```bash
git status
```

**Step 2: Create summary commit if any stragglers**

```bash
git add -A && git commit -m "docs: complete documentation cleanup and consolidation" || echo "Nothing to commit"
```

**Step 3: Delete this plan file**

Once complete, this plan can be deleted:

```bash
rm docs/plans/2026-01-15-documentation-cleanup.md
git add -A && git commit -m "chore: remove completed documentation cleanup plan"
```

---

## Summary of Changes

| Category            | Action                      | Files Affected        |
| ------------------- | --------------------------- | --------------------- |
| **Deletions**       | Remove stale plan           | 1 file                |
| **Version Fix**     | Cargo.toml 0.1.4 → 0.1.5    | 1 file                |
| **Security Drift**  | Update syscall list         | sandbox.md            |
| **Signature Drift** | Fix function signature      | discovery.md          |
| **Volatile Data**   | Remove hardcoded counts     | 4 research files      |
| **Consolidation**   | Merge architecture docs     | 2 merges, 2 deletions |
| **Redundancy**      | De-duplicate development.md | 1 file                |
| **Research Index**  | Merge index files           | 2 files → 1 README    |
| **Archive**         | Move verbose papers         | 1 directory           |
| **CLAUDE.md Sync**  | Update counts, conventions  | 1 file (gitignored)   |

**Total: ~18 tasks across 8 batches**
