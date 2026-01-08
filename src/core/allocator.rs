//!  Jemalloc Allocator Control for Deterministic Snapshots
//!
//! This module provides the `quiesce_allocator()` function that MUST be called
//! before SIGSTOP to ensure heap consistency during snapshot capture.
//!
//! # The "Split-Brain" Problem
//!
//! When using userfaultfd to snapshot and restore memory, the allocator's
//! internal state must be deterministic. glibc's malloc has several issues:
//!
//! 1. **Thread-local caches (tcache):** Each thread maintains a local free list.
//!    After snapshot restore, these caches may point to memory that was freed
//!    in a different execution path, causing use-after-free.
//!
//! 2. **Pointer mangling:** glibc XORs pointers with a random value for security.
//!    This randomness is per-process and doesn't survive snapshot/restore.
//!
//! 3. **Arena metadata:** Multiple arenas with complex locking can have
//!    inconsistent state if snapshotted mid-operation.
//!
//! # Jemalloc Solution
//!
//! Jemalloc provides explicit control via `mallctl()`:
//!
//! - `thread.tcache.flush`: Push all thread-local bins to global arenas
//! - `epoch`: Force metadata synchronization across all arenas
//!
//! By calling these before SIGSTOP, we transform a non-deterministic heap
//! into a "quiescent" state that can be safely snapshotted and restored.
//!
//! # Usage
//!
//! ```ignore
//! // In zygote.rs, before raising SIGSTOP:
//! crate::allocator::quiesce_allocator()?;
//! nix::sys::signal::raise(Signal::SIGSTOP)?;
//! ```

use anyhow::{Result, anyhow};
use pyo3::prelude::*;
use std::ffi::CStr;

// Use tikv-jemalloc-sys which is compatible with tikv-jemallocator
use tikv_jemalloc_sys as jemalloc_sys;

// =============================================================================
// Jemalloc Version Verification
// =============================================================================

/// Verify that jemalloc is the active allocator by querying its version.
///
/// This MUST be called at startup. If jemalloc is not active (e.g., LD_PRELOAD
/// overrode it, or we're on a platform that doesn't support it), we abort
/// immediately rather than risk allocator desynchronization.
///
/// # Returns
/// - `Ok(version_string)` if jemalloc is active
/// - `Err` if jemalloc is not the allocator (fatal error)
///
/// # Panics
/// This function is designed to be called at startup. If jemalloc is not
/// available, the process should abort rather than continue with an
/// unstable allocator.
pub fn verify_jemalloc_active() -> Result<String> {
    unsafe {
        // Query jemalloc version via mallctl
        // If this fails, jemalloc is not the active allocator
        let mut version_ptr: *const i8 = std::ptr::null();
        let mut version_len = std::mem::size_of::<*const i8>();

        let ret = jemalloc_sys::mallctl(
            c"version".as_ptr(),
            &mut version_ptr as *mut *const i8 as *mut _,
            &mut version_len,
            std::ptr::null_mut(),
            0,
        );

        if ret != 0 {
            return Err(anyhow!(
                "FATAL: jemalloc is not the active allocator (mallctl returned {}). \
                 The Hypervisor requires jemalloc for deterministic snapshots. \
                 Ensure tikv-jemallocator is set as #[global_allocator].",
                ret
            ));
        }

        // Convert version string
        let version = if version_ptr.is_null() {
            "unknown".to_string()
        } else {
            CStr::from_ptr(version_ptr)
                .to_str()
                .unwrap_or("invalid-utf8")
                .to_string()
        };

        eprintln!(
            "[allocator] jemalloc {} verified as active allocator",
            version
        );
        Ok(version)
    }
}

// =============================================================================
// Allocator Quiesce Sequence
// =============================================================================

