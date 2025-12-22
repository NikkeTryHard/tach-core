//! Integration tests for the resolver module

use std::path::PathBuf;
use tach_core::discovery::{
    DiscoveryResult, FixtureDefinition, FixtureScope, TestCase, TestModule,
};
use tach_core::resolver::{FixtureRegistry, ResolutionError, Resolver};

fn create_test_discovery() -> DiscoveryResult {
    DiscoveryResult {
        modules: vec![
            // Module with fixtures (simulating conftest.py)
            TestModule {
                path: PathBuf::from("conftest.py"),
                tests: vec![],
                fixtures: vec![
                    FixtureDefinition {
                        name: "db".to_string(),
                        scope: FixtureScope::Module,
                        dependencies: vec![],
                    },
                    FixtureDefinition {
                        name: "cache".to_string(),
                        scope: FixtureScope::Function,
                        dependencies: vec!["db".to_string()],
                    },
                    FixtureDefinition {
                        name: "client".to_string(),
                        scope: FixtureScope::Function,
                        dependencies: vec!["db".to_string(), "cache".to_string()],
                    },
                ],
            },
            // Module with tests
            TestModule {
                path: PathBuf::from("tests/test_example.py"),
                tests: vec![
                    TestCase {
                        name: "test_simple".to_string(),
                        dependencies: vec![],
                        is_async: false,
                        line_number: 1,
                    },
                    TestCase {
                        name: "test_with_db".to_string(),
                        dependencies: vec!["db".to_string()],
                        is_async: false,
                        line_number: 1,
                    },
                    TestCase {
                        name: "test_with_client".to_string(),
                        dependencies: vec!["client".to_string()],
                        is_async: true,
                        line_number: 1,
                    },
                ],
                fixtures: vec![],
            },
        ],
    }
}

#[test]
fn test_fixture_registry_creation() {
    let discovery = create_test_discovery();
    let registry = FixtureRegistry::from_discovery(&discovery);

    // Registry should exist and be usable
    let resolver = Resolver::new(&registry);
    let (tests, errors) = resolver.resolve_all(&discovery);

    // Should resolve all 3 tests
    assert_eq!(tests.len(), 3, "Should resolve all tests");
    assert!(errors.is_empty(), "Should have no resolution errors");
}

#[test]
fn test_resolve_test_with_dependencies() {
    let discovery = create_test_discovery();
    let registry = FixtureRegistry::from_discovery(&discovery);
    let resolver = Resolver::new(&registry);
    let (tests, _) = resolver.resolve_all(&discovery);

    // Find the test with client dependency
    let test_with_client = tests.iter().find(|t| t.test_name == "test_with_client");
    assert!(test_with_client.is_some(), "Should find test_with_client");

    let resolved = test_with_client.unwrap();
    // Should have resolved fixtures in order (db -> cache -> client)
    assert!(
        !resolved.fixtures.is_empty(),
        "Should have resolved fixtures"
    );
}

#[test]
fn test_resolve_test_without_dependencies() {
    let discovery = create_test_discovery();
    let registry = FixtureRegistry::from_discovery(&discovery);
    let resolver = Resolver::new(&registry);
    let (tests, _) = resolver.resolve_all(&discovery);

    // Find the simple test
    let simple_test = tests.iter().find(|t| t.test_name == "test_simple");
    assert!(simple_test.is_some(), "Should find test_simple");

    let resolved = simple_test.unwrap();
    assert!(
        resolved.fixtures.is_empty(),
        "Simple test should have no fixtures"
    );
}

#[test]
fn test_missing_fixture_error() {
    let discovery = DiscoveryResult {
        modules: vec![TestModule {
            path: PathBuf::from("test.py"),
            tests: vec![TestCase {
                name: "test_with_missing".to_string(),
                dependencies: vec!["nonexistent_fixture".to_string()],
                is_async: false,
                line_number: 1,
            }],
            fixtures: vec![],
        }],
    };

    let registry = FixtureRegistry::from_discovery(&discovery);
    let resolver = Resolver::new(&registry);
    let (tests, errors) = resolver.resolve_all(&discovery);

    // Should have one error for missing fixture
    assert_eq!(errors.len(), 1, "Should have one error");
    assert!(
        matches!(&errors[0], ResolutionError::MissingFixture { .. }),
        "Should be MissingFixture error"
    );

    // Test should not be in resolved list
    assert!(
        tests.is_empty(),
        "Test with missing fixture should not be resolved"
    );
}

