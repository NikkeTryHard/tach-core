use anyhow::Result;
use nix::sys::signal::{signal, SigHandler, Signal};
use nix::unistd::{fork, ForkResult};
use pyo3::prelude::*;
use pyo3::types::PyList;
use std::env;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::process;
use std::thread;
use std::time::Duration;

const CMD_EXIT: u8 = 0x00;
const CMD_FORK: u8 = 0x01;
const MSG_READY: u8 = 0x42;

pub fn entrypoint(mut socket: UnixStream) -> Result<()> {
    // Prevent zombies
    unsafe { signal(Signal::SIGCHLD, SigHandler::SigIgn) }?;

    println!("[zygote] Initializing Python...");
    let cwd = env::current_dir()?.to_string_lossy().to_string();

    Python::with_gil(|py| -> Result<()> {
        let sys = py.import("sys")?;
        let path_attr = sys.getattr("path")?;
        let path: &Bound<PyList> = path_attr
            .downcast()
            .map_err(|e| anyhow::anyhow!("sys.path not a list: {}", e))?;
        path.insert(0, &cwd)?;

        // HEAVY ALLOCATION (100MB)
        println!("[zygote] Allocating 100MB dummy data...");
        let code = std::ffi::CString::new("global data; data = b'x' * 100 * 1024 * 1024")?;
        py.run(
            std::ffi::CStr::from_bytes_with_nul(code.as_bytes_with_nul())?,
            None,
            None,
        )?;

        py.import("pytest")?;
        Ok(())
    })?;

    println!("[zygote] Python warmed up. Sending READY.");
    socket.write_all(&[MSG_READY])?;

    // Command loop
    let mut buf = [0u8; 1];
    loop {
        if socket.read(&mut buf).is_err() {
            break;
        }
        match buf[0] {
            CMD_FORK => {
                match unsafe { fork() } {
                    Ok(ForkResult::Parent { child }) => {
                        let pid_bytes = child.as_raw().to_le_bytes();
                        socket.write_all(&pid_bytes)?;
                    }
                    Ok(ForkResult::Child) => {
                        run_worker_payload();
                        // Sleep to allow measurement
                        thread::sleep(Duration::from_secs(2));
                        process::exit(0);
                    }
                    Err(e) => eprintln!("[zygote] Fork failed: {}", e),
                }
            }
            CMD_EXIT => break,
            _ => eprintln!("[zygote] Unknown command"),
        }
    }
    Ok(())
}

fn run_worker_payload() {
    // Minimal output for clean measurement
    let pid = process::id();
    println!("[worker:{}] Active", pid);
}
