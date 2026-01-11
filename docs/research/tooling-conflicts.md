# Tooling Conflicts: The `ignore` Crate and File Discovery

> **Status**: Research Document
> **Created**: 2025-01-11
> **Related**: Discovery subsystem, scanner.rs

## Executive Summary

Tach-core uses the `ignore` crate for file discovery, which respects `.ignore`, `.gitignore`, and global gitignore files. This creates potential conflicts with other developer tools that also use or modify these files. Most critically, patterns like `*.py` in `.ignore` can completely break tach-core's test discovery.

## Background

### How Tach-Core Uses the `ignore` Crate

In `src/discovery/scanner.rs`, tach-core uses `WalkBuilder` from the `ignore` crate:

```rust
let paths: Vec<PathBuf> = WalkBuilder::new(&canonical_root)
    .standard_filters(true)  // <-- This enables .ignore/.gitignore respect
    .follow_links(true)
    .build()
    .filter_map(|e| e.ok())
    .filter(|e| is_test_file(e.path()))
    .map(|e| /* path canonicalization */)
    .collect();
```

The `standard_filters(true)` call enables:
1. `.gitignore` file parsing and respect
2. `.ignore` file parsing (HIGHER priority than .gitignore)
3. Global gitignore (`~/.config/git/ignore` or `~/.gitignore_global`)
4. Hidden file filtering (files starting with `.`)
5. Parent directory ignore file inheritance

### File Precedence (Highest to Lowest)

1. **`.ignore`** - Highest priority, used by ignore-crate tools
2. **`.gitignore`** - Standard git ignore patterns
3. **Global gitignore** - User's global git configuration
4. **Hidden file rules** - Files/directories starting with `.`

The `.ignore` file takes precedence over `.gitignore`, meaning a negation in `.ignore` can override a pattern in `.gitignore`, and vice versa.

## Tools That Share the `.ignore` File

### Tools That READ `.ignore`

| Tool | Purpose | Notes |
|------|---------|-------|
| **ripgrep (rg)** | Fast text search | Primary user of `.ignore`; created the convention |
| **fd** | Fast file finder | Alternative to `find` |
| **tach-core** | Python test runner | Uses for test file discovery |
| **tokei** | Code statistics | Line counting |
| **watchexec** | File watcher | Re-run commands on file changes |
| **delta** | Git diff viewer | Syntax highlighting |

### Tools That WRITE to `.ignore`

| Tool | Patterns Added | Impact on Tach |
|------|----------------|----------------|
| **Claude Code** | `*.py` | **CRITICAL**: Blocks ALL Python files |
| **Various AI assistants** | Various patterns | May add broad patterns |
| **IDE plugins** | Language-specific patterns | Varies |

## Dangerous Patterns

### Critical (Breaks Tach Completely)

| Pattern | Effect | Severity |
|---------|--------|----------|
| `*.py` | Blocks all Python files | CRITICAL |
| `test*.py` | Blocks all test files | CRITICAL |
| `*test*.py` | Blocks test files | CRITICAL |
| `tests/` | Blocks test directory | CRITICAL |
| `test/` | Blocks test directory | CRITICAL |
| `**/test_*.py` | Blocks test files recursively | CRITICAL |

### High Risk (May Break Discovery)

| Pattern | Effect | Severity |
|---------|--------|----------|
| `conftest.py` | Blocks pytest fixtures | HIGH |
| `**/conftest.py` | Blocks all conftest files | HIGH |
| `src/` + `tests/` combo | May orphan test files | HIGH |
| `*.pyc` | Generally safe, but watch for typos | LOW |

### Safe Patterns (No Impact)

| Pattern | Purpose | Safe? |
|---------|---------|-------|
| `__pycache__/` | Python bytecode cache | Yes |
| `*.pyc` | Compiled Python | Yes |
| `.venv/` | Virtual environment | Yes |
| `venv/` | Virtual environment | Yes |
| `node_modules/` | Node.js dependencies | Yes |
| `target/` | Rust build artifacts | Yes |
| `.tach/` | Tach cache directory | Yes |
| `.git/` | Git metadata | Yes |

## Current Tach-Core `.ignore` File

The current `.ignore` file in tach-core is safe:

```
/target
fuzz/target/
/.venv
__pycache__/
.tach/

# Git worktrees
.worktrees/
```

All patterns target build artifacts or caches, not source files.

## The Claude Code Incident

### What Happened

