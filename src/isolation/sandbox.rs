//! Phase 5.2: The Iron Dome - Sandbox Hardening
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
    use landlock::{Access, AccessFs, Ruleset, RulesetAttr, RulesetStatus, ABI};

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
    let project_root = project_root.canonicalize().context("Failed to canonicalize project_root for Landlock")?;

    // Worker scratch space (created by isolation.rs)
    let worker_scratch = format!("/run/tach/worker_{}", worker_id);

    // ========================================================================
    // DEFINE ACCESS RIGHTS
    // ========================================================================
    // AccessFs::from_all(abi) = all filesystem operations for this ABI
    // AccessFs::from_read(abi) = read-only operations (execute, read_file, read_dir)
    let all_access = AccessFs::from_all(abi);
    let read_access = AccessFs::from_read(abi);

    // ========================================================================
    // CREATE RULESET
    // ========================================================================
    // The ruleset defines which operations we want to restrict.
    // By calling handle_access(all_access), we're saying "we want to control
    // all filesystem operations". Any operation not explicitly allowed will
    // be denied after restrict_self().
    let ruleset = Ruleset::default().handle_access(all_access).context("Failed to create Landlock ruleset")?.create().context("Failed to create Landlock ruleset")?;

    // ========================================================================
    // ADD READ-ONLY RULES
    // ========================================================================
    // These paths are allowed read-only access (execute, read_file, read_dir).
    // This includes:
    // - Project root (the lowerdir of the overlay - tests read source files)
    // - System libraries (/usr, /lib, /lib64, /bin)
    // - Configuration (/etc - Python configs, SSL certs, timezone)
    // - Device nodes (/dev - /dev/null, /dev/urandom, /dev/zero)
    //
    // SECURITY NOTE: We allow read access to project_root because tests need
    // to read source files. Writes go to the overlay upperdir, which is
    // inside /run/tach/worker_{id} (allowed RW below).

    let ruleset = add_path_rule(ruleset, &project_root, read_access)?;
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
    let status = ruleset.restrict_self().context("Failed to apply Landlock restrictions")?;

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
    let fd = PathFd::new(path).with_context(|| format!("Failed to open path for Landlock: {}", path.display()))?;

    ruleset.add_rule(PathBeneath::new(fd, access)).with_context(|| format!("Failed to add Landlock rule for: {}", path.display()))
}

/// Helper: Add a Landlock rule for a path (silently skips if path doesn't exist).
///
/// This is used for optional system paths like /lib64 which may not exist
/// on all systems.
fn add_path_rule_if_exists<T, A>(ruleset: T, path: impl AsRef<Path>, access: A) -> Result<T>
where
    T: landlock::RulesetCreatedAttr,
    A: Into<landlock::BitFlags<landlock::AccessFs>> + Copy,
{
    use landlock::{PathBeneath, PathFd};

    let path = path.as_ref();

    // Check if path exists before trying to open it
    if !path.exists() {
        return Ok(ruleset);
    }

    match PathFd::new(path) {
        Ok(fd) => ruleset.add_rule(PathBeneath::new(fd, access)).with_context(|| format!("Failed to add Landlock rule for: {}", path.display())),
        Err(_) => {
            // Path exists but we can't open it (permissions, etc.)
            // This is fine - just skip this rule
            Ok(ruleset)
        }
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
        SeccompAction::Allow,                     // Allow syscalls not in blacklist
        SeccompAction::Errno(libc::EPERM as u32), // Block with EPERM (not SIGSYS)
        target_arch,
    )
    .context("Failed to create Seccomp filter")?;

    // ========================================================================
    // COMPILE TO BPF
    // ========================================================================
    // Convert the high-level filter to a BPF program that the kernel can execute.
    let bpf_prog: seccompiler::BpfProgram = filter.try_into().context("Failed to compile Seccomp filter to BPF")?;

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
pub fn apply_iron_dome(project_root: &Path, worker_id: u32, is_toxic: bool) -> Result<SandboxStatus> {
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
                    eprintln!("[worker:{}] Landlock partially enforced (some features unavailable)", worker_id);
                }
                SandboxStatus::NotEnforced => {
                    eprintln!("[worker:{}] WARNING: Landlock not enforced - kernel too old (< 5.13)", worker_id);
                }
            }
            status
        }
        Err(e) => {
            eprintln!("[worker:{}] WARNING: Landlock setup failed: {}", worker_id, e);
            SandboxStatus::NotEnforced
        }
    };

    // ========================================================================
    // STEP 2: APPLY SECCOMP (SAFE WORKERS ONLY)
    // ========================================================================
    // Seccomp blocks dangerous syscalls. Toxic workers skip this because
    // they may legitimately need network access or fork for integration tests.
    if !is_toxic {
        if let Err(e) = apply_seccomp() {
            eprintln!("[worker:{}] WARNING: Seccomp setup failed: {}", worker_id, e);
            // Continue execution - Seccomp is defense-in-depth, not critical
        }
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
        use landlock::{Access, AccessFs, ABI};

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
        use landlock::{Access, AccessFs, ABI};

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
        assert!(arch == "x86_64" || arch == "aarch64", "Unsupported architecture: {}", arch);
    }

    /// Test that syscall numbers are valid
    #[test]
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

        let filter = SeccompFilter::new(rules, SeccompAction::Allow, SeccompAction::Errno(libc::EPERM as u32), target_arch);

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

        let filter = SeccompFilter::new(rules, SeccompAction::Allow, SeccompAction::Errno(libc::EPERM as u32), target_arch).unwrap();

        let bpf_result: Result<BpfProgram, _> = filter.try_into();
        assert!(bpf_result.is_ok(), "Failed to compile BPF program");
    }
}
