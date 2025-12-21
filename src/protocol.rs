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
