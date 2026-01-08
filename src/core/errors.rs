//! Tach Core Error Types
//!
//! This module defines the unified error hierarchy for Project Tach.
//! All errors are domain-specific to enable intelligent error handling:
//! - Retry on transient failures (jitter, timing)
//! - Kill on corruption (memory desync, orphaned locks)
//! - Degrade on missing capabilities (Landlock, Seccomp)
//!
//! # Error Domains
//!
//! - [`RestorationError`]: Memory/TLS synchronization failures
//! - [`IsolationError`]: Sandbox (Landlock/Seccomp/Namespace) failures
//! - [`CalibrationError`]: Sentinel scan and offset discovery failures
//! - [`TeleportationError`]: SCM_RIGHTS FD adoption failures
//! - [`DiscoveryError`]: Test discovery and resolution failures
//! - [`SchedulerError`]: Worker pool and IPC failures
//!
//! # Example
//!
//! ```ignore
//! use tach_core::errors::{TachError, RestorationError};
//!
//! fn restore_memory() -> Result<(), TachError> {
//!     // ... restoration logic
//!     Err(TachError::Restoration(RestorationError::HeapDesync {
//!         expected_checksum: 0xDEADBEEF,
//!         actual_checksum: 0xCAFEBABE,
//!     }))
//! }
//! ```

use std::fmt;
use std::path::PathBuf;
use thiserror::Error;

// =============================================================================
// Top-Level Error Enum
// =============================================================================

