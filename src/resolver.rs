//! Dependency Resolution & Graph Construction
//! Resolves fixture dependencies and builds execution order.

use crate::discovery::{DiscoveryResult, FixtureDefinition, FixtureScope, TestCase, TestModule};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

/// A fully resolved test ready for execution
#[derive(Debug, Clone)]
pub struct RunnableTest {
    pub file_path: PathBuf,
    pub test_name: String,
    pub is_async: bool,
    /// Fixtures in topological order (dependencies first)
    pub fixtures: Vec<ResolvedFixture>,
}

/// A resolved fixture with full context
#[derive(Debug, Clone)]
pub struct ResolvedFixture {
    pub name: String,
    pub source_file: PathBuf,
    pub scope: FixtureScope,
}

/// Error types for resolution failures
#[derive(Debug)]
pub enum ResolutionError {
    MissingFixture { test: String, fixture: String },
    CyclicDependency { test: String, cycle: Vec<String> },
}

/// pytest builtin fixtures that are provided at runtime, not discovered statically.
/// These are injected by pytest's fixture machinery, not user-defined.
const PYTEST_BUILTINS: &[&str] = &[
    // Monkey-patching and environment
    "monkeypatch",
    // Temporary directories
    "tmp_path",
    "tmp_path_factory",
    "tmpdir",
    "tmpdir_factory",
    // Output capture
    "capsys",
    "capfd",
    "capsysbinary",
    "capfdbinary",
    "caplog",
    // Fixture metadata
    "request",
    // Caching
    "cache",
    // Recording
    "record_property",
    "record_testsuite_property",
    "record_xml_attribute",
    // Doctest
    "doctest_namespace",
    // Recwarn
    "recwarn",
    // Pytestconfig
    "pytestconfig",
];

/// Check if a fixture name is a pytest builtin
fn is_builtin_fixture(name: &str) -> bool {
    PYTEST_BUILTINS.contains(&name)
}

/// Registry holding all discovered fixtures
pub struct FixtureRegistry {
    /// Global fixtures from conftest.py files
    global: HashMap<String, (FixtureDefinition, PathBuf)>,
    /// Local fixtures per module (non-class-scoped only)
    local: HashMap<PathBuf, HashMap<String, FixtureDefinition>>,
    /// Class-scoped fixtures: (module_path, class_name) -> fixture_name -> fixture
    class_scoped: HashMap<(PathBuf, String), HashMap<String, FixtureDefinition>>,
}

impl FixtureRegistry {
    /// Build registry from discovery results
    pub fn from_discovery(result: &DiscoveryResult) -> Self {
        let mut global = HashMap::new();
        let mut local = HashMap::new();
        let mut class_scoped: HashMap<(PathBuf, String), HashMap<String, FixtureDefinition>> =
            HashMap::new();

        for module in &result.modules {
            let is_conftest = module
                .path
                .file_name()
                .map_or(false, |n| n == "conftest.py");

            let mut module_fixtures = HashMap::new();
            for fixture in &module.fixtures {
                // Phase 7c: Handle class-scoped fixtures
                if let Some(ref class_name) = fixture.class_scope {
                    let key = (module.path.clone(), class_name.clone());
                    class_scoped
                        .entry(key)
                        .or_default()
                        .insert(fixture.name.clone(), fixture.clone());
                } else if is_conftest {
                    global.insert(fixture.name.clone(), (fixture.clone(), module.path.clone()));
                } else {
                    module_fixtures.insert(fixture.name.clone(), fixture.clone());
                }
            }

            if !module_fixtures.is_empty() {
                local.insert(module.path.clone(), module_fixtures);
            }
        }

        Self {
            global,
            local,
            class_scoped,
        }
    }

    /// Look up a fixture: class scope -> local scope -> global scope
    fn lookup(
        &self,
        name: &str,
        module_path: &PathBuf,
        test_name: &str,
    ) -> Option<(FixtureDefinition, PathBuf)> {
        // Phase 7c: Check class-scoped fixtures first for tests in classes
        // Test names in classes have format "ClassName::method_name"
        if let Some(class_name) = test_name.split("::").next() {
            if class_name.starts_with("Test") && test_name.contains("::") {
                let key = (module_path.clone(), class_name.to_string());
                if let Some(class_fixtures) = self.class_scoped.get(&key) {
                    if let Some(fixture) = class_fixtures.get(name) {
                        return Some((fixture.clone(), module_path.clone()));
                    }
                }
            }
        }

        // Check local module scope
        if let Some(local_fixtures) = self.local.get(module_path) {
            if let Some(fixture) = local_fixtures.get(name) {
                return Some((fixture.clone(), module_path.clone()));
            }
        }
        // Fall back to global scope
        self.global.get(name).cloned()
    }
}

