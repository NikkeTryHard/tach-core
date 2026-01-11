# Documentation Cleanup Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Remove volatile data, redundancy, and stale content from all 59 markdown files while preserving depth and detail.

**Architecture:** Delete stale plans first, then fix volatile data in core docs, merge redundant files, consolidate research topics, and finally gitignore auto-generated files.

**Tech Stack:** Markdown, Git

---

## Task 1: Delete Stale Plans

**Files:**
- Delete: `docs/plans/2026-01-09-documentation-updates.md`
- Delete: `docs/plans/2026-01-10-docker-setup.md`
- Delete: `docs/plans/2026-01-10-docker-setup-design.md`
- Delete: `docs/plans/2026-01-10-documentation-fixes.md`
- Delete: `docs/plans/2026-01-10-investigation-roadmap-expansion.md`
- Delete: `docs/plans/2026-01-10-test-suite-and-ignore-docs.md`

**Step 1:** Delete all 6 stale plan files

```bash
rm docs/plans/2026-01-09-documentation-updates.md
rm docs/plans/2026-01-10-docker-setup.md
rm docs/plans/2026-01-10-docker-setup-design.md
rm docs/plans/2026-01-10-documentation-fixes.md
rm docs/plans/2026-01-10-investigation-roadmap-expansion.md
rm docs/plans/2026-01-10-test-suite-and-ignore-docs.md
```

**Step 2:** Commit

```bash
git add -A
git commit -m "chore: delete completed plan files"
```

---

## Task 2: Fix Volatile Data in README.md

**Files:**
- Modify: `README.md`

**Issue:** Hardcoded version number in status badge (`Version-0.1.4`)

**Fix:** Remove hardcoded version badge or link to CHANGELOG.md instead.

**Step 1:** Find and remove or fix the version badge line

**Step 2:** Commit

```bash
git add README.md
git commit -m "docs: remove hardcoded version from README badge"
```

---

## Task 3: Fix Volatile Data in troubleshooting.md

**Files:**
- Modify: `docs/troubleshooting.md`

**Issue:** Hardcoded test counts like "Discovered 0 tests in 0 files"

**Fix:** Replace with generic placeholders like "Discovered N tests in M files"

**Step 1:** Find and replace all hardcoded test counts with placeholders

**Step 2:** Commit

```bash
git add docs/troubleshooting.md
git commit -m "docs: replace hardcoded test counts with placeholders in troubleshooting"
```

---

## Task 4: Fix Volatile Data in api-reference.md

**Files:**
- Modify: `docs/api-reference.md`

**Issue:** Hardcoded NDJSON event counts and LCOV line numbers

**Fix:** Replace specific numbers with generic examples or placeholders

**Step 1:** Replace hardcoded values like `"total":100`, `"passed":98` with `"total": N`, `"passed": M`

**Step 2:** Simplify LCOV examples to use fewer lines with placeholder comments

**Step 3:** Commit

```bash
git add docs/api-reference.md
git commit -m "docs: replace hardcoded counts with placeholders in api-reference"
```

---

## Task 5: Fix Volatile Data in reporter.md

**Files:**
- Modify: `docs/architecture/reporter.md`

**Issue:** Hardcoded progress bar counts like "45/100 P:40 F:3 S:2"

**Fix:** Use placeholders like `[pos]/[total] P:[passed] F:[failed] S:[skipped]`

**Step 1:** Find and replace hardcoded progress examples

**Step 2:** Commit

```bash
git add docs/architecture/reporter.md
git commit -m "docs: replace hardcoded progress counts with placeholders in reporter"
```

---

## Task 6: Fix Volatile Data in container-compatibility.md

**Files:**
- Modify: `docs/research/container-compatibility.md`

**Issue:** Hardcoded line number reference `// src/isolation/sandbox.rs line 208`

**Fix:** Reference function name instead of line number

**Step 1:** Replace line number references with function/module names