/// Unified error type for all Tach operations.
///
/// This enum provides domain-specific error variants that enable the supervisor
/// to make intelligent decisions about error handling:
///
/// - **Retry**: Transient failures (jitter, timing, resource contention)
/// - **Kill**: Corruption detected (memory desync, orphaned locks)
/// - **Degrade**: Missing capabilities (graceful degradation)
/// - **Abort**: Fatal system errors (out of memory, kernel bugs)
#[derive(Error, Debug)]
pub enum TachError {
    /// Memory or TLS restoration failures.
    ///
    /// These errors indicate the Restoration Quadrant (BSS, Heap, Stack, TLS)
    /// has detected inconsistency. The worker MUST be killed.
    #[error("Restoration failure: {0}")]
    Restoration(#[from] RestorationError),

    /// Sandbox isolation failures.
    ///
    /// These errors indicate Landlock, Seccomp, or Namespace setup failed.
    /// May allow graceful degradation on older kernels.
    #[error("Isolation failure: {0}")]
    Isolation(#[from] IsolationError),

    /// Calibration and sentinel scan failures.
    ///
    /// These errors indicate TLS offset discovery or allocator calibration
    /// failed. The Hypervisor cannot proceed without valid offsets.
    #[error("Calibration failure: {0}")]
    Calibration(#[from] CalibrationError),

    /// File descriptor teleportation failures.
    ///
    /// These errors indicate SCM_RIGHTS transmission or FD adoption failed.
    /// The resource (socket, file, DB connection) cannot be shared.
    #[error("Teleportation failure: {0}")]
    Teleportation(#[from] TeleportationError),

    /// Test discovery and resolution failures.
    ///
    /// These errors indicate test collection or fixture resolution failed.
    /// Usually non-fatal; specific tests are skipped.
    #[error("Discovery failure: {0}")]
    Discovery(#[from] DiscoveryError),

    /// Scheduler and worker pool failures.
    ///
    /// These errors indicate IPC, worker lifecycle, or scheduling failures.
    #[error("Scheduler failure: {0}")]
    Scheduler(#[from] SchedulerError),

    /// Protocol and serialization failures.
    ///
    /// These errors indicate message format or IPC protocol violations.
    #[error("Protocol failure: {0}")]
    Protocol(#[from] ProtocolError),

    /// System-level errors (IO, OS).
    ///
    /// Wrapper for underlying system errors.
    #[error("System error: {0}")]
    System(#[from] std::io::Error),

    /// Generic errors for edge cases.
    ///
    /// Use sparingly; prefer specific error variants.
    #[error("{0}")]
    Other(String),
}

impl TachError {
    /// Check if this error is retryable.
    ///
    /// Transient failures (jitter, timing) may succeed on retry.
    pub fn is_retryable(&self) -> bool {
        match self {
            TachError::Restoration(e) => e.is_retryable(),
            TachError::Isolation(e) => e.is_retryable(),
            TachError::Teleportation(e) => e.is_retryable(),
            TachError::Scheduler(e) => e.is_retryable(),
            _ => false,
        }
    }

    /// Check if this error requires worker termination.
    ///
    /// Corruption or desync errors require killing the worker.
    pub fn requires_kill(&self) -> bool {
        match self {
            TachError::Restoration(e) => e.requires_kill(),
            TachError::Calibration(_) => true, // Calibration failures are fatal
            _ => false,
        }
    }

    /// Check if this error allows graceful degradation.
    ///
    /// Some isolation failures (missing kernel features) allow degraded mode.
    pub fn allows_degradation(&self) -> bool {
        match self {
            TachError::Isolation(e) => e.allows_degradation(),
            _ => false,
        }
    }
}

// =============================================================================
// Restoration Errors (Memory/TLS Synchronization)
// =============================================================================

/// Errors during memory or TLS restoration.
///
/// The Restoration Quadrant consists of four regions:
/// 1. **TCB** (Thread Control Block) - pthread structures
/// 2. **BSS** (.data/.bss) - Python allocator freelists
/// 3. **Heap** - PyObject graph
/// 4. **Stack** - Call frames and local variables
///
/// Any desync between these regions after restore indicates corruption.
#[derive(Error, Debug)]
pub enum RestorationError {
    /// Heap checksum mismatch after restore.
    #[error("Heap desync: expected {expected_checksum:#X}, got {actual_checksum:#X}")]
    HeapDesync {
        expected_checksum: u64,
        actual_checksum: u64,
    },

    /// BSS region mismatch (Python allocator freelists).
    #[error("BSS desync: PyFloat_FreeList corruption detected")]
    BssDesync,

    /// TLS region restoration failed.
    #[error("TLS restoration failed: {reason}")]
    TlsFailed { reason: String },

    /// Stack restoration via ptrace failed.
    #[error("Stack restoration failed: ptrace error {errno}")]
    StackFailed { errno: i32 },

    /// userfaultfd page fault handling failed.
    #[error("UFFD fault handling failed: {reason}")]
    UffdFault { reason: String },

    /// Snapshot capture failed.
    #[error("Snapshot capture failed: {reason}")]
    SnapshotFailed { reason: String },

    /// Memory region not found in /proc/pid/maps.
    #[error("Memory region not found: {region}")]
    RegionNotFound { region: String },

    /// process_vm_readv/writev failed.
    #[error("Cross-process memory operation failed: {operation}")]
    ProcessVmFailed { operation: String },

    /// Jemalloc quiesce failed.
    #[error("Allocator quiesce failed: {reason}")]
    AllocatorQuiesceFailed { reason: String },

    /// Ghost object detected (memory leak after restore).
    #[error("Ghost object detected: RSS grew by {bytes} bytes after {cycles} cycles")]
    GhostObject { bytes: usize, cycles: usize },

    /// DTV (Dynamic Thread Vector) counter mismatch.
    #[error("DTV generation mismatch: expected {expected}, got {actual}")]
    DtvMismatch { expected: u64, actual: u64 },
}

impl RestorationError {
    /// Check if this restoration error is retryable.
    pub fn is_retryable(&self) -> bool {
        matches!(self, RestorationError::UffdFault { .. })
    }

    /// Check if this error requires worker termination.
    pub fn requires_kill(&self) -> bool {
        matches!(
            self,
            RestorationError::HeapDesync { .. }
                | RestorationError::BssDesync
                | RestorationError::GhostObject { .. }
                | RestorationError::DtvMismatch { .. }
        )
    }
}

// =============================================================================
// Isolation Errors (Landlock/Seccomp/Namespace)
// =============================================================================

/// Errors during sandbox isolation setup.
///
/// The Iron Dome consists of three layers:
/// 1. **Landlock** - Filesystem sandboxing
/// 2. **Seccomp** - Syscall filtering
/// 3. **Namespaces** - Process isolation (mount, network, PID)
#[derive(Error, Debug)]
pub enum IsolationError {
    /// Landlock ruleset creation failed.
    #[error("Landlock ruleset creation failed: {reason}")]
    LandlockRulesetFailed { reason: String },

    /// Landlock path rule addition failed.
    #[error("Landlock path rule failed for {path}: {reason}")]
    LandlockPathFailed { path: PathBuf, reason: String },

    /// Landlock enforcement failed.
    #[error("Landlock enforcement failed: {reason}")]
    LandlockEnforceFailed { reason: String },

    /// Landlock not available on this kernel.
    #[error("Landlock unavailable: kernel {kernel_version} < 5.13")]
    LandlockUnavailable { kernel_version: String },

    /// Seccomp filter installation failed.
    #[error("Seccomp filter installation failed: {reason}")]
    SeccompFailed { reason: String },

    /// Seccomp not available.
    #[error("Seccomp unavailable: {reason}")]
    SeccompUnavailable { reason: String },

    /// Namespace unshare failed.
    #[error("Namespace unshare failed ({namespace}): {reason}")]
    NamespaceFailed { namespace: String, reason: String },

    /// CAP_SYS_ADMIN required but not available.
    #[error("CAP_SYS_ADMIN required for namespace isolation")]
    CapabilityMissing,

    /// OverlayFS mount failed.
    #[error("OverlayFS mount failed: {reason}")]
    OverlayFsFailed { reason: String },

    /// Network namespace isolation failed.
    #[error("Network isolation failed: {reason}")]
    NetworkIsolationFailed { reason: String },
}

impl IsolationError {
    /// Check if this isolation error is retryable.
    pub fn is_retryable(&self) -> bool {
        false // Isolation failures are configuration issues
    }

    /// Check if this error allows graceful degradation.
    ///
    /// Missing kernel features (Landlock < 5.13, Seccomp disabled)
    /// allow running with reduced isolation.
    pub fn allows_degradation(&self) -> bool {
        matches!(
            self,
            IsolationError::LandlockUnavailable { .. }
                | IsolationError::SeccompUnavailable { .. }
                | IsolationError::CapabilityMissing
        )
    }
}

// =============================================================================
// Calibration Errors (Sentinel Scan/Offset Discovery)
// =============================================================================

/// Errors during TLS calibration and offset discovery.
///
/// Calibration is required for Python 3.13+ (mimalloc/free-threaded builds)
/// where TLS layout is not deterministic.
#[derive(Error, Debug)]
pub enum CalibrationError {
    /// Sentinel pattern not found in TLS region.
    #[error("Sentinel scan failed: pattern {pattern:#X} not found in TLS")]
    SentinelNotFound { pattern: u64 },

    /// Multiple sentinel matches found (ambiguous).
    #[error("Sentinel scan ambiguous: {count} matches found")]
    SentinelAmbiguous { count: usize },

    /// TLS region bounds detection failed.
    #[error("TLS bounds detection failed: {reason}")]
    TlsBoundsDetectionFailed { reason: String },

    /// fs_base register read failed.
    #[error("fs_base read failed: {reason}")]
    FsBaseReadFailed { reason: String },

    /// Calibration data stale (Python version changed).
    #[error("Calibration stale: expected Python {expected}, got {actual}")]
    CalibrationStale { expected: String, actual: String },

    /// mimalloc heap offset discovery failed.
    #[error("mimalloc heap offset discovery failed: {reason}")]
    MimallocOffsetFailed { reason: String },
}

// =============================================================================
// Teleportation Errors (SCM_RIGHTS/FD Adoption)
// =============================================================================

/// Errors during file descriptor teleportation.
///
/// The FD Teleporter uses SCM_RIGHTS to pass file descriptors from
/// the supervisor to workers, enabling database connection sharing.
#[derive(Error, Debug)]
pub enum TeleportationError {
    /// SCM_RIGHTS sendmsg failed.
    #[error("SCM_RIGHTS send failed: {reason}")]
    SendFailed { reason: String },

    /// SCM_RIGHTS recvmsg failed.
    #[error("SCM_RIGHTS receive failed: {reason}")]
    ReceiveFailed { reason: String },

    /// FD adoption (dup2) failed.
    #[error("FD adoption failed: dup2({source_fd}, {target_fd}) = {errno}")]
    AdoptionFailed {
        source_fd: i32,
        target_fd: i32,
        errno: i32,
    },

    /// Socket pair creation failed.
    #[error("Teleporter socket creation failed: {reason}")]
    SocketCreationFailed { reason: String },

    /// FD type mismatch (expected socket, got file).
    #[error("FD type mismatch: expected {expected}, got {actual}")]
    TypeMismatch { expected: String, actual: String },

    /// FD already closed (ghost close prevention failed).
    #[error("FD {fd} already closed (ghost close detected)")]
    GhostClose { fd: i32 },

    /// Database connection validation failed after teleport.
    #[error("DB connection validation failed: {reason}")]
    DbValidationFailed { reason: String },

    /// Too many FDs to teleport in single message.
    #[error("FD batch too large: {count} FDs (max {max})")]
    BatchTooLarge { count: usize, max: usize },
}

impl TeleportationError {
    /// Check if this teleportation error is retryable.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            TeleportationError::SendFailed { .. } | TeleportationError::ReceiveFailed { .. }
        )
    }
}

// =============================================================================
// Discovery Errors (Test Collection/Resolution)
// =============================================================================

/// Errors during test discovery and fixture resolution.
#[derive(Error, Debug)]
pub enum DiscoveryError {
    /// Test file parsing failed.
    #[error("Failed to parse {path}: {reason}")]
    ParseFailed { path: PathBuf, reason: String },

    /// Fixture not found.
    #[error("Fixture '{fixture}' not found for test '{test}'")]
    MissingFixture { test: String, fixture: String },

    /// Circular fixture dependency detected.
    #[error("Circular dependency in fixture '{fixture}': {cycle:?}")]
    CyclicDependency { fixture: String, cycle: Vec<String> },

    /// Toxicity analysis failed.
    #[error("Toxicity analysis failed for {path}: {reason}")]
    ToxicityAnalysisFailed { path: PathBuf, reason: String },

    /// No tests found matching filter.
    #[error("No tests found matching '{filter}'")]
    NoTestsFound { filter: String },
}

// =============================================================================
// Scheduler Errors (Worker Pool/IPC)
// =============================================================================

/// Errors in the scheduler and worker pool.
#[derive(Error, Debug)]
pub enum SchedulerError {
    /// Worker process crashed.
    #[error("Worker {pid} crashed with signal {signal}")]
    WorkerCrashed { pid: i32, signal: i32 },

    /// Worker timed out.
    #[error("Worker {pid} timed out after {timeout_ms}ms")]
    WorkerTimeout { pid: i32, timeout_ms: u64 },

    /// IPC channel broken.
    #[error("IPC channel to worker {pid} broken: {reason}")]
    IpcBroken { pid: i32, reason: String },

    /// Zygote process died.
    #[error("Zygote process died unexpectedly")]
    ZygoteDied,

    /// No available workers in pool.
    #[error("Worker pool exhausted")]
    PoolExhausted,

    /// Worker handshake failed.
    #[error("Worker {pid} handshake failed: {reason}")]
    HandshakeFailed { pid: i32, reason: String },

    /// Result deserialization failed.
    #[error("Failed to deserialize result from worker {pid}: {reason}")]
    ResultDeserializationFailed { pid: i32, reason: String },
}

impl SchedulerError {
    /// Check if this scheduler error is retryable.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            SchedulerError::WorkerTimeout { .. } | SchedulerError::PoolExhausted
        )
    }
}

// =============================================================================
// Protocol Errors (Message Format/IPC)
// =============================================================================

/// Errors in message serialization and protocol handling.
#[derive(Error, Debug)]
pub enum ProtocolError {
    /// Invalid message magic number.
    #[error("Invalid message magic: expected {expected:#X}, got {actual:#X}")]
    InvalidMagic { expected: u32, actual: u32 },

    /// Message too large.
    #[error("Message too large: {size} bytes (max {max})")]
    MessageTooLarge { size: usize, max: usize },

    /// Unexpected message type.
    #[error("Unexpected message type: {msg_type}")]
    UnexpectedMessageType { msg_type: String },

    /// Serialization failed.
    #[error("Serialization failed: {reason}")]
    SerializationFailed { reason: String },

    /// Deserialization failed.
    #[error("Deserialization failed: {reason}")]
    DeserializationFailed { reason: String },

    /// Protocol version mismatch.
    #[error("Protocol version mismatch: expected {expected}, got {actual}")]
    VersionMismatch { expected: u32, actual: u32 },
}

// =============================================================================
// Result Type Alias
// =============================================================================

/// Result type for Tach operations.
pub type TachResult<T> = Result<T, TachError>;

// =============================================================================
// Conversion Helpers
// =============================================================================

impl From<String> for TachError {
    fn from(s: String) -> Self {
        TachError::Other(s)
    }
}

impl From<&str> for TachError {
    fn from(s: &str) -> Self {
        TachError::Other(s.to_string())
    }
}

impl From<nix::Error> for TachError {
    fn from(e: nix::Error) -> Self {
        TachError::System(std::io::Error::from_raw_os_error(e as i32))
    }
}

// =============================================================================
// Error Categorization (User-Facing Error Codes)
// =============================================================================

/// Error category for user-facing error messages.
///
/// Categorization helps users understand whether an error is:
/// - Something they can fix (User errors)
/// - A system/environment issue (System errors)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCategory {
    /// User errors: Test failures, import errors, fixture errors.
    /// These are typically fixable by modifying test code or configuration.
    User,

    /// System errors: Kernel issues, permissions, OOM.
    /// These require system-level fixes (kernel config, permissions, resources).
    System,
}

impl fmt::Display for ErrorCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ErrorCategory::User => write!(f, "User Error"),
            ErrorCategory::System => write!(f, "System Error"),
        }
    }
}

