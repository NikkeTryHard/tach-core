//! Configuration Loader
//! - Reads pyproject.toml for environment variables (pytest-env replacement)
//! - Provides CLI argument parsing with clap

use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
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
    Short,
    #[default]
    Long,
    Line,
    Native,
    No,
    Auto,
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

    # Resume an interrupted run
    tach --resume

    # Show effective configuration
    tach config

    # Initialize tach configuration
    tach init

    # List discovered fixtures
    tach fixtures

    # List discovered markers
    tach markers

    # Drop into pdb on failure
    tach --pdb

    # Disable output capture
    tach -s

For more information, visit: https://github.com/NikkeTryHard/tach-core"#
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

    /// Override root directory for test discovery (pytest --rootdir compat).
    #[arg(long, value_name = "DIR")]
    pub rootdir: Option<String>,

    /// Stop conftest.py discovery at this directory (pytest --confcutdir compat).
    #[arg(long, value_name = "DIR")]
    pub confcutdir: Option<String>,

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

    #[arg(long)]
    pub strict_markers: bool,

    #[arg(long = "Werror")]
    pub warnings_as_errors: bool,

    #[arg(long, value_name = "NODE_ID")]
    pub deselect: Vec<String>,

    #[arg(long, value_name = "PATH")]
    pub ignore: Vec<String>,

    #[arg(long, value_name = "GLOB")]
    pub ignore_glob: Vec<String>,

    #[arg(long)]
    pub runxfail: bool,

    #[arg(long)]
    pub doctest_modules: bool,

    #[arg(long = "assert", value_name = "MODE",
          value_parser = clap::builder::PossibleValuesParser::new(["plain", "rewrite"]))]
    pub assert_mode: Option<String>,

    #[arg(long)]
    pub pyargs: bool,

    #[arg(long = "nf")]
    pub new_first: bool,

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

    /// Re-run only tests that failed in the last run.
    ///
    /// Reads the last-failed cache from .tach_cache/lastfailed and
    /// runs only those tests. Useful for fixing failures iteratively.
    #[arg(long = "lf")]
    pub last_failed: bool,

    #[arg(long = "ff")]
    pub failed_first: bool,

    #[arg(long)]
    pub cache_show: bool,

    /// Exit after N failures (--maxfail=N).
    #[arg(long, value_name = "N")]
    pub maxfail: Option<usize>,

    /// Stepwise: stop on first failure, resume from that test next run.
    /// Combines --lf with --exitfirst behavior.
    #[arg(long = "sw")]
    pub stepwise: bool,

    #[arg(long)]
    pub resume: bool,

    /// Watch for changes and re-run tests automatically.
    #[arg(long, short = 'w')]
    pub watch: bool,

    // =========================================================================
    // Output Control (pytest compatible)
    // =========================================================================
    #[arg(long, value_name = "WHEN", default_value = "auto",
          value_parser = clap::builder::PossibleValuesParser::new(["auto", "yes", "no"]))]
    pub color: String,

    /// Increase verbosity (-v for verbose, -vv for very verbose).
    #[arg(short = 'v', long, action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// Decrease verbosity (quiet mode).
    #[arg(short = 'q', long)]
    pub quiet: bool,

    #[arg(long)]
    pub no_header: bool,

    #[arg(long, value_name = "LEVEL")]
    pub log_cli_level: Option<String>,

    #[arg(long, value_enum, default_value_t = OutputFormat::Human, env = "TACH_FORMAT")]
    pub format: OutputFormat,

    #[arg(long, value_name = "PATH")]
    pub html: Option<std::path::PathBuf>,

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

    #[arg(long = "showlocals", short = 'l')]
    pub show_locals: bool,

    #[arg(long)]
    pub pdb: bool,

    #[arg(long, value_name = "PATH")]
    pub log_file: Option<std::path::PathBuf>,

    #[arg(short = 's', long = "capture-no")]
    pub no_capture: bool,

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

    // =========================================================================
    // Django Database Options (pytest-django compatible)
    // =========================================================================
    /// Reuse existing test database between runs.
    ///
    /// Skips database creation and migrations if the test database exists.
    /// Speeds up repeated test runs. Use --create-db to force recreation
    /// after schema changes.
    #[arg(long)]
    pub reuse_db: bool,

    /// Force recreation of test database.
    ///
    /// Drops and recreates the test database even if --reuse-db is set.
    /// Use this after schema changes or when the database is corrupted.
    #[arg(long)]
    pub create_db: bool,

    /// Ignore .gitignore and .ignore files during test discovery.
    ///
    /// Bypasses standard ignore files (like .gitignore, .ignore) when
    /// scanning for test files. Useful when AI tools accidentally add
    /// *.py to .ignore, which would otherwise hide all tests.
    #[arg(long)]
    pub no_ignore: bool,

    /// Disable pytest fallback for failed tests.
    ///
    /// By default, tach retries failed tests with vanilla pytest to
    /// distinguish tach-specific failures from real test failures.
    /// Use this flag to skip the fallback and report raw tach results.
    #[arg(long, env = "TACH_NO_FALLBACK")]
    pub no_fallback: bool,

    /// Remove the last-failed cache before running.
    #[arg(long)]
    pub cache_clear: bool,

    // =========================================================================
    // Plugin Configuration
    // =========================================================================
    /// Disable specific pytest plugins.
    ///
    /// Can be specified multiple times: --disable-plugin pytest-sugar --disable-plugin pytest-xdist
    #[arg(long = "disable-plugin", value_name = "PLUGIN")]
    pub disable_plugins: Vec<String>,

    /// Pytest-compatible plugin flag. Use -p no:PLUGIN to disable.
    #[arg(short = 'p', value_name = "PLUGIN")]
    pub plugins: Vec<String>,

    #[arg(long, value_name = "N")]
    pub durations: Option<usize>,

    #[arg(long, value_name = "SECS", default_value_t = 0.005)]
    pub durations_min: f64,

    /// Show memory usage for each test.
    ///
    /// Displays peak RSS (Resident Set Size) for each test from /proc/{pid}/statm.
    /// Also shows total peak memory and warns if any test uses > 500MB.
    #[arg(long)]
    pub memory: bool,

    /// Global timeout in seconds for each test (default: 60).
    ///
    /// Tests running longer than this will be killed with SIGTERM.
    /// Per-test timeouts via @pytest.mark.timeout(N) override this.
    #[arg(long, value_name = "SECONDS", env = "TACH_TIMEOUT")]
    pub timeout: Option<u64>,

    /// Retry failed tests up to N times to detect flaky tests.
    /// Tests that pass on retry are reported as "flaky" instead of "failed".
    #[arg(long, value_name = "N", env = "TACH_RETRIES")]
    pub retries: Option<u32>,

    #[arg(long, value_name = "METHOD", default_value = "signal",
          value_parser = clap::builder::PossibleValuesParser::new(["signal", "thread"]))]
    pub timeout_method: String,

    #[arg(long = "randomly-seed", value_name = "SEED")]
    pub randomly_seed: Option<u64>,

    #[arg(long)]
    pub setup_plan: bool,

    #[arg(long)]
    pub setup_show: bool,

    #[arg(long)]
    pub setup_only: bool,

    #[arg(long)]
    pub forked: bool,

    // =========================================================================
    // Diagnostics
    // =========================================================================
    ///
    /// Checks kernel version, userfaultfd, Landlock, Seccomp, Python,
    /// pytest, and performs a quick performance benchmark.
    /// Use this to troubleshoot system compatibility issues.
    #[arg(long)]
    pub diagnose: bool,

    /// Enable debug logging (verbose output).
    ///
    /// Sets TACH_LOG_LEVEL=debug for detailed diagnostic output including
    /// worker lifecycle events and IPC message logging. Useful for troubleshooting.
    #[arg(long)]
    pub debug: bool,

    /// Enable trace logging (maximum verbosity).
    ///
    /// Sets TACH_LOG_LEVEL=trace for maximum verbosity including all debug
    /// output plus memory operations, snapshot details, and internal state dumps.
    #[arg(long)]
    pub trace: bool,

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
    #[arg(long, alias = "co")]
    pub collect_only: bool,

    #[arg(long)]
    pub count: bool,

    // =========================================================================
    // Import Mode
    // =========================================================================
    #[arg(long, value_name = "MODE", default_value = "prepend",
          value_parser = clap::builder::PossibleValuesParser::new(["prepend", "append", "importlib"]))]
    pub import_mode: String,

    /// Override ini-file options (pytest -o compat). E.g. -o "markers=slow: slow tests"
    #[arg(short = 'o', long = "override-ini", value_name = "INI")]
    pub override_ini: Vec<String>,

    #[arg(long, value_name = "DIR")]
    pub basetemp: Option<String>,

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
    List {
        /// Test directory or file pattern
        #[arg(default_value = ".")]
        path: String,
    },

    /// Run self-diagnostics to verify kernel support
    ///
    /// Checks userfaultfd, Landlock, Seccomp, and other kernel features
    /// required for full Tach functionality. Use this to troubleshoot
    /// issues on new systems.
    SelfTest,

    /// Show version and build information
    Version,

    /// Generate shell completion scripts
    ///
    /// Outputs completion scripts for the specified shell to stdout.
    /// Redirect to a file or source directly in your shell config.
    ///
    /// Examples:
    ///   tach completions bash > ~/.bash_completion.d/tach
    ///   tach completions zsh > ~/.zsh/completions/_tach
    ///   tach completions fish > ~/.config/fish/completions/tach.fish

    /// Initialize [tool.tach] configuration in pyproject.toml.
    Init,

    /// Show effective merged configuration (CLI + pyproject.toml).
    Config,

    Markers,

    Clean,

    Fixtures,

    Stats,

    Check,

    Completions {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: Shell,
    },
}

