//! Tach Core Library
//!
//! This library exposes the core modules for integration testing.
//! The binary entry point is in main.rs.

pub mod analysis;
pub mod config;
pub mod debugger;
pub mod discovery;
pub mod environment;
pub mod graph;
pub mod isolation;
pub mod junit;
pub mod lifecycle;
pub mod loader;
pub mod logcapture;
pub mod protocol;
pub mod reporter;
pub mod resolver;
pub mod scheduler;
pub mod signals;
pub mod snapshot;
pub mod watch;
pub mod zygote;

// =============================================================================
// Phase 3.3: Toxicity Integration
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
            path.extension().map_or(false, |ext| ext == "py")
                // Exclude hidden directories, __pycache__, .git, etc.
                && !path.ancestors().any(|p| {
                    p.file_name().map_or(false, |name| {
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
/// providing a single entry point for the Phase 3 pipeline.
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
    // 1. Run standard discovery (finds test files and fixtures)
    let discovery = discovery::discover(root)?;

    // 2. Collect ALL Python files in project (not just test modules)
    // This is critical for transitive toxicity propagation:
    // If test_foo.py imports helper.py which imports threading,
    // we need helper.py in the graph to propagate toxicity to test_foo.py
    let all_py_files = collect_all_py_files(root);

    // 3. Build toxicity graph (analyzes all files and propagates toxicity)
    let graph = ToxicityGraph::build(&all_py_files, root);

    Ok((discovery, graph))
}
