use tach_core::config::{self, Cli, Commands, OutputFormat};
use tach_core::debugger::{self, DebugServer};
use tach_core::discovery;
use tach_core::junit::JunitReporter;
use tach_core::lifecycle::CleanupGuard;
use tach_core::logcapture::LogCapture;
use tach_core::reporter::{HumanReporter, JsonReporter, MultiReporter, Reporter};
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
use std::path::PathBuf;
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
                eprintln!(
                    "[supervisor] WARN: Failed to create UFFD listener: {}. Snapshot mode disabled.",
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
    // PHASE 1.1: FORCE SYSTEM ALLOCATOR (Snapshot-Hypervisor "Physics" Fix)
    // ==========================================================================
    // Python's obmalloc desynchronizes during snapshot/fork operations.
    // glibc's malloc is robust if we disable the thread cache (tcache).
    // These MUST be set before ANY Python operations or fork() calls.
    // See: implementation_plan.md Phase 1 for details.
    std::env::set_var("PYTHONMALLOC", "malloc");
    std::env::set_var("GLIBC_TUNABLES", "glibc.malloc.tcache_count=0");

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
            eprintln!(
                "[supervisor] Warning: Failed to install signal handlers: {}",
                e
            );
        }
    }

    let cwd = std::env::current_dir()?;

    // Handle `list` subcommand (no watch mode)
    if let Some(Commands::List) = cli.command {
        return handle_list_command(&cwd, is_json);
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

        return watch::start_watch_loop(&cwd, move || {
            execute_session(&cwd_clone, &format, &junit_path, &path_clone)
        });
    }

    // --- SINGLE RUN MODE ---
    execute_session(&cwd, &cli.format, &cli.junit_xml, &cli.path)
}

/// Execute a complete test session (discovery → resolution → zygote → run)
/// This is the reusable function that watch mode calls repeatedly.
fn execute_session(
    cwd: &PathBuf,
    format: &OutputFormat,
    junit_path: &Option<PathBuf>,
    target_path: &str,
) -> Result<()> {
    let is_json = *format == OutputFormat::Json;

    // Create reporters
    let mut reporters: Vec<Box<dyn Reporter>> = Vec::new();
    match format {
        OutputFormat::Json => reporters.push(Box::new(JsonReporter)),
        OutputFormat::Human => reporters.push(Box::new(HumanReporter)),
    }
    if let Some(path) = junit_path {
        reporters.push(Box::new(JunitReporter::new(path.clone())));
    }
    let mut reporter = MultiReporter::new(reporters);

    let cleanup = CleanupGuard::new();

    // --- DISCOVERY PHASE ---
    if !is_json {
        eprintln!("[supervisor] Scanning {}...", cwd.display());
    }

    let start = std::time::Instant::now();
    let discovery_result = discovery::discover(cwd)?;

    if !is_json {
        eprintln!(
            "[supervisor] Discovered {} tests, {} fixtures in {:?}",
            discovery_result.test_count(),
            discovery_result.fixture_count(),
            start.elapsed()
        );
    }

    // --- RESOLUTION PHASE ---
    let registry = FixtureRegistry::from_discovery(&discovery_result);
    let resolver = Resolver::new(&registry);
    let (runnable_tests, errors) = resolver.resolve_all(&discovery_result);

    if !is_json {
        eprintln!(
            "[supervisor] Resolved {} tests ({} errors)",
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
            test_canonical.starts_with(&target_canonical) || 
            test_canonical == target_canonical ||
            // Handle relative path matching
            test_path.starts_with(target)
        })
        .collect();

    if !is_json {
        eprintln!("[supervisor] Selected {} tests to run (filtered by path: {})", 
            filtered_tests.len(), target_path);
    }

    if filtered_tests.is_empty() {
        if !is_json {
            eprintln!("[supervisor] No tests found matching path: {}", target_path);
        }
        return Ok(());
    }

    // --- RUN TESTS ---
    run_tests(&cleanup, filtered_tests, &mut reporter, is_json)
}

/// Handle the `list` subcommand
fn handle_list_command(cwd: &PathBuf, is_json: bool) -> Result<()> {
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

fn run_tests(
    cleanup: &CleanupGuard,
    runnable_tests: Vec<resolver::RunnableTest>,
    reporter: &mut dyn Reporter,
    is_json: bool,
) -> Result<()> {
    let cwd = std::env::current_dir()?;

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
            let mut scheduler = Scheduler::new(
                sup_cmd_sock,
                sup_result_sock,
                log_capture,
                debug_socket_path,
            )?;

            scheduler.run(runnable_tests, reporter)?;

            // Shutdown
            scheduler.shutdown()?;
            waitpid(zygote_pid, None)?;

            if !is_json {
                eprintln!("[supervisor] Done.");
            }
        }
    }

    Ok(())
}
