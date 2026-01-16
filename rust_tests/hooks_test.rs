//! Integration tests for Hook Result types and Aggregation strategies
//!
//! Tests the HookResult type, AggregationStrategy enum, and aggregate_results function
//! introduced in v0.2.0 for the Hook Execution Framework.

use std::path::PathBuf;

use tach_core::hooks::{
    AggregationStrategy, HookEffect, HookResult, SysPathAction, aggregate_results,
};

// =============================================================================
// HookResult Construction Tests
// =============================================================================

#[test]
fn test_hook_result_new_creates_empty_result() {
    let result = HookResult::new();

    assert!(result.return_value.is_none());
    assert!(result.all_values.is_empty());
    assert!(result.effects.is_empty());
    assert!(result.source.is_none());
    assert!(result.error.is_none());
}

#[test]
fn test_hook_result_with_value_sets_return_value() {
    let result = HookResult::with_value(Some("test_value".to_string()));

    assert_eq!(result.return_value, Some("test_value".to_string()));
    assert!(result.all_values.is_empty());
    assert!(result.effects.is_empty());
    assert!(result.error.is_none());
}

#[test]
fn test_hook_result_with_value_none() {
    let result = HookResult::with_value(None);

    assert!(result.return_value.is_none());
}

#[test]
fn test_hook_result_with_error_sets_error_and_source() {
    let source = PathBuf::from("/project/conftest.py");
    let result = HookResult::with_error("Hook failed".to_string(), source.clone());

    assert_eq!(result.error, Some("Hook failed".to_string()));
    assert_eq!(result.source, Some(source));
    assert!(result.return_value.is_none());
    assert!(result.effects.is_empty());
}

#[test]
fn test_hook_result_add_effect() {
    let mut result = HookResult::new();

    result.add_effect(HookEffect::SetEnv {
        key: "TEST_VAR".to_string(),
        value: "test_value".to_string(),
    });

    assert_eq!(result.effects.len(), 1);
    assert!(matches!(
        &result.effects[0],
        HookEffect::SetEnv { key, value } if key == "TEST_VAR" && value == "test_value"
    ));
}

#[test]
fn test_hook_result_add_multiple_effects() {
    let mut result = HookResult::new();

    result.add_effect(HookEffect::SetEnv {
        key: "VAR1".to_string(),
        value: "value1".to_string(),
    });
    result.add_effect(HookEffect::ModifySysPath {
        action: SysPathAction::Prepend,
        path: "/custom/path".to_string(),
    });

    assert_eq!(result.effects.len(), 2);
}

// =============================================================================
// AggregationStrategy Tests
// =============================================================================

#[test]
fn test_aggregation_strategy_default_is_first_result() {
    let strategy = AggregationStrategy::default();
    assert_eq!(strategy, AggregationStrategy::FirstResult);
}

#[test]
fn test_aggregation_strategy_equality() {
    assert_eq!(
        AggregationStrategy::FirstResult,
        AggregationStrategy::FirstResult
    );
    assert_eq!(
        AggregationStrategy::AllResults,
        AggregationStrategy::AllResults
    );
    assert_eq!(AggregationStrategy::NoReturn, AggregationStrategy::NoReturn);
    assert_ne!(
        AggregationStrategy::FirstResult,
        AggregationStrategy::AllResults
    );
}

// =============================================================================
// aggregate_results Tests - FirstResult Strategy
// =============================================================================

#[test]
fn test_aggregate_first_result_returns_first_non_none() {
    let results = vec![
        HookResult::with_value(None),
        HookResult::with_value(Some("second".to_string())),
        HookResult::with_value(Some("third".to_string())),
    ];

    let aggregated = aggregate_results(&results, AggregationStrategy::FirstResult);

    assert_eq!(aggregated.return_value, Some("second".to_string()));
    assert!(aggregated.all_values.is_empty());
}

#[test]
fn test_aggregate_first_result_with_all_none() {
    let results = vec![HookResult::with_value(None), HookResult::with_value(None)];

    let aggregated = aggregate_results(&results, AggregationStrategy::FirstResult);

    assert!(aggregated.return_value.is_none());
}

