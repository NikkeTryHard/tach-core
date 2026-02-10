use tach_core::config::{self, Cli, Commands, OutputFormat, TracebackStyle};
use tach_core::coverage;
use tach_core::debugger::{self, DebugServer};
use tach_core::discover_with_toxicity_options;
use tach_core::discovery;
use tach_core::errors::CategorizedError;
use tach_core::hooks::HookRegistry;
use tach_core::junit::JunitReporter;
use tach_core::lifecycle::CleanupGuard;
use tach_core::loader;
use tach_core::logcapture::LogCapture;
use tach_core::logredirect::{self, LogRedirect};
use tach_core::reporter::{
    DotsReporter, JsonReporter, MultiReporter, ProgressReporter, Reporter, TachReporter,
};
use tach_core::resolver::{self, FixtureRegistry, Resolver};
use tach_core::scheduler::Scheduler;
use tach_core::signals;
use tach_core::suggestions;
use tach_core::watch;
use tach_core::zygote;

use anyhow::Result;
use clap::Parser;
use nix::sys::wait::waitpid;
use nix::unistd::{ForkResult, fork};
use std::io::Read;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use uuid::Uuid;

// =============================================================================
// SessionConfig: Groups configuration parameters for execute_session
// =============================================================================

/// Configuration for a test session.
/// Groups boolean/config parameters to reduce function argument count.
#[derive(Clone, Copy)]
struct SessionConfig {
    /// Enable coverage collection
    coverage_enabled: bool,
    /// Traceback display style
    traceback_style: TracebackStyle,
    /// Enable memory profiling
    memory_enabled: bool,
    /// Skip .ignore/.gitignore patterns
    no_ignore: bool,
}

// =============================================================================
// RunContext: Manages per-session resources including UFFD listener
// =============================================================================

/// Runtime context for a test session.
/// Creates a unique run directory for sockets and manages UFFD listener.
pub struct RunContext {
    /// Unique run directory: /tmp/tach_run_{uuid}/
    pub run_dir: PathBuf,
    /// Path to UFFD socket: /tmp/tach_run_{uuid}/uffd.sock
    pub uffd_sock_path: PathBuf,
    /// UFFD listener for worker handshakes (None if snapshot mode disabled)
    pub uffd_listener: Option<UnixListener>,
}

impl RunContext {
    /// Create a new run context with UFFD listener
    pub fn new() -> Result<Self> {
        let uuid = Uuid::new_v4();
        let run_dir = PathBuf::from(format!("/tmp/tach_run_{}", uuid));
        std::fs::create_dir_all(&run_dir)?;

        let uffd_sock_path = run_dir.join("uffd.sock");

        // Try to create UFFD listener (may fail if userfaultfd not available)
        let uffd_listener = match UnixListener::bind(&uffd_sock_path) {
            Ok(listener) => {
                // Set TACH_SUPERVISOR_SOCK so workers know where to connect
                // SAFETY: set_var is unsafe in Rust 2024 due to potential data races.
                // This is called during initialization before any worker threads spawn.
                unsafe { std::env::set_var("TACH_SUPERVISOR_SOCK", &uffd_sock_path) };
                Some(listener)
            }
            Err(e) => {
                eprintln!(
                    "[tach:supervisor] WARN: Failed to create UFFD listener: {}. Snapshot mode disabled.",
                    e
                );
                None
            }
        };

        Ok(Self {
            run_dir,
            uffd_sock_path,
            uffd_listener,
        })
    }

    /// Check if snapshot mode is available
    pub fn snapshot_enabled(&self) -> bool {
        self.uffd_listener.is_some()
    }
}

impl Drop for RunContext {
    fn drop(&mut self) {
        // Clean up run directory on exit
        if self.run_dir.exists() {
            let _ = std::fs::remove_dir_all(&self.run_dir);
        }
    }
}

