//! Hook Caller - Rust-side orchestration of hook execution
//!
//! This module provides the HookCaller struct which coordinates hook execution
//! across multiple conftest.py files. It uses PyO3 to call into the Python
//! `call_hook_impl()` function and aggregates results based on strategy.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use pyo3::prelude::*;
use pyo3::types::PyDict;

use super::{AggregationStrategy, Hook, HookEffect, HookRegistry, HookResult, aggregate_results};

/// Orchestrates hook execution across multiple conftest.py files
///
/// The HookCaller is responsible for:
/// 1. Ordering hooks by conftest hierarchy (root -> leaf)
/// 2. Calling each hook implementation via Python bridge
/// 3. Aggregating results based on the specified strategy
/// 4. Converting Python results back to Rust types
#[derive(Debug)]
pub struct HookCaller<'a> {
    /// Reference to the hook registry
    registry: &'a HookRegistry,
    /// Project root directory for path resolution
    project_root: PathBuf,
}

impl<'a> HookCaller<'a> {
    /// Create a new HookCaller with the given registry and project root
    pub fn new(registry: &'a HookRegistry, project_root: impl Into<PathBuf>) -> Self {
        Self {
            registry,
            project_root: project_root.into(),
        }
    }

    /// Call a hook for a specific test path
    ///
    /// This method:
    /// 1. Resolves all hooks that apply to the test path
    /// 2. Orders them by conftest hierarchy (root first)
    /// 3. Calls each hook and collects results
    /// 4. Aggregates results based on the provided strategy
    ///
    /// # Arguments
    /// * `hook_name` - Name of the hook to call (e.g., "pytest_configure")
    /// * `test_path` - Path to the test file
    /// * `args` - Arguments to pass to the hook
    /// * `strategy` - How to aggregate results from multiple hooks
    ///
    /// # Returns
    /// Aggregated HookResult from all applicable hooks
    pub fn call_hook(
        &self,
        hook_name: &str,
        test_path: &Path,
        args: &[(&str, &str)],
        strategy: AggregationStrategy,
    ) -> Result<HookResult> {
        // Resolve hooks in hierarchy order (root -> leaf)
        let hooks = self
            .registry
            .resolve_hooks_for_path(test_path, &self.project_root);

        // Filter to only hooks matching the requested name
        let matching_hooks: Vec<&Hook> =
            hooks.iter().filter(|h| h.spec.name == hook_name).collect();

        if matching_hooks.is_empty() {
            return Ok(HookResult::new());
        }

        // Call each hook and collect results
        let mut results = Vec::with_capacity(matching_hooks.len());

        for hook in matching_hooks {
            let result = self.call_single_hook(hook, args)?;
            results.push(result);
        }

        // Aggregate results based on strategy
        Ok(aggregate_results(&results, strategy))
    }

    /// Call all hooks of a given name (session-level hooks)
    ///
    /// Unlike `call_hook`, this method calls ALL registered hooks of the given name,
    /// not just those applicable to a specific test path. This is used for
    /// session-level hooks like `pytest_configure` that run once per session.
    ///
    /// # Arguments
    /// * `hook_name` - Name of the hook to call
    /// * `args` - Arguments to pass to the hook
    /// * `strategy` - How to aggregate results
    ///
    /// # Returns
    /// Aggregated HookResult from all hooks
    pub fn call_all_hooks(
        &self,
        hook_name: &str,
        args: &[(&str, &str)],
        strategy: AggregationStrategy,
    ) -> Result<HookResult> {
        let hooks = self.registry.get_hooks(hook_name);

        if hooks.is_empty() {
            return Ok(HookResult::new());
        }

        // Sort hooks by source path depth (root first)
        let mut sorted_hooks: Vec<&Hook> = hooks.iter().collect();
        sorted_hooks.sort_by_key(|h| h.source.components().count());

        let mut results = Vec::with_capacity(sorted_hooks.len());

        for hook in sorted_hooks {
            let result = self.call_single_hook(hook, args)?;
            results.push(result);
        }

        Ok(aggregate_results(&results, strategy))
    }

    /// Call a single hook implementation via Python bridge
    fn call_single_hook(&self, hook: &Hook, args: &[(&str, &str)]) -> Result<HookResult> {
        Python::attach(|py| self.call_hook_python(py, hook, args))
    }

