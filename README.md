<div align="center">

# Tach

**A Snapshot-Hypervisor for Python Tests**

[![Version](https://img.shields.io/badge/Version-0.1.0_Alpha-red)](CHANGELOG.md)
[![Rust](https://img.shields.io/badge/Rust-1.75+-orange?logo=rust)](https://www.rust-lang.org/)
[![Python](https://img.shields.io/badge/Python-3.10+-blue?logo=python)](https://www.python.org/)
[![Linux](https://img.shields.io/badge/Platform-Linux-green?logo=linux)](https://kernel.org/)
[![License](https://img.shields.io/badge/License-MIT-purple)](LICENSE)

_Replace pytest's execution model with microsecond-scale memory snapshots._

> **Alpha Release**: APIs may change. Not recommended for production use. See [CHANGELOG.md](CHANGELOG.md) for roadmap.

</div>

---

## Overview

Tach is a **Runtime Hypervisor for Python Tests**. It abandons the traditional process creation model (`fork()` or `spawn()`) in favor of **Snapshot/Restore** architecture using Linux `userfaultfd`.

Instead of creating a new process for every test (~2ms + import time), Tach creates a process **once**, captures a memory snapshot, runs a test, and **restores** the memory state in **less than 50 microseconds**.

### The Problem

Traditional test runners suffer from three fundamental performance bottlenecks:

1. **Import Tax**: Python module imports are expensive. `import pandas` takes 200ms+.
2. **Fork Safety**: `fork()` copies locked mutexes from background threads, causing deadlocks.
3. **Allocator Churn**: Python's `obmalloc` fragments memory, making snapshots unstable.

### The Tach Solution

```mermaid
flowchart LR
    subgraph Traditional["TRADITIONAL (pytest-xdist)"]
        T1[Fork] --> T2[Import] --> T3[Run Test] --> T4[Exit]
        T5[Fork] --> T6[Import] --> T7[Run Test] --> T8[Exit]
    end

    subgraph Tach["TACH HYPERVISOR"]
        Z1[Initialize Once] --> Z2[Snapshot]
        Z2 --> Z3[Run Test 1]
        Z3 --> Z4[Reset 50us]
        Z4 --> Z5[Run Test 2]
        Z5 --> Z6[Reset 50us]
        Z6 --> Z7[Run Test N...]
    end
```

| Metric            | pytest (Standard)  | Tach (Hypervisor)  |
| :---------------- | :----------------- | :----------------- |
| **Reset Latency** | ~200ms             | **< 50us**         |
| **Throughput**    | 1x                 | **100x+**          |
| **Fork Safety**   | Unsafe (Deadlocks) | Safe (Lock Reset)  |
| **Security**      | None               | Landlock + Seccomp |

---

## Quick Start

```bash
# Clone and build
git clone https://github.com/user/tach-core.git && cd tach-core
python -m venv .venv && source .venv/bin/activate && pip install pytest
export PYO3_PYTHON=$(which python) && cargo build --release

# Run tests
./target/release/tach-core .

# With coverage (Python 3.12+)
./target/release/tach-core --coverage .
```

---

## System Requirements

| Requirement    | Specification                                  |
| :------------- | :--------------------------------------------- |
| **OS**         | Linux Kernel 5.13+ (Ubuntu 22.04+, Fedora 34+) |
| **Python**     | 3.10+ (3.12+ for PEP 669 coverage)             |
| **Rust**       | 1.75+                                          |
| **Privileges** | `CAP_SYS_PTRACE` or root                       |

---

## Project Structure

Tach uses a modular architecture organized into 5 domain-based subdirectories:

```
src/
├── core/           # Infrastructure
│   ├── allocator.rs    # Jemalloc integration
│   ├── config.rs       # Configuration engine
│   ├── environment.rs  # Environment variable handling
│   ├── lifecycle.rs    # Process lifecycle management
│   ├── protocol.rs     # Binary IPC protocol
│   └── signals.rs      # Signal handling
│
├── discovery/      # Test discovery
│   ├── scanner.rs      # AST-based test scanning
│   ├── resolver.rs     # Fixture resolution
│   ├── loader.rs       # Zero-copy bytecode loading
│   ├── graph.rs        # Dependency graph (petgraph)
│   └── analysis.rs     # Toxicity analysis
│
├── execution/      # Test execution
│   ├── scheduler.rs    # Dual-queue test scheduler
│   ├── watch.rs        # File watch mode
│   └── zygote.rs       # Process spawning and worker pool
│
├── isolation/      # Process isolation
│   ├── namespace.rs    # Linux namespaces
│   ├── sandbox.rs      # Landlock + Seccomp (Iron Dome)
│   └── snapshot.rs     # userfaultfd memory snapshots
│
├── reporting/      # Result reporting
│   ├── reporter.rs     # Progress and output formatting
│   ├── junit.rs        # JUnit XML generation
│   ├── logcapture.rs   # stdout/stderr capture
│   ├── debugger.rs     # Debug output
│   └── coverage.rs     # PEP 669 coverage collection
│
├── lib.rs          # Library entry point (re-exports all modules)
├── main.rs         # CLI entry point
└── tach_harness.py # Python test harness
```

All modules are re-exported at the top level via `lib.rs` for backward compatibility.

---

## Architecture

Tach consists of 5 domain modules with interconnected subsystems:

```mermaid
flowchart TB
    subgraph Supervisor["RUST SUPERVISOR"]
        subgraph Core["core/"]
            Config["Config Engine"]
            Protocol["IPC Protocol"]
            Allocator["Allocator<br/>(Jemalloc)"]
        end

        subgraph DiscoveryMod["discovery/"]
            Scanner["Scanner<br/>(rustpython-parser)"]
            Analysis["Toxicity Analyzer<br/>(petgraph)"]
            Loader["Zero-Copy Loader<br/>(PyMarshal)"]
        end

        subgraph Execution["execution/"]
            Scheduler["Test Scheduler<br/>(Dual-Queue)"]
            Zygote["Zygote<br/>(Worker Pool)"]
        end

        subgraph IsolationMod["isolation/"]
            Sandbox["Iron Dome<br/>(Landlock+Seccomp)"]
            Namespace["Namespaces<br/>(OverlayFS)"]
            Snapshot["Physics Engine<br/>(userfaultfd)"]
        end

        subgraph Reporting["reporting/"]
            Reporter["Reporter<br/>(indicatif)"]
            Coverage["Coverage<br/>(PEP 669)"]
        end
    end

    subgraph Worker["PYTHON WORKER"]
        Harness["Test Harness<br/>(tach_harness.py)"]
    end

    Scanner --> Analysis --> Scheduler
    Loader --> Zygote
    Scheduler --> Zygote
    Zygote --> Sandbox --> Namespace --> Harness
    Harness --> Coverage
    Snapshot <--> Worker
    Allocator --> Snapshot
```

### Documentation

Detailed technical documentation for each subsystem:

| Document                                           | Description                                     |
| :------------------------------------------------- | :---------------------------------------------- |
| **Architecture**                                   |                                                 |
| [Overview](docs/architecture/overview.md)          | System architecture and component interactions  |
| [Discovery Engine](docs/architecture/discovery.md) | AST-based test discovery with rustpython-parser |
| [Zero-Copy Loader](docs/architecture/loader.md)    | Bytecode compilation and injection              |
| [Toxicity Analysis](docs/architecture/toxicity.md) | Module toxicity detection and propagation       |
| [Physics Engine](docs/architecture/snapshot.md)    | userfaultfd memory snapshots                    |
| [Zygote Lifecycle](docs/architecture/zygote.md)    | Process management and worker spawning          |
| [Iron Dome](docs/architecture/sandbox.md)          | Landlock and Seccomp security                   |
| [Isolation](docs/architecture/isolation.md)        | Namespaces and OverlayFS                        |
| [Coverage System](docs/architecture/coverage.md)   | PEP 669 ring buffer coverage                    |
| [Fixture Resolver](docs/architecture/resolver.md)  | Fixture discovery and resolution                |
| [Allocator](docs/architecture/allocator.md)        | Jemalloc integration                            |
| [IPC Protocol](docs/architecture/protocol.md)      | Binary protocol and message format              |
| [Scheduler](docs/architecture/scheduler.md)        | Test scheduling and dispatch                    |
| [Reporter](docs/architecture/reporter.md)          | Output formatting and progress                  |
| **Reference**                                      |                                                 |
| [Configuration](docs/configuration.md)             | CLI, pyproject.toml, environment variables      |
| [Development](docs/development.md)                 | Build, test, project structure                  |
| [Troubleshooting](docs/troubleshooting.md)         | Common issues and debug commands                |
| [API Reference](docs/api-reference.md)             | FFI functions and data structures               |

---

## CLI Usage

```bash
# Run all tests
tach-core .

# Run specific file
tach-core tests/test_example.py

# List tests without running
tach-core list .

# Enable coverage
tach-core --coverage .

# JSON output
tach-core --format json .

# JUnit XML report
tach-core --junit-xml results.xml .

# Disable sandbox (development)
tach-core --no-isolation .

# Watch mode
tach-core --watch .
```

---

## Configuration

Configure via `pyproject.toml`:

```toml
[tool.tach]
test_pattern = "test_*.py"
timeout = 60
workers = 4

[tool.tach.coverage]
enabled = true
source = ["src"]
omit = ["**/test_*"]
format = "lcov"

[tool.pytest_env]
DATABASE_URL = "sqlite:///:memory:"
```

See [Configuration Reference](docs/configuration.md) for full details.

---

## Feature Completion

| Component                   | Status   |
| :-------------------------- | :------- |
| Physics Check (userfaultfd) | Complete |
| Zero-Copy Loader            | Complete |
| Toxicity Filter             | Complete |
| Worker Loop                 | Complete |
| Coverage (PEP 669)          | Complete |
| Iron Dome (Sandbox)         | Complete |
| Hot Reload                  | Complete |
| Allocator (Jemalloc)        | Complete |
| Coverage Resolution         | Complete |
| Configuration Engine        | Complete |
| Progress Reporter           | Complete |
| Security Hardening          | Complete |

See [CHANGELOG.md](CHANGELOG.md) for version history.

---

## Key Technical Features

- **Zero-Copy Module Loading**: Bypasses `importlib` entirely via `PyMarshal_ReadObjectFromString`
- **userfaultfd Snapshots**: Sub-50us memory reset via `madvise(MADV_DONTNEED)`
- **Landlock + Seccomp**: Defense-in-depth sandbox for worker processes
- **PEP 669 Coverage**: Lock-free ring buffer with `memfd_create`
- **Jemalloc Integration**: Deterministic heap via `mallctl` tcache flush
- **Toxicity Propagation**: Fixed-point algorithm over petgraph dependency graph
- **Django Integration**: Automatic transaction rollback and connection pooling
- **Async Support**: Built-in asyncio loop management for coroutine tests

---

## Security Hardening

Tach implements comprehensive security hardening across all subsystems:

### Memory Safety

| Fix                             | Description                                                                        |
| :------------------------------ | :--------------------------------------------------------------------------------- |
| **Static Mut Elimination**      | Replaced `static mut` with `OnceLock`/`Mutex` for thread-safe global state         |
| **Dangling Pointer Prevention** | Fixed CString lifetime issues in FFI calls to prevent use-after-free               |
| **TOCTOU Race Fix**             | Lock-free CAS loop in ring buffer prevents race conditions                         |
| **Mutex Poisoning Recovery**    | All 37 mutex locks use `unwrap_or_else(\|e\| e.into_inner())` for crash resilience |

### Syscall Security

| Fix                      | Description                                                                    |
| :----------------------- | :----------------------------------------------------------------------------- |
| **Seccomp Hardening**    | Added `ptrace`, `mount`, `umount2`, `unshare`, `setns` to syscall blacklist    |
| **Landlock TOCTOU**      | Removed `path.exists()` check, handle `ENOENT` atomically                      |
| **Environment Denylist** | Blocks 11 dangerous env vars: `LD_PRELOAD`, `PYTHONPATH`, `PYTHONMALLOC`, etc. |

### Performance Optimizations

| Fix                            | Description                                                      |
| :----------------------------- | :--------------------------------------------------------------- |
| **RwLock for Read-Heavy Data** | `code_map` uses `RwLock` instead of `Mutex` for concurrent reads |
| **Zero-Copy Data Extraction**  | `take_data()` uses `std::mem::take()` to avoid HashMap cloning   |
| **Pre-sized Collections**      | Thread-local `HashSet` pre-allocated with 1024 capacity          |

### Test Coverage

Added 50+ regression tests across critical subsystems:

- `namespace.rs`: 15 tests for path logic, overlay options, isolation bypass
- `logcapture.rs`: 20 tests for memfd operations, read/clear, fd lifecycle
- `scheduler.rs`: 15 tests for queue separation, priority dispatch, slot calculation

---

## License

MIT License. See [LICENSE](LICENSE) for details.

---

<div align="center">

**Built with Rust for performance and reliability.**

[Documentation](docs/) | [Issues](https://github.com/user/tach-core/issues)

</div>