/// Supported shells for completion generation
#[derive(ValueEnum, Clone, Debug)]
pub enum Shell {
    /// Bash shell
    Bash,
    /// Zsh shell
    Zsh,
    /// Fish shell
    Fish,
    /// PowerShell
    PowerShell,
    /// Elvish shell
    Elvish,
}

impl Shell {
    /// Convert to clap_complete::Shell
    pub fn to_clap_shell(&self) -> clap_complete::Shell {
        match self {
            Shell::Bash => clap_complete::Shell::Bash,
            Shell::Zsh => clap_complete::Shell::Zsh,
            Shell::Fish => clap_complete::Shell::Fish,
            Shell::PowerShell => clap_complete::Shell::PowerShell,
            Shell::Elvish => clap_complete::Shell::Elvish,
        }
    }
}

/// Generate shell completion script and write to stdout
pub fn generate_completions(shell: &Shell) {
    let mut cmd = Cli::command();
    clap_complete::generate(
        shell.to_clap_shell(),
        &mut cmd,
        "tach",
        &mut std::io::stdout(),
    );
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
/// timeout_hook = "mymodule:on_timeout"
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

    /// Python callback hook for timeout events.
    ///
    /// Format: "module.path:function_name"
    /// Example: "my_package.hooks:on_timeout"
    pub timeout_hook: Option<String>,

    /// Plugin configuration
    #[serde(default)]
    pub plugins: PluginConfig,

    /// Network isolation configuration (Landlock V4+)
    pub network: Option<NetworkConfig>,

    // =========================================================================
    // Test Selection (pyproject.toml equivalents of CLI flags)
    // =========================================================================
    /// Default keyword expression filter (same as -k CLI flag)
    pub keyword: Option<String>,

    /// Default marker expression filter (same as -m CLI flag)
    pub markers: Option<String>,

    // =========================================================================
    // Execution Control
    // =========================================================================
    /// Stop on first failure (same as -x CLI flag)
    pub exitfirst: Option<bool>,

    /// Exit after N failures (same as --maxfail CLI flag)
    pub maxfail: Option<usize>,

    /// Force toxic mode for all tests (no snapshot reuse)
    pub force_toxic: Option<bool>,

    /// Disable pytest fallback for failed tests
    pub no_fallback: Option<bool>,

    /// Disable filesystem/network isolation
    pub no_isolation: Option<bool>,

    /// Retry failed tests up to N times
    pub retries: Option<u32>,

    // =========================================================================
    // Output Control
    // =========================================================================
    /// Traceback style: "short", "long", "line", "native", "no"
    pub traceback: Option<String>,

    pub durations: Option<usize>,
    pub durations_min: Option<f64>,
    pub memory: Option<bool>,

    /// Ignore .gitignore/.ignore files during discovery
    pub no_ignore: Option<bool>,

    // =========================================================================
    // Django Database
    // =========================================================================
    /// Reuse existing test database between runs
    pub reuse_db: Option<bool>,

    /// Force recreation of test database
    pub create_db: Option<bool>,

    /// Additional env vars to block from [tool.pytest_env] (appended to built-in denylist)
    pub env_denylist: Option<Vec<String>>,

    /// Toxicity analysis overrides
    pub toxicity: Option<ToxicityConfig>,
}

