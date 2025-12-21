//! Zygote: Fork server with dual-channel IPC

use crate::logcapture::redirect_output;
use crate::protocol::{encode_with_length, TestPayload, TestResult, CMD_EXIT, CMD_FORK, MSG_READY};
use anyhow::Result;
use nix::sys::signal::{signal, SigHandler, Signal};
use nix::unistd::{fork, ForkResult};
use pyo3::prelude::*;
use pyo3::types::PyList;
use std::env;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::process;
use std::sync::mpsc;
use std::thread;
use std::time::Instant;

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

                        // Redirect stdout/stderr
                        if payload.log_fd >= 0 {
                            let _ = redirect_output(payload.log_fd);
                        }

                        // Run test
                        let result = run_worker(&payload);

                        // Flush and send result
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
    let start = Instant::now();

    println!(
        "Running {} with fixtures {:?}",
        payload.test_name,
        payload.fixtures.iter().map(|f| &f.name).collect::<Vec<_>>()
    );

    // SLEEP TEST
    if payload.test_name.contains("sleep") {
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
    // STRESS TEST
    else if payload.test_name.contains("stress") || payload.test_name.contains("big") {
        for i in 0..20 {
            println!("STRESS LINE {}: {}", i, "X".repeat(100_000));
        }
    }
    // CRASH TEST
    else if payload.test_name.contains("crash") {
        unsafe {
            libc::abort();
        }
    }
    // Normal work
    else {
        std::thread::sleep(std::time::Duration::from_millis(
            5 + (payload.test_id % 10) as u64,
        ));
    }

    let duration_ns = start.elapsed().as_nanos() as u64;

    if payload.test_name.contains("fail") {
        TestResult::fail(
            payload.test_id,
            duration_ns,
            "Simulated failure".to_string(),
        )
    } else {
        TestResult::pass(payload.test_id, duration_ns)
    }
}
