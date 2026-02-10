//! Plugin overhead benchmarks for measuring hook/effect performance.
//!
//! These benchmarks target the plugin system's performance-critical paths:
//! - Registry lookups
//! - Effect serialization/deserialization
//! - Batch effect replay
//!
//! Run with: `cargo bench --bench plugin_overhead`

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use std::hint::black_box;
use std::path::PathBuf;

use tach_core::hooks::{Hook, HookEffect, HookRegistry, HookSpec, LoopScope, SysPathAction};

/// Create sample effects for benchmarking
fn sample_effects() -> Vec<HookEffect> {
    vec![
        HookEffect::SetEnv {
            key: "DJANGO_SETTINGS_MODULE".to_string(),
            value: "myproject.settings.test".to_string(),
        },
        HookEffect::ModifySysPath {
            action: SysPathAction::Prepend,
            path: "/home/user/project/src".to_string(),
        },
        HookEffect::ModifySysPath {
            action: SysPathAction::Append,
            path: "/home/user/project/vendor".to_string(),
        },
        HookEffect::DjangoDbSetup {
            transaction: true,
            reset_sequences: false,
            databases: vec!["default".to_string(), "replica".to_string()],
        },
        HookEffect::AsyncioSetup {
            loop_scope: LoopScope::Module,
            auto_mode: true,
        },
        HookEffect::RegisterMarker {
            name: "slow".to_string(),
            description: "Mark test as slow running".to_string(),
        },
        HookEffect::ModifyItems {
            removed: vec!["test_skip_me".to_string(), "test_also_skip".to_string()],
            reordered: true,
        },
        HookEffect::NoEffect,
    ]
}

/// Create a populated registry for benchmarking
fn populated_registry(hook_count: usize) -> HookRegistry {
    let mut registry = HookRegistry::new();

    let hook_names = [
        "pytest_configure",
        "pytest_sessionstart",
        "pytest_collection_modifyitems",
        "pytest_runtest_setup",
        "pytest_runtest_call",
        "pytest_runtest_teardown",
        "pytest_runtest_makereport",
        "pytest_sessionfinish",
    ];

    for i in 0..hook_count {
        let hook_name = hook_names[i % hook_names.len()];
        let hook = Hook {
            spec: HookSpec {
                name: hook_name.to_string(),
                modifies_global_state: hook_name == "pytest_configure",
                cacheable: true,
            },
            source: PathBuf::from(format!("tests/level{}/conftest.py", i % 5)),
            function_name: hook_name.to_string(),
            line_number: 10 + i,
            is_wrapper: i % 3 == 0,
        };
        registry.register(hook);
    }

    registry
}

/// Benchmark registry lookup operations
fn bench_registry_lookup(c: &mut Criterion) {
    let mut group = c.benchmark_group("registry_lookup");

    for size in [10, 50, 100, 500] {
        let registry = populated_registry(size);

        group.bench_with_input(BenchmarkId::new("get_hooks", size), &registry, |b, reg| {
            b.iter(|| {
                let hooks = reg.get_hooks(black_box("pytest_configure"));
                black_box(hooks)
            })
        });

        group.bench_with_input(
            BenchmarkId::new("has_global_state_hooks", size),
            &registry,
            |b, reg| {
                b.iter(|| {
                    let result = reg.has_global_state_hooks();
                    black_box(result)
                })
            },
        );

        group.bench_with_input(BenchmarkId::new("hook_count", size), &registry, |b, reg| {
            b.iter(|| {
                let count = reg.hook_count();
                black_box(count)
            })
        });

        group.bench_with_input(
            BenchmarkId::new("get_hooks_for_file", size),
            &registry,
            |b, reg| {
                let path = PathBuf::from("tests/level2/conftest.py");
                b.iter(|| {
                    let hooks = reg.get_hooks_for_file(black_box(&path));
                    black_box(hooks)
                })
            },
        );
    }

    group.finish();
}