#[test]
fn test_aggregate_first_result_empty_input() {
    let results: Vec<HookResult> = vec![];

    let aggregated = aggregate_results(&results, AggregationStrategy::FirstResult);

    assert!(aggregated.return_value.is_none());
    assert!(aggregated.effects.is_empty());
}

// =============================================================================
// aggregate_results Tests - AllResults Strategy
// =============================================================================

#[test]
fn test_aggregate_all_results_collects_all_values() {
    let results = vec![
        HookResult::with_value(Some("first".to_string())),
        HookResult::with_value(None),
        HookResult::with_value(Some("third".to_string())),
    ];

    let aggregated = aggregate_results(&results, AggregationStrategy::AllResults);

    assert!(aggregated.return_value.is_none()); // return_value not set for AllResults
    assert_eq!(aggregated.all_values.len(), 2);
    assert_eq!(aggregated.all_values[0], "first");
    assert_eq!(aggregated.all_values[1], "third");
}

#[test]
fn test_aggregate_all_results_empty_when_all_none() {
    let results = vec![HookResult::with_value(None), HookResult::with_value(None)];

    let aggregated = aggregate_results(&results, AggregationStrategy::AllResults);

    assert!(aggregated.all_values.is_empty());
}

// =============================================================================
// aggregate_results Tests - NoReturn Strategy
// =============================================================================

#[test]
fn test_aggregate_no_return_ignores_values() {
    let results = vec![
        HookResult::with_value(Some("first".to_string())),
        HookResult::with_value(Some("second".to_string())),
    ];

    let aggregated = aggregate_results(&results, AggregationStrategy::NoReturn);

    assert!(aggregated.return_value.is_none());
    assert!(aggregated.all_values.is_empty());
}

// =============================================================================
// aggregate_results Tests - Effect Aggregation
// =============================================================================

#[test]
fn test_aggregate_collects_all_effects() {
    let mut result1 = HookResult::new();
    result1.add_effect(HookEffect::SetEnv {
        key: "VAR1".to_string(),
        value: "value1".to_string(),
    });

    let mut result2 = HookResult::new();
    result2.add_effect(HookEffect::ModifySysPath {
        action: SysPathAction::Append,
        path: "/path1".to_string(),
    });
    result2.add_effect(HookEffect::ModifySysPath {
        action: SysPathAction::Prepend,
        path: "/path2".to_string(),
    });

    let results = vec![result1, result2];
    let aggregated = aggregate_results(&results, AggregationStrategy::NoReturn);

    assert_eq!(aggregated.effects.len(), 3);
}

#[test]
fn test_aggregate_preserves_effect_order() {
    let mut result1 = HookResult::new();
    result1.add_effect(HookEffect::SetEnv {
        key: "FIRST".to_string(),
        value: "1".to_string(),
    });

    let mut result2 = HookResult::new();
    result2.add_effect(HookEffect::SetEnv {
        key: "SECOND".to_string(),
        value: "2".to_string(),
    });

    let results = vec![result1, result2];
    let aggregated = aggregate_results(&results, AggregationStrategy::NoReturn);

    // Effects should be in order: result1's effects, then result2's effects
    assert!(matches!(
        &aggregated.effects[0],
        HookEffect::SetEnv { key, .. } if key == "FIRST"
    ));
    assert!(matches!(
        &aggregated.effects[1],
        HookEffect::SetEnv { key, .. } if key == "SECOND"
    ));
}

// =============================================================================
// aggregate_results Tests - Error Handling
// =============================================================================

#[test]
fn test_aggregate_captures_first_error() {
    let result1 = HookResult::with_error(
        "First error".to_string(),
        PathBuf::from("/first/conftest.py"),
    );
    let result2 = HookResult::with_error(
        "Second error".to_string(),
        PathBuf::from("/second/conftest.py"),
    );

    let results = vec![result1, result2];
    let aggregated = aggregate_results(&results, AggregationStrategy::FirstResult);

    assert_eq!(aggregated.error, Some("First error".to_string()));
    assert_eq!(aggregated.source, Some(PathBuf::from("/first/conftest.py")));
}