fn main() -> Result<()> {
    // ==========================================================================
    // JEMALLOC VERIFICATION
    // ==========================================================================
    //
    // CRITICAL: Verify jemalloc is the active allocator BEFORE any allocations.
    //
    // Why this matters:
    // -----------------
    // The Hypervisor uses userfaultfd to snapshot and restore memory. For this
    // to work correctly, the allocator must be deterministic:
    //
    // 1. glibc's malloc has thread-local caches (tcache) that desync after restore
    // 2. glibc uses pointer mangling that doesn't survive snapshot/restore
    // 3. Python's obmalloc has similar issues
    //
    // Jemalloc solves this by providing:
    // - mallctl("thread.tcache.flush") to flush thread caches before snapshot
    // - mallctl("epoch") to synchronize metadata
    // - Deterministic arena layout without pointer mangling
    //
    // The #[global_allocator] in lib.rs sets jemalloc as the allocator.
    // This verification ensures it's actually active (not overridden by LD_PRELOAD).
    //
    // If jemalloc is not active, we MUST abort immediately. Running the Hypervisor
    // with glibc malloc will cause memory corruption after the first reset.
    // ==========================================================================

    // Verify jemalloc is active - FATAL if not
    match tach_core::allocator::verify_jemalloc_active() {
        Ok(version) => {
            eprintln!(
                "[tach:supervisor] Jemalloc {} verified - Hypervisor allocator ready",
                version
            );
        }
        Err(e) => {
            // Use CategorizedError for user-friendly error display
            let suggestion = suggestions::get_suggestion(
                suggestions::FailureCondition::JemallocNotActive,
                &suggestions::SuggestionContext::detect(),
            );
            let cat_error = CategorizedError::new(
                tach_core::errors::error_codes::E008,
                tach_core::errors::ErrorCategory::System,
                format!("Jemalloc allocator not active: {}", e),
                Some(suggestion),
            );
            cat_error.print_to_stderr();
            std::process::exit(1);
        }
    }

    // NOTE: MALLOC_CONF is now set at compile-time via the _rjem_malloc_conf
    // symbol in lib.rs. This ensures jemalloc reads the configuration at
    // process startup, before any allocations occur. Setting it here via
    // std::env::set_var() would be too late.

    // Parse CLI arguments FIRST
    let cli = Cli::parse();
    let is_json = cli.format == OutputFormat::Json;
    let is_watch = cli.watch;

    // --- DEBUG/TRACE MODE ---
    // Set environment variables for debug/trace logging so all components can check
    if cli.trace {
        // SAFETY: set_var is unsafe in Rust 2024 due to potential data races.
        // This is called during initialization before any worker threads spawn.
        unsafe { std::env::set_var("TACH_LOG_LEVEL", "trace") };
        if !is_json {
            eprintln!("[tach:supervisor] Trace logging enabled (maximum verbosity)");
        }
    } else if cli.debug {
        unsafe { std::env::set_var("TACH_LOG_LEVEL", "debug") };
        if !is_json {
            eprintln!("[tach:supervisor] Debug logging enabled");
        }
    }

    // Set TACH_NO_ISOLATION env var from CLI flag (inherits to all children)
    if cli.no_isolation {
        // SAFETY: set_var is unsafe in Rust 2024 due to potential data races.
        // This is called during initialization before any worker threads spawn.
        unsafe { std::env::set_var("TACH_NO_ISOLATION", "1") };
    }

    // Set TACH_TARGET_PATH for Zygote to know which path to collect tests from
    // SAFETY: Same as above - called before worker threads spawn.
    unsafe { std::env::set_var("TACH_TARGET_PATH", &cli.path) };

    // Set Django test DB flags for Zygote to read during setup_databases()
    // SAFETY: Same as above - called before worker threads spawn.
    if cli.reuse_db {
        unsafe { std::env::set_var("TACH_REUSE_DB", "1") };
    }
    if cli.create_db {
        unsafe { std::env::set_var("TACH_CREATE_DB", "1") };
    }

    // --- LIFECYCLE SETUP ---
    debugger::install_panic_hook();

    if let Err(e) = signals::install_signal_handlers()
        && !is_json
    {
        eprintln!(
            "[tach:supervisor] Warning: Failed to install signal handlers: {}",
            e
        );
    }

    // Spawn shutdown watchdog to force exit if graceful shutdown hangs
    if let Err(e) = signals::spawn_shutdown_watchdog()
        && !is_json
    {
        eprintln!(
            "[tach:supervisor] Warning: Failed to spawn shutdown watchdog: {}",
            e
        );
    }

    let cwd = std::env::current_dir()?;

    // Handle subcommands
    match &cli.command {
        Some(Commands::List { path }) => {
            return handle_list_command(&cwd, path, is_json, cli.no_ignore);
        }
        Some(Commands::SelfTest) => {
            return handle_self_test_command();
        }
        Some(Commands::Version) => {
            return handle_version_command(cli.verbose > 0);
        }
        Some(Commands::Completions { shell }) => {
            config::generate_completions(shell);
            return Ok(());
        }
        Some(Commands::Test) | None => {
            // Continue to test execution below
        }
    }

    // --- DIAGNOSE FLAG (can be combined with any command) ---
    // Handle --diagnose flag: run diagnostics and exit
    if cli.diagnose {
        return handle_diagnose_command();
    }

    // --- COLLECT-ONLY MODE (pytest compatibility) ---
    // Alias for 'tach list' command
    if cli.collect_only {
        return handle_list_command(&cwd, &cli.path, is_json, cli.no_ignore);
    }

    // --- DRY-RUN MODE ---
    // Discover tests and show what would run without executing
    if cli.dry_run {
        return handle_dry_run_command(&cwd, is_json, &cli.path, cli.no_ignore);
    }

    // --- WATCH MODE ---
    if is_watch {
        if is_json {
            eprintln!("[tach:supervisor] Warning: JSON output not recommended in watch mode");
        }

        // Clone config values for the closure
        let junit_path = cli.junit_xml.clone();
        let format = cli.format.clone();
        let cwd_clone = cwd.clone();
        let path_clone = cli.path.clone();
        let session_config = SessionConfig {
            coverage_enabled: false, // Coverage not supported in watch mode
            traceback_style: cli.traceback,
            memory_enabled: cli.memory,
            no_ignore: cli.no_ignore,
        };

        return watch::start_watch_loop(&cwd, move || {
            execute_session(
                &cwd_clone,
                &format,
                &junit_path,
                &path_clone,
                session_config,
            )
        });
    }

    // --- SINGLE RUN MODE ---
    execute_session(
        &cwd,
        &cli.format,
        &cli.junit_xml,
        &cli.path,
        SessionConfig {
            coverage_enabled: cli.coverage,
            traceback_style: cli.traceback,
            memory_enabled: cli.memory,
            no_ignore: cli.no_ignore,
        },
    )
}

