# Physics Engine (Snapshot)

The Physics Engine manages memory snapshots using Linux `userfaultfd`.

---

## Overview

Traditional test isolation creates a new process per test. Tach instead:

1. **Captures** a "golden" memory snapshot after Python initialization
2. **Runs** the test, which modifies memory
3. **Resets** memory by invalidating dirty pages
4. **Restores** pages on-demand when accessed

This achieves sub-50 microsecond reset times.

```mermaid
flowchart TB
    subgraph Capture["SNAPSHOT CAPTURE"]
        Parse["Parse /proc/pid/maps"]
        Filter["Filter regions"]
        Copy["process_vm_readv"]
        Store["Store golden pages"]
    end

    subgraph Reset["MEMORY RESET"]
        Invalidate["madvise(MADV_DONTNEED)"]
        Fault["Page fault"]
        Restore["UFFDIO_COPY"]
    end

    Capture --> Reset
    Invalidate --> Fault --> Restore --> Invalidate
```

---

## Data Structures

### MemoryRegion

Represents a raw entry from `/proc/pid/maps`.

```rust
pub struct MemoryRegion {
    pub start: usize,
    pub end: usize,
    pub len: usize,
    pub perms: String,    // e.g., "rw-p"
    pub name: String,     // e.g., "[heap]"
}
```

### AlignedSegment

A page-aligned memory range for userfaultfd registration.

```rust
pub struct AlignedSegment {
    pub start: usize,     // Aligned down to page boundary
    pub end: usize,       // Aligned up to page boundary
    pub description: String,
}
```

### WorkerSnapshot

Holds the golden state for a specific worker.

```rust
pub struct WorkerSnapshot {
    pub uffd: Uffd,
    pub golden_pages: HashMap<usize, Vec<u8>>,
    pub regions: Vec<MemoryRegion>,
    #[cfg(target_arch = "x86_64")]
    pub tls_snapshot: Option<TlsSnapshot>,
}
```

| Field          | Description                                                  |
| :------------- | :----------------------------------------------------------- |
| `uffd`         | The userfaultfd object for this worker                       |
| `golden_pages` | Map of page address to page data                             |
| `regions`      | Original memory regions                                      |
| `tls_snapshot` | TLS snapshot for Python 3.13+ mimalloc support (x86_64 only) |

### SnapshotManager

Central supervisor-side authority.

```rust
pub struct SnapshotManager {
    pub available: bool,
    workers: HashMap<i32, WorkerSnapshot>,  // private field
    #[cfg(target_arch = "x86_64")]
    calibration: Option<TlsCalibration>,    // TLS calibration data
}
```

| Field         | Description                                             |
| :------------ | :------------------------------------------------------ |
| `available`   | Whether userfaultfd is available on this kernel         |
| `workers`     | Per-worker snapshot state (private field)               |
| `calibration` | TLS calibration data for mimalloc support (x86_64 only) |

### LibpythonInfo

Metadata for locating Python's global state.

```rust
pub struct LibpythonInfo {
    pub path: PathBuf,
    pub base_addr: usize,
    pub is_static: bool,
}
```

---

## Snapshot Capture Sequence

```mermaid
sequenceDiagram
    participant Worker
    participant Supervisor
    participant Kernel

    Worker->>Worker: Initialize Python
    Worker->>Worker: Quiesce jemalloc
    Worker->>Kernel: userfaultfd()
    Worker->>Supervisor: send_fd(uffd, pid)
    Worker->>Worker: SIGSTOP

    Supervisor->>Supervisor: recv_fd()
    Supervisor->>Kernel: read /proc/pid/maps
    Supervisor->>Supervisor: Filter regions
    Supervisor->>Supervisor: Parse libpython ELF
    Supervisor->>Kernel: process_vm_readv()
    Supervisor->>Supervisor: Store golden_pages
    Supervisor->>Kernel: UFFDIO_REGISTER
    Supervisor->>Worker: SIGCONT
```

### Step 1: Quiesce Allocator

Before snapshot, the worker flushes jemalloc state:

```rust
fn quiesce_allocator() {
    mallctl(c"thread.tcache.flush", ...);
    mallctl(c"epoch", ...);
}
```

This ensures deterministic heap layout.

### Step 2: UFFD Handshake

The worker creates a userfaultfd and sends it to the supervisor via SCM_RIGHTS:

```rust
fn init_snapshot_mode(supervisor_sock: &str) -> bool {
    let uffd = userfaultfd::UffdBuilder::new()
        .non_blocking(false)
        .create()?;

    send_fd(&sock, pid, uffd.as_raw_fd())?;
    raise(SIGSTOP);
    true
}
```

