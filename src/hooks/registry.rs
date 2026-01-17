//! Hook registry for tracking and dispatching pytest hooks.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Standard pytest hook names as constants to avoid magic strings
pub mod hook_names {
    // Session hooks
    pub const PYTEST_CONFIGURE: &str = "pytest_configure";
    pub const PYTEST_SESSIONSTART: &str = "pytest_sessionstart";
    pub const PYTEST_SESSIONFINISH: &str = "pytest_sessionfinish";

    // Collection hooks
    pub const PYTEST_COLLECTION_MODIFYITEMS: &str = "pytest_collection_modifyitems";

    // Runtest hooks
    pub const PYTEST_RUNTEST_SETUP: &str = "pytest_runtest_setup";
    pub const PYTEST_RUNTEST_CALL: &str = "pytest_runtest_call";
    pub const PYTEST_RUNTEST_TEARDOWN: &str = "pytest_runtest_teardown";
    pub const PYTEST_RUNTEST_MAKEREPORT: &str = "pytest_runtest_makereport";
}

/// Action type for sys.path modifications
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SysPathAction {
    /// Add path to the beginning of sys.path
    Prepend,
    /// Add path to the end of sys.path
    #[default]
    Append,
    /// Remove path from sys.path
    Remove,
}

impl std::fmt::Display for SysPathAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SysPathAction::Prepend => write!(f, "prepend"),
            SysPathAction::Append => write!(f, "append"),
            SysPathAction::Remove => write!(f, "remove"),
        }
    }
}

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
    /// Whether this hook uses @pytest.hookimpl(hookwrapper=True)
    #[serde(default)]
    pub is_wrapper: bool,
}

/// Result from executing a single hook function
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HookResult {
    /// Return value from hook (JSON-serialized for flexibility)
    pub return_value: Option<String>,
    /// All return values when using AllResults aggregation
    pub all_values: Vec<String>,
    /// Side effects captured during hook execution
    pub effects: Vec<HookEffect>,
    /// Source file of the hook that produced this result
    pub source: Option<PathBuf>,
    /// Error message if hook failed
    pub error: Option<String>,
    /// Whether the hook function was found in the conftest
    #[serde(default)]
    pub hook_found: bool,
}

impl HookResult {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with_value(value: Option<String>) -> Self {
        Self {
            return_value: value,
            hook_found: true, // If we have a value, hook was found
            ..Default::default()
        }
    }

    #[must_use]
    pub fn with_error(error: String, source: PathBuf) -> Self {
        Self {
            error: Some(error),
            source: Some(source),
            ..Default::default()
        }
    }

    pub fn add_effect(&mut self, effect: HookEffect) {
        self.effects.push(effect);
    }
}

/// How to aggregate results from multiple hook implementations
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum AggregationStrategy {
    /// Return first non-None result (pytest default for most hooks)
    #[default]
    FirstResult,
    /// Collect all results into a list
    AllResults,
    /// No return value expected (side-effect only hooks)
    NoReturn,
}

/// Aggregate multiple hook results based on strategy
pub fn aggregate_results(results: &[HookResult], strategy: AggregationStrategy) -> HookResult {
    let mut aggregated = HookResult::new();

    // Collect all effects and track first error
    for result in results {
        aggregated.effects.extend(result.effects.clone());

        if let Some(ref err) = result.error
            && aggregated.error.is_none()
        {
            aggregated.error = Some(err.clone());
            aggregated.source = result.source.clone();
        }
    }

    // Aggregate return values based on strategy
    match strategy {
        AggregationStrategy::FirstResult => {
            for result in results {
                if result.return_value.is_some() {
                    aggregated.return_value = result.return_value.clone();
                    break;
                }
            }
        }
        AggregationStrategy::AllResults => {
            for result in results {
                if let Some(ref val) = result.return_value {
                    aggregated.all_values.push(val.clone());
                }
            }
        }
        AggregationStrategy::NoReturn => {
            // No return value aggregation needed
        }
    }

    // Propagate hook_found if any result had it set
    aggregated.hook_found = results.iter().any(|r| r.hook_found);

    aggregated
}

