//! Plugin Shim Registry
//!
//! Tracks known pytest plugins, their compatibility status,
//! and provides shims for common plugin behaviors.

use std::collections::{HashMap, HashSet};

/// Status of a pytest plugin with respect to Tach
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginStatus {
    /// Plugin is fully supported
    Supported { description: String },
    /// Plugin is partially supported with limitations
    Partial {
        description: String,
        limitations: Vec<String>,
    },
    /// Plugin functionality is superseded by Tach
    Superseded {
        description: String,
        tach_equivalent: String,
    },
    /// Plugin is known to be incompatible
    Incompatible { reason: String },
    /// Plugin is unknown (may or may not work)
    Unknown,
}

/// Registry of pytest plugins and their compatibility
#[derive(Debug)]
pub struct PluginRegistry {
    plugins: HashMap<String, PluginStatus>,
    disabled: HashSet<String>,
    priority: Vec<String>,
}

impl PluginRegistry {
    #[must_use]
    pub fn new() -> Self {
        let mut registry = Self {
            plugins: HashMap::new(),
            disabled: HashSet::new(),
            priority: Vec::new(),
        };

        registry.register_known_plugins();
        registry
    }

    fn register_known_plugins(&mut self) {
        // Supported plugins
        self.plugins.insert(
            "pytest-django".to_string(),
            PluginStatus::Supported {
                description: "Django test support via marker detection".to_string(),
            },
        );
        self.plugins.insert(
            "pytest-asyncio".to_string(),
            PluginStatus::Supported {
                description: "Async test support via is_async detection".to_string(),
            },
        );
        self.plugins.insert(
            "pytest-mock".to_string(),
            PluginStatus::Supported {
                description: "Mocking fixtures work normally".to_string(),
            },
        );
        self.plugins.insert(
            "pytest-env".to_string(),
            PluginStatus::Supported {
                description: "Environment variables captured via effect recording".to_string(),
            },
        );
        self.plugins.insert(
            "pytest-randomly".to_string(),
            PluginStatus::Supported {
                description: "Test randomization works normally".to_string(),
            },
        );
        self.plugins.insert(
            "pytest-timeout".to_string(),
            PluginStatus::Partial {
                description: "Timeout support".to_string(),
                limitations: vec![
                    "Use Tach's native --timeout flag for better integration".to_string(),
                ],
            },
        );

        // Superseded plugins
        self.plugins.insert(
            "pytest-xdist".to_string(),
            PluginStatus::Superseded {
                description: "Parallel test execution".to_string(),
                tach_equivalent: "Tach's native -n flag with worker pool".to_string(),
            },
        );
        self.plugins.insert(
            "pytest-forked".to_string(),
            PluginStatus::Superseded {
                description: "Forked test execution".to_string(),
                tach_equivalent: "Tach's zygote/fork model".to_string(),
            },
        );
        self.plugins.insert(
            "pytest-parallel".to_string(),
            PluginStatus::Superseded {
                description: "Parallel test execution".to_string(),
                tach_equivalent: "Tach's native parallelism".to_string(),
            },
        );
        self.plugins.insert(
            "pytest-cov".to_string(),
            PluginStatus::Superseded {
                description: "Coverage collection".to_string(),
                tach_equivalent: "Tach's --coverage flag with PEP 669".to_string(),
            },
        );

        // Incompatible plugins
        self.plugins.insert(
            "pytest-sugar".to_string(),
            PluginStatus::Incompatible {
                reason: "Terminal manipulation conflicts with Tach's progress display".to_string(),
            },
        );
    }

    /// Get the compatibility status of a plugin
    pub fn get_plugin_status(&self, plugin_name: &str) -> PluginStatus {
        self.plugins
            .get(plugin_name)
            .cloned()
            .unwrap_or(PluginStatus::Unknown)
    }

    /// Disable a plugin by name
    pub fn disable_plugin(&mut self, plugin_name: &str) {
        self.disabled.insert(plugin_name.to_string());
    }

    /// Enable a previously disabled plugin
    pub fn enable_plugin(&mut self, plugin_name: &str) {
        self.disabled.remove(plugin_name);
    }