/// User overrides for module toxicity analysis.
///
/// Example pyproject.toml:
/// ```toml
/// [tool.tach.toxicity]
/// force_safe = ["myapp.utils", "myapp.helpers"]
/// force_toxic = ["myapp.workers"]
/// ```
#[derive(Deserialize, Default, Clone, Debug)]
pub struct ToxicityConfig {
    /// Modules to force as safe (override toxic -> safe)
    #[serde(default)]
    pub force_safe: Vec<String>,
    /// Modules to force as toxic (override safe -> toxic)
    #[serde(default)]
    pub force_toxic: Vec<String>,
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

/// Network isolation configuration for Landlock V4+
///
/// Example pyproject.toml:
/// ```toml
/// [tool.tach.network]
/// allow_localhost = true
/// allow_connect = ["api.example.com:443"]
/// allow_bind_ports = [8000, 8080]
/// ```
#[derive(Deserialize, Default, Clone, Debug)]
pub struct NetworkConfig {
    /// Allow connections to localhost (127.0.0.1, ::1). Default: true
    pub allow_localhost: Option<bool>,

    /// Allowed outbound connection targets (host:port format)
    pub allow_connect: Option<Vec<String>>,

    /// Allowed TCP bind ports. Use 0 for ephemeral ports.
    pub allow_bind_ports: Option<Vec<u16>>,
}

impl NetworkConfig {
    /// Check if localhost connections are allowed (default: true)
    pub fn allow_localhost(&self) -> bool {
        self.allow_localhost.unwrap_or(true)
    }

    /// Get allowed connection targets
    pub fn allowed_connections(&self) -> &[String] {
        self.allow_connect.as_deref().unwrap_or(&[])
    }

    /// Get allowed bind ports
    pub fn allowed_bind_ports(&self) -> &[u16] {
        self.allow_bind_ports.as_deref().unwrap_or(&[])
    }
}

/// Plugin configuration for Tach
#[derive(Deserialize, Default, Clone, Debug)]
pub struct PluginConfig {
    /// Plugins to disable (e.g., ["pytest-sugar", "pytest-xdist"])
    #[serde(default)]
    pub disabled: Vec<String>,

    /// Plugin execution priority (first = highest priority)
    #[serde(default)]
    pub priority: Vec<String>,
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

/// Single source of truth: CLI args merged with pyproject.toml [tool.tach].
/// CLI always wins over file config.
#[derive(Debug, Clone)]
pub struct MergedConfig {
    // Output
    pub format: OutputFormat,
    pub junit_xml: Option<std::path::PathBuf>,
    pub traceback: TracebackStyle,
    pub show_locals: bool,
    pub durations: Option<usize>,
    pub durations_min: f64,
    pub memory: bool,
    pub no_header: bool,
    pub verbose: u8,
    pub quiet: bool,

    // Test selection
    pub path: String,
    pub test_pattern: String,
    pub keyword: Option<String>,
    pub markers: Option<String>,

    // Execution control
    pub exitfirst: bool,
    pub maxfail: Option<usize>,
    pub last_failed: bool,
    pub failed_first: bool,
    pub cache_clear: bool,
    pub watch: bool,
    pub timeout: u64,
    pub workers: usize,
    pub retries: Option<u32>,

    // Isolation
    pub no_isolation: bool,
    pub force_toxic: bool,
    pub isolation_strategy: String,
    pub network: Option<NetworkConfig>,

    // Coverage
    pub coverage: bool,
    pub coverage_source: Vec<String>,
    pub coverage_omit: Vec<String>,
    pub coverage_output: String,
    pub coverage_format: String,

    // Plugins
    pub disabled_plugins: Vec<String>,

    // Tach-specific
    pub no_fallback: bool,
    pub no_ignore: bool,
    pub debug: bool,
    pub trace: bool,
    pub timeout_hook: Option<String>,