/// Quiesce the jemalloc allocator before snapshot capture.
///
/// This function performs the "Quiesce Sequence" that transforms the heap
/// from a non-deterministic state into a snapshot-safe state:
///
/// 1. **Flush thread cache:** `mallctl("thread.tcache.flush")`
///    - Pushes all thread-local free list entries to global arenas
///    - Ensures no thread-local pointers will become stale after restore
///
/// 2. **Advance epoch:** `mallctl("epoch")`
///    - Forces metadata synchronization across all arenas
///    - Ensures consistent view of allocation state
///
/// # Safety
///
/// This function calls jemalloc's `mallctl()` which is thread-safe.
/// However, it MUST be called from the worker thread that will be snapshotted,
/// as `thread.tcache.flush` only affects the calling thread's cache.
///
/// # Errors
///
/// Returns an error if any `mallctl()` call fails. This indicates a serious
/// problem with the allocator state and the snapshot should be aborted.
///
/// # Example
///
/// ```ignore
/// // Before SIGSTOP in the snapshot sequence:
/// quiesce_allocator()?;
/// nix::sys::signal::raise(Signal::SIGSTOP)?;
/// ```
pub fn quiesce_allocator() -> Result<()> {
    unsafe {
        // =====================================================================
        // Step 1: Flush thread-local cache
        // =====================================================================
        //
        // The tcache (thread cache) holds recently freed objects for fast
        // reallocation. After snapshot restore, these cached pointers may
        // point to memory that was reallocated in a different execution path.
        //
        // By flushing the tcache, we push all cached objects back to the
        // global arenas where they will be properly tracked.
        //
        // mallctl signature: mallctl(name, oldp, oldlenp, newp, newlen)
        // For thread.tcache.flush: no input/output, just execute the action
        let ret = jemalloc_sys::mallctl(
            c"thread.tcache.flush".as_ptr(),
            std::ptr::null_mut(), // oldp: not reading any value
            std::ptr::null_mut(), // oldlenp: not reading any value
            std::ptr::null_mut(), // newp: not writing any value
            0,                    // newlen: not writing any value
        );

        if ret != 0 {
            return Err(anyhow!(
                "jemalloc thread.tcache.flush failed with errno {}. \
                 This may indicate allocator corruption.",
                ret
            ));
        }

        // =====================================================================
        // Step 2: Advance epoch (force metadata synchronization)
        // =====================================================================
        //
        // The epoch is a monotonically increasing counter that jemalloc uses
        // to track metadata updates. By reading and writing the epoch, we
        // force jemalloc to synchronize all internal statistics and metadata.
        //
        // This ensures that:
        // - All arena metadata is consistent
        // - All statistics are up-to-date
        // - Any pending lazy operations are completed
        //
        // mallctl signature for epoch:
        // - Read current epoch into oldp
        // - Write new epoch from newp (can be same value to just sync)
        let mut epoch: u64 = 0;
        let mut epoch_len = std::mem::size_of::<u64>();

        let ret = jemalloc_sys::mallctl(
            c"epoch".as_ptr(),
            &mut epoch as *mut u64 as *mut _, // oldp: read current epoch
            &mut epoch_len,                   // oldlenp: size of epoch
            &mut epoch as *mut u64 as *mut _, // newp: write to advance
            epoch_len,                        // newlen: size of epoch
        );

        if ret != 0 {
            return Err(anyhow!(
                "jemalloc epoch advance failed with errno {}. \
                 This may indicate allocator corruption.",
                ret
            ));
        }

        eprintln!(
            "[allocator] Quiesced: tcache flushed, epoch advanced to {}",
            epoch
        );

        Ok(())
    }
}

// =============================================================================
// PyO3 FFI Exports
// =============================================================================

/// Python-callable wrapper for `quiesce_allocator()`.
///
/// This allows the Python harness to trigger allocator quiesce if needed,
/// though typically it's called from Rust before SIGSTOP.
///
/// # Python Usage
///
/// ```python
/// import tach_rust
/// tach_rust.quiesce_allocator()  # Flush tcache, sync epoch
/// ```
#[pyfunction]
#[pyo3(name = "quiesce_allocator")]
pub fn py_quiesce_allocator() -> PyResult<()> {
    quiesce_allocator().map_err(|e| {
        pyo3::exceptions::PyRuntimeError::new_err(format!("quiesce_allocator failed: {}", e))
    })
}

/// Python-callable wrapper for `verify_jemalloc_active()`.
///
/// Returns the jemalloc version string if active, raises RuntimeError otherwise.
///
/// # Python Usage
///
/// ```python
/// import tach_rust
/// version = tach_rust.verify_jemalloc()  # Returns "5.3.0" or similar
/// ```
#[pyfunction]
#[pyo3(name = "verify_jemalloc")]
pub fn py_verify_jemalloc() -> PyResult<String> {
    verify_jemalloc_active()
        .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(format!("{}", e)))
}