/// Execute a complete test session (discovery -> resolution -> zygote -> run)
/// This is the reusable function that watch mode calls repeatedly.
fn execute_session(
    cwd: &PathBuf,
    format: &OutputFormat,
    junit_path: &Option<PathBuf>,
    target_path: &str,
    config: SessionConfig,
) -> Result<()> {
    let is_json = *format == OutputFormat::Json;

    // Create reporters
    //  Use TachReporter for interactive terminals, DotsReporter for CI
    let mut reporters: Vec<Box<dyn Reporter>> = Vec::new();
    match format {
        OutputFormat::Json => reporters.push(Box::new(JsonReporter)),
        OutputFormat::Human => {
            if ProgressReporter::should_use_progress_bar() {
                let tach_reporter = TachReporter::with_traceback_style(config.traceback_style);
                reporters.push(Box::new(tach_reporter));
            } else {
                reporters.push(Box::new(DotsReporter::with_traceback_style(
                    config.traceback_style,
                )));
            }
        }
    }
    if let Some(path) = junit_path {
        reporters.push(Box::new(JunitReporter::new(path.clone())));
    }
    let mut reporter = MultiReporter::new(reporters);

    // Redirect stderr to log file for interactive terminal mode.
    // This keeps diagnostic [tach:*] and [worker:*] logs out of the terminal
    // while TachReporter outputs test results to stdout.
    //
    // NOTE: Child processes (Zygote, workers) inherit this redirect, which is
    // intentional -- worker diagnostic noise goes to the log file. If the
    // Zygote fails to start, the parent process detects the child exit and
    // surfaces the error through the reporter's on_error() method (which uses
    // stdout). The child's eprintln! in that path is a secondary diagnostic
    // captured in the log file at /tmp/tach.log.
    let log_redirect =
        if *format != OutputFormat::Json && ProgressReporter::should_use_progress_bar() {
            match LogRedirect::new() {
                Ok(redirect) => {
                    // Only show log path in summary if redirect actually succeeded
                    reporter.set_log_path(logredirect::DEFAULT_LOG_PATH);
                    Some(redirect)
                }
                Err(_) => None,
            }
        } else {
            None
        };

    let cleanup = CleanupGuard::new();

    // --- DISCOVERY ---
    if !is_json {
        eprintln!("[tach:supervisor] Scanning {}...", cwd.display());
    }

    let start = std::time::Instant::now();
    let (discovery_result, toxicity_graph) = discover_with_toxicity_options(cwd, config.no_ignore)?;

    if !is_json {
        let toxic_count = toxicity_graph.toxic_modules().len();
        let safe_count = toxicity_graph.safe_modules().len();
        eprintln!(
            "[tach:supervisor] Discovered {} tests, {} fixtures in {:?} (toxic: {}, safe: {})",
            discovery_result.test_count(),
            discovery_result.fixture_count(),
            start.elapsed(),
            toxic_count,
            safe_count
        );
    }

    // Warn if no tests found and dangerous patterns detected in .ignore
    warn_if_blocking_patterns(cwd, discovery_result.modules.is_empty(), is_json);

    // --- EAGER COMPILATION ---
    // Compile ALL .py files in project and populate global registry BEFORE fork.
    // Workers will inherit this registry via CoW (copy-on-write).
    let start_compile = std::time::Instant::now();

    // Collect ALL .py files in project (not just test modules)
    let py_files: Vec<std::path::PathBuf> = walkdir::WalkDir::new(cwd)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            let path = e.path();
            // Include only .py files
            path.extension().is_some_and(|ext| ext == "py")
                // Exclude hidden directories, __pycache__, .git, etc.
                && !path.ancestors().any(|p| {
                    p.file_name().is_some_and(|name| {
                        let n = name.to_string_lossy();
                        n.starts_with('.') || n == "__pycache__" || n == "target" || n == "node_modules"
                    })
                })
        })
        .map(|e| e.path().to_path_buf())
        .collect();

    // Initialize registry and compile
    let registry = loader::init_registry(cwd.clone());
    if let Ok(compiler) = loader::BytecodeCompiler::new(cwd) {
        let compiled = compiler.compile_batch(&py_files, registry);
        if !is_json {
            eprintln!(
                "[tach:supervisor] Compiled {} of {} modules for zero-copy loading in {:?}",
                compiled,
                py_files.len(),
                start_compile.elapsed()
            );
        }
    } else if !is_json {
        eprintln!(
            "[tach:supervisor] WARN: Failed to create bytecode compiler, falling back to importlib"
        );
    }

    // --- RESOLUTION ---
    let fixture_registry = FixtureRegistry::from_discovery(&discovery_result);
    let resolver = Resolver::new(&fixture_registry);
    let (runnable_tests, errors) = resolver.resolve_all(&discovery_result);

    if !is_json {
        eprintln!(
            "[tach:supervisor] Resolved {} tests ({} errors)",
            runnable_tests.len(),
            errors.len()
        );

        for error in &errors {
            match error {
                resolver::ResolutionError::MissingFixture { test, fixture } => {
                    eprintln!("  ⚠ {} - missing: {}", test, fixture);
                }
                resolver::ResolutionError::CyclicDependency { test, cycle } => {
                    eprintln!("  ⚠ {} - cycle: {:?}", test, cycle);
                }
            }
        }
    }

    // --- TOXICITY TAGGING ---
    // Tag each resolved test with its toxicity status from the graph.
    // Toxic tests use fork/kill instead of snapshot/reset.
    let mut runnable_tests = runnable_tests;
    let mut toxic_test_count = 0;
    for test in &mut runnable_tests {
        test.is_toxic =
            toxicity_graph.is_toxic(&test.file_path) || test.has_django_transaction_marker();
        if test.is_toxic {
            toxic_test_count += 1;
        }
    }

    if !is_json && toxic_test_count > 0 {
        eprintln!(
            "[tach:supervisor] Toxicity: {} of {} tests marked toxic (will use fork/kill)",
            toxic_test_count,
            runnable_tests.len()
        );
    }

    // --- PATH FILTERING ---
    // Filter tests to only include those matching the target path
    let target = std::path::Path::new(target_path);
    let target_canonical = target
        .canonicalize()
        .unwrap_or_else(|_| target.to_path_buf());

    let filtered_tests: Vec<resolver::RunnableTest> = runnable_tests
        .into_iter()
        .filter(|test| {
            let test_path = std::path::Path::new(&test.file_path);
            let test_canonical = test_path
                .canonicalize()
                .unwrap_or_else(|_| test_path.to_path_buf());

            // Match if test is under target directory OR matches exactly
            test_canonical.starts_with(&target_canonical)
                || test_canonical == target_canonical
                ||
            // Handle relative path matching
            test_path.starts_with(target)
        })
        .collect();

    if !is_json {
        eprintln!(
            "[tach:supervisor] Selected {} tests to run (filtered by path: {})",
            filtered_tests.len(),
            target_path
        );
    }

    if filtered_tests.is_empty() {
        if !is_json {
            eprintln!(
                "[tach:supervisor] No tests found matching path: {}",
                target_path
            );
        }
        return Ok(());
    }

    // --- BUILD HOOK REGISTRY ---
    // Build the hook registry from discovery results before forking
    let hook_registry = discovery_result.build_hook_registry(cwd);

    // --- COMPUTE PER-FILE TEST COUNTS (for real-time streaming) ---
    let mut file_counts: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    for test in &filtered_tests {
        *file_counts
            .entry(test.file_path.to_string_lossy().to_string())
            .or_insert(0) += 1;
    }
    reporter.on_session_setup(&file_counts);

    // --- RUN TESTS ---
    let failed_count = run_tests(
        &cleanup,
        filtered_tests,
        &mut reporter,
        is_json,
        config.coverage_enabled,
        config.memory_enabled,
        hook_registry,
        cwd.clone(),
    )?;

    // Restore stderr before exiting (LogRedirect drops and restores automatically)
    drop(log_redirect);

    // Exit with code 1 if any tests failed
    if failed_count > 0 {
        std::process::exit(1);
    }

    Ok(())
}

