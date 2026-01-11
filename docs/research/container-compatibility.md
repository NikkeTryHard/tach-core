# Container Compatibility: Docker and Sandbox Behavior

> **Status**: Research Document (Updated with empirical testing)
> **Created**: 2026-01-11
> **Updated**: 2026-01-11
> **Related**: Iron Dome sandbox, Landlock, Seccomp

---

## Executive Summary

Tach-core's sandbox tests (`test_fs_destruction.py`) behave differently depending on how they are executed. When running **directly via pytest** in a privileged container, the tests **fail** because there's no sandbox. When running **through tach-core**, the Iron Dome sandbox provides filesystem protection.

**Critical Finding:** Even with tach-core, only 3 of 5 tests pass in containers. Two tests fail due to:

1. **Landlock configuration**: Project root is set to read-only, preventing CWD writes
2. **Root user context**: Container root can read /etc/shadow via symlink

**Required Dependency:** The Docker container must have `iproute2` installed for network namespace configuration.

---

## Empirical Test Results (2026-01-11)

### Test Environment

| Property       | Value                            |
| -------------- | -------------------------------- |
| Container      | `tach-dev:latest` (Ubuntu 24.04) |
| Kernel         | 6.6.87.2-microsoft-standard-WSL2 |
| Container Mode | Privileged                       |
| User           | root (uid=0)                     |
| Landlock ABI   | v4                               |
| userfaultfd    | Enabled                          |

### Actual Test Results

| Execution Method                  | Passed | Failed | Notes                 |
| --------------------------------- | ------ | ------ | --------------------- |
| `pytest` (direct)                 | 0      | 5      | No sandbox - expected |
| `tach-core` (no iproute2)         | 0      | 5      | Isolation crashes     |
| `tach-core` (with iproute2)       | 3      | 2      | Partial sandbox       |
| `tach-core` (TACH_NO_ISOLATION=1) | 3      | 2      | Same as above         |

### Individual Test Results with tach-core

| Test                             | Result | Reason                                |
| -------------------------------- | ------ | ------------------------------------- |
| `test_proc_self_protection`      | PASS   | Sandbox protects /proc                |
| `test_etc_readonly`              | PASS   | Sandbox protects /etc                 |
| `test_usr_readonly`              | PASS   | Sandbox protects /usr                 |
| `test_fs_destruction`            | FAIL   | CWD not writable (Landlock restricts) |
| `test_symlink_escape_prevention` | FAIL   | Root can read /etc/shadow             |

### Missing Dependency: iproute2

Without `iproute2` installed, tach-core fails with:

```
[tach:worker] CRITICAL: Isolation failed. Aborting to protect host.
Error: Failed to configure loopback interface: Failed to execute 'ip' command - is iproute2 installed?
```

**Fix:** Add to Dockerfile:

```dockerfile
RUN apt-get update && apt-get install -y iproute2
```

---

## Understanding the Test Failures

### The 5 Failing Tests in `test_fs_destruction.py`

| Test                             | Purpose                              | Why It Fails in Privileged Container |
| -------------------------------- | ------------------------------------ | ------------------------------------ |
| `test_fs_destruction`            | Verify /etc/passwd is read-only      | Container root can write to /etc     |
| `test_symlink_escape_prevention` | Block symlink escapes to /etc/shadow | Container can read /etc/shadow       |
| `test_proc_self_protection`      | Verify /proc is read-only            | Container can write to /proc         |
| `test_etc_readonly`              | Verify /etc is read-only             | Container root has full access       |
| `test_usr_readonly`              | Verify /usr is read-only             | Container root has full access       |

### Root Cause

These tests are designed to verify that tach-core's **Iron Dome sandbox** protects the filesystem. When running:

1. **`pytest tests/gauntlet/test_fs_destruction.py`** - No sandbox is applied. The tests run as root in a privileged container, which can write anywhere. Tests fail because they _expect_ protection that isn't there.

2. **`./target/release/tach-core tests/gauntlet/test_fs_destruction.py`** - Tach-core applies Iron Dome (Landlock + Seccomp) before running tests. The sandbox blocks writes to /etc, /usr, /proc. However, two tests still fail in containers (see below).

### Root Cause: Two Remaining Failures

#### 1. `test_fs_destruction` - CWD Not Writable

The test expects the project root (CWD) to be writable via the overlay filesystem. However, Landlock is configured to allow only **read access** to project_root:

```rust
// src/isolation/sandbox.rs line 208
let ruleset = add_path_rule(ruleset, &project_root, read_access)?;  // READ-ONLY
```

