# Test Discovery Analysis: Ignored Tests and Edge Cases

> Research document cataloguing all ignored tests and discovery edge cases in tach-core.
> Generated: 2026-01-11

---

## Executive Summary

This analysis covers:
1. **24 ignored tests** across the codebase
2. **5 categories** of ignored tests
3. **Discovery edge case coverage** in integration tests
4. **Gaps and improvement recommendations**

---

## 1. Complete List of Ignored Tests

### 1.1 Implementation Tests (`rust_tests/implementation_tests.rs`)

| Test Name | Ignore Reason | Category |
|-----------|---------------|----------|
| `test_binary_discovers_tests` | Requires sudo and built binary | Environment |
| `test_binary_runs_simple_test` | Requires sudo and built binary | Environment |
| `test_binary_handles_env_vars` | Requires sudo and built binary | Environment |
| `test_binary_reports_pass_fail_counts` | Requires sudo and built binary | Environment |
| `test_binary_creates_zygote` | Requires sudo and built binary | Environment |
| `test_binary_handles_async_tests` | Requires sudo and built binary | Environment |
| `test_binary_isolation_protects_host` | Requires sudo and built binary | Environment |
| `test_binary_handles_missing_directory` | Requires sudo and built binary | Environment |
| `test_binary_list_command_human` | Requires built binary | Environment |
| `test_binary_list_command_json` | Requires built binary | Environment |
| `test_binary_json_format_to_stdout` | Requires built binary | Environment |
| `test_binary_help_shows_watch_flag` | Requires built binary | Environment |
| `test_binary_help_shows_format_flag` | Requires built binary | Environment |
| `test_binary_help_shows_junit_xml_flag` | Requires built binary | Environment |
| `test_binary_json_discovery_has_line_numbers` | Requires sudo and built binary | Environment |

**Total: 15 tests**

### 1.2 Memory Invariant Tests (`rust_tests/memory_invariant.rs`)

| Test Name | Ignore Reason | Category |
|-----------|---------------|----------|
| `test_rss_stability_after_1000_restores` | Run manually (1000 iterations, slow) | Slow |
| `test_jitter_benchmark_p99_latency` | Run manually (benchmark) | Slow |
| `test_jitter_nanosecond_precision` | Run manually (precision benchmark) | Slow |

**Total: 3 tests**

### 1.3 Source Unit Tests (`src/`)

| File | Test Name | Ignore Reason | Category |
|------|-----------|---------------|----------|
| `src/isolation/calibration.rs` | `test_tls_calibration` | Integration test - requires Python | Environment |
| `src/execution/plugin_bridge.rs` | `test_full_fd_teleportation` | Requires forking or threading | Environment |

**Total: 2 tests**

### 1.4 Experiments (`experiments/`)

| File | Test Name | Ignore Reason | Category |
|------|-----------|---------------|----------|
| `experiments/tls_restoration_poc.rs` | `test_explore_tls_full` | Full exploration test (run explicitly) | WIP/Experimental |
| `experiments/tls_sentinel_scan.rs` | `test_calibrate_tls_offsets` | Run explicitly with --nocapture | WIP/Experimental |
| `experiments/tls_python_poc.rs` | `test_explore_python_tls` | Full exploration test | WIP/Experimental |

**Total: 3 tests**

---

## 2. Categorization Summary

| Category | Count | Description |
|----------|-------|-------------|
| **Environment** | 17 | Requires built binary, sudo, Python, or specific setup |
| **Slow** | 3 | Long-running performance/stress tests (1000+ iterations) |
| **WIP/Experimental** | 3 | Proof-of-concept or exploration tests |
| **Kernel Requirements** | 0 | No tests ignored solely for kernel requirements (handled via graceful degradation) |
| **Flaky** | 0 | No tests marked as flaky |

**Total Ignored: 24 tests**

---

## 3. Category Details

### 3.1 Environment-Dependent Tests (17 tests)

These tests require specific environment conditions:

**Built Binary Requirement (15 tests in implementation_tests.rs):**
- Tests run the `tach-core` binary as a subprocess
- CI/CD runs these separately with the binary built first
- Local development skips them by default

