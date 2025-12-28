//! Zygote: Fork server with dual-channel IPC

use crate::environment::find_site_packages;
use crate::logcapture::redirect_output;
use crate::protocol::{encode_with_length, TestPayload, TestResult, CMD_EXIT, CMD_FORK, MSG_READY};
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
const TACH_HARNESS_PY: &str = include_str!("tach_harness.py");

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
        *RESET_REGIONS.lock().unwrap() = cached;
        eprintln!("[tach_rust] Cached {} regions for self-reset", count);
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

    let regions = RESET_REGIONS.lock().unwrap();
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

    // Phase 1: Snapshot mode functions
    tach_mod.add_function(wrap_pyfunction!(init_snapshot_mode, &tach_mod)?)?;
    tach_mod.add_function(wrap_pyfunction!(reset_memory, &tach_mod)?)?;

    // Phase 2: Zero-Copy Loader functions (Request Model)
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

/// Zygote with separate command and result channels
pub fn entrypoint(cmd_socket: UnixStream, result_socket: UnixStream) -> Result<()> {
    // DEAD MAN'S SWITCH (Phase 4.2): If supervisor dies, we die
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

    // Phase 8: Detect venv and get site-packages path
    let site_packages = find_site_packages(&cwd);
    if let Some(ref sp) = site_packages {
        eprintln!("[zygote] Found venv: {}", sp.display());
    }

    Python::with_gil(|py| -> Result<()> {
        let sys = py.import("sys")?;
        let path_attr = sys.getattr("path")?;
        let path: &Bound<PyList> = path_attr
            .downcast()
            .map_err(|e| anyhow::anyhow!("sys.path not a list: {}", e))?;

        // Phase 8: Inject venv site-packages FIRST (highest priority)
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
    let mut result_socket = result_socket;
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

                let payload: TestPayload = match bincode::deserialize(&payload_buf) {
                    Ok(p) => p,
                    Err(e) => {
                        eprintln!("[zygote] Deserialize error: {}", e);
                        continue;
                    }
                };

                // Create dedicated socket for worker result
                let (parent_sock, child_sock) = UnixStream::pair()?;
                let result_tx = result_tx.clone();

                match unsafe { fork() } {
                    Ok(ForkResult::Parent { child }) => {
                        drop(child_sock);
                        // Send PID back on command socket
                        cmd_socket.write_all(&child.as_raw().to_le_bytes())?;

                        // Spawn thread to collect this worker's result
                        thread::spawn(move || {
                            let mut socket = parent_sock;
                            let mut result_len_buf = [0u8; 4];

                            if socket.read_exact(&mut result_len_buf).is_ok() {
                                let result_len = u32::from_le_bytes(result_len_buf) as usize;
                                let mut result_buf = vec![0u8; result_len];

                                if socket.read_exact(&mut result_buf).is_ok() {
                                    let mut full = result_len_buf.to_vec();
                                    full.extend(result_buf);
                                    let _ = result_tx.send(full);
                                }
                            }
                        });
                    }
                    Ok(ForkResult::Child) => {
                        drop(parent_sock);

                        // 0. DEAD MAN'S SWITCH (Phase 4.2): If Zygote dies, worker dies
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

                        // 4. Redirect stdout/stderr to memfd
                        if payload.log_fd >= 0 {
                            let _ = redirect_output(payload.log_fd);
                        }

                        // 5. Set debug socket path for breakpoint() support
                        // This enables interactive debugging via TTY proxy
                        if !payload.debug_socket_path.is_empty() {
                            Python::with_gil(|py| -> Result<(), PyErr> {
                                let harness = py.import("tach_harness")?;
                                harness
                                    .getattr("set_debug_socket_path")?
                                    .call1((&payload.debug_socket_path,))?;
                                Ok(())
                            })
                            .ok(); // Non-fatal if this fails
                        }

                        // 6. POST-FORK INIT: Snapshot mode handshake
                        // This performs hygiene (RNG reseed, logging reset) and
                        // initiates snapshot if TACH_SUPERVISOR_SOCK is set.
                        // Worker will SIGSTOP here; Supervisor captures snapshot and SIGCONTs.
                        Python::with_gil(|py| -> Result<(), PyErr> {
                            let harness = py.import("tach_harness")?;
                            harness.getattr("post_fork_init")?.call0()?;
                            Ok(())
                        })
                        .ok(); // Continue even if snapshot fails (graceful degradation)

                        // 7. Run test
                        let result = run_worker(&payload);

                        // 8. Flush and send result (CRITICAL: BEFORE exit decision)
                        // Invariant: Scheduler receives result even if worker exits
                        let _ = std::io::stdout().flush();
                        if let Ok(result_bytes) = encode_with_length(&result) {
                            let _ = child_sock.try_clone().unwrap().write_all(&result_bytes);
                        }

                        // 9. Phase 4: Dual-path decision based on toxicity
                        // TOXIC PATH: Exit immediately (OS cleans up threads, FDs, etc.)
                        // SAFE PATH: Reset memory for future Hypervisor Mode
                        if payload.is_toxic {
                            // Toxic test: exit without reset
                            // This is the Isolation Mode path
                            process::exit(0);
                        } else {
                            // Safe test: reset memory before exit
                            // This validates the reset path works and prepares for
                            // Sub-Stage 4.3 where safe workers will loop instead of exit
                            Python::with_gil(|py| -> Result<(), PyErr> {
                                let tach_rust = py.import("tach_rust")?;
                                tach_rust.getattr("reset_memory")?.call0()?;
                                Ok(())
                            })
                            .ok(); // Non-fatal if reset fails

                            // For now, still exit (Sub-Stage 4.3 will add loop)
                            process::exit(0);
                        }
                    }
                    Err(e) => eprintln!("[zygote] Fork failed: {}", e),
                }
            }
            CMD_EXIT => {
                eprintln!("[zygote] Received EXIT.");
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
    let result = Python::with_gil(|py| -> Result<(u8, f64, String), PyErr> {
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
// Phase 4: Worker Loop Prototype Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Simulates the Phase 4 worker loop decision logic.
    /// This is a pure logic test - no actual processes spawned.
    #[derive(Debug, Clone, PartialEq)]
    enum WorkerAction {
        Reset,  // Safe test: reset memory and continue loop
        Exit,   // Toxic test: exit process
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
            (1, "test_safe_1", false),  // Safe
            (2, "test_safe_2", false),  // Safe
            (3, "test_toxic", true),    // Toxic
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

            // Phase 4 decision point
            let action = decide_worker_action(is_toxic);
            actions.push((test_id, action.clone()));

            // If toxic, break the loop (worker exits)
            if action == WorkerAction::Exit {
                break;
            }
            // If safe, continue loop (worker resets and waits for next)
        }

        // Verify behavior
        assert_eq!(loop_iterations, 3, "Should have processed 3 tests before exit");
        assert_eq!(actions.len(), 3, "Should have 3 action decisions");

        // Verify action sequence
        assert_eq!(actions[0], (1, WorkerAction::Reset), "First test should Reset");
        assert_eq!(actions[1], (2, WorkerAction::Reset), "Second test should Reset");
        assert_eq!(actions[2], (3, WorkerAction::Exit), "Third test should Exit");
    }

    #[test]
    fn test_worker_loop_all_safe() {
        // All safe tests - worker should reset after each
        let payloads = vec![
            (1, false),
            (2, false),
            (3, false),
        ];

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
            (1, true),   // Toxic - should exit
            (2, false),  // Never reached
            (3, false),  // Never reached
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
}
