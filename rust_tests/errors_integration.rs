//! Error Types Integration Tests
//!
//! These tests verify the error type hierarchy, conversion traits,
//! and error handling behavior.

use std::path::PathBuf;
use tach_core::errors::{
    CalibrationError, DiscoveryError, IsolationError, ProtocolError, RestorationError,
    SchedulerError, TachError, TeleportationError,
};

// =============================================================================
// RestorationError Tests
// =============================================================================

#[test]
fn test_restoration_error_heap_desync() {
    let err = RestorationError::HeapDesync {
        expected_checksum: 0xDEADBEEF,
        actual_checksum: 0xCAFEBABE,
    };

    assert!(err.requires_kill());
    assert!(!err.is_retryable());

    let msg = format!("{}", err);
    assert!(msg.contains("DEADBEEF"));
    assert!(msg.contains("CAFEBABE"));
}

#[test]
fn test_restoration_error_bss_desync() {
    let err = RestorationError::BssDesync;

    assert!(err.requires_kill());
    assert!(!err.is_retryable());
}

#[test]
fn test_restoration_error_tls_failed() {
    let err = RestorationError::TlsFailed {
        reason: "offset out of bounds".to_string(),
    };

    assert!(!err.requires_kill());
    assert!(!err.is_retryable());

    let msg = format!("{}", err);
    assert!(msg.contains("offset out of bounds"));
}

#[test]
fn test_restoration_error_stack_failed() {
    let err = RestorationError::StackFailed { errno: 3 };

    assert!(!err.requires_kill());
    assert!(!err.is_retryable());

    let msg = format!("{}", err);
    assert!(msg.contains("3"));
}

#[test]
fn test_restoration_error_uffd_fault() {
    let err = RestorationError::UffdFault {
        reason: "page not present".to_string(),
    };

    assert!(!err.requires_kill());
    assert!(err.is_retryable()); // UFFD faults are retryable
}

#[test]
fn test_restoration_error_snapshot_failed() {
    let err = RestorationError::SnapshotFailed {
        reason: "mmap failed".to_string(),
    };

    assert!(!err.requires_kill());
    assert!(!err.is_retryable());
}

#[test]
fn test_restoration_error_region_not_found() {
    let err = RestorationError::RegionNotFound {
        region: "[heap]".to_string(),
    };

    assert!(!err.requires_kill());
    let msg = format!("{}", err);
    assert!(msg.contains("[heap]"));
}

#[test]
fn test_restoration_error_process_vm_failed() {
    let err = RestorationError::ProcessVmFailed {
        operation: "readv".to_string(),
    };

    assert!(!err.requires_kill());
    let msg = format!("{}", err);
    assert!(msg.contains("readv"));
}

#[test]
fn test_restoration_error_allocator_quiesce_failed() {
    let err = RestorationError::AllocatorQuiesceFailed {
        reason: "arena locked".to_string(),
    };

    assert!(!err.requires_kill());
}

#[test]
fn test_restoration_error_ghost_object() {
    let err = RestorationError::GhostObject {
        bytes: 4096,
        cycles: 10,
    };

    assert!(err.requires_kill()); // Ghost objects require kill
    let msg = format!("{}", err);
    assert!(msg.contains("4096"));
    assert!(msg.contains("10"));
}

#[test]
fn test_restoration_error_dtv_mismatch() {
    let err = RestorationError::DtvMismatch {
        expected: 5,
        actual: 7,
    };

    assert!(err.requires_kill()); // DTV mismatch requires kill
}

// =============================================================================
// IsolationError Tests
// =============================================================================

#[test]
fn test_isolation_error_landlock_unavailable() {
    let err = IsolationError::LandlockUnavailable {
        kernel_version: "5.10".to_string(),
    };

    assert!(err.allows_degradation());
    assert!(!err.is_retryable());

    let msg = format!("{}", err);
    assert!(msg.contains("5.10"));
}

#[test]
fn test_isolation_error_landlock_ruleset_failed() {
    let err = IsolationError::LandlockRulesetFailed {
        reason: "ENOMEM".to_string(),
    };

    assert!(!err.allows_degradation());
}

