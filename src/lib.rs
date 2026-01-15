//! Tach Core Library
//!
//! This library exposes the core modules for integration testing.
//! The binary entry point is in main.rs.

// Lint configuration for code quality
#![warn(unused_imports)]
#![warn(unused_variables)]
#![warn(dead_code)]
#![warn(unused_mut)]

// =============================================================================
//  Jemalloc Global Allocator
// =============================================================================
//
// CRITICAL: This MUST be at the top of lib.rs before any allocations occur.
//
// Why Jemalloc?
// -------------
// The "Split-Brain" problem: glibc's malloc uses thread-local caches (tcache)
// and pointer mangling that create non-deterministic heap state. When we
// snapshot memory with userfaultfd and restore it later, the allocator's
// internal metadata can become desynchronized, causing:
//   - Use-after-free when tcache points to freed memory
//   - Double-free when arena metadata is stale
//   - Reference count corruption for Python's small_ints cache
//
// Jemalloc solves this by providing:
//   1. mallctl("thread.tcache.flush") - Explicit tcache flush before snapshot
//   2. mallctl("epoch") - Force metadata synchronization
//   3. Deterministic arena layout without pointer mangling
//
// Configuration: background_thread:false,dirty_decay_ms:0,muzzy_decay_ms:0
//   - background_thread:false - We control memory explicitly via quiesce
//   - dirty_decay_ms:0 - Immediate purge for deterministic state
//   - muzzy_decay_ms:0 - Immediate purge for deterministic state
//
// The quiesce_allocator() function in src/allocator.rs MUST be called before
// SIGSTOP to ensure the heap is in a consistent, snapshot-able state.
//
// WSL2 Compatibility Note:
// ------------------------
// Jemalloc causes WSL2 kernel instability when running the full test suite.
// To work around this, jemalloc is DISABLED during `cargo test` and only
// enabled for the production binary (`cargo build`/`cargo run`).
//
// The allocator tests will gracefully skip when jemalloc isn't active.
// To run allocator tests with jemalloc on a stable Linux system, use:
//   cargo test --lib allocator -- --ignored
//
// For production, set MALLOC_CONF before running the binary:
//   MALLOC_CONF="background_thread:false,dirty_decay_ms:0,muzzy_decay_ms:0" \
//   ./target/release/tach-core
// =============================================================================

// Jemalloc is only enabled for non-test builds to avoid WSL2 instability.
// During tests, the system allocator is used instead.
#[cfg(all(not(target_env = "msvc"), not(test)))]
#[global_allocator]
static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

pub mod core;
pub mod discovery;
pub mod execution;
pub mod hooks;
pub mod isolation;
pub mod reporting;

// Re-export core modules at top level for backward compatibility
pub use core::allocator;
pub use core::config;
pub use core::diagnostics;
pub use core::environment;
pub use core::errors;
pub use core::lifecycle;
pub use core::protocol;
pub use core::signals;
pub use core::suggestions;

// Re-export discovery modules at top level for backward compatibility
pub use discovery::analysis;
pub use discovery::graph;
pub use discovery::loader;
pub use discovery::resolver;
pub use discovery::scanner;

// Re-export isolation modules at top level for backward compatibility
pub use isolation::namespace;
pub use isolation::sandbox;
pub use isolation::snapshot;

// Re-export reporting modules at top level for backward compatibility
pub use reporting::coverage;
pub use reporting::debugger;
pub use reporting::junit;
pub use reporting::logcapture;
pub use reporting::reporter;

// Re-export execution modules at top level for backward compatibility
pub use execution::plugin_bridge;
pub use execution::scheduler;
pub use execution::watch;
pub use execution::zygote;

// =============================================================================
//  Toxicity Integration
// =============================================================================

use anyhow::Result;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use crate::discovery::DiscoveryResult;
use crate::graph::ToxicityGraph;

/// Collect all Python files in a directory (excluding hidden dirs, __pycache__, etc.)
fn collect_all_py_files(root: &Path) -> Vec<PathBuf> {
    WalkDir::new(root)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            let path = e.path();
            // Include only .py files
            path.extension().is_some_and(|ext| ext == "py")
                // Exclude hidden directories, __pycache__, .git, etc.
                && !path.ancestors().any(|p| {
                    p.file_name().is_some_and(|name| {
                        let n = name.to_string_lossy();
                        n.starts_with('.') || n == "__pycache__" || n == "target" || n == "node_modules"
                    })
                })
        })
        .map(|e| e.path().to_path_buf())
        .collect()
}

/// Discover tests with toxicity analysis.
///
/// This function combines test discovery with toxicity graph construction,
/// providing a single entry point for the toxicity pipeline.
///
/// # Arguments
/// * `root` - The project root directory to scan for tests
///
/// # Returns
/// A tuple of (DiscoveryResult, ToxicityGraph) where:
/// - DiscoveryResult contains all discovered test modules and fixtures
/// - ToxicityGraph contains the toxicity analysis for ALL Python modules
///   (not just test files, to enable transitive toxicity propagation)
///
/// # Example
/// ```ignore
/// let (discovery, toxicity) = discover_with_toxicity(&project_root)?;
/// for module in &discovery.modules {
///     let is_toxic = toxicity.is_toxic(&module.path);
///     println!("{}: toxic={}", module.path.display(), is_toxic);
/// }
/// ```
pub fn discover_with_toxicity(root: &Path) -> Result<(DiscoveryResult, ToxicityGraph)> {
    discover_with_toxicity_options(root, false)
}

/// Discover tests with toxicity analysis, with options.
///
/// # Arguments
/// * `root` - The root directory to scan
/// * `no_ignore` - If true, ignore .gitignore and .ignore files during discovery
pub fn discover_with_toxicity_options(
    root: &Path,
    no_ignore: bool,
) -> Result<(DiscoveryResult, ToxicityGraph)> {
    // 1. Run standard discovery (finds test files and fixtures)
    let discovery = discovery::discover(root, no_ignore)?;

    // 2. Build hook registry from discovered hooks
    let registry = discovery.build_hook_registry();

    // 3. Collect ALL Python files in project (not just test modules)
    // This is critical for transitive toxicity propagation:
    // If test_foo.py imports helper.py which imports threading,
    // we need helper.py in the graph to propagate toxicity to test_foo.py
    let all_py_files = collect_all_py_files(root);

    // 4. Build toxicity graph (analyzes all files, includes hook toxicity, propagates)
    let graph = ToxicityGraph::build(&all_py_files, root, &registry);

    Ok((discovery, graph))
}
