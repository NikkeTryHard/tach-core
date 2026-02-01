//! Integration tests for the protocol module

use tach_core::protocol::{
    CMD_EXIT, CMD_FORK, FixtureInfo, HEADER_SIZE, MAX_PAYLOAD_SIZE, PROTOCOL_MAGIC,
    PROTOCOL_VERSION, STATUS_CRASH, STATUS_FAIL, STATUS_PASS, STATUS_SKIP, TestPayload, TestResult,
    decode_with_limit, encode_with_length,
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
            is_async: false,
        }],
        log_fd: 5,
        debug_socket_path: String::new(),
        is_toxic: false,
        timeout_secs: None,
        hooks: vec![],
        cached_effects: vec![],
        markers: vec![],
        marker_info: vec![],
    };

    let encoded = encode_with_length(&payload).expect("Should serialize");

    // Should have protocol header (8 bytes) + payload
    assert!(encoded.len() > HEADER_SIZE, "Encoded should have header");

    // Verify header format
    assert_eq!(
        &encoded[0..2],
        &PROTOCOL_MAGIC,
        "Magic bytes should be 'TA'"
    );
    assert_eq!(encoded[2], PROTOCOL_VERSION, "Version should match");
    assert_eq!(encoded[3], 0, "Reserved byte should be 0");

    // Extract length from bytes 4-7 (little-endian u32)
    let len = u32::from_le_bytes([encoded[4], encoded[5], encoded[6], encoded[7]]);
    assert_eq!(
        len as usize,
        encoded.len() - HEADER_SIZE,
        "Length field should match payload size"
    );
}

#[test]
fn test_serialize_test_result() {
    let result = TestResult {
        test_id: 123,
        status: STATUS_PASS,
        duration_ns: 1_000_000_000, // 1 second
        message: String::new(),
        memory_rss_bytes: None,
    };

    let encoded = encode_with_length(&result).expect("Should serialize");
    assert!(encoded.len() > HEADER_SIZE, "Encoded should have content");
}

#[test]
fn test_serialize_test_result_with_message() {
    let result = TestResult {
        test_id: 456,
        status: STATUS_FAIL,
        duration_ns: 500_000_000,
        message: "AssertionError: expected True".to_string(),
        memory_rss_bytes: None,
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
                is_async: false,
            },
            FixtureInfo {
                name: "fixture_b".to_string(),
                scope: "session".to_string(),
                is_async: false,
            },
        ],
        log_fd: 10,
        debug_socket_path: "/tmp/tach_debug_test.sock".to_string(),
        is_toxic: false,
        timeout_secs: Some(60),
        hooks: vec![],
        cached_effects: vec![],
        markers: vec![],
        marker_info: vec![],
    };

    let encoded = encode_with_length(&original).expect("Should serialize");

    // Decode using decode_with_limit (validates header)
    let decoded: TestPayload =
        decode_with_limit(&encoded, MAX_PAYLOAD_SIZE).expect("Should deserialize");

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
        memory_rss_bytes: None,
    };

    let encoded = encode_with_length(&original).expect("Should serialize");

    // Decode using decode_with_limit (validates header)
    let decoded: TestResult =
        decode_with_limit(&encoded, MAX_PAYLOAD_SIZE).expect("Should deserialize");

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
        is_async: false,
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
        is_toxic: false,
        timeout_secs: None,
        hooks: vec![],
        cached_effects: vec![],
        markers: vec![],
        marker_info: vec![],
    };

    let encoded = encode_with_length(&payload).expect("Should serialize empty fixtures");
    assert!(encoded.len() > HEADER_SIZE);
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
        is_toxic: false,
        timeout_secs: None,
        hooks: vec![],
        cached_effects: vec![],
        markers: vec![],
        marker_info: vec![],
    };

    let encoded = encode_with_length(&payload).expect("Should serialize");
    let decoded: TestPayload = decode_with_limit(&encoded, MAX_PAYLOAD_SIZE).unwrap();
    assert!(decoded.is_async, "Async flag should be preserved");
}