### Step 3: Memory Discovery

The supervisor parses `/proc/pid/maps`:

```
7f1234560000-7f1234580000 rw-p 00000000 00:00 0    [heap]
7f1234580000-7f12345a0000 rw-p 00000000 00:00 0
7ffd12340000-7ffd12360000 rw-p 00000000 00:00 0    [stack]
```

### Step 4: Region Filtering

Regions are filtered for snapshot eligibility:

| Region                | Included | Reason              |
| :-------------------- | :------- | :------------------ |
| `[heap]`              | Yes      | Python objects      |
| `[stack]`             | Yes      | Local variables     |
| Anonymous (`rw-p`)    | Yes      | Dynamic allocations |
| `libpython.so` data   | Yes      | Python globals      |
| `[vdso]`              | No       | Kernel-provided     |
| `[vsyscall]`          | No       | Kernel-provided     |
| `memfd:tach_coverage` | No       | Must survive reset  |
| Read-only             | No       | Code segments       |

### Step 5: ELF Parsing

For `libpython.so`, the supervisor uses `goblin` to find writable segments:

```rust
fn find_libpython_segments(path: &Path, base: usize) -> Vec<AlignedSegment> {
    let elf = goblin::elf::Elf::parse(&data)?;
    elf.program_headers
        .iter()
        .filter(|ph| ph.p_type == PT_LOAD && ph.p_flags & PF_W != 0)
        .map(|ph| AlignedSegment {
            start: base + ph.p_vaddr as usize,
            end: base + ph.p_vaddr as usize + ph.p_memsz as usize,
            description: "libpython data".into(),
        })
        .collect()
}
```

### Step 6: Memory Capture

```rust
fn capture_golden(pid: i32, regions: &[MemoryRegion]) -> HashMap<usize, Vec<u8>> {
    let mut golden = HashMap::new();
    for region in regions {
        let mut data = vec![0u8; region.len];
        let local = IoSliceMut::new(&mut data);
        let remote = RemoteIoVec {
            base: region.start,
            len: region.len,
        };
        process_vm_readv(pid, &mut [local], &[remote])?;

        for offset in (0..region.len).step_by(PAGE_SIZE) {
            let page_addr = region.start + offset;
            let page_data = data[offset..offset + PAGE_SIZE].to_vec();
            golden.insert(page_addr, page_data);
        }
    }
    golden
}
```

---

## Memory Reset

### The Seppuku Pattern

Workers reset their own memory:

```rust
fn reset_memory() {
    for region in RESET_REGIONS.lock().iter() {
        // Skip stack to avoid crashing current execution
        if region.name == "[stack]" {
            continue;
        }
        unsafe {
            libc::madvise(
                region.start as *mut _,
                region.len,
                libc::MADV_DONTNEED,
            );
        }
    }
}
```

`MADV_DONTNEED` tells the kernel to discard the pages. The next access triggers a page fault.

### Page Fault Handling

```mermaid
sequenceDiagram
    participant Worker
    participant Kernel
    participant Supervisor

    Worker->>Kernel: Access invalidated page
    Kernel->>Supervisor: UFFD event (page fault)
    Supervisor->>Supervisor: Lookup golden_pages[addr]
    Supervisor->>Kernel: UFFDIO_COPY(addr, data)
    Kernel->>Worker: Resume execution
```

```rust
fn handle_pending_faults(worker: &mut WorkerSnapshot) {
    loop {
        match worker.uffd.read_event() {
            Ok(Event::Pagefault { addr, .. }) => {
                let page_addr = addr & !0xFFF;
                if let Some(data) = worker.golden_pages.get(&page_addr) {
                    worker.uffd.copy(page_addr, data, true)?;
                } else {
                    worker.uffd.zeropage(page_addr, PAGE_SIZE, true)?;
                }
            }
            Err(e) if e.kind() == WouldBlock => break,
            _ => break,
        }
    }
}
```

---

## TLS Restoration System

Python 3.13+ uses mimalloc instead of pymalloc. mimalloc stores critical heap pointers in Thread Local Storage (TLS). Without restoring TLS alongside memory, workers suffer from "Fractured Brain" syndrome where heap pointers point to stale data.

### The Restoration Quadrant

The complete memory restoration requires four components:

```mermaid
flowchart TB
    subgraph Quadrant["RESTORATION QUADRANT"]
        BSS["BSS Segment<br/>(Python globals)"]
        Heap["Heap<br/>(Object allocations)"]
        Stack["Stack<br/>(Local variables)"]
        TCB["Thread Control Block<br/>(TLS + fs_base)"]
    end

    MADV["madvise(MADV_DONTNEED)"]
    UFFD["userfaultfd lazy restore"]
    TLS["TLS direct restore"]

    MADV --> BSS & Heap & Stack
    UFFD --> BSS & Heap & Stack
    TLS --> TCB

    style TCB fill:#f96,stroke:#333,stroke-width:2px
```