#[test]
fn test_aggregate_preserves_effects_when_error_occurs() {
    let mut result1 = HookResult::new();
    result1.add_effect(HookEffect::SetEnv {
        key: "BEFORE_ERROR".to_string(),
        value: "value".to_string(),
    });

    let result2 =
        HookResult::with_error("Error occurred".to_string(), PathBuf::from("/conftest.py"));

    let results = vec![result1, result2];
    let aggregated = aggregate_results(&results, AggregationStrategy::NoReturn);

    // Effects from before error should still be present
    assert_eq!(aggregated.effects.len(), 1);
    assert!(aggregated.error.is_some());
}

// =============================================================================
// Serialization Tests
// =============================================================================

#[test]
fn test_hook_result_serialization_roundtrip() {
    let mut result = HookResult::with_value(Some("test".to_string()));
    result.add_effect(HookEffect::SetEnv {
        key: "KEY".to_string(),
        value: "VALUE".to_string(),
    });
    result.source = Some(PathBuf::from("/project/conftest.py"));

    let json = serde_json::to_string(&result).expect("Should serialize");
    let deserialized: HookResult = serde_json::from_str(&json).expect("Should deserialize");

    assert_eq!(deserialized.return_value, result.return_value);
    assert_eq!(deserialized.effects.len(), 1);
    assert_eq!(deserialized.source, result.source);
}

#[test]
fn test_aggregation_strategy_serialization() {
    let strategies = vec![
        AggregationStrategy::FirstResult,
        AggregationStrategy::AllResults,
        AggregationStrategy::NoReturn,
    ];

    for strategy in strategies {
        let json = serde_json::to_string(&strategy).expect("Should serialize");
        let deserialized: AggregationStrategy =
            serde_json::from_str(&json).expect("Should deserialize");
        assert_eq!(deserialized, strategy);
    }
}

// =============================================================================
// Integration Tests - Combined Scenarios
// =============================================================================

#[test]
fn test_realistic_pytest_configure_aggregation() {
    // Simulate multiple conftest.py files each contributing effects
    let mut root_result = HookResult::new();
    root_result.add_effect(HookEffect::SetEnv {
        key: "PROJECT_ROOT".to_string(),
        value: "/project".to_string(),
    });
    root_result.source = Some(PathBuf::from("/project/conftest.py"));

    let mut tests_result = HookResult::new();
    tests_result.add_effect(HookEffect::ModifySysPath {
        action: SysPathAction::Prepend,
        path: "/project/src".to_string(),
    });
    tests_result.source = Some(PathBuf::from("/project/tests/conftest.py"));

    let mut subdir_result = HookResult::new();
    subdir_result.add_effect(HookEffect::RegisterMarker {
        name: "slow".to_string(),
        description: "Marks slow tests".to_string(),
    });
    subdir_result.source = Some(PathBuf::from("/project/tests/unit/conftest.py"));

    let results = vec![root_result, tests_result, subdir_result];

    // pytest_configure uses NoReturn (side-effect only)
    let aggregated = aggregate_results(&results, AggregationStrategy::NoReturn);

    assert_eq!(aggregated.effects.len(), 3);
    assert!(aggregated.return_value.is_none());
    assert!(aggregated.error.is_none());
}

#[test]
fn test_realistic_collection_modifyitems_aggregation() {
    // pytest_collection_modifyitems uses FirstResult
    let result1 = HookResult::with_value(None); // Root conftest doesn't modify
    let result2 = HookResult::with_value(Some(r#"["test_a", "test_b"]"#.to_string()));

    let results = vec![result1, result2];
    let aggregated = aggregate_results(&results, AggregationStrategy::FirstResult);

    assert_eq!(
        aggregated.return_value,
        Some(r#"["test_a", "test_b"]"#.to_string())
    );
}