// =============================================================================
// Error Codes Registry
// =============================================================================

/// Error code constants for categorized errors.
///
/// Error codes follow the pattern `E###` where:
/// - E001-E004, E010: User errors (test code, configuration)
/// - E005-E009: System errors (kernel, permissions, resources)
/// - E011+: Reserved for future error codes
pub mod error_codes {
    /// Test assertion failed.
    pub const E001: &str = "E001";

    /// Import error in test file.
    pub const E002: &str = "E002";

    /// Fixture not found.
    pub const E003: &str = "E003";

    /// Invalid marker expression.
    pub const E004: &str = "E004";

    /// userfaultfd not available.
    pub const E005: &str = "E005";

    /// Landlock not supported.
    pub const E006: &str = "E006";

    /// Permission denied.
    pub const E007: &str = "E007";

    /// Out of memory.
    pub const E008: &str = "E008";

    /// Too many open files.
    pub const E009: &str = "E009";

    /// Timeout exceeded.
    pub const E010: &str = "E010";
}

/// Categorized error with error code for user-facing output.
///
/// This struct provides a standardized format for displaying errors to users
/// with actionable information:
/// - Error code for quick identification and documentation lookup
/// - Category to indicate who can fix it (user vs system admin)
/// - Clear message describing what went wrong
/// - Optional suggestion for how to fix it
///
/// # Example Output
///
/// ```text
/// [E005] System Error: userfaultfd not available
///   Hint: Set vm.unprivileged_userfaultfd=1 or run with CAP_SYS_PTRACE
/// ```
#[derive(Debug, Clone)]
pub struct CategorizedError {
    /// Error code (e.g., "E001", "E005").
    pub code: &'static str,

