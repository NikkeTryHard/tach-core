//! Hot-path microbenchmarks for regression detection.
//!
//! These benchmarks target performance-critical paths in tach-core.
//! Run with: `cargo bench`
//!
//! Results are stored in target/criterion/ for historical comparison.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::hint::black_box as hint_black_box;

/// Baseline benchmark to verify criterion is working correctly.
/// This provides a reference point for timing measurements.
fn bench_baseline(c: &mut Criterion) {
    let mut group = c.benchmark_group("baseline");

    group.bench_function("noop", |b| b.iter(|| hint_black_box(42)));

    group.bench_function("sum_1000", |b| {
        b.iter(|| {
            let sum: u64 = (0..1000).sum();
            black_box(sum)
        })
    });

    group.finish();
}

/// Protocol message parsing benchmarks.
/// Tests the speed of deserializing worker-to-supervisor messages.
fn bench_protocol_parsing(c: &mut Criterion) {
    let mut group = c.benchmark_group("protocol");

    // Simulate JSON-like parsing workload
    let small_payload =
        r#"{"event":"test_finish","name":"test_a","outcome":"passed","duration_ms":1.5}"#;
    let medium_payload = r#"{"event":"test_finish","name":"test_module::TestClass::test_long_name_here","outcome":"failed","duration_ms":125.7,"message":"AssertionError: expected True but got False\n  File \"test.py\", line 42"}"#;

    group.throughput(Throughput::Bytes(small_payload.len() as u64));
    group.bench_with_input(
        BenchmarkId::new("json_parse", "small"),
        &small_payload,
        |b, payload| {
            b.iter(|| {
                // Simulate parsing overhead
                let parsed: Result<serde_json::Value, _> = serde_json::from_str(black_box(payload));
                black_box(parsed)
            })
        },
    );

    group.throughput(Throughput::Bytes(medium_payload.len() as u64));
    group.bench_with_input(
        BenchmarkId::new("json_parse", "medium"),
        &medium_payload,
        |b, payload| {
            b.iter(|| {
                let parsed: Result<serde_json::Value, _> = serde_json::from_str(black_box(payload));
                black_box(parsed)
            })
        },
    );

    group.finish();
}

/// String processing benchmarks for test name handling.
/// Tests operations like path normalization and test ID formatting.
fn bench_string_processing(c: &mut Criterion) {
    let mut group = c.benchmark_group("strings");

    let test_paths = vec![
        "tests/unit/test_simple.py::test_function",
        "tests/integration/test_database.py::TestDatabase::test_connection",
        "tests/e2e/very/deep/nested/path/test_complex.py::TestSuite::test_with_params[param1-param2]",
    ];

    group.bench_function("test_id_split", |b| {
        b.iter(|| {
            for path in &test_paths {
                let parts: Vec<&str> = black_box(path).split("::").collect();
                black_box(parts);
            }
        })
    });

    group.bench_function("path_normalize", |b| {
        b.iter(|| {
            for path in &test_paths {
                let normalized = black_box(path).replace('\\', "/").to_lowercase();
                black_box(normalized);
            }
        })
    });

    // Test ID truncation (used in progress display)
    group.bench_function("test_id_truncate", |b| {
        let long_id = "tests/integration/database/postgres/test_connection_pool.py::TestConnectionPool::test_acquire_release_cycle_with_timeout";
        let max_width = 60;

        b.iter(|| {
            let id = black_box(long_id);
            let truncated = if id.len() > max_width { format!("...{}", &id[id.len() - max_width + 3..]) } else { id.to_string() };
            black_box(truncated)
        })
    });

    group.finish();
}

/// Collection operations that occur in the scheduler.
/// Tests hash map and vec operations at scale.
fn bench_collections(c: &mut Criterion) {
    let mut group = c.benchmark_group("collections");

    // Simulate test result storage
    group.bench_function("hashmap_insert_100", |b| {
        b.iter(|| {
            let mut map = std::collections::HashMap::new();
            for i in 0..100 {
                map.insert(format!("test_{}", i), i);
            }
            black_box(map)
        })
    });

    group.bench_function("vec_sort_100", |b| {
        let mut data: Vec<u64> = (0..100).rev().collect();
        b.iter(|| {
            data.sort_unstable();
            black_box(&data);
            // Reset for next iteration
            data.reverse();
        })
    });

    // DashMap concurrent access (used in scheduler)
    group.bench_function("dashmap_insert_100", |b| {
        b.iter(|| {
            let map = dashmap::DashMap::new();
            for i in 0..100 {
                map.insert(format!("test_{}", i), i);
            }
            black_box(map)
        })
    });

    group.finish();
}

/// File path matching benchmarks.
/// Tests glob pattern matching used in test discovery.
fn bench_path_matching(c: &mut Criterion) {
    let mut group = c.benchmark_group("path_matching");

    let paths = vec![
        "tests/test_simple.py",
        "tests/unit/test_auth.py",
        "tests/integration/test_db.py",
        "src/main.py",
        "conftest.py",
        "tests/conftest.py",
        "tests/unit/conftest.py",
    ];

    group.bench_function("prefix_match", |b| {
        b.iter(|| {
            let mut matched = Vec::new();
            for path in &paths {
                if black_box(path).starts_with("tests/") {
                    matched.push(*path);
                }
            }
            black_box(matched)
        })
    });

    group.bench_function("suffix_match", |b| {
        b.iter(|| {
            let mut matched = Vec::new();
            for path in &paths {
                if black_box(path).ends_with("_test.py") || black_box(path).starts_with("test_") {
                    matched.push(*path);
                }
            }
            black_box(matched)
        })
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_baseline,
    bench_protocol_parsing,
    bench_string_processing,
    bench_collections,
    bench_path_matching,
);

criterion_main!(benches);
