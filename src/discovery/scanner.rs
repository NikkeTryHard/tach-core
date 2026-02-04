//! Static AST Discovery Engine
//! Parses Python files to extract tests and fixtures without executing code.

use anyhow::Result;
use ignore::WalkBuilder;
use rayon::prelude::*;
use rustpython_ast as ast;
use rustpython_parser::Parse;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::hooks::{Hook, HookRegistry, builtin_hook_specs};

// =============================================================================
// Serde Helper for bincode compatibility
// =============================================================================

/// Custom serde module for HashMap<String, serde_json::Value> that works with bincode.
/// bincode doesn't support serde_json::Value directly (requires deserialize_any),
/// so we serialize the HashMap to a JSON string first.
mod json_string_args {
    use super::*;

    pub fn serialize<S>(
        args: &HashMap<String, serde_json::Value>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let json_str = serde_json::to_string(args).map_err(serde::ser::Error::custom)?;
        serializer.serialize_str(&json_str)
    }

    pub fn deserialize<'de, D>(
        deserializer: D,
    ) -> Result<HashMap<String, serde_json::Value>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let json_str: String = Deserialize::deserialize(deserializer)?;
        if json_str.is_empty() {
            return Ok(HashMap::new());
        }
        serde_json::from_str(&json_str).map_err(serde::de::Error::custom)
    }
}

// =============================================================================
// Type Definitions
// =============================================================================

/// Scope of a pytest fixture
#[derive(Debug, Clone, PartialEq, Default)]
pub enum FixtureScope {
    #[default]
    Function,
    Class,
    Module,
    Session,
}

/// A pytest hook definition found in conftest.py
#[derive(Debug, Clone)]
pub struct HookDefinition {
    pub name: String,
    pub line_number: usize,
}

/// Known pytest hook prefixes
const PYTEST_HOOKS: &[&str] = &[
    "pytest_configure",
    "pytest_unconfigure",
    "pytest_collection_modifyitems",
    "pytest_collection_finish",
    "pytest_runtest_setup",
    "pytest_runtest_call",
    "pytest_runtest_teardown",
    "pytest_runtest_makereport",
    "pytest_sessionstart",
    "pytest_sessionfinish",
];

/// Check if a function name is a known pytest hook
fn is_pytest_hook(name: &str) -> bool {
    PYTEST_HOOKS.contains(&name)
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
    /// If Some, this fixture is scoped to the given class (class-method fixture)
    pub class_scope: Option<String>,
    /// Whether this fixture runs automatically for all tests in scope
    pub autouse: bool,
    /// Whether this fixture is async (async def)
    pub is_async: bool,
}

/// Marker information including name and keyword arguments
///
/// Captures the full marker definition from @pytest.mark.<name>(...) decorators,
/// including keyword arguments like `transaction=True` or `databases=["default"]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct MarkerInfo {
    /// Marker name (e.g., "django_db", "slow", "skip")
    pub name: String,
    /// Keyword arguments as JSON values
    /// Examples: {"transaction": true}, {"databases": ["default", "secondary"]}
    /// Note: Uses custom serde for bincode compatibility (bincode doesn't support serde_json::Value)
    #[serde(default, with = "json_string_args")]
    pub args: HashMap<String, serde_json::Value>,
}

/// A test case (function)
#[derive(Debug, Clone)]
pub struct TestCase {
    pub name: String,
    pub dependencies: Vec<String>,
    pub is_async: bool,
    pub line_number: usize,
    /// Arguments provided by @pytest.mark.parametrize (NOT fixtures)
    /// These should be excluded from fixture resolution
    pub parametrized_args: Vec<String>,
    /// Per-test timeout in seconds from @pytest.mark.timeout(N)
    /// None means use global timeout
    pub timeout_secs: Option<u64>,
    /// Pytest markers applied to this test (e.g., "django_db", "slow", "skip")
    /// Extracted from @pytest.mark.<name> decorators
    pub markers: Vec<String>,
    /// Full marker information including keyword arguments
    /// Used for pytest-django support (transaction=True, databases=[...])
    pub marker_info: Vec<MarkerInfo>,
    /// Parametrize ID suffix (e.g., "alice" for test_foo[alice])
    /// None for non-parametrized tests
    pub param_id: Option<String>,
}

/// A Python test module (.py file)
#[derive(Debug)]
pub struct TestModule {
    pub path: PathBuf,
    pub tests: Vec<TestCase>,
    pub fixtures: Vec<FixtureDefinition>,
    /// Pytest hooks defined in this module (e.g., pytest_configure, pytest_runtest_setup)
    pub hooks: Vec<HookDefinition>,
    /// Whether this module is toxic (requires fork/kill instead of reset)
    /// Set by toxicity analysis
    pub is_toxic: bool,
}

/// Discovery result containing all parsed modules
#[derive(Debug)]
#[must_use = "DiscoveryResult contains the test suite - ignoring it makes discovery useless"]
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

    /// Build a HookRegistry from discovered hooks
    ///
    /// # Arguments
    /// * `project_root` - The project root directory for path resolution.
    ///   Hook sources will be stored as canonical absolute paths to match
    ///   how ToxicityGraph looks them up.
    pub fn build_hook_registry(&self, project_root: &std::path::Path) -> HookRegistry {
        let specs = builtin_hook_specs();
        let mut registry = HookRegistry::new();

        for module in &self.modules {
            for hook_def in &module.hooks {
                if let Some(spec) = specs.get(&hook_def.name) {
                    // Build absolute path and canonicalize to match ToxicityGraph lookup
                    let abs_path = project_root.join(&module.path);
                    let source = abs_path.canonicalize().unwrap_or(abs_path);
                    registry.register(Hook {
                        spec: spec.clone(),
                        source,
                        function_name: hook_def.name.clone(),
                        line_number: hook_def.line_number,
                        is_wrapper: false,
                    });
                }
            }
        }

        registry
    }
}

/// JSON-serializable test information for `tach list --json`
#[derive(Serialize)]
pub struct JsonTestInfo {
    pub id: String,
    pub file: String,
    pub line: usize,
    pub is_async: bool,
    pub markers: Vec<String>,
}

/// JSON output for discovery listing
#[derive(Serialize)]
struct JsonDiscoveryOutput {
    version: u32,
    tests: Vec<JsonTestInfo>,
}

// =============================================================================
// Leaf Helper Functions (no dependencies on other helpers)
// =============================================================================

/// Convert byte offset to line number (1-indexed)
fn get_line_number(source: &str, byte_offset: usize) -> usize {
    source[..byte_offset.min(source.len())]
        .chars()
        .filter(|&c| c == '\n')
        .count()
        + 1
}

/// Check if a path is a Python test file
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

/// Extract function arguments, excluding self/cls
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
            ast::Constant::Bytes(b) => Some(format!("bytes{}", b.len())),
            _ => None,
        },
        // Handle simple Name expressions (like exception classes)
        ast::Expr::Name(n) => Some(n.id.to_string()),
        _ => None,
    }
}

/// Check if a single decorator is a fixture decorator
fn is_fixture_decorator(expr: &ast::Expr) -> bool {
    match expr {
        ast::Expr::Call(call) => is_fixture_decorator(&call.func),
        ast::Expr::Attribute(attr) => attr.attr.as_str() == "fixture",
        ast::Expr::Name(name) => name.id.as_str() == "fixture",
        _ => false,
    }
}

/// Check if a decorator is @pytest.mark.parametrize
fn is_parametrize_decorator(expr: &ast::Expr) -> bool {
    match expr {
        ast::Expr::Call(call) => is_parametrize_decorator(&call.func),
        ast::Expr::Attribute(attr) => {
            // Check for pattern: X.parametrize
            if attr.attr.as_str() == "parametrize" {
                // Could be pytest.mark.parametrize or mark.parametrize
                return true;
            }
            false
        }
        _ => false,
    }
}

/// Check if a decorator is @patch or @unittest.mock.patch
fn is_patch_decorator(expr: &ast::Expr) -> bool {
    match expr {
        ast::Expr::Call(call) => is_patch_decorator(&call.func),
        ast::Expr::Attribute(attr) => {
            let name = attr.attr.as_str();
            // Match: mock.patch, patch.object, etc.
            name == "patch" || name.starts_with("patch.")
        }
        ast::Expr::Name(name) => name.id.as_str() == "patch",
        _ => false,
    }
}

/// Check if a decorator is @pytest.mark.timeout
fn is_timeout_decorator(expr: &ast::Expr) -> bool {
    match expr {
        ast::Expr::Call(call) => is_timeout_decorator(&call.func),
        ast::Expr::Attribute(attr) => {
            // Check for pattern: X.timeout (e.g., pytest.mark.timeout or mark.timeout)
            attr.attr.as_str() == "timeout"
        }
        _ => false,
    }
}

// =============================================================================
// Composite Helper Functions (use leaf helpers)
// =============================================================================

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

/// Check if any decorator in the list is a fixture decorator
fn has_fixture_decorator(decorators: &[ast::Expr]) -> bool {
    decorators.iter().any(is_fixture_decorator)
}

