//! Binary IPC Protocol for Supervisor ↔ Zygote communication
//! Uses bincode for zero-copy serialization.

use crate::discovery::FixtureScope;
use serde::{Deserialize, Serialize};

// Command bytes
pub const CMD_EXIT: u8 = 0x00;
pub const CMD_FORK: u8 = 0x01;
pub const CMD_RUN_TEST: u8 = 0x02; //  Send test to existing worker
pub const CMD_PING: u8 = 0x03; //  Health check ping to worker
pub const MSG_READY: u8 = 0x42;
pub const MSG_WORKER_READY: u8 = 0x43; //  Worker signals availability for reuse
pub const MSG_PONG: u8 = 0x44; //  Worker responds to health check

// Result status codes
pub const STATUS_PASS: u8 = 0;
pub const STATUS_FAIL: u8 = 1;
pub const STATUS_SKIP: u8 = 2;
pub const STATUS_CRASH: u8 = 3;
pub const STATUS_ERROR: u8 = 4;
pub const STATUS_HARNESS_ERROR: u8 = 5;
pub const STATUS_TIMEOUT: u8 = 6;

/// Payload sent to Zygote with fork command
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestPayload {
    pub test_id: u32,
    pub file_path: String,
    pub test_name: String,
    pub is_async: bool,
    pub fixtures: Vec<FixtureInfo>,
    /// File descriptor for log capture (memfd)
    pub log_fd: i32,
    /// Path to supervisor's debug socket for breakpoint() support
    pub debug_socket_path: String,
    /// Whether this test is toxic (requires fork/kill instead of reset)
    ///  Toxic tests exit after run, Safe tests reset and loop
    pub is_toxic: bool,
    /// Per-test timeout in seconds from @pytest.mark.timeout(N)
    /// None means use global timeout
    pub timeout_secs: Option<u64>,
}

/// Fixture info for payload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixtureInfo {
    pub name: String,
    pub scope: String,
}

impl FixtureInfo {
    pub fn from_scope(name: String, scope: &FixtureScope) -> Self {
        Self {
            name,
            scope: match scope {
                FixtureScope::Function => "function".to_string(),
                FixtureScope::Class => "class".to_string(),
                FixtureScope::Module => "module".to_string(),
                FixtureScope::Session => "session".to_string(),
            },
        }
    }
}

/// Binary result sent back from worker (fixed-size header + variable message)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestResult {
    pub test_id: u32,
    pub status: u8,
    pub duration_ns: u64,
    /// Truncated to 4KB max
    pub message: String,
    /// Peak memory usage in bytes (resident set size from /proc/pid/statm)
    /// None if memory monitoring failed or not available
    #[serde(default)]
    pub memory_rss_bytes: Option<u64>,
}

impl TestResult {
    pub fn pass(test_id: u32, duration_ns: u64) -> Self {
        Self {
            test_id,
            status: STATUS_PASS,
            duration_ns,
            message: String::new(),
            memory_rss_bytes: None,
        }
    }

    pub fn fail(test_id: u32, duration_ns: u64, message: String) -> Self {
        Self {
            test_id,
            status: STATUS_FAIL,
            duration_ns,
            message: truncate_message(message),
            memory_rss_bytes: None,
        }
    }

    pub fn crash(test_id: u32) -> Self {
        Self {
            test_id,
            status: STATUS_CRASH,
            duration_ns: 0,
            message: "Worker crashed (EOF on socket)".to_string(),
            memory_rss_bytes: None,
        }
    }

    pub fn timeout(test_id: u32, duration_ns: u64) -> Self {
        Self {
            test_id,
            status: STATUS_TIMEOUT,
            duration_ns,
            message: "Test exceeded timeout limit".to_string(),
            memory_rss_bytes: None,
        }
    }

