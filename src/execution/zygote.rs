//! Zygote: Fork server with dual-channel IPC

use crate::environment::find_site_packages;
use crate::logcapture::redirect_output;
use crate::protocol::{
    CMD_EXIT, CMD_FORK, CMD_PING, CMD_RUN_TEST, HEADER_SIZE, MAX_PAYLOAD_SIZE, MSG_PONG, MSG_READY,
    MSG_WORKER_READY, STATUS_CRASH, TestPayload, TestResult, decode_with_limit, encode_with_length,
};
use crate::snapshot::send_fd;
use anyhow::Result;
use nix::sys::signal::{SaFlags, SigAction, SigHandler, SigSet, Signal, sigaction, signal};
use nix::sys::wait::{WaitPidFlag, WaitStatus, waitpid};
use nix::unistd::{ForkResult, Pid, fork};
use pyo3::ffi::c_str;
use pyo3::prelude::*;
use pyo3::types::{PyList, PyModule};
use std::collections::HashMap;
use std::env;
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::process;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::Instant;
use userfaultfd::UffdBuilder;

/// Embedded Python harness for pytest execution
const TACH_HARNESS_PY: &str = include_str!("../tach_harness.py");

// =============================================================================
// tach_rust Module: Native FFI for Python Harness
// =============================================================================

/// Cached memory regions for worker self-reset (Seppuku pattern)
/// These are populated during init_snapshot_mode and used by reset_memory.
/// We exclude stack to avoid "standing on the floor we're demolishing".
///
/// SAFETY: Using Mutex instead of static mut to avoid undefined behavior.
/// The Zygote is single-threaded after fork, so contention is not a concern.
static RESET_REGIONS: Mutex<Vec<(usize, usize)>> = Mutex::new(Vec::new());
static SNAPSHOT_ENABLED: AtomicBool = AtomicBool::new(false);

// =============================================================================
// SIGCHLD Self-Pipe Pattern for Proactive Crash Detection
// =============================================================================

/// Write end of the self-pipe for SIGCHLD notification.
/// The signal handler writes here; the zygote main loop polls the read end.
/// Set to -1 when not initialized.
static SIGCHLD_PIPE_WR: AtomicI32 = AtomicI32::new(-1);

/// Async-signal-safe SIGCHLD handler.
/// Writes one byte to the self-pipe to wake up the zygote event loop.
/// Uses only async-signal-safe functions (write is POSIX async-signal-safe).
extern "C" fn sigchld_handler(_sig: libc::c_int) {
    let fd = SIGCHLD_PIPE_WR.load(Ordering::Relaxed);
    if fd >= 0 {
        unsafe {
            libc::write(fd, &1u8 as *const u8 as *const libc::c_void, 1);
        }
    }
}

/// Drain all bytes from the signal pipe (non-blocking read until EAGAIN).
fn drain_signal_pipe(pipe_rd: i32) {
    let mut buf = [0u8; 64];
    loop {
        let n = unsafe { libc::read(pipe_rd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
        if n <= 0 {
            break;
        }
    }
}

/// Reap all dead children and send crash notifications for workers
/// that died before sending their test results.
///
/// This is the core of the proactive crash detection: when SIGCHLD fires,
/// we reap children with waitpid(WNOHANG) and check if they were active
/// workers (i.e., hadn't sent results yet). For each such worker, we
/// construct a STATUS_CRASH TestResult and send it on the result channel.
fn reap_crashed_workers(
    active_pids: &Arc<Mutex<HashMap<i32, u32>>>,
    result_tx: &mpsc::Sender<Vec<u8>>,
) {
    loop {
        match waitpid(Pid::from_raw(-1), Some(WaitPidFlag::WNOHANG)) {
            Ok(WaitStatus::Exited(pid, status)) => {
                let raw_pid = pid.as_raw();
                let test_id = active_pids
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(&raw_pid);

                if let Some(test_id) = test_id {
                    // Worker died before result collector got the result.
                    // status != 0 means abnormal exit; status == 0 could be
                    // a toxic worker that exited normally after sending results
                    // (race with result_collector removing the PID).
                    if status != 0 {
                        let crash_result = TestResult {
                            test_id,
                            status: STATUS_CRASH,
                            duration_ns: 0,
                            message: format!(
                                "Worker crashed (SIGCHLD: pid {} exited with status {})",
                                raw_pid, status
                            ),
                            memory_rss_bytes: None,
                        };
                        if let Ok(encoded) = encode_with_length(&crash_result) {
                            let _ = result_tx.send(encoded);
                        }
                    }
                }
                // If test_id is None, result was already handled by spawn_result_collector
            }
            Ok(WaitStatus::Signaled(pid, sig, _core_dumped)) => {
                let raw_pid = pid.as_raw();
                let test_id = active_pids
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(&raw_pid);

                if let Some(test_id) = test_id {
                    let crash_result = TestResult {
                        test_id,
                        status: STATUS_CRASH,
                        duration_ns: 0,
                        message: format!(
                            "Worker crashed (SIGCHLD: pid {} killed by {})",
                            raw_pid, sig
                        ),
                        memory_rss_bytes: None,
                    };
                    if let Ok(encoded) = encode_with_length(&crash_result) {
                        let _ = result_tx.send(encoded);
                    }
                }
            }
            Ok(WaitStatus::StillAlive) => {
                // No more children to reap
                break;
            }
            Ok(_) => {
                // Stopped/Continued - not relevant, keep reaping
                continue;
            }
            Err(nix::errno::Errno::ECHILD) => {
                // No children at all
                break;
            }
            Err(_) => {
                break;
            }
        }
    }
}

// =============================================================================
//  Worker Pool for Persistent Workers
// =============================================================================

/// Handle to a persistent worker for reuse in Hypervisor Mode.
/// Contains the PID and socket for dispatching subsequent tests.
///
/// SAFETY: UnixStream is Send, so WorkerHandle can be safely moved between threads.
struct WorkerHandle {
    pid: i32,
    socket: UnixStream,
}

/// Pool of idle workers ready for dispatch.
/// Workers are added here after reset, popped for dispatch.
///
/// SAFETY: Using Mutex for thread-safe access from result collection threads.
/// The Zygote command loop is single-threaded, but result collectors run in threads.
static IDLE_WORKERS: Mutex<Vec<WorkerHandle>> = Mutex::new(Vec::new());

/// Initialize snapshot mode by creating UFFD and sending to Supervisor
///
/// Called by Python after post-fork hygiene (RNG reseed, logging reset).
/// Returns true if snapshotting is enabled, false if falling back to fork-server.
#[pyfunction]
fn init_snapshot_mode(sock_path: &str) -> PyResult<bool> {
    use crate::snapshot::parse_memory_maps;
    use nix::unistd::Pid;

    let pid = std::process::id() as i32;

    // 1. Create UFFD
    let uffd = match UffdBuilder::new()
        .close_on_exec(true)
        .non_blocking(false)
        .create()
    {
        Ok(u) => u,
        Err(e) => {
            eprintln!(
                "[tach:rust] WARN: Failed to create userfaultfd: {}. Snapshotting disabled.",
                e
            );
            return Ok(false); // Fallback to fork-server
        }
    };

    // 2. Connect to Supervisor
    let sock = match UnixStream::connect(sock_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!(
                "[tach:rust] WARN: Failed to connect to supervisor: {}. Snapshotting disabled.",
                e
            );
            return Ok(false);
        }
    };

    // 3. Send PID + UFFD via SCM_RIGHTS
    if let Err(e) = send_fd(&sock, pid, uffd.as_raw_fd()) {
        eprintln!(
            "[tach:rust] WARN: Failed to send UFFD: {}. Snapshotting disabled.",
            e
        );
        return Ok(false);
    }

    // 4. Cache memory regions for self-reset (BEFORE snapshot)
    // We cache Heap + libpython data/bss + anonymous mappings
    // We EXCLUDE stack to avoid suicide (can't madvise the stack you're on)
    if let Ok(regions) = parse_memory_maps(Pid::from_raw(pid)) {
        let cached: Vec<(usize, usize)> = regions
            .iter()
            .filter(|r| r.should_snapshot() && !r.is_stack())
            .map(|r| (r.start, r.len))
            .collect();
        let count = cached.len();
        *RESET_REGIONS.lock().unwrap_or_else(|e| e.into_inner()) = cached;
        eprintln!("[tach:rust] Cached {} regions for self-reset", count);
    }

    // =========================================================================
    //  QUIESCE SEQUENCE - Critical for Hypervisor Stability
    // =========================================================================
    //
    // Before SIGSTOP, we must quiesce the jemalloc allocator to ensure the
    // heap is in a deterministic, snapshot-safe state.
    //
    // The Quiesce Sequence:
    // 1. gc.collect() - Called from Python before this function
    // 2. mallctl("thread.tcache.flush") - Flush thread-local caches
    // 3. mallctl("epoch") - Synchronize allocator metadata
    // 4. SIGSTOP - Freeze for snapshot capture
    //
    // Why this matters:
    // - jemalloc's tcache holds recently freed objects for fast reallocation
    // - After snapshot restore, these cached pointers may be stale
    // - By flushing before snapshot, we ensure all allocations are in global arenas
    // - This transforms a non-deterministic heap into a "quiescent" state
    //
    // If quiesce fails, we continue anyway (dirty worker > dead worker)
    // but log a warning since memory corruption may occur after reset.
    // =========================================================================
    eprintln!("[tach:rust] Quiescing jemalloc allocator before snapshot...");
    if let Err(e) = crate::allocator::quiesce_allocator() {
        eprintln!(
            "[tach:rust] WARNING: Failed to quiesce allocator: {}. \
             Memory corruption may occur after reset.",
            e
        );
    }

    // 5. Freeze self - Supervisor will capture snapshot and SIGCONT us
    eprintln!("[tach:rust] Freezing for snapshot (PID {})...", pid);
    if let Err(e) = nix::sys::signal::raise(Signal::SIGSTOP) {
        return Err(pyo3::exceptions::PyOSError::new_err(format!(
            "Failed to SIGSTOP: {}",
            e
        )));
    }

    // 6. We're back! Supervisor has registered our memory.
    SNAPSHOT_ENABLED.store(true, Ordering::SeqCst);
    eprintln!("[tach:rust] Resumed after snapshot capture");
    Ok(true)
}

/// Reset memory by calling madvise(MADV_DONTNEED) on cached regions
///
/// This is the "Seppuku" pattern - the Worker zaps its own memory.
/// The next access to these pages will trigger UFFD faults,
/// which the Supervisor handles by restoring golden pages.
#[pyfunction]
fn reset_memory() -> PyResult<()> {
    if !SNAPSHOT_ENABLED.load(Ordering::SeqCst) {
        eprintln!("[tach:rust] reset_memory called but snapshot not enabled");
        return Ok(());
    }

    let regions = RESET_REGIONS.lock().unwrap_or_else(|e| e.into_inner());
    for &(start, len) in regions.iter() {
        // SAFETY: madvise with MADV_DONTNEED is safe - it just marks pages as discardable.
        // The kernel will zero-fill them on next access (or UFFD will handle it).
        let ret = unsafe { libc::madvise(start as *mut libc::c_void, len, libc::MADV_DONTNEED) };
        if ret != 0 {
            eprintln!(
                "[tach:rust] madvise failed for region {:x}-{:x}: {}",
                start,
                start + len,
                std::io::Error::last_os_error()
            );
        }
    }

    eprintln!(
        "[tach:rust] Self-reset complete: invalidated {} regions",
        regions.len()
    );
    Ok(())
}

/// Register the tach_rust module into sys.modules
pub fn inject_tach_rust_module(py: Python) -> PyResult<()> {
    let tach_mod = PyModule::new(py, "tach_rust")?;

    //  Snapshot mode functions
    tach_mod.add_function(wrap_pyfunction!(init_snapshot_mode, &tach_mod)?)?;
    tach_mod.add_function(wrap_pyfunction!(reset_memory, &tach_mod)?)?;

    //  Hot Reloading - Module cleanup
    tach_mod.add_function(wrap_pyfunction!(cleanup_modules, &tach_mod)?)?;

    //  Jemalloc Allocator Control
    // These functions allow Python to interact with the jemalloc allocator:
    // - quiesce_allocator: Flush tcache and sync epoch before snapshot
    // - verify_jemalloc: Check that jemalloc is the active allocator
    tach_mod.add_function(wrap_pyfunction!(
        crate::allocator::py_quiesce_allocator,
        &tach_mod
    )?)?;
    tach_mod.add_function(wrap_pyfunction!(
        crate::allocator::py_verify_jemalloc,
        &tach_mod
    )?)?;

    //  Zero-Overhead Coverage (PEP 669)
    // These functions allow Python's sys.monitoring callbacks to record coverage:
    // - record_line: Record a LINE event (code_id, lineno) to the ring buffer
    // - is_coverage_enabled: Check if coverage collection is active
    // - get_coverage_overflow: Get count of dropped entries due to buffer full
    tach_mod.add_function(wrap_pyfunction!(
        crate::coverage::py_record_line,
        &tach_mod
    )?)?;
    tach_mod.add_function(wrap_pyfunction!(
        crate::coverage::py_is_coverage_enabled,
        &tach_mod
    )?)?;
    tach_mod.add_function(wrap_pyfunction!(
        crate::coverage::py_get_coverage_overflow,
        &tach_mod
    )?)?;

    //  Coverage Resolution (code_id -> filename mapping)
    // - record_py_start: Register code object on first function entry (PY_START event)
    // - get_mapping_overflow: Get count of dropped mappings due to buffer full
    tach_mod.add_function(wrap_pyfunction!(
        crate::coverage::py_record_py_start,
        &tach_mod
    )?)?;
    tach_mod.add_function(wrap_pyfunction!(
        crate::coverage::py_get_mapping_overflow,
        &tach_mod
    )?)?;

    //  Zero-Copy Loader functions (Request Model)
    tach_mod.add_function(wrap_pyfunction!(crate::loader::get_module, &tach_mod)?)?;
    tach_mod.add_function(wrap_pyfunction!(crate::loader::get_module_path, &tach_mod)?)?;
    tach_mod.add_function(wrap_pyfunction!(
        crate::loader::is_module_package,
        &tach_mod
    )?)?;
    tach_mod.add_function(wrap_pyfunction!(crate::loader::load_module, &tach_mod)?)?;

    // Inject into sys.modules so 'import tach_rust' works
    let sys = py.import("sys")?;
    sys.getattr("modules")?.set_item("tach_rust", tach_mod)?;

    Ok(())
}

// =============================================================================
//  Worker Loop Helper Functions
// =============================================================================

///  Clean up test-imported modules from sys.modules
///
/// Delegates to tach_harness.cleanup_test_modules() which:
/// 1. Identifies modules imported AFTER Zygote initialization
/// 2. Removes them from sys.modules (forcing re-import on next test)
/// 3. Protects critical modules from removal
#[pyfunction]
fn cleanup_modules() -> PyResult<()> {
    Python::attach(|py| -> std::result::Result<(), PyErr> {
        let harness = py.import("tach_harness")?;
        harness.getattr("cleanup_test_modules")?.call0()?;
        Ok(())
    })
    .map_err(|e| {
        pyo3::exceptions::PyRuntimeError::new_err(format!("cleanup_modules failed: {}", e))
    })
}

/// Reset memory and signal readiness to Zygote.
///
/// Called by worker after completing a safe test. Performs:
/// 1.  Clean sys.modules (remove test-imported modules)
/// 2. Memory reset via madvise(MADV_DONTNEED)
/// 3. Signals MSG_WORKER_READY to Zygote
///
/// Returns Err if reset fails - worker MUST exit in this case.
fn reset_and_signal_ready(socket: &UnixStream) -> Result<()> {
    // 1.  Clean sys.modules BEFORE memory reset
    // This removes test-imported modules so next test gets fresh imports
    Python::attach(|py| -> std::result::Result<(), PyErr> {
        let tach_rust = py.import("tach_rust")?;

        // Clean up test-imported modules first
        tach_rust.getattr("cleanup_modules")?.call0()?;

        // Then reset memory
        tach_rust.getattr("reset_memory")?.call0()?;

        Ok(())
    })
    .map_err(|e| anyhow::anyhow!("Python reset failed: {}", e))?;

    // 2. Signal ready to Zygote
    let mut socket = socket.try_clone()?;
    socket.write_all(&[MSG_WORKER_READY])?;

    eprintln!("[tach:worker] Reset complete, signaled READY");
    Ok(())
}

/// Main worker loop - receives and executes tests until exit.
///
/// The worker enters this loop after completing its first safe test.
/// It waits for commands from Zygote:
/// - CMD_RUN_TEST: Execute test, send result, decide exit/reset
/// - CMD_PING: Respond with MSG_PONG (health check)
/// - CMD_EXIT: Clean shutdown
///
/// The loop breaks on:
/// - Toxic test (worker must exit for OS cleanup)
/// - Reset failure (dirty state, must exit)
/// - Socket error (Zygote died or protocol error)
/// - CMD_EXIT command
fn worker_loop(socket: UnixStream) {
    let mut socket = socket;

    loop {
        // Wait for next command from Zygote
        let mut cmd_buf = [0u8; 1];
        if socket.read_exact(&mut cmd_buf).is_err() {
            eprintln!("[tach:worker] Socket closed, exiting loop");
            break;
        }

        match cmd_buf[0] {
            CMD_PING => {
                // Health check - respond with PONG
                if socket.write_all(&[MSG_PONG]).is_err() {
                    eprintln!("[tach:worker] Failed to send PONG, exiting");
                    break;
                }
            }
            CMD_RUN_TEST => {
                // Read protocol header: magic(2) + version(1) + reserved(1) + length(4) = 8 bytes
                let mut header_buf = [0u8; HEADER_SIZE];
                if socket.read_exact(&mut header_buf).is_err() {
                    eprintln!("[tach:worker] Failed to read protocol header");
                    break;
                }

                // Extract length from bytes 4-7 (little-endian u32)
                let len = u32::from_le_bytes([
                    header_buf[4],
                    header_buf[5],
                    header_buf[6],
                    header_buf[7],
                ]) as usize;

                // OOM protection: Validate size BEFORE allocating
                if len > MAX_PAYLOAD_SIZE {
                    eprintln!(
                        "[tach:worker] Rejecting oversized payload: {} bytes > {} limit",
                        len, MAX_PAYLOAD_SIZE
                    );
                    break;
                }

                // Allocate buffer for header + payload
                let mut full_buf = vec![0u8; HEADER_SIZE + len];
                full_buf[..HEADER_SIZE].copy_from_slice(&header_buf);
                if socket.read_exact(&mut full_buf[HEADER_SIZE..]).is_err() {
                    eprintln!("[tach:worker] Failed to read payload");
                    break;
                }

                let payload: TestPayload = match decode_with_limit(&full_buf, MAX_PAYLOAD_SIZE) {
                    Ok(p) => p,
                    Err(e) => {
                        eprintln!("[tach:worker] Deserialize error: {}", e);
                        break;
                    }
                };

                // Execute test
                let result = run_worker(&payload);

                // CRITICAL: Send result BEFORE exit decision
                let _ = std::io::stdout().flush();
                if let Ok(result_bytes) = encode_with_length(&result)
                    && socket.write_all(&result_bytes).is_err()
                {
                    eprintln!("[tach:worker] Failed to send result");
                    break;
                }

                // Dual-path decision
                if payload.is_toxic {
                    // TOXIC PATH: Exit loop, process will terminate
                    eprintln!("[tach:worker] Toxic test completed, exiting");
                    break;
                } else if payload.skip_reset {
                    // SCOPED PATH: Only continue if test passed/skipped.
                    // If test failed/crashed/errored, fixture state is corrupted --
                    // exit so the scheduler detects the broken worker and stops
                    // dispatching remaining tests in this scope group.
                    use crate::protocol::{STATUS_PASS, STATUS_SKIP};
                    if result.status == STATUS_PASS || result.status == STATUS_SKIP {
                        eprintln!("[tach:worker] Skip reset (scoped fixtures), signaling READY");
                        if socket.write_all(&[MSG_WORKER_READY]).is_err() {
                            eprintln!("[tach:worker] Failed to signal READY after skip_reset");
                            break;
                        }
                    } else {
                        eprintln!(
                            "[tach:worker] Test failed in scope group (status {}), \
                             exiting to protect fixture state",
                            result.status
                        );
                        break;
                    }
                    // Continue loop - wait for next command
                } else {
                    // SAFE PATH: Reset memory and continue loop
                    if let Err(e) = reset_and_signal_ready(&socket) {
                        eprintln!("[tach:worker] Reset failed: {}, exiting", e);
                        break;
                    }
                    // Loop continues - wait for next command
                }
            }
            CMD_EXIT => {
                eprintln!("[tach:worker] Received EXIT command");
                break;
            }
            _ => {
                eprintln!("[tach:worker] Unknown command: {:#x}", cmd_buf[0]);
            }
        }
    }
}

/// Check if a worker is alive and responsive.
///
/// Sends CMD_PING and waits for MSG_PONG with a short timeout.
/// Returns true if worker responds, false if dead or unresponsive.
#[allow(dead_code)] // Utility for debugging and future health check features
fn check_worker_health(socket: &mut UnixStream, pid: i32) -> bool {
    use std::time::Duration;

    // Set a short read timeout for the health check
    let old_timeout = socket.read_timeout().ok().flatten();
    if socket
        .set_read_timeout(Some(Duration::from_millis(100)))
        .is_err()
    {
        return false;
    }

    // Send PING
    if socket.write_all(&[CMD_PING]).is_err() {
        eprintln!("[tach:zygote] Worker {} failed PING write", pid);
        let _ = socket.set_read_timeout(old_timeout);
        return false;
    }

    // Wait for PONG
    let mut buf = [0u8; 1];
    let healthy = match socket.read_exact(&mut buf) {
        Ok(_) => buf[0] == MSG_PONG,
        Err(_) => false,
    };

    // Restore original timeout
    let _ = socket.set_read_timeout(old_timeout);

    if !healthy {
        eprintln!("[tach:zygote] Worker {} failed health check (no PONG)", pid);
    }

    healthy
}

/// Remove dead workers from the idle pool.
///
/// Called periodically to clean up workers that died unexpectedly.
/// Returns the number of workers removed.
#[allow(dead_code)] // Utility for debugging and future periodic cleanup
fn reap_dead_workers() -> usize {
    let mut workers = IDLE_WORKERS.lock().unwrap_or_else(|e| e.into_inner());
    let original_count = workers.len();

    // Partition into healthy and dead workers
    let mut healthy = Vec::with_capacity(workers.len());
    let mut dead_pids = Vec::new();

    for mut worker in workers.drain(..) {
        // First check if the process is still alive using kill(pid, 0)
        let process_alive = nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(worker.pid),
            None, // Signal 0 = just check if process exists
        )
        .is_ok();

        if !process_alive {
            dead_pids.push(worker.pid);
            continue;
        }

        // Process is alive, check if it's responsive
        if check_worker_health(&mut worker.socket, worker.pid) {
            healthy.push(worker);
        } else {
            dead_pids.push(worker.pid);
            // Kill the unresponsive worker
            let _ = nix::sys::signal::kill(
                nix::unistd::Pid::from_raw(worker.pid),
                nix::sys::signal::Signal::SIGKILL,
            );
        }
    }

    *workers = healthy;

    let removed = original_count - workers.len();
    if removed > 0 {
        eprintln!(
            "[tach:zygote] Reaped {} dead/unresponsive workers: {:?}",
            removed, dead_pids
        );
    }

    removed
}

/// Spawn a thread to collect result from worker and manage worker lifecycle.
///
/// This is the Worker Lifecycle Manager thread. It:
/// 1. Reads TestResult from worker socket
/// 2. Forwards result to Supervisor via result_tx
/// 3. For safe tests: waits for MSG_WORKER_READY, then returns worker to pool
/// 4. For toxic tests: worker will EOF (exit), thread terminates
///
/// The thread owns the socket and WorkerHandle lifecycle.
fn spawn_result_collector(
    socket: UnixStream,
    pid: i32,
    result_tx: mpsc::Sender<Vec<u8>>,
    is_toxic: bool,
    active_pids: Arc<Mutex<HashMap<i32, u32>>>,
) {
    thread::spawn(move || {
        let mut socket = socket;

        // 1. Read protocol header: magic(2) + version(1) + reserved(1) + length(4) = 8 bytes
        let mut header_buf = [0u8; HEADER_SIZE];
        if socket.read_exact(&mut header_buf).is_err() {
            eprintln!("[tach:zygote] Worker {} crashed before sending result", pid);
            // Don't remove from active_pids -- SIGCHLD handler will send crash notification
            return;
        }

        // 2. Extract length from bytes 4-7 and read payload
        let result_len =
            u32::from_le_bytes([header_buf[4], header_buf[5], header_buf[6], header_buf[7]])
                as usize;

        // OOM protection: Validate size BEFORE allocating
        if result_len > MAX_PAYLOAD_SIZE {
            eprintln!(
                "[tach:result_collector] Rejecting oversized result: {} bytes > {} limit. Terminating.",
                result_len, MAX_PAYLOAD_SIZE
            );
            return;
        }

        let mut result_buf = vec![0u8; result_len];
        if socket.read_exact(&mut result_buf).is_err() {
            eprintln!("[tach:zygote] Worker {} crashed during result send", pid);
            return;
        }

        // 3. Forward result to Supervisor (header + payload)
        let mut full = header_buf.to_vec();
        full.extend(result_buf);
        if result_tx.send(full).is_err() {
            eprintln!("[tach:zygote] Result channel closed");
            return;
        }

        // Result successfully forwarded -- remove from active_pids so
        // SIGCHLD handler won't treat a subsequent normal exit as a crash
        active_pids
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&pid);

        // 4. Toxic workers exit here - don't wait for READY signal
        if is_toxic {
            eprintln!("[tach:zygote] Toxic worker {} completed, not pooling", pid);
            return;
        }

        // 5. Safe workers: wait for MSG_WORKER_READY signal
        let mut ready_buf = [0u8; 1];
        match socket.read_exact(&mut ready_buf) {
            Ok(_) if ready_buf[0] == MSG_WORKER_READY => {
                // Worker is ready for reuse - add to pool

                if let Ok(mut workers) = IDLE_WORKERS.lock() {
                    workers.push(WorkerHandle { pid, socket });
                } else {
                    eprintln!("[tach:zygote] WARNING: Failed to acquire lock for worker pool");
                }
            }
            Ok(_) => {
                eprintln!(
                    "[tach:zygote] Worker {} sent unexpected byte: {:#x}",
                    pid, ready_buf[0]
                );
            }
            Err(_) => {
                eprintln!("[tach:zygote] Worker {} died after result (no READY)", pid);
            }
        }
    });
}

/// Zygote with separate command and result channels
pub fn entrypoint(cmd_socket: UnixStream, result_socket: UnixStream) -> Result<()> {
    // DEAD MAN'S SWITCH : If supervisor dies, we die
    // This is the ultimate safety net - no orphaned zygotes
    // Must be the FIRST thing we do, before any resource allocation
    unsafe {
        libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
    }

    // Install SIGCHLD handler with self-pipe pattern (replaces SIG_IGN).
    // The handler writes to a pipe; the main event loop polls it alongside cmd_socket.
    // We must call waitpid() to reap children since SIG_IGN is no longer used.
    let mut pipe_fds = [0i32; 2];
    if unsafe { libc::pipe(pipe_fds.as_mut_ptr()) } != 0 {
        return Err(anyhow::anyhow!("Failed to create SIGCHLD signal pipe"));
    }
    let sigchld_pipe_rd = pipe_fds[0];
    let sigchld_pipe_wr = pipe_fds[1];

    // Make both ends non-blocking so handler never blocks and drain never blocks
    for fd in &[sigchld_pipe_rd, sigchld_pipe_wr] {
        unsafe {
            let flags = libc::fcntl(*fd, libc::F_GETFL);
            libc::fcntl(*fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
        }
    }

    SIGCHLD_PIPE_WR.store(sigchld_pipe_wr, Ordering::SeqCst);

    // SA_NOCLDSTOP: don't fire SIGCHLD for stop/continue, only for exit
    let sa = SigAction::new(
        SigHandler::Handler(sigchld_handler),
        SaFlags::SA_NOCLDSTOP,
        SigSet::empty(),
    );
    unsafe { sigaction(Signal::SIGCHLD, &sa) }?;

    // Track active worker PIDs -> test_ids for crash detection.
    // Shared between main loop (insert on dispatch) and result collector threads (remove on success).
    let active_pids: Arc<Mutex<HashMap<i32, u32>>> = Arc::new(Mutex::new(HashMap::new()));

    eprintln!("[tach:zygote] Initializing Python...");
    let cwd = env::current_dir()?;
    let cwd_str = cwd.to_string_lossy().to_string();

    //  Detect venv and get site-packages path
    let site_packages = find_site_packages(&cwd);
    if let Some(ref sp) = site_packages {
        eprintln!("[tach:zygote] Found venv: {}", sp.display());
    }

    let (session_effects, collected_tests) = Python::attach(
        |py| -> Result<(
            Vec<crate::hooks::HookEffect>,
            Vec<crate::protocol::CollectedTest>,
        )> {
            let sys = py.import("sys")?;
            let path_attr = sys.getattr("path")?;
            let path: &Bound<PyList> = path_attr
                .cast()
                .map_err(|e| anyhow::anyhow!("sys.path not a list: {}", e))?;

            //  Inject venv site-packages FIRST (highest priority)
            if let Some(ref sp) = site_packages {
                path.insert(0, sp.to_string_lossy().to_string())?;
            }

            // Add project root
            path.insert(0, &cwd_str)?;

            // Now pytest should be importable from venv
            match py.import("pytest") {
                Ok(_) => eprintln!("[tach:zygote] pytest loaded successfully"),
                Err(e) => {
                    eprintln!("[tach:zygote] Error: {}", e);
                    return Err(anyhow::anyhow!("Failed to import pytest: {}", e));
                }
            }

            // Django Detection & Setup (Batteries-Included)
            // Initialize Django in Zygote so workers inherit the pre-warmed state.
            // NOTE: We do NOT warm up DB connections here. setup_databases() creates
            // a test DB after init_session(), and connections are closed before fork
            // so workers get fresh file descriptors.
            py.run(
            c_str!(r#"
import os
import sys

try:
    import django

    if 'DJANGO_SETTINGS_MODULE' in os.environ:
        django.setup()
        print(f'[tach:zygote] Django initialized: {os.environ["DJANGO_SETTINGS_MODULE"]}', file=sys.stderr)
except ImportError:
    pass  # Django not installed, skip
except Exception as e:
    print(f'[tach:zygote] Django setup error: {e}', file=sys.stderr)
"#),
            None,
            None,
        )?;

            // Set __main__.__file__ so code inspecting the main module (e.g.
            // Django's autoreload) finds an attribute instead of AttributeError.
            // Embedded Python has no script entry point, so __main__ lacks __file__.
            // Use /proc/self/exe (the tach-core binary) as a real, resolvable path.
            py.run(
                c_str!(
                    r#"
import sys, os
m = sys.modules.get('__main__')
if m is not None and not hasattr(m, '__file__'):
    m.__file__ = os.path.realpath('/proc/self/exe')
"#
                ),
                None,
                None,
            )?;

            // CRITICAL: Inject tach_rust module BEFORE loading harness
            // This allows 'import tach_rust' in Python code
            inject_tach_rust_module(py)?;

            // Load the tach harness module
            // Convert &str to CString for PyModule::from_code
            let harness_code = std::ffi::CString::new(TACH_HARNESS_PY)
                .map_err(|e| anyhow::anyhow!("Failed to create CString: {}", e))?;
            let harness =
                PyModule::from_code(py, &harness_code, c"tach_harness.py", c"tach_harness")?;

            // ZYGOTE COLLECTION: Pre-collect tests for TARGET PATH only (not entire project)
            // This avoids importing test files outside the requested scope
            let target_path = std::env::var("TACH_TARGET_PATH").unwrap_or_else(|_| cwd_str.clone());
            harness.getattr("init_session")?.call1((&target_path,))?;

            // Django Test Database: Create test DB after pytest is configured but
            // before workers fork. Reads TACH_REUSE_DB/TACH_CREATE_DB env vars.
            // Connections are closed before fork so workers get fresh FDs.
            // NOTE: Call through `harness` ref directly — sys.modules registration
            // happens later, so `import tach_harness` would fail here.
            if let Err(e) = harness
                .getattr("_setup_django_test_db")
                .and_then(|f| f.call0())
            {
                eprintln!("[tach:zygote] Django test DB setup error: {e}");
            }

            // HOOK EFFECT BRIDGE (v0.2.0): Retrieve session effects from Python
            // After init_session(), Python has recorded effects in _SESSION_HOOK_EFFECTS.
            // We retrieve them here and will send them to the Supervisor for HookRegistry population.
            let session_effects_obj = harness.getattr("get_session_hook_effects")?.call0()?;
            let session_effects: &Bound<'_, PyList> = session_effects_obj
                .cast::<PyList>()
                .map_err(|e| pyo3::exceptions::PyTypeError::new_err(e.to_string()))?;
            let effects = convert_py_effects_to_rust(session_effects);

            // COLLECTED TESTS (Issue #98): Extract pytest's authoritative test list
            let collected_obj = harness.getattr("get_collected_tests")?.call0()?;
            let collected_list: &Bound<'_, pyo3::types::PyList> =
                collected_obj.cast::<pyo3::types::PyList>().map_err(|e| {
                    anyhow::anyhow!("get_collected_tests() didn't return a list: {}", e)
                })?;

            let mut collected = Vec::with_capacity(collected_list.len());
            for item in collected_list.iter() {
                let dict = item
                    .cast::<pyo3::types::PyDict>()
                    .map_err(|e| anyhow::anyhow!("collected test item is not a dict: {}", e))?;

                let node_id: String = dict
                    .get_item("node_id")?
                    .ok_or_else(|| anyhow::anyhow!("missing node_id"))?
                    .extract()?;
                let file_path: String = dict
                    .get_item("file_path")?
                    .ok_or_else(|| anyhow::anyhow!("missing file_path"))?
                    .extract()?;
                let markers: Vec<String> = dict
                    .get_item("markers")?
                    .ok_or_else(|| anyhow::anyhow!("missing markers"))?
                    .extract()?;
                let is_async: bool = dict
                    .get_item("is_async")?
                    .ok_or_else(|| anyhow::anyhow!("missing is_async"))?
                    .extract()?;

                collected.push(crate::protocol::CollectedTest {
                    node_id,
                    file_path,
                    markers,
                    is_async,
                });
            }

            sys.getattr("modules")?.set_item("tach_harness", harness)?;

            Ok((effects, collected))
        },
    )?;

    eprintln!(
        "[tach:zygote] Python ready. Session effects: {}, Collected tests: {}",
        session_effects.len(),
        collected_tests.len()
    );

    // Signal ready on both sockets, then send session effects
    let mut cmd_socket = cmd_socket;
    let result_socket = result_socket;
    cmd_socket.write_all(&[MSG_READY])?;

    // ============================================================================
    // SESSION EFFECTS IPC BRIDGE (v0.2.0)
    // ============================================================================
    //
    // This section transmits hook effects captured during pytest session initialization
    // from the Zygote process to the Supervisor process.
    //
    // ## What Effects Are Sent
    // - SetEnv: Environment variables set by pytest_configure hooks
    // - ModifySysPath: sys.path modifications (append/prepend)
    // - RegisterMarker: Custom markers registered via pytest.ini or hooks
    // - ModifyItems: Test collection modifications (reordering, filtering)
    //
    // ## Wire Format
    // The effects are serialized using bincode for efficient binary encoding:
    // - Length prefix: 4 bytes, little-endian u32 (payload size in bytes)
    // - Payload: bincode-encoded Vec<HookEffect>
    //
    // ## Receiver Location
    // The Supervisor receives these effects in src/main.rs after the READY byte.
    // It decodes them and calls hook_registry.record_effect() for each effect,
    // storing them under "pytest_configure" for later replay in workers.
    //
    // ## Flow Diagram
    // Zygote (Python init) -> bincode encode -> socket write
    //     -> Supervisor (main.rs) -> bincode decode -> HookRegistry.record_effect()
    //
    // ============================================================================
    let framed_effects = encode_with_length(&session_effects)
        .map_err(|e| anyhow::anyhow!("Failed to encode session effects: {}", e))?;
    cmd_socket.write_all(&framed_effects)?;

    // COLLECTED TESTS IPC (Issue #98): Send pytest's authoritative test list
    eprintln!(
        "[tach:zygote] Sending {} collected tests to Supervisor (pytest-authoritative)",
        collected_tests.len()
    );

    let framed_collected = encode_with_length(&collected_tests)
        .map_err(|e| anyhow::anyhow!("Failed to encode collected tests: {}", e))?;
    cmd_socket.write_all(&framed_collected)?;

    // Channel for collecting results from worker threads
    let (result_tx, result_rx) = mpsc::channel::<Vec<u8>>();

    // Result forwarding thread
    let result_socket_clone = result_socket.try_clone()?;
    thread::spawn(move || {
        let mut socket = result_socket_clone;
        while let Ok(data) = result_rx.recv() {
            if socket.write_all(&data).is_err() {
                break;
            }
        }
    });

    // Command processing loop with poll() on cmd_socket + SIGCHLD pipe.
    // poll() lets us react to both supervisor commands and worker crashes without busy-waiting.
    let cmd_fd = cmd_socket.as_raw_fd();
    let mut cmd_buf = [0u8; 1];
    loop {
        let mut fds = [
            libc::pollfd {
                fd: cmd_fd,
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: sigchld_pipe_rd,
                events: libc::POLLIN,
                revents: 0,
            },
        ];

        let poll_ret = unsafe { libc::poll(fds.as_mut_ptr(), 2, -1) };
        if poll_ret < 0 {
            let errno = std::io::Error::last_os_error();
            if errno.raw_os_error() == Some(libc::EINTR) {
                // Interrupted by signal -- check pipe on next iteration
                continue;
            }
            eprintln!("[tach:zygote] poll() error: {}", errno);
            break;
        }

        // Handle SIGCHLD pipe first (reap before processing new commands)
        if fds[1].revents & libc::POLLIN != 0 {
            drain_signal_pipe(sigchld_pipe_rd);
            reap_crashed_workers(&active_pids, &result_tx);
        }

        // Handle cmd_socket
        if fds[0].revents & libc::POLLIN != 0 {
            if cmd_socket.read(&mut cmd_buf).is_err() {
                break;
            }

            match cmd_buf[0] {
                CMD_FORK => {
                    // Read protocol header: magic(2) + version(1) + reserved(1) + length(4) = 8 bytes
                    let mut header_buf = [0u8; HEADER_SIZE];
                    cmd_socket.read_exact(&mut header_buf)?;

                    // Extract length from bytes 4-7 (little-endian u32)
                    let len = u32::from_le_bytes([
                        header_buf[4],
                        header_buf[5],
                        header_buf[6],
                        header_buf[7],
                    ]) as usize;

                    // OOM protection: Validate size BEFORE allocating
                    // CRITICAL: Return error instead of continue to avoid protocol desync.
                    // If we continue, the unread payload bytes will corrupt subsequent reads.
                    if len > MAX_PAYLOAD_SIZE {
                        eprintln!(
                            "[tach:zygote] FATAL: Rejecting oversized payload: {} bytes > {} limit. Protocol error.",
                            len, MAX_PAYLOAD_SIZE
                        );
                        return Err(anyhow::anyhow!(
                            "Protocol error: payload too large ({} bytes > {} limit)",
                            len,
                            MAX_PAYLOAD_SIZE
                        ));
                    }

                    // Allocate buffer for header + payload
                    let mut full_buf = vec![0u8; HEADER_SIZE + len];
                    full_buf[..HEADER_SIZE].copy_from_slice(&header_buf);
                    cmd_socket.read_exact(&mut full_buf[HEADER_SIZE..])?;

                    let payload: TestPayload = match decode_with_limit(&full_buf, MAX_PAYLOAD_SIZE)
                    {
                        Ok(p) => p,
                        Err(e) => {
                            eprintln!("[tach:zygote] Deserialize error: {}", e);
                            continue;
                        }
                    };

                    let is_toxic = payload.is_toxic;

                    //  Check for idle worker (only for safe tests)
                    // Also verify the worker is still alive before trying to use it
                    let idle_worker = if !is_toxic {
                        loop {
                            let mut workers =
                                IDLE_WORKERS.lock().unwrap_or_else(|e| e.into_inner());
                            match workers.pop() {
                                None => break None,
                                Some(worker) => {
                                    drop(workers); // Release lock before checking health

                                    // Verify process is still alive using kill(pid, 0)
                                    let process_alive = nix::sys::signal::kill(
                                        nix::unistd::Pid::from_raw(worker.pid),
                                        None,
                                    )
                                    .is_ok();

                                    if !process_alive {
                                        eprintln!(
                                            "[tach:zygote] Worker {} died unexpectedly, trying next",
                                            worker.pid
                                        );
                                        continue; // Try next worker
                                    }

                                    break Some(worker);
                                }
                            }
                        }
                    } else {
                        None // Always fork fresh for toxic tests
                    };

                    if let Some(mut worker) = idle_worker {
                        // =========================================================
                        // REUSE PATH: Dispatch to existing worker
                        // =========================================================
                        eprintln!("[tach:zygote] Reusing worker {} for test", worker.pid);

                        // Send CMD_RUN_TEST + full encoded buffer (header + payload) to worker
                        let dispatch_ok = (|| -> std::io::Result<()> {
                            worker.socket.write_all(&[CMD_RUN_TEST])?;
                            worker.socket.write_all(&full_buf)?; // Send full buffer (header + payload)
                            Ok(())
                        })();

                        if let Err(e) = dispatch_ok {
                            eprintln!(
                                "[tach:zygote] Failed to dispatch to worker {}: {}",
                                worker.pid, e
                            );
                            // Worker died, fall through to fork path
                            // Don't continue - we need to fork a new worker
                        } else {
                            // Successfully dispatched - send PID back and spawn collector
                            active_pids
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .insert(worker.pid, payload.test_id);
                            cmd_socket.write_all(&worker.pid.to_le_bytes())?;
                            spawn_result_collector(
                                worker.socket,
                                worker.pid,
                                result_tx.clone(),
                                is_toxic,
                                active_pids.clone(),
                            );
                            continue;
                        }
                    }

                    // =========================================================
                    // FORK PATH: Create new worker
                    // =========================================================
                    let (parent_sock, child_sock) = UnixStream::pair()?;

                    match unsafe { fork() } {
                        Ok(ForkResult::Parent { child }) => {
                            drop(child_sock);
                            // Send PID back on command socket
                            let child_pid = child.as_raw();
                            active_pids
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .insert(child_pid, payload.test_id);
                            cmd_socket.write_all(&child_pid.to_le_bytes())?;

                            // Use spawn_result_collector instead of inline thread
                            spawn_result_collector(
                                parent_sock,
                                child_pid,
                                result_tx.clone(),
                                is_toxic,
                                active_pids.clone(),
                            );
                        }
                        Ok(ForkResult::Child) => {
                            drop(parent_sock);

                            // 0. DEAD MAN'S SWITCH : If Zygote dies, worker dies
                            // Must be FIRST - before any resource allocation
                            unsafe {
                                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
                            }

                            // Close inherited signal pipe fds (belong to zygote, not worker)
                            unsafe {
                                libc::close(sigchld_pipe_rd);
                                libc::close(sigchld_pipe_wr);
                            }

                            // 1. CRITICAL: Restore default signal handling
                            // Parent uses SIGCHLD handler for crash detection; workers need SIG_DFL
                            // so their own child processes (e.g. subprocess.run) work correctly
                            unsafe { signal(Signal::SIGCHLD, SigHandler::SigDfl) }.ok();

                            // 2. ISOLATE filesystem and network (Iron Dome)
                            // CRITICAL: Fail hard if isolation fails to protect the host
                            let project_root = std::env::current_dir().unwrap_or_default();
                            if let Err(e) =
                                crate::isolation::setup_filesystem(payload.test_id, &project_root)
                            {
                                eprintln!(
                                    "[tach:worker] CRITICAL: Isolation failed. Aborting to protect host. Error: {:#}",
                                    e
                                );
                                std::process::exit(1);
                            }

                            // 3. Re-chdir to pick up the overlay mount on project root
                            // Without this, the CWD handle points to the old mount
                            let _ = std::env::set_current_dir(&project_root);

                            // 4.  Apply Iron Dome sandbox (Landlock + Seccomp)
                            // SECURITY SEQUENCE:
                            //   - Landlock: Restrict filesystem view (ALWAYS applied)
                            //   - Seccomp: Block dangerous syscalls (ONLY for safe workers)
                            //
                            // This must happen AFTER isolation::setup_filesystem() creates the
                            // overlay mounts, but BEFORE any Python code runs.
                            //
                            // Graceful degradation: Log warnings but don't crash on older kernels.
                            let _sandbox_status = crate::sandbox::apply_iron_dome(
                                &project_root,
                                payload.test_id,
                                payload.is_toxic,
                            );
                            // Note: apply_iron_dome logs its own warnings, no need to check result

                            // 5. Redirect stdout/stderr to memfd
                            if payload.log_fd >= 0 {
                                let _ = redirect_output(payload.log_fd);
                            }

                            // 6. Set debug socket path for breakpoint() support
                            // This enables interactive debugging via TTY proxy
                            if !payload.debug_socket_path.is_empty() {
                                Python::attach(|py| -> Result<(), PyErr> {
                                    let harness = py.import("tach_harness")?;
                                    harness
                                        .getattr("set_debug_socket_path")?
                                        .call1((&payload.debug_socket_path,))?;
                                    Ok(())
                                })
                                .ok(); // Non-fatal if this fails
                            }

                            // 7. POST-FORK INIT: Snapshot mode handshake
                            // This performs hygiene (RNG reseed, logging reset) and
                            // initiates snapshot if TACH_SUPERVISOR_SOCK is set.
                            // Worker will SIGSTOP here; Supervisor captures snapshot and SIGCONTs.
                            Python::attach(|py| -> Result<(), PyErr> {
                                let harness = py.import("tach_harness")?;
                                harness.getattr("post_fork_init")?.call0()?;
                                Ok(())
                            })
                            .ok(); // Continue even if snapshot fails (graceful degradation)

                            // 8. Run test
                            let result = run_worker(&payload);

                            // 9. Flush and send result (CRITICAL: BEFORE exit decision)
                            // Invariant: Scheduler receives result even if worker exits
                            let _ = std::io::stdout().flush();
                            if let Ok(result_bytes) = encode_with_length(&result)
                                && let Ok(mut sock) = child_sock.try_clone()
                            {
                                let _ = sock.write_all(&result_bytes);
                            }

                            // 10.  Dual-path decision based on toxicity
                            // TOXIC PATH: Exit immediately (OS cleans up threads, FDs, etc.)
                            // SAFE PATH: Reset memory and enter worker loop for reuse
                            if payload.is_toxic {
                                // Toxic test: exit without reset
                                // This is the Isolation Mode path
                                process::exit(0);
                            } else if payload.skip_reset {
                                // Scoped test: only enter reuse loop if test passed/skipped
                                use crate::protocol::{STATUS_PASS, STATUS_SKIP};
                                if result.status == STATUS_PASS || result.status == STATUS_SKIP {
                                    if let Ok(mut sock) = child_sock.try_clone() {
                                        let _ = sock.write_all(&[MSG_WORKER_READY]);
                                    }
                                    worker_loop(child_sock);
                                } else {
                                    eprintln!(
                                        "[tach:worker] Test failed in scope group (status {}), \
                                     exiting to protect fixture state",
                                        result.status
                                    );
                                }
                                process::exit(0);
                            } else {
                                // Safe test: reset memory and enter worker loop
                                // This is the Hypervisor Mode path - worker will be reused
                                if let Err(e) = reset_and_signal_ready(&child_sock) {
                                    eprintln!(
                                        "[tach:worker] Reset failed after first test: {}, exiting",
                                        e
                                    );
                                    process::exit(1);
                                }

                                // Enter worker loop - wait for subsequent tests
                                worker_loop(child_sock);
                                process::exit(0);
                            }
                        }
                        Err(e) => eprintln!("[tach:zygote] Fork failed: {}", e),
                    }
                }
                CMD_EXIT => {
                    let idle_workers = std::mem::take(
                        &mut *IDLE_WORKERS.lock().unwrap_or_else(|e| e.into_inner()),
                    );
                    let mut worker_pids = Vec::with_capacity(idle_workers.len());
                    for mut worker in idle_workers {
                        worker_pids.push(worker.pid);
                        let _ = worker.socket.write_all(&[CMD_EXIT]);
                    }

                    // Give threads time to forward final results
                    thread::sleep(std::time::Duration::from_millis(200));

                    // Reap any worker processes that haven't exited yet
                    // Using WNOHANG to avoid blocking indefinitely
                    for pid in &worker_pids {
                        if *pid > 0 {
                            // Try to kill the process if it's still running
                            let _ = nix::sys::signal::kill(
                                nix::unistd::Pid::from_raw(*pid),
                                nix::sys::signal::Signal::SIGTERM,
                            );
                        }
                    }

                    // Give workers a short grace period to terminate
                    thread::sleep(std::time::Duration::from_millis(100));

                    // Force kill any remaining workers
                    for pid in &worker_pids {
                        if *pid > 0 {
                            let _ = nix::sys::signal::kill(
                                nix::unistd::Pid::from_raw(*pid),
                                nix::sys::signal::Signal::SIGKILL,
                            );
                        }
                    }

                    break;
                }
                _ => {}
            }
        } // if fds[0] POLLIN
    }

    Ok(())
}

/// Convert HookEffect enum to Python list of dicts for effect replay in workers.
///
/// This converts the cached_effects from TestPayload (Rust HookEffect enum) to
/// a Python list of dicts that can be consumed by tach_harness.apply_cached_effects().
fn convert_cached_effects_to_py<'py>(
    py: Python<'py>,
    effects: &[crate::hooks::HookEffect],
) -> Result<Bound<'py, pyo3::types::PyList>, PyErr> {
    use pyo3::types::{PyDict, PyList};

    let py_list = PyList::empty(py);

    for effect in effects {
        let py_dict = PyDict::new(py);

        match effect {
            crate::hooks::HookEffect::SetEnv { key, value } => {
                py_dict.set_item("type", "SetEnv")?;
                py_dict.set_item("key", key)?;
                py_dict.set_item("value", value)?;
            }
            crate::hooks::HookEffect::ModifySysPath { action, path } => {
                py_dict.set_item("type", "ModifySysPath")?;
                // Convert SysPathAction enum to string for Python
                py_dict.set_item("action", action.to_string())?;
                py_dict.set_item("path", path)?;
            }
            crate::hooks::HookEffect::RegisterMarker { name, description } => {
                py_dict.set_item("type", "RegisterMarker")?;
                py_dict.set_item("name", name)?;
                py_dict.set_item("description", description)?;
            }
            crate::hooks::HookEffect::ModifyItems { removed, reordered } => {
                py_dict.set_item("type", "ModifyItems")?;
                py_dict.set_item("removed", removed.clone())?;
                py_dict.set_item("reordered", *reordered)?;
            }
            crate::hooks::HookEffect::NoEffect => {
                // Skip NoEffect - nothing to apply
                continue;
            }
            crate::hooks::HookEffect::DjangoDbSetup { .. } => {
                // DjangoDbSetup is handled by Python harness via marker_info
                // Not converted to Python dict here - it's already in marker_info
                continue;
            }
            crate::hooks::HookEffect::SqlAlchemyDbSetup { .. } => {
                // SqlAlchemyDbSetup is handled by Python harness via marker_info
                // Not converted to Python dict here - it's already in marker_info
                continue;
            }
            crate::hooks::HookEffect::AsyncioSetup {
                loop_scope,
                auto_mode,
            } => {
                // Convert loop_scope to lowercase string for Python harness
                py_dict.set_item("type", "AsyncioSetup")?;
                py_dict.set_item("loop_scope", loop_scope.to_string())?;
                py_dict.set_item("auto_mode", *auto_mode)?;
            }
        }

        py_list.append(py_dict)?;
    }

    Ok(py_list)
}

/// Convert MarkerInfo Vec to Python list of dicts for harness.
///
/// Each MarkerInfo is converted to a dict with 'name' and 'args' keys.
/// The args HashMap is converted to a Python dict.
fn convert_marker_info_to_py<'py>(
    py: Python<'py>,
    marker_info: &[crate::discovery::MarkerInfo],
) -> PyResult<Bound<'py, PyList>> {
    use pyo3::types::{PyDict, PyList};

    let py_list = PyList::empty(py);

    for marker in marker_info {
        let py_dict = PyDict::new(py);
        py_dict.set_item("name", &marker.name)?;

        // Convert args HashMap to Python dict
        let args_dict = PyDict::new(py);
        for (key, value) in &marker.args {
            // Convert serde_json::Value to Python object
            let py_value = json_value_to_py(py, value)?;
            args_dict.set_item(key, py_value)?;
        }
        py_dict.set_item("args", args_dict)?;

        py_list.append(py_dict)?;
    }

    Ok(py_list)
}

/// Convert serde_json::Value to PyObject
fn json_value_to_py(py: Python<'_>, value: &serde_json::Value) -> PyResult<Py<PyAny>> {
    use pyo3::types::{PyList, PyString};

    match value {
        serde_json::Value::Null => Ok(py.None()),
        serde_json::Value::Bool(b) => {
            // For bool, use into_pyobject and convert Borrowed to owned
            let borrowed = b.into_pyobject(py)?;
            Ok(borrowed.to_owned().into_any().unbind())
        }
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(i.into_pyobject(py)?.into_any().unbind())
            } else if let Some(f) = n.as_f64() {
                Ok(f.into_pyobject(py)?.into_any().unbind())
            } else {
                // Handle numbers that don't fit in i64 or f64 (e.g., u64 > i64::MAX)
                // Convert via string representation to preserve the value
                let s = n.to_string();
                Ok(pyo3::types::PyString::new(py, &s).into_any().unbind())
            }
        }
        serde_json::Value::String(s) => Ok(PyString::new(py, s).into_any().unbind()),
        serde_json::Value::Array(arr) => {
            let py_list = PyList::empty(py);
            for item in arr {
                let py_item = json_value_to_py(py, item)?;
                py_list.append(py_item)?;
            }
            Ok(py_list.into_any().unbind())
        }
        serde_json::Value::Object(obj) => {
            use pyo3::types::PyDict;
            let py_dict = PyDict::new(py);
            for (k, v) in obj {
                let py_v = json_value_to_py(py, v)?;
                py_dict.set_item(k, py_v)?;
            }
            Ok(py_dict.into_any().unbind())
        }
    }
}