/// Resolver engine
pub struct Resolver<'a> {
    registry: &'a FixtureRegistry,
}

impl<'a> Resolver<'a> {
    pub fn new(registry: &'a FixtureRegistry) -> Self {
        Self { registry }
    }

    /// Resolve all tests from discovery results
    pub fn resolve_all(
        &self,
        result: &DiscoveryResult,
    ) -> (Vec<RunnableTest>, Vec<ResolutionError>) {
        let mut runnable = Vec::new();
        let mut errors = Vec::new();

        for module in &result.modules {
            for test in &module.tests {
                match self.resolve_test(test, &module.path) {
                    Ok(resolved) => runnable.push(resolved),
                    Err(e) => errors.push(e),
                }
            }
        }

        (runnable, errors)
    }

    /// Resolve a single test's fixture dependencies
    fn resolve_test(
        &self,
        test: &TestCase,
        module_path: &PathBuf,
    ) -> Result<RunnableTest, ResolutionError> {
        let mut resolved_fixtures = Vec::new();
        let mut visited = HashSet::new();
        let mut stack = Vec::new();

        // Phase 7b: Filter out parametrized args - they're NOT fixtures
        // @pytest.mark.parametrize("arg") injects arg from the decorator, not fixture system
        let parametrized_set: HashSet<_> = test.parametrized_args.iter().collect();

        // Resolve each direct dependency (excluding parametrized args)
        for dep_name in &test.dependencies {
            // Skip if this is a parametrized arg (NOT a fixture)
            if parametrized_set.contains(dep_name) {
                continue;
            }

            self.resolve_fixture(
                dep_name,
                module_path,
                &test.name,
                &mut resolved_fixtures,
                &mut visited,
                &mut stack,
            )?;
        }

        Ok(RunnableTest {
            file_path: module_path.clone(),
            test_name: test.name.clone(),
            is_async: test.is_async,
            fixtures: resolved_fixtures,
        })
    }

    /// Recursively resolve a fixture and its dependencies (DFS with cycle detection)
    fn resolve_fixture(
        &self,
        name: &str,
        module_path: &PathBuf,
        test_name: &str,
        resolved: &mut Vec<ResolvedFixture>,
        visited: &mut HashSet<String>,
        stack: &mut Vec<String>,
    ) -> Result<(), ResolutionError> {
        // Already fully resolved
        if visited.contains(name) {
            return Ok(());
        }

        // Cycle detection
        if stack.contains(&name.to_string()) {
            stack.push(name.to_string());
            return Err(ResolutionError::CyclicDependency {
                test: test_name.to_string(),
                cycle: stack.clone(),
            });
        }

        // PHASE 6: Skip resolution for pytest builtin fixtures
        // These are provided by pytest at runtime, not discovered statically.
        // We mark them as visited and continue - pytest will inject them.
        if is_builtin_fixture(name) {
            visited.insert(name.to_string());
            return Ok(());
        }

        // Look up fixture (pass test_name for class-scoped lookup)
        let (fixture, source_file) = self
            .registry
            .lookup(name, module_path, test_name)
            .ok_or_else(|| ResolutionError::MissingFixture {
                test: test_name.to_string(),
                fixture: name.to_string(),
            })?;

        // Push onto recursion stack
        stack.push(name.to_string());

        // Resolve transitive dependencies first
        for dep in &fixture.dependencies {
            self.resolve_fixture(dep, module_path, test_name, resolved, visited, stack)?;
        }

        // Pop from stack
        stack.pop();

        // Mark as visited and add to resolved list
        visited.insert(name.to_string());
        resolved.push(ResolvedFixture {
            name: name.to_string(),
            source_file,
            scope: fixture.scope,
        });

        Ok(())
    }
}

