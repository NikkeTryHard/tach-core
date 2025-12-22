//! Integration tests for the protocol module

use tach_core::protocol::{
    encode_with_length, FixtureInfo, TestPayload, TestResult, CMD_EXIT, CMD_FORK, STATUS_CRASH,
    STATUS_FAIL, STATUS_PASS, STATUS_SKIP,
};

#[test]
fn test_serialize_test_payload() {
    let payload = TestPayload {
        test_id: 42,
        file_path: "tests/test_example.py".to_string(),
        test_name: "test_something".to_string(),
        is_async: false,
        fixtures: vec![FixtureInfo {
            name: "db".to_string(),
            scope: "module".to_string(),
        }],
        log_fd: 5,
        debug_socket_path: String::new(),
    };

    let encoded = encode_with_length(&payload).expect("Should serialize");

    // Should have length prefix (4 bytes) + payload
    assert!(encoded.len() > 4, "Encoded should have length prefix");

    // First 4 bytes should be length
    let len = u32::from_le_bytes([encoded[0], encoded[1], encoded[2], encoded[3]]);
    assert_eq!(
        len as usize,
        encoded.len() - 4,
        "Length prefix should match payload size"
    );
}

#[test]
fn test_serialize_test_result() {
    let result = TestResult {
        test_id: 123,
        status: STATUS_PASS,
        duration_ns: 1_000_000_000, // 1 second
        message: String::new(),
    };

    let encoded = encode_with_length(&result).expect("Should serialize");
    assert!(encoded.len() > 4, "Encoded should have content");
}

#[test]
fn test_serialize_test_result_with_message() {
    let result = TestResult {
        test_id: 456,
        status: STATUS_FAIL,
        duration_ns: 500_000_000,
        message: "AssertionError: expected True".to_string(),
    };

    let encoded = encode_with_length(&result).expect("Should serialize");
    assert!(encoded.len() > 10, "Should include message");
}

#[test]
fn test_roundtrip_test_payload() {
    let original = TestPayload {
        test_id: 99,
        file_path: "path/to/test.py".to_string(),
        test_name: "test_roundtrip".to_string(),
        is_async: true,
        fixtures: vec![
            FixtureInfo {
                name: "fixture_a".to_string(),
                scope: "function".to_string(),
            },
            FixtureInfo {
                name: "fixture_b".to_string(),
                scope: "session".to_string(),
            },
        ],
        log_fd: 10,
        debug_socket_path: "/tmp/tach_debug_test.sock".to_string(),
    };

    let encoded = encode_with_length(&original).expect("Should serialize");

    // Decode
    let payload_bytes = &encoded[4..]; // Skip length prefix
    let decoded: TestPayload = bincode::deserialize(payload_bytes).expect("Should deserialize");

    assert_eq!(decoded.test_id, original.test_id);
    assert_eq!(decoded.file_path, original.file_path);
    assert_eq!(decoded.test_name, original.test_name);
    assert_eq!(decoded.fixtures.len(), 2);
    assert_eq!(decoded.log_fd, original.log_fd);
    assert_eq!(decoded.is_async, original.is_async);
}

#[test]
fn test_roundtrip_test_result() {
    let original = TestResult {
        test_id: 777,
        status: STATUS_CRASH,
        duration_ns: 123456789,
        message: "Segmentation fault".to_string(),
    };

    let encoded = encode_with_length(&original).expect("Should serialize");

    // Decode
    let payload_bytes = &encoded[4..];
    let decoded: TestResult = bincode::deserialize(payload_bytes).expect("Should deserialize");

    assert_eq!(decoded.test_id, original.test_id);
    assert_eq!(decoded.status, original.status);
    assert_eq!(decoded.duration_ns, original.duration_ns);
    assert_eq!(decoded.message, original.message);
}

#[test]
fn test_command_constants() {
    // Verify command constants are distinct
    assert_ne!(CMD_FORK, CMD_EXIT);

    // Verify status constants are distinct
    assert_ne!(STATUS_PASS, STATUS_FAIL);
    assert_ne!(STATUS_FAIL, STATUS_SKIP);
    assert_ne!(STATUS_SKIP, STATUS_CRASH);
}

#[test]
fn test_fixture_info_creation() {
    let info = FixtureInfo {
        name: "my_fixture".to_string(),
        scope: "module".to_string(),
    };

    assert_eq!(info.name, "my_fixture");
    assert_eq!(info.scope, "module");
}

#[test]
fn test_empty_fixtures_payload() {
    let payload = TestPayload {
        test_id: 1,
        file_path: "test.py".to_string(),
        test_name: "test_simple".to_string(),
        is_async: false,
        fixtures: vec![],
        log_fd: -1,
        debug_socket_path: String::new(),
    };

    let encoded = encode_with_length(&payload).expect("Should serialize empty fixtures");
    assert!(encoded.len() > 4);
}

#[test]
fn test_async_payload() {
    let payload = TestPayload {
        test_id: 2,
        file_path: "test.py".to_string(),
        test_name: "test_async".to_string(),
        is_async: true,
        fixtures: vec![],
        log_fd: -1,
        debug_socket_path: String::new(),
    };

    let encoded = encode_with_length(&payload).expect("Should serialize");
    let decoded: TestPayload = bincode::deserialize(&encoded[4..]).unwrap();
    assert!(decoded.is_async, "Async flag should be preserved");
}