#[test]
fn test_cyclic_dependency_error() {
    let discovery = DiscoveryResult {
        modules: vec![TestModule {
            path: PathBuf::from("conftest.py"),
            tests: vec![TestCase {
                name: "test_cyclic".to_string(),
                dependencies: vec!["fixture_a".to_string()],
                is_async: false,
                line_number: 1,
            }],
            fixtures: vec![
                FixtureDefinition {
                    name: "fixture_a".to_string(),
                    scope: FixtureScope::Function,
                    dependencies: vec!["fixture_b".to_string()],
                },
                FixtureDefinition {
                    name: "fixture_b".to_string(),
                    scope: FixtureScope::Function,
                    dependencies: vec!["fixture_a".to_string()],
                },
            ],
        }],
    };

    let registry = FixtureRegistry::from_discovery(&discovery);
    let resolver = Resolver::new(&registry);
    let (tests, errors) = resolver.resolve_all(&discovery);

    // Should detect cycle
    assert_eq!(errors.len(), 1, "Should have one error");
    assert!(
        matches!(&errors[0], ResolutionError::CyclicDependency { .. }),
        "Should be CyclicDependency error"
    );
    assert!(tests.is_empty(), "Test with cycle should not be resolved");
}

#[test]
fn test_empty_discovery() {
    let discovery = DiscoveryResult { modules: vec![] };

    let registry = FixtureRegistry::from_discovery(&discovery);
    let resolver = Resolver::new(&registry);
    let (tests, errors) = resolver.resolve_all(&discovery);

    assert!(tests.is_empty(), "No tests in empty discovery");
    assert!(errors.is_empty(), "No errors in empty discovery");
}

#[test]
fn test_async_flag_preserved() {
    let discovery = create_test_discovery();
    let registry = FixtureRegistry::from_discovery(&discovery);
    let resolver = Resolver::new(&registry);
    let (tests, _) = resolver.resolve_all(&discovery);

    // Check async flags are preserved
    let async_test = tests.iter().find(|t| t.test_name == "test_with_client");
    assert!(async_test.is_some());
    assert!(async_test.unwrap().is_async, "Should preserve async flag");

    let sync_test = tests.iter().find(|t| t.test_name == "test_simple");
    assert!(sync_test.is_some());
    assert!(!sync_test.unwrap().is_async, "Should preserve sync flag");
}

#[test]
fn test_fixture_order_is_topological() {
    let discovery = create_test_discovery();
    let registry = FixtureRegistry::from_discovery(&discovery);
    let resolver = Resolver::new(&registry);
    let (tests, _) = resolver.resolve_all(&discovery);

    // Find the test with client dependency
    let test_with_client = tests.iter().find(|t| t.test_name == "test_with_client");
    let resolved = test_with_client.unwrap();

    // Fixtures should be in dependency order: db first, then cache, then client
    let fixture_names: Vec<_> = resolved.fixtures.iter().map(|f| f.name.as_str()).collect();

    // db should come before cache
    let db_pos = fixture_names.iter().position(|n| *n == "db");
    let cache_pos = fixture_names.iter().position(|n| *n == "cache");
    let client_pos = fixture_names.iter().position(|n| *n == "client");

    assert!(db_pos.is_some(), "Should have db fixture");
    assert!(cache_pos.is_some(), "Should have cache fixture");
    assert!(client_pos.is_some(), "Should have client fixture");

    assert!(
        db_pos.unwrap() < cache_pos.unwrap(),
        "db should come before cache"
    );
    assert!(
        cache_pos.unwrap() < client_pos.unwrap(),
        "cache should come before client"
    );
}
