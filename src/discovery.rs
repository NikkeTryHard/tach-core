//! Static AST Discovery Engine
//! Parses Python files to extract tests and fixtures without executing code.

use anyhow::Result;
use ignore::WalkBuilder;
use rayon::prelude::*;
use rustpython_ast as ast;
use rustpython_parser::parse_program;
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
}

/// A test case (function)
#[derive(Debug, Clone)]
pub struct TestCase {
    pub name: String,
    pub dependencies: Vec<String>,
    pub is_async: bool,
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

/// Scan project for test files and parse them in parallel
pub fn discover(root: &Path) -> Result<DiscoveryResult> {
    let paths: Vec<PathBuf> = WalkBuilder::new(root)
        .standard_filters(true)
        .build()
        .filter_map(|e| e.ok())
        .filter(|e| is_test_file(e.path()))
        .map(|e| e.path().to_path_buf())
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
                analyze_function(&func, &mut tests, &mut fixtures, false);
            }
            ast::Stmt::AsyncFunctionDef(func) => {
                let name = func.name.as_str();
                if name.starts_with("test_") {
                    tests.push(TestCase {
                        name: name.to_string(),
                        dependencies: extract_args_from_arguments(&func.args),
                        is_async: true,
                    });
                }
                if has_fixture_decorator(&func.decorator_list) {
                    fixtures.push(FixtureDefinition {
                        name: name.to_string(),
                        scope: extract_scope_from_decorators(&func.decorator_list),
                        dependencies: extract_args_from_arguments(&func.args),
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
                                tests.push(TestCase {
                                    name: format!("{}::{}", class_name, method_name),
                                    dependencies: extract_args_from_arguments(&func.args),
                                    is_async: false,
                                });
                            }
                        } else if let ast::Stmt::AsyncFunctionDef(func) = stmt {
                            let method_name = func.name.as_str();
                            if method_name.starts_with("test_") {
                                tests.push(TestCase {
                                    name: format!("{}::{}", class_name, method_name),
                                    dependencies: extract_args_from_arguments(&func.args),
                                    is_async: true,
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
    tests: &mut Vec<TestCase>,
    fixtures: &mut Vec<FixtureDefinition>,
    is_async: bool,
) {
    let name = func.name.as_str();

    if name.starts_with("test_") {
        tests.push(TestCase {
            name: name.to_string(),
            dependencies: extract_args_from_arguments(&func.args),
            is_async,
        });
    }

    if has_fixture_decorator(&func.decorator_list) {
        fixtures.push(FixtureDefinition {
            name: name.to_string(),
            scope: extract_scope_from_decorators(&func.decorator_list),
            dependencies: extract_args_from_arguments(&func.args),
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
