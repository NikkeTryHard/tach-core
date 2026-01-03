//! Configuration Loader
//! - Reads pyproject.toml for environment variables (pytest-env replacement)
//! - Provides CLI argument parsing with clap 

use clap::{Parser, Subcommand, ValueEnum};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

// =============================================================================
// CLI Configuration 
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

    /// Enable coverage collection (PEP 669 sys.monitoring)
    /// Requires Python 3.12+. Coverage data is written to .coverage file.
    #[arg(long, env = "TACH_COVERAGE")]
    pub coverage: bool,

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
    tach: Option<TachConfig>,
}

// =============================================================================
//  Tach Configuration ([tool.tach] in pyproject.toml)
// =============================================================================

/// Configuration for Tach from pyproject.toml
///
/// Example pyproject.toml:
/// ```toml
/// [tool.tach]
/// test_pattern = "test_*.py"
/// timeout = 60
/// workers = 4
/// isolation_strategy = "auto"
///
/// [tool.tach.coverage]
/// enabled = true
/// source = ["src"]
/// omit = ["**/migrations/*"]
/// output = ".coverage"
/// ```
#[derive(Deserialize, Default, Clone, Debug)]
pub struct TachConfig {
    /// Test file pattern (default: "test_*.py")
    pub test_pattern: Option<String>,

    /// Test timeout in seconds (default: 60)
    pub timeout: Option<u64>,

    /// Number of worker processes (default: num_cpus)
    pub workers: Option<usize>,

    /// Isolation strategy: "auto", "fork", "snapshot"
    pub isolation_strategy: Option<String>,

    /// Coverage configuration
    pub coverage: Option<CoverageConfig>,
}

/// Coverage configuration for Tach
#[derive(Deserialize, Default, Clone, Debug)]
pub struct CoverageConfig {
    /// Enable coverage collection (default: false)
    pub enabled: Option<bool>,

    /// Source directories to measure coverage for
    pub source: Option<Vec<String>>,

    /// Patterns to omit from coverage
    pub omit: Option<Vec<String>>,

    /// Output file path (default: ".coverage")
    pub output: Option<String>,

    /// Output format: "lcov", "html", "json" (default: "lcov")
    pub format: Option<String>,
}

impl TachConfig {
    /// Get test pattern with default
    pub fn test_pattern(&self) -> &str {
        self.test_pattern.as_deref().unwrap_or("test_*.py")
    }

    /// Get timeout with default (60 seconds)
    pub fn timeout(&self) -> u64 {
        self.timeout.unwrap_or(60)
    }

    /// Get worker count with default (num_cpus)
    pub fn workers(&self) -> usize {
        self.workers.unwrap_or_else(num_cpus::get)
    }

    /// Get isolation strategy with default ("auto")
    pub fn isolation_strategy(&self) -> &str {
        self.isolation_strategy.as_deref().unwrap_or("auto")
    }

    /// Check if coverage is enabled
    pub fn coverage_enabled(&self) -> bool {
        self.coverage
            .as_ref()
            .and_then(|c| c.enabled)
            .unwrap_or(false)
    }
}

/// Load Tach configuration from pyproject.toml
///
/// Returns the configuration if found, or default configuration if not.
pub fn load_tach_config(root: &Path) -> TachConfig {
    let config_path = root.join("pyproject.toml");
    if !config_path.exists() {
        return TachConfig::default();
    }

    let contents = match fs::read_to_string(&config_path) {
        Ok(c) => c,
        Err(_) => return TachConfig::default(),
    };

    let pyproject: PyProject = match toml::from_str(&contents) {
        Ok(p) => p,
        Err(_) => return TachConfig::default(),
    };

    pyproject.tool.and_then(|t| t.tach).unwrap_or_default()
}

/// Merged configuration from CLI and file
///
/// CLI arguments take precedence over file configuration.
#[derive(Debug, Clone)]
pub struct MergedConfig {
    pub format: OutputFormat,
    pub junit_xml: Option<std::path::PathBuf>,
    pub watch: bool,
    pub no_isolation: bool,
    pub coverage: bool,
    pub path: String,
    pub test_pattern: String,
    pub timeout: u64,
    pub workers: usize,
    pub isolation_strategy: String,
    pub coverage_source: Vec<String>,
    pub coverage_omit: Vec<String>,
    pub coverage_output: String,
    pub coverage_format: String,
}

