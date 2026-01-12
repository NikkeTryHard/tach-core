# Troubleshooting Guide

Common issues and solutions for Tach.

---

## Quick Diagnostics

Run these commands to check system compatibility:

```bash
# Kernel version (needs 5.13+ for full features)
uname -r

# Landlock support
cat /sys/kernel/security/lsm | grep landlock

# Seccomp support
grep CONFIG_SECCOMP /boot/config-$(uname -r)

# Python version
python --version

# userfaultfd support
cat /proc/sys/vm/unprivileged_userfaultfd
```

---

## Build Issues

### PYO3_PYTHON Not Set

**Symptom:**

```
error: could not find Python interpreter
```

**Solution:**

```bash
export PYO3_PYTHON=$(which python)
cargo build
```

> **WSL2 Users:** Source `.envrc` to automatically set PYO3_PYTHON:
>
> ```bash
> source .envrc
> ```

### Wrong Python Version

**Symptom:**

```
error: Python 3.10+ required
```

**Solution:**

```bash
# Use specific Python
export PYO3_PYTHON=/usr/bin/python3.12
cargo build

# Or with virtual environment
python3.12 -m venv .venv
source .venv/bin/activate
export PYO3_PYTHON=$(which python)
cargo build
```

### Missing Build Tools

**Symptom:**

```
error: linker `cc` not found
```

**Solution:**

```bash
# Ubuntu/Debian
sudo apt install build-essential

# Fedora
sudo dnf install gcc make

# Arch
sudo pacman -S base-devel
```

### Jemalloc Build Failure

**Symptom:**

```
error: failed to run custom build command for `tikv-jemallocator`
```

**Solution:**

```bash
# Install autoconf
sudo apt install autoconf

# Clean and rebuild
cargo clean
cargo build
```

---

## Runtime Issues

### EPERM on Landlock

**Symptom:**

```
[WARN] Landlock not available: EPERM
```

**Cause:** Kernel < 5.13 or Landlock not enabled.

**Diagnosis:**

```bash
# Check kernel version
uname -r

# Check if Landlock is in LSM list
cat /sys/kernel/security/lsm
```

**Solution:**

- Upgrade kernel to 5.13+
- Or run with `--no-isolation` (reduced security)

### EPERM on Seccomp

**Symptom:**

```
[WARN] Seccomp filter rejected: EPERM
```

**Cause:** Seccomp-BPF not enabled in kernel.

**Diagnosis:**

```bash
grep CONFIG_SECCOMP /boot/config-$(uname -r)
# Should show: CONFIG_SECCOMP=y and CONFIG_SECCOMP_FILTER=y
```

If `/boot/config-*` doesn't exist (common on WSL2 or cloud kernels):

```bash
zgrep CONFIG_SECCOMP /proc/config.gz
```

**Solution:**

- Tach degrades gracefully (Landlock-only mode)
- For full security, enable seccomp in kernel config

### userfaultfd Permission Denied

**Symptom:**

```
Error: userfaultfd creation failed: EPERM
```

**Cause:** Unprivileged userfaultfd disabled.

**Diagnosis:**

```bash
cat /proc/sys/vm/unprivileged_userfaultfd
# 0 = disabled, 1 = enabled
```

**Solution:**

```bash
# Enable temporarily
sudo sysctl vm.unprivileged_userfaultfd=1

# Enable permanently
echo 'vm.unprivileged_userfaultfd=1' | sudo tee /etc/sysctl.d/99-userfaultfd.conf
sudo sysctl --system
```

### Test Hangs

**Symptom:** Tests hang indefinitely without progress.

**Common Causes:**

| Cause                    | Solution                               |
| :----------------------- | :------------------------------------- |
| Clone syscall blocked    | Ensure clone NOT in seccomp filter     |
| Deadlock in test         | Check for lock contention in test code |
| Infinite loop in fixture | Add timeout to fixture                 |
| Network wait in sandbox  | Use `--no-isolation` for network tests |

**Diagnosis:**

```bash
# Check for stuck processes
ps aux | grep tach

# Trace syscalls
strace -f -p <PID> 2>&1 | tail -20

# Check what process is waiting on
cat /proc/<PID>/wchan
```

### Worker Crashes

**Symptom:**

```
CRASH: test_example.py::test_foo
```

**Common Causes:**

| Cause                   | Solution                      |
| :---------------------- | :---------------------------- |
| Segfault in C extension | Check extension compatibility |
| Out of memory           | Increase memory limits        |
| Blocked syscall         | Check seccomp filter          |
| Signal handling         | Test may be catching signals  |

**Diagnosis:**

```bash
# Run with reduced isolation
tach-core --no-isolation tests/

# Check coredump
coredumpctl list
coredumpctl info <PID>
```

### Coverage Data Missing

