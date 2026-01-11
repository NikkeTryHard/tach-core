# Quickstart Guide

Get started with Tach in minutes. This guide covers installation, running your first tests, and migrating from pytest.

---

## Installation

Tach runs on Linux with kernel 5.13 or later. Choose your distribution below.

### Ubuntu (22.04+)

```bash
# Install system dependencies
sudo apt update
sudo apt install -y build-essential python3-dev python3-venv

# Install Rust (if not already installed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"

# Clone and build Tach
git clone https://github.com/NikkeTryHard/tach-core.git
cd tach-core

# Create Python environment
python3 -m venv .venv
source .venv/bin/activate
pip install pytest

# Build Tach
export PYO3_PYTHON=$(which python)
cargo build --release

# Verify installation
./target/release/tach-core --version
./target/release/tach-core self-test
```

### Fedora (34+)

```bash
# Install system dependencies
sudo dnf install -y gcc make python3-devel

# Install Rust (if not already installed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"

# Clone and build Tach
git clone https://github.com/NikkeTryHard/tach-core.git
cd tach-core

# Create Python environment
python3 -m venv .venv
source .venv/bin/activate
pip install pytest

# Build Tach
export PYO3_PYTHON=$(which python)
cargo build --release

# Verify installation
./target/release/tach-core --version
./target/release/tach-core self-test
```

### Arch Linux

```bash
# Install system dependencies
sudo pacman -S base-devel python

# Install Rust (if not already installed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"

# Clone and build Tach
git clone https://github.com/NikkeTryHard/tach-core.git
cd tach-core

# Create Python environment
python -m venv .venv
source .venv/bin/activate
pip install pytest

# Build Tach
export PYO3_PYTHON=$(which python)
cargo build --release

# Verify installation
./target/release/tach-core --version
./target/release/tach-core self-test
```

### Verifying Kernel Support

After installation, verify your system supports Tach:

```bash
# Check kernel version (must be 5.13+)
uname -r

# Check Landlock support
cat /sys/kernel/security/lsm | grep landlock

# Run self-test for complete verification
./target/release/tach-core self-test
```

---

## First Test Run

### Step 1: Create a Test File

Create a simple test file to verify Tach works:

```python
# tests/test_example.py
def test_addition():
    assert 1 + 1 == 2

def test_string():
    assert "hello".upper() == "HELLO"

def test_list():
    items = [1, 2, 3]
    assert len(items) == 3
```

### Step 2: Run with Tach

```bash
# Run tests
./target/release/tach-core tests/

# Expected output:
# tests/test_example.py::test_addition PASSED
# tests/test_example.py::test_string PASSED
# tests/test_example.py::test_list PASSED
#
# 3 passed in 0.05s
```

### Step 3: Explore Options

```bash
# Verbose output
./target/release/tach-core -v tests/

# Run with 4 parallel workers
./target/release/tach-core -n 4 tests/

# Filter tests by keyword
./target/release/tach-core -k "string" tests/

# Stop on first failure
./target/release/tach-core -x tests/

# List tests without running
./target/release/tach-core list tests/
```

---

## Comparison with pytest

Tach is designed as a drop-in replacement for pytest with better performance. Here is a side-by-side comparison.

### Running Tests

| Task               | pytest                   | Tach                                 |
| :----------------- | :----------------------- | :----------------------------------- |
| Run all tests      | `pytest .`               | `tach-core .`                        |
| Run specific file  | `pytest tests/test_a.py` | `tach-core tests/test_a.py`          |
| Parallel execution | `pytest -n 4`            | `tach-core -n 4`                     |
| Verbose output     | `pytest -v`              | `tach-core -v`                       |
| Stop on failure    | `pytest -x`              | `tach-core -x`                       |
| Filter by keyword  | `pytest -k "pattern"`    | `tach-core -k "pattern"`             |
| Filter by marker   | `pytest -m "slow"`       | `tach-core -m "slow"`                |
| List tests         | `pytest --collect-only`  | `tach-core list` or `--collect-only` |

### Coverage

| Task            | pytest                   | Tach                             |
| :-------------- | :----------------------- | :------------------------------- |
| Enable coverage | `pytest --cov=src`       | `tach-core --coverage --cov=src` |
| Coverage report | Generated via pytest-cov | Generated in LCOV format         |

### Output Formats

| Task            | pytest                     | Tach                          |
| :-------------- | :------------------------- | :---------------------------- |
| JUnit XML       | `pytest --junit-xml=r.xml` | `tach-core --junit-xml=r.xml` |
| JSON output     | Requires plugins           | `tach-core --format json`     |
| Traceback style | `pytest --tb=short`        | `tach-core --tb short`        |