**Python Environment (2 tests):**
- `test_tls_calibration` - Needs Python interpreter for TLS calibration
- `test_full_fd_teleportation` - Needs forking/threading with Python

**Run command:**
```bash
cargo build --release
cargo test --test implementation_tests -- --ignored
```

### 3.2 Slow Tests (3 tests)

These are performance/stability tests that take significant time:

| Test | Purpose | Duration |
|------|---------|----------|
| `test_rss_stability_after_1000_restores` | RSS memory leak detection | ~30-60s |
| `test_jitter_benchmark_p99_latency` | P99 latency measurement | ~10-30s |
| `test_jitter_nanosecond_precision` | Nanosecond precision verification | ~10-30s |

**Run command:**
```bash
cargo test --test memory_invariant -- --ignored --nocapture
```

### 3.3 WIP/Experimental Tests (3 tests)

Located in `experiments/` directory with `required-features = ["experiments"]`:

- Used for TLS (Thread-Local Storage) exploration
- Not part of normal test suite
- Run explicitly for research purposes

**Run command:**
```bash
cargo test --features experiments -- --ignored --nocapture
```

---

## 4. Discovery Edge Cases

### 4.1 Currently Tested Edge Cases

Based on analysis of `rust_tests/discovery_integration.rs` and `src/discovery/scanner.rs`:

| Edge Case | Tested | Location |
|-----------|--------|----------|
| Empty directory | Yes | `discovery_integration.rs::test_discover_empty_temp_directory` |
| Non-test files ignored | Yes | `discovery_integration.rs::test_discover_ignores_non_test_files` |
| Async test detection | Yes | `discovery_integration.rs::test_discover_finds_specific_test_files` |
| Class-based tests | Yes | `discovery_integration.rs::test_discover_finds_specific_test_files` |
| Fixture scope parsing | Yes | `discovery_integration.rs::test_discover_fixture_scopes` |
| Decorated test functions | Yes | `scanner.rs::tests::test_parse_decorated_test_functions` |
| `@pytest.mark.parametrize` | Yes | `scanner.rs::tests::test_parse_decorated_test_in_class` |
| `@pytest.mark.timeout` | Yes | Multiple tests in `scanner.rs::tests` |
| Nested class methods | Yes | `scanner.rs::tests::test_parse_test_class` |
| Self/cls exclusion | Yes | `scanner.rs::tests::test_parse_self_and_cls_excluded_from_deps` |
| Empty Python file | Yes | `scanner.rs::tests::test_parse_empty_file` |
| Bare fixture decorator | Yes | `scanner.rs::tests::test_parse_bare_fixture_decorator` |
| Symlink handling | Yes | `scanner.rs::tests::test_symlink_path_canonicalization_concept` |
| Symlink cycle protection | Conceptual | `scanner.rs::tests::test_symlink_cycle_protection_concept` |

### 4.2 PropTest Coverage (`rust_tests/proptest_discovery.rs`)

Property-based tests verify invariants with randomized inputs:

| Property | Cases | Description |
|----------|-------|-------------|
| DAG fixtures acyclic | 200 | Dependency graphs have no cycles |
| Topological sort succeeds | 200 | Sorting DAGs always works |
| Self-dependency detected | - | Circular dependencies caught |
| Module paths unique | 300 | No path collisions |
| Path parent shorter | 300 | Parent path length invariant |
| Import statements parseable | 300 | Generated imports are valid |
| Scope priorities distinct | 100 | All fixture scopes have unique priority |
| Test prefix recognized | 200 | `test_*` pattern matching |
| Class::method split | - | Nested name parsing |

---

## 5. Discovery Edge Cases NOT Tested

### 5.1 Potential Gaps

