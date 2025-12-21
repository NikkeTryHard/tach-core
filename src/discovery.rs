use anyhow::Result;
use ignore::WalkBuilder;
use rustpython_ast as ast;
use rustpython_parser::parse_program;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct DiscoveryResult {
    pub tests: Vec<TestUnit>,
    pub fixtures: Vec<String>,
}

#[derive(Debug)]
pub struct TestUnit {
    pub file_path: PathBuf,
    pub test_name: String,
}

pub fn scan_project(root: &Path) -> Result<DiscoveryResult> {
    let mut results = DiscoveryResult {
        tests: Vec::new(),
        fixtures: Vec::new(),
    };

    let walker = WalkBuilder::new(root).standard_filters(true).build();

    for result in walker {
        let entry = result?;
        let path = entry.path();

        if path.is_file() && path.extension().map_or(false, |ext| ext == "py") {
            let _ = analyze_file(path, &mut results);
        }
    }

    Ok(results)
}

fn analyze_file(path: &Path, results: &mut DiscoveryResult) -> Result<()> {
    let source = fs::read_to_string(path)?;
    let path_str = path.to_string_lossy();

    let suite = match parse_program(&source, &path_str) {
        Ok(s) => s,
        Err(_) => return Ok(()),
    };

    for stmt in suite {
        if let ast::Stmt::FunctionDef(func) = stmt {
            let name = func.name.as_str();

            if name.starts_with("test_") {
                results.tests.push(TestUnit {
                    file_path: path.to_path_buf(),
                    test_name: name.to_string(),
                });
            }

            for decorator in &func.decorator_list {
                if is_fixture_decorator(decorator) {
                    results.fixtures.push(name.to_string());
                }
            }
        }
    }
    Ok(())
}

fn is_fixture_decorator(expr: &ast::Expr) -> bool {
    match expr {
        ast::Expr::Call(call) => is_fixture_decorator(&call.func),
        ast::Expr::Attribute(attr) => attr.attr.as_str() == "fixture",
        ast::Expr::Name(name) => name.id.as_str() == "fixture",
        _ => false,
    }
}