### Example Workflows

**pytest workflow:**

```bash
# Traditional pytest with xdist for parallel execution
pip install pytest pytest-xdist pytest-cov
pytest tests/ -n 4 --cov=src --junit-xml=results.xml
```

**Tach workflow:**

```bash
# Tach with built-in parallelism and coverage
./target/release/tach-core tests/ -n 4 --coverage --cov=src --junit-xml=results.xml
```

---

## Migration Guide from pytest

Migrating from pytest to Tach is straightforward. Most test suites work without modification.

### What Stays the Same

- **Test discovery** - `test_*.py` files and `test_*` functions work identically
- **Assertions** - Standard Python assertions and pytest assertions work
- **Fixtures** - pytest fixtures work (module, function, session scope)
- **Markers** - `@pytest.mark.*` decorators are supported
- **conftest.py** - Configuration files are recognized
- **pyproject.toml** - pytest settings in `[tool.pytest.ini_options]` are read

### What Changes

| Aspect             | pytest                | Tach                         |
| :----------------- | :-------------------- | :--------------------------- |
| Parallel execution | Requires pytest-xdist | Built-in (`-n` flag)         |
| Coverage           | Requires pytest-cov   | Built-in (`--coverage` flag) |
| Process isolation  | Fork per test         | Memory snapshots             |
| Platform support   | Windows, macOS, Linux | Linux only (kernel 5.13+)    |
| Watch mode         | Requires pytest-watch | Built-in (`--watch` flag)    |

### Migration Checklist

1. **Verify kernel version**

   ```bash
   uname -r  # Must be 5.13 or later
   ```

2. **Run self-test**

   ```bash
   ./target/release/tach-core self-test
   ```

3. **Test with existing suite**

   ```bash
   # Run your existing tests with Tach
   ./target/release/tach-core tests/
   ```

4. **Compare results**

   ```bash
   # Run with pytest for comparison
   pytest tests/ -v > pytest_output.txt

   # Run with Tach
   ./target/release/tach-core tests/ -v > tach_output.txt

   # Compare (test counts and results should match)
   diff pytest_output.txt tach_output.txt
   ```

5. **Add Tach configuration** (optional)

   ```toml
   # pyproject.toml
   [tool.tach]
   test_pattern = "test_*.py"
   timeout = 60
   workers = 4
   ```

### Known Differences

| Feature                 | pytest behavior          | Tach behavior                    |
| :---------------------- | :----------------------- | :------------------------------- |
| Plugin system           | Extensive plugin support | Limited (core features built-in) |
| Subprocess tests        | Work normally            | Sandboxed (some restrictions)    |
| Network access in tests | Allowed                  | Blocked by default (Seccomp)     |
| Database connections    | Per-test setup           | Connection pooling preserved     |

### Handling Incompatibilities

**Network-dependent tests:**

Tests that require network access will fail with Seccomp enabled. Disable sandboxing for these tests:

```bash
# Disable isolation (development only)
./target/release/tach-core --no-isolation tests/
```

**Subprocess-heavy tests:**

Tests marked as "toxic" (using subprocess, multiprocessing) run in a separate mode:

```bash
# Force toxic mode for all tests
./target/release/tach-core --force-toxic tests/
```

---

## WSL2 Setup

Windows Subsystem for Linux 2 (WSL2) requires additional configuration for full Tach feature support. This section covers WSL2-specific limitations and workarounds.

### Quick Diagnosis

Run this to check your system's feature availability:

```bash
# Check userfaultfd
cat /proc/sys/vm/unprivileged_userfaultfd
# 0 = disabled (needs fix), 1 = enabled

# Check Landlock
cat /sys/kernel/security/landlock/abi_version
# Should show version number, "No such file" = not loaded

# Check kernel config
zcat /proc/config.gz | grep -E "USERFAULTFD|LANDLOCK|SECCOMP"
```

Or run the built-in diagnostics:

```bash
./target/debug/tach-core self-test
```

### Feature Status in WSL2

| Feature     | Purpose            | Typical WSL2 Status                  | Impact if Missing                  |
| ----------- | ------------------ | ------------------------------------ | ---------------------------------- |
| userfaultfd | Memory snapshots   | Compiled in, but disabled by default | Falls back to fork-server (slower) |
| Landlock    | Filesystem sandbox | Compiled in, but LSM not loaded      | No filesystem isolation            |
| Seccomp     | Syscall filtering  | Works                                | N/A                                |
| Namespaces  | Process isolation  | Works                                | N/A                                |
| OverlayFS   | Test isolation     | Works on ext4, issues on /mnt/c/     | Use native Linux paths             |

### Workarounds

#### 1. Enable userfaultfd (Highest Priority)

