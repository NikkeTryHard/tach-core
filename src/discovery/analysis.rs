//! Toxicity Analysis Module 
//!
//! Static analysis to detect "toxic" patterns that are unsafe for snapshot/reset.
//! Toxic patterns include: threading, multiprocessing, sockets, native code (ctypes),
//! and external packages with native dependencies.
//!
//! Key Design Decisions:
//! - Star imports from toxic modules = Toxic (aggressive stance)
//! - Dynamic imports (importlib.import_module, __import__, exec) = Toxic
//! - Imports inside functions are NOT ignored (Tach worker recycling model)
//! - Any toxic token ANYWHERE in the file triggers Isolation Mode

use rustpython_ast as ast;
use rustpython_parser::Parse;
use std::collections::HashMap;
use std::path::Path;

// =============================================================================
// Blocklists
// =============================================================================

/// Toxic standard library modules that spawn threads, processes, or use native code
const TOXIC_STD_LIB: &[&str] = &[
    "threading",
    "_thread",
    "multiprocessing",
    "socket",
    "ctypes",
    "signal",
    "concurrent.futures",
];

/// Toxic external packages with native dependencies or thread pools
const TOXIC_EXTERNAL_MODULES: &[&str] = &[
    "grpc",
    "pandas",     // OpenMP threads
    "tensorflow", // CUDA context
    "torch",      // CUDA context
    "cv2",        // OpenCV threads
    "gevent",     // Greenlets
    "cffi",
];

// =============================================================================
// Data Structures
// =============================================================================

/// Result of toxicity analysis for a single file
#[derive(Debug, Clone, Default)]
pub struct ToxicityReport {
    /// Whether the file contains toxic patterns
    pub is_toxic: bool,
    /// Human-readable reasons for toxicity
    pub reasons: Vec<String>,
    /// All imports found (for graph construction)
    pub imports: Vec<String>,
}

// =============================================================================
// Public API
// =============================================================================

/// Analyze a single Python source file for toxicity
///
/// This function parses the source code and walks the entire AST to detect
/// toxic patterns in ALL scopes (global and function bodies).
///
/// # Arguments
/// * `source` - Python source code as a string
/// * `path` - Path to the file (used for error messages and parse_program)
///
/// # Returns
/// A `ToxicityReport` containing toxicity status, reasons, and imports
pub fn analyze_file(source: &str, path: &Path) -> ToxicityReport {
    let path_str = path.to_string_lossy();

    let suite = match ast::Suite::parse(source, &path_str) {
        Ok(s) => s,
        Err(_) => {
            // Parse errors are treated as toxic (conservative approach)
            return ToxicityReport {
                is_toxic: true,
                reasons: vec!["Parse error - treating as toxic".to_string()],
                imports: vec![],
            };
        }
    };

    let mut report = ToxicityReport::default();
    let mut import_aliases: HashMap<String, String> = HashMap::new();
    let mut from_imports: HashMap<String, String> = HashMap::new();

    // Walk all top-level statements
    for stmt in &suite {
        analyze_stmt(stmt, &mut report, &mut import_aliases, &mut from_imports);
    }

    report
}

// =============================================================================
// Statement Analysis
// =============================================================================

/// Recursively analyze a statement for toxic patterns
fn analyze_stmt(
    stmt: &ast::Stmt,
    report: &mut ToxicityReport,
    import_aliases: &mut HashMap<String, String>,
    from_imports: &mut HashMap<String, String>,
) {
    match stmt {
        // Handle: import threading, import multiprocessing as mp
        ast::Stmt::Import(import) => {
            for alias in &import.names {
                let name = alias.name.as_str();
                report.imports.push(name.to_string());

                if is_toxic_module(name) {
                    report.is_toxic = true;
                    report.reasons.push(format!("Imported '{}'", name));
                }

                // Track alias for later call detection
                let local = alias.asname.as_ref().map(|s| s.as_str()).unwrap_or(name);
                import_aliases.insert(local.to_string(), name.to_string());
            }
        }

        // Handle: from threading import Thread, from socket import *
        ast::Stmt::ImportFrom(import) => {
            if let Some(module) = &import.module {
                let module_name = module.as_str();
                report.imports.push(module_name.to_string());

                for alias in &import.names {
                    let name = alias.name.as_str();

                    // Star import from toxic module = Toxic (aggressive stance)
                    if name == "*" && is_toxic_module(module_name) {
                        report.is_toxic = true;
                        report
                            .reasons
                            .push(format!("Star import from toxic module '{}'", module_name));
                        continue;
                    }

                    // Track from-import for call detection
                    let local = alias.asname.as_ref().map(|s| s.as_str()).unwrap_or(name);
                    from_imports.insert(local.to_string(), format!("{}.{}", module_name, name));
                }

                // Any import from a toxic module = Toxic
                if is_toxic_module(module_name) {
                    report.is_toxic = true;
                    report
                        .reasons
                        .push(format!("Imported from '{}'", module_name));
                }
            }
        }

        // Recurse into function bodies (imports inside functions are NOT ignored)
        ast::Stmt::FunctionDef(func) => {
            for s in &func.body {
                analyze_stmt(s, report, import_aliases, from_imports);
            }
        }

        // Recurse into async function bodies
        ast::Stmt::AsyncFunctionDef(func) => {
            for s in &func.body {
                analyze_stmt(s, report, import_aliases, from_imports);
            }
        }

        // Recurse into class bodies
        ast::Stmt::ClassDef(class) => {
            for s in &class.body {
                analyze_stmt(s, report, import_aliases, from_imports);
            }
        }

        // Check expression statements for toxic calls
        ast::Stmt::Expr(expr_stmt) => {
            check_expr_toxicity(&expr_stmt.value, report, import_aliases, from_imports);
        }

        // Check assignments for toxic calls on RHS
        ast::Stmt::Assign(assign) => {
            check_expr_toxicity(&assign.value, report, import_aliases, from_imports);
        }

        // Check annotated assignments
        ast::Stmt::AnnAssign(ann_assign) => {
            if let Some(ref value) = ann_assign.value {
                check_expr_toxicity(value, report, import_aliases, from_imports);
            }
        }

        // Check return statements
        ast::Stmt::Return(ret) => {
            if let Some(ref value) = ret.value {
                check_expr_toxicity(value, report, import_aliases, from_imports);
            }
        }

        // Recurse into if/elif/else bodies
        // CRITICAL: Skip TYPE_CHECKING blocks to avoid false positives
        ast::Stmt::If(if_stmt) => {
            // Check if this is `if TYPE_CHECKING:` or `if typing.TYPE_CHECKING:`
            if !is_type_checking_block(&if_stmt.test) {
                for s in &if_stmt.body {
                    analyze_stmt(s, report, import_aliases, from_imports);
                }
            }
            // Always analyze else branch (not guarded by TYPE_CHECKING)
            for s in &if_stmt.orelse {
                analyze_stmt(s, report, import_aliases, from_imports);
            }
        }

        // Recurse into for loop bodies
        ast::Stmt::For(for_stmt) => {
            for s in &for_stmt.body {
                analyze_stmt(s, report, import_aliases, from_imports);
            }
            for s in &for_stmt.orelse {
                analyze_stmt(s, report, import_aliases, from_imports);
            }
        }

        // Recurse into while loop bodies
        ast::Stmt::While(while_stmt) => {
            for s in &while_stmt.body {
                analyze_stmt(s, report, import_aliases, from_imports);
            }
            for s in &while_stmt.orelse {
                analyze_stmt(s, report, import_aliases, from_imports);
            }
        }

        // Recurse into try/except/finally bodies
        ast::Stmt::Try(try_stmt) => {
            for s in &try_stmt.body {
                analyze_stmt(s, report, import_aliases, from_imports);
            }
            for handler in &try_stmt.handlers {
                let ast::ExceptHandler::ExceptHandler(h) = handler;
                for s in &h.body {
                    analyze_stmt(s, report, import_aliases, from_imports);
                }
            }
            for s in &try_stmt.orelse {
                analyze_stmt(s, report, import_aliases, from_imports);
            }
            for s in &try_stmt.finalbody {
                analyze_stmt(s, report, import_aliases, from_imports);
            }
        }

        // Recurse into with statement bodies
        ast::Stmt::With(with_stmt) => {
            for s in &with_stmt.body {
                analyze_stmt(s, report, import_aliases, from_imports);
            }
        }

        // Recurse into async with statement bodies
        ast::Stmt::AsyncWith(async_with) => {
            for s in &async_with.body {
                analyze_stmt(s, report, import_aliases, from_imports);
            }
        }

        // Recurse into async for loop bodies
        ast::Stmt::AsyncFor(async_for) => {
            for s in &async_for.body {
                analyze_stmt(s, report, import_aliases, from_imports);
            }
            for s in &async_for.orelse {
                analyze_stmt(s, report, import_aliases, from_imports);
            }
        }

        // Other statements don't contain imports or toxic calls
        _ => {}
    }
}

// =============================================================================
// Expression Analysis
// =============================================================================

/// Check an expression for toxic patterns (dynamic imports, toxic calls)
fn check_expr_toxicity(
    expr: &ast::Expr,
    report: &mut ToxicityReport,
    import_aliases: &HashMap<String, String>,
    from_imports: &HashMap<String, String>,
) {
    match expr {
        ast::Expr::Call(call) => {
            // Check for dynamic imports
            if is_dynamic_import(&call.func, import_aliases, from_imports) {
                report.is_toxic = true;
                if !report.reasons.iter().any(|r| r.contains("Dynamic import")) {
                    report.reasons.push("Dynamic import detected".to_string());
                }
            }

            // Check for toxic function calls
            if let Some(reason) = check_call_toxicity(&call.func, import_aliases, from_imports) {
                report.is_toxic = true;
                if !report.reasons.contains(&reason) {
                    report.reasons.push(reason);
                }
            }

            // Recurse into call arguments
            for arg in &call.args {
                check_expr_toxicity(arg, report, import_aliases, from_imports);
            }
            for keyword in &call.keywords {
                check_expr_toxicity(&keyword.value, report, import_aliases, from_imports);
            }
        }

        // Recurse into binary operations
        ast::Expr::BinOp(binop) => {
            check_expr_toxicity(&binop.left, report, import_aliases, from_imports);
            check_expr_toxicity(&binop.right, report, import_aliases, from_imports);
        }

        // Recurse into unary operations
        ast::Expr::UnaryOp(unaryop) => {
            check_expr_toxicity(&unaryop.operand, report, import_aliases, from_imports);
        }

        // Recurse into lambda bodies
        ast::Expr::Lambda(lambda) => {
            check_expr_toxicity(&lambda.body, report, import_aliases, from_imports);
        }

        // Recurse into if expressions
        ast::Expr::IfExp(ifexp) => {
            check_expr_toxicity(&ifexp.test, report, import_aliases, from_imports);
            check_expr_toxicity(&ifexp.body, report, import_aliases, from_imports);
            check_expr_toxicity(&ifexp.orelse, report, import_aliases, from_imports);
        }

        // Recurse into list/set/dict comprehensions
        ast::Expr::ListComp(comp) => {
            check_expr_toxicity(&comp.elt, report, import_aliases, from_imports);
        }
        ast::Expr::SetComp(comp) => {
            check_expr_toxicity(&comp.elt, report, import_aliases, from_imports);
        }
        ast::Expr::DictComp(comp) => {
            check_expr_toxicity(&comp.key, report, import_aliases, from_imports);
            check_expr_toxicity(&comp.value, report, import_aliases, from_imports);
        }
        ast::Expr::GeneratorExp(gen) => {
            check_expr_toxicity(&gen.elt, report, import_aliases, from_imports);
        }

        // Recurse into await expressions
        ast::Expr::Await(await_expr) => {
            check_expr_toxicity(&await_expr.value, report, import_aliases, from_imports);
        }

        // Recurse into yield expressions
        ast::Expr::Yield(yield_expr) => {
            if let Some(ref value) = yield_expr.value {
                check_expr_toxicity(value, report, import_aliases, from_imports);
            }
        }
        ast::Expr::YieldFrom(yield_from) => {
            check_expr_toxicity(&yield_from.value, report, import_aliases, from_imports);
        }

        // Recurse into compare expressions
        ast::Expr::Compare(compare) => {
            check_expr_toxicity(&compare.left, report, import_aliases, from_imports);
            for comparator in &compare.comparators {
                check_expr_toxicity(comparator, report, import_aliases, from_imports);
            }
        }

        // Recurse into subscript expressions
        ast::Expr::Subscript(subscript) => {
            check_expr_toxicity(&subscript.value, report, import_aliases, from_imports);
            check_expr_toxicity(&subscript.slice, report, import_aliases, from_imports);
        }

        // Recurse into attribute access
        ast::Expr::Attribute(attr) => {
            check_expr_toxicity(&attr.value, report, import_aliases, from_imports);
        }

        // Recurse into starred expressions
        ast::Expr::Starred(starred) => {
            check_expr_toxicity(&starred.value, report, import_aliases, from_imports);
        }

        // Recurse into list/tuple/set literals
        ast::Expr::List(list) => {
            for elt in &list.elts {
                check_expr_toxicity(elt, report, import_aliases, from_imports);
            }
        }
        ast::Expr::Tuple(tuple) => {
            for elt in &tuple.elts {
                check_expr_toxicity(elt, report, import_aliases, from_imports);
            }
        }
        ast::Expr::Set(set) => {
            for elt in &set.elts {
                check_expr_toxicity(elt, report, import_aliases, from_imports);
            }
        }

        // Recurse into dict literals
        ast::Expr::Dict(dict) => {
            for k in dict.keys.iter().flatten() {
                check_expr_toxicity(k, report, import_aliases, from_imports);
            }
            for value in &dict.values {
                check_expr_toxicity(value, report, import_aliases, from_imports);
            }
        }

        // Other expressions don't contain toxic patterns
        _ => {}
    }
}

