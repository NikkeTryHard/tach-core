# Research Reference

Quick lookup for mapping research papers and technical deep-dives to Tach implementation.

## Paper-to-Component Mapping

| Research Topic / Paper | Primary Component | Key Files (Proposed) | Implementation Status |
| :--- | :--- | :--- | :--- |
| **Forklift: Fitting Zygote Trees** | Zygote Manager | `tach-core/src/zygote/tree.rs` | In Progress |
| **Cross-Platform Cloning** | Platform Shim | `tach-core/src/sys/unix/clone.rs` | Research |
| **Userfaultfd Snapshotting** | Snapshot Engine | `tach-core/src/mem/snapshot.rs` | Prototype |
| **Matrix Layer (Virtualization)** | Syscall Interceptor | `libtach_preload.so`, `tach-vfs/` | Planning |
| **Toxic Module Analysis** | Static Analyzer | `tach-analyzer/src/toxic.rs` | Research |
| **Zero-Copy Loader** | Module Loader | `tach-core/src/python/loader.rs` | Planning |
| **Kineton (Rust Orchestrator)** | Test Runner | `tach-cli/src/runner/` | In Progress |

---

## Paper-to-Document Mapping

### Forklift: Fitting Zygote Trees (Yang et al.)
**Maps to docs:**
- `docs/architecture/zygote-tree.md` - Hierarchical initialization and DAAC algorithm.
- `docs/architecture/memory-sharing.md` - CoW optimization strategies.

**Key searchable quotes:**
- "Forklift: A new algorithm for training zygote trees based on invocation history."
- "Improving median invocation latency by about 5× while using only a modest memory footprint."

### Cross-Platform Process Cloning Research
**Maps to docs:**
- `docs/architecture/platform-support.md` - Mach VM remapping (macOS) and NT cloning (Windows).

**Key searchable quotes:**
- "The Darwin kernel (XNU)... Mach virtual memory remapping... approximate the performance of Linux fork()."
- "Windows NT kernel... NtCreateUserProcess... alien to the Win32 subsystem."

### Fork-Safety of Python C-Extensions
**Maps to docs:**
- `docs/architecture/safety-constraints.md` - Handling "Poison Forks" and orphaned locks.
- `docs/architecture/static-analysis.md` - Identifying toxic modules (gRPC, TensorFlow).

**Key searchable quotes:**
- "The fundamental assumptions of fork()... are incompatible with the complex internal threading pools."
- "Orphaned lock scenario: If a background thread holds a mutex... that lock is copied into the child process's memory in a 'locked' state."

### Python Memory Snapshotting with Userfaultfd
**Maps to docs:**
- `docs/architecture/snapshot-engine.md` - UFFD mechanics and allocator stability.
- `docs/architecture/allocator-tuning.md` - jemalloc/mimalloc integration for deterministic heaps.

**Key searchable quotes:**
- "Userfaultfd (UFFD) mechanism offers a compelling alternative: user-space demand paging."
- "Preserving the logical consistency of the allocator's metadata across the temporal boundary of a restore."

### Project Tach Compatibility Layer (The Matrix)
**Maps to docs:**
- `docs/architecture/virtualization.md` - Filesystem and Network isolation via `LD_PRELOAD`.

**Key searchable quotes:**
- "Isolation without overhead requires moving from userspace interception to kernel-level integration."
- "Rewrite: /tmp/log.txt -> /tmp/tach_overlay/5/log.txt"

---

## Implementation Checklist

Derived from research requirements:

### 1. Zygote Management
- [ ] Implement **DAAC (Dependency-Aware Agglomerative Clustering)** for tree construction.
- [ ] Build **Side-Effect Toxicity** scanner to prevent "poisoned" zygotes.
- [ ] Support **Hierarchical Forking** (Parent -> Zygote -> Worker).

### 2. Memory & Performance
- [ ] Implement **userfaultfd** handler in Rust for microsecond-scale resets.
- [ ] Integrate **jemalloc** with manual cache flushing to prevent heap corruption during snapshots.
- [ ] Develop **Zero-Copy Loader** using `mmap` to bypass `importlib` overhead.

### 3. Isolation (The Matrix)
- [ ] Create **LD_PRELOAD** shim for `open`, `bind`, and `connect` interception.
- [ ] Implement **Path Rewriting** logic for per-worker `/tmp` and `/dev/shm` isolation.
- [ ] Add **eBPF** hooks for syscall-level virtualization on supported Linux kernels.

### 4. Platform Compatibility
- [ ] Implement **Mach VM Remapping** for macOS "pseudo-fork."
- [ ] Implement **NT Process Cloning** for Windows performance parity.
- [ ] Fallback to **Spawn/Forkserver** for non-supported environments.

### 5. Static Analysis
- [ ] Use **ruff_python_parser** to detect `threading.Thread` or `multiprocessing` calls in top-level module code.
- [ ] Map **C-Extension dependencies** to a known "Fork-Unsafe" database (NumPy, gRPC, etc.).