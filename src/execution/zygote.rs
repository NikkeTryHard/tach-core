//! Zygote: Fork server with dual-channel IPC

use crate::environment::find_site_packages;
use crate::logcapture::redirect_output;
use crate::protocol::{
    encode_with_length, TestPayload, TestResult, CMD_EXIT, CMD_FORK, CMD_RUN_TEST, MSG_READY,
    MSG_WORKER_READY,
};
use crate::snapshot::send_fd;
use anyhow::Result;
use nix::sys::signal::{signal, SigHandler, Signal};
use nix::unistd::{fork, ForkResult};
use pyo3::ffi::c_str;
use pyo3::prelude::*;
use pyo3::types::{PyList, PyModule};
use std::env;
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::process;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Mutex;
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
                "[tach_rust] WARN: Failed to create userfaultfd: {}. Snapshotting disabled.",
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
                "[tach_rust] WARN: Failed to connect to supervisor: {}. Snapshotting disabled.",
                e
            );
            return Ok(false);
        }
    };

    // 3. Send PID + UFFD via SCM_RIGHTS
    if let Err(e) = send_fd(&sock, pid, uffd.as_raw_fd()) {
        eprintln!(
            "[tach_rust] WARN: Failed to send UFFD: {}. Snapshotting disabled.",
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
        eprintln!("[tach_rust] Cached {} regions for self-reset", count);
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
    eprintln!("[tach_rust] Quiescing jemalloc allocator before snapshot...");
    if let Err(e) = crate::allocator::quiesce_allocator() {
        eprintln!(
            "[tach_rust] WARNING: Failed to quiesce allocator: {}. \
             Memory corruption may occur after reset.",
            e
        );
    }

    // 5. Freeze self - Supervisor will capture snapshot and SIGCONT us
    eprintln!("[tach_rust] Freezing for snapshot (PID {})...", pid);
    if let Err(e) = nix::sys::signal::raise(Signal::SIGSTOP) {
        return Err(pyo3::exceptions::PyOSError::new_err(format!(
            "Failed to SIGSTOP: {}",
            e
        )));
    }

    // 6. We're back! Supervisor has registered our memory.
    SNAPSHOT_ENABLED.store(true, Ordering::SeqCst);
    eprintln!("[tach_rust] Resumed after snapshot capture");
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
        eprintln!("[tach_rust] reset_memory called but snapshot not enabled");
        return Ok(());
    }

    let regions = RESET_REGIONS.lock().unwrap_or_else(|e| e.into_inner());
    for &(start, len) in regions.iter() {
        // SAFETY: madvise with MADV_DONTNEED is safe - it just marks pages as discardable.
        // The kernel will zero-fill them on next access (or UFFD will handle it).
        let ret = unsafe { libc::madvise(start as *mut libc::c_void, len, libc::MADV_DONTNEED) };
        if ret != 0 {
            eprintln!(
                "[tach_rust] madvise failed for region {:x}-{:x}: {}",
                start,
                start + len,
                std::io::Error::last_os_error()
            );
        }
    }

    eprintln!(
        "[tach_rust] Self-reset complete: invalidated {} regions",
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

    eprintln!("[worker] Reset complete, signaled READY");
    Ok(())
}

/// Main worker loop - receives and executes tests until exit.
///
/// The worker enters this loop after completing its first safe test.
/// It waits for commands from Zygote:
/// - CMD_RUN_TEST: Execute test, send result, decide exit/reset
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
            eprintln!("[worker] Socket closed, exiting loop");
            break;
        }

        match cmd_buf[0] {
            CMD_RUN_TEST => {
                // Read payload length
                let mut len_buf = [0u8; 4];
                if socket.read_exact(&mut len_buf).is_err() {
                    eprintln!("[worker] Failed to read payload length");
                    break;
                }
                let len = u32::from_le_bytes(len_buf) as usize;

                // Read payload
                let mut payload_buf = vec![0u8; len];
                if socket.read_exact(&mut payload_buf).is_err() {
                    eprintln!("[worker] Failed to read payload");
                    break;
                }

                let payload: TestPayload = match bincode::serde::decode_from_slice(
                    &payload_buf,
                    bincode::config::standard(),
                ) {
                    Ok((p, _)) => p,
                    Err(e) => {
                        eprintln!("[worker] Deserialize error: {}", e);
                        break;
                    }
                };

                // Execute test
                let result = run_worker(&payload);

                // CRITICAL: Send result BEFORE exit decision
                let _ = std::io::stdout().flush();
                if let Ok(result_bytes) = encode_with_length(&result) {
                    if socket.write_all(&result_bytes).is_err() {
                        eprintln!("[worker] Failed to send result");
                        break;
                    }
                }

                // Dual-path decision
                if payload.is_toxic {
                    // TOXIC PATH: Exit loop, process will terminate
                    eprintln!("[worker] Toxic test completed, exiting");
                    break;
                } else {
                    // SAFE PATH: Reset memory and continue loop
                    if let Err(e) = reset_and_signal_ready(&socket) {
                        eprintln!("[worker] Reset failed: {}, exiting", e);
                        break;
                    }
                    // Loop continues - wait for next command
                }
            }
            CMD_EXIT => {
                eprintln!("[worker] Received EXIT command");
                break;
            }
            _ => {
                eprintln!("[worker] Unknown command: {:#x}", cmd_buf[0]);
            }
        }
    }
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
) {
    thread::spawn(move || {
        let mut socket = socket;

        // 1. Read result length prefix
        let mut result_len_buf = [0u8; 4];
        if socket.read_exact(&mut result_len_buf).is_err() {
            eprintln!("[zygote] Worker {} crashed before sending result", pid);
            return;
        }

        // 2. Read result payload
        let result_len = u32::from_le_bytes(result_len_buf) as usize;
        let mut result_buf = vec![0u8; result_len];
        if socket.read_exact(&mut result_buf).is_err() {
            eprintln!("[zygote] Worker {} crashed during result send", pid);
            return;
        }

        // 3. Forward result to Supervisor
        let mut full = result_len_buf.to_vec();
        full.extend(result_buf);
        if result_tx.send(full).is_err() {
            eprintln!("[zygote] Result channel closed");
            return;
        }

        // 4. Toxic workers exit here - don't wait for READY signal
        if is_toxic {
            eprintln!("[zygote] Toxic worker {} completed, not pooling", pid);
            return;
        }

        // 5. Safe workers: wait for MSG_WORKER_READY signal
        let mut ready_buf = [0u8; 1];
        match socket.read_exact(&mut ready_buf) {
            Ok(_) if ready_buf[0] == MSG_WORKER_READY => {
                // Worker is ready for reuse - add to pool
                eprintln!("[zygote] Worker {} ready, adding to pool", pid);
                if let Ok(mut workers) = IDLE_WORKERS.lock() {
                    workers.push(WorkerHandle { pid, socket });
                } else {
                    eprintln!("[zygote] WARNING: Failed to acquire lock for worker pool");
                }
            }
            Ok(_) => {
                eprintln!(
                    "[zygote] Worker {} sent unexpected byte: {:#x}",
                    pid, ready_buf[0]
                );
            }
            Err(_) => {
                eprintln!("[zygote] Worker {} died after result (no READY)", pid);
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

    // Prevent zombies
    unsafe { signal(Signal::SIGCHLD, SigHandler::SigIgn) }?;

    eprintln!("[zygote] Initializing Python...");
    let cwd = env::current_dir()?;
    let cwd_str = cwd.to_string_lossy().to_string();

    //  Detect venv and get site-packages path
    let site_packages = find_site_packages(&cwd);
    if let Some(ref sp) = site_packages {
        eprintln!("[zygote] Found venv: {}", sp.display());
    }

    Python::attach(|py| -> Result<()> {
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
            Ok(_) => eprintln!("[zygote] pytest loaded successfully"),
            Err(e) => {
                eprintln!("[zygote] Error: {}", e);
                return Err(anyhow::anyhow!("Failed to import pytest: {}", e));
            }
        }

        // Django Detection & Setup (Batteries-Included)
        // Initialize Django in Zygote so workers inherit the pre-warmed state
        py.run(
            c_str!(r#"
import os
import sys

try:
    import django
    
    # Check if DJANGO_SETTINGS_MODULE is already set
    if 'DJANGO_SETTINGS_MODULE' in os.environ:
        django.setup()
        print(f'[zygote] Django initialized: {os.environ["DJANGO_SETTINGS_MODULE"]}', file=sys.stderr)
        
        # CRITICAL: Warm up DB connections before forking
        # File descriptors must exist in Zygote to be inherited by workers
        try:
            from django.db import connections
            for alias in connections:
                connections[alias].ensure_connection()
            print(f'[zygote] Django DB connections warmed up', file=sys.stderr)
        except Exception as e:
            print(f'[zygote] Django DB warmup failed: {e}', file=sys.stderr)
except ImportError:
    pass  # Django not installed, skip
except Exception as e:
    print(f'[zygote] Django setup error: {e}', file=sys.stderr)
"#),
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
        let harness = PyModule::from_code(py, &harness_code, c"tach_harness.py", c"tach_harness")?;

        // ZYGOTE COLLECTION: Pre-collect tests for TARGET PATH only (not entire project)
        // This avoids importing test files outside the requested scope
        let target_path = std::env::var("TACH_TARGET_PATH").unwrap_or_else(|_| cwd_str.clone());
        harness.getattr("init_session")?.call1((&target_path,))?;

        sys.getattr("modules")?.set_item("tach_harness", harness)?;

        Ok(())
    })?;

    eprintln!("[zygote] Python ready.");

    // Signal ready on both sockets
    let mut cmd_socket = cmd_socket;
    let result_socket = result_socket;
    cmd_socket.write_all(&[MSG_READY])?;

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

    // Command processing loop
    let mut cmd_buf = [0u8; 1];
    loop {
        if cmd_socket.read(&mut cmd_buf).is_err() {
            break;
        }

        match cmd_buf[0] {
            CMD_FORK => {
                // Read payload
                let mut len_buf = [0u8; 4];
                cmd_socket.read_exact(&mut len_buf)?;
                let len = u32::from_le_bytes(len_buf) as usize;

                let mut payload_buf = vec![0u8; len];
                cmd_socket.read_exact(&mut payload_buf)?;

                let payload: TestPayload = match bincode::serde::decode_from_slice(
                    &payload_buf,
                    bincode::config::standard(),
                ) {
                    Ok((p, _)) => p,
                    Err(e) => {
                        eprintln!("[zygote] Deserialize error: {}", e);
                        continue;
                    }
                };

                let is_toxic = payload.is_toxic;

                //  Check for idle worker (only for safe tests)
                let idle_worker = if !is_toxic {
                    IDLE_WORKERS.lock().unwrap_or_else(|e| e.into_inner()).pop()
                } else {
                    None // Always fork fresh for toxic tests
                };

                if let Some(mut worker) = idle_worker {
                    // =========================================================
                    // REUSE PATH: Dispatch to existing worker
                    // =========================================================
                    eprintln!("[zygote] Reusing worker {} for test", worker.pid);

                    // Send CMD_RUN_TEST + payload to worker
                    let dispatch_ok = (|| -> std::io::Result<()> {
                        worker.socket.write_all(&[CMD_RUN_TEST])?;
                        worker.socket.write_all(&len_buf)?;
                        worker.socket.write_all(&payload_buf)?;
                        Ok(())
                    })();

                    if let Err(e) = dispatch_ok {
                        eprintln!(
                            "[zygote] Failed to dispatch to worker {}: {}",
                            worker.pid, e
                        );
                        // Worker died, fall through to fork path
                        // Don't continue - we need to fork a new worker
                    } else {
                        // Successfully dispatched - send PID back and spawn collector
                        cmd_socket.write_all(&worker.pid.to_le_bytes())?;
                        spawn_result_collector(
                            worker.socket,
                            worker.pid,
                            result_tx.clone(),
                            is_toxic,
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
                        cmd_socket.write_all(&child_pid.to_le_bytes())?;

                        // Use spawn_result_collector instead of inline thread
                        spawn_result_collector(parent_sock, child_pid, result_tx.clone(), is_toxic);
                    }
                    Ok(ForkResult::Child) => {
                        drop(parent_sock);

                        // 0. DEAD MAN'S SWITCH : If Zygote dies, worker dies
                        // Must be FIRST - before any resource allocation
                        unsafe {
                            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
                        }

                        // 1. CRITICAL: Restore default signal handling
                        // Parent sets SIG_IGN to avoid zombies, but this breaks Command::new()
                        // because waitpid fails when kernel auto-reaps children
                        unsafe { signal(Signal::SIGCHLD, SigHandler::SigDfl) }.ok();

                        // 2. ISOLATE filesystem and network (Iron Dome)
                        // CRITICAL: Fail hard if isolation fails to protect the host
                        let project_root = std::env::current_dir().unwrap_or_default();
                        if let Err(e) =
                            crate::isolation::setup_filesystem(payload.test_id, &project_root)
                        {
                            eprintln!("[worker] CRITICAL: Isolation failed. Aborting to protect host. Error: {:#}", e);
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
                        if let Ok(result_bytes) = encode_with_length(&result) {
                            if let Ok(mut sock) = child_sock.try_clone() {
                                let _ = sock.write_all(&result_bytes);
                            }
                        }

                        // 10.  Dual-path decision based on toxicity
                        // TOXIC PATH: Exit immediately (OS cleans up threads, FDs, etc.)
                        // SAFE PATH: Reset memory and enter worker loop for reuse
                        if payload.is_toxic {
                            // Toxic test: exit without reset
                            // This is the Isolation Mode path
                            process::exit(0);
                        } else {
                            // Safe test: reset memory and enter worker loop
                            // This is the Hypervisor Mode path - worker will be reused
                            if let Err(e) = reset_and_signal_ready(&child_sock) {
                                eprintln!("[worker] Reset failed after first test: {}, exiting", e);
                                process::exit(1);
                            }

                            // Enter worker loop - wait for subsequent tests
                            worker_loop(child_sock);
                            process::exit(0);
                        }
                    }
                    Err(e) => eprintln!("[zygote] Fork failed: {}", e),
                }
            }
            CMD_EXIT => {
                eprintln!("[zygote] Received EXIT.");

                //  Drain idle workers and send them EXIT commands
                let idle_workers =
                    std::mem::take(&mut *IDLE_WORKERS.lock().unwrap_or_else(|e| e.into_inner()));
                let worker_count = idle_workers.len();
                for mut worker in idle_workers {
                    eprintln!("[zygote] Sending EXIT to idle worker {}", worker.pid);
                    let _ = worker.socket.write_all(&[CMD_EXIT]);
                    // Socket drops here, worker will see EOF if write fails
                }
                if worker_count > 0 {
                    eprintln!("[zygote] Drained {} idle workers", worker_count);
                }

                // Give threads time to forward final results
                thread::sleep(std::time::Duration::from_millis(200));
                break;
            }
            _ => {}
        }
    }

    Ok(())
}

fn run_worker(payload: &TestPayload) -> TestResult {
    use crate::protocol::STATUS_HARNESS_ERROR;

    let start = Instant::now();

    // Build FULL node_id for pytest (must match pytest's nodeid exactly)
    // Format: path/to/file.py::test_name or path/to/file.py::ClassName::test_method
    let full_node_id = format!("{}::{}", payload.file_path, payload.test_name);

    println!(
        "Executing {} with fixtures {:?}",
        full_node_id,
        payload.fixtures.iter().map(|f| &f.name).collect::<Vec<_>>()
    );

    // Call Python harness
    let result = Python::attach(|py| -> Result<(u8, f64, String), PyErr> {
        let harness = py.import("tach_harness")?;
        let run_test = harness.getattr("run_test")?;

        // Pass file_path and FULL node_id to harness
        let result = run_test.call1((&payload.file_path, &full_node_id))?;
        let tuple = result.extract::<(u8, f64, String)>()?;
        Ok(tuple)
    });

    let duration_ns = start.elapsed().as_nanos() as u64;

    match result {
        Ok((status, _, message)) => TestResult {
            test_id: payload.test_id,
            status,
            duration_ns,
            message,
        },
        Err(e) => TestResult {
            test_id: payload.test_id,
            status: STATUS_HARNESS_ERROR,
            duration_ns,
            message: format!("PyO3 Error: {}", e),
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
        use crate::protocol::{encode_with_length, TestResult, STATUS_PASS};
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
        spawn_result_collector(zygote_sock, fake_pid, result_tx, is_toxic);

        // Simulate worker sending result
        let test_result = TestResult {
            test_id: 42,
            status: STATUS_PASS,
            duration_ns: 1_000_000,
            message: String::new(),
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
        use crate::protocol::{encode_with_length, TestResult, STATUS_PASS};
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

        spawn_result_collector(zygote_sock, fake_pid, result_tx, is_toxic);

        // Simulate worker sending result
        let test_result = TestResult {
            test_id: 1,
            status: STATUS_PASS,
            duration_ns: 500_000,
            message: String::new(),
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
        assert!(IDLE_WORKERS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_empty());

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
        assert!(IDLE_WORKERS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_empty());
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
}
