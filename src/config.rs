//! Configuration Loader
//! - Reads pyproject.toml for environment variables (pytest-env replacement)
//! - Provides CLI argument parsing with clap (Phase 5.1)

use clap::{Parser, Subcommand, ValueEnum};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

// =============================================================================
// CLI Configuration (Phase 5.1)
// =============================================================================

/// Output format for tach results
#[derive(ValueEnum, Clone, Debug, Default, PartialEq)]
pub enum OutputFormat {
    /// Human-readable CLI output (to stderr)
    #[default]
    Human,
    /// Machine-readable NDJSON (to stdout)
    Json,
}

/// Tach CLI - Fast Python Test Runner
#[derive(Parser)]
#[command(name = "tach", version, about = "Fast Python Test Runner")]
pub struct Cli {
    /// Output format (also: TACH_FORMAT env var)
    #[arg(long, value_enum, default_value_t = OutputFormat::Human, env = "TACH_FORMAT")]
    pub format: OutputFormat,

    /// Path to generate JUnit XML report (also: TACH_JUNIT_XML env var)
    #[arg(long, env = "TACH_JUNIT_XML")]
    pub junit_xml: Option<std::path::PathBuf>,

    /// Watch for changes and re-run tests automatically
    #[arg(long, short = 'w')]
    pub watch: bool,

    /// Disable filesystem and network isolation (runs without CAP_SYS_ADMIN)
    #[arg(long)]
    pub no_isolation: bool,

    /// Test directory or file pattern
    #[arg(default_value = ".")]
    pub path: String,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

/// Subcommands
#[derive(Subcommand, Clone)]
pub enum Commands {
    /// Run tests (default if no subcommand)
    Test,
    /// List discovered tests without running
    List,
}

// =============================================================================
// PyProject Configuration
// =============================================================================

#[derive(Deserialize, Default)]
struct PyProject {
    tool: Option<ToolConfig>,
}

#[derive(Deserialize, Default)]
struct ToolConfig {
    pytest_env: Option<HashMap<String, String>>,
}

/// Load environment variables from pyproject.toml and apply to current process.
///
/// This function reads `[tool.pytest_env]` section from pyproject.toml and
/// sets each key-value pair as an environment variable. Must be called
/// BEFORE forking the Zygote so workers inherit the environment.
pub fn load_env_from_pyproject(root: &Path) {
    let config_path = root.join("pyproject.toml");
    if !config_path.exists() {
        return;
    }

    let contents = match fs::read_to_string(&config_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[config] Failed to read pyproject.toml: {}", e);
            return;
        }
    };

    let pyproject: PyProject = match toml::from_str(&contents) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[config] Failed to parse pyproject.toml: {}", e);
            return;
        }
    };

    if let Some(tool) = pyproject.tool {
        if let Some(env_vars) = tool.pytest_env {
            for (key, value) in env_vars {
                std::env::set_var(&key, &value);
                eprintln!("[config] Set env: {}={}", key, value);
            }
        }
    }
}

// =============================================================================
// Unit Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn test_parse_pyproject_with_pytest_env() {
        let toml_content = r#"
[tool.pytest_env]
FOO = "bar"
BAZ = "123"
"#;
        let pyproject: PyProject = toml::from_str(toml_content).unwrap();
        let env_vars = pyproject.tool.unwrap().pytest_env.unwrap();
        assert_eq!(env_vars.get("FOO"), Some(&"bar".to_string()));
        assert_eq!(env_vars.get("BAZ"), Some(&"123".to_string()));
    }

    #[test]
    fn test_parse_pyproject_without_pytest_env() {
        let toml_content = r#"
[tool.other]
key = "value"
"#;
        let pyproject: PyProject = toml::from_str(toml_content).unwrap();
        assert!(pyproject.tool.is_some());
        // pytest_env should be None
    }

    #[test]
    fn test_parse_empty_pyproject() {
        let toml_content = "";
        let pyproject: PyProject = toml::from_str(toml_content).unwrap();
        assert!(pyproject.tool.is_none());
    }

    #[test]
    fn test_load_env_from_pyproject_sets_env_vars() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("pyproject.toml");

        let toml_content = r#"
[tool.pytest_env]
TEST_COVERAGE_VAR_1 = "value1"
TEST_COVERAGE_VAR_2 = "value2"
"#;
        std::fs::write(&config_path, toml_content).unwrap();

        // Load and verify env vars are set
        load_env_from_pyproject(temp_dir.path());

        assert_eq!(std::env::var("TEST_COVERAGE_VAR_1").unwrap(), "value1");
        assert_eq!(std::env::var("TEST_COVERAGE_VAR_2").unwrap(), "value2");

        // Cleanup
        std::env::remove_var("TEST_COVERAGE_VAR_1");
        std::env::remove_var("TEST_COVERAGE_VAR_2");
    }

    #[test]
    fn test_load_env_nonexistent_file() {
        let temp_dir = TempDir::new().unwrap();
        // Don't create any file - should return early without error
        load_env_from_pyproject(temp_dir.path());
    }

    #[test]
    fn test_load_env_no_tool_section() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("pyproject.toml");

        let toml_content = r#"
[project]
name = "myproject"
"#;
        std::fs::write(&config_path, toml_content).unwrap();

        // Should complete without error
        load_env_from_pyproject(temp_dir.path());
    }

    #[test]
    fn test_load_env_no_pytest_env_section() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("pyproject.toml");

        let toml_content = r#"
[tool.black]
line-length = 100
"#;
        std::fs::write(&config_path, toml_content).unwrap();

        // Should complete without error
        load_env_from_pyproject(temp_dir.path());
    }

    #[test]
    fn test_parse_pyproject_with_multiple_tool_sections() {
        let toml_content = r#"
[tool.black]
line-length = 100

[tool.pytest_env]
DB_URL = "sqlite:///:memory:"

[tool.ruff]
select = ["E", "F"]
"#;
        let pyproject: PyProject = toml::from_str(toml_content).unwrap();
        let env_vars = pyproject.tool.unwrap().pytest_env.unwrap();
        assert_eq!(
            env_vars.get("DB_URL"),
            Some(&"sqlite:///:memory:".to_string())
        );
    }

    #[test]
    fn test_parse_pyproject_empty_pytest_env() {
        let toml_content = r#"
[tool.pytest_env]
"#;
        let pyproject: PyProject = toml::from_str(toml_content).unwrap();
        let env_vars = pyproject.tool.unwrap().pytest_env.unwrap();
        assert!(env_vars.is_empty());
    }
}