Claude Code (Anthropic's AI coding assistant) adds `*.py` to `.ignore` to prevent itself from recursively processing Python files during certain operations. This pattern is inherited by all tools using the `ignore` crate.

### Impact on Tach

When `*.py` is in `.ignore`:
1. `WalkBuilder` skips ALL Python files
2. `discover()` returns zero test modules
3. Tach reports "no tests found" with exit code 0
4. Users see no error, just empty results

This is particularly insidious because:
- No error message is displayed
- Exit code is 0 (success)
- Users may think they have no tests

### Detection

Currently, tach-core does NOT detect this condition. The discovery silently returns an empty result.

## Recommended Safeguards

### 1. Add `--no-ignore` CLI Flag (Recommended)

Add a CLI flag to bypass ignore files:

```rust
// In config.rs
#[arg(long)]
pub no_ignore: bool,

// In scanner.rs
WalkBuilder::new(&canonical_root)
    .standard_filters(!config.no_ignore)  // Disable when flag is set
```

**Pros:**
- Simple to implement
- Follows ripgrep/fd conventions
- User has explicit control

**Cons:**
- User must know to use the flag
- Doesn't solve the detection problem

### 2. Detect Dangerous Patterns (Recommended)

Scan `.ignore` for patterns that would block Python files:

```rust
fn check_ignore_file(root: &Path) -> Option<Vec<String>> {
    let ignore_path = root.join(".ignore");
    if !ignore_path.exists() {
        return None;
    }

    let content = std::fs::read_to_string(&ignore_path).ok()?;
    let dangerous: Vec<String> = content
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .filter(|line| {
            line.contains("*.py") ||
            line.contains("test") ||
            line.contains("conftest")
        })
        .map(|s| s.to_string())
        .collect();

    if dangerous.is_empty() {
        None
    } else {
        Some(dangerous)
    }
}
```

**Pros:**
- Proactive warning to users
- Explains why no tests are found
- Can suggest fixes

**Cons:**
- May have false positives
- Adds startup overhead

### 3. Use Custom `.tachignore` (Alternative)

Create a tach-specific ignore file:

```rust
WalkBuilder::new(&canonical_root)
    .standard_filters(false)  // Don't use .ignore/.gitignore
    .add_custom_ignore_filename(".tachignore")  // Use our own
```

**Pros:**
- Complete isolation from other tools
- No conflict possible

**Cons:**
- Users must duplicate ignore patterns
- Breaks convention
- More cognitive overhead

### 4. Add Negation Patterns to Default Behavior (Not Recommended)

Force-include Python files regardless of ignore:

```rust
WalkBuilder::new(&canonical_root)
    .standard_filters(true)
    .add("!*.py")  // Force include Python
    .add("!**/test_*.py")
```

**Pros:**
- Automatic fix

**Cons:**
- May include files user intentionally ignored
- Breaks user expectations
- Could include vendor/third-party code

### 5. Warn on Zero Tests Found (Recommended)

When discovery returns no tests, check for potential causes:

```rust
if discovery_result.test_count() == 0 {
    // Check .ignore for dangerous patterns
    if let Some(patterns) = check_ignore_file(&cwd) {
        eprintln!("[tach:warning] No tests found!");
        eprintln!("[tach:warning] The following patterns in .ignore may be blocking Python files:");
        for pattern in patterns {
            eprintln!("  - {}", pattern);
        }
        eprintln!("[tach:warning] Try running with --no-ignore or remove these patterns");
    }
}
```

## Implementation Recommendation

Implement safeguards in this order:

### Phase 1: Detection and Warning
1. Add check for dangerous patterns in `.ignore`
2. Warn user when zero tests found AND dangerous patterns exist
3. Suggest `--no-ignore` as workaround

### Phase 2: User Control
1. Add `--no-ignore` CLI flag
2. Add `TACH_NO_IGNORE=1` environment variable
3. Add `[tool.tach] ignore_files = false` config option

### Phase 3: Documentation
1. Document the `.ignore` interaction in troubleshooting
2. Add FAQ entry for "no tests found"
3. Document which tools may conflict

## Best Practices for Users

### If You Use Claude Code or Similar AI Tools

1. Check your `.ignore` file regularly
2. Never add `*.py` to `.ignore` unless you understand the consequences
3. Use more specific patterns: `generated/*.py` instead of `*.py`
4. If AI adds broad patterns, immediately revert or narrow them

### Recommended `.ignore` Contents

```gitignore
# Safe patterns - won't break tach-core
__pycache__/
*.pyc
*.pyo
*.pyd
.pytest_cache/
.mypy_cache/
.ruff_cache/
.coverage
htmlcov/
*.egg-info/
dist/
build/
.venv/
venv/
env/
.env/
node_modules/
target/
.tach/
.worktrees/

# DANGER: These patterns break tach-core discovery
# *.py           # DON'T DO THIS
# test*.py       # DON'T DO THIS
# tests/         # DON'T DO THIS
```

### If Tach Reports No Tests

1. Check if `.ignore` exists: `cat .ignore`
2. Look for `*.py` or `test` patterns
3. Either remove the pattern or run: `tach --no-ignore`
4. Report the issue to the tool that added the pattern

## Related Issues

- Discovery returns 0 tests when `*.py` in `.ignore`
- No warning when dangerous patterns detected
- No CLI flag to bypass ignore files

## References

- [ignore crate documentation](https://docs.rs/ignore)
- [ripgrep ignore file format](https://github.com/BurntSushi/ripgrep/blob/master/GUIDE.md#configuration)
- [gitignore pattern format](https://git-scm.com/docs/gitignore)
- Claude Code behavior documentation (internal)

## Appendix: Ignore Crate Internals

### WalkBuilder Configuration Options

```rust
let walker = WalkBuilder::new(path)
    // Filter controls
    .standard_filters(true)      // Enable all standard filters
    .hidden(true)                // Skip hidden files
    .parents(true)               // Check parent directories for ignore files
    .git_ignore(true)            // Respect .gitignore
    .git_global(true)            // Respect global gitignore
    .git_exclude(true)           // Respect .git/info/exclude
    .ignore(true)                // Respect .ignore files

    // Custom patterns
    .add_custom_ignore_filename(".customignore")

    // Performance
    .threads(num_cpus::get())    // Parallel walking
    .follow_links(true)          // Follow symlinks

    .build();
```

### Override Priority

When a file matches multiple patterns, the most specific wins:
1. Negation (`!pattern`) always has highest priority at its level
2. Child directories override parent directories
3. `.ignore` overrides `.gitignore` at the same level
4. Later patterns in the same file override earlier ones

### Example Conflict Resolution

```gitignore
# .gitignore
*.py           # Ignore all Python

# .ignore (OVERRIDES .gitignore)
!test_*.py     # But keep test files
```

In this case, `test_*.py` files would be INCLUDED because `.ignore` has higher priority and contains a negation.

However:
```gitignore
# .ignore
*.py           # Ignore ALL Python - nothing can override this
```

This blocks everything because there's no negation.
