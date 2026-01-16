//! Hook registry for tracking and dispatching pytest hooks.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Specification for a pytest hook
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookSpec {
    /// Hook name (e.g., "pytest_configure")
    pub name: String,
    /// Whether this hook can modify global state (affects toxicity)
    pub modifies_global_state: bool,
    /// Whether hook results should be cached
    pub cacheable: bool,
}

/// A registered hook implementation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hook {
    /// Hook specification
    pub spec: HookSpec,
    /// Source file containing the hook (conftest.py path)
    pub source: PathBuf,
    /// Function name implementing the hook
    pub function_name: String,
    /// Line number in source file
    pub line_number: usize,
}

/// Effects produced by hook execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HookEffect {
    /// Hook set an environment variable
    SetEnv { key: String, value: String },
    /// Hook modified sys.path
    ModifySysPath { action: String, path: String },
    /// Hook registered a marker
    RegisterMarker { name: String, description: String },
    /// Hook modified test collection
    ModifyItems { removed: Vec<String>, reordered: bool },
    /// Hook has no observable effects
    NoEffect,
}

/// Registry of all discovered hooks
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct HookRegistry {
    /// Hooks indexed by hook name
    hooks: HashMap<String, Vec<Hook>>,
    /// Cached effects from hook execution
    effects: HashMap<String, Vec<HookEffect>>,
}

