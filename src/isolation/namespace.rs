//! Worker isolation using Linux Namespaces and OverlayFS
//!
//! Each worker gets:
//! - Private /tmp via Copy-on-Write overlay
//! - Private network namespace with its own localhost
//! - READ-ONLY root filesystem (Iron Dome protection)
//! - Writable overlay on project directory

use anyhow::{Context, Result};
use nix::mount::{MsFlags, mount};
use nix::sched::{CloneFlags, unshare};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// overlayfs filesystem magic number from the Linux kernel
const OVERLAYFS_SUPER_MAGIC: i64 = 0x794c7630;

/// Detect if the given path resides on an overlayfs filesystem.
///
/// Used to prevent nested overlay mounts which the Linux kernel does not
/// support — this is the root cause of isolation failures in Docker
/// containers where the storage driver is overlay2.
pub fn is_overlayfs(path: &Path) -> bool {
    let mut buf: libc::statfs = unsafe { std::mem::zeroed() };
    let c_path = match std::ffi::CString::new(path.to_string_lossy().as_bytes()) {
        Ok(p) => p,
        Err(_) => return false,
    };
    let rc = unsafe { libc::statfs(c_path.as_ptr(), &mut buf) };
    if rc != 0 {
        return false;
    }
    buf.f_type == OVERLAYFS_SUPER_MAGIC
}

/// Set up complete isolation for a worker (Iron Dome)
///
/// CRITICAL SEQUENCE:
/// 1. Unshare namespaces
/// 2. Make mounts private
/// 3. Create mount point dirs (WHILE ROOT IS STILL WRITABLE)
/// 4. Remount root as RO
/// 5. Mount tmpfs (allowed over RO dir)
/// 6. Mount overlays
///
/// If TACH_NO_ISOLATION=1 is set, skip all isolation (for benchmarking/debugging)
pub fn setup_filesystem(worker_id: u32, project_root: &Path) -> Result<()> {
    //  Allow skipping isolation for raw speed benchmarks
    if std::env::var("TACH_NO_ISOLATION").unwrap_or_default() == "1" {
        return Ok(());
    }

    let overlay_disabled = is_overlayfs(project_root);
    if overlay_disabled {
        eprintln!(
            "[tach:isolation] Project root is on overlayfs (Docker detected). \
             Overlay mounts disabled — using fork-only isolation."
        );
    }

    // 1. Create new mount AND network namespaces
    unshare(CloneFlags::CLONE_NEWNS | CloneFlags::CLONE_NEWNET)
        .context("unshare(CLONE_NEWNS | CLONE_NEWNET) failed - requires CAP_SYS_ADMIN")?;

    // 2. Make all mounts private (prevent leaking to host)
    mount::<str, str, str, str>(None, "/", None, MsFlags::MS_REC | MsFlags::MS_PRIVATE, None)
        .context("Failed to mark root as MS_PRIVATE")?;

    // 3. Bring up loopback interface
    setup_loopback().context("Failed to configure loopback interface")?;

    // 4. PREPARE MOUNT POINTS (while root is still writable!)
    let base = PathBuf::from(format!("/run/tach/worker_{}", worker_id));
    fs::create_dir_all(&base).context("Failed to create base dir")?;

    // 5. IRON DOME: Lock down root filesystem as READ-ONLY
    // Bind mount / to itself (allows changing mount flags)
    mount::<str, str, str, str>(
        Some("/"),
        "/",
        None,
        MsFlags::MS_BIND | MsFlags::MS_REC,
        None,
    )
    .context("Failed to bind-mount root")?;

    // Remount / as Read-Only
    mount::<str, str, str, str>(
        Some("/"),
        "/",
        None,
        MsFlags::MS_BIND | MsFlags::MS_REMOUNT | MsFlags::MS_RDONLY | MsFlags::MS_REC,
        None,
    )
    .context("Failed to remount root as RO")?;

    // 6. Mount tmpfs on the base directory (allowed: mounting over RO dir)
    mount::<str, PathBuf, str, str>(
        Some("tmpfs"),
        &base,
        Some("tmpfs"),
        MsFlags::empty(),
        Some("size=100M,mode=0755"),
    )
    .context("Failed to mount tmpfs")?;

    // 7. Create subdirs INSIDE the writable tmpfs
    let tmp_upper = base.join("tmp_upper");
    let tmp_work = base.join("tmp_work");
    let proj_upper = base.join("proj_upper");
    let proj_work = base.join("proj_work");
    fs::create_dir_all(&tmp_upper)?;
    fs::create_dir_all(&tmp_work)?;
    fs::create_dir_all(&proj_upper)?;
    fs::create_dir_all(&proj_work)?;

    if !overlay_disabled {
        // 8. Overlay /tmp (writable zone #1)
        let tmp_overlay_opts = format!(
            "lowerdir=/tmp,upperdir={},workdir={}",
            tmp_upper.display(),
            tmp_work.display()
        );

        mount::<str, str, str, str>(
            Some("overlay"),
            "/tmp",
            Some("overlay"),
            MsFlags::empty(),
            Some(&tmp_overlay_opts),
        )
        .context("Failed to mount overlay on /tmp")?;

        // 9. Overlay project root (writable zone #2)
        let proj_overlay_opts = format!(
            "lowerdir={},upperdir={},workdir={}",
            project_root.display(),
            proj_upper.display(),
            proj_work.display()
        );

        mount::<str, Path, str, str>(
            Some("overlay"),
            project_root,
            Some("overlay"),
            MsFlags::empty(),
            Some(&proj_overlay_opts),
        )
        .context("Failed to mount overlay on project root")?;
    } else {
        // Docker/overlayfs: Can't nest overlays. Instead of mounting a
        // bare tmpfs (which would hide existing /tmp contents like the
        // project root when tests live under /tmp), bind-mount /tmp onto
        // itself and remount it writable so workers can use tempfile.
        mount::<str, str, str, str>(Some("/tmp"), "/tmp", None, MsFlags::MS_BIND, None)
            .context("Failed to bind-mount /tmp (Docker fallback)")?;

        mount::<str, str, str, str>(
            Some("/tmp"),
            "/tmp",
            None,
            MsFlags::MS_BIND | MsFlags::MS_REMOUNT,
            None,
        )
        .context("Failed to remount /tmp as writable (Docker fallback)")?;
    }

    Ok(())
}

