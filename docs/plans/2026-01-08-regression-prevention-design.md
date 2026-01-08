# Regression Prevention Design

> Comprehensive analysis of tach-core test suite and source code with recommendations for maximum regression prevention.

---

## Executive Summary

**Current State:**

- **635 unit tests** in Rust (`src/**/*.rs`)
- **380+ integration tests** in Rust (`rust_tests/*.rs`)
- **145 Python gauntlet tests** (`tests/**/*.py`)
- **10 fuzz targets** (`fuzz/fuzz_targets/*.rs`)
- **6 proptest suites** for property-based testing

**Key Findings:**

1. Strong coverage of core paths (isolation, scheduling, toxicity analysis)
2. Several critical gaps in edge case testing
3. Missing negative tests for security boundaries
4. Insufficient aarch64 platform coverage
5. No systematic mutation testing or coverage tracking

---

## Section 1: Test Inventory

### 1.1 Rust Unit Tests (~710 tests)

| Module                       | Tests | Quality   | Notes                        |
| ---------------------------- | ----- | --------- | ---------------------------- |
| `core/errors.rs`             | 53    | Excellent | Full error taxonomy coverage |
| `core/config.rs`             | 28    | Excellent | TOML parsing, validation     |
| `core/protocol.rs`           | 15    | Good      | IPC serialization            |
| `core/diagnostics.rs`        | 27    | Good      | Kernel feature detection     |
| `core/suggestions.rs`        | 24    | Excellent | Context-aware fix generation |
| `discovery/analysis.rs`      | 55+   | Excellent | Toxicity detection           |
| `discovery/scanner.rs`       | 46    | Excellent | AST test scanning            |
| `discovery/graph.rs`         | 21    | Good      | Dependency resolution        |
| `discovery/resolver.rs`      | 14    | Good      | Fixture resolution           |
| `execution/scheduler.rs`     | 38    | Excellent | Race condition prevention    |
| `execution/zygote.rs`        | 78    | Excellent | Fork server stability        |
| `execution/plugin_bridge.rs` | 42    | Excellent | FD teleportation             |
| `isolation/snapshot.rs`      | 50+   | Excellent | Memory snapshotting          |
| `isolation/sandbox.rs`       | 20    | Good      | Landlock/Seccomp             |
| `isolation/calibration.rs`   | 27    | Good      | TLS offset discovery         |
| `reporting/coverage.rs`      | 43    | Excellent | Ring buffer concurrency      |
| `reporting/reporter.rs`      | 33    | Excellent | Output formatting            |
| `reporting/junit.rs`         | 14    | Good      | XML generation               |

### 1.2 Rust Integration Tests (~380 tests)

| Category         | Files | Tests | Quality   |
| ---------------- | ----- | ----- | --------- |
| Sandbox/Kernel   | 2     | 35    | Excellent |
| Proptest         | 6     | 60+   | Excellent |
| CLI Flags        | 12    | 140   | Excellent |
| Core Integration | 10    | 110   | Good      |
| Process/Stress   | 5     | 40    | Good      |

### 1.3 Python Gauntlet Tests (145 files)

| Directory               | Purpose              | Quality   |
| ----------------------- | -------------------- | --------- |
| `gauntlet/`             | Security, stress     | Excellent |
| `gauntlet_011/`         | CLI compatibility    | Good      |
| `gauntlet_012/`         | Pytest parity        | Excellent |
| `gauntlet_013/`         | Diagnostics          | Good      |
| `gauntlet_014/`         | Python 3.14          | Good      |
| `gauntlet_db/`          | Database FD handover | Good      |
| `gauntlet_restoration/` | TLS/memory           | Excellent |
| `gauntlet_numpy/`       | C-extension compat   | Good      |
| `gauntlet_coverage/`    | Ring buffer          | Good      |
| `gauntlet_concurrent/`  | Worker storm         | Good      |

### 1.4 Fuzz Targets (10 fuzzers)

| Target                      | Input      | Coverage           |
| --------------------------- | ---------- | ------------------ |
| `fuzz_config_toml`          | TOML bytes | Config parsing     |
| `fuzz_protocol_deserialize` | Binary     | IPC messages       |
| `fuzz_scanner_paths`        | Path bytes | Path normalization |
| `fuzz_graph_deps`           | Structured | Dependency graph   |
| `fuzz_snapshot_tls`         | Structured | TLS arithmetic     |
| `fuzz_scheduler_events`     | Structured | State machine      |
| `fuzz_plugin_bridge`        | Structured | PyValue mapping    |
| `fuzz_logcapture_fd`        | Structured | FD operations      |
| `fuzz_coverage_entry`       | Integers   | Coverage flags     |
| `fuzz_mapping_entry`        | String     | Mapping validation |

