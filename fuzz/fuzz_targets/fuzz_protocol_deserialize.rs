//! Fuzz target for protocol deserialization
//!
//! This fuzzer tests that bincode deserialization of TestPayload and TestResult
//! doesn't panic on arbitrary input (returns Err on invalid data instead).

#![no_main]

use libfuzzer_sys::fuzz_target;
use tach_core::protocol::{TestPayload, TestResult};

fuzz_target!(|data: &[u8]| {
    // Try to deserialize as TestPayload - should not panic
    let _: Result<TestPayload, _> = bincode::deserialize(data);

    // Try to deserialize as TestResult - should not panic
    let _: Result<TestResult, _> = bincode::deserialize(data);

    // If we have enough data, try specific patterns
    if data.len() >= 4 {
        // Try deserializing just the first 4 bytes as u32 (test_id)
        let _: Result<u32, _> = bincode::deserialize(&data[..4]);
    }

    // Verify that valid serialized data can be deserialized
    if data.len() > 50 {
        // Create a valid payload and verify roundtrip
        let test_payload = TestPayload {
            test_id: 1,
            file_path: String::from("test.py"),
            test_name: String::from("test_foo"),
            is_async: false,
            fixtures: vec![],
            log_fd: -1,
            debug_socket_path: String::new(),
            is_toxic: false,
        };

        let serialized = bincode::serialize(&test_payload).unwrap();
        let deserialized: TestPayload = bincode::deserialize(&serialized).unwrap();
        assert_eq!(deserialized.test_id, 1);
    }
});