The overlay mount (namespace.rs) makes the directory writable at the filesystem level, but Landlock's access control overrides this. This is a **configuration mismatch** between the overlay expectation and Landlock policy.

**Potential fixes:**

1. Change Landlock to allow write access to project_root
2. Update test to expect CWD as read-only
3. Accept that tests should write to /tmp instead

#### 2. `test_symlink_escape_prevention` - Root Can Read /etc/shadow

The test creates a symlink to /etc/shadow and expects the read to fail. However:

1. Container runs as **root** (uid=0)
2. /etc/shadow has permissions `-rw-r----- root shadow`
3. Root can read /etc/shadow regardless of symlink
4. Landlock allows read access to /etc (required for Python, SSL certs, etc.)

**This is expected behavior for privileged containers.** The test assumes a non-root context.

**Potential fixes:**

1. Run test worker as non-root user (drop privileges after fork)
2. Mark this test as expected-to-fail in privileged containers
3. Accept that symlink protection only works with non-root users

---

## Iron Dome Sandbox Architecture

### What Iron Dome Provides

| Protection            | Technology       | Effect                                                |
| --------------------- | ---------------- | ----------------------------------------------------- |
| Filesystem isolation  | Landlock         | Blocks writes to /etc, /usr, /bin, /lib, /sbin        |
| System call filtering | Seccomp          | Blocks dangerous syscalls (execve, fork in safe mode) |
| Overlay filesystem    | mount namespaces | /tmp and CWD are writable overlays                    |
| Process isolation     | PID namespace    | Tests can't see other processes                       |

### Landlock Rules Applied

```rust
// Paths marked read-only (from sandbox.rs)
"/",           // Root filesystem
"/etc",        // System configuration
"/usr",        // System binaries and libraries
"/bin",        // Essential binaries
"/sbin",       // System binaries
"/lib",        // Libraries
"/lib64",      // 64-bit libraries

// Paths marked read-write (overlay)
"/tmp",        // Temporary files
"/workspace",  // Project directory (CWD)
```

### Why Seccomp Isn't Enough

Seccomp blocks syscalls but doesn't restrict file paths. A process could still call `open()` on `/etc/passwd` - Seccomp can't distinguish between opening `/tmp/foo` and `/etc/passwd`. Landlock provides path-based restrictions.

---

## Container Compatibility Matrix

| Configuration         | Landlock | Seccomp | userfaultfd | Sandbox Tests | Notes                                 |
| --------------------- | -------- | ------- | ----------- | ------------- | ------------------------------------- |
| **Docker default**    | ❌       | ✅      | ❌          | N/A           | Most features disabled                |
| **Docker privileged** | ✅       | ✅      | ✅          | ⚠️ See below  | Full kernel access                    |
| **Docker + caps**     | ✅       | ✅      | ✅          | ⚠️ See below  | Recommended                           |
| **Podman rootless**   | ❌       | ❌      | ❌          | N/A           | User namespaces block kernel features |
| **Kubernetes Pod**    | Varies   | Varies  | ❌          | N/A           | Depends on SecurityContext            |
| **Native Linux**      | ✅       | ✅      | ✅          | ✅            | Full support                          |

### Key Insight

Sandbox tests behave differently based on **execution method**, not container type:

| Execution Method                                                   | Result | Reason                        |
| ------------------------------------------------------------------ | ------ | ----------------------------- |
| `pytest tests/gauntlet/test_fs_destruction.py`                     | FAIL   | No sandbox applied            |
| `./target/release/tach-core tests/gauntlet/test_fs_destruction.py` | PASS   | Iron Dome protects filesystem |

---

## Docker Configuration Requirements

### Recommended docker-compose.yml

```yaml
services:
  dev:
    build: .
    privileged: true # Required for userfaultfd, Landlock, namespaces

    # Alternative: specific capabilities instead of privileged
    # cap_add:
    #   - SYS_PTRACE    # For userfaultfd
    #   - SYS_ADMIN     # For namespaces and mounts
    #   - NET_ADMIN     # For network namespaces (optional)
    # security_opt:
    #   - seccomp:unconfined  # Required if using custom seccomp

    volumes:
      - .:/workspace
    working_dir: /workspace
```

### Capability Requirements

| Capability         | Feature Enabled | Required For           |
| ------------------ | --------------- | ---------------------- |
| `SYS_PTRACE`       | userfaultfd     | Memory snapshots       |
| `SYS_ADMIN`        | namespaces      | PID/mount isolation    |
| `privileged: true` | All of above    | Simplest configuration |

