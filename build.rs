//! Build script for tach-core
//!
//! Enforces Docker container development environment at compile time.
//! WSL2 causes kernel instability with userfaultfd and jemalloc.

use std::fs;
use std::path::Path;

fn main() {
    // Re-run if these change
    println!("cargo:rerun-if-env-changed=TACH_ALLOW_NATIVE_BUILD");

    // Check if we're allowing native builds (escape hatch for CI)
    if std::env::var("TACH_ALLOW_NATIVE_BUILD").is_ok() {
        println!("cargo:warning=Native build allowed via TACH_ALLOW_NATIVE_BUILD");
        return;
    }

    // Check if we're in a Docker container
    if is_docker_environment() {
        return; // All good
    }

    // Check if we're in WSL2
    if is_wsl2() {
        panic!(
            r#"
========================================
ERROR: WSL2 BUILDS ARE NOT ALLOWED
========================================

WSL2 causes kernel instability with userfaultfd and jemalloc.
You MUST build inside the Docker container.

RECOMMENDED - Use Docker:

  docker compose up -d
  docker compose exec dev bash

BYPASS (NOT RECOMMENDED):

  TACH_ALLOW_NATIVE_BUILD=1 cargo build --release

WARNING: Bypassing may cause kernel crashes, permission errors, and flaky tests.
"#
        );
    }

    // Native Linux - warn but allow (CI systems, etc.)
    println!("cargo:warning=Building outside Docker container. Use 'docker compose exec dev bash' for full feature support.");
}

fn is_docker_environment() -> bool {
    // Method 1: Check for /.dockerenv file
    if Path::new("/.dockerenv").exists() {
        return true;
    }

    // Method 2: Check cgroup for docker/container references
    if let Ok(cgroup) = fs::read_to_string("/proc/1/cgroup")
        && (cgroup.contains("docker") || cgroup.contains("containerd") || cgroup.contains("lxc"))
    {
        return true;
    }

    // Method 3: Check for container environment variable
    if std::env::var("container").is_ok() {
        return true;
    }

    false
}

fn is_wsl2() -> bool {
    if let Ok(version) = fs::read_to_string("/proc/version") {
        return version.to_lowercase().contains("microsoft");
    }
    false
}