---

## Section 2: Identified Problems

### 2.1 Critical Gaps

#### 2.1.1 Missing Network Isolation Test

**Location:** `rust_tests/sandbox_enforcement.rs`
**Problem:** No test verifies that Seccomp blocks socket operations.
**Risk:** Workers could make network calls, leaking test data.

```rust
// MISSING: Should exist but doesn't
#[test]
fn test_seccomp_blocks_socket() {
    // Fork, apply seccomp, attempt socket() -> expect EPERM
}
```

#### 2.1.2 No bincode Size Limit

**Location:** `src/core/protocol.rs:195-204`
**Problem:** `encode_with_length` uses bincode without size limits.
**Risk:** Malicious payload could claim huge Vec size, causing OOM.

```rust
// Current: No limit
let payload = bincode::serde::encode_to_vec(value, bincode::config::standard())?;

// Recommended: Add size limit
let config = bincode::config::standard().with_limit::<1024 * 1024>();
```

#### 2.1.3 ANSI Stripper Edge Cases

**Location:** `src/reporting/junit.rs:13-36`
**Problem:** `strip_ansi_codes` has weak tests for malformed sequences.
**Risk:** Infinite loop or panic on adversarial input.

```rust
// Not tested:
// - Incomplete escape: "\x1b[" (no terminator)
// - Very long escape: "\x1b[1;2;3;4;5;6;7;8;9;10m"
// - Non-CSI escapes: "\x1b]" (OSC sequences)
```

#### 2.1.4 aarch64 TLS Handling

**Location:** `src/isolation/snapshot.rs`
**Problem:** All TLS tests are `#[cfg(target_arch = "x86_64")]`. No aarch64 equivalents.
**Risk:** TLS restoration fails silently on ARM platforms.

### 2.2 Weak Test Patterns

#### 2.2.1 "Doesn't Panic" Assertions

**Location:** `src/core/diagnostics.rs`, `src/isolation/calibration.rs`
**Problem:** Several tests only verify "function doesn't panic" without checking output.

```rust
// Weak pattern found:
#[test]
fn test_kernel_version() {
    let _ = get_kernel_version(); // No assertion!
}

// Should be:
#[test]
fn test_kernel_version() {
    let version = get_kernel_version().unwrap();
    assert!(version.major >= 4, "Requires kernel 4.x+");
}
```

#### 2.2.2 Missing Error Path Tests

**Location:** `src/isolation/sandbox.rs:365-471`
**Problem:** `apply_seccomp()` error paths not tested.
**Risk:** Unknown behavior when Seccomp setup fails.

#### 2.2.3 Timeout-Dependent Tests

**Location:** `rust_tests/watch_mode_tests.rs`, `rust_tests/physics_check.rs`
**Problem:** Tests use `thread::sleep` with hardcoded durations.
**Risk:** Flaky on slow CI runners.

### 2.3 Coverage Gaps

#### 2.3.1 Untested Public APIs

| Module           | Function                        | Reason                      |
| ---------------- | ------------------------------- | --------------------------- |
| `snapshot.rs`    | `find_libpython()`              | Only tested via integration |
| `snapshot.rs`    | `parse_elf_writable_segments()` | Requires real ELF file      |
| `sandbox.rs`     | `apply_iron_dome()`             | Would sandbox test runner   |
| `calibration.rs` | `TlsCalibration::calibrate()`   | Requires real Python        |

#### 2.3.2 Missing Fuzzer Targets

| Component                 | Why Needed                                 |
| ------------------------- | ------------------------------------------ |
| `analysis.rs` AST walking | Malformed Python source could panic parser |
| `junit.rs` ANSI stripping | Escape sequences could infinite loop       |
| Path canonicalization     | Symlink attacks could bypass Landlock      |
| Python bytecode loader    | Corrupt .pyc could crash                   |

---

## Section 3: Recommendations

### 3.1 New Test Categories (Priority: Critical)

#### 3.1.1 Network Isolation Negative Test

