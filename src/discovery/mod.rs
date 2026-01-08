//! Test discovery and analysis modules
//!
//! This module handles finding and analyzing Python test files:
//! - `scanner`: Test file discovery and AST parsing
//! - `resolver`: Fixture resolution and dependency tracking
//! - `loader`: Zero-copy module loading
//! - `graph`: Dependency graph construction
//! - `analysis`: Toxicity analysis and classification

pub mod analysis;
pub mod graph;
pub mod loader;
pub mod resolver;
pub mod scanner;

// Re-export main types from scanner for backward compatibility
pub use scanner::{
    DiscoveryResult, FixtureDefinition, FixtureScope, TestCase, TestModule, discover, dump_json,
};
