//! Hook Interception Framework for pytest plugin compatibility
//!
//! This module provides a lightweight hook system that intercepts common
//! pytest hooks without requiring full pluggy support. It records hook
//! effects in the supervisor and replays them in workers.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────┐     ┌──────────────┐     ┌─────────────┐
//! │  conftest   │────>│ HookRegistry │────>│   Workers   │
//! │  discovery  │     │  (Rust)      │     │  (Python)   │
//! └─────────────┘     └──────────────┘     └─────────────┘
//! ```
//!
//! # Supported Hooks (v0.2.0)
//!
//! **Session-level hooks** (run once, effects cached and replayed):
//! - `pytest_configure` - Plugin configuration, env vars, sys.path
//! - `pytest_sessionstart` - Session initialization
//! - `pytest_sessionfinish` - Session cleanup
//! - `pytest_unconfigure` - Plugin cleanup
//!
//! **Collection hooks**:
//! - `pytest_collection_modifyitems` - Test ordering/filtering
//! - `pytest_collection_finish` - Post-collection processing
//!
//! **Per-test hooks**:
//! - `pytest_runtest_setup` - Pre-test setup
//! - `pytest_runtest_call` - Test execution
//! - `pytest_runtest_teardown` - Post-test teardown
//! - `pytest_runtest_makereport` - Result reporting

mod caller;
mod graph;
mod plugins;
mod registry;

pub use caller::HookCaller;
pub use graph::HookDependencyGraph;
pub use plugins::{PluginRegistry, PluginStatus};
pub use registry::{
    AggregationStrategy, Hook, HookEffect, HookRegistry, HookResult, HookSpec, SysPathAction,
    aggregate_results, builtin_hook_specs,
};

/// Sort hooks by source path depth (root conftest first, leaf last)
///
/// Used by both HookCaller and HookDependencyGraph to ensure consistent
/// ordering based on conftest hierarchy. Hooks from shallower paths
/// (fewer components) execute before hooks from deeper paths.
pub fn sort_hooks_by_depth(hooks: &mut [&Hook]) {
    hooks.sort_by_key(|h| h.source.components().count());
}
