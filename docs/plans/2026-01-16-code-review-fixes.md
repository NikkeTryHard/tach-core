# Code Review Fixes Implementation Plan

> **For Claude:** Use `superpowers:executing-plans` to implement this plan task-by-task.

**Goal:** Address all 8 verified issues from the comprehensive code review.

**Architecture:** Pure documentation and configuration fixes. No code changes to src/.

**Tech Stack:** Markdown, TOML, YAML

---

## Summary of Verified Issues

| #   | Issue                                | Complexity | Files  |
| --- | ------------------------------------ | ---------- | ------ |
| 1   | Discovery.md struct drift            | Low        | 1 file |
| 2   | README broken links                  | Medium     | 1 file |
| 3   | API Reference missing STATUS_TIMEOUT | Low        | 1 file |
| 4   | Coverage threshold mismatch          | Low        | 1 file |
| 5   | Volatile version in roadmap          | Low        | 1 file |
| 6   | Benchmarks.md placeholder            | Low        | 1 file |
| 7   | Research README incorrect paths      | Medium     | 1 file |
| 8   | pyproject.toml missing metadata      | Medium     | 1 file |

**Total: 8 tasks across 3 batches**

---

## Batch 1: High Priority Documentation Fixes

### Task 1: Update discovery.md with missing struct fields

**Files:**

- Modify: `docs/architecture/discovery.md`
- Reference: `src/discovery/scanner.rs` (for actual struct definitions)

**Step 1: Add HookDefinition struct**

After the `FixtureScope` enum section, add a new section:

```markdown
### HookDefinition

Represents a pytest hook discovered in conftest.py files.

\`\`\`rust
pub struct HookDefinition {
pub name: String,
pub line_number: usize,
}
\`\`\`

Hook detection is limited to conftest.py files only (not test files).
```

**Step 2: Update TestCase struct**

Add `markers` field to the TestCase struct:

```rust
pub struct TestCase {
    pub name: String,
    pub dependencies: Vec<String>,
    pub is_async: bool,
    pub line_number: usize,
    pub parametrized_args: Vec<String>,
    pub timeout_secs: Option<u64>,
    pub markers: Vec<String>,  // pytest markers (e.g., "slow", "integration")
}
```

**Step 3: Update FixtureDefinition struct**

Add `autouse` field:

```rust
pub struct FixtureDefinition {
    pub name: String,
    pub scope: FixtureScope,
    pub dependencies: Vec<String>,
    pub params: Option<Vec<String>>,
    pub class_scope: Option<String>,
    pub autouse: bool,  // True if @pytest.fixture(autouse=True)
}
```

**Step 4: Update TestModule struct**

Add `hooks` field:

```rust
pub struct TestModule {
    pub path: PathBuf,
    pub tests: Vec<TestCase>,
    pub fixtures: Vec<FixtureDefinition>,
    pub hooks: Vec<HookDefinition>,  // pytest hooks from conftest.py
    pub is_toxic: bool,
}
```

**Step 5: Commit**

```bash
git add docs/architecture/discovery.md && git commit -m "docs(architecture): sync discovery.md struct definitions with implementation"
```

---

### Task 2: Fix broken links in README.md

**Files:**

- Modify: `README.md`

**Step 1: Identify the documentation table**

Find the "Documentation" section with the architecture links table.

**Step 2: Update broken links**

Replace:

- `docs/architecture/protocol.md` → `docs/architecture/overview.md#communication-protocol`
- `docs/architecture/internal-architecture.md` → `docs/architecture/snapshot.md#restoration-physics`
- `docs/security/sandbox-enforcement.md` → `docs/architecture/sandbox.md`

**Step 3: Update link descriptions if needed**

Ensure the descriptions still match the actual content at the new locations.

**Step 4: Commit**

```bash
git add README.md && git commit -m "docs: fix broken architecture documentation links in README"
```

---

### Task 3: Add STATUS_TIMEOUT to api-reference.md

**Files:**

- Modify: `docs/api-reference.md`

**Step 1: Find the Status Codes table**

Look for the table containing STATUS_PASS, STATUS_FAIL, etc.

**Step 2: Add missing row**

After `STATUS_HARNESS_ERROR | 5 | Harness error`, add:

```markdown
| `STATUS_TIMEOUT` | 6 | Test exceeded timeout |
```

**Step 3: Commit**

```bash
git add docs/api-reference.md && git commit -m "docs(api): add missing STATUS_TIMEOUT to status codes table"
```

---

## Batch 2: Medium Priority Configuration Fixes

### Task 4: Align coverage thresholds

**Files:**

- Modify: `codecov.yml`

**Step 1: Update project target to realistic level**