// =============================================================================
// Unit Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to create a fixture definition
    fn make_fixture(name: &str, deps: Vec<&str>) -> FixtureDefinition {
        FixtureDefinition {
            name: name.to_string(),
            scope: FixtureScope::Function,
            dependencies: deps.into_iter().map(|s| s.to_string()).collect(),
            params: None,
            class_scope: None,
        }
    }

    /// Helper to create a test case
    fn make_test(name: &str, deps: Vec<&str>) -> TestCase {
        TestCase {
            name: name.to_string(),
            dependencies: deps.into_iter().map(|s| s.to_string()).collect(),
            is_async: false,
            line_number: 1,
            parametrized_args: vec![],
        }
    }

    #[test]
    fn test_fixture_lookup_local_over_global() {
        // Create discovery with both global (conftest.py) and local fixtures
        let discovery = DiscoveryResult {
            modules: vec![
                // Global conftest.py with a "db" fixture (no dependencies)
                TestModule {
                    path: PathBuf::from("conftest.py"),
                    tests: vec![],
                    fixtures: vec![make_fixture("db", vec![])],
                },
                // Local module with same-named "db" fixture (has dependencies)
                TestModule {
                    path: PathBuf::from("test_local.py"),
                    tests: vec![],
                    fixtures: vec![make_fixture("db", vec!["connection"])],
                },
            ],
        };

        let registry = FixtureRegistry::from_discovery(&discovery);

        // Local lookup should return local fixture (has deps)
        // Using "test_simple" as test_name (no class scope)
        let local_path = PathBuf::from("test_local.py");
        let (fixture, _) = registry.lookup("db", &local_path, "test_simple").unwrap();
        assert!(
            !fixture.dependencies.is_empty(),
            "Local fixture should have dependencies"
        );

        // Other module lookup should return global fixture (no deps)
        let other_path = PathBuf::from("test_other.py");
        let (fixture, _) = registry.lookup("db", &other_path, "test_simple").unwrap();
        assert!(
            fixture.dependencies.is_empty(),
            "Global fixture should have no dependencies"
        );
    }

    #[test]
    fn test_cycle_detection() {
        // Create a cyclic dependency: a -> b -> a
        let discovery = DiscoveryResult {
            modules: vec![
                TestModule {
                    path: PathBuf::from("conftest.py"),
                    tests: vec![],
                    fixtures: vec![
                        make_fixture("a", vec!["b"]),
                        make_fixture("b", vec!["a"]), // Cycle!
                    ],
                },
                TestModule {
                    path: PathBuf::from("test_cycle.py"),
                    tests: vec![make_test("test_foo", vec!["a"])],
                    fixtures: vec![],
                },
            ],
        };

        let registry = FixtureRegistry::from_discovery(&discovery);
        let resolver = Resolver::new(&registry);
        let (runnable, errors) = resolver.resolve_all(&discovery);

        // Should have no runnable tests and one error
        assert!(
            runnable.is_empty(),
            "Cyclic dependency should fail resolution"
        );
        assert!(!errors.is_empty(), "Should have resolution error");

        // Verify it's a CyclicDependency error
        match &errors[0] {
            ResolutionError::CyclicDependency { cycle, .. } => {
                assert!(cycle.contains(&"a".to_string()), "Cycle should contain 'a'");
                assert!(cycle.contains(&"b".to_string()), "Cycle should contain 'b'");
            }
            _ => panic!("Expected CyclicDependency error"),
        }
    }

    #[test]
    fn test_missing_fixture_error() {
        // Create a test that depends on a non-existent fixture
        let discovery = DiscoveryResult {
            modules: vec![TestModule {
                path: PathBuf::from("test_missing.py"),
                tests: vec![make_test("test_foo", vec!["nonexistent"])],
                fixtures: vec![],
            }],
        };

        let registry = FixtureRegistry::from_discovery(&discovery);
        let resolver = Resolver::new(&registry);
        let (runnable, errors) = resolver.resolve_all(&discovery);

        // Should have no runnable tests and one error
        assert!(
            runnable.is_empty(),
            "Missing fixture should fail resolution"
        );
        assert!(!errors.is_empty(), "Should have resolution error");

        // Verify it's a MissingFixture error
        match &errors[0] {
            ResolutionError::MissingFixture { fixture, test } => {
                assert_eq!(fixture, "nonexistent");
                assert_eq!(test, "test_foo");
            }
            _ => panic!("Expected MissingFixture error"),
        }
    }

    #[test]
    fn test_transitive_dependency_resolution() {
        // Create a chain: test_foo -> db -> connection -> base
        let discovery = DiscoveryResult {
            modules: vec![
                TestModule {
                    path: PathBuf::from("conftest.py"),
                    tests: vec![],
                    fixtures: vec![
                        make_fixture("base", vec![]),
                        make_fixture("connection", vec!["base"]),
                        make_fixture("db", vec!["connection"]),
                    ],
                },
                TestModule {
                    path: PathBuf::from("test_chain.py"),
                    tests: vec![make_test("test_foo", vec!["db"])],
                    fixtures: vec![],
                },
            ],
        };

        let registry = FixtureRegistry::from_discovery(&discovery);
        let resolver = Resolver::new(&registry);
        let (runnable, errors) = resolver.resolve_all(&discovery);

        assert!(errors.is_empty(), "Should have no errors");
        assert_eq!(runnable.len(), 1);

        // Fixtures should be in topological order (dependencies first)
        let test = &runnable[0];
        assert_eq!(test.fixtures.len(), 3);
        assert_eq!(test.fixtures[0].name, "base");
        assert_eq!(test.fixtures[1].name, "connection");
        assert_eq!(test.fixtures[2].name, "db");
    }

    // =========================================================================
    // Phase 6: Builtin Fixture Tests
    // =========================================================================

    #[test]
    fn test_is_builtin_fixture_common() {
        assert!(is_builtin_fixture("monkeypatch"));
        assert!(is_builtin_fixture("tmp_path"));
        assert!(is_builtin_fixture("tmp_path_factory"));
        assert!(is_builtin_fixture("capsys"));
        assert!(is_builtin_fixture("capfd"));
        assert!(is_builtin_fixture("request"));
    }

    #[test]
    fn test_is_builtin_fixture_all() {
        for name in PYTEST_BUILTINS {
            assert!(is_builtin_fixture(name), "Expected {} to be builtin", name);
        }
    }

    #[test]
    fn test_is_builtin_fixture_negative() {
        assert!(!is_builtin_fixture("my_custom_fixture"));
        assert!(!is_builtin_fixture("db"));
        assert!(!is_builtin_fixture("mock_page"));
    }

    #[test]
    fn test_builtin_fixture_resolves_without_error() {
        // Test that depends on builtin fixture should resolve without error
        let discovery = DiscoveryResult {
            modules: vec![TestModule {
                path: PathBuf::from("test_builtins.py"),
                tests: vec![
                    make_test("test_with_monkeypatch", vec!["monkeypatch"]),
                    make_test("test_with_tmp_path", vec!["tmp_path"]),
                    make_test("test_with_capsys", vec!["capsys"]),
                    make_test("test_with_request", vec!["request"]),
                ],
                fixtures: vec![],
            }],
        };

        let registry = FixtureRegistry::from_discovery(&discovery);
        let resolver = Resolver::new(&registry);
        let (runnable, errors) = resolver.resolve_all(&discovery);

        // All tests should resolve - builtins are skipped, not errors
        assert!(
            errors.is_empty(),
            "Builtin fixtures should not cause errors: {:?}",
            errors
        );
        assert_eq!(runnable.len(), 4);
    }

    #[test]
    fn test_mixed_builtin_and_user_fixtures() {
        // Test depends on both builtin and user-defined fixture
        let discovery = DiscoveryResult {
            modules: vec![
                TestModule {
                    path: PathBuf::from("conftest.py"),
                    tests: vec![],
                    fixtures: vec![make_fixture("db", vec![])],
                },
                TestModule {
                    path: PathBuf::from("test_mixed.py"),
                    tests: vec![make_test("test_db_with_tmp", vec!["db", "tmp_path"])],
                    fixtures: vec![],
                },
            ],
        };

        let registry = FixtureRegistry::from_discovery(&discovery);
        let resolver = Resolver::new(&registry);
        let (runnable, errors) = resolver.resolve_all(&discovery);

        assert!(errors.is_empty());
        assert_eq!(runnable.len(), 1);
        // Only user fixture should be in resolved list (builtin is skipped)
        assert_eq!(runnable[0].fixtures.len(), 1);
        assert_eq!(runnable[0].fixtures[0].name, "db");
    }
}
