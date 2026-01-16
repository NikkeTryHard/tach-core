# Architecture Overview

This document provides a high-level view of Tach's architecture and how its components interact.

---

## System Architecture

Tach operates as a three-tier system: **Supervisor**, **Zygote**, and **Workers**.

```mermaid
flowchart TB
    subgraph Tier1["TIER 1: RUST SUPERVISOR"]
        direction TB
        CLI["CLI Entry Point<br/>(main.rs)"]
        Config["Configuration<br/>(config.rs)"]
        Discovery["Discovery Engine<br/>(discovery.rs)"]
        Toxicity["Toxicity Analyzer<br/>(analysis.rs, graph.rs)"]
        Loader["Bytecode Compiler<br/>(loader.rs)"]
        Resolver["Fixture Resolver<br/>(resolver.rs)"]
        Scheduler["Test Scheduler<br/>(scheduler.rs)"]
        Snapshot["Snapshot Manager<br/>(snapshot.rs)"]
        Coverage["Coverage Aggregator<br/>(coverage.rs)"]
        Reporter["Reporter System<br/>(reporter.rs)"]
    end

    subgraph Tier2["TIER 2: ZYGOTE PROCESS"]
        direction TB
        ZygoteInit["Python Initialization"]
        ZygoteWarm["Warm-up (pytest, Django)"]
        ZygotePool["Worker Pool Management"]
        ZygoteFork["Fork/Dispatch"]
    end

    subgraph Tier3["TIER 3: WORKER PROCESSES"]
        direction TB
        WorkerSandbox["Iron Dome (Sandbox)"]
        WorkerIsolation["Namespace Isolation"]
        WorkerHarness["Test Harness"]
        WorkerExec["Test Execution"]
        WorkerReset["Memory Reset"]
    end

    CLI --> Config --> Discovery
    Discovery --> Toxicity --> Resolver --> Scheduler
    Loader --> ZygoteInit
    Scheduler <--> ZygoteFork
    ZygoteInit --> ZygoteWarm --> ZygotePool --> ZygoteFork
    ZygoteFork --> WorkerSandbox --> WorkerIsolation --> WorkerHarness
    WorkerHarness --> WorkerExec --> WorkerReset
    WorkerReset --> ZygotePool
    Snapshot <--> WorkerReset
    WorkerExec --> Coverage
    Coverage --> Reporter
```

---

## Component Responsibilities

### Tier 1: Rust Supervisor

| Component         | File                                          | Responsibility                                        |
| :---------------- | :-------------------------------------------- | :---------------------------------------------------- |
| **CLI**           | `main.rs`                                     | Parse arguments, orchestrate execution                |
| **Config**        | `core/config.rs`                              | Load pyproject.toml, merge CLI/env/file settings      |
| **Discovery**     | `discovery/scanner.rs`                        | AST-based test discovery using rustpython-parser      |
| **Toxicity**      | `discovery/analysis.rs`, `discovery/graph.rs` | Detect unsafe modules, propagate via dependency graph |
| **Loader**        | `discovery/loader.rs`                         | Compile .py to .pyc, manage bytecode cache            |
| **Resolver**      | `discovery/resolver.rs`                       | Resolve fixture dependencies, topological sort        |
| **Scheduler**     | `execution/scheduler.rs`                      | Dispatch tests to workers, manage queues              |
| **Snapshot**      | `isolation/snapshot.rs`                       | userfaultfd registration, page fault handling         |
| **Coverage**      | `reporting/coverage.rs`                       | Ring buffer management, aggregation thread            |
| **Reporter**      | `reporting/reporter.rs`                       | Progress bar, dots, JSON output                       |
| **Suggestions**   | `core/suggestions.rs`                         | Error remediation suggestions                         |
| **Plugin Bridge** | `execution/plugin_bridge.rs`                  | Bridges pytest plugin hooks to Tach execution model   |

### Tier 2: Zygote Process

The Zygote is a long-lived Python process that:

1. **Initializes Python** with all imports pre-loaded
2. **Warms up frameworks** (pytest, Django)
3. **Manages the worker pool** for safe test reuse
4. **Forks workers** on demand from the Supervisor

### Tier 3: Worker Processes

Workers are short-lived (toxic) or long-lived (safe) processes that:

1. **Apply security sandbox** (Landlock + Seccomp)
2. **Enter namespace isolation** (Mount + Network)
3. **Execute tests** via the Python harness
4. **Reset memory** or exit based on toxicity

---

## Data Flow