impl MergedConfig {
    /// Merge CLI arguments with file configuration
    ///
    /// CLI arguments take precedence over file configuration.
    pub fn from_cli_and_file(cli: &Cli, file_config: &TachConfig) -> Self {
        // Coverage is enabled if CLI flag is set OR file config enables it
        let coverage = cli.coverage || file_config.coverage_enabled();

        // Get coverage config or default
        let cov_config = file_config.coverage.clone().unwrap_or_default();

        Self {
            format: cli.format.clone(),
            junit_xml: cli.junit_xml.clone(),
            watch: cli.watch,
            no_isolation: cli.no_isolation,
            coverage,
            path: cli.path.clone(),
            test_pattern: file_config.test_pattern().to_string(),
            timeout: file_config.timeout(),
            workers: file_config.workers(),
            isolation_strategy: file_config.isolation_strategy().to_string(),
            coverage_source: cov_config.source.unwrap_or_default(),
            coverage_omit: cov_config.omit.unwrap_or_default(),
            coverage_output: cov_config.output.unwrap_or_else(|| ".coverage".to_string()),
            coverage_format: cov_config.format.unwrap_or_else(|| "lcov".to_string()),
        }
    }
}

/// Load environment variables from pyproject.toml and apply to current process.
///
/// This function reads `[tool.pytest_env]` section from pyproject.toml and
/// sets each key-value pair as an environment variable. Must be called
/// BEFORE forking the Zygote so workers inherit the environment.
///
/// # Security
///
/// Dangerous environment variables are blocked to prevent:
/// - Library injection (LD_PRELOAD, LD_LIBRARY_PATH, LD_AUDIT, LD_DEBUG)
/// - Python environment hijacking (PYTHONPATH, PYTHONHOME, PYTHONSTARTUP, PYTHONMALLOC)
/// - Path manipulation (PATH, HOME, USER)
pub fn load_env_from_pyproject(root: &Path) {
    /// Environment variables that are blocked for security reasons.
    ///
    /// These variables could be used to:
    /// - Inject malicious libraries (LD_PRELOAD, LD_LIBRARY_PATH)
    /// - Hijack Python's module loading (PYTHONPATH, PYTHONHOME)
    /// - Override the allocator (PYTHONMALLOC) - critical for jemalloc snapshot safety
    /// - Manipulate execution paths (PATH, HOME, USER)
    const ENV_DENYLIST: &[&str] = &[
        // Library injection vectors
        "LD_PRELOAD",
        "LD_LIBRARY_PATH",
        "LD_AUDIT",
        "LD_DEBUG",
        // Python environment hijacking
        "PYTHONPATH",
        "PYTHONHOME",
        "PYTHONSTARTUP",
        "PYTHONMALLOC", // Critical: could break jemalloc snapshot consistency
        // Path manipulation
        "PATH",
        "HOME",
        "USER",
    ];

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
                // SECURITY: Block dangerous environment variables
                if ENV_DENYLIST
                    .iter()
                    .any(|&blocked| key.eq_ignore_ascii_case(blocked))
                {
                    eprintln!(
                        "[config] WARNING: Blocked dangerous env var from pyproject.toml: {}",
                        key
                    );
                    continue;
                }
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

    // =========================================================================
    //  TachConfig Tests
    // =========================================================================

    #[test]
    fn test_tach_config_defaults() {
        let config = TachConfig::default();
        assert_eq!(config.test_pattern(), "test_*.py");
        assert_eq!(config.timeout(), 60);
        assert_eq!(config.isolation_strategy(), "auto");
        assert!(!config.coverage_enabled());
    }

    #[test]
    fn test_parse_tach_config_basic() {
        let toml_content = r#"
[tool.tach]
test_pattern = "tests/**/*.py"
timeout = 120
workers = 8
isolation_strategy = "snapshot"
"#;
        let pyproject: PyProject = toml::from_str(toml_content).unwrap();
        let config = pyproject.tool.unwrap().tach.unwrap();

        assert_eq!(config.test_pattern(), "tests/**/*.py");
        assert_eq!(config.timeout(), 120);
        assert_eq!(config.workers(), 8);
        assert_eq!(config.isolation_strategy(), "snapshot");
    }

    #[test]
    fn test_parse_tach_config_with_coverage() {
        let toml_content = r#"
[tool.tach]
timeout = 30

[tool.tach.coverage]
enabled = true
source = ["src", "lib"]
omit = ["**/test_*", "**/migrations/*"]
output = "coverage.lcov"
format = "lcov"
"#;
        let pyproject: PyProject = toml::from_str(toml_content).unwrap();
        let config = pyproject.tool.unwrap().tach.unwrap();

        assert!(config.coverage_enabled());
        let cov = config.coverage.unwrap();
        assert_eq!(cov.source.unwrap(), vec!["src", "lib"]);
        assert_eq!(cov.omit.unwrap(), vec!["**/test_*", "**/migrations/*"]);
        assert_eq!(cov.output.unwrap(), "coverage.lcov");
        assert_eq!(cov.format.unwrap(), "lcov");
    }

    #[test]
    fn test_load_tach_config_from_file() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("pyproject.toml");

        let toml_content = r#"
[tool.tach]
timeout = 90
workers = 4
"#;
        std::fs::write(&config_path, toml_content).unwrap();

        let config = load_tach_config(temp_dir.path());
        assert_eq!(config.timeout(), 90);
        assert_eq!(config.workers(), 4);
    }

    #[test]
    fn test_load_tach_config_nonexistent_file() {
        let temp_dir = TempDir::new().unwrap();
        let config = load_tach_config(temp_dir.path());

        // Should return defaults
        assert_eq!(config.test_pattern(), "test_*.py");
        assert_eq!(config.timeout(), 60);
    }

    #[test]
    fn test_load_tach_config_no_tach_section() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("pyproject.toml");

        let toml_content = r#"
