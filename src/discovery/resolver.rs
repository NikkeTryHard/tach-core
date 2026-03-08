//! Dependency Resolution & Graph Construction
//!
//! This module resolves pytest fixture dependencies and builds the execution order
//! for test fixtures. It implements the fixture lookup algorithm described in the
//! _Python Monorepo Zygote Tree Design_ research paper:
//!
//! > "The Rust resolver calculates the module's fully qualified name based on its
//! > file path relative to the nearest __init__.py or namespace root."
//!
//! # Fixture Resolution Algorithm
//!
//! Pytest fixtures follow a specific lookup order:
//!
//! 1. **Class-scoped fixtures** - Fixtures defined within a test class
//! 2. **Module-local fixtures** - Fixtures defined in the same file as the test
//! 3. **Conftest hierarchy** - Walk UP the directory tree, checking each conftest.py
//!
//! ```text
//! project/
//! ├── conftest.py           <- Level 3 (root)
//! ├── tests/
//! │   ├── conftest.py       <- Level 2
//! │   └── subdir/
//! │       ├── conftest.py   <- Level 1 (closest)
//! │       └── test_foo.py   <- Test file
//! ```
//!
//! For `test_foo.py`, fixtures are searched in order:
//! 1. `tests/subdir/conftest.py` (closest)
//! 2. `tests/conftest.py`
//! 3. `conftest.py` (root)
//!
//! **Inner conftest.py files take precedence over outer ones** - if a fixture
//! with the same name exists at multiple levels, the closest one wins.
//!
//! # Dependency Ordering (Topological Sort)
//!
//! Fixtures may depend on other fixtures. We use **DFS with cycle detection**
//! to produce a topological ordering (dependencies come before dependents):
//!
//! ```text
//! test_foo depends on: [db]
//! db depends on: [connection]
//! connection depends on: [base]
//!
//! Resolution order (DFS post-order):
//!   1. Resolve 'db' -> push to stack
//!   2. Resolve 'connection' -> push to stack
//!   3. Resolve 'base' -> no deps -> add to result: [base]
//!   4. Pop 'connection' -> add to result: [base, connection]
//!   5. Pop 'db' -> add to result: [base, connection, db]
//! ```
//!
//! # Cycle Detection
//!
//! We maintain a "recursion stack" during DFS. If we encounter a fixture that's
//! already on the stack, we've found a cycle:
//!
//! ```text
//! a -> b -> c -> a  (cycle detected when 'a' is on stack and we try to add it again)
//! ```
//!
//! Cycles produce a `ResolutionError::CyclicDependency` with the full cycle path.
//!
//! # Pytest Builtin Fixtures
//!
//! Some fixtures are provided by pytest at runtime (e.g., `tmp_path`, `monkeypatch`).
//! These are NOT discovered statically - we skip them during resolution and let
//! pytest inject them at runtime.

use crate::discovery::{DiscoveryResult, FixtureDefinition, FixtureScope, TestCase};
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
    /// Whether this test is toxic (requires fork/kill instead of reset)
    /// Set by toxicity analysis
    pub is_toxic: bool,
    /// Per-test timeout in seconds from @pytest.mark.timeout(N)
    /// None means use global timeout
    pub timeout_secs: Option<u64>,
    /// Pytest markers on this test (e.g., "slow", "skip", "django_db")
    pub markers: Vec<String>,
    /// Structured marker info with arguments (for pytest-django support)
    /// Contains parsed marker arguments like @pytest.mark.django_db(transaction=True)
    pub marker_info: Vec<crate::discovery::MarkerInfo>,
}

impl RunnableTest {
    /// Returns true if this test has `@pytest.mark.django_db(transaction=True)`.
    ///
    /// Transaction tests commit directly to the database, requiring process-level
    /// isolation (toxic mode) instead of savepoint-based rollback.
    pub fn has_django_transaction_marker(&self) -> bool {
        self.marker_info.iter().any(|m| {
            m.name == "django_db"
                && m.args
                    .get("transaction")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
        })
    }

    /// Returns true if this test uses any module or class-scoped fixtures.
    ///
    /// Tests with scoped fixtures must be grouped with other tests from the same
    /// module/class so the scheduler can skip memory reset between them, preserving
    /// fixture state across tests.
    pub fn has_scoped_fixtures(&self) -> bool {
        self.fixtures.iter().any(|f| {
            matches!(
                f.scope,
                FixtureScope::Module | FixtureScope::Class | FixtureScope::Session
            )
        })
    }

