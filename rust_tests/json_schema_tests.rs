//! JSON Schema Validation Tests
//!
//! These tests ensure the JSON output format (--format json) remains stable.
//! They validate:
//! - NDJSON format (each line is valid JSON)
//! - Required "event" field on all events
//! - Valid event types (run_start, test_start, test_finished, run_finished, error)
//! - Required fields for each event type
//!
//! This prevents silent breaking changes to the machine-readable output format.

use serde_json::Value;
use std::process::{Command, Stdio};

/// Get the path to the built binary (check multiple locations for different build profiles)
fn binary_path() -> String {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");

    // Check in priority order:
    // 1. Release build (for CI)
    // 2. Debug build (for local dev)
    // 3. llvm-cov release build (for coverage CI)
    // 4. llvm-cov debug build (for coverage local)
    let paths = [
        format!("{}/target/release/tach-core", manifest_dir),
        format!("{}/target/debug/tach-core", manifest_dir),
        format!("{}/target/llvm-cov-target/release/tach-core", manifest_dir),
        format!("{}/target/llvm-cov-target/debug/tach-core", manifest_dir),
    ];

    for path in &paths {
        if std::path::Path::new(path).exists() {
            return path.clone();
        }
    }

    // Fall back to debug path (will error with clear message if not found)
    paths[1].clone()
}

/// Get the project root directory
fn project_root() -> &'static str {
    env!("CARGO_MANIFEST_DIR")
}

/// Get the main project root (for accessing shared venv)
fn main_project_root() -> String {
    // Worktrees are typically in .worktrees/<name>
    // The main project venv is in the parent repo
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    if manifest_dir.contains(".worktrees") {
        // Extract path before .worktrees
        if let Some(pos) = manifest_dir.find(".worktrees") {
            return manifest_dir[..pos].trim_end_matches('/').to_string();
        }
    }
    manifest_dir.to_string()
}

/// Helper to run tach-core with given args and return output
/// Uses --no-isolation to avoid requiring sudo
fn run_tach(args: &[&str]) -> std::process::Output {
    let binary = binary_path();
    let main_root = main_project_root();

    Command::new(&binary)
        .args(["--no-isolation", "-n", "1"])
        .args(args)
        .current_dir(project_root())
        .env("PYTHONHOME", "")
        .env(
            "PYTHONPATH",
            format!(
                "{}/.venv/lib/python3.12/site-packages:{}",
                main_root,
                project_root()
            ),
        )
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("Failed to execute tach-core")
}

/// Valid event types that can appear in JSON output
const VALID_EVENT_TYPES: &[&str] = &[
    "run_start",
    "test_start",
    "test_finished",
    "run_finished",
    "error",
];

/// Parse JSON output into a vector of JSON values
fn parse_ndjson_output(stdout: &str) -> Vec<Value> {
    stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

// =============================================================================
// JSON Output Format Tests
// =============================================================================

#[test]
fn test_json_output_is_valid_ndjson() {
    // Run with JSON format on a test directory
    let output = run_tach(&["--format=json", "tests/dummy_project/"]);
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Each non-empty line should be valid JSON
    let mut line_count = 0;
    for (i, line) in stdout.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        line_count += 1;

        let parsed: Result<Value, _> = serde_json::from_str(line);
        assert!(
            parsed.is_ok(),
            "Line {} is not valid JSON: '{}'\nError: {:?}",
            i + 1,
            line,
            parsed.err()
        );
    }

    // Should have at least some output (run_start + run_finished minimum)
    assert!(
        line_count >= 2,
        "Expected at least 2 JSON events (run_start + run_finished), got {}",
        line_count
    );
}

#[test]
fn test_json_output_has_required_event_field() {
    let output = run_tach(&["--format=json", "tests/dummy_project/"]);
    let stdout = String::from_utf8_lossy(&output.stdout);

    let events = parse_ndjson_output(&stdout);

    // Every event must have an "event" field
    for (i, event) in events.iter().enumerate() {
        assert!(
            event.get("event").is_some(),
            "Event {} is missing required 'event' field: {:?}",
            i,
            event
        );

        // The event field must be a string
        assert!(
            event["event"].is_string(),
            "Event {} has non-string 'event' field: {:?}",
            i,
            event
        );
    }
}

