//! Hook Interception Framework for pytest plugin compatibility
//!
//! This module provides a lightweight hook system that intercepts common
//! pytest hooks without requiring full pluggy support. It records hook
//! effects in the supervisor and replays them in workers.
//! # Architecture
//!
//! ```text
//! ┌─────────────┐     ┌──────────────┐     ┌─────────────┐
//! │  conftest   │────>│ HookRegistry │────>│   Workers   │
//! │  discovery  │     │  (Rust)      │     │  (Python)   │
//! └─────────────┘     └──────────────┘     └─────────────┘
//! ```
//!
//! # Supported Hooks (0.2.0)
//!
//! - `pytest_configure` - Plugin configuration
//! - `pytest_collection_modifyitems` - Test ordering/filtering
//! - `pytest_runtest_setup` - Per-test setup
//! - `pytest_runtest_teardown` - Per-test teardown

mod caller;
mod graph;
mod registry;

pub use caller::HookCaller;
pub use graph::HookDependencyGraph;
pub use registry::{
    AggregationStrategy, Hook, HookEffect, HookRegistry, HookResult, HookSpec, SysPathAction,
    aggregate_results, builtin_hook_specs,
};
