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

            // Wait for READY signal
            let mut ready_buf = [0u8; 1];
            supervisor_sock.read_exact(&mut ready_buf)?;
            if ready_buf[0] == 0x42 {
                println!("[supervisor] Zygote is READY.");
            }

            // Spawn 10 workers
            let mut worker_pids = Vec::new();
            println!("[supervisor] Spawning 10 workers...");

            for _ in 0..10 {
                supervisor_sock.write_all(&[0x01])?;
                let mut pid_buf = [0u8; 4];
                supervisor_sock.read_exact(&mut pid_buf)?;
                worker_pids.push(i32::from_le_bytes(pid_buf));
            }

            // Measure memory
            println!("[supervisor] Measuring memory usage...");
            thread::sleep(Duration::from_millis(500));

            let mut total_rss_kb = 0u64;
            let mut total_pss_kb = 0u64;

            for pid in &worker_pids {
                if let Ok((rss, pss)) = read_memory_stats(*pid) {
                    total_rss_kb += rss;
                    total_pss_kb += pss;
                }
            }

            println!("\n--- MEMORY REPORT ---");
            println!("Workers: 10");
            println!("Zygote Payload: 100 MB");
            println!("Total RSS (Virtual): {} MB", total_rss_kb / 1024);
            println!("Total PSS (Physical): {} MB", total_pss_kb / 1024);

            if total_rss_kb > 0 {
                let savings = 1.0 - (total_pss_kb as f64 / total_rss_kb as f64);
                println!("Deduplication Ratio: {:.2}%", savings * 100.0);

                if savings > 0.8 {
                    println!("PASS: Copy-on-Write is working.");
                } else {
                    println!("FAIL: Memory is being duplicated.");
                }
            }

            // Cleanup
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
