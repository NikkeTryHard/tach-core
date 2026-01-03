# Changelog

All notable changes to Project Tach will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.8.0-alpha] - 2026-01-03

### Physics-Complete Release

This release marks the completion of the **Restoration Physics Engine** - the core technology that enables Tach to snapshot and restore Python interpreter state with bit-perfect accuracy.

### Highlights

- **Python 3.13 Free-Threaded Support**: Full TLS/mimalloc consistency for Python 3.13+ builds
- **Self-Calibrating TLS Restoration**: Dynamic sentinel scan discovers allocator offsets at runtime
- **Iron Dome Sandbox**: Landlock + Seccomp hardening with graceful kernel degradation
- **Zero Ghost Objects**: Verified via 1000-cycle RSS stability test (0.62% growth)

### Added

#### Phase 2.3: TLS Restoration (The Cardiac Transplant)

- `capture_tls_snapshot()` / `restore_tls_snapshot()` for Thread-Local Storage preservation
- Dynamic TLS boundary detection from `/proc/pid/maps` (replaces 12KB hardcode)
- `TlsCalibration` module with sentinel pattern allocation (`0xDEADC0DE_BAADF00D`)
- Self-calibrating offset discovery for `mi_heap_t` (Python 3.13 mimalloc support)
- Boot logging: `[restoration] Sentinel found at fs_base + 0xXXXX`

#### Phase 2.2: Iron Dome Activation

- Landlock filesystem sandboxing (ABI V1+ support)
- Seccomp syscall filtering with blacklist approach
- Safe workers: Full Iron Dome (Landlock + Seccomp)
- Toxic workers: Landlock only (for subprocess support)
- Graceful degradation on older kernels (no crashes)

#### Phase 2.1: Deep State Integration

- Unified `SnapshotManager` with UFFD handling
- BSS/Heap split-brain prevention
- `process_vm_readv`/`process_vm_writev` for cross-process memory operations
- Stack restoration via ptrace

#### Testing Infrastructure

- `test_rss_stability_after_1000_restores` - Ghost Hunt validation (1000 cycles, 5% limit)
- `test_rss_stability_quick` - CI-friendly 100-cycle variant
- `test_bss_heap_split_brain_validation` - PyFloat_FreeList consistency test
- Calibration integration tests with boot log verification

### Changed

- `TLS_SNAPSHOT_SIZE` renamed to `TLS_SNAPSHOT_SIZE_HINT` (now a minimum, not a hard limit)
- `SnapshotManager` now supports optional calibration via `calibrate()` method
- Improved "Fractured Brain" detection for partial TLS reads

### Technical Details

#### Restoration Quadrant

The Physics Engine restores four memory regions atomically:

1. **TCB** (Thread Control Block) - pthread structures
2. **BSS** (.data/.bss) - Python allocator freelists
3. **Heap** - PyObject graph
4. **Stack** - Call frames and local variables

#### Kernel Requirements

- Linux 5.13+ for full Landlock support
- `userfaultfd` privileges (CAP_SYS_PTRACE or `vm.unprivileged_userfaultfd=1`)
- x86_64 or aarch64 architecture

### Performance

- **Ghost Hunt Results**: 562 iterations/sec, 0.62% RSS growth over 1000 cycles
- **TLS Restoration**: Sub-millisecond via ptrace ARCH_PRCTL
- **Memory Snapshot**: Zero-copy via UFFD lazy page restoration

---

## [Unreleased]

### Planned for 0.9.0 (Beta)

#### Phase 3.1: Shadow Plugin Shim

- Hook interception for `pytest_runtest_setup`/`pytest_runtest_teardown`
- Effect Capture for pytest plugin compatibility
- JSON "Effect Pack" serialization for worker replay

#### Phase 3.2: CLI Compatibility

- Drop-in replacement for `pytest` command line
- Argument passthrough and result formatting

---

[0.8.0-alpha]: https://github.com/anthropics/tach-core/releases/tag/v0.8.0-alpha