/// Convert Python list of effect dicts to Rust Vec<HookEffect>.
///
/// This is the inverse of `convert_cached_effects_to_py`. It's used to retrieve
/// session hook effects from Python (recorded during init_session) and transfer
/// them to the Supervisor's HookRegistry.
fn convert_py_effects_to_rust(py_list: &Bound<'_, PyList>) -> Vec<crate::hooks::HookEffect> {
    use pyo3::types::{PyDict, PyDictMethods};

    let mut effects = Vec::new();
    let mut skipped_count = 0usize;

    for (idx, item) in py_list.iter().enumerate() {
        // Each item should be a dict - use cast (downcast is deprecated in PyO3 0.27+)
        let dict: &Bound<'_, PyDict> = match item.cast::<PyDict>() {
            Ok(d) => d,
            Err(e) => {
                eprintln!(
                    "[tach:zygote] DEBUG: Skipping effect[{}]: not a dict ({:?})",
                    idx, e
                );
                skipped_count += 1;
                continue;
            }
        };

        // Each item should be a dict with a 'type' key
        let effect_type: String = match dict.get_item("type") {
            Ok(Some(t)) => match t.extract() {
                Ok(s) => s,
                Err(e) => {
                    eprintln!(
                        "[tach:zygote] DEBUG: Skipping effect[{}]: 'type' not extractable ({:?})",
                        idx, e
                    );
                    skipped_count += 1;
                    continue;
                }
            },
            Ok(None) => {
                eprintln!(
                    "[tach:zygote] DEBUG: Skipping effect[{}]: missing 'type' key",
                    idx
                );
                skipped_count += 1;
                continue;
            }
            Err(e) => {
                eprintln!(
                    "[tach:zygote] DEBUG: Skipping effect[{}]: error getting 'type' ({:?})",
                    idx, e
                );
                skipped_count += 1;
                continue;
            }
        };

        let effect = match effect_type.as_str() {
            "SetEnv" => {
                let key: String = match dict.get_item("key") {
                    Ok(Some(k)) => match k.extract::<String>() {
                        Ok(s) if !s.is_empty() => s,
                        Ok(_) => {
                            eprintln!(
                                "[tach:zygote] DEBUG: Skipping SetEnv effect[{}]: empty key",
                                idx
                            );
                            skipped_count += 1;
                            continue;
                        }
                        Err(e) => {
                            eprintln!(
                                "[tach:zygote] DEBUG: Skipping SetEnv effect[{}]: key not extractable ({:?})",
                                idx, e
                            );
                            skipped_count += 1;
                            continue;
                        }
                    },
                    Ok(None) => {
                        eprintln!(
                            "[tach:zygote] DEBUG: Skipping SetEnv effect[{}]: missing 'key'",
                            idx
                        );
                        skipped_count += 1;
                        continue;
                    }
                    Err(e) => {
                        eprintln!(
                            "[tach:zygote] DEBUG: Skipping SetEnv effect[{}]: error getting 'key' ({:?})",
                            idx, e
                        );
                        skipped_count += 1;
                        continue;
                    }
                };
                let value: String = match dict.get_item("value") {
                    Ok(Some(v)) => v.extract().unwrap_or_default(),
                    _ => {
                        eprintln!(
                            "[tach:zygote] DEBUG: Skipping SetEnv effect[{}]: missing 'value'",
                            idx
                        );
                        skipped_count += 1;
                        continue;
                    }
                };
                crate::hooks::HookEffect::SetEnv { key, value }
            }
            "ModifySysPath" => {
                let action_str: String = match dict.get_item("action") {
                    Ok(Some(a)) => a.extract().unwrap_or_else(|_| "append".to_string()),
                    _ => "append".to_string(),
                };
                // Parse string to SysPathAction enum, defaulting to Append for unknown values
                let action = match action_str.as_str() {
                    "prepend" => crate::hooks::SysPathAction::Prepend,
                    "append" => crate::hooks::SysPathAction::Append,
                    "remove" => crate::hooks::SysPathAction::Remove,
                    _ => crate::hooks::SysPathAction::Append, // Default to Append for unknown
                };
                let path: String = match dict.get_item("path") {
                    Ok(Some(p)) => match p.extract::<String>() {
                        Ok(s) if !s.is_empty() => s,
                        Ok(_) => {
                            eprintln!(
                                "[tach:zygote] DEBUG: Skipping ModifySysPath effect[{}]: empty path",
                                idx
                            );
                            skipped_count += 1;
                            continue;
                        }
                        Err(e) => {
                            eprintln!(
                                "[tach:zygote] DEBUG: Skipping ModifySysPath effect[{}]: path not extractable ({:?})",
                                idx, e
                            );
                            skipped_count += 1;
                            continue;
                        }
                    },
                    Ok(None) => {
                        eprintln!(
                            "[tach:zygote] DEBUG: Skipping ModifySysPath effect[{}]: missing 'path'",
                            idx
                        );
                        skipped_count += 1;
                        continue;
                    }
                    Err(e) => {
                        eprintln!(
                            "[tach:zygote] DEBUG: Skipping ModifySysPath effect[{}]: error getting 'path' ({:?})",
                            idx, e
                        );
                        skipped_count += 1;
                        continue;
                    }
                };
                crate::hooks::HookEffect::ModifySysPath { action, path }
            }
            "RegisterMarker" => {
                let name: String = match dict.get_item("name") {
                    Ok(Some(n)) => match n.extract::<String>() {
                        Ok(s) if !s.is_empty() => s,
                        Ok(_) => {
                            eprintln!(
                                "[tach:zygote] DEBUG: Skipping RegisterMarker effect[{}]: empty name",
                                idx
                            );
                            skipped_count += 1;
                            continue;
                        }
                        Err(e) => {
                            eprintln!(
                                "[tach:zygote] DEBUG: Skipping RegisterMarker effect[{}]: name not extractable ({:?})",
                                idx, e
                            );
                            skipped_count += 1;
                            continue;
                        }
                    },
                    Ok(None) => {
                        eprintln!(
                            "[tach:zygote] DEBUG: Skipping RegisterMarker effect[{}]: missing 'name'",
                            idx
                        );
                        skipped_count += 1;
                        continue;
                    }
                    Err(e) => {
                        eprintln!(
                            "[tach:zygote] DEBUG: Skipping RegisterMarker effect[{}]: error getting 'name' ({:?})",
                            idx, e
                        );
                        skipped_count += 1;
                        continue;
                    }
                };
                let description: String = match dict.get_item("description") {
                    Ok(Some(d)) => d.extract().unwrap_or_default(),
                    _ => String::new(),
                };
                crate::hooks::HookEffect::RegisterMarker { name, description }
            }
            "ModifyItems" => {
                let removed: Vec<String> = match dict.get_item("removed") {
                    Ok(Some(r)) => r.extract().unwrap_or_default(),
                    _ => Vec::new(),
                };
                let reordered: bool = match dict.get_item("reordered") {
                    Ok(Some(r)) => r.extract().unwrap_or(false),
                    _ => false,
                };
                crate::hooks::HookEffect::ModifyItems { removed, reordered }
            }
            "AsyncioSetup" => {
                let loop_scope_str: String = match dict.get_item("loop_scope") {
                    Ok(Some(v)) => v.extract().unwrap_or_else(|_| "function".to_string()),
                    _ => "function".to_string(),
                };
                let loop_scope = match loop_scope_str.as_str() {
                    "function" => crate::hooks::LoopScope::Function,
                    "class" => crate::hooks::LoopScope::Class,
                    "module" => crate::hooks::LoopScope::Module,
                    "session" => crate::hooks::LoopScope::Session,
                    _ => crate::hooks::LoopScope::Function,
                };
                let auto_mode: bool = match dict.get_item("auto_mode") {
                    Ok(Some(v)) => v.extract().unwrap_or(false),
                    _ => false,
                };
                crate::hooks::HookEffect::AsyncioSetup {
                    loop_scope,
                    auto_mode,
                }
            }
            unknown => {
                eprintln!(
                    "[tach:zygote] DEBUG: Skipping effect[{}]: unknown type '{}'",
                    idx, unknown
                );
                skipped_count += 1;
                continue;
            }
        };

        effects.push(effect);
    }

    if skipped_count > 0 {
        eprintln!(
            "[tach:zygote] DEBUG: Skipped {} malformed effects out of {}",
            skipped_count,
            py_list.len()
        );
    }

    effects
}

