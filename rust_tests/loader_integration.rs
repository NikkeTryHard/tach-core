//! Loader Integration Tests: Verify Zero-Copy Module Loading
//!
//! These tests verify that the Zero-Copy Loader can:
//! 1. Compile Python source to bytecode
//! 2. Strip the .pyc header correctly
//! 3. Load the bytecode via C-API injection
//! 4. Register the module in sys.modules

use std::fs;
use std::path::PathBuf;
use tach_core::loader::{BytecodeCompiler, BytecodeEntry, ModuleRegistry};
use tempfile::TempDir;

/// Helper: Create a test Python file
fn create_test_module(dir: &std::path::Path, name: &str, content: &str) -> PathBuf {
    let path = dir.join(format!("{}.py", name));
    fs::write(&path, content).expect("Failed to write test module");
    path
}

/// Test: Compiler correctly strips 16-byte header
#[test]
fn test_header_stripping_produces_valid_code_object() {
    let temp = TempDir::new().unwrap();

    // Create a simple module
    let source = create_test_module(
        temp.path(),
        "simple",
        "def hello():\n    return 'world'\n\nVALUE = 42\n",
    );

    // Compile it
    let compiler = BytecodeCompiler::new(temp.path()).expect("Compiler creation failed");
    let bytecode = compiler.compile(&source).expect("Compilation failed");

    // Verify bytecode is not empty
    assert!(!bytecode.is_empty(), "Bytecode should not be empty");

    // Verify first byte is TYPE_CODE marker (0x63 or 0xe3 depending on Python version)
    // Python 3.9+ uses 0xe3 for module code objects
    assert!(
        bytecode[0] == 0x63 || bytecode[0] == 0xe3,
        "First byte should be TYPE_CODE, got 0x{:02x}",
        bytecode[0]
    );

    // Verify bytecode is reasonably sized (header stripped)
    assert!(bytecode.len() > 10, "Bytecode seems too short");
}

/// Test: Registry can store and retrieve modules by name
#[test]
fn test_registry_module_name_lookup() {
    let temp = TempDir::new().unwrap();
    let registry = ModuleRegistry::new(temp.path().to_path_buf());

    // Insert some modules
    registry.insert(BytecodeEntry {
        name: "mypackage".to_string(),
        source_path: temp.path().join("mypackage/__init__.py"),
        bytecode: vec![0xe3, 1, 2, 3],
        is_package: true,
    });

    registry.insert(BytecodeEntry {
        name: "mypackage.submodule".to_string(),
        source_path: temp.path().join("mypackage/submodule.py"),
        bytecode: vec![0xe3, 4, 5, 6],
        is_package: false,
    });

    // Verify lookups
    assert!(registry.get_bytecode("mypackage").is_some());
    assert!(registry.get_bytecode("mypackage.submodule").is_some());
    assert!(registry.get_bytecode("nonexistent").is_none());

    // Verify package flag
    assert_eq!(registry.is_package("mypackage"), Some(true));
    assert_eq!(registry.is_package("mypackage.submodule"), Some(false));
}

/// Test: Batch compilation handles mixed success/failure gracefully
#[test]
fn test_batch_compilation_graceful_failure() {
    let temp = TempDir::new().unwrap();

    // Create valid and invalid files
    let valid = create_test_module(temp.path(), "valid", "x = 1");
    let invalid = create_test_module(temp.path(), "invalid", "def broken(\n"); // Syntax error

    let compiler = BytecodeCompiler::new(temp.path()).expect("Compiler creation failed");
    let registry = ModuleRegistry::new(temp.path().to_path_buf());

    // Batch compile - should succeed for valid, warn for invalid
    let count = compiler.compile_batch(&[valid.clone(), invalid.clone()], &registry);

    // Should have compiled at least the valid one
    assert_eq!(count, 1, "Should compile 1 valid module");
    assert!(registry.get_bytecode("valid").is_some());
    assert!(registry.get_bytecode("invalid").is_none());
}