/// Handle the `self-test` subcommand
fn handle_self_test_command() -> Result<()> {
    let success = tach_core::diagnostics::run_and_print_diagnostics();
    if success {
        Ok(())
    } else {
        std::process::exit(1);
    }
}

/// Handle the `--diagnose` flag
///
/// Runs comprehensive system diagnostics with a user-friendly output format.
/// This is an alias for `self-test` with enhanced formatting.
fn handle_diagnose_command() -> Result<()> {
    let success = tach_core::diagnostics::run_and_print_diagnose();
    if success {
        Ok(())
    } else {
        std::process::exit(1);
    }
}

/// Handle the `version` subcommand
fn handle_version_command(verbose: bool) -> Result<()> {
    eprintln!("tach {}", env!("CARGO_PKG_VERSION"));
    eprintln!("Hypervisor-Accelerated Python Test Runner");
    eprintln!();

    // Show jemalloc status
    match tach_core::allocator::verify_jemalloc_active() {
        Ok(version) => eprintln!("Allocator: Jemalloc {}", version),
        Err(_) => eprintln!("Allocator: System (not jemalloc)"),
    }

    // Show Python version if available
    let python_path = std::env::var("PYO3_PYTHON")
        .or_else(|_| std::env::var("PYTHON"))
        .unwrap_or_else(|_| "python3".to_string());

    if let Ok(output) = std::process::Command::new(&python_path)
        .args(["--version"])
        .output()
    {
        let version = String::from_utf8_lossy(&output.stdout);
        eprintln!("Python: {}", version.trim());
    }

    // Show kernel version
    if let Ok(version_str) = std::fs::read_to_string("/proc/version") {
        let parts: Vec<&str> = version_str.split_whitespace().collect();
        if parts.len() >= 3 {
            eprintln!("Kernel: {}", parts[2]);
        }
    }

    // --- VERBOSE OUTPUT: Show capabilities ---
    if verbose {
        eprintln!();
        eprintln!("Build Information:");

        // Rust MSRV (Minimum Supported Rust Version from Cargo.toml)
        let msrv = option_env!("CARGO_PKG_RUST_VERSION").unwrap_or("unknown");
        eprintln!("  Rust MSRV: {}", msrv);

        // Target triple (compile-time)
        #[cfg(target_arch = "x86_64")]
        let arch = "x86_64";
        #[cfg(target_arch = "aarch64")]
        let arch = "aarch64";
        #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
        let arch = "unknown";

        #[cfg(target_os = "linux")]
        let os = "linux";
        #[cfg(not(target_os = "linux"))]
        let os = "unknown";

        eprintln!("  Target: {}-unknown-{}-gnu", arch, os);

        // Git commit hash - try runtime detection
        if let Ok(output) = std::process::Command::new("git")
            .args(["rev-parse", "--short", "HEAD"])
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .output()
            && output.status.success()
        {
            let hash = String::from_utf8_lossy(&output.stdout);
            let hash = hash.trim();
            if !hash.is_empty() {
                eprintln!("  Git commit: {}", hash);
            }
        }

        eprintln!();
        eprintln!("Capabilities:");

        // Check userfaultfd availability
        let uffd_available = std::fs::read_to_string("/proc/sys/vm/unprivileged_userfaultfd")
            .map(|s| s.trim() == "1")
            .unwrap_or(false);
        eprintln!(
            "  userfaultfd: {}",
            if uffd_available {
                "available"
            } else {
                "restricted (requires CAP_SYS_PTRACE)"
            }
        );

        // Check Landlock ABI version based on kernel version
        if let Ok(version_str) = std::fs::read_to_string("/proc/version") {
            let parts: Vec<&str> = version_str.split_whitespace().collect();
            if parts.len() >= 3 {
                let version_part = parts[2];
                let version_nums: Vec<&str> = version_part.split('.').collect();
                if version_nums.len() >= 2 {
                    let major: u32 = version_nums[0].parse().unwrap_or(0);
                    let minor: u32 = version_nums[1].parse().unwrap_or(0);

                    let landlock_abi = if major > 6 || (major == 6 && minor >= 1) {
                        "ABI v4"
                    } else if major == 5 && minor >= 19 {
                        "ABI v3"
                    } else if major == 5 && minor >= 13 {
                        "ABI v1"
                    } else {
                        "unavailable (kernel < 5.13)"
                    };
                    eprintln!("  Landlock: {}", landlock_abi);
                }
            }
        }

        // Check Seccomp support
        // SAFETY: PR_GET_SECCOMP is a read-only query with no side effects.
        let seccomp_result = unsafe { libc::prctl(libc::PR_GET_SECCOMP, 0, 0, 0, 0) };
        eprintln!(
            "  Seccomp: {}",
            if seccomp_result >= 0 {
                "available"
            } else {
                "not supported"
            }
        );
    }

    eprintln!();
    eprintln!("Run 'tach self-test' for full system diagnostics.");

    Ok(())
}

