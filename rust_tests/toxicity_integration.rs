//! Integration tests for Toxicity Analysis: Toxicity Analysis
//!
//! Tests the full pipeline: discover_with_toxicity() -> RunnableTest.is_toxic

use std::path::Path;
use std::path::PathBuf;
use tach_core::discover_with_toxicity;
use tach_core::discovery::{DiscoveryResult, TestCase, TestModule};
use tach_core::graph::ToxicityGraph;
use tach_core::resolver::{FixtureRegistry, Resolver};

// =============================================================================
// ToxicityGraph Unit Tests (using mock data)
// =============================================================================

#[test]
fn test_toxicity_graph_safe_module() {
    use tempfile::TempDir;

    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    // Create a safe Python file
    std::fs::write(
        root.join("safe.py"),
        r#"
import os
import json

def helper():
    pass
"#,
    )
    .unwrap();

    let paths = vec![root.join("safe.py")];
    let graph = ToxicityGraph::build(&paths, root);

    assert!(
        !graph.is_toxic(&root.join("safe.py")),
        "safe.py should NOT be toxic"
    );
    assert_eq!(graph.safe_modules().len(), 1);
    assert_eq!(graph.toxic_modules().len(), 0);
}

#[test]
fn test_toxicity_graph_toxic_module() {
    use tempfile::TempDir;

    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    // Create a toxic Python file
    std::fs::write(
        root.join("toxic.py"),
        r#"
import threading

def worker():
    pass
"#,
    )
    .unwrap();

    let paths = vec![root.join("toxic.py")];
    let graph = ToxicityGraph::build(&paths, root);

    assert!(
        graph.is_toxic(&root.join("toxic.py")),
        "toxic.py SHOULD be toxic"
    );
    assert_eq!(graph.toxic_modules().len(), 1);
    assert_eq!(graph.safe_modules().len(), 0);
}

#[test]
fn test_toxicity_graph_transitive_propagation() {
    use tempfile::TempDir;

    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    // Create a toxic helper module
    std::fs::write(
        root.join("toxic_helper.py"),
        r#"
import socket

def create_connection():
    return socket.socket()
"#,
    )
    .unwrap();

    // Create a module that imports the toxic helper
    std::fs::write(
        root.join("uses_toxic.py"),
        r#"
import toxic_helper

def do_something():
    toxic_helper.create_connection()
"#,
    )
    .unwrap();

    let paths = vec![root.join("toxic_helper.py"), root.join("uses_toxic.py")];
    let graph = ToxicityGraph::build(&paths, root);

    assert!(
        graph.is_toxic(&root.join("toxic_helper.py")),
        "toxic_helper.py SHOULD be toxic"
    );
    assert!(
        graph.is_toxic(&root.join("uses_toxic.py")),
        "uses_toxic.py SHOULD be toxic (transitive)"
    );
    assert_eq!(graph.toxic_modules().len(), 2);
}

#[test]
fn test_toxicity_graph_type_checking_skip() {
    use tempfile::TempDir;

    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    // Create a file with TYPE_CHECKING import (should NOT be toxic)
    std::fs::write(
        root.join("type_hints.py"),
        r#"
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    import threading  # Only for type hints

def safe_function():
    pass
"#,
    )
    .unwrap();

    let paths = vec![root.join("type_hints.py")];
    let graph = ToxicityGraph::build(&paths, root);

    assert!(
        !graph.is_toxic(&root.join("type_hints.py")),
        "TYPE_CHECKING imports should NOT make module toxic"
    );
    assert_eq!(graph.safe_modules().len(), 1);
    assert_eq!(graph.toxic_modules().len(), 0);
}

#[test]
fn test_toxicity_graph_mixed_imports() {
    use tempfile::TempDir;

    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    // Create a file with both TYPE_CHECKING and runtime toxic import
    std::fs::write(
        root.join("mixed.py"),
        r#"
from typing import TYPE_CHECKING
import socket  # Runtime import - TOXIC

if TYPE_CHECKING:
    import threading  # Type-only import - NOT toxic

def mixed_function():
    pass
"#,
    )
    .unwrap();

    let paths = vec![root.join("mixed.py")];
    let graph = ToxicityGraph::build(&paths, root);

    assert!(
        graph.is_toxic(&root.join("mixed.py")),
        "Runtime socket import should make module toxic"
    );
}

// =============================================================================
// RunnableTest.is_toxic Tagging Tests (using mock DiscoveryResult)
// =============================================================================

