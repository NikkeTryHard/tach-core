# Tach Performance Benchmarks

## Methodology

### Test Environment

Benchmarks should be run on:

- Clean system (no background processes)
- Warm filesystem cache (run twice, report second)
- Minimum 3 runs with median reported
- Docker container with privileged mode (required for userfaultfd)

### Benchmark Suites

| Benchmark | Description | Command |
|---|---|---|
| Cold Start | First run, empty cache | `time tach tests/` |
| Warm Start | Repeated run, cached | `time tach tests/` |
| Parallel Scaling | 1-16 workers | `tach -n {N} tests/` |
| Django ORM | 150 Django model/query/aggregation tests | `scripts/bench_django.sh` |
| Large Suite | 1000+ tests | `tach tests/benchmark/` |

## Performance Targets

Based on architecture design, expected improvements over pytest:

| Metric | pytest Baseline | Tach Target | Mechanism |
|---|---|---|---|
| Discovery | 100% | 10% | Static AST vs runtime import |
| Fork latency | ~50ms | <1ms | Zygote pre-initialization |
| Isolation | N/A | <5% overhead | Namespace + Landlock |

## Django ORM Benchmark

### Suite

150 tests exercising Django's ORM across 6 categories:

| Category | Tests | Operations |
|---|---|---|
| CRUD basics | 20 | single/bulk create, update, delete, M2M, get_or_create |
| Query & filter | 30 | FK traversal, Q objects, select/prefetch_related, subqueries |
| Aggregation | 22 | Avg/Sum/Min/Max, Count, annotate, Case/When, F-expressions |
| Isolation proof | 60 | parametrized x20, empty table assertions per test |
| Write throughput | 10 | bulk_create 100-500 rows, mixed CRUD cycles, cascade delete |
| Edge cases | 10 | empty querysets, decimal precision, unicode, max values |

### Running

```bash
# Quick comparison (3 runs, median)
./scripts/bench_django.sh

# Custom run count and worker count
BENCH_RUNS=5 BENCH_WORKERS=8 ./scripts/bench_django.sh

# Programmatic via pytest (saves baselines)
UPDATE_PERF_BASELINE=1 pytest tests/regression/perf/test_perf_regression.py::TestXdistComparison -s
```

### Results

**Environment**: Outside Docker (no userfaultfd). Python 3.12, Django 6.0.3, SQLite.

| Runner | Time (ms) | vs serial | Notes |
|---|---|---|---|
| pytest (serial) | 1719 | 1.00x | baseline |
| pytest-xdist (4 workers) | 1058 | 1.62x | process-per-worker overhead |
| tach-core (no-isolation) | 3382 | 0.50x | fallback-mode penalty (see below) |

**Why tach-core is slower outside Docker**: Without kernel features (userfaultfd, landlock, seccomp), tach-core falls back to pytest for 93% of tests (FK constraint failures during snapshot/restore). This double-execution (attempt + fallback) inflates wall time 2x. Inside Docker with `--privileged`, tach uses sub-50us memory snapshots instead of fork+import, which is where the 100x+ throughput improvement comes from.

**DJANGO_COMPAT.md reference** (Docker, full Django test suite):

| Metric | tach-core | pytest |
|---|---|---|
| Passed | 8513 | 8516 |
| Time | ~106s | ~144s |
| Speedup | 1.36x | baseline |

### Environment Requirements

Tach's speed advantage requires Linux kernel features:

- **userfaultfd**: `vm.unprivileged_userfaultfd=1` (kernel 5.13+)
- **Landlock**: ABI v4 (kernel 5.19+)
- **Seccomp**: BPF filters
- **CAP_SYS_PTRACE**: Process tracing capability

Run `tach-core self-test` to verify. Docker with `--privileged` provides all of these.

## Rust Microbenchmarks

Internal hot-path benchmarks for regression detection:

```bash
cargo bench                        # all benchmarks
cargo bench --bench hot_paths      # protocol parsing, string ops, collections
cargo bench --bench plugin_overhead # hook registry, effect serialization
```

Results stored in `target/criterion/` for historical comparison.

## Perf Regression Framework

Automated regression detection against stored baselines:

```bash
# Run regression checks
pytest tests/regression/perf/

# Update baselines after intentional changes
UPDATE_PERF_BASELINE=1 pytest tests/regression/perf/

# Skip in noisy CI
SKIP_PERF_TESTS=1 pytest tests/regression/perf/
```

Thresholds: 10% timing, 20% memory. Baselines in `tests/regression/perf/baselines/`.

## Comparison with Other Tools

See [docs/research/external-research.md](research/external-research.md#23-rust-based-python-test-runners) for competitive analysis.
