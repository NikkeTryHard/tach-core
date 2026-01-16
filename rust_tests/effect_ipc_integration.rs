//! Integration tests for Effect IPC between Zygote and Supervisor
//!
//! These tests verify the full effect transmission path:
//! 1. Effects are serialized via bincode in Zygote
//! 2. Effects are transmitted via IPC
//! 3. Effects are deserialized and stored in HookRegistry
//! 4. Effects are available for worker application

use tach_core::hooks::{HookEffect, HookRegistry, SysPathAction};

/// Test that HookEffect can be serialized and deserialized via bincode
/// This mirrors the IPC path between Zygote and Supervisor
#[test]
fn test_effect_bincode_roundtrip() {
    let effects = vec![
        HookEffect::SetEnv {
            key: "TEST_VAR".to_string(),
            value: "test_value".to_string(),
        },
        HookEffect::ModifySysPath {
            action: SysPathAction::Prepend,
            path: "/custom/path".to_string(),
        },
        HookEffect::RegisterMarker {
            name: "slow".to_string(),
            description: "marks tests as slow".to_string(),
        },
        HookEffect::NoEffect,
    ];

    // Serialize effects (as Zygote does)
    let encoded = bincode::serialize(&effects).expect("Should serialize effects");

    // Deserialize effects (as Supervisor does)
    let decoded: Vec<HookEffect> = bincode::deserialize(&encoded).expect("Should deserialize effects");

    assert_eq!(effects.len(), decoded.len());
    assert_eq!(effects, decoded);
}

/// Test that effects can be recorded in HookRegistry and retrieved
#[test]
fn test_effect_registry_storage() {
    let mut registry = HookRegistry::default();

    // Record effects (as Supervisor does after receiving from Zygote)
    let effect1 = HookEffect::SetEnv {
        key: "DB_URL".to_string(),
        value: "sqlite://test.db".to_string(),
    };
    let effect2 = HookEffect::ModifySysPath {
        action: SysPathAction::Append,
        path: "/app/plugins".to_string(),
    };

    registry.record_effect("pytest_configure", effect1.clone());
    registry.record_effect("pytest_configure", effect2.clone());

    // Retrieve effects (as Scheduler does for worker dispatch)
    let stored_effects = registry.get_effects("pytest_configure");

    assert_eq!(stored_effects.len(), 2);
    assert_eq!(&stored_effects[0], &effect1);
    assert_eq!(&stored_effects[1], &effect2);
}

/// Test the full IPC simulation: serialize -> transmit -> deserialize -> store -> retrieve
#[test]
fn test_full_effect_ipc_path() {
    // === ZYGOTE SIDE ===
    // Collect effects from Python hook execution
    let zygote_effects = vec![
        HookEffect::SetEnv {
            key: "PYTEST_CURRENT_TEST".to_string(),
            value: "test_module.py::test_func".to_string(),
        },
        HookEffect::RegisterMarker {
            name: "integration".to_string(),
            description: "marks integration tests".to_string(),
        },
        HookEffect::ModifySysPath {
            action: SysPathAction::Prepend,
            path: "/project/src".to_string(),
        },
    ];

    // Serialize for IPC transmission
    let wire_data = bincode::serialize(&zygote_effects).expect("Zygote should serialize effects");

    // === IPC TRANSMISSION ===
    // In real code, this goes through a pipe/socket
    // Here we just pass the bytes directly

    // === SUPERVISOR SIDE ===
    // Deserialize received effects
    let received_effects: Vec<HookEffect> = bincode::deserialize(&wire_data).expect("Supervisor should deserialize effects");

    // Store in HookRegistry
    let mut registry = HookRegistry::default();
    for effect in received_effects {
        registry.record_effect("pytest_configure", effect);
    }

    // === SCHEDULER/WORKER SIDE ===
    // Retrieve effects for worker application
    let worker_effects = registry.get_effects("pytest_configure");

    // Verify all effects made it through the full path
    assert_eq!(worker_effects.len(), 3);

    // Verify effect contents are preserved
    match &worker_effects[0] {
        HookEffect::SetEnv { key, value } => {
            assert_eq!(key, "PYTEST_CURRENT_TEST");
            assert_eq!(value, "test_module.py::test_func");
        }
        _ => panic!("Expected SetEnv effect"),
    }

    match &worker_effects[1] {
        HookEffect::RegisterMarker { name, description } => {
            assert_eq!(name, "integration");
            assert_eq!(description, "marks integration tests");
        }
        _ => panic!("Expected RegisterMarker effect"),
    }

    match &worker_effects[2] {
        HookEffect::ModifySysPath { action, path } => {
            assert_eq!(*action, SysPathAction::Prepend);
            assert_eq!(path, "/project/src");
        }
        _ => panic!("Expected ModifySysPath effect"),
    }
}

