# API Reference

Complete reference for Tach internal APIs and data structures.

---

## Core Data Structures

### TestCase

```rust
pub struct TestCase {
    pub name: String,        // Function name (e.g., `test_foo`)
    pub is_async: bool,      // Whether function is async
    pub fixtures: Vec<String>, // Required fixture names
    pub markers: Vec<String>,  // Applied markers
    pub lineno: usize,       // Line number in source file
}
```

---

### TestModule

```rust
pub struct TestModule {
    pub path: PathBuf,                   // Absolute path to file
    pub tests: Vec<TestCase>,            // Discovered test functions
    pub fixtures: Vec<FixtureDefinition>, // Defined fixtures
    pub imports: Vec<String>,            // Import statements
}
```

---

### FixtureDefinition

```rust
pub struct FixtureDefinition {
    pub name: String,              // Fixture function name
    pub scope: FixtureScope,       // Lifetime scope
    pub dependencies: Vec<String>, // Other fixtures this depends on
    pub is_async: bool,            // Whether fixture is async
    pub autouse: bool,             // Whether auto-applied
}
```

---

### FixtureScope

```rust
pub enum FixtureScope {
    Function,  // Created per test (default)
    Class,     // Shared within test class
    Module,    // Shared within test module
    Session,   // Shared across entire session
}
```

---

### RunnableTest

```rust
pub struct RunnableTest {
    pub file_path: PathBuf,              // Path to test file
    pub test_name: String,               // Fully qualified name (node ID)
    pub is_async: bool,                  // Whether test is async
    pub fixtures: Vec<ResolvedFixture>,  // Resolved fixture chain
    pub is_toxic: bool,                  // Requires worker restart
}
```

---

### ResolvedFixture

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
    pub test_id: u32,              // Unique test identifier
    pub file_path: String,         // Path to test file
    pub test_name: String,         // Fully qualified test name
    pub is_async: bool,
    pub fixtures: Vec<FixtureInfo>,
    pub log_fd: i32,               // File descriptor for logging
    pub debug_socket_path: String, // Path for pdb tunneling
    pub is_toxic: bool,            // Whether worker should exit
}
```

---

### TestResult

Sent from Worker to Supervisor.

```rust
#[derive(Serialize, Deserialize)]
pub struct TestResult {
    pub test_id: u32,       // Matching test identifier
    pub status: u8,         // Status code (see below)
    pub duration_ns: u64,   // Execution time in ns
    pub message: String,    // Error message if failed
}
```

---

### FixtureInfo

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

## Coverage Module API

High-performance coverage collection using shared memory ring buffers and lock-free atomic operations.

### Constants

```rust
pub const DEFAULT_CAPACITY: usize = 262_144;  // 4MB total (16 bytes/entry)
pub const HEADER_SIZE: usize = 64;            // Cache-line aligned
pub const ENTRY_SIZE: usize = 16;
pub const MEMFD_NAME: &str = "tach_coverage";
pub const MAPPING_CAPACITY: usize = 8_192;
pub const MAPPING_ENTRY_SIZE: usize = 256;
pub const MAPPING_MEMFD_NAME: &str = "tach_mapping";
```

---

### RingBufferHeader

```rust
#[repr(C, align(64))]
pub struct RingBufferHeader {
    pub write_idx: AtomicU64,     // Next write position
    pub read_idx: AtomicU64,      // Next read position
    pub capacity: u64,            // Buffer capacity in entries
    pub overflow_count: AtomicU64, // Number of dropped entries
    _padding: [u8; 32],
}

impl RingBufferHeader {
    #[inline] pub fn is_full(&self) -> bool;
    #[inline] pub fn available(&self) -> u64;
}
```

---

### CoverageEntry

```rust
#[repr(C, align(16))]
#[derive(Clone, Copy, Debug, Default)]
pub struct CoverageEntry {
    pub code_id: u64,  // Memory address of code object
    pub lineno: u32,   // Line number executed
    pub flags: u32,    // Event type (LINE=0x01, CALL=0x02, RETURN=0x04)
}

impl CoverageEntry {
    #[inline] pub fn line(code_id: u64, lineno: u32) -> Self;
}
```

---

### MappingEntry

```rust
#[repr(C, align(8))]
pub struct MappingEntry {
    pub code_id: u64,        // Memory address of code object
    pub filename_len: u16,   // Length of filename (max 240)
    pub _padding: [u8; 6],
    pub filename: [u8; 240], // UTF-8 filename, left-truncated if long
}

