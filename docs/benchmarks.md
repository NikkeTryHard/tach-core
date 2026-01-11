# Tach Performance Benchmarks

> **Status:** Benchmark framework established. Results pending systematic collection.

## Methodology

### Test Environment

Benchmarks should be run on:

- Clean system (no background processes)
- Warm filesystem cache (run twice, report second)
- Minimum 3 runs with median reported

### Benchmark Suite

| Benchmark        | Description            | Command                 |
| ---------------- | ---------------------- | ----------------------- |
| Cold Start       | First run, empty cache | `time tach tests/`      |
| Warm Start       | Repeated run, cached   | `time tach tests/`      |
| Parallel Scaling | 1-16 workers           | `tach -n {N} tests/`    |
| Large Suite      | 1000+ tests            | `tach tests/benchmark/` |

## Performance Targets

Based on architecture design, expected improvements over pytest:

| Metric       | pytest Baseline | Tach Target  | Mechanism                    |
| ------------ | --------------- | ------------ | ---------------------------- |
| Discovery    | 100%            | 10%          | Static AST vs runtime import |
| Fork latency | ~50ms           | <1ms         | Zygote pre-initialization    |
| Isolation    | N/A             | <5% overhead | Namespace + Landlock         |

## Collected Results

_Benchmark results will be added after systematic collection across representative test suites._

### How to Contribute Benchmarks

1. Run: `./scripts/benchmark.sh` (when available)
2. Include system specs in results
3. Submit via PR to `docs/benchmarks.md`

## Comparison with Other Tools

See [docs/research/external-research.md](research/external-research.md#23-rust-based-python-test-runners) for competitive analysis.