    /// Python GIL-holding implementation of hook calling
    fn call_hook_python(
        &self,
        py: Python<'_>,
        hook: &Hook,
        args: &[(&str, &str)],
    ) -> Result<HookResult> {
        // Import the tach_harness module
        let harness = py
            .import("tach_harness")
            .context("Failed to import tach_harness module")?;

        // Get the call_hook_impl function
        let call_hook_impl = harness
            .getattr("call_hook_impl")
            .context("Failed to get call_hook_impl function")?;

        // Build the args dictionary
        let py_args = PyDict::new(py);
        for (key, value) in args {
            py_args.set_item(*key, *value)?;
        }

        // Call the Python function
        let conftest_path = hook.source.to_string_lossy();
        let result = call_hook_impl
            .call1((conftest_path.as_ref(), &hook.function_name, py_args))
            .context("Failed to call hook implementation")?;

        // Parse the result dictionary
        self.parse_python_result(py, result, &hook.source)
    }

    /// Parse Python result dictionary into HookResult
    fn parse_python_result(
        &self,
        _py: Python<'_>,
        result: Bound<'_, PyAny>,
        source: &Path,
    ) -> Result<HookResult> {
        let mut hook_result = HookResult::new();
        hook_result.source = Some(source.to_path_buf());

        // Extract return_value - get_item returns Result<Bound<PyAny>>
        if let Ok(return_value) = result.get_item("return_value")
            && !return_value.is_none()
        {
            hook_result.return_value = return_value.extract::<String>().ok();
        }

        // Extract error
        if let Ok(error) = result.get_item("error")
            && !error.is_none()
        {
            hook_result.error = error.extract::<String>().ok();
        }

        // Extract effects
        if let Ok(effects) = result.get_item("effects")
            && let Ok(effects_list) = effects.extract::<Vec<Bound<'_, PyAny>>>()
        {
            for effect in effects_list {
                if let Ok(parsed_effect) = self.parse_effect(&effect) {
                    hook_result.effects.push(parsed_effect);
                }
            }
        }

        Ok(hook_result)
    }

    /// Parse a single effect dictionary from Python
    fn parse_effect(&self, effect: &Bound<'_, PyAny>) -> Result<HookEffect> {
        let effect_type: String = effect
            .get_item("type")
            .context("Missing effect type")?
            .extract()?;

        match effect_type.as_str() {
            "SetEnv" => {
                let key: String = effect.get_item("key").context("Missing key")?.extract()?;
                let value: String = effect
                    .get_item("value")
                    .context("Missing value")?
                    .extract()?;
                Ok(HookEffect::SetEnv { key, value })
            }
            "ModifySysPath" => {
                let action_str: String = effect
                    .get_item("action")
                    .context("Missing action")?
                    .extract()?;
                let path: String = effect.get_item("path").context("Missing path")?.extract()?;

                let action = match action_str.as_str() {
                    "prepend" => super::SysPathAction::Prepend,
                    "append" => super::SysPathAction::Append,
                    "remove" => super::SysPathAction::Remove,
                    _ => super::SysPathAction::Append,
                };

                Ok(HookEffect::ModifySysPath { action, path })
            }
            _ => Ok(HookEffect::NoEffect),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hook_caller_creation() {
        let registry = HookRegistry::new();
        let caller = HookCaller::new(&registry, "/project");

        assert_eq!(caller.project_root, PathBuf::from("/project"));
    }

    #[test]
    fn test_call_hook_empty_registry() {
        let registry = HookRegistry::new();
        let caller = HookCaller::new(&registry, "/project");

        let result = caller
            .call_hook(
                "pytest_configure",
                Path::new("/project/tests/test_foo.py"),
                &[],
                AggregationStrategy::NoReturn,
            )
            .expect("Should succeed with empty registry");

        assert!(result.return_value.is_none());
        assert!(result.effects.is_empty());
        assert!(result.error.is_none());
    }

    #[test]
    fn test_call_all_hooks_empty_registry() {
        let registry = HookRegistry::new();
        let caller = HookCaller::new(&registry, "/project");

        let result = caller
            .call_all_hooks("pytest_configure", &[], AggregationStrategy::NoReturn)
            .expect("Should succeed with empty registry");

        assert!(result.return_value.is_none());
        assert!(result.effects.is_empty());
    }
}