### TlsSnapshot

```rust
pub struct TlsSnapshot {
    pub fs_base: usize,           // x86_64 fs_base register (TCB address)
    pub tls_data: Vec<u8>,        // Captured TLS memory (dynamic size)
    pub tls_region_start: usize,  // From /proc/pid/maps
    pub tls_region_end: usize,
}
```

### Key Functions (x86_64 only)

```rust
pub fn get_fs_base_ptrace(pid: Pid) -> Result<usize>;
pub fn set_fs_base_ptrace(pid: Pid, fs_base: usize) -> Result<()>;
pub fn capture_tls_snapshot(pid: Pid) -> Result<TlsSnapshot>;
pub fn restore_tls_snapshot(pid: Pid, snapshot: &TlsSnapshot) -> Result<()>;
pub fn restore_vectorized(pid: Pid, regions: &[RestoreRegion]) -> Result<VectorizedRestoreResult>;
```

**Dynamic TLS Sizing**: Capture size from `/proc/pid/maps` boundaries, not fixed 12KB. Handles TensorFlow/PyTorch C-extensions with large Dynamic Thread Vectors.

**Performance**: Expected 20-40% reduction in restoration time compared to individual restores.

---

## Full Reset Methods

### reset_worker_full

Complete worker reset with TLS restoration.

```rust
#[cfg(target_arch = "x86_64")]
pub fn reset_worker_full(&self, pid: Pid) -> Result<()>;
```

**Sequence**:

1. Invalidate memory pages via `process_madvise(MADV_DONTNEED)`
2. Restore TLS block via `process_vm_writev`
3. Restore fs_base register via `ptrace ARCH_SET_FS`
4. Page faults restore heap/BSS via userfaultfd

### reset_worker_full_vectorized

Optimized version using batched writes.

```rust
#[cfg(target_arch = "x86_64")]
pub fn reset_worker_full_vectorized(&self, pid: Pid) -> Result<VectorizedRestoreResult>;
```

**Sequence**:

1. Invalidate memory pages via `process_madvise(MADV_DONTNEED)`
2. Vectorized restore: TLS + critical regions in single syscall
3. Restore fs_base register via ptrace
4. Page faults restore remaining heap/BSS via userfaultfd

---

## System Calls

| System Call              | Purpose                                           |
| :----------------------- | :------------------------------------------------ |
| `userfaultfd`            | Create tracking object for lazy restoration       |
| `process_vm_readv`       | Copy worker memory to supervisor without ptrace   |
| `process_vm_writev`      | Write memory to worker (TLS restoration)          |
| `madvise(MADV_DONTNEED)` | Drop pages, forcing re-fault on access            |
| `ioctl(UFFDIO_REGISTER)` | Register memory regions for fault notification    |
| `ioctl(UFFDIO_COPY)`     | Copy golden page back to worker                   |
| `ioctl(UFFDIO_ZEROPAGE)` | Zero-fill new pages                               |
| `pidfd_open`             | Get process file descriptor for remote operations |
| `process_madvise`        | Remote memory invalidation (syscall 440)          |
| `ptrace(ARCH_PRCTL)`     | Get/set fs_base register for TLS (x86_64)         |

---

## Coverage Buffer Exclusion

The coverage ring buffer must survive memory resets:

```rust
fn should_snapshot(region: &MemoryRegion) -> bool {
    // Exclude coverage buffer and other tach-managed memfd regions
    if region.name.contains("tach_coverage") ||
       region.name.contains("memfd:tach") {
        return false;
    }
    // ... other filters
}
```

---

## Performance Characteristics

| Operation           | Time             |
| :------------------ | :--------------- |
| Snapshot capture    | ~10ms (one-time) |
| Memory reset        | **< 50us**       |
| Page fault handling | ~1us per page    |

---

## Kernel Requirements

| Feature          | Minimum Kernel | Recommended |
| :--------------- | :------------- | :---------- |
| userfaultfd      | 4.11           | 5.11+       |
| process_vm_readv | 3.2            | 5.0+        |
| process_madvise  | 5.10           | 5.10+       |

---

## Related Documentation

- [Allocator](allocator.md) - Jemalloc quiesce sequence
- [Zygote Lifecycle](zygote.md) - Worker initialization
- [IPC Protocol](protocol.md) - SCM_RIGHTS fd passing
