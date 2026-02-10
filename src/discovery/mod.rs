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
pub mod inheritance;
pub mod loader;
pub mod resolver;
pub mod scanner;

/// Check whether a class name matches pytest's `python_classes` prefix convention.
///
/// Pytest's default `python_classes = ["Test"]` uses **prefix matching**:
/// `name.starts_with("Test")`. This matches `TestFoo`, `Testing`, etc.
///
/// For classes that DON'T start with `Test` but inherit from `unittest.TestCase`,
/// pytest's unittest plugin collects them via a separate path that bypasses
/// name matching entirely. We handle that via `has_testcase_base()` in the scanner.
#[inline]
pub fn is_test_class(name: &str) -> bool {
    name.starts_with("Test") && name.len() >= 5
}

/// Check whether a class name follows `*TestCase` suffix conventions.
///
/// This matches classes like `MyFeatureTestCase` which strongly imply
/// unittest.TestCase lineage. Used together with `has_testcase_base()` to
/// identify test classes that don't follow the `Test*` prefix convention.
#[inline]
pub fn is_testcase_by_suffix(name: &str) -> bool {
    name.ends_with("TestCase") && name.len() > 8
}

/// Check whether any base class in the AST suggests a `unittest.TestCase` lineage.
///
/// Inspects the class definition's `bases` list for names ending in `TestCase`,
/// matching patterns like `TestCase`, `unittest.TestCase`, and `django.test.TestCase`.
/// This catches test classes that don't follow pytest's `Test*` prefix convention
/// but inherit from `unittest.TestCase` (or any `*TestCase` base).
pub fn has_testcase_base(bases: &[rustpython_ast::Expr]) -> bool {
    use rustpython_ast as ast;
    bases.iter().any(|base| match base {
        // Simple name: `class Foo(TestCase):`
        ast::Expr::Name(name) => name.id.as_str().ends_with("TestCase"),
        // Dotted name: `class Foo(unittest.TestCase):` or `class Foo(django.test.TestCase):`
        ast::Expr::Attribute(attr) => attr.attr.as_str().ends_with("TestCase"),
        _ => false,
    })
}

// Re-export main types from scanner for backward compatibility
pub use scanner::{
    DiscoveryResult, FixtureDefinition, FixtureScope, HookDefinition, MarkerInfo, TestCase,
    TestModule, detect_blocking_patterns, discover, dump_json,
};

// Re-export config types
pub use config::{AsyncioConfig, parse_asyncio_config};