userfaultfd enables memory snapshots for sub-millisecond test isolation reset.

**Option A: Temporary (until WSL restart)**

```bash
sudo sysctl -w vm.unprivileged_userfaultfd=1
```

**Option B: Persistent via .wslconfig**

Create or edit `C:\Users\<YourUsername>\.wslconfig` on Windows:

```ini
[wsl2]
kernelCommandLine = sysctl.vm.unprivileged_userfaultfd=1
```

Then restart WSL from PowerShell:

```powershell
wsl --shutdown
```

**Option C: Startup script**

Add to `~/.bashrc` or create `/etc/profile.d/tach.sh`:

```bash
if [ -f /proc/sys/vm/unprivileged_userfaultfd ]; then
    current=$(cat /proc/sys/vm/unprivileged_userfaultfd)
    if [ "$current" = "0" ]; then
        sudo sysctl -w vm.unprivileged_userfaultfd=1 >/dev/null 2>&1
    fi
fi
```

#### 2. Enable Landlock LSM

Landlock provides filesystem sandboxing. Microsoft's WSL2 kernel has it compiled in but doesn't load it by default.

**Option A: Add LSM to kernel command line**

Edit `C:\Users\<YourUsername>\.wslconfig`:

```ini
[wsl2]
kernelCommandLine = lsm=landlock,lockdown,yama,integrity,apparmor,bpf sysctl.vm.unprivileged_userfaultfd=1
```

Restart WSL:

```powershell
wsl --shutdown
```

**Option B: Build custom WSL2 kernel**

For full control, build a custom kernel:

```bash
# Clone Microsoft's kernel source
git clone --depth 1 https://github.com/microsoft/WSL2-Linux-Kernel.git
cd WSL2-Linux-Kernel

# Use Microsoft's config as base
cp Microsoft/config-wsl .config

# Enable Landlock in menuconfig
make menuconfig
# Navigate to: Security options -> Landlock support
# Ensure it's set to [*] (built-in) and in LSM stack

# Build
make -j$(nproc) bzImage

# Copy to Windows-accessible location
cp arch/x86/boot/bzImage /mnt/c/Users/<YourUsername>/wsl-kernel
```

Then edit `.wslconfig`:

```ini
[wsl2]
kernel=C:\\Users\\<YourUsername>\\wsl-kernel\\bzImage
kernelCommandLine = lsm=landlock,lockdown,yama,integrity,apparmor,bpf sysctl.vm.unprivileged_userfaultfd=1
```

#### 3. Filesystem Considerations

**Use Native ext4 Paths**

WSL2 performance is much better on the native ext4 filesystem:

```bash
# Good - native ext4
/home/username/dev/project

# Bad - Windows filesystem via 9P (slow, OverlayFS issues)
/mnt/c/Users/username/projects
```

If your project is on `/mnt/c/`, consider moving it:

```bash
mv /mnt/c/Users/username/project ~/dev/
```

#### 4. Docker Development Environment

For a fully-configured development environment, use the included Docker setup:

**Quick Start (Docker Compose)**

```bash
# Build and start container
docker compose up -d

# Enter container
docker compose exec dev bash

# Inside container - activate Python environment
# (venv is auto-created by VS Code Dev Container; create manually for docker compose)
source .venv/bin/activate 2>/dev/null || \
  (python3.12 -m venv .venv && source .venv/bin/activate && pip install pytest)

# Build and verify
cargo build --release
./target/release/tach-core self-test
```

**VS Code Dev Container**

1. Install the "Dev Containers" extension in VS Code
2. Open the tach-core folder
3. Click "Reopen in Container" when prompted (or Ctrl+Shift+P -> "Dev Containers: Reopen in Container")
4. Wait for the container to build and setup to complete
5. Open a terminal - all tools are ready

**What's Included**

| Tool        | Purpose                  |
| ----------- | ------------------------ |
| Rust 1.88+  | Build tach-core          |
| Python 3.12 | Run tests, PyO3 bindings |
| gdb         | Debug Rust/Python        |
| strace      | Trace syscalls           |
| perf        | Performance profiling    |
| ripgrep, fd | Fast searching           |

**Kernel Features in Docker**

The container runs with `--privileged` mode, which grants access to all kernel features:

```bash
# Inside container, all features work:
./target/release/tach-core self-test
# [PASS] userfaultfd: Enabled
# [PASS] Landlock: ABI v4 supported
# [PASS] Seccomp: BPF filters available
```

Note: The Docker container inherits kernel features from WSL2. If your WSL2 kernel doesn't have a feature enabled (e.g., Landlock LSM not loaded), Docker cannot provide it. See sections above for WSL2 kernel configuration.