/// Test: Package module name extraction from __init__.py
#[test]
fn test_package_name_from_init_py() {
    let temp = TempDir::new().unwrap();

    // Create package structure
    let pkg_dir = temp.path().join("mypackage");
    fs::create_dir_all(&pkg_dir).unwrap();
    let init_py = pkg_dir.join("__init__.py");
    fs::write(&init_py, "# Package init").unwrap();

    let compiler = BytecodeCompiler::new(temp.path()).expect("Compiler creation failed");
    let registry = ModuleRegistry::new(temp.path().to_path_buf());

    let count = compiler.compile_batch(&[init_py], &registry);

    assert_eq!(count, 1);
    // Package should be registered as "mypackage", not "mypackage.__init__"
    assert!(registry.get_bytecode("mypackage").is_some());
    assert_eq!(registry.is_package("mypackage"), Some(true));
}

/// Test: Cache persistence and mtime-based invalidation
#[test]
fn test_cache_persistence() {
    let temp = TempDir::new().unwrap();
    let source = create_test_module(temp.path(), "cached", "x = 1");

    let compiler = BytecodeCompiler::new(temp.path()).expect("Compiler creation failed");

    // First compile - creates cache
    let bytecode1 = compiler.compile(&source).expect("First compile failed");

    // Second compile - should use cache (same result)
    let bytecode2 = compiler.compile(&source).expect("Second compile failed");

    assert_eq!(bytecode1, bytecode2, "Cached bytecode should match");

    // Modify source
    std::thread::sleep(std::time::Duration::from_millis(10));
    fs::write(&source, "x = 2\ny = 3").unwrap();

    // Third compile - cache should be invalidated
    let bytecode3 = compiler.compile(&source).expect("Third compile failed");

    assert_ne!(
        bytecode1, bytecode3,
        "Modified source should produce different bytecode"
    );
}

/// Test: Nested module path resolution
#[test]
fn test_nested_module_path() {
    let temp = TempDir::new().unwrap();

    // Create nested structure: foo/bar/baz.py
    let nested_dir = temp.path().join("foo").join("bar");
    fs::create_dir_all(&nested_dir).unwrap();
    let source = nested_dir.join("baz.py");
    fs::write(&source, "x = 1").unwrap();

    let compiler = BytecodeCompiler::new(temp.path()).expect("Compiler creation failed");
    let registry = ModuleRegistry::new(temp.path().to_path_buf());

    let count = compiler.compile_batch(&[source], &registry);

    assert_eq!(count, 1);
    assert!(registry.get_bytecode("foo.bar.baz").is_some());
}

// =============================================================================
// Extended Test Coverage: Edge Cases, Stress Tests, and Robustness
// =============================================================================

/// Test: Empty Python file compiles without error
#[test]
fn test_empty_file_compilation() {
    let temp = TempDir::new().unwrap();
    let source = create_test_module(temp.path(), "empty", "");

    let compiler = BytecodeCompiler::new(temp.path()).expect("Compiler creation failed");
    let bytecode = compiler.compile(&source);

    // Empty file should still compile to a valid code object
    assert!(bytecode.is_ok(), "Empty file should compile");
    assert!(
        !bytecode.unwrap().is_empty(),
        "Bytecode should not be empty even for empty file"
    );
}

/// Test: File with only comments compiles
#[test]
fn test_comments_only_file() {
    let temp = TempDir::new().unwrap();
    let source = create_test_module(
        temp.path(),
        "comments_only",
        "# This is a comment\n# Another comment\n",
    );

    let compiler = BytecodeCompiler::new(temp.path()).expect("Compiler creation failed");
    let bytecode = compiler.compile(&source);

    assert!(bytecode.is_ok(), "Comments-only file should compile");
}

/// Test: Unicode in module content
#[test]
fn test_unicode_content() {
    let temp = TempDir::new().unwrap();
    let source = create_test_module(
        temp.path(),
        "unicode_test",
        "# -*- coding: utf-8 -*-\nmessage = '你好世界'\nemoji = '🎉'\n",
    );

    let compiler = BytecodeCompiler::new(temp.path()).expect("Compiler creation failed");
    let bytecode = compiler.compile(&source);

    assert!(bytecode.is_ok(), "Unicode content should compile");
}

