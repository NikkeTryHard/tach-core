# Research Reference

Quick lookup for mapping research papers to Tach implementation.

> **See Also**: [research-investigation.md](research-investigation.md) for paper summaries and the `topics/` folder for detailed analysis.

---

## Topic Files

| Topic                                                | Description                                 | Key Papers                                 |
| :--------------------------------------------------- | :------------------------------------------ | :----------------------------------------- |
| [Zygote Patterns](topics/zygote-patterns.md)         | DAAC algorithm, hierarchical initialization | Forklift, Zygote Tree Design               |
| [Memory Snapshotting](topics/memory-snapshotting.md) | userfaultfd, allocator interactions, TLS    | Memory Snapshotting, Allocator Interaction |
| [Fork Safety](topics/fork-safety.md)                 | Toxic modules, orphaned locks, C-extensions | Fork Safety, Static Analysis               |
| [Test Isolation](topics/isolation.md)                | Namespaces, Matrix Layer, Shadow Plugin     | Compatibility Layer, Isolation Blueprint   |
| [Rust Integration](topics/rust-integration.md)       | Kineton engine, zero-copy loading, PyO3     | Rust Breakthroughs, Execution Blueprint    |
| [Cross-Platform](topics/cross-platform.md)           | macOS Mach, Windows NT cloning              | Cross-Platform Cloning                     |

---

## Paper-to-Component Mapping

| Research Topic / Paper             | Primary Component   | Key Files (Proposed)              | Implementation Status |
| :--------------------------------- | :------------------ | :-------------------------------- | :-------------------- |
| **Forklift: Fitting Zygote Trees** | Zygote Manager      | `tach-core/src/zygote/tree.rs`    | In Progress           |
| **Cross-Platform Cloning**         | Platform Shim       | `tach-core/src/sys/unix/clone.rs` | Research              |
| **Userfaultfd Snapshotting**       | Snapshot Engine     | `tach-core/src/mem/snapshot.rs`   | Prototype             |
| **Matrix Layer (Virtualization)**  | Syscall Interceptor | `libtach_preload.so`, `tach-vfs/` | Planning              |
| **Toxic Module Analysis**          | Static Analyzer     | `tach-analyzer/src/toxic.rs`      | Research              |
| **Zero-Copy Loader**               | Module Loader       | `tach-core/src/python/loader.rs`  | Planning              |
| **Kineton (Rust Orchestrator)**    | Test Runner         | `tach-cli/src/runner/`            | In Progress           |

---

## Paper-to-Document Mapping

| Paper                                         | Topic File                                              | Architecture Doc                |
| :-------------------------------------------- | :------------------------------------------------------ | :------------------------------ |
| Forklift: Fitting Zygote Trees                | [zygote-patterns.md](topics/zygote-patterns.md)         | `docs/architecture/zygote.md`   |
| Python Monorepo Zygote Tree Design            | [zygote-patterns.md](topics/zygote-patterns.md)         | `docs/architecture/zygote.md`   |
| Python Memory Snapshotting with Userfaultfd   | [memory-snapshotting.md](topics/memory-snapshotting.md) | `docs/architecture/snapshot.md` |
| Userfaultfd and CPython Allocator Interaction | [memory-snapshotting.md](topics/memory-snapshotting.md) | `docs/architecture/snapshot.md` |
| Fork Safety of Python C-Extensions            | [fork-safety.md](topics/fork-safety.md)                 | `docs/architecture/toxicity.md` |
| Rust Static Analysis for Toxic Python Modules | [fork-safety.md](topics/fork-safety.md)                 | `docs/architecture/toxicity.md` |
| Project Tach Compatibility Layer Blueprint    | [isolation.md](topics/isolation.md)                     | `docs/architecture/loader.md`   |
| Rust-Python Test Isolation Blueprint          | [isolation.md](topics/isolation.md)                     | `docs/architecture/loader.md`   |
| Python Testing Engine Rust Breakthroughs      | [rust-integration.md](topics/rust-integration.md)       | —                               |
| Rust-CPython Execution Blueprint              | [rust-integration.md](topics/rust-integration.md)       | —                               |
| Zero-Copy Python Module Loading               | [rust-integration.md](topics/rust-integration.md)       | —                               |
| Cross-Platform Process Cloning Research       | [cross-platform.md](topics/cross-platform.md)           | —                               |

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
