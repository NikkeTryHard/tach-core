# Tach Error Reference

This document provides a comprehensive reference for all Tach error codes, their causes, and remediation steps.

## Error Code Overview

Error codes follow the pattern `EXXX` where:

- **E001-E004, E010, E012**: User errors (test code, configuration, Python version)
- **E005-E009, E011, E013-E016**: System errors (kernel, permissions, resources)
- **E017-E020**: Extended user errors (syntax, fixtures, test status)

## User Errors

### E001: Test Assertion Failed

**Category:** User

**Cause:** A test assertion failed during execution. The test's expected outcome did not match the actual result.

**Solution:**

1. Review the test assertion and expected values
2. Check if the code under test has changed
3. Verify test data and fixtures are correct

---

### E002: Import Error

**Category:** User

**Cause:** Failed to import a module in a test file. This could be a missing dependency or incorrect import path.

**Solution:**

1. Ensure the module is installed: `pip install <module>`
2. Verify the import path is correct
3. Check for circular imports
4. Ensure `PYTHONPATH` is set correctly

---

### E003: Fixture Not Found

**Category:** User

**Cause:** A test requests a fixture that does not exist or is not accessible.

**Solution:**

1. Define the fixture in `conftest.py` or the test file
2. Check for typos in the fixture name
3. Ensure conftest.py is in the correct directory
4. Verify fixture scope is appropriate

---

### E004: Invalid Marker Expression

**Category:** User

**Cause:** The marker expression passed via `-m` flag has invalid syntax.

**Solution:**

1. Check marker syntax: `-m "slow and not integration"`
2. Use proper boolean operators: `and`, `or`, `not`
3. Ensure marker names are valid identifiers

---

### E010: Timeout Exceeded

**Category:** User

**Cause:** A test exceeded the configured timeout limit.

**Solution:**

1. Increase timeout: `@pytest.mark.timeout(N)` on the test
2. Increase global timeout: `--timeout N` CLI flag
3. Optimize the test for better performance
4. Check for infinite loops or deadlocks

---

### E012: Python Version Mismatch

**Category:** User

**Cause:** The Python binary used does not match the expected version.

**Solution:**

1. Set `PYO3_PYTHON` to the correct Python binary path
2. Verify Python version: `python --version`
3. Create a virtual environment with the correct version

---

### E017: Syntax Error in Test File

**Category:** User

**Cause:** A Python syntax error was found in a test file.

**Solution:**

1. Run `python -m py_compile <file>` to locate the error
2. Fix the syntax error at the indicated line
3. Check for missing colons, brackets, or indentation issues

---

### E018: Circular Fixture Dependency

**Category:** User

**Cause:** Fixtures have circular dependencies that cannot be resolved.

**Solution:**

1. Review fixture dependency graph
2. Refactor fixtures to break the cycle
3. Use factory patterns to defer fixture creation
4. Consider using fixture scopes to avoid the cycle

---

### E019: Skipped Test

**Category:** User (Informational)

**Cause:** A test was skipped due to a skip marker or condition.

**Note:** This is informational, not an error. The test was intentionally skipped.

---

### E020: Expected Failure (Xfail)

**Category:** User (Informational)

**Cause:** A test is marked as expected to fail (`@pytest.mark.xfail`).

**Note:** This is informational, not an error. The test is known to fail and tracked.

---

## System Errors

### E005: userfaultfd Not Available

**Category:** System

**Cause:** The userfaultfd system call is not available. This is required for Tach's memory snapshot feature.

**Solution:**

1. Enable unprivileged userfaultfd:
   ```bash
   sudo sysctl -w vm.unprivileged_userfaultfd=1
   ```
2. Make it persistent by adding to `/etc/sysctl.conf`:
   ```
   vm.unprivileged_userfaultfd=1
   ```
3. Alternatively, run with `CAP_SYS_PTRACE`:
   ```bash
   sudo setcap cap_sys_ptrace+ep ./tach-core
   ```

---

### E006: Landlock Not Supported

**Category:** System

**Cause:** Landlock filesystem sandboxing is not available. Requires Linux kernel 5.13+.

**Solution:**

1. Upgrade to Linux kernel 5.13 or later
2. Tach will run with degraded filesystem isolation
3. Check kernel config: `CONFIG_SECURITY_LANDLOCK=y`

---

### E007: Permission Denied

**Category:** System

**Cause:** An operation was denied due to insufficient permissions.

**Solution:**

1. Check file and directory permissions
2. Run with elevated privileges if necessary
3. In containers, use `--privileged` flag
4. Check SELinux/AppArmor policies

---

### E008: Out of Memory

**Category:** System

**Cause:** System ran out of memory during test execution.

**Solution:**

1. Reduce worker count: `-n 2`
2. Increase system memory or swap
3. Check for memory leaks in tests
4. Use `--force-toxic` to reduce snapshot memory usage

---

### E009: Too Many Open Files

**Category:** System

**Cause:** The process exceeded the file descriptor limit.

**Solution:**

1. Increase file descriptor limit:
   ```bash
   ulimit -n 65536
   ```
2. Make permanent in `/etc/security/limits.conf`:
   ```
   * soft nofile 65536
   * hard nofile 65536
   ```
3. Reduce worker count to use fewer file descriptors

---

### E011: OverlayFS Mount Failed

**Category:** System

**Cause:** Failed to mount an OverlayFS filesystem for test isolation.

**Solution:**

1. Ensure the overlayfs kernel module is loaded:
   ```bash
   sudo modprobe overlay
   ```
2. Check mount permissions
3. Verify the work directory supports overlayfs

---

### E013: Namespace Creation Failed

**Category:** System

**Cause:** Failed to create a Linux namespace for process isolation.

**Solution:**

1. Check kernel configuration for namespace support
2. Run with `CAP_SYS_ADMIN`:
   ```bash
   sudo setcap cap_sys_admin+ep ./tach-core
   ```
3. In Docker, use `--privileged` or specific capability flags

---

### E014: Worker Crash

**Category:** System

**Cause:** A worker process crashed with a signal (SIGSEGV, SIGBUS, etc.).

**Solution:**

1. Check for memory corruption in C extensions
2. Increase stack size: `ulimit -s unlimited`
3. Run with `--force-toxic` to isolate problematic tests
4. Check for segfault-causing code in tests

---

### E015: IPC Channel Failure

**Category:** System

**Cause:** Communication between supervisor and worker failed.

**Solution:**

1. Check system resources (memory, file descriptors)
2. Reduce worker count: `-n 2`
3. Check for worker crashes in logs
4. Ensure `/dev/shm` has sufficient space

---

### E016: Snapshot Integrity Failure

**Category:** System

**Cause:** Memory snapshot verification failed, indicating corruption.

**Solution:**

1. This is an internal error - please report a bug
2. Try running with `--force-toxic` as a workaround
3. Check for memory-corrupting C extensions
4. Verify system memory is healthy: `memtest86+`

---

## Diagnostic Commands

### Check System Compatibility

```bash
tach self-test
```

### Run with Maximum Verbosity

```bash
tach --debug --trace tests/
```

### Run Comprehensive Diagnostics

```bash
tach --diagnose
```

## See Also

- [Configuration Reference](configuration.md)
- [Troubleshooting Guide](troubleshooting.md)
- [Development Guide](development.md)