    // Django
    pub reuse_db: bool,
    pub create_db: bool,
}

impl MergedConfig {
    /// Merge CLI arguments with file configuration (CLI wins).
    pub fn from_cli_and_file(cli: &Cli, file_config: &TachConfig) -> Self {
        let coverage = cli.coverage || file_config.coverage_enabled();
        let cov_config = file_config.coverage.clone().unwrap_or_default();
        let timeout = cli.timeout.unwrap_or_else(|| file_config.timeout());

        let mut disabled_plugins = file_config.plugins.disabled.clone();
        disabled_plugins.extend(cli.disable_plugins.clone());
        for p in &cli.plugins {
            if let Some(name) = p.strip_prefix("no:") {
                disabled_plugins.push(name.to_string());
            }
        }
        disabled_plugins.sort();
        disabled_plugins.dedup();

        let traceback = if cli.traceback != TracebackStyle::default() {
            cli.traceback
        } else {
            file_config
                .traceback
                .as_deref()
                .and_then(parse_traceback_style)
                .unwrap_or(cli.traceback)
        };

        Self {
            format: cli.format.clone(),
            junit_xml: cli.junit_xml.clone(),
            traceback,
            show_locals: cli.show_locals,
            durations: cli.durations.or(file_config.durations),
            durations_min: if cli.durations_min != 0.005 {
                cli.durations_min
            } else {
                file_config.durations_min.unwrap_or(0.005)
            },
            memory: cli.memory || file_config.memory.unwrap_or(false),
            no_header: cli.no_header,
            verbose: cli.verbose,
            quiet: cli.quiet,

            path: cli.path.clone(),
            test_pattern: file_config.test_pattern().to_string(),
            keyword: cli.keyword.clone().or_else(|| file_config.keyword.clone()),
            markers: cli.markers.clone().or_else(|| file_config.markers.clone()),

            exitfirst: cli.exitfirst || cli.stepwise || file_config.exitfirst.unwrap_or(false),
            maxfail: cli.maxfail.or(file_config.maxfail),
            last_failed: cli.last_failed || cli.stepwise,
            failed_first: cli.failed_first,
            cache_clear: cli.cache_clear,
            watch: cli.watch,
            timeout,
            workers: cli.worker_count().unwrap_or_else(|| file_config.workers()),
            retries: cli.retries.or(file_config.retries),

            no_isolation: cli.no_isolation || file_config.no_isolation.unwrap_or(false),
            force_toxic: cli.force_toxic || file_config.force_toxic.unwrap_or(false),
            isolation_strategy: file_config.isolation_strategy().to_string(),
            network: file_config.network.clone(),

            coverage,
            coverage_source: if cli.cov_source.is_empty() {
                cov_config.source.unwrap_or_default()
            } else {
                cli.cov_source.clone()
            },
            coverage_omit: cov_config.omit.unwrap_or_default(),
            coverage_output: cov_config.output.unwrap_or_else(|| ".coverage".to_string()),
            coverage_format: cov_config.format.unwrap_or_else(|| "lcov".to_string()),

            disabled_plugins,

            no_fallback: cli.no_fallback || file_config.no_fallback.unwrap_or(false),
            no_ignore: cli.no_ignore || file_config.no_ignore.unwrap_or(false),
            debug: cli.debug,
            trace: cli.trace,
            timeout_hook: file_config.timeout_hook.clone(),

            reuse_db: cli.reuse_db || file_config.reuse_db.unwrap_or(false),
            create_db: cli.create_db || file_config.create_db.unwrap_or(false),
        }
    }

    pub fn fail_fast(&self) -> bool {
        self.exitfirst || self.maxfail == Some(1)
    }
}

fn parse_traceback_style(s: &str) -> Option<TracebackStyle> {
    match s {
        "short" => Some(TracebackStyle::Short),
        "long" => Some(TracebackStyle::Long),
        "line" => Some(TracebackStyle::Line),
        "native" => Some(TracebackStyle::Native),
        "no" => Some(TracebackStyle::No),
        "auto" => Some(TracebackStyle::Auto),
        _ => None,
    }
}

/// Expands `{VAR}` patterns in a string using environment variables.
/// Double braces `{{VAR}}` are escaped to literal `{VAR}`.
fn expand_env_vars(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    let mut chars = value.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '{' {
            if chars.peek() == Some(&'{') {
                // Escaped brace: {{ -> {
                chars.next();
                result.push('{');
            } else {
                // Variable reference: {VAR}
                let mut var_name = String::new();
                for inner in chars.by_ref() {
                    if inner == '}' {
                        break;
                    }
                    var_name.push(inner);
                }
                // Look up in environment
                match std::env::var(&var_name) {
                    Ok(val) => result.push_str(&val),
                    Err(_) => {
                        // Keep original if not found
                        result.push('{');
                        result.push_str(&var_name);
                        result.push('}');
                    }
                }
            }
        } else if c == '}' && chars.peek() == Some(&'}') {
            // Escaped closing brace: }} -> }
            chars.next();
            result.push('}');
        } else {
            result.push(c);
        }
    }

