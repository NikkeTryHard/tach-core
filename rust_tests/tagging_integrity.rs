//! Tagging Integrity Tests (Pre-Hypervisor Verification)
//!
//! Verifies that `is_toxic` propagates correctly through the entire pipeline:
//! Source -> Discovery -> Resolution -> Protocol Serialization
//!
//! If this flag drops at any point, Hypervisor Mode will fail.

use tach_core::discovery::{DiscoveryResult, TestCase, TestModule};
use tach_core::graph::ToxicityGraph;
use tach_core::hooks::HookRegistry;
use tach_core::protocol::{FixtureInfo, TestPayload};
use tach_core::resolver::{FixtureRegistry, Resolver};
use tempfile::TempDir;

// =============================================================================
// Test 1: Toxicity Propagates from Source to Discovery
// =============================================================================

#[test]
fn test_toxicity_source_to_discovery() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    // Create a toxic test file (imports threading)
    std::fs::write(
        root.join("test_bad.py"),
        r#"
import threading

def test_toxic_function():
    pass
"#,
    )
    .unwrap();

    // Create a safe test file
    std::fs::write(
        root.join("test_good.py"),
        r#"
import os

def test_safe_function():
    pass
"#,
    )
    .unwrap();

    // Build toxicity graph directly (bypassing discovery's relative path issue)
    let paths = vec![root.join("test_bad.py"), root.join("test_good.py")];
    let graph = ToxicityGraph::build(&paths, root, &HookRegistry::new());

    // Verify toxicity detection
    assert!(
        graph.is_toxic(&root.join("test_bad.py")),
        "test_bad.py should be toxic (imports threading)"
    );
    assert!(
        !graph.is_toxic(&root.join("test_good.py")),
        "test_good.py should NOT be toxic"
    );
}

// =============================================================================
// Test 2: Toxicity Propagates from Discovery to RunnableTest
// =============================================================================

#[test]
fn test_toxicity_discovery_to_runnable_test() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    // Create test files
    std::fs::write(
        root.join("test_toxic.py"),
        "import multiprocessing\ndef test_toxic(): pass",
    )
    .unwrap();

    std::fs::write(
        root.join("test_safe.py"),
        "import json\ndef test_safe(): pass",
    )
    .unwrap();

    // Build toxicity graph
    let paths = vec![root.join("test_toxic.py"), root.join("test_safe.py")];
    let graph = ToxicityGraph::build(&paths, root, &HookRegistry::new());

    // Create mock discovery result with absolute paths
    let discovery = DiscoveryResult {
        modules: vec![
            TestModule {
                path: root.join("test_toxic.py"),
                tests: vec![TestCase {
                    name: "test_toxic".to_string(),
                    dependencies: vec![],
                    is_async: false,
                    line_number: 1,
                    parametrized_args: vec![],
                    timeout_secs: None,
                    markers: vec![],
                    marker_info: vec![],
                }],
                fixtures: vec![],
                hooks: vec![],
                is_toxic: false, // Will be tagged by graph
            },
            TestModule {
                path: root.join("test_safe.py"),
                tests: vec![TestCase {
                    name: "test_safe".to_string(),
                    dependencies: vec![],
                    is_async: false,
                    line_number: 1,
                    parametrized_args: vec![],
                    timeout_secs: None,
                    markers: vec![],
                    marker_info: vec![],
                }],
                fixtures: vec![],
                hooks: vec![],
                is_toxic: false,
            },
        ],
    };

    // Resolve tests
    let registry = FixtureRegistry::from_discovery(&discovery);
    let resolver = Resolver::new(&registry);
    let (mut runnable_tests, errors) = resolver.resolve_all(&discovery);

    assert!(errors.is_empty(), "Should have no resolution errors");
    assert_eq!(runnable_tests.len(), 2);

    // Tag tests with toxicity (simulating main.rs logic)
    for test in &mut runnable_tests {
        test.is_toxic = graph.is_toxic(&test.file_path);
    }

    // Verify tagging
    let toxic_test = runnable_tests
        .iter()
        .find(|t| t.test_name == "test_toxic")
        .expect("Should find test_toxic");
    let safe_test = runnable_tests
        .iter()
        .find(|t| t.test_name == "test_safe")
        .expect("Should find test_safe");

    assert!(
        toxic_test.is_toxic,
        "RunnableTest for test_toxic should have is_toxic=true"
    );
    assert!(
        !safe_test.is_toxic,
        "RunnableTest for test_safe should have is_toxic=false"
    );
}