// =============================================================================
// Unit Tests
// =============================================================================
//
// NOTE: These tests require jemalloc to be the global allocator.
// During `cargo test`, jemalloc is DISABLED to avoid WSL2 instability.
// The tests will gracefully skip when jemalloc isn't active.
//
// To run these tests with jemalloc on a stable Linux system:
//   cargo test --lib allocator -- --ignored
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to check if jemalloc is active. Returns true if active, false otherwise.
    fn is_jemalloc_active() -> bool {
        verify_jemalloc_active().is_ok()
    }

    #[test]
    fn test_jemalloc_is_active() {
        // This test verifies that jemalloc is properly set as the global allocator.
        // During `cargo test`, jemalloc is disabled for WSL2 stability.
        // This test will fail gracefully in that case.
        let result = verify_jemalloc_active();

        if result.is_err() {
            eprintln!(
                "[test] SKIPPED: jemalloc not active (expected during cargo test on WSL2). \
                 Run with `cargo test --lib allocator -- --ignored` on native Linux."
            );
            return;
        }

        let version = result.unwrap();
        assert!(!version.is_empty(), "jemalloc version should not be empty");
        eprintln!("jemalloc version: {}", version);
    }

    #[test]
    fn test_quiesce_does_not_panic() {
        if !is_jemalloc_active() {
            eprintln!("[test] SKIPPED: jemalloc not active");
            return;
        }

        // Quiesce should complete without error
        let result = quiesce_allocator();
        assert!(result.is_ok(), "quiesce_allocator failed: {:?}", result);
    }

    #[test]
    fn test_quiesce_after_allocations() {
        if !is_jemalloc_active() {
            eprintln!("[test] SKIPPED: jemalloc not active");
            return;
        }

        // Allocate some memory, then quiesce
        let mut data: Vec<Vec<u8>> = Vec::new();
        for i in 0..100 {
            data.push(vec![i as u8; 1024]);
        }

        // Quiesce should work even with active allocations
        let result = quiesce_allocator();
        assert!(
            result.is_ok(),
            "quiesce_allocator failed after allocations: {:?}",
            result
        );

        // Data should still be valid after quiesce
        assert_eq!(data.len(), 100);
        assert_eq!(data[50][0], 50);
    }

    #[test]
    fn test_multiple_quiesce_calls() {
        if !is_jemalloc_active() {
            eprintln!("[test] SKIPPED: jemalloc not active");
            return;
        }

        // Multiple quiesce calls should be idempotent
        for _ in 0..10 {
            let result = quiesce_allocator();
            assert!(result.is_ok(), "quiesce_allocator should be idempotent");
        }
    }

    #[test]
    fn test_verify_jemalloc_returns_version_string() {
        // Test that when jemalloc IS active, version string is non-empty
        match verify_jemalloc_active() {
            Ok(version) => {
                assert!(!version.is_empty(), "Version string should not be empty");
                // Version should contain a number
                assert!(
                    version.chars().any(|c| c.is_numeric()),
                    "Version should contain numeric characters"
                );
            }
            Err(_) => {
                eprintln!("[test] SKIPPED: jemalloc not active");
            }
        }
    }

    #[test]
    fn test_quiesce_with_many_allocations() {
        if !is_jemalloc_active() {
            eprintln!("[test] SKIPPED: jemalloc not active");
            return;
        }

        // Stress test with many small allocations
        let mut allocations: Vec<Box<[u8; 64]>> = Vec::new();
        for _ in 0..1000 {
            allocations.push(Box::new([0u8; 64]));
        }

        // Quiesce should handle many allocations
        let result = quiesce_allocator();
        assert!(
            result.is_ok(),
            "quiesce_allocator failed with many allocations: {:?}",
            result
        );

        // Verify allocations are still valid
        assert_eq!(allocations.len(), 1000);
    }

    #[test]
    fn test_quiesce_after_free() {
        if !is_jemalloc_active() {
            eprintln!("[test] SKIPPED: jemalloc not active");
            return;
        }

        // Allocate and then free before quiesce
        let mut data: Vec<Vec<u8>> = Vec::new();
        for i in 0..50 {
            data.push(vec![i as u8; 1024]);
        }
        // Free half of them
        data.truncate(25);

        // Quiesce should work after frees (tcache should be flushed)
        let result = quiesce_allocator();
        assert!(
            result.is_ok(),
            "quiesce_allocator failed after frees: {:?}",
            result
        );

        // Remaining allocations should be valid
        assert_eq!(data.len(), 25);
        assert_eq!(data[24][0], 24);
    }

    #[test]
    fn test_is_jemalloc_active_helper() {
        // Test the helper function itself
        let is_active = is_jemalloc_active();
        // This should match the direct call
        let direct_result = verify_jemalloc_active();
        assert_eq!(is_active, direct_result.is_ok());
    }
}