#[test]
fn test_json_output_event_types_are_valid() {
    let output = run_tach(&["--format=json", "tests/dummy_project/"]);
    let stdout = String::from_utf8_lossy(&output.stdout);

    let events = parse_ndjson_output(&stdout);

    // Every event type must be in our allowed list
    for event in &events {
        if let Some(event_type) = event.get("event").and_then(|v| v.as_str()) {
            assert!(
                VALID_EVENT_TYPES.contains(&event_type),
                "Invalid event type '{}'. Valid types are: {:?}",
                event_type,
                VALID_EVENT_TYPES
            );
        }
    }
}

#[test]
fn test_json_test_finish_has_outcome() {
    let output = run_tach(&["--format=json", "tests/dummy_project/"]);
    let stdout = String::from_utf8_lossy(&output.stdout);

    let events = parse_ndjson_output(&stdout);

    // Filter to test_finished events
    let test_finished_events: Vec<_> = events
        .iter()
        .filter(|e| e.get("event").and_then(|v| v.as_str()) == Some("test_finished"))
        .collect();

    // Each test_finished event must have required fields
    for event in &test_finished_events {
        // Must have "id" field (string)
        assert!(
            event.get("id").and_then(|v| v.as_str()).is_some(),
            "test_finished missing 'id' field: {:?}",
            event
        );

        // Must have "status" field with valid value
        let status = event.get("status").and_then(|v| v.as_str());
        assert!(
            status.is_some(),
            "test_finished missing 'status' field: {:?}",
            event
        );
        let status = status.unwrap();
        assert!(
            ["pass", "fail", "skip"].contains(&status),
            "test_finished has invalid status '{}'. Valid values: pass, fail, skip. Event: {:?}",
            status,
            event
        );

        // Must have "duration_ms" field (integer)
        assert!(
            event.get("duration_ms").and_then(|v| v.as_u64()).is_some()
                || event.get("duration_ms").and_then(|v| v.as_i64()).is_some(),
            "test_finished missing 'duration_ms' field: {:?}",
            event
        );
    }
}

#[test]
fn test_json_run_start_has_count() {
    let output = run_tach(&["--format=json", "tests/dummy_project/"]);
    let stdout = String::from_utf8_lossy(&output.stdout);

    let events = parse_ndjson_output(&stdout);

    // Find run_start event
    let run_start = events
        .iter()
        .find(|e| e.get("event").and_then(|v| v.as_str()) == Some("run_start"));

    assert!(run_start.is_some(), "Missing run_start event");
    let run_start = run_start.unwrap();

    // Must have "count" field
    assert!(
        run_start.get("count").and_then(|v| v.as_u64()).is_some()
            || run_start.get("count").and_then(|v| v.as_i64()).is_some(),
        "run_start missing 'count' field: {:?}",
        run_start
    );
}

#[test]
fn test_json_run_finished_has_summary() {
    let output = run_tach(&["--format=json", "tests/dummy_project/"]);
    let stdout = String::from_utf8_lossy(&output.stdout);

    let events = parse_ndjson_output(&stdout);

    // Find run_finished event
    let run_finished = events
        .iter()
        .find(|e| e.get("event").and_then(|v| v.as_str()) == Some("run_finished"));

    assert!(run_finished.is_some(), "Missing run_finished event");
    let run_finished = run_finished.unwrap();

    // Must have required summary fields
    for field in &["passed", "failed", "skipped", "duration_ms"] {
        assert!(
            run_finished.get(*field).is_some(),
            "run_finished missing '{}' field: {:?}",
            field,
            run_finished
        );
    }
}

#[test]
fn test_json_test_start_has_required_fields() {
    let output = run_tach(&["--format=json", "tests/dummy_project/"]);
    let stdout = String::from_utf8_lossy(&output.stdout);

    let events = parse_ndjson_output(&stdout);

    // Find all test_start events
    let test_start_events: Vec<_> = events
        .iter()
        .filter(|e| e.get("event").and_then(|v| v.as_str()) == Some("test_start"))
        .collect();

    // Each test_start must have id and file
    for event in &test_start_events {
        assert!(
            event.get("id").and_then(|v| v.as_str()).is_some(),
            "test_start missing 'id' field: {:?}",
            event
        );
        assert!(
            event.get("file").and_then(|v| v.as_str()).is_some(),
            "test_start missing 'file' field: {:?}",
            event
        );
    }
}

