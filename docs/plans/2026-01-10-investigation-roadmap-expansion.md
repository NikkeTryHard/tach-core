# Investigation and Roadmap Expansion Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Deep investigation of Docker behavior, tooling conflicts, and discovery edge cases, then expand roadmap.md with research findings.

**Architecture:** Three independent investigation tracks that can run in parallel, followed by roadmap expansion.

**Tech Stack:** Docker, Cargo, pytest, Rust (ignore crate), Markdown

---

## Task 1: Investigate Docker/Container Sandbox Behavior

**Files:**

- Read: `tests/gauntlet/test_fs_destruction.py`
- Read: `src/isolation/sandbox.rs`
- Read: `src/isolation/landlock.rs`
- Create: `docs/research/container-compatibility.md`

**Step 1: Analyze why sandbox tests fail in privileged containers**

Run:

```bash
docker-compose exec dev bash -c 'cd /workspace && pytest tests/gauntlet/test_fs_destruction.py -v 2>&1 | head -40'
```

Expected: See 5 failures with reasons (privileged container can write to `/etc`, `/usr`)

**Step 2: Verify sandbox works when running through tach-core**

Run:

```bash
docker-compose exec dev bash -c 'cd /workspace && ./target/release/tach-core tests/gauntlet/test_fs_destruction.py 2>&1 | head -40'
```

Expected: Sandbox blocks writes, tests pass or are properly isolated

**Step 3: Document container capability requirements**

Research what Docker capabilities affect Landlock/Seccomp:

- `SYS_ADMIN` - Affects namespace creation
- `SYS_PTRACE` - Affects userfaultfd
- `--security-opt seccomp=unconfined` - Bypasses seccomp

**Step 4: Create container-compatibility.md**

Document findings with a compatibility matrix:

| Container Type | Landlock | Seccomp | userfaultfd | Notes |
|----------------|----------|---------|-------------|-------|
| Docker (default) | ? | ? | ? | |
| Docker (privileged) | ? | ? | ? | |
| Docker (with caps) | ? | ? | ? | |
| Podman (rootless) | ? | ? | ? | |
| Kubernetes Pod | ? | ? | ? | |

---

## Task 2: Research .ignore/Tooling Conflicts

**Files:**

- Read: `src/discovery/scanner.rs`
- Read: `.ignore`
- Create: `docs/research/tooling-conflicts.md`

**Step 1: Document how WalkBuilder uses ignore files**

Read the ignore crate documentation:

- `.gitignore` - Git patterns
- `.ignore` - ripgrep/fd/tach patterns
- `.fdignore` - fd-specific patterns

**Step 2: Check what patterns are safe vs dangerous**

Analyze common patterns that could break tach-core:

- `*.py` - Dangerous (blocks all Python)
- `__pycache__/` - Safe (should be ignored)
- `.venv/` - Safe (should be ignored)
- `*.pyc` - Safe (bytecode)

**Step 3: Research potential safeguards**

Options:
1. Add a `--ignore-ignore-files` flag
2. Check for `*.py` in `.ignore` and warn
3. Document in troubleshooting.md (already done)
4. Create a `.tachignore` that takes precedence

**Step 4: Create tooling-conflicts.md**

Document the interaction between:

- Claude Code (adds `*.py` to `.ignore` for context filtering)
- ripgrep/fd (respects `.ignore`)
- tach-core (uses `ignore` crate via WalkBuilder)

---

## Task 3: Investigate Ignored Tests and Discovery Edge Cases

**Files:**

- Read: `rust_tests/` (multiple test files)
- Create: `docs/research/test-discovery-analysis.md`

**Step 1: List all ignored tests**

Run:

```bash
docker-compose exec dev bash -c 'cd /workspace && cargo test --test "*" 2>&1 | grep -E "ignored|IGNORED"'
```

Expected: See 18 ignored tests with names

**Step 2: Categorize ignored tests**

For each ignored test, determine why it's ignored:

- `#[ignore]` attribute (intentional)
- Requires specific kernel features
- Requires specific environment
- Flaky/unstable

**Step 3: Analyze discovery edge cases**

Look for tests that exercise discovery:

```bash
docker-compose exec dev bash -c 'cd /workspace && cargo test --test discovery_integration 2>&1'
```

**Step 4: Create test-discovery-analysis.md**

Document:

- Categories of ignored tests
- Known discovery limitations
- Edge cases in AST parsing
- Potential improvements

---

## Task 4: Expand Roadmap with Research Findings

**Files:**

- Modify: `docs/research/roadmap.md`
- Reference: All research documents created above

**Step 1: Add Container Compatibility section to 0.1.x**

Add findings about container behavior to the current version section.

**Step 2: Add Tooling Ecosystem section**

Document how tach-core interacts with other developer tools.

**Step 3: Add Discovery Robustness improvements**

Based on edge case analysis, add items for improving discovery.

**Step 4: Add Research Verification items**

Add checkboxes for verifying:

- [ ] Container compatibility matrix documented
- [ ] Tooling conflict patterns identified
- [ ] Discovery edge cases catalogued

---

## Task 5: Commit and Finish

**Step 1: Commit research documents**

```bash
git add docs/research/
git commit -m "docs: add research on container compatibility and tooling conflicts

- Document Docker/container sandbox behavior
- Analyze .ignore file interactions with developer tools
- Catalogue ignored tests and discovery edge cases

Co-Authored-By: Claude <noreply@anthropic.com>"
```

**Step 2: Commit roadmap updates**

```bash
git add docs/research/roadmap.md
git commit -m "docs: expand roadmap with container and tooling research

- Add container compatibility research to 0.1.x
- Add tooling ecosystem considerations
- Add discovery robustness improvements

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Completion Checklist

- [ ] Task 1: Container compatibility matrix documented
- [ ] Task 2: Tooling conflicts research complete
- [ ] Task 3: Ignored tests analyzed
- [ ] Task 4: Roadmap expanded
- [ ] Task 5: Changes committed