    /// Returns the highest fixture scope used by this test.
    /// Session > Module > Class > Function
    pub fn max_fixture_scope(&self) -> FixtureScope {
        self.fixtures
            .iter()
            .map(|f| &f.scope)
            .fold(FixtureScope::Function, |acc, scope| match (&acc, scope) {
                (_, FixtureScope::Session) | (FixtureScope::Session, _) => FixtureScope::Session,
                (_, FixtureScope::Module) | (FixtureScope::Module, _) => FixtureScope::Module,
                (_, FixtureScope::Class) | (FixtureScope::Class, _) => FixtureScope::Class,
                _ => FixtureScope::Function,
            })
    }

    /// Returns the class name if this test is a method inside a test class.
    /// Extracted from test_name format: "ClassName::method_name"
    pub fn class_name(&self) -> Option<&str> {
        let parts: Vec<&str> = self.test_name.splitn(2, "::").collect();
        if parts.len() == 2 {
            Some(parts[0])
        } else {
            None
        }
    }
}

/// A resolved fixture with full context
#[derive(Debug, Clone)]
pub struct ResolvedFixture {
    pub name: String,
    pub source_file: PathBuf,
    pub scope: FixtureScope,
    /// Whether this fixture is async (async def)
    pub is_async: bool,
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
    // pytest-django fixtures (Issue #39)
    // Priority 1: Core
    "db",
    "client",
    "rf",
    // Priority 2: Admin/User
    "admin_client",
    "admin_user",
    "django_user_model",
    "django_username_field",
    // Priority 3: Advanced
    "settings",
    "transactional_db",
    "live_server",
];

/// Check if a fixture name is a pytest builtin
fn is_builtin_fixture(name: &str) -> bool {
    PYTEST_BUILTINS.contains(&name)
}

/// Registry holding all discovered fixtures.
///
/// The registry organizes fixtures into three tiers based on their scope and origin:
///
/// 1. **Class-scoped fixtures** (`class_scoped`): Fixtures defined inside test classes.
///    These have the highest priority and are only visible to tests within that class.
///
/// 2. **Module-local fixtures** (`local`): Fixtures defined in test files (not conftest.py).
///    These are only visible to tests within the same file.
///
/// 3. **Conftest fixtures** (`conftest`): Fixtures defined in conftest.py files.
///    These are organized by directory and follow Python's namespace inheritance rules.
///
/// # Conftest Hierarchy
///
/// The `conftest` map uses directory paths as keys, enabling the "walk up" lookup:
///
/// ```text
/// conftest: {
///     ""        -> {db: (fixture, "./conftest.py")},         // root
///     "tests"   -> {setup: (fixture, "tests/conftest.py")},  // level 1
///     "tests/unit" -> {...}                                   // level 2
/// }
/// ```
///
/// When looking up a fixture for `tests/unit/test_foo.py`, we search:
/// 1. `tests/unit` (closest)
/// 2. `tests`
/// 3. `` (root)
///
/// First match wins.
pub struct FixtureRegistry {
    /// Conftest fixtures by directory path
    /// Key: directory containing conftest.py
    /// Value: fixture_name -> (fixture, conftest_path)
    conftest: HashMap<PathBuf, HashMap<String, (FixtureDefinition, PathBuf)>>,
    /// Local fixtures per module (non-class-scoped only)
    local: HashMap<PathBuf, HashMap<String, FixtureDefinition>>,
    /// Class-scoped fixtures: (module_path, class_name) -> fixture_name -> fixture
    class_scoped: HashMap<(PathBuf, String), HashMap<String, FixtureDefinition>>,
}

