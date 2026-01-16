# Research Documentation

> This directory contains research papers and analysis informing Project Tach's architecture.

---

## Executive Summary

Project Tach implements a **Rust-native hypervisor** for Python test execution, moving away from the "dynamic tax" of interpreted test runners. The architecture addresses three core challenges:

1. **Cold Start Latency** - The "Import Tax" from `importlib` overhead
2. **Fork-Safety Paradox** - C-extensions with thread pools break across `fork()`
3. **Isolation vs Performance** - Achieving <5% overhead with full test isolation

> "This reliance on runtime reflection, while offering immense flexibility, imposes a severe 'dynamic tax' that scales linearly with the size of the codebase."
> — _Python Testing Engine Rust Breakthroughs_

---

## Topic Index

| Topic                                                                    | Description                                 | Key Papers                                 | CHANGELOG   |
| :----------------------------------------------------------------------- | :------------------------------------------ | :----------------------------------------- | :---------- |
| [Zygote Patterns](topic-archive.md#zygote-patterns-for-test-execution)   | DAAC algorithm, hierarchical initialization | Forklift, Zygote Tree Design               | 0.4.x       |
| [Memory Snapshotting](topic-archive.md#memory-snapshotting)              | userfaultfd, allocator interactions, TLS    | Memory Snapshotting, Allocator Interaction | 0.7.x       |
| [Fork Safety](topic-archive.md#fork-safety-in-tach)                      | Toxic modules, orphaned locks, C-extensions | Fork Safety, Static Analysis               | 0.3.x       |
| [Test Isolation](topic-archive.md#test-isolation-for-parallel-execution) | Namespaces, Matrix Layer, Shadow Plugin     | Compatibility Layer, Isolation Blueprint   | 0.2.x       |
| [Rust Integration](topic-archive.md#rust-integration-for-tach)           | Kineton engine, zero-copy loading, PyO3     | Rust Breakthroughs, Execution Blueprint    | 0.1.x-0.6.x |
| [Cross-Platform](topic-archive.md#cross-platform-process-cloning)        | macOS Mach, Windows NT cloning              | Cross-Platform Cloning                     | 0.8.x+      |

---

## Paper Summaries

### Core Architecture

| Paper                                        | Key Insight                               | Primary Quote                                                                                     |
| :------------------------------------------- | :---------------------------------------- | :------------------------------------------------------------------------------------------------ |
| **Python Testing Engine Rust Breakthroughs** | Rust hypervisor eliminates "Import Tax"   | "shifts the heavy lifting... into a high-performance, compiled substrate: Rust"                   |
| **Rust-CPython Execution Blueprint**         | CPython as "Leaf Node" under Rust control | "the runner is a high-performance native binary that acts as a hypervisor for the Python runtime" |

### Memory & Snapshotting

| Paper                                             | Key Insight                             | Primary Quote                                                                    |
| :------------------------------------------------ | :-------------------------------------- | :------------------------------------------------------------------------------- |
| **Python Memory Snapshotting with Userfaultfd**   | Microsecond-scale memory reset via UFFD | "achieve reset times measured in microseconds rather than milliseconds"          |
| **Userfaultfd and CPython Allocator Interaction** | Allocator metadata must be synchronized | "the critical state to capture is not just the 'heap' but the Data/BSS segments" |

### Fork Safety & Toxicity

| Paper                                             | Key Insight                              | Primary Quote                                                                                                               |
| :------------------------------------------------ | :--------------------------------------- | :-------------------------------------------------------------------------------------------------------------------------- |
| **Fork Safety of Python C-Extensions**            | Orphaned locks cause silent deadlocks    | "If a background thread holds a mutex at the precise nanosecond fork() is invoked, that lock is copied in a 'locked' state" |
| **Rust Static Analysis for Toxic Python Modules** | Static detection of fork-unsafe patterns | "identify 'toxic' or 'fork-unsafe' Python modules through static analysis"                                                  |

### Zygote Patterns

| Paper                                  | Key Insight                                  | Primary Quote                                                                                |
| :------------------------------------- | :------------------------------------------- | :------------------------------------------------------------------------------------------- |
| **Forklift (USENIX WoSC'24)**          | Hierarchical zygote trees improve latency 5x | "improving median invocation latency by about 5x while using only a modest memory footprint" |
| **Python Monorepo Zygote Tree Design** | DAAC algorithm for optimal tree construction | "A novel 'Dependency-Aware Agglomerative Clustering' (DAAC) algorithm"                       |

### Isolation & Compatibility

| Paper                                          | Key Insight                        | Primary Quote                                                                                     |
| :--------------------------------------------- | :--------------------------------- | :------------------------------------------------------------------------------------------------ |
| **Project Tach Compatibility Layer Blueprint** | Matrix Layer for syscall isolation | "Every syscall that modifies global state is transparently isolated per-worker with <5% overhead" |
| **Rust-Python Test Isolation Blueprint**       | Namespaces superior to LD_PRELOAD  | "Namespaces provide complete, kernel-enforced isolation with acceptable overhead"                 |

### Module Loading

| Paper                               | Key Insight                       | Primary Quote                                                                                               |
| :---------------------------------- | :-------------------------------- | :---------------------------------------------------------------------------------------------------------- |
| **Zero-Copy Python Module Loading** | Bypass importlib via mmap + C-API | "The term 'Zero-Copy' refers to the elimination of redundant userspace buffer copying during the I/O phase" |

### Cross-Platform

| Paper                                       | Key Insight                              | Primary Quote                                                                                 |
| :------------------------------------------ | :--------------------------------------- | :-------------------------------------------------------------------------------------------- |
| **Cross-Platform Process Cloning Research** | macOS/Windows require kernel-level hacks | "leveraging undocumented kernel primitives... to approximate the performance of Linux fork()" |

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
| Python Testing Engine Rust Breakthroughs      | [Rust Integration](topic-archive.md#rust-integration-for-tach)           | -                               |
| Rust-CPython Execution Blueprint              | [Rust Integration](topic-archive.md#rust-integration-for-tach)           | -                               |
| Zero-Copy Python Module Loading               | [Rust Integration](topic-archive.md#rust-integration-for-tach)           | -                               |
| Cross-Platform Process Cloning Research       | [Cross-Platform](topic-archive.md#cross-platform-process-cloning)        | -                               |

---

## Technology Requirements

| Component      | Requirement                    | Source                |
| :------------- | :----------------------------- | :-------------------- |
| Rust Toolchain | 1.88+ (2024 Edition)           | Cargo.toml            |
| Python         | 3.10+ (3.12+ for coverage)     | CLAUDE.md             |
| Linux Kernel   | 5.10+ (userfaultfd)            | _Memory Snapshotting_ |
| Allocator      | jemalloc 5+ (for tcache flush) | _Memory Snapshotting_ |

---

## Implementation Status

> **Single Source of Truth:** See [CHANGELOG.md](../../CHANGELOG.md) for the authoritative implementation status. Each version section shows checked/unchecked items indicating completion status.

---

## Open Questions

These research gaps require investigation before implementation:

1. **Allocator Metadata Consistency** - How to guarantee `ptmalloc` state after UFFD reset?
2. **C-Extension Threading** - Can background threads be "neutralized" automatically?
3. **Python 3.13+ JIT** - How does the JIT interact with CoW pages?

---

## External Research

See [external-research.md](external-research.md) for analysis of related projects:

- Firecracker (userfaultfd patterns)
- AFL-Snapshot-LKM (kernel-level snapshots)
- LibAFL (Rust fuzzing patterns)
- PyO3 (GIL management)
- rust-landlock / seccompiler (sandboxing)

---

## Archived Papers

Verbose source papers have been moved to `docs/archive/research-papers/`.

> **Note for LLMs:** These are lengthy documents. Read selectively based on specific needs rather than loading entire files.

---

## Related Documentation

- [topic-archive.md](topic-archive.md) - Detailed analysis of each research topic
- [external-research.md](external-research.md) - Analysis of related external projects
- [container-compatibility.md](container-compatibility.md) - Container runtime compatibility research