/// Bring up the loopback interface in the current network namespace
fn setup_loopback() -> Result<()> {
    let output = Command::new("ip")
        .args(["link", "set", "lo", "up"])
        .output()
        .context("Failed to execute 'ip' command - is iproute2 installed?")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("'ip link set lo up' failed: {}", stderr);
    }

    Ok(())
}

// =============================================================================
// Helper functions for testing (exposed for unit tests)
// =============================================================================

/// Calculate base directory path for a worker
/// This is a pure function that can be tested without root privileges.
#[inline]
pub fn worker_base_dir(worker_id: u32) -> PathBuf {
    PathBuf::from(format!("/run/tach/worker_{}", worker_id))
}

/// Generate overlay mount options string for /tmp
/// This is a pure function that can be tested without root privileges.
pub fn tmp_overlay_options(base: &Path) -> String {
    let tmp_upper = base.join("tmp_upper");
    let tmp_work = base.join("tmp_work");
    format!(
        "lowerdir=/tmp,upperdir={},workdir={}",
        tmp_upper.display(),
        tmp_work.display()
    )
}

/// Generate overlay mount options string for project root
/// This is a pure function that can be tested without root privileges.
pub fn project_overlay_options(base: &Path, project_root: &Path) -> String {
    let proj_upper = base.join("proj_upper");
    let proj_work = base.join("proj_work");
    format!(
        "lowerdir={},upperdir={},workdir={}",
        project_root.display(),
        proj_upper.display(),
        proj_work.display()
    )
}

/// Check if isolation is disabled via environment variable
pub fn is_isolation_disabled() -> bool {
    std::env::var("TACH_NO_ISOLATION").unwrap_or_default() == "1"
}

