//! Zygote: Fork server with dual-channel IPC

use crate::logcapture::redirect_output;
use crate::protocol::{encode_with_length, TestPayload, TestResult, CMD_EXIT, CMD_FORK, MSG_READY};
use anyhow::Result;
use nix::sys::signal::{signal, SigHandler, Signal};
use nix::unistd::{fork, ForkResult};
use pyo3::ffi::c_str;
use pyo3::prelude::*;
use pyo3::types::{PyList, PyModule};
use std::env;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::process;
use std::sync::mpsc;
use std::thread;
use std::time::Instant;

/// Embedded Python harness for pytest execution
const TACH_HARNESS_PY: &str = include_str!("tach_harness.py");

/// Zygote with separate command and result channels
pub fn entrypoint(cmd_socket: UnixStream, result_socket: UnixStream) -> Result<()> {
    // Prevent zombies
    unsafe { signal(Signal::SIGCHLD, SigHandler::SigIgn) }?;

    eprintln!("[zygote] Initializing Python...");
    let cwd = env::current_dir()?.to_string_lossy().to_string();

    Python::with_gil(|py| -> Result<()> {
        let sys = py.import("sys")?;
        let path_attr = sys.getattr("path")?;
        let path: &Bound<PyList> = path_attr
            .downcast()
            .map_err(|e| anyhow::anyhow!("sys.path not a list: {}", e))?;
        path.insert(0, &cwd)?;
        py.import("pytest")?;

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

        // Load the tach harness module
        // Convert &str to CString for PyModule::from_code
        let harness_code = std::ffi::CString::new(TACH_HARNESS_PY)
            .map_err(|e| anyhow::anyhow!("Failed to create CString: {}", e))?;
        let harness = PyModule::from_code(py, &harness_code, c"tach_harness.py", c"tach_harness")?;
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

                        // 3. Run test
                        let result = run_worker(&payload);

                        // 4. Flush and send result
                        let _ = std::io::stdout().flush();
                        if let Ok(result_bytes) = encode_with_length(&result) {
                            let _ = child_sock.try_clone().unwrap().write_all(&result_bytes);
                        }
                        process::exit(0);
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