```rust
// rust_tests/sandbox_enforcement.rs
#[test]
fn test_seccomp_blocks_socket_creation() {
    let result = run_in_sandbox(|| {
        unsafe { libc::socket(libc::AF_INET, libc::SOCK_STREAM, 0) }
    });
    assert_eq!(result, -1);
    assert_eq!(errno(), libc::EPERM);
}

#[test]
fn test_seccomp_blocks_connect() {
    // Similar for connect(), bind(), etc.
}
```

#### 3.1.2 bincode Size Limit Enforcement

```rust
// src/core/protocol.rs - Add new function
pub fn decode_with_limit<T: serde::de::DeserializeOwned>(
    data: &[u8],
    max_size: usize,
) -> Result<T, bincode::error::DecodeError> {
    let config = bincode::config::standard().with_limit::<{ max_size }>();
    bincode::serde::decode_from_slice(data, config).map(|(v, _)| v)
}

// Add test
#[test]
fn test_reject_oversized_payload() {
    // Craft payload claiming Vec of 1GB
    let malicious = /* ... */;
    assert!(decode_with_limit::<TestPayload>(&malicious, 1024 * 1024).is_err());
}
```

#### 3.1.3 ANSI Stripper Fuzz Target

```rust
// fuzz/fuzz_targets/fuzz_ansi_stripper.rs
#![no_main]
use libfuzzer_sys::fuzz_target;
use tach_core::reporting::junit::strip_ansi_codes;

fuzz_target!(|data: &str| {
    let _ = strip_ansi_codes(data);
    // Should never panic or hang
});
```

### 3.2 Test Improvements (Priority: High)

#### 3.2.1 Strengthen Weak Assertions

```rust
// Before (weak):
#[test]
fn test_sandbox_status_debug() {
    let status = SandboxStatus::FullyEnforced;
    let debug_str = format!("{:?}", status);
    assert!(debug_str.contains("FullyEnforced"));
}

// After (strong):
#[test]
fn test_sandbox_status_debug() {
    assert_eq!(format!("{:?}", SandboxStatus::FullyEnforced), "FullyEnforced");
    assert_eq!(format!("{:?}", SandboxStatus::PartiallyEnforced), "PartiallyEnforced");
    assert_eq!(format!("{:?}", SandboxStatus::NotEnforced), "NotEnforced");
}
```

#### 3.2.2 Add Error Path Coverage

```rust
// src/isolation/sandbox.rs tests
#[test]
fn test_apply_landlock_nonexistent_path() {
    let result = apply_landlock(Path::new("/nonexistent/path/12345"), 0);
    assert!(result.is_err());
}

#[test]
fn test_apply_seccomp_unsupported_arch() {
    // Mock architecture detection
    let result = apply_seccomp_for_arch("riscv64");
    assert!(result.is_err());
}
```

#### 3.2.3 Remove Timing Dependencies

```rust
// Before (flaky):
#[test]
fn test_file_watcher() {
    start_watcher();
    thread::sleep(Duration::from_millis(100));
    modify_file();
    thread::sleep(Duration::from_millis(100));
    assert!(watcher_triggered());
}

// After (stable):
#[test]
fn test_file_watcher() {
    let (tx, rx) = channel();
    start_watcher_with_callback(|_| tx.send(()).unwrap());
    modify_file();
    rx.recv_timeout(Duration::from_secs(5)).expect("Watcher should trigger");
}
```

### 3.3 New Fuzz Targets (Priority: High)

| Target                       | Purpose                          |
| ---------------------------- | -------------------------------- |
| `fuzz_toxicity_ast`          | Python source -> analysis.rs     |
| `fuzz_ansi_stripper`         | Escape sequences -> junit.rs     |
| `fuzz_path_canonicalization` | Symlinks -> config.rs            |
| `fuzz_memory_maps_parsing`   | /proc/maps format -> snapshot.rs |

### 3.4 Platform Coverage (Priority: Medium)

#### 3.4.1 aarch64 Test Stubs

```rust
#[cfg(target_arch = "aarch64")]
mod aarch64_tests {
    #[test]
    fn test_tls_capture_aarch64() {
        // TPIDR_EL0 based TLS
        todo!("Implement aarch64 TLS tests")
    }

    #[test]
    fn test_seccomp_syscall_numbers_aarch64() {
        // ARM64 syscall numbers differ from x86_64
        assert!(libc::SYS_socket > 0);
        assert!(libc::SYS_fork > 0);
    }
}
```