// =============================================================================
// Helper Functions
// =============================================================================

/// Check if a module name is in the toxic blocklists
fn is_toxic_module(name: &str) -> bool {
    // Check exact match
    if TOXIC_STD_LIB.contains(&name) || TOXIC_EXTERNAL_MODULES.contains(&name) {
        return true;
    }

    // Check prefix match for submodules (e.g., "concurrent.futures.thread")
    for toxic in TOXIC_STD_LIB.iter().chain(TOXIC_EXTERNAL_MODULES.iter()) {
        if name.starts_with(&format!("{}.", toxic)) {
            return true;
        }
    }

    false
}

/// Check if an if-statement test is `TYPE_CHECKING` or `typing.TYPE_CHECKING`
///
/// This is used to skip imports inside `if TYPE_CHECKING:` blocks,
/// which are only used for type hints and never executed at runtime.
fn is_type_checking_block(test: &ast::Expr) -> bool {
    match test {
        // `if TYPE_CHECKING:`
        ast::Expr::Name(name) => name.id.as_str() == "TYPE_CHECKING",
        // `if typing.TYPE_CHECKING:`
        ast::Expr::Attribute(attr) => {
            if attr.attr.as_str() == "TYPE_CHECKING" {
                if let ast::Expr::Name(name) = &*attr.value {
                    return name.id.as_str() == "typing";
                }
            }
            false
        }
        _ => false,
    }
}

/// Check if a call expression is a dynamic import
fn is_dynamic_import(
    func: &ast::Expr,
    import_aliases: &HashMap<String, String>,
    from_imports: &HashMap<String, String>,
) -> bool {
    match func {
        ast::Expr::Name(name) => {
            let local = name.id.as_str();
            // __import__ and exec are always dynamic
            if local == "__import__" || local == "exec" {
                return true;
            }
            // Check if this is importlib.import_module imported via from-import
            if let Some(full) = from_imports.get(local) {
                return full == "importlib.import_module";
            }
            false
        }
        ast::Expr::Attribute(attr) => {
            // Check for importlib.import_module pattern
            if let ast::Expr::Name(name) = &*attr.value {
                let local = name.id.as_str();
                let attr_name = attr.attr.as_str();
                let module = import_aliases
                    .get(local)
                    .map(|s| s.as_str())
                    .unwrap_or(local);
                return module == "importlib" && attr_name == "import_module";
            }
            false
        }
        _ => false,
    }
}

/// Check if a call is to a toxic function and return the reason
fn check_call_toxicity(
    func: &ast::Expr,
    import_aliases: &HashMap<String, String>,
    from_imports: &HashMap<String, String>,
) -> Option<String> {
    match func {
        ast::Expr::Name(name) => {
            let local = name.id.as_str();
            // Check if this is a from-imported toxic function
            if let Some(full) = from_imports.get(local) {
                let parts: Vec<&str> = full.split('.').collect();
                if !parts.is_empty() && is_toxic_module(parts[0]) {
                    return Some(format!("Called {}", full));
                }
            }
            None
        }
        ast::Expr::Attribute(attr) => {
            // Check for module.function pattern (e.g., threading.Thread())
            if let ast::Expr::Name(name) = &*attr.value {
                let local = name.id.as_str();
                let attr_name = attr.attr.as_str();
                let module = import_aliases
                    .get(local)
                    .map(|s| s.as_str())
                    .unwrap_or(local);
                if is_toxic_module(module) {
                    return Some(format!("Called {}.{}", module, attr_name));
                }
            }
            None
        }
        _ => None,
    }
}

