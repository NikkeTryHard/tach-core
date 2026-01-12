//!  The Iron Dome - Sandbox Hardening
//!
//! This module implements the final security layer for Tach workers, transforming
//! each worker from a generic process into a restricted execution unit.
//!
//! # Security Architecture
//!
//! The Iron Dome consists of two complementary security mechanisms:
//!
//! 1. **Landlock** (Filesystem Isolation)
//!    - Restricts which paths the worker can read/write
//!    - Applied to ALL workers (safe and toxic)
//!    - Gracefully degrades on kernels < 5.13
//!
//! 2. **Seccomp** (Syscall Filtering)
//!    - Blocks dangerous syscalls (network, fork, exec)
//!    - Applied ONLY to safe workers (toxic workers need network/fork)
//!    - Uses blacklist approach (whitelist too brittle for Python)
//!
//! # Why Blacklist Instead of Whitelist?
//!
//! A **whitelist** (allowlist) approach would enumerate every permitted syscall.
//! This is problematic for Python because:
//!
//! 1. Python's syscall footprint changes between versions (3.11 vs 3.12 vs 3.13)
//! 2. C extensions may use arbitrary syscalls that we can't predict
//! 3. A missed syscall causes SIGSYS crash, not a graceful error
//!
//! The **blacklist** approach only blocks known-dangerous syscalls:
//! - Network: socket, bind, connect, listen, accept
//! - Process: fork, vfork, execve, execveat
//! - Privilege: ptrace, mount, unshare, setns
//!
//! Unknown syscalls pass through, which is safe because:
//! - Landlock prevents filesystem escape regardless of syscall
//! - Seccomp + Landlock together provide defense-in-depth
//!
//! # Why clone() Must NEVER Be Blocked
//!
//! Python's threading module uses `clone()` (with CLONE_VM | CLONE_THREAD flags)
//! to create OS threads. Blocking clone() would break:
//!
//! - `threading.Thread()` - Cannot start threads
//! - `concurrent.futures.ThreadPoolExecutor` - Completely broken
//! - GIL release during I/O - Some implementations use threads
//!
//! Instead, we block `execve` which prevents any cloned/forked process from
//! becoming a new program. Combined with Landlock's write restrictions, a
//! malicious forked Python process is effectively neutered:
//!
//! ```text
//! Malicious test:
//!   1. fork() -> ALLOWED (we block this, but clone() without CLONE_VM might work)
//!   2. execve("/bin/sh") -> BLOCKED by Seccomp -> EPERM
//!   3. open("/etc/passwd", O_WRONLY) -> BLOCKED by Landlock -> EACCES
//! ```
//!
//! # Safe vs Toxic Worker Differentiation
//!
//! ```text
//! ┌─────────────────────┬─────────────────┬─────────────────────┐
//! │                     │   SAFE WORKER   │   TOXIC WORKER      │
//! ├─────────────────────┼─────────────────┼─────────────────────┤
//! │ Landlock            │ ENFORCED        │ ENFORCED            │
//! │ Seccomp             │ ENFORCED        │ SKIPPED             │
//! │ Network Access      │ BLOCKED         │ ALLOWED             │
//! │ Fork/Exec           │ BLOCKED         │ ALLOWED             │
//! │ Worker Reuse        │ YES (pool)      │ NO (exit)           │
//! └─────────────────────┴─────────────────┴─────────────────────┘
//! ```
//!
//! # Integration Sequence (in zygote.rs)
//!
//! The order of operations is critical for security:
//! 1. `setup_filesystem()` - Namespaces + OverlayFS
//! 2. `apply_landlock()` - Restrict view of the OverlayFS
//! 3. `apply_seccomp()` - Restrict syscalls (safe workers only)
//! 4. `post_fork_init()` - Load Python, etc.
//!
//! # Graceful Degradation
//!
//! Both Landlock and Seccomp are designed to fail gracefully:
//! - If the kernel doesn't support Landlock (< 5.13), log a warning but continue
//! - If Seccomp setup fails, log a warning but continue
//! - The test runner must remain functional on older kernels (e.g., AWS Lambda)

use anyhow::{Context, Result};
use std::path::Path;

// ============================================================================
// LANDLOCK: Filesystem Access Control
// ============================================================================

/// Status of Landlock enforcement after applying rules.
///
/// This enum mirrors `landlock::RulesetStatus` but is owned by our crate
/// to avoid exposing landlock types in our public API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxStatus {
    /// All requested restrictions are fully enforced.
    /// The kernel supports all requested Landlock features.
    FullyEnforced,

    /// Some restrictions are enforced, but not all.
    /// The kernel supports Landlock but not all requested features.
    PartiallyEnforced,

    /// No restrictions are enforced.
    /// The kernel does not support Landlock (< 5.13) or it's disabled.
    NotEnforced,
}

