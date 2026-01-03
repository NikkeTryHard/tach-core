use tach_core::config::{self, Cli, Commands, OutputFormat};
use tach_core::coverage;
use tach_core::debugger::{self, DebugServer};
use tach_core::discover_with_toxicity;
use tach_core::discovery;
use tach_core::junit::JunitReporter;
use tach_core::lifecycle::CleanupGuard;
use tach_core::loader;
use tach_core::logcapture::LogCapture;
use tach_core::reporter::{DotsReporter, JsonReporter, MultiReporter, ProgressReporter, Reporter};
use tach_core::resolver::{self, FixtureRegistry, Resolver};
use tach_core::scheduler::Scheduler;
use tach_core::signals;
use tach_core::watch;
use tach_core::zygote;

use anyhow::Result;
use clap::Parser;
use nix::sys::wait::waitpid;
use nix::unistd::{fork, ForkResult};
use std::io::Read;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use uuid::Uuid;

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
                std::env::set_var("TACH_SUPERVISOR_SOCK", &uffd_sock_path);
                Some(listener)
            }
            Err(e) => {
                eprintln!("[supervisor] WARN: Failed to create UFFD listener: {}. Snapshot mode disabled.", e);
                None
            }
        };

        Ok(Self { run_dir, uffd_sock_path, uffd_listener })
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
            eprintln!("[supervisor] Jemalloc {} verified - Hypervisor allocator ready", version);
        }
        Err(e) => {
            eprintln!("[supervisor] FATAL: {}", e);
            eprintln!("[supervisor] The Hypervisor cannot run without jemalloc.");
            eprintln!("[supervisor] Ensure tikv-jemallocator is set as #[global_allocator] in lib.rs");
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

    // Set TACH_NO_ISOLATION env var from CLI flag (inherits to all children)
    if cli.no_isolation {
        std::env::set_var("TACH_NO_ISOLATION", "1");
    }

    // Set TACH_TARGET_PATH for Zygote to know which path to collect tests from
    std::env::set_var("TACH_TARGET_PATH", &cli.path);

    // --- PHASE 4.2: LIFECYCLE SETUP ---
    debugger::install_panic_hook();

    if let Err(e) = signals::install_signal_handlers() {
        if !is_json {
            eprintln!("[supervisor] Warning: Failed to install signal handlers: {}", e);
        }
    }

    let cwd = std::env::current_dir()?;

    // Handle subcommands
    match &cli.command {
        Some(Commands::List) => {
            return handle_list_command(&cwd, is_json);
        }
        Some(Commands::SelfTest) => {
            return handle_self_test_command();
        }
        Some(Commands::Version) => {
            return handle_version_command();
        }
        Some(Commands::Test) | None => {
            // Continue to test execution below
        }
    }

    // --- WATCH MODE ---
    if is_watch {
        if is_json {
            eprintln!("[tach] Warning: JSON output not recommended in watch mode");
        }

        // Clone config values for the closure
        let junit_path = cli.junit_xml.clone();
        let format = cli.format.clone();
        let cwd_clone = cwd.clone();
        let path_clone = cli.path.clone();

        return watch::start_watch_loop(&cwd, move || execute_session(&cwd_clone, &format, &junit_path, &path_clone, false));
    }

    // --- SINGLE RUN MODE ---
    execute_session(&cwd, &cli.format, &cli.junit_xml, &cli.path, cli.coverage)
}

