//! Integration tests for Landlock V4 network isolation
//!
//! Uses the "suicide worker" pattern: fork a child, apply sandbox,
//! attempt forbidden operation, verify it fails with expected error.

use std::net::TcpListener;

/// Test that TCP bind is blocked when not in allowed ports
#[test]
fn test_landlock_network_blocks_bind() {
    // Skip if kernel doesn't support Landlock V4
    if !tach_core::isolation::sandbox::supports_landlock_network() {
        eprintln!("Skipping: kernel doesn't support Landlock V4 network");
        return;
    }

    let status = unsafe { libc::fork() };

    match status {
        -1 => panic!("fork failed"),
        0 => {
            // Child: apply Landlock network restrictions and try to bind
            use tach_core::core::config::NetworkConfig;
            use tach_core::isolation::sandbox::apply_landlock_network;

            let config = NetworkConfig {
                allow_localhost: Some(false),
                allow_bind_ports: Some(vec![]), // No ports allowed
                allow_connect: None,
            };

            apply_landlock_network(&config).expect("Failed to apply Landlock");

            match TcpListener::bind("127.0.0.1:9999") {
                Ok(_) => std::process::exit(1), // FAIL: bind should have been blocked
                Err(e) => {
                    if e.raw_os_error() == Some(libc::EACCES) {
                        std::process::exit(0); // SUCCESS
                    } else {
                        eprintln!("Unexpected error: {:?}", e);
                        std::process::exit(2);
                    }
                }
            }
        }
        pid => {
            let mut status: i32 = 0;
            unsafe { libc::waitpid(pid, &mut status, 0) };

            if libc::WIFEXITED(status) {
                let exit_code = libc::WEXITSTATUS(status);
                assert_eq!(
                    exit_code, 0,
                    "Child should exit with 0 (bind correctly blocked)"
                );
            } else {
                panic!("Child did not exit normally");
            }
        }
    }
}

/// Test that allowed ports work correctly
#[test]
fn test_landlock_network_allows_configured_ports() {
    if !tach_core::isolation::sandbox::supports_landlock_network() {
        eprintln!("Skipping: kernel doesn't support Landlock V4 network");
        return;
    }

    let status = unsafe { libc::fork() };

    match status {
        -1 => panic!("fork failed"),
        0 => {
            use tach_core::core::config::NetworkConfig;
            use tach_core::isolation::sandbox::apply_landlock_network;

            let config = NetworkConfig {
                allow_localhost: Some(true),
                allow_bind_ports: Some(vec![0, 19999]),
                allow_connect: None,
            };

            apply_landlock_network(&config).expect("Failed to apply Landlock");

            match TcpListener::bind("127.0.0.1:19999") {
                Ok(_) => std::process::exit(0), // SUCCESS
                Err(e) => {
                    eprintln!("Bind failed: {:?}", e);
                    std::process::exit(1);
                }
            }
        }
        pid => {
            let mut status: i32 = 0;
            unsafe { libc::waitpid(pid, &mut status, 0) };

            if libc::WIFEXITED(status) {
                let exit_code = libc::WEXITSTATUS(status);
                assert_eq!(exit_code, 0, "Child should exit with 0 (bind allowed)");
            } else {
                panic!("Child did not exit normally");
            }
        }
    }
}

/// Test graceful fallback on unsupported kernels
#[test]
fn test_landlock_network_graceful_fallback() {
    use tach_core::core::config::NetworkConfig;
    use tach_core::isolation::sandbox::{NetworkIsolationStatus, apply_landlock_network};

    let config = NetworkConfig::default();
    let status = apply_landlock_network(&config);

    assert!(status.is_ok());

    let status = status.unwrap();
    match status {
        NetworkIsolationStatus::LandlockV4 => {
            println!("Kernel supports Landlock V4 network");
        }
        NetworkIsolationStatus::SeccompOnly => {
            println!("Kernel < 6.7, using Seccomp fallback");
        }
        _ => {}
    }
}
