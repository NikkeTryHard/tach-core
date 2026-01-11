//! Integration tests for the discovery module
//!
//! These tests use the actual test fixtures in the project's tests/ directory
//! to verify discovery works correctly.

use std::path::Path;
use tach_core::discovery::discover;
use tempfile::TempDir;

/// Test discovery on the actual project's test fixtures
#[test]
fn test_discover_real_project_tests() {
    // Use the actual tach-core tests directory
    let project_root = Path::new(env!("CARGO_MANIFEST_DIR"));

    let result = discover(project_root, false).expect("Discovery should succeed on real project");

    // We know the project has at least 1000 tests (from the gauntlet)
    assert!(
        result.test_count() >= 100,
        "Should find many tests in real project, found {}",
        result.test_count()
    );

    // We know the project has fixtures
    assert!(
        result.fixture_count() >= 1,
        "Should find fixtures in real project, found {}",
        result.fixture_count()
    );
}

#[test]
fn test_discover_empty_temp_directory() {
    let temp_dir = TempDir::new().unwrap();

    // Initialize git so WalkBuilder doesn't apply default ignores
    std::fs::create_dir(temp_dir.path().join(".git")).unwrap();

    let result = discover(temp_dir.path(), false).expect("Discovery should succeed");

    assert_eq!(result.test_count(), 0, "Empty dir should have no tests");
    assert_eq!(
        result.fixture_count(),
        0,
        "Empty dir should have no fixtures"
    );
}

#[test]
fn test_discover_ignores_non_test_files() {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();

    // Initialize git
    std::fs::create_dir(root.join(".git")).unwrap();

    // Create non-test Python files (no test_ prefix)
    std::fs::write(root.join("utils.py"), "def helper(): pass").unwrap();
    std::fs::write(root.join("main.py"), "def main(): pass").unwrap();

    let result = discover(root, false).expect("Discovery should succeed");

    assert_eq!(result.test_count(), 0, "Non-test files should be ignored");
}

#[test]
fn test_discovery_result_accessors() {
    // Verify DiscoveryResult methods don't panic
    let project_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let result = discover(project_root, false).unwrap();

    // These should return reasonable values and not panic
    let test_count = result.test_count();
    let fixture_count = result.fixture_count();

    assert!(test_count > 0, "Should find tests");
    println!("Found {} tests, {} fixtures", test_count, fixture_count);
}

/// Test that specific test patterns are discovered
#[test]
fn test_discover_finds_specific_test_files() {
    let project_root = Path::new(env!("CARGO_MANIFEST_DIR"));

    let result = discover(project_root, false).expect("Discovery should succeed");

    // Check that we find tests from known test files
    let all_test_names: Vec<String> = result
        .modules
        .iter()
        .flat_map(|m| m.tests.iter().map(|t| t.name.clone()))
        .collect();

    // We should find some async tests
    let has_async_tests = result
        .modules
        .iter()
        .flat_map(|m| &m.tests)
        .any(|t| t.is_async);

    assert!(has_async_tests, "Should find at least one async test");

    // We should find class-based tests (TestClass::method format)
    let has_class_tests = all_test_names.iter().any(|n| n.contains("::"));
    assert!(has_class_tests, "Should find class-based tests");
}

/// Test that --no-ignore flag bypasses .ignore/.gitignore files
#[test]
fn test_discover_respects_no_ignore_flag() {
    use std::fs;
    use tempfile::tempdir;

    let dir = tempdir().unwrap();
    let test_file = dir.path().join("test_example.py");
    fs::write(&test_file, "def test_foo(): pass").unwrap();

    // Create .ignore that blocks all Python files
    let ignore_file = dir.path().join(".ignore");
    fs::write(&ignore_file, "*.py").unwrap();

    // Initialize git so WalkBuilder applies .ignore rules
    fs::create_dir(dir.path().join(".git")).unwrap();

    // Without no_ignore, should find nothing (blocked by .ignore)
    let result = discover(dir.path(), false).unwrap();
    assert!(
        result.modules.is_empty(),
        "Should find no tests with .ignore blocking, found {}",
        result.test_count()
    );

    // With no_ignore=true, should find the test
    let result = discover(dir.path(), true).unwrap();
    assert_eq!(
        result.modules.len(),
        1,
        "Should find test when ignoring .ignore"
    );
}

/// Test detection of dangerous patterns in .ignore that block Python file discovery
#[test]
fn test_detect_blocking_ignore_patterns() {
    use std::fs;
    use tempfile::tempdir;

    let dir = tempdir().unwrap();

    // Create .ignore with dangerous pattern
    let ignore_file = dir.path().join(".ignore");
    fs::write(&ignore_file, "*.py\n__pycache__/").unwrap();

    let patterns = tach_core::discovery::detect_blocking_patterns(dir.path());

    assert_eq!(patterns.len(), 1, "Should detect *.py as dangerous");
    assert!(patterns[0].contains("*.py"), "Should contain the pattern");
}

/// Test that safe patterns in .ignore are not flagged as dangerous
#[test]
fn test_detect_blocking_patterns_safe_ignore() {
    use std::fs;
    use tempfile::tempdir;

    let dir = tempdir().unwrap();

    // Create .ignore with only safe patterns
    let ignore_file = dir.path().join(".ignore");
    fs::write(&ignore_file, "__pycache__/\n.venv/\ntarget/").unwrap();

    let patterns = tach_core::discovery::detect_blocking_patterns(dir.path());

    assert!(patterns.is_empty(), "Safe patterns should not be flagged");
}

/// Test fixture scope parsing
#[test]
fn test_discover_fixture_scopes() {
    let project_root = Path::new(env!("CARGO_MANIFEST_DIR"));

    let result = discover(project_root, false).expect("Discovery should succeed");

    // Get all fixture scopes
    let scopes: Vec<_> = result
        .modules
        .iter()
        .flat_map(|m| &m.fixtures)
        .map(|f| f.scope.clone())
        .collect();

    // Should have at least one fixture
    assert!(
        !scopes.is_empty(),
        "Should find at least one fixture with a scope"
    );
}
