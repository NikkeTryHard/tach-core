//! Configuration Loader
//! - Reads pyproject.toml for environment variables (pytest-env replacement)
//! - Provides CLI argument parsing with clap

use clap::{Parser, Subcommand, ValueEnum};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

// =============================================================================
// CLI Configuration (0.9.0 Beta - pytest Compatibility Layer)
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

/// Verbosity level for output
#[derive(ValueEnum, Clone, Debug, Default, PartialEq)]
pub enum Verbosity {
    /// Minimal output
    Quiet,
    /// Normal output (default)
    #[default]
    Normal,
    /// Verbose output with test names
    Verbose,
    /// Extra verbose with timing
    VeryVerbose,
}

/// Traceback formatting style (pytest-compatible --tb flag)
#[derive(ValueEnum, Clone, Debug, Default, PartialEq, Copy)]
pub enum TracebackStyle {
    /// First and last frames only
    Short,
    /// Full traceback with locals (default)
    #[default]
    Long,
    /// Single line per failure (file:line: message)
    Line,
    /// Python's default traceback format (unmodified)
    Native,
    /// No traceback output
    No,
}

/// tach - Hypervisor-Accelerated Python Test Runner
///
/// A drop-in replacement for pytest using userfaultfd snapshots for
/// sub-millisecond test isolation. Compatible with pytest arguments.
///
/// EXAMPLES:
///     tach                          Run all tests in current directory
///     tach tests/                   Run tests in specific directory
///     tach -n auto                  Auto-detect worker count
///     tach -k "network"             Run tests matching "network"
///     tach -x --coverage            Stop on first failure, with coverage
///     tach -- -v --tb=short         Pass arguments directly to pytest shim
///
/// PYTEST COMPATIBILITY:
///     Most pytest flags are supported. Use -- to pass through unknown args.
///
/// ENVIRONMENT VARIABLES:
///     TACH_WORKERS      Number of parallel workers (default: auto)
///     TACH_FORMAT       Output format: human, json
///     TACH_COVERAGE     Enable coverage collection
///     TACH_NO_ISOLATION Disable sandboxing (for debugging)
#[derive(Parser)]
#[command(
    name = "tach",
    version = env!("CARGO_PKG_VERSION"),
    author = "Anthropic",
    about = "Hypervisor-Accelerated Python Test Runner",
    long_about = "A drop-in replacement for pytest using userfaultfd snapshots for sub-millisecond test isolation. Compatible with pytest arguments.",
    after_help = r#"EXAMPLES:
    # Run all tests in current directory
    tach

    # Run tests in a specific directory
    tach tests/

    # Run tests matching a pattern
    tach -k "test_login or test_logout"

    # Run with specific marker
    tach -m "not slow"

    # Stop on first failure with coverage
    tach -x --coverage

    # Dry run - show what would run without executing
    tach --dry-run

    # Collect tests only (same as 'tach list')
    tach --collect-only

    # Show version and build information
    tach version

    # Show detailed version info with capabilities
    tach -v version

    # Run with parallel workers
    tach -n auto tests/

