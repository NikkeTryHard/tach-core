//! Hook registry for tracking and dispatching pytest hooks.

use std::collections::HashMap;
use std::path::PathBuf;

/// Specification for a pytest hook
#[derive(Debug, Clone)]
pub struct HookSpec {
    /// Hook name (e.g., "pytest_configure")
    pub name: String,
    /// Whether this hook can modify global state (affects toxicity)
    pub modifies_global_state: bool,
    /// Whether hook results should be cached
    pub cacheable: bool,
}

/// A registered hook implementation
#[derive(Debug, Clone)]
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
#[derive(Debug, Clone)]
pub enum HookEffect {
    /// Hook set an environment variable
    SetEnv { key: String, value: String },
    /// Hook modified sys.path
    ModifySysPath { action: String, path: String },
    /// Hook registered a marker
    RegisterMarker { name: String, description: String },
    /// Hook modified test collection
    ModifyItems {
        removed: Vec<String>,
        reordered: bool,
    },
    /// Hook has no observable effects
    NoEffect,
}

/// Registry of all discovered hooks
#[derive(Debug, Default)]
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

    /// Number of registered hooks
    pub fn hook_count(&self) -> usize {
        self.hooks.values().map(|v| v.len()).sum()
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
        assert!(specs.contains_key("pytest_configure"));
        assert!(specs.contains_key("pytest_runtest_setup"));
    }
}