/// Warn if dangerous patterns are detected in .ignore that may block tests
///
/// This helper is called from both `execute_session` and `handle_list_command`
/// to provide actionable feedback when .ignore patterns may be blocking Python files.
///
/// - If no tests found: WARNING level (critical - user likely has a problem)
/// - If some tests found: NOTE level (informational - patterns may be intentional)
fn warn_if_blocking_patterns(cwd: &Path, is_empty: bool, is_json: bool) {
    if is_json {
        return;
    }

    let patterns = discovery::detect_blocking_patterns(cwd);
    if patterns.is_empty() {
        return;
    }

    if is_empty {
        eprintln!("[tach:discovery] WARNING: No tests discovered!");
        eprintln!("[tach:discovery] These patterns in .ignore may be blocking Python files:");
    } else {
        eprintln!("[tach:discovery] NOTE: These patterns in .ignore may be hiding some tests:");
    }

    for pattern in &patterns {
        eprintln!("  - {}", pattern);
    }
    eprintln!("[tach:discovery] Try running with --no-ignore to verify.");
}

/// Handle the `list` subcommand
fn handle_list_command(
    cwd: &Path,
    target_path: &str,
    is_json: bool,
    no_ignore: bool,
) -> Result<()> {
    // Resolve target path (absolute or relative to cwd)
    let target = if std::path::Path::new(target_path).is_absolute() {
        std::path::PathBuf::from(target_path)
    } else {
        cwd.join(target_path)
    };
    let discovery_result = discovery::discover(&target, no_ignore)?;

    // Warn if no tests found and dangerous patterns detected in .ignore
    warn_if_blocking_patterns(&target, discovery_result.modules.is_empty(), is_json);

    if is_json {
        discovery::dump_json(&discovery_result)?;
    } else {
        for module in &discovery_result.modules {
            for test in &module.tests {
                eprintln!("{}::{}", module.path.display(), test.name);
            }
        }
        eprintln!();
        eprintln!(
            "Discovered {} tests in {} files",
            discovery_result.test_count(),
            discovery_result.modules.len()
        );
    }
    Ok(())
}