#[test]
fn test_isolation_error_landlock_path_failed() {
    let err = IsolationError::LandlockPathFailed {
        path: PathBuf::from("/tmp"),
        reason: "ENOENT".to_string(),
    };

    assert!(!err.allows_degradation());
    let msg = format!("{}", err);
    assert!(msg.contains("/tmp"));
}

#[test]
fn test_isolation_error_landlock_enforce_failed() {
    let err = IsolationError::LandlockEnforceFailed {
        reason: "prctl failed".to_string(),
    };

    assert!(!err.allows_degradation());
}

#[test]
fn test_isolation_error_seccomp_failed() {
    let err = IsolationError::SeccompFailed {
        reason: "filter too large".to_string(),
    };

    assert!(!err.allows_degradation());
    let msg = format!("{}", err);
    assert!(msg.contains("filter too large"));
}

#[test]
fn test_isolation_error_seccomp_unavailable() {
    let err = IsolationError::SeccompUnavailable {
        reason: "CONFIG_SECCOMP not enabled".to_string(),
    };

    assert!(err.allows_degradation()); // Seccomp unavailable allows degradation
}

#[test]
fn test_isolation_error_namespace_failed() {
    let err = IsolationError::NamespaceFailed {
        namespace: "mount".to_string(),
        reason: "EPERM".to_string(),
    };

    assert!(!err.allows_degradation());
    let msg = format!("{}", err);
    assert!(msg.contains("mount"));
    assert!(msg.contains("EPERM"));
}

#[test]
fn test_isolation_error_capability_missing() {
    let err = IsolationError::CapabilityMissing;

    assert!(err.allows_degradation()); // Capability missing allows degradation
}

#[test]
fn test_isolation_error_overlayfs_failed() {
    let err = IsolationError::OverlayFsFailed {
        reason: "no workdir".to_string(),
    };

    assert!(!err.allows_degradation());
}

#[test]
fn test_isolation_error_network_isolation_failed() {
    let err = IsolationError::NetworkIsolationFailed {
        reason: "setns failed".to_string(),
    };

    assert!(!err.allows_degradation());
}

// =============================================================================
// CalibrationError Tests
// =============================================================================

#[test]
fn test_calibration_error_sentinel_not_found() {
    let err = CalibrationError::SentinelNotFound {
        pattern: 0xDEADC0DE,
    };

    let msg = format!("{}", err);
    assert!(msg.contains("DEADC0DE"));
}

#[test]
fn test_calibration_error_sentinel_ambiguous() {
    let err = CalibrationError::SentinelAmbiguous { count: 3 };

    let msg = format!("{}", err);
    assert!(msg.contains("3"));
}

#[test]
fn test_calibration_error_tls_bounds_detection_failed() {
    let err = CalibrationError::TlsBoundsDetectionFailed {
        reason: "no readable pages".to_string(),
    };

    let msg = format!("{}", err);
    assert!(msg.contains("readable pages"));
}

#[test]
fn test_calibration_error_fs_base_read_failed() {
    let err = CalibrationError::FsBaseReadFailed {
        reason: "ENOSYS".to_string(),
    };

    let msg = format!("{}", err);
    assert!(msg.contains("ENOSYS"));
}

#[test]
fn test_calibration_error_calibration_stale() {
    let err = CalibrationError::CalibrationStale {
        expected: "3.12".to_string(),
        actual: "3.13".to_string(),
    };

    let msg = format!("{}", err);
    assert!(msg.contains("3.12"));
    assert!(msg.contains("3.13"));
}

#[test]
fn test_calibration_error_mimalloc_offset_failed() {
    let err = CalibrationError::MimallocOffsetFailed {
        reason: "heap not initialized".to_string(),
    };

    let msg = format!("{}", err);
    assert!(msg.contains("heap not initialized"));
}

// =============================================================================
// TeleportationError Tests
// =============================================================================

#[test]
fn test_teleportation_error_send_failed() {
    let err = TeleportationError::SendFailed {
        reason: "EAGAIN".to_string(),
    };

    assert!(err.is_retryable());
    let msg = format!("{}", err);
    assert!(msg.contains("EAGAIN"));
}