**Step 2:** Commit

```bash
git add docs/research/container-compatibility.md
git commit -m "docs: replace line numbers with function names in container-compatibility"
```

---

## Task 7: Condense Verbose roadmap.md

**Files:**
- Modify: `docs/research/roadmap.md`

**Issue:** Wordy "Strategic Context" and specific month/year references

**Fix:**
- Convert competitive analysis prose to summary table
- Replace specific dates like `2027-10` with relative quarters `Q4 2027`

**Step 1:** Condense verbose sections into tables or bullet points

**Step 2:** Replace absolute dates with relative references

**Step 3:** Commit

```bash
git add docs/research/roadmap.md
git commit -m "docs: condense verbose sections and fix date references in roadmap"
```

---

## Task 8: Merge errors.md into troubleshooting.md

**Files:**
- Modify: `docs/troubleshooting.md`
- Delete: `docs/errors.md`

**Reason:** Error codes (E001, etc.) are logical part of troubleshooting

**Step 1:** Read `docs/errors.md` content

**Step 2:** Add "Error Codes" section to `docs/troubleshooting.md` with the error code content

**Step 3:** Delete `docs/errors.md`

**Step 4:** Update any references to errors.md to point to troubleshooting.md

**Step 5:** Commit

```bash
git add -A
git commit -m "docs: merge errors.md into troubleshooting.md"
```

---

## Task 9: Merge wsl2-setup.md into quickstart.md

**Files:**
- Modify: `docs/quickstart.md`
- Delete: `docs/wsl2-setup.md`

**Reason:** WSL2 setup is platform-specific onboarding, belongs in quickstart

**Step 1:** Read `docs/wsl2-setup.md` content

**Step 2:** Add "WSL2 Setup" section to `docs/quickstart.md`

**Step 3:** Delete `docs/wsl2-setup.md`

**Step 4:** Update any references to wsl2-setup.md

**Step 5:** Commit

```bash
git add -A
git commit -m "docs: merge wsl2-setup.md into quickstart.md"
```

---

## Task 10: Consolidate Research Topics

**Files:**
- Create: `docs/research/topic-archive.md`
- Delete: `docs/research/topics/fork-safety.md`
- Delete: `docs/research/topics/memory-snapshotting.md`
- Delete: `docs/research/topics/zygote-patterns.md`
- Delete: `docs/research/topics/isolation.md`
- Delete: `docs/research/topics/rust-integration.md`
- Delete: `docs/research/topics/cross-platform.md`
- Delete: `docs/research/topics/` (directory)

**Reason:** 6 small blueprint files better as sections in one archive

**Step 1:** Create `topic-archive.md` with header

**Step 2:** Read each topic file and add as section in archive

**Step 3:** Delete individual topic files

**Step 4:** Remove empty `topics/` directory

**Step 5:** Commit

```bash
git add -A
git commit -m "docs: consolidate research topics into single archive"
```

---

## Task 11: Merge Redundant Isolation Docs

**Files:**
- Modify: `docs/architecture/isolation.md`
- Delete: `docs/research/topics/isolation.md` (already deleted in Task 10)

**Note:** If isolation.md wasn't fully merged in Task 10, ensure architecture version has all content.

**Step 1:** Verify `docs/architecture/isolation.md` has complete content

**Step 2:** Commit if any changes

```bash
git add docs/architecture/isolation.md
git commit -m "docs: ensure isolation.md has complete content"
```

---

## Task 12: Merge Sandbox Security Docs

**Files:**
- Modify: `docs/architecture/sandbox.md`
- Delete: `docs/security/sandbox-enforcement.md`

**Reason:** Significant overlap in Landlock/Seccomp explanations

**Step 1:** Read `docs/security/sandbox-enforcement.md`

**Step 2:** Merge unique content (like "Suicide Worker" testing pattern) into `docs/architecture/sandbox.md`