/// Benchmark effect serialization (JSON)
fn bench_effect_serialization(c: &mut Criterion) {
    let mut group = c.benchmark_group("effect_serialization");

    // Individual effect variants
    let set_env = HookEffect::SetEnv {
        key: "PYTHONPATH".to_string(),
        value: "/home/user/project/src:/home/user/project/lib".to_string(),
    };

    let modify_sys_path = HookEffect::ModifySysPath {
        action: SysPathAction::Prepend,
        path: "/home/user/project/custom_modules".to_string(),
    };

    let django_db = HookEffect::DjangoDbSetup {
        transaction: true,
        reset_sequences: true,
        databases: vec![
            "default".to_string(),
            "analytics".to_string(),
            "cache".to_string(),
        ],
    };

    let asyncio = HookEffect::AsyncioSetup {
        loop_scope: LoopScope::Session,
        auto_mode: true,
    };

    // Benchmark individual serialization
    group.bench_function("serialize_set_env", |b| {
        b.iter(|| {
            let json = serde_json::to_string(black_box(&set_env)).unwrap();
            black_box(json)
        })
    });

    group.bench_function("serialize_modify_sys_path", |b| {
        b.iter(|| {
            let json = serde_json::to_string(black_box(&modify_sys_path)).unwrap();
            black_box(json)
        })
    });

    group.bench_function("serialize_django_db", |b| {
        b.iter(|| {
            let json = serde_json::to_string(black_box(&django_db)).unwrap();
            black_box(json)
        })
    });

    group.bench_function("serialize_asyncio", |b| {
        b.iter(|| {
            let json = serde_json::to_string(black_box(&asyncio)).unwrap();
            black_box(json)
        })
    });

    // Batch serialization
    let effects = sample_effects();
    group.throughput(Throughput::Elements(effects.len() as u64));

    group.bench_function("serialize_batch", |b| {
        b.iter(|| {
            let json = serde_json::to_string(black_box(&effects)).unwrap();
            black_box(json)
        })
    });

    group.finish();
}

/// Benchmark effect deserialization (JSON)
fn bench_effect_deserialization(c: &mut Criterion) {
    let mut group = c.benchmark_group("effect_deserialization");

    // Pre-serialize effects for deserialization benchmarks
    let set_env_json = serde_json::to_string(&HookEffect::SetEnv {
        key: "PYTHONPATH".to_string(),
        value: "/home/user/project/src:/home/user/project/lib".to_string(),
    })
    .unwrap();

    let modify_sys_path_json = serde_json::to_string(&HookEffect::ModifySysPath {
        action: SysPathAction::Prepend,
        path: "/home/user/project/custom_modules".to_string(),
    })
    .unwrap();

    let django_db_json = serde_json::to_string(&HookEffect::DjangoDbSetup {
        transaction: true,
        reset_sequences: true,
        databases: vec![
            "default".to_string(),
            "analytics".to_string(),
            "cache".to_string(),
        ],
    })
    .unwrap();

    let asyncio_json = serde_json::to_string(&HookEffect::AsyncioSetup {
        loop_scope: LoopScope::Session,
        auto_mode: true,
    })
    .unwrap();

    let batch_json = serde_json::to_string(&sample_effects()).unwrap();

    // Benchmark individual deserialization
    group.bench_function("deserialize_set_env", |b| {
        b.iter(|| {
            let effect: HookEffect = serde_json::from_str(black_box(&set_env_json)).unwrap();
            black_box(effect)
        })
    });

    group.bench_function("deserialize_modify_sys_path", |b| {
        b.iter(|| {
            let effect: HookEffect =
                serde_json::from_str(black_box(&modify_sys_path_json)).unwrap();
            black_box(effect)
        })
    });

    group.bench_function("deserialize_django_db", |b| {
        b.iter(|| {
            let effect: HookEffect = serde_json::from_str(black_box(&django_db_json)).unwrap();
            black_box(effect)
        })
    });

    group.bench_function("deserialize_asyncio", |b| {
        b.iter(|| {
            let effect: HookEffect = serde_json::from_str(black_box(&asyncio_json)).unwrap();
            black_box(effect)
        })
    });

    // Batch deserialization
    group.throughput(Throughput::Elements(sample_effects().len() as u64));

    group.bench_function("deserialize_batch", |b| {
        b.iter(|| {
            let effects: Vec<HookEffect> = serde_json::from_str(black_box(&batch_json)).unwrap();
            black_box(effects)
        })
    });

    group.finish();
}