impl MappingEntry {
    pub fn new(code_id: u64, filename: &str) -> Self;
    pub fn filename(&self) -> String;
}
```

---

### CoverageRingBuffer

Shared memory ring buffer via `memfd_create` + `mmap` for zero-copy IPC.

```rust
pub struct CoverageRingBuffer {
    ptr: *mut u8,
    size: usize,
    fd: i32,
    capacity: usize,
}

unsafe impl Send for CoverageRingBuffer {}
unsafe impl Sync for CoverageRingBuffer {}

impl CoverageRingBuffer {
    pub fn new(capacity: usize) -> Result<Self>;
    pub fn fd(&self) -> i32;
    #[inline] pub fn header(&self) -> &RingBufferHeader;
    #[inline] pub fn header_mut(&self) -> &mut RingBufferHeader;

    /// Write entry using lock-free CAS loop. Returns false if buffer full.
    #[inline] pub fn write(&self, entry: CoverageEntry) -> bool;

    /// Drain up to max_entries into out vector. Returns count read.
    pub fn drain(&self, out: &mut Vec<CoverageEntry>, max_entries: usize) -> usize;

    pub fn overflow_count(&self) -> u64;
    pub fn base_addr(&self) -> usize;
    pub fn region_size(&self) -> usize;
}
```

---

### MappingRingBuffer

Similar to `CoverageRingBuffer` but with 256-byte entries for filenames.

```rust
pub struct MappingRingBuffer { /* same fields */ }

impl MappingRingBuffer {
    pub fn new(capacity: usize) -> Result<Self>;
    #[inline] pub fn header(&self) -> &RingBufferHeader;
    #[inline] pub fn write(&self, entry: MappingEntry) -> bool;
    pub fn drain(&self, out: &mut Vec<MappingEntry>, max_entries: usize) -> usize;
    pub fn overflow_count(&self) -> u64;
}
```

---

### CoverageAggregator

Drains ring buffers and accumulates coverage data in a dedicated thread.

```rust
pub type CoverageData = HashMap<(String, u32), u64>;

pub struct CoverageAggregator {
    data: Arc<Mutex<CoverageData>>,
    code_map: Arc<RwLock<HashMap<u64, String>>>,
    stop_flag: Arc<AtomicBool>,
    thread_handle: Option<JoinHandle<()>>,
}

impl CoverageAggregator {
    pub fn new() -> Self;
    pub fn start(&mut self, poll_interval: Duration);
    pub fn register_code(&self, code_id: u64, filename: String);
    pub fn stop(&mut self);
    pub fn get_data(&self) -> CoverageData;
    pub fn take_data(&mut self) -> CoverageData;  // Zero-copy extraction
    pub fn covered_lines(&self) -> usize;
    pub fn total_hits(&self) -> u64;
}
```

---

### Global Buffer Functions

```rust
pub fn init_coverage_buffer(capacity: usize) -> Result<&'static CoverageRingBuffer>;
pub fn get_coverage_buffer() -> Option<&'static CoverageRingBuffer>;
pub fn is_coverage_enabled() -> bool;
pub fn init_mapping_buffer(capacity: usize) -> Result<&'static MappingRingBuffer>;
pub fn get_mapping_buffer() -> Option<&'static MappingRingBuffer>;
```

---

## Namespace Module API

Worker isolation using Linux Namespaces and OverlayFS.

### Main Function

```rust
/// Set up complete isolation (Iron Dome).
/// Sequence: unshare -> private mounts -> create dirs -> RO root -> tmpfs -> overlays
/// Set TACH_NO_ISOLATION=1 to disable.
pub fn setup_filesystem(worker_id: u32, project_root: &Path) -> Result<()>;
```

### Helper Functions

```rust
#[inline] pub fn worker_base_dir(worker_id: u32) -> PathBuf;
// Returns: /run/tach/worker_{worker_id}

pub fn tmp_overlay_options(base: &Path) -> String;
// Format: lowerdir=/tmp,upperdir={base}/tmp_upper,workdir={base}/tmp_work

pub fn project_overlay_options(base: &Path, project_root: &Path) -> String;
// Format: lowerdir={project_root},upperdir={base}/proj_upper,workdir={base}/proj_work

pub fn is_isolation_disabled() -> bool;
// Returns true only if TACH_NO_ISOLATION=1
```

### Overlay Directory Structure

```
/run/tach/
  worker_0/
    tmp_upper/     # Writable layer for /tmp
    tmp_work/      # OverlayFS work directory
    proj_upper/    # Writable layer for project
    proj_work/     # OverlayFS work directory
```

---

## LogCapture API

Non-blocking stdout/stderr capture using memfd.

```rust
pub const LOG_BUFFER_SIZE: usize = 1024 * 1024;  // 1MB per slot

