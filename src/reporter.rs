//! Reporter Module: Trait-based output for Human (CLI) and Machine (JSON) formats
//!
//! Phase 5.1: Machine Interface for IDE/CI integration.
//!
//! ## Architecture
//!
//! - `Reporter` trait defines the event callbacks
//! - `JsonReporter` outputs NDJSON to stdout (for --format=json)
//! - `HumanReporter` outputs human-readable text to stderr
//!
//! ## Boss Refinement: Stdout Purity
//!
//! When JsonReporter is active, ONLY valid JSON goes to stdout.
//! All other output (logs, errors, debug) must go to stderr.

use serde::Serialize;

/// Machine-readable events for JSON output
#[derive(Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum MachineEvent<'a> {
    /// Emitted at start of test run
    RunStart { count: usize },
    /// Emitted when a test begins execution
    TestStart { id: &'a str, file: &'a str },
    /// Emitted when a test completes
    TestFinished {
        id: &'a str,
        status: &'a str, // "pass", "fail", "skip"
        duration_ms: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        message: Option<&'a str>,
    },
    /// Emitted at end of test run
    RunFinished {
        passed: usize,
        failed: usize,
        skipped: usize,
        duration_ms: u64,
    },
    /// Emitted on fatal error (Boss Refinement #2)
    Error { message: &'a str },
}

/// Reporter trait for output abstraction
pub trait Reporter {
    /// Called at start of test run
    fn on_run_start(&mut self, count: usize);

    /// Called when a test begins execution
    fn on_test_start(&mut self, id: &str, file: &str);

    /// Called when a test completes
    fn on_test_finished(&mut self, id: &str, status: &str, duration_ms: u64, message: Option<&str>);

    /// Called at end of test run
    fn on_run_finished(&mut self, passed: usize, failed: usize, skipped: usize, duration_ms: u64);

    /// Called on fatal error (Boss Refinement #2)
    fn on_error(&mut self, message: &str);
}

/// JSON Reporter - outputs NDJSON to stdout
pub struct JsonReporter;

impl Reporter for JsonReporter {
    fn on_run_start(&mut self, count: usize) {
        let event = MachineEvent::RunStart { count };
        // ONLY JsonReporter touches stdout
        println!("{}", serde_json::to_string(&event).unwrap());
    }

    fn on_test_start(&mut self, id: &str, file: &str) {
        let event = MachineEvent::TestStart { id, file };
        println!("{}", serde_json::to_string(&event).unwrap());
    }

    fn on_test_finished(
        &mut self,
        id: &str,
        status: &str,
        duration_ms: u64,
        message: Option<&str>,
    ) {
        let event = MachineEvent::TestFinished {
            id,
            status,
            duration_ms,
            message,
        };
        println!("{}", serde_json::to_string(&event).unwrap());
    }

    fn on_run_finished(&mut self, passed: usize, failed: usize, skipped: usize, duration_ms: u64) {
        let event = MachineEvent::RunFinished {
            passed,
            failed,
            skipped,
            duration_ms,
        };
        println!("{}", serde_json::to_string(&event).unwrap());
    }

    fn on_error(&mut self, message: &str) {
        let event = MachineEvent::Error { message };
        println!("{}", serde_json::to_string(&event).unwrap());
    }
}

/// Human Reporter - outputs readable text to stderr
pub struct HumanReporter;

impl Reporter for HumanReporter {
    fn on_run_start(&mut self, count: usize) {
        eprintln!("[tach] Running {} tests...\n", count);
    }

    fn on_test_start(&mut self, id: &str, _file: &str) {
        eprint!("  {} ... ", id);
    }

    fn on_test_finished(
        &mut self,
        _id: &str,
        status: &str,
        duration_ms: u64,
        message: Option<&str>,
    ) {
        match status {
            "pass" => eprintln!("✓ ({}ms)", duration_ms),
            "fail" => {
                eprintln!("✗ ({}ms)", duration_ms);
                if let Some(msg) = message {
                    // Indent failure message
                    for line in msg.lines().take(10) {
                        eprintln!("    {}", line);
                    }
                }
            }
            "skip" => eprintln!("⊘ skipped"),
            _ => eprintln!("{}", status),
        }
    }

    fn on_run_finished(&mut self, passed: usize, failed: usize, skipped: usize, duration_ms: u64) {
        eprintln!();
        eprintln!(
            "[tach] {} passed, {} failed, {} skipped in {}ms",
            passed, failed, skipped, duration_ms
        );
    }

    fn on_error(&mut self, message: &str) {
        eprintln!("[tach] FATAL ERROR: {}", message);
    }
}

// =============================================================================
// MultiReporter (Phase 5.2)
// =============================================================================

/// MultiReporter - broadcasts events to multiple reporters
pub struct MultiReporter {
    reporters: Vec<Box<dyn Reporter>>,
}

impl MultiReporter {
    pub fn new(reporters: Vec<Box<dyn Reporter>>) -> Self {
        Self { reporters }
    }
}

impl Reporter for MultiReporter {
    fn on_run_start(&mut self, count: usize) {
        for r in &mut self.reporters {
            r.on_run_start(count);
        }
    }

    fn on_test_start(&mut self, id: &str, file: &str) {
        for r in &mut self.reporters {
            r.on_test_start(id, file);
        }
    }

    fn on_test_finished(
        &mut self,
        id: &str,
        status: &str,
        duration_ms: u64,
        message: Option<&str>,
    ) {
        for r in &mut self.reporters {
            r.on_test_finished(id, status, duration_ms, message);
        }
    }

    fn on_run_finished(&mut self, passed: usize, failed: usize, skipped: usize, duration_ms: u64) {
        for r in &mut self.reporters {
            r.on_run_finished(passed, failed, skipped, duration_ms);
        }
    }

    fn on_error(&mut self, message: &str) {
        for r in &mut self.reporters {
            r.on_error(message);
        }
    }
}

// =============================================================================
// Unit Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_json_event_serialization() {
        let event = MachineEvent::TestFinished {
            id: "test_foo",
            status: "pass",
            duration_ms: 42,
            message: None,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"event\":\"test_finished\""));
        assert!(json.contains("\"id\":\"test_foo\""));
        assert!(json.contains("\"status\":\"pass\""));
        assert!(!json.contains("message")); // skip_serializing_if = None
    }

    #[test]
    fn test_json_event_with_message() {
        let event = MachineEvent::TestFinished {
            id: "test_bar",
            status: "fail",
            duration_ms: 100,
            message: Some("assertion failed"),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"message\":\"assertion failed\""));
    }

    #[test]
    fn test_error_event() {
        let event = MachineEvent::Error {
            message: "Zygote died unexpectedly",
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"event\":\"error\""));
    }
}
