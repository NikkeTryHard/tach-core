# Configuration Reference

Complete reference for Tach configuration options.

---

## Configuration Sources

Tach reads configuration from multiple sources with the following precedence:

1. **CLI arguments** (highest priority)
2. **Environment variables**
3. **pyproject.toml** (lowest priority)

---

## CLI Arguments

```bash
tach-core [OPTIONS] [COMMAND] [PATH]
```

### Commands

| Command | Description                           |
| :------ | :------------------------------------ |
| `test`  | Run tests (default)                   |
| `list`  | List discovered tests without running |

### Options

| Flag                 | Description                         | Default |
| :------------------- | :---------------------------------- | :------ |
| `--format <FORMAT>`  | Output format: `human` or `json`    | `human` |
| `--junit-xml <PATH>` | Generate JUnit XML report           | -       |
| `--coverage`         | Enable PEP 669 coverage collection  | false   |
| `--no-isolation`     | Disable namespace/sandbox isolation | false   |
| `--watch`, `-w`      | Re-run tests on file changes        | false   |
| `[PATH]`             | Test path (file or directory)       | `.`     |

### Examples

```bash
# Run all tests
tach-core .

# Run specific file
tach-core tests/test_auth.py

# List tests without running
tach-core list .

# Enable coverage
tach-core --coverage .

# JSON output for IDE integration
tach-core --format json .

# Generate JUnit XML
tach-core --junit-xml results.xml .

# Development mode (no sandbox)
tach-core --no-isolation .

# Watch mode
tach-core --watch .
```

---

## Environment Variables

| Variable               | Description                       | Default |
| :--------------------- | :-------------------------------- | :------ |
| `TACH_FORMAT`          | Output format (`human` or `json`) | `human` |
| `TACH_JUNIT_XML`       | Path to JUnit XML output          | -       |
| `TACH_COVERAGE`        | Enable coverage (`1` or `true`)   | -       |
| `TACH_NO_ISOLATION`    | Disable sandbox (`1` or `true`)   | -       |
| `TACH_TARGET_PATH`     | Test path (set internally)        | `.`     |
| `TACH_SUPERVISOR_SOCK` | UFFD socket path (set internally) | -       |
| `CI`                   | Detected for reporter selection   | -       |
| `PYO3_PYTHON`          | Python interpreter path for build | -       |
| `MALLOC_CONF`          | Jemalloc configuration            | -       |

### Examples

```bash
# Enable coverage via environment
TACH_COVERAGE=1 tach-core .

# Force JSON output
TACH_FORMAT=json tach-core .

# Disable sandbox
TACH_NO_ISOLATION=1 tach-core .

# Configure jemalloc
MALLOC_CONF="background_thread:false,dirty_decay_ms:0" tach-core .
```

---

## pyproject.toml

Configure Tach via the `[tool.tach]` section:

```toml
[tool.tach]
# Test file pattern (glob)
test_pattern = "test_*.py"

# Test timeout in seconds
timeout = 60

# Number of worker processes
workers = 4

# Isolation strategy: "auto", "fork", "snapshot"
isolation_strategy = "auto"

[tool.tach.coverage]
# Enable coverage collection
enabled = true

# Source directories to measure
source = ["src", "lib"]

# Patterns to omit from coverage
omit = ["**/test_*", "**/migrations/*"]

# Output file path
output = ".coverage"

# Output format: "lcov", "html", "json"
format = "lcov"
```

### [tool.tach] Options

| Option               | Type    | Default       | Description                 |
| :------------------- | :------ | :------------ | :-------------------------- |
| `test_pattern`       | string  | `"test_*.py"` | Glob pattern for test files |
| `timeout`            | integer | `60`          | Test timeout in seconds     |
| `workers`            | integer | `num_cpus`    | Number of worker processes  |
| `isolation_strategy` | string  | `"auto"`      | Isolation mode              |

### [tool.tach.coverage] Options

| Option    | Type    | Default       | Description                   |
| :-------- | :------ | :------------ | :---------------------------- |
| `enabled` | boolean | `false`       | Enable coverage collection    |
| `source`  | array   | `[]`          | Source directories to measure |
| `omit`    | array   | `[]`          | Patterns to exclude           |
| `output`  | string  | `".coverage"` | Output file path              |
| `format`  | string  | `"lcov"`      | Output format                 |

---

## pytest-env Compatibility

Tach supports `[tool.pytest_env]` for environment variable injection:

```toml
[tool.pytest_env]
DATABASE_URL = "sqlite:///:memory:"
DEBUG = "true"
SECRET_KEY = "test-secret"
```

These variables are set before test execution.

---

## Security: Environment Variable Denylist

Tach blocks dangerous environment variables in `[tool.pytest_env]` to prevent supply chain attacks via compromised `pyproject.toml` files.

### Blocked Variables

| Variable          | Category              | Risk                                                               |
| :---------------- | :-------------------- | :----------------------------------------------------------------- |
| `LD_PRELOAD`      | Library Injection     | Loads arbitrary shared libraries before all others                 |
| `LD_LIBRARY_PATH` | Library Injection     | Redirects library loading to attacker-controlled paths             |
| `LD_AUDIT`        | Library Injection     | Loads audit libraries that can intercept all function calls        |
| `LD_DEBUG`        | Library Injection     | Enables debug output that can leak sensitive information           |
| `PYTHONPATH`      | Python Hijacking      | Injects malicious Python modules into import path                  |
| `PYTHONHOME`      | Python Hijacking      | Redirects Python installation to attacker-controlled location      |
| `PYTHONSTARTUP`   | Python Hijacking      | Executes arbitrary Python code on interpreter startup              |
| `PYTHONMALLOC`    | Allocator Override    | Overrides memory allocator, breaking jemalloc snapshot consistency |
| `PATH`            | Path Manipulation     | Redirects command execution to malicious binaries                  |
| `HOME`            | Path Manipulation     | Changes home directory, affecting config file loading              |
| `USER`            | Identity Manipulation | Spoofs user identity for permission checks                         |

### Why These Are Dangerous

- **Library Injection** (`LD_*`): Allows arbitrary code execution by loading malicious shared libraries before your application starts.
- **Python Hijacking** (`PYTHON*`): Enables module injection and startup code execution. `PYTHONMALLOC` is critical for Tach since overriding the allocator breaks jemalloc snapshot consistency.
- **Path Manipulation** (`PATH`, `HOME`, `USER`): Redirects command execution or config file loading to attacker-controlled locations.

Matching is **case-insensitive** to prevent bypass attempts (e.g., `ld_preload` is also blocked).

### Warning Message

When a blocked variable is detected, Tach emits a warning and skips it:

```
[config] WARNING: Blocked dangerous env var from pyproject.toml: LD_PRELOAD
```

### Workarounds

If you legitimately need these variables, set them via shell environment (not blocked):

```bash
# Shell environment is trusted - only pyproject.toml parsing is restricted
export PYTHONPATH="/my/custom/path"
tach-core .
```

Or use a wrapper script:

```bash
#!/bin/bash
export PYTHONPATH="/my/custom/path"
exec tach-core "$@"
```

---

## Isolation Strategies

| Strategy   | Description                                 |
| :--------- | :------------------------------------------ |
| `auto`     | Automatically choose based on test toxicity |
| `fork`     | Traditional fork-based isolation            |
| `snapshot` | userfaultfd-based memory snapshots          |

---

## Configuration Precedence Examples

### Coverage

```bash
# CLI wins
tach-core --coverage .  # Coverage enabled

# Environment wins over file
TACH_COVERAGE=1 tach-core .  # Coverage enabled

# File is lowest priority
# pyproject.toml: [tool.tach.coverage] enabled = true
tach-core .  # Coverage enabled (from file)
```

### Format

```bash
# CLI wins
tach-core --format json .  # JSON output

# Environment wins over default
TACH_FORMAT=json tach-core .  # JSON output
```

---

## Docker Configuration

When running in Docker, you may need additional capabilities:

```yaml
# docker-compose.yml
services:
  tests:
    image: your-image
    security_opt:
      - seccomp:unconfined
    cap_add:
      - SYS_PTRACE
```

Or with `docker run`:

```bash
docker run --cap-add SYS_PTRACE --security-opt seccomp=unconfined your-image
```

---

## CI Configuration Examples

### GitHub Actions

```yaml
- name: Run tests
  run: |
    ./target/release/tach-core --junit-xml results.xml .

- name: Upload results
  uses: actions/upload-artifact@v3
  with:
    name: test-results
    path: results.xml
```

### GitLab CI

```yaml
test:
  script:
    - ./target/release/tach-core --junit-xml results.xml .
  artifacts:
    reports:
      junit: results.xml
```

---

## Related Documentation

- [Development](development.md) - Build and test commands
- [Troubleshooting](troubleshooting.md) - Common issues
- [Reporter](architecture/reporter.md) - Output format details