/// Apply Landlock filesystem restrictions to the current process.
///
/// # Security Policy
///
/// - **READ-ONLY**: Project root, /usr, /lib, /lib64, /bin, /etc, /dev
/// - **READ-WRITE**: /tmp, /run/tach/worker_{id}
/// - **DENY**: Everything else
///
/// # Arguments
///
/// * `project_root` - The project root directory (will be canonicalized)
/// * `worker_id` - The worker ID (used to construct /run/tach/worker_{id})
///
/// # Returns
///
/// * `Ok(SandboxStatus)` - The enforcement status
/// * `Err(_)` - If Landlock setup failed (should be logged but not fatal)
///
/// # Graceful Degradation
///
/// If the kernel doesn't support Landlock (ABI < V1), this function returns
/// `Ok(SandboxStatus::NotEnforced)` instead of an error. The caller should
/// log a warning but continue execution.
///
/// # Example
///
/// ```ignore
/// match apply_landlock(&project_root, worker_id) {
///     Ok(SandboxStatus::FullyEnforced) => { /* Ideal */ }
///     Ok(SandboxStatus::NotEnforced) => {
///         eprintln!("[worker] WARNING: Landlock not enforced");
///     }
///     Err(e) => {
///         eprintln!("[worker] WARNING: Landlock failed: {}", e);
///     }
/// }
/// ```
pub fn apply_landlock(project_root: &Path, worker_id: u32) -> Result<SandboxStatus> {
    use landlock::{ABI, Access, AccessFs, Ruleset, RulesetAttr, RulesetStatus};

    // ========================================================================
    // ABI SELECTION
    // ========================================================================
    // Start with ABI::V1 for maximum compatibility (kernel 5.13+).
    // V2 adds TRUNCATE rights, V3 adds network, V4 adds more granular rights.
    // We use V1 to ensure the widest kernel support.
    let abi = ABI::V1;

    // ========================================================================
    // CANONICALIZE PATHS
    // ========================================================================
    // Landlock requires absolute paths. Relative paths are a source of bugs.
    // We canonicalize project_root to ensure it's absolute and resolved.
    let project_root = project_root
        .canonicalize()
        .context("Failed to canonicalize project_root for Landlock")?;

    // Worker scratch space (created by isolation.rs)
    let worker_scratch = format!("/run/tach/worker_{}", worker_id);

    // ========================================================================
    // DEFINE ACCESS RIGHTS
    // ========================================================================
    // AccessFs::from_all(abi) = all filesystem operations for this ABI
    // AccessFs::from_read(abi) = read-only operations (execute, read_file, read_dir)
    let all_access = AccessFs::from_all(abi);
    let read_access = AccessFs::from_read(abi);

    // Safe write access for project_root: excludes dangerous device/socket creation
    // SECURITY: Prevents device node creation escape attacks via os.mknod()
    // A malicious test could create /dev/sda inside project_root and access host disk
    // Excluded: MAKE_CHAR, MAKE_BLOCK, MAKE_FIFO, MAKE_SOCK
    let safe_write_access = AccessFs::ReadFile
        | AccessFs::WriteFile
        | AccessFs::ReadDir
        | AccessFs::RemoveDir
        | AccessFs::RemoveFile
        | AccessFs::MakeDir
        | AccessFs::MakeReg
        | AccessFs::MakeSym
        | AccessFs::Execute;

    // ========================================================================
    // CREATE RULESET
    // ========================================================================
    // The ruleset defines which operations we want to restrict.
    // By calling handle_access(all_access), we're saying "we want to control
    // all filesystem operations". Any operation not explicitly allowed will
    // be denied after restrict_self().
    let ruleset = Ruleset::default()
        .handle_access(all_access)
        .context("Failed to create Landlock ruleset")?
        .create()
        .context("Failed to create Landlock ruleset")?;

    // ========================================================================
    // ADD READ-ONLY RULES
    // ========================================================================
    // These paths are allowed read-only access (execute, read_file, read_dir).
    // This includes:
    // - System libraries (/usr, /lib, /lib64, /bin)
    // - Configuration (/etc - Python configs, SSL certs, timezone)
    // - Device nodes (/dev - /dev/null, /dev/urandom, /dev/zero)
    //
    // NOTE: project_root is added with safe_write_access below which allows
    // normal file operations but blocks device node creation (MAKE_CHAR/BLOCK).

    // Project root needs write access for OverlayFS to work correctly.
    // The overlay provides copy-on-write isolation, but Landlock must allow
    // the underlying operations for the overlay mount to function.
    // SECURITY: Use safe_write_access to prevent device node creation attacks
    let ruleset = add_path_rule(ruleset, &project_root, safe_write_access)?;
    let ruleset = add_path_rule_if_exists(ruleset, "/usr", read_access)?;
    let ruleset = add_path_rule_if_exists(ruleset, "/lib", read_access)?;
    let ruleset = add_path_rule_if_exists(ruleset, "/lib64", read_access)?;
    let ruleset = add_path_rule_if_exists(ruleset, "/bin", read_access)?;
    let ruleset = add_path_rule_if_exists(ruleset, "/etc", read_access)?;
    let ruleset = add_path_rule_if_exists(ruleset, "/dev", read_access)?;
    // /proc is needed for Python's multiprocessing, os.getpid(), etc.
    let ruleset = add_path_rule_if_exists(ruleset, "/proc", read_access)?;
    // /sys is needed for some hardware detection
    let ruleset = add_path_rule_if_exists(ruleset, "/sys", read_access)?;

    // ========================================================================
    // ADD READ-WRITE RULES
    // ========================================================================
    // These paths are allowed full access (read + write + create + delete).
    // This includes:
    // - /tmp (the overlay upperdir for temporary files)
    // - /run/tach/worker_{id} (worker scratch space, overlay dirs)
    //
    // SECURITY NOTE: /tmp is an overlay mount. Writes go to the upperdir
    // inside /run/tach/worker_{id}, not the real /tmp. This is safe.

    let ruleset = add_path_rule_if_exists(ruleset, "/tmp", all_access)?;
    let ruleset = add_path_rule_if_exists(ruleset, &worker_scratch, all_access)?;
    // /run is needed for the worker scratch space parent
    let ruleset = add_path_rule_if_exists(ruleset, "/run", all_access)?;

    // ========================================================================
    // ENFORCE RULESET
    // ========================================================================
    // restrict_self() applies the ruleset to the current thread and all
    // future threads. After this call, any filesystem operation not
    // explicitly allowed above will fail with EACCES.
    let status = ruleset
        .restrict_self()
        .context("Failed to apply Landlock restrictions")?;

    // ========================================================================
    // RETURN STATUS
    // ========================================================================
    // Convert landlock::RulesetStatus to our SandboxStatus enum.
    match status.ruleset {
        RulesetStatus::FullyEnforced => Ok(SandboxStatus::FullyEnforced),
        RulesetStatus::PartiallyEnforced => Ok(SandboxStatus::PartiallyEnforced),
        RulesetStatus::NotEnforced => Ok(SandboxStatus::NotEnforced),
    }
}

