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

/// Registry holding all discovered fixtures
pub struct FixtureRegistry {
    /// Global fixtures from conftest.py files
    global: HashMap<String, (FixtureDefinition, PathBuf)>,
    /// Local fixtures per module
    local: HashMap<PathBuf, HashMap<String, FixtureDefinition>>,
}

impl FixtureRegistry {
    /// Build registry from discovery results
    pub fn from_discovery(result: &DiscoveryResult) -> Self {
        let mut global = HashMap::new();
        let mut local = HashMap::new();

        for module in &result.modules {
            let is_conftest = module
                .path
                .file_name()
                .map_or(false, |n| n == "conftest.py");

            let mut module_fixtures = HashMap::new();
            for fixture in &module.fixtures {
                if is_conftest {
                    global.insert(fixture.name.clone(), (fixture.clone(), module.path.clone()));
                } else {
                    module_fixtures.insert(fixture.name.clone(), fixture.clone());
                }
            }

            if !module_fixtures.is_empty() {
                local.insert(module.path.clone(), module_fixtures);
            }
        }

        Self { global, local }
    }

    /// Look up a fixture: local scope first, then global
    fn lookup(&self, name: &str, module_path: &PathBuf) -> Option<(FixtureDefinition, PathBuf)> {
        // Check local scope first
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

        // Resolve each direct dependency
        for dep_name in &test.dependencies {
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

        // Look up fixture
        let (fixture, source_file) = self.registry.lookup(name, module_path).ok_or_else(|| {
            ResolutionError::MissingFixture {
                test: test_name.to_string(),
                fixture: name.to_string(),
            }
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
        }
    }

    /// Helper to create a test case
    fn make_test(name: &str, deps: Vec<&str>) -> TestCase {
        TestCase {
            name: name.to_string(),
            dependencies: deps.into_iter().map(|s| s.to_string()).collect(),
            is_async: false,
            line_number: 1,
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
        let local_path = PathBuf::from("test_local.py");
        let (fixture, _) = registry.lookup("db", &local_path).unwrap();
        assert!(
            !fixture.dependencies.is_empty(),
            "Local fixture should have dependencies"
        );

        // Other module lookup should return global fixture (no deps)
        let other_path = PathBuf::from("test_other.py");
        let (fixture, _) = registry.lookup("db", &other_path).unwrap();
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
}
