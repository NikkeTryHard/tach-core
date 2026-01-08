//! Toxicity Graph Module
//!
//! The "Contagion Engine" - builds a dependency graph of Python modules and
//! propagates toxicity transitively. If module B is toxic and module A imports B,
//! then A becomes toxic.
//!
//! Key Design:
//! - Uses petgraph DiGraph for dependency relationships
//! - Edge A -> B means "A imports B"
//! - Fixed-point iteration for propagation (handles cycles)
//! - Module resolution: path -> dotted module name

use crate::analysis::{ToxicityReport, analyze_file};
use petgraph::graph::{DiGraph, NodeIndex};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

// =============================================================================
// Data Structures
// =============================================================================

/// Node data in the toxicity graph
#[derive(Debug, Clone)]
pub struct ModuleNode {
    /// Dotted module name (e.g., "app.utils")
    pub name: String,
    /// File path
    pub path: PathBuf,
    /// Whether this module is toxic
    pub is_toxic: bool,
    /// Reasons for toxicity (from local analysis + propagation)
    pub reasons: Vec<String>,
}

/// The Toxicity Dependency Graph
///
/// Tracks import relationships between Python modules and propagates
/// toxicity transitively.
#[derive(Debug)]
pub struct ToxicityGraph {
    /// The directed graph: Edge A -> B means "A imports B"
    graph: DiGraph<ModuleNode, ()>,
    /// Map dotted module name -> NodeIndex for fast lookup
    name_to_node: HashMap<String, NodeIndex>,
    /// Map file path -> NodeIndex
    path_to_node: HashMap<PathBuf, NodeIndex>,
}

impl ToxicityGraph {
    /// Create a new empty toxicity graph
    pub fn new() -> Self {
        Self {
            graph: DiGraph::new(),
            name_to_node: HashMap::new(),
            path_to_node: HashMap::new(),
        }
    }

    /// Build a toxicity graph from a list of Python file paths
    ///
    /// This is the main entry point. It:
    /// 1. Indexes all files (path -> module name)
    /// 2. Analyzes each file for local toxicity
    /// 3. Builds import edges
    /// 4. Propagates toxicity transitively
    ///
    /// # Arguments
    /// * `paths` - List of Python file paths to analyze
    /// * `project_root` - Root directory for module name resolution
    pub fn build(paths: &[PathBuf], project_root: &Path) -> Self {
        let mut graph = Self::new();

        // Step 1: Index all files and create nodes
        let mut reports: HashMap<PathBuf, ToxicityReport> = HashMap::new();

        for path in paths {
            // Compute dotted module name from path
            let module_name = path_to_module_name(path, project_root);

            // Read and analyze file
            let source = match fs::read_to_string(path) {
                Ok(s) => s,
                Err(_) => continue, // Skip unreadable files
            };

            let report = analyze_file(&source, path);

            // Create node
            let node = ModuleNode {
                name: module_name.clone(),
                path: path.clone(),
                is_toxic: report.is_toxic,
                reasons: report.reasons.clone(),
            };

            let idx = graph.graph.add_node(node);
            graph.name_to_node.insert(module_name, idx);
            graph.path_to_node.insert(path.clone(), idx);

            reports.insert(path.clone(), report);
        }

        // Step 2: Build edges from import relationships
        for (path, report) in &reports {
            let Some(&from_idx) = graph.path_to_node.get(path) else {
                continue;
            };

            for import in &report.imports {
                // Try to resolve import to a local module
                if let Some(&to_idx) = graph.resolve_import(import, project_root) {
                    // Add edge: from_idx imports to_idx
                    graph.graph.add_edge(from_idx, to_idx, ());
                }
                // If import doesn't resolve to local module, it's external
                // External toxicity is already handled by local analysis
            }
        }

        // Step 3: Propagate toxicity
        graph.propagate();

        graph
    }

