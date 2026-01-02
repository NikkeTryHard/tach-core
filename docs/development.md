# Development Guide

Guide for building, testing, and contributing to Tach.

---

## Prerequisites

| Requirement | Version                    |
| :---------- | :------------------------- |
| Rust        | 1.75+                      |
| Python      | 3.10+ (3.12+ for coverage) |
| Linux       | Kernel 5.13+               |
| Build tools | gcc, make, autoconf        |

---

## Quick Start

```bash
# Clone repository
git clone https://github.com/user/tach-core.git
cd tach-core

# Setup Python virtual environment
python -m venv .venv
source .venv/bin/activate
pip install pytest

# Build
export PYO3_PYTHON=$(which python)
cargo build

# Run tests
cargo test --lib
```

---

## Build Commands

### Development Build

```bash
export PYO3_PYTHON=$(which python)
cargo build
```

### Release Build

```bash
export PYO3_PYTHON=$(which python)
cargo build --release
```

### Check (No Build)

```bash
cargo check
```

### Format

```bash
cargo fmt
```

### Lint

```bash
cargo clippy
```

---

## Testing

### Rust Unit Tests

```bash
# All unit tests
cargo test --lib

# Specific module
cargo test --lib sandbox::
cargo test --lib coverage::
cargo test --lib analysis::
cargo test --lib graph::
```

### Rust Integration Tests

```bash
# All integration tests
cargo test --test '*'

# Specific test file
cargo test --test phase4_integration
cargo test --test toxicity_integration
cargo test --test loader_integration

# Physics check (requires sudo)
sudo -E cargo test --test physics_check -- --ignored
```

### Python Gauntlet Tests

```bash
# All gauntlet tests
python -m pytest tests/gauntlet_phase*/

# Specific phase
python -m pytest tests/gauntlet_phase1/ -v
python -m pytest tests/gauntlet_phase2/ -v
python -m pytest tests/gauntlet_phase5_1/ -v  # Coverage
python -m pytest tests/gauntlet_phase5_2/ -v  # Sandbox
python -m pytest tests/gauntlet_phase5_4/ -v  # Allocator
```

---

## Project Structure

```
tach-core/
  src/
    main.rs           # CLI entry point
    lib.rs            # Module exports

    # Discovery & Analysis
    discovery.rs      # AST-based test discovery
    analysis.rs       # Local toxicity detection
    graph.rs          # ToxicityGraph, propagation
    resolver.rs       # Fixture resolution

    # Execution
    zygote.rs         # Process lifecycle, FFI
    scheduler.rs      # Test dispatch
    protocol.rs       # IPC messages

    # Isolation & Security
    sandbox.rs        # Landlock + Seccomp
    isolation.rs      # Namespaces + OverlayFS

    # Memory Management
    snapshot.rs       # userfaultfd, golden pages
    allocator.rs      # Jemalloc integration

    # Observability
    coverage.rs       # Ring buffers, aggregator
    reporter.rs       # Output formatting

    # Support
    config.rs         # Configuration loading
    loader.rs         # Bytecode compilation
    environment.rs    # Environment injection
    tach_harness.py   # Python test harness

  rust_tests/         # Rust integration tests
    physics_check.rs
    snapshot_integration.rs
    loader_integration.rs
    toxicity_integration.rs
    tagging_integrity.rs
    phase4_integration.rs

  tests/              # Python test fixtures
    gauntlet_phase1/  # Memory reset verification
    gauntlet_phase2/  # Loader tests
    gauntlet_phase5/  # Hot reload tests
    gauntlet_phase5_1/ # Coverage tests
    gauntlet_phase5_2/ # Sandbox tests
    gauntlet_phase5_4/ # Allocator tests
    benchmark/        # Performance tests

  docs/               # Documentation
    architecture/     # Architecture docs
    configuration.md
    development.md
    troubleshooting.md
    api-reference.md

  .tach/              # Generated cache (gitignored)
    cache/            # Bytecode cache
```

---

## Key Files

| File                  | Purpose                            |
| :-------------------- | :--------------------------------- |
| `src/zygote.rs`       | Process lifecycle, worker spawning |
| `src/sandbox.rs`      | Landlock + Seccomp implementation  |
| `src/coverage.rs`     | Zero-overhead coverage collection  |
| `src/allocator.rs`    | Jemalloc configuration             |
| `src/snapshot.rs`     | userfaultfd memory snapshots       |
| `src/config.rs`       | Configuration and CLI              |
| `src/tach_harness.py` | Python test harness                |

---

## Git Workflow

### Commit Message Format

```
<type>: <short description>

<optional body>

Co-Authored-By: Claude <noreply@anthropic.com>
```

### Commit Types

| Type        | Description               |
| :---------- | :------------------------ |
| `feat:`     | New feature               |
| `fix:`      | Bug fix                   |
| `docs:`     | Documentation only        |
| `test:`     | Adding or modifying tests |
| `refactor:` | Code restructure          |
| `chore:`    | Maintenance, dependencies |
| `perf:`     | Performance improvement   |

### Example

```bash
git commit -m "feat: add coverage buffer overflow detection

Adds overflow counter to ring buffer header and exposes
get_coverage_overflow() FFI function.

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Debug Commands

### Check Kernel Version

```bash
uname -r
```

### Check Landlock Support

```bash
cat /sys/kernel/security/lsm | grep landlock
```

### Check Seccomp Support

```bash
grep CONFIG_SECCOMP /boot/config-$(uname -r)
```

### Trace Syscalls

```bash
strace -f ./target/release/tach-core . 2>&1 | head -100
```

### Check Python Version

```bash
python --version
```

### Verify Jemalloc

```bash
./target/release/tach-core --help 2>&1 | grep -i jemalloc
```

---

## Performance Profiling

### With perf

```bash
perf record -g ./target/release/tach-core .
perf report
```

### Lock Contention

```bash
perf lock record ./target/release/tach-core .
perf lock report
```

### Memory Usage

```bash
/usr/bin/time -v ./target/release/tach-core .
```

---

## Common Development Tasks

### Adding a New FFI Function

1. Add function in `src/zygote.rs`:

   ```rust
   #[pyfunction]
   fn my_function(py: Python) -> PyResult<()> {
       Ok(())
   }
   ```

2. Register in module:

   ```rust
   m.add_function(wrap_pyfunction!(my_function, m)?)?;
   ```

3. Use in `tach_harness.py`:
   ```python
   tach_rust.my_function()
   ```

### Adding a New Test Phase

1. Create directory: `tests/gauntlet_phaseN/`
2. Add test files: `test_*.py`
3. Update CI workflow if needed

### Modifying the Protocol

1. Update structs in `src/protocol.rs`
2. Update serialization if needed
3. Update Python harness if needed
4. Add integration tests

---

## Troubleshooting Build Issues

See [Troubleshooting](troubleshooting.md) for common issues.

---

## Related Documentation

- [Architecture Overview](architecture/overview.md)
- [Configuration](configuration.md)
- [Troubleshooting](troubleshooting.md)
