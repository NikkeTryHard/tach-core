//! Asyncio configuration parsing from pyproject.toml
//!
//! This module handles parsing pytest-asyncio configuration options
//! from pyproject.toml files.

use anyhow::{Context, Result};
use std::fs;
use std::path::Path;
use toml::Value;

/// Asyncio configuration parsed from pyproject.toml
#[derive(Debug, Clone, PartialEq)]
pub struct AsyncioConfig {
    /// The asyncio_mode value: "auto", "strict", or empty
    pub asyncio_mode: String,
    /// Whether auto_mode is enabled (asyncio_mode == "auto")
    pub auto_mode: bool,
    /// Default loop scope for fixtures
    pub loop_scope: String,
}

impl Default for AsyncioConfig {
    fn default() -> Self {
        Self {
            asyncio_mode: "strict".to_string(),
            auto_mode: false,
            loop_scope: "function".to_string(),
        }
    }
}

/// Parse asyncio configuration from pyproject.toml
///
/// Looks for `[tool.pytest.ini_options]` section and extracts:
/// - `asyncio_mode`: The pytest-asyncio mode ("auto" or "strict")
/// - `asyncio_default_fixture_loop_scope`: Default event loop scope for fixtures
///
/// # Arguments
/// * `project_dir` - Path to the project directory containing pyproject.toml
///
/// # Returns
/// * `AsyncioConfig` with parsed values, or defaults if not found
///
/// # Note
/// This duplicates logic in `src/tach_harness.py::_configure_asyncio_from_pyproject()`.
/// Both are needed: Python runs in Zygote before Rust effects are wired up.
pub fn parse_asyncio_config(project_dir: &Path) -> Result<AsyncioConfig> {
    let pyproject_path = project_dir.join("pyproject.toml");

    if !pyproject_path.exists() {
        return Ok(AsyncioConfig::default());
    }

    let content = fs::read_to_string(&pyproject_path).context("Failed to read pyproject.toml")?;

    let parsed: Value = toml::from_str(&content).context("Failed to parse pyproject.toml")?;

    let mut config = AsyncioConfig::default();

    // Navigate to [tool.pytest.ini_options]
    if let Some(tool) = parsed.get("tool")
        && let Some(pytest) = tool.get("pytest")
        && let Some(ini_options) = pytest.get("ini_options")
    {
        // Parse asyncio_mode
        if let Some(mode) = ini_options.get("asyncio_mode")
            && let Some(mode_str) = mode.as_str()
        {
            config.asyncio_mode = mode_str.to_string();
            config.auto_mode = mode_str == "auto";
        }

        // Parse asyncio_default_fixture_loop_scope
        if let Some(scope) = ini_options.get("asyncio_default_fixture_loop_scope")
            && let Some(scope_str) = scope.as_str()
        {
            config.loop_scope = scope_str.to_string();
        }
    }

    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_default_config() {
        let config = AsyncioConfig::default();
        assert_eq!(config.asyncio_mode, "strict");
        assert!(!config.auto_mode);
        assert_eq!(config.loop_scope, "function");
    }

    #[test]
    fn test_parse_empty_pyproject() {
        let temp = TempDir::new().unwrap();
        let pyproject = temp.path().join("pyproject.toml");
        fs::write(&pyproject, "").unwrap();

        let config = parse_asyncio_config(temp.path()).unwrap();
        assert_eq!(config, AsyncioConfig::default());
    }

    #[test]
    fn test_parse_malformed_toml() {
        let temp = TempDir::new().unwrap();
        let pyproject = temp.path().join("pyproject.toml");
        fs::write(&pyproject, "this is not valid { toml").unwrap();

        let result = parse_asyncio_config(temp.path());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("parse"));
    }
}
