mod discovery;
mod zygote;

use anyhow::Result;
use nix::sys::wait::waitpid;
use nix::unistd::{fork, ForkResult};
use std::fs;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::thread;
use std::time::Duration;

fn main() -> Result<()> {
    // --- DISCOVERY PHASE ---
    let cwd = std::env::current_dir()?;
    println!("[supervisor] Scanning {}...", cwd.display());

    let discovery = discovery::scan_project(&cwd)?;
    println!(
        "[supervisor] Found {} tests and {} fixtures.",
        discovery.tests.len(),
        discovery.fixtures.len()
    );

    if !discovery.tests.is_empty() {
        println!("[supervisor] Example: {:?}", discovery.tests[0]);
    }
    // ------------------------

    let (mut supervisor_sock, zygote_sock) = UnixStream::pair()?;
    println!("[supervisor] Forking Zygote...");

    match unsafe { fork() }? {
        ForkResult::Child => {
            drop(supervisor_sock);
            if let Err(e) = zygote::entrypoint(zygote_sock) {
                eprintln!("[zygote] Error: {:?}", e);
                std::process::exit(1);
            }
            std::process::exit(0);
        }
        ForkResult::Parent { child: zygote_pid } => {
            drop(zygote_sock);
            println!("[supervisor] Zygote PID: {}", zygote_pid);

            let mut ready_buf = [0u8; 1];
            supervisor_sock.read_exact(&mut ready_buf)?;
            if ready_buf[0] == 0x42 {
                println!("[supervisor] Zygote is READY.");
            }

            // Spawn 3 workers for quick test
            for i in 1..=3 {
                supervisor_sock.write_all(&[0x01])?;
                let mut pid_buf = [0u8; 4];
                supervisor_sock.read_exact(&mut pid_buf)?;
                println!(
                    "[supervisor] Worker #{} spawned: {}",
                    i,
                    i32::from_le_bytes(pid_buf)
                );
            }

            thread::sleep(Duration::from_millis(100));
            supervisor_sock.write_all(&[0x00])?;
            waitpid(zygote_pid, None)?;
            println!("[supervisor] Done.");
        }
    }
    Ok(())
}

fn read_memory_stats(pid: i32) -> Result<(u64, u64)> {
    let content = fs::read_to_string(format!("/proc/{}/smaps_rollup", pid))?;
    let mut rss = 0u64;
    let mut pss = 0u64;

    for line in content.lines() {
        if line.starts_with("Rss:") {
            if let Some(val) = line.split_whitespace().nth(1) {
                rss = val.parse().unwrap_or(0);
            }
        } else if line.starts_with("Pss:") {
            if let Some(val) = line.split_whitespace().nth(1) {
                pss = val.parse().unwrap_or(0);
            }
        }
    }
    Ok((rss, pss))
}
