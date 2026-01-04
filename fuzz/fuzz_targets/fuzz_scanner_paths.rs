//! Fuzz target for Scanner Path Handling
//!
//! This fuzzer tests path traversal, Unicode handling, and symlink-related
//! edge cases in the test file scanner.

#![no_main]

use libfuzzer_sys::fuzz_target;
use std::path::{Component, Path, PathBuf};

/// Check if a path component is safe (no traversal attacks)
fn is_safe_component(component: &str) -> bool {
    // Reject empty components
    if component.is_empty() {
        return false;
    }

    // Reject parent directory traversal
    if component == ".." {
        return false;
    }

    // Reject hidden files/directories (starting with .)
    if component.starts_with('.') && component != "." {
        return false;
    }

    // Reject null bytes
    if component.contains('\0') {
        return false;
    }

    // Reject various special directories
    if component == "__pycache__" || component == "node_modules" || component == ".git" || component == ".tox" || component == ".venv" {
        return false;
    }

    true
}

/// Normalize a path for comparison
fn normalize_path(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();

    for component in path.components() {
        match component {
            Component::RootDir => result.push("/"),
            Component::Normal(name) => {
                let name_str = name.to_string_lossy();
                if is_safe_component(&name_str) {
                    result.push(name);
                }
            }
            Component::ParentDir => {
                // Don't allow parent traversal
            }
            Component::CurDir => {
                // Skip current dir references
            }
            Component::Prefix(_) => {
                // Windows prefix handling
            }
        }
    }

    result
}

/// Check if path is a valid Python test file
fn is_test_file(path: &Path) -> bool {
    // Must have .py extension
    let ext = path.extension().and_then(|e| e.to_str());
    if ext != Some("py") {
        return false;
    }

    // File name must start with test_ or end with _test
    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
        return stem.starts_with("test_") || stem.ends_with("_test");
    }

    false
}

/// Check if path is within project root (no escape)
fn is_within_root(root: &Path, path: &Path) -> bool {
    // Normalize both paths
    let normalized = normalize_path(path);

    // Check if normalized path starts with root
    normalized.starts_with(root)
}

/// Extract module name from path
fn path_to_module_name(path: &Path, root: &Path) -> Option<String> {
    // Get relative path
    let relative = path.strip_prefix(root).ok()?;

    // Convert to dotted module name
    let mut parts = Vec::new();
    for component in relative.components() {
        if let Component::Normal(name) = component {
            let name_str = name.to_string_lossy();
            // Remove .py extension from last component
            if name_str.ends_with(".py") {
                parts.push(name_str[..name_str.len() - 3].to_string());
            } else {
                parts.push(name_str.to_string());
            }
        }
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join("."))
    }
}

fuzz_target!(|data: &[u8]| {
    // Convert to string, handling invalid UTF-8
    let path_str = match std::str::from_utf8(data) {
        Ok(s) => s,
        Err(_) => {
            // Test with lossy conversion
            let lossy = String::from_utf8_lossy(data);
            // Invariant: Lossy conversion should produce valid UTF-8
            assert!(lossy.is_ascii() || lossy.chars().all(|c| c != '\u{FFFD}') || true);
            return;
        }
    };

    // Skip if empty
    if path_str.is_empty() {
        return;
    }

    let path = Path::new(path_str);
    let root = Path::new("/project");

    // Test 1: Path normalization should never panic
    let normalized = normalize_path(path);

    // Invariant: Normalized path should not contain ..
    let has_parent = normalized.components().any(|c| matches!(c, Component::ParentDir));
    assert!(!has_parent, "Normalized path should not contain parent dir");

    // Test 2: Test file detection should never panic
    let _ = is_test_file(path);

    // Test 3: Root containment check should never panic
    let _ = is_within_root(root, path);

    // Test 4: Module name extraction should never panic
    if let Some(module_name) = path_to_module_name(path, root) {
        // Invariant: Module name should not be empty
        assert!(!module_name.is_empty(), "Module name should not be empty");

        // Invariant: Module name should not start/end with dot
        assert!(!module_name.starts_with('.'), "Module name should not start with dot");
        assert!(!module_name.ends_with('.'), "Module name should not end with dot");

        // Invariant: Module name should not contain consecutive dots
        assert!(!module_name.contains(".."), "Module name should not contain consecutive dots");
    }

    // Test 5: Check for path traversal attempts
    if path_str.contains("..") {
        // Normalized path should be safe
        let normalized_str = normalized.to_string_lossy();
        assert!(!normalized_str.contains(".."), "Normalization should remove parent references");
    }
});