#[test]
fn test_json_event_sequence_is_logical() {
    let output = run_tach(&["--format=json", "tests/dummy_project/"]);
    let stdout = String::from_utf8_lossy(&output.stdout);

    let events = parse_ndjson_output(&stdout);

    if events.is_empty() {
        // No events means nothing to validate
        return;
    }

    // First event should be run_start
    let first_event = events
        .first()
        .and_then(|e| e.get("event"))
        .and_then(|v| v.as_str());
    assert_eq!(
        first_event,
        Some("run_start"),
        "First event should be run_start, got {:?}",
        first_event
    );

    // Last event should be run_finished (or error if run failed)
    let last_event = events
        .last()
        .and_then(|e| e.get("event"))
        .and_then(|v| v.as_str());
    assert!(
        last_event == Some("run_finished") || last_event == Some("error"),
        "Last event should be run_finished or error, got {:?}",
        last_event
    );
}

#[test]
fn test_json_no_extra_fields_in_events() {
    // This test ensures we don't accidentally add fields without updating schema
    let output = run_tach(&["--format=json", "tests/dummy_project/"]);
    let stdout = String::from_utf8_lossy(&output.stdout);

    let events = parse_ndjson_output(&stdout);

    // Define expected fields for each event type
    let expected_fields: std::collections::HashMap<&str, Vec<&str>> = [
        ("run_start", vec!["event", "count"]),
        ("test_start", vec!["event", "id", "file"]),
        (
            "test_finished",
            vec!["event", "id", "status", "duration_ms", "message"],
        ),
        (
            "run_finished",
            vec!["event", "passed", "failed", "skipped", "duration_ms"],
        ),
        ("error", vec!["event", "message"]),
    ]
    .into_iter()
    .collect();

    for event in &events {
        if let Some(event_type) = event.get("event").and_then(|v| v.as_str())
            && let Some(allowed_fields) = expected_fields.get(event_type)
            && let Value::Object(map) = event
        {
            for key in map.keys() {
                assert!(
                    allowed_fields.contains(&key.as_str()),
                    "Unexpected field '{}' in {} event. Allowed fields: {:?}",
                    key,
                    event_type,
                    allowed_fields
                );
            }
        }
    }
}

// =============================================================================
// Unit Tests (no binary required)
// =============================================================================

#[test]
fn test_valid_event_types_constant() {
    // Ensure our constant is properly defined
    assert_eq!(VALID_EVENT_TYPES.len(), 5);
    assert!(VALID_EVENT_TYPES.contains(&"run_start"));
    assert!(VALID_EVENT_TYPES.contains(&"test_start"));
    assert!(VALID_EVENT_TYPES.contains(&"test_finished"));
    assert!(VALID_EVENT_TYPES.contains(&"run_finished"));
    assert!(VALID_EVENT_TYPES.contains(&"error"));
}

#[test]
fn test_parse_ndjson_empty() {
    let events = parse_ndjson_output("");
    assert!(events.is_empty());
}

#[test]
fn test_parse_ndjson_single_line() {
    let events = parse_ndjson_output(r#"{"event":"run_start","count":5}"#);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["event"], "run_start");
    assert_eq!(events[0]["count"], 5);
}

#[test]
fn test_parse_ndjson_multiple_lines() {
    let input = r#"{"event":"run_start","count":2}
{"event":"test_start","id":"test_foo","file":"test.py"}
{"event":"test_finished","id":"test_foo","status":"pass","duration_ms":42}
{"event":"run_finished","passed":1,"failed":0,"skipped":0,"duration_ms":100}"#;

    let events = parse_ndjson_output(input);
    assert_eq!(events.len(), 4);
    assert_eq!(events[0]["event"], "run_start");
    assert_eq!(events[1]["event"], "test_start");
    assert_eq!(events[2]["event"], "test_finished");
    assert_eq!(events[3]["event"], "run_finished");
}

#[test]
fn test_parse_ndjson_skips_invalid_lines() {
    let input = r#"{"event":"run_start","count":1}
not valid json
{"event":"run_finished","passed":1,"failed":0,"skipped":0,"duration_ms":50}"#;

    let events = parse_ndjson_output(input);
    assert_eq!(events.len(), 2);
    assert_eq!(events[0]["event"], "run_start");
    assert_eq!(events[1]["event"], "run_finished");
}

#[test]
fn test_parse_ndjson_skips_empty_lines() {
    let input = r#"{"event":"run_start","count":1}

{"event":"run_finished","passed":1,"failed":0,"skipped":0,"duration_ms":50}
"#;

    let events = parse_ndjson_output(input);
    assert_eq!(events.len(), 2);
}