```mermaid
sequenceDiagram
    participant CLI as CLI
    participant Disc as Discovery
    participant Tox as Toxicity
    participant Sched as Scheduler
    participant Zyg as Zygote
    participant Work as Worker
    participant Snap as Snapshot
    participant Cov as Coverage
    participant Rep as Reporter

    CLI->>Disc: discover(path)
    Disc->>Tox: analyze_all()
    Tox->>Tox: propagate()
    Tox->>Sched: RunnableTest[]

    loop For each test
        Sched->>Zyg: CMD_FORK + TestPayload
        Zyg->>Work: fork()
        Work->>Work: apply_iron_dome()
        Work->>Snap: init_snapshot_mode()
        Snap->>Snap: capture_golden()
        Work->>Work: run_test()
        Work->>Cov: record_line()
        Work->>Zyg: TestResult
        Zyg->>Sched: TestResult

        alt Safe Test
            Work->>Snap: reset_memory()
            Snap->>Work: restore_pages()
        else Toxic Test
            Work->>Work: exit(0)
            Zyg->>Work: spawn_replacement()
        end
    end

    Sched->>Rep: report_results()
    Cov->>Rep: coverage_data()
```

---

## Memory Architecture

```mermaid
flowchart LR
    subgraph Supervisor["SUPERVISOR MEMORY"]
        GoldenPages["Golden Pages<br/>HashMap<addr, Vec<u8>>"]
        CovBuffer["Coverage Buffer<br/>(memfd, 4MB)"]
        MapBuffer["Mapping Buffer<br/>(memfd, 2MB)"]
    end

    subgraph Worker["WORKER MEMORY"]
        Heap["[heap]"]
        Stack["[stack]"]
        Anon["Anonymous Mappings"]
        Libpython["libpython.so<br/>(data/bss)"]
    end

    subgraph Kernel["KERNEL"]
        UFFD["userfaultfd"]
        PageTable["Page Tables"]
    end

    Worker -->|process_vm_readv| GoldenPages
    UFFD -->|UFFDIO_COPY| Worker
    Worker -->|madvise MADV_DONTNEED| PageTable
    PageTable -->|Page Fault| UFFD
    UFFD -->|Restore| GoldenPages
```

---

## IPC Architecture

```mermaid
flowchart TB
    subgraph Channels["IPC CHANNELS"]
        CmdSock["Command Socket<br/>(UnixStream)"]
        ResSock["Result Socket<br/>(UnixStream)"]
        WorkSock["Worker Socket<br/>(UnixStream::pair)"]
        UffdSock["UFFD Socket<br/>(SCM_RIGHTS)"]
    end

    subgraph Messages["MESSAGE TYPES"]
        CmdFork["CMD_FORK (0x01)"]
        CmdRun["CMD_RUN_TEST (0x02)"]
        CmdExit["CMD_EXIT (0x00)"]
        MsgReady["MSG_READY (0x42)"]
        MsgWorkerReady["MSG_WORKER_READY (0x43)"]
    end

    subgraph Payloads["PAYLOADS (bincode)"]
        TestPayload["TestPayload"]
        TestResult["TestResult"]
    end

    CmdSock --> CmdFork --> TestPayload
    CmdSock --> CmdRun --> TestPayload
    ResSock --> TestResult
    WorkSock --> MsgWorkerReady
    UffdSock --> |"FD + PID"| UFFD
```

---

## Security Layers

```mermaid
flowchart TB
    subgraph Layers["DEFENSE IN DEPTH"]
        L1["Layer 1: Process Isolation<br/>(fork + namespaces)"]
        L2["Layer 2: Filesystem Isolation<br/>(Landlock + OverlayFS)"]
        L3["Layer 3: Syscall Filtering<br/>(Seccomp-BPF)"]
        L4["Layer 4: Memory Isolation<br/>(userfaultfd reset)"]
    end

    L1 --> L2 --> L3 --> L4

    subgraph SafeWorker["SAFE WORKER"]
        S1["All 4 layers active"]
        S2["Can be reused"]
    end

    subgraph ToxicWorker["TOXIC WORKER"]
        T1["Layers 1, 2, 4 active"]
        T2["Seccomp skipped"]
        T3["Must exit after test"]
    end
```

---

## File Organization