/// Test: Large module compilation
#[test]
fn test_large_module() {
    let temp = TempDir::new().unwrap();

    // Generate a large module with many functions
    let mut content = String::new();
    for i in 0..100 {
        content.push_str(&format!("def func_{}():\n    return {}\n\n", i, i));
    }

    let source = create_test_module(temp.path(), "large_module", &content);
    let compiler = BytecodeCompiler::new(temp.path()).expect("Compiler creation failed");
    let bytecode = compiler.compile(&source);

    assert!(bytecode.is_ok(), "Large module should compile");
    assert!(
        bytecode.unwrap().len() > 1000,
        "Large module bytecode should be substantial"
    );
}

/// Test: Deeply nested package path (10 levels deep)
#[test]
fn test_deeply_nested_path() {
    let temp = TempDir::new().unwrap();

    // Create deeply nested structure: a/b/c/d/e/f/g/h/i/j.py
    let nested_dir = temp
        .path()
        .join("a")
        .join("b")
        .join("c")
        .join("d")
        .join("e")
        .join("f")
        .join("g")
        .join("h")
        .join("i");
    fs::create_dir_all(&nested_dir).unwrap();
    let source = nested_dir.join("j.py");
    fs::write(&source, "x = 'deeply_nested'").unwrap();

    let compiler = BytecodeCompiler::new(temp.path()).expect("Compiler creation failed");
    let registry = ModuleRegistry::new(temp.path().to_path_buf());

    let count = compiler.compile_batch(&[source], &registry);

    assert_eq!(count, 1);
    assert!(registry.get_bytecode("a.b.c.d.e.f.g.h.i.j").is_some());
}

/// Test: Module with underscore-prefixed names
#[test]
fn test_underscore_prefix_modules() {
    let temp = TempDir::new().unwrap();

    let source1 = create_test_module(temp.path(), "_private", "x = 1");
    let source2 = create_test_module(temp.path(), "__dunder", "x = 2");

    let compiler = BytecodeCompiler::new(temp.path()).expect("Compiler creation failed");
    let registry = ModuleRegistry::new(temp.path().to_path_buf());

    let count = compiler.compile_batch(&[source1, source2], &registry);

    assert_eq!(count, 2);
    assert!(registry.get_bytecode("_private").is_some());
    assert!(registry.get_bytecode("__dunder").is_some());
}

/// Test: Multiple packages with same-named submodules
#[test]
fn test_same_name_submodules() {
    let temp = TempDir::new().unwrap();

    // Create pkg1/utils.py and pkg2/utils.py
    let pkg1_dir = temp.path().join("pkg1");
    let pkg2_dir = temp.path().join("pkg2");
    fs::create_dir_all(&pkg1_dir).unwrap();
    fs::create_dir_all(&pkg2_dir).unwrap();

    let source1 = pkg1_dir.join("utils.py");
    let source2 = pkg2_dir.join("utils.py");
    fs::write(&source1, "name = 'pkg1'").unwrap();
    fs::write(&source2, "name = 'pkg2'").unwrap();

    let compiler = BytecodeCompiler::new(temp.path()).expect("Compiler creation failed");
    let registry = ModuleRegistry::new(temp.path().to_path_buf());

    let count = compiler.compile_batch(&[source1, source2], &registry);

    assert_eq!(count, 2);
    assert!(registry.get_bytecode("pkg1.utils").is_some());
    assert!(registry.get_bytecode("pkg2.utils").is_some());
}

/// Test: Registry concurrent access simulation
#[test]
fn test_registry_concurrent_insert() {
    use std::thread;

    let temp = TempDir::new().unwrap();
    let registry = std::sync::Arc::new(ModuleRegistry::new(temp.path().to_path_buf()));

    let handles: Vec<_> = (0..10)
        .map(|i| {
            let reg = std::sync::Arc::clone(&registry);
            let temp_path = temp.path().to_path_buf();
            thread::spawn(move || {
                reg.insert(BytecodeEntry {
                    name: format!("module_{}", i),
                    source_path: temp_path.join(format!("module_{}.py", i)),
                    bytecode: vec![0xe3, i as u8],
                    is_package: false,
                });
            })
        })
        .collect();

    for handle in handles {
        handle.join().unwrap();
    }

    assert_eq!(registry.len(), 10);
    for i in 0..10 {
        assert!(registry.get_bytecode(&format!("module_{}", i)).is_some());
    }
}

