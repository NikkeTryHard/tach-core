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

/// Test that blocking patterns are detected even when some tests are found
/// This tests the "proactive warning" feature - patterns should be detected
/// regardless of whether discovery finds tests or not.
#[test]
fn test_detect_blocking_patterns_with_partial_block() {
    use std::fs;
    use tempfile::tempdir;

    let dir = tempdir().unwrap();

    // Create a test file that won't be blocked
    fs::write(dir.path().join("test_valid.py"), "def test_pass(): pass").unwrap();

    // Create .ignore that blocks a pattern (but not test_valid.py)
    let ignore_file = dir.path().join(".ignore");
    fs::write(&ignore_file, "test_blocked_*.py").unwrap();

    // Verify detection finds the blocking pattern
    let patterns = tach_core::discovery::detect_blocking_patterns(dir.path());
    assert!(!patterns.is_empty(), "Should detect blocking pattern");
    assert!(
        patterns.iter().any(|p| p.contains("test_blocked")),
        "Should contain the blocking pattern"
    );

    // Verify discovery still finds the valid test
    let result = discover(dir.path(), false).unwrap();
    assert!(
        !result.modules.is_empty(),
        "Should find test_valid.py despite blocking pattern"
    );
}

/// Test autouse fixture detection
#[test]
fn test_discover_autouse_fixture() {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();

    // Initialize git
    std::fs::create_dir(root.join(".git")).unwrap();

    // Create test file with autouse fixture
    std::fs::write(
        root.join("conftest.py"),
        r#"
import pytest

@pytest.fixture(autouse=True)
def setup_env():
    """This fixture runs automatically for every test."""
    pass

@pytest.fixture(autouse=True, scope="module")
def module_setup():
    """Module-scoped autouse fixture."""
    pass

@pytest.fixture
def regular_fixture():
    """Non-autouse fixture."""
    pass
"#,
    )
    .unwrap();

    std::fs::write(root.join("test_example.py"), "def test_something(): pass\n").unwrap();

    let result = discover(root, false).expect("Discovery should succeed");

    // Find the conftest module
    let conftest = result
        .modules
        .iter()
        .find(|m| m.path.ends_with("conftest.py"))
        .expect("Should find conftest.py");

    assert_eq!(conftest.fixtures.len(), 3, "Should find 3 fixtures");

    // Check autouse detection
    let setup_env = conftest
        .fixtures
        .iter()
        .find(|f| f.name == "setup_env")
        .unwrap();
    assert!(setup_env.autouse, "setup_env should be autouse=True");

    let module_setup = conftest
        .fixtures
        .iter()
        .find(|f| f.name == "module_setup")
        .unwrap();
    assert!(module_setup.autouse, "module_setup should be autouse=True");

    let regular = conftest
        .fixtures
        .iter()
        .find(|f| f.name == "regular_fixture")
        .unwrap();
    assert!(!regular.autouse, "regular_fixture should be autouse=False");
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

/// Test pytest hook detection in conftest.py
#[test]
fn test_discover_pytest_hooks_in_conftest() {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();

    std::fs::create_dir(root.join(".git")).unwrap();

    std::fs::write(
        root.join("conftest.py"),
        r#"
import pytest

def pytest_configure(config):
    """Called after command line options parsed."""
    config.addinivalue_line("markers", "slow: marks tests as slow")

def pytest_collection_modifyitems(config, items):
    """Called after collection is complete."""
    items.sort(key=lambda x: x.name)

def pytest_runtest_setup(item):
    """Called before each test."""
    pass

def not_a_hook():
    """Regular function, not a hook."""
    pass
"#,
    )
    .unwrap();

    let result = discover(root, false).expect("Discovery should succeed");

    let conftest = result
        .modules
        .iter()
        .find(|m| m.path.ends_with("conftest.py"))
        .expect("Should find conftest.py");

    // Check hooks are detected
    assert_eq!(conftest.hooks.len(), 3, "Should find 3 pytest hooks");

    let hook_names: Vec<&str> = conftest.hooks.iter().map(|h| h.name.as_str()).collect();
    assert!(hook_names.contains(&"pytest_configure"));
    assert!(hook_names.contains(&"pytest_collection_modifyitems"));
    assert!(hook_names.contains(&"pytest_runtest_setup"));
    assert!(!hook_names.contains(&"not_a_hook"));
}

/// Test nested TestClass detection behavior
/// This test documents whether nested classes (class inside class) are supported.
/// Pytest supports nested test classes, but static AST discovery may not fully support them.
#[test]
fn test_discover_nested_test_classes() {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();

    std::fs::create_dir(root.join(".git")).unwrap();

    // Create test file with nested classes
    std::fs::write(
        root.join("test_nested.py"),
        r#"
class TestOuter:
    """Outer test class."""

    def test_outer_method(self):
        pass

    class TestInner:
        """Nested test class - pytest supports this."""

        def test_inner_method(self):
            pass

        def test_another_inner(self):
            pass
"#,
    )
    .unwrap();

    let result = discover(root, false).expect("Discovery should succeed");

    let module = result
        .modules
        .iter()
        .find(|m| m.path.ends_with("test_nested.py"))
        .expect("Should find test_nested.py");

    // Pytest discovers nested classes - verify our behavior
    // Note: Current implementation may or may not support nested classes
    // This test documents the current behavior
    let test_names: Vec<&str> = module.tests.iter().map(|t| t.name.as_str()).collect();

    // At minimum, outer class tests should be found
    assert!(
        test_names.contains(&"TestOuter::test_outer_method"),
        "Should find outer class test, found: {:?}",
        test_names
    );

    // Document whether nested classes are supported
    let has_nested = test_names.iter().any(|n| n.contains("TestInner"));

    // Assert current behavior: nested TestClass is NOT supported
    // If this starts passing, update docs to reflect new capability
    assert!(
        !has_nested,
        "Nested TestClass support has changed - update documentation if this is intentional"
    );

    println!(
        "Nested TestClass support: {}",
        if has_nested {
            "YES"
        } else {
            "NO (known limitation)"
        }
    );
}

/// Test that discovery can populate a HookRegistry from discovered hooks
#[test]
fn test_discovery_populates_hook_registry() {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();

    std::fs::create_dir(root.join(".git")).unwrap();

    std::fs::write(
        root.join("conftest.py"),
        r#"
def pytest_configure(config):
    pass
"#,
    )
    .unwrap();

    std::fs::write(root.join("test_example.py"), "def test_one(): pass\n").unwrap();

    let result = discover(root, false).expect("Discovery should succeed");

    // Build hook registry from discovery result
    let registry = result.build_hook_registry();

    assert_eq!(registry.hook_count(), 1);
    assert!(registry.has_global_state_hooks());
}

/// Test that hooks are only detected in conftest.py files, not regular test files
/// pytest only processes hooks from conftest.py, so we should not detect them elsewhere
#[test]
fn test_hooks_only_detected_in_conftest() {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();

    std::fs::create_dir(root.join(".git")).unwrap();

    // Hook in conftest.py - should be detected
    std::fs::write(
        root.join("conftest.py"),
        "def pytest_configure(config): pass\n",
    )
    .unwrap();

    // Hook in regular test file - should NOT be detected
    std::fs::write(
        root.join("test_example.py"),
        r#"
def pytest_configure(config):
    pass  # This should be ignored - not in conftest

def test_something():
    pass
"#,
    )
    .unwrap();

    let result = discover(root, false).expect("Discovery should succeed");

    // conftest.py should have the hook
    let conftest = result
        .modules
        .iter()
        .find(|m| m.path.ends_with("conftest.py"))
        .unwrap();
    assert_eq!(conftest.hooks.len(), 1, "conftest.py should have 1 hook");

    // test_example.py should NOT have hooks
    let test_file = result
        .modules
        .iter()
        .find(|m| m.path.ends_with("test_example.py"))
        .unwrap();
    assert_eq!(
        test_file.hooks.len(),
        0,
        "test_example.py should have 0 hooks (hooks only in conftest)"
    );
}

/// Test detection of @pytest.mark.django_db and other pytest markers
#[test]
fn test_discover_django_db_marker() {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();

    std::fs::create_dir(root.join(".git")).unwrap();

    std::fs::write(
        root.join("test_django.py"),
        r#"
import pytest

@pytest.mark.django_db
def test_with_db():
    pass

@pytest.mark.django_db(transaction=True)
def test_with_transaction():
    pass

@pytest.mark.django_db(reset_sequences=True)
def test_with_reset():
    pass

def test_without_db():
    pass
"#,
    )
    .unwrap();

    let result = discover(root, false).expect("Discovery should succeed");

    let module = result
        .modules
        .iter()
        .find(|m| m.path.ends_with("test_django.py"))
        .expect("Should find test_django.py");

    // Find tests and check django_db marker
    let with_db = module
        .tests
        .iter()
        .find(|t| t.name == "test_with_db")
        .unwrap();
    assert!(with_db.markers.contains(&"django_db".to_string()));

    let with_transaction = module
        .tests
        .iter()
        .find(|t| t.name == "test_with_transaction")
        .unwrap();
    assert!(with_transaction.markers.contains(&"django_db".to_string()));

    let with_reset = module
        .tests
        .iter()
        .find(|t| t.name == "test_with_reset")
        .unwrap();
    assert!(with_reset.markers.contains(&"django_db".to_string()));

    let without_db = module
        .tests
        .iter()
        .find(|t| t.name == "test_without_db")
        .unwrap();
    assert!(!without_db.markers.contains(&"django_db".to_string()));
}

/// Test that decorator-only markers are excluded from the markers list
/// parametrize, usefixtures, filterwarnings are not test selection markers
#[test]
fn test_markers_exclude_decorator_only_markers() {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();

    std::fs::create_dir(root.join(".git")).unwrap();

    std::fs::write(
        root.join("test_markers.py"),
        r#"
import pytest

@pytest.mark.django_db
@pytest.mark.slow
@pytest.mark.parametrize("x", [1, 2])
@pytest.mark.usefixtures("some_fixture")
@pytest.mark.filterwarnings("ignore::DeprecationWarning")
def test_many_markers(x):
    pass
"#,
    )
    .unwrap();

    let result = discover(root, false).expect("Discovery should succeed");
    let module = result
        .modules
        .iter()
        .find(|m| m.path.ends_with("test_markers.py"))
        .unwrap();
    let test = module
        .tests
        .iter()
        .find(|t| t.name == "test_many_markers")
        .unwrap();

    // Should include real markers
    assert!(test.markers.contains(&"django_db".to_string()));
    assert!(test.markers.contains(&"slow".to_string()));

    // Should exclude decorator-only markers
    assert!(
        !test.markers.contains(&"parametrize".to_string()),
        "parametrize should be filtered"
    );
    assert!(
        !test.markers.contains(&"usefixtures".to_string()),
        "usefixtures should be filtered"
    );
    assert!(
        !test.markers.contains(&"filterwarnings".to_string()),
        "filterwarnings should be filtered"
    );
}
