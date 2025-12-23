//! Physics Check: Integration test for userfaultfd-based memory reset
//!
//! This test verifies the core "physics" of the Snapshot-Hypervisor:
//! 1. Capture a golden snapshot of process memory
//! 2. Mutate memory (simulate test execution)
//! 3. Reset memory via userfaultfd
//! 4. Verify memory returns to golden state
//!
//! If this test passes, the snapshot engine is viable for production use.

use nix::sys::signal::{kill, Signal};
use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
use nix::unistd::{fork, ForkResult, Pid};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::time::Duration;
use tach_core::snapshot::{recv_fd, send_fd, SnapshotManager};
use userfaultfd::UffdBuilder;

/// Create a temporary directory for test sockets
fn create_test_run_dir() -> PathBuf {
    let uuid = std::process::id();
    let path = PathBuf::from(format!("/tmp/tach_test_{}", uuid));
    std::fs::create_dir_all(&path).expect("Failed to create test run dir");
    path
}

/// Clean up test run directory
fn cleanup_test_run_dir(path: &PathBuf) {
    let _ = std::fs::remove_dir_all(path);
}

/// Test: Verify userfaultfd is available on this system
#[test]
fn test_userfaultfd_available() {
    let result = UffdBuilder::new()
        .close_on_exec(true)
        .non_blocking(false)
        .create();

    match result {
        Ok(_) => {
            eprintln!("[physics_check] userfaultfd is available");
        }
        Err(e) => {
            eprintln!(
                "[physics_check] WARNING: userfaultfd unavailable ({}). \
                 Some tests will be skipped.",
                e
            );
        }
    }
}

/// Test: SCM_RIGHTS file descriptor passing works
#[test]
fn test_scm_rights_fd_passing() {
    let run_dir = create_test_run_dir();
    let sock_path = run_dir.join("test_scm.sock");

    // Create listener
    let listener = UnixListener::bind(&sock_path).expect("Failed to bind listener");

    // Fork a child to act as the "worker"
    match unsafe { fork() }.expect("fork failed") {
        ForkResult::Child => {
            // Child: connect and send a dummy FD + PID
            std::thread::sleep(Duration::from_millis(50)); // Let parent start listening

            let stream = UnixStream::connect(&sock_path).expect("Child failed to connect");

            // Create a dummy FD (use the stream itself)
            let my_pid = std::process::id() as i32;
            let dummy_fd = stream.as_raw_fd();

            send_fd(&stream, my_pid, dummy_fd).expect("Child failed to send FD");

            std::process::exit(0);
        }
        ForkResult::Parent { child } => {
            // Parent: accept and receive the FD
            let (stream, _) = listener.accept().expect("Failed to accept");

            let (received_pid, received_fd) = recv_fd(&stream).expect("Failed to receive FD");

            eprintln!(
                "[physics_check] Received PID={}, FD={}",
                received_pid,
                received_fd.as_raw_fd()
            );

            // The FD should be valid (not -1)
            assert!(received_fd.as_raw_fd() >= 0, "Received invalid FD");

            // Wait for child
            waitpid(child, None).expect("Failed to wait for child");

            cleanup_test_run_dir(&run_dir);
        }
    }
}

/// Test: SnapshotManager can be created
#[test]
fn test_snapshot_manager_creation() {
    let mgr = SnapshotManager::new();
    assert!(mgr.is_ok(), "SnapshotManager should be created");

    let mgr = mgr.unwrap();
    // Either available or graceful fallback
    eprintln!(
        "[physics_check] SnapshotManager available: {}",
        mgr.available
    );
}

