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
}
