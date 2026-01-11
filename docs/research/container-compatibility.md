# Container Compatibility: Docker and Sandbox Behavior

> **Status**: Research Document
> **Created**: 2026-01-11
> **Related**: Iron Dome sandbox, Landlock, Seccomp

---

## Executive Summary

Tach-core's sandbox tests (`test_fs_destruction.py`) behave differently depending on how they are executed. When running **directly via pytest** in a privileged container, the tests **fail** because there's no sandbox. When running **through tach-core**, the Iron Dome sandbox provides filesystem protection and the tests **pass**.

---

## Understanding the Test Failures

### The 5 Failing Tests in `test_fs_destruction.py`

| Test | Purpose | Why It Fails in Privileged Container |
|------|---------|-------------------------------------|
| `test_fs_destruction` | Verify /etc/passwd is read-only | Container root can write to /etc |
| `test_symlink_escape_prevention` | Block symlink escapes to /etc/shadow | Container can read /etc/shadow |
| `test_proc_self_protection` | Verify /proc is read-only | Container can write to /proc |
| `test_etc_readonly` | Verify /etc is read-only | Container root has full access |
| `test_usr_readonly` | Verify /usr is read-only | Container root has full access |

### Root Cause

These tests are designed to verify that tach-core's **Iron Dome sandbox** protects the filesystem. When running:

1. **`pytest tests/gauntlet/test_fs_destruction.py`** - No sandbox is applied. The tests run as root in a privileged container, which can write anywhere. Tests fail because they *expect* protection that isn't there.

2. **`./target/release/tach-core tests/gauntlet/test_fs_destruction.py`** - Tach-core applies Iron Dome (Landlock + Seccomp) before running tests. The sandbox blocks writes to /etc, /usr, /proc. Tests pass because protection is working.

---

## Iron Dome Sandbox Architecture

### What Iron Dome Provides

| Protection | Technology | Effect |
|------------|------------|--------|
| Filesystem isolation | Landlock | Blocks writes to /etc, /usr, /bin, /lib, /sbin |
| System call filtering | Seccomp | Blocks dangerous syscalls (execve, fork in safe mode) |
| Overlay filesystem | mount namespaces | /tmp and CWD are writable overlays |
| Process isolation | PID namespace | Tests can't see other processes |

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

| Configuration | Landlock | Seccomp | userfaultfd | Sandbox Tests | Notes |
|---------------|----------|---------|-------------|---------------|-------|
| **Docker default** | ❌ | ✅ | ❌ | N/A | Most features disabled |
| **Docker privileged** | ✅ | ✅ | ✅ | ⚠️ See below | Full kernel access |
| **Docker + caps** | ✅ | ✅ | ✅ | ⚠️ See below | Recommended |
| **Podman rootless** | ❌ | ❌ | ❌ | N/A | User namespaces block kernel features |
| **Kubernetes Pod** | Varies | Varies | ❌ | N/A | Depends on SecurityContext |
| **Native Linux** | ✅ | ✅ | ✅ | ✅ | Full support |

### Key Insight

Sandbox tests behave differently based on **execution method**, not container type:

| Execution Method | Result | Reason |
|------------------|--------|--------|
| `pytest tests/gauntlet/test_fs_destruction.py` | FAIL | No sandbox applied |
| `./target/release/tach-core tests/gauntlet/test_fs_destruction.py` | PASS | Iron Dome protects filesystem |

---

## Docker Configuration Requirements

### Recommended docker-compose.yml

```yaml
services:
  dev:
    build: .
    privileged: true  # Required for userfaultfd, Landlock, namespaces

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

| Capability | Feature Enabled | Required For |
|------------|-----------------|--------------|
| `SYS_PTRACE` | userfaultfd | Memory snapshots |
| `SYS_ADMIN` | namespaces | PID/mount isolation |
| `privileged: true` | All of above | Simplest configuration |

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

| Test Suite | Via pytest | Via tach-core |
|------------|-----------|---------------|
| `test_fs_destruction.py` | 5 FAIL | 5 PASS |
| Other gauntlet tests | 21 PASS | 21 PASS |
| Rust unit tests | N/A | 695 PASS |
| Rust integration tests | N/A | 298 PASS |

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