impl FixtureRegistry {
    /// Build registry from discovery results
    pub fn from_discovery(result: &DiscoveryResult) -> Self {
        let mut conftest: HashMap<PathBuf, HashMap<String, (FixtureDefinition, PathBuf)>> =
            HashMap::new();
        let mut local = HashMap::new();
        let mut class_scoped: HashMap<(PathBuf, String), HashMap<String, FixtureDefinition>> =
            HashMap::new();

        for module in &result.modules {
            let is_conftest = module.path.file_name().is_some_and(|n| n == "conftest.py");

            let mut module_fixtures = HashMap::new();
            for fixture in &module.fixtures {
                // Handle class-scoped fixtures
                if let Some(ref class_name) = fixture.class_scope {
                    let key = (module.path.clone(), class_name.clone());
                    class_scoped
                        .entry(key)
                        .or_default()
                        .insert(fixture.name.clone(), fixture.clone());
                } else if is_conftest {
                    // Get directory containing the conftest.py
                    let conftest_dir = module
                        .path
                        .parent()
                        .map(|p| p.to_path_buf())
                        .unwrap_or_default();
                    conftest
                        .entry(conftest_dir)
                        .or_default()
                        .insert(fixture.name.clone(), (fixture.clone(), module.path.clone()));
                } else {
                    module_fixtures.insert(fixture.name.clone(), fixture.clone());
                }
            }

            if !module_fixtures.is_empty() {
                local.insert(module.path.clone(), module_fixtures);
            }
        }

        Self {
            conftest,
            local,
            class_scoped,
        }
    }