    /// Resolve an import string to a NodeIndex if it matches a local module
    fn resolve_import(&self, import: &str, _project_root: &Path) -> Option<&NodeIndex> {
        // Direct match: "app.utils" -> node "app.utils"
        if let Some(idx) = self.name_to_node.get(import) {
            return Some(idx);
        }

        // Try parent module: "app.utils.helper" -> "app.utils"
        // This handles "from app.utils import helper" where we indexed "app.utils"
        let parts: Vec<&str> = import.split('.').collect();
        for i in (1..parts.len()).rev() {
            let parent = parts[..i].join(".");
            if let Some(idx) = self.name_to_node.get(&parent) {
                return Some(idx);
            }
        }

        None
    }

    /// Propagate toxicity through the graph using fixed-point iteration
    ///
    /// Rule: If B is toxic and A imports B (edge A -> B), then A becomes toxic.
    /// Handles cycles by iterating until no changes occur.
    fn propagate(&mut self) {
        loop {
            let mut changed = false;

            // Collect edges to avoid borrow issues
            let edges: Vec<(NodeIndex, NodeIndex)> = self
                .graph
                .edge_indices()
                .filter_map(|e| self.graph.edge_endpoints(e))
                .collect();

            for (from_idx, to_idx) in edges {
                let to_toxic = self.graph[to_idx].is_toxic;
                let to_name = self.graph[to_idx].name.clone();

                if to_toxic && !self.graph[from_idx].is_toxic {
                    self.graph[from_idx].is_toxic = true;
                    self.graph[from_idx]
                        .reasons
                        .push(format!("Imports toxic module '{}'", to_name));
                    changed = true;
                }
            }

            if !changed {
                break;
            }
        }
    }

    /// Check if a module (by path) is toxic
    pub fn is_toxic(&self, path: &Path) -> bool {
        self.path_to_node
            .get(path)
            .map(|&idx| self.graph[idx].is_toxic)
            .unwrap_or(false)
    }

    /// Check if a module (by name) is toxic
    pub fn is_toxic_by_name(&self, name: &str) -> bool {
        self.name_to_node
            .get(name)
            .map(|&idx| self.graph[idx].is_toxic)
            .unwrap_or(false)
    }

    /// Get the toxicity report for a module (by path)
    pub fn get_report(&self, path: &Path) -> Option<(bool, Vec<String>)> {
        self.path_to_node.get(path).map(|&idx| {
            let node = &self.graph[idx];
            (node.is_toxic, node.reasons.clone())
        })
    }

    /// Get the toxicity report for a module (by name)
    pub fn get_report_by_name(&self, name: &str) -> Option<(bool, Vec<String>)> {
        self.name_to_node.get(name).map(|&idx| {
            let node = &self.graph[idx];
            (node.is_toxic, node.reasons.clone())
        })
    }

    /// Get all toxic modules
    pub fn toxic_modules(&self) -> Vec<&ModuleNode> {
        self.graph
            .node_indices()
            .filter_map(|idx| {
                let node = &self.graph[idx];
                if node.is_toxic { Some(node) } else { None }
            })
            .collect()
    }

    /// Get all safe modules
    pub fn safe_modules(&self) -> Vec<&ModuleNode> {
        self.graph
            .node_indices()
            .filter_map(|idx| {
                let node = &self.graph[idx];
                if !node.is_toxic { Some(node) } else { None }
            })
            .collect()
    }

    /// Get the number of nodes in the graph
    pub fn node_count(&self) -> usize {
        self.graph.node_count()
    }

    /// Get the number of edges in the graph
    pub fn edge_count(&self) -> usize {
        self.graph.edge_count()
    }
}