#[test]
fn test_teleportation_error_receive_failed() {
    let err = TeleportationError::ReceiveFailed {
        reason: "connection reset".to_string(),
    };

    assert!(err.is_retryable());
}

#[test]
fn test_teleportation_error_adoption_failed() {
    let err = TeleportationError::AdoptionFailed {
        source_fd: 5,
        target_fd: 3,
        errno: 9,
    };

    assert!(!err.is_retryable());
    let msg = format!("{}", err);
    assert!(msg.contains("dup2(5, 3)"));
    assert!(msg.contains("9"));
}

#[test]
fn test_teleportation_error_socket_creation_failed() {
    let err = TeleportationError::SocketCreationFailed {
        reason: "EMFILE".to_string(),
    };

    assert!(!err.is_retryable());
}

#[test]
fn test_teleportation_error_type_mismatch() {
    let err = TeleportationError::TypeMismatch {
        expected: "socket".to_string(),
        actual: "file".to_string(),
    };

    assert!(!err.is_retryable());
    let msg = format!("{}", err);
    assert!(msg.contains("socket"));
    assert!(msg.contains("file"));
}

#[test]
fn test_teleportation_error_ghost_close() {
    let err = TeleportationError::GhostClose { fd: 42 };

    assert!(!err.is_retryable());
    let msg = format!("{}", err);
    assert!(msg.contains("42"));
}

#[test]
fn test_teleportation_error_db_validation_failed() {
    let err = TeleportationError::DbValidationFailed {
        reason: "connection closed".to_string(),
    };

    assert!(!err.is_retryable());
}

#[test]
fn test_teleportation_error_batch_too_large() {
    let err = TeleportationError::BatchTooLarge {
        count: 500,
        max: 253,
    };

    assert!(!err.is_retryable());
    let msg = format!("{}", err);
    assert!(msg.contains("500"));
    assert!(msg.contains("253"));
}

// =============================================================================
// DiscoveryError Tests
// =============================================================================

#[test]
fn test_discovery_error_parse_failed() {
    let err = DiscoveryError::ParseFailed {
        path: PathBuf::from("tests/test_example.py"),
        reason: "SyntaxError".to_string(),
    };

    let msg = format!("{}", err);
    assert!(msg.contains("test_example.py"));
    assert!(msg.contains("SyntaxError"));
}

#[test]
fn test_discovery_error_missing_fixture() {
    let err = DiscoveryError::MissingFixture {
        test: "test_example".to_string(),
        fixture: "db_session".to_string(),
    };

    let msg = format!("{}", err);
    assert!(msg.contains("test_example"));
    assert!(msg.contains("db_session"));
}

#[test]
fn test_discovery_error_cyclic_dependency() {
    let err = DiscoveryError::CyclicDependency {
        fixture: "fixture_a".to_string(),
        cycle: vec![
            "fixture_a".to_string(),
            "fixture_b".to_string(),
            "fixture_a".to_string(),
        ],
    };

    let msg = format!("{}", err);
    assert!(msg.contains("fixture_a"));
    assert!(msg.contains("fixture_b"));
}

#[test]
fn test_discovery_error_toxicity_analysis_failed() {
    let err = DiscoveryError::ToxicityAnalysisFailed {
        path: PathBuf::from("src/module.py"),
        reason: "import error".to_string(),
    };

    let msg = format!("{}", err);
    assert!(msg.contains("module.py"));
}

#[test]
fn test_discovery_error_no_tests_found() {
    let err = DiscoveryError::NoTestsFound {
        filter: "test_nonexistent".to_string(),
    };

    let msg = format!("{}", err);
    assert!(msg.contains("test_nonexistent"));
}

// =============================================================================
// SchedulerError Tests
// =============================================================================

#[test]
fn test_scheduler_error_worker_crashed() {
    let err = SchedulerError::WorkerCrashed {
        pid: 1234,
        signal: 11,
    };

    assert!(!err.is_retryable());
    let msg = format!("{}", err);
    assert!(msg.contains("1234"));
    assert!(msg.contains("11"));
}

