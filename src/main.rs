use tach_core::cache::{
    read_duration_cache, read_lastfailed_cache, read_lastfailed_cache_from, write_duration_cache,
    write_lastfailed_cache,
};
use tach_core::config::{self, Cli, Commands, OutputFormat, TracebackStyle};
use tach_core::coverage;
use tach_core::debugger::{self, DebugServer};
use tach_core::discover_with_toxicity_options;
use tach_core::discovery;
use tach_core::errors::CategorizedError;
use tach_core::fallback::pytest_fallback_retry;
use tach_core::graph::ToxicityGraph;
use tach_core::hooks::HookRegistry;
use tach_core::junit::JunitReporter;
use tach_core::lifecycle::CleanupGuard;
use tach_core::loader;
use tach_core::logcapture::LogCapture;
use tach_core::logredirect::{self, LogRedirect};
use tach_core::ratatui_reporter::RatatuiReporter;
use tach_core::reporter::{
    DotsReporter, JsonReporter, MultiReporter, PhaseDetail, ProgressReporter, Reporter,
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
    coverage_enabled: bool,
    traceback_style: TracebackStyle,
    memory_enabled: bool,
    no_ignore: bool,
    no_fallback: bool,
    last_failed: bool,
    failed_first: bool,
    cache_clear: bool,
    resume: bool,
    durations: Option<usize>,
    verbose: u8,
    _quiet: bool,
    _retries: Option<u32>,
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

fn cleanup_stale_run_dirs() {
    let Ok(entries) = std::fs::read_dir("/tmp") else {
        return;
    };
    let threshold = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name_str) = name.to_str() else {
            continue;
        };
        if !name_str.starts_with("tach_run_") {
            continue;
        }
        if let Ok(meta) = entry.metadata()
            && let Ok(modified) = meta.modified()
            && modified < threshold
        {
            let _ = std::fs::remove_dir_all(entry.path());
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
        Ok(_version) => {}
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

    cleanup_stale_run_dirs();

    // Parse CLI arguments and merge with pyproject.toml config
    let cli = Cli::parse();
    let cwd = if let Some(ref rootdir) = cli.rootdir {
        let p = std::path::Path::new(rootdir);
        let resolved = if p.is_absolute() {
            p.to_path_buf()
        } else {
            std::env::current_dir()?.join(p)
        };
        resolved.canonicalize().unwrap_or(resolved)
    } else {
        std::env::current_dir()?
    };
    let file_config = config::load_tach_config(&cwd);
    config::load_env_from_pyproject(&cwd);
    let merged = config::MergedConfig::from_cli_and_file(&cli, &file_config);

    let is_json = merged.format == OutputFormat::Json;
    let is_watch = merged.watch;

    match cli.color.as_str() {
        "no" => unsafe { std::env::set_var("NO_COLOR", "1") },
        "yes" => unsafe { std::env::set_var("FORCE_COLOR", "1") },
        _ => {}
    }

    if merged.trace {
        unsafe { std::env::set_var("TACH_LOG_LEVEL", "trace") };
        if !is_json {
            eprintln!("[tach:supervisor] Trace logging enabled (maximum verbosity)");
        }
    } else if merged.debug {
        unsafe { std::env::set_var("TACH_LOG_LEVEL", "debug") };
        if !is_json {
            eprintln!("[tach:supervisor] Debug logging enabled");
        }
    }

    if merged.no_isolation {
        unsafe { std::env::set_var("TACH_NO_ISOLATION", "1") };
    }

    if merged.verbose == 0 {
        unsafe { std::env::set_var("TACH_QUIET", "1") };
    }
    if merged.no_header {
        unsafe { std::env::set_var("TACH_NO_HEADER", "1") };
    }
    if let Some(ref log_file) = cli.log_file {
        unsafe { std::env::set_var("TACH_LOG_FILE", log_file.as_os_str()) };
    }

    let target_file_path = if merged.path.contains("::") {
        merged.path.split("::").next().unwrap_or(&merged.path)
    } else {
        &merged.path
    };
    unsafe { std::env::set_var("TACH_TARGET_PATH", target_file_path) };

    if merged.reuse_db {
        unsafe { std::env::set_var("TACH_REUSE_DB", "1") };
    }
    if merged.create_db {
        unsafe { std::env::set_var("TACH_CREATE_DB", "1") };
    }

    // Ensure UTF-8 locale for all subprocesses and file operations.
    // Without this, workers forked from the zygote may use ASCII encoding
    // for filesystem paths and subprocess I/O, breaking non-ASCII tests.
    // SAFETY: Same as above - called before worker threads spawn.
    if std::env::var("LANG").unwrap_or_default().is_empty() {
        unsafe { std::env::set_var("LANG", "C.UTF-8") };
    }
    if std::env::var("PYTHONUTF8").unwrap_or_default().is_empty() {
        unsafe { std::env::set_var("PYTHONUTF8", "1") };
    }
    if std::env::var("PYTHONIOENCODING")
        .unwrap_or_default()
        .is_empty()
    {
        unsafe { std::env::set_var("PYTHONIOENCODING", "utf-8") };
    }

    if let Some(ref keyword) = merged.keyword {
        unsafe { std::env::set_var("TACH_KEYWORD", keyword) };
    }
    if let Some(ref markers) = merged.markers {
        unsafe { std::env::set_var("TACH_MARKERS", markers) };
    }
    if cli.strict_markers {
        unsafe { std::env::set_var("TACH_STRICT_MARKERS", "1") };
    }
    if cli.runxfail {
        unsafe { std::env::set_var("TACH_RUNXFAIL", "1") };
    }
    if cli.pdb {
        unsafe { std::env::set_var("TACH_PDB", "1") };
    }
    if cli.no_capture {
        unsafe { std::env::set_var("TACH_NO_CAPTURE", "1") };
    }
    if cli.doctest_modules {
        unsafe { std::env::set_var("TACH_DOCTEST_MODULES", "1") };
    }
    if !cli.override_ini.is_empty() {
        unsafe { std::env::set_var("TACH_OVERRIDE_INI", cli.override_ini.join("\x1f")) };
    }

    if cli.pyargs {
        unsafe { std::env::set_var("TACH_PYARGS", "1") };
    }
    if !cli.deselect.is_empty() {
        unsafe { std::env::set_var("TACH_DESELECT", cli.deselect.join("\x1f")) };
    }
    if !cli.ignore.is_empty() {
        unsafe { std::env::set_var("TACH_IGNORE", cli.ignore.join("\x1f")) };
    }
    if !cli.ignore_glob.is_empty() {
        unsafe { std::env::set_var("TACH_IGNORE_GLOB", cli.ignore_glob.join("\x1f")) };
    }
    if let Some(ref confcutdir) = cli.confcutdir {
        unsafe { std::env::set_var("TACH_CONFCUTDIR", confcutdir) };
    }
    if let Some(ref basetemp) = cli.basetemp {
        unsafe { std::env::set_var("TACH_BASETEMP", basetemp) };
    }
    if cli.import_mode != "prepend" {
        unsafe { std::env::set_var("TACH_IMPORT_MODE", &cli.import_mode) };
    }
    if let Some(seed) = cli.randomly_seed {
        unsafe { std::env::set_var("TACH_RANDOMLY_SEED", seed.to_string()) };
    }

    let maxfail = if merged.exitfirst {
        Some(1)
    } else {
        merged.maxfail
    };
    if let Some(mf) = maxfail {
        unsafe { std::env::set_var("TACH_MAXFAIL", mf.to_string()) };
    }
    if merged.force_toxic {
        unsafe { std::env::set_var("TACH_FORCE_TOXIC", "1") };
    }
    if !cli.pytest_args.is_empty() {
        unsafe { std::env::set_var("TACH_PYTEST_ARGS", cli.pytest_args.join("\x1f")) };
    }
    unsafe { std::env::set_var("TACH_TIMEOUT", merged.timeout.to_string()) };
    if merged.show_locals {
        unsafe { std::env::set_var("TACH_SHOWLOCALS", "1") };
    }
    if let Some((_, node)) = merged.path.split_once("::") {
        let kw = node.replace("::", " and ");
        let existing = std::env::var("TACH_KEYWORD").unwrap_or_default();
        let new_kw = if existing.is_empty() {
            kw
        } else {
            format!("{existing} and {kw}")
        };
        unsafe { std::env::set_var("TACH_KEYWORD", &new_kw) };
    }
    for plugin in &merged.disabled_plugins {
        let key = format!(
            "TACH_DISABLE_PLUGIN_{}",
            plugin.to_uppercase().replace('-', "_")
        );
        unsafe { std::env::set_var(&key, "1") };
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

    // Handle subcommands
    match &cli.command {
        Some(Commands::List { path }) => {
            return handle_list_command(&cwd, path, is_json, merged.no_ignore);
        }
        Some(Commands::SelfTest) => {
            return handle_self_test_command();
        }
        Some(Commands::Version) => {
            return handle_version_command(merged.verbose > 0);
        }
        Some(Commands::Completions { shell }) => {
            config::generate_completions(shell);
            return Ok(());
        }
        Some(Commands::Init) => {
            return handle_init_command(&cwd);
        }
        Some(Commands::Config) => {
            return handle_config_command(&cwd, &merged);
        }
        Some(Commands::Markers) => {
            return handle_markers_command(&cwd, merged.no_ignore);
        }
        Some(Commands::Clean) => {
            return handle_clean_command(&cwd);
        }
        Some(Commands::Fixtures) => {
            return handle_fixtures_command(&cwd, merged.no_ignore);
        }
        Some(Commands::Test) | None => {}
    }

    // --- DIAGNOSE FLAG (can be combined with any command) ---
    // Handle --diagnose flag: run diagnostics and exit
    if cli.diagnose {
        return handle_diagnose_command();
    }

    if cli.cache_show {
        return handle_cache_show(&cwd);
    }

    if cli.count {
        let result = tach_core::discovery::scanner::discover(&cwd, merged.no_ignore)?;
        let count: usize = result.modules.iter().map(|m| m.tests.len()).sum();
        if is_json {
            println!("{{\"count\":{count}}}");
        } else {
            println!("{count}");
        }
        return Ok(());
    }

    if cli.collect_only {
        return handle_list_command(&cwd, &merged.path, is_json, merged.no_ignore);
    }

    if cli.dry_run {
        return handle_dry_run_command(&cwd, is_json, &merged.path, merged.no_ignore);
    }

    // --- WATCH MODE ---
    if is_watch {
        if is_json {
            eprintln!("[tach:supervisor] Warning: JSON output not recommended in watch mode");
        }

        let junit_path = merged.junit_xml.clone();
        let format = merged.format.clone();
        let cwd_clone = cwd.clone();
        let path_clone = merged.path.clone();
        let session_config = SessionConfig {
            coverage_enabled: false,
            traceback_style: merged.traceback,
            memory_enabled: merged.memory,
            no_ignore: merged.no_ignore,
            no_fallback: merged.no_fallback,
            last_failed: merged.last_failed,
            failed_first: merged.failed_first,
            cache_clear: merged.cache_clear,
            resume: cli.resume,
            durations: merged.durations,
            verbose: merged.verbose,
            _quiet: merged.quiet,
            _retries: merged.retries,
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

    execute_session(
        &cwd,
        &merged.format,
        &merged.junit_xml,
        &merged.path,
        SessionConfig {
            coverage_enabled: merged.coverage,
            traceback_style: merged.traceback,
            memory_enabled: merged.memory,
            no_ignore: merged.no_ignore,
            no_fallback: merged.no_fallback,
            last_failed: merged.last_failed,
            failed_first: merged.failed_first,
            cache_clear: merged.cache_clear,
            resume: cli.resume,
            durations: merged.durations,
            verbose: merged.verbose,
            _quiet: merged.quiet,
            _retries: merged.retries,
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

    if config.cache_clear {
        let cache_dir = cwd.join(".tach_cache");
        if cache_dir.exists() {
            let _ = std::fs::remove_dir_all(&cache_dir);
            if !is_json {
                eprintln!("[tach:supervisor] Cache cleared");
            }
        }
    }

    // Create reporters
    //  Use TachReporter for interactive terminals, DotsReporter for CI
    let mut reporters: Vec<Box<dyn Reporter>> = Vec::new();
    match format {
        OutputFormat::Json => reporters.push(Box::new(JsonReporter)),
        OutputFormat::Human => {
            if config.verbose > 0 {
                reporters.push(Box::new(DotsReporter::with_traceback_style(
                    config.traceback_style,
                )));
            } else if ProgressReporter::should_use_progress_bar() {
                let ratatui_reporter =
                    RatatuiReporter::with_traceback_style(config.traceback_style);
                reporters.push(Box::new(ratatui_reporter));
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
    if tach_core::github::is_github_actions() {
        reporters.push(Box::new(tach_core::github::GitHubReporter::new()));
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
    let log_redirect = if *format != OutputFormat::Json
        && ProgressReporter::should_use_progress_bar()
        && config.verbose == 0
    {
        match LogRedirect::new() {
            Ok(redirect) => {
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
    reporter.on_phase("scanning", None);
    if !is_json {
        eprintln!("[tach:supervisor] Scanning {}...", cwd.display());
    }

    let start = std::time::Instant::now();
    let (discovery_result, mut toxicity_graph) =
        discover_with_toxicity_options(cwd, config.no_ignore)?;

    let tach_config = tach_core::config::load_tach_config(cwd);
    if let Some(ref tox) = tach_config.toxicity {
        toxicity_graph.apply_overrides(&tox.force_safe, &tox.force_toxic);
    }

    reporter.on_phase(
        "scanning",
        Some(&PhaseDetail {
            current: discovery_result.test_count(),
            total: discovery_result.test_count(),
            label: "tests".into(),
        }),
    );

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
    reporter.on_phase("compiling", None);
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
        reporter.on_phase(
            "compiling",
            Some(&PhaseDetail {
                current: compiled,
                total: py_files.len(),
                label: "files".into(),
            }),
        );
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
    reporter.on_phase("resolving", None);
    let fixture_registry = FixtureRegistry::from_discovery(&discovery_result);
    let resolver = Resolver::new(&fixture_registry);
    let (mut runnable_tests, errors) = resolver.resolve_all(&discovery_result);

    if config.resume {
        let completed = tach_core::cache::read_interrupted_cache(cwd);
        if !completed.is_empty() {
            let completed_set: std::collections::HashSet<&str> =
                completed.iter().map(|s| s.as_str()).collect();
            let before = runnable_tests.len();
            runnable_tests.retain(|t| !completed_set.contains(t.test_name.as_str()));
            if !is_json {
                eprintln!(
                    "[tach:resume] Skipping {} already-completed tests ({} remaining)",
                    before - runnable_tests.len(),
                    runnable_tests.len()
                );
            }
        }
    }

    reporter.on_phase(
        "resolving",
        Some(&PhaseDetail {
            current: runnable_tests.len(),
            total: runnable_tests.len() + errors.len(),
            label: "resolved".into(),
        }),
    );

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
    // Handle pytest-style node IDs: "file.py::Class::method"
    let (file_target, node_filter) = if target_path.contains("::") {
        let parts: Vec<&str> = target_path.splitn(2, "::").collect();
        (parts[0].to_string(), Some(parts[1].to_string()))
    } else {
        (target_path.to_string(), None)
    };

    let target = std::path::Path::new(&file_target);
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

            let path_matches = test_canonical.starts_with(&target_canonical)
                || test_canonical == target_canonical
                || test_path.starts_with(target);

            if !path_matches {
                return false;
            }

            if let Some(ref filter) = node_filter {
                test.test_name.contains(filter.as_str())
            } else {
                true
            }
        })
        .collect();

    if config.last_failed || config.failed_first {
        let last_failed = read_lastfailed_cache(cwd);
        if !last_failed.is_empty() {
            let cache_dir = cwd.join(".tach_cache");
            let _ = std::fs::create_dir_all(&cache_dir);
            let filter_file = cache_dir.join("_lf_filter.txt");
            let _ = std::fs::write(&filter_file, last_failed.join("\n"));
            let env_key = if config.last_failed {
                "TACH_LF_FILE"
            } else {
                "TACH_FF_FILE"
            };
            unsafe { std::env::set_var(env_key, filter_file.to_string_lossy().as_ref()) };
            if !is_json {
                let mode = if config.last_failed { "--lf" } else { "--ff" };
                eprintln!(
                    "[tach:supervisor] {}: {} last-failed tests",
                    mode,
                    last_failed.len()
                );
            }
        } else if !is_json {
            eprintln!("[tach:supervisor] No last-failed cache found, running all tests");
        }
    }

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
        std::process::exit(5);
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
    let stats = run_tests(
        &cleanup,
        filtered_tests,
        &mut reporter,
        is_json,
        config.coverage_enabled,
        config.memory_enabled,
        hook_registry,
        cwd.clone(),
        &toxicity_graph,
    )?;

    drop(log_redirect);

    if let Some(n) = config.durations
        && !is_json
    {
        let mut sorted = stats.test_durations.clone();
        sorted.sort_by(|a, b| b.1.cmp(&a.1));
        let show = if n == 0 {
            sorted.len()
        } else {
            n.min(sorted.len())
        };
        eprintln!("\n= slowest {} durations =", show);
        for (name, ms) in sorted.iter().take(show) {
            if *ms >= 1000 {
                eprintln!("{:.2}s {}", *ms as f64 / 1000.0, name);
            } else {
                eprintln!("{}ms {}", ms, name);
            }
        }
    }

    let _ = std::fs::remove_file(cwd.join(".tach_cache/_lf_filter.txt"));

    let was_interrupted = tach_core::signals::shutdown_requested();
    if was_interrupted {
        let completed: Vec<String> = stats
            .test_durations
            .iter()
            .map(|(name, _)| name.clone())
            .collect();
        tach_core::cache::write_interrupted_cache(cwd, &completed);
        if !is_json {
            eprintln!(
                "[tach:interrupted] Saved {} completed tests for --resume",
                completed.len()
            );
        }
    } else {
        tach_core::cache::clear_interrupted_cache(cwd);
    }

    // --- PYTEST FALLBACK ---
    // When tests fail in tach, retry them with vanilla pytest to distinguish
    // tach-specific failures from real test failures. This makes tach a true
    // drop-in replacement: users get tach's speed for passing tests and
    // pytest's compatibility for edge cases.
    let final_failed = if !stats.failed_test_ids.is_empty() && !config.no_fallback {
        pytest_fallback_retry(&stats, cwd, is_json, target_path)
    } else {
        stats.failed
    };

    // Write the lastfailed cache. If fallback ran, use only the REAL failures
    // (tests that failed in both tach and pytest). Otherwise use all tach failures.
    let results_file = cwd.join(".tach_cache/_fallback_results.txt");
    let results_file_tmp = Path::new("/tmp/_fallback_results.txt");
    let actual_results = if results_file.exists() {
        Some(results_file.as_path())
    } else if results_file_tmp.exists() {
        Some(results_file_tmp)
    } else {
        None
    };
    if let Some(rf) = actual_results {
        let real_failures = read_lastfailed_cache_from(rf);
        write_lastfailed_cache(cwd, &real_failures);
        let _ = std::fs::remove_file(rf);
    } else {
        write_lastfailed_cache(cwd, &stats.failed_test_ids);
    }

    if !stats.test_durations.is_empty() {
        write_duration_cache(cwd, &stats.test_durations);
    }

    if final_failed > 0 {
        std::process::exit(1);
    }

    Ok(())
}

fn handle_cache_show(cwd: &std::path::Path) -> Result<()> {
    let cache_dir = cwd.join(".tach_cache");
    eprintln!("cachedir: {}", cache_dir.display());
    eprintln!();

    let lastfailed = tach_core::cache::read_lastfailed_cache(cwd);
    if !lastfailed.is_empty() {
        eprintln!("lastfailed ({} entries):", lastfailed.len());
        for id in &lastfailed {
            eprintln!("  {id}");
        }
    } else {
        eprintln!("lastfailed: (empty)");
    }
    eprintln!();

    let durations = tach_core::cache::read_duration_cache(cwd);
    if !durations.is_empty() {
        eprintln!("durations ({} entries):", durations.len());
        let mut sorted: Vec<_> = durations.iter().collect();
        sorted.sort_by(|a, b| b.1.cmp(a.1));
        for (name, ms) in sorted.iter().take(20) {
            eprintln!("  {ms:>6}ms  {name}");
        }
        if sorted.len() > 20 {
            eprintln!("  ... and {} more", sorted.len() - 20);
        }
    } else {
        eprintln!("durations: (empty)");
    }

    Ok(())
}

fn handle_config_command(cwd: &std::path::Path, merged: &config::MergedConfig) -> Result<()> {
    if merged.format == config::OutputFormat::Json {
        return handle_config_json(cwd, merged);
    }
    eprintln!("rootdir: {}", cwd.display());
    eprintln!("version: {}", env!("CARGO_PKG_VERSION"));
    let pyproject = cwd.join("pyproject.toml");
    if pyproject.exists() {
        eprintln!("configfile: {}", pyproject.display());
    }
    eprintln!();
    eprintln!("[execution]");
    eprintln!("  workers = {}", merged.workers);
    eprintln!("  timeout = {}s", merged.timeout);
    eprintln!("  isolation = {}", merged.isolation_strategy);
    eprintln!("  force_toxic = {}", merged.force_toxic);
    eprintln!("  no_isolation = {}", merged.no_isolation);
    eprintln!("  no_fallback = {}", merged.no_fallback);
    eprintln!();
    eprintln!("[selection]");
    eprintln!("  path = {}", merged.path);
    eprintln!("  pattern = {}", merged.test_pattern);
    if let Some(ref k) = merged.keyword {
        eprintln!("  keyword = {k}");
    }
    if let Some(ref m) = merged.markers {
        eprintln!("  markers = {m}");
    }
    eprintln!("  exitfirst = {}", merged.exitfirst);
    if let Some(mf) = merged.maxfail {
        eprintln!("  maxfail = {mf}");
    }
    eprintln!();
    eprintln!("[output]");
    eprintln!("  traceback = {:?}", merged.traceback);
    eprintln!("  verbose = {}", merged.verbose);
    eprintln!("  show_locals = {}", merged.show_locals);
    eprintln!("  no_header = {}", merged.no_header);
    if let Some(d) = merged.durations {
        eprintln!("  durations = {d}");
    }
    eprintln!();
    eprintln!("[coverage]");
    eprintln!("  enabled = {}", merged.coverage);
    if !merged.coverage_source.is_empty() {
        eprintln!("  source = {:?}", merged.coverage_source);
    }
    if !merged.disabled_plugins.is_empty() {
        eprintln!();
        eprintln!("[plugins]");
        eprintln!("  disabled = {:?}", merged.disabled_plugins);
    }
    if merged.reuse_db || merged.create_db {
        eprintln!();
        eprintln!("[database]");
        eprintln!("  reuse_db = {}", merged.reuse_db);
        eprintln!("  create_db = {}", merged.create_db);
    }
    Ok(())
}

fn handle_config_json(cwd: &std::path::Path, m: &config::MergedConfig) -> Result<()> {
    let json = serde_json::json!({
        "rootdir": cwd.display().to_string(),
        "version": env!("CARGO_PKG_VERSION"),
        "workers": m.workers,
        "timeout": m.timeout,
        "isolation": &m.isolation_strategy,
        "path": &m.path,
        "pattern": &m.test_pattern,
        "exitfirst": m.exitfirst,
        "coverage": m.coverage,
        "no_fallback": m.no_fallback,
        "force_toxic": m.force_toxic,
        "no_isolation": m.no_isolation,
    });
    println!("{json}");
    Ok(())
}

fn handle_clean_command(cwd: &std::path::Path) -> Result<()> {
    let cache_dir = cwd.join(".tach_cache");
    if cache_dir.exists() {
        std::fs::remove_dir_all(&cache_dir)?;
        eprintln!("Removed {}", cache_dir.display());
    } else {
        eprintln!("No .tach_cache to clean");
    }

    let mut pycache_count = 0usize;
    clean_pycache_recursive(cwd, &mut pycache_count);
    if pycache_count > 0 {
        eprintln!("Removed {} __pycache__ directories", pycache_count);
    }
    Ok(())
}

fn clean_pycache_recursive(dir: &std::path::Path, count: &mut usize) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if entry.file_name() == "__pycache__" {
                let _ = std::fs::remove_dir_all(&path);
                *count += 1;
            } else if entry.file_name() != ".git" && entry.file_name() != "node_modules" {
                clean_pycache_recursive(&path, count);
            }
        }
    }
}

fn handle_markers_command(cwd: &std::path::Path, no_ignore: bool) -> Result<()> {
    let result = tach_core::discovery::scanner::discover(cwd, no_ignore)?;
    let mut markers = std::collections::BTreeSet::new();
    for module in &result.modules {
        for test in &module.tests {
            for marker in &test.markers {
                markers.insert(marker.clone());
            }
        }
    }
    if markers.is_empty() {
        eprintln!("no markers found");
    } else {
        for marker in &markers {
            println!("@pytest.mark.{marker}");
        }
        eprintln!("\n{} markers found", markers.len());
    }
    Ok(())
}

fn handle_fixtures_command(cwd: &std::path::Path, no_ignore: bool) -> Result<()> {
    let result = tach_core::discovery::scanner::discover(cwd, no_ignore)?;
    let mut fixtures = std::collections::BTreeMap::new();
    for module in &result.modules {
        for fixture in &module.fixtures {
            fixtures.entry(fixture.name.clone()).or_insert_with(|| {
                (
                    format!("{:?}", fixture.scope),
                    module.path.display().to_string(),
                )
            });
        }
    }
    if fixtures.is_empty() {
        eprintln!("no fixtures found");
    } else {
        for (name, (scope, file)) in &fixtures {
            println!("{name} [{scope}] -- {file}");
        }
        eprintln!("\n{} fixtures found", fixtures.len());
    }
    Ok(())
}

fn handle_init_command(cwd: &std::path::Path) -> Result<()> {
    let pyproject_path = cwd.join("pyproject.toml");
    if pyproject_path.exists() {
        let content = std::fs::read_to_string(&pyproject_path)?;
        if content.contains("[tool.tach]") || content.contains("[tool.tach.") {
            eprintln!("[tool.tach] already exists in pyproject.toml");
            return Ok(());
        }
        let config = detect_project_config(cwd);
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&pyproject_path)?;
        std::io::Write::write_all(&mut file, config.as_bytes())?;
        eprintln!("Added [tool.tach] to {}", pyproject_path.display());
    } else {
        let config = detect_project_config(cwd);
        std::fs::write(&pyproject_path, &config)?;
        eprintln!("Created {}", pyproject_path.display());
    }
    Ok(())
}

fn detect_project_config(cwd: &std::path::Path) -> String {
    let has_django = cwd.join("manage.py").exists() || has_dependency(cwd, "django");
    let has_flask = has_dependency(cwd, "flask");

    let mut config = String::from("\n[tool.tach]\ntimeout = 60\nworkers = 0\n");
    config.push_str("isolation_strategy = \"auto\"\n");

    if has_django {
        config.push_str("reuse_db = true\n");
        eprintln!("  Detected Django project");
    }
    if has_flask {
        config.push_str("force_toxic = true\n");
        eprintln!("  Detected Flask project");
    }

    config.push_str("\n[tool.tach.coverage]\nenabled = false\nsource = [\".\"]\n");
    config
}

fn has_dependency(cwd: &std::path::Path, name: &str) -> bool {
    let pyproject = cwd.join("pyproject.toml");
    if let Ok(content) = std::fs::read_to_string(pyproject) {
        for line in content.lines() {
            let t = line.trim().trim_matches('"').trim_matches('\'').trim();
            if t == name
                || t.starts_with(&format!("{name}>="))
                || t.starts_with(&format!("{name}["))
            {
                return true;
            }
        }
    }
    let req = cwd.join("requirements.txt");
    if let Ok(content) = std::fs::read_to_string(req) {
        return content.lines().any(|l| {
            let l = l.trim().to_lowercase();
            l.starts_with(name)
                && (l.len() == name.len()
                    || l.as_bytes()
                        .get(name.len())
                        .is_some_and(|&b| b == b'=' || b == b'>' || b == b'<' || b == b'['))
        });
    }
    false
}

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
    let quiet = std::env::var("TACH_QUIET").is_ok();
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
                println!("{}::{}", module.path.display(), test.name);
            }
        }
        if !quiet {
            println!("\n{} tests collected", discovery_result.test_count());
        }
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
    toxicity_graph: &ToxicityGraph,
) -> Result<tach_core::scheduler::SchedulerStats> {
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

    reporter.on_phase("booting", None);
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

            // COLLECTED TESTS IPC (Issue #98): Receive pytest's authoritative test list
            let mut collected_header = [0u8; tach_core::protocol::HEADER_SIZE];
            cmd_sock_clone.read_exact(&mut collected_header)?;

            let collected_len = u32::from_le_bytes([
                collected_header[4],
                collected_header[5],
                collected_header[6],
                collected_header[7],
            ]) as usize;

            let collected_tests: Vec<tach_core::protocol::CollectedTest> = if collected_len > 0 {
                let mut collected_buf = vec![0u8; tach_core::protocol::HEADER_SIZE + collected_len];
                collected_buf[..tach_core::protocol::HEADER_SIZE]
                    .copy_from_slice(&collected_header);
                cmd_sock_clone
                    .read_exact(&mut collected_buf[tach_core::protocol::HEADER_SIZE..])?;

                tach_core::protocol::decode_with_limit(
                    &collected_buf,
                    tach_core::protocol::MAX_PAYLOAD_SIZE,
                )
                .unwrap_or_else(|e| {
                    eprintln!(
                        "[tach:supervisor] Warning: Failed to decode collected tests: {}",
                        e
                    );
                    vec![]
                })
            } else {
                vec![]
            };

            if !is_json {
                eprintln!(
                    "[tach:supervisor] Received {} collected tests from Zygote (pytest-authoritative)",
                    collected_tests.len()
                );
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
            let global_timeout = std::env::var("TACH_TIMEOUT")
                .ok()
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or_else(|| tach_config.timeout());
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

            let maxfail_env = std::env::var("TACH_MAXFAIL")
                .ok()
                .and_then(|v| v.parse::<usize>().ok());
            scheduler.set_maxfail(maxfail_env);

            let dur_cache = read_duration_cache(&cwd);
            if !dur_cache.is_empty() {
                scheduler.set_duration_cache(dur_cache);
            }

            let collection_filtered = std::env::var("TACH_KEYWORD").is_ok()
                || std::env::var("TACH_MARKERS").is_ok()
                || std::env::var("TACH_LF_FILE").is_ok()
                || std::env::var("TACH_PYTEST_ARGS").is_ok();

            let runnable_tests = if !collected_tests.is_empty() || collection_filtered {
                let mut rust_index: std::collections::HashMap<String, resolver::RunnableTest> =
                    std::collections::HashMap::new();
                for test in runnable_tests {
                    let node_id =
                        format!("{}::{}", test.file_path.to_string_lossy(), test.test_name);
                    rust_index.insert(node_id, test);
                }

                let mut merged: Vec<resolver::RunnableTest> =
                    Vec::with_capacity(collected_tests.len());
                let mut python_only_count = 0usize;

                for ct in &collected_tests {
                    if let Some(rust_test) = rust_index.remove(&ct.node_id) {
                        merged.push(rust_test);
                    } else {
                        python_only_count += 1;

                        let test_name = ct
                            .node_id
                            .split_once("::")
                            .map(|(_, name)| name.to_string())
                            .unwrap_or_else(|| ct.node_id.clone());

                        let file_path = std::path::PathBuf::from(&ct.file_path);

                        let is_toxic = toxicity_graph.is_toxic(&file_path)
                            || ct.markers.iter().any(|m| m == "django_db");

                        merged.push(resolver::RunnableTest {
                            file_path,
                            test_name,
                            is_async: ct.is_async,
                            fixtures: vec![],
                            is_toxic,
                            timeout_secs: None,
                            markers: ct.markers.clone(),
                            marker_info: vec![],
                        });
                    }
                }

                if !is_json {
                    eprintln!(
                        "[tach:supervisor] Merged test list: {} total ({} from Rust+Python, {} Python-only)",
                        merged.len(),
                        merged.len() - python_only_count,
                        python_only_count
                    );
                }

                merged
            } else {
                if !is_json {
                    eprintln!(
                        "[tach:supervisor] Warning: pytest collected 0 tests, using Rust-only discovery ({} tests)",
                        runnable_tests.len()
                    );
                }
                runnable_tests
            };

            let stats = scheduler.run(runnable_tests, reporter)?;

            // Shutdown
            scheduler.shutdown()?;
            waitpid(zygote_pid, None)?;

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

            // Mark shutdown as complete to prevent watchdog from force-exiting
            signals::mark_shutdown_complete();

            Ok(stats)
        }
    }
}