| Edge Case | Risk | Recommendation |
|-----------|------|----------------|
| Unicode in test names | Low | Add test for unicode function names |
| Very long test names | Low | Add property test for name length limits |
| Nested TestClass in TestClass | Medium | Verify nested class behavior |
| `pytest.importorskip` | Medium | Document limitation or add detection |
| Dynamic test generation (`pytest_generate_tests`) | High | Cannot discover - document limitation |
| `pytest.mark.usefixtures` class decorator | Medium | Add test for class-level fixture injection |
| Multiple `@pytest.fixture` on same function | Low | Verify behavior |
| Fixture with yield (generator fixtures) | Low | Verify detection works |
| Autouse fixtures | Medium | Add test for autouse detection |
| `@pytest.mark.skip`/`xfail` | Low | Verify tests are still discovered |
| conftest.py in subdirectories | Medium | Add hierarchical conftest test |
| Windows path separators | N/A | Linux-only project |

### 5.2 Loader Edge Cases (`rust_tests/loader_integration.rs`)

The loader integration tests cover:
- Header stripping (16-byte .pyc header)
- Registry module lookup
- Batch compilation with failures
- Package `__init__.py` handling
- Cache persistence and mtime invalidation
- Nested module path resolution
- Empty file compilation

---

## 6. Recommendations

### 6.1 Short-term (Low Effort)

1. **Document ignored tests in CLAUDE.md** - Add section explaining when to run ignored tests
2. **Add CI job for ignored tests** - Run with `--ignored` flag in separate CI step
3. **Label categories** - Update `#[ignore]` comments to include category

### 6.2 Medium-term (Moderate Effort)

1. **Add missing discovery edge case tests:**
   - Nested TestClass behavior
   - `autouse=True` fixture detection
   - Hierarchical conftest.py resolution
   - `@pytest.mark.usefixtures` class decorator

2. **Create integration test for symlinks:**
   - Actual symlink creation (not just conceptual)
   - Symlink cycle detection verification

3. **Add fuzz target for discovery:**
   - Fuzz the Python AST parser with malformed inputs
   - Target: `fuzz_scanner_paths.rs` or new target

### 6.3 Long-term (High Effort)

1. **Dynamic test generation detection:**
   - Detect `pytest_generate_tests` hooks
   - Warn user that dynamic tests cannot be discovered statically

2. **Performance regression tests for discovery:**
   - Benchmark discovery on large codebases
   - Add to `tests/regression/perf/`

3. **Golden tests for discovery output:**
   - Snapshot test discovery JSON output
   - Detect unintended changes to discovery behavior

---

## 7. Running Ignored Tests

### All Ignored Tests
```bash
cargo test -- --ignored
```

### By Category

**Implementation tests (requires binary):**
```bash
cargo build --release
cargo test --test implementation_tests -- --ignored
```

**Memory invariant tests (slow):**
```bash
cargo test --test memory_invariant -- --ignored --nocapture
```

**Experiments (requires feature):**
```bash
cargo test --features experiments -- --ignored --nocapture
```

**Inside Docker container:**
```bash
docker compose exec dev bash -c 'source .venv/bin/activate && cargo test -- --ignored'
```

---

## 8. Test File Reference

| File | Purpose | Ignored Count |
|------|---------|---------------|
| `rust_tests/implementation_tests.rs` | Binary CLI tests | 15 |
| `rust_tests/memory_invariant.rs` | Memory stability tests | 3 |
| `rust_tests/discovery_integration.rs` | Discovery API tests | 0 |
| `rust_tests/proptest_discovery.rs` | Property-based discovery tests | 0 |
| `rust_tests/sandbox_enforcement.rs` | Kernel sandbox tests | 0 |
| `rust_tests/kernel_compat_tests.rs` | Graceful degradation tests | 0 |
| `src/isolation/calibration.rs` | TLS calibration unit tests | 1 |
| `src/execution/plugin_bridge.rs` | FD teleportation tests | 1 |
| `experiments/tls_*.rs` | TLS exploration experiments | 3 |

---

## 9. Conclusion

The tach-core test suite has a well-organized approach to ignored tests:

1. **Environment-dependent tests** are clearly marked and can be run in CI with proper setup
2. **Slow tests** are separated to avoid impacting developer iteration speed
3. **Experimental tests** are gated behind feature flags
4. **No flaky tests** - all ignored tests have legitimate reasons

The discovery system has good test coverage, with property-based tests providing strong confidence in core invariants. The main gaps are around advanced pytest features like dynamic test generation and autouse fixtures, which are documented limitations of static AST analysis.
