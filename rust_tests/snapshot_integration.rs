//! Snapshot Integration Tests
//!
//! Dedicated tests for the UFFD-based snapshot/reset mechanism.
//! These tests verify the SnapshotManager without the full worker lifecycle.

use nix::unistd::Pid;
use tach_core::snapshot::{parse_memory_maps, MemoryRegion, SnapshotManager};

/// Test: SnapshotManager can be constructed
#[test]
fn test_snapshot_manager_construction() {
    let result = SnapshotManager::new();
    assert!(result.is_ok(), "SnapshotManager should construct");

    let mgr = result.unwrap();
    eprintln!("[snapshot_integration] UFFD available: {}", mgr.available);
}

/// Test: SnapshotManager starts with no workers
#[test]
fn test_snapshot_manager_empty() {
    let mgr = SnapshotManager::new().unwrap();
    assert!(mgr.worker_pids().is_empty());
}

/// Test: Can parse own process memory maps
#[test]
fn test_parse_own_memory_maps() {
    let pid = Pid::from_raw(std::process::id() as i32);
    let regions = parse_memory_maps(pid).expect("Should parse own maps");

    // Verify we get regions
    assert!(!regions.is_empty(), "Should find memory regions");

    // Verify stack exists
    let has_stack = regions.iter().any(|r| r.is_stack());
    assert!(has_stack, "Should find stack region");

    // Count snapshotable regions
    let snapshotable: Vec<_> = regions.iter().filter(|r| r.should_snapshot()).collect();
    eprintln!(
        "[snapshot_integration] Found {} snapshotable regions out of {}",
        snapshotable.len(),
        regions.len()
    );
    assert!(!snapshotable.is_empty(), "Should have snapshotable regions");
}

/// Test: Memory region filtering logic
#[test]
fn test_memory_region_should_snapshot() {
    // Heap should be snapshotted
    let heap = MemoryRegion {
        start: 0x1000,
        end: 0x2000,
        len: 0x1000,
        perms: "rw-p".to_string(),
        name: "[heap]".to_string(),
    };
    assert!(heap.should_snapshot());

    // Read-only should not be snapshotted
    let readonly = MemoryRegion {
        start: 0x3000,
        end: 0x4000,
        len: 0x1000,
        perms: "r--p".to_string(),
        name: "/lib/libc.so".to_string(),
    };
    assert!(!readonly.should_snapshot());
}

/// Test: Stack detection
#[test]
fn test_memory_region_is_stack() {
    let stack = MemoryRegion {
        start: 0x5000,
        end: 0x6000,
        len: 0x1000,
        perms: "rw-p".to_string(),
        name: "[stack]".to_string(),
    };
    assert!(stack.is_stack());

    let heap = MemoryRegion {
        start: 0x7000,
        end: 0x8000,
        len: 0x1000,
        perms: "rw-p".to_string(),
        name: "[heap]".to_string(),
    };
    assert!(!heap.is_stack());
}

/// Test: Get UFFD for non-existent worker returns None
#[test]
fn test_get_worker_uffd_nonexistent() {
    let mgr = SnapshotManager::new().unwrap();
    let fake_pid = Pid::from_raw(99999);
    assert!(mgr.get_worker_uffd(fake_pid).is_none());
}

/// Test: Remove non-existent worker doesn't panic
#[test]
fn test_remove_nonexistent_worker() {
    let mut mgr = SnapshotManager::new().unwrap();
    let fake_pid = Pid::from_raw(99999);
    mgr.remove_worker(fake_pid); // Should not panic
    assert!(mgr.worker_pids().is_empty());
}
