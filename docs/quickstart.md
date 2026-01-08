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
git clone https://github.com/user/tach-core.git
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
git clone https://github.com/user/tach-core.git
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
git clone https://github.com/user/tach-core.git
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