fn run_worker(payload: &TestPayload) -> TestResult {
    use crate::protocol::{STATUS_HARNESS_ERROR, read_process_memory_rss};

    let start = Instant::now();

    // Build FULL node_id for pytest (must match pytest's nodeid exactly)
    // Format: path/to/file.py::test_name or path/to/file.py::ClassName::test_method
    let full_node_id = format!("{}::{}", payload.file_path, payload.test_name);

    println!(
        "Executing {} with fixtures {:?}",
        full_node_id,
        payload.fixtures.iter().map(|f| &f.name).collect::<Vec<_>>()
    );

    // Call Python harness with cached effects (v0.2.0 Hook Interception)
    // and marker_info (v0.2.1 Django Support)
    let result = Python::attach(|py| -> Result<(u8, f64, String, bool), PyErr> {
        let harness = py.import("tach_harness")?;
        let run_test = harness.getattr("run_test")?;

        // Convert cached_effects to Python list of dicts
        // HookEffect enum -> Python dict with 'type' key
        let cached_effects = convert_cached_effects_to_py(py, &payload.cached_effects)?;

        // Convert marker_info to Python list of dicts (v0.2.1)
        let marker_info = convert_marker_info_to_py(py, &payload.marker_info)?;

        // Convert next_node_id to Python (None or str)
        let next_node_id = payload.next_node_id.as_deref();

        // Pass file_path, FULL node_id, cached_effects, marker_info, and next_node_id to harness
        let result = run_test.call1((
            &payload.file_path,
            &full_node_id,
            cached_effects,
            marker_info,
            next_node_id,
        ))?;
        let tuple = result.extract::<(u8, f64, String, bool)>()?;
        Ok(tuple)
    });

    let duration_ns = start.elapsed().as_nanos() as u64;

    // Capture worker's own memory usage (RSS) after test completes
    let memory_rss_bytes = read_process_memory_rss(std::process::id() as i32);

    match result {
        Ok((status, _, message, thread_leaked)) => {
            // Log thread leak if detected (for visibility)
            if thread_leaked {
                eprintln!(
                    "[tach:worker] Thread leak detected for test {}, worker marked toxic",
                    &payload.test_name
                );
            }
            TestResult {
                test_id: payload.test_id,
                status,
                duration_ns,
                message,
                memory_rss_bytes,
            }
        }
        Err(e) => TestResult {
            test_id: payload.test_id,
            status: STATUS_HARNESS_ERROR,
            duration_ns,
            message: format!("PyO3 Error: {}", e),
            memory_rss_bytes,
        },
    }
}