    /// Category of the error (User or System).
    pub category: ErrorCategory,

    /// Human-readable error message.
    pub message: String,

    /// Optional suggestion for how to fix the error.
    pub suggestion: Option<String>,
}

impl fmt::Display for CategorizedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}: {}", self.code, self.category, self.message)?;
        if let Some(ref hint) = self.suggestion {
            write!(f, "\n  Hint: {}", hint)?;
        }
        Ok(())
    }
}

impl CategorizedError {
    /// Create a new categorized error.
    pub fn new(
        code: &'static str,
        category: ErrorCategory,
        message: impl Into<String>,
        suggestion: Option<String>,
    ) -> Self {
        Self {
            code,
            category,
            message: message.into(),
            suggestion,
        }
    }

    // =========================================================================
    // User Error Constructors (E001-E004, E010)
    // =========================================================================

    /// E001: Test assertion failed.
    pub fn assertion_failed(test_name: &str, reason: &str) -> Self {
        Self::new(
            error_codes::E001,
            ErrorCategory::User,
            format!("Test '{}' assertion failed: {}", test_name, reason),
            Some("Check the test assertions and expected values".to_string()),
        )
    }

    /// E002: Import error in test file.
    pub fn import_error(path: &str, module: &str, reason: &str) -> Self {
        Self::new(
            error_codes::E002,
            ErrorCategory::User,
            format!("Failed to import '{}' in {}: {}", module, path, reason),
            Some("Ensure the module is installed and the import path is correct".to_string()),
        )
    }