### 3.5 Infrastructure Improvements (Priority: Medium)

#### 3.5.1 Coverage Tracking

```toml
# .github/workflows/coverage.yml
- name: Generate coverage
  run: |
    cargo llvm-cov --lib --lcov --output-path lcov.info
    cargo llvm-cov report --fail-under-lines 80
```

#### 3.5.2 Mutation Testing

```bash
# Add to CI (nightly)
cargo mutants --package tach-core -- --lib
```

#### 3.5.3 Regression Test Baseline

```toml
# tests/regression/baselines/timing.toml
[scheduler.dispatch_latency_p99_us]
max = 500

[snapshot.restore_time_p99_us]
max = 100

[memory.rss_after_1000_tests_mb]
max = 200
```

---

## Section 4: Implementation Plan

### Phase 1: Critical Fixes (Week 1)

| Task                               | Files                    | Tests Added |
| ---------------------------------- | ------------------------ | ----------- |
| Add network isolation test         | `sandbox_enforcement.rs` | 3           |
| Add bincode size limit             | `protocol.rs`            | 2           |
| Add ANSI stripper fuzz target      | `fuzz_ansi_stripper.rs`  | 1           |
| Fix weak assertions in diagnostics | `diagnostics.rs`         | 5           |

### Phase 2: Coverage Gaps (Week 2)

| Task                          | Files                  | Tests Added |
| ----------------------------- | ---------------------- | ----------- |
| Error path tests for sandbox  | `sandbox.rs`           | 8           |
| Error path tests for snapshot | `snapshot.rs`          | 6           |
| Add `fuzz_toxicity_ast`       | `fuzz_toxicity_ast.rs` | 1           |
| Add `fuzz_memory_maps`        | `fuzz_memory_maps.rs`  | 1           |

### Phase 3: Platform & Infrastructure (Week 3)

| Task                    | Files                | Tests Added |
| ----------------------- | -------------------- | ----------- |
| aarch64 test stubs      | Multiple             | 10          |
| Coverage CI integration | `.github/workflows/` | 0           |
| Timing baseline setup   | `tests/regression/`  | 0           |
| Mutation testing setup  | `Cargo.toml`         | 0           |

### Phase 4: Advanced (Week 4+)

| Task                          | Files                    | Tests Added |
| ----------------------------- | ------------------------ | ----------- |
| Property tests for scheduler  | `proptest_scheduler.rs`  | 10          |
| Cross-kernel regression suite | `kernel_compat_tests.rs` | 15          |
| Database driver gauntlets     | `gauntlet_db/`           | 20          |
| Distro compatibility tests    | `tests/distro/`          | 10          |

---

## Section 5: Test Quality Checklist

### For Every New Test

- [ ] Tests one specific behavior
- [ ] Has descriptive name (`test_<component>_<scenario>_<expected>`)
- [ ] Uses strong assertions (not just "doesn't panic")
- [ ] Cleans up resources in Drop or teardown
- [ ] Documents why this test exists
- [ ] Runs in <1 second (or marked `#[ignore]`)

### For Integration Tests

- [ ] Tests real components (minimal mocking)
- [ ] Handles async cleanup (no orphan processes)
- [ ] Uses retry-with-timeout for async events
- [ ] Documents kernel version requirements

### For Fuzz Targets

- [ ] Targets parsing/deserialization code
- [ ] Has seed corpus checked in
- [ ] Runs in CI for at least 60 seconds
- [ ] Documents what panics/crashes it's looking for

---

## Section 6: Summary

### Current Strengths

1. Excellent unit test coverage for core logic
2. Strong property-based testing for data structures
3. Comprehensive gauntlet tests for real-world scenarios
4. Good "suicide worker" pattern for sandbox verification

### Critical Improvements Needed

1. Network isolation negative test (security)
2. bincode size limit (security)
3. ANSI stripper fuzzing (robustness)
4. aarch64 platform tests (portability)

### Estimated Effort

- **Phase 1 (Critical):** 2-3 days
- **Phase 2 (Coverage):** 3-4 days
- **Phase 3 (Infrastructure):** 2-3 days
- **Phase 4 (Advanced):** Ongoing

### Expected Outcome

- **Test count:** 635 -> ~750 unit tests
- **Coverage:** ~75% -> 85%+ line coverage
- **Platforms:** x86_64 only -> x86_64 + aarch64
- **Regression risk:** Medium -> Low
