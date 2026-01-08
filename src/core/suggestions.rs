//! Context-Aware Suggestions for Common Failure Modes
//!
//! This module provides intelligent suggestions for common errors based on
//! runtime context detection. It analyzes the system state and provides
//! actionable guidance to help users resolve issues.
//!
//! # Design Philosophy
//!
//! Suggestions are context-aware: instead of generic hints, we detect the
//! actual system state and provide targeted advice. For example:
//! - If userfaultfd fails with EPERM, we check if the sysctl is set
//! - If pytest isn't found, we suggest pip install
//! - If we're in Docker, we suggest container-specific flags
//!
//! # Example
//!
//! ```ignore
//! use tach_core::suggestions::{SuggestionContext, get_suggestion};
//!
//! let ctx = SuggestionContext::detect();
//! let suggestion = get_suggestion(FailureCondition::UserfaultfdEperm, &ctx);
//! eprintln!("Hint: {}", suggestion);
//! ```

use std::fs;
use std::path::Path;

// =============================================================================
// Failure Conditions
// =============================================================================

/// Known failure conditions that can be detected at runtime.
///
/// Each condition maps to a specific error situation that we can provide
/// targeted suggestions for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureCondition {
    /// EPERM when creating userfaultfd
    UserfaultfdEperm,
    /// Kernel version too old for Landlock
    LandlockKernelTooOld,
    /// pytest not installed
    PytestNotFound,
    /// PYO3_PYTHON not set or incorrect
    Pyo3PythonInvalid,
    /// Too many open files (EMFILE/ENFILE)
    TooManyOpenFiles,
    /// Shared memory exhaustion
    SharedMemoryExhausted,
    /// Running inside Docker/container with restrictions
    ContainerRestrictions,
    /// Permission denied on file access
    PermissionDenied,
    /// Out of memory
    OutOfMemory,
    /// Seccomp filter blocked a syscall
    SeccompBlocked,
    /// Jemalloc not active
    JemallocNotActive,
    /// libpython not found
    LibpythonNotFound,
}

// =============================================================================
// System Context Detection
// =============================================================================

/// Detected system context for providing targeted suggestions.
///
/// This struct captures the current runtime environment to enable
/// context-aware suggestions.
#[derive(Debug, Clone)]
pub struct SuggestionContext {
    /// Kernel version as (major, minor, patch)
    pub kernel_version: Option<(u32, u32, u32)>,
    /// Whether unprivileged userfaultfd is enabled
    pub userfaultfd_enabled: bool,
    /// Whether running inside a container (Docker, Podman, etc.)
    pub in_container: bool,
    /// Container runtime if detected (e.g., "docker", "podman")
    pub container_runtime: Option<String>,
    /// Current file descriptor limit (soft limit)
    pub fd_limit: Option<u64>,
    /// Whether pytest is available
    pub pytest_available: bool,
    /// Python interpreter path
    pub python_path: Option<String>,
    /// Whether jemalloc is the active allocator
    pub jemalloc_active: bool,
}

impl SuggestionContext {
    /// Detect the current system context.
    ///
    /// This performs various system checks to understand the runtime
    /// environment. The detection is designed to be fast and non-blocking.
    pub fn detect() -> Self {
        Self {
            kernel_version: detect_kernel_version(),
            userfaultfd_enabled: detect_userfaultfd_enabled(),
            in_container: detect_in_container(),
            container_runtime: detect_container_runtime(),
            fd_limit: detect_fd_limit(),
            pytest_available: detect_pytest_available(),
            python_path: detect_python_path(),
            jemalloc_active: detect_jemalloc_active(),
        }
    }

    /// Create a minimal context for testing.
    #[cfg(test)]
    pub fn minimal() -> Self {
        Self {
            kernel_version: None,
            userfaultfd_enabled: false,
            in_container: false,
            container_runtime: None,
            fd_limit: None,
            pytest_available: false,
            python_path: None,
            jemalloc_active: false,
        }
    }
}

impl Default for SuggestionContext {
    fn default() -> Self {
        Self::detect()
    }
}

// =============================================================================
// Context Detection Functions
// =============================================================================