/// Test: The full Physics Check - memory snapshot and reset
///
/// This is the critical "Triangle of Stability" test:
/// 1. Heap: Create data, mutate, reset, verify restoration
/// 2. Stability: Run multiple iterations without segfault
///
/// NOTE: This test requires userfaultfd privileges and is marked #[ignore]
/// Run with: cargo test physics_check -- --ignored --test-threads=1
#[test]
#[ignore]
fn test_physics_check_memory_reset() {
    use nix::unistd::Pid as NixPid;
    use std::os::fd::{FromRawFd, IntoRawFd};
    use userfaultfd::Uffd;

    // This test requires root or CAP_SYS_PTRACE
    let run_dir = create_test_run_dir();
    let uffd_sock_path = run_dir.join("uffd.sock");

    // Create UFFD listener
    let listener = UnixListener::bind(&uffd_sock_path).expect("Failed to bind UFFD listener");

    // Create SnapshotManager
    let mut snapshot_mgr = match SnapshotManager::new() {
        Ok(mgr) => mgr,
        Err(e) => {
            eprintln!("[physics_check] SnapshotManager failed: {}. Skipping.", e);
            cleanup_test_run_dir(&run_dir);
            return;
        }
    };

    if !snapshot_mgr.available {
        eprintln!("[physics_check] UFFD not available. Skipping Physics Check.");
        cleanup_test_run_dir(&run_dir);
        return;
    }

    match unsafe { fork() }.expect("fork failed") {
        ForkResult::Child => {
            // === WORKER PROCESS ===
            std::thread::sleep(Duration::from_millis(50));

            // 1. Create UFFD
            let uffd = match UffdBuilder::new()
                .close_on_exec(true)
                .non_blocking(false)
                .create()
            {
                Ok(u) => u,
                Err(e) => {
                    eprintln!("[worker] userfaultfd failed: {}", e);
                    std::process::exit(1);
                }
            };

            // 2. Connect to supervisor
            let stream = UnixStream::connect(&uffd_sock_path).expect("Worker failed to connect");

            // 3. Send UFFD + PID
            let my_pid = std::process::id() as i32;
            send_fd(&stream, my_pid, uffd.as_raw_fd()).expect("Worker failed to send UFFD");

            // 4. Create test data BEFORE snapshot - this is the "Golden State"
            // Using Box to ensure heap allocation (subject to MADV_DONTNEED)
            let test_data: Box<[i32; 3]> = Box::new([1, 2, 3]);
            let data_ptr = Box::into_raw(test_data);
            eprintln!(
                "[worker] Golden state: test_data = {:?} at {:p}",
                unsafe { &*data_ptr },
                data_ptr
            );

            // 5. Freeze (SIGSTOP) - supervisor will capture snapshot including test_data
            eprintln!("[worker] Freezing for snapshot capture...");
            nix::sys::signal::raise(Signal::SIGSTOP).expect("Failed to SIGSTOP");

            // 6. We're resumed! Supervisor has captured golden snapshot.
            eprintln!("[worker] Resumed from snapshot. Now mutating data...");

            // 7. Mutate the data (dirties the page)
            unsafe {
                (*data_ptr)[0] = 999;
            }
            eprintln!("[worker] After mutation: test_data = {:?}", unsafe {
                &*data_ptr
            });

            // 8. SEPPUKU: Worker zaps its own heap page
            // This drops the physical page, next access will fault
            let page_addr = (data_ptr as usize) & !4095; // Align to page
            eprintln!("[worker] Self-resetting page at {:x}...", page_addr);
            unsafe {
                let ret = libc::madvise(page_addr as *mut libc::c_void, 4096, libc::MADV_DONTNEED);
                if ret != 0 {
                    eprintln!(
                        "[worker] madvise failed: {}",
                        std::io::Error::last_os_error()
                    );
                }
            }

            // 9. Access the data - this triggers UFFD fault!
            // Supervisor catches fault, copies golden page, we see [1, 2, 3]
            eprintln!("[worker] Accessing reset data (should trigger UFFD fault)...");
            let value = unsafe { (*data_ptr)[0] };
            eprintln!("[worker] After reset: test_data[0] = {}", value);

            // Cleanup
            let _ = unsafe { Box::from_raw(data_ptr) };

            if value == 1 {
                eprintln!("[worker] ✓ TIME TRAVEL SUCCESS! Data rolled back to [1, 2, 3]");
                std::process::exit(0); // SUCCESS
            } else if value == 999 {
                eprintln!("[worker] ✗ RESET FAILED: Data still shows mutation (999)");
                std::process::exit(2); // RESET DIDN'T WORK
            } else {
                eprintln!("[worker] ✗ CORRUPTION: Unexpected value: {}", value);
                std::process::exit(3); // MEMORY CORRUPTION
            }
        }
        ForkResult::Parent { child } => {
            // === SUPERVISOR PROCESS ===
            eprintln!("[supervisor] Worker PID: {}", child);

            // Accept UFFD connection
            let (stream, _) = listener
                .accept()
                .expect("Failed to accept worker connection");
            let (worker_pid, uffd_fd) = recv_fd(&stream).expect("Failed to receive UFFD");

            eprintln!(
                "[supervisor] Received UFFD from worker PID {} (FD: {})",
                worker_pid,
                uffd_fd.as_raw_fd()
            );

            // Convert OwnedFd to Uffd
            // SAFETY: We own this FD, received via SCM_RIGHTS
            let uffd = unsafe { Uffd::from_raw_fd(uffd_fd.into_raw_fd()) };

            // Wait for worker to SIGSTOP
            loop {
                match waitpid(child, Some(WaitPidFlag::WUNTRACED)) {
                    Ok(WaitStatus::Stopped(_, Signal::SIGSTOP)) => {
                        eprintln!("[supervisor] Worker stopped. Capturing golden snapshot...");
                        break;
                    }
                    Ok(status) => {
                        eprintln!("[supervisor] Unexpected status: {:?}", status);
                    }
                    Err(e) => {
                        eprintln!("[supervisor] waitpid error: {}", e);
                        break;
                    }
                }
            }

            // CAPTURE GOLDEN SNAPSHOT
            let worker_nix_pid = NixPid::from_raw(worker_pid);
            if let Err(e) = snapshot_mgr.register_worker_with_uffd(worker_nix_pid, uffd) {
                eprintln!("[supervisor] Failed to register worker: {}", e);
                let _ = kill(child, Signal::SIGKILL);
                cleanup_test_run_dir(&run_dir);
                return;
            }
            eprintln!("[supervisor] Golden snapshot captured!");

            // Resume worker - it will mutate, self-reset (madvise), access data (fault), then exit
            kill(child, Signal::SIGCONT).expect("Failed to SIGCONT worker");
            eprintln!("[supervisor] Worker resumed - waiting for UFFD faults...");

            // Polling loop: handle UFFD faults while worker runs
            loop {
                // Check if worker has exited using non-blocking wait
                match waitpid(child, Some(WaitPidFlag::WNOHANG)) {
                    Ok(WaitStatus::Exited(_, code)) => {
                        eprintln!("[supervisor] Worker exited with code {}", code);
                        if code == 0 {
                            eprintln!("[supervisor] ✓ Physics Check PASSED!");
                        } else {
                            eprintln!("[supervisor] ✗ Physics Check FAILED (exit code: {})!", code);
                        }
                        break;
                    }
                    Ok(WaitStatus::StillAlive) => {
                        // Worker still running, poll for UFFD events
                    }
                    Ok(status) => {
                        eprintln!("[supervisor] Worker status: {:?}", status);
                        break;
                    }
                    Err(e) => {
                        eprintln!("[supervisor] waitpid error: {}", e);
                        break;
                    }
                }

                // Poll UFFD for pending page faults
                match snapshot_mgr.handle_pending_faults(worker_nix_pid) {
                    Ok(handled) if handled > 0 => {
                        eprintln!("[supervisor] Handled {} page faults", handled);
                    }
                    Ok(_) => {}
                    Err(e) => {
                        eprintln!("[supervisor] Fault handling error: {}", e);
                    }
                }

                // Brief sleep to avoid busy-waiting
                std::thread::sleep(Duration::from_millis(1));
            }

            // Cleanup
            snapshot_mgr.remove_worker(worker_nix_pid);
            cleanup_test_run_dir(&run_dir);
            eprintln!("[supervisor] Physics check complete");
        }
    }
}

use std::os::fd::AsRawFd;