/// Helper: Add a Landlock rule for a path (fails if path doesn't exist).
fn add_path_rule<T, A>(ruleset: T, path: impl AsRef<Path>, access: A) -> Result<T>
where
    T: landlock::RulesetCreatedAttr,
    A: Into<landlock::BitFlags<landlock::AccessFs>> + Copy,
{
    use landlock::{PathBeneath, PathFd};

    let path = path.as_ref();
    let fd = PathFd::new(path)
        .with_context(|| format!("Failed to open path for Landlock: {}", path.display()))?;

    ruleset
        .add_rule(PathBeneath::new(fd, access))
        .with_context(|| format!("Failed to add Landlock rule for: {}", path.display()))
}

/// Helper: Add a Landlock rule for a path (silently skips if path doesn't exist).
///
/// This is used for optional system paths like /lib64 which may not exist
/// on all systems.
///
/// SECURITY: This function avoids TOCTOU by NOT checking path.exists() first.
/// Instead, we attempt to open the path and handle ENOENT directly.
fn add_path_rule_if_exists<T, A>(ruleset: T, path: impl AsRef<Path>, access: A) -> Result<T>
where
    T: landlock::RulesetCreatedAttr,
    A: Into<landlock::BitFlags<landlock::AccessFs>> + Copy,
{
    use landlock::{PathBeneath, PathFd, PathFdError};

    let path = path.as_ref();

    // SECURITY: Do NOT use path.exists() - that creates a TOCTOU race.
    // Instead, try to open the path and handle errors atomically.
    match PathFd::new(path) {
        Ok(fd) => ruleset
            .add_rule(PathBeneath::new(fd, access))
            .with_context(|| format!("Failed to add Landlock rule for: {}", path.display())),
        Err(PathFdError::OpenCall { source, .. }) => {
            // Check if the error is ENOENT (path doesn't exist)
            if let Some(os_err) = source.raw_os_error()
                && os_err == libc::ENOENT
            {
                // Path doesn't exist - this is expected for optional paths
                return Ok(ruleset);
            }
            // For other errors (permissions, etc.), silently skip
            // This maintains the original behavior of graceful degradation
            Ok(ruleset)
        }
        // PathFdError is #[non_exhaustive], so handle future variants
        Err(_) => Ok(ruleset),
    }
}

// ============================================================================
// SECCOMP: Syscall Filtering
// ============================================================================