impl Default for ToxicityGraph {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Helper Functions
// =============================================================================

/// Convert a file path to a dotted module name
///
/// Examples:
/// - "app/utils.py" -> "app.utils"
/// - "app/utils/__init__.py" -> "app.utils"
/// - "test_foo.py" -> "test_foo"
pub fn path_to_module_name(path: &Path, project_root: &Path) -> String {
    // Strip project root if path is absolute or starts with root
    let relative = path.strip_prefix(project_root).unwrap_or(path);

    let mut parts: Vec<&str> = relative
        .components()
        .filter_map(|c| c.as_os_str().to_str())
        .collect();

    // Handle the filename
    if let Some(last) = parts.last_mut() {
        if *last == "__init__.py" {
            // Remove __init__.py, the directory name is the module
            parts.pop();
        } else if last.ends_with(".py") {
            // Remove .py extension
            *last = &last[..last.len() - 3];
        }
    }

    parts.join(".")
}

/// Convert a dotted module name to potential file paths
///
/// Examples:
/// - "app.utils" -> ["app/utils.py", "app/utils/__init__.py"]
#[allow(dead_code)]
pub fn module_name_to_paths(name: &str, project_root: &Path) -> Vec<PathBuf> {
    let parts: Vec<&str> = name.split('.').collect();
    let dir_path: PathBuf = parts.iter().collect();

    vec![
        project_root.join(&dir_path).with_extension("py"),
        project_root.join(&dir_path).join("__init__.py"),
    ]
}

// =============================================================================
// Unit Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    /// Helper to create a test file
    fn create_file(dir: &Path, name: &str, content: &str) -> PathBuf {
        let path = dir.join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let mut file = fs::File::create(&path).unwrap();
        file.write_all(content.as_bytes()).unwrap();
        path
    }

    // =========================================================================
    // Path to Module Name Tests
    // =========================================================================

    #[test]
    fn test_path_to_module_simple() {
        let root = Path::new("/project");
        let path = Path::new("utils.py");
        assert_eq!(path_to_module_name(path, root), "utils");
    }

    #[test]
    fn test_path_to_module_nested() {
        let root = Path::new("/project");
        let path = Path::new("app/utils.py");
        assert_eq!(path_to_module_name(path, root), "app.utils");
    }

    #[test]
    fn test_path_to_module_init() {
        let root = Path::new("/project");
        let path = Path::new("app/utils/__init__.py");
        assert_eq!(path_to_module_name(path, root), "app.utils");
    }

    #[test]
    fn test_path_to_module_deep() {
        let root = Path::new("/project");
        let path = Path::new("app/core/utils/helpers.py");
        assert_eq!(path_to_module_name(path, root), "app.core.utils.helpers");
    }

    // =========================================================================
    // Basic Graph Tests
    // =========================================================================

    #[test]
    fn test_empty_graph() {
        let graph = ToxicityGraph::new();
        assert_eq!(graph.node_count(), 0);
        assert_eq!(graph.edge_count(), 0);
    }

    #[test]
    fn test_single_safe_module() {
        let tmp = TempDir::new().unwrap();
        let path = create_file(tmp.path(), "safe.py", "import os\nx = 1");

        let graph = ToxicityGraph::build(std::slice::from_ref(&path), tmp.path());

        assert_eq!(graph.node_count(), 1);
        assert!(!graph.is_toxic(&path));
    }

    #[test]
    fn test_single_toxic_module() {
        let tmp = TempDir::new().unwrap();
        let path = create_file(tmp.path(), "toxic.py", "import threading");

        let graph = ToxicityGraph::build(std::slice::from_ref(&path), tmp.path());

        assert_eq!(graph.node_count(), 1);
        assert!(graph.is_toxic(&path));
    }

    // =========================================================================
    // Propagation Tests
    // =========================================================================

    #[test]
    fn test_propagation_one_hop() {
        // A imports B. B imports threading. -> A is toxic.
        let tmp = TempDir::new().unwrap();

        let b_path = create_file(tmp.path(), "b.py", "import threading");
        let a_path = create_file(tmp.path(), "a.py", "import b");

        let graph = ToxicityGraph::build(&[a_path.clone(), b_path.clone()], tmp.path());

        assert!(
            graph.is_toxic(&b_path),
            "B should be toxic (imports threading)"
        );
        assert!(
            graph.is_toxic(&a_path),
            "A should be toxic (imports toxic B)"
        );

        // Check reason
        let (_, reasons) = graph.get_report(&a_path).unwrap();
        assert!(reasons.iter().any(|r| r.contains("Imports toxic module")));
    }

