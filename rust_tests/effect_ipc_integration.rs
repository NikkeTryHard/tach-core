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
            description: Some("marks tests as slow".to_string()),
        },
        HookEffect::NoEffect,
    ];

    // Serialize effects (as Zygote does)
    let encoded = bincode::serialize(&effects).expect("Should serialize effects");

    // Deserialize effects (as Supervisor does)
    let decoded: Vec<HookEffect> =
        bincode::deserialize(&encoded).expect("Should deserialize effects");

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

    registry.record_effect(effect1.clone());
    registry.record_effect(effect2.clone());

    // Retrieve effects (as Scheduler does for worker dispatch)
    let stored_effects = registry.get_effects();

    assert_eq!(stored_effects.len(), 2);
    assert_eq!(stored_effects[0], effect1);
    assert_eq!(stored_effects[1], effect2);
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
            description: Some("marks integration tests".to_string()),
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
    let received_effects: Vec<HookEffect> =
        bincode::deserialize(&wire_data).expect("Supervisor should deserialize effects");

    // Store in HookRegistry
    let mut registry = HookRegistry::default();
    for effect in received_effects {
        registry.record_effect(effect);
    }

    // === SCHEDULER/WORKER SIDE ===
    // Retrieve effects for worker application
    let worker_effects = registry.get_effects();

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
            assert_eq!(description.as_deref(), Some("marks integration tests"));
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
    let actions = vec![
        SysPathAction::Prepend,
        SysPathAction::Append,
        SysPathAction::Remove,
    ];

    for action in actions {
        let encoded = bincode::serialize(&action).expect("Should serialize action");
        let decoded: SysPathAction =
            bincode::deserialize(&encoded).expect("Should deserialize action");
        assert_eq!(action, decoded);
    }
}

/// Test that empty effects list serializes correctly
#[test]
fn test_empty_effects_ipc() {
    let empty_effects: Vec<HookEffect> = vec![];

    let encoded = bincode::serialize(&empty_effects).expect("Should serialize empty vec");
    let decoded: Vec<HookEffect> =
        bincode::deserialize(&encoded).expect("Should deserialize empty vec");

    assert!(decoded.is_empty());
}

/// Test ModifyItems effect (used for test collection modification)
#[test]
fn test_modify_items_effect_ipc() {
    let effect = HookEffect::ModifyItems {
        added: vec!["test_new.py::test_added".to_string()],
        removed: vec!["test_old.py::test_removed".to_string()],
        reordered: true,
    };

    let encoded = bincode::serialize(&effect).expect("Should serialize ModifyItems");
    let decoded: HookEffect =
        bincode::deserialize(&encoded).expect("Should deserialize ModifyItems");

    match decoded {
        HookEffect::ModifyItems {
            added,
            removed,
            reordered,
        } => {
            assert_eq!(added, vec!["test_new.py::test_added"]);
            assert_eq!(removed, vec!["test_old.py::test_removed"]);
            assert!(reordered);
        }
        _ => panic!("Expected ModifyItems effect"),
    }
}
