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

## [0.9.0-beta] - 2026-01-03

### Beta Release: pytest Compatibility Layer

This release introduces the **pytest Compatibility Layer** - making tach a drop-in replacement for pytest with identical command-line arguments.

### Highlights

- **pytest-Compatible CLI**: `-n`, `-x`, `-v`, `-q`, `-k`, `-m` flags work as expected
- **Nanosecond Jitter Audit**: P99.9 at 63ns over 10K cycles (sub-microsecond precision)
- **FD Teleporter Complete**: Physical FD handover via SCM_RIGHTS + dup2

### Added

#### Phase 0.9.0: CLI Compatibility Layer

- pytest-xdist compatible `-n WORKERS` (auto-detect CPU count)
- Test selection: `-k EXPRESSION` for keyword filtering
- Marker filtering: `-m MARKERS` for pytest markers
- Fail-fast: `-x` / `--exitfirst` and `--maxfail=N`
- Verbosity: `-v`, `-vv`, `-q` (quiet mode)
- Coverage: `--coverage`, `--cov PATH` (pytest-cov compatible)
- Passthrough: `-- PYTEST_ARGS` for unknown arguments
- New subcommands: `self-test`, `version`

#### Phase 0.8.9.5: High-Resolution Jitter (Nanosecond Precision)

- `test_jitter_nanosecond_precision` - 10K cycle benchmark with ns timing
- `test_jitter_nanosecond_quick` - CI-friendly 1K cycle variant
- `format_duration_ns()` - Nanosecond formatting (ns/us/ms/s)
- `generate_histogram_ns()` - ASCII histogram with nanosecond labels
- Compiler fences for measurement noise reduction

#### Phase 0.8.9: FD Adoption (The Physical Handover)

- `FdTeleportRequest` struct for batch FD transfers
- `FdAdoptionResult` struct for adoption feedback
- `send_fds()` - Supervisor-side SCM_RIGHTS transmission
- `receive_and_adopt_fds()` - Worker-side recvmsg + dup2
- `forget_sent_fd()` - Ghost Close prevention via `mem::forget()`
- `create_teleporter_socket_pair()` - Socket pair factory
- Unit tests for all FD teleportation functions

### Changed

- CLI struct reorganized with pytest-compatible groupings
- `Verbosity` enum added for `-v`/`-q` handling
- Help text includes examples and environment variables
- Version now uses `env!("CARGO_PKG_VERSION")` for consistency

### Performance (Nanosecond Jitter Report)

```
Percentile Distribution:
  Min:     56ns
  P50:     57ns
  P90:     58ns
  P95:     58ns
  P99:     59ns
  P99.9:   63ns
  Max:     143ns

Throughput:
  Operations:  17,426,248 ops/sec
  Data rate:   204,213 MB/sec
  Mean per-op: 57.00 ns/op
```

---

## [0.8.5-alpha] - 2026-01-03

### Vital Types Registry & Jitter Audit

This release adds the Vital Types Registry for detecting non-serializable fixtures (sockets, DB connections) and the Jitter Benchmark for measuring restoration latency.

### Added

#### Vital Types Registry (Fidelity Gap Protection)

- `VitalTypeCategory` enum: FILE_DESCRIPTOR, DATABASE, NETWORK, LOCK, RESOURCE
- `VitalTypeInfo` dataclass for tracking vital type metadata
- `_VITAL_TYPES_REGISTRY` with 15+ common vital types (socket, sqlite3, psycopg2, etc.)
- `_check_vital_type()` - Heuristic detection for types with `fileno()` or `close()`
- `_emit_vital_type_warning()` - CRITICAL log + RuntimeWarning emission
- CRITICAL warnings when vital types are degraded via repr()

#### FileDescriptorEffect (SCM_RIGHTS FD Teleporter)

- `FileDescriptorEffect` dataclass for FD handover via SCM_RIGHTS
- `FileDescriptorEffect.from_value()` - Factory for extracting FDs from fixtures
- Socket/file metadata extraction (local_addr, remote_addr, path, mode)
- Integration with `EffectPack` serialization

#### Jitter Benchmark

- `test_jitter_benchmark_p99_latency` - 10K cycle P99 histogram (ignored by default)
- `test_jitter_quick` - CI-friendly 1K cycle variant
- `percentile()` helper for P50/P90/P95/P99/P99.9 calculation
- `generate_histogram()` - ASCII histogram rendering
- Throughput metrics: ops/sec, MB/sec data rate

#### DTV Counter Verification

- `test_dtv_consistency_after_memory_ops` - TLS/DTV state verification
- `test_dtv_stress_with_python_imports` - Module import stress test
- `read_dtv_generation()` - Inline assembly for fs_base/DTV access
- Python TLS verification via `threading.local()`

### Changed

- `_try_serialize()` now returns 3-tuple: `(value, reason, is_vital)`
- `MarkerEffect` includes `has_vital_types` field
- `tach/__init__.py` exports all key types with `__all__`
- Version bumped to 0.8.5-alpha

### Performance

- **Jitter (baseline memcpy)**: P99 = 0us, Max = 10us over 10K iterations
- **Throughput**: 588M ops/sec for 12KB TLS restoration (baseline)
- **DTV Consistency**: TLS value unchanged after memory operations

---

[0.8.0-alpha]: https://github.com/anthropics/tach-core/releases/tag/v0.8.0-alpha
[0.8.5-alpha]: https://github.com/anthropics/tach-core/releases/tag/v0.8.5-alpha
[0.9.0-beta]: https://github.com/anthropics/tach-core/releases/tag/v0.9.0-beta
