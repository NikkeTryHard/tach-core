# API Reference

Complete reference for Tach internal APIs and data structures.

---

## Core Data Structures

### TestCase

Represents a discovered test function.

```rust
pub struct TestCase {
    pub name: String,
    pub is_async: bool,
    pub fixtures: Vec<String>,
    pub markers: Vec<String>,
    pub lineno: usize,
}
```

| Field      | Type          | Description                      |
| :--------- | :------------ | :------------------------------- |
| `name`     | `String`      | Function name (e.g., `test_foo`) |
| `is_async` | `bool`        | Whether function is async        |
| `fixtures` | `Vec<String>` | Required fixture names           |
| `markers`  | `Vec<String>` | Applied markers                  |
| `lineno`   | `usize`       | Line number in source file       |

---

### TestModule

Represents a parsed test file.

```rust
pub struct TestModule {
    pub path: PathBuf,
    pub tests: Vec<TestCase>,
    pub fixtures: Vec<FixtureDefinition>,
    pub imports: Vec<String>,
}
```

| Field      | Type                     | Description               |
| :--------- | :----------------------- | :------------------------ |
| `path`     | `PathBuf`                | Absolute path to file     |
| `tests`    | `Vec<TestCase>`          | Discovered test functions |
| `fixtures` | `Vec<FixtureDefinition>` | Defined fixtures          |
| `imports`  | `Vec<String>`            | Import statements         |

---

### FixtureDefinition

Represents a fixture definition.

```rust
pub struct FixtureDefinition {
    pub name: String,
    pub scope: FixtureScope,
    pub dependencies: Vec<String>,
    pub is_async: bool,
    pub autouse: bool,
}
```

| Field          | Type           | Description                    |
| :------------- | :------------- | :----------------------------- |
| `name`         | `String`       | Fixture function name          |
| `scope`        | `FixtureScope` | Lifetime scope                 |
| `dependencies` | `Vec<String>`  | Other fixtures this depends on |
| `is_async`     | `bool`         | Whether fixture is async       |
| `autouse`      | `bool`         | Whether auto-applied           |

---

### FixtureScope

Enum for fixture lifetime.

```rust
pub enum FixtureScope {
    Function,
    Class,
    Module,
    Session,
}
```

| Variant    | Description                  |
| :--------- | :--------------------------- |
| `Function` | Created per test (default)   |
| `Class`    | Shared within test class     |
| `Module`   | Shared within test module    |
| `Session`  | Shared across entire session |

---

### RunnableTest

Test ready for execution with resolved fixtures.

```rust
pub struct RunnableTest {
    pub file_path: PathBuf,
    pub test_name: String,
    pub is_async: bool,
    pub fixtures: Vec<ResolvedFixture>,
    pub is_toxic: bool,
}
```

| Field       | Type                   | Description                    |
| :---------- | :--------------------- | :----------------------------- |
| `file_path` | `PathBuf`              | Path to test file              |
| `test_name` | `String`               | Fully qualified name (node ID) |
| `is_async`  | `bool`                 | Whether test is async          |
| `fixtures`  | `Vec<ResolvedFixture>` | Resolved fixture chain         |
| `is_toxic`  | `bool`                 | Requires worker restart        |

---

### ResolvedFixture

Fixture with source location resolved.

```rust
pub struct ResolvedFixture {
    pub name: String,
    pub scope: FixtureScope,
    pub source_path: PathBuf,
    pub is_async: bool,
}
```

---

## Protocol Structures

### TestPayload

Sent from Supervisor to Worker.

```rust
#[derive(Serialize, Deserialize)]
pub struct TestPayload {
    pub test_id: u32,
    pub file_path: String,
    pub test_name: String,
    pub is_async: bool,
    pub fixtures: Vec<FixtureInfo>,
    pub log_fd: i32,
    pub debug_socket_path: String,
    pub is_toxic: bool,
}
```

| Field               | Type               | Description                 |
| :------------------ | :----------------- | :-------------------------- |
| `test_id`           | `u32`              | Unique test identifier      |
| `file_path`         | `String`           | Path to test file           |
| `test_name`         | `String`           | Fully qualified test name   |
| `is_async`          | `bool`             | Whether test is async       |
| `fixtures`          | `Vec<FixtureInfo>` | Required fixture metadata   |
| `log_fd`            | `i32`              | File descriptor for logging |
| `debug_socket_path` | `String`           | Path for pdb tunneling      |
| `is_toxic`          | `bool`             | Whether worker should exit  |

---

### TestResult

Sent from Worker to Supervisor.

```rust
#[derive(Serialize, Deserialize)]
pub struct TestResult {
    pub test_id: u32,
    pub status: u8,
    pub duration_ns: u64,
    pub message: String,
}
```