    pub fn status_str(&self) -> &'static str {
        match self.status {
            STATUS_PASS => "PASS",
            STATUS_FAIL => "FAIL",
            STATUS_SKIP => "SKIP",
            STATUS_CRASH => "CRASH",
            STATUS_ERROR => "ERROR",
            STATUS_HARNESS_ERROR => "HARNESS_ERROR",
            STATUS_TIMEOUT => "TIMEOUT",
            _ => "UNKNOWN",
        }
    }

    pub fn status_icon(&self) -> &'static str {
        match self.status {
            STATUS_PASS => "✓",
            STATUS_FAIL => "✗",
            STATUS_SKIP => "○",
            STATUS_CRASH => "💥",
            STATUS_ERROR => "!",
            STATUS_HARNESS_ERROR => "⚠",
            STATUS_TIMEOUT => "⏱",
            _ => "?",
        }
    }

    pub fn duration_ms(&self) -> f64 {
        self.duration_ns as f64 / 1_000_000.0
    }

    /// Set memory usage (builder pattern)
    pub fn with_memory(mut self, memory_rss_bytes: Option<u64>) -> Self {
        self.memory_rss_bytes = memory_rss_bytes;
        self
    }
}

/// Memory usage threshold for warnings (500MB)
pub const MEMORY_WARNING_THRESHOLD_BYTES: u64 = 500 * 1024 * 1024;

/// Read resident set size (RSS) from /proc/{pid}/statm
///
/// Returns the RSS in bytes, or None if the file cannot be read.
/// The statm file has format: size resident share text lib data dt
/// We read the second field (resident) and multiply by page size.
pub fn read_process_memory_rss(pid: i32) -> Option<u64> {
    use std::fs;

    // Get page size (usually 4096 bytes)
    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) } as u64;
    if page_size == 0 {
        return None;
    }

    // Read /proc/{pid}/statm
    let statm_path = format!("/proc/{}/statm", pid);
    let content = fs::read_to_string(&statm_path).ok()?;

    // Parse the second field (resident pages)
    let resident_pages: u64 = content.split_whitespace().nth(1)?.parse().ok()?;

    Some(resident_pages * page_size)
}

fn truncate_message(msg: String) -> String {
    const MAX_LEN: usize = 4096;
    if msg.len() > MAX_LEN {
        format!("{}... [truncated]", &msg[..MAX_LEN])
    } else {
        msg
    }
}

/// Maximum allowed payload size for IPC messages (16 MiB)
/// This prevents OOM attacks from malicious payloads claiming huge sizes
pub const MAX_PAYLOAD_SIZE: usize = 16 * 1024 * 1024;

/// Protocol magic bytes for frame validation
/// "TA" = Tach, used to detect corrupted or misaligned frames
pub const PROTOCOL_MAGIC: [u8; 2] = *b"TA";

/// Protocol version for compatibility checking
/// Increment this when making breaking changes to the protocol
pub const PROTOCOL_VERSION: u8 = 1;

/// Header size: magic(2) + version(1) + reserved(1) + length(4) = 8 bytes
pub const HEADER_SIZE: usize = 8;

/// Encode a struct to bincode bytes with protocol header
///
/// Frame format:
/// - Magic (2 bytes): "TA" for frame validation
/// - Version (1 byte): Protocol version for compatibility
/// - Reserved (1 byte): Reserved for future use (always 0)
/// - Length (4 bytes): Payload size in little-endian u32
/// - Payload: bincode-encoded data
pub fn encode_with_length<T: serde::Serialize>(
    value: &T,
) -> Result<Vec<u8>, bincode::error::EncodeError> {
    let payload = bincode::serde::encode_to_vec(value, bincode::config::standard())?;
    let len = payload.len() as u32;
    let mut result = Vec::with_capacity(HEADER_SIZE + payload.len());
    // Magic bytes (2)
    result.extend_from_slice(&PROTOCOL_MAGIC);
    // Version (1)
    result.push(PROTOCOL_VERSION);
    // Reserved (1)
    result.push(0);
    // Length (4)
    result.extend_from_slice(&len.to_le_bytes());
    // Payload
    result.extend_from_slice(&payload);
    Ok(result)
}