/// Execute a complete test session (discovery → resolution → zygote → run)
/// This is the reusable function that watch mode calls repeatedly.
fn execute_session(cwd: &PathBuf, format: &OutputFormat, junit_path: &Option<PathBuf>, target_path: &str, coverage_enabled: bool) -> Result<()> {
    let is_json = *format == OutputFormat::Json;

    // Create reporters
    //  Use ProgressReporter for interactive terminals, DotsReporter for CI
    let mut reporters: Vec<Box<dyn Reporter>> = Vec::new();
    match format {
        OutputFormat::Json => reporters.push(Box::new(JsonReporter)),
        OutputFormat::Human => {
            // Use progress bar for interactive terminals, dots for CI
            if ProgressReporter::should_use_progress_bar() {
                reporters.push(Box::new(ProgressReporter::new()));
            } else {
                reporters.push(Box::new(DotsReporter::new()));
            }
        }
    }
    if let Some(path) = junit_path {
        reporters.push(Box::new(JunitReporter::new(path.clone())));
    }
    let mut reporter = MultiReporter::new(reporters);

    let cleanup = CleanupGuard::new();

    // --- DISCOVERY PHASE (with Toxicity Analysis) ---
    if !is_json {
        eprintln!("[supervisor] Scanning {}...", cwd.display());
    }

    let start = std::time::Instant::now();
    let (discovery_result, toxicity_graph) = discover_with_toxicity(cwd)?;

    if !is_json {
        let toxic_count = toxicity_graph.toxic_modules().len();
        let safe_count = toxicity_graph.safe_modules().len();
        eprintln!("[supervisor] Discovered {} tests, {} fixtures in {:?} (toxic: {}, safe: {})", discovery_result.test_count(), discovery_result.fixture_count(), start.elapsed(), toxic_count, safe_count);
    }

    // --- PHASE 2: EAGER COMPILATION (Zero-Copy Loader) ---
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
            eprintln!("[supervisor] Compiled {} of {} modules for zero-copy loading in {:?}", compiled, py_files.len(), start_compile.elapsed());
        }
    } else if !is_json {
        eprintln!("[supervisor] WARN: Failed to create bytecode compiler, falling back to importlib");
    }

    // --- RESOLUTION PHASE ---
    let fixture_registry = FixtureRegistry::from_discovery(&discovery_result);
    let resolver = Resolver::new(&fixture_registry);
    let (runnable_tests, errors) = resolver.resolve_all(&discovery_result);

    if !is_json {
        eprintln!("[supervisor] Resolved {} tests ({} errors)", runnable_tests.len(), errors.len());

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

    // --- PHASE 3.3: TOXICITY TAGGING ---
    // Tag each resolved test with its toxicity status from the graph.
    // Toxic tests use fork/kill instead of snapshot/reset.
    let mut runnable_tests = runnable_tests;
    let mut toxic_test_count = 0;
    for test in &mut runnable_tests {
        test.is_toxic = toxicity_graph.is_toxic(&test.file_path);
        if test.is_toxic {
            toxic_test_count += 1;
        }
    }

    if !is_json && toxic_test_count > 0 {
        eprintln!("[supervisor] Toxicity: {} of {} tests marked toxic (will use fork/kill)", toxic_test_count, runnable_tests.len());
    }

    // --- PHASE 8.3: PATH FILTERING ---
    // Filter tests to only include those matching the target path
    let target = std::path::Path::new(target_path);
    let target_canonical = target.canonicalize().unwrap_or_else(|_| target.to_path_buf());

    let filtered_tests: Vec<resolver::RunnableTest> = runnable_tests
        .into_iter()
        .filter(|test| {
            let test_path = std::path::Path::new(&test.file_path);
            let test_canonical = test_path.canonicalize().unwrap_or_else(|_| test_path.to_path_buf());

            // Match if test is under target directory OR matches exactly
            test_canonical.starts_with(&target_canonical)
                || test_canonical == target_canonical
                ||
            // Handle relative path matching
            test_path.starts_with(target)
        })
        .collect();

    if !is_json {
        eprintln!("[supervisor] Selected {} tests to run (filtered by path: {})", filtered_tests.len(), target_path);
    }

    if filtered_tests.is_empty() {
        if !is_json {
            eprintln!("[supervisor] No tests found matching path: {}", target_path);
        }
        return Ok(());
    }

    // --- RUN TESTS ---
    run_tests(&cleanup, filtered_tests, &mut reporter, is_json, coverage_enabled)
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

/// Handle the `version` subcommand
fn handle_version_command() -> Result<()> {
    eprintln!("tach {}", env!("CARGO_PKG_VERSION"));
    eprintln!("Hypervisor-Accelerated Python Test Runner");
    eprintln!();

    // Show jemalloc status
    match tach_core::allocator::verify_jemalloc_active() {
        Ok(version) => eprintln!("Allocator: Jemalloc {}", version),
        Err(_) => eprintln!("Allocator: System (not jemalloc)"),
    }

    // Show Python version if available
    let python_path = std::env::var("PYO3_PYTHON").or_else(|_| std::env::var("PYTHON")).unwrap_or_else(|_| "python3".to_string());

    if let Ok(output) = std::process::Command::new(&python_path).args(["--version"]).output() {
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

    eprintln!();
    eprintln!("Run 'tach self-test' for full system diagnostics.");

    Ok(())
}

/// Handle the `list` subcommand
fn handle_list_command(cwd: &Path, is_json: bool) -> Result<()> {
    let discovery_result = discovery::discover(cwd)?;

    if is_json {
        discovery::dump_json(&discovery_result)?;
    } else {
        for module in &discovery_result.modules {
            for test in &module.tests {
                eprintln!("{}::{}", module.path.display(), test.name);
            }
        }
    }
    Ok(())
}

fn run_tests(cleanup: &CleanupGuard, runnable_tests: Vec<resolver::RunnableTest>, reporter: &mut dyn Reporter, is_json: bool, coverage_enabled: bool) -> Result<()> {
    let cwd = std::env::current_dir()?;

    // --- PHASE 6.1: COVERAGE INITIALIZATION ---
    // Initialize coverage ring buffers BEFORE forking Zygote.
    // These are shared memory regions (memfd) that workers will inherit via fork.
    let mut coverage_aggregator: Option<coverage::CoverageAggregator> = None;

    if coverage_enabled {
        if !is_json {
            eprintln!("[supervisor] Initializing coverage collection...");
        }

        // Initialize coverage ring buffer (LINE events)
        match coverage::init_coverage_buffer(coverage::DEFAULT_CAPACITY) {
            Ok(_) => {
                if !is_json {
                    eprintln!("[supervisor] Coverage buffer: {} entries ({} bytes)", coverage::DEFAULT_CAPACITY, coverage::DEFAULT_CAPACITY * coverage::ENTRY_SIZE + coverage::HEADER_SIZE);
                }
            }
            Err(e) => {
                eprintln!("[supervisor] WARNING: Failed to init coverage buffer: {}", e);
            }
        }

        // Initialize mapping ring buffer (PY_START events for code_id -> filename)
        match coverage::init_mapping_buffer(coverage::MAPPING_CAPACITY) {
            Ok(_) => {
                if !is_json {
                    eprintln!("[supervisor] Mapping buffer: {} entries ({} bytes)", coverage::MAPPING_CAPACITY, coverage::MAPPING_CAPACITY * coverage::MAPPING_ENTRY_SIZE + coverage::HEADER_SIZE);
                }
            }
            Err(e) => {
                eprintln!("[supervisor] WARNING: Failed to init mapping buffer: {}", e);
            }
        }

        // Start aggregator thread to drain buffers
        let mut aggregator = coverage::CoverageAggregator::new();
        aggregator.start(std::time::Duration::from_millis(100));
        coverage_aggregator = Some(aggregator);

        // Set env var so workers know to enable coverage
        std::env::set_var("TACH_COVERAGE", "1");
    }

    // --- CREATE DEBUG SERVER ---
    let debug_server = DebugServer::new()?;
    let debug_socket_path = debug_server.socket_path().to_path_buf();
    cleanup.track_socket(debug_socket_path.clone());

    // --- CREATE LOG CAPTURE ---
    let max_workers = num_cpus::get().min(runnable_tests.len()).max(1);
    let log_capture = LogCapture::new(max_workers)?;

    if !is_json {
        eprintln!("[supervisor] Created {} log buffers (memfd)", max_workers);
    }

    // --- SOCKET PAIRS ---
    let (sup_cmd_sock, zyg_cmd_sock) = UnixStream::pair()?;
    let (sup_result_sock, zyg_result_sock) = UnixStream::pair()?;

    // --- LOAD CONFIG ---
    config::load_env_from_pyproject(&cwd);

    // --- NO-ISOLATION MODE ---
    // Set env var so workers can check it (must be before fork to inherit)
    if std::env::var("TACH_NO_ISOLATION").unwrap_or_default() == "1" {
        eprintln!("[supervisor] Isolation disabled via TACH_NO_ISOLATION");
    }

    // --- CREATE RUN CONTEXT (Snapshot Mode) ---
    // This creates the UFFD listener socket and sets TACH_SUPERVISOR_SOCK env var
    // Must be before fork so the env var is inherited by Zygote
    let run_context = RunContext::new()?;
    if run_context.snapshot_enabled() && !is_json {
        eprintln!("[supervisor] Snapshot mode enabled: {}", run_context.uffd_sock_path.display());
    }

    if !is_json {
        eprintln!("[supervisor] Forking Zygote...");
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
                eprintln!("[zygote] Error: {:?}", e);
                std::process::exit(1);
            }
            std::process::exit(0);
        }
        ForkResult::Parent { child: zygote_pid } => {
            drop(zyg_cmd_sock);
            drop(zyg_result_sock);

            cleanup.set_zygote_pid(zygote_pid.as_raw());

            if !is_json {
                eprintln!("[supervisor] Zygote PID: {}", zygote_pid);
            }

            // Wait for READY
            let mut ready_buf = [0u8; 1];
            let mut cmd_sock_clone = sup_cmd_sock.try_clone()?;
            cmd_sock_clone.read_exact(&mut ready_buf)?;

            if ready_buf[0] == 0x42 && !is_json {
                eprintln!("[supervisor] Zygote is READY.\n");
            }

            // --- SCHEDULER PHASE ---
            let mut scheduler = Scheduler::new(sup_cmd_sock, sup_result_sock, log_capture, debug_socket_path)?;

            scheduler.run(runnable_tests, reporter)?;

            // Shutdown
            scheduler.shutdown()?;
            waitpid(zygote_pid, None)?;

            // --- PHASE 6.1: COVERAGE FINALIZATION ---
            // Stop aggregator and report coverage statistics
            if let Some(mut aggregator) = coverage_aggregator {
                aggregator.stop();
                // Use take_data() to avoid cloning the entire HashMap
                let coverage_data = aggregator.take_data();
                let total_hits: u64 = coverage_data.values().sum();

                if !is_json {
                    eprintln!("[supervisor] Coverage: {} unique lines covered, {} total hits", coverage_data.len(), total_hits);

                    // Report overflow counts if any
                    if let Some(buffer) = coverage::get_coverage_buffer() {
                        let overflow = buffer.overflow_count();
                        if overflow > 0 {
                            eprintln!("[supervisor] WARNING: {} coverage entries dropped (buffer overflow)", overflow);
                        }
                    }
                    if let Some(buffer) = coverage::get_mapping_buffer() {
                        let overflow = buffer.overflow_count();
                        if overflow > 0 {
                            eprintln!("[supervisor] WARNING: {} mapping entries dropped (buffer overflow)", overflow);
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
                            "[supervisor] WARNING: Failed to write coverage report: {}",
                            e
                        );
                    } else if !is_json {
                        eprintln!(
                            "[supervisor] Coverage report written to: {}",
                            output_path.display()
                        );
                    }
                }
            }

            if !is_json {
                eprintln!("[supervisor] Done.");
            }
        }
    }

    Ok(())
}