/// Parse kernel version from /proc/version
fn detect_kernel_version() -> Option<(u32, u32, u32)> {
    let version_str = fs::read_to_string("/proc/version").ok()?;
    let parts: Vec<&str> = version_str.split_whitespace().collect();
    if parts.len() < 3 {
        return None;
    }

    let version_part = parts[2];
    let version_nums: Vec<&str> = version_part.split('.').collect();
    if version_nums.len() < 2 {
        return None;
    }

    let major: u32 = version_nums[0].parse().ok()?;
    let minor: u32 = version_nums[1].parse().ok()?;
    let patch: u32 = version_nums
        .get(2)
        .and_then(|p| p.split('-').next())
        .and_then(|p| p.parse().ok())
        .unwrap_or(0);

    Some((major, minor, patch))
}

/// Check if unprivileged userfaultfd is enabled via sysctl
fn detect_userfaultfd_enabled() -> bool {
    fs::read_to_string("/proc/sys/vm/unprivileged_userfaultfd")
        .map(|s| s.trim() == "1")
        .unwrap_or(false)
}

/// Detect if running inside a container
fn detect_in_container() -> bool {
    // Check for /.dockerenv
    if Path::new("/.dockerenv").exists() {
        return true;
    }

    // Check for container environment variables
    if std::env::var("container").is_ok() {
        return true;
    }

    // Check cgroup for container indicators
    if let Ok(cgroup) = fs::read_to_string("/proc/1/cgroup") {
        if cgroup.contains("docker")
            || cgroup.contains("lxc")
            || cgroup.contains("kubepods")
            || cgroup.contains("containerd")
        {
            return true;
        }
    }

    // Check for container-specific files
    if Path::new("/run/.containerenv").exists() {
        return true;
    }

    false
}

/// Detect container runtime type
fn detect_container_runtime() -> Option<String> {
    if Path::new("/.dockerenv").exists() {
        return Some("docker".to_string());
    }

    if Path::new("/run/.containerenv").exists() {
        return Some("podman".to_string());
    }

    if let Ok(cgroup) = fs::read_to_string("/proc/1/cgroup") {
        if cgroup.contains("docker") {
            return Some("docker".to_string());
        }
        if cgroup.contains("lxc") {
            return Some("lxc".to_string());
        }
        if cgroup.contains("kubepods") {
            return Some("kubernetes".to_string());
        }
    }

    None
}

/// Get current file descriptor soft limit
fn detect_fd_limit() -> Option<u64> {
    // Read /proc/self/limits
    let limits = fs::read_to_string("/proc/self/limits").ok()?;
    for line in limits.lines() {
        if line.starts_with("Max open files") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            // Format: "Max open files            1024                 1048576              files"
            if parts.len() >= 4 {
                return parts[3].parse().ok();
            }
        }
    }
    None
}

