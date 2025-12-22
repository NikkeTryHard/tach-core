//! Static AST Discovery Engine
//! Parses Python files to extract tests and fixtures without executing code.

use anyhow::Result;
use ignore::WalkBuilder;
use rayon::prelude::*;
use rustpython_ast as ast;
use rustpython_parser::parse_program;
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

/// Scope of a pytest fixture
#[derive(Debug, Clone, PartialEq)]
pub enum FixtureScope {
    Function,
    Class,
    Module,
    Session,
}

impl Default for FixtureScope {
    fn default() -> Self {
        Self::Function
    }
}

/// A pytest fixture definition
#[derive(Debug, Clone)]
pub struct FixtureDefinition {
    pub name: String,
    pub scope: FixtureScope,
    pub dependencies: Vec<String>,
    /// Parametrization values (if @pytest.fixture(params=[...]))
    /// None = no params or dynamic (e.g., params=load_from_db())
    /// Some([]) = empty params list
    /// Some(["a", "b"]) = static params extracted from AST
    pub params: Option<Vec<String>>,
}

/// A test case (function)
#[derive(Debug, Clone)]
pub struct TestCase {
    pub name: String,
    pub dependencies: Vec<String>,
    pub is_async: bool,
    pub line_number: usize,
}

/// A Python test module (.py file)
#[derive(Debug)]
pub struct TestModule {
    pub path: PathBuf,
    pub tests: Vec<TestCase>,
    pub fixtures: Vec<FixtureDefinition>,
}

/// Discovery result containing all parsed modules
#[derive(Debug)]
pub struct DiscoveryResult {
    pub modules: Vec<TestModule>,
}

impl DiscoveryResult {
    pub fn test_count(&self) -> usize {
        self.modules.iter().map(|m| m.tests.len()).sum()
    }

    pub fn fixture_count(&self) -> usize {
        self.modules.iter().map(|m| m.fixtures.len()).sum()
    }
}

/// Convert byte offset to line number (1-indexed)
fn get_line_number(source: &str, byte_offset: usize) -> usize {
    source[..byte_offset.min(source.len())]
        .chars()
        .filter(|&c| c == '\n')
        .count()
        + 1
}

/// JSON-serializable test information for `tach list --json`
#[derive(Serialize)]
pub struct JsonTestInfo {
    pub id: String,
    pub file: String,
    pub line: usize,
    pub is_async: bool,
}

/// JSON output for discovery listing
#[derive(Serialize)]
struct JsonDiscoveryOutput {
    version: u32,
    tests: Vec<JsonTestInfo>,
}

/// Dump discovery result as JSON to stdout
///
/// Used by `tach list --format=json` for IDE integration.
/// Output format:
/// ```json
/// { "version": 1, "tests": [{ "id": "...", "file": "...", "line": 1 }] }
/// ```
pub fn dump_json(result: &DiscoveryResult) -> Result<()> {
    let tests: Vec<JsonTestInfo> = result
        .modules
        .iter()
        .flat_map(|module| {
            module.tests.iter().map(move |test| {
                let file = module.path.to_string_lossy().to_string();
                JsonTestInfo {
                    id: format!("{}::{}", file, test.name),
                    file,
                    line: test.line_number,
                    is_async: test.is_async,
                }
            })
        })
        .collect();

    let output = JsonDiscoveryOutput { version: 1, tests };

    // ONLY dump_json touches stdout with JSON
    println!("{}", serde_json::to_string(&output)?);
    Ok(())
}

/// Scan project for test files and parse them in parallel
pub fn discover(root: &Path) -> Result<DiscoveryResult> {
    let paths: Vec<PathBuf> = WalkBuilder::new(root)
        .standard_filters(true)
        .build()
        .filter_map(|e| e.ok())
        .filter(|e| is_test_file(e.path()))
        .map(|e| {
            // Convert to relative path for pytest node_id compatibility
            e.path()
                .strip_prefix(root)
                .unwrap_or(e.path())
                .to_path_buf()
        })
        .collect();

    let modules: Vec<TestModule> = paths
        .par_iter()
        .filter_map(|path| parse_module(path).ok())
        .filter(|m| !m.tests.is_empty() || !m.fixtures.is_empty())
        .collect();

    Ok(DiscoveryResult { modules })
}