impl HookRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a hook from a conftest.py file
    pub fn register(&mut self, hook: Hook) {
        self.hooks.entry(hook.spec.name.clone()).or_default().push(hook);
    }

    /// Get all hooks for a given hook name
    pub fn get_hooks(&self, name: &str) -> &[Hook] {
        self.hooks.get(name).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Check if any hooks modify global state (makes tests toxic)
    pub fn has_global_state_hooks(&self) -> bool {
        self.hooks.values().flatten().any(|h| h.spec.modifies_global_state)
    }

    /// Record an effect from hook execution
    pub fn record_effect(&mut self, hook_name: &str, effect: HookEffect) {
        self.effects.entry(hook_name.to_string()).or_default().push(effect);
    }

    /// Get cached effects for replay in workers
    pub fn get_effects(&self, hook_name: &str) -> &[HookEffect] {
        self.effects.get(hook_name).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Number of registered hooks
    pub fn hook_count(&self) -> usize {
        self.hooks.values().map(|v| v.len()).sum()
    }

    /// Check if a specific file contains any hooks that modify global state
    pub fn file_has_toxic_hooks(&self, path: &std::path::Path) -> bool {
        self.hooks.values().flatten().any(|h| h.source == path && h.spec.modifies_global_state)
    }

    /// Get all hooks defined in a specific file
    pub fn get_hooks_for_file(&self, path: &std::path::Path) -> Vec<&Hook> {
        self.hooks.values().flatten().filter(|h| h.source == path).collect()
    }

    /// Get all registered hooks (for debugging)
    pub fn all_hooks(&self) -> impl Iterator<Item = &Hook> {
        self.hooks.values().flatten()
    }

    /// Resolve all hooks that apply to a given test path
    ///
    /// Traverses from the test's directory up to project root,
    /// collecting hooks from each conftest.py in order (root first).
    ///
    /// # Arguments
    /// * `test_path` - Path to the test file
    /// * `project_root` - Project root directory
    ///
    /// # Returns
    /// Vec of hooks in application order (root conftest first, leaf last)
    pub fn resolve_hooks_for_path(&self, test_path: &Path, project_root: &Path) -> Vec<Hook> {
        let mut conftest_dirs = Vec::new();
        let mut current = test_path.parent();

        // Collect directories from test up to root
        while let Some(dir) = current {
            conftest_dirs.push(dir.to_path_buf());
            if dir == project_root {
                break;
            }
            current = dir.parent();
        }

        // Reverse to get root-first order
        conftest_dirs.reverse();

        // Collect hooks from each conftest.py
        let mut result = Vec::new();
        for dir in conftest_dirs {
            let conftest_path = dir.join("conftest.py");
            for hook in self.get_hooks_for_file(&conftest_path) {
                result.push(hook.clone());
            }
        }

        result
    }
}

/// Well-known pytest hooks with their specifications
pub fn builtin_hook_specs() -> HashMap<String, HookSpec> {
    let mut specs = HashMap::new();

    specs.insert(
        "pytest_configure".to_string(),
        HookSpec {
            name: "pytest_configure".to_string(),
            modifies_global_state: true,
            cacheable: true,
        },
    );

    specs.insert(
        "pytest_collection_modifyitems".to_string(),
        HookSpec {
            name: "pytest_collection_modifyitems".to_string(),
            modifies_global_state: false,
            cacheable: true,
        },
    );

    specs.insert(
        "pytest_runtest_setup".to_string(),
        HookSpec {
            name: "pytest_runtest_setup".to_string(),
            modifies_global_state: false,
            cacheable: false,
        },
    );

    specs.insert(
        "pytest_runtest_teardown".to_string(),
        HookSpec {
            name: "pytest_runtest_teardown".to_string(),
            modifies_global_state: false,
            cacheable: false,
        },
    );

    specs.insert(
        "pytest_unconfigure".to_string(),
        HookSpec {
            name: "pytest_unconfigure".to_string(),
            modifies_global_state: true, // Counterpart to configure
            cacheable: true,
        },
    );

    specs.insert(
        "pytest_collection_finish".to_string(),
        HookSpec {
            name: "pytest_collection_finish".to_string(),
            modifies_global_state: false,
            cacheable: true,
        },
    );

    specs.insert(
        "pytest_runtest_call".to_string(),
        HookSpec {
            name: "pytest_runtest_call".to_string(),
            modifies_global_state: false,
            cacheable: false, // Called per-test
        },
    );

    specs.insert(
        "pytest_runtest_makereport".to_string(),
        HookSpec {
            name: "pytest_runtest_makereport".to_string(),
            modifies_global_state: false,
            cacheable: false, // Called per-test
        },
    );

    specs.insert(
        "pytest_sessionstart".to_string(),
        HookSpec {
            name: "pytest_sessionstart".to_string(),
            modifies_global_state: true, // Session-level
            cacheable: true,
        },
    );

    specs.insert(
        "pytest_sessionfinish".to_string(),
        HookSpec {
            name: "pytest_sessionfinish".to_string(),
            modifies_global_state: true, // Session-level
            cacheable: true,
        },
    );

    specs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_creation() {
        let registry = HookRegistry::new();
        assert_eq!(registry.hook_count(), 0);
    }

    #[test]
    fn test_register_hook() {
        let mut registry = HookRegistry::new();

        let hook = Hook {
            spec: HookSpec {
                name: "pytest_configure".to_string(),
                modifies_global_state: true,
                cacheable: true,
            },
            source: PathBuf::from("conftest.py"),
            function_name: "pytest_configure".to_string(),
            line_number: 10,
        };

        registry.register(hook);
        assert_eq!(registry.hook_count(), 1);
        assert!(registry.has_global_state_hooks());
    }

    #[test]
    fn test_builtin_specs() {
        let specs = builtin_hook_specs();
        assert_eq!(specs.len(), 10, "Should have 10 builtin hook specs");
        assert!(specs.contains_key("pytest_configure"));
        assert!(specs.contains_key("pytest_unconfigure"));
        assert!(specs.contains_key("pytest_collection_modifyitems"));
        assert!(specs.contains_key("pytest_collection_finish"));
        assert!(specs.contains_key("pytest_runtest_setup"));
        assert!(specs.contains_key("pytest_runtest_call"));
        assert!(specs.contains_key("pytest_runtest_teardown"));
        assert!(specs.contains_key("pytest_runtest_makereport"));
        assert!(specs.contains_key("pytest_sessionstart"));
        assert!(specs.contains_key("pytest_sessionfinish"));
    }

    #[test]
    fn test_file_has_toxic_hooks() {
        let mut registry = HookRegistry::new();

        // Register a toxic hook (pytest_configure modifies global state)
        let toxic_hook = Hook {
            spec: HookSpec {
                name: "pytest_configure".to_string(),
                modifies_global_state: true,
                cacheable: true,
            },
            source: PathBuf::from("tests/conftest.py"),
            function_name: "pytest_configure".to_string(),
            line_number: 5,
        };
        registry.register(toxic_hook);

        // Register a non-toxic hook
        let safe_hook = Hook {
            spec: HookSpec {
                name: "pytest_runtest_setup".to_string(),
                modifies_global_state: false,
                cacheable: false,
            },
            source: PathBuf::from("tests/sub/conftest.py"),
            function_name: "pytest_runtest_setup".to_string(),
            line_number: 10,
        };
        registry.register(safe_hook);

        // Test file_has_toxic_hooks
        assert!(registry.file_has_toxic_hooks(&PathBuf::from("tests/conftest.py")));
        assert!(!registry.file_has_toxic_hooks(&PathBuf::from("tests/sub/conftest.py")));
        assert!(!registry.file_has_toxic_hooks(&PathBuf::from("nonexistent.py")));
    }

    #[test]
    fn test_get_hooks_for_file() {
        let mut registry = HookRegistry::new();

        let hook1 = Hook {
            spec: HookSpec {
                name: "pytest_configure".to_string(),
                modifies_global_state: true,
                cacheable: true,
            },
            source: PathBuf::from("conftest.py"),
            function_name: "pytest_configure".to_string(),
            line_number: 5,
        };
        let hook2 = Hook {
            spec: HookSpec {
                name: "pytest_collection_modifyitems".to_string(),
                modifies_global_state: false,
                cacheable: true,
            },
            source: PathBuf::from("conftest.py"),
            function_name: "pytest_collection_modifyitems".to_string(),
            line_number: 15,
        };
        registry.register(hook1);
        registry.register(hook2);

        let hooks = registry.get_hooks_for_file(&PathBuf::from("conftest.py"));
        assert_eq!(hooks.len(), 2);

        let hooks = registry.get_hooks_for_file(&PathBuf::from("other.py"));
        assert!(hooks.is_empty());
    }

    #[test]
    fn test_resolve_hooks_for_path() {
        let mut registry = HookRegistry::new();

        // Root conftest hook
        registry.register(Hook {
            spec: HookSpec {
                name: "pytest_configure".to_string(),
                modifies_global_state: true,
                cacheable: true,
            },
            source: PathBuf::from("/project/conftest.py"),
            function_name: "pytest_configure".to_string(),
            line_number: 1,
        });

        // Sub-directory conftest hook
        registry.register(Hook {
            spec: HookSpec {
                name: "pytest_runtest_setup".to_string(),
                modifies_global_state: false,
                cacheable: false,
            },
            source: PathBuf::from("/project/tests/conftest.py"),
            function_name: "pytest_runtest_setup".to_string(),
            line_number: 1,
        });

        // Resolve hooks for a test in tests/
        let project_root = Path::new("/project");
        let test_path = Path::new("/project/tests/test_example.py");
        let hooks = registry.resolve_hooks_for_path(test_path, project_root);

        // Should get both hooks: root first, then subdirectory
        assert_eq!(hooks.len(), 2);
        assert_eq!(hooks[0].source, PathBuf::from("/project/conftest.py"));
        assert_eq!(hooks[1].source, PathBuf::from("/project/tests/conftest.py"));
    }
}