#[test]
fn test_scheduler_error_worker_timeout() {
    let err = SchedulerError::WorkerTimeout {
        pid: 5678,
        timeout_ms: 30000,
    };

    assert!(err.is_retryable()); // Timeouts are retryable
    let msg = format!("{}", err);
    assert!(msg.contains("5678"));
    assert!(msg.contains("30000"));
}

#[test]
fn test_scheduler_error_ipc_broken() {
    let err = SchedulerError::IpcBroken {
        pid: 9999,
        reason: "EPIPE".to_string(),
    };

    assert!(!err.is_retryable());
    let msg = format!("{}", err);
    assert!(msg.contains("9999"));
    assert!(msg.contains("EPIPE"));
}

#[test]
fn test_scheduler_error_zygote_died() {
    let err = SchedulerError::ZygoteDied;

    assert!(!err.is_retryable());
}

#[test]
fn test_scheduler_error_pool_exhausted() {
    let err = SchedulerError::PoolExhausted;

    assert!(err.is_retryable()); // Pool exhaustion is retryable
}

#[test]
fn test_scheduler_error_handshake_failed() {
    let err = SchedulerError::HandshakeFailed {
        pid: 1111,
        reason: "timeout".to_string(),
    };

    assert!(!err.is_retryable());
}

#[test]
fn test_scheduler_error_result_deserialization_failed() {
    let err = SchedulerError::ResultDeserializationFailed {
        pid: 2222,
        reason: "invalid format".to_string(),
    };

    assert!(!err.is_retryable());
}

// =============================================================================
// ProtocolError Tests
// =============================================================================

#[test]
fn test_protocol_error_invalid_magic() {
    let err = ProtocolError::InvalidMagic {
        expected: 0x54414348,
        actual: 0xDEADBEEF,
    };

    let msg = format!("{}", err);
    assert!(msg.contains("54414348"));
    assert!(msg.contains("DEADBEEF"));
}

#[test]
fn test_protocol_error_message_too_large() {
    let err = ProtocolError::MessageTooLarge {
        size: 1_000_000,
        max: 65536,
    };

    let msg = format!("{}", err);
    assert!(msg.contains("1000000"));
    assert!(msg.contains("65536"));
}

#[test]
fn test_protocol_error_unexpected_message_type() {
    let err = ProtocolError::UnexpectedMessageType {
        msg_type: "UNKNOWN".to_string(),
    };

    let msg = format!("{}", err);
    assert!(msg.contains("UNKNOWN"));
}

#[test]
fn test_protocol_error_serialization_failed() {
    let err = ProtocolError::SerializationFailed {
        reason: "buffer overflow".to_string(),
    };

    let msg = format!("{}", err);
    assert!(msg.contains("buffer overflow"));
}

#[test]
fn test_protocol_error_deserialization_failed() {
    let err = ProtocolError::DeserializationFailed {
        reason: "unexpected EOF".to_string(),
    };

    let msg = format!("{}", err);
    assert!(msg.contains("unexpected EOF"));
}

#[test]
fn test_protocol_error_version_mismatch() {
    let err = ProtocolError::VersionMismatch {
        expected: 1,
        actual: 2,
    };

    let msg = format!("{}", err);
    assert!(msg.contains("1"));
    assert!(msg.contains("2"));
}

// =============================================================================
// TachError Wrapper Tests
// =============================================================================

#[test]
fn test_tach_error_from_restoration() {
    let inner = RestorationError::BssDesync;
    let err: TachError = inner.into();

    assert!(err.requires_kill());
    assert!(matches!(err, TachError::Restoration(_)));
}

#[test]
fn test_tach_error_from_isolation() {
    let inner = IsolationError::LandlockUnavailable {
        kernel_version: "5.10".to_string(),
    };
    let err: TachError = inner.into();

    assert!(err.allows_degradation());
    assert!(matches!(err, TachError::Isolation(_)));
}

#[test]
fn test_tach_error_from_calibration() {
    let inner = CalibrationError::SentinelNotFound { pattern: 0xDEAD };
    let err: TachError = inner.into();

    assert!(err.requires_kill()); // Calibration failures are fatal
    assert!(matches!(err, TachError::Calibration(_)));
}