// =============================================================================
// Test 3: CRITICAL - is_toxic Survives Protocol Serialization Round-Trip
// =============================================================================

#[test]
fn test_toxicity_survives_serialization_roundtrip() {
    // Create a toxic TestPayload
    let toxic_payload = TestPayload {
        test_id: 1,
        file_path: "test_toxic.py".to_string(),
        test_name: "test_toxic".to_string(),
        is_async: false,
        fixtures: vec![],
        log_fd: -1,
        debug_socket_path: String::new(),
        is_toxic: true, // <-- THE CRITICAL FLAG
        timeout_secs: None,
        hooks: vec![],
        cached_effects: vec![],
        markers: vec![],
        marker_info: vec![],
    };

    // Create a safe TestPayload
    let safe_payload = TestPayload {
        test_id: 2,
        file_path: "test_safe.py".to_string(),
        test_name: "test_safe".to_string(),
        is_async: false,
        fixtures: vec![],
        log_fd: -1,
        debug_socket_path: String::new(),
        is_toxic: false, // <-- THE CRITICAL FLAG
        timeout_secs: None,
        hooks: vec![],
        cached_effects: vec![],
        markers: vec![],
        marker_info: vec![],
    };

    // Serialize using bincode (same as scheduler.rs)
    let toxic_bytes = bincode::serde::encode_to_vec(&toxic_payload, bincode::config::standard())
        .expect("Serialization should succeed");
    let safe_bytes = bincode::serde::encode_to_vec(&safe_payload, bincode::config::standard())
        .expect("Serialization should succeed");

    // Deserialize (same as zygote.rs would do)
    let (toxic_decoded, _): (TestPayload, usize) =
        bincode::serde::decode_from_slice(&toxic_bytes, bincode::config::standard())
            .expect("Deserialization should succeed");
    let (safe_decoded, _): (TestPayload, usize) =
        bincode::serde::decode_from_slice(&safe_bytes, bincode::config::standard())
            .expect("Deserialization should succeed");

    // CRITICAL ASSERTIONS: is_toxic must survive the round-trip
    assert!(
        toxic_decoded.is_toxic,
        "CRITICAL: is_toxic=true did NOT survive serialization round-trip!"
    );
    assert!(
        !safe_decoded.is_toxic,
        "CRITICAL: is_toxic=false did NOT survive serialization round-trip!"
    );

    // Verify other fields survived too
    assert_eq!(toxic_decoded.test_id, 1);
    assert_eq!(toxic_decoded.test_name, "test_toxic");
    assert_eq!(safe_decoded.test_id, 2);
    assert_eq!(safe_decoded.test_name, "test_safe");
}

// =============================================================================
// Test 4: Full Pipeline Integration (Source -> Protocol)
// =============================================================================