/// Test SysPathAction enum serialization
#[test]
fn test_syspathaction_serialization() {
    let actions = vec![SysPathAction::Prepend, SysPathAction::Append, SysPathAction::Remove];

    for action in actions {
        let encoded = bincode::serialize(&action).expect("Should serialize action");
        let decoded: SysPathAction = bincode::deserialize(&encoded).expect("Should deserialize action");
        assert_eq!(action, decoded);
    }
}

/// Test that empty effects list serializes correctly
#[test]
fn test_empty_effects_ipc() {
    let empty_effects: Vec<HookEffect> = vec![];

    let encoded = bincode::serialize(&empty_effects).expect("Should serialize empty vec");
    let decoded: Vec<HookEffect> = bincode::deserialize(&encoded).expect("Should deserialize empty vec");

    assert!(decoded.is_empty());
}

/// Test ModifyItems effect (used for test collection modification)
#[test]
fn test_modify_items_effect_ipc() {
    let effect = HookEffect::ModifyItems {
        removed: vec!["test_old.py::test_removed".to_string()],
        reordered: true,
    };

    let encoded = bincode::serialize(&effect).expect("Should serialize ModifyItems");
    let decoded: HookEffect = bincode::deserialize(&encoded).expect("Should deserialize ModifyItems");

    match decoded {
        HookEffect::ModifyItems { removed, reordered } => {
            assert_eq!(removed, vec!["test_old.py::test_removed"]);
            assert!(reordered);
        }
        _ => panic!("Expected ModifyItems effect"),
    }
}

/// Test graceful error handling when deserializing malformed bincode data
#[test]
fn test_malformed_bincode_data() {
    // Completely invalid data
    let garbage: Vec<u8> = vec![0xFF, 0xFE, 0x00, 0x01, 0x02];
    let result: Result<Vec<HookEffect>, _> = bincode::deserialize(&garbage);
    assert!(result.is_err(), "Should fail to deserialize garbage data");

    // Truncated data (valid start but incomplete)
    let effects = vec![HookEffect::SetEnv { key: "TEST".to_string(), value: "value".to_string() }];
    let mut encoded = bincode::serialize(&effects).expect("Should serialize");
    encoded.truncate(encoded.len() / 2); // Cut in half
    let result: Result<Vec<HookEffect>, _> = bincode::deserialize(&encoded);
    assert!(result.is_err(), "Should fail to deserialize truncated data");

    // Wrong type marker (try to deserialize as wrong enum variant)
    let single_effect = HookEffect::NoEffect;
    let encoded = bincode::serialize(&single_effect).expect("Should serialize");
    // Try to deserialize single effect as Vec - should fail or produce unexpected result
    let result: Result<Vec<HookEffect>, _> = bincode::deserialize(&encoded);
    // This may succeed with wrong data or fail - either way we handle it gracefully
    // The key is no panic occurs
    let _ = result;
}