**Symptom:** Coverage report shows 0% or missing files.

**Common Causes:**

| Cause              | Solution                                    |
| :----------------- | :------------------------------------------ |
| Python < 3.12      | Upgrade Python (PEP 669 required)           |
| Source not in path | Add source to `[tool.tach.coverage].source` |
| Files omitted      | Check `[tool.tach.coverage].omit` patterns  |
| Buffer overflow    | Increase buffer size (rare)                 |

**Diagnosis:**

```bash
# Check Python version
python --version

# Verify coverage enabled
tach-core --coverage . 2>&1 | head -20
```

---

## Test Discovery Issues

### Tests Not Found

**Symptom:**

```
Discovered N tests in M files
```

**Common Causes:**

| Cause                     | Solution                         |
| :------------------------ | :------------------------------- |
| Wrong pattern             | Check `[tool.tach].test_pattern` |
| Syntax error in test file | Fix Python syntax                |
| Non-standard naming       | Rename to `test_*.py`            |
| Wrong directory           | Specify correct path             |

**Diagnosis:**

```bash
# List discovered tests
tach-core list .

# Check for syntax errors
python -m py_compile tests/test_example.py
```

### .ignore File Blocking Python Files

**Symptom:**

```
Discovered N tests, M fixtures
```

Discovery reports zero tests even though test files exist and have valid syntax.

**Cause:**

The `.ignore` file (used by tools like Claude Code for context filtering) may contain a pattern that blocks Python files:

```
*.py
```

Tach uses the `ignore` crate for file discovery, which respects `.ignore` files. This pattern causes ALL Python files to be skipped during test discovery.

**Diagnosis:**

```bash
# Check if .ignore contains *.py
grep '^\*\.py$' .ignore && echo "FOUND: *.py in .ignore is blocking discovery"

# Verify files exist but are being ignored
ls tests/**/*.py  # Files exist
tach-core list .  # But discovery finds nothing
```

**Solution:**

Remove `*.py` from `.ignore`:

```bash
sed -i '/^\*\.py$/d' .ignore
```

Or edit `.ignore` manually and remove the `*.py` line.

**Prevention:**

If you need to exclude Python files from other tools but not from tach-core, use more specific patterns (e.g., `src/**/*.py` instead of `*.py`).

> **Note:** The `.ignore` file format is shared between multiple tools. Patterns added for one tool may affect others that use the `ignore` crate (ripgrep, fd, tach-core, etc.).

### Fixtures Not Found

**Symptom:**

```
Error: Fixture 'my_fixture' not found
```

**Common Causes:**

| Cause                  | Solution                        |
| :--------------------- | :------------------------------ |
| Missing conftest.py    | Create conftest.py with fixture |
| Fixture in wrong scope | Move to correct conftest.py     |
| Typo in fixture name   | Check spelling                  |
| Dynamic fixture        | Tach uses static analysis only  |

**Diagnosis:**

```bash
# Check conftest.py exists
ls -la tests/conftest.py

# Verify fixture is defined
grep -r "def my_fixture" tests/
```

### Async Tests Skipped

**Symptom:** Async tests marked as skipped.

**Solution:** Ensure `pytest-asyncio` is installed and fixtures are properly scoped:

```python
# conftest.py
import pytest

@pytest.fixture
def event_loop():
    import asyncio
    loop = asyncio.new_event_loop()
    yield loop
    loop.close()
```

---

## Performance Issues

### Slow Test Startup

**Symptom:** Long delay before first test runs.

**Cause:** Zygote initialization includes importing all dependencies.

**Solution:**

- Reduce imports in conftest.py
- Lazy-load heavy dependencies
- Use bytecode cache (enabled by default)

**Diagnosis:**

```bash
# Profile import time
python -X importtime -c "import your_module" 2>&1 | head -30
```

### Memory Usage High

**Symptom:** Tests consuming excessive memory.

**Cause:** Large test data or memory leaks in tests.

**Solution:**

```bash
# Check memory usage
/usr/bin/time -v tach-core .

# Profile with valgrind
valgrind --tool=massif ./target/release/tach-core .
```

### Worker Reset Slow

**Symptom:** Tests running slower than expected.

**Diagnosis:**

```bash
# Check for toxic tests (require full restart)
tach-core list . 2>&1 | grep -i toxic

# Profile with perf
perf record -g ./target/release/tach-core .
perf report
```

---

## Docker Issues

### Sandbox Fails in Container

**Symptom:**

```
[WARN] Landlock not available in container
```

**Solution:** Add required capabilities:

```yaml
# docker-compose.yml
services:
  tests:
    security_opt:
      - seccomp:unconfined
    cap_add:
      - SYS_PTRACE
      - SYS_ADMIN
```

Or with `docker run`:

```bash
docker run \
  --cap-add SYS_PTRACE \
  --cap-add SYS_ADMIN \
  --security-opt seccomp=unconfined \
  your-image
```

### userfaultfd in Container

**Symptom:**

```
userfaultfd not available in container
```

**Solution:** Ensure host kernel supports userfaultfd and container has `SYS_PTRACE`:

```bash
# On host
sudo sysctl vm.unprivileged_userfaultfd=1

# In container
docker run --cap-add SYS_PTRACE your-image
```

---

## CI Issues

### GitHub Actions Permissions

**Symptom:** Tests fail in GitHub Actions with EPERM.

**Solution:** Ensure runner has required permissions. For self-hosted runners:

```yaml
jobs:
  test:
    runs-on: self-hosted
    steps:
      - uses: actions/checkout@v4
      - name: Run tests
        run: |
          # May need --no-isolation in some environments
          ./target/release/tach-core --no-isolation .
```

### JUnit XML Not Generated

**Symptom:** No JUnit XML output in CI.

**Solution:**

```bash
# Specify output path explicitly
tach-core --junit-xml results.xml .

# Verify file exists
ls -la results.xml
```

---

## Database Issues

### Django Test Database

**Symptom:** Database errors in Django tests.

**Cause:** Transaction isolation not working.

**Solution:** Configure Django for Tach:

```python
# settings.py
DATABASES['default']['TEST'] = {
    'NAME': ':memory:',  # Use in-memory SQLite
}
```

### Connection Pool Exhaustion

**Symptom:**

```
OperationalError: too many connections
```

**Solution:** Configure connection limits:

```python
# Django
DATABASES['default']['CONN_MAX_AGE'] = 0

# SQLAlchemy
engine = create_engine(url, pool_size=5, max_overflow=0)
```

---

## Log Analysis

### Enable Debug Logging

```bash
# Verbose output
RUST_LOG=debug tach-core .

# Specific module
RUST_LOG=tach_core::isolation::sandbox=debug tach-core .
```

### Interpreting Log Messages

| Log Pattern                             | Meaning                          |
| :-------------------------------------- | :------------------------------- |
| `[DEBUG] Landlock ABI: V3`              | Landlock version detected        |
| `[WARN] Falling back to fork isolation` | Snapshot mode unavailable        |
| `[INFO] Worker reset: 45us`             | Healthy reset time               |
| `[WARN] Worker reset: 5ms`              | Slow reset (check memory usage)  |
| `[ERROR] Worker crashed`                | Worker process died unexpectedly |

---

## Getting Help

### Collect Diagnostic Information

```bash
# System info
uname -a
python --version
cat /etc/os-release

# Tach version
./target/release/tach-core --version

# Kernel features
cat /sys/kernel/security/lsm
cat /proc/sys/vm/unprivileged_userfaultfd

# Run self-test
./target/release/tach-core self-test
```

### Report Issues

When reporting issues, include:

1. Full error message
2. System diagnostic output (above)
3. Minimal reproduction case
4. pyproject.toml configuration

---

## Error Codes

Tach uses structured error codes to help diagnose issues. Error codes follow the pattern `EXXX` where:

- **E001-E004, E010, E012**: User errors (test code, configuration, Python version)
- **E005-E009, E011, E013-E016**: System errors (kernel, permissions, resources)
- **E017-E020**: Extended user errors (syntax, fixtures, test status)

### User Errors

#### E001: Test Assertion Failed

**Category:** User

**Cause:** A test assertion failed during execution. The test's expected outcome did not match the actual result.

**Solution:**

1. Review the test assertion and expected values
2. Check if the code under test has changed
3. Verify test data and fixtures are correct

---

#### E002: Import Error

**Category:** User

**Cause:** Failed to import a module in a test file. This could be a missing dependency or incorrect import path.

**Solution:**

1. Ensure the module is installed: `pip install <module>`
2. Verify the import path is correct
3. Check for circular imports
4. Ensure `PYTHONPATH` is set correctly

---

#### E003: Fixture Not Found

**Category:** User

**Cause:** A test requests a fixture that does not exist or is not accessible.

**Solution:**

1. Define the fixture in `conftest.py` or the test file
2. Check for typos in the fixture name
3. Ensure conftest.py is in the correct directory
4. Verify fixture scope is appropriate

---

#### E004: Invalid Marker Expression

**Category:** User

**Cause:** The marker expression passed via `-m` flag has invalid syntax.

**Solution:**

1. Check marker syntax: `-m "slow and not integration"`
2. Use proper boolean operators: `and`, `or`, `not`
3. Ensure marker names are valid identifiers

---

#### E010: Timeout Exceeded

**Category:** User

**Cause:** A test exceeded the configured timeout limit.

**Solution:**