// =============================================================================
// Unit Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    // =========================================================================
    // Worker Base Directory Tests
    // =========================================================================

    #[test]
    fn test_worker_base_dir_format() {
        assert_eq!(worker_base_dir(0), PathBuf::from("/run/tach/worker_0"));
        assert_eq!(worker_base_dir(1), PathBuf::from("/run/tach/worker_1"));
        assert_eq!(worker_base_dir(42), PathBuf::from("/run/tach/worker_42"));
    }

    #[test]
    fn test_worker_base_dir_large_id() {
        // Verify no overflow or formatting issues with large worker IDs
        assert_eq!(
            worker_base_dir(u32::MAX),
            PathBuf::from("/run/tach/worker_4294967295")
        );
    }

    #[test]
    fn test_worker_base_dir_is_absolute() {
        let base = worker_base_dir(123);
        assert!(
            base.is_absolute(),
            "Worker base dir should be absolute path"
        );
        assert!(
            base.starts_with("/run/tach"),
            "Worker base dir should be under /run/tach"
        );
    }

    // =========================================================================
    // Overlay Options Format Tests
    // =========================================================================

    #[test]
    fn test_tmp_overlay_options_format() {
        let base = PathBuf::from("/run/tach/worker_0");
        let opts = tmp_overlay_options(&base);

        // Verify format: lowerdir=/tmp,upperdir=<upper>,workdir=<work>
        assert!(
            opts.starts_with("lowerdir=/tmp,"),
            "Should start with lowerdir=/tmp"
        );
        assert!(
            opts.contains("upperdir=/run/tach/worker_0/tmp_upper"),
            "Should contain upperdir"
        );
        assert!(
            opts.contains("workdir=/run/tach/worker_0/tmp_work"),
            "Should contain workdir"
        );
    }

    #[test]
    fn test_tmp_overlay_options_no_spaces() {
        let base = PathBuf::from("/run/tach/worker_5");
        let opts = tmp_overlay_options(&base);

        // Overlay options should not contain spaces (mount parsing issue)
        assert!(
            !opts.contains(' '),
            "Overlay options should not contain spaces"
        );
    }

    #[test]
    fn test_project_overlay_options_format() {
        let base = PathBuf::from("/run/tach/worker_0");
        let project = PathBuf::from("/home/user/myproject");
        let opts = project_overlay_options(&base, &project);

        // Verify format: lowerdir=<project>,upperdir=<upper>,workdir=<work>
        assert!(
            opts.starts_with("lowerdir=/home/user/myproject,"),
            "Should start with project as lowerdir"
        );
        assert!(
            opts.contains("upperdir=/run/tach/worker_0/proj_upper"),
            "Should contain upperdir"
        );
        assert!(
            opts.contains("workdir=/run/tach/worker_0/proj_work"),
            "Should contain workdir"
        );
    }

    #[test]
    fn test_project_overlay_options_preserves_path() {
        let base = PathBuf::from("/run/tach/worker_1");
        let project = PathBuf::from("/very/deep/nested/path/to/project");
        let opts = project_overlay_options(&base, &project);

        assert!(
            opts.contains("/very/deep/nested/path/to/project"),
            "Should preserve full project path"
        );
    }

    #[test]
    fn test_overlay_options_different_workers() {
        // Verify each worker gets unique paths
        let base0 = worker_base_dir(0);
        let base1 = worker_base_dir(1);
        let project = PathBuf::from("/home/user/proj");

        let opts0 = project_overlay_options(&base0, &project);
        let opts1 = project_overlay_options(&base1, &project);

        assert_ne!(
            opts0, opts1,
            "Different workers should have different overlay paths"
        );
        assert!(
            opts0.contains("worker_0"),
            "Worker 0 opts should reference worker_0"
        );
        assert!(
            opts1.contains("worker_1"),
            "Worker 1 opts should reference worker_1"
        );
    }

    // =========================================================================
    // TACH_NO_ISOLATION Environment Variable Tests
    // =========================================================================

    #[test]
    fn test_isolation_disabled_when_set_to_1() {
        // Save original value
        let original = env::var("TACH_NO_ISOLATION").ok();

        unsafe { env::set_var("TACH_NO_ISOLATION", "1") };
        assert!(
            is_isolation_disabled(),
            "Isolation should be disabled when TACH_NO_ISOLATION=1"
        );

        // Restore
        match original {
            Some(v) => unsafe { env::set_var("TACH_NO_ISOLATION", v) },
            None => unsafe { env::remove_var("TACH_NO_ISOLATION") },
        }
    }

    #[test]
    fn test_isolation_enabled_when_set_to_0() {
        let original = env::var("TACH_NO_ISOLATION").ok();

        unsafe { env::set_var("TACH_NO_ISOLATION", "0") };
        assert!(
            !is_isolation_disabled(),
            "Isolation should be enabled when TACH_NO_ISOLATION=0"
        );

        match original {
            Some(v) => unsafe { env::set_var("TACH_NO_ISOLATION", v) },
            None => unsafe { env::remove_var("TACH_NO_ISOLATION") },
        }
    }

    #[test]
    fn test_isolation_enabled_when_unset() {
        let original = env::var("TACH_NO_ISOLATION").ok();

        unsafe { env::remove_var("TACH_NO_ISOLATION") };
        assert!(
            !is_isolation_disabled(),
            "Isolation should be enabled when TACH_NO_ISOLATION is unset"
        );

        if let Some(v) = original {
            unsafe { env::set_var("TACH_NO_ISOLATION", v) };
        }
    }

    #[test]
    fn test_isolation_enabled_when_set_to_other() {
        let original = env::var("TACH_NO_ISOLATION").ok();

        unsafe { env::set_var("TACH_NO_ISOLATION", "yes") };
        assert!(
            !is_isolation_disabled(),
            "Isolation should be enabled for non-'1' values"
        );

        unsafe { env::set_var("TACH_NO_ISOLATION", "true") };
        assert!(
            !is_isolation_disabled(),
            "Isolation should be enabled for non-'1' values"
        );

        unsafe { env::set_var("TACH_NO_ISOLATION", "") };
        assert!(
            !is_isolation_disabled(),
            "Isolation should be enabled for empty string"
        );

        match original {
            Some(v) => unsafe { env::set_var("TACH_NO_ISOLATION", v) },
            None => unsafe { env::remove_var("TACH_NO_ISOLATION") },
        }
    }

    // =========================================================================
    // setup_filesystem Early Return Tests
    // =========================================================================

    #[test]
    fn test_setup_filesystem_skipped_when_no_isolation() {
        let original = env::var("TACH_NO_ISOLATION").ok();

        unsafe { env::set_var("TACH_NO_ISOLATION", "1") };

        // This should return Ok(()) immediately without requiring root
        let result = setup_filesystem(999, Path::new("/tmp/test"));
        assert!(
            result.is_ok(),
            "setup_filesystem should succeed (early return) when TACH_NO_ISOLATION=1"
        );

        match original {
            Some(v) => unsafe { env::set_var("TACH_NO_ISOLATION", v) },
            None => unsafe { env::remove_var("TACH_NO_ISOLATION") },
        }
    }

    // =========================================================================
    // Path Component Tests
    // =========================================================================

    #[test]
    fn test_overlay_subdirs_are_consistent() {
        let base = worker_base_dir(7);

        // Verify the subdirectory names match what setup_filesystem uses
        assert_eq!(base.join("tmp_upper").file_name().unwrap(), "tmp_upper");
        assert_eq!(base.join("tmp_work").file_name().unwrap(), "tmp_work");
        assert_eq!(base.join("proj_upper").file_name().unwrap(), "proj_upper");
        assert_eq!(base.join("proj_work").file_name().unwrap(), "proj_work");
    }

    #[test]
    fn test_base_dir_parent_exists() {
        let base = worker_base_dir(0);
        let parent = base.parent().unwrap();
        assert_eq!(parent, Path::new("/run/tach"));
    }
}