    /// Check if a plugin is disabled
    pub fn is_disabled(&self, plugin_name: &str) -> bool {
        self.disabled.contains(plugin_name)
    }

    /// Set plugin execution priority (first = highest)
    pub fn set_priority(&mut self, order: Vec<String>) {
        self.priority = order;
    }

    /// Get plugin execution priority
    pub fn get_priority(&self) -> &[String] {
        &self.priority
    }

    /// Get all known plugin names
    pub fn known_plugins(&self) -> Vec<&String> {
        self.plugins.keys().collect()
    }

    /// Get all disabled plugin names
    pub fn disabled_plugins(&self) -> Vec<&String> {
        self.disabled.iter().collect()
    }

    /// Check if a plugin is known (has an explicit status)
    pub fn is_known(&self, plugin_name: &str) -> bool {
        self.plugins.contains_key(plugin_name)
    }

    /// Get all superseded plugins (to warn users)
    pub fn superseded_plugins(&self) -> Vec<(&String, &str)> {
        self.plugins
            .iter()
            .filter_map(|(name, status)| {
                if let PluginStatus::Superseded {
                    tach_equivalent, ..
                } = status
                {
                    Some((name, tach_equivalent.as_str()))
                } else {
                    None
                }
            })
            .collect()
    }

    /// Get all incompatible plugins (to warn users)
    pub fn incompatible_plugins(&self) -> Vec<(&String, &str)> {
        self.plugins
            .iter()
            .filter_map(|(name, status)| {
                if let PluginStatus::Incompatible { reason } = status {
                    Some((name, reason.as_str()))
                } else {
                    None
                }
            })
            .collect()
    }
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plugin_registry_known_plugins() {
        let registry = PluginRegistry::new();

        // pytest-django should be known
        let status = registry.get_plugin_status("pytest-django");
        assert!(matches!(status, PluginStatus::Supported { .. }));

        // pytest-xdist should be superseded
        let status = registry.get_plugin_status("pytest-xdist");
        assert!(matches!(status, PluginStatus::Superseded { .. }));
    }

    #[test]
    fn test_plugin_registry_unknown_plugin() {
        let registry = PluginRegistry::new();

        let status = registry.get_plugin_status("pytest-unknown-plugin");
        assert!(matches!(status, PluginStatus::Unknown));
    }

    #[test]
    fn test_plugin_registry_disabled_plugins() {
        let mut registry = PluginRegistry::new();

        registry.disable_plugin("pytest-randomly");

        assert!(registry.is_disabled("pytest-randomly"));
        assert!(!registry.is_disabled("pytest-django"));
    }

    #[test]
    fn test_plugin_registry_enable_plugin() {
        let mut registry = PluginRegistry::new();

        registry.disable_plugin("pytest-mock");
        assert!(registry.is_disabled("pytest-mock"));

        registry.enable_plugin("pytest-mock");
        assert!(!registry.is_disabled("pytest-mock"));
    }

    #[test]
    fn test_plugin_registry_priority() {
        let mut registry = PluginRegistry::new();

        let priority = vec!["pytest-django".to_string(), "pytest-mock".to_string()];
        registry.set_priority(priority.clone());

        assert_eq!(registry.get_priority(), &priority);
    }

    #[test]
    fn test_plugin_registry_superseded_plugins() {
        let registry = PluginRegistry::new();

        let superseded = registry.superseded_plugins();
        assert!(!superseded.is_empty());

        // pytest-xdist should be in the list
        assert!(superseded.iter().any(|(name, _)| *name == "pytest-xdist"));
    }

    #[test]
    fn test_plugin_registry_incompatible_plugins() {
        let registry = PluginRegistry::new();

        let incompatible = registry.incompatible_plugins();
        assert!(!incompatible.is_empty());

        // pytest-sugar should be in the list
        assert!(incompatible.iter().any(|(name, _)| *name == "pytest-sugar"));
    }

    #[test]
    fn test_plugin_registry_is_known() {
        let registry = PluginRegistry::new();

        assert!(registry.is_known("pytest-django"));
        assert!(!registry.is_known("pytest-unknown"));
    }
}