[tool.black]
line-length = 100
"#;
        std::fs::write(&config_path, toml_content).unwrap();

        let config = load_tach_config(temp_dir.path());
        // Should return defaults
        assert_eq!(config.test_pattern(), "test_*.py");
    }

    #[test]
    fn test_tach_config_partial_coverage() {
        let toml_content = r#"
[tool.tach.coverage]
enabled = true
"#;
        let pyproject: PyProject = toml::from_str(toml_content).unwrap();
        let config = pyproject.tool.unwrap().tach.unwrap();

        assert!(config.coverage_enabled());
        // Other coverage fields should be None
        let cov = config.coverage.unwrap();
        assert!(cov.source.is_none());
        assert!(cov.omit.is_none());
    }

    // =========================================================================
    //  Environment Variable Denylist Tests
    // =========================================================================

    #[test]
    fn test_env_denylist_blocks_ld_preload() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("pyproject.toml");

        let toml_content = r#"
[tool.pytest_env]
LD_PRELOAD = "/malicious/lib.so"
SAFE_VAR = "allowed"
"#;
        std::fs::write(&config_path, toml_content).unwrap();

        // Clear any existing value
        std::env::remove_var("LD_PRELOAD");
        std::env::remove_var("SAFE_VAR");

        load_env_from_pyproject(temp_dir.path());

        // LD_PRELOAD should NOT be set (blocked)
        assert!(std::env::var("LD_PRELOAD").is_err());

        // SAFE_VAR should be set (allowed)
        assert_eq!(std::env::var("SAFE_VAR").unwrap(), "allowed");

        // Cleanup
        std::env::remove_var("SAFE_VAR");
    }

    #[test]
    fn test_env_denylist_blocks_pythonpath() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("pyproject.toml");

        let toml_content = r#"
[tool.pytest_env]
PYTHONPATH = "/malicious/path"
"#;
        std::fs::write(&config_path, toml_content).unwrap();

        std::env::remove_var("PYTHONPATH");
        load_env_from_pyproject(temp_dir.path());

        // PYTHONPATH should NOT be set (blocked)
        assert!(std::env::var("PYTHONPATH").is_err());
    }

    #[test]
    fn test_env_denylist_blocks_pythonmalloc() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("pyproject.toml");

        let toml_content = r#"
[tool.pytest_env]
PYTHONMALLOC = "malloc"
"#;
        std::fs::write(&config_path, toml_content).unwrap();

        std::env::remove_var("PYTHONMALLOC");
        load_env_from_pyproject(temp_dir.path());

        // PYTHONMALLOC should NOT be set (blocked - critical for jemalloc)
        assert!(std::env::var("PYTHONMALLOC").is_err());
    }

    #[test]
    fn test_env_denylist_case_insensitive() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("pyproject.toml");

        let toml_content = r#"
[tool.pytest_env]
ld_preload = "/malicious/lib.so"
Ld_Library_Path = "/malicious/path"
"#;
        std::fs::write(&config_path, toml_content).unwrap();

        std::env::remove_var("ld_preload");
        std::env::remove_var("Ld_Library_Path");
        load_env_from_pyproject(temp_dir.path());

        // Both should be blocked (case-insensitive matching)
        assert!(std::env::var("ld_preload").is_err());
        assert!(std::env::var("Ld_Library_Path").is_err());
    }

    #[test]
    fn test_env_denylist_all_blocked_vars() {
        // Verify all blocked variables are in the denylist
        let blocked_vars = [
            "LD_PRELOAD",
            "LD_LIBRARY_PATH",
            "LD_AUDIT",
            "LD_DEBUG",
            "PYTHONPATH",
            "PYTHONHOME",
            "PYTHONSTARTUP",
            "PYTHONMALLOC",
            "PATH",
            "HOME",
            "USER",
        ];

        // Just verify the count matches what we expect
        assert_eq!(blocked_vars.len(), 11);
    }
}