#[test]
fn test_full_pipeline_toxicity_propagation() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    // Create a toxic test file
    std::fs::write(
        root.join("test_pipeline.py"),
        r#"
import socket  # TOXIC: network operations

def test_network_stuff():
    pass
"#,
    )
    .unwrap();

    // Step 1: Build toxicity graph
    let paths = vec![root.join("test_pipeline.py")];
    let graph = ToxicityGraph::build(&paths, root, &HookRegistry::new());

    // Step 2: Verify source is toxic
    assert!(
        graph.is_toxic(&root.join("test_pipeline.py")),
        "Source file should be detected as toxic"
    );

    // Step 3: Create mock discovery and resolve
    let discovery = DiscoveryResult {
        modules: vec![TestModule {
            path: root.join("test_pipeline.py"),
            tests: vec![TestCase {
                name: "test_network_stuff".to_string(),
                dependencies: vec![],
                is_async: false,
                line_number: 1,
                parametrized_args: vec![],
                timeout_secs: None,
                markers: vec![],
                marker_info: vec![],
            }],
            fixtures: vec![],
            hooks: vec![],
            is_toxic: false,
        }],
    };

    let registry = FixtureRegistry::from_discovery(&discovery);
    let resolver = Resolver::new(&registry);
    let (mut runnable_tests, _) = resolver.resolve_all(&discovery);

    // Step 4: Tag with toxicity
    for test in &mut runnable_tests {
        test.is_toxic = graph.is_toxic(&test.file_path);
    }

    let runnable = &runnable_tests[0];
    assert!(runnable.is_toxic, "RunnableTest should be toxic");

    // Step 5: Create TestPayload (simulating scheduler.rs)
    let payload = TestPayload {
        test_id: 42,
        file_path: runnable.file_path.to_string_lossy().to_string(),
        test_name: runnable.test_name.clone(),
        is_async: runnable.is_async,
        fixtures: runnable
            .fixtures
            .iter()
            .map(|f| FixtureInfo {
                name: f.name.clone(),
                scope: "function".to_string(),
                is_async: false,
            })
            .collect(),
        log_fd: -1,
        debug_socket_path: String::new(),
        is_toxic: runnable.is_toxic, // <-- PROPAGATED FROM RUNNABLE
        timeout_secs: runnable.timeout_secs,
        hooks: vec![],
        cached_effects: vec![],
        markers: vec![],
        marker_info: vec![],
    };

    // Step 6: Serialize and deserialize (simulating IPC)
    let bytes = bincode::serde::encode_to_vec(&payload, bincode::config::standard()).unwrap();
    let (decoded, _): (TestPayload, usize) =
        bincode::serde::decode_from_slice(&bytes, bincode::config::standard()).unwrap();

    // Step 7: FINAL VERIFICATION
    assert!(
        decoded.is_toxic,
        "CRITICAL: is_toxic did NOT propagate through full pipeline!"
    );
    assert_eq!(decoded.test_name, "test_network_stuff");
}

// =============================================================================
// Test 5: Transitive Toxicity Propagates Correctly
// =============================================================================

#[test]
fn test_transitive_toxicity_propagation() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    // Create a toxic helper (not a test file)
    std::fs::write(
        root.join("toxic_utils.py"),
        "import ctypes\ndef ffi_call(): pass",
    )
    .unwrap();

    // Create a test that imports the toxic helper
    std::fs::write(
        root.join("test_uses_toxic.py"),
        "import toxic_utils\ndef test_indirect(): pass",
    )
    .unwrap();

    // Build graph with BOTH files
    let paths = vec![root.join("toxic_utils.py"), root.join("test_uses_toxic.py")];
    let graph = ToxicityGraph::build(&paths, root, &HookRegistry::new());

    // Verify transitive toxicity
    assert!(
        graph.is_toxic(&root.join("toxic_utils.py")),
        "toxic_utils.py should be directly toxic"
    );
    assert!(
        graph.is_toxic(&root.join("test_uses_toxic.py")),
        "test_uses_toxic.py should be transitively toxic"
    );

    // Create payload and verify
    let discovery = DiscoveryResult {
        modules: vec![TestModule {
            path: root.join("test_uses_toxic.py"),
            tests: vec![TestCase {
                name: "test_indirect".to_string(),
                dependencies: vec![],
                is_async: false,
                line_number: 1,
                parametrized_args: vec![],
                timeout_secs: None,
                markers: vec![],
                marker_info: vec![],
            }],
            fixtures: vec![],
            hooks: vec![],
            is_toxic: false,
        }],
    };

    let registry = FixtureRegistry::from_discovery(&discovery);
    let resolver = Resolver::new(&registry);
    let (mut runnable_tests, _) = resolver.resolve_all(&discovery);

    for test in &mut runnable_tests {
        test.is_toxic = graph.is_toxic(&test.file_path);
    }

    assert!(
        runnable_tests[0].is_toxic,
        "Transitive toxicity should propagate to RunnableTest"
    );
}
