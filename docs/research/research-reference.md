# Research Reference

Quick lookup for mapping research papers to Tach implementation.

> **See Also**: [research-investigation.md](research-investigation.md) for paper summaries and [topic-archive.md](topic-archive.md) for detailed analysis.

---

## Topic Archive

| Topic                                                                    | Description                                 | Key Papers                                 |
| :----------------------------------------------------------------------- | :------------------------------------------ | :----------------------------------------- |
| [Zygote Patterns](topic-archive.md#zygote-patterns-for-test-execution)   | DAAC algorithm, hierarchical initialization | Forklift, Zygote Tree Design               |
| [Memory Snapshotting](topic-archive.md#memory-snapshotting)              | userfaultfd, allocator interactions, TLS    | Memory Snapshotting, Allocator Interaction |
| [Fork Safety](topic-archive.md#fork-safety-in-tach)                      | Toxic modules, orphaned locks, C-extensions | Fork Safety, Static Analysis               |
| [Test Isolation](topic-archive.md#test-isolation-for-parallel-execution) | Namespaces, Matrix Layer, Shadow Plugin     | Compatibility Layer, Isolation Blueprint   |
| [Rust Integration](topic-archive.md#rust-integration-for-tach)           | Kineton engine, zero-copy loading, PyO3     | Rust Breakthroughs, Execution Blueprint    |
| [Cross-Platform](topic-archive.md#cross-platform-process-cloning)        | macOS Mach, Windows NT cloning              | Cross-Platform Cloning                     |

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

| Paper                                         | Topic File                                                               | Architecture Doc                |
| :-------------------------------------------- | :----------------------------------------------------------------------- | :------------------------------ |
| Forklift: Fitting Zygote Trees                | [Zygote Patterns](topic-archive.md#zygote-patterns-for-test-execution)   | `docs/architecture/zygote.md`   |
| Python Monorepo Zygote Tree Design            | [Zygote Patterns](topic-archive.md#zygote-patterns-for-test-execution)   | `docs/architecture/zygote.md`   |
| Python Memory Snapshotting with Userfaultfd   | [Memory Snapshotting](topic-archive.md#memory-snapshotting)              | `docs/architecture/snapshot.md` |
| Userfaultfd and CPython Allocator Interaction | [Memory Snapshotting](topic-archive.md#memory-snapshotting)              | `docs/architecture/snapshot.md` |
| Fork Safety of Python C-Extensions            | [Fork Safety](topic-archive.md#fork-safety-in-tach)                      | `docs/architecture/toxicity.md` |
| Rust Static Analysis for Toxic Python Modules | [Fork Safety](topic-archive.md#fork-safety-in-tach)                      | `docs/architecture/toxicity.md` |
| Project Tach Compatibility Layer Blueprint    | [Test Isolation](topic-archive.md#test-isolation-for-parallel-execution) | `docs/architecture/loader.md`   |
| Rust-Python Test Isolation Blueprint          | [Test Isolation](topic-archive.md#test-isolation-for-parallel-execution) | `docs/architecture/loader.md`   |
| Python Testing Engine Rust Breakthroughs      | [Rust Integration](topic-archive.md#rust-integration-for-tach)           | —                               |
| Rust-CPython Execution Blueprint              | [Rust Integration](topic-archive.md#rust-integration-for-tach)           | —                               |
| Zero-Copy Python Module Loading               | [Rust Integration](topic-archive.md#rust-integration-for-tach)           | —                               |
| Cross-Platform Process Cloning Research       | [Cross-Platform](topic-archive.md#cross-platform-process-cloning)        | —                               |

---

## Implementation Status

> **Single Source of Truth:** See [CHANGELOG.md](../../CHANGELOG.md) for the authoritative implementation status. Each version section shows checked/unchecked items indicating completion status.

---
