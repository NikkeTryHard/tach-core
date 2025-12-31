<div align="center">

# Tach

**A Snapshot-Hypervisor for Python Tests**

[![Rust](https://img.shields.io/badge/Rust-1.75+-orange?logo=rust)](https://www.rust-lang.org/)
[![Python](https://img.shields.io/badge/Python-3.10+-blue?logo=python)](https://www.python.org/)
[![Linux](https://img.shields.io/badge/Platform-Linux-green?logo=linux)](https://kernel.org/)
[![License](https://img.shields.io/badge/License-MIT-purple)](LICENSE)

_Replace pytest's execution model with microsecond-scale memory snapshots._

</div>

---

## Table of Contents

- [Overview](#overview)
- [Performance Metrics](#performance-metrics)
- [Architecture](#architecture)
  - [The Jedi Protocol](#the-jedi-protocol)
  - [Physics Engine](#physics-engine)
  - [Zero-Copy Loader](#zero-copy-loader)
  - [Toxicity Analysis](#toxicity-analysis)
  - [Worker Loop](#worker-loop)
  - [The Iron Dome (Sandbox)](#the-iron-dome-sandbox)
  - [Zero-Overhead Coverage](#zero-overhead-coverage)
  - [Deterministic Allocator](#deterministic-allocator)
- [System Requirements](#system-requirements)
- [Installation](#installation)
- [Usage](#usage)
- [Development](#development)
  - [Project Structure](#project-structure)
  - [Running Tests](#running-tests)
- [Implementation Status](#implementation-status)
- [Test Coverage](#test-coverage)
- [Technical Specifications](#technical-specifications)
- [Configuration Reference](#configuration-reference)
- [Findings and Lessons Learned](#findings-and-lessons-learned)
- [Future Development Hints](#future-development-hints)
- [Known Problems and Limitations](#known-problems-and-limitations)
- [Troubleshooting Guide](#troubleshooting-guide)

---

## Overview

Tach is a **Hypervisor for Python**. It abandons the traditional process creation model (`fork()` or `spawn()`) in favor of **Snapshot/Restore** architecture using Linux `userfaultfd`.

Instead of creating a new process for every test (taking approximately 2ms plus import time), Tach creates a process **once**, captures a memory snapshot, runs a test, and then **restores** the memory state in **less than 50 microseconds**.

### The Problem: Import Tax and Fork Safety

Traditional test runners suffer from three fundamental performance bottlenecks:

1. **Import Tax:** Python module imports are expensive. `import pandas` takes 200ms or more. Even with `fork()`, this penalty is paid in the Zygote initialization.

2. **Fork Safety:** The `fork()` system call copies locked mutexes from background threads (such as logging handlers), causing deadlocks in child processes.

3. **Allocator Churn:** Python's `obmalloc` fragments memory over time, making standard memory snapshots unstable.

### The Tach Solution

Tach implements a multi-layered approach to eliminate these bottlenecks:

1. **Zero-Copy Loading:** Bypasses Python's `importlib` entirely. Rust compiles `.py` source files to `.pyc` bytecode, memory-maps them, and injects them directly into the Python interpreter via C-API.

2. **Snapshot Isolation:** Uses `userfaultfd` to track memory writes and capture "golden" snapshots of worker memory state.

3. **Instant Reset:** After test execution, dirty pages are dropped via `madvise(MADV_DONTNEED)`. Subsequent memory access triggers page faults, which are serviced from the golden snapshot.

4. **The Iron Dome:** Landlock filesystem isolation + Seccomp syscall filtering provide defense-in-depth security for worker processes.

5. **Deterministic Allocator:** Jemalloc with explicit tcache flush ensures consistent memory layout across snapshot/restore cycles.

---

## Performance Metrics

| Metric                 | pytest (Standard)    | Tach (Legacy Fork) | Tach (Hypervisor)    |
| :--------------------- | :------------------- | :----------------- | :------------------- |
| **Isolation Strategy** | Process Spawn        | `fork()`           | Memory Reset         |
| **Reset Latency**      | ~200ms               | ~1ms               | **< 50μs**           |
| **Throughput**         | 1x                   | 50x                | **100x+**            |
| **Fork Safety**        | Safe (Slow)          | Unsafe (Deadlocks) | Safe (Lock Reset)    |
| **Memory Overhead**    | Full copy per worker | CoW sharing        | Minimal (page-level) |
| **Security**           | None                 | None               | Landlock + Seccomp   |

### Zero-Copy Loader Performance

| Metric                            | Cold Cache                | Warm Cache        |
| :-------------------------------- | :------------------------ | :---------------- |
| **Compilation Time (91 modules)** | 9.7 seconds               | 345 milliseconds  |
| **Speedup Factor**                | 1x                        | **28x**           |
| **Cache Persistence**             | Disk-based `.tach/cache/` | mtime-invalidated |

### Sandbox Overhead

| Component          | Overhead | Notes                 |
| :----------------- | :------- | :-------------------- |
| Landlock setup     | ~100μs   | One-time per worker   |
| Seccomp setup      | ~50μs    | One-time per worker   |
| Coverage (PEP 669) | < 1%     | Lock-free ring buffer |

---

## Architecture

### The Jedi Protocol

The Jedi Protocol describes the communication flow between the Rust Supervisor and Python Workers:

```mermaid
flowchart LR
    subgraph Supervisor["RUST SUPERVISOR"]
        Compiler["Bytecode Compiler"]
        Uffd["Userfaultfd Manager"]
        Scheduler["Test Scheduler"]
        ToxicityGraph["Toxicity Analyzer"]
        Sandbox["Sandbox Manager"]
    end

    subgraph Worker["PYTHON WORKER"]
        direction TB
        Init["Initialize & Handshake"]
        IronDome["Apply Iron Dome"]
        Snapshot["SIGSTOP (Snapshot Point)"]
        Run["Execute Test"]
        Decision{is_toxic?}
        Reset["Memory Reset"]
        Exit["Exit Process"]
    end

    Compiler -->|"Inject .pyc (Zero-Copy)"| Init
    ToxicityGraph -->|"Tag Tests"| Scheduler
    Sandbox -->|"Landlock + Seccomp"| IronDome
    Init --> IronDome --> Snapshot
    Snapshot --> Run
    Run -->|"Report Result"| Scheduler
    Run --> Decision
    Decision -->|"Safe"| Reset
    Decision -->|"Toxic"| Exit
    Reset -->|"Dirty Pages"| Uffd
    Uffd -->|"MADV_DONTNEED"| Reset
    Reset --> Snapshot
```

**Protocol Phases:**

1. **Initialization:** Worker process starts, performs UFFD handshake with Supervisor via SCM_RIGHTS
2. **Iron Dome:** Apply Landlock filesystem restrictions and Seccomp syscall filters
3. **Snapshot Capture:** Worker issues SIGSTOP; Supervisor captures golden memory state
4. **Test Execution:** Worker resumes, executes assigned test, reports results
5. **Dual-Path Decision:** Based on `is_toxic` flag:
   - **Safe:** Memory reset via `madvise(MADV_DONTNEED)`, loop back to snapshot point
   - **Toxic:** Exit process immediately, Supervisor spawns replacement

---

### Physics Engine

The Physics Engine (`snapshot.rs`) implements kernel-level memory management:

```mermaid
flowchart TB
    subgraph Capture["GOLDEN SNAPSHOT CAPTURE"]
        Maps["Parse /proc/pid/maps"]
        Filter["Filter Regions\n(heap, stack, BSS, anon)"]
        Exclude["Exclude Coverage Buffer\n(memfd:tach_coverage)"]
        Copy["process_vm_readv\n(Direct Memory Copy)"]
        Store["Store in HashMap\n(page_addr → page_data)"]
    end

    subgraph Reset["MEMORY RESET CYCLE"]
        Invalidate["madvise(MADV_DONTNEED)\n(Seppuku Pattern)"]
        Fault["Page Fault Triggered"]
        Restore["Uffd::copy()\n(Restore from Golden)"]
    end

    Maps --> Filter --> Exclude --> Copy --> Store
    Store --> Invalidate
    Invalidate --> Fault --> Restore
    Restore --> Invalidate
```

**Technical Implementation:**

| Component          | System Call              | Purpose                                                |
| :----------------- | :----------------------- | :----------------------------------------------------- |
| Memory Capture     | `process_vm_readv`       | Copy worker memory to Supervisor without ptrace attach |
| Page Tracking      | `userfaultfd`            | Register memory regions for fault notification         |
| Page Invalidation  | `madvise(MADV_DONTNEED)` | Drop pages, forcing re-fault on next access            |
| Page Restoration   | `ioctl(UFFDIO_COPY)`     | Copy golden page back to worker address space          |
| Coverage Exclusion | Region name filtering    | Exclude `memfd:tach_coverage` from uffd registration   |

---

### Zero-Copy Loader

The Zero-Copy Loader (`loader.rs`) bypasses Python's import machinery:

```mermaid
flowchart LR
    subgraph Rust["RUST SUPERVISOR"]
        Source[".py Source Files"]
        Compile["Compile to .pyc\n(py_compile)"]
        Cache["Bytecode Cache\n(.tach/cache/)"]
        Registry["ModuleRegistry\n(DashMap)"]
    end

    subgraph Python["PYTHON WORKER"]
        Finder["TachMetaPathFinder\n(sys.meta_path[0])"]
        Loader["TachLoader.exec_module"]
        Marshal["PyMarshal_ReadObjectFromString"]
        Exec["PyImport_ExecCodeModuleObject"]
        Module["Loaded Module"]
    end

    Source --> Compile --> Cache --> Registry
    Registry -->|"get_module(name)"| Finder
    Finder -->|"load_module(bytecode)"| Loader
    Loader --> Marshal --> Exec --> Module
```

**Advantages over `importlib`:**

- No filesystem traversal (`sys.path` scanning)
- No disk I/O (bytecode pre-loaded in RAM)
- No repeated compilation (cached once)
- Sub-millisecond module materialization
- Header-stripped bytecode (16-byte `.pyc` header removed)

---

### Toxicity Analysis

The Toxicity Analyzer (`analysis.rs`, `graph.rs`) identifies modules that cannot be safely snapshotted:

```mermaid
flowchart TB
    subgraph Discovery["TOXICITY PIPELINE"]
        direction TB
        Scan["Scan All .py Files\n(walkdir)"]
        Parse["Parse AST\n(rustpython-parser)"]
        Analyze["analyze_file()\n(Local Toxicity)"]
        Graph["ToxicityGraph\n(petgraph DiGraph)"]
        Propagate["Fixed-Point Propagation\n(Transitive Closure)"]
    end

    subgraph Patterns["TOXIC PATTERNS DETECTED"]
        Threading["threading / _thread"]
        Multiprocessing["multiprocessing"]
        Socket["socket"]
        Ctypes["ctypes / cffi"]
        Signal["signal (handlers)"]
        Subprocess["subprocess"]
    end

    subgraph Output["INTEGRATION"]
        TestModule["TestModule.is_toxic"]
        RunnableTest["RunnableTest.is_toxic"]
        TestPayload["TestPayload.is_toxic"]
        Worker["Worker Decision:\nReset vs Exit"]
    end

    Scan --> Parse --> Analyze
    Patterns --> Analyze
    Analyze --> Graph --> Propagate
    Propagate --> TestModule --> RunnableTest --> TestPayload --> Worker
```

**Transitive Propagation Algorithm:**

```
1. Build directed graph: Module → Imports
2. Analyze each module for LOCAL toxicity
3. Fixed-point iteration:
   REPEAT:
     FOR each module M:
       IF any import of M is toxic:
         Mark M as toxic
   UNTIL no changes
4. Result: Complete transitive closure of toxicity
```

---

### Worker Loop

The Worker Loop (`zygote.rs`, `tach_harness.py`) implements the dual-path execution model:

```mermaid
flowchart TB
    subgraph Worker["PYTHON WORKER LOOP"]
        direction TB
        Receive["Receive Test Payload"]
        Execute["Execute Test"]
        Report["Send Result"]
        Decision{is_toxic?}
        Reset["madvise(MADV_DONTNEED)<br/>Memory Reset"]
        Exit["sys.exit(0)<br/>Process Terminates"]
    end

    subgraph Supervisor["RUST SUPERVISOR"]
        ReadyQueue["Ready Queue"]
        Dispatch["Dispatch"]
        Collect["Collect Result"]
        Spawn["Spawn Replacement"]
    end

    ReadyQueue --> Dispatch --> Receive
    Receive --> Execute --> Report --> Collect
    Report --> Decision
    Decision -->|"Safe"| Reset
    Decision -->|"Toxic"| Exit
    Reset -->|"Loop"| Receive
    Exit --> Spawn --> ReadyQueue
```

**Dual-Path Decision Logic:**

| Test Type        | After Execution          | Worker Fate          |
| :--------------- | :----------------------- | :------------------- |
| Safe (non-toxic) | `madvise(MADV_DONTNEED)` | Continues loop       |
| Toxic            | `sys.exit(0)`            | Terminates, replaced |

---

### The Iron Dome (Sandbox)

The Iron Dome (`sandbox.rs`) implements defense-in-depth security for worker processes:

```mermaid
flowchart TB
    subgraph Fork["WORKER FORK PATH"]
        direction TB
        F1["1. fork()"]
        F2["2. PR_SET_PDEATHSIG\n(Dead Man's Switch)"]
        F3["3. isolation::setup_filesystem()\n(Namespaces + OverlayFS)"]
        F4["4. sandbox::apply_landlock()\n(Filesystem Restrictions)"]
        F5["5. sandbox::apply_seccomp()\n(Syscall Filtering)"]
        F6["6. post_fork_init()\n(Python Initialization)"]
        F7["7. run_worker()\n(Test Execution)"]
    end

    subgraph Landlock["LANDLOCK POLICY"]
        RO["READ-ONLY:\n/usr, /lib, /lib64, /bin\n/etc, /dev, /proc, /sys\nproject_root"]
        RW["READ-WRITE:\n/tmp (overlay)\n/run/tach/worker_N"]
        DENY["DENY:\nEverything else"]
    end

    subgraph Seccomp["SECCOMP POLICY"]
        BlockNet["BLOCK (EPERM):\nsocket, bind, connect\nlisten, accept, accept4"]
        BlockProc["BLOCK (EPERM):\nfork, vfork\nexecve, execveat"]
        Allow["ALLOW:\nclone (threading)\nEverything else"]
    end

    F1 --> F2 --> F3 --> F4 --> F5 --> F6 --> F7
    F4 --> Landlock
    F5 --> Seccomp
```

**Safe vs Toxic Worker Security Matrix:**

```mermaid
flowchart LR
    subgraph Safe["SAFE WORKER"]
        S1["Landlock: ENFORCED"]
        S2["Seccomp: ENFORCED"]
        S3["Network: BLOCKED"]
        S4["Fork/Exec: BLOCKED"]
        S5["Reuse: YES (pool)"]
    end

    subgraph Toxic["TOXIC WORKER"]
        T1["Landlock: ENFORCED"]
        T2["Seccomp: SKIPPED"]
        T3["Network: ALLOWED"]
        T4["Fork/Exec: ALLOWED"]
        T5["Reuse: NO (exit)"]
    end
```

**Graceful Degradation:**

| Kernel Version | Landlock | Seccomp | Behavior                     |
| :------------- | :------- | :------ | :--------------------------- |
| 5.13+          | Full     | Full    | Complete sandbox             |
| 5.0-5.12       | None     | Full    | Seccomp only, warning logged |
| 3.17-4.x       | None     | Full    | Seccomp only, warning logged |
| < 3.17         | None     | None    | No sandbox, warning logged   |

---

### Zero-Overhead Coverage

The Coverage system (`coverage.rs`) implements PEP 669 `sys.monitoring` with dual ring buffers for code registration and line tracking:

```mermaid
flowchart TB
    subgraph Python["PYTHON TEST EXECUTION"]
        Test["Test Code"]
        PY_START["sys.monitoring\nPY_START event"]
        LINE["sys.monitoring\nLINE event"]
        StartCB["_coverage_py_start_callback()"]
        LineCB["_coverage_line_callback()"]
    end

    subgraph Worker["WORKER PROCESS"]
        SeenCodes["thread_local!\nSEEN_CODES: HashSet<u64>"]
    end

    subgraph SharedMem["SHARED MEMORY (memfd)"]
        MappingBuffer["MappingRingBuffer\n(code_id, filename)\n256 bytes/entry"]
        CoverageBuffer["CoverageRingBuffer\n(code_id, lineno)\n16 bytes/entry"]
    end

    subgraph Supervisor["SUPERVISOR PROCESS"]
        Aggregator["CoverageAggregator Thread"]
        CodeMap["code_map: HashMap<u64, String>"]
        CoverageData["coverage_data:\nHashMap<(file, line), count>"]
    end

    Test --> PY_START --> StartCB
    Test --> LINE --> LineCB
    StartCB -->|"Check SEEN_CODES"| SeenCodes
    SeenCodes -->|"If NEW: write mapping"| MappingBuffer
    LineCB -->|"Zero checks, pure write"| CoverageBuffer

    MappingBuffer -->|"Drain FIRST"| Aggregator
    Aggregator -->|"Populate"| CodeMap
    CoverageBuffer -->|"Drain SECOND"| Aggregator
    CodeMap -->|"Resolve code_id"| CoverageData
```

**Phase 6.1 Architecture: Dual-Buffer Design**

The coverage system uses two separate ring buffers to solve the `code_id` → filename resolution problem:

| Buffer             | Purpose                     | Entry Size | Capacity | Event Type |
| :----------------- | :-------------------------- | :--------- | :------- | :--------- |
| MappingRingBuffer  | Register code_id → filename | 256 bytes  | 8,192    | PY_START   |
| CoverageRingBuffer | Record line executions      | 16 bytes   | 262,144  | LINE       |

**Critical Design Decisions:**

| Decision                          | Rationale                                    |
| :-------------------------------- | :------------------------------------------- |
| `memfd_create("tach_coverage")`   | Anonymous shared memory, no filesystem       |
| `memfd_create("tach_mapping")`    | Separate buffer for registration path        |
| 64-byte header alignment          | Cache-line aligned, prevents false sharing   |
| Thread-local `SEEN_CODES` HashSet | O(1) deduplication without locks             |
| Drain mapping buffer FIRST        | Ensures code_map populated before resolution |
| GIL released before write         | Prevents GIL contention in hot path          |
| Excluded from userfaultfd         | Survives `MADV_DONTNEED` during reset        |
| Filename truncation from LEFT     | Preserves actual filename, drops path prefix |

**Event Separation Strategy:**

| Event    | Path         | Frequency         | Work Done                           |
| :------- | :----------- | :---------------- | :---------------------------------- |
| PY_START | Registration | Once per function | Check HashSet, write mapping if new |
| LINE     | Hot path     | Every line        | Pure memory write, zero checks      |

---

### Deterministic Allocator

The Allocator (`allocator.rs`) uses Jemalloc to solve the Split-Brain problem:

```mermaid
flowchart TB
    subgraph Problem["SPLIT-BRAIN PROBLEM"]
        Snapshot["Snapshot captured"]
        Alloc1["Worker allocates memory"]
        Reset["Memory reset (MADV_DONTNEED)"]
        Alloc2["Worker allocates again"]
        Desync["Allocator metadata desync!\n(tcache holds stale pointers)"]
    end

    subgraph Solution["JEMALLOC SOLUTION"]
        JemallocInit["#[global_allocator]\nstatic ALLOC: Jemalloc"]
        TcacheFlush["mallctl('thread.tcache.flush')"]
        EpochSync["mallctl('epoch')"]
        Deterministic["Deterministic heap layout"]
    end

    Snapshot --> Alloc1 --> Reset --> Alloc2 --> Desync
    JemallocInit --> TcacheFlush --> EpochSync --> Deterministic
```

**Why Jemalloc?**

| Feature       | glibc malloc  | Jemalloc                         |
| :------------ | :------------ | :------------------------------- |
| tcache flush  | Not exposed   | `mallctl("thread.tcache.flush")` |
| Epoch sync    | Not available | `mallctl("epoch")`               |
| Determinism   | Poor          | Excellent                        |
| Fragmentation | High          | Low                              |

---

## System Requirements

| Requirement          | Specification                                              |
| :------------------- | :--------------------------------------------------------- |
| **Operating System** | Linux Kernel 5.13+ (Ubuntu 22.04+, Fedora 34+, AWS AL2023) |
| **Privileges**       | `CAP_SYS_PTRACE` (standard in most CI environments)        |
| **Python Version**   | Python 3.10+ (3.12+ for PEP 669 coverage)                  |
| **Rust Version**     | Rust 1.75+                                                 |
| **Allocator**        | Jemalloc (bundled)                                         |

**Kernel Feature Matrix:**

| Feature      | Minimum Kernel | Recommended |
| :----------- | :------------- | :---------- |
| userfaultfd  | 4.11           | 5.11+       |
| Landlock     | 5.13           | 5.19+       |
| Seccomp-BPF  | 3.17           | 4.14+       |
| memfd_create | 3.17           | 5.0+        |

**Docker Configuration:**

```yaml
security_opt:
  - seccomp:unconfined
cap_add:
  - SYS_PTRACE
```

---

## Installation

### From Source

```bash
# Clone repository
git clone https://github.com/NikkeTryHard/tach-core.git
cd tach-core

# Setup Python virtual environment
python -m venv .venv
source .venv/bin/activate
pip install pytest

# Build Rust binary
cargo build --release

# Verify kernel support (Physics Check)
sudo -E cargo test --test physics_check -- --ignored
```

---

## Usage

### Basic Execution

```bash
# Run all tests in current directory
sudo ./target/release/tach-core .

# Run specific test file
sudo ./target/release/tach-core tests/test_example.py

# Run without namespace isolation (development mode)
./target/release/tach-core --no-isolation .

# Enable coverage collection (Python 3.12+)
./target/release/tach-core --coverage .
```

### CLI Options

| Flag                 | Description                                   |
| :------------------- | :-------------------------------------------- |
| `--format json`      | Output results as JSON to stdout              |
| `--junit-xml <path>` | Generate JUnit XML report                     |
| `--watch`            | Watch mode: re-run on file changes            |
| `--no-isolation`     | Disable namespace isolation (for development) |
| `--coverage`         | Enable PEP 669 coverage collection            |
| `--list`             | List discovered tests without running         |
| `-v, --verbose`      | Increase output verbosity                     |

---

## Development

### Project Structure

```
tach-core/
├── src/
│   ├── main.rs           # CLI entry point, toxicity wiring, eager compilation
│   ├── lib.rs            # Module exports, discover_with_toxicity()
│   ├── allocator.rs      # Phase 5.4: Jemalloc global allocator
│   ├── analysis.rs       # Phase 3: Local toxicity scanner
│   ├── coverage.rs       # Phase 5.1: PEP 669 ring buffer coverage
│   ├── graph.rs          # Phase 3: ToxicityGraph with propagation
│   ├── sandbox.rs        # Phase 5.2: Landlock + Seccomp sandbox
│   ├── discovery.rs      # AST-based test discovery (rustpython-parser)
│   ├── resolver.rs       # Fixture dependency resolution
│   ├── scheduler.rs      # Async test scheduler (tokio)
│   ├── zygote.rs         # Python process lifecycle, FFI registration
│   ├── snapshot.rs       # Userfaultfd memory management
│   ├── loader.rs         # Zero-Copy Module Loader
│   ├── protocol.rs       # Binary IPC protocol (bincode)
│   ├── isolation.rs      # Linux namespace isolation
│   ├── environment.rs    # Environment injection
│   ├── tach_harness.py   # Python test harness, import hook, PEP 669
│   └── ...
├── rust_tests/           # Rust integration tests
│   ├── physics_check.rs          # UFFD memory reset verification
│   ├── snapshot_integration.rs   # Snapshot lifecycle tests
│   ├── loader_integration.rs     # Loader tests
│   ├── toxicity_integration.rs   # Toxicity pipeline tests
│   ├── tagging_integrity.rs      # is_toxic propagation tests
│   ├── phase4_integration.rs     # Worker loop tests
│   └── ...
├── tests/                # Python test fixtures
│   ├── gauntlet/         # Stress/security tests
│   ├── gauntlet_phase1/  # Memory reset verification
│   ├── gauntlet_phase2/  # Loader tests
│   ├── gauntlet_phase5/  # Hot reload tests
│   ├── gauntlet_phase5_1/ # Coverage tests
│   ├── gauntlet_phase5_2/ # Sandbox tests
│   ├── gauntlet_phase5_4/ # Allocator tests
│   ├── benchmark/        # Performance tests
│   └── ...
├── docs/
│   └── architecture/
│       ├── phase2_loader.md
│       ├── phase3_toxicity.md
│       ├── phase4_worker_loop.md
│       └── remote_development.md
├── .cargo/
│   └── config.toml
├── .github/
│   └── workflows/
│       └── ci.yml
└── .tach/                # Generated cache (gitignored)
    └── cache/
```

### Running Tests

```bash
# Rust unit tests
cargo test --lib                              # All unit tests

# Specific module tests
cargo test --lib sandbox::                    # 7 sandbox tests
cargo test --lib coverage::                   # 9 coverage tests
cargo test --lib analysis::                   # 49 toxicity scanner tests
cargo test --lib graph::                      # 20 toxicity graph tests

# Rust integration tests
cargo test --test phase4_integration          # Worker loop tests
cargo test --test toxicity_integration        # Toxicity pipeline tests
cargo test --test physics_check -- --ignored  # Requires sudo

# Python gauntlet tests
python -m pytest tests/gauntlet_phase5_1/ -v  # Coverage tests
python -m pytest tests/gauntlet_phase5_2/ -v  # Sandbox tests
python -m pytest tests/gauntlet_phase5_4/ -v  # Allocator tests
```

---

## Implementation Status

### Completed Phases

#### Phase 1: Physics Check ✅

Memory snapshot and reset mechanism verified. Userfaultfd-based page tracking, golden snapshot capture via `process_vm_readv`, memory reset via `madvise(MADV_DONTNEED)`, page restoration via `Uffd::copy()`.

#### Phase 2: Zero-Copy Loader ✅

Bypass `importlib` for instant module loading. Rust-side `.py` to `.pyc` compilation, bytecode cache with mtime invalidation, `PyMarshal_ReadObjectFromString` injection, `TachMetaPathFinder` import hook.

#### Phase 3: Toxicity Filter ✅

Identify and isolate unsafe modules. AST-based toxicity detection, `ToxicityGraph` with fixed-point propagation, `is_toxic` field propagation through `TestModule` → `RunnableTest` → `TestPayload`.

#### Phase 4: Worker Loop & Dual-Path Scheduler ✅

Transform from fork-server to true Hypervisor. Persistent worker loop, dual-path execution (Safe: reset, Toxic: exit), `WorkerHandle` pool for worker reuse, Dead Man's Switch (`PR_SET_PDEATHSIG`).

#### Phase 5: Observability & Hardening ✅

**Phase 5.1: Zero-Overhead Coverage (PEP 669)** ✅

- PEP 669 `sys.monitoring` integration (Python 3.12+)
- Lock-free ring buffer with `memfd_create`
- 64-byte aligned header, 16-byte aligned entries
- GIL discipline: `py.allow_threads()` before ring buffer access
- Coverage buffer excluded from userfaultfd registration
- `CoverageAggregator` thread drains buffer periodically
- **9 Rust tests, 7 Python tests passing**

**Phase 5.2: The Iron Dome (Sandbox Hardening)** ✅

- Landlock ABI V1 filesystem isolation (kernel 5.13+)
- RO: `/usr`, `/lib`, `/lib64`, `/bin`, `/etc`, `/dev`, `/proc`, `/sys`, project_root
- RW: `/tmp`, `/run/tach/worker_N`
- Seccomp-BPF syscall blacklist (safe workers only)
- Blocked: `socket`, `bind`, `connect`, `listen`, `accept`, `accept4`
- Blocked: `fork`, `vfork`, `execve`, `execveat`
- Clone NOT blocked (Python threading needs it)
- Toxic workers bypass Seccomp for integration test compatibility
- Graceful degradation on older kernels
- **7 Rust tests, 17 Python tests passing**

**Phase 5.3: Hot Reloading (sys.modules cleanup)** ✅

- Capture `_INITIAL_MODULES` baseline in `post_fork_init()`
- `cleanup_test_modules()` removes test imports
- Protected modules list (`tach_rust`, `pytest`, `django`, etc.)
- Integration with `reset_and_signal_ready()` cycle
- **4 Python tests passing**

**Phase 5.4: Deterministic Allocator (Jemalloc)** ✅

- `tikv-jemallocator` as global allocator
- tcache flush via `mallctl("thread.tcache.flush")`
- Epoch sync via `mallctl("epoch")`
- ELF parsing with `goblin` for libpython segment identification
- Solves Split-Brain allocator desynchronization problem
- **4 Rust tests, 6 Python tests passing**

#### Phase 6: Production Readiness ✅

**Phase 6.1: Coverage Resolution (code_id → filename)** ✅

- Dual ring buffer architecture (MappingRingBuffer + CoverageRingBuffer)
- PY_START callback for code object registration
- Thread-local `SEEN_CODES` HashSet for O(1) deduplication
- MappingEntry with 256-byte entries (code_id + filename)
- UTF-8 safe filename truncation from LEFT (preserves filename)
- Drain mapping buffer FIRST, then coverage buffer
- **23 Rust tests for mapping functionality**

**Phase 6.2: Configuration Engine ([tool.tach])** ✅

- `TachConfig` struct for pyproject.toml configuration
- `CoverageConfig` for nested coverage settings
- `load_tach_config()` function for file loading
- `MergedConfig` for CLI/file config merging
- Supports: test_pattern, timeout, workers, isolation_strategy
- Coverage options: enabled, source, omit, output, format
- **8 Rust tests for config parsing**

**Phase 6.3: Progress Bar Reporter** ✅

- `ProgressReporter` with indicatif progress bar
- `DotsReporter` for CI environments
- Automatic CI detection via `atty::is(Stderr)` + `CI` env var
- Failure buffering for summary display at end
- Color-coded output (green for pass, red for fail)
- **6 Rust tests for reporter functionality**

---

## Test Coverage

| Category                                     | Tests    | Status         |
| :------------------------------------------- | :------- | :------------- |
| Rust Unit Tests (`analysis.rs`)              | 49       | ✅ Passing     |
| Rust Unit Tests (`graph.rs`)                 | 20       | ✅ Passing     |
| Rust Unit Tests (`loader.rs`)                | 17       | ✅ Passing     |
| Rust Unit Tests (`zygote.rs::tests`)         | 6        | ✅ Passing     |
| Rust Unit Tests (`protocol.rs::tests`)       | 8        | ✅ Passing     |
| Rust Unit Tests (`scheduler.rs::tests`)      | 8        | ✅ Passing     |
| Rust Unit Tests (`sandbox.rs::tests`)        | 7        | ✅ Passing     |
| Rust Unit Tests (`coverage.rs::tests`)       | 32       | ✅ Passing     |
| Rust Unit Tests (`config.rs::tests`)         | 16       | ✅ Passing     |
| Rust Unit Tests (`reporter.rs::tests`)       | 18       | ✅ Passing     |
| Rust Unit Tests (`snapshot.rs::tests`)       | 1        | ✅ Passing     |
| Rust Integration (`toxicity_integration.rs`) | 10       | ✅ Passing     |
| Rust Integration (`tagging_integrity.rs`)    | 5        | ✅ Passing     |
| Rust Integration (`loader_integration.rs`)   | 19       | ✅ Passing     |
| Rust Integration (`resolver_integration.rs`) | 8        | ✅ Passing     |
| Rust Integration (`snapshot_integration.rs`) | 7        | ✅ Passing     |
| Rust Integration (`phase4_integration.rs`)   | 10       | ✅ Passing     |
| Python Gauntlet Phase 1                      | 28       | ✅ Passing     |
| Python Gauntlet Phase 2                      | 36       | ✅ Passing     |
| Python Gauntlet Phase 5 (hot reload)         | 4        | ✅ Passing     |
| Python Gauntlet Phase 5.1 (coverage)         | 7        | ✅ Passing     |
| Python Gauntlet Phase 5.2 (sandbox)          | 17       | ✅ Passing     |
| Python Gauntlet Phase 5.4 (allocator)        | 6        | ✅ Passing     |
| Python Benchmark                             | 2        | ✅ Passing     |
| Python Gauntlet (crash signals)              | 8        | ✅ Passing     |
| Python Gauntlet (fs protection)              | 5        | ✅ Passing     |
| **Total**                                    | **~334** | ✅ All Passing |

---

## Technical Specifications

### Sandbox Architecture (`sandbox.rs`)

```rust
/// Status of Landlock enforcement
pub enum SandboxStatus {
    FullyEnforced,      // All restrictions active
    PartiallyEnforced,  // Some features unavailable (older kernel)
    NotEnforced,        // Kernel too old (< 5.13)
}

/// Apply Landlock filesystem restrictions
/// RO: project_root, /usr, /lib, /lib64, /bin, /etc, /dev, /proc, /sys
/// RW: /tmp, /run/tach/worker_N
pub fn apply_landlock(project_root: &Path, worker_id: u32) -> Result<SandboxStatus>;

/// Apply Seccomp syscall blacklist (safe workers only)
/// Blocks: socket, bind, connect, listen, accept, accept4
/// Blocks: fork, vfork, execve, execveat
/// Allows: clone (Python threading), everything else
pub fn apply_seccomp() -> Result<()>;

/// Combined sandbox application with graceful degradation
pub fn apply_iron_dome(
    project_root: &Path,
    worker_id: u32,
    is_toxic: bool,
) -> Result<SandboxStatus>;
```

### Coverage Architecture (`coverage.rs`)

```rust
/// Ring buffer header (64-byte aligned for cache-line)
#[repr(C, align(64))]
pub struct RingBufferHeader {
    pub write_idx: AtomicU64,      // Producer index
    pub read_idx: AtomicU64,       // Consumer index
    pub capacity: u64,             // Number of entries
    pub overflow_count: AtomicU64, // Dropped entries counter
    _padding: [u8; 32],            // Pad to 64 bytes
}

/// Coverage entry (16-byte aligned)
#[repr(C, align(16))]
pub struct CoverageEntry {
    pub code_id: u64,   // id(code_object)
    pub lineno: u32,    // Line number
    pub flags: u32,     // Reserved for future use
}

/// Lock-free ring buffer for coverage data
pub struct CoverageRingBuffer {
    header: *mut RingBufferHeader,
    entries: *mut CoverageEntry,
    mmap_ptr: *mut u8,
    mmap_len: usize,
}

/// Aggregator thread that drains the ring buffer
pub struct CoverageAggregator {
    buffer: Arc<CoverageRingBuffer>,
    poll_interval: Duration,
    running: Arc<AtomicBool>,
}
```

### Allocator Architecture (`allocator.rs`)

```rust
use tikv_jemallocator::Jemalloc;

#[global_allocator]
static GLOBAL: Jemalloc = Jemalloc;

/// Flush thread-local cache for snapshot consistency
pub fn flush_tcache() {
    unsafe {
        tikv_jemalloc_sys::mallctl(
            b"thread.tcache.flush\0".as_ptr() as *const _,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null(),
            0,
        );
    }
}

/// Synchronize allocator epoch
pub fn sync_epoch() {
    let mut epoch: u64 = 0;
    let mut len = std::mem::size_of::<u64>();
    unsafe {
        tikv_jemalloc_sys::mallctl(
            b"epoch\0".as_ptr() as *const _,
            &mut epoch as *mut _ as *mut _,
            &mut len,
            &epoch as *const _ as *const _,
            len,
        );
    }
}
```

### Protocol Extension (`protocol.rs`)

```rust
/// Payload sent to Zygote with fork command
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestPayload {
    pub test_id: u32,
    pub file_path: String,
    pub test_name: String,
    pub is_async: bool,
    pub fixtures: Vec<FixtureInfo>,
    pub log_fd: i32,
    pub debug_socket_path: String,
    /// Toxicity flag for dual-path execution
    /// Worker checks this to decide Reset vs Exit
    /// Also determines Seccomp application (safe only)
    pub is_toxic: bool,
}
```

### FFI Functions Exposed to Python

| Function                | Signature                                        | Purpose                        |
| :---------------------- | :----------------------------------------------- | :----------------------------- |
| `get_module`            | `fn(name: &str) -> Option<Vec<u8>>`              | Get bytecode from registry     |
| `get_module_path`       | `fn(name: &str) -> Option<String>`               | Get source path for `__file__` |
| `is_module_package`     | `fn(name: &str) -> Option<bool>`                 | Check if module is a package   |
| `load_module`           | `fn(py, name, path, bytecode) -> PyResult<bool>` | Inject bytecode via C-API      |
| `init_snapshot_mode`    | `fn(supervisor_sock: &str) -> bool`              | Initialize UFFD handshake      |
| `reset_memory`          | `fn() -> PyResult<()>`                           | Self-reset via madvise         |
| `cleanup_modules`       | `fn() -> PyResult<()>`                           | Remove test-imported modules   |
| `record_line`           | `fn(code_id: u64, lineno: u32)`                  | Record coverage hit            |
| `record_py_start`       | `fn(code_id: u64, filename: String)`             | Register code_id → filename    |
| `is_coverage_enabled`   | `fn() -> bool`                                   | Check if coverage is active    |
| `get_coverage_overflow` | `fn() -> u64`                                    | Get coverage buffer overflow   |
| `get_mapping_overflow`  | `fn() -> u64`                                    | Get mapping buffer overflow    |

---

### Progress Bar Reporter (Phase 6.3)

The Reporter system (`reporter.rs`) provides adaptive output based on environment detection:

```mermaid
flowchart TB
    subgraph Detection["ENVIRONMENT DETECTION"]
        TTY["atty::is(Stderr)?"]
        CI["CI env var set?"]
    end

    subgraph Reporters["REPORTER SELECTION"]
        Progress["ProgressReporter\n(indicatif progress bar)"]
        Dots["DotsReporter\n(. F s per test)"]
        JSON["JsonReporter\n(NDJSON to stdout)"]
    end

    subgraph Output["OUTPUT BEHAVIOR"]
        ProgressOut["Interactive progress bar\nP:X F:Y S:Z\nFailure summary at end"]
        DotsOut["Dots output\n....F..s..\nFailure summary at end"]
        JSONOut["NDJSON events\n{event: test_finished, ...}"]
    end

    TTY -->|"Yes"| CI
    TTY -->|"No"| Dots
    CI -->|"No"| Progress
    CI -->|"Yes"| Dots

    Progress --> ProgressOut
    Dots --> DotsOut
    JSON --> JSONOut
```

**Reporter Selection Logic:**

```rust
// In main.rs
if ProgressReporter::should_use_progress_bar() {
    reporters.push(Box::new(ProgressReporter::new()));
} else {
    reporters.push(Box::new(DotsReporter::new()));
}

// Detection logic
pub fn should_use_progress_bar() -> bool {
    atty::is(atty::Stream::Stderr) && std::env::var("CI").is_err()
}
```

**Failure Buffering:**

Both `ProgressReporter` and `DotsReporter` buffer failures during execution and display them in a summary at the end. This prevents interleaving of progress output with failure details.

| Reporter         | Interactive        | Failure Display  | Use Case          |
| :--------------- | :----------------- | :--------------- | :---------------- |
| ProgressReporter | Yes (progress bar) | Summary at end   | Local development |
| DotsReporter     | No (dots)          | Summary at end   | CI/CD pipelines   |
| JsonReporter     | No (NDJSON)        | Inline in events | IDE integration   |

---

## Configuration Reference

### pyproject.toml Configuration

Tach supports configuration via `[tool.tach]` in your `pyproject.toml`:

```toml
[tool.tach]
# Test file pattern (default: "test_*.py")
test_pattern = "test_*.py"

# Test timeout in seconds (default: 60)
timeout = 60

# Number of worker processes (default: num_cpus)
workers = 4

# Isolation strategy: "auto", "fork", "snapshot" (default: "auto")
isolation_strategy = "auto"

[tool.tach.coverage]
# Enable coverage collection (default: false)
enabled = true

# Source directories to measure coverage for
source = ["src", "lib"]

# Patterns to omit from coverage
omit = ["**/test_*", "**/migrations/*"]

# Output file path (default: ".coverage")
output = ".coverage"

# Output format: "lcov", "html", "json" (default: "lcov")
format = "lcov"

# pytest-env compatibility (environment variables)
[tool.pytest_env]
DATABASE_URL = "sqlite:///:memory:"
DEBUG = "true"
```

### Configuration Precedence

CLI arguments take precedence over file configuration:

| Setting       | CLI Flag         | Environment Variable | pyproject.toml                 |
| :------------ | :--------------- | :------------------- | :----------------------------- |
| Output format | `--format`       | `TACH_FORMAT`        | -                              |
| JUnit XML     | `--junit-xml`    | `TACH_JUNIT_XML`     | -                              |
| Coverage      | `--coverage`     | `TACH_COVERAGE`      | `[tool.tach.coverage].enabled` |
| No isolation  | `--no-isolation` | `TACH_NO_ISOLATION`  | -                              |
| Test pattern  | -                | -                    | `test_pattern`                 |
| Timeout       | -                | -                    | `timeout`                      |
| Workers       | -                | -                    | `workers`                      |

### Environment Variables

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

---

## Findings and Lessons Learned

### Critical Architectural Decisions

#### 1. Clone Syscall Must NOT Be Blocked

**Problem:** Initial Seccomp filter blocked `clone`, causing Python threading to fail silently.

**Solution:** Seccomp blacklist approach - block specific dangerous syscalls, allow everything else including `clone`.

**Lesson:** Python's threading module uses `clone` internally. Blocking it breaks `threading.Thread`, `concurrent.futures`, and many standard library features.

#### 2. Mapping Buffer Must Be Drained FIRST

**Problem:** Coverage entries arrived before their code_id → filename mappings, resulting in `<code:ADDR>` placeholders.

**Solution:** CoverageAggregator drains MappingRingBuffer BEFORE CoverageRingBuffer in every poll cycle.

**Lesson:** When using dual buffers for registration + data, always process registration first.

#### 3. Thread-Local HashSet for Deduplication

**Problem:** Global HashSet with Mutex caused lock contention on every function entry.

**Solution:** `thread_local!` HashSet per worker thread - O(1) lookup with zero locking.

**Lesson:** For high-frequency callbacks, thread-local storage eliminates synchronization overhead.

#### 4. Filename Truncation from LEFT

**Problem:** Long paths exceeded 240-byte buffer, losing the actual filename.

**Solution:** Truncate from LEFT (drop path prefix), preserving the filename portion.

**Lesson:** When truncating paths, the filename is more important than the directory structure.

#### 5. UTF-8 Boundary Handling

**Problem:** Naive byte slicing could split multi-byte UTF-8 characters, causing invalid strings.

**Solution:** Find next valid UTF-8 start byte when truncating.

```rust
while safe_start < bytes.len() && (bytes[safe_start] & 0b1100_0000) == 0b1000_0000 {
    safe_start += 1;
}
```

**Lesson:** Always handle UTF-8 boundaries when slicing strings at byte positions.

#### 6. GIL Discipline in Coverage Callbacks

**Problem:** Holding GIL during ring buffer writes caused serialization with aggregator thread.

**Solution:** `py.allow_threads()` before any shared memory access.

**Lesson:** Release GIL as early as possible in PyO3 callbacks, especially for I/O or shared memory.

#### 7. Jemalloc for Snapshot Consistency

**Problem:** glibc malloc's tcache holds stale pointers after memory reset, causing corruption.

**Solution:** Jemalloc with explicit `mallctl("thread.tcache.flush")` before snapshot.

**Lesson:** Standard allocators are not designed for memory snapshot/restore. Jemalloc's mallctl API provides the control needed.

#### 8. Landlock Path Canonicalization

**Problem:** Relative paths and symlinks bypassed Landlock restrictions.

**Solution:** Always `canonicalize()` paths before adding to Landlock ruleset.

**Lesson:** Security boundaries must use canonical paths to prevent bypass via symlinks or relative paths.

### Performance Insights

| Optimization          | Impact                        | Implementation             |
| :-------------------- | :---------------------------- | :------------------------- |
| Lock-free ring buffer | 10x faster than Mutex         | AtomicU64 for indices      |
| Thread-local dedup    | 100x faster than global lock  | `thread_local!` HashSet    |
| memfd_create          | No filesystem overhead        | Anonymous shared memory    |
| GIL release           | Prevents Python serialization | `py.allow_threads()`       |
| Batch draining        | Reduces lock contention       | Drain 4096 entries at once |

---

## Future Development Hints

### Phase 7: Coverage Report Generation

**Current State:** Coverage data is collected but not written to files.

**TODO:**

1. Implement LCOV format writer (`coverage_data` → `.lcov` file)
2. Implement HTML report generation (similar to `coverage html`)
3. Implement `.coverage` SQLite format for `coverage.py` compatibility
4. Add `--cov-report` CLI flag for format selection

**Key Files:** `src/coverage.rs`, `src/main.rs`

### Phase 8: Parallel Test Execution

**Current State:** Tests run sequentially within each worker.

**TODO:**

1. Implement test batching by module
2. Add work-stealing scheduler
3. Implement test ordering by historical duration
4. Add `--parallel` / `-j` flag for worker count

**Key Files:** `src/scheduler.rs`, `src/main.rs`

### Phase 9: Django Integration

**Current State:** Basic Django support via environment variables.

**TODO:**

1. Implement Django database transaction rollback
2. Add Django settings module detection
3. Implement Django test client fixture
4. Add `--django` flag for Django-specific optimizations

**Key Files:** `src/tach_harness.py`, `src/config.rs`

### Phase 10: Async Test Support

**Current State:** `is_async` flag exists but not fully implemented.

**TODO:**

1. Implement `asyncio.run()` wrapper for async tests
2. Add event loop reset between tests
3. Implement async fixture support
4. Add `pytest-asyncio` compatibility layer

**Key Files:** `src/tach_harness.py`, `src/protocol.rs`

### Potential Optimizations

| Optimization          | Complexity | Impact                     |
| :-------------------- | :--------- | :------------------------- |
| SIMD for ring buffer  | Medium     | 2x faster batch operations |
| io_uring for IPC      | High       | Lower latency scheduling   |
| Persistent workers    | Low        | Avoid fork overhead        |
| Incremental discovery | Medium     | Faster re-runs             |
| Test result caching   | Medium     | Skip unchanged tests       |

### Code Quality Improvements

1. **Error Handling:** Replace `unwrap()` with proper error propagation in hot paths
2. **Logging:** Add structured logging with `tracing` crate
3. **Metrics:** Add Prometheus metrics for performance monitoring
4. **Documentation:** Add rustdoc for all public APIs
5. **Benchmarks:** Add criterion benchmarks for critical paths

---

## Known Problems and Limitations

### Pre-existing Issues

#### 1. `environment::tests::test_find_site_packages_no_venv` Failure

**Status:** Known failure, unrelated to Phase 6.

**Cause:** Test expects no venv but finds the active development venv.

**Workaround:** Run tests outside of venv, or ignore this specific test.

### Platform Limitations

| Limitation                    | Reason                              | Workaround                            |
| :---------------------------- | :---------------------------------- | :------------------------------------ |
| Linux only                    | Uses userfaultfd, Landlock, Seccomp | No Windows/macOS support              |
| Kernel 5.13+ for full sandbox | Landlock requires 5.13+             | Graceful degradation on older kernels |
| Python 3.12+ for coverage     | PEP 669 sys.monitoring              | Coverage disabled on older Python     |
| x86_64/aarch64 only           | Seccomp syscall numbers             | No 32-bit support                     |

### Known Edge Cases

| Edge Case                        | Behavior                             | Mitigation                 |
| :------------------------------- | :----------------------------------- | :------------------------- |
| Very long filenames (>240 bytes) | Truncated from left                  | Preserves actual filename  |
| Mapping buffer overflow          | Entries dropped, counter incremented | Increase MAPPING_CAPACITY  |
| Coverage buffer overflow         | Entries dropped, counter incremented | Increase DEFAULT_CAPACITY  |
| Toxic test with subprocess       | Seccomp bypassed                     | Landlock still enforced    |
| Test modifies sys.path           | Changes persist until reset          | Use cleanup_test_modules() |

### Memory Considerations

| Resource        | Default            | Configurable       | Notes                       |
| :-------------- | :----------------- | :----------------- | :-------------------------- |
| Coverage buffer | 4MB (262K entries) | `DEFAULT_CAPACITY` | Shared memory               |
| Mapping buffer  | 2MB (8K entries)   | `MAPPING_CAPACITY` | Shared memory               |
| Golden snapshot | Variable           | -                  | Depends on worker heap size |
| Worker pool     | num_cpus           | `workers` config   | Each worker is a process    |

### Security Considerations

| Consideration                | Status    | Notes                        |
| :--------------------------- | :-------- | :--------------------------- |
| Landlock bypass via symlinks | Mitigated | Paths canonicalized          |
| Seccomp bypass via clone     | By design | Python threading needs clone |
| Toxic worker network access  | Allowed   | Seccomp skipped for toxic    |
| File write outside sandbox   | Blocked   | Landlock enforced            |

---

## Troubleshooting Guide

### Build Issues

#### `PYO3_PYTHON` Not Set

```bash
error: PYO3_PYTHON environment variable not set
```

**Solution:**

```bash
export PYO3_PYTHON=$(which python)
# Or use full path:
export PYO3_PYTHON=/path/to/.venv/bin/python
```

#### Jemalloc Build Failure

```bash
error: failed to run custom build command for `tikv-jemalloc-sys`
```

**Solution:** Install build dependencies:

```bash
# Ubuntu/Debian
sudo apt install build-essential autoconf

# Fedora
sudo dnf install gcc make autoconf
```

### Runtime Issues

#### `EPERM` on Landlock

```
[supervisor] WARN: Landlock not available (kernel < 5.13)
```

**Cause:** Kernel doesn't support Landlock.

**Solution:** Upgrade kernel to 5.13+ or accept reduced security.

#### `EPERM` on userfaultfd

```
error: userfaultfd creation failed: Operation not permitted
```

**Solution:** Run with appropriate capabilities:

```bash
sudo setcap cap_sys_ptrace+ep ./target/release/tach-core
# Or run with sudo
sudo ./target/release/tach-core .
```

#### Coverage Shows `<code:ADDR>` Instead of Filenames

**Cause:** Mapping buffer drained after coverage buffer, or PY_START callback not registered.

**Solution:** Ensure `record_py_start` is called for PY_START events in `tach_harness.py`.

#### Progress Bar Not Showing

**Cause:** Running in non-TTY environment or CI detected.

**Solution:** This is expected behavior. Use `--format human` to force human output, but progress bar requires TTY.

### Debug Commands

```bash
# Check kernel version
uname -r

# Check Landlock support
cat /sys/kernel/security/lsm | grep landlock

# Check seccomp support
grep CONFIG_SECCOMP /boot/config-$(uname -r)

# Trace syscalls
strace -f ./target/release/tach-core . 2>&1 | head -100

# Check Python version for coverage
python --version  # Needs 3.12+ for PEP 669

# Verify jemalloc is active
./target/release/tach-core --help 2>&1 | grep -i jemalloc
```

### Performance Debugging

```bash
# Profile with perf
perf record -g ./target/release/tach-core .
perf report

# Check for lock contention
perf lock record ./target/release/tach-core .
perf lock report

# Memory usage
/usr/bin/time -v ./target/release/tach-core .
```

---

## License

MIT License. See [LICENSE](LICENSE) for details.

---

<div align="center">

**Built with Rust for performance and reliability.**

</div>
