//! Test discovery and analysis modules
//!
//! This module handles finding and analyzing Python test files:
//! - `scanner`: Test file discovery and AST parsing
//! - `resolver`: Fixture resolution and dependency tracking
//! - `loader`: Zero-copy module loading
//! - `graph`: Dependency graph construction
//! - `analysis`: Toxicity analysis and classification
//! - `cache`: Disk-based conftest parsing cache
//! - `config`: Asyncio configuration parsing

pub mod analysis;
pub mod cache;
pub mod config;
pub mod graph;
pub mod loader;
pub mod resolver;
pub mod scanner;

// Re-export main types from scanner for backward compatibility
pub use scanner::{
    DiscoveryResult, FixtureDefinition, FixtureScope, HookDefinition, MarkerInfo, TestCase,
    TestModule, detect_blocking_patterns, discover, dump_json,
};

// Re-export config types
pub use config::{AsyncioConfig, parse_asyncio_config};