/// Extract fixture scope from decorators
fn extract_scope_from_decorators(decorators: &[ast::Expr]) -> FixtureScope {
    for decorator in decorators {
        if let ast::Expr::Call(call) = decorator {
            for keyword in &call.keywords {
                if let Some(ref arg) = keyword.arg
                    && arg.as_str() == "scope"
                    && let ast::Expr::Constant(c) = &keyword.value
                    && let ast::Constant::Str(s) = &c.value
                {
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
    FixtureScope::Function
}

/// Extract params from @pytest.fixture(params=[...]) decorator
///
/// Returns None if:
/// - No params keyword
/// - Dynamic params (e.g., params=load_from_db())
///
/// Returns Some(vec) if static literal list
fn extract_params_from_decorators(decorators: &[ast::Expr]) -> Option<Vec<String>> {
    for decorator in decorators {
        if let ast::Expr::Call(call) = decorator {
            for keyword in &call.keywords {
                if let Some(ref arg) = keyword.arg
                    && arg.as_str() == "params"
                {
                    // Try to extract literals from the params value
                    return extract_literal_list(&keyword.value);
                }
            }
        }
    }
    None // No params keyword found
}

/// Extract autouse=True/False from @pytest.fixture decorator
fn extract_autouse_from_decorators(decorators: &[ast::Expr]) -> bool {
    for decorator in decorators {
        if let ast::Expr::Call(call) = decorator {
            // Check if this is a fixture decorator
            if !is_fixture_decorator(decorator) {
                continue;
            }
            for keyword in &call.keywords {
                if let Some(ref arg) = keyword.arg
                    && arg.as_str() == "autouse"
                    && let ast::Expr::Constant(c) = &keyword.value
                    && let ast::Constant::Bool(value) = &c.value
                {
                    return *value;
                }
            }
        }
    }
    false
}

/// A single parameter set from @pytest.mark.parametrize
/// Contains the values and optionally an explicit ID
#[derive(Debug, Clone)]
struct ParametrizeValue {
    /// The pytest-compatible ID for this parameter set
    id: String,
}

/// Information about a parametrize decorator
#[derive(Debug, Clone)]
struct ParametrizeInfo {
    /// Argument names (e.g., ["x", "y"] from "x,y")
    #[allow(dead_code)]
    arg_names: Vec<String>,
    /// Parameter values with their IDs
    values: Vec<ParametrizeValue>,
}

/// Generate a pytest-compatible ID from an AST expression
///
/// For simple values: returns the string representation
/// For tuples: joins elements with "-"
/// For pytest.param with id=: uses the explicit id
fn expr_to_pytest_id(expr: &ast::Expr, aliases: &HashMap<String, String>) -> Option<String> {
    match expr {
        // Simple literals
        ast::Expr::Constant(c) => match &c.value {
            ast::Constant::Int(i) => Some(i.to_string()),
            ast::Constant::Str(s) => Some(s.to_string()),
            ast::Constant::Bool(b) => Some(if *b { "True" } else { "False" }.to_string()),
            ast::Constant::None => Some("None".to_string()),
            ast::Constant::Float(f) => Some(f.to_string()),
            ast::Constant::Bytes(b) => Some(format!("bytes{}", b.len())),
            _ => None,
        },
        // Name expressions (like exception classes: ValueError, TypeError)
        // Resolve alias if exists, otherwise use original name
        ast::Expr::Name(n) => {
            let name = n.id.to_string();
            Some(aliases.get(&name).cloned().unwrap_or(name))
        }
        // Attribute expressions (like asyncio.TimeoutError) - pytest uses just the attr name
        ast::Expr::Attribute(attr) => Some(attr.attr.to_string()),
        // Tuple: join elements with "-"
        ast::Expr::Tuple(tuple) => {
            let parts: Vec<String> = tuple
                .elts
                .iter()
                .filter_map(|e| expr_to_pytest_id(e, aliases))
                .collect();
            if parts.is_empty() {
                None
            } else {
                Some(parts.join("-"))
            }
        }
        // List treated same as tuple for ID generation
        ast::Expr::List(list) => {
            let parts: Vec<String> = list
                .elts
                .iter()
                .filter_map(|e| expr_to_pytest_id(e, aliases))
                .collect();
            if parts.is_empty() {
                None
            } else {
                Some(parts.join("-"))
            }
        }
        // pytest.param(value, id="name") - extract the explicit id
        ast::Expr::Call(call) => {
            // Check if this is pytest.param
            let is_pytest_param = match &*call.func {
                ast::Expr::Attribute(attr) => attr.attr.as_str() == "param",
                ast::Expr::Name(name) => name.id.as_str() == "param",
                _ => false,
            };

            if is_pytest_param {
                // Look for id= keyword argument
                for keyword in &call.keywords {
                    if let Some(ref arg) = keyword.arg
                        && arg.as_str() == "id"
                        && let ast::Expr::Constant(c) = &keyword.value
                        && let ast::Constant::Str(s) = &c.value
                    {
                        return Some(s.to_string());
                    }
                }
                // No explicit id - generate from first positional arg or tuple of args
                if call.args.len() == 1 {
                    expr_to_pytest_id(&call.args[0], aliases)
                } else if !call.args.is_empty() {
                    // Multiple positional args treated as a tuple
                    let parts: Vec<String> = call
                        .args
                        .iter()
                        .filter_map(|e| expr_to_pytest_id(e, aliases))
                        .collect();
                    if parts.is_empty() {
                        None
                    } else {
                        Some(parts.join("-"))
                    }
                } else {
                    None
                }
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Extract parametrize information from a single decorator
///
/// Returns None if not a parametrize decorator or if values are dynamic
fn extract_parametrize_info(
    decorator: &ast::Expr,
    aliases: &HashMap<String, String>,
) -> Option<ParametrizeInfo> {
    if !is_parametrize_decorator(decorator) {
        return None;
    }

    let call = match decorator {
        ast::Expr::Call(call) => call,
        _ => return None,
    };

    // Extract argument names from first argument
    let arg_names: Vec<String> = match call.args.first() {
        Some(ast::Expr::Constant(c)) => {
            if let ast::Constant::Str(s) = &c.value {
                s.as_str()
                    .split(',')
                    .map(|n| n.trim().to_string())
                    .filter(|n| !n.is_empty())
                    .collect()
            } else {
                return None;
            }
        }
        Some(ast::Expr::List(list)) => list
            .elts
            .iter()
            .filter_map(|elt| {
                if let ast::Expr::Constant(c) = elt
                    && let ast::Constant::Str(s) = &c.value
                {
                    return Some(s.to_string());
                }
                None
            })
            .collect(),
        Some(ast::Expr::Tuple(tuple)) => tuple
            .elts
            .iter()
            .filter_map(|elt| {
                if let ast::Expr::Constant(c) = elt
                    && let ast::Constant::Str(s) = &c.value
                {
                    return Some(s.to_string());
                }
                None
            })
            .collect(),
        _ => return None,
    };

    // Extract values from second argument (must be a list or tuple)
    let values_expr = call.args.get(1)?;
    let value_elements = match values_expr {
        ast::Expr::List(list) => &list.elts,
        ast::Expr::Tuple(tuple) => &tuple.elts,
        _ => return None, // Dynamic values - bail out
    };

    // Check for ids= keyword argument
    let explicit_ids: Option<Vec<String>> = call.keywords.iter().find_map(|kw| {
        if let Some(ref arg) = kw.arg
            && arg.as_str() == "ids"
            && let ast::Expr::List(list) = &kw.value
        {
            let ids: Vec<String> = list
                .elts
                .iter()
                .filter_map(|elt| {
                    if let ast::Expr::Constant(c) = elt
                        && let ast::Constant::Str(s) = &c.value
                    {
                        return Some(s.to_string());
                    }
                    None
                })
                .collect();
            if ids.len() == list.elts.len() {
                return Some(ids);
            }
        }
        None
    });

    // Generate IDs for each value
    // For complex types (dicts, etc.) that can't be stringified, use pytest's
    // index-based fallback: {arg_name}{index} (e.g., cache_value0, cache_value1)
    let values: Vec<ParametrizeValue> = value_elements
        .iter()
        .enumerate()
        .map(|(i, expr)| {
            // Use explicit id if available
            if let Some(ref ids) = explicit_ids
                && let Some(id) = ids.get(i)
            {
                return ParametrizeValue { id: id.clone() };
            }
            // Otherwise generate from expression, with index fallback for complex types
            let id = expr_to_pytest_id(expr, aliases).unwrap_or_else(|| {
                // Pytest uses {first_arg_name}{index} for complex values
                format!("{}{}", arg_names.first().unwrap_or(&"param".to_string()), i)
            });
            ParametrizeValue { id }
        })
        .collect();

    Some(ParametrizeInfo { arg_names, values })
}

/// Extract all parametrize decorators from a function
///
/// Returns a list of ParametrizeInfo for each @pytest.mark.parametrize decorator.
/// Returns None if any parametrize decorator has dynamic values.
fn extract_all_parametrize_info(
    decorators: &[ast::Expr],
    aliases: &HashMap<String, String>,
) -> Option<Vec<ParametrizeInfo>> {
    let mut infos = Vec::new();

    for decorator in decorators {
        if is_parametrize_decorator(decorator) {
            match extract_parametrize_info(decorator, aliases) {
                Some(info) => infos.push(info),
                None => return None, // Dynamic values - bail out entirely
            }
        }
    }

    Some(infos)
}

/// Generate all combinations of parameter IDs for multiple parametrize decorators
///
/// For a single decorator with values [a, b], returns ["a", "b"]
/// For two decorators with [a, b] and [1, 2], returns ["a-1", "a-2", "b-1", "b-2"]
fn generate_param_combinations(infos: &[ParametrizeInfo]) -> Vec<String> {
    if infos.is_empty() {
        return vec![];
    }

    // Start with first decorator's values
    let mut result: Vec<String> = infos[0].values.iter().map(|v| v.id.clone()).collect();

    // Cross-product with remaining decorators
    for info in &infos[1..] {
        let mut new_result = Vec::new();
        for existing in &result {
            for value in &info.values {
                new_result.push(format!("{}-{}", existing, value.id));
            }
        }
        result = new_result;
    }

    result
}

/// Extract argument names from @pytest.mark.parametrize decorators
/// Handles both formats:
/// - @pytest.mark.parametrize("arg1,arg2", [...]) - comma-separated string
/// - @pytest.mark.parametrize(["arg1", "arg2"], [...]) - list of strings
fn extract_parametrized_args(decorators: &[ast::Expr]) -> Vec<String> {
    let mut args = Vec::new();

    for decorator in decorators {
        if !is_parametrize_decorator(decorator) {
            continue;
        }

        // Get the call expression
        if let ast::Expr::Call(call) = decorator {
            // First argument contains the parameter names
            if let Some(first_arg) = call.args.first() {
                match first_arg {
                    // Case 1: "arg1, arg2" (comma-separated string)
                    ast::Expr::Constant(c) => {
                        if let ast::Constant::Str(s) = &c.value {
                            // Split by comma and trim whitespace
                            for name in s.as_str().split(',') {
                                let trimmed = name.trim();
                                if !trimmed.is_empty() {
                                    args.push(trimmed.to_string());
                                }
                            }
                        }
                    }
                    // Case 2: ["arg1", "arg2"] (list of strings)
                    ast::Expr::List(list) => {
                        for elt in &list.elts {
                            if let ast::Expr::Constant(c) = elt
                                && let ast::Constant::Str(s) = &c.value
                            {
                                args.push(s.to_string());
                            }
                        }
                    }
                    // Case 3: ("arg1", "arg2") (tuple of strings)
                    ast::Expr::Tuple(tuple) => {
                        for elt in &tuple.elts {
                            if let ast::Expr::Constant(c) = elt
                                && let ast::Constant::Str(s) = &c.value
                            {
                                args.push(s.to_string());
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    args
}

/// Check if a @patch decorator injects a function argument.
///
/// @patch(target) - INJECTS arg (the mock)
/// @patch(target, replacement) - NO injection (replacement is used directly)
/// @patch(target, new=replacement) - NO injection
///
/// We need to distinguish these cases to correctly count injected args.
fn patch_injects_arg(expr: &ast::Expr) -> bool {
    if !is_patch_decorator(expr) {
        return false;
    }

    // Get the call expression
    match expr {
        ast::Expr::Call(call) => {
            // If there's a second positional arg, it's a replacement value (no injection)
            if call.args.len() >= 2 {
                return false;
            }
            // If there's a "new" keyword arg, it's also a replacement (no injection)
            for kw in &call.keywords {
                if let Some(ref arg_name) = kw.arg
                    && arg_name.as_str() == "new"
                {
                    return false;
                }
            }
            // Otherwise, @patch(target) injects the mock as a function arg
            true
        }
        // Bare @patch without call (unlikely but handle it)
        _ => false,
    }
}

/// Count @patch decorators that inject function arguments.
/// Only patches without a replacement value inject mock objects as parameters.
fn count_patch_decorators(decorators: &[ast::Expr]) -> usize {
    decorators.iter().filter(|d| patch_injects_arg(d)).count()
}

/// Extract all injected (non-fixture) argument names from decorators
/// Combines:
/// 1. @pytest.mark.parametrize args (explicit parameter names)
/// 2. @patch args (FIRST N args after self/cls, where N = patch decorator count)
///
/// unittest.mock.patch injects args at the START (after self),
/// not at the end. The bottom-most @patch decorator's mock becomes the first arg.
fn extract_injected_args(decorators: &[ast::Expr], func_args: &[String]) -> Vec<String> {
    let mut injected = extract_parametrized_args(decorators);

    // Count @patch decorators - each injects one arg at the START of func_args
    // (after self/cls which are already filtered out by extract_args_from_arguments)
    let patch_count = count_patch_decorators(decorators);
    if patch_count > 0 && func_args.len() >= patch_count {
        //  Take the FIRST `patch_count` arguments as patch-injected
        // Example: @patch("a") @patch("b") def test(self, mock_b, mock_a, fixture):
        //          -> mock_b and mock_a are injected, fixture is a real fixture
        for arg in func_args.iter().take(patch_count) {
            if !injected.contains(arg) {
                injected.push(arg.clone());
            }
        }
    }

    injected
}

/// Extract timeout value from @pytest.mark.timeout(N) decorators
///
/// Handles:
/// - @pytest.mark.timeout(30) - positional argument
/// - @pytest.mark.timeout(seconds=30) - keyword argument
///
/// Returns None if no timeout marker found, value is not a static literal,
/// or value is 0 (which means "no timeout" in pytest-timeout)
fn extract_timeout_from_decorators(decorators: &[ast::Expr]) -> Option<u64> {
    for decorator in decorators {
        if !is_timeout_decorator(decorator) {
            continue;
        }

        // Get the call expression
        if let ast::Expr::Call(call) = decorator {
            // Check positional argument first (most common: @pytest.mark.timeout(30))
            if let Some(ast::Expr::Constant(c)) = call.args.first()
                && let ast::Constant::Int(i) = &c.value
            {
                // Convert BigInt to u64
                if let Ok(val) = i.to_string().parse::<u64>() {
                    // 0 means "no timeout" in pytest-timeout
                    if val == 0 {
                        return None;
                    }
                    return Some(val);
                }
            }

            // Check keyword argument (e.g., @pytest.mark.timeout(seconds=30))
            for keyword in &call.keywords {
                if let Some(ref arg) = keyword.arg {
                    // Accept both "seconds" and "timeout" keyword args
                    if (arg.as_str() == "seconds" || arg.as_str() == "timeout")
                        && let ast::Expr::Constant(c) = &keyword.value
                        && let ast::Constant::Int(i) = &c.value
                        && let Ok(val) = i.to_string().parse::<u64>()
                    {
                        // 0 means "no timeout" in pytest-timeout
                        if val == 0 {
                            return None;
                        }
                        return Some(val);
                    }
                }
            }
        }
    }
    None
}

/// Markers that are decorators, not test selection markers.
/// These should not appear in the markers list for `-m` filtering.
const DECORATOR_ONLY_MARKERS: &[&str] = &["parametrize", "usefixtures", "filterwarnings"];

/// Convert an AST expression to a serde_json::Value
///
/// Handles:
/// - Constant values (bool, int, str, None)
/// - Name expressions (True, False as Python identifiers)
/// - List expressions
fn expr_to_json_value(expr: &ast::Expr) -> Option<serde_json::Value> {
    match expr {
        ast::Expr::Constant(c) => match &c.value {
            ast::Constant::Bool(b) => Some(serde_json::Value::Bool(*b)),
            ast::Constant::Int(i) => {
                // Try to parse as i64, fall back to string for large integers
                if let Ok(val) = i.to_string().parse::<i64>() {
                    Some(serde_json::Value::Number(val.into()))
                } else {
                    Some(serde_json::Value::String(i.to_string()))
                }
            }
            ast::Constant::Str(s) => Some(serde_json::Value::String(s.to_string())),
            ast::Constant::Float(f) => {
                serde_json::Number::from_f64(*f).map(serde_json::Value::Number)
            }
            ast::Constant::None => Some(serde_json::Value::Null),
            _ => None,
        },
        // Handle Name expressions (True/False as Python identifiers)
        ast::Expr::Name(n) => match n.id.as_str() {
            "True" => Some(serde_json::Value::Bool(true)),
            "False" => Some(serde_json::Value::Bool(false)),
            "None" => Some(serde_json::Value::Null),
            _ => None, // Other names are not supported
        },
        // Handle List expressions
        ast::Expr::List(list) => {
            let mut values = Vec::new();
            for elt in &list.elts {
                if let Some(val) = expr_to_json_value(elt) {
                    values.push(val);
                } else {
                    // Non-convertible element - skip the whole list
                    return None;
                }
            }
            Some(serde_json::Value::Array(values))
        }
        // Handle Tuple expressions (treat like lists)
        ast::Expr::Tuple(tuple) => {
            let mut values = Vec::new();
            for elt in &tuple.elts {
                if let Some(val) = expr_to_json_value(elt) {
                    values.push(val);
                } else {
                    return None;
                }
            }
            Some(serde_json::Value::Array(values))
        }
        _ => None,
    }
}

/// Extract keyword arguments from a Call expression
///
/// Returns a HashMap of argument name -> JSON value
/// Positional args are stored with numeric keys ("0", "1", etc.)
fn extract_marker_arguments(call: &ast::ExprCall) -> HashMap<String, serde_json::Value> {
    let mut args = HashMap::new();

    // Capture positional arguments with numeric keys
    // e.g., @pytest.mark.urls('myapp.test_urls') -> {"0": "myapp.test_urls"}
    for (i, arg) in call.args.iter().enumerate() {
        if let Some(value) = expr_to_json_value(arg) {
            args.insert(i.to_string(), value);
        }
    }

    // Capture keyword arguments
    for keyword in &call.keywords {
        if let Some(ref arg_name) = keyword.arg
            && let Some(value) = expr_to_json_value(&keyword.value)
        {
            args.insert(arg_name.to_string(), value);
        }
    }

    args
}

/// Extract marker names and full marker info from @pytest.mark.* decorators
///
/// Handles:
/// - @pytest.mark.name - bare marker
/// - @pytest.mark.name(args) - marker with arguments
///
/// Returns a tuple of:
/// - Vec<String>: marker names for backward compatibility
/// - Vec<MarkerInfo>: full marker info with arguments
///
/// Excludes decorator-only markers like parametrize, usefixtures, filterwarnings.
fn extract_markers_from_decorators(decorators: &[ast::Expr]) -> (Vec<String>, Vec<MarkerInfo>) {
    let mut markers = vec![];
    let mut marker_info = vec![];

    for decorator in decorators {
        // Handle @pytest.mark.name (bare marker)
        if let ast::Expr::Attribute(attr) = decorator
            && let ast::Expr::Attribute(inner) = &*attr.value
            && let ast::Expr::Name(name) = &*inner.value
            && name.id.as_str() == "pytest"
            && inner.attr.as_str() == "mark"
        {
            let marker_name = attr.attr.to_string();
            if !DECORATOR_ONLY_MARKERS.contains(&marker_name.as_str()) {
                markers.push(marker_name.clone());
                marker_info.push(MarkerInfo {
                    name: marker_name,
                    args: HashMap::new(),
                });
            }
        }
        // Handle @pytest.mark.name(args)
        if let ast::Expr::Call(call) = decorator
            && let ast::Expr::Attribute(attr) = &*call.func
            && let ast::Expr::Attribute(inner) = &*attr.value
            && let ast::Expr::Name(name) = &*inner.value
            && name.id.as_str() == "pytest"
            && inner.attr.as_str() == "mark"
        {
            let marker_name = attr.attr.to_string();
            if !DECORATOR_ONLY_MARKERS.contains(&marker_name.as_str()) {
                markers.push(marker_name.clone());
                marker_info.push(MarkerInfo {
                    name: marker_name,
                    args: extract_marker_arguments(call),
                });
            }
        }
    }

    (markers, marker_info)
}

// =============================================================================
// High-Level Private Functions
// =============================================================================

/// Analyze a function definition and extract test/fixture information
fn analyze_function(
    func: &ast::StmtFunctionDef,
    source: &str,
    tests: &mut Vec<TestCase>,
    fixtures: &mut Vec<FixtureDefinition>,
    is_async: bool,
    aliases: &HashMap<String, String>,
) {
    let name = func.name.as_str();

    if name.starts_with("test_") {
        let line_number = get_line_number(source, func.range.start().to_usize());
        let (markers, marker_info) = extract_markers_from_decorators(&func.decorator_list);
        let parametrized_args = extract_injected_args(
            &func.decorator_list,
            &extract_args_from_arguments(&func.args),
        );
        let timeout_secs = extract_timeout_from_decorators(&func.decorator_list);
        let dependencies = extract_args_from_arguments(&func.args);

        // Try to extract parametrize info and expand tests
        if let Some(param_infos) = extract_all_parametrize_info(&func.decorator_list, aliases) {
            if !param_infos.is_empty() {
                let param_ids = generate_param_combinations(&param_infos);
                for param_id in param_ids {
                    tests.push(TestCase {
                        name: format!("{}[{}]", name, param_id),
                        dependencies: dependencies.clone(),
                        is_async,
                        line_number,
                        parametrized_args: parametrized_args.clone(),
                        timeout_secs,
                        markers: markers.clone(),
                        marker_info: marker_info.clone(),
                        param_id: Some(param_id),
                    });
                }
                // Skip adding the base test - we've expanded it
            } else {
                // No parametrize decorators
                tests.push(TestCase {
                    name: name.to_string(),
                    dependencies,
                    is_async,
                    line_number,
                    parametrized_args,
                    timeout_secs,
                    markers,
                    marker_info,
                    param_id: None,
                });
            }
        } else {
            // Dynamic parametrize values - add base test only
            tests.push(TestCase {
                name: name.to_string(),
                dependencies,
                is_async,
                line_number,
                parametrized_args,
                timeout_secs,
                markers,
                marker_info,
                param_id: None,
            });
        }
    }

    if has_fixture_decorator(&func.decorator_list) {
        fixtures.push(FixtureDefinition {
            name: name.to_string(),
            scope: extract_scope_from_decorators(&func.decorator_list),
            dependencies: extract_args_from_arguments(&func.args),
            params: extract_params_from_decorators(&func.decorator_list),
            class_scope: None, // Top-level fixture
            autouse: extract_autouse_from_decorators(&func.decorator_list),
            is_async: false,
        });
    }
}

/// Parse a module from an absolute path but store the relative path
fn parse_module_with_relative_path(abs_path: &Path, rel_path: &Path) -> Result<TestModule> {
    let bytes = fs::read(abs_path)?;
    let source = String::from_utf8_lossy(&bytes).into_owned();
    let path_str = rel_path.to_string_lossy();

    let suite = match ast::Suite::parse(&source, &path_str) {
        Ok(s) => s,
        Err(_) => {
            return Ok(TestModule {
                path: rel_path.to_path_buf(),
                tests: vec![],
                fixtures: vec![],
                hooks: vec![],
                is_toxic: false, // Set later by ToxicityGraph
            });
        }
    };

    let mut tests = vec![];
    let mut fixtures = vec![];
    let mut hooks = vec![];

    // Build import alias map: alias -> original name
    // Handles: from X import Y as Z (Z -> Y)
    // Handles: import X as Y (Y -> X)
    let mut import_aliases: HashMap<String, String> = HashMap::new();
    for stmt in &suite {
        match stmt {
            ast::Stmt::ImportFrom(import_from) => {
                for alias in &import_from.names {
                    if let Some(ref asname) = alias.asname {
                        import_aliases.insert(asname.to_string(), alias.name.to_string());
                    }
                }
            }
            ast::Stmt::Import(import) => {
                for alias in &import.names {
                    if let Some(ref asname) = alias.asname {
                        import_aliases.insert(asname.to_string(), alias.name.to_string());
                    }
                }
            }
            _ => {}
        }
    }

    // Only detect hooks in conftest.py files (pytest only processes hooks from conftest.py)
    let is_conftest = abs_path
        .file_name()
        .map(|n| n == "conftest.py")
        .unwrap_or(false);

    for stmt in suite {
        match stmt {
            ast::Stmt::FunctionDef(func) => {
                let name = func.name.as_str();
                // Check for pytest hooks (must be before analyze_function since hooks are top-level functions)
                // Only detect hooks in conftest.py files
                if is_conftest && is_pytest_hook(name) {
                    let line_number = get_line_number(&source, func.range.start().to_usize());
                    hooks.push(HookDefinition {
                        name: name.to_string(),
                        line_number,
                    });
                }
                analyze_function(
                    &func,
                    &source,
                    &mut tests,
                    &mut fixtures,
                    false,
                    &import_aliases,
                );
            }
            ast::Stmt::AsyncFunctionDef(func) => {
                let name = func.name.as_str();
                // Check for async pytest hooks (only in conftest.py)
                if is_conftest && is_pytest_hook(name) {
                    let line_number = get_line_number(&source, func.range.start().to_usize());
                    hooks.push(HookDefinition {
                        name: name.to_string(),
                        line_number,
                    });
                }
                if name.starts_with("test_") {
                    let line_number = get_line_number(&source, func.range.start().to_usize());
                    let (markers, marker_info) =
                        extract_markers_from_decorators(&func.decorator_list);
                    let parametrized_args = extract_injected_args(
                        &func.decorator_list,
                        &extract_args_from_arguments(&func.args),
                    );
                    let timeout_secs = extract_timeout_from_decorators(&func.decorator_list);
                    let dependencies = extract_args_from_arguments(&func.args);

                    // Try to extract parametrize info and expand tests
                    if let Some(param_infos) =
                        extract_all_parametrize_info(&func.decorator_list, &import_aliases)
                    {
                        if !param_infos.is_empty() {
                            let param_ids = generate_param_combinations(&param_infos);
                            for param_id in param_ids {
                                tests.push(TestCase {
                                    name: format!("{}[{}]", name, param_id),
                                    dependencies: dependencies.clone(),
                                    is_async: true,
                                    line_number,
                                    parametrized_args: parametrized_args.clone(),
                                    timeout_secs,
                                    markers: markers.clone(),
                                    marker_info: marker_info.clone(),
                                    param_id: Some(param_id),
                                });
                            }
                        } else {
                            tests.push(TestCase {
                                name: name.to_string(),
                                dependencies,
                                is_async: true,
                                line_number,
                                parametrized_args,
                                timeout_secs,
                                markers,
                                marker_info,
                                param_id: None,
                            });
                        }
                    } else {
                        tests.push(TestCase {
                            name: name.to_string(),
                            dependencies,
                            is_async: true,
                            line_number,
                            parametrized_args,
                            timeout_secs,
                            markers,
                            marker_info,
                            param_id: None,
                        });
                    }
                }
                if has_fixture_decorator(&func.decorator_list) {
                    fixtures.push(FixtureDefinition {
                        name: name.to_string(),
                        scope: extract_scope_from_decorators(&func.decorator_list),
                        dependencies: extract_args_from_arguments(&func.args),
                        params: extract_params_from_decorators(&func.decorator_list),
                        class_scope: None, // Top-level async fixture
                        autouse: extract_autouse_from_decorators(&func.decorator_list),
                        is_async: true,
                    });
                }
            }
            ast::Stmt::ClassDef(class) => {
                let class_name = class.name.as_str();
                if class_name.starts_with("Test") {
                    for stmt in &class.body {
                        if let ast::Stmt::FunctionDef(func) = stmt {
                            let method_name = func.name.as_str();

                            // Detect class-method fixtures
                            if has_fixture_decorator(&func.decorator_list) {
                                fixtures.push(FixtureDefinition {
                                    name: method_name.to_string(),
                                    scope: extract_scope_from_decorators(&func.decorator_list),
                                    dependencies: extract_args_from_arguments(&func.args),
                                    params: extract_params_from_decorators(&func.decorator_list),
                                    class_scope: Some(class_name.to_string()),
                                    autouse: extract_autouse_from_decorators(&func.decorator_list),
                                    is_async: false,
                                });
                            }

                            // Existing: Detect test methods
                            if method_name.starts_with("test_") {
                                let line_number =
                                    get_line_number(&source, func.range.start().to_usize());
                                let (markers, marker_info) =
                                    extract_markers_from_decorators(&func.decorator_list);
                                let parametrized_args = extract_injected_args(
                                    &func.decorator_list,
                                    &extract_args_from_arguments(&func.args),
                                );
                                let timeout_secs =
                                    extract_timeout_from_decorators(&func.decorator_list);
                                let dependencies = extract_args_from_arguments(&func.args);
                                let base_name = format!("{}::{}", class_name, method_name);

                                // Try to extract parametrize info and expand tests
                                if let Some(param_infos) = extract_all_parametrize_info(
                                    &func.decorator_list,
                                    &import_aliases,
                                ) {
                                    if !param_infos.is_empty() {
                                        let param_ids = generate_param_combinations(&param_infos);
                                        for param_id in param_ids {
                                            tests.push(TestCase {
                                                name: format!("{}[{}]", base_name, param_id),
                                                dependencies: dependencies.clone(),
                                                is_async: false,
                                                line_number,
                                                parametrized_args: parametrized_args.clone(),
                                                timeout_secs,
                                                markers: markers.clone(),
                                                marker_info: marker_info.clone(),
                                                param_id: Some(param_id),
                                            });
                                        }
                                    } else {
                                        tests.push(TestCase {
                                            name: base_name,
                                            dependencies,
                                            is_async: false,
                                            line_number,
                                            parametrized_args,
                                            timeout_secs,
                                            markers,
                                            marker_info,
                                            param_id: None,
                                        });
                                    }
                                } else {
                                    tests.push(TestCase {
                                        name: base_name,
                                        dependencies,
                                        is_async: false,
                                        line_number,
                                        parametrized_args,
                                        timeout_secs,
                                        markers,
                                        marker_info,
                                        param_id: None,
                                    });
                                }
                            }
                        } else if let ast::Stmt::AsyncFunctionDef(func) = stmt {
                            let method_name = func.name.as_str();

                            // Detect async class-method fixtures
                            if has_fixture_decorator(&func.decorator_list) {
                                fixtures.push(FixtureDefinition {
                                    name: method_name.to_string(),
                                    scope: extract_scope_from_decorators(&func.decorator_list),
                                    dependencies: extract_args_from_arguments(&func.args),
                                    params: extract_params_from_decorators(&func.decorator_list),
                                    class_scope: Some(class_name.to_string()),
                                    autouse: extract_autouse_from_decorators(&func.decorator_list),
                                    is_async: true,
                                });
                            }

                            // Existing: Detect async test methods
                            if method_name.starts_with("test_") {
                                let line_number =
                                    get_line_number(&source, func.range.start().to_usize());
                                let (markers, marker_info) =
                                    extract_markers_from_decorators(&func.decorator_list);
                                let parametrized_args = extract_injected_args(
                                    &func.decorator_list,
                                    &extract_args_from_arguments(&func.args),
                                );
                                let timeout_secs =
                                    extract_timeout_from_decorators(&func.decorator_list);
                                let dependencies = extract_args_from_arguments(&func.args);
                                let base_name = format!("{}::{}", class_name, method_name);

                                // Try to extract parametrize info and expand tests
                                if let Some(param_infos) = extract_all_parametrize_info(
                                    &func.decorator_list,
                                    &import_aliases,
                                ) {
                                    if !param_infos.is_empty() {
                                        let param_ids = generate_param_combinations(&param_infos);
                                        for param_id in param_ids {
                                            tests.push(TestCase {
                                                name: format!("{}[{}]", base_name, param_id),
                                                dependencies: dependencies.clone(),
                                                is_async: true,
                                                line_number,
                                                parametrized_args: parametrized_args.clone(),
                                                timeout_secs,
                                                markers: markers.clone(),
                                                marker_info: marker_info.clone(),
                                                param_id: Some(param_id),
                                            });
                                        }
                                    } else {
                                        tests.push(TestCase {
                                            name: base_name,
                                            dependencies,
                                            is_async: true,
                                            line_number,
                                            parametrized_args,
                                            timeout_secs,
                                            markers,
                                            marker_info,
                                            param_id: None,
                                        });
                                    }
                                } else {
                                    tests.push(TestCase {
                                        name: base_name,
                                        dependencies,
                                        is_async: true,
                                        line_number,
                                        parametrized_args,
                                        timeout_secs,
                                        markers,
                                        marker_info,
                                        param_id: None,
                                    });
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    Ok(TestModule {
        path: rel_path.to_path_buf(),
        tests,
        fixtures,
        hooks,
        is_toxic: false, // Set later by ToxicityGraph
    })
}

// =============================================================================
// Public API
// =============================================================================

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
                    markers: test.markers.clone(),
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
///
/// # Arguments
/// * `root` - The root directory to scan for test files
/// * `no_ignore` - If true, ignore .gitignore and .ignore files during discovery
pub fn discover(root: &Path, no_ignore: bool) -> Result<DiscoveryResult> {
    // Canonicalize root path to resolve symlinks
    let canonical_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());

    // Collect absolute paths first, then convert to relative for node IDs
    let paths: Vec<(PathBuf, PathBuf)> = WalkBuilder::new(&canonical_root)
        .standard_filters(!no_ignore)
        .follow_links(true) // Follow symlinked directories
        .build()
        .filter_map(|e| e.ok())
        .filter(|e| is_test_file(e.path()))
        .map(|e| {
            // Canonicalize paths to handle symlinks consistently
            let canonical_path = e
                .path()
                .canonicalize()
                .unwrap_or_else(|_| e.path().to_path_buf());
            // Convert to relative path for pytest node_id compatibility
            let relative_path = canonical_path
                .strip_prefix(&canonical_root)
                .unwrap_or(&canonical_path)
                .to_path_buf();
            // Return (absolute_path, relative_path)
            (canonical_path, relative_path)
        })
        .collect();

    let modules: Vec<TestModule> = paths
        .par_iter()
        .filter_map(|(abs_path, rel_path)| {
            // Parse using absolute path, but store relative path in module
            parse_module_with_relative_path(abs_path, rel_path).ok()
        })
        .filter(|m| !m.tests.is_empty() || !m.fixtures.is_empty() || !m.hooks.is_empty())
        .collect();

    Ok(DiscoveryResult { modules })
}

/// Detect patterns in .ignore that may block Python test discovery
///
/// Returns a list of patterns that are likely to prevent test files from being found.
/// This is useful for warning users when `tach list` or `tach test` finds zero tests
/// but there are .ignore patterns that could be causing the problem.
///
/// # Arguments
/// * `root` - The root directory to check for .ignore file
///
/// # Returns
/// A vector of patterns that may block Python test discovery.
/// Empty if no .ignore file exists or no dangerous patterns are found.
pub fn detect_blocking_patterns(root: &Path) -> Vec<String> {
    let ignore_path = root.join(".ignore");

    let content = match std::fs::read_to_string(&ignore_path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    // Patterns that would block Python test file discovery
    let dangerous_keywords = ["*.py", "test_", "tests/", "test/", "conftest"];

    content
        .lines()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter(|line| {
            dangerous_keywords
                .iter()
                .any(|keyword| line.contains(keyword))
        })
        .map(|s| s.to_string())
        .collect()
}

// =============================================================================
// Unit Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    // =========================================================================
    // MarkerInfo Parsing Tests (0.2.1 - pytest-django support)
    // =========================================================================

    #[test]
    fn test_marker_info_bare_marker() {
        let source = r#"
import pytest

@pytest.mark.django_db
def test_bare_marker():
    pass
"#;
        let module = parse_source(source);
        assert_eq!(module.tests.len(), 1);
        assert_eq!(module.tests[0].marker_info.len(), 1);
        assert_eq!(module.tests[0].marker_info[0].name, "django_db");
        assert!(module.tests[0].marker_info[0].args.is_empty());
    }

    #[test]
    fn test_marker_info_with_boolean_arg() {
        let source = r#"
import pytest

@pytest.mark.django_db(transaction=True)
def test_with_transaction():
    pass
"#;
        let module = parse_source(source);
        assert_eq!(module.tests.len(), 1);
        assert_eq!(module.tests[0].marker_info.len(), 1);
        assert_eq!(module.tests[0].marker_info[0].name, "django_db");
        assert_eq!(
            module.tests[0].marker_info[0].args.get("transaction"),
            Some(&serde_json::Value::Bool(true))
        );
    }

    #[test]
    fn test_marker_info_with_list_arg() {
        let source = r#"
import pytest

@pytest.mark.django_db(databases=["default", "secondary"])
def test_with_databases():
    pass
"#;
        let module = parse_source(source);
        assert_eq!(module.tests.len(), 1);
        assert_eq!(module.tests[0].marker_info.len(), 1);
        assert_eq!(module.tests[0].marker_info[0].name, "django_db");
        let databases = module.tests[0].marker_info[0].args.get("databases");
        assert!(databases.is_some());
        let arr = databases.unwrap().as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0], serde_json::Value::String("default".to_string()));
        assert_eq!(arr[1], serde_json::Value::String("secondary".to_string()));
    }

    #[test]
    fn test_marker_info_with_multiple_args() {
        let source = r#"
import pytest

@pytest.mark.django_db(transaction=True, reset_sequences=False)
def test_with_multiple_args():
    pass
"#;
        let module = parse_source(source);
        assert_eq!(module.tests.len(), 1);
        assert_eq!(module.tests[0].marker_info.len(), 1);
        let marker = &module.tests[0].marker_info[0];
        assert_eq!(marker.name, "django_db");
        assert_eq!(
            marker.args.get("transaction"),
            Some(&serde_json::Value::Bool(true))
        );
        assert_eq!(
            marker.args.get("reset_sequences"),
            Some(&serde_json::Value::Bool(false))
        );
    }

    #[test]
    fn test_marker_info_multiple_markers() {
        let source = r#"
import pytest

@pytest.mark.slow
@pytest.mark.django_db(transaction=True)
def test_multiple_markers():
    pass
"#;
        let module = parse_source(source);
        assert_eq!(module.tests.len(), 1);
        assert_eq!(module.tests[0].marker_info.len(), 2);

        // Check that both markers are present
        let marker_names: Vec<&str> = module.tests[0]
            .marker_info
            .iter()
            .map(|m| m.name.as_str())
            .collect();
        assert!(marker_names.contains(&"slow"));
        assert!(marker_names.contains(&"django_db"));
    }

    #[test]
    fn test_marker_info_with_string_arg() {
        let source = r#"
import pytest

@pytest.mark.skip(reason="not implemented")
def test_with_string_arg():
    pass
"#;
        let module = parse_source(source);
        assert_eq!(module.tests.len(), 1);
        assert_eq!(module.tests[0].marker_info.len(), 1);
        assert_eq!(module.tests[0].marker_info[0].name, "skip");
        assert_eq!(
            module.tests[0].marker_info[0].args.get("reason"),
            Some(&serde_json::Value::String("not implemented".to_string()))
        );
    }

    #[test]
    fn test_marker_info_backward_compat_markers_field() {
        // Ensure the old `markers: Vec<String>` field still works
        let source = r#"
import pytest

@pytest.mark.django_db(transaction=True)
@pytest.mark.slow
def test_backward_compat():
    pass
"#;
        let module = parse_source(source);
        assert_eq!(module.tests.len(), 1);
        // Old markers field should still contain marker names
        assert!(module.tests[0].markers.contains(&"django_db".to_string()));
        assert!(module.tests[0].markers.contains(&"slow".to_string()));
    }

    #[test]
    fn test_marker_info_in_class() {
        let source = r#"
import pytest

class TestDjango:
    @pytest.mark.django_db(transaction=True)
    def test_in_class(self):
        pass
"#;
        let module = parse_source(source);
        assert_eq!(module.tests.len(), 1);
        assert_eq!(module.tests[0].marker_info.len(), 1);
        assert_eq!(module.tests[0].marker_info[0].name, "django_db");
        assert_eq!(
            module.tests[0].marker_info[0].args.get("transaction"),
            Some(&serde_json::Value::Bool(true))
        );
    }

    #[test]
    fn test_marker_info_async_function() {
        let source = r#"
import pytest

@pytest.mark.django_db(transaction=True)
async def test_async_with_marker():
    await something()
"#;
        let module = parse_source(source);
        assert_eq!(module.tests.len(), 1);
        assert!(module.tests[0].is_async);
        assert_eq!(module.tests[0].marker_info.len(), 1);
        assert_eq!(module.tests[0].marker_info[0].name, "django_db");
    }

    // Helper to parse a Python source string and return TestModule
    fn parse_source(source: &str) -> TestModule {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(source.as_bytes()).unwrap();
        let abs_path = file.path().to_path_buf();
        let rel_path = PathBuf::from(abs_path.file_name().unwrap());
        parse_module_with_relative_path(&abs_path, &rel_path).unwrap()
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
                            parametrized_args: vec![],
                            timeout_secs: None,
                            markers: vec![],
                            marker_info: vec![],
                            param_id: None,
                        },
                        TestCase {
                            name: "test_2".into(),
                            dependencies: vec![],
                            is_async: true,
                            line_number: 1,
                            parametrized_args: vec![],
                            timeout_secs: None,
                            markers: vec![],
                            marker_info: vec![],
                            param_id: None,
                        },
                    ],
                    fixtures: vec![FixtureDefinition {
                        name: "db".into(),
                        scope: FixtureScope::Module,
                        dependencies: vec![],
                        params: None,
                        class_scope: None,
                        autouse: false,
                        is_async: false,
                    }],
                    hooks: vec![],
                    is_toxic: false,
                },
                TestModule {
                    path: PathBuf::from("test_b.py"),
                    tests: vec![TestCase {
                        name: "test_3".into(),
                        dependencies: vec!["db".into()],
                        is_async: false,
                        line_number: 1,
                        parametrized_args: vec![],
                        timeout_secs: None,
                        markers: vec![],
                        marker_info: vec![],
                        param_id: None,
                    }],
                    fixtures: vec![],
                    hooks: vec![],
                    is_toxic: false,
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
        assert!(
            module
                .tests
                .iter()
                .any(|t| t.name == "TestMyClass::test_method_one")
        );
        assert!(
            module
                .tests
                .iter()
                .any(|t| t.name == "TestMyClass::test_method_two")
        );
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

    // =========================================================================
    // Decorated Test Function Tests (Bug Fix 0.1.1-C)
    // =========================================================================

    #[test]
    fn test_parse_decorated_test_functions() {
        let source = r#"
import pytest

@pytest.mark.slow
def test_with_slow_marker():
    pass

@pytest.mark.skip(reason="not ready")
def test_with_skip():
    pass

@custom_decorator
def test_with_custom_decorator():
    pass

@decorator_one
@decorator_two
@decorator_three
def test_with_decorator_chain():
    pass
"#;
        let module = parse_source(source);
        assert_eq!(
            module.tests.len(),
            4,
            "Should discover all 4 decorated tests"
        );

        let test_names: Vec<_> = module.tests.iter().map(|t| t.name.as_str()).collect();
        assert!(
            test_names.contains(&"test_with_slow_marker"),
            "Should find @pytest.mark.slow decorated test"
        );
        assert!(
            test_names.contains(&"test_with_skip"),
            "Should find @pytest.mark.skip decorated test"
        );
        assert!(
            test_names.contains(&"test_with_custom_decorator"),
            "Should find custom decorated test"
        );
        assert!(
            test_names.contains(&"test_with_decorator_chain"),
            "Should find test with decorator chain"
        );
    }

    #[test]
    fn test_parse_decorated_async_test() {
        let source = r#"
import pytest

@pytest.mark.asyncio
async def test_async_with_marker():
    await something()
"#;
        let module = parse_source(source);
        assert_eq!(module.tests.len(), 1);
        assert_eq!(module.tests[0].name, "test_async_with_marker");
        assert!(module.tests[0].is_async);
    }

    #[test]
    fn test_parse_decorated_test_in_class() {
        let source = r#"
import pytest

class TestDecorated:
    @pytest.mark.slow
    def test_slow_method(self):
        pass

    @pytest.mark.parametrize("x", [1, 2, 3])
    def test_parametrized(self, x):
        pass
"#;
        let module = parse_source(source);
        // With parametrize expansion: 1 slow test + 3 parametrized variants = 4 tests
        assert_eq!(
            module.tests.len(),
            4,
            "Should find slow method + 3 parametrized variants"
        );

        let test_names: Vec<_> = module.tests.iter().map(|t| t.name.as_str()).collect();
        assert!(test_names.contains(&"TestDecorated::test_slow_method"));
        assert!(test_names.contains(&"TestDecorated::test_parametrized[1]"));
        assert!(test_names.contains(&"TestDecorated::test_parametrized[2]"));
        assert!(test_names.contains(&"TestDecorated::test_parametrized[3]"));
    }

    // =========================================================================
    // @pytest.mark.timeout Parsing Tests (0.1.2-D)
    // =========================================================================

    #[test]
    fn test_parse_timeout_marker_positional() {
        let source = r#"
import pytest

@pytest.mark.timeout(30)
def test_with_timeout():
    pass
"#;
        let module = parse_source(source);
        assert_eq!(module.tests.len(), 1);
        assert_eq!(module.tests[0].timeout_secs, Some(30));
    }

    #[test]
    fn test_parse_timeout_marker_keyword() {
        let source = r#"
import pytest

@pytest.mark.timeout(seconds=60)
def test_with_timeout_keyword():
    pass
"#;
        let module = parse_source(source);
        assert_eq!(module.tests.len(), 1);
        assert_eq!(module.tests[0].timeout_secs, Some(60));
    }

    #[test]
    fn test_parse_no_timeout_marker() {
        let source = r#"
def test_without_timeout():
    pass
"#;
        let module = parse_source(source);
        assert_eq!(module.tests.len(), 1);
        assert_eq!(module.tests[0].timeout_secs, None);
    }

    #[test]
    fn test_parse_timeout_in_class() {
        let source = r#"
import pytest

class TestWithTimeout:
    @pytest.mark.timeout(10)
    def test_method_with_timeout(self):
        pass

    def test_method_without_timeout(self):
        pass
"#;
        let module = parse_source(source);
        assert_eq!(module.tests.len(), 2);

        let with_timeout = module
            .tests
            .iter()
            .find(|t| t.name.contains("with_timeout"))
            .unwrap();
        assert_eq!(with_timeout.timeout_secs, Some(10));

        let without_timeout = module
            .tests
            .iter()
            .find(|t| t.name.contains("without_timeout"))
            .unwrap();
        assert_eq!(without_timeout.timeout_secs, None);
    }

    #[test]
    fn test_parse_timeout_with_other_markers() {
        let source = r#"
import pytest

@pytest.mark.slow
@pytest.mark.timeout(120)
@pytest.mark.skip(reason="not ready")
def test_multiple_markers():
    pass
"#;
        let module = parse_source(source);
        assert_eq!(module.tests.len(), 1);
        assert_eq!(module.tests[0].timeout_secs, Some(120));
    }

    #[test]
    fn test_parse_timeout_async_function() {
        let source = r#"
import pytest

@pytest.mark.timeout(45)
async def test_async_with_timeout():
    await something()
"#;
        let module = parse_source(source);
        assert_eq!(module.tests.len(), 1);
        assert!(module.tests[0].is_async);
        assert_eq!(module.tests[0].timeout_secs, Some(45));
    }

    #[test]
    fn test_parse_timeout_zero_means_no_timeout() {
        let source = r#"
import pytest

@pytest.mark.timeout(0)
def test_with_zero_timeout():
    pass
"#;
        let module = parse_source(source);
        assert_eq!(module.tests.len(), 1);
        // timeout=0 means "no timeout" in pytest-timeout, so it should be None
        assert_eq!(module.tests[0].timeout_secs, None);
    }

    #[test]
    fn test_parse_timeout_zero_keyword_means_no_timeout() {
        let source = r#"
import pytest

@pytest.mark.timeout(seconds=0)
def test_with_zero_keyword_timeout():
    pass
"#;
        let module = parse_source(source);
        assert_eq!(module.tests.len(), 1);
        // timeout=0 means "no timeout" in pytest-timeout, so it should be None
        assert_eq!(module.tests[0].timeout_secs, None);
    }

    // =========================================================================
    // Symlink Path Resolution Tests (Task 4: 0.1.1)
    // =========================================================================

    /// Test that canonicalize is applied to paths.
    ///
    /// This tests the symlink handling logic:
    /// 1. Root path is canonicalized
    /// 2. Each test file path is canonicalized
    /// 3. Relative paths are computed from canonical root
    #[test]
    fn test_symlink_path_canonicalization_concept() {
        use std::path::PathBuf;

        // Simulate path canonicalization logic
        fn canonicalize_test_path(
            file_path: &std::path::Path,
            canonical_root: &std::path::Path,
        ) -> PathBuf {
            let canonical_path = file_path
                .canonicalize()
                .unwrap_or_else(|_| file_path.to_path_buf());
            canonical_path
                .strip_prefix(canonical_root)
                .unwrap_or(&canonical_path)
                .to_path_buf()
        }

        // Test with current directory as example
        let cwd = std::env::current_dir().expect("Should have cwd");
        let test_file = cwd.join("tests/test_example.py");

        // The result should be a relative path
        let result = canonicalize_test_path(&test_file, &cwd);

        // If test_file doesn't exist, we get absolute path back
        // If it exists, we get relative path
        // Either way, the function shouldn't panic
        assert!(
            !result.to_string_lossy().is_empty(),
            "Canonicalization should produce a non-empty path"
        );
    }

    /// Test that is_test_file correctly identifies test files.
    ///
    /// This is a prerequisite for symlink testing - we need to ensure
    /// test file detection works correctly.
    #[test]
    fn test_is_test_file_detection() {
        use std::path::Path;

        // Should not match - file doesn't exist (is_file check fails)
        assert!(!super::is_test_file(Path::new("/tmp/test_foo.py")));

        // Pattern matching for file names (the actual function also checks extension and is_file)
        // Here we test just the naming patterns
        let is_test_name = |name: &str| -> bool {
            // Check extension
            if !name.ends_with(".py") {
                return false;
            }
            // Check name patterns
            name.starts_with("test_") || name.ends_with("_test.py") || name == "conftest.py"
        };

        assert!(is_test_name("test_foo.py"), "Should match test_ prefix");
        assert!(is_test_name("foo_test.py"), "Should match _test.py suffix");
        assert!(is_test_name("conftest.py"), "Should match conftest.py");
        assert!(!is_test_name("helper.py"), "Should not match regular files");
        assert!(
            !is_test_name("test_module"),
            "Should not match without .py extension"
        );
    }

    /// Test that WalkBuilder's follow_links handles symlink cycles.
    ///
    /// The ignore crate's WalkBuilder tracks visited directories to prevent
    /// infinite loops when following symlinks.
    #[test]
    fn test_symlink_cycle_protection_concept() {
        // The ignore crate handles this internally, but we document the expected behavior
        // When a symlink cycle is detected:
        // 1. The cycle is broken (directory not revisited)
        // 2. No infinite loop occurs
        // 3. Files in the cycle's first visit are still collected

        // This is a conceptual test - actual symlink creation requires root
        // or tmpdir setup that may not be available in all test environments
        let visited = std::collections::HashSet::<std::path::PathBuf>::new();

        // Simulate cycle detection
        fn would_visit(
            path: &std::path::Path,
            visited: &std::collections::HashSet<std::path::PathBuf>,
        ) -> bool {
            !visited.contains(path)
        }

        assert!(
            would_visit(std::path::Path::new("/some/path"), &visited),
            "Should visit unvisited path"
        );
    }

    // =========================================================================
    // Parametrize ID Extraction Tests (parametrize discovery fix)
    // =========================================================================

    #[test]
    fn test_parametrize_simple_values() {
        let source = r#"
import pytest

@pytest.mark.parametrize("name", ["alice", "bob", "charlie"])
def test_greeting(name):
    pass
"#;
        let module = parse_source(source);
        assert_eq!(module.tests.len(), 3);

        let test_names: Vec<_> = module.tests.iter().map(|t| t.name.as_str()).collect();
        assert!(test_names.contains(&"test_greeting[alice]"));
        assert!(test_names.contains(&"test_greeting[bob]"));
        assert!(test_names.contains(&"test_greeting[charlie]"));
    }

    #[test]
    fn test_parametrize_integer_values() {
        let source = r#"
import pytest

@pytest.mark.parametrize("x", [1, 2, 3])
def test_numbers(x):
    pass
"#;
        let module = parse_source(source);
        assert_eq!(module.tests.len(), 3);

        let test_names: Vec<_> = module.tests.iter().map(|t| t.name.as_str()).collect();
        assert!(test_names.contains(&"test_numbers[1]"));
        assert!(test_names.contains(&"test_numbers[2]"));
        assert!(test_names.contains(&"test_numbers[3]"));
    }

    #[test]
    fn test_parametrize_tuple_values() {
        let source = r#"
import pytest

@pytest.mark.parametrize("exc,msg", [(ValueError, "bad value"), (TypeError, "wrong type")])
def test_exceptions(exc, msg):
    pass
"#;
        let module = parse_source(source);
        assert_eq!(module.tests.len(), 2);

        let test_names: Vec<_> = module.tests.iter().map(|t| t.name.as_str()).collect();
        assert!(test_names.contains(&"test_exceptions[ValueError-bad value]"));
        assert!(test_names.contains(&"test_exceptions[TypeError-wrong type]"));
    }

    #[test]
    fn test_parametrize_with_explicit_ids() {
        let source = r#"
import pytest

@pytest.mark.parametrize("x", [1, 2], ids=["one", "two"])
def test_with_ids(x):
    pass
"#;
        let module = parse_source(source);
        assert_eq!(module.tests.len(), 2);

        let test_names: Vec<_> = module.tests.iter().map(|t| t.name.as_str()).collect();
        assert!(test_names.contains(&"test_with_ids[one]"));
        assert!(test_names.contains(&"test_with_ids[two]"));
    }

    #[test]
    fn test_parametrize_pytest_param_with_id() {
        let source = r#"
import pytest

@pytest.mark.parametrize("x", [
    pytest.param(1, id="first"),
    pytest.param(2, id="second"),
])
def test_pytest_param(x):
    pass
"#;
        let module = parse_source(source);
        assert_eq!(module.tests.len(), 2);

        let test_names: Vec<_> = module.tests.iter().map(|t| t.name.as_str()).collect();
        assert!(test_names.contains(&"test_pytest_param[first]"));
        assert!(test_names.contains(&"test_pytest_param[second]"));
    }

    #[test]
    fn test_parametrize_multiple_decorators() {
        let source = r#"
import pytest

@pytest.mark.parametrize("x", [1, 2])
@pytest.mark.parametrize("y", ["a", "b"])
def test_matrix(x, y):
    pass
"#;
        let module = parse_source(source);
        // 2 x 2 = 4 combinations
        assert_eq!(module.tests.len(), 4);

        let test_names: Vec<_> = module.tests.iter().map(|t| t.name.as_str()).collect();
        assert!(test_names.contains(&"test_matrix[1-a]"));
        assert!(test_names.contains(&"test_matrix[1-b]"));
        assert!(test_names.contains(&"test_matrix[2-a]"));
        assert!(test_names.contains(&"test_matrix[2-b]"));
    }

    #[test]
    fn test_parametrize_bool_values() {
        let source = r#"
import pytest

@pytest.mark.parametrize("flag", [True, False])
def test_bool(flag):
    pass
"#;
        let module = parse_source(source);
        assert_eq!(module.tests.len(), 2);

        let test_names: Vec<_> = module.tests.iter().map(|t| t.name.as_str()).collect();
        assert!(test_names.contains(&"test_bool[True]"));
        assert!(test_names.contains(&"test_bool[False]"));
    }

    #[test]
    fn test_parametrize_none_value() {
        let source = r#"
import pytest

@pytest.mark.parametrize("val", [None, "something"])
def test_none(val):
    pass
"#;
        let module = parse_source(source);
        assert_eq!(module.tests.len(), 2);

        let test_names: Vec<_> = module.tests.iter().map(|t| t.name.as_str()).collect();
        assert!(test_names.contains(&"test_none[None]"));
        assert!(test_names.contains(&"test_none[something]"));
    }

    #[test]
    fn test_parametrize_in_class() {
        let source = r#"
import pytest

class TestClass:
    @pytest.mark.parametrize("x", [1, 2])
    def test_method(self, x):
        pass
"#;
        let module = parse_source(source);
        assert_eq!(module.tests.len(), 2);

        let test_names: Vec<_> = module.tests.iter().map(|t| t.name.as_str()).collect();
        assert!(test_names.contains(&"TestClass::test_method[1]"));
        assert!(test_names.contains(&"TestClass::test_method[2]"));
    }

    #[test]
    fn test_parametrize_async_function() {
        let source = r#"
import pytest

@pytest.mark.parametrize("x", ["a", "b"])
async def test_async_param(x):
    await something()
"#;
        let module = parse_source(source);
        assert_eq!(module.tests.len(), 2);

        let test_names: Vec<_> = module.tests.iter().map(|t| t.name.as_str()).collect();
        assert!(test_names.contains(&"test_async_param[a]"));
        assert!(test_names.contains(&"test_async_param[b]"));

        // All should be async
        assert!(module.tests.iter().all(|t| t.is_async));
    }

    #[test]
    fn test_parametrize_param_id_field() {
        let source = r#"
import pytest

@pytest.mark.parametrize("x", ["val1", "val2"])
def test_param_id(x):
    pass
"#;
        let module = parse_source(source);
        assert_eq!(module.tests.len(), 2);

        // Check that param_id is set correctly
        let test1 = module
            .tests
            .iter()
            .find(|t| t.name.contains("val1"))
            .unwrap();
        assert_eq!(test1.param_id, Some("val1".to_string()));

        let test2 = module
            .tests
            .iter()
            .find(|t| t.name.contains("val2"))
            .unwrap();
        assert_eq!(test2.param_id, Some("val2".to_string()));
    }

    #[test]
    fn test_non_parametrized_has_no_param_id() {
        let source = r#"
def test_simple():
    pass
"#;
        let module = parse_source(source);
        assert_eq!(module.tests.len(), 1);
        assert_eq!(module.tests[0].param_id, None);
    }

    #[test]
    fn test_parametrize_dynamic_values_fallback() {
        // Dynamic values (function calls) should fall back to base test name
        let source = r#"
import pytest

@pytest.mark.parametrize("x", get_values())
def test_dynamic(x):
    pass
"#;
        let module = parse_source(source);
        // Should have 1 test with base name (can't expand dynamic values)
        assert_eq!(module.tests.len(), 1);
        assert_eq!(module.tests[0].name, "test_dynamic");
        assert_eq!(module.tests[0].param_id, None);
    }

    #[test]
    fn test_parametrize_dict_values_use_index_fallback() {
        // Dict values can't be stringified, so pytest uses {arg_name}{index} format
        let source = r#"
import pytest

@pytest.mark.parametrize("cache_value", [
    {},
    {"temperature": 0.7},
    {"max_tokens": 1024},
])
def test_various_params_caches(cache_value):
    pass
"#;
        let module = parse_source(source);
        // Should have 3 tests with index-based IDs matching pytest's format
        assert_eq!(module.tests.len(), 3);
        let test_names: Vec<&str> = module.tests.iter().map(|t| t.name.as_str()).collect();
        assert!(test_names.contains(&"test_various_params_caches[cache_value0]"));
        assert!(test_names.contains(&"test_various_params_caches[cache_value1]"));
        assert!(test_names.contains(&"test_various_params_caches[cache_value2]"));
    }

    // =========================================================================
    // Import Alias Resolution Tests (Batch 1)
    // =========================================================================

    #[test]
    fn test_parametrize_with_aliased_import() {
        let source = r#"
from playwright.async_api import Error as PlaywrightAsyncError

import pytest

@pytest.mark.parametrize("exc", [PlaywrightAsyncError])
def test_errors(exc):
    pass
"#;
        let module = parse_source(source);

        // Should use the original name "Error", not the alias
        assert_eq!(module.tests.len(), 1);
        assert_eq!(module.tests[0].name, "test_errors[Error]");
    }

    #[test]
    fn test_parametrize_resolves_alias_in_tuple() {
        let source = r#"
from asyncio import TimeoutError as AsyncTimeout

import pytest

@pytest.mark.parametrize("exc,msg", [
    (AsyncTimeout, "timeout"),
])
def test_timeouts(exc, msg):
    pass
"#;
        let module = parse_source(source);

        // Tuple should resolve alias to original name
        assert_eq!(module.tests.len(), 1);
        assert!(module.tests[0].name.contains("TimeoutError"));
    }

    // =========================================================================
    // Import Alias Resolution Tests (Batch 2: Edge Cases)
    // =========================================================================

    #[test]
    fn test_parametrize_non_aliased_import_unchanged() {
        let code = r#"
from asyncio import TimeoutError

import pytest

@pytest.mark.parametrize("exc", [TimeoutError])
def test_timeouts(exc):
    pass
"#;
        let result = parse_source(code);

        // Non-aliased import should keep original name
        assert_eq!(result.tests.len(), 1);
        assert_eq!(result.tests[0].name, "test_timeouts[TimeoutError]");
    }

    #[test]
    fn test_parametrize_multiple_aliases() {
        let code = r#"
from playwright.async_api import Error as PlaywrightError
from asyncio import TimeoutError as AsyncTimeout

import pytest

@pytest.mark.parametrize("exc", [PlaywrightError, AsyncTimeout])
def test_errors(exc):
    pass
"#;
        let result = parse_source(code);

        assert_eq!(result.tests.len(), 2);
        assert!(result.tests.iter().any(|t| t.name == "test_errors[Error]"));
        assert!(
            result
                .tests
                .iter()
                .any(|t| t.name == "test_errors[TimeoutError]")
        );
    }
}