pub struct LogCapture {
    fds: HashMap<usize, RawFd>,
    num_slots: usize,
}

impl LogCapture {
    pub fn new(max_slots: usize) -> Result<Self>;
    pub fn get_fd(&self, slot: usize) -> Option<RawFd>;
    pub fn slot_count(&self) -> usize;
    pub fn read_and_clear(&self, slot: usize) -> Result<String>;
}

impl Drop for LogCapture {
    fn drop(&mut self);  // Closes all file descriptors
}

/// Redirect stdout/stderr to fd (call in worker after fork).
pub fn redirect_output(fd: RawFd) -> Result<()>;
```

---

## Toxicity Structures

### ToxicityReport

```rust
pub struct ToxicityReport {
    pub is_toxic: bool,
    pub reason: Option<String>,
    pub propagated_from: Vec<String>,
}
```

### ToxicityGraph

```rust
pub struct ToxicityGraph {
    graph: DiGraph<ModuleNode, ()>,
    node_map: HashMap<String, NodeIndex>,
}
```

---

## Snapshot Structures

### MemoryRegion

```rust
pub struct MemoryRegion {
    pub start: usize,        // Start address
    pub end: usize,          // End address
    pub prot: i32,           // Protection flags (mmap)
    pub path: Option<String>, // Backing file path if any
}
```

### WorkerSnapshot

```rust
pub struct WorkerSnapshot {
    pub regions: Vec<MemoryRegion>,
    pub segments: Vec<AlignedSegment>,
    pub uffd: OwnedFd,
}
```

### AlignedSegment

```rust
pub struct AlignedSegment {
    pub base: usize,
    pub data: Vec<u8>,
}
```

---

## Sandbox Types

```rust
pub enum SandboxStatus {
    Full,         // Landlock + Seccomp
    LandlockOnly, // Landlock without Seccomp
    Degraded,     // Partial isolation
    Disabled,     // No isolation
}
```

---

## Configuration Structures

### TachConfig

```rust
pub struct TachConfig {
    pub test_pattern: String,
    pub timeout: u64,
    pub workers: usize,
    pub isolation_strategy: IsolationStrategy,
    pub coverage: CoverageConfig,
}
```

### CoverageConfig

```rust
pub struct CoverageConfig {
    pub enabled: bool,
    pub source: Vec<String>,
    pub omit: Vec<String>,
    pub output: PathBuf,
    pub format: CoverageFormat,
}
```

### IsolationStrategy

```rust
pub enum IsolationStrategy {
    Auto,      // Choose based on toxicity
    Fork,      // Traditional fork
    Snapshot,  // userfaultfd snapshots
}
```

---

## Reporter Trait

```rust
pub trait Reporter {
    fn on_run_start(&mut self, total: usize);
    fn on_test_started(&mut self, test: &RunnableTest);
    fn on_test_finished(&mut self, result: &TestResult);
    fn on_run_finished(&mut self, results: &[TestResult]);
}
```

---

## FFI Functions

### Python-Callable Functions (PyO3)

| Function                | Signature                   | Description                 |
| :---------------------- | :-------------------------- | :-------------------------- |
| `run_test`              | `(payload: bytes) -> bytes` | Execute test, return result |
| `reset_memory`          | `() -> bool`                | Trigger memory reset        |
| `get_coverage_buffer`   | `() -> memoryview`          | Get coverage ring buffer    |
| `get_mapping_buffer`    | `() -> memoryview`          | Get file mapping buffer     |
| `get_coverage_overflow` | `() -> u64`                 | Get overflow count          |
| `quiesce_allocator`     | `()`                        | Flush jemalloc caches       |
| `inject_entropy`        | `() -> bool`                | Refresh random state        |

### Coverage PyO3 Functions

```rust
#[pyfunction]
pub fn py_record_line(py: Python<'_>, code_id: u64, lineno: u32) -> bool;

#[pyfunction]
pub fn py_is_coverage_enabled() -> bool;

#[pyfunction]
pub fn py_get_coverage_overflow() -> u64;

#[pyfunction]
pub fn py_record_py_start(py: Python<'_>, code_id: u64, filename: String);

#[pyfunction]
pub fn py_get_mapping_overflow() -> u64;
```

### Internal FFI

| Function             | Description                            |
| :------------------- | :------------------------------------- |
| `send_fd`            | Send file descriptor via SCM_RIGHTS    |
| `recv_fd`            | Receive file descriptor via SCM_RIGHTS |
| `encode_with_length` | Serialize with length prefix           |
| `decode_with_length` | Deserialize with length prefix         |

---

## Environment Variables

See [Configuration Reference](configuration.md#environment-variables) for complete environment variable documentation.

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

### NDJSON

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