/// Check if pytest is available
fn detect_pytest_available() -> bool {
    let python_path = std::env::var("PYO3_PYTHON")
        .or_else(|_| std::env::var("PYTHON"))
        .unwrap_or_else(|_| "python3".to_string());

    std::process::Command::new(&python_path)
        .args(["-c", "import pytest"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Get Python interpreter path
fn detect_python_path() -> Option<String> {
    std::env::var("PYO3_PYTHON")
        .or_else(|_| std::env::var("PYTHON"))
        .ok()
        .or_else(|| {
            // Check if python3 is available
            std::process::Command::new("which")
                .arg("python3")
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        })
}

/// Check if jemalloc is active
fn detect_jemalloc_active() -> bool {
    crate::allocator::verify_jemalloc_active().is_ok()
}

// =============================================================================
// Suggestion Generation
// =============================================================================

/// Get a context-aware suggestion for a failure condition.
///
/// This function analyzes the current context and provides targeted,
/// actionable advice for the specific failure.
pub fn get_suggestion(condition: FailureCondition, ctx: &SuggestionContext) -> String {
    match condition {
        FailureCondition::UserfaultfdEperm => suggest_userfaultfd_eperm(ctx),
        FailureCondition::LandlockKernelTooOld => suggest_landlock_kernel(ctx),
        FailureCondition::PytestNotFound => suggest_pytest_not_found(ctx),
        FailureCondition::Pyo3PythonInvalid => suggest_pyo3_python(ctx),
        FailureCondition::TooManyOpenFiles => suggest_too_many_files(ctx),
        FailureCondition::SharedMemoryExhausted => suggest_shared_memory(ctx),
        FailureCondition::ContainerRestrictions => suggest_container_restrictions(ctx),
        FailureCondition::PermissionDenied => suggest_permission_denied(ctx),
        FailureCondition::OutOfMemory => suggest_out_of_memory(ctx),
        FailureCondition::SeccompBlocked => suggest_seccomp_blocked(ctx),
        FailureCondition::JemallocNotActive => suggest_jemalloc(ctx),
        FailureCondition::LibpythonNotFound => suggest_libpython(ctx),
    }
}

fn suggest_userfaultfd_eperm(ctx: &SuggestionContext) -> String {
    let mut suggestion = String::new();

    if ctx.in_container {
        suggestion.push_str("Running in a container. ");
        if let Some(ref runtime) = ctx.container_runtime {
            match runtime.as_str() {
                "docker" => {
                    suggestion
                        .push_str("Run with: docker run --privileged or --cap-add=SYS_PTRACE");
                }
                "podman" => {
                    suggestion.push_str("Run with: podman run --cap-add=SYS_PTRACE");
                }
                "kubernetes" => {
                    suggestion.push_str("Add SYS_PTRACE capability to your pod security context");
                }
                _ => {
                    suggestion.push_str("Add CAP_SYS_PTRACE capability to the container");
                }
            }
        } else {
            suggestion.push_str("Add CAP_SYS_PTRACE capability to the container");
        }
    } else if !ctx.userfaultfd_enabled {
        suggestion.push_str("Set vm.unprivileged_userfaultfd=1:\n");
        suggestion.push_str("  sudo sysctl -w vm.unprivileged_userfaultfd=1\n");
        suggestion.push_str("Or run with CAP_SYS_PTRACE capability");
    } else {
        suggestion.push_str("Run with CAP_SYS_PTRACE capability or as root");
    }

    suggestion
}

fn suggest_landlock_kernel(ctx: &SuggestionContext) -> String {
    let mut suggestion = String::new();

    if let Some((major, minor, _)) = ctx.kernel_version {
        suggestion.push_str(&format!("Kernel {}.{} detected. ", major, minor));
        suggestion.push_str("Landlock requires kernel 5.13+. ");
    } else {
        suggestion.push_str("Kernel version could not be detected. ");
    }

    suggestion.push_str("Tach will run without filesystem isolation (reduced security).");

    suggestion
}

fn suggest_pytest_not_found(ctx: &SuggestionContext) -> String {
    let mut suggestion = String::new();

    if let Some(ref python) = ctx.python_path {
        suggestion.push_str(&format!(
            "Install pytest in your Python environment:\n  {} -m pip install pytest",
            python
        ));
    } else {
        suggestion.push_str("Install pytest:\n  pip install pytest");
    }

    suggestion
}

fn suggest_pyo3_python(ctx: &SuggestionContext) -> String {
    let mut suggestion = String::new();

    suggestion.push_str("Set PYO3_PYTHON to your Python interpreter path:\n");
    suggestion.push_str("  export PYO3_PYTHON=$(which python3)\n");
    suggestion.push_str("Or:\n");
    suggestion.push_str("  export PYO3_PYTHON=/path/to/your/python");

    if ctx.python_path.is_none() {
        suggestion.push_str("\n\nNo Python interpreter found in PATH.");
    }

    suggestion
}

fn suggest_too_many_files(ctx: &SuggestionContext) -> String {
    let mut suggestion = String::new();

    if let Some(limit) = ctx.fd_limit {
        suggestion.push_str(&format!("Current limit: {} files. ", limit));
    }

    suggestion.push_str("Increase the file descriptor limit:\n");
    suggestion.push_str("  ulimit -n 65536\n");
    suggestion.push_str("For permanent change, edit /etc/security/limits.conf");

    if ctx.in_container {
        suggestion.push_str("\n\nIn containers, also check:\n");
        suggestion.push_str("  docker run --ulimit nofile=65536:65536");
    }

    suggestion
}

fn suggest_shared_memory(ctx: &SuggestionContext) -> String {
    let mut suggestion = String::new();

    suggestion.push_str("Shared memory exhausted. Options:\n");
    suggestion.push_str("  1. Reduce worker count with: tach -n 2 tests/\n");
    suggestion.push_str("  2. Increase /dev/shm size:\n");
    suggestion.push_str("     mount -o remount,size=4G /dev/shm");

    if ctx.in_container {
        suggestion.push_str("\n\n");
        if let Some(ref runtime) = ctx.container_runtime {
            match runtime.as_str() {
                "docker" => {
                    suggestion.push_str("In Docker, use:\n");
                    suggestion.push_str("  docker run --shm-size=4g");
                }
                "kubernetes" => {
                    suggestion.push_str("In Kubernetes, add a memory-backed emptyDir volume:\n");
                    suggestion.push_str("  volumes:\n");
                    suggestion.push_str("    - name: shm\n");
                    suggestion.push_str("      emptyDir:\n");
                    suggestion.push_str("        medium: Memory\n");
                    suggestion.push_str("        sizeLimit: 4Gi");
                }
                _ => {
                    suggestion.push_str("Increase shared memory in your container runtime");
                }
            }
        }
    }

    suggestion
}

fn suggest_container_restrictions(ctx: &SuggestionContext) -> String {
    let mut suggestion = String::new();

    suggestion.push_str("Container security restrictions detected.\n\n");

    if let Some(ref runtime) = ctx.container_runtime {
        match runtime.as_str() {
            "docker" => {
                suggestion.push_str("For Docker, try one of:\n");
                suggestion.push_str("  docker run --privileged ...\n");
                suggestion.push_str(
                    "  docker run --cap-add=SYS_PTRACE --security-opt seccomp=unconfined ...\n",
                );
                suggestion.push_str("  docker run --cap-add=SYS_PTRACE --cap-add=SYS_ADMIN ...");
            }
            "podman" => {
                suggestion.push_str("For Podman, try:\n");
                suggestion.push_str("  podman run --cap-add=SYS_PTRACE --cap-add=SYS_ADMIN ...");
            }
            "kubernetes" => {
                suggestion.push_str("For Kubernetes, add to your pod spec:\n");
                suggestion.push_str("  securityContext:\n");
                suggestion.push_str("    capabilities:\n");
                suggestion.push_str("      add: [\"SYS_PTRACE\"]");
            }
            _ => {
                suggestion.push_str("Add SYS_PTRACE capability to your container.");
            }
        }
    } else {
        suggestion.push_str("Add SYS_PTRACE capability to your container.\n");
        suggestion.push_str("For Docker: docker run --cap-add=SYS_PTRACE ...");
    }

    suggestion
}

fn suggest_permission_denied(ctx: &SuggestionContext) -> String {
    let mut suggestion = String::new();

    suggestion.push_str("Permission denied. Check:\n");
    suggestion.push_str("  1. File and directory permissions\n");
    suggestion.push_str("  2. User ownership of test files\n");
    suggestion.push_str("  3. SELinux or AppArmor restrictions");

    if ctx.in_container {
        suggestion.push_str("\n\nIn containers, also check:\n");
        suggestion.push_str("  - Volume mount permissions (--user flag)\n");
        suggestion.push_str("  - Security context settings");
    }

    suggestion
}

fn suggest_out_of_memory(ctx: &SuggestionContext) -> String {
    let mut suggestion = String::new();

    suggestion.push_str("Out of memory. Options:\n");
    suggestion.push_str("  1. Reduce worker count: tach -n 2 tests/\n");
    suggestion.push_str("  2. Increase system memory or swap\n");
    suggestion.push_str("  3. Run tests in smaller batches");

    if ctx.in_container {
        suggestion.push_str("\n\nIn containers, also check:\n");
        suggestion.push_str("  docker run --memory=4g ...");
    }

    suggestion
}

fn suggest_seccomp_blocked(ctx: &SuggestionContext) -> String {
    let mut suggestion = String::new();

    suggestion.push_str("A syscall was blocked by seccomp. ");

    if ctx.in_container {
        suggestion.push_str("This is common in containers.\n\n");
        if let Some(ref runtime) = ctx.container_runtime {
            match runtime.as_str() {
                "docker" => {
                    suggestion.push_str("For Docker, try:\n");
                    suggestion.push_str("  docker run --security-opt seccomp=unconfined ...");
                }
                _ => {
                    suggestion.push_str("Disable seccomp or use a permissive profile.");
                }
            }
        }
    } else {
        suggestion.push_str("Try running Tach with --no-isolation to disable seccomp filtering.");
    }

    suggestion
}

fn suggest_jemalloc(ctx: &SuggestionContext) -> String {
    let mut suggestion = String::new();

    suggestion.push_str("Jemalloc is not the active allocator.\n\n");
    suggestion.push_str("The Hypervisor requires jemalloc for snapshot consistency.\n");
    suggestion.push_str("Ensure tikv-jemallocator is set as #[global_allocator] in lib.rs.\n\n");
    suggestion.push_str("If using LD_PRELOAD with another allocator, this may override jemalloc.");

    if ctx.in_container {
        suggestion.push_str("\n\nNote: In containers, ensure you're using the tach binary,\n");
        suggestion.push_str("not a development build with test configuration.");
    }

    suggestion
}

fn suggest_libpython(ctx: &SuggestionContext) -> String {
    let mut suggestion = String::new();

    suggestion.push_str("libpython shared library not found.\n\n");

    if let Some(ref python) = ctx.python_path {
        suggestion.push_str(&format!("Python at {} may be statically linked.\n", python));
    }

    suggestion.push_str("Install Python with shared library support:\n");
    suggestion.push_str("  - Debian/Ubuntu: apt install python3-dev\n");
    suggestion.push_str("  - RHEL/CentOS: yum install python3-devel\n");
    suggestion
        .push_str("  - pyenv: PYTHON_CONFIGURE_OPTS=\"--enable-shared\" pyenv install 3.12.0");

    suggestion
}

// =============================================================================
// Quick Suggestion Helpers
// =============================================================================

/// Get a suggestion without needing to detect full context.
///
/// This is useful for quick suggestions where full context detection
/// would be too slow.
pub fn quick_suggestion(condition: FailureCondition) -> &'static str {
    match condition {
        FailureCondition::UserfaultfdEperm => {
            "Set vm.unprivileged_userfaultfd=1 or run with CAP_SYS_PTRACE"
        }
        FailureCondition::LandlockKernelTooOld => {
            "Landlock requires kernel 5.13+. Running without filesystem isolation."
        }
        FailureCondition::PytestNotFound => "Install pytest: pip install pytest",
        FailureCondition::Pyo3PythonInvalid => "Set PYO3_PYTHON to your Python interpreter path",
        FailureCondition::TooManyOpenFiles => "Increase ulimit: ulimit -n 65536",
        FailureCondition::SharedMemoryExhausted => {
            "Reduce worker count with -n or increase /dev/shm size"
        }
        FailureCondition::ContainerRestrictions => "Add CAP_SYS_PTRACE capability to the container",
        FailureCondition::PermissionDenied => "Check file permissions and ownership",
        FailureCondition::OutOfMemory => "Reduce worker count with -n or increase system memory",
        FailureCondition::SeccompBlocked => "Try --no-isolation to disable seccomp filtering",
        FailureCondition::JemallocNotActive => {
            "Ensure tikv-jemallocator is set as #[global_allocator]"
        }
        FailureCondition::LibpythonNotFound => {
            "Install Python with shared library support (python3-dev)"
        }
    }
}

/// Analyze an error message and suggest a failure condition.
///
/// This function attempts to parse error messages and map them to
/// known failure conditions for suggestion lookup.
pub fn detect_condition_from_error(error_msg: &str) -> Option<FailureCondition> {
    let lower = error_msg.to_lowercase();

    if lower.contains("eperm") && lower.contains("userfaultfd") {
        return Some(FailureCondition::UserfaultfdEperm);
    }
    if lower.contains("landlock") && (lower.contains("unavailable") || lower.contains("5.13")) {
        return Some(FailureCondition::LandlockKernelTooOld);
    }
    if lower.contains("pytest") && (lower.contains("not found") || lower.contains("no module")) {
        return Some(FailureCondition::PytestNotFound);
    }
    if lower.contains("pyo3_python") || (lower.contains("python") && lower.contains("not found")) {
        return Some(FailureCondition::Pyo3PythonInvalid);
    }
    if lower.contains("too many open files") || lower.contains("emfile") || lower.contains("enfile")
    {
        return Some(FailureCondition::TooManyOpenFiles);
    }
    if lower.contains("shm") && (lower.contains("exhausted") || lower.contains("no space")) {
        return Some(FailureCondition::SharedMemoryExhausted);
    }
    if lower.contains("container") && lower.contains("restriction") {
        return Some(FailureCondition::ContainerRestrictions);
    }
    if lower.contains("permission denied") || lower.contains("eacces") {
        return Some(FailureCondition::PermissionDenied);
    }
    if lower.contains("out of memory") || lower.contains("enomem") || lower.contains("oom") {
        return Some(FailureCondition::OutOfMemory);
    }
    if lower.contains("seccomp") && lower.contains("blocked") {
        return Some(FailureCondition::SeccompBlocked);
    }
    if lower.contains("jemalloc") && lower.contains("not") {
        return Some(FailureCondition::JemallocNotActive);
    }
    if lower.contains("libpython") && lower.contains("not found") {
        return Some(FailureCondition::LibpythonNotFound);
    }

    None
}

/// Get a suggestion for an error message.
///
/// This is a convenience function that detects the condition and
/// returns an appropriate suggestion.
pub fn suggest_for_error(error_msg: &str) -> Option<String> {
    let condition = detect_condition_from_error(error_msg)?;
    Some(quick_suggestion(condition).to_string())
}

/// Get a detailed suggestion for an error message with full context.
///
/// This performs full context detection for more detailed suggestions.
pub fn suggest_for_error_detailed(error_msg: &str) -> Option<String> {
    let condition = detect_condition_from_error(error_msg)?;
    let ctx = SuggestionContext::detect();
    Some(get_suggestion(condition, &ctx))
}

// =============================================================================
// Unit Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_failure_condition_variants() {
        // Ensure all variants are distinct
        let conditions = vec![
            FailureCondition::UserfaultfdEperm,
            FailureCondition::LandlockKernelTooOld,
            FailureCondition::PytestNotFound,
            FailureCondition::Pyo3PythonInvalid,
            FailureCondition::TooManyOpenFiles,
            FailureCondition::SharedMemoryExhausted,
            FailureCondition::ContainerRestrictions,
            FailureCondition::PermissionDenied,
            FailureCondition::OutOfMemory,
            FailureCondition::SeccompBlocked,
            FailureCondition::JemallocNotActive,
            FailureCondition::LibpythonNotFound,
        ];

        // Each should produce a non-empty quick suggestion
        for condition in conditions {
            let suggestion = quick_suggestion(condition);
            assert!(
                !suggestion.is_empty(),
                "Empty suggestion for {:?}",
                condition
            );
        }
    }

    #[test]
    fn test_quick_suggestion_userfaultfd() {
        let suggestion = quick_suggestion(FailureCondition::UserfaultfdEperm);
        assert!(suggestion.contains("userfaultfd") || suggestion.contains("CAP_SYS_PTRACE"));
    }

    #[test]
    fn test_quick_suggestion_landlock() {
        let suggestion = quick_suggestion(FailureCondition::LandlockKernelTooOld);
        assert!(suggestion.contains("5.13") || suggestion.contains("Landlock"));
    }

    #[test]
    fn test_quick_suggestion_pytest() {
        let suggestion = quick_suggestion(FailureCondition::PytestNotFound);
        assert!(suggestion.contains("pip install pytest"));
    }

    #[test]
    fn test_quick_suggestion_pyo3() {
        let suggestion = quick_suggestion(FailureCondition::Pyo3PythonInvalid);
        assert!(suggestion.contains("PYO3_PYTHON"));
    }

    #[test]
    fn test_detect_condition_userfaultfd() {
        let msg = "userfaultfd creation failed: EPERM";
        let condition = detect_condition_from_error(msg);
        assert_eq!(condition, Some(FailureCondition::UserfaultfdEperm));
    }

    #[test]
    fn test_detect_condition_pytest() {
        let msg = "ModuleNotFoundError: No module named 'pytest'";
        let condition = detect_condition_from_error(msg);
        assert_eq!(condition, Some(FailureCondition::PytestNotFound));
    }

    #[test]
    fn test_detect_condition_too_many_files() {
        let msg = "Failed to open file: Too many open files";
        let condition = detect_condition_from_error(msg);
        assert_eq!(condition, Some(FailureCondition::TooManyOpenFiles));
    }

    #[test]
    fn test_detect_condition_permission() {
        let msg = "Error: Permission denied when accessing /etc/shadow";
        let condition = detect_condition_from_error(msg);
        assert_eq!(condition, Some(FailureCondition::PermissionDenied));
    }

    #[test]
    fn test_detect_condition_oom() {
        let msg = "Failed to allocate: out of memory";
        let condition = detect_condition_from_error(msg);
        assert_eq!(condition, Some(FailureCondition::OutOfMemory));
    }

    #[test]
    fn test_detect_condition_unknown() {
        let msg = "Some random error message";
        let condition = detect_condition_from_error(msg);
        assert_eq!(condition, None);
    }

    #[test]
    fn test_suggest_for_error() {
        let msg = "userfaultfd: EPERM";
        let suggestion = suggest_for_error(msg);
        assert!(suggestion.is_some());
        let sugg_str = suggestion.unwrap();
        assert!(
            sugg_str.contains("userfaultfd") || sugg_str.contains("PTRACE"),
            "Expected userfaultfd or PTRACE in suggestion: {}",
            sugg_str
        );
    }

    #[test]
    fn test_suggest_for_error_unknown() {
        let msg = "Some unknown error";
        let suggestion = suggest_for_error(msg);
        assert!(suggestion.is_none());
    }

    #[test]
    fn test_suggestion_context_minimal() {
        let ctx = SuggestionContext::minimal();
        assert!(ctx.kernel_version.is_none());
        assert!(!ctx.userfaultfd_enabled);
        assert!(!ctx.in_container);
    }

    #[test]
    fn test_get_suggestion_with_context() {
        let mut ctx = SuggestionContext::minimal();
        ctx.in_container = true;
        ctx.container_runtime = Some("docker".to_string());

        let suggestion = get_suggestion(FailureCondition::UserfaultfdEperm, &ctx);
        assert!(suggestion.contains("docker") || suggestion.contains("container"));
    }

    #[test]
    fn test_get_suggestion_landlock_with_version() {
        let mut ctx = SuggestionContext::minimal();
        ctx.kernel_version = Some((5, 10, 0));

        let suggestion = get_suggestion(FailureCondition::LandlockKernelTooOld, &ctx);
        assert!(suggestion.contains("5.10") || suggestion.contains("5.13"));
    }

    #[test]
    fn test_get_suggestion_pytest_with_python_path() {
        let mut ctx = SuggestionContext::minimal();
        ctx.python_path = Some("/usr/bin/python3".to_string());

        let suggestion = get_suggestion(FailureCondition::PytestNotFound, &ctx);
        assert!(suggestion.contains("/usr/bin/python3") || suggestion.contains("pip install"));
    }

    #[test]
    fn test_get_suggestion_too_many_files_with_limit() {
        let mut ctx = SuggestionContext::minimal();
        ctx.fd_limit = Some(1024);

        let suggestion = get_suggestion(FailureCondition::TooManyOpenFiles, &ctx);
        assert!(suggestion.contains("1024") || suggestion.contains("ulimit"));
    }

    #[test]
    fn test_get_suggestion_container_kubernetes() {
        let mut ctx = SuggestionContext::minimal();
        ctx.in_container = true;
        ctx.container_runtime = Some("kubernetes".to_string());

        let suggestion = get_suggestion(FailureCondition::ContainerRestrictions, &ctx);
        assert!(
            suggestion.contains("Kubernetes")
                || suggestion.contains("pod")
                || suggestion.contains("securityContext")
        );
    }

    #[test]
    fn test_get_suggestion_shared_memory_docker() {
        let mut ctx = SuggestionContext::minimal();
        ctx.in_container = true;
        ctx.container_runtime = Some("docker".to_string());

        let suggestion = get_suggestion(FailureCondition::SharedMemoryExhausted, &ctx);
        assert!(suggestion.contains("--shm-size") || suggestion.contains("docker"));
    }
}