    /// E003: Fixture not found.
    pub fn fixture_not_found(test_name: &str, fixture_name: &str) -> Self {
        Self::new(
            error_codes::E003,
            ErrorCategory::User,
            format!(
                "Fixture '{}' not found for test '{}'",
                fixture_name, test_name
            ),
            Some(
                "Define the fixture in conftest.py or the test file, or check for typos"
                    .to_string(),
            ),
        )
    }

    /// E004: Invalid marker expression.
    pub fn invalid_marker(expression: &str, reason: &str) -> Self {
        Self::new(
            error_codes::E004,
            ErrorCategory::User,
            format!("Invalid marker expression '{}': {}", expression, reason),
            Some("Check the marker syntax. Example: -m 'slow and not integration'".to_string()),
        )
    }

    /// E010: Timeout exceeded.
    pub fn timeout_exceeded(test_name: &str, timeout_secs: u64) -> Self {
        Self::new(
            error_codes::E010,
            ErrorCategory::User,
            format!(
                "Test '{}' timed out after {} seconds",
                test_name, timeout_secs
            ),
            Some(
                "Increase the timeout with @pytest.mark.timeout(N) or optimize the test"
                    .to_string(),
            ),
        )
    }

    // =========================================================================
    // System Error Constructors (E005-E009)
    // =========================================================================