**Step 3:** Delete `docs/security/sandbox-enforcement.md`

**Step 4:** If `docs/security/` is now empty, delete the directory

**Step 5:** Commit

```bash
git add -A
git commit -m "docs: merge sandbox-enforcement into architecture/sandbox.md"
```

---

## Task 13: Gitignore FULL_DOCUMENTATION.md

**Files:**
- Modify: `.gitignore`
- Delete: `docs/FULL_DOCUMENTATION.md`

**Reason:** Auto-generated 10,000+ line file causes merge conflicts and redundancy

**Step 1:** Add `docs/FULL_DOCUMENTATION.md` to `.gitignore`

**Step 2:** Delete the file from git tracking

```bash
git rm docs/FULL_DOCUMENTATION.md
```

**Step 3:** Commit

```bash
git add .gitignore
git commit -m "docs: gitignore auto-generated FULL_DOCUMENTATION.md"
```

---

## Task 14: Clean Up Verbose benchmarks.md

**Files:**
- Modify: `docs/benchmarks.md`

**Issue:** Contains TODO placeholder "Add actual benchmark results"

**Fix:** Either add real benchmarks or remove the placeholder section

**Step 1:** Check if real benchmark data exists; if not, remove TODO section

**Step 2:** Commit

```bash
git add docs/benchmarks.md
git commit -m "docs: clean up benchmarks.md placeholder"
```

---

## Task 15: Deduplicate README.md and quickstart.md

**Files:**
- Modify: `README.md`
- Modify: `docs/quickstart.md`

**Issue:** Installation steps for Ubuntu/Fedora/Arch duplicated in both

**Fix:** Keep brief "Quick Start" in README, move detailed steps to quickstart.md

**Step 1:** In README.md, reduce installation to brief summary with link to quickstart.md

**Step 2:** Ensure quickstart.md has complete installation instructions

**Step 3:** Commit

```bash
git add README.md docs/quickstart.md
git commit -m "docs: deduplicate installation steps between README and quickstart"
```

---

## Task 16: Final Verification

**Step 1:** Count remaining markdown files

```bash
find . -name "*.md" -not -path "./.git/*" -not -path "./.worktrees/*" -not -path "./target/*" | wc -l
```

Expected: ~45-50 files (down from 59)

**Step 2:** Grep for remaining volatile data

```bash
grep -r "line [0-9]\+" docs/ --include="*.md" | grep -v "command line"
grep -r "[0-9]\+ tests\|[0-9]\+ passed\|[0-9]\+ failed" docs/ --include="*.md"
```

Expected: No matches or only legitimate examples

**Step 3:** Verify no broken internal links

```bash
grep -r "\[.*\](.*\.md)" docs/ --include="*.md" | grep -v "http"
```

Check that referenced files exist.

---

## Summary

| Task | Files Affected | Action |
|------|----------------|--------|
| 1 | 6 plan files | Delete |
| 2 | README.md | Fix version badge |
| 3 | troubleshooting.md | Fix test counts |
| 4 | api-reference.md | Fix hardcoded values |
| 5 | reporter.md | Fix progress counts |
| 6 | container-compatibility.md | Fix line numbers |
| 7 | roadmap.md | Condense verbose |
| 8 | errors.md → troubleshooting.md | Merge |
| 9 | wsl2-setup.md → quickstart.md | Merge |
| 10 | 6 topic files → topic-archive.md | Consolidate |
| 11 | isolation.md | Verify complete |
| 12 | sandbox-enforcement.md → sandbox.md | Merge |
| 13 | FULL_DOCUMENTATION.md | Gitignore |
| 14 | benchmarks.md | Clean TODO |
| 15 | README + quickstart | Deduplicate |
| 16 | All | Final verification |

**Expected Results:**
- ~15 files deleted or merged
- 0 volatile data (line numbers, test counts)
- 0 redundant content
- Cleaner directory structure