/// Handle the `--dry-run` flag
///
/// Discovers tests, resolves fixtures, applies path filtering,
/// and prints a summary of what would be executed without actually running.
fn handle_dry_run_command(
    cwd: &Path,
    is_json: bool,
    target_path: &str,
    no_ignore: bool,
) -> Result<()> {
    if !is_json {
        eprintln!("[tach:dry-run] Discovering tests in {}...", cwd.display());
    }

    let start = std::time::Instant::now();
    let (discovery_result, toxicity_graph) = discover_with_toxicity_options(cwd, no_ignore)?;

    if !is_json {
        let toxic_count = toxicity_graph.toxic_modules().len();
        let safe_count = toxicity_graph.safe_modules().len();
        eprintln!(
            "[tach:dry-run] Discovered {} tests, {} fixtures in {:?} (toxic: {}, safe: {})",
            discovery_result.test_count(),
            discovery_result.fixture_count(),
            start.elapsed(),
            toxic_count,
            safe_count
        );
    }

    // --- RESOLUTION ---
    let fixture_registry = resolver::FixtureRegistry::from_discovery(&discovery_result);
    let resolver = resolver::Resolver::new(&fixture_registry);
    let (runnable_tests, errors) = resolver.resolve_all(&discovery_result);

    if !is_json {
        eprintln!(
            "[tach:dry-run] Resolved {} tests ({} errors)",
            runnable_tests.len(),
            errors.len()
        );
    }

    // --- TOXICITY TAGGING ---
    let mut runnable_tests = runnable_tests;
    for test in &mut runnable_tests {
        test.is_toxic =
            toxicity_graph.is_toxic(&test.file_path) || test.has_django_transaction_marker();
    }

    // --- PATH FILTERING ---
    let target = std::path::Path::new(target_path);
    let target_canonical = target
        .canonicalize()
        .unwrap_or_else(|_| target.to_path_buf());

    let filtered_tests: Vec<resolver::RunnableTest> = runnable_tests
        .into_iter()
        .filter(|test| {
            let test_path = std::path::Path::new(&test.file_path);
            let test_canonical = test_path
                .canonicalize()
                .unwrap_or_else(|_| test_path.to_path_buf());

            test_canonical.starts_with(&target_canonical)
                || test_canonical == target_canonical
                || test_path.starts_with(target)
        })
        .collect();

    // --- OUTPUT SUMMARY ---
    if is_json {
        // JSON output for machine consumption
        let filtered_toxic_count = filtered_tests.iter().filter(|t| t.is_toxic).count();
        println!("{{");
        println!("  \"dry_run\": true,");
        println!("  \"test_count\": {},", filtered_tests.len());
        println!("  \"toxic_count\": {},", filtered_toxic_count);
        println!("  \"error_count\": {},", errors.len());
        println!("  \"tests\": [");
        for (i, test) in filtered_tests.iter().enumerate() {
            let comma = if i < filtered_tests.len() - 1 {
                ","
            } else {
                ""
            };
            println!(
                "    {{\"id\": \"{}::{}\", \"file\": \"{}\", \"toxic\": {}}}{}",
                test.file_path.display(),
                test.test_name,
                test.file_path.display(),
                test.is_toxic,
                comma
            );
        }
        println!("  ]");
        println!("}}");
    } else {
        eprintln!();
        eprintln!("=== DRY RUN SUMMARY ===");
        eprintln!();
        eprintln!("Would run {} tests:", filtered_tests.len());

        // Group tests by file for cleaner output
        let mut by_file: std::collections::HashMap<PathBuf, Vec<&resolver::RunnableTest>> =
            std::collections::HashMap::new();
        for test in &filtered_tests {
            by_file
                .entry(test.file_path.clone())
                .or_default()
                .push(test);
        }

        let mut files: Vec<_> = by_file.keys().collect();
        files.sort();

        for file in files {
            let tests = &by_file[file];
            let toxic_marker = if tests.iter().any(|t| t.is_toxic) {
                " [TOXIC]"
            } else {
                ""
            };
            eprintln!("  {}{}:", file.display(), toxic_marker);
            for test in tests {
                eprintln!("    - {}", test.test_name);
            }
        }

        eprintln!();
        eprintln!("Summary:");
        eprintln!("  Total tests: {}", filtered_tests.len());
        eprintln!(
            "  Safe tests: {}",
            filtered_tests.iter().filter(|t| !t.is_toxic).count()
        );
        eprintln!(
            "  Toxic tests: {}",
            filtered_tests.iter().filter(|t| t.is_toxic).count()
        );
        if !errors.is_empty() {
            eprintln!("  Resolution errors: {}", errors.len());
        }
        eprintln!();
        eprintln!("(No tests were executed - this was a dry run)");
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn run_tests(
    cleanup: &CleanupGuard,
    runnable_tests: Vec<resolver::RunnableTest>,
    reporter: &mut dyn Reporter,
    is_json: bool,
    coverage_enabled: bool,
    memory_enabled: bool,
    mut hook_registry: HookRegistry,
    project_root: PathBuf,
) -> Result<usize> {
    let cwd = std::env::current_dir()?;

    // --- LOAD TACH CONFIG ---
    let tach_config = config::load_tach_config(&cwd);

    // --- COVERAGE INITIALIZATION ---
    // Initialize coverage ring buffers BEFORE forking Zygote.
    // These are shared memory regions (memfd) that workers will inherit via fork.
    let mut coverage_aggregator: Option<coverage::CoverageAggregator> = None;

    if coverage_enabled {
        if !is_json {
            eprintln!("[tach:supervisor] Initializing coverage collection...");
        }

        // Initialize coverage ring buffer (LINE events)
        match coverage::init_coverage_buffer(coverage::DEFAULT_CAPACITY) {
            Ok(_) => {
                if !is_json {
                    eprintln!(
                        "[tach:supervisor] Coverage buffer: {} entries ({} bytes)",
                        coverage::DEFAULT_CAPACITY,
                        coverage::DEFAULT_CAPACITY * coverage::ENTRY_SIZE + coverage::HEADER_SIZE
                    );
                }
            }
            Err(e) => {
                eprintln!(
                    "[tach:supervisor] WARNING: Failed to init coverage buffer: {}",
                    e
                );
            }
        }

        // Initialize mapping ring buffer (PY_START events for code_id -> filename)
        match coverage::init_mapping_buffer(coverage::MAPPING_CAPACITY) {
            Ok(_) => {
                if !is_json {
                    eprintln!(
                        "[tach:supervisor] Mapping buffer: {} entries ({} bytes)",
                        coverage::MAPPING_CAPACITY,
                        coverage::MAPPING_CAPACITY * coverage::MAPPING_ENTRY_SIZE
                            + coverage::HEADER_SIZE
                    );
                }
            }
            Err(e) => {
                eprintln!(
                    "[tach:supervisor] WARNING: Failed to init mapping buffer: {}",
                    e
                );
            }
        }

        // Start aggregator thread to drain buffers
        let mut aggregator = coverage::CoverageAggregator::new();
        aggregator.start(std::time::Duration::from_millis(100));
        coverage_aggregator = Some(aggregator);

        // Set env var so workers know to enable coverage
        // SAFETY: set_var is unsafe in Rust 2024 due to potential data races.
        // This is called during initialization before any worker threads spawn.
        unsafe { std::env::set_var("TACH_COVERAGE", "1") };
    }

    // --- CREATE DEBUG SERVER ---
    let debug_server = DebugServer::new()?;
    let debug_socket_path = debug_server.socket_path().to_path_buf();
    cleanup.track_socket(debug_socket_path.clone());

    // --- CREATE LOG CAPTURE ---
    let max_workers = num_cpus::get().min(runnable_tests.len()).max(1);
    let log_capture = LogCapture::new(max_workers)?;

    if !is_json {
        eprintln!(
            "[tach:supervisor] Created {} log buffers (memfd)",
            max_workers
        );
    }

    // --- SOCKET PAIRS ---
    let (sup_cmd_sock, zyg_cmd_sock) = UnixStream::pair()?;
    let (sup_result_sock, zyg_result_sock) = UnixStream::pair()?;

    // --- LOAD CONFIG ---
    config::load_env_from_pyproject(&cwd);

    // --- NO-ISOLATION MODE ---
    // Set env var so workers can check it (must be before fork to inherit)
    if std::env::var("TACH_NO_ISOLATION").unwrap_or_default() == "1" {
        eprintln!("[tach:supervisor] Isolation disabled via TACH_NO_ISOLATION");
    }

    // --- CREATE RUN CONTEXT (Snapshot Mode) ---
    // This creates the UFFD listener socket and sets TACH_SUPERVISOR_SOCK env var
    // Must be before fork so the env var is inherited by Zygote
    let run_context = RunContext::new()?;
    if run_context.snapshot_enabled() && !is_json {
        eprintln!(
            "[tach:supervisor] Snapshot mode enabled: {}",
            run_context.uffd_sock_path.display()
        );
    }

    if !is_json {
        eprintln!("[tach:supervisor] Forking Zygote...");
    }

    match unsafe { fork() }? {
        ForkResult::Child => {
            drop(sup_cmd_sock);
            drop(sup_result_sock);
            std::mem::forget(debug_server);
            std::mem::forget(log_capture);
            std::mem::forget(run_context); // Don't cleanup in child
            std::mem::forget(unsafe { std::ptr::read(cleanup) });

            if let Err(e) = zygote::entrypoint(zyg_cmd_sock, zyg_result_sock) {
                eprintln!("[tach:zygote] Error: {:?}", e);
                std::process::exit(1);
            }
            std::process::exit(0);
        }
        ForkResult::Parent { child: zygote_pid } => {
            drop(zyg_cmd_sock);
            drop(zyg_result_sock);

            cleanup.set_zygote_pid(zygote_pid.as_raw());

            if !is_json {
                eprintln!("[tach:supervisor] Zygote PID: {}", zygote_pid);
            }

            // Wait for READY
            let mut ready_buf = [0u8; 1];
            let mut cmd_sock_clone = sup_cmd_sock.try_clone()?;
            cmd_sock_clone.read_exact(&mut ready_buf)?;

            if ready_buf[0] == 0x42 && !is_json {
                eprintln!("[tach:supervisor] Zygote is READY.\n");
            }

            // HOOK EFFECT BRIDGE (v0.2.0): Receive session effects from Zygote
            // The Zygote sends framed bincode-encoded Vec<HookEffect> after the ready byte
            // Format: magic(2) + version(1) + reserved(1) + length(4) + bincode data
            let mut header_buf = [0u8; tach_core::protocol::HEADER_SIZE];
            cmd_sock_clone.read_exact(&mut header_buf)?;

            // Extract length from header bytes 4-7 (little-endian u32)
            let effects_len =
                u32::from_le_bytes([header_buf[4], header_buf[5], header_buf[6], header_buf[7]])
                    as usize;

            if effects_len > 0 {
                // Allocate buffer for header + payload and decode with validation
                let mut full_buf = vec![0u8; tach_core::protocol::HEADER_SIZE + effects_len];
                full_buf[..tach_core::protocol::HEADER_SIZE].copy_from_slice(&header_buf);
                cmd_sock_clone.read_exact(&mut full_buf[tach_core::protocol::HEADER_SIZE..])?;

                let session_effects: Vec<tach_core::hooks::HookEffect> =
                    tach_core::protocol::decode_with_limit(
                        &full_buf,
                        tach_core::protocol::MAX_PAYLOAD_SIZE,
                    )
                    .unwrap_or_else(|e| {
                        eprintln!(
                            "[tach:supervisor] Warning: Failed to decode session effects: {}",
                            e
                        );
                        Vec::new()
                    });

                if !session_effects.is_empty() {
                    if !is_json {
                        eprintln!(
                            "[tach:supervisor] Received {} session hook effects from Zygote",
                            session_effects.len()
                        );
                    }
                    // Populate HookRegistry with session effects
                    // These are recorded under "pytest_configure" as they come from session initialization
                    for effect in session_effects {
                        hook_registry.record_effect("pytest_configure", effect);
                    }
                }
            }

            // --- ASYNCIO CONFIG ---
            // Parse asyncio configuration from pyproject.toml and add as session effect
            // This enables pytest-asyncio auto mode and loop_scope settings
            if let Ok(asyncio_config) = tach_core::discovery::parse_asyncio_config(&project_root) {
                // Only add effect if auto_mode is enabled or non-default loop_scope
                if asyncio_config.auto_mode || asyncio_config.loop_scope != "function" {
                    let loop_scope = match asyncio_config.loop_scope.as_str() {
                        "session" => tach_core::hooks::LoopScope::Session,
                        "module" => tach_core::hooks::LoopScope::Module,
                        "class" => tach_core::hooks::LoopScope::Class,
                        _ => tach_core::hooks::LoopScope::Function,
                    };
                    let asyncio_effect = tach_core::hooks::HookEffect::AsyncioSetup {
                        loop_scope,
                        auto_mode: asyncio_config.auto_mode,
                    };
                    hook_registry.record_effect("pytest_configure", asyncio_effect);

                    if !is_json {
                        eprintln!(
                            "[tach:supervisor] Asyncio config: mode={}, loop_scope={}",
                            if asyncio_config.auto_mode {
                                "auto"
                            } else {
                                "strict"
                            },
                            asyncio_config.loop_scope
                        );
                    }
                }
            }

            // --- SCHEDULER ---
            // Use with_config to pass timeout_hook from pyproject.toml
            let global_timeout = tach_config.timeout();
            let timeout_hook = tach_config.timeout_hook.clone();
            let mut scheduler = Scheduler::with_config(
                sup_cmd_sock,
                sup_result_sock,
                log_capture,
                debug_socket_path,
                global_timeout,
                timeout_hook,
                hook_registry,
                project_root,
                std::env::var("TACH_REUSE_DB").unwrap_or_default() == "1",
                std::env::var("TACH_CREATE_DB").unwrap_or_default() == "1",
            )?;

            let stats = scheduler.run(runnable_tests, reporter)?;

            // Shutdown
            scheduler.shutdown()?;
            waitpid(zygote_pid, None)?;

            // Track failure count for exit code
            let failed_count = stats.failed;

            // --- MEMORY REPORTING ---
            // Display memory usage statistics if enabled
            if memory_enabled && !is_json && !stats.memory_usage.is_empty() {
                eprintln!();
                eprintln!("[tach:supervisor] Memory Usage:");

                // Calculate statistics
                let total_memory: u64 = stats.memory_usage.iter().map(|(_, m)| *m).sum();
                let peak_memory = stats
                    .memory_usage
                    .iter()
                    .map(|(_, m)| *m)
                    .max()
                    .unwrap_or(0);
                let avg_memory = total_memory / stats.memory_usage.len() as u64;

                // Format memory in human-readable units
                let format_bytes = |bytes: u64| -> String {
                    if bytes >= 1024 * 1024 * 1024 {
                        format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
                    } else if bytes >= 1024 * 1024 {
                        format!("{:.2} MB", bytes as f64 / (1024.0 * 1024.0))
                    } else if bytes >= 1024 {
                        format!("{:.2} KB", bytes as f64 / 1024.0)
                    } else {
                        format!("{} B", bytes)
                    }
                };

                eprintln!("  Total RSS: {}", format_bytes(total_memory));
                eprintln!("  Peak RSS:  {} (single test)", format_bytes(peak_memory));
                eprintln!("  Avg RSS:   {} (per test)", format_bytes(avg_memory));

                // Show top 5 memory-heavy tests
                let mut sorted_usage = stats.memory_usage.clone();
                sorted_usage.sort_by(|a, b| b.1.cmp(&a.1));

                if sorted_usage.len() > 1 {
                    eprintln!();
                    eprintln!("  Top {} memory users:", sorted_usage.len().min(5));
                    for (test_name, memory) in sorted_usage.iter().take(5) {
                        eprintln!("    {} - {}", format_bytes(*memory), test_name);
                    }
                }

                // Warn if any test uses > 500MB
                const MEMORY_WARNING_THRESHOLD: u64 = 500 * 1024 * 1024; // 500MB
                let high_memory_tests: Vec<_> = stats
                    .memory_usage
                    .iter()
                    .filter(|(_, m)| *m > MEMORY_WARNING_THRESHOLD)
                    .collect();

                if !high_memory_tests.is_empty() {
                    eprintln!();
                    eprintln!(
                        "  WARNING: {} test(s) exceeded 500MB RSS:",
                        high_memory_tests.len()
                    );
                    for (test_name, memory) in high_memory_tests.iter().take(3) {
                        eprintln!("    {} - {}", format_bytes(*memory), test_name);
                    }
                }
            }

            // --- COVERAGE FINALIZATION ---
            // Stop aggregator and report coverage statistics
            if let Some(mut aggregator) = coverage_aggregator {
                aggregator.stop();
                // Use take_data() to avoid cloning the entire HashMap
                let coverage_data = aggregator.take_data();
                let total_hits: u64 = coverage_data.values().sum();

                if !is_json {
                    eprintln!(
                        "[tach:supervisor] Coverage: {} unique lines covered, {} total hits",
                        coverage_data.len(),
                        total_hits
                    );

                    // Report overflow counts if any
                    if let Some(buffer) = coverage::get_coverage_buffer() {
                        let overflow = buffer.overflow_count();
                        if overflow > 0 {
                            eprintln!(
                                "[tach:supervisor] WARNING: {} coverage entries dropped (buffer overflow)",
                                overflow
                            );
                        }
                    }
                    if let Some(buffer) = coverage::get_mapping_buffer() {
                        let overflow = buffer.overflow_count();
                        if overflow > 0 {
                            eprintln!(
                                "[tach:supervisor] WARNING: {} mapping entries dropped (buffer overflow)",
                                overflow
                            );
                        }
                    }
                }

                // Write coverage report to file
                // Use defaults or environment variables for output path and format
                if !coverage_data.is_empty() {
                    let output_file = std::env::var("TACH_COVERAGE_OUTPUT")
                        .unwrap_or_else(|_| "coverage.lcov".to_string());
                    let output_path = std::path::Path::new(&output_file);
                    let format_str = std::env::var("TACH_COVERAGE_FORMAT").ok();
                    let format = format_str.as_deref();

                    if let Err(e) =
                        coverage::write_coverage_report(&coverage_data, output_path, format)
                    {
                        eprintln!(
                            "[tach:supervisor] WARNING: Failed to write coverage report: {}",
                            e
                        );
                    } else if !is_json {
                        eprintln!(
                            "[tach:supervisor] Coverage report written to: {}",
                            output_path.display()
                        );
                    }
                }
            }

            if !is_json {
                eprintln!("[tach:supervisor] Done.");
            }

            // Mark shutdown as complete to prevent watchdog from force-exiting
            signals::mark_shutdown_complete();

            // Return failure count for exit code
            Ok(failed_count)
        }
    }
}