#[test]
fn test_runnable_test_is_toxic_tagging_mock() {
    use tempfile::TempDir;

    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    // Create safe test file
    std::fs::write(
        root.join("test_safe.py"),
        r#"
import os

def test_safe():
    pass
"#,
    )
    .unwrap();

    // Create toxic test file
    std::fs::write(
        root.join("test_toxic.py"),
        r#"
import multiprocessing

def test_toxic():
    pass
"#,
    )
    .unwrap();

    // Build toxicity graph
    let paths = vec![root.join("test_safe.py"), root.join("test_toxic.py")];
    let graph = ToxicityGraph::build(&paths, root);

    // Create mock discovery result
    let discovery = DiscoveryResult {
        modules: vec![
            TestModule {
                path: root.join("test_safe.py"),
                tests: vec![TestCase {
                    name: "test_safe".to_string(),
                    dependencies: vec![],
                    is_async: false,
                    line_number: 1,
                    parametrized_args: vec![],
                    timeout_secs: None,
                }],
                fixtures: vec![],
                is_toxic: false,
            },
            TestModule {
                path: root.join("test_toxic.py"),
                tests: vec![TestCase {
                    name: "test_toxic".to_string(),
                    dependencies: vec![],
                    is_async: false,
                    line_number: 1,
                    parametrized_args: vec![],
                    timeout_secs: None,
                }],
                fixtures: vec![],
                is_toxic: false,
            },
        ],
    };

    // Resolve tests
    let registry = FixtureRegistry::from_discovery(&discovery);
    let resolver = Resolver::new(&registry);
    let (mut runnable_tests, errors) = resolver.resolve_all(&discovery);

    assert!(errors.is_empty(), "Should have no resolution errors");
    assert_eq!(runnable_tests.len(), 2);

    // Tag tests with toxicity (simulating what main.rs does)
    for test in &mut runnable_tests {
        test.is_toxic = graph.is_toxic(&test.file_path);
    }

    // Find and verify each test
    let safe_test = runnable_tests
        .iter()
        .find(|t| t.test_name == "test_safe")
        .expect("Should find test_safe");
    let toxic_test = runnable_tests
        .iter()
        .find(|t| t.test_name == "test_toxic")
        .expect("Should find test_toxic");

    assert!(!safe_test.is_toxic, "test_safe should NOT be toxic");
    assert!(toxic_test.is_toxic, "test_toxic SHOULD be toxic");
}

// =============================================================================
// Real Project Integration Tests
// =============================================================================

#[test]
fn test_discover_with_toxicity_real_project() {
    // Use the actual tach-core project directory
    let project_root = Path::new(env!("CARGO_MANIFEST_DIR"));

    // Skip this test when running from a worktree, as the collect_all_py_files
    // function excludes directories starting with '.' (like .worktrees)
    if project_root.to_string_lossy().contains(".worktrees") {
        eprintln!("Skipping test in worktree environment");
        return;
    }

    let (discovery, graph) =
        discover_with_toxicity(project_root).expect("Discovery should succeed on real project");

    // Should find tests in the real project
    assert!(
        discovery.test_count() >= 100,
        "Should find many tests in real project, found {}",
        discovery.test_count()
    );

    // The graph should have analyzed modules
    let total_modules = graph.toxic_modules().len() + graph.safe_modules().len();
    assert!(
        total_modules > 0,
        "Should have analyzed at least some modules"
    );

    eprintln!(
        "Real project: {} tests, {} toxic modules, {} safe modules",
        discovery.test_count(),
        graph.toxic_modules().len(),
        graph.safe_modules().len()
    );
}

// =============================================================================
// Edge Cases
// =============================================================================

#[test]
fn test_empty_graph() {
    use tempfile::TempDir;

    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    let paths: Vec<PathBuf> = vec![];
    let graph = ToxicityGraph::build(&paths, root);

    assert_eq!(graph.toxic_modules().len(), 0);
    assert_eq!(graph.safe_modules().len(), 0);
}

#[test]
fn test_all_safe_modules() {
    use tempfile::TempDir;

    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    std::fs::write(root.join("a.py"), "import os\ndef a(): pass").unwrap();
    std::fs::write(root.join("b.py"), "import json\ndef b(): pass").unwrap();

    let paths = vec![root.join("a.py"), root.join("b.py")];
    let graph = ToxicityGraph::build(&paths, root);

    assert_eq!(graph.toxic_modules().len(), 0, "No modules should be toxic");
    assert_eq!(graph.safe_modules().len(), 2, "All modules should be safe");
}

#[test]
fn test_all_toxic_modules() {
    use tempfile::TempDir;

    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    std::fs::write(root.join("a.py"), "import threading\ndef a(): pass").unwrap();
    std::fs::write(root.join("b.py"), "import multiprocessing\ndef b(): pass").unwrap();

    let paths = vec![root.join("a.py"), root.join("b.py")];
    let graph = ToxicityGraph::build(&paths, root);

    assert_eq!(
        graph.toxic_modules().len(),
        2,
        "All modules should be toxic"
    );
    assert_eq!(graph.safe_modules().len(), 0, "No modules should be safe");
}