| Field         | Type     | Description              |
| :------------ | :------- | :----------------------- |
| `test_id`     | `u32`    | Matching test identifier |
| `status`      | `u8`     | Status code (see below)  |
| `duration_ns` | `u64`    | Execution time in ns     |
| `message`     | `String` | Error message if failed  |

---

### FixtureInfo

Fixture metadata for test execution.

```rust
#[derive(Serialize, Deserialize)]
pub struct FixtureInfo {
    pub name: String,
    pub scope: String,
}
```

---

## Status Codes

| Constant               | Value | Description             |
| :--------------------- | :---- | :---------------------- |
| `STATUS_PASS`          | 0     | Test passed             |
| `STATUS_FAIL`          | 1     | Test failed (assertion) |
| `STATUS_SKIP`          | 2     | Test skipped            |
| `STATUS_CRASH`         | 3     | Worker crashed          |
| `STATUS_ERROR`         | 4     | Test error (exception)  |
| `STATUS_HARNESS_ERROR` | 5     | Harness error           |

---

## Command Bytes

| Constant           | Value | Description           |
| :----------------- | :---- | :-------------------- |
| `CMD_EXIT`         | 0x00  | Shutdown signal       |
| `CMD_FORK`         | 0x01  | Spawn/dispatch test   |
| `CMD_RUN_TEST`     | 0x02  | Run test on worker    |
| `MSG_READY`        | 0x42  | Zygote initialized    |
| `MSG_WORKER_READY` | 0x43  | Worker ready for next |

---

## Coverage Structures

### RingBufferHeader

Shared memory buffer header.

```rust
#[repr(C)]
pub struct RingBufferHeader {
    pub write_pos: AtomicU64,
    pub read_pos: AtomicU64,
    pub capacity: u64,
    pub overflow_count: AtomicU64,
}
```

| Field            | Type        | Description                |
| :--------------- | :---------- | :------------------------- |
| `write_pos`      | `AtomicU64` | Next write position        |
| `read_pos`       | `AtomicU64` | Next read position         |
| `capacity`       | `u64`       | Buffer capacity in entries |
| `overflow_count` | `AtomicU64` | Number of dropped entries  |

---

### CoverageEntry

Single coverage event.

```rust
#[repr(C)]
pub struct CoverageEntry {
    pub file_id: u32,
    pub lineno: u32,
}
```

| Field     | Type  | Description          |
| :-------- | :---- | :------------------- |
| `file_id` | `u32` | Interned file ID     |
| `lineno`  | `u32` | Line number executed |

---

### MappingEntry

File ID to path mapping.

```rust
#[repr(C)]
pub struct MappingEntry {
    pub file_id: u32,
    pub path_len: u32,
    pub path: [u8; 256],
}
```

---

## Toxicity Structures

### ToxicityReport

Module toxicity classification.

```rust
pub struct ToxicityReport {
    pub is_toxic: bool,
    pub reason: Option<String>,
    pub propagated_from: Vec<String>,
}
```

| Field             | Type             | Description                     |
| :---------------- | :--------------- | :------------------------------ |
| `is_toxic`        | `bool`           | Whether module is toxic         |
| `reason`          | `Option<String>` | Direct toxicity reason          |
| `propagated_from` | `Vec<String>`    | Modules that caused propagation |

---

### ToxicityGraph

Dependency graph for toxicity propagation.

```rust
pub struct ToxicityGraph {
    graph: DiGraph<ModuleNode, ()>,
    node_map: HashMap<String, NodeIndex>,
}
```

---

## Snapshot Structures

### MemoryRegion

Captured memory region.

```rust
pub struct MemoryRegion {
    pub start: usize,
    pub end: usize,
    pub prot: i32,
    pub path: Option<String>,
}
```

| Field   | Type             | Description              |
| :------ | :--------------- | :----------------------- |
| `start` | `usize`          | Start address            |
| `end`   | `usize`          | End address              |
| `prot`  | `i32`            | Protection flags (mmap)  |
| `path`  | `Option<String>` | Backing file path if any |

---

### WorkerSnapshot

Complete worker memory state.

```rust
pub struct WorkerSnapshot {
    pub regions: Vec<MemoryRegion>,
    pub segments: Vec<AlignedSegment>,
    pub uffd: OwnedFd,
}
```

---

### AlignedSegment

Page-aligned data segment.

```rust
pub struct AlignedSegment {
    pub base: usize,
    pub data: Vec<u8>,
}
```

---

## Sandbox Types

### SandboxStatus

Result of sandbox application.

```rust
pub enum SandboxStatus {
    Full,           // Landlock + Seccomp
    LandlockOnly,   // Landlock without Seccomp
    Degraded,       // Partial isolation
    Disabled,       // No isolation
}
```

---

## Configuration Structures

### TachConfig

Parsed configuration.