#[test]
fn test_tach_error_from_teleportation() {
    let inner = TeleportationError::SendFailed {
        reason: "EAGAIN".to_string(),
    };
    let err: TachError = inner.into();

    assert!(err.is_retryable());
    assert!(matches!(err, TachError::Teleportation(_)));
}

#[test]
fn test_tach_error_from_discovery() {
    let inner = DiscoveryError::NoTestsFound {
        filter: "test_*".to_string(),
    };
    let err: TachError = inner.into();

    assert!(!err.is_retryable());
    assert!(matches!(err, TachError::Discovery(_)));
}

#[test]
fn test_tach_error_from_scheduler() {
    let inner = SchedulerError::WorkerTimeout {
        pid: 1234,
        timeout_ms: 5000,
    };
    let err: TachError = inner.into();

    assert!(err.is_retryable());
    assert!(matches!(err, TachError::Scheduler(_)));
}

#[test]
fn test_tach_error_from_protocol() {
    let inner = ProtocolError::InvalidMagic {
        expected: 0x1234,
        actual: 0x5678,
    };
    let err: TachError = inner.into();

    assert!(!err.is_retryable());
    assert!(matches!(err, TachError::Protocol(_)));
}

#[test]
fn test_tach_error_from_string() {
    let err: TachError = "custom error".into();

    assert!(matches!(err, TachError::Other(_)));
    let msg = format!("{}", err);
    assert!(msg.contains("custom error"));
}

#[test]
fn test_tach_error_from_io_error() {
    let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
    let err: TachError = io_err.into();

    assert!(matches!(err, TachError::System(_)));
}

// =============================================================================
// Error Chain Tests
// =============================================================================

#[test]
fn test_error_chain_display() {
    let err = TachError::Restoration(RestorationError::HeapDesync {
        expected_checksum: 0xAAAA,
        actual_checksum: 0xBBBB,
    });

    let msg = format!("{}", err);
    assert!(msg.contains("Restoration failure"));
    assert!(msg.contains("Heap desync"));
}

#[test]
fn test_error_chain_debug() {
    let err = TachError::Isolation(IsolationError::SeccompFailed {
        reason: "bad filter".to_string(),
    });

    let debug = format!("{:?}", err);
    assert!(debug.contains("SeccompFailed"));
    assert!(debug.contains("bad filter"));
}

#[test]
fn test_error_chain_source() {
    use std::error::Error;

    let err = TachError::Restoration(RestorationError::BssDesync);

    // Error chain is accessible
    let _ = err.source();
}

// =============================================================================
// Edge Cases
// =============================================================================

#[test]
fn test_error_with_empty_string() {
    let err = RestorationError::TlsFailed {
        reason: String::new(),
    };

    let msg = format!("{}", err);
    assert!(msg.contains("TLS restoration failed"));
}

#[test]
fn test_error_with_unicode() {
    let err = DiscoveryError::ParseFailed {
        path: PathBuf::from("tests/test_\u{1F600}.py"),
        reason: "unexpected token".to_string(),
    };

    let msg = format!("{}", err);
    // Should not panic with Unicode
    assert!(!msg.is_empty());
}

#[test]
fn test_error_with_special_characters() {
    let err = SchedulerError::IpcBroken {
        pid: 1234,
        reason: "error: <script>alert(1)</script>".to_string(),
    };

    let msg = format!("{}", err);
    assert!(msg.contains("<script>"));
}

#[test]
fn test_error_with_large_values() {
    let err = RestorationError::HeapDesync {
        expected_checksum: u64::MAX,
        actual_checksum: u64::MAX - 1,
    };

    let msg = format!("{}", err);
    assert!(msg.contains("FFFFFFFFFFFFFFFF"));
}

#[test]
fn test_error_with_zero_values() {
    let err = TeleportationError::AdoptionFailed {
        source_fd: 0,
        target_fd: 0,
        errno: 0,
    };

    let msg = format!("{}", err);
    assert!(msg.contains("dup2(0, 0)"));
}

#[test]
fn test_error_with_negative_values() {
    let err = SchedulerError::WorkerCrashed {
        pid: -1,
        signal: -1,
    };

    let msg = format!("{}", err);
    assert!(msg.contains("-1"));
}