    /// Look up a fixture: class scope -> local scope -> conftest hierarchy
    ///
    /// For conftest fixtures, we walk up the directory tree to find the fixture,
    /// starting from the test's directory. Inner conftest.py files take precedence.
    fn lookup(
        &self,
        name: &str,
        module_path: &PathBuf,
        test_name: &str,
    ) -> Option<(FixtureDefinition, PathBuf)> {
        // Check class-scoped fixtures first for tests in classes
        // Test names in classes have format "ClassName::method_name"
        if let Some(class_name) = test_name.split("::").next()
            && test_name.contains("::")
        {
            let key = (module_path.clone(), class_name.to_string());
            if let Some(class_fixtures) = self.class_scoped.get(&key)
                && let Some(fixture) = class_fixtures.get(name)
            {
                return Some((fixture.clone(), module_path.clone()));
            }
        }

        // Check local module scope
        if let Some(local_fixtures) = self.local.get(module_path)
            && let Some(fixture) = local_fixtures.get(name)
        {
            return Some((fixture.clone(), module_path.clone()));
        }

        // Walk up the directory tree to find conftest fixtures
        // Start from the test file's directory
        let mut current_dir = module_path.parent();
        while let Some(dir) = current_dir {
            if let Some(conftest_fixtures) = self.conftest.get(dir)
                && let Some((fixture, source)) = conftest_fixtures.get(name)
            {
                return Some((fixture.clone(), source.clone()));
            }
            current_dir = dir.parent();
        }

        // Also check root-level conftest (empty path for relative paths)
        if let Some(conftest_fixtures) = self.conftest.get(&PathBuf::new())
            && let Some((fixture, source)) = conftest_fixtures.get(name)
        {
            return Some((fixture.clone(), source.clone()));
        }

        None
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

        // Filter out parametrized args - they're NOT fixtures
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
            is_toxic: false, // Set later by ToxicityGraph
            timeout_secs: test.timeout_secs,
            markers: test.markers.clone(),
            marker_info: test.marker_info.clone(),
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

        // Skip resolution for pytest builtin fixtures
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
            is_async: fixture.is_async,
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
    use crate::discovery::{MarkerInfo, TestModule};

    /// Helper to create a fixture definition
    fn make_fixture(name: &str, deps: Vec<&str>) -> FixtureDefinition {
        FixtureDefinition {
            name: name.to_string(),
            scope: FixtureScope::Function,
            dependencies: deps.into_iter().map(|s| s.to_string()).collect(),
            params: None,
            class_scope: None,
            autouse: false,
            is_async: false,
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
            timeout_secs: None,
            markers: vec![],
            marker_info: vec![],
            param_id: None,
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
                    hooks: vec![],
                    is_toxic: false,
                    class_defs: vec![],
                },
                // Local module with same-named "db" fixture (has dependencies)
                TestModule {
                    path: PathBuf::from("test_local.py"),
                    tests: vec![],
                    fixtures: vec![make_fixture("db", vec!["connection"])],
                    hooks: vec![],
                    is_toxic: false,
                    class_defs: vec![],
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
                    hooks: vec![],
                    is_toxic: false,
                    class_defs: vec![],
                },
                TestModule {
                    path: PathBuf::from("test_cycle.py"),
                    tests: vec![make_test("test_foo", vec!["a"])],
                    fixtures: vec![],
                    hooks: vec![],
                    is_toxic: false,
                    class_defs: vec![],
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
                hooks: vec![],
                is_toxic: false,
                class_defs: vec![],
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
        // Create a chain: test_foo -> database -> connection -> base
        // Note: Using "database" instead of "db" since "db" is now a pytest-django builtin
        let discovery = DiscoveryResult {
            modules: vec![
                TestModule {
                    path: PathBuf::from("conftest.py"),
                    tests: vec![],
                    fixtures: vec![
                        make_fixture("base", vec![]),
                        make_fixture("connection", vec!["base"]),
                        make_fixture("database", vec!["connection"]),
                    ],
                    hooks: vec![],
                    is_toxic: false,
                    class_defs: vec![],
                },
                TestModule {
                    path: PathBuf::from("test_chain.py"),
                    tests: vec![make_test("test_foo", vec!["database"])],
                    fixtures: vec![],
                    hooks: vec![],
                    is_toxic: false,
                    class_defs: vec![],
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
        assert_eq!(test.fixtures[2].name, "database");
    }

    // =========================================================================
    //  Builtin Fixture Tests
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
        assert!(!is_builtin_fixture("mock_page"));
        assert!(!is_builtin_fixture("unknown_fixture"));
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
                hooks: vec![],
                is_toxic: false,
                class_defs: vec![],
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
        // Note: Using "database" instead of "db" since "db" is now a pytest-django builtin
        let discovery = DiscoveryResult {
            modules: vec![
                TestModule {
                    path: PathBuf::from("conftest.py"),
                    tests: vec![],
                    fixtures: vec![make_fixture("database", vec![])],
                    hooks: vec![],
                    is_toxic: false,
                    class_defs: vec![],
                },
                TestModule {
                    path: PathBuf::from("test_mixed.py"),
                    tests: vec![make_test("test_db_with_tmp", vec!["database", "tmp_path"])],
                    fixtures: vec![],
                    hooks: vec![],
                    is_toxic: false,
                    class_defs: vec![],
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
        assert_eq!(runnable[0].fixtures[0].name, "database");
    }

    // =========================================================================
    // Nested Conftest Inheritance Tests (Bug Fix 0.1.1-C)
    // =========================================================================

    #[test]
    fn test_nested_conftest_inheritance() {
        // Tests should see fixtures from parent conftest.py files
        // Structure:
        //   conftest.py        -> defines "root_fixture"
        //   tests/
        //     conftest.py      -> defines "tests_fixture"
        //     subdir/
        //       conftest.py    -> defines "subdir_fixture"
        //       test_nested.py -> should see all three fixtures
        let discovery = DiscoveryResult {
            modules: vec![
                // Root conftest.py
                TestModule {
                    path: PathBuf::from("conftest.py"),
                    tests: vec![],
                    fixtures: vec![make_fixture("root_fixture", vec![])],
                    hooks: vec![],
                    is_toxic: false,
                    class_defs: vec![],
                },
                // tests/conftest.py
                TestModule {
                    path: PathBuf::from("tests/conftest.py"),
                    tests: vec![],
                    fixtures: vec![make_fixture("tests_fixture", vec![])],
                    hooks: vec![],
                    is_toxic: false,
                    class_defs: vec![],
                },
                // tests/subdir/conftest.py
                TestModule {
                    path: PathBuf::from("tests/subdir/conftest.py"),
                    tests: vec![],
                    fixtures: vec![make_fixture("subdir_fixture", vec![])],
                    hooks: vec![],
                    is_toxic: false,
                    class_defs: vec![],
                },
                // Test file that uses fixtures from all levels
                TestModule {
                    path: PathBuf::from("tests/subdir/test_nested.py"),
                    tests: vec![make_test(
                        "test_all_fixtures",
                        vec!["root_fixture", "tests_fixture", "subdir_fixture"],
                    )],
                    fixtures: vec![],
                    hooks: vec![],
                    is_toxic: false,
                    class_defs: vec![],
                },
            ],
        };

        let registry = FixtureRegistry::from_discovery(&discovery);
        let resolver = Resolver::new(&registry);
        let (runnable, errors) = resolver.resolve_all(&discovery);

        // Should resolve all fixtures without errors
        assert!(
            errors.is_empty(),
            "Should find all fixtures from parent conftest files: {:?}",
            errors
        );
        assert_eq!(runnable.len(), 1);
        assert_eq!(
            runnable[0].fixtures.len(),
            3,
            "Should resolve 3 fixtures from nested conftest hierarchy"
        );

        let fixture_names: Vec<_> = runnable[0]
            .fixtures
            .iter()
            .map(|f| f.name.as_str())
            .collect();
        assert!(fixture_names.contains(&"root_fixture"));
        assert!(fixture_names.contains(&"tests_fixture"));
        assert!(fixture_names.contains(&"subdir_fixture"));
    }

    #[test]
    fn test_nested_conftest_override() {
        // Inner conftest.py should override fixtures with same name from outer conftest.py
        // Note: Using "database" instead of "db" since "db" is now a pytest-django builtin
        let mut outer_database = make_fixture("database", vec!["connection"]);
        let inner_database = make_fixture("database", vec![]); // Inner fixture has no deps

        // Need to distinguish them
        outer_database.scope = FixtureScope::Session; // Outer has session scope

        let discovery = DiscoveryResult {
            modules: vec![
                // Outer conftest.py with database fixture (session scope, has deps)
                TestModule {
                    path: PathBuf::from("conftest.py"),
                    tests: vec![],
                    fixtures: vec![outer_database, make_fixture("connection", vec![])],
                    hooks: vec![],
                    is_toxic: false,
                    class_defs: vec![],
                },
                // Inner conftest.py with database fixture (function scope, no deps)
                TestModule {
                    path: PathBuf::from("tests/conftest.py"),
                    tests: vec![],
                    fixtures: vec![inner_database],
                    hooks: vec![],
                    is_toxic: false,
                    class_defs: vec![],
                },
                // Test in inner directory should use inner fixture
                TestModule {
                    path: PathBuf::from("tests/test_override.py"),
                    tests: vec![make_test("test_database", vec!["database"])],
                    fixtures: vec![],
                    hooks: vec![],
                    is_toxic: false,
                    class_defs: vec![],
                },
            ],
        };

        let registry = FixtureRegistry::from_discovery(&discovery);
        let resolver = Resolver::new(&registry);
        let (runnable, errors) = resolver.resolve_all(&discovery);

        assert!(errors.is_empty());
        assert_eq!(runnable.len(), 1);
        // Should only have 1 fixture (inner database, no transitive deps)
        assert_eq!(
            runnable[0].fixtures.len(),
            1,
            "Should use inner conftest fixture which has no dependencies"
        );
        assert_eq!(runnable[0].fixtures[0].name, "database");
        // Inner fixture has function scope (default)
        assert_eq!(runnable[0].fixtures[0].scope, FixtureScope::Function);
    }

    #[test]
    fn test_has_django_transaction_marker_true() {
        let test = RunnableTest {
            file_path: PathBuf::from("test_views.py"),
            test_name: "test_create_user".to_string(),
            is_async: false,
            fixtures: vec![],
            is_toxic: false,
            timeout_secs: None,
            markers: vec!["django_db".to_string()],
            marker_info: vec![MarkerInfo {
                name: "django_db".to_string(),
                args: {
                    let mut m = std::collections::HashMap::new();
                    m.insert("transaction".to_string(), serde_json::Value::Bool(true));
                    m
                },
            }],
        };
        assert!(test.has_django_transaction_marker());
    }

    #[test]
    fn test_has_django_transaction_marker_false_when_no_marker() {
        let test = RunnableTest {
            file_path: PathBuf::from("test_utils.py"),
            test_name: "test_parse".to_string(),
            is_async: false,
            fixtures: vec![],
            is_toxic: false,
            timeout_secs: None,
            markers: vec![],
            marker_info: vec![],
        };
        assert!(!test.has_django_transaction_marker());
    }

    // =========================================================================
    // Phase 4.0: RunnableTest Scope-Aware Method Tests
    // =========================================================================

    #[test]
    fn test_has_scoped_fixtures_function_only() {
        let test = RunnableTest {
            file_path: PathBuf::from("test.py"),
            test_name: "test_fn".into(),
            is_async: false,
            fixtures: vec![ResolvedFixture {
                name: "tmp".into(),
                source_file: PathBuf::from("conftest.py"),
                scope: FixtureScope::Function,
                is_async: false,
            }],
            is_toxic: false,
            timeout_secs: None,
            markers: vec![],
            marker_info: vec![],
        };
        assert!(
            !test.has_scoped_fixtures(),
            "Function-only fixtures should return false"
        );
    }

    #[test]
    fn test_has_scoped_fixtures_module() {
        let test = RunnableTest {
            file_path: PathBuf::from("test.py"),
            test_name: "test_mod".into(),
            is_async: false,
            fixtures: vec![ResolvedFixture {
                name: "db".into(),
                source_file: PathBuf::from("conftest.py"),
                scope: FixtureScope::Module,
                is_async: false,
            }],
            is_toxic: false,
            timeout_secs: None,
            markers: vec![],
            marker_info: vec![],
        };
        assert!(
            test.has_scoped_fixtures(),
            "Module-scoped fixture should return true"
        );
    }

    #[test]
    fn test_has_scoped_fixtures_session() {
        let test = RunnableTest {
            file_path: PathBuf::from("test.py"),
            test_name: "test_sess".into(),
            is_async: false,
            fixtures: vec![ResolvedFixture {
                name: "app".into(),
                source_file: PathBuf::from("conftest.py"),
                scope: FixtureScope::Session,
                is_async: false,
            }],
            is_toxic: false,
            timeout_secs: None,
            markers: vec![],
            marker_info: vec![],
        };
        assert!(
            test.has_scoped_fixtures(),
            "Session-scoped fixture should return true"
        );
    }

    #[test]
    fn test_max_fixture_scope() {
        let test = RunnableTest {
            file_path: PathBuf::from("test.py"),
            test_name: "test_mixed".into(),
            is_async: false,
            fixtures: vec![
                ResolvedFixture {
                    name: "a".into(),
                    source_file: PathBuf::from("conftest.py"),
                    scope: FixtureScope::Function,
                    is_async: false,
                },
                ResolvedFixture {
                    name: "b".into(),
                    source_file: PathBuf::from("conftest.py"),
                    scope: FixtureScope::Module,
                    is_async: false,
                },
                ResolvedFixture {
                    name: "c".into(),
                    source_file: PathBuf::from("conftest.py"),
                    scope: FixtureScope::Class,
                    is_async: false,
                },
            ],
            is_toxic: false,
            timeout_secs: None,
            markers: vec![],
            marker_info: vec![],
        };
        assert_eq!(
            test.max_fixture_scope(),
            FixtureScope::Module,
            "Module > Class > Function, so max should be Module"
        );

        let session_test = RunnableTest {
            file_path: PathBuf::from("test.py"),
            test_name: "test_sess".into(),
            is_async: false,
            fixtures: vec![
                ResolvedFixture {
                    name: "x".into(),
                    source_file: PathBuf::from("conftest.py"),
                    scope: FixtureScope::Module,
                    is_async: false,
                },
                ResolvedFixture {
                    name: "y".into(),
                    source_file: PathBuf::from("conftest.py"),
                    scope: FixtureScope::Session,
                    is_async: false,
                },
            ],
            is_toxic: false,
            timeout_secs: None,
            markers: vec![],
            marker_info: vec![],
        };
        assert_eq!(
            session_test.max_fixture_scope(),
            FixtureScope::Session,
            "Session is the highest scope"
        );
    }

    #[test]
    fn test_class_name_extraction() {
        let test = RunnableTest {
            file_path: PathBuf::from("test.py"),
            test_name: "TestUsers::test_create".into(),
            is_async: false,
            fixtures: vec![],
            is_toxic: false,
            timeout_secs: None,
            markers: vec![],
            marker_info: vec![],
        };
        assert_eq!(test.class_name(), Some("TestUsers"));
    }

    #[test]
    fn test_class_name_no_class() {
        let test = RunnableTest {
            file_path: PathBuf::from("test.py"),
            test_name: "test_standalone".into(),
            is_async: false,
            fixtures: vec![],
            is_toxic: false,
            timeout_secs: None,
            markers: vec![],
            marker_info: vec![],
        };
        assert_eq!(
            test.class_name(),
            None,
            "Plain test name without :: should return None"
        );
    }

    #[test]
    fn test_has_django_transaction_marker_false_when_transaction_false() {
        let test = RunnableTest {
            file_path: PathBuf::from("test_views.py"),
            test_name: "test_read_user".to_string(),
            is_async: false,
            fixtures: vec![],
            is_toxic: false,
            timeout_secs: None,
            markers: vec!["django_db".to_string()],
            marker_info: vec![MarkerInfo {
                name: "django_db".to_string(),
                args: {
                    let mut m = std::collections::HashMap::new();
                    m.insert("transaction".to_string(), serde_json::Value::Bool(false));
                    m
                },
            }],
        };
        assert!(!test.has_django_transaction_marker());
    }
}