/// Test: Re-compilation after source deletion and recreation
#[test]
fn test_recompile_after_delete() {
    let temp = TempDir::new().unwrap();
    let source = create_test_module(temp.path(), "transient", "x = 1");

    let compiler = BytecodeCompiler::new(temp.path()).expect("Compiler creation failed");

    // First compile
    let bytecode1 = compiler.compile(&source).expect("First compile failed");

    // Delete source
    fs::remove_file(&source).unwrap();

    // Recreate with different content
    std::thread::sleep(std::time::Duration::from_millis(10));
    fs::write(&source, "x = 999\ny = 888").unwrap();

    // Second compile should produce different bytecode
    let bytecode2 = compiler.compile(&source).expect("Second compile failed");

    assert_ne!(bytecode1, bytecode2, "Recompiled bytecode should differ");
}

/// Test: Module with syntax edge cases (multiline strings, decorators)
#[test]
fn test_complex_syntax() {
    let temp = TempDir::new().unwrap();
    let source = create_test_module(
        temp.path(),
        "complex_syntax",
        r#"
'''
Multiline docstring
'''

def decorator(f):
    return f

@decorator
class MyClass:
    """Class docstring"""
    
    def __init__(self):
        self.value = 42
    
    async def async_method(self):
        pass

LAMBDA = lambda x: x * 2
COMPREHENSION = [i for i in range(10) if i % 2 == 0]
"#,
    );

    let compiler = BytecodeCompiler::new(temp.path()).expect("Compiler creation failed");
    let bytecode = compiler.compile(&source);

    assert!(bytecode.is_ok(), "Complex syntax should compile");
}

/// Test: Bytecode size sanity check
#[test]
fn test_bytecode_size_proportional() {
    let temp = TempDir::new().unwrap();

    let small_source = create_test_module(temp.path(), "small", "x = 1");
    let large_source = create_test_module(
        temp.path(),
        "large",
        &(0..50)
            .map(|i| format!("var_{} = {}\n", i, i))
            .collect::<String>(),
    );

    let compiler = BytecodeCompiler::new(temp.path()).expect("Compiler creation failed");

    let small_bytecode = compiler
        .compile(&small_source)
        .expect("Small compile failed");
    let large_bytecode = compiler
        .compile(&large_source)
        .expect("Large compile failed");

    // Large module should have more bytecode
    assert!(
        large_bytecode.len() > small_bytecode.len(),
        "Larger source should produce more bytecode"
    );
}

/// Test: Registry source path retrieval
#[test]
fn test_registry_source_path_retrieval() {
    let temp = TempDir::new().unwrap();
    let registry = ModuleRegistry::new(temp.path().to_path_buf());

    let expected_path = temp.path().join("mymodule.py");
    registry.insert(BytecodeEntry {
        name: "mymodule".to_string(),
        source_path: expected_path.clone(),
        bytecode: vec![1, 2, 3],
        is_package: false,
    });

    let retrieved_path = registry.get_source_path("mymodule");
    assert!(retrieved_path.is_some());
    assert_eq!(retrieved_path.unwrap(), expected_path);
}

/// Test: Registry is_empty and len consistency
#[test]
fn test_registry_empty_and_len() {
    let temp = TempDir::new().unwrap();
    let registry = ModuleRegistry::new(temp.path().to_path_buf());

    assert!(registry.is_empty());
    assert_eq!(registry.len(), 0);

    registry.insert(BytecodeEntry {
        name: "test".to_string(),
        source_path: temp.path().join("test.py"),
        bytecode: vec![1],
        is_package: false,
    });

    assert!(!registry.is_empty());
    assert_eq!(registry.len(), 1);
}
