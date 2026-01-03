# Changelog

All notable changes to Project Tach will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.0.0] - 2026-01-03

### Gold Release

This is the first stable release of Tach - the Hypervisor-Accelerated Python Test Runner.

### Highlights

- **Complete Restoration Physics Engine**: Bit-perfect snapshot/restore of Python interpreter state
- **Zero-Copy Test Execution**: Workers inherit pre-initialized Python via fork, no startup overhead
- **Iron Dome Sandbox**: Landlock + Seccomp hardening with graceful kernel degradation
- **pytest Compatibility**: Drop-in replacement with identical command-line arguments

### What's New

- **Time Saved Metric**: Shows estimated time saved vs cold-start execution
- **Pre-commit Hook**: Local CI checks before push
- **Codebase Cleanup**: Removed internal phase nomenclature, now uses semantic versioning

### Core Features

#### Test Discovery

- Static AST-based test discovery (no Python execution during discovery)
- Fixture resolution with topological dependency ordering
- Parametrized test expansion with proper deduplication
- Class-scoped and module-scoped fixture support

#### Execution Engine

- Zygote fork-server pattern for instant worker spawning
- userfaultfd memory snapshots for sub-millisecond restore
- Toxicity classification for safe vs unsafe tests
- Parallel worker pool with automatic CPU detection

#### Memory Restoration

- TLS (Thread-Local Storage) capture and restore
- BSS/Heap split-brain prevention
- Stack restoration via ptrace
- Self-calibrating mimalloc offset discovery

#### Sandbox (Iron Dome)

- Landlock filesystem restrictions (ABI V1+)
- Seccomp syscall filtering (blacklist approach)
- Safe workers: Full Iron Dome (Landlock + Seccomp)
- Toxic workers: Landlock only (subprocess support)

#### Coverage

- Zero-overhead bytecode instrumentation
- Ring buffer collection via memfd
- LCOV/JSON output formats

#### Reporting

- Progress bar for interactive terminals
- Dots reporter for CI environments
- JUnit XML output for CI integration
- Time Saved metric showing initialization overhead reduction

### CLI Interface

```
tach [OPTIONS] [PATH] [COMMAND]

Commands:
  test       Run tests (default)
  list       List discovered tests
  self-test  Run system diagnostics
  version    Show version information

Options:
  -n <WORKERS>     Number of parallel workers (default: auto)
  -x, --exitfirst  Exit on first failure
  --maxfail <N>    Exit after N failures
  -v, -vv          Increase verbosity
  -q               Quiet mode
  -k <EXPR>        Filter tests by keyword expression
  -m <MARKERS>     Filter tests by markers
  --coverage       Enable coverage collection
  --no-isolation   Disable worker isolation
  --junit-xml <F>  Write JUnit XML report
  --json           Output results as JSON
  --watch          Watch mode for file changes
```

### System Requirements

- Linux 5.13+ (for Landlock ABI V1)
- x86_64 or aarch64 architecture
- Python 3.8+ with libpython
- userfaultfd privileges (CAP_SYS_PTRACE or `vm.unprivileged_userfaultfd=1`)

Run `tach self-test` to verify system compatibility.

---

## Development History

> The following documents the internal development phases that led to v1.0.0.
> This history is preserved for roadmap and architectural reference.

### Pre-1.0 Development Phases

| Version | Internal Name | Key Deliverables                           |
| ------- | ------------- | ------------------------------------------ |
| 0.1.x   | Discovery     | AST-based test discovery, static analysis  |
| 0.2.x   | Zygote        | Process initialization, fork-based cloning |
| 0.3.x   | Snapshot      | userfaultfd memory snapshots               |
| 0.4.x   | Workers       | Worker pool, IPC, result collection        |
| 0.5.1   | Coverage      | Zero-overhead bytecode coverage            |
| 0.5.2   | Iron Dome     | Landlock + Seccomp sandbox hardening       |
| 0.5.3   | Hot Reload    | sys.modules cleanup for test isolation     |
| 0.5.4   | Allocator     | jemalloc integration                       |
| 0.8.x   | Restoration   | TLS capture, BSS/Heap sync, stack restore  |
| 0.9.x   | CLI           | pytest-compatible command line interface   |

### Key Technical Learnings

- **Clone syscall**: Never block in Seccomp - Python threading requires it
- **Landlock ABI**: Use V1 minimum for kernel 5.13+ compatibility
- **Path canonicalization**: Always canonicalize before adding Landlock rules
- **Seccomp blacklist**: Safer than allowlist - don't break unknown syscalls
- **Graceful degradation**: Log warnings, never crash on unsupported kernels
- **Toxic workers**: Need subprocess support, so bypass Seccomp

---

## Pre-Release Versions

### [0.9.0-beta] - 2026-01-03

#### pytest Compatibility Layer

- pytest-xdist compatible `-n WORKERS`
- Test selection: `-k EXPRESSION` for keyword filtering
- Marker filtering: `-m MARKERS` for pytest markers
- Fail-fast: `-x` / `--exitfirst` and `--maxfail=N`
- Verbosity controls and quiet mode
- Coverage flags: `--coverage`, `--cov PATH`

#### FD Teleporter

- `FdTeleportRequest` struct for batch FD transfers
- SCM_RIGHTS transmission for file descriptor handover
- Ghost Close prevention via `mem::forget()`

### [0.8.5-alpha] - 2026-01-03

#### Vital Types Registry

- Detection for non-serializable fixtures (sockets, DB connections)
- CRITICAL warnings when vital types are degraded

#### Jitter Benchmark

- P99 latency histogram over 10K cycles
- Nanosecond precision timing
- DTV counter verification

### [0.8.0-alpha] - 2026-01-03

#### Restoration Physics Engine

- TLS capture/restore for Thread-Local Storage preservation
- Dynamic TLS boundary detection from `/proc/pid/maps`
- Self-calibrating offset discovery for mimalloc (Python 3.13+)
- Unified `SnapshotManager` with UFFD handling
- BSS/Heap split-brain prevention

#### Iron Dome Sandbox

- Landlock filesystem sandboxing
- Seccomp syscall filtering
- Safe/toxic worker differentiation

#### Testing

- Ghost Hunt: 1000-cycle RSS stability test
- BSS/Heap split-brain validation
- Calibration integration tests

---

[1.0.0]: https://github.com/NikkeTryHard/tach-core/releases/tag/v1.0.0
[0.9.0-beta]: https://github.com/NikkeTryHard/tach-core/releases/tag/v0.9.0-beta
[0.8.5-alpha]: https://github.com/NikkeTryHard/tach-core/releases/tag/v0.8.5-alpha
[0.8.0-alpha]: https://github.com/NikkeTryHard/tach-core/releases/tag/v0.8.0-alpha