    #[test]
    fn test_propagation_two_hops() {
        // A imports B. B imports C. C imports pandas. -> A is toxic.
        let tmp = TempDir::new().unwrap();

        let c_path = create_file(tmp.path(), "c.py", "import pandas");
        let b_path = create_file(tmp.path(), "b.py", "import c");
        let a_path = create_file(tmp.path(), "a.py", "import b");

        let graph = ToxicityGraph::build(
            &[a_path.clone(), b_path.clone(), c_path.clone()],
            tmp.path(),
        );

        assert!(
            graph.is_toxic(&c_path),
            "C should be toxic (imports pandas)"
        );
        assert!(
            graph.is_toxic(&b_path),
            "B should be toxic (imports toxic C)"
        );
        assert!(
            graph.is_toxic(&a_path),
            "A should be toxic (imports toxic B)"
        );
    }

    #[test]
    fn test_propagation_circular() {
        // A -> B -> A. B is toxic. -> A is toxic.
        let tmp = TempDir::new().unwrap();

        // B imports A and threading (toxic)
        let b_path = create_file(tmp.path(), "b.py", "import a\nimport threading");
        // A imports B
        let a_path = create_file(tmp.path(), "a.py", "import b");

        let graph = ToxicityGraph::build(&[a_path.clone(), b_path.clone()], tmp.path());

        assert!(
            graph.is_toxic(&b_path),
            "B should be toxic (imports threading)"
        );
        assert!(
            graph.is_toxic(&a_path),
            "A should be toxic (imports toxic B)"
        );
    }

    #[test]
    fn test_propagation_circular_three_nodes() {
        // A -> B -> C -> A. C is toxic.
        let tmp = TempDir::new().unwrap();

        let c_path = create_file(tmp.path(), "c.py", "import a\nimport socket");
        let b_path = create_file(tmp.path(), "b.py", "import c");
        let a_path = create_file(tmp.path(), "a.py", "import b");

        let graph = ToxicityGraph::build(
            &[a_path.clone(), b_path.clone(), c_path.clone()],
            tmp.path(),
        );

        assert!(
            graph.is_toxic(&c_path),
            "C should be toxic (imports socket)"
        );
        assert!(
            graph.is_toxic(&b_path),
            "B should be toxic (imports toxic C)"
        );
        assert!(
            graph.is_toxic(&a_path),
            "A should be toxic (imports toxic B)"
        );
    }

    // =========================================================================
    // No Propagation Tests (Safe Chains)
    // =========================================================================

    #[test]
    fn test_no_propagation_safe_chain() {
        // A imports B. B imports C. All safe.
        let tmp = TempDir::new().unwrap();

        let c_path = create_file(tmp.path(), "c.py", "import os");
        let b_path = create_file(tmp.path(), "b.py", "import c");
        let a_path = create_file(tmp.path(), "a.py", "import b");

        let graph = ToxicityGraph::build(
            &[a_path.clone(), b_path.clone(), c_path.clone()],
            tmp.path(),
        );

        assert!(!graph.is_toxic(&c_path));
        assert!(!graph.is_toxic(&b_path));
        assert!(!graph.is_toxic(&a_path));
    }

    #[test]
    fn test_partial_toxicity() {
        // A imports B (safe). A imports C (toxic). -> A is toxic, B is safe.
        let tmp = TempDir::new().unwrap();

        let b_path = create_file(tmp.path(), "b.py", "import os");
        let c_path = create_file(tmp.path(), "c.py", "import threading");
        let a_path = create_file(tmp.path(), "a.py", "import b\nimport c");

        let graph = ToxicityGraph::build(
            &[a_path.clone(), b_path.clone(), c_path.clone()],
            tmp.path(),
        );

        assert!(!graph.is_toxic(&b_path), "B should be safe");
        assert!(graph.is_toxic(&c_path), "C should be toxic");
        assert!(graph.is_toxic(&a_path), "A should be toxic (imports C)");
    }

