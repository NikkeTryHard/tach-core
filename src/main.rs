mod discovery;
mod logcapture;
mod protocol;
mod resolver;
mod scheduler;
mod zygote;

use anyhow::Result;
use logcapture::LogCapture;
use nix::sys::wait::waitpid;
use nix::unistd::{fork, ForkResult};
use resolver::{FixtureRegistry, Resolver};
use scheduler::Scheduler;
use std::io::Read;
use std::os::unix::net::UnixStream;

fn main() -> Result<()> {
    // --- DISCOVERY PHASE ---
    let cwd = std::env::current_dir()?;
    eprintln!("[supervisor] Scanning {}...", cwd.display());

    let start = std::time::Instant::now();
    let discovery_result = discovery::discover(&cwd)?;
    eprintln!(
        "[supervisor] Discovered {} tests, {} fixtures in {:?}",
        discovery_result.test_count(),
        discovery_result.fixture_count(),
        start.elapsed()
    );

    // --- RESOLUTION PHASE ---
    let registry = FixtureRegistry::from_discovery(&discovery_result);
    let resolver = Resolver::new(&registry);
    let (runnable_tests, errors) = resolver.resolve_all(&discovery_result);

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

    if runnable_tests.is_empty() {
        eprintln!("[supervisor] No tests to run.");
        return Ok(());
    }

    // --- CREATE LOG CAPTURE BEFORE FORK ---
    let max_workers = num_cpus::get().min(runnable_tests.len()).max(1);
    let log_capture = LogCapture::new(max_workers)?;
    eprintln!("[supervisor] Created {} log buffers (memfd)", max_workers);

    // --- CREATE DUAL SOCKET PAIRS ---
    let (sup_cmd_sock, zyg_cmd_sock) = UnixStream::pair()?;
    let (sup_result_sock, zyg_result_sock) = UnixStream::pair()?;

    eprintln!("[supervisor] Forking Zygote...");

    match unsafe { fork() }? {
        ForkResult::Child => {
            drop(sup_cmd_sock);
            drop(sup_result_sock);
            std::mem::forget(log_capture); // Don't close FDs

            if let Err(e) = zygote::entrypoint(zyg_cmd_sock, zyg_result_sock) {
                eprintln!("[zygote] Error: {:?}", e);
                std::process::exit(1);
            }
            std::process::exit(0);
        }
        ForkResult::Parent { child: zygote_pid } => {
            drop(zyg_cmd_sock);
            drop(zyg_result_sock);
            eprintln!("[supervisor] Zygote PID: {}", zygote_pid);

            // Wait for READY
            let mut ready_buf = [0u8; 1];
            let mut cmd_sock_clone = sup_cmd_sock.try_clone()?;
            cmd_sock_clone.read_exact(&mut ready_buf)?;
            if ready_buf[0] == 0x42 {
                eprintln!("[supervisor] Zygote is READY.\n");
            }

            // --- SCHEDULER PHASE ---
            let mut scheduler = Scheduler::new(sup_cmd_sock, sup_result_sock, log_capture)?;

            scheduler.run(runnable_tests)?;

            // Shutdown
            scheduler.shutdown()?;
            waitpid(zygote_pid, None)?;
            eprintln!("[supervisor] Done.");
        }
    }

    Ok(())
}