/// Error type for decode_with_limit
#[derive(Debug)]
pub enum DecodeWithLimitError {
    /// Payload claims size larger than allowed limit
    PayloadTooLarge { claimed: usize, limit: usize },
    /// Not enough data in buffer
    InsufficientData { needed: usize, available: usize },
    /// Bincode decoding error
    Bincode(bincode::error::DecodeError),
    /// Invalid magic bytes - frame is corrupted or misaligned
    InvalidMagic,
    /// Protocol version mismatch - sender and receiver are incompatible
    VersionMismatch { expected: u8, found: u8 },
}

impl std::fmt::Display for DecodeWithLimitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PayloadTooLarge { claimed, limit } => {
                write!(
                    f,
                    "payload claims {} bytes, exceeds limit of {}",
                    claimed, limit
                )
            }
            Self::InsufficientData { needed, available } => {
                write!(f, "need {} bytes but only {} available", needed, available)
            }
            Self::Bincode(e) => write!(f, "bincode decode error: {}", e),
            Self::InvalidMagic => write!(f, "invalid protocol magic bytes"),
            Self::VersionMismatch { expected, found } => {
                write!(
                    f,
                    "protocol version mismatch: expected {}, found {}",
                    expected, found
                )
            }
        }
    }
}

impl std::error::Error for DecodeWithLimitError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Bincode(e) => Some(e),
            _ => None,
        }
    }
}

/// Decode a protocol-framed bincode payload with size limit protection
///
/// Validates the protocol header (magic, version) and checks the length prefix
/// against `max_size` before allocating memory, preventing OOM attacks from
/// malicious payloads that claim huge sizes.
///
/// Frame format:
/// - Magic (2 bytes): "TA" for frame validation
/// - Version (1 byte): Protocol version for compatibility
/// - Reserved (1 byte): Reserved for future use
/// - Length (4 bytes): Payload size in little-endian u32
/// - Payload: bincode-encoded data
///
/// # Arguments
/// * `data` - Raw bytes containing the protocol-framed bincode payload
/// * `max_size` - Maximum allowed payload size in bytes
///
/// # Returns
/// * `Ok(T)` - Successfully decoded value
/// * `Err(DecodeWithLimitError)` - Decoding failed or payload exceeds limit
///
/// # Example
/// ```ignore
/// let encoded = encode_with_length(&payload)?;
/// let decoded: TestPayload = decode_with_limit(&encoded, MAX_PAYLOAD_SIZE)?;
/// ```
pub fn decode_with_limit<T: serde::de::DeserializeOwned>(
    data: &[u8],
    max_size: usize,
) -> Result<T, DecodeWithLimitError> {
    // Need at least HEADER_SIZE bytes for full header
    if data.len() < HEADER_SIZE {
        return Err(DecodeWithLimitError::InsufficientData {
            needed: HEADER_SIZE,
            available: data.len(),
        });
    }

    // Validate magic bytes
    if data[0..2] != PROTOCOL_MAGIC {
        return Err(DecodeWithLimitError::InvalidMagic);
    }

    // Validate protocol version
    let version = data[2];
    if version != PROTOCOL_VERSION {
        return Err(DecodeWithLimitError::VersionMismatch {
            expected: PROTOCOL_VERSION,
            found: version,
        });
    }

    // Reserved byte at data[3] is ignored (forward compatibility)

    // Read length from bytes 4-7 (little-endian u32)
    let len = u32::from_le_bytes([data[4], data[5], data[6], data[7]]) as usize;

    // Check against size limit BEFORE any allocation
    if len > max_size {
        return Err(DecodeWithLimitError::PayloadTooLarge {
            claimed: len,
            limit: max_size,
        });
    }

    // Verify we have enough bytes for header + payload
    if data.len() < HEADER_SIZE + len {
        return Err(DecodeWithLimitError::InsufficientData {
            needed: HEADER_SIZE + len,
            available: data.len(),
        });
    }

    // Safe to decode now - size is within limit
    // Note: bincode's with_limit requires a const generic, not a runtime value.
    // Since we've validated the outer message size, and TestPayload has fixed-size
    // fields (u16, u64, String), the internal structure cannot claim more bytes
    // than the validated payload size allows.
    let (value, _): (T, usize) = bincode::serde::decode_from_slice(
        &data[HEADER_SIZE..HEADER_SIZE + len],
        bincode::config::standard(),
    )
    .map_err(DecodeWithLimitError::Bincode)?;

    Ok(value)
}