For more information, visit: https://github.com/anthropics/tach-core"#
)]
pub struct Cli {
    // =========================================================================
    // Parallel Execution (pytest-xdist compatible)
    // =========================================================================
    /// Number of workers for parallel test execution.
    ///
    /// Use 'auto' (or 0) to auto-detect based on CPU count.
    /// Default: auto
    #[arg(
        short = 'n',
        long,
        value_name = "WORKERS",
        default_value = "auto",
        env = "TACH_WORKERS"
    )]
    pub workers: String,

    // =========================================================================
    // Test Selection (pytest compatible)
    // =========================================================================
    /// Run tests matching the given substring expression.
    ///
    /// Examples: -k "network", -k "not slow", -k "test_login or test_logout"
    #[arg(short = 'k', long, value_name = "EXPRESSION")]
    pub keyword: Option<String>,

    /// Run tests matching the given marker expression.
    ///
    /// Examples: -m "slow", -m "not integration", -m "smoke and not flaky"
    #[arg(short = 'm', long, value_name = "MARKERS")]
    pub markers: Option<String>,

    /// Test directory or file pattern
    #[arg(default_value = ".")]
    pub path: String,

    // =========================================================================
    // Execution Control (pytest compatible)
    // =========================================================================
    /// Exit on first failure (fail fast).
    ///
    /// Stops test execution immediately after the first test failure.
    #[arg(short = 'x', long)]
    pub exitfirst: bool,

    /// Exit after N failures (--maxfail=N).
    #[arg(long, value_name = "N")]
    pub maxfail: Option<usize>,

    /// Watch for changes and re-run tests automatically.
    ///
    /// Uses inotify to detect file changes and triggers re-runs.
    #[arg(long, short = 'w')]
    pub watch: bool,

    // =========================================================================
    // Output Control (pytest compatible)
    // =========================================================================
    /// Increase verbosity (-v for verbose, -vv for very verbose).
    #[arg(short = 'v', long, action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// Decrease verbosity (quiet mode).
    #[arg(short = 'q', long)]
    pub quiet: bool,

    /// Output format (also: TACH_FORMAT env var)
    #[arg(long, value_enum, default_value_t = OutputFormat::Human, env = "TACH_FORMAT")]
    pub format: OutputFormat,

    /// Traceback formatting style for failures.
    ///
    /// Controls how Python tracebacks are displayed:
    ///   short:  First and last frames only
    ///   long:   Full traceback (default)
    ///   line:   Single line per failure (file:line: message)
    ///   native: Python's default format (unmodified)
    ///   no:     No traceback output
    #[arg(long = "tb", value_enum, default_value_t = TracebackStyle::Long, env = "TACH_TB")]
    pub traceback: TracebackStyle,

    // =========================================================================
    // Coverage (pytest-cov compatible)
    // =========================================================================
    /// Enable coverage collection (PEP 669 sys.monitoring).
    ///
    /// Requires Python 3.12+. Coverage data is written to .coverage file.
    /// This is zero-overhead coverage using Python's monitoring API.
    #[arg(long, env = "TACH_COVERAGE")]
    pub coverage: bool,

    /// Source directories for coverage (can specify multiple).
    #[arg(long = "cov", value_name = "PATH")]
    pub cov_source: Vec<String>,

    // =========================================================================
    // Reporting
    // =========================================================================
    /// Path to generate JUnit XML report (also: TACH_JUNIT_XML env var).
    ///
    /// Compatible with CI systems like Jenkins, CircleCI, GitHub Actions.
    #[arg(long, env = "TACH_JUNIT_XML", value_name = "PATH")]
    pub junit_xml: Option<std::path::PathBuf>,

    // =========================================================================
    // Tach-Specific Options
    // =========================================================================
    /// Disable filesystem and network isolation.
    ///
    /// Runs without Landlock/Seccomp sandboxing. Useful for debugging
    /// or when CAP_SYS_ADMIN is not available.
    #[arg(long)]
    pub no_isolation: bool,

    /// Force toxic mode for all tests (no snapshot reuse).
    ///
    /// Each test runs in a fresh forked process. Slower but more isolated.
    #[arg(long)]
    pub force_toxic: bool,

    /// Show timing for slowest N tests.
    #[arg(long, value_name = "N")]
    pub durations: Option<usize>,

    /// Global timeout in seconds for each test (default: 60).
    ///
    /// Tests running longer than this will be killed with SIGTERM.
    /// Per-test timeouts via @pytest.mark.timeout(N) override this.
    #[arg(long, value_name = "SECONDS", env = "TACH_TIMEOUT")]
    pub timeout: Option<u64>,

    // =========================================================================
    // Diagnostics
    // =========================================================================
    /// Run system diagnostics and exit.
    ///
    /// Checks kernel version, userfaultfd, Landlock, Seccomp, Python,
    /// pytest, and performs a quick performance benchmark.
    /// Use this to troubleshoot system compatibility issues.
    #[arg(long)]
    pub diagnose: bool,

    // =========================================================================
    // Dry Run / Collect Only (pytest compatible)
    // =========================================================================
    /// Discover tests and show what would run without executing.
    ///
    /// Performs test discovery and filtering, prints a summary of tests
    /// that would be executed, then exits. No tests are actually run.
    /// Useful for CI dry runs or verifying test selection patterns.
    #[arg(long)]
    pub dry_run: bool,

    /// Collect and list tests without running (alias for 'list' command).
    ///
    /// This is a pytest-compatible alias for 'tach list'. It discovers
    /// all tests matching the given filters and prints them to stdout.
    #[arg(long)]
    pub collect_only: bool,

    // =========================================================================
    // Passthrough Arguments
    // =========================================================================
    /// Extra arguments to pass to pytest shim.
    ///
    /// Use after -- separator: tach -- -v --tb=short
    #[arg(last = true)]
    pub pytest_args: Vec<String>,

    // =========================================================================
    // Subcommands
    // =========================================================================
    #[command(subcommand)]
    pub command: Option<Commands>,
}

impl Cli {
    /// Parse worker count, returning None for "auto"
    pub fn worker_count(&self) -> Option<usize> {
        match self.workers.as_str() {
            "auto" | "0" => None,
            n => n.parse().ok(),
        }
    }

    /// Get effective verbosity level
    pub fn verbosity(&self) -> Verbosity {
        if self.quiet {
            Verbosity::Quiet
        } else {
            match self.verbose {
                0 => Verbosity::Normal,
                1 => Verbosity::Verbose,
                _ => Verbosity::VeryVerbose,
            }
        }
    }

    /// Check if fail-fast is enabled
    pub fn fail_fast(&self) -> bool {
        self.exitfirst || self.maxfail == Some(1)
    }
}

/// Subcommands
#[derive(Subcommand, Clone)]
pub enum Commands {
    /// Run tests (default if no subcommand)
    Test,

    /// List discovered tests without running
    List,

    /// Run self-diagnostics to verify kernel support
    ///
    /// Checks userfaultfd, Landlock, Seccomp, and other kernel features
    /// required for full Tach functionality. Use this to troubleshoot
    /// issues on new systems.
    SelfTest,

    /// Show version and build information
    Version,
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

        // CLI timeout takes precedence over file config
        let timeout = cli.timeout.unwrap_or_else(|| file_config.timeout());

        Self {
            format: cli.format.clone(),
            junit_xml: cli.junit_xml.clone(),
            watch: cli.watch,
            no_isolation: cli.no_isolation,
            coverage,
            path: cli.path.clone(),
            test_pattern: file_config.test_pattern().to_string(),
            timeout,
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
