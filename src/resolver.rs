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