See [README.md](../../README.md#project-structure) for complete source file organization.

---

## Key Design Decisions

| Decision              | Rationale                                             |
| :-------------------- | :---------------------------------------------------- |
| **Rust Supervisor**   | Zero-cost abstractions, memory safety, syscall access |
| **Python Zygote**     | Pay import tax once, share via fork                   |
| **userfaultfd**       | Sub-microsecond page restoration                      |
| **Jemalloc**          | Deterministic heap for snapshot consistency           |
| **Landlock**          | Kernel-level filesystem isolation (5.13+)             |
| **Seccomp**           | Syscall filtering without ptrace overhead             |
| **bincode**           | Fast binary serialization for IPC                     |
| **petgraph**          | Efficient dependency graph for toxicity               |
| **rustpython-parser** | Pure Rust AST parsing, no Python execution            |
| **memfd_create**      | Anonymous shared memory for coverage                  |

---

## Communication Protocol

Tach uses Unix domain sockets with binary serialization for IPC between Supervisor, Zygote, and Workers.

### Message Framing

All structured messages use an 8-byte header with magic bytes, version, and length:

```
+--------+---------+----------+--------+------------------+
| Magic  | Version | Reserved | Length | Payload          |
| 2 bytes| 1 byte  | 1 byte   | 4 bytes| (bincode)        |
| "TA"   | 0x01    | 0x00     | LE u32 |                  |
+--------+---------+----------+--------+------------------+

Total header size: 8 bytes (HEADER_SIZE constant)
```

### Data Structures

**TestPayload** - Sent from Supervisor to Worker to initiate a test:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestPayload {
    pub test_id: u32,
    pub file_path: String,
    pub test_name: String,
    pub is_async: bool,
    pub fixtures: Vec<FixtureInfo>,
    pub log_fd: i32,
    pub debug_socket_path: String,
    pub is_toxic: bool,
    pub timeout_secs: Option<u64>,
}
```

**TestResult** - Sent from Worker to Supervisor upon test completion:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestResult {
    pub test_id: u32,
    pub status: u8,
    pub duration_ns: u64,
    pub message: String,
    pub memory_rss_bytes: Option<u64>,
}
```

### Command Bytes

| Constant           | Value | Direction            | Purpose                     |
| :----------------- | :---- | :------------------- | :-------------------------- |
| `CMD_EXIT`         | 0x00  | Supervisor -> Zygote | Shutdown                    |
| `CMD_FORK`         | 0x01  | Supervisor -> Zygote | Spawn/dispatch test         |
| `CMD_RUN_TEST`     | 0x02  | Zygote -> Worker     | Run test on existing worker |
| `CMD_PING`         | 0x03  | Supervisor -> Worker | Health check ping           |
| `MSG_READY`        | 0x42  | Zygote -> Supervisor | Zygote initialized          |
| `MSG_WORKER_READY` | 0x43  | Worker -> Zygote     | Worker reset complete       |
| `MSG_PONG`         | 0x44  | Worker -> Supervisor | Health check response       |

### Status Codes

| Constant               | Value | Meaning                 |
| :--------------------- | :---- | :---------------------- |
| `STATUS_PASS`          | 0     | Test passed             |
| `STATUS_FAIL`          | 1     | Test failed (assertion) |
| `STATUS_SKIP`          | 2     | Test skipped            |
| `STATUS_CRASH`         | 3     | Worker crashed          |
| `STATUS_ERROR`         | 4     | Test error (exception)  |
| `STATUS_HARNESS_ERROR` | 5     | Harness error           |
| `STATUS_TIMEOUT`       | 6     | Test timed out          |

### SCM_RIGHTS (File Descriptor Passing)

Used to pass userfaultfd from Worker to Supervisor:

```rust
pub fn send_fd(sock: &UnixStream, pid: i32, fd: RawFd) -> Result<()> {
    let pid_bytes = pid.to_le_bytes();
    let iov = [IoSlice::new(&pid_bytes)];
    let fds = [fd];
    let cmsg = [ControlMessage::ScmRights(&fds)];
    sendmsg::<()>(sock.as_raw_fd(), &iov, &cmsg, MsgFlags::empty(), None)?;
    Ok(())
}
```

### Message Size Limits

To prevent OOM attacks, all IPC messages enforce size limits:

| Limit              | Value  | Purpose                            |
| ------------------ | ------ | ---------------------------------- |
| `MAX_PAYLOAD_SIZE` | 16 MiB | Maximum serialized message size    |
| Message truncation | 4 KiB  | Maximum error/output string length |

Size validation occurs **before** memory allocation using `decode_with_limit`.

---

## Next Steps

- [Discovery Engine](discovery.md) - How tests are found
- [Zero-Copy Loader](loader.md) - How modules are loaded
- [Toxicity Analysis](toxicity.md) - How unsafe code is detected
