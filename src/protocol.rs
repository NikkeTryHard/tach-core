//! Binary IPC Protocol for Supervisor ↔ Zygote communication
//! Uses bincode for zero-copy serialization.

use crate::discovery::FixtureScope;
use serde::{Deserialize, Serialize};

// Command bytes
pub const CMD_EXIT: u8 = 0x00;
pub const CMD_FORK: u8 = 0x01;
pub const MSG_READY: u8 = 0x42;

// Result status codes
pub const STATUS_PASS: u8 = 0;
pub const STATUS_FAIL: u8 = 1;
pub const STATUS_SKIP: u8 = 2;
pub const STATUS_CRASH: u8 = 3;
pub const STATUS_ERROR: u8 = 4;
pub const STATUS_HARNESS_ERROR: u8 = 5;

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
}

impl TestResult {
    pub fn pass(test_id: u32, duration_ns: u64) -> Self {
        Self {
            test_id,
            status: STATUS_PASS,
            duration_ns,
            message: String::new(),
        }
    }

    pub fn fail(test_id: u32, duration_ns: u64, message: String) -> Self {
        Self {
            test_id,
            status: STATUS_FAIL,
            duration_ns,
            message: truncate_message(message),
        }
    }

    pub fn crash(test_id: u32) -> Self {
        Self {
            test_id,
            status: STATUS_CRASH,
            duration_ns: 0,
            message: "Worker crashed (EOF on socket)".to_string(),
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
            _ => "?",
        }
    }

    pub fn duration_ms(&self) -> f64 {
        self.duration_ns as f64 / 1_000_000.0
    }
}

fn truncate_message(msg: String) -> String {
    const MAX_LEN: usize = 4096;
    if msg.len() > MAX_LEN {
        format!("{}... [truncated]", &msg[..MAX_LEN])
    } else {
        msg
    }
}

/// Encode a struct to bincode bytes with length prefix
pub fn encode_with_length<T: Serialize>(value: &T) -> Result<Vec<u8>, bincode::Error> {
    let payload = bincode::serialize(value)?;
    let len = payload.len() as u32;
    let mut result = Vec::with_capacity(4 + payload.len());
    result.extend_from_slice(&len.to_le_bytes());
    result.extend_from_slice(&payload);
    Ok(result)
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
        };

        let encoded = encode_with_length(&payload).unwrap();

        // First 4 bytes are length prefix (little-endian u32)
        let len = u32::from_le_bytes([encoded[0], encoded[1], encoded[2], encoded[3]]) as usize;
        assert_eq!(len, encoded.len() - 4);

        // Verify we can deserialize the payload correctly
        let decoded: TestPayload = bincode::deserialize(&encoded[4..]).unwrap();
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
}