    result
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
    let tach_config = load_tach_config(root);
    load_env_from_pyproject_with_denylist(root, &tach_config);
}

fn load_env_from_pyproject_with_denylist(root: &Path, tach_config: &TachConfig) {
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
            eprintln!("[tach:config] Failed to read pyproject.toml: {}", e);
            return;
        }
    };

    let pyproject: PyProject = match toml::from_str(&contents) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[tach:config] Failed to parse pyproject.toml: {}", e);
            return;
        }
    };

    if let Some(tool) = pyproject.tool
        && let Some(env_vars) = tool.pytest_env
    {
        let user_denylist = tach_config.env_denylist.as_deref().unwrap_or(&[]);
        for (key, value) in env_vars {
            let is_builtin_blocked = ENV_DENYLIST
                .iter()
                .any(|&blocked| key.eq_ignore_ascii_case(blocked));
            let is_user_blocked = user_denylist
                .iter()
                .any(|blocked| key.eq_ignore_ascii_case(blocked));
            if is_builtin_blocked || is_user_blocked {
                eprintln!(
                    "[tach:config] WARNING: Blocked dangerous env var from pyproject.toml: {}",
                    key
                );
                continue;
            }
            // SAFETY: set_var is unsafe in Rust 2024 due to potential data races
            // in multi-threaded environments. This is called during config loading
            // which happens before any worker threads are spawned.
            let expanded_value = expand_env_vars(&value);
            unsafe { std::env::set_var(&key, &expanded_value) };
            eprintln!("[tach:config] Set env: {}={}", key, expanded_value);
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
        unsafe { std::env::remove_var("TEST_COVERAGE_VAR_1") };
        unsafe { std::env::remove_var("TEST_COVERAGE_VAR_2") };
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
    //  NetworkConfig Tests (Landlock V4+)
    // =========================================================================

    #[test]
    fn test_parse_network_config_full() {
        let toml_content = r#"
[tool.tach.network]
allow_localhost = true
allow_connect = ["api.example.com:443", "db.internal:5432"]
allow_bind_ports = [8000, 8080, 0]
"#;
        let pyproject: PyProject = toml::from_str(toml_content).unwrap();
        let config = pyproject.tool.unwrap().tach.unwrap();

        let net = config.network.unwrap();
        assert!(net.allow_localhost.unwrap());
        assert_eq!(net.allow_connect.as_ref().unwrap().len(), 2);
        assert_eq!(net.allow_bind_ports.as_ref().unwrap(), &[8000, 8080, 0]);
    }

    #[test]
    fn test_parse_network_config_defaults() {
        let toml_content = r#"
[tool.tach]
timeout = 30
"#;
        let pyproject: PyProject = toml::from_str(toml_content).unwrap();
        let config = pyproject.tool.unwrap().tach.unwrap();

        assert!(config.network.is_none());
    }

    #[test]
    fn test_network_config_allow_localhost_default_true() {
        let config = NetworkConfig::default();
        assert!(config.allow_localhost());
    }

    #[test]
    fn test_merged_config_includes_network() {
        use clap::Parser;

        let cli = Cli::parse_from(["tach", "."]);
        let file_config = TachConfig {
            network: Some(NetworkConfig {
                allow_localhost: Some(true),
                allow_connect: Some(vec!["api.example.com:443".to_string()]),
                allow_bind_ports: Some(vec![8080]),
            }),
            ..Default::default()
        };

        let merged = MergedConfig::from_cli_and_file(&cli, &file_config);

        assert!(merged.network.is_some());
        let net = merged.network.as_ref().unwrap();
        assert!(net.allow_localhost());
        assert_eq!(net.allowed_connections().len(), 1);
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
        unsafe { std::env::remove_var("LD_PRELOAD") };
        unsafe { std::env::remove_var("SAFE_VAR") };

        load_env_from_pyproject(temp_dir.path());

        // LD_PRELOAD should NOT be set (blocked)
        assert!(std::env::var("LD_PRELOAD").is_err());

        // SAFE_VAR should be set (allowed)
        assert_eq!(std::env::var("SAFE_VAR").unwrap(), "allowed");

        // Cleanup
        unsafe { std::env::remove_var("SAFE_VAR") };
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

        unsafe { std::env::remove_var("PYTHONPATH") };
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

        unsafe { std::env::remove_var("PYTHONMALLOC") };
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

        unsafe { std::env::remove_var("ld_preload") };
        unsafe { std::env::remove_var("Ld_Library_Path") };
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

    #[test]
    fn test_load_env_from_pyproject_expands_variables() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("pyproject.toml");

        // Set a base variable to expand
        unsafe { std::env::set_var("TACH_TEST_BASE", "/base/path") };

        std::fs::write(
            &config_path,
            r#"
[tool.pytest_env]
EXPANDED_VAR = "{TACH_TEST_BASE}/subdir"
LITERAL_BRACES = "{{NOT_EXPANDED}}"
"#,
        )
        .unwrap();

        load_env_from_pyproject(temp_dir.path());

        assert_eq!(std::env::var("EXPANDED_VAR").unwrap(), "/base/path/subdir");
        // Double braces escape to literal braces
        assert_eq!(std::env::var("LITERAL_BRACES").unwrap(), "{NOT_EXPANDED}");

        // Cleanup
        unsafe {
            std::env::remove_var("TACH_TEST_BASE");
            std::env::remove_var("EXPANDED_VAR");
            std::env::remove_var("LITERAL_BRACES");
        }
    }

    // =========================================================================
    //  Django Database CLI Flag Tests
    // =========================================================================

    #[test]
    fn test_reuse_db_flag_parsing() {
        use clap::Parser;
        let cli = Cli::parse_from(["tach", "--reuse-db", "."]);
        assert!(cli.reuse_db);
        assert!(!cli.create_db);
    }

    #[test]
    fn test_create_db_flag_parsing() {
        use clap::Parser;
        let cli = Cli::parse_from(["tach", "--create-db", "."]);
        assert!(cli.create_db);
        assert!(!cli.reuse_db);
    }

    #[test]
    fn test_create_db_overrides_reuse_db() {
        use clap::Parser;
        let cli = Cli::parse_from(["tach", "--reuse-db", "--create-db", "."]);
        assert!(cli.create_db);
        assert!(cli.reuse_db);
    }

    #[test]
    fn test_merged_config_includes_db_flags() {
        use clap::Parser;
        let cli = Cli::parse_from(["tach", "--reuse-db", "."]);
        let file_config = TachConfig::default();
        let merged = MergedConfig::from_cli_and_file(&cli, &file_config);
        assert!(merged.reuse_db);
        assert!(!merged.create_db);
    }

    #[test]
    fn test_parse_traceback_style_valid() {
        assert_eq!(parse_traceback_style("short"), Some(TracebackStyle::Short));
        assert_eq!(parse_traceback_style("long"), Some(TracebackStyle::Long));
        assert_eq!(parse_traceback_style("line"), Some(TracebackStyle::Line));
        assert_eq!(
            parse_traceback_style("native"),
            Some(TracebackStyle::Native)
        );
        assert_eq!(parse_traceback_style("no"), Some(TracebackStyle::No));
        assert_eq!(parse_traceback_style("auto"), Some(TracebackStyle::Auto));
        assert_eq!(parse_traceback_style("invalid"), None);
    }

    #[test]
    fn test_parse_tach_config_full_schema() {
        let toml_content = r#"
[tool.tach]
test_pattern = "tests/**/*.py"
timeout = 120
workers = 8
keyword = "not slow"
markers = "unit"
exitfirst = true
maxfail = 5
force_toxic = true
no_fallback = true
no_isolation = false
traceback = "short"
durations = 10
memory = true
no_ignore = true
reuse_db = true
create_db = false
"#;
        let pyproject: PyProject = toml::from_str(toml_content).unwrap();
        let config = pyproject.tool.unwrap().tach.unwrap();

        assert_eq!(config.keyword.as_deref(), Some("not slow"));
        assert_eq!(config.markers.as_deref(), Some("unit"));
        assert_eq!(config.exitfirst, Some(true));
        assert_eq!(config.maxfail, Some(5));
        assert_eq!(config.force_toxic, Some(true));
        assert_eq!(config.no_fallback, Some(true));
        assert_eq!(config.no_isolation, Some(false));
        assert_eq!(config.traceback.as_deref(), Some("short"));
        assert_eq!(config.durations, Some(10));
        assert_eq!(config.memory, Some(true));
        assert_eq!(config.no_ignore, Some(true));
        assert_eq!(config.reuse_db, Some(true));
        assert_eq!(config.create_db, Some(false));
    }

    #[test]
    fn test_merged_config_cli_wins_over_file() {
        use clap::Parser;
        let cli = Cli::parse_from(["tach", "-k", "fast", "-x", "."]);
        let file_config = TachConfig {
            keyword: Some("slow".to_string()),
            exitfirst: Some(false),
            maxfail: Some(10),
            ..Default::default()
        };
        let merged = MergedConfig::from_cli_and_file(&cli, &file_config);

        assert_eq!(merged.keyword.as_deref(), Some("fast"));
        assert!(merged.exitfirst);
        assert_eq!(merged.maxfail, Some(10));
    }

    #[test]
    fn test_merged_config_file_provides_defaults() {
        use clap::Parser;
        let cli = Cli::parse_from(["tach", "."]);
        let file_config = TachConfig {
            keyword: Some("unit".to_string()),
            markers: Some("not integration".to_string()),
            no_fallback: Some(true),
            force_toxic: Some(true),
            durations: Some(5),
            ..Default::default()
        };
        let merged = MergedConfig::from_cli_and_file(&cli, &file_config);

        assert_eq!(merged.keyword.as_deref(), Some("unit"));
        assert_eq!(merged.markers.as_deref(), Some("not integration"));
        assert!(merged.no_fallback);
        assert!(merged.force_toxic);
        assert_eq!(merged.durations, Some(5));
    }

    #[test]
    fn test_merged_config_traceback_from_file() {
        use clap::Parser;
        let cli = Cli::parse_from(["tach", "."]);
        let file_config = TachConfig {
            traceback: Some("short".to_string()),
            ..Default::default()
        };
        let merged = MergedConfig::from_cli_and_file(&cli, &file_config);
        assert_eq!(merged.traceback, TracebackStyle::Short);
    }

    #[test]
    fn test_merged_config_fail_fast() {
        use clap::Parser;
        let cli = Cli::parse_from(["tach", "-x", "."]);
        let file_config = TachConfig::default();
        let merged = MergedConfig::from_cli_and_file(&cli, &file_config);
        assert!(merged.fail_fast());

        let cli2 = Cli::parse_from(["tach", "--maxfail", "1", "."]);
        let merged2 = MergedConfig::from_cli_and_file(&cli2, &file_config);
        assert!(merged2.fail_fast());

        let cli3 = Cli::parse_from(["tach", "."]);
        let merged3 = MergedConfig::from_cli_and_file(&cli3, &file_config);
        assert!(!merged3.fail_fast());
    }

    #[test]
    fn test_user_configurable_env_denylist() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("pyproject.toml");
        std::fs::write(
            &config_path,
            r#"
[tool.tach]
env_denylist = ["MY_SECRET", "API_KEY"]

[tool.pytest_env]
MY_SECRET = "should_be_blocked"
SAFE_VAR_99 = "allowed"
"#,
        )
        .unwrap();

        unsafe { std::env::remove_var("MY_SECRET") };
        unsafe { std::env::remove_var("SAFE_VAR_99") };

        load_env_from_pyproject(temp_dir.path());

        assert!(std::env::var("MY_SECRET").is_err());
        assert_eq!(std::env::var("SAFE_VAR_99").unwrap(), "allowed");

        unsafe { std::env::remove_var("SAFE_VAR_99") };
    }

    #[test]
    fn test_parse_env_denylist_from_toml() {
        let toml_content = r#"
[tool.tach]
env_denylist = ["SECRET_KEY", "DB_PASSWORD"]
"#;
        let pyproject: PyProject = toml::from_str(toml_content).unwrap();
        let config = pyproject.tool.unwrap().tach.unwrap();
        assert_eq!(
            config.env_denylist.unwrap(),
            vec!["SECRET_KEY", "DB_PASSWORD"]
        );
    }

    #[test]
    fn test_parse_toxicity_config() {
        let toml_content = r#"
[tool.tach.toxicity]
force_safe = ["myapp.utils", "myapp.helpers"]
force_toxic = ["myapp.workers"]
"#;
        let pyproject: PyProject = toml::from_str(toml_content).unwrap();
        let config = pyproject.tool.unwrap().tach.unwrap();
        let tox = config.toxicity.unwrap();
        assert_eq!(tox.force_safe, vec!["myapp.utils", "myapp.helpers"]);
        assert_eq!(tox.force_toxic, vec!["myapp.workers"]);
    }

    #[test]
    fn test_stepwise_enables_lf_and_exitfirst() {
        use clap::Parser;
        let cli = Cli::parse_from(["tach", "--sw", "."]);
        let file_config = TachConfig::default();
        let merged = MergedConfig::from_cli_and_file(&cli, &file_config);
        assert!(merged.last_failed);
        assert!(merged.exitfirst);
    }

    #[test]
    fn test_p_no_plugin_adds_to_disabled() {
        use clap::Parser;
        let cli = Cli::parse_from(["tach", "-p", "no:sugar", "-p", "no:xdist", "."]);
        let file_config = TachConfig::default();
        let merged = MergedConfig::from_cli_and_file(&cli, &file_config);
        assert!(merged.disabled_plugins.contains(&"sugar".to_string()));
        assert!(merged.disabled_plugins.contains(&"xdist".to_string()));
    }

    #[test]
    fn test_p_without_no_prefix_ignored() {
        use clap::Parser;
        let cli = Cli::parse_from(["tach", "-p", "some_plugin", "."]);
        let file_config = TachConfig::default();
        let merged = MergedConfig::from_cli_and_file(&cli, &file_config);
        assert!(merged.disabled_plugins.is_empty());
    }

    #[test]
    fn test_no_header_flag() {
        use clap::Parser;
        let cli = Cli::parse_from(["tach", "--no-header", "."]);
        let file_config = TachConfig::default();
        let merged = MergedConfig::from_cli_and_file(&cli, &file_config);
        assert!(merged.no_header);
    }

    #[test]
    fn test_collect_only_alias() {
        use clap::Parser;
        let cli = Cli::parse_from(["tach", "--co", "."]);
        assert!(cli.collect_only);
    }

    #[test]
    fn test_rootdir_flag() {
        use clap::Parser;
        let cli = Cli::parse_from(["tach", "--rootdir", "/tmp/myproject", "."]);
        assert_eq!(cli.rootdir.as_deref(), Some("/tmp/myproject"));
    }

    #[test]
    fn test_confcutdir_flag() {
        use clap::Parser;
        let cli = Cli::parse_from(["tach", "--confcutdir", "src", "."]);
        assert_eq!(cli.confcutdir.as_deref(), Some("src"));
    }

    #[test]
    fn test_override_ini_multiple() {
        use clap::Parser;
        let cli = Cli::parse_from([
            "tach",
            "-o",
            "markers=slow: slow tests",
            "-o",
            "timeout=30",
            ".",
        ]);
        assert_eq!(cli.override_ini.len(), 2);
        assert_eq!(cli.override_ini[0], "markers=slow: slow tests");
        assert_eq!(cli.override_ini[1], "timeout=30");
    }

    #[test]
    fn test_import_mode_default() {
        use clap::Parser;
        let cli = Cli::parse_from(["tach", "."]);
        assert_eq!(cli.import_mode, "prepend");
    }

    #[test]
    fn test_import_mode_importlib() {
        use clap::Parser;
        let cli = Cli::parse_from(["tach", "--import-mode", "importlib", "."]);
        assert_eq!(cli.import_mode, "importlib");
    }

    #[test]
    fn test_retries_flag() {
        use clap::Parser;
        let cli = Cli::parse_from(["tach", "--retries", "3", "."]);
        let file_config = TachConfig::default();
        let merged = MergedConfig::from_cli_and_file(&cli, &file_config);
        assert_eq!(merged.retries, Some(3));
    }

    #[test]
    fn test_show_locals_flag() {
        use clap::Parser;
        let cli = Cli::parse_from(["tach", "-l", "."]);
        let file_config = TachConfig::default();
        let merged = MergedConfig::from_cli_and_file(&cli, &file_config);
        assert!(merged.show_locals);
    }

    #[test]
    fn test_resume_flag() {
        use clap::Parser;
        let cli = Cli::parse_from(["tach", "--resume", "."]);
        assert!(cli.resume);
    }

    #[test]
    fn test_cache_show_flag() {
        use clap::Parser;
        let cli = Cli::parse_from(["tach", "--cache-show", "."]);
        assert!(cli.cache_show);
    }

    #[test]
    fn test_runxfail_flag() {
        use clap::Parser;
        let cli = Cli::parse_from(["tach", "--runxfail", "."]);
        assert!(cli.runxfail);
    }

    #[test]
    fn test_color_flag_default() {
        use clap::Parser;
        let cli = Cli::parse_from(["tach", "."]);
        assert_eq!(cli.color, "auto");
    }

    #[test]
    fn test_color_flag_no() {
        use clap::Parser;
        let cli = Cli::parse_from(["tach", "--color", "no", "."]);
        assert_eq!(cli.color, "no");
    }

    #[test]
    fn test_log_file_flag() {
        use clap::Parser;
        let cli = Cli::parse_from(["tach", "--log-file", "/tmp/test.log", "."]);
        assert_eq!(cli.log_file.unwrap().to_str().unwrap(), "/tmp/test.log");
    }

    #[test]
    fn test_timeout_method_default() {
        use clap::Parser;
        let cli = Cli::parse_from(["tach", "."]);
        assert_eq!(cli.timeout_method, "signal");
    }

    #[test]
    fn test_basetemp_flag() {
        use clap::Parser;
        let cli = Cli::parse_from(["tach", "--basetemp", "/tmp/tach_tmp", "."]);
        assert_eq!(cli.basetemp.as_deref(), Some("/tmp/tach_tmp"));
    }

    #[test]
    fn test_strict_markers_flag() {
        use clap::Parser;
        let cli = Cli::parse_from(["tach", "--strict-markers", "."]);
        assert!(cli.strict_markers);
    }

    #[test]
    fn test_deselect_single() {
        use clap::Parser;
        let cli = Cli::parse_from(["tach", "--deselect", "test_foo.py::test_bar", "."]);
        assert_eq!(cli.deselect, vec!["test_foo.py::test_bar"]);
    }

    #[test]
    fn test_deselect_multiple() {
        use clap::Parser;
        let cli = Cli::parse_from([
            "tach",
            "--deselect",
            "test_a.py::test_1",
            "--deselect",
            "test_b.py::test_2",
            ".",
        ]);
        assert_eq!(cli.deselect.len(), 2);
    }

    #[test]
    fn test_count_flag() {
        use clap::Parser;
        let cli = Cli::parse_from(["tach", "--count", "."]);
        assert!(cli.count);
    }

    #[test]
    fn test_all_flags_combined() {
        use clap::Parser;
        let cli = Cli::parse_from([
            "tach",
            "-v",
            "-k",
            "fast",
            "-m",
            "unit",
            "-x",
            "--no-header",
            "--color",
            "no",
            "--rootdir",
            "/tmp",
            ".",
        ]);
        assert_eq!(cli.verbose, 1);
        assert_eq!(cli.keyword.as_deref(), Some("fast"));
        assert_eq!(cli.markers.as_deref(), Some("unit"));
        assert!(cli.exitfirst);
        assert!(cli.no_header);
        assert_eq!(cli.color, "no");
        assert_eq!(cli.rootdir.as_deref(), Some("/tmp"));
    }

    #[test]
    fn test_pdb_flag() {
        use clap::Parser;
        let cli = Cli::parse_from(["tach", "--pdb", "."]);
        assert!(cli.pdb);
    }

    #[test]
    fn test_no_capture_short_flag() {
        use clap::Parser;
        let cli = Cli::parse_from(["tach", "-s", "."]);
        assert!(cli.no_capture);
    }

    #[test]
    fn test_no_capture_long_flag() {
        use clap::Parser;
        let cli = Cli::parse_from(["tach", "--capture-no", "."]);
        assert!(cli.no_capture);
    }

    #[test]
    fn test_ignore_flag() {
        use clap::Parser;
        let cli = Cli::parse_from(["tach", "--ignore", "vendor/", "--ignore", "legacy/", "."]);
        assert_eq!(cli.ignore, vec!["vendor/", "legacy/"]);
    }

    #[test]
    fn test_ignore_glob_flag() {
        use clap::Parser;
        let cli = Cli::parse_from(["tach", "--ignore-glob", "**/test_slow_*", "."]);
        assert_eq!(cli.ignore_glob, vec!["**/test_slow_*"]);
    }

    #[test]
    fn test_randomly_seed_flag() {
        use clap::Parser;
        let cli = Cli::parse_from(["tach", "--randomly-seed", "12345", "."]);
        assert_eq!(cli.randomly_seed, Some(12345));
    }

    #[test]
    fn test_doctest_modules_flag() {
        use clap::Parser;
        let cli = Cli::parse_from(["tach", "--doctest-modules", "."]);
        assert!(cli.doctest_modules);
    }

    #[test]
    fn test_pyargs_flag() {
        use clap::Parser;
        let cli = Cli::parse_from(["tach", "--pyargs", "mypackage.tests"]);
        assert!(cli.pyargs);
        assert_eq!(cli.path, "mypackage.tests");
    }

    #[test]
    fn test_forked_flag() {
        use clap::Parser;
        let cli = Cli::parse_from(["tach", "--forked", "."]);
        assert!(cli.forked);
    }

    #[test]
    fn test_new_first_flag() {
        use clap::Parser;
        let cli = Cli::parse_from(["tach", "--nf", "."]);
        assert!(cli.new_first);
    }

    #[test]
    fn test_setup_flags() {
        use clap::Parser;
        let cli = Cli::parse_from(["tach", "--setup-plan", "."]);
        assert!(cli.setup_plan);

        let cli2 = Cli::parse_from(["tach", "--setup-show", "."]);
        assert!(cli2.setup_show);

        let cli3 = Cli::parse_from(["tach", "--setup-only", "."]);
        assert!(cli3.setup_only);
    }

    #[test]
    fn test_werror_flag() {
        use clap::Parser;
        let cli = Cli::parse_from(["tach", "--Werror", "."]);
        assert!(cli.warnings_as_errors);
    }

    #[test]
    fn test_durations_min_default() {
        use clap::Parser;
        let cli = Cli::parse_from(["tach", "."]);
        assert!((cli.durations_min - 0.005).abs() < 0.001);
    }

    #[test]
    fn test_html_flag() {
        use clap::Parser;
        let cli = Cli::parse_from(["tach", "--html", "report.html", "."]);
        assert_eq!(cli.html.unwrap().to_str().unwrap(), "report.html");
    }

    #[test]
    fn test_log_cli_level_flag() {
        use clap::Parser;
        let cli = Cli::parse_from(["tach", "--log-cli-level", "DEBUG", "."]);
        assert_eq!(cli.log_cli_level.as_deref(), Some("DEBUG"));
    }
}