fn is_test_file(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    let ext = path.extension().and_then(|e| e.to_str());
    if ext != Some("py") {
        return false;
    }
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    name.starts_with("test_") || name.ends_with("_test.py") || name == "conftest.py"
}

fn parse_module(path: &Path) -> Result<TestModule> {
    let source = fs::read_to_string(path)?;
    let path_str = path.to_string_lossy();

    let suite = match parse_program(&source, &path_str) {
        Ok(s) => s,
        Err(_) => {
            return Ok(TestModule {
                path: path.to_path_buf(),
                tests: vec![],
                fixtures: vec![],
            });
        }
    };

    let mut tests = vec![];
    let mut fixtures = vec![];

    for stmt in suite {
        match stmt {
            ast::Stmt::FunctionDef(func) => {
                analyze_function(&func, &source, &mut tests, &mut fixtures, false);
            }
            ast::Stmt::AsyncFunctionDef(func) => {
                let name = func.name.as_str();
                if name.starts_with("test_") {
                    let line_number = get_line_number(&source, func.range.start().to_usize());
                    tests.push(TestCase {
                        name: name.to_string(),
                        dependencies: extract_args_from_arguments(&func.args),
                        is_async: true,
                        line_number,
                    });
                }
                if has_fixture_decorator(&func.decorator_list) {
                    fixtures.push(FixtureDefinition {
                        name: name.to_string(),
                        scope: extract_scope_from_decorators(&func.decorator_list),
                        dependencies: extract_args_from_arguments(&func.args),
                        params: extract_params_from_decorators(&func.decorator_list),
                    });
                }
            }
            ast::Stmt::ClassDef(class) => {
                let class_name = class.name.as_str();
                if class_name.starts_with("Test") {
                    for stmt in &class.body {
                        if let ast::Stmt::FunctionDef(func) = stmt {
                            let method_name = func.name.as_str();
                            if method_name.starts_with("test_") {
                                let line_number =
                                    get_line_number(&source, func.range.start().to_usize());
                                tests.push(TestCase {
                                    name: format!("{}::{}", class_name, method_name),
                                    dependencies: extract_args_from_arguments(&func.args),
                                    is_async: false,
                                    line_number,
                                });
                            }
                        } else if let ast::Stmt::AsyncFunctionDef(func) = stmt {
                            let method_name = func.name.as_str();
                            if method_name.starts_with("test_") {
                                let line_number =
                                    get_line_number(&source, func.range.start().to_usize());
                                tests.push(TestCase {
                                    name: format!("{}::{}", class_name, method_name),
                                    dependencies: extract_args_from_arguments(&func.args),
                                    is_async: true,
                                    line_number,
                                });
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    Ok(TestModule {
        path: path.to_path_buf(),
        tests,
        fixtures,
    })
}

fn analyze_function(
    func: &ast::StmtFunctionDef,
    source: &str,
    tests: &mut Vec<TestCase>,
    fixtures: &mut Vec<FixtureDefinition>,
    is_async: bool,
) {
    let name = func.name.as_str();

    if name.starts_with("test_") {
        let line_number = get_line_number(source, func.range.start().to_usize());
        tests.push(TestCase {
            name: name.to_string(),
            dependencies: extract_args_from_arguments(&func.args),
            is_async,
            line_number,
        });
    }

    if has_fixture_decorator(&func.decorator_list) {
        fixtures.push(FixtureDefinition {
            name: name.to_string(),
            scope: extract_scope_from_decorators(&func.decorator_list),
            dependencies: extract_args_from_arguments(&func.args),
            params: extract_params_from_decorators(&func.decorator_list),
        });
    }
}

fn extract_args_from_arguments(args: &ast::Arguments) -> Vec<String> {
    let mut result = vec![];
    for arg in &args.args {
        let name = arg.def.arg.as_str();
        if name != "self" && name != "cls" {
            result.push(name.to_string());
        }
    }
    result
}

fn has_fixture_decorator(decorators: &[ast::Expr]) -> bool {
    decorators.iter().any(|d| is_fixture_decorator(d))
}

fn is_fixture_decorator(expr: &ast::Expr) -> bool {
    match expr {
        ast::Expr::Call(call) => is_fixture_decorator(&call.func),
        ast::Expr::Attribute(attr) => attr.attr.as_str() == "fixture",
        ast::Expr::Name(name) => name.id.as_str() == "fixture",
        _ => false,
    }
}

fn extract_scope_from_decorators(decorators: &[ast::Expr]) -> FixtureScope {
    for decorator in decorators {
        if let ast::Expr::Call(call) = decorator {
            for keyword in &call.keywords {
                if let Some(ref arg) = keyword.arg {
                    if arg.as_str() == "scope" {
                        if let ast::Expr::Constant(c) = &keyword.value {
                            if let ast::Constant::Str(s) = &c.value {
                                return match s.as_str() {
                                    "class" => FixtureScope::Class,
                                    "module" => FixtureScope::Module,
                                    "session" => FixtureScope::Session,
                                    _ => FixtureScope::Function,
                                };
                            }
                        }
                    }
                }
            }
        }
    }
    FixtureScope::Function
}

/// Extract params from @pytest.fixture(params=[...]) decorator
/// Returns None if:
/// - No params keyword
/// - Dynamic params (e.g., params=load_from_db())
/// Returns Some(vec) if static literal list
fn extract_params_from_decorators(decorators: &[ast::Expr]) -> Option<Vec<String>> {
    for decorator in decorators {
        if let ast::Expr::Call(call) = decorator {
            for keyword in &call.keywords {
                if let Some(ref arg) = keyword.arg {
                    if arg.as_str() == "params" {
                        // Try to extract literals from the params value
                        return extract_literal_list(&keyword.value);
                    }
                }
            }
        }
    }
    None // No params keyword found
}

/// Extract literals from a List or Tuple expression
/// Returns None if the expression is not a static list of literals
fn extract_literal_list(expr: &ast::Expr) -> Option<Vec<String>> {
    match expr {
        ast::Expr::List(list) => {
            let mut values = Vec::new();
            for elt in &list.elts {
                if let Some(s) = expr_to_string(elt) {
                    values.push(s);
                } else {
                    // Non-literal element - bail out
                    return None;
                }
            }
            Some(values)
        }
        ast::Expr::Tuple(tuple) => {
            let mut values = Vec::new();
            for elt in &tuple.elts {
                if let Some(s) = expr_to_string(elt) {
                    values.push(s);
                } else {
                    return None;
                }
            }
            Some(values)
        }
        _ => None, // Dynamic expression (function call, variable, etc.)
    }
}

/// Convert an AST expression to its string representation
/// Only handles literals (int, str, bool, None)
fn expr_to_string(expr: &ast::Expr) -> Option<String> {
    match expr {
        ast::Expr::Constant(c) => match &c.value {
            ast::Constant::Int(i) => Some(i.to_string()),
            ast::Constant::Str(s) => Some(s.to_string()),
            ast::Constant::Bool(b) => Some(if *b { "True" } else { "False" }.to_string()),
            ast::Constant::None => Some("None".to_string()),
            ast::Constant::Float(f) => Some(f.to_string()),
            _ => None,
        },
        // Handle simple Name expressions (like exception classes)
        ast::Expr::Name(n) => Some(n.id.to_string()),
        _ => None,
    }
}

// =============================================================================
// Unit Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    // Helper to parse a Python source string and return TestModule
    fn parse_source(source: &str) -> TestModule {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(source.as_bytes()).unwrap();
        parse_module(file.path()).unwrap()
    }

    #[test]
    fn test_fixture_scope_default() {
        assert_eq!(FixtureScope::default(), FixtureScope::Function);
    }

    #[test]
    fn test_discovery_result_counts() {
        let result = DiscoveryResult {
            modules: vec![
                TestModule {
                    path: PathBuf::from("test_a.py"),
                    tests: vec![
                        TestCase {
                            name: "test_1".into(),
                            dependencies: vec![],
                            is_async: false,
                            line_number: 1,
                        },
                        TestCase {
                            name: "test_2".into(),
                            dependencies: vec![],
                            is_async: true,
                            line_number: 1,
                        },
                    ],
                    fixtures: vec![FixtureDefinition {
                        name: "db".into(),
                        scope: FixtureScope::Module,
                        dependencies: vec![],
                        params: None,
                    }],
                },
                TestModule {
                    path: PathBuf::from("test_b.py"),
                    tests: vec![TestCase {
                        name: "test_3".into(),
                        dependencies: vec!["db".into()],
                        is_async: false,
                        line_number: 1,
                    }],
                    fixtures: vec![],
                },
            ],
        };
        assert_eq!(result.test_count(), 3);
        assert_eq!(result.fixture_count(), 1);
    }

    #[test]
    fn test_discovery_result_empty() {
        let result = DiscoveryResult { modules: vec![] };
        assert_eq!(result.test_count(), 0);
        assert_eq!(result.fixture_count(), 0);
    }

    #[test]
    fn test_fixture_scope_equality() {
        assert_eq!(FixtureScope::Function, FixtureScope::Function);
        assert_eq!(FixtureScope::Class, FixtureScope::Class);
        assert_eq!(FixtureScope::Module, FixtureScope::Module);
        assert_eq!(FixtureScope::Session, FixtureScope::Session);
        assert_ne!(FixtureScope::Function, FixtureScope::Session);
    }

    // =========================================================================
    // AST Parsing Tests
    // =========================================================================

    #[test]
    fn test_parse_simple_test_function() {
        let source = r#"
def test_simple():
    pass
"#;
        let module = parse_source(source);
        assert_eq!(module.tests.len(), 1);
        assert_eq!(module.tests[0].name, "test_simple");
        assert!(!module.tests[0].is_async);
        assert!(module.tests[0].dependencies.is_empty());
    }

    #[test]
    fn test_parse_async_test_function() {
        let source = r#"
async def test_async():
    await something()
"#;
        let module = parse_source(source);
        assert_eq!(module.tests.len(), 1);
        assert_eq!(module.tests[0].name, "test_async");
        assert!(module.tests[0].is_async);
    }

    #[test]
    fn test_parse_test_with_dependencies() {
        let source = r#"
def test_with_deps(db, cache, client):
    pass
"#;
        let module = parse_source(source);
        assert_eq!(module.tests.len(), 1);
        assert_eq!(module.tests[0].dependencies, vec!["db", "cache", "client"]);
    }

    #[test]
    fn test_parse_fixture_simple() {
        let source = r#"
import pytest

@pytest.fixture
def my_fixture():
    return 42
"#;
        let module = parse_source(source);
        assert_eq!(module.fixtures.len(), 1);
        assert_eq!(module.fixtures[0].name, "my_fixture");
        assert_eq!(module.fixtures[0].scope, FixtureScope::Function);
    }

    #[test]
    fn test_parse_fixture_with_scope() {
        let source = r#"
import pytest

@pytest.fixture(scope="module")
def module_fixture():
    return "module"

@pytest.fixture(scope="session")
def session_fixture():
    return "session"

@pytest.fixture(scope="class")
def class_fixture():
    return "class"
"#;
        let module = parse_source(source);
        assert_eq!(module.fixtures.len(), 3);

        let scopes: Vec<_> = module.fixtures.iter().map(|f| f.scope.clone()).collect();
        assert!(scopes.contains(&FixtureScope::Module));
        assert!(scopes.contains(&FixtureScope::Session));
        assert!(scopes.contains(&FixtureScope::Class));
    }

    #[test]
    fn test_parse_fixture_with_dependencies() {
        let source = r#"
import pytest

@pytest.fixture
def derived_fixture(base_fixture, db):
    return base_fixture + db
"#;
        let module = parse_source(source);
        assert_eq!(module.fixtures.len(), 1);
        assert_eq!(module.fixtures[0].dependencies, vec!["base_fixture", "db"]);
    }

    #[test]
    fn test_parse_test_class() {
        let source = r#"
class TestMyClass:
    def test_method_one(self):
        pass
    
    def test_method_two(self, db):
        pass
    
    def helper_not_a_test(self):
        pass
"#;
        let module = parse_source(source);
        assert_eq!(module.tests.len(), 2);
        assert!(module
            .tests
            .iter()
            .any(|t| t.name == "TestMyClass::test_method_one"));
        assert!(module
            .tests
            .iter()
            .any(|t| t.name == "TestMyClass::test_method_two"));
    }

    #[test]
    fn test_parse_async_test_in_class() {
        let source = r#"
class TestAsync:
    async def test_async_method(self, client):
        await client.get()
"#;
        let module = parse_source(source);
        assert_eq!(module.tests.len(), 1);
        assert_eq!(module.tests[0].name, "TestAsync::test_async_method");
        assert!(module.tests[0].is_async);
        assert_eq!(module.tests[0].dependencies, vec!["client"]);
    }

    #[test]
    fn test_parse_non_test_functions_ignored() {
        let source = r#"
def helper_function():
    pass

def setup_module():
    pass

def teardown():
    pass
"#;
        let module = parse_source(source);
        assert!(module.tests.is_empty());
        assert!(module.fixtures.is_empty());
    }

    #[test]
    fn test_parse_non_test_class_ignored() {
        let source = r#"
class MyClass:
    def test_looks_like_test(self):
        pass
"#;
        let module = parse_source(source);
        // Class doesn't start with "Test", so methods should be ignored
        assert!(module.tests.is_empty());
    }

    #[test]
    fn test_parse_self_and_cls_excluded_from_deps() {
        let source = r#"
class TestWithSelf:
    def test_method(self, db, cache):
        pass
    
    @classmethod
    def test_classmethod(cls, db):
        pass
"#;
        let module = parse_source(source);

        for test in &module.tests {
            assert!(!test.dependencies.contains(&"self".to_string()));
            assert!(!test.dependencies.contains(&"cls".to_string()));
        }
    }

    #[test]
    fn test_parse_empty_file() {
        let source = "";
        let module = parse_source(source);
        assert!(module.tests.is_empty());
        assert!(module.fixtures.is_empty());
    }

    #[test]
    fn test_parse_mixed_content() {
        let source = r#"
import pytest

@pytest.fixture(scope="module")
def db():
    return "connection"

def test_with_db(db):
    assert db == "connection"

class TestIntegration:
    def test_in_class(self, db):
        pass

async def test_async_standalone():
    await asyncio.sleep(0)
"#;
        let module = parse_source(source);

        assert_eq!(module.fixtures.len(), 1);
        assert_eq!(module.fixtures[0].name, "db");
        assert_eq!(module.fixtures[0].scope, FixtureScope::Module);

        assert_eq!(module.tests.len(), 3);
        let test_names: Vec<_> = module.tests.iter().map(|t| t.name.as_str()).collect();
        assert!(test_names.contains(&"test_with_db"));
        assert!(test_names.contains(&"TestIntegration::test_in_class"));
        assert!(test_names.contains(&"test_async_standalone"));
    }

    #[test]
    fn test_parse_bare_fixture_decorator() {
        let source = r#"
@fixture
def bare_fixture():
    return 1
"#;
        let module = parse_source(source);
        assert_eq!(module.fixtures.len(), 1);
        assert_eq!(module.fixtures[0].name, "bare_fixture");
    }
}