/// Benchmark batch effect replay simulation
fn bench_effect_replay(c: &mut Criterion) {
    let mut group = c.benchmark_group("effect_replay");

    // Create registry with recorded effects
    let mut registry = populated_registry(50);

    // Record session effects
    for effect in sample_effects() {
        registry.record_effect("pytest_configure", effect);
    }

    // Benchmark getting session effects (what workers do on startup)
    group.bench_function("get_session_effects", |b| {
        b.iter(|| {
            let effects = registry.get_session_effects();
            black_box(effects)
        })
    });

    // Benchmark effect matching/filtering (simulates replay logic)
    let effects = sample_effects();

    group.bench_function("filter_env_effects", |b| {
        b.iter(|| {
            let env_effects: Vec<_> = effects
                .iter()
                .filter(|e| matches!(e, HookEffect::SetEnv { .. }))
                .collect();
            black_box(env_effects)
        })
    });

    group.bench_function("filter_path_effects", |b| {
        b.iter(|| {
            let path_effects: Vec<_> = effects
                .iter()
                .filter(|e| matches!(e, HookEffect::ModifySysPath { .. }))
                .collect();
            black_box(path_effects)
        })
    });

    // Simulate full replay: deserialize + filter + apply
    let batch_json = serde_json::to_string(&sample_effects()).unwrap();

    group.bench_function("full_replay_simulation", |b| {
        b.iter(|| {
            // 1. Deserialize effects from IPC
            let effects: Vec<HookEffect> = serde_json::from_str(black_box(&batch_json)).unwrap();

            // 2. Categorize effects by type
            let mut env_vars = Vec::new();
            let mut sys_paths = Vec::new();
            let mut other = Vec::new();

            for effect in effects {
                match &effect {
                    HookEffect::SetEnv { .. } => env_vars.push(effect),
                    HookEffect::ModifySysPath { .. } => sys_paths.push(effect),
                    _ => other.push(effect),
                }
            }

            // 3. Return categorized effects (actual application would happen in Python)
            black_box((env_vars, sys_paths, other))
        })
    });

    // Benchmark effect cloning (common during distribution to workers)
    group.bench_function("clone_effects_batch", |b| {
        let effects = sample_effects();
        b.iter(|| {
            let cloned: Vec<HookEffect> = effects.to_vec();
            black_box(cloned)
        })
    });

    group.finish();
}

/// Benchmark round-trip serialization (common IPC pattern)
fn bench_roundtrip(c: &mut Criterion) {
    let mut group = c.benchmark_group("roundtrip");

    for batch_size in [1, 5, 10, 50] {
        let effects: Vec<HookEffect> = sample_effects()
            .into_iter()
            .cycle()
            .take(batch_size)
            .collect();

        group.throughput(Throughput::Elements(batch_size as u64));

        group.bench_with_input(
            BenchmarkId::new("serialize_deserialize", batch_size),
            &effects,
            |b, effects| {
                b.iter(|| {
                    let json = serde_json::to_string(black_box(effects)).unwrap();
                    let parsed: Vec<HookEffect> = serde_json::from_str(&json).unwrap();
                    black_box(parsed)
                })
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_registry_lookup,
    bench_effect_serialization,
    bench_effect_deserialization,
    bench_effect_replay,
    bench_roundtrip,
);

criterion_main!(benches);