Change from `target: 90%` to `target: 20%` (slightly above CI's 15% floor).

Or alternatively, make it informational until coverage improves:

```yaml
status:
  project:
    default:
      target: auto # Use base commit as target
      threshold: 5%
      informational: true # Don't fail PRs
```

**Step 2: Update patch target**

Change from `target: 80%` to a more realistic level or make informational.

**Step 3: Add comment explaining the strategy**

```yaml
# Coverage Strategy:
# - CI enforces 15% minimum floor (hard failure)
# - Codecov tracks improvement trend (informational)
# - Target: Increase to 90% by v1.0.0
```

**Step 4: Commit**

```bash
git add codecov.yml && git commit -m "chore(codecov): align coverage targets with CI threshold"
```

---

### Task 5: Fix research README file paths

**Files:**

- Modify: `docs/research/README.md`

**Step 1: Update Paper-to-Component Mapping table**

Replace incorrect paths with actual paths:

| Paper Topic     | Old Path                     | Correct Path                                    |
| --------------- | ---------------------------- | ----------------------------------------------- |
| Zygote Tree     | `src/zygote/tree.rs`         | `src/execution/zygote.rs`                       |
| Memory Snapshot | `src/mem/snapshot.rs`        | `src/isolation/snapshot.rs`                     |
| Module Loader   | `src/python/loader.rs`       | `src/discovery/loader.rs`                       |
| Toxicity        | `tach-analyzer/src/toxic.rs` | `src/discovery/scanner.rs` (toxicity detection) |
| CLI Runner      | `tach-cli/src/runner/`       | `src/execution/`                                |

**Step 2: Mark unimplemented paths**

For paths that don't exist yet (libtach_preload.so, tach-vfs/), add note:
`(Planned - not yet implemented)`

**Step 3: Commit**

```bash
git add docs/research/README.md && git commit -m "docs(research): update file paths to match actual codebase structure"
```

---

### Task 6: Add pyproject.toml metadata

**Files:**

- Modify: `pyproject.toml`

**Step 1: Add project metadata section at the top**

```toml
[project]
name = "tach-core"
version = "0.1.5"
description = "Hypervisor-accelerated Python test runner"
readme = "README.md"
license = "MIT"
requires-python = ">=3.10"
authors = [
    { name = "NikkeTryHard" }
]
keywords = ["testing", "pytest", "performance", "isolation"]
classifiers = [
    "Development Status :: 3 - Alpha",
    "Environment :: Console",
    "Intended Audience :: Developers",
    "License :: OSI Approved :: MIT License",
    "Operating System :: POSIX :: Linux",
    "Programming Language :: Python :: 3",
    "Programming Language :: Python :: 3.10",
    "Programming Language :: Python :: 3.11",
    "Programming Language :: Python :: 3.12",
    "Programming Language :: Python :: 3.13",
    "Topic :: Software Development :: Testing",
]

[project.urls]
Homepage = "https://github.com/NikkeTryHard/tach-core"
Repository = "https://github.com/NikkeTryHard/tach-core"
Changelog = "https://github.com/NikkeTryHard/tach-core/blob/master/CHANGELOG.md"
```

**Step 2: Keep existing tool sections**

Leave the `[tool.pytest_env]` and `[tool.pytest.ini_options]` sections as-is.

**Step 3: Commit**

```bash
git add pyproject.toml && git commit -m "chore(python): add standard project metadata to pyproject.toml"
```

---

## Batch 3: Low Priority Cleanup

### Task 7: Remove volatile version from roadmap

**Files:**

- Modify: `docs/research/roadmap.md`

**Step 1: Update line 3**

Change from:

```markdown
> **Current Version:** 0.1.5 (see [CHANGELOG.md](../../CHANGELOG.md) for release notes)
```

To:

```markdown
> **Current Version:** See [CHANGELOG.md](../../CHANGELOG.md) for the latest release and version history.
```

**Step 2: Commit**

```bash
git add docs/research/roadmap.md && git commit -m "docs(research): remove volatile version number from roadmap"
```

---

### Task 8: Update benchmarks.md placeholder

**Files:**

- Modify: `docs/benchmarks.md`

**Step 1: Add work-in-progress notice at the top**

After the title, add:

```markdown
> **Status:** Work in Progress - Benchmark infrastructure is defined but data collection is pending.
```

**Step 2: Fix or remove script reference**

Find reference to `./scripts/benchmark.sh` and either:

- Remove it, OR
- Change to: `Benchmark scripts will be added in a future release.`

**Step 3: Commit**

```bash
git add docs/benchmarks.md && git commit -m "docs: mark benchmarks.md as work-in-progress"
```

---

## Final Verification

### Task 9: Verify all fixes

**Step 1: Check for remaining broken links**

```bash
grep -roh '\[.*\]([^)]*\.md[^)]*)' docs/ README.md | grep -v http | sort -u
```

Verify each linked file exists.

**Step 2: Run documentation linter (optional)**

```bash
npx markdownlint docs/**/*.md README.md || true
```

**Step 3: Final commit summary**

```bash
git log --oneline -10
```

---

## Summary of Changes

| Batch       | Tasks | Files Modified                                  |
| ----------- | ----- | ----------------------------------------------- |
| **Batch 1** | 3     | discovery.md, README.md, api-reference.md       |
| **Batch 2** | 3     | codecov.yml, research/README.md, pyproject.toml |
| **Batch 3** | 2     | roadmap.md, benchmarks.md                       |
| **Verify**  | 1     | (verification only)                             |

**Total: 9 tasks, 8 files modified**

---

## Issues NOT Addressed (Verified as Non-Issues)

1. **Cargo.toml test declarations** - All 39 files have declarations
2. **Quickstart.md broken link** - examples/django/README.md exists
