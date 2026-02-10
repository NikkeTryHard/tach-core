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

/// Check whether a class name follows pytest's test class naming conventions.
///
/// Pytest discovers test classes through two mechanisms:
/// 1. **Name matching**: classes whose name starts with `Test` (the `python_classes` default)
/// 2. **Inheritance**: any `unittest.TestCase` subclass, regardless of name
///
/// Since tach performs static AST analysis without import-time MRO resolution,
/// we approximate mechanism (2) by also matching common suffix conventions:
/// - `*Test`     (e.g. `LoginTest`, `FormTest`)
/// - `*Tests`    (e.g. `LoginTests`, `ModelFormTests`)
/// - `*TestCase` (e.g. `AutodiscoverModulesTestCase`, `MyFeatureTestCase`)
///
/// The name must be at least 5 characters so that bare `"Test"` alone doesn't match —
/// a descriptive component is always required.
#[inline]
pub fn is_test_class(name: &str) -> bool {
    let len = name.len();
    if len < 5 {
        return false;
    }
    name.starts_with("Test")
        || name.ends_with("Test")
        || name.ends_with("Tests")
        || name.ends_with("TestCase")
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
