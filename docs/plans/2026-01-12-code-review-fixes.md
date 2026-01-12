# Code Review Fixes Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Address all issues identified in the code review of the neglected-issues fix.

**Architecture:** Add missing tests for security fixes, improve test reliability, and clean up stale files.

**Tech Stack:** Rust, Python, Markdown

---

## Investigation Results

### /run/tach Directory Creation (NOT A BUG)

Investigation confirmed the correct sequence:

1. `setup_filesystem()` creates `/run/tach/worker_{id}` using `fs::create_dir_all()`
2. `apply_landlock()` is called AFTER, with rules for the existing directories

No fix needed.

---

## Task 1: Add Device Node Blocking Test (Critical)

**Files:**

- Modify: `rust_tests/sandbox_enforcement.rs`

**Issue:** The security fix removing MAKE_CHAR/MAKE_BLOCK from project_root is untested.

**Step 1:** Read `rust_tests/sandbox_enforcement.rs` to understand the "suicide worker" pattern

**Step 2:** Add test using libc::mknod to verify device creation is blocked

```rust
#[test]
fn test_landlock_blocks_mknod_in_project_root() {
    match unsafe { fork() }.expect("fork failed") {
        ForkResult::Child => {
            let project_root = std::env::current_dir().expect("Failed to get cwd");

            // Apply Landlock - project_root gets safe_write_access (no MakeChar/MakeBlock)
            match apply_landlock(&project_root, 9999) {
                Ok(SandboxStatus::NotEnforced) => std::process::exit(0), // Skip if no Landlock
                Ok(_) => {}
                Err(_) => std::process::exit(254),
            }

            // Attempt to create a character device in project root
            let path = project_root.join("test_dev_node");
            let c_path = std::ffi::CString::new(path.to_str().unwrap()).unwrap();

            // S_IFCHR is character device, makedev(1, 3) is /dev/null
            let dev = unsafe { libc::makedev(1, 3) };
            let mode = libc::S_IFCHR | 0o666;

            let result = unsafe { libc::mknod(c_path.as_ptr(), mode, dev) };

            if result == -1 {
                let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
                if errno == libc::EACCES {
                    std::process::exit(0); // SUCCESS: Blocked by Landlock
                }
                std::process::exit(errno);
            } else {
                // SECURITY FAILURE: mknod succeeded!
                let _ = std::fs::remove_file(path);
                std::process::exit(255);
            }
        }
        ForkResult::Parent { child } => match waitpid(child, None).expect("waitpid failed") {
            WaitStatus::Exited(_, code) => {
                assert_eq!(code, 0, "mknod should be blocked with EACCES (exit 0)");
            }
            status => panic!("Child process did not exit normally: {:?}", status),
        },
    }
}
```

**Step 3:** Run the test

```bash
cargo test --test sandbox_enforcement -- test_landlock_blocks_mknod
```

**Step 4:** Commit

```bash
git add rust_tests/sandbox_enforcement.rs
git commit -m "test: add mknod blocking test for Landlock security fix

Verifies that MAKE_CHAR and MAKE_BLOCK are blocked in project_root
to prevent device node creation escape attacks."
```

---

## Task 2: Improve Symlink Test Fallback Targets (Important)

**Files:**

- Modify: `tests/gauntlet/test_fs_destruction.py`

**Issue:** Current fallbacks only use `/root/*` files which may not exist in minimal containers.

**Step 1:** Read current test implementation

**Step 2:** Change fallback targets to include system files that always exist but are protected:

```python
def test_symlink_escape_prevention():
    """Verify symlink traversal cannot escape sandbox."""
    # Try multiple targets - prefer /root (not allowed by Landlock)
    # Fall back to system files that exist but should be protected
    targets = [
        "/root/.bashrc",      # Common in containers (not in Landlock allow list)
        "/root/.profile",     # Alternative shell config
        "/etc/shadow",        # Always exists, should be read-protected
        "/etc/gshadow",       # Always exists, should be read-protected
    ]

    target = None
    for t in targets:
        if os.path.exists(t):
            target = t
            break

    if target is None:
        pytest.skip("No suitable symlink target found")

    # Rest of test...
```

**Step 3:** Run test to verify

```bash
./target/release/tach-core tests/gauntlet/test_fs_destruction.py::test_symlink_escape_prevention -v
```

**Step 4:** Commit