```rust
pub struct TachConfig {
    pub test_pattern: String,
    pub timeout: u64,
    pub workers: usize,
    pub isolation_strategy: IsolationStrategy,
    pub coverage: CoverageConfig,
}
```

---

### CoverageConfig

Coverage collection settings.

```rust
pub struct CoverageConfig {
    pub enabled: bool,
    pub source: Vec<String>,
    pub omit: Vec<String>,
    pub output: PathBuf,
    pub format: CoverageFormat,
}
```

---

### IsolationStrategy

Worker isolation mode.

```rust
pub enum IsolationStrategy {
    Auto,      // Choose based on toxicity
    Fork,      // Traditional fork
    Snapshot,  // userfaultfd snapshots
}
```

---

## Reporter Trait

Interface for test result reporting.

```rust
pub trait Reporter {
    fn on_run_start(&mut self, total: usize);
    fn on_test_started(&mut self, test: &RunnableTest);
    fn on_test_finished(&mut self, result: &TestResult);
    fn on_run_finished(&mut self, results: &[TestResult]);
}
```

| Method             | Description                     |
| :----------------- | :------------------------------ |
| `on_run_start`     | Called before first test        |
| `on_test_started`  | Called when test begins         |
| `on_test_finished` | Called when test completes      |
| `on_run_finished`  | Called after all tests complete |

---

## FFI Functions

### Python-Callable Functions

Exposed via PyO3 for the Python harness.

| Function                | Signature                   | Description                 |
| :---------------------- | :-------------------------- | :-------------------------- |
| `run_test`              | `(payload: bytes) -> bytes` | Execute test, return result |
| `reset_memory`          | `() -> bool`                | Trigger memory reset        |
| `get_coverage_buffer`   | `() -> memoryview`          | Get coverage ring buffer    |
| `get_mapping_buffer`    | `() -> memoryview`          | Get file mapping buffer     |
| `get_coverage_overflow` | `() -> u64`                 | Get overflow count          |
| `quiesce_allocator`     | `()`                        | Flush jemalloc caches       |
| `inject_entropy`        | `() -> bool`                | Refresh random state        |

---

### Internal FFI

Used between Rust components.

| Function             | Description                            |
| :------------------- | :------------------------------------- |
| `send_fd`            | Send file descriptor via SCM_RIGHTS    |
| `recv_fd`            | Receive file descriptor via SCM_RIGHTS |
| `encode_with_length` | Serialize with length prefix           |
| `decode_with_length` | Deserialize with length prefix         |

---

## Environment Variables

### Build-Time

| Variable      | Description             |
| :------------ | :---------------------- |
| `PYO3_PYTHON` | Python interpreter path |
| `MALLOC_CONF` | Jemalloc configuration  |

### Runtime

| Variable               | Description                       |
| :--------------------- | :-------------------------------- |
| `TACH_FORMAT`          | Output format (`human` or `json`) |
| `TACH_JUNIT_XML`       | JUnit XML output path             |
| `TACH_COVERAGE`        | Enable coverage (`1` or `true`)   |
| `TACH_NO_ISOLATION`    | Disable sandbox (`1` or `true`)   |
| `TACH_TARGET_PATH`     | Test path (set internally)        |
| `TACH_SUPERVISOR_SOCK` | UFFD socket path (set internally) |
| `RUST_LOG`             | Log level for debugging           |
| `CI`                   | Detected for reporter selection   |

---

## Exit Codes

| Code | Meaning             |
| :--- | :------------------ |
| 0    | All tests passed    |
| 1    | Some tests failed   |
| 2    | Configuration error |
| 3    | Discovery error     |
| 4    | Runtime error       |

---

## File Formats

### JUnit XML

```xml
<?xml version="1.0" encoding="UTF-8"?>
<testsuites>
  <testsuite name="tach" tests="N" failures="F" errors="E" skipped="S">
    <testcase name="test_name" classname="module" time="0.123"/>
    <testcase name="test_fail" classname="module" time="0.456">
      <failure message="AssertionError">Traceback...</failure>
    </testcase>
  </testsuite>
</testsuites>
```

### NDJSON (JSON Lines)

```json
{"event":"run_start","total":100}
{"event":"test_started","test":"test_example.py::test_foo"}
{"event":"test_finished","test":"test_example.py::test_foo","status":"pass","duration_ms":12}
{"event":"run_finished","passed":98,"failed":2,"skipped":0}
```

### LCOV Coverage

```
TN:
SF:/path/to/source.py
DA:1,1
DA:2,1
DA:5,0
LF:3
LH:2
end_of_record
```

---

## Related Documentation

- [Protocol](architecture/protocol.md) - IPC protocol details
- [Coverage](architecture/coverage.md) - Coverage implementation
- [Toxicity](architecture/toxicity.md) - Toxicity classification
- [Configuration](configuration.md) - Configuration options
