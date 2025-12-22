//! Worker isolation using Linux Namespaces and OverlayFS
//!
//! Each worker gets:
//! - Private /tmp via Copy-on-Write overlay
//! - Private network namespace with its own localhost
//! - READ-ONLY root filesystem (Iron Dome protection)
//! - Writable overlay on project directory

use anyhow::{Context, Result};
use nix::mount::{mount, MsFlags};
use nix::sched::{unshare, CloneFlags};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Set up complete isolation for a worker (Iron Dome)
///
/// CRITICAL SEQUENCE:
/// 1. Unshare namespaces
/// 2. Make mounts private
/// 3. Create mount point dirs (WHILE ROOT IS STILL WRITABLE)
/// 4. Remount root as RO
/// 5. Mount tmpfs (allowed over RO dir)
/// 6. Mount overlays
pub fn setup_filesystem(worker_id: u32, project_root: &Path) -> Result<()> {
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