/// Stress test with large number of effects
#[test]
fn test_large_effect_list() {
    const EFFECT_COUNT: usize = 1000;

    // Create a large list of mixed effects
    let mut effects: Vec<HookEffect> = Vec::with_capacity(EFFECT_COUNT);
    for i in 0..EFFECT_COUNT {
        let effect = match i % 5 {
            0 => HookEffect::SetEnv { key: format!("VAR_{}", i), value: format!("value_{}", i) },
            1 => HookEffect::ModifySysPath {
                action: SysPathAction::Prepend,
                path: format!("/path/to/module_{}", i),
            },
            2 => HookEffect::RegisterMarker {
                name: format!("marker_{}", i),
                description: format!("Description for marker {}", i),
            },
            3 => HookEffect::ModifyItems { removed: vec![], reordered: false },
            _ => HookEffect::NoEffect,
        };
        effects.push(effect);
    }

    // Serialize large list
    let encoded = bincode::serialize(&effects).expect("Should serialize large effect list");

    // Deserialize and verify
    let decoded: Vec<HookEffect> = bincode::deserialize(&encoded).expect("Should deserialize large effect list");

    assert_eq!(decoded.len(), EFFECT_COUNT);

    // Verify first and last effects
    match &decoded[0] {
        HookEffect::SetEnv { key, value } => {
            assert_eq!(key, "VAR_0");
            assert_eq!(value, "value_0");
        }
        _ => panic!("Expected SetEnv for index 0"),
    }

    match &decoded[EFFECT_COUNT - 1] {
        HookEffect::NoEffect => {} // 999 % 5 = 4, which maps to NoEffect
        _ => panic!("Expected NoEffect for last index"),
    }

    // Store in registry and verify retrieval
    let mut registry = HookRegistry::default();
    for effect in decoded {
        registry.record_effect("pytest_configure", effect);
    }
    assert_eq!(registry.get_effects("pytest_configure").len(), EFFECT_COUNT);
}

/// Test special characters in effect strings (Unicode, newlines, escape sequences)
#[test]
fn test_special_characters_in_effects() {
    let effects = vec![
        // Unicode characters
        HookEffect::SetEnv {
            key: "UNICODE_VAR".to_string(),
            value: "Hello 🚀 World ☃ Test".to_string(), // Rocket and snowman emoji
        },
        // Newlines and tabs
        HookEffect::SetEnv {
            key: "MULTILINE".to_string(),
            value: "line1\nline2\nline3\ttabbed".to_string(),
        },
        // Empty strings
        HookEffect::SetEnv { key: "".to_string(), value: "".to_string() },
        // Escape sequences and special chars
        HookEffect::ModifySysPath {
            action: SysPathAction::Append,
            path: "/path/with spaces/and\"quotes\"/and\\backslashes".to_string(),
        },
        // Japanese characters
        HookEffect::RegisterMarker {
            name: "test_marker".to_string(),
            description: "This is a test with Japanese: 日本語".to_string(),
        },
        // Null-like strings (not actual null bytes, but the word)
        HookEffect::SetEnv {
            key: "NULL_TEST".to_string(),
            value: "null\0embedded".to_string(), // Actual null byte
        },
        // Very long string
        HookEffect::SetEnv { key: "LONG_KEY".to_string(), value: "x".repeat(10000) },
        // Path with unusual but valid characters
        HookEffect::ModifySysPath {
            action: SysPathAction::Remove,
            path: "/tmp/test-file_name.2024@host#tag".to_string(),
        },
    ];

    // Serialize
    let encoded = bincode::serialize(&effects).expect("Should serialize special characters");

    // Deserialize
    let decoded: Vec<HookEffect> = bincode::deserialize(&encoded).expect("Should deserialize special characters");

    assert_eq!(decoded.len(), effects.len());

    // Verify Unicode preserved
    match &decoded[0] {
        HookEffect::SetEnv { value, .. } => {
            assert!(value.contains('🚀')); // Rocket emoji
            assert!(value.contains('☃')); // Snowman
        }
        _ => panic!("Expected SetEnv"),
    }

    // Verify newlines preserved
    match &decoded[1] {
        HookEffect::SetEnv { value, .. } => {
            assert!(value.contains('\n'));
            assert!(value.contains('\t'));
        }
        _ => panic!("Expected SetEnv"),
    }

    // Verify empty strings work
    match &decoded[2] {
        HookEffect::SetEnv { key, value } => {
            assert!(key.is_empty());
            assert!(value.is_empty());
        }
        _ => panic!("Expected SetEnv"),
    }

    // Verify long string preserved
    match &decoded[6] {
        HookEffect::SetEnv { value, .. } => {
            assert_eq!(value.len(), 10000);
        }
        _ => panic!("Expected SetEnv"),
    }
}
