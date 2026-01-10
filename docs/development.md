# Development Guide

Guide for building, testing, and contributing to Tach - the Runtime Hypervisor for Python tests.

---

## Prerequisites

| Requirement | Version                    | Notes                           |
| :---------- | :------------------------- | :------------------------------ |
| Rust        | 1.88+                      | Async traits, Rust 2024 Edition |
| Python      | 3.10+ (3.12+ for coverage) | Coverage uses PEP 669           |
| Linux       | Kernel 5.13+               | Landlock filesystem isolation   |
| Build tools | gcc, make, autoconf        | Jemalloc compilation            |
| iproute2    | Any                        | Network namespace setup         |

**Optional:** perf (profiling), strace (debugging), valgrind (memory leaks)

---

## Quick Start

```bash
git clone https://github.com/NikkeTryHard/tach-core.git && cd tach-core
python -m venv .venv && source .venv/bin/activate && pip install pytest
export PYO3_PYTHON=$(which python) && cargo build
cargo test --lib
```

### Environment Variables

| Variable            | Purpose                           |
| :------------------ | :-------------------------------- |
| `PYO3_PYTHON`       | Python interpreter path for PyO3  |
| `TACH_NO_ISOLATION` | Skip filesystem/network isolation |
| `TACH_FORMAT`       | Output format (human/json)        |
| `TACH_COVERAGE`     | Enable coverage collection        |
| `MALLOC_CONF`       | Jemalloc production config        |

**Production Jemalloc:**

```bash
MALLOC_CONF="background_thread:false,dirty_decay_ms:0,muzzy_decay_ms:0" ./target/release/tach-core
```

---

## Build Commands

```bash
export PYO3_PYTHON=$(which python)
cargo build                    # Development
cargo build --release          # Release
cargo check                    # Check only
cargo fmt                      # Format
cargo clippy                   # Lint
cargo fmt --check && cargo clippy -- -D warnings && cargo test --lib  # Full CI
```

---

## Testing

| Category        | Command                           | Purpose                   |
| :-------------- | :-------------------------------- | :------------------------ |
| Unit Tests      | `cargo test --lib`                | Pure logic, no OS mocking |
| Integration     | `cargo test --test '*'`           | Real Zygotes/Workers      |
| Property Tests  | `cargo test --test 'proptest*'`   | Randomized input fuzzing  |
| Fuzz Tests      | `cargo fuzz run <target>`         | Crash/panic discovery     |
| Golden Tests    | `pytest tests/regression/golden/` | Output stability          |
| Perf Regression | `pytest tests/regression/perf/`   | Timing/memory baselines   |
| Python Gauntlet | `pytest tests/gauntlet*/`         | End-to-end through tach   |

### Rust Unit Tests

```bash
cargo test --lib                    # All unit tests
cargo test --lib sandbox::          # Sandbox/Iron Dome
cargo test --lib coverage::         # Coverage ring buffer
cargo test --lib analysis::         # Toxicity analysis
cargo test --lib graph::            # Toxicity graph
cargo test --lib namespace::        # Namespace isolation
cargo test --lib logcapture::       # Log capture
cargo test --lib scheduler::        # Scheduler
cargo test --lib config::           # Configuration engine
cargo test --lib reporter::         # Progress bar/reporter
```

### Rust Integration Tests

```bash
cargo test --test '*'                                    # All
cargo test --test phase4_integration                     # Specific test
cargo test --test sandbox_enforcement                    # Sandbox only
cargo test --test 'proptest*'                            # Property tests
sudo -E cargo test --test physics_check -- --ignored    # Physics (requires sudo)
```

### Fuzz Tests

```bash
# Requires nightly toolchain
cargo +nightly fuzz run fuzz_config_toml -- -max_total_time=60
cargo +nightly fuzz run fuzz_protocol_deserialize
cargo +nightly fuzz run fuzz_scanner_paths
```

### Python Gauntlet Tests

```bash
pytest tests/gauntlet/ -v          # General gauntlet tests
pytest tests/gauntlet_db/ -v       # Database integration
pytest tests/gauntlet_numpy/ -v    # NumPy compatibility
pytest tests/gauntlet_coverage/ -v # Coverage tests
pytest tests/gauntlet_phase*/ -v   # All phase tests
pytest tests/gauntlet_012/ -v      # Version-specific (0.1.2)
pytest tests/regression/ -v        # Regression suite
```

**Jemalloc tests** (disabled by default for WSL2 stability):

```bash
cargo test --lib allocator -- --ignored
```

---

## Project Structure

```
tach-core/
  src/
    main.rs, lib.rs, tach_harness.py
    core/         # allocator, config, environment, lifecycle, protocol, signals
    discovery/    # scanner, resolver, loader, graph, analysis
    execution/    # scheduler, watch, zygote
    isolation/    # namespace, sandbox, snapshot
    reporting/    # reporter, junit, logcapture, debugger, coverage

  rust_tests/     # Integration tests
  tests/          # Python gauntlet tests (phase1-5)
  docs/           # Documentation
  .tach/          # Generated cache (gitignored)
```

---

## Key Files