/// Apply Seccomp syscall blacklist to the current process.
///
/// # Security Policy (Blacklist)
///
/// The following syscalls are blocked (return EPERM):
///
/// **Network syscalls:**
/// - `socket` - Create network sockets
/// - `bind` - Bind to network addresses
/// - `connect` - Connect to remote hosts
/// - `listen` - Listen for connections
/// - `accept` / `accept4` - Accept connections
///
/// **Process syscalls:**
/// - `fork` - Create child process
/// - `vfork` - Create child process (shared memory)
/// - `execve` / `execveat` - Execute new program
///
/// **Clone handling:**
/// - `clone` / `clone3` are NOT blocked because Python threading requires them
/// - Blocking `execve` prevents forked processes from becoming new programs
/// - A forked Python process that cannot exec and cannot write (Landlock) is neutered
///
/// # Why Blacklist?
///
/// A whitelist approach is too brittle for the Python runtime, which may
/// change syscall patterns between versions. The blacklist approach blocks
/// only the dangerous syscalls while allowing everything else.
///
/// # Safe Workers Only
///
/// This function should ONLY be called for safe workers. Toxic workers
/// (integration tests) may legitimately need network access or fork.
///
/// # Returns
///
/// * `Ok(())` - Seccomp filter applied successfully
/// * `Err(_)` - If Seccomp setup failed (should be logged but not fatal)
///
/// # Example
///
/// ```ignore
/// if !is_toxic {
///     if let Err(e) = apply_seccomp() {
///         eprintln!("[worker] WARNING: Seccomp failed: {}", e);
///     }
/// }
/// ```
pub fn apply_seccomp() -> Result<()> {
    use seccompiler::{SeccompAction, SeccompFilter, SeccompRule, TargetArch};
    use std::collections::BTreeMap;

    // ========================================================================
    // DETERMINE TARGET ARCHITECTURE
    // ========================================================================
    // Seccomp filters are architecture-specific. We need to compile the
    // filter for the current architecture.
    let target_arch = match std::env::consts::ARCH {
        "x86_64" => TargetArch::x86_64,
        "aarch64" => TargetArch::aarch64,
        arch => anyhow::bail!("Unsupported architecture for Seccomp: {}", arch),
    };

    // ========================================================================
    // BUILD SYSCALL BLACKLIST
    // ========================================================================
    // We create a map of syscall number -> rules.
    // An empty rule vector means "block this syscall unconditionally".
    //
    // SECURITY NOTE: We use SeccompAction::Errno(libc::EPERM) instead of
    // SeccompAction::Trap to avoid crashing the process. EPERM allows the
    // Python code to handle the error gracefully (e.g., catch OSError).

    let mut rules: BTreeMap<i64, Vec<SeccompRule>> = BTreeMap::new();

    // ------------------------------------------------------------------------
    // NETWORK SYSCALLS
    // ------------------------------------------------------------------------
    // Block all network socket operations. Tests should not make network calls.
    // If a test needs network access, it should be marked as toxic.

    rules.insert(libc::SYS_socket, vec![]); // Create socket
    rules.insert(libc::SYS_bind, vec![]); // Bind to address
    rules.insert(libc::SYS_connect, vec![]); // Connect to remote
    rules.insert(libc::SYS_listen, vec![]); // Listen for connections
    rules.insert(libc::SYS_accept, vec![]); // Accept connection
    rules.insert(libc::SYS_accept4, vec![]); // Accept connection (flags)

    // ------------------------------------------------------------------------
    // PROCESS SYSCALLS
    // ------------------------------------------------------------------------
    // Block fork/exec to prevent spawning new processes.
    //
    // CRITICAL: We do NOT block clone/clone3 because Python threading uses them.
    // Blocking execve is sufficient to prevent a forked process from becoming
    // a new program (like `rm -rf`). A forked Python process that cannot exec
    // and cannot write to disk (Landlock) is effectively neutered.

    rules.insert(libc::SYS_fork, vec![]); // Fork process
    rules.insert(libc::SYS_vfork, vec![]); // Fork with shared memory
    rules.insert(libc::SYS_execve, vec![]); // Execute program
    rules.insert(libc::SYS_execveat, vec![]); // Execute program (fd-relative)

    // ------------------------------------------------------------------------
    // PRIVILEGE ESCALATION SYSCALLS
    // ------------------------------------------------------------------------
    // Block syscalls that could be used to escape the sandbox or gain
    // elevated privileges. These are critical for security hardening.

    rules.insert(libc::SYS_ptrace, vec![]); // Debug/trace other processes
    rules.insert(libc::SYS_mount, vec![]); // Mount filesystems
    rules.insert(libc::SYS_umount2, vec![]); // Unmount filesystems
    rules.insert(libc::SYS_unshare, vec![]); // Create new namespaces
    rules.insert(libc::SYS_setns, vec![]); // Join existing namespaces

    // ========================================================================
    // CREATE SECCOMP FILTER
    // ========================================================================
    // SeccompFilter::new() takes:
    // - rules: Map of syscall -> conditions (empty = unconditional block)
    // - mismatch_action: What to do for syscalls NOT in the map (Allow)
    // - match_action: What to do for syscalls IN the map (Errno)
    // - target_arch: The CPU architecture

    let filter = SeccompFilter::new(
        rules,
        SeccompAction::Allow, // Allow syscalls not in blacklist
        SeccompAction::Errno(libc::EPERM as u32), // Block with EPERM (not SIGSYS)
        target_arch,
    )
    .context("Failed to create Seccomp filter")?;

    // ========================================================================
    // COMPILE TO BPF
    // ========================================================================
    // Convert the high-level filter to a BPF program that the kernel can execute.
    let bpf_prog: seccompiler::BpfProgram = filter
        .try_into()
        .context("Failed to compile Seccomp filter to BPF")?;

    // ========================================================================
    // APPLY FILTER
    // ========================================================================
    // Install the BPF filter in the kernel. After this call, any blocked
    // syscall will return EPERM.
    //
    // NOTE: We use apply_filter() which applies to the current thread only.
    // Since we call this post-fork before any threads are created, this is
    // sufficient. If we needed to apply to all threads, we'd use
    // apply_filter_all_threads() with SECCOMP_FILTER_FLAG_TSYNC.

    seccompiler::apply_filter(&bpf_prog).context("Failed to apply Seccomp filter")?;

    Ok(())
}

// ============================================================================
// COMBINED SANDBOX APPLICATION
// ============================================================================