### Kernel Feature Verification

Run inside container to verify features:

```bash
./target/release/tach-core self-test
```

Expected output (privileged container):

```
Kernel version .............. PASS (6.6.87 >= 5.13)
Landlock support ............ PASS (ABI v4)
Seccomp BPF support ......... PASS
userfaultfd support ......... PASS
Clone3 syscall .............. PASS
PID namespace ............... PASS
Mount namespace ............. PASS
Network namespace ........... PASS
```

---

## Running Tests Correctly

### For Sandbox Verification Tests

These tests should be run **through tach-core**:

```bash
# Inside Docker container
./target/release/tach-core tests/gauntlet/test_fs_destruction.py
```

### For All Other Tests

Standard pytest execution works:

```bash
# Inside Docker container
source .venv/bin/activate
pytest tests/gauntlet/ -v
```

### Expected Results Summary

| Test Suite               | Via pytest | Via tach-core                          |
| ------------------------ | ---------- | -------------------------------------- |
| `test_fs_destruction.py` | 5 FAIL     | 3 PASS, 2 FAIL (see Empirical Results) |
| Other gauntlet tests     | Varies     | Varies                                 |
| Rust unit tests          | N/A        | See `cargo test --lib`                 |
| Rust integration tests   | N/A        | See `cargo test --test '*'`            |

---

## CI/CD Considerations

### GitHub Actions

```yaml
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Run standard tests
        run: |
          cargo test --lib
          cargo test --test '*'

      - name: Run sandbox verification (optional)
        run: |
          # Skip if kernel features unavailable
          ./target/release/tach-core self-test || exit 0
          ./target/release/tach-core tests/gauntlet/test_fs_destruction.py
```

### Self-Hosted Runners

For full sandbox testing, self-hosted runners need:

- Kernel 5.13+ (for Landlock)
- `vm.unprivileged_userfaultfd=1` sysctl
- Docker with privileged access

---

## Troubleshooting

### "Failed to configure loopback interface" / "iproute2 not installed"

**Symptom:**

```
[tach:worker] CRITICAL: Isolation failed. Aborting to protect host.
Error: Failed to configure loopback interface: Failed to execute 'ip' command - is iproute2 installed?
```

**Cause:** The `iproute2` package is not installed in the Docker container. Tach-core needs the `ip` command to configure the loopback interface in network namespaces.

**Solution:** Add `iproute2` to your Dockerfile:

```dockerfile
RUN apt-get update && apt-get install -y iproute2
```

### "Tests fail with 'was writable' errors"

**Symptom:**

```
AssertionError: ERROR: /etc/passwd was writable!
```

**Cause:** Running directly via pytest instead of through tach-core.

**Solution:** Run through tach-core:

```bash
./target/release/tach-core tests/gauntlet/test_fs_destruction.py
```

### "Landlock not available"

**Symptom:**

```
[WARN] Landlock not available: EPERM
```

**Cause:** Kernel < 5.13 or container lacks privileges.

**Solution:** Add `privileged: true` to docker-compose.yml or upgrade kernel.

### "userfaultfd creation failed"

**Symptom:**

```
Error: userfaultfd creation failed: EPERM
```

**Cause:** `vm.unprivileged_userfaultfd=0` on host.

**Solution:**

```bash
# On host (not in container)
sudo sysctl vm.unprivileged_userfaultfd=1
```

---

## Design Decisions

### Why Run Sandbox Tests via Tach-Core?

1. **Purpose:** These tests verify the sandbox works correctly
2. **Expectation:** Sandbox protection should be active when tests run
3. **Method:** Tach-core applies sandbox, then runs test code

Running via pytest would be testing "does Python run?" not "does Iron Dome work?"

### Why Are These Tests in `tests/gauntlet/`?

1. **Gauntlet tests** exercise tach-core's features end-to-end
2. They require tach-core to be built and working
3. They verify security and isolation properties

### Why 5 Failures is Expected Without Sandbox?

The tests are designed to fail if the sandbox isn't protecting them. This is correct behavior - if they passed, it would mean the sandbox tests are broken or meaningless.

---

## References

- [Landlock documentation](https://landlock.io/)
- [Seccomp BPF](https://www.kernel.org/doc/html/latest/userspace-api/seccomp_filter.html)
- [userfaultfd man page](https://man7.org/linux/man-pages/man2/userfaultfd.2.html)
- [Docker capabilities](https://docs.docker.com/engine/reference/run/#runtime-privilege-and-linux-capabilities)