**Persistent Cargo Cache**

Docker volumes preserve cargo's package cache between container restarts:

```bash
# First build downloads all crates
cargo build  # ~2 minutes

# Container restart
docker compose down && docker compose up -d

# Subsequent builds use cached crates
cargo build  # ~10 seconds (incremental)
```

#### 5. Accept Graceful Degradation

tach-core is designed to handle missing features gracefully:

- **Without userfaultfd**: Uses fork-server pattern (no snapshots, slower but works)
- **Without Landlock**: Logs warning, continues without filesystem sandbox
- **Without Seccomp**: Only affects safe workers (toxic workers bypass it anyway)

Use `--no-isolation` flag to explicitly disable sandboxing:

```bash
./target/debug/tach-core --no-isolation tests/
```

### Complete .wslconfig Template

Create `C:\Users\<YourUsername>\.wslconfig`:

```ini
[wsl2]
# Enable userfaultfd for memory snapshots
# Enable Landlock LSM for filesystem sandboxing
kernelCommandLine = lsm=landlock,lockdown,yama,integrity,apparmor,bpf sysctl.vm.unprivileged_userfaultfd=1

# Optional: Limit memory/CPU if needed
# memory=8GB
# processors=4

# Optional: Custom kernel path (if you built one)
# kernel=C:\\Users\\<YourUsername>\\wsl-kernel\\bzImage
```

After creating/editing, restart WSL:

```powershell
wsl --shutdown
```

### Verification Script

Save as `~/verify-tach-wsl2.sh`:

```bash
#!/bin/bash
echo "=== tach-core WSL2 Feature Check ==="
echo ""

# userfaultfd
uffd=$(cat /proc/sys/vm/unprivileged_userfaultfd 2>/dev/null)
if [ "$uffd" = "1" ]; then
    echo "[OK] userfaultfd: enabled"
else
    echo "[!!] userfaultfd: DISABLED (run: sudo sysctl -w vm.unprivileged_userfaultfd=1)"
fi

# Landlock
ll_ver=$(cat /sys/kernel/security/landlock/abi_version 2>/dev/null)
if [ -n "$ll_ver" ]; then
    echo "[OK] Landlock: ABI v$ll_ver"
else
    echo "[!!] Landlock: NOT LOADED (add lsm= to .wslconfig kernelCommandLine)"
fi

# Seccomp
if grep -q "CONFIG_SECCOMP=y" /proc/config.gz 2>/dev/null; then
    echo "[OK] Seccomp: enabled"
else
    echo "[??] Seccomp: unknown"
fi

# Filesystem
if [[ "$(pwd)" == /mnt/* ]]; then
    echo "[!!] Filesystem: Windows path (slow) - consider moving to ~/dev/"
else
    echo "[OK] Filesystem: native ext4"
fi

echo ""
echo "Run './target/debug/tach-core self-test' for full diagnostics"
```

### WSL2 Troubleshooting

**"EPERM on userfaultfd"**

userfaultfd is disabled. Fix:

```bash
sudo sysctl -w vm.unprivileged_userfaultfd=1
```

**"Landlock not available"**

LSM not loaded. Add to `.wslconfig` kernelCommandLine or accept degraded mode.

**Tests hang or timeout**

Possible causes:

- Project on `/mnt/c/` (slow 9P filesystem)
- Insufficient memory allocated to WSL2
- Docker Desktop consuming resources

**Build fails with PyO3 errors**

Ensure Python is accessible:

```bash
export PYO3_PYTHON=$(which python3)
cargo build
```

### WSL2 References

- [Microsoft WSL2 Kernel Source](https://github.com/microsoft/WSL2-Linux-Kernel)
- [WSL Configuration Options](https://learn.microsoft.com/en-us/windows/wsl/wsl-config)
- [Landlock Documentation](https://docs.kernel.org/userspace-api/landlock.html)
- [userfaultfd Documentation](https://www.kernel.org/doc/html/latest/admin-guide/mm/userfaultfd.html)

---

## Next Steps

- [Configuration Reference](configuration.md) - Full CLI and pyproject.toml options
- [Django Example](../examples/django/README.md) - Database testing example
- [Development Guide](development.md) - Contributing and building
- [Troubleshooting](troubleshooting.md) - Common issues and solutions

---

## Quick Reference

```bash
# Run all tests
tach-core .

# Parallel execution
tach-core -n 4 .

# Verbose with coverage
tach-core -v --coverage .

# Filter and fail fast
tach-core -k "auth" -x .

# JUnit output for CI
tach-core --junit-xml results.xml .

# List tests
tach-core list .

# Watch mode
tach-core --watch .

# Self-test
tach-core self-test
```