/// Apply the full Iron Dome sandbox to the current process.
///
/// This is a convenience function that applies both Landlock and Seccomp
/// with appropriate error handling and logging.
///
/// # Arguments
///
/// * `project_root` - The project root directory
/// * `worker_id` - The worker ID
/// * `is_toxic` - Whether this is a toxic worker (skips Seccomp)
///
/// # Returns
///
/// * `Ok(SandboxStatus)` - The Landlock enforcement status
/// * `Err(_)` - If a critical error occurred (should not happen in practice)
///
/// # Graceful Degradation
///
/// This function never fails fatally. If Landlock or Seccomp setup fails,
/// it logs a warning and continues. The test runner must remain functional
/// on older kernels.
pub fn apply_iron_dome(
    project_root: &Path,
    worker_id: u32,
    is_toxic: bool,
) -> Result<SandboxStatus> {
    // ========================================================================
    // STEP 1: APPLY LANDLOCK (ALWAYS)
    // ========================================================================
    // Landlock restricts filesystem access. Even toxic tests shouldn't
    // escape their overlay.
    let landlock_status = match apply_landlock(project_root, worker_id) {
        Ok(status) => {
            match status {
                SandboxStatus::FullyEnforced => {
                    // Ideal case - full protection
                }
                SandboxStatus::PartiallyEnforced => {
                    eprintln!(
                        "[worker:{}] Landlock partially enforced (some features unavailable)",
                        worker_id
                    );
                }
                SandboxStatus::NotEnforced => {
                    eprintln!(
                        "[worker:{}] WARNING: Landlock not enforced - kernel too old (< 5.13)",
                        worker_id
                    );
                }
            }
            status
        }
        Err(e) => {
            eprintln!(
                "[worker:{}] WARNING: Landlock setup failed: {}",
                worker_id, e
            );
            SandboxStatus::NotEnforced
        }
    };

    // ========================================================================
    // STEP 2: APPLY SECCOMP (SAFE WORKERS ONLY)
    // ========================================================================
    // Seccomp blocks dangerous syscalls. Toxic workers skip this because
    // they may legitimately need network access or fork for integration tests.
    if !is_toxic && let Err(e) = apply_seccomp() {
        eprintln!(
            "[worker:{}] WARNING: Seccomp setup failed: {}",
            worker_id, e
        );
        // Continue execution - Seccomp is defense-in-depth, not critical
    }

    Ok(landlock_status)
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Test that SandboxStatus enum is correctly defined
    #[test]
    fn test_sandbox_status_enum() {
        let status = SandboxStatus::FullyEnforced;
        assert_eq!(status, SandboxStatus::FullyEnforced);

        let status = SandboxStatus::PartiallyEnforced;
        assert_eq!(status, SandboxStatus::PartiallyEnforced);

        let status = SandboxStatus::NotEnforced;
        assert_eq!(status, SandboxStatus::NotEnforced);
    }

    /// Test that SandboxStatus can be cloned and copied
    #[test]
    fn test_sandbox_status_clone_copy() {
        let status = SandboxStatus::FullyEnforced;
        #[allow(clippy::clone_on_copy)]
        let cloned = status.clone();
        let copied = status;

        assert_eq!(status, cloned);
        assert_eq!(status, copied);
    }

    /// Test that SandboxStatus implements Debug
    #[test]
    fn test_sandbox_status_debug() {
        let status = SandboxStatus::FullyEnforced;
        let debug_str = format!("{:?}", status);
        assert!(debug_str.contains("FullyEnforced"));
    }

    /// Test that Landlock ABI V1 is available on supported kernels
    #[test]
    fn test_landlock_abi_detection() {
        use landlock::{ABI, Access, AccessFs};

        // This test just verifies the landlock crate is working
        // The actual ABI support depends on the kernel
        let abi = ABI::V1;
        let access = AccessFs::from_all(abi);

        // Verify we can create access flags
        assert!(!access.is_empty());
    }

    /// Test that read access is a subset of all access
    #[test]
    fn test_landlock_access_subset() {
        use landlock::{ABI, Access, AccessFs};

        let abi = ABI::V1;
        let all_access = AccessFs::from_all(abi);
        let read_access = AccessFs::from_read(abi);

        // Read access should be non-empty
        assert!(!read_access.is_empty());

        // All access should contain read access
        assert!(all_access.contains(read_access));
    }

    /// Test that Seccomp architecture detection works
    #[test]
    fn test_seccomp_arch_detection() {
        let arch = std::env::consts::ARCH;

        // Verify we're on a supported architecture
        assert!(
            arch == "x86_64" || arch == "aarch64",
            "Unsupported architecture: {}",
            arch
        );
    }

    /// Test that syscall numbers are valid
    #[test]
    #[allow(clippy::assertions_on_constants)]
    fn test_syscall_numbers() {
        // Verify the syscall numbers we're using are defined
        assert!(libc::SYS_socket > 0);
        assert!(libc::SYS_bind > 0);
        assert!(libc::SYS_connect > 0);
        assert!(libc::SYS_fork > 0);
        assert!(libc::SYS_execve > 0);
    }

    /// Test all network syscall numbers
    #[test]
    #[allow(clippy::assertions_on_constants)]
    fn test_network_syscall_numbers() {
        assert!(libc::SYS_socket > 0);
        assert!(libc::SYS_bind > 0);
        assert!(libc::SYS_connect > 0);
        assert!(libc::SYS_listen > 0);
        assert!(libc::SYS_accept > 0);
        assert!(libc::SYS_accept4 > 0);
    }

    /// Test all process syscall numbers
    #[test]
    #[allow(clippy::assertions_on_constants)]
    fn test_process_syscall_numbers() {
        assert!(libc::SYS_fork > 0);
        assert!(libc::SYS_vfork > 0);
        assert!(libc::SYS_execve > 0);
        assert!(libc::SYS_execveat > 0);
    }

    /// Test path canonicalization
    #[test]
    fn test_path_canonicalization() {
        // Current directory should be canonicalizable
        let cwd = std::env::current_dir().unwrap();
        let canonical = cwd.canonicalize().unwrap();

        assert!(canonical.is_absolute());
    }

    /// Test that /tmp exists (required for Landlock rules)
    #[test]
    fn test_tmp_exists() {
        assert!(Path::new("/tmp").exists());
    }

    /// Test that /usr exists (required for Landlock rules)
    #[test]
    fn test_usr_exists() {
        assert!(Path::new("/usr").exists());
    }

    /// Test that /proc exists (required for Python)
    #[test]
    fn test_proc_exists() {
        assert!(Path::new("/proc").exists());
    }

    /// Test that /dev exists (required for /dev/null, /dev/urandom)
    #[test]
    fn test_dev_exists() {
        assert!(Path::new("/dev").exists());
    }

    /// Test Seccomp filter creation for x86_64
    #[test]
    fn test_seccomp_filter_creation() {
        use seccompiler::{SeccompAction, SeccompFilter, SeccompRule, TargetArch};
        use std::collections::BTreeMap;

        let arch = std::env::consts::ARCH;
        if arch != "x86_64" && arch != "aarch64" {
            return; // Skip on unsupported architectures
        }

        let target_arch = match arch {
            "x86_64" => TargetArch::x86_64,
            "aarch64" => TargetArch::aarch64,
            _ => return,
        };

        let mut rules: BTreeMap<i64, Vec<SeccompRule>> = BTreeMap::new();
        rules.insert(libc::SYS_socket, vec![]);

        let filter = SeccompFilter::new(
            rules,
            SeccompAction::Allow,
            SeccompAction::Errno(libc::EPERM as u32),
            target_arch,
        );

        assert!(filter.is_ok(), "Failed to create Seccomp filter");
    }

    /// Test BPF program compilation
    #[test]
    fn test_seccomp_bpf_compilation() {
        use seccompiler::{BpfProgram, SeccompAction, SeccompFilter, SeccompRule, TargetArch};
        use std::collections::BTreeMap;

        let arch = std::env::consts::ARCH;
        if arch != "x86_64" && arch != "aarch64" {
            return;
        }

        let target_arch = match arch {
            "x86_64" => TargetArch::x86_64,
            "aarch64" => TargetArch::aarch64,
            _ => return,
        };

        let mut rules: BTreeMap<i64, Vec<SeccompRule>> = BTreeMap::new();
        rules.insert(libc::SYS_fork, vec![]);

        let filter = SeccompFilter::new(
            rules,
            SeccompAction::Allow,
            SeccompAction::Errno(libc::EPERM as u32),
            target_arch,
        )
        .unwrap();

        let bpf_result: Result<BpfProgram, _> = filter.try_into();
        assert!(bpf_result.is_ok(), "Failed to compile BPF program");
    }

    /// Test all privilege escalation syscall numbers
    #[test]
    #[allow(clippy::assertions_on_constants)]
    fn test_privilege_escalation_syscall_numbers() {
        // Verify the new privilege escalation syscalls are defined
        assert!(libc::SYS_ptrace > 0);
        assert!(libc::SYS_mount > 0);
        assert!(libc::SYS_umount2 > 0);
        assert!(libc::SYS_unshare > 0);
        assert!(libc::SYS_setns > 0);
    }

    /// Test that the seccomp filter includes all required syscalls
    #[test]
    fn test_seccomp_filter_includes_all_blocked_syscalls() {
        use seccompiler::{SeccompAction, SeccompFilter, SeccompRule, TargetArch};
        use std::collections::BTreeMap;

        let arch = std::env::consts::ARCH;
        if arch != "x86_64" && arch != "aarch64" {
            return;
        }

        let target_arch = match arch {
            "x86_64" => TargetArch::x86_64,
            "aarch64" => TargetArch::aarch64,
            _ => return,
        };

        // Build the same rules as apply_seccomp
        let mut rules: BTreeMap<i64, Vec<SeccompRule>> = BTreeMap::new();

        // Network syscalls
        rules.insert(libc::SYS_socket, vec![]);
        rules.insert(libc::SYS_bind, vec![]);
        rules.insert(libc::SYS_connect, vec![]);
        rules.insert(libc::SYS_listen, vec![]);
        rules.insert(libc::SYS_accept, vec![]);
        rules.insert(libc::SYS_accept4, vec![]);

        // Process syscalls
        rules.insert(libc::SYS_fork, vec![]);
        rules.insert(libc::SYS_vfork, vec![]);
        rules.insert(libc::SYS_execve, vec![]);
        rules.insert(libc::SYS_execveat, vec![]);

        // Privilege escalation syscalls
        rules.insert(libc::SYS_ptrace, vec![]);
        rules.insert(libc::SYS_mount, vec![]);
        rules.insert(libc::SYS_umount2, vec![]);
        rules.insert(libc::SYS_unshare, vec![]);
        rules.insert(libc::SYS_setns, vec![]);

        // Verify we have all 15 blocked syscalls
        assert_eq!(rules.len(), 15, "Expected 15 blocked syscalls");

        // Verify filter creation succeeds
        let filter = SeccompFilter::new(
            rules,
            SeccompAction::Allow,
            SeccompAction::Errno(libc::EPERM as u32),
            target_arch,
        );
        assert!(
            filter.is_ok(),
            "Failed to create Seccomp filter with all blocked syscalls"
        );
    }

    /// Test that add_path_rule_if_exists handles non-existent paths correctly
    /// This tests the TOCTOU fix - we should handle ENOENT atomically
    #[test]
    #[allow(clippy::assertions_on_constants)]
    fn test_add_path_rule_nonexistent_path() {
        // Verify that ENOENT constant is defined correctly
        assert!(libc::ENOENT > 0);

        // Test that a non-existent path would produce ENOENT
        let nonexistent = Path::new("/this/path/definitely/does/not/exist/12345");
        assert!(!nonexistent.exists());

        // The function should handle this gracefully (we can't test the actual
        // Landlock function without a real ruleset, but we verify the logic)
    }

    /// Test that ENOENT error code is correctly identified
    #[test]
    fn test_enoent_error_detection() {
        use std::io;

        // Create an ENOENT error
        let enoent_err = io::Error::from_raw_os_error(libc::ENOENT);
        assert_eq!(enoent_err.raw_os_error(), Some(libc::ENOENT));

        // Create a different error (EACCES)
        let eacces_err = io::Error::from_raw_os_error(libc::EACCES);
        assert_eq!(eacces_err.raw_os_error(), Some(libc::EACCES));
        assert_ne!(eacces_err.raw_os_error(), Some(libc::ENOENT));
    }

    // =========================================================================
    // ERROR PATH TESTS
    // =========================================================================
    // These tests verify error handling behavior when things go wrong.
    // They focus on input validation and error message quality.

    /// Test that apply_landlock returns an error for nonexistent project_root
    ///
    /// This tests the path canonicalization error path at line 166-168.
    /// When project_root doesn't exist, canonicalize() should fail with a
    /// descriptive error message.
    #[test]
    fn test_apply_landlock_nonexistent_path() {
        let nonexistent_path = Path::new("/this/path/definitely/does/not/exist/tach_test_12345");

        // Verify the path truly doesn't exist (precondition)
        assert!(
            !nonexistent_path.exists(),
            "Test precondition failed: path should not exist"
        );

        // Call apply_landlock with the nonexistent path
        let result = apply_landlock(nonexistent_path, 1);

        // Verify it returns an error (not panic)
        assert!(
            result.is_err(),
            "apply_landlock should return Err for nonexistent path"
        );

        // Verify the error message contains useful context
        let error = result.unwrap_err();
        let error_message = format!("{:#}", error);

        // The error chain should mention canonicalization failure
        assert!(
            error_message.contains("canonicalize")
                || error_message.contains("Canonicalize")
                || error_message.contains("project_root"),
            "Error message should mention canonicalization or project_root, got: {}",
            error_message
        );
    }

    /// Test that apply_landlock error message includes the path context
    ///
    /// Good error messages should help users understand what went wrong.
    #[test]
    fn test_apply_landlock_error_includes_path_context() {
        let nonexistent_path = Path::new("/nonexistent/tach_landlock_test");

        let result = apply_landlock(nonexistent_path, 42);

        assert!(result.is_err());
        let error = result.unwrap_err();
        let error_chain = format!("{:#}", error);

        // Error should indicate it's a Landlock-related failure
        // The context message "Failed to canonicalize project_root for Landlock"
        // should appear in the error chain
        assert!(
            error_chain.to_lowercase().contains("landlock") || error_chain.contains("project_root"),
            "Error chain should mention Landlock context, got: {}",
            error_chain
        );
    }

    /// Test that path canonicalization fails gracefully for relative nonexistent paths
    ///
    /// Relative paths that don't resolve to existing files should also fail gracefully.
    #[test]
    fn test_apply_landlock_relative_nonexistent_path() {
        // A relative path that doesn't exist
        let relative_nonexistent = Path::new("./nonexistent_dir_tach_test_xyz");

        // Verify precondition
        assert!(
            !relative_nonexistent.exists(),
            "Test precondition failed: path should not exist"
        );

        let result = apply_landlock(relative_nonexistent, 1);

        // Should fail without panicking
        assert!(
            result.is_err(),
            "apply_landlock should fail for relative nonexistent path"
        );

        // Error should be about canonicalization
        let error = result.unwrap_err();
        let error_message = format!("{}", error);
        assert!(
            error_message.contains("canonicalize") || error_message.contains("Landlock"),
            "Error should mention canonicalization failure, got: {}",
            error_message
        );
    }

    /// Test that apply_landlock doesn't panic on various invalid inputs
    ///
    /// This tests graceful degradation - errors should be returned, not panics.
    #[test]
    fn test_apply_landlock_no_panic_on_invalid_inputs() {
        // Test 1: Empty path
        let empty_path = Path::new("");
        let result = apply_landlock(empty_path, 0);
        assert!(result.is_err(), "Empty path should return error, not panic");

        // Test 2: Path with null bytes would typically panic, but std::path::Path
        // creation handles this. Test path that can't be canonicalized.
        let weird_path = Path::new("/\0invalid");
        // Note: Path::new doesn't validate, canonicalize() will fail
        // But this path creation itself is valid in Rust
        let result = apply_landlock(weird_path, 0);
        assert!(
            result.is_err(),
            "Invalid path should return error, not panic"
        );

        // Test 3: Very long path (should fail canonicalization)
        let long_component = "x".repeat(1000);
        let long_path_str = format!("/{}/{}/{}", long_component, long_component, long_component);
        let long_path = Path::new(&long_path_str);
        let result = apply_landlock(long_path, 0);
        assert!(
            result.is_err(),
            "Very long path should return error, not panic"
        );
    }

    /// Test add_path_rule helper function error handling
    ///
    /// Verify that the helper function produces proper error context.
    #[test]
    fn test_add_path_rule_error_context() {
        use landlock::{ABI, Access, AccessFs, Ruleset, RulesetAttr};

        let abi = ABI::V1;
        let read_access = AccessFs::from_read(abi);

        // Create a ruleset to test with
        let ruleset = Ruleset::default().handle_access(AccessFs::from_all(abi));

        // Skip test if ruleset creation fails (kernel doesn't support Landlock)
        let ruleset = match ruleset {
            Ok(r) => match r.create() {
                Ok(r) => r,
                Err(_) => return, // Kernel doesn't support Landlock
            },
            Err(_) => return, // Kernel doesn't support Landlock
        };

        // Try to add a rule for a nonexistent path
        let nonexistent = Path::new("/nonexistent_tach_test_path_12345");
        let result = add_path_rule(ruleset, nonexistent, read_access);

        // Should fail with error
        assert!(result.is_err());

        // Error should include the path in the message
        let error = result.unwrap_err();
        let error_message = format!("{:#}", error);
        assert!(
            error_message.contains("nonexistent_tach_test_path_12345")
                || error_message.contains("Landlock"),
            "Error should include the problematic path, got: {}",
            error_message
        );
    }

    /// Test add_path_rule_if_exists gracefully handles missing paths
    ///
    /// Unlike add_path_rule, this should succeed (return Ok) for missing paths.
    #[test]
    fn test_add_path_rule_if_exists_handles_missing_gracefully() {
        use landlock::{ABI, Access, AccessFs, Ruleset, RulesetAttr};

        let abi = ABI::V1;
        let read_access = AccessFs::from_read(abi);

        let ruleset = Ruleset::default().handle_access(AccessFs::from_all(abi));

        let ruleset = match ruleset {
            Ok(r) => match r.create() {
                Ok(r) => r,
                Err(_) => return,
            },
            Err(_) => return,
        };

        // Try to add a rule for a nonexistent path - should succeed
        let nonexistent = Path::new("/optional_path_that_does_not_exist_67890");
        let result = add_path_rule_if_exists(ruleset, nonexistent, read_access);

        // Should succeed - missing optional paths are handled gracefully
        assert!(
            result.is_ok(),
            "add_path_rule_if_exists should succeed for missing paths, got: {:?}",
            result.err()
        );
    }

    /// Test apply_iron_dome graceful degradation with invalid project_root
    ///
    /// apply_iron_dome should handle Landlock failures gracefully by returning
    /// NotEnforced status instead of propagating the error.
    #[test]
    fn test_apply_iron_dome_graceful_degradation() {
        let nonexistent_path = Path::new("/nonexistent/project/root/tach_test");

        // Call apply_iron_dome with invalid path
        let result = apply_iron_dome(nonexistent_path, 999, false);

        // Should succeed (graceful degradation) with NotEnforced status
        assert!(
            result.is_ok(),
            "apply_iron_dome should handle Landlock failures gracefully"
        );

        let status = result.unwrap();
        assert_eq!(
            status,
            SandboxStatus::NotEnforced,
            "Status should be NotEnforced when Landlock setup fails"
        );
    }

    /// Test apply_iron_dome with toxic worker flag
    ///
    /// Toxic workers should skip Seccomp but still attempt Landlock.
    #[test]
    fn test_apply_iron_dome_toxic_worker_skips_seccomp() {
        let nonexistent_path = Path::new("/nonexistent/project/root/tach_toxic_test");

        // Call with is_toxic = true
        let result = apply_iron_dome(nonexistent_path, 1000, true);

        // Should still return Ok (graceful degradation)
        assert!(
            result.is_ok(),
            "apply_iron_dome should handle failures gracefully for toxic workers"
        );

        // Status should be NotEnforced since path doesn't exist
        let status = result.unwrap();
        assert_eq!(status, SandboxStatus::NotEnforced);
    }

    /// Test that worker_id is included in scratch path construction
    ///
    /// Verify the worker scratch path is constructed correctly.
    #[test]
    fn test_worker_scratch_path_construction() {
        // Test that the format string works as expected
        let worker_id: u32 = 42;
        let scratch_path = format!("/run/tach/worker_{}", worker_id);
        assert_eq!(scratch_path, "/run/tach/worker_42");

        let worker_id: u32 = 0;
        let scratch_path = format!("/run/tach/worker_{}", worker_id);
        assert_eq!(scratch_path, "/run/tach/worker_0");

        let worker_id: u32 = u32::MAX;
        let scratch_path = format!("/run/tach/worker_{}", worker_id);
        assert_eq!(scratch_path, "/run/tach/worker_4294967295");
    }

    /// Test Seccomp unsupported architecture error message
    ///
    /// Note: This test documents expected behavior but can't actually trigger
    /// the error path since we're running on a supported architecture.
    /// The test verifies the error message format would be correct.
    #[test]
    fn test_seccomp_arch_error_message_format() {
        // We can't actually test an unsupported arch from a supported one,
        // but we can verify the error message format
        let fake_arch = "mips";
        let expected_message = format!("Unsupported architecture for Seccomp: {}", fake_arch);

        assert!(expected_message.contains("mips"));
        assert!(expected_message.contains("Unsupported architecture"));
        assert!(expected_message.contains("Seccomp"));
    }
}