// =============================================================================
// Unit Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn analyze(source: &str) -> ToxicityReport {
        analyze_file(source, &PathBuf::from("test.py"))
    }

    // =========================================================================
    // Basic Import Tests
    // =========================================================================

    #[test]
    fn test_import_threading() {
        let source = "import threading";
        let report = analyze(source);
        assert!(report.is_toxic);
        assert!(report.reasons.iter().any(|r| r.contains("threading")));
    }

    #[test]
    fn test_import_multiprocessing() {
        let source = "import multiprocessing";
        let report = analyze(source);
        assert!(report.is_toxic);
        assert!(report.reasons.iter().any(|r| r.contains("multiprocessing")));
    }

    #[test]
    fn test_import_socket() {
        let source = "import socket";
        let report = analyze(source);
        assert!(report.is_toxic);
        assert!(report.reasons.iter().any(|r| r.contains("socket")));
    }

    #[test]
    fn test_import_ctypes() {
        let source = "import ctypes";
        let report = analyze(source);
        assert!(report.is_toxic);
        assert!(report.reasons.iter().any(|r| r.contains("ctypes")));
    }

    #[test]
    fn test_import_signal() {
        let source = "import signal";
        let report = analyze(source);
        assert!(report.is_toxic);
        assert!(report.reasons.iter().any(|r| r.contains("signal")));
    }

    #[test]
    fn test_import_concurrent_futures() {
        let source = "import concurrent.futures";
        let report = analyze(source);
        assert!(report.is_toxic);
        assert!(report
            .reasons
            .iter()
            .any(|r| r.contains("concurrent.futures")));
    }

    // =========================================================================
    // From-Import Tests
    // =========================================================================

    #[test]
    fn test_from_import_thread() {
        let source = "from threading import Thread";
        let report = analyze(source);
        assert!(report.is_toxic);
        assert!(report.reasons.iter().any(|r| r.contains("threading")));
    }

    #[test]
    fn test_from_import_process() {
        let source = "from multiprocessing import Process";
        let report = analyze(source);
        assert!(report.is_toxic);
        assert!(report.reasons.iter().any(|r| r.contains("multiprocessing")));
    }

    // =========================================================================
    // Star Import Tests (Aggressive Toxicity)
    // =========================================================================

    #[test]
    fn test_star_import_toxic() {
        let source = "from threading import *";
        let report = analyze(source);
        assert!(report.is_toxic);
        assert!(report.reasons.iter().any(|r| r.contains("Star import")));
    }

    #[test]
    fn test_star_import_safe() {
        let source = "from os import *";
        let report = analyze(source);
        assert!(!report.is_toxic);
    }

    // =========================================================================
    // Aliased Import Tests
    // =========================================================================

    #[test]
    fn test_aliased_import() {
        let source = r#"
import threading as t
x = t.Thread()
"#;
        let report = analyze(source);
        assert!(report.is_toxic);
        assert!(report.reasons.iter().any(|r| r.contains("threading")));
    }

    #[test]
    fn test_aliased_import_call() {
        let source = r#"
import multiprocessing as mp
p = mp.Process(target=foo)
"#;
        let report = analyze(source);
        assert!(report.is_toxic);
        assert!(report.reasons.iter().any(|r| r.contains("multiprocessing")));
    }

    // =========================================================================
    // Dynamic Import Tests
    // =========================================================================

    #[test]
    fn test_dynamic_import_importlib() {
        let source = r#"
import importlib
mod = importlib.import_module("foo")
"#;
        let report = analyze(source);
        assert!(report.is_toxic);
        assert!(report.reasons.iter().any(|r| r.contains("Dynamic import")));
    }

    #[test]
    fn test_dynamic_import_from_importlib() {
        let source = r#"
from importlib import import_module
mod = import_module("foo")
"#;
        let report = analyze(source);
        assert!(report.is_toxic);
        assert!(report.reasons.iter().any(|r| r.contains("Dynamic import")));
    }

    #[test]
    fn test_dynamic_import_dunder() {
        let source = r#"
mod = __import__("threading")
"#;
        let report = analyze(source);
        assert!(report.is_toxic);
        assert!(report.reasons.iter().any(|r| r.contains("Dynamic import")));
    }

    #[test]
    fn test_dynamic_import_exec() {
        let source = r#"
exec("import threading")
"#;
        let report = analyze(source);
        assert!(report.is_toxic);
        assert!(report.reasons.iter().any(|r| r.contains("Dynamic import")));
    }

    // =========================================================================
    // External Package Tests
    // =========================================================================

    #[test]
    fn test_external_grpc() {
        let source = "import grpc";
        let report = analyze(source);
        assert!(report.is_toxic);
        assert!(report.reasons.iter().any(|r| r.contains("grpc")));
    }

    #[test]
    fn test_external_pandas() {
        let source = "import pandas";
        let report = analyze(source);
        assert!(report.is_toxic);
        assert!(report.reasons.iter().any(|r| r.contains("pandas")));
    }

    #[test]
    fn test_external_tensorflow() {
        let source = "import tensorflow";
        let report = analyze(source);
        assert!(report.is_toxic);
        assert!(report.reasons.iter().any(|r| r.contains("tensorflow")));
    }

    #[test]
    fn test_external_torch() {
        let source = "import torch";
        let report = analyze(source);
        assert!(report.is_toxic);
        assert!(report.reasons.iter().any(|r| r.contains("torch")));
    }

    #[test]
    fn test_external_cv2() {
        let source = "import cv2";
        let report = analyze(source);
        assert!(report.is_toxic);
        assert!(report.reasons.iter().any(|r| r.contains("cv2")));
    }

    #[test]
    fn test_external_gevent() {
        let source = "import gevent";
        let report = analyze(source);
        assert!(report.is_toxic);
        assert!(report.reasons.iter().any(|r| r.contains("gevent")));
    }

    #[test]
    fn test_external_cffi() {
        let source = "import cffi";
        let report = analyze(source);
        assert!(report.is_toxic);
        assert!(report.reasons.iter().any(|r| r.contains("cffi")));
    }

    // =========================================================================
    // Safe Module Tests
    // =========================================================================

    #[test]
    fn test_safe_module_os() {
        let source = r#"
import os
x = os.path.join("a", "b")
"#;
        let report = analyze(source);
        assert!(!report.is_toxic);
    }

    #[test]
    fn test_safe_module_json() {
        let source = r#"
import json
data = json.loads("{}")
"#;
        let report = analyze(source);
        assert!(!report.is_toxic);
    }

    #[test]
    fn test_safe_module_pathlib() {
        let source = r#"
from pathlib import Path
p = Path("/tmp")
"#;
        let report = analyze(source);
        assert!(!report.is_toxic);
    }

    #[test]
    fn test_safe_module_collections() {
        let source = r#"
from collections import defaultdict
d = defaultdict(list)
"#;
        let report = analyze(source);
        assert!(!report.is_toxic);
    }

    // =========================================================================
    // Function Body Tests (Imports inside functions are NOT ignored)
    // =========================================================================

    #[test]
    fn test_import_inside_function() {
        let source = r#"
def my_function():
    import threading
    t = threading.Thread()
"#;
        let report = analyze(source);
        assert!(report.is_toxic);
        assert!(report.reasons.iter().any(|r| r.contains("threading")));
    }

    #[test]
    fn test_import_inside_nested_function() {
        let source = r#"
def outer():
    def inner():
        import socket
        s = socket.socket()
    inner()
"#;
        let report = analyze(source);
        assert!(report.is_toxic);
        assert!(report.reasons.iter().any(|r| r.contains("socket")));
    }

    #[test]
    fn test_import_inside_class_method() {
        let source = r#"
class MyClass:
    def my_method(self):
        import ctypes
        lib = ctypes.CDLL("foo.so")
"#;
        let report = analyze(source);
        assert!(report.is_toxic);
        assert!(report.reasons.iter().any(|r| r.contains("ctypes")));
    }

    #[test]
    fn test_import_inside_async_function() {
        let source = r#"
async def my_async():
    import threading
    t = threading.Thread()
"#;
        let report = analyze(source);
        assert!(report.is_toxic);
        assert!(report.reasons.iter().any(|r| r.contains("threading")));
    }

    // =========================================================================
    // Control Flow Tests
    // =========================================================================

    #[test]
    fn test_import_inside_if() {
        let source = r#"
if True:
    import threading
"#;
        let report = analyze(source);
        assert!(report.is_toxic);
    }

    #[test]
    fn test_import_inside_try() {
        let source = r#"
try:
    import threading
except ImportError:
    pass
"#;
        let report = analyze(source);
        assert!(report.is_toxic);
    }

    #[test]
    fn test_import_inside_except() {
        let source = r#"
try:
    pass
except:
    import threading
"#;
        let report = analyze(source);
        assert!(report.is_toxic);
    }

    #[test]
    fn test_import_inside_with() {
        let source = r#"
with open("file") as f:
    import threading
"#;
        let report = analyze(source);
        assert!(report.is_toxic);
    }

    #[test]
    fn test_import_inside_for() {
        let source = r#"
for i in range(10):
    import threading
"#;
        let report = analyze(source);
        assert!(report.is_toxic);
    }

    #[test]
    fn test_import_inside_while() {
        let source = r#"
while True:
    import threading
    break
"#;
        let report = analyze(source);
        assert!(report.is_toxic);
    }

    // =========================================================================
    // Parse Error Tests
    // =========================================================================

    #[test]
    fn test_parse_error_is_toxic() {
        let source = "def broken(";
        let report = analyze(source);
        assert!(report.is_toxic);
        assert!(report.reasons.iter().any(|r| r.contains("Parse error")));
    }

    // =========================================================================
    // Import Tracking Tests
    // =========================================================================

    #[test]
    fn test_imports_are_tracked() {
        let source = r#"
import os
import json
from pathlib import Path
"#;
        let report = analyze(source);
        assert!(report.imports.contains(&"os".to_string()));
        assert!(report.imports.contains(&"json".to_string()));
        assert!(report.imports.contains(&"pathlib".to_string()));
    }

    // =========================================================================
    // Submodule Tests
    // =========================================================================

    #[test]
    fn test_submodule_toxic() {
        let source = "import concurrent.futures.thread";
        let report = analyze(source);
        assert!(report.is_toxic);
    }

    // =========================================================================
    // Mixed Content Tests
    // =========================================================================

    #[test]
    fn test_mixed_safe_and_toxic() {
        let source = r#"
import os
import json
import threading
"#;
        let report = analyze(source);
        assert!(report.is_toxic);
        assert!(report.reasons.len() == 1);
        assert!(report.reasons[0].contains("threading"));
    }

    #[test]
    fn test_multiple_toxic_imports() {
        let source = r#"
import threading
import multiprocessing
import socket
"#;
        let report = analyze(source);
        assert!(report.is_toxic);
        assert!(report.reasons.len() == 3);
    }

    // =========================================================================
    // Empty/Minimal File Tests
    // =========================================================================

    #[test]
    fn test_empty_file() {
        let source = "";
        let report = analyze(source);
        assert!(!report.is_toxic);
        assert!(report.reasons.is_empty());
        assert!(report.imports.is_empty());
    }

    #[test]
    fn test_comments_only() {
        let source = r#"
# This is a comment
# import threading  <- this should not trigger
"#;
        let report = analyze(source);
        assert!(!report.is_toxic);
    }

    #[test]
    fn test_docstring_only() {
        let source = r#"
"""
This module does something.
import threading  <- this should not trigger
"""
"#;
        let report = analyze(source);
        assert!(!report.is_toxic);
    }

    // =========================================================================
    // TYPE_CHECKING Skip Tests
    // =========================================================================

    #[test]
    fn test_type_checking_import_skipped() {
        let source = r#"
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    import threading  # Should NOT be detected as toxic
"#;
        let report = analyze(source);
        assert!(!report.is_toxic, "TYPE_CHECKING imports should be skipped");
    }

    #[test]
    fn test_typing_type_checking_import_skipped() {
        let source = r#"
import typing

if typing.TYPE_CHECKING:
    import ctypes  # Should NOT be detected as toxic
"#;
        let report = analyze(source);
        assert!(
            !report.is_toxic,
            "typing.TYPE_CHECKING imports should be skipped"
        );
    }

    #[test]
    fn test_type_checking_else_branch_analyzed() {
        let source = r#"
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    import threading  # Skipped
else:
    import socket  # Should be detected
"#;
        let report = analyze(source);
        assert!(report.is_toxic, "else branch should still be analyzed");
        assert!(report.reasons.iter().any(|r| r.contains("socket")));
    }

    #[test]
    fn test_regular_if_not_skipped() {
        let source = r#"
if True:
    import threading  # Should be detected
"#;
        let report = analyze(source);
        assert!(report.is_toxic, "regular if blocks should be analyzed");
    }
}