// =============================================================================
// Unit Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_result_pass_constructor() {
        let result = TestResult::pass(42, 1_000_000);
        assert_eq!(result.test_id, 42);
        assert_eq!(result.status, STATUS_PASS);
        assert_eq!(result.duration_ns, 1_000_000);
        assert!(result.message.is_empty());
    }

    #[test]
    fn test_result_fail_constructor() {
        let result = TestResult::fail(42, 1_000_000, "assertion error".to_string());
        assert_eq!(result.test_id, 42);
        assert_eq!(result.status, STATUS_FAIL);
        assert_eq!(result.duration_ns, 1_000_000);
        assert_eq!(result.message, "assertion error");
    }

    #[test]
    fn test_result_crash_constructor() {
        let result = TestResult::crash(42);
        assert_eq!(result.test_id, 42);
        assert_eq!(result.status, STATUS_CRASH);
        assert_eq!(result.duration_ns, 0);
        assert!(result.message.contains("crashed"));
    }

    #[test]
    fn test_status_str_mappings() {
        assert_eq!(TestResult::pass(0, 0).status_str(), "PASS");
        assert_eq!(TestResult::fail(0, 0, "".into()).status_str(), "FAIL");
        assert_eq!(TestResult::crash(0).status_str(), "CRASH");

        // Test all status codes directly
        let mut r = TestResult::pass(0, 0);
        r.status = STATUS_SKIP;
        assert_eq!(r.status_str(), "SKIP");
        r.status = STATUS_ERROR;
        assert_eq!(r.status_str(), "ERROR");
        r.status = STATUS_HARNESS_ERROR;
        assert_eq!(r.status_str(), "HARNESS_ERROR");
        r.status = 255; // Unknown
        assert_eq!(r.status_str(), "UNKNOWN");
    }

    #[test]
    fn test_duration_ms_conversion() {
        // 1.5ms = 1,500,000 ns
        let result = TestResult::pass(0, 1_500_000);
        assert!((result.duration_ms() - 1.5).abs() < 0.001);

        // 0ms
        let zero = TestResult::pass(0, 0);
        assert_eq!(zero.duration_ms(), 0.0);

        // 1 second = 1,000,000,000 ns = 1000ms
        let one_sec = TestResult::pass(0, 1_000_000_000);
        assert!((one_sec.duration_ms() - 1000.0).abs() < 0.001);
    }

    #[test]
    fn test_truncate_message_edge_cases() {
        // Short message - no truncation
        let short = truncate_message("hello".to_string());
        assert_eq!(short, "hello");

        // Empty message
        let empty = truncate_message(String::new());
        assert_eq!(empty, "");

        // Exactly 4096 chars - no truncation
        let exact = "x".repeat(4096);
        let result = truncate_message(exact.clone());
        assert_eq!(result.len(), 4096);
        assert!(!result.contains("truncated"));

        // Over 4096 - truncated with suffix
        let long = "y".repeat(5000);
        let truncated = truncate_message(long);
        assert!(truncated.ends_with("... [truncated]"));
        assert!(truncated.len() < 5000);
        // Should be 4096 + "... [truncated]".len()
        assert_eq!(truncated.len(), 4096 + 15);
    }

    #[test]
    fn test_encode_with_length_roundtrip() {
        let payload = TestPayload {
            test_id: 42,
            file_path: "tests/test_foo.py".to_string(),
            test_name: "test_bar".to_string(),
            is_async: true,
            fixtures: vec![FixtureInfo {
                name: "db".to_string(),
                scope: "module".to_string(),
            }],
            log_fd: -1,
            debug_socket_path: String::new(),
            is_toxic: false,
            timeout_secs: Some(30),
        };

        let encoded = encode_with_length(&payload).unwrap();

        // Verify header format: magic(2) + version(1) + reserved(1) + length(4) = 8 bytes
        assert_eq!(
            &encoded[0..2],
            &PROTOCOL_MAGIC,
            "Magic bytes should be 'TA'"
        );
        assert_eq!(encoded[2], PROTOCOL_VERSION, "Version should match");
        assert_eq!(encoded[3], 0, "Reserved byte should be 0");

        // Extract length from bytes 4-7 (little-endian u32)
        let len = u32::from_le_bytes([encoded[4], encoded[5], encoded[6], encoded[7]]) as usize;
        assert_eq!(
            len,
            encoded.len() - HEADER_SIZE,
            "Length should match payload size"
        );

        // Verify we can deserialize the payload correctly
        let (decoded, _): (TestPayload, usize) =
            bincode::serde::decode_from_slice(&encoded[HEADER_SIZE..], bincode::config::standard())
                .unwrap();
        assert_eq!(decoded.test_id, 42);
        assert_eq!(decoded.file_path, "tests/test_foo.py");
        assert_eq!(decoded.test_name, "test_bar");
        assert!(decoded.is_async);
        assert_eq!(decoded.fixtures.len(), 1);
        assert_eq!(decoded.fixtures[0].name, "db");
        assert_eq!(decoded.log_fd, -1);
    }

    #[test]
    fn test_fixture_info_from_scope() {
        assert_eq!(
            FixtureInfo::from_scope("db".into(), &FixtureScope::Function).scope,
            "function"
        );
        assert_eq!(
            FixtureInfo::from_scope("db".into(), &FixtureScope::Class).scope,
            "class"
        );
        assert_eq!(
            FixtureInfo::from_scope("db".into(), &FixtureScope::Module).scope,
            "module"
        );
        assert_eq!(
            FixtureInfo::from_scope("db".into(), &FixtureScope::Session).scope,
            "session"
        );
    }

    #[test]
    fn test_memory_monitoring_read_own_process() {
        // Read our own process memory - should always succeed on Linux
        let pid = std::process::id() as i32;
        let memory = read_process_memory_rss(pid);

        // Should return Some value on Linux
        assert!(
            memory.is_some(),
            "Should be able to read own process memory"
        );

        // Memory should be non-zero (we're running)
        let rss = memory.unwrap();
        assert!(rss > 0, "Process should have non-zero memory usage");

        // Should be less than 10GB (sanity check for reasonable value)
        assert!(
            rss < 10 * 1024 * 1024 * 1024,
            "Memory should be less than 10GB"
        );
    }

    #[test]
    fn test_memory_monitoring_invalid_pid() {
        // Reading non-existent process should return None
        let memory = read_process_memory_rss(-1);
        assert!(memory.is_none(), "Invalid PID should return None");

        let memory = read_process_memory_rss(999999999);
        assert!(memory.is_none(), "Non-existent PID should return None");
    }

    #[test]
    fn test_memory_warning_threshold() {
        // Verify constant is 500MB
        assert_eq!(MEMORY_WARNING_THRESHOLD_BYTES, 500 * 1024 * 1024);
    }

    #[test]
    fn test_result_with_memory() {
        let result = TestResult::pass(1, 1000).with_memory(Some(1024 * 1024));
        assert_eq!(result.memory_rss_bytes, Some(1024 * 1024));

        let result_none = TestResult::pass(2, 2000).with_memory(None);
        assert_eq!(result_none.memory_rss_bytes, None);
    }

    #[test]
    fn test_timeout_status() {
        let result = TestResult::timeout(1, 60_000_000_000);
        assert_eq!(result.status, STATUS_TIMEOUT);
        assert_eq!(result.status_str(), "TIMEOUT");
        assert_eq!(result.status_icon(), "⏱");
    }

    #[test]
    fn test_decode_with_limit_accepts_valid_payload() {
        let payload = TestPayload {
            test_id: 42,
            file_path: "test.py".to_string(),
            test_name: "test_foo".to_string(),
            is_async: false,
            fixtures: vec![],
            log_fd: -1,
            debug_socket_path: String::new(),
            is_toxic: false,
            timeout_secs: None,
        };

        let encoded = encode_with_length(&payload).unwrap();
        let decoded: TestPayload = decode_with_limit(&encoded, MAX_PAYLOAD_SIZE).unwrap();

        assert_eq!(decoded.test_id, 42);
        assert_eq!(decoded.file_path, "test.py");
        assert_eq!(decoded.test_name, "test_foo");
        assert!(!decoded.is_async);
        assert!(decoded.fixtures.is_empty());
    }

    #[test]
    fn test_decode_with_limit_rejects_oversized_payload() {
        // Create a valid encoded payload
        let payload = TestPayload {
            test_id: 1,
            file_path: "test.py".to_string(),
            test_name: "test".to_string(),
            is_async: false,
            fixtures: vec![],
            log_fd: -1,
            debug_socket_path: String::new(),
            is_toxic: false,
            timeout_secs: None,
        };
        let encoded = encode_with_length(&payload).unwrap();

        // Set limit smaller than the actual payload
        let tiny_limit = 10; // Way smaller than any valid payload

        let result: Result<TestPayload, _> = decode_with_limit(&encoded, tiny_limit);
        assert!(result.is_err());

        match result.unwrap_err() {
            DecodeWithLimitError::PayloadTooLarge { claimed, limit } => {
                assert!(claimed > limit);
                assert_eq!(limit, tiny_limit);
            }
            other => panic!("Expected PayloadTooLarge, got {:?}", other),
        }
    }

    #[test]
    fn test_decode_with_limit_handles_insufficient_data() {
        // Empty buffer - needs at least HEADER_SIZE (8) bytes
        let result: Result<TestPayload, _> = decode_with_limit(&[], MAX_PAYLOAD_SIZE);
        assert!(matches!(
            result,
            Err(DecodeWithLimitError::InsufficientData {
                needed: 8,
                available: 0
            })
        ));

        // Only 2 bytes (less than 8-byte header)
        let result: Result<TestPayload, _> = decode_with_limit(&[0, 0], MAX_PAYLOAD_SIZE);
        assert!(matches!(
            result,
            Err(DecodeWithLimitError::InsufficientData {
                needed: 8,
                available: 2
            })
        ));

        // Valid header but claims 100 bytes, only 12 available (8 header + 4 payload)
        // Header: magic(2) + version(1) + reserved(1) + length(4)
        let mut data = vec![b'T', b'A', PROTOCOL_VERSION, 0]; // magic + version + reserved
        data.extend_from_slice(&100u32.to_le_bytes()); // length = 100
        data.extend_from_slice(&[0; 4]); // only 4 more bytes, not 100
        let result: Result<TestPayload, _> = decode_with_limit(&data, MAX_PAYLOAD_SIZE);
        assert!(matches!(
            result,
            Err(DecodeWithLimitError::InsufficientData {
                needed: 108,
                available: 12
            })
        ));
    }

    #[test]
    fn test_decode_with_limit_roundtrip_with_fixtures() {
        let payload = TestPayload {
            test_id: 999,
            file_path: "tests/integration/test_db.py".to_string(),
            test_name: "test_complex_query".to_string(),
            is_async: true,
            fixtures: vec![
                FixtureInfo {
                    name: "db".to_string(),
                    scope: "module".to_string(),
                },
                FixtureInfo {
                    name: "client".to_string(),
                    scope: "function".to_string(),
                },
            ],
            log_fd: 42,
            debug_socket_path: "/tmp/debug.sock".to_string(),
            is_toxic: true,
            timeout_secs: Some(120),
        };

        let encoded = encode_with_length(&payload).unwrap();
        let decoded: TestPayload = decode_with_limit(&encoded, MAX_PAYLOAD_SIZE).unwrap();

        assert_eq!(decoded.test_id, payload.test_id);
        assert_eq!(decoded.file_path, payload.file_path);
        assert_eq!(decoded.test_name, payload.test_name);
        assert_eq!(decoded.is_async, payload.is_async);
        assert_eq!(decoded.fixtures.len(), 2);
        assert_eq!(decoded.fixtures[0].name, "db");
        assert_eq!(decoded.fixtures[1].scope, "function");
        assert_eq!(decoded.log_fd, 42);
        assert_eq!(decoded.debug_socket_path, "/tmp/debug.sock");
        assert!(decoded.is_toxic);
        assert_eq!(decoded.timeout_secs, Some(120));
    }

    #[test]
    fn test_max_payload_size_constant() {
        // Verify the constant is 16 MiB
        assert_eq!(MAX_PAYLOAD_SIZE, 16 * 1024 * 1024);
    }

    // =========================================================================
    // Protocol Header Validation Tests
    // =========================================================================

    #[test]
    fn test_protocol_header_constants() {
        // Verify header constants
        assert_eq!(PROTOCOL_MAGIC, *b"TA", "Magic bytes should be 'TA'");
        assert_eq!(PROTOCOL_VERSION, 1, "Initial protocol version should be 1");
        assert_eq!(HEADER_SIZE, 8, "Header size should be 8 bytes");
    }

    #[test]
    fn test_decode_with_invalid_magic() {
        // Create a valid header but with wrong magic bytes
        let mut data = vec![b'X', b'X', PROTOCOL_VERSION, 0]; // Wrong magic
        data.extend_from_slice(&10u32.to_le_bytes()); // length = 10
        data.extend_from_slice(&[0; 10]); // payload

        let result: Result<TestPayload, _> = decode_with_limit(&data, MAX_PAYLOAD_SIZE);
        assert!(
            matches!(result, Err(DecodeWithLimitError::InvalidMagic)),
            "Should reject invalid magic bytes, got {:?}",
            result
        );
    }

    #[test]
    fn test_decode_with_version_mismatch() {
        // Create a valid header but with wrong version
        let wrong_version = 99u8;
        let mut data = vec![b'T', b'A', wrong_version, 0]; // Wrong version
        data.extend_from_slice(&10u32.to_le_bytes()); // length = 10
        data.extend_from_slice(&[0; 10]); // payload

        let result: Result<TestPayload, _> = decode_with_limit(&data, MAX_PAYLOAD_SIZE);
        match result {
            Err(DecodeWithLimitError::VersionMismatch { expected, found }) => {
                assert_eq!(expected, PROTOCOL_VERSION);
                assert_eq!(found, wrong_version);
            }
            other => panic!("Expected VersionMismatch, got {:?}", other),
        }
    }

    #[test]
    fn test_header_format_in_encoded_message() {
        let result = TestResult::pass(123, 1_000_000);
        let encoded = encode_with_length(&result).unwrap();

        // Verify header structure
        assert!(
            encoded.len() >= HEADER_SIZE,
            "Encoded message should have at least header bytes"
        );
        assert_eq!(encoded[0], b'T', "First byte should be 'T'");
        assert_eq!(encoded[1], b'A', "Second byte should be 'A'");
        assert_eq!(encoded[2], PROTOCOL_VERSION, "Third byte should be version");
        assert_eq!(encoded[3], 0, "Fourth byte should be reserved (0)");

        // Verify length matches
        let claimed_len =
            u32::from_le_bytes([encoded[4], encoded[5], encoded[6], encoded[7]]) as usize;
        assert_eq!(
            claimed_len,
            encoded.len() - HEADER_SIZE,
            "Length field should match actual payload size"
        );
    }

    #[test]
    fn test_decode_with_limit_validates_header_before_allocation() {
        // This test verifies that header validation happens BEFORE any large allocation
        // by checking that invalid magic is rejected even with a huge claimed length

        // Invalid magic with huge claimed length (16GB)
        let mut data = vec![b'X', b'X', PROTOCOL_VERSION, 0];
        data.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes()); // 4GB length claim
        data.extend_from_slice(&[0; 100]); // Some payload

        // Should fail on magic check, not try to allocate 4GB
        let result: Result<TestPayload, _> = decode_with_limit(&data, MAX_PAYLOAD_SIZE);
        assert!(
            matches!(result, Err(DecodeWithLimitError::InvalidMagic)),
            "Should reject invalid magic before checking length"
        );
    }
}
