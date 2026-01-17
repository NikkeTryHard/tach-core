//! Hook Dependency Graph
//!
//! Manages hook execution order based on conftest.py hierarchy
//! and wrapper specifications.

use crate::hooks::{Hook, sort_hooks_by_depth};
use std::collections::HashMap;

/// Manages hook dependencies and execution order
#[derive(Debug, Default)]
pub struct HookDependencyGraph {
    /// Hooks indexed by name
    hooks: HashMap<String, Vec<Hook>>,
    /// Wrapper hooks (hookwrapper=True)
    wrappers: HashMap<String, Vec<Hook>>,
}

impl HookDependencyGraph {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a hook to the graph
    pub fn add_hook(&mut self, hook: Hook) {
        let name = hook.spec.name.clone();

        if hook.is_wrapper {
            self.wrappers.entry(name).or_default().push(hook);
        } else {
            self.hooks.entry(name).or_default().push(hook);
        }
    }

    /// Get hooks in execution order (root conftest -> leaf conftest)
    pub fn get_execution_order(&self, hook_name: &str) -> Vec<&Hook> {
        let mut hooks: Vec<_> = self
            .hooks
            .get(hook_name)
            .map(|v| v.iter().collect())
            .unwrap_or_default();

        // Sort by path depth (fewer components = closer to root)
        sort_hooks_by_depth(&mut hooks);

        hooks
    }

    /// Get wrapper hooks for a given hook name
    pub fn get_wrappers(&self, hook_name: &str) -> Vec<&Hook> {
        self.wrappers
            .get(hook_name)
            .map(|v| v.iter().collect())
            .unwrap_or_default()
    }

    /// Get all hook names registered
    pub fn hook_names(&self) -> Vec<&String> {
        let mut names: Vec<_> = self.hooks.keys().collect();
        for name in self.wrappers.keys() {
            if !names.contains(&name) {
                names.push(name);
            }
        }
        names
    }

    /// Get total number of hooks (regular + wrappers)
    pub fn hook_count(&self) -> usize {
        let regular: usize = self.hooks.values().map(|v| v.len()).sum();
        let wrapper: usize = self.wrappers.values().map(|v| v.len()).sum();
        regular + wrapper
    }

    /// Check if any hooks are registered for a given name
    pub fn has_hooks(&self, hook_name: &str) -> bool {
        self.hooks.contains_key(hook_name) || self.wrappers.contains_key(hook_name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::HookSpec;
    use std::path::PathBuf;

    fn create_hook(name: &str, path: &str) -> Hook {
        Hook {
            spec: HookSpec {
                name: name.to_string(),
                modifies_global_state: false,
                cacheable: true,
            },
            source: PathBuf::from(path),
            function_name: name.to_string(),
            line_number: 1,
            is_wrapper: false,
        }
    }

    fn create_wrapper_hook(name: &str, path: &str) -> Hook {
        Hook {
            spec: HookSpec {
                name: name.to_string(),
                modifies_global_state: false,
                cacheable: false,
            },
            source: PathBuf::from(path),
            function_name: name.to_string(),
            line_number: 10,
            is_wrapper: true,
        }
    }

    #[test]
    fn test_hook_dependency_graph_orders_by_conftest_hierarchy() {
        let mut graph = HookDependencyGraph::new();

        // Add hooks from nested conftest files (out of order)
        let hooks = vec![
            create_hook("pytest_configure", "/project/tests/unit/conftest.py"),
            create_hook("pytest_configure", "/project/conftest.py"),
            create_hook("pytest_configure", "/project/tests/conftest.py"),
        ];

        for hook in hooks {
            graph.add_hook(hook);
        }

        let ordered = graph.get_execution_order("pytest_configure");

        // Should be: project/ -> project/tests/ -> project/tests/unit/
        assert_eq!(ordered.len(), 3);
        assert!(
            ordered[0]
                .source
                .to_string_lossy()
                .ends_with("project/conftest.py")
        );
        assert!(
            ordered[1]
                .source
                .to_string_lossy()
                .ends_with("tests/conftest.py")
        );
        assert!(
            ordered[2]
                .source
                .to_string_lossy()
                .ends_with("unit/conftest.py")
        );
    }

    #[test]
    fn test_hook_dependency_graph_handles_wrappers() {
        let mut graph = HookDependencyGraph::new();

        let hook = create_wrapper_hook("pytest_runtest_makereport", "/project/conftest.py");
        graph.add_hook(hook);

        let wrappers = graph.get_wrappers("pytest_runtest_makereport");
        assert_eq!(wrappers.len(), 1);
        assert!(wrappers[0].is_wrapper);
    }

    #[test]
    fn test_hook_dependency_graph_separates_regular_and_wrapper() {
        let mut graph = HookDependencyGraph::new();

        // Add regular hook
        graph.add_hook(create_hook("pytest_configure", "/project/conftest.py"));

        // Add wrapper hook for same hook name
        graph.add_hook(create_wrapper_hook(
            "pytest_configure",
            "/project/tests/conftest.py",
        ));

        // Regular hooks should not include wrapper
        let regular = graph.get_execution_order("pytest_configure");
        assert_eq!(regular.len(), 1);
        assert!(!regular[0].is_wrapper);

        // Wrappers should be separate
        let wrappers = graph.get_wrappers("pytest_configure");
        assert_eq!(wrappers.len(), 1);
        assert!(wrappers[0].is_wrapper);
    }

    #[test]
    fn test_hook_dependency_graph_empty() {
        let graph = HookDependencyGraph::new();

        assert_eq!(graph.hook_count(), 0);
        assert!(graph.get_execution_order("nonexistent").is_empty());
        assert!(graph.get_wrappers("nonexistent").is_empty());
        assert!(!graph.has_hooks("nonexistent"));
    }

    #[test]
    fn test_hook_names() {
        let mut graph = HookDependencyGraph::new();

        graph.add_hook(create_hook("pytest_configure", "/a/conftest.py"));
        graph.add_hook(create_hook("pytest_sessionstart", "/a/conftest.py"));
        graph.add_hook(create_wrapper_hook(
            "pytest_runtest_makereport",
            "/a/conftest.py",
        ));

        let names = graph.hook_names();
        assert_eq!(names.len(), 3);
    }
}