1. Increase timeout: `@pytest.mark.timeout(N)` on the test
2. Increase global timeout: `--timeout N` CLI flag
3. Optimize the test for better performance
4. Check for infinite loops or deadlocks

---

#### E012: Python Version Mismatch

**Category:** User

**Cause:** The Python binary used does not match the expected version.

**Solution:**

1. Set `PYO3_PYTHON` to the correct Python binary path
2. Verify Python version: `python --version`
3. Create a virtual environment with the correct version

---

#### E017: Syntax Error in Test File

**Category:** User

**Cause:** A Python syntax error was found in a test file.

**Solution:**

1. Run `python -m py_compile <file>` to locate the error
2. Fix the syntax error at the indicated line
3. Check for missing colons, brackets, or indentation issues

---

#### E018: Circular Fixture Dependency

**Category:** User

**Cause:** Fixtures have circular dependencies that cannot be resolved.

**Solution:**

1. Review fixture dependency graph
2. Refactor fixtures to break the cycle
3. Use factory patterns to defer fixture creation
4. Consider using fixture scopes to avoid the cycle

---

#### E019: Skipped Test

**Category:** User (Informational)

**Cause:** A test was skipped due to a skip marker or condition.

**Note:** This is informational, not an error. The test was intentionally skipped.

---

#### E020: Expected Failure (Xfail)

**Category:** User (Informational)

**Cause:** A test is marked as expected to fail (`@pytest.mark.xfail`).

**Note:** This is informational, not an error. The test is known to fail and tracked.

---

### System Errors

#### E005: userfaultfd Not Available

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

#### E006: Landlock Not Supported

**Category:** System

**Cause:** Landlock filesystem sandboxing is not available. Requires Linux kernel 5.13+.

**Solution:**

1. Upgrade to Linux kernel 5.13 or later
2. Tach will run with degraded filesystem isolation
3. Check kernel config: `CONFIG_SECURITY_LANDLOCK=y`

---

#### E007: Permission Denied

**Category:** System

**Cause:** An operation was denied due to insufficient permissions.

**Solution:**

1. Check file and directory permissions
2. Run with elevated privileges if necessary
3. In containers, use `--privileged` flag
4. Check SELinux/AppArmor policies

---

#### E008: Out of Memory

**Category:** System

**Cause:** System ran out of memory during test execution.

**Solution:**

1. Reduce worker count: `-n 2`
2. Increase system memory or swap
3. Check for memory leaks in tests
4. Use `--force-toxic` to reduce snapshot memory usage

---

#### E009: Too Many Open Files

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

#### E011: OverlayFS Mount Failed

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

#### E013: Namespace Creation Failed

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

#### E014: Worker Crash

**Category:** System

**Cause:** A worker process crashed with a signal (SIGSEGV, SIGBUS, etc.).

**Solution:**

1. Check for memory corruption in C extensions
2. Increase stack size: `ulimit -s unlimited`
3. Run with `--force-toxic` to isolate problematic tests
4. Check for segfault-causing code in tests

---

#### E015: IPC Channel Failure

**Category:** System

**Cause:** Communication between supervisor and worker failed.

**Solution:**

1. Check system resources (memory, file descriptors)
2. Reduce worker count: `-n 2`
3. Check for worker crashes in logs
4. Ensure `/dev/shm` has sufficient space

---

#### E016: Snapshot Integrity Failure

**Category:** System

**Cause:** Memory snapshot verification failed, indicating corruption.

**Solution:**

1. This is an internal error - please report a bug
2. Try running with `--force-toxic` as a workaround
3. Check for memory-corrupting C extensions
4. Verify system memory is healthy: `memtest86+`

---

## Known Limitations

### Static Discovery Limitations

Tach uses static AST analysis for test discovery, which cannot detect:

| Feature                 | Limitation                                     | Workaround                             |
| ----------------------- | ---------------------------------------------- | -------------------------------------- |
| `pytest_generate_tests` | Dynamic test generation not visible statically | Use explicit parametrize decorators    |
| Autouse fixtures        | May not be fully detected in all cases         | Document in test or use explicit marks |
| Nested TestClass        | Deeply nested classes may not be discovered    | Flatten test class hierarchy           |
| Plugin-generated tests  | Tests created by plugins at runtime            | Run with `--collect-only` to verify    |

These limitations are inherent to static analysis. If tests are missing, use `--no-ignore` to verify they aren't being filtered, or run `pytest --collect-only` to compare discovery results.

---

## Related Documentation

- [README](../README.md) - Project overview and quick start
- [Configuration](configuration.md) - CLI and config options
- [Development](development.md) - Build and test commands
- [Sandbox](architecture/sandbox.md) - Security architecture
- [Snapshot](architecture/snapshot.md) - Memory snapshot details