| File                          | Purpose                                 |
| :---------------------------- | :-------------------------------------- |
| `src/execution/zygote.rs`     | Process lifecycle, worker spawning, FFI |
| `src/isolation/sandbox.rs`    | Landlock + Seccomp (Iron Dome)          |
| `src/isolation/namespace.rs`  | Linux Namespaces + OverlayFS            |
| `src/reporting/coverage.rs`   | Zero-overhead coverage                  |
| `src/reporting/logcapture.rs` | memfd-based stdout/stderr capture       |
| `src/core/allocator.rs`       | Jemalloc configuration                  |
| `src/isolation/snapshot.rs`   | userfaultfd memory snapshots            |
| `src/core/config.rs`          | Configuration, CLI, env denylist        |
| `src/execution/scheduler.rs`  | Dual-path test scheduling               |

---

## Security Hardening

### Memory Safety Patterns

```rust
// BAD: static mut causes UB
static mut COUNTER: u32 = 0;

// GOOD: Use atomics or Mutex
static COUNTER: AtomicU32 = AtomicU32::new(0);
static STATE: Mutex<Option<State>> = Mutex::new(None);
```

```rust
// BAD: TOCTOU race condition
if path.exists() { let fd = PathFd::new(path)?; }

// GOOD: Atomic open with error handling
match PathFd::new(path) {
    Ok(fd) => { /* use fd */ }
    Err(e) => { /* handle */ }
}
```

### Syscall Security

**Seccomp Blacklist** (blocks dangerous syscalls, allows Python threading):

| Category  | Blocked                                | Reason                 |
| :-------- | :------------------------------------- | :--------------------- |
| Network   | socket, bind, connect, listen, accept  | Prevent network access |
| Process   | fork, vfork, execve, execveat          | Prevent spawning       |
| Privilege | ptrace, mount, umount2, unshare, setns | Prevent escape         |

**Critical:** `clone`/`clone3` NOT blocked - Python threading requires them.

**Landlock Filesystem:**

| Access     | Paths                                      | Purpose        |
| :--------- | :----------------------------------------- | :------------- |
| READ-ONLY  | Project root, /usr, /lib, /bin, /etc, /dev | System libs    |
| READ-WRITE | /tmp, /run/tach/worker\_{id}               | Temp files     |
| DENY       | Everything else                            | Default policy |

**Environment Denylist:** `LD_PRELOAD`, `LD_LIBRARY_PATH`, `PYTHONPATH`, `PYTHONHOME`, `PATH`, `HOME`

---

## Testing Guidelines

### Common Patterns

```rust
// Mutex poisoning recovery
let guard = self.data.lock().unwrap_or_else(|e| e.into_inner());

// Environment variable isolation
let original = std::env::var("MY_VAR").ok();
std::env::set_var("MY_VAR", "test_value");
// ... test ...
match original {
    Some(v) => std::env::set_var("MY_VAR", v),
    None => std::env::remove_var("MY_VAR"),
}
```

### Naming Convention

```python
# Python: test_<component>.py, test_<letter>_<description>
def test_a_kernel_version_detection():
```

```rust
// Rust: test_<component>_<behavior>
fn test_worker_base_dir_format() { }
```

---

## Git Workflow

```
<type>: <short description>

<optional body>

Co-Authored-By: Claude <noreply@anthropic.com>
```

**Types:** `feat:`, `fix:`, `docs:`, `test:`, `refactor:`, `chore:`, `perf:`

---

## Debug Commands

```bash
uname -r                                              # Kernel version
cat /sys/kernel/security/lsm | grep landlock          # Landlock support
grep CONFIG_SECCOMP /boot/config-$(uname -r)          # Seccomp support
strace -f ./target/release/tach-core . 2>&1 | head -100  # Trace syscalls
cat /proc/sys/kernel/unprivileged_userns_clone        # User namespaces
```

---

## Performance Profiling

```bash
perf record -g ./target/release/tach-core . && perf report  # CPU profiling
perf lock record ./target/release/tach-core . && perf lock report  # Lock contention
/usr/bin/time -v ./target/release/tach-core .         # Memory usage
cargo flamegraph --bin tach-core -- .                 # Flamegraph
```

---

## Common Development Tasks

### Adding FFI Function

```rust
// 1. Add in src/execution/zygote.rs
#[pyfunction]
fn my_function(py: Python) -> PyResult<()> { Ok(()) }

// 2. Register: m.add_function(wrap_pyfunction!(my_function, m)?)?;
```

```python
# 3. Use in tach_harness.py
tach_rust.my_function()
```

### Adding Test Phase

1. Create `tests/gauntlet_phaseN/`
2. Add `test_*.py` files
3. Update CI if needed

### Adding Reporter

Implement `Reporter` trait in `src/reporting/reporter.rs`:

- `on_run_start`, `on_test_start`, `on_test_finished`, `on_run_finished`, `on_error`

---

## Troubleshooting

| Issue                 | Cause            | Solution                             |
| :-------------------- | :--------------- | :----------------------------------- |
| `PYO3_PYTHON` not set | Missing env var  | `export PYO3_PYTHON=$(which python)` |
| `EPERM` on Landlock   | Kernel < 5.13    | Graceful degradation                 |
| `EPERM` on Seccomp    | Bad filter       | Check syscalls, use blacklist        |
| Test hangs            | Clone blocked    | Ensure clone NOT in seccomp          |
| Coverage wrong        | GIL held         | Release GIL during Rust ops          |
| WSL2 instability      | Jemalloc + tests | Jemalloc disabled in `cargo test`    |

---

## Related Documentation

- [README](../README.md) - Project overview and quick start
- [Architecture Overview](architecture/overview.md)
- [Configuration](configuration.md)
- [Troubleshooting](troubleshooting.md)
- [API Reference](api-reference.md)
