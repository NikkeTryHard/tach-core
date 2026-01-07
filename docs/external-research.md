# External Research: Related Projects and Technologies

> **Purpose**: This document synthesizes research on external projects, libraries, and technologies that inform Project Tach's architecture. All findings include source links for verification.

---

## 1. Similar Python Test Runners and Isolation Projects

### pytest-forked

**Repository**: [pytest-dev/pytest-forked](https://github.com/pytest-dev/pytest-forked)

Runs each test in a forked subprocess for isolation. Key insights:

- Simple fork-per-test model ensures complete isolation
- Performance limited by fork overhead for each test
- **Tach Improvement**: Use zygote pattern to amortize fork cost across multiple tests

### pytest-isolate

**Repository**: [pytest-dev/pytest-isolate](https://github.com/pytest-dev/pytest-isolate)

Fork-based isolation without xdist overhead:

- Lightweight alternative to pytest-xdist for isolation
- No parallel execution, just isolation
- **Tach Improvement**: Combine isolation with parallel execution via worker pool

### pytest-parallel

**Repository**: [browsertron/pytest-parallel](https://github.com/browsertron/pytest-parallel)

Thread and process-based parallelization:

- Supports both threading and multiprocessing
- Worker pool pattern similar to Tach
- **Tach Improvement**: userfaultfd snapshots avoid pickle serialization overhead

### pytest-xdist

**Repository**: [pytest-dev/pytest-xdist](https://github.com/pytest-dev/pytest-xdist)

Industry standard for parallel pytest execution:

- Uses execnet for remote execution
- Pickle serialization bottleneck for test results
- **Tach Improvement**: Zero-copy shared memory IPC eliminates serialization

---

## 2. userfaultfd Implementations

### Firecracker MicroVM

**Repository**: [firecracker-microvm/firecracker](https://github.com/firecracker-microvm/firecracker)
**Documentation**: [Snapshotting](https://github.com/firecracker-microvm/firecracker/blob/main/docs/snapshotting/snapshotting.md)

AWS's microVM manager uses userfaultfd for lazy page loading:

```
Snapshot Creation:
1. Pause VM
2. Dump memory to file
3. Save device state
4. Resume or terminate

Snapshot Restore:
1. Create microVM with userfaultfd
2. Load device state
3. Demand-page memory from snapshot file
4. Resume execution
```

**Key Techniques**:

- `UFFDIO_COPY` for demand paging from snapshot file
- Background threads prefetch pages for performance
- Memory regions registered selectively (avoid kernel pages)

**Tach Application**: Same lazy-loading pattern for Python heap restoration

### CodeSandbox userfaultfd Implementation

**Repository**: [codesandbox/userfaultfd](https://github.com/nicfb/userfaultfd)

Rust wrapper for userfaultfd syscalls:

```rust
// Create userfaultfd file descriptor
let uffd = userfaultfd::Uffd::new()?;

// Register memory region for fault handling
uffd.register(addr, len)?;

// Handle page faults in separate thread
loop {
    let event = uffd.read_event()?;
    match event {
        Event::Pagefault { addr, .. } => {
            // Copy page from snapshot
            uffd.copy(addr, source_page)?;
        }
        _ => {}
    }
}
```

**Tach Application**: Direct Rust userfaultfd wrapper patterns

---

## 3. Fuzzing Snapshot Techniques

### AFL-Snapshot-LKM

**Repository**: [AFLplusplus/AFL-Snapshot-LKM](https://github.com/AFLplusplus/AFL-Snapshot-LKM)

Linux kernel module for ultra-fast process snapshots:

**Performance Claims**:

- 20-360% speedup over traditional fork-server
- Sub-millisecond snapshot/restore cycles
- Kernel-level CoW without fork() overhead

**Architecture**:

```
1. Take snapshot at stable point
2. Execute fuzz iteration
3. Restore via kernel module (faster than fork)
4. Repeat
```

**Key Insight**: Kernel-level snapshotting bypasses userspace overhead entirely

**Tach Application**: Consider kernel module for maximum performance (future)

### SnapFuzz

**Paper Reference**: "SnapFuzz: High-Throughput Fuzzing of Network Applications"

**Performance**: 62.8x speedup over baseline AFL for network applications

**Technique**:

- Snapshot after network initialization
- Restore includes socket state
- Eliminates connection setup overhead

**Tach Application**: Similar pattern for Django/database test initialization

### LibAFL

**Repository**: [AFLplusplus/LibAFL](https://github.com/AFLplusplus/LibAFL)

Rust fuzzing framework with multiple snapshot backends:

**Snapshot Executors**:

1. Fork-server executor (traditional)
2. In-process executor (no isolation)
3. Snapshot executor (userfaultfd-based)

```rust
// LibAFL snapshot usage pattern
let mut executor = SnapshotExecutor::new(
    harness,
    observers,
    &mut fuzzer,
    &mut state,
    &mut mgr,
)?;
```

**Tach Application**: Executor abstraction pattern for different isolation modes

---

## 4. Process Snapshotting and CRIU

### CRIU (Checkpoint/Restore In Userspace)

**Repository**: [checkpoint-restore/criu](https://github.com/checkpoint-restore/criu)
**Documentation**: [criu.org](https://criu.org/)

Full process state checkpointing:

**Capabilities**:

- Complete process tree dump/restore
- Memory, file descriptors, network connections
- Container live migration

**Performance Characteristics**:

- Dump: 100-500ms for typical process
- Restore: 50-200ms
- Too slow for per-test snapshots

**Tach Application**: CRIU is overkill for test isolation; userfaultfd is more targeted

### DMTCP

**Repository**: [dmtcp/dmtcp](https://github.com/dmtcp/dmtcp)

Distributed checkpointing without kernel modifications:

- Userspace-only implementation
- Plugin architecture for resource handling
- Slower than CRIU but more portable

**Tach Application**: Plugin concepts for handling different resource types

---

## 5. Rust Seccomp Libraries

### seccompiler (rust-vmm)

**Repository**: [rust-vmm/seccompiler](https://github.com/rust-vmm/seccompiler)
**Docs**: [docs.rs/seccompiler](https://docs.rs/seccompiler)

High-level seccomp-bpf wrapper used by Firecracker:

```rust
use seccompiler::{SeccompAction, SeccompFilter, SeccompRule};

// Create allowlist filter
let filter = SeccompFilter::new(
    vec![
        (libc::SYS_read, vec![]),
        (libc::SYS_write, vec![]),
        (libc::SYS_exit_group, vec![]),
    ].into_iter().collect(),
    SeccompAction::Errno(libc::EPERM as u32),
    SeccompAction::Allow,
    std::env::consts::ARCH.try_into()?,
)?;

filter.load()?;
```

**Key Features**:

- JSON-based filter definitions
- Architecture-aware syscall numbers
- BPF program generation

**Tach Application**: Reference for seccomp filter construction

### libseccomp-rs

**Repository**: [libseccomp-rs/libseccomp-rs](https://github.com/libseccomp-rs/libseccomp-rs)
**Docs**: [docs.rs/libseccomp](https://docs.rs/libseccomp)

Rust bindings for libseccomp:

```rust
use libseccomp::*;

let mut filter = ScmpFilterContext::new(ScmpAction::Allow)?;
filter.add_arch(ScmpArch::X8664)?;

// Block specific syscalls
let syscall = ScmpSyscall::from_name("execve")?;
filter.add_rule(ScmpAction::Errno(libc::EPERM), syscall)?;

filter.load()?;
```

**Key Features**:

- Syscall name resolution
- Multi-architecture support
- Argument filtering

**Tach Application**: Current Tach implementation could migrate to this for portability

---

## 6. Rust Landlock Libraries

### rust-landlock

**Repository**: [landlock-lsm/rust-landlock](https://github.com/landlock-lsm/rust-landlock)
**Docs**: [docs.rs/landlock](https://docs.rs/landlock)

Official Rust bindings for Landlock LSM:

```rust
use landlock::{
    ABI, Access, AccessFs, PathBeneath, PathFd,
    Ruleset, RulesetAttr, RulesetCreatedAttr,
};

fn sandbox_filesystem() -> Result<(), Box<dyn std::error::Error>> {
    let abi = ABI::V1;

    Ruleset::default()
        .handle_access(AccessFs::from_all(abi))?
        .create()?
        // Read-only access to /usr and /lib
        .add_rules(path_beneath_rules(
            &["/usr", "/lib"],
            AccessFs::from_read(abi)
        ))?
        // Read-write access to project directory
        .add_rule(PathBeneath::new(
            PathFd::new("/home/user/project")?,
            AccessFs::from_all(abi)
        ))?
        .restrict_self()?;

    Ok(())
}
```

**ABI Version Features**:
| ABI | Kernel | Features |
|-----|--------|----------|
| V1 | 5.13 | Basic filesystem access |
| V2 | 5.19 | `LANDLOCK_ACCESS_FS_REFER` (rename across dirs) |
| V3 | 6.2 | `LANDLOCK_ACCESS_FS_TRUNCATE` |
| V4 | 6.7 | Network restrictions (TCP bind/connect) |
| V5 | 6.10 | `LANDLOCK_ACCESS_FS_IOCTL_DEV` |

**Graceful Degradation Pattern**:

```rust
let abi = match landlock_abi_version() {
    Ok(v) if v >= 5 => ABI::V5,
    Ok(v) if v >= 4 => ABI::V4,
    Ok(v) if v >= 3 => ABI::V3,
    Ok(v) if v >= 2 => ABI::V2,
    Ok(_) => ABI::V1,
    Err(_) => return Ok(()), // Landlock not available
};
```

**Tach Application**: Current implementation aligns with these patterns

---

## 7. PyO3 Patterns for Embedding Python

### GIL Management

**Source**: [PyO3 Parallelism Guide](https://pyo3.rs/main/parallelism)

```rust
use pyo3::prelude::*;

#[pyfunction]
fn cpu_intensive_task(py: Python<'_>, data: &str) -> usize {
    // Release GIL during Rust computation
    py.detach(|| {
        // Pure Rust work - no Python objects
        data.lines()
            .map(|line| process_line(line))
            .sum()
    })
}
```

**Key Patterns**:

- `py.detach()` / `Python::allow_threads()` for GIL release
- `Py<T>` for Python objects that outlive GIL acquisition
- `Python::with_gil()` for reacquiring GIL

### Parallel Processing with Rayon

```rust
use pyo3::prelude::*;
use rayon::prelude::*;

#[pyfunction]
fn parallel_process(py: Python<'_>, items: Vec<Py<PyAny>>) -> Vec<usize> {
    // Detach from GIL for parallel work
    py.detach(|| {
        items.par_iter()
            .map(|item| {
                // Reacquire GIL for Python object access
                Python::with_gil(|py| {
                    let borrowed = item.bind(py);
                    // Process item
                    borrowed.len().unwrap_or(0)
                })
            })
            .collect()
    })
}
```

**Tach Application**: Worker threads must properly manage GIL during parallel test execution

### Free-Threaded Python (3.13+)

**Source**: [PEP 703](https://peps.python.org/pep-0703/)

PyO3 supports free-threaded Python builds:

- `PyInterpreterConfig_OWN_GIL` for sub-interpreter isolation
- True parallel Python execution without GIL contention
- Requires explicit synchronization for shared state

**Tach Application**: Future optimization for Python 3.14+ when no-GIL is stable

---

## 8. Jemalloc Integration for Snapshotting

### Thread Cache Flushing

**Source**: [jemalloc documentation](https://jemalloc.net/jemalloc.3.html)

Critical for consistent snapshots:

```c
// Flush calling thread's cache
mallctl("thread.tcache.flush", NULL, NULL, NULL, 0);

// Disable tcache before snapshot
bool enabled = false;
mallctl("thread.tcache.enabled", NULL, NULL, &enabled, sizeof(enabled));
```

**Rust Integration via jemalloc-sys**:

```rust
use jemalloc_sys::mallctl;
use std::ptr;

fn flush_tcache() {
    unsafe {
        mallctl(
            b"thread.tcache.flush\0".as_ptr() as *const _,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            0,
        );
    }
}
```

**When to Flush**:

- Before taking memory snapshot
- When thread goes idle (prevents memory hoarding)
- Automatic incremental flushing during allocation

**Tach Application**: Must call `thread.tcache.flush` before userfaultfd snapshot

### Arena Statistics for Debugging

```rust
fn get_allocated_bytes() -> usize {
    let mut allocated: usize = 0;
    let mut len = std::mem::size_of::<usize>();
    unsafe {
        mallctl(
            b"stats.allocated\0".as_ptr() as *const _,
            &mut allocated as *mut _ as *mut _,
            &mut len,
            ptr::null_mut(),
            0,
        );
    }
    allocated
}
```

---

## 9. mimalloc Considerations

### Thread-Local Allocation

**Source**: [Microsoft mimalloc](https://github.com/microsoft/mimalloc)

mimalloc uses thread-local segments:

- Each thread has private "pages" for small allocations
- `mi_heap_t` per thread/heap
- TLS state must be considered for snapshotting

**Key Differences from jemalloc**:
| Aspect | jemalloc | mimalloc |
|--------|----------|----------|
| Cache flush API | `thread.tcache.flush` | No explicit API |
| Thread exit | Automatic cleanup | Automatic cleanup |
| Heap reset | Per-arena | Per-heap `mi_heap_reset` |

**mimalloc Heap Reset**:

```c
mi_heap_t* heap = mi_heap_new();
// ... allocations ...
mi_heap_reset(heap);  // Free all allocations in heap
```

**Tach Application**: mimalloc lacks explicit tcache flush; may need wrapper or jemalloc preference

---

## 10. Linux Namespace Patterns

### Namespace Creation for Isolation

```rust
use nix::sched::{clone, CloneFlags};
use nix::sys::wait::waitpid;

fn create_isolated_process<F>(f: F) -> nix::Result<Pid>
where
    F: FnOnce() -> isize,
{
    const STACK_SIZE: usize = 1024 * 1024;
    let mut stack = vec![0u8; STACK_SIZE];

    let flags = CloneFlags::CLONE_NEWNS    // Mount namespace
              | CloneFlags::CLONE_NEWPID   // PID namespace
              | CloneFlags::CLONE_NEWNET   // Network namespace
              | CloneFlags::CLONE_NEWUSER; // User namespace

    clone(
        Box::new(f),
        &mut stack,
        flags,
        Some(libc::SIGCHLD),
    )
}
```

### User Namespace for Unprivileged Mount

```rust
use std::fs;

fn setup_user_namespace() -> std::io::Result<()> {
    // Map current user to root inside namespace
    fs::write("/proc/self/uid_map", format!("0 {} 1", unsafe { libc::getuid() }))?;
    fs::write("/proc/self/setgroups", "deny")?;
    fs::write("/proc/self/gid_map", format!("0 {} 1", unsafe { libc::getgid() }))?;
    Ok(())
}
```

### OverlayFS for Copy-on-Write Filesystem

```rust
use nix::mount::{mount, MsFlags};

fn setup_overlay(
    lower: &Path,
    upper: &Path,
    work: &Path,
    merged: &Path,
) -> nix::Result<()> {
    let options = format!(
        "lowerdir={},upperdir={},workdir={}",
        lower.display(),
        upper.display(),
        work.display()
    );

    mount(
        Some("overlay"),
        merged,
        Some("overlay"),
        MsFlags::empty(),
        Some(options.as_str()),
    )
}
```

**Tach Application**: Namespace + OverlayFS provides filesystem isolation without Docker

---

## 11. Performance Benchmarks from Research

### Fork Server vs Snapshot Performance

**Source**: AFL-Snapshot-LKM, SnapFuzz papers

| Technique             | Overhead per Iteration | Relative Speed |
| --------------------- | ---------------------- | -------------- |
| Fork (baseline)       | ~500-1000 μs           | 1x             |
| Fork server           | ~100-200 μs            | 5x             |
| userfaultfd snapshot  | ~10-50 μs              | 10-50x         |
| Kernel snapshot (LKM) | ~1-5 μs                | 100-500x       |

### Memory Overhead Comparison

| Technique     | Memory Overhead               | Notes                     |
| ------------- | ----------------------------- | ------------------------- |
| Full fork     | 100% (CoW)                    | Pages duplicated on write |
| userfaultfd   | Proportional to touched pages | Only faulted pages copied |
| Shared memory | Minimal                       | Explicit sharing required |

**Tach Target**: userfaultfd-based approach targets 10-50μs reset time

---

## 12. Key Takeaways for Tach

### Architecture Recommendations

1. **Snapshot Strategy**:
   - Use userfaultfd for memory reset (proven in Firecracker, AFL)
   - Flush jemalloc tcache before snapshot
   - Track heap, BSS, and data segments

2. **Isolation Stack**:
   - Landlock for filesystem (ABI V1 minimum)
   - Seccomp for syscall filtering (blacklist approach)
   - Namespaces for additional isolation if needed

3. **PyO3 Best Practices**:
   - Always release GIL during Rust-heavy operations
   - Use `Py<T>` for cross-thread Python object sharing
   - Prepare for free-threaded Python in 3.14+

4. **Allocator Handling**:
   - Prefer jemalloc for explicit cache control
   - Call `thread.tcache.flush` before snapshots
   - Consider per-interpreter heaps for isolation

### Future Research Directions

1. **Kernel Module**: Evaluate AFL-Snapshot-LKM approach for maximum performance
2. **Network Snapshotting**: Investigate SnapFuzz techniques for database connections
3. **Free-Threaded Python**: Monitor PEP 703 progress for parallel sub-interpreters
4. **Cross-Platform**: Track Mach vm_remap for macOS port (per research papers)

---

## References

### Repositories

- [Firecracker](https://github.com/firecracker-microvm/firecracker)
- [AFL++](https://github.com/AFLplusplus/AFLplusplus)
- [LibAFL](https://github.com/AFLplusplus/LibAFL)
- [CRIU](https://github.com/checkpoint-restore/criu)
- [rust-vmm/seccompiler](https://github.com/rust-vmm/seccompiler)
- [rust-landlock](https://github.com/landlock-lsm/rust-landlock)
- [PyO3](https://github.com/PyO3/pyo3)
- [jemalloc](https://github.com/jemalloc/jemalloc)

### Documentation

- [PyO3 Guide](https://pyo3.rs/)
- [Landlock Kernel Docs](https://docs.kernel.org/userspace-api/landlock.html)
- [jemalloc Manual](https://jemalloc.net/jemalloc.3.html)
- [Seccomp BPF](https://www.kernel.org/doc/html/latest/userspace-api/seccomp_filter.html)

### Papers

- "SnapFuzz: High-Throughput Fuzzing of Network Applications"
- "Forklift: Fitting Zygote Trees for Faster Package Initialization"
- AFL-Snapshot-LKM performance analysis

---

_Last Updated: 2026-01-07_