```bash
git add tests/gauntlet/test_fs_destruction.py
git commit -m "test: add /etc/shadow fallback to symlink escape test

Ensures test runs in minimal containers where /root files may not exist."
```

---

## Task 3: Add Proactive Pattern Warning Test (Important)

**Files:**

- Modify: `rust_tests/discovery_integration.rs`

**Issue:** The NOTE-level warning when some tests are found is untested.

**Step 1:** Read existing pattern detection tests in `rust_tests/discovery_integration.rs`

**Step 2:** Add test that verifies NOTE-level warning logic

Since the warning is printed to stderr (in main.rs binary), we can't directly test the output.
However, we can test the underlying logic by verifying that `detect_blocking_patterns` is called
regardless of whether tests are found.

Add a test that verifies the detection function works correctly:

```rust
#[test]
fn test_detect_blocking_patterns_with_partial_block() {
    // Setup: Create a directory with one test file and a pattern that would block others
    let dir = tempfile::tempdir().unwrap();

    // Create a test file that won't be blocked
    std::fs::write(dir.path().join("test_valid.py"), "def test_pass(): pass").unwrap();

    // Create .ignore that blocks a pattern (but not test_valid.py)
    std::fs::write(dir.path().join(".ignore"), "test_blocked_*.py").unwrap();

    // Verify detection finds the blocking pattern
    let patterns = tach_core::discovery::detect_blocking_patterns(dir.path());
    assert!(!patterns.is_empty(), "Should detect blocking pattern");
    assert!(patterns.iter().any(|p| p.contains("test_blocked")));

    // Verify discovery still finds the valid test
    let result = tach_core::discovery::discover(dir.path(), false).unwrap();
    assert!(!result.modules.is_empty(), "Should find test_valid.py");
}
```

**Step 3:** Run tests

```bash
cargo test --test discovery_integration -- test_detect_blocking_patterns
```

**Step 4:** Commit

```bash
git add rust_tests/discovery_integration.rs
git commit -m "test: add partial blocking pattern detection test

Verifies that blocking patterns are detected even when some tests are found."
```

---

## Task 4: Delete Completed Plan File (Important)

**Files:**

- Delete: `docs/plans/2026-01-11-fix-neglected-issues.md`

**Step 1:** Delete the completed plan

```bash
rm docs/plans/2026-01-11-fix-neglected-issues.md
```

**Step 2:** Commit

```bash
git add -A
git commit -m "chore: delete completed fix-neglected-issues plan"
```

---

## Task 5: Fix Minor Issues (Optional)

**Files:**

- Modify: `src/isolation/sandbox.rs` (comment improvement)
- Modify: `src/main.rs` (grammar fix)

**Step 1:** Improve comment in sandbox.rs

Change line ~184 from:

```rust
// Excluded: MAKE_CHAR, MAKE_BLOCK, MAKE_FIFO, MAKE_SOCK
```

To:

```rust
// Safe write operations only - device creation (MAKE_CHAR, MAKE_BLOCK) and
// IPC creation (MAKE_FIFO, MAKE_SOCK) are intentionally omitted for security.
```

**Step 2:** Improve warning message grammar in main.rs

Change line ~676 from:

```rust
eprintln!("[tach:discovery] NOTE: These patterns in .ignore may be blocking some tests:");
```

To:

```rust
eprintln!("[tach:discovery] NOTE: These patterns in .ignore may be hiding some tests:");
```

**Step 3:** Commit

```bash
git add src/isolation/sandbox.rs src/main.rs
git commit -m "docs: improve security comment and warning message clarity"
```

---

## Task 6: Final Verification

**Step 1:** Run all tests

```bash
cargo test --lib
cargo test --test sandbox_enforcement
cargo test --test discovery_integration
```

**Step 2:** Verify no stale plan files

```bash
ls docs/plans/
```

Expected: Only `2026-01-12-code-review-fixes.md` (this plan)

---

## Summary

| Task | Description                       | Priority     |
| ---- | --------------------------------- | ------------ |
| 1    | Add device node blocking test     | **Critical** |
| 2    | Improve symlink test fallbacks    | Important    |
| 3    | Add partial blocking pattern test | Important    |
| 4    | Delete completed plan file        | Important    |
| 5    | Fix minor comment/grammar issues  | Optional     |
| 6    | Final verification                | Required     |

**Expected Results:**

- Security fix for device node blocking is now tested
- Tests are more robust across different environments
- Stale plan files cleaned up
- Code quality improvements