    /// E005: userfaultfd not available.
    pub fn userfaultfd_unavailable(reason: &str) -> Self {
        Self::new(
            error_codes::E005,
            ErrorCategory::System,
            format!("userfaultfd not available: {}", reason),
            Some("Set vm.unprivileged_userfaultfd=1 or run with CAP_SYS_PTRACE".to_string()),
        )
    }

    /// E006: Landlock not supported.
    pub fn landlock_unavailable(kernel_version: &str) -> Self {
        Self::new(
            error_codes::E006,
            ErrorCategory::System,
            format!("Landlock not supported on kernel {}", kernel_version),
            Some(
                "Landlock requires kernel 5.13+. Running without filesystem isolation.".to_string(),
            ),
        )
    }

    /// E007: Permission denied.
    pub fn permission_denied(operation: &str, path: Option<&str>) -> Self {
        let message = if let Some(p) = path {
            format!("Permission denied: {} on '{}'", operation, p)
        } else {
            format!("Permission denied: {}", operation)
        };
        Self::new(
            error_codes::E007,
            ErrorCategory::System,
            message,
            Some("Check file permissions or run with appropriate privileges".to_string()),
        )
    }

    /// E008: Out of memory.
    pub fn out_of_memory(context: &str) -> Self {
        Self::new(
            error_codes::E008,
            ErrorCategory::System,
            format!("Out of memory: {}", context),
            Some("Reduce worker count with -n or increase system memory".to_string()),
        )
    }

    /// E009: Too many open files.
    pub fn too_many_files(current: Option<u64>, required: Option<u64>) -> Self {
        let message = match (current, required) {
            (Some(c), Some(r)) => format!(
                "Too many open files: current limit {}, need at least {}",
                c, r
            ),
            _ => "Too many open files".to_string(),
        };
        Self::new(
            error_codes::E009,
            ErrorCategory::System,
            message,
            Some("Increase ulimit with: ulimit -n 65536".to_string()),
        )
    }

    // =========================================================================
    // Conversion from TachError
    // =========================================================================

    /// Convert a TachError to a categorized error for user display.
    ///
    /// This provides a user-friendly view of internal errors with
    /// actionable suggestions.
    pub fn from_tach_error(err: &TachError) -> Self {
        match err {
            TachError::Restoration(e) => Self::new(
                error_codes::E008,
                ErrorCategory::System,
                format!("Memory restoration failed: {}", e),
                Some("This is an internal error. Please report a bug.".to_string()),
            ),

            TachError::Isolation(IsolationError::LandlockUnavailable { kernel_version }) => {
                Self::landlock_unavailable(kernel_version)
            }

            TachError::Isolation(IsolationError::CapabilityMissing) => Self::new(
                error_codes::E007,
                ErrorCategory::System,
                "CAP_SYS_ADMIN required for namespace isolation".to_string(),
                Some(
                    "Run with elevated privileges or in a container with --privileged".to_string(),
                ),
            ),

            TachError::Isolation(e) => Self::new(
                error_codes::E007,
                ErrorCategory::System,
                format!("Isolation setup failed: {}", e),
                Some("Check kernel capabilities and permissions".to_string()),
            ),

            TachError::Discovery(DiscoveryError::MissingFixture { test, fixture }) => {
                Self::fixture_not_found(test, fixture)
            }

            TachError::Discovery(DiscoveryError::ParseFailed { path, reason }) => Self::new(
                error_codes::E002,
                ErrorCategory::User,
                format!("Failed to parse {}: {}", path.display(), reason),
                Some("Check the test file for syntax errors".to_string()),
            ),

            TachError::Discovery(e) => Self::new(
                error_codes::E002,
                ErrorCategory::User,
                format!("Test discovery failed: {}", e),
                None,
            ),

            TachError::Scheduler(SchedulerError::WorkerTimeout { pid, timeout_ms }) => Self::new(
                error_codes::E010,
                ErrorCategory::User,
                format!("Worker {} timed out after {}ms", pid, timeout_ms),
                Some("Increase timeout or check for infinite loops".to_string()),
            ),

            TachError::Scheduler(e) => Self::new(
                error_codes::E008,
                ErrorCategory::System,
                format!("Scheduler error: {}", e),
                Some("Check system resources and try reducing worker count".to_string()),
            ),

            TachError::System(e) => {
                // Map common IO errors to specific codes
                match e.kind() {
                    std::io::ErrorKind::PermissionDenied => {
                        Self::permission_denied(&e.to_string(), None)
                    }
                    std::io::ErrorKind::OutOfMemory => Self::out_of_memory(&e.to_string()),
                    _ => Self::new(
                        error_codes::E007,
                        ErrorCategory::System,
                        format!("System error: {}", e),
                        None,
                    ),
                }
            }

            TachError::Other(msg) => {
                Self::new(error_codes::E001, ErrorCategory::User, msg.clone(), None)
            }

            TachError::Calibration(e) => Self::new(
                error_codes::E008,
                ErrorCategory::System,
                format!("Calibration failed: {}", e),
                Some("This is an internal error. Please report a bug.".to_string()),
            ),

            TachError::Teleportation(e) => Self::new(
                error_codes::E008,
                ErrorCategory::System,
                format!("FD teleportation failed: {}", e),
                Some("This is an internal error. Please report a bug.".to_string()),
            ),

            TachError::Protocol(e) => Self::new(
                error_codes::E008,
                ErrorCategory::System,
                format!("Protocol error: {}", e),
                Some("This is an internal error. Please report a bug.".to_string()),
            ),
        }
    }