    // =========================================================================
    // Nested Module Tests
    // =========================================================================

    #[test]
    fn test_nested_module_import() {
        let tmp = TempDir::new().unwrap();

        // Create app/utils.py (toxic)
        let utils_path = create_file(tmp.path(), "app/utils.py", "import threading");
        // Create test_app.py that imports app.utils
        let test_path = create_file(tmp.path(), "test_app.py", "import app.utils");

        let graph = ToxicityGraph::build(&[utils_path.clone(), test_path.clone()], tmp.path());

        assert!(graph.is_toxic(&utils_path));
        assert!(graph.is_toxic(&test_path));
    }

    #[test]
    fn test_from_import_resolution() {
        let tmp = TempDir::new().unwrap();

        // Create utils.py (toxic)
        let utils_path = create_file(tmp.path(), "utils.py", "import multiprocessing");
        // Create main.py with from-import
        let main_path = create_file(tmp.path(), "main.py", "from utils import helper");

        let graph = ToxicityGraph::build(&[utils_path.clone(), main_path.clone()], tmp.path());

        assert!(graph.is_toxic(&utils_path));
        assert!(graph.is_toxic(&main_path));
    }

    // =========================================================================
    // Query API Tests
    // =========================================================================

    #[test]
    fn test_is_toxic_by_name() {
        let tmp = TempDir::new().unwrap();
        create_file(tmp.path(), "toxic.py", "import threading");
        create_file(tmp.path(), "safe.py", "import os");

        let paths: Vec<PathBuf> = vec![tmp.path().join("toxic.py"), tmp.path().join("safe.py")];
        let graph = ToxicityGraph::build(&paths, tmp.path());

        assert!(graph.is_toxic_by_name("toxic"));
        assert!(!graph.is_toxic_by_name("safe"));
        assert!(!graph.is_toxic_by_name("nonexistent"));
    }

    #[test]
    fn test_toxic_modules_list() {
        let tmp = TempDir::new().unwrap();
        create_file(tmp.path(), "a.py", "import threading");
        create_file(tmp.path(), "b.py", "import socket");
        create_file(tmp.path(), "c.py", "import os");

        let paths: Vec<PathBuf> = vec![
            tmp.path().join("a.py"),
            tmp.path().join("b.py"),
            tmp.path().join("c.py"),
        ];
        let graph = ToxicityGraph::build(&paths, tmp.path());

        let toxic = graph.toxic_modules();
        assert_eq!(toxic.len(), 2);

        let safe = graph.safe_modules();
        assert_eq!(safe.len(), 1);
        assert_eq!(safe[0].name, "c");
    }

    // =========================================================================
    // Edge Cases
    // =========================================================================

    #[test]
    fn test_external_import_ignored() {
        // Import of external package that's not in blocklist should be ignored
        let tmp = TempDir::new().unwrap();
        let path = create_file(tmp.path(), "main.py", "import requests\nimport flask");

        let graph = ToxicityGraph::build(std::slice::from_ref(&path), tmp.path());

        assert!(!graph.is_toxic(&path));
    }

    #[test]
    fn test_unresolved_local_import() {
        // Import of local module that doesn't exist in graph
        let tmp = TempDir::new().unwrap();
        let path = create_file(tmp.path(), "main.py", "import nonexistent_local");

        let graph = ToxicityGraph::build(std::slice::from_ref(&path), tmp.path());

        // Should not crash, just ignore unresolved import
        assert!(!graph.is_toxic(&path));
    }

    #[test]
    fn test_self_import() {
        // Module imports itself (edge case)
        let tmp = TempDir::new().unwrap();
        let path = create_file(tmp.path(), "self_ref.py", "import self_ref");

        let graph = ToxicityGraph::build(std::slice::from_ref(&path), tmp.path());

        // Should not crash or infinite loop
        assert!(!graph.is_toxic(&path));
    }
}