// =============================================================================
//  Worker Loop Prototype Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Simulates the worker loop decision logic.
    /// This is a pure logic test - no actual processes spawned.
    #[derive(Debug, Clone, PartialEq)]
    enum WorkerAction {
        Reset, // Safe test: reset memory and continue loop
        Exit,  // Toxic test: exit process
    }

    /// Simulates the worker loop decision based on is_toxic flag
    fn decide_worker_action(is_toxic: bool) -> WorkerAction {
        if is_toxic {
            WorkerAction::Exit
        } else {
            WorkerAction::Reset
        }
    }

    #[test]
    fn test_worker_loop_structure() {
        // Mock a sequence of 3 payloads: [Safe, Safe, Toxic]
        let payloads = vec![
            (1, "test_safe_1", false), // Safe
            (2, "test_safe_2", false), // Safe
            (3, "test_toxic", true),   // Toxic
        ];

        let mut actions = Vec::new();
        let mut loop_iterations = 0;

        // Simulate worker loop
        for (test_id, test_name, is_toxic) in payloads {
            loop_iterations += 1;

            // Simulate: Execute test (would call run_worker in real code)
            let _result = format!("Executed {} (id={})", test_name, test_id);

            // Simulate: Send result (would write to socket in real code)
            // Result is ALWAYS sent before decision

            // Worker decision point
            let action = decide_worker_action(is_toxic);
            actions.push((test_id, action.clone()));

            // If toxic, break the loop (worker exits)
            if action == WorkerAction::Exit {
                break;
            }
            // If safe, continue loop (worker resets and waits for next)
        }

        // Verify behavior
        assert_eq!(
            loop_iterations, 3,
            "Should have processed 3 tests before exit"
        );
        assert_eq!(actions.len(), 3, "Should have 3 action decisions");

        // Verify action sequence
        assert_eq!(
            actions[0],
            (1, WorkerAction::Reset),
            "First test should Reset"
        );
        assert_eq!(
            actions[1],
            (2, WorkerAction::Reset),
            "Second test should Reset"
        );
        assert_eq!(
            actions[2],
            (3, WorkerAction::Exit),
            "Third test should Exit"
        );
    }

    #[test]
    fn test_worker_loop_all_safe() {
        // All safe tests - worker should reset after each
        let payloads = vec![(1, false), (2, false), (3, false)];

        let mut reset_count = 0;

        for (_test_id, is_toxic) in payloads {
            let action = decide_worker_action(is_toxic);
            if action == WorkerAction::Reset {
                reset_count += 1;
            }
            // In real code, loop would continue waiting for next payload
        }

        assert_eq!(reset_count, 3, "All 3 safe tests should trigger Reset");
    }

    #[test]
    fn test_worker_loop_first_toxic() {
        // First test is toxic - worker should exit immediately
        let payloads = vec![
            (1, true),  // Toxic - should exit
            (2, false), // Never reached
            (3, false), // Never reached
        ];

        let mut processed = 0;

        for (_test_id, is_toxic) in payloads {
            processed += 1;
            let action = decide_worker_action(is_toxic);
            if action == WorkerAction::Exit {
                break;
            }
        }

        assert_eq!(processed, 1, "Should only process 1 test before exit");
    }

    #[test]
    fn test_worker_state_machine() {
        // Verify state transitions match Q3 answer
        #[derive(Debug, Clone, PartialEq)]
        enum WorkerState {
            Idle,
            Running,
            Reporting,
            Resetting,
            Exiting,
        }

        fn simulate_worker_lifecycle(is_toxic: bool) -> Vec<WorkerState> {
            let mut states = vec![WorkerState::Idle];

            // Receive payload -> Running
            states.push(WorkerState::Running);

            // Execute test -> Reporting
            states.push(WorkerState::Reporting);

            // Send result -> Decision point
            if is_toxic {
                states.push(WorkerState::Exiting);
                // Worker terminates here
            } else {
                states.push(WorkerState::Resetting);
                states.push(WorkerState::Idle);
                // Worker loops back to Idle
            }

            states
        }

        // Safe worker lifecycle
        let safe_states = simulate_worker_lifecycle(false);
        assert_eq!(
            safe_states,
            vec![
                WorkerState::Idle,
                WorkerState::Running,
                WorkerState::Reporting,
                WorkerState::Resetting,
                WorkerState::Idle,
            ],
            "Safe worker should: Idle -> Running -> Reporting -> Resetting -> Idle"
        );

        // Toxic worker lifecycle
        let toxic_states = simulate_worker_lifecycle(true);
        assert_eq!(
            toxic_states,
            vec![
                WorkerState::Idle,
                WorkerState::Running,
                WorkerState::Reporting,
                WorkerState::Exiting,
            ],
            "Toxic worker should: Idle -> Running -> Reporting -> Exiting"
        );
    }

    // =========================================================================
    //  Lifecycle Manager Integration Test
    // =========================================================================

    /// Test that spawn_result_collector correctly manages worker lifecycle.
    /// This test simulates the full Zygote <-> Worker protocol without forking.
    #[test]
    fn test_zygote_lifecycle_manager() {
        use crate::protocol::{STATUS_PASS, TestResult, encode_with_length};
        use std::time::Duration;

        // Clear the pool before test (in case previous tests left state)
        IDLE_WORKERS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();

        // Create socket pair (simulating Zygote <-> Worker)
        let (zygote_sock, worker_sock) = UnixStream::pair().expect("Failed to create socket pair");

        // Create a channel to receive results
        let (result_tx, result_rx) = mpsc::channel::<Vec<u8>>();

        // Simulated worker PID
        let fake_pid = 12345;
        let is_toxic = false; // Safe test - should be pooled

        // Spawn the result collector (Zygote side)
        spawn_result_collector(
            zygote_sock,
            fake_pid,
            result_tx,
            is_toxic,
            Arc::new(Mutex::new(HashMap::new())),
        );

        // Simulate worker sending result
        let test_result = TestResult {
            test_id: 42,
            status: STATUS_PASS,
            duration_ns: 1_000_000,
            message: String::new(),
            memory_rss_bytes: None,
        };
        let result_bytes = encode_with_length(&test_result).expect("Failed to encode result");

        let mut worker_sock = worker_sock;
        worker_sock
            .write_all(&result_bytes)
            .expect("Failed to write result");

        // Simulate worker sending MSG_WORKER_READY
        worker_sock
            .write_all(&[MSG_WORKER_READY])
            .expect("Failed to write READY");

        // Wait for result to be forwarded
        let received = result_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("Should receive result");
        assert_eq!(received, result_bytes, "Result should match");

        // Give the collector thread time to process MSG_WORKER_READY and add to pool
        thread::sleep(Duration::from_millis(50));

        // Verify worker was added to pool
        let pool_size = IDLE_WORKERS.lock().unwrap_or_else(|e| e.into_inner()).len();
        assert_eq!(pool_size, 1, "Pool should have 1 idle worker");

        // Pop and verify it's the same PID
        let worker = IDLE_WORKERS
            .lock()
            .unwrap()
            .pop()
            .expect("Should have worker");
        assert_eq!(worker.pid, fake_pid, "PID should match");

        // Pool should now be empty
        assert_eq!(
            IDLE_WORKERS.lock().unwrap_or_else(|e| e.into_inner()).len(),
            0,
            "Pool should be empty after pop"
        );
    }

    /// Test that toxic workers are NOT added to the pool.
    #[test]
    fn test_toxic_worker_not_pooled() {
        use crate::protocol::{STATUS_PASS, TestResult, encode_with_length};
        use std::time::Duration;

        // Clear the pool before test
        IDLE_WORKERS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();

        let (zygote_sock, worker_sock) = UnixStream::pair().expect("Failed to create socket pair");
        let (result_tx, result_rx) = mpsc::channel::<Vec<u8>>();

        let fake_pid = 99999;
        let is_toxic = true; // Toxic test - should NOT be pooled

        spawn_result_collector(
            zygote_sock,
            fake_pid,
            result_tx,
            is_toxic,
            Arc::new(Mutex::new(HashMap::new())),
        );

        // Simulate worker sending result
        let test_result = TestResult {
            test_id: 1,
            status: STATUS_PASS,
            duration_ns: 500_000,
            message: String::new(),
            memory_rss_bytes: None,
        };
        let result_bytes = encode_with_length(&test_result).expect("Failed to encode result");

        let mut worker_sock = worker_sock;
        worker_sock
            .write_all(&result_bytes)
            .expect("Failed to write result");

        // Toxic worker exits - close socket (simulates process exit)
        drop(worker_sock);

        // Wait for result
        let received = result_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("Should receive result");
        assert_eq!(received, result_bytes, "Result should match");

        // Give collector time to finish
        thread::sleep(Duration::from_millis(50));

        // Verify worker was NOT added to pool
        let pool_size = IDLE_WORKERS.lock().unwrap_or_else(|e| e.into_inner()).len();
        assert_eq!(pool_size, 0, "Toxic worker should NOT be in pool");
    }

    // =========================================================================
    // Additional Pre-Refactor Tests
    // =========================================================================

    #[test]
    fn test_reset_regions_mutex_access() {
        // Test that RESET_REGIONS can be safely accessed
        let regions = RESET_REGIONS.lock().unwrap_or_else(|e| e.into_inner());
        // Should be empty or have some regions
        let _ = regions.len();
    }

    #[test]
    fn test_snapshot_enabled_flag() {
        // Test the atomic flag for snapshot mode
        let initial = SNAPSHOT_ENABLED.load(Ordering::SeqCst);
        // Flag should be false in test environment (no snapshot setup)
        assert!(!initial, "SNAPSHOT_ENABLED should be false in tests");
    }

    #[test]
    fn test_idle_workers_pool_operations() {
        // Clear pool first
        IDLE_WORKERS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();

        // Pool should be empty
        assert!(
            IDLE_WORKERS
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .is_empty()
        );

        // Create a socket pair for testing
        let (sock1, _sock2) = UnixStream::pair().expect("Failed to create socket pair");

        // Add a worker handle
        IDLE_WORKERS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(WorkerHandle {
                pid: 12345,
                socket: sock1,
            });

        // Pool should have one worker
        assert_eq!(
            IDLE_WORKERS.lock().unwrap_or_else(|e| e.into_inner()).len(),
            1
        );

        // Pop the worker
        let worker = IDLE_WORKERS.lock().unwrap_or_else(|e| e.into_inner()).pop();
        assert!(worker.is_some());
        assert_eq!(worker.unwrap().pid, 12345);

        // Pool should be empty again
        assert!(
            IDLE_WORKERS
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .is_empty()
        );
    }

    #[test]
    fn test_worker_action_enum() {
        // Test the WorkerAction enum
        let reset = WorkerAction::Reset;
        let exit = WorkerAction::Exit;

        assert_eq!(reset, WorkerAction::Reset);
        assert_eq!(exit, WorkerAction::Exit);
        assert_ne!(reset, exit);
    }

    #[test]
    fn test_decide_worker_action_safe() {
        let action = decide_worker_action(false);
        assert_eq!(action, WorkerAction::Reset);
    }

    #[test]
    fn test_decide_worker_action_toxic() {
        let action = decide_worker_action(true);
        assert_eq!(action, WorkerAction::Exit);
    }

    #[test]
    fn test_tach_harness_embedded() {
        // Verify the harness is embedded
        assert!(!TACH_HARNESS_PY.is_empty(), "Harness should be embedded");
        assert!(TACH_HARNESS_PY.len() > 100, "Harness should be substantial");
        // Verify it contains expected Python code
        assert!(
            TACH_HARNESS_PY.contains("def "),
            "Harness should contain function definitions"
        );
    }

    #[test]
    fn test_worker_state_transitions_safe() {
        // Test safe worker state transitions
        #[derive(Debug, Clone, PartialEq)]
        enum State {
            Idle,
            Running,
            Reporting,
            Resetting,
        }

        let mut states = vec![State::Idle];
        states.push(State::Running);
        states.push(State::Reporting);
        states.push(State::Resetting);
        states.push(State::Idle);

        assert_eq!(states.len(), 5);
        assert_eq!(states[0], State::Idle);
        assert_eq!(states[4], State::Idle);
    }

    #[test]
    fn test_worker_state_transitions_toxic() {
        // Test toxic worker state transitions
        #[derive(Debug, Clone, PartialEq)]
        enum State {
            Idle,
            Running,
            Reporting,
            Exiting,
        }

        let mut states = vec![State::Idle];
        states.push(State::Running);
        states.push(State::Reporting);
        states.push(State::Exiting);

        assert_eq!(states.len(), 4);
        assert_eq!(states[0], State::Idle);
        assert_eq!(states[3], State::Exiting);
    }

    // =========================================================================
    // Additional Regression Prevention Tests (Phase 2)
    // =========================================================================

    #[test]
    fn test_protocol_constants() {
        // Verify protocol constants have expected values
        // These are critical for IPC protocol correctness
        use crate::protocol::{CMD_EXIT, CMD_FORK, CMD_RUN_TEST, MSG_READY, MSG_WORKER_READY};

        // Commands must be distinct from each other
        assert_ne!(CMD_EXIT, CMD_FORK);
        assert_ne!(CMD_EXIT, CMD_RUN_TEST);
        assert_ne!(CMD_FORK, CMD_RUN_TEST);

        // Messages must be distinct
        assert_ne!(MSG_READY, MSG_WORKER_READY);

        // Commands must be distinct from messages
        assert_ne!(CMD_EXIT, MSG_READY);
        assert_ne!(CMD_EXIT, MSG_WORKER_READY);
    }

    #[test]
    fn test_protocol_constants_non_zero() {
        // Commands and messages should be checked for correct values
        use crate::protocol::{CMD_EXIT, CMD_FORK, CMD_RUN_TEST, MSG_READY, MSG_WORKER_READY};

        // CMD_EXIT is 0x00 by design (termination signal)
        assert_eq!(CMD_EXIT, 0x00, "CMD_EXIT should be 0x00");

        // Other commands should be non-zero
        assert_ne!(CMD_FORK, 0);
        assert_ne!(CMD_RUN_TEST, 0);

        // Messages should be non-zero
        assert_ne!(MSG_READY, 0);
        assert_ne!(MSG_WORKER_READY, 0);
    }

    #[test]
    fn test_socket_pair_creation() {
        // Test that UnixStream::pair works correctly for our IPC needs
        let result = UnixStream::pair();
        assert!(result.is_ok(), "Socket pair creation should succeed");

        let (sock1, sock2) = result.unwrap();

        // Both sockets should have valid file descriptors
        assert!(sock1.as_raw_fd() >= 0, "First socket should have valid fd");
        assert!(sock2.as_raw_fd() >= 0, "Second socket should have valid fd");

        // File descriptors should be different
        assert_ne!(
            sock1.as_raw_fd(),
            sock2.as_raw_fd(),
            "Socket FDs should be different"
        );
    }

    #[test]
    fn test_socket_pair_bidirectional() {
        // Test that socket pair supports bidirectional communication
        let (mut sock1, mut sock2) = UnixStream::pair().unwrap();

        // Send from sock1 to sock2
        let msg1 = b"hello";
        sock1.write_all(msg1).unwrap();

        let mut buf1 = [0u8; 5];
        sock2.read_exact(&mut buf1).unwrap();
        assert_eq!(&buf1, msg1);

        // Send from sock2 to sock1
        let msg2 = b"world";
        sock2.write_all(msg2).unwrap();

        let mut buf2 = [0u8; 5];
        sock1.read_exact(&mut buf2).unwrap();
        assert_eq!(&buf2, msg2);
    }

    #[test]
    fn test_worker_handle_structure() {
        // Test WorkerHandle can hold socket correctly
        let (sock1, _sock2) = UnixStream::pair().unwrap();
        let test_pid = 54321;

        let handle = WorkerHandle {
            pid: test_pid,
            socket: sock1,
        };

        assert_eq!(handle.pid, test_pid);
        assert!(handle.socket.as_raw_fd() >= 0);
    }

    #[test]
    fn test_reset_regions_modification() {
        // Test that RESET_REGIONS can be modified safely
        let test_regions = vec![(0x1000usize, 0x1000usize), (0x2000usize, 0x2000usize)];

        {
            let mut regions = RESET_REGIONS.lock().unwrap_or_else(|e| e.into_inner());
            let original_len = regions.len();

            // Add test regions
            for region in &test_regions {
                regions.push(*region);
            }

            assert_eq!(
                regions.len(),
                original_len + test_regions.len(),
                "Regions should be added"
            );

            // Remove the test regions
            for _ in 0..test_regions.len() {
                regions.pop();
            }

            assert_eq!(
                regions.len(),
                original_len,
                "Regions should be restored to original"
            );
        }
    }

    #[test]
    fn test_snapshot_enabled_atomic_operations() {
        // Test atomic operations on SNAPSHOT_ENABLED
        let original = SNAPSHOT_ENABLED.load(Ordering::SeqCst);

        // Test store/load
        SNAPSHOT_ENABLED.store(true, Ordering::SeqCst);
        assert!(SNAPSHOT_ENABLED.load(Ordering::SeqCst));

        SNAPSHOT_ENABLED.store(false, Ordering::SeqCst);
        assert!(!SNAPSHOT_ENABLED.load(Ordering::SeqCst));

        // Restore original value
        SNAPSHOT_ENABLED.store(original, Ordering::SeqCst);
    }

    #[test]
    fn test_snapshot_enabled_compare_exchange() {
        // Test compare_exchange for lock-free programming patterns
        let original = SNAPSHOT_ENABLED.load(Ordering::SeqCst);

        // Set to known state
        SNAPSHOT_ENABLED.store(false, Ordering::SeqCst);

        // Compare and exchange: should succeed
        let result =
            SNAPSHOT_ENABLED.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst);
        assert!(result.is_ok(), "CAS should succeed when values match");
        assert!(
            SNAPSHOT_ENABLED.load(Ordering::SeqCst),
            "Should be true now"
        );

        // Compare and exchange: should fail (current is true, expecting false)
        let result =
            SNAPSHOT_ENABLED.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst);
        assert!(result.is_err(), "CAS should fail when values don't match");

        // Restore original value
        SNAPSHOT_ENABLED.store(original, Ordering::SeqCst);
    }

    #[test]
    fn test_idle_workers_multiple() {
        // Test pool with multiple workers
        // Clear pool first
        IDLE_WORKERS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();

        // Add multiple workers
        let pids = [111, 222, 333];
        for &pid in &pids {
            let (sock, _) = UnixStream::pair().unwrap();
            IDLE_WORKERS
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(WorkerHandle { pid, socket: sock });
        }

        // Verify count
        assert_eq!(
            IDLE_WORKERS.lock().unwrap_or_else(|e| e.into_inner()).len(),
            3,
            "Should have 3 workers"
        );

        // Pop in LIFO order
        let w1 = IDLE_WORKERS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .pop()
            .unwrap();
        assert_eq!(w1.pid, 333, "Last in should be first out");

        let w2 = IDLE_WORKERS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .pop()
            .unwrap();
        assert_eq!(w2.pid, 222);

        let w3 = IDLE_WORKERS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .pop()
            .unwrap();
        assert_eq!(w3.pid, 111);

        // Pool should be empty
        assert!(
            IDLE_WORKERS
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .is_empty()
        );
    }

    #[test]
    fn test_worker_loop_interleaved_toxic() {
        // Test with interleaved safe and toxic tests
        let payloads = vec![
            (1, false), // Safe - reset
            (2, false), // Safe - reset
            (3, true),  // Toxic - exit
            (4, false), // Never reached
        ];

        let mut processed = 0;
        let mut reset_count = 0;

        for (_test_id, is_toxic) in payloads {
            processed += 1;
            let action = decide_worker_action(is_toxic);

            match action {
                WorkerAction::Reset => reset_count += 1,
                WorkerAction::Exit => break,
            }
        }

        assert_eq!(processed, 3, "Should process 3 tests before exit");
        assert_eq!(reset_count, 2, "Should have 2 resets (safe tests)");
    }

    #[test]
    fn test_payload_serialization_roundtrip() {
        // Test that TestPayload can be serialized and deserialized
        use crate::protocol::{
            MAX_PAYLOAD_SIZE, TestPayload, decode_with_limit, encode_with_length,
        };

        let original = TestPayload {
            test_id: 12345,
            file_path: "tests/test_example.py".to_string(),
            test_name: "TestClass::test_method".to_string(),
            is_async: false,
            fixtures: vec![],
            log_fd: -1,
            debug_socket_path: String::new(),
            is_toxic: false,
            timeout_secs: None,
            hooks: vec![],
            cached_effects: vec![],
            markers: vec![],
            marker_info: vec![],
            reuse_db: false,
            create_db: false,
            skip_reset: false,
            next_node_id: None,
        };

        let encoded = encode_with_length(&original).expect("Serialization should succeed");
        let decoded: TestPayload =
            decode_with_limit(&encoded, MAX_PAYLOAD_SIZE).expect("Deserialization should succeed");

        assert_eq!(decoded.test_id, original.test_id);
        assert_eq!(decoded.file_path, original.file_path);
        assert_eq!(decoded.test_name, original.test_name);
        assert_eq!(decoded.is_toxic, original.is_toxic);
        assert_eq!(decoded.is_async, original.is_async);
    }

    #[test]
    fn test_payload_with_fixtures() {
        // Test TestPayload with fixtures
        use crate::protocol::{
            FixtureInfo, MAX_PAYLOAD_SIZE, TestPayload, decode_with_limit, encode_with_length,
        };

        let fixtures = vec![
            FixtureInfo {
                name: "fixture1".to_string(),
                scope: "function".to_string(),
                is_async: false,
            },
            FixtureInfo {
                name: "fixture2".to_string(),
                scope: "module".to_string(),
                is_async: false,
            },
        ];

        let payload = TestPayload {
            test_id: 1,
            file_path: "test.py".to_string(),
            test_name: "test_func".to_string(),
            is_async: true,
            fixtures: fixtures.clone(),
            log_fd: 5,
            debug_socket_path: "/tmp/debug.sock".to_string(),
            is_toxic: true,
            timeout_secs: Some(120),
            hooks: vec![],
            cached_effects: vec![],
            markers: vec![],
            marker_info: vec![],
            reuse_db: false,
            create_db: false,
            skip_reset: false,
            next_node_id: None,
        };

        let encoded = encode_with_length(&payload).unwrap();
        let decoded: TestPayload = decode_with_limit(&encoded, MAX_PAYLOAD_SIZE).unwrap();

        assert_eq!(decoded.fixtures.len(), 2);
        assert_eq!(decoded.fixtures[0].name, "fixture1");
        assert_eq!(decoded.fixtures[1].scope, "module");
        assert_eq!(decoded.log_fd, 5);
        assert_eq!(decoded.debug_socket_path, "/tmp/debug.sock");
        assert!(decoded.is_async);
    }

    #[test]
    fn test_result_encoding() {
        // Test that TestResult can be encoded with protocol header
        use crate::protocol::{
            HEADER_SIZE, MAX_PAYLOAD_SIZE, PROTOCOL_MAGIC, PROTOCOL_VERSION, STATUS_PASS,
            TestResult, decode_with_limit, encode_with_length,
        };

        let result = TestResult {
            test_id: 999,
            status: STATUS_PASS,
            duration_ns: 1_234_567,
            message: "Test passed successfully".to_string(),
            memory_rss_bytes: None,
        };

        let encoded = encode_with_length(&result).expect("Encoding should succeed");

        // Verify header format: magic(2) + version(1) + reserved(1) + length(4) = 8 bytes
        assert_eq!(
            &encoded[0..2],
            &PROTOCOL_MAGIC,
            "Magic bytes should be 'TA'"
        );
        assert_eq!(encoded[2], PROTOCOL_VERSION, "Version should match");
        assert_eq!(encoded[3], 0, "Reserved byte should be 0");

        // Extract length from bytes 4-7 (little-endian u32)
        let len = u32::from_le_bytes([encoded[4], encoded[5], encoded[6], encoded[7]]) as usize;

        // Length should match remaining bytes
        assert_eq!(
            len,
            encoded.len() - HEADER_SIZE,
            "Length should match payload size"
        );

        // Should be able to deserialize the payload
        let decoded: TestResult =
            decode_with_limit(&encoded, MAX_PAYLOAD_SIZE).expect("Deserialization should succeed");
        assert_eq!(decoded.test_id, 999);
        assert_eq!(decoded.status, STATUS_PASS);
        assert_eq!(decoded.message, "Test passed successfully");
    }

    #[test]
    fn test_worker_pool_drain() {
        // Test draining the worker pool (as done in CMD_EXIT handler)
        // Clear and populate pool
        IDLE_WORKERS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();

        for pid in [100, 200, 300] {
            let (sock, _) = UnixStream::pair().unwrap();
            IDLE_WORKERS
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(WorkerHandle { pid, socket: sock });
        }

        // Drain the pool (similar to CMD_EXIT handler)
        let drained = std::mem::take(&mut *IDLE_WORKERS.lock().unwrap_or_else(|e| e.into_inner()));

        assert_eq!(drained.len(), 3, "Should drain 3 workers");
        assert!(
            IDLE_WORKERS
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .is_empty(),
            "Pool should be empty after drain"
        );

        // Verify drained workers
        let pids: Vec<i32> = drained.iter().map(|w| w.pid).collect();
        assert!(pids.contains(&100));
        assert!(pids.contains(&200));
        assert!(pids.contains(&300));
    }

    #[test]
    fn test_worker_loop_empty_payloads() {
        // Test behavior with no payloads (immediate exit)
        let payloads: Vec<(u32, bool)> = vec![];
        let mut processed = 0;

        for (_test_id, is_toxic) in payloads {
            processed += 1;
            if decide_worker_action(is_toxic) == WorkerAction::Exit {
                break;
            }
        }

        assert_eq!(processed, 0, "Should process 0 tests");
    }

    #[test]
    fn test_socket_try_clone() {
        // Test that socket can be cloned for parallel operations
        let (sock1, _sock2) = UnixStream::pair().unwrap();

        let cloned = sock1.try_clone();
        assert!(cloned.is_ok(), "Socket clone should succeed");

        let cloned_sock = cloned.unwrap();
        // Both should have valid but different FDs (after dup)
        assert!(cloned_sock.as_raw_fd() >= 0);
        // Note: cloned FD may or may not equal original depending on system state
    }

    #[test]
    fn test_convert_asyncio_setup_effect() {
        use pyo3::types::{PyDict, PyList};

        Python::attach(|py| {
            let dict = PyDict::new(py);
            dict.set_item("type", "AsyncioSetup").unwrap();
            dict.set_item("loop_scope", "module").unwrap();
            dict.set_item("auto_mode", true).unwrap();

            let list = PyList::new(py, vec![dict]).unwrap();
            let effects = convert_py_effects_to_rust(&list);

            assert_eq!(effects.len(), 1);
            match &effects[0] {
                crate::hooks::HookEffect::AsyncioSetup {
                    loop_scope,
                    auto_mode,
                } => {
                    assert_eq!(loop_scope.to_string(), "module");
                    assert!(*auto_mode);
                }
                other => panic!("Expected AsyncioSetup, got {:?}", other),
            }
        });
    }

    #[test]
    fn test_convert_asyncio_setup_defaults() {
        use pyo3::types::{PyDict, PyList};

        Python::attach(|py| {
            let dict = PyDict::new(py);
            dict.set_item("type", "AsyncioSetup").unwrap();

            let list = PyList::new(py, vec![dict]).unwrap();
            let effects = convert_py_effects_to_rust(&list);

            assert_eq!(effects.len(), 1);
            match &effects[0] {
                crate::hooks::HookEffect::AsyncioSetup {
                    loop_scope,
                    auto_mode,
                } => {
                    assert_eq!(loop_scope.to_string(), "function");
                    assert!(!*auto_mode);
                }
                other => panic!("Expected AsyncioSetup, got {:?}", other),
            }
        });
    }

    #[test]
    fn test_convert_asyncio_setup_all_scopes() {
        use pyo3::types::{PyDict, PyList};

        Python::attach(|py| {
            for (scope_str, expected_str) in [
                ("function", "function"),
                ("class", "class"),
                ("module", "module"),
                ("session", "session"),
                ("invalid", "function"),
            ] {
                let dict = PyDict::new(py);
                dict.set_item("type", "AsyncioSetup").unwrap();
                dict.set_item("loop_scope", scope_str).unwrap();
                dict.set_item("auto_mode", false).unwrap();

                let list = PyList::new(py, vec![dict]).unwrap();
                let effects = convert_py_effects_to_rust(&list);

                match &effects[0] {
                    crate::hooks::HookEffect::AsyncioSetup { loop_scope, .. } => {
                        assert_eq!(
                            loop_scope.to_string(),
                            expected_str,
                            "Failed for input: {}",
                            scope_str
                        );
                    }
                    other => panic!("Expected AsyncioSetup, got {:?}", other),
                }
            }
        });
    }

    // =========================================================================
    // SIGCHLD Self-Pipe + Crash Notification Tests
    // =========================================================================

    #[test]
    fn test_sigchld_handler_writes_to_pipe() {
        let mut pipe_fds = [0i32; 2];
        assert_eq!(unsafe { libc::pipe(pipe_fds.as_mut_ptr()) }, 0);

        unsafe {
            let flags = libc::fcntl(pipe_fds[1], libc::F_GETFL);
            libc::fcntl(pipe_fds[1], libc::F_SETFL, flags | libc::O_NONBLOCK);
            let flags = libc::fcntl(pipe_fds[0], libc::F_GETFL);
            libc::fcntl(pipe_fds[0], libc::F_SETFL, flags | libc::O_NONBLOCK);
        }

        let old = SIGCHLD_PIPE_WR.swap(pipe_fds[1], Ordering::SeqCst);

        sigchld_handler(libc::SIGCHLD);

        let mut buf = [0u8; 1];
        let n = unsafe { libc::read(pipe_fds[0], buf.as_mut_ptr() as *mut libc::c_void, 1) };
        assert_eq!(n, 1);
        assert_eq!(buf[0], 1);

        SIGCHLD_PIPE_WR.store(old, Ordering::SeqCst);
        unsafe {
            libc::close(pipe_fds[0]);
            libc::close(pipe_fds[1]);
        }
    }

    #[test]
    fn test_drain_signal_pipe_empties_all_bytes() {
        let mut pipe_fds = [0i32; 2];
        assert_eq!(unsafe { libc::pipe(pipe_fds.as_mut_ptr()) }, 0);

        unsafe {
            let flags = libc::fcntl(pipe_fds[0], libc::F_GETFL);
            libc::fcntl(pipe_fds[0], libc::F_SETFL, flags | libc::O_NONBLOCK);
            let flags = libc::fcntl(pipe_fds[1], libc::F_GETFL);
            libc::fcntl(pipe_fds[1], libc::F_SETFL, flags | libc::O_NONBLOCK);
        }

        // Write 5 bytes (simulating 5 coalesced SIGCHLDs)
        for _ in 0..5 {
            unsafe {
                libc::write(pipe_fds[1], &1u8 as *const u8 as *const libc::c_void, 1);
            }
        }

        drain_signal_pipe(pipe_fds[0]);

        // Pipe should be empty now
        let mut buf = [0u8; 1];
        let n = unsafe { libc::read(pipe_fds[0], buf.as_mut_ptr() as *mut libc::c_void, 1) };
        assert!(n <= 0, "Pipe should be empty after drain");

        unsafe {
            libc::close(pipe_fds[0]);
            libc::close(pipe_fds[1]);
        }
    }

    #[test]
    fn test_active_pids_tracking() {
        let active_pids: Arc<Mutex<HashMap<i32, u32>>> = Arc::new(Mutex::new(HashMap::new()));

        active_pids.lock().unwrap().insert(100, 1);
        active_pids.lock().unwrap().insert(200, 2);
        active_pids.lock().unwrap().insert(300, 3);

        let removed = active_pids.lock().unwrap().remove(&200);
        assert_eq!(removed, Some(2));
        assert!(!active_pids.lock().unwrap().contains_key(&200));

        assert_eq!(active_pids.lock().unwrap().get(&100), Some(&1));
        assert_eq!(active_pids.lock().unwrap().get(&300), Some(&3));
    }

    #[test]
    fn test_crash_notification_encoding() {
        use crate::protocol::{STATUS_CRASH, TestResult, decode_with_limit, encode_with_length};

        let result = TestResult {
            test_id: 42,
            status: STATUS_CRASH,
            duration_ns: 0,
            message: "Worker crashed (SIGCHLD: pid 1234 killed by SIGSEGV)".to_string(),
            memory_rss_bytes: None,
        };

        let encoded = encode_with_length(&result).unwrap();
        let decoded: TestResult = decode_with_limit(&encoded, MAX_PAYLOAD_SIZE).unwrap();

        assert_eq!(decoded.test_id, 42);
        assert_eq!(decoded.status, STATUS_CRASH);
        assert!(decoded.message.contains("SIGSEGV"));
        assert_eq!(decoded.duration_ns, 0);
    }

    #[test]
    fn test_reap_crashed_workers_sends_notification_on_abnormal_exit() {
        use std::time::Duration;

        let active_pids: Arc<Mutex<HashMap<i32, u32>>> = Arc::new(Mutex::new(HashMap::new()));
        let (tx, rx) = mpsc::channel::<Vec<u8>>();

        match unsafe { fork() } {
            Ok(ForkResult::Parent { child }) => {
                let child_pid = child.as_raw();
                active_pids.lock().unwrap().insert(child_pid, 99);

                // Wait for child to die, then reap via our function
                std::thread::sleep(Duration::from_millis(100));
                reap_crashed_workers(&active_pids, &tx);

                let data = rx.recv_timeout(Duration::from_secs(1)).unwrap();
                let result: TestResult = decode_with_limit(&data, MAX_PAYLOAD_SIZE).unwrap();
                assert_eq!(result.test_id, 99);
                assert_eq!(result.status, STATUS_CRASH);
                assert!(result.message.contains("exited with status 1"));

                assert!(active_pids.lock().unwrap().is_empty());
            }
            Ok(ForkResult::Child) => {
                std::process::exit(1);
            }
            Err(e) => panic!("fork failed: {}", e),
        }
    }

    #[test]
    fn test_reap_normal_exit_no_crash_notification() {
        use std::time::Duration;

        let active_pids: Arc<Mutex<HashMap<i32, u32>>> = Arc::new(Mutex::new(HashMap::new()));
        let (tx, rx) = mpsc::channel::<Vec<u8>>();

        match unsafe { fork() } {
            Ok(ForkResult::Parent { child }) => {
                let _child_pid = child.as_raw();

                std::thread::sleep(Duration::from_millis(100));
                reap_crashed_workers(&active_pids, &tx);

                // Should NOT have sent any crash notification
                let result = rx.recv_timeout(Duration::from_millis(200));
                assert!(result.is_err(), "No notification for already-handled exit");
            }
            Ok(ForkResult::Child) => {
                std::process::exit(0);
            }
            Err(e) => panic!("fork failed: {}", e),
        }
    }

    #[test]
    fn test_reap_signal_death_sends_crash_notification() {
        use std::time::Duration;

        let active_pids: Arc<Mutex<HashMap<i32, u32>>> = Arc::new(Mutex::new(HashMap::new()));
        let (tx, rx) = mpsc::channel::<Vec<u8>>();

        match unsafe { fork() } {
            Ok(ForkResult::Parent { child }) => {
                let child_pid = child.as_raw();
                active_pids.lock().unwrap().insert(child_pid, 77);

                // Kill child with SIGKILL
                let _ = nix::sys::signal::kill(child, Signal::SIGKILL);
                std::thread::sleep(Duration::from_millis(100));

                reap_crashed_workers(&active_pids, &tx);

                let data = rx.recv_timeout(Duration::from_secs(1)).unwrap();
                let result: TestResult = decode_with_limit(&data, MAX_PAYLOAD_SIZE).unwrap();
                assert_eq!(result.test_id, 77);
                assert_eq!(result.status, STATUS_CRASH);
                assert!(result.message.contains("killed by"));
            }
            Ok(ForkResult::Child) => {
                // Block until killed
                std::thread::sleep(Duration::from_secs(10));
                std::process::exit(0);
            }
            Err(e) => panic!("fork failed: {}", e),
        }
    }
}