    /// Check if this is a user error (something the user can fix).
    pub fn is_user_error(&self) -> bool {
        self.category == ErrorCategory::User
    }

    /// Check if this is a system error (requires system-level fix).
    pub fn is_system_error(&self) -> bool {
        self.category == ErrorCategory::System
    }

    /// Get the error code.
    pub fn code(&self) -> &'static str {
        self.code
    }
}

// =============================================================================
// Unit Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_restoration_error_requires_kill() {
        let heap_desync = RestorationError::HeapDesync {
            expected_checksum: 0xDEADBEEF,
            actual_checksum: 0xCAFEBABE,
        };
        assert!(heap_desync.requires_kill());

        let uffd_fault = RestorationError::UffdFault {
            reason: "page not present".to_string(),
        };
        assert!(!uffd_fault.requires_kill());
    }

    #[test]
    fn test_isolation_error_allows_degradation() {
        let landlock_unavail = IsolationError::LandlockUnavailable {
            kernel_version: "5.10".to_string(),
        };
        assert!(landlock_unavail.allows_degradation());

        let landlock_failed = IsolationError::LandlockRulesetFailed {
            reason: "ENOMEM".to_string(),
        };
        assert!(!landlock_failed.allows_degradation());
    }

    #[test]
    fn test_tach_error_is_retryable() {
        let err = TachError::Scheduler(SchedulerError::WorkerTimeout {
            pid: 1234,
            timeout_ms: 5000,
        });
        assert!(err.is_retryable());

        let err = TachError::Calibration(CalibrationError::SentinelNotFound {
            pattern: 0xDEADC0DE,
        });
        assert!(!err.is_retryable());
    }

    #[test]
    fn test_tach_error_requires_kill() {
        let err = TachError::Restoration(RestorationError::BssDesync);
        assert!(err.requires_kill());

        let err = TachError::Isolation(IsolationError::CapabilityMissing);
        assert!(!err.requires_kill());
    }

    #[test]
    fn test_error_display() {
        let err = TachError::Teleportation(TeleportationError::AdoptionFailed {
            source_fd: 5,
            target_fd: 3,
            errno: 9,
        });
        let msg = format!("{}", err);
        assert!(msg.contains("dup2(5, 3)"));
        assert!(msg.contains("9"));
    }

    // =========================================================================
    // Error Categorization Tests
    // =========================================================================

    #[test]
    fn test_error_category_display() {
        assert_eq!(format!("{}", ErrorCategory::User), "User Error");
        assert_eq!(format!("{}", ErrorCategory::System), "System Error");
    }

    #[test]
    fn test_categorized_error_display_with_hint() {
        let err = CategorizedError::userfaultfd_unavailable("EPERM");
        let display = format!("{}", err);
        assert!(display.contains("[E005]"));
        assert!(display.contains("System Error"));
        assert!(display.contains("userfaultfd not available"));
        assert!(display.contains("Hint:"));
        assert!(display.contains("vm.unprivileged_userfaultfd=1"));
    }

    #[test]
    fn test_categorized_error_display_without_hint() {
        let err =
            CategorizedError::new(error_codes::E001, ErrorCategory::User, "Test failed", None);
        let display = format!("{}", err);
        assert!(display.contains("[E001]"));
        assert!(display.contains("User Error"));
        assert!(display.contains("Test failed"));
        assert!(!display.contains("Hint:"));
    }

    #[test]
    fn test_user_error_constructors() {
        // E001: Assertion failed
        let err = CategorizedError::assertion_failed("test_foo", "expected 1, got 2");
        assert_eq!(err.code, error_codes::E001);
        assert!(err.is_user_error());
        assert!(!err.is_system_error());
        assert!(err.message.contains("test_foo"));

        // E002: Import error
        let err = CategorizedError::import_error("test_bar.py", "numpy", "No module named numpy");
        assert_eq!(err.code, error_codes::E002);
        assert!(err.is_user_error());

        // E003: Fixture not found
        let err = CategorizedError::fixture_not_found("test_baz", "db_connection");
        assert_eq!(err.code, error_codes::E003);
        assert!(err.is_user_error());

        // E004: Invalid marker
        let err = CategorizedError::invalid_marker("slow and", "unexpected end of expression");
        assert_eq!(err.code, error_codes::E004);
        assert!(err.is_user_error());

        // E010: Timeout
        let err = CategorizedError::timeout_exceeded("test_slow", 30);
        assert_eq!(err.code, error_codes::E010);
        assert!(err.is_user_error());
    }

    #[test]
    fn test_system_error_constructors() {
        // E005: userfaultfd unavailable
        let err = CategorizedError::userfaultfd_unavailable("EPERM");
        assert_eq!(err.code, error_codes::E005);
        assert!(err.is_system_error());
        assert!(!err.is_user_error());

        // E006: Landlock unavailable
        let err = CategorizedError::landlock_unavailable("5.10");
        assert_eq!(err.code, error_codes::E006);
        assert!(err.is_system_error());

        // E007: Permission denied
        let err = CategorizedError::permission_denied("read", Some("/etc/shadow"));
        assert_eq!(err.code, error_codes::E007);
        assert!(err.is_system_error());
        assert!(err.message.contains("/etc/shadow"));

        // E007: Permission denied without path
        let err = CategorizedError::permission_denied("mount", None);
        assert_eq!(err.code, error_codes::E007);
        assert!(!err.message.contains("'"));

        // E008: Out of memory
        let err = CategorizedError::out_of_memory("worker allocation");
        assert_eq!(err.code, error_codes::E008);
        assert!(err.is_system_error());

        // E009: Too many files
        let err = CategorizedError::too_many_files(Some(1024), Some(4096));
        assert_eq!(err.code, error_codes::E009);
        assert!(err.is_system_error());
        assert!(err.message.contains("1024"));
        assert!(err.message.contains("4096"));
    }

    #[test]
    fn test_categorized_error_from_tach_error() {
        // Test conversion from IsolationError::LandlockUnavailable
        let tach_err = TachError::Isolation(IsolationError::LandlockUnavailable {
            kernel_version: "5.10".to_string(),
        });
        let cat_err = CategorizedError::from_tach_error(&tach_err);
        assert_eq!(cat_err.code, error_codes::E006);
        assert!(cat_err.is_system_error());

        // Test conversion from DiscoveryError::MissingFixture
        let tach_err = TachError::Discovery(DiscoveryError::MissingFixture {
            test: "test_foo".to_string(),
            fixture: "db".to_string(),
        });
        let cat_err = CategorizedError::from_tach_error(&tach_err);
        assert_eq!(cat_err.code, error_codes::E003);
        assert!(cat_err.is_user_error());

        // Test conversion from SchedulerError::WorkerTimeout
        let tach_err = TachError::Scheduler(SchedulerError::WorkerTimeout {
            pid: 1234,
            timeout_ms: 5000,
        });
        let cat_err = CategorizedError::from_tach_error(&tach_err);
        assert_eq!(cat_err.code, error_codes::E010);
        assert!(cat_err.is_user_error());
    }

    #[test]
    fn test_error_codes_values() {
        assert_eq!(error_codes::E001, "E001");
        assert_eq!(error_codes::E002, "E002");
        assert_eq!(error_codes::E003, "E003");
        assert_eq!(error_codes::E004, "E004");
        assert_eq!(error_codes::E005, "E005");
        assert_eq!(error_codes::E006, "E006");
        assert_eq!(error_codes::E007, "E007");
        assert_eq!(error_codes::E008, "E008");
        assert_eq!(error_codes::E009, "E009");
        assert_eq!(error_codes::E010, "E010");
    }
}