/// Effects produced by hook execution
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum HookEffect {
    /// Hook set an environment variable
    SetEnv { key: String, value: String },
    /// Hook modified sys.path
    ModifySysPath { action: SysPathAction, path: String },
    /// Hook registered a custom pytest marker
    ///
    /// Custom markers can be registered via pytest_configure hooks or pytest.ini.
    /// While the Python-side doesn't currently emit this effect (markers are
    /// typically registered via config.addinivalue_line), the type exists for
    /// future use and IPC completeness.
    ///
    /// TODO(v0.3.0): Implement marker registration tracking in call_hook_impl
    /// when pytest_configure hooks call config.addinivalue_line("markers", ...).
    RegisterMarker { name: String, description: String },
    /// Hook modified test collection (reordering, deselection)
    ModifyItems {
        removed: Vec<String>,
        reordered: bool,
    },
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
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a hook from a conftest.py file
    pub fn register(&mut self, hook: Hook) {
        self.hooks
            .entry(hook.spec.name.clone())
            .or_default()
            .push(hook);
    }

    /// Get all hooks for a given hook name
    pub fn get_hooks(&self, name: &str) -> &[Hook] {
        self.hooks.get(name).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Check if any hooks modify global state (makes tests toxic)
    pub fn has_global_state_hooks(&self) -> bool {
        self.hooks
            .values()
            .flatten()
            .any(|h| h.spec.modifies_global_state)
    }

    /// Record an effect from hook execution
    pub fn record_effect(&mut self, hook_name: &str, effect: HookEffect) {
        self.effects
            .entry(hook_name.to_string())
            .or_default()
            .push(effect);
    }

    /// Get cached effects for replay in workers
    pub fn get_effects(&self, hook_name: &str) -> &[HookEffect] {
        self.effects
            .get(hook_name)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Get all session-level effects for replay in workers.
    /// Session-level hooks are those that run once per session (pytest_configure).
    /// These effects should be applied to each worker before running tests.
    pub fn get_session_effects(&self) -> Vec<HookEffect> {
        // Session-level hooks that should be replayed in workers
        const SESSION_HOOKS: &[&str] = &[
            hook_names::PYTEST_CONFIGURE,
            hook_names::PYTEST_SESSIONSTART,
        ];

        let mut effects = Vec::new();
        for hook_name in SESSION_HOOKS {
            effects.extend(self.get_effects(hook_name).iter().cloned());
        }
        effects
    }

    /// Number of registered hooks
    pub fn hook_count(&self) -> usize {
        self.hooks.values().map(|v| v.len()).sum()
    }

    /// Check if a specific file contains any hooks that modify global state.
    ///
    /// This method handles path normalization to support both relative and absolute paths.
    /// Hooks may be registered with paths as-provided during discovery, while callers
    /// (like the toxicity graph) may use canonicalized paths. We attempt canonicalization
    /// and fall back to direct comparison if it fails.
    pub fn file_has_toxic_hooks(&self, path: &std::path::Path) -> bool {
        // Try to canonicalize the input path for consistent comparison
        let canonical_path = path.canonicalize().ok();

        self.hooks.values().flatten().any(|h| {
            // Check direct match first (handles case where both are same form)
            if h.source == path {
                return h.spec.modifies_global_state;
            }

            // Try canonical comparison if we have a canonical input path
            if let Some(ref canon) = canonical_path {
                if &h.source == canon {
                    return h.spec.modifies_global_state;
                }
                // Also try canonicalizing the stored hook source
                if let Ok(hook_canon) = h.source.canonicalize()
                    && &hook_canon == canon
                {
                    return h.spec.modifies_global_state;
                }
            }

            // Try canonicalizing just the hook source against original input
            if let Ok(hook_canon) = h.source.canonicalize()
                && hook_canon == path
            {
                return h.spec.modifies_global_state;
            }

            false
        })
    }

    /// Get all hooks defined in a specific file.
    ///
    /// This method handles path normalization to support both relative and absolute paths.
    /// See `file_has_toxic_hooks` for details on path matching strategy.
    pub fn get_hooks_for_file(&self, path: &std::path::Path) -> Vec<&Hook> {
        // Try to canonicalize the input path for consistent comparison
        let canonical_path = path.canonicalize().ok();

        self.hooks
            .values()
            .flatten()
            .filter(|h| {
                // Check direct match first
                if h.source == path {
                    return true;
                }

                // Try canonical comparison if we have a canonical input path
                if let Some(ref canon) = canonical_path {
                    if &h.source == canon {
                        return true;
                    }
                    // Also try canonicalizing the stored hook source
                    if let Ok(hook_canon) = h.source.canonicalize()
                        && &hook_canon == canon
                    {
                        return true;
                    }
                }

                // Try canonicalizing just the hook source against original input
                if let Ok(hook_canon) = h.source.canonicalize()
                    && hook_canon == path
                {
                    return true;
                }

                false
            })
            .collect()
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
            is_wrapper: false,
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
            is_wrapper: false,
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
            is_wrapper: false,
        };
        registry.register(safe_hook);

        // Test file_has_toxic_hooks
        assert!(registry.file_has_toxic_hooks(&PathBuf::from("tests/conftest.py")));
        assert!(!registry.file_has_toxic_hooks(&PathBuf::from("tests/sub/conftest.py")));
        assert!(!registry.file_has_toxic_hooks(&PathBuf::from("nonexistent.py")));
    }

    #[test]
    fn test_file_has_toxic_hooks_with_path_normalization() {
        // Test that path canonicalization works correctly when hooks are registered
        // with relative paths but queried with absolute paths (or vice versa).
        // This is important because discovery may provide relative paths while
        // the toxicity graph uses canonicalized paths.

        let mut registry = HookRegistry::new();

        // Create a temporary file to enable canonicalization
        let temp_dir = std::env::temp_dir();
        let conftest_path = temp_dir.join("test_hooks_conftest.py");

        // Create the file so canonicalization works
        std::fs::write(&conftest_path, "# test conftest").expect("Failed to create temp file");

        // Register hook with absolute path
        let toxic_hook = Hook {
            spec: HookSpec {
                name: "pytest_configure".to_string(),
                modifies_global_state: true,
                cacheable: true,
            },
            source: conftest_path.clone(),
            function_name: "pytest_configure".to_string(),
            line_number: 5,
            is_wrapper: false,
        };
        registry.register(toxic_hook);

        // Query with the same absolute path - should match
        assert!(
            registry.file_has_toxic_hooks(&conftest_path),
            "Should find toxic hook with same absolute path"
        );

        // Query with canonicalized path - should still match
        let canonical = conftest_path.canonicalize().expect("Should canonicalize");
        assert!(
            registry.file_has_toxic_hooks(&canonical),
            "Should find toxic hook with canonicalized path"
        );

        // Clean up
        std::fs::remove_file(&conftest_path).ok();
    }

    #[test]
    fn test_get_hooks_for_file_with_path_normalization() {
        // Similar test for get_hooks_for_file

        let mut registry = HookRegistry::new();

        let temp_dir = std::env::temp_dir();
        let conftest_path = temp_dir.join("test_hooks_conftest2.py");

        // Create the file so canonicalization works
        std::fs::write(&conftest_path, "# test conftest").expect("Failed to create temp file");

        // Register hooks with absolute path
        registry.register(Hook {
            spec: HookSpec {
                name: "pytest_configure".to_string(),
                modifies_global_state: true,
                cacheable: true,
            },
            source: conftest_path.clone(),
            function_name: "pytest_configure".to_string(),
            line_number: 5,
            is_wrapper: false,
        });
        registry.register(Hook {
            spec: HookSpec {
                name: "pytest_collection_modifyitems".to_string(),
                modifies_global_state: false,
                cacheable: true,
            },
            source: conftest_path.clone(),
            function_name: "pytest_collection_modifyitems".to_string(),
            line_number: 15,
            is_wrapper: false,
        });

        // Query with canonicalized path
        let canonical = conftest_path.canonicalize().expect("Should canonicalize");
        let hooks = registry.get_hooks_for_file(&canonical);
        assert_eq!(
            hooks.len(),
            2,
            "Should find both hooks with canonicalized path"
        );

        // Clean up
        std::fs::remove_file(&conftest_path).ok();
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
            is_wrapper: false,
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
            is_wrapper: false,
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
            is_wrapper: false,
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
            is_wrapper: false,
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

    #[test]
    fn test_hook_effect_serialization() {
        let effect = HookEffect::ModifySysPath {
            action: SysPathAction::Prepend,
            path: "/custom/path".to_string(),
        };

        // Serialize to JSON
        let json = serde_json::to_string(&effect).expect("Should serialize");
        assert!(json.contains("ModifySysPath"));

        // Deserialize back
        let parsed: HookEffect = serde_json::from_str(&json).expect("Should deserialize");
        if let HookEffect::ModifySysPath { action, path } = parsed {
            assert_eq!(action, SysPathAction::Prepend);
            assert_eq!(path, "/custom/path");
        } else {
            panic!("Wrong variant");
        }
    }

    #[test]
    fn test_get_session_effects() {
        let mut registry = HookRegistry::new();

        // Record effects for pytest_configure (session-level hook)
        registry.record_effect(
            "pytest_configure",
            HookEffect::SetEnv {
                key: "TEST_VAR".to_string(),
                value: "test_value".to_string(),
            },
        );
        registry.record_effect(
            "pytest_configure",
            HookEffect::ModifySysPath {
                action: SysPathAction::Append,
                path: "/test/path".to_string(),
            },
        );

        // Record effects for a non-session-level hook (should NOT be included)
        registry.record_effect(
            "pytest_runtest_setup",
            HookEffect::SetEnv {
                key: "PER_TEST_VAR".to_string(),
                value: "should_not_appear".to_string(),
            },
        );

        // Get session effects - should only include pytest_configure effects
        let session_effects = registry.get_session_effects();

        assert_eq!(session_effects.len(), 2);

        // Verify the effects are from pytest_configure
        let has_env_effect = session_effects.iter().any(|e| {
            matches!(
                e,
                HookEffect::SetEnv { key, value }
                    if key == "TEST_VAR" && value == "test_value"
            )
        });
        assert!(
            has_env_effect,
            "Should have SetEnv effect from pytest_configure"
        );

        let has_path_effect = session_effects.iter().any(|e| {
            matches!(
                e,
                HookEffect::ModifySysPath { action, path }
                    if *action == SysPathAction::Append && path == "/test/path"
            )
        });
        assert!(
            has_path_effect,
            "Should have ModifySysPath effect from pytest_configure"
        );
    }
}
