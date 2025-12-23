//! Zero-Copy Module Loader: Bypass importlib via direct bytecode injection
//!
//! This module implements the "Push" model for Python module loading:
//! - Compile .py → .pyc using system Python (eager, during discovery)
//! - Store header-stripped bytecode in a thread-safe registry
//! - Expose FFI for workers to request and load modules
//!
//! Key C-API functions used:
//! - `PyMarshal_ReadObjectFromString`: Deserialize bytecode → code object
//! - `PyImport_ExecCodeModuleObject`: Execute code, register in sys.modules

use anyhow::{anyhow, Result};
use dashmap::DashMap;
use pyo3::ffi;
use pyo3::prelude::*;
use pyo3::types::PyList;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::SystemTime;

/// .pyc header size for Python 3.7+ (PEP 552)
/// Format: Magic (4) + BitField (4) + Timestamp (4) + Size (4) = 16 bytes
const PYC_HEADER_SIZE: usize = 16;

/// Global registry instance (initialized once at startup)
static REGISTRY: OnceLock<ModuleRegistry> = OnceLock::new();

/// Global cache for Python executable path (prevents repeated subprocess spawning)
static CACHED_PYTHON_EXE: OnceLock<PathBuf> = OnceLock::new();

/// Global cache for Python magic number (prevents repeated subprocess spawning)
/// This is CRITICAL: each call to get magic number spawns a Python process.
/// Without caching, parallel tests would spawn many Python processes, causing OOM.
static CACHED_MAGIC: OnceLock<[u8; 4]> = OnceLock::new();

// =============================================================================
// BytecodeEntry: Registry entry for a compiled module
// =============================================================================

/// A compiled Python module ready for injection
#[derive(Clone)]
pub struct BytecodeEntry {
    /// Python module name (e.g., "foo.bar")
    pub name: String,
    /// Absolute path to source .py file
    pub source_path: PathBuf,
    /// Header-stripped bytecode (bytes 16+ of .pyc)
    pub bytecode: Vec<u8>,
    /// True if this is a package (__init__.py)
    pub is_package: bool,
}

// =============================================================================
// ModuleRegistry: Thread-safe bytecode storage
// =============================================================================

/// Thread-safe registry of compiled Python modules
///
/// Uses DashMap for concurrent access from multiple workers.
/// Keyed by module name (e.g., "foo.bar"), not file path.
pub struct ModuleRegistry {
    entries: DashMap<String, BytecodeEntry>,
    /// Project root for path resolution (reserved for future use)
    #[allow(dead_code)]
    project_root: PathBuf,
}

impl ModuleRegistry {
    /// Create a new empty registry
    pub fn new(project_root: PathBuf) -> Self {
        Self {
            entries: DashMap::new(),
            project_root,
        }
    }

    /// Insert a compiled module into the registry
    pub fn insert(&self, entry: BytecodeEntry) {
        self.entries.insert(entry.name.clone(), entry);
    }

    /// Get bytecode for a module by name
    pub fn get_bytecode(&self, name: &str) -> Option<Vec<u8>> {
        self.entries.get(name).map(|e| e.bytecode.clone())
    }

    /// Get source path for a module by name
    pub fn get_source_path(&self, name: &str) -> Option<PathBuf> {
        self.entries.get(name).map(|e| e.source_path.clone())
    }

    /// Check if a module is a package
    pub fn is_package(&self, name: &str) -> Option<bool> {
        self.entries.get(name).map(|e| e.is_package)
    }

    /// Get number of entries in registry
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if registry is empty
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// =============================================================================
// BytecodeCompiler: .py → .pyc with persistent caching
// =============================================================================

/// Compiles Python source files to bytecode using system Python
///
/// Features:
/// - Persistent cache in `.tach/cache/`
/// - mtime-based staleness detection
/// - Magic number validation
pub struct BytecodeCompiler {
    /// Cache directory (.tach/cache)
    cache_dir: PathBuf,
    /// Project root for module name resolution
    project_root: PathBuf,
    /// Python interpreter path
    python_exe: PathBuf,
    /// Expected magic number (from running Python)
    expected_magic: Option<[u8; 4]>,
}

impl BytecodeCompiler {
    /// Create a new compiler with cache in project_root/.tach/cache
    ///
    /// This uses global caches for Python path and magic number to avoid
    /// spawning multiple Python subprocesses during parallel test execution.
    pub fn new(project_root: &Path) -> Result<Self> {
        let cache_dir = project_root.join(".tach").join("cache");
        fs::create_dir_all(&cache_dir)?;

        // Find Python executable (cached globally)
        let python_exe = Self::find_python_cached()?;

        // Get expected magic number from running Python (cached globally)
        let expected_magic = Self::get_python_magic_cached(&python_exe)?;

        Ok(Self {
            cache_dir,
            project_root: project_root.to_path_buf(),
            python_exe,
            expected_magic: Some(expected_magic),
        })
    }

    /// Find the Python interpreter (cached globally)
    ///
    /// Uses CACHED_PYTHON_EXE to ensure we only spawn `which` once
    /// regardless of how many tests run in parallel.
    fn find_python_cached() -> Result<PathBuf> {
        // Try to get from cache first
        if let Some(cached) = CACHED_PYTHON_EXE.get() {
            return Ok(cached.clone());
        }

        // Not cached yet, find it
        let path = Self::find_python_impl()?;

        // Try to store it (may fail if another thread beat us)
        let _ = CACHED_PYTHON_EXE.set(path.clone());

        Ok(path)
    }

    /// Internal: actually find the Python interpreter
    fn find_python_impl() -> Result<PathBuf> {
        // Check PYO3_PYTHON first (set during build)
        if let Ok(path) = std::env::var("PYO3_PYTHON") {
            return Ok(PathBuf::from(path));
        }

        // Try python3, then python
        for name in &["python3", "python"] {
            if let Ok(output) = Command::new("which").arg(name).output() {
                if output.status.success() {
                    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    return Ok(PathBuf::from(path));
                }
            }
        }

        Err(anyhow!("Python interpreter not found"))
    }

    /// Get the magic number from the running Python interpreter (cached globally)
    ///
    /// CRITICAL: This uses CACHED_MAGIC to ensure we only spawn Python ONCE
    /// regardless of how many tests run in parallel. Without this cache,
    /// parallel tests would spawn many Python processes, potentially causing OOM.
    fn get_python_magic_cached(python_exe: &Path) -> Result<[u8; 4]> {
        // Try to get from cache first
        if let Some(cached) = CACHED_MAGIC.get() {
            return Ok(*cached);
        }

        // Not cached yet, fetch from Python
        let magic = Self::get_python_magic_impl(python_exe)?;

        // Try to store it (may fail if another thread beat us)
        let _ = CACHED_MAGIC.set(magic);

        Ok(magic)
    }

    /// Internal: actually get the magic number from Python
    fn get_python_magic_impl(python_exe: &Path) -> Result<[u8; 4]> {
        let output = Command::new(python_exe)
            .args(["-c", "import importlib.util; import sys; sys.stdout.buffer.write(importlib.util.MAGIC_NUMBER)"])
            .output()?;

        if !output.status.success() {
            return Err(anyhow!("Failed to get Python magic number"));
        }

        if output.stdout.len() < 4 {
            return Err(anyhow!("Invalid magic number length"));
        }

        let mut magic = [0u8; 4];
        magic.copy_from_slice(&output.stdout[..4]);
        Ok(magic)
    }

    /// Convert a file path to a Python module name
    fn path_to_module_name(&self, path: &Path) -> String {
        let relative = path.strip_prefix(&self.project_root).unwrap_or(path);

        let mut name = relative
            .with_extension("")
            .to_string_lossy()
            .replace(std::path::MAIN_SEPARATOR, ".");

        // Remove __init__ suffix for packages
        if name.ends_with(".__init__") {
            name = name.trim_end_matches(".__init__").to_string();
        }

        name
    }

    /// Get cache path for a source file
    fn cache_path(&self, source: &Path) -> PathBuf {
        let relative = source.strip_prefix(&self.project_root).unwrap_or(source);

        let mut cache_name = relative
            .to_string_lossy()
            .replace(std::path::MAIN_SEPARATOR, "_");
        cache_name.push_str(".pyc");

        self.cache_dir.join(cache_name)
    }

    /// Check if cached .pyc is stale (source mtime > cache mtime)
    fn is_cache_stale(&self, source: &Path, cache: &Path) -> bool {
        // If cache doesn't exist, it's stale
        if !cache.exists() {
            return true;
        }

        // Compare mtimes
        let source_mtime = fs::metadata(source)
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);

        let cache_mtime = fs::metadata(cache)
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);

        source_mtime > cache_mtime
    }

    /// Validate magic number of a .pyc file
    fn validate_magic(&self, pyc_path: &Path) -> Result<bool> {
        let mut file = fs::File::open(pyc_path)?;
        let mut magic = [0u8; 4];
        file.read_exact(&mut magic)?;

        if let Some(expected) = self.expected_magic {
            Ok(magic == expected)
        } else {
            Ok(true) // No validation if we couldn't get expected magic
        }
    }

    /// Compile a single source file, returning header-stripped bytecode
    ///
    /// Uses persistent cache with mtime-based invalidation.
    /// Validates magic number and recompiles on mismatch.
    pub fn compile(&self, source: &Path) -> Result<Vec<u8>> {
        let cache_path = self.cache_path(source);

        // Check if we need to recompile
        let needs_compile = if self.is_cache_stale(source, &cache_path) {
            true
        } else {
            // Cache exists and is fresh, but check magic number
            match self.validate_magic(&cache_path) {
                Ok(true) => false, // Magic matches, use cache
                Ok(false) => {
                    eprintln!(
                        "[loader] Magic mismatch for {}, recompiling",
                        source.display()
                    );
                    true
                }
                Err(_) => true, // Can't read cache, recompile
            }
        };

        if needs_compile {
            self.compile_to_cache(source, &cache_path)?;
        }

        // Read and strip header
        self.read_and_strip_header(&cache_path)
    }

    /// Compile source to cache using py_compile
    fn compile_to_cache(&self, source: &Path, cache: &Path) -> Result<()> {
        // Ensure parent directory exists
        if let Some(parent) = cache.parent() {
            fs::create_dir_all(parent)?;
        }

        let script = format!(
            "import py_compile; py_compile.compile('{}', '{}', doraise=True)",
            source.display(),
            cache.display()
        );

        let output = Command::new(&self.python_exe)
            .args(["-c", &script])
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!(
                "Compilation failed for {}: {}",
                source.display(),
                stderr
            ));
        }

        Ok(())
    }

    /// Read .pyc file and strip the 16-byte header
    fn read_and_strip_header(&self, pyc_path: &Path) -> Result<Vec<u8>> {
        let data = fs::read(pyc_path)?;

        if data.len() < PYC_HEADER_SIZE {
            return Err(anyhow!(
                "Invalid .pyc file (too short): {}",
                pyc_path.display()
            ));
        }

        // Return bytes after header
        Ok(data[PYC_HEADER_SIZE..].to_vec())
    }

    /// Batch compile all files, populating the registry
    ///
    /// Logs warnings for compilation failures but continues.
    pub fn compile_batch(&self, files: &[PathBuf], registry: &ModuleRegistry) -> usize {
        let mut success_count = 0;

        for file in files {
            // Skip non-.py files
            if file.extension().map_or(true, |e| e != "py") {
                continue;
            }

            match self.compile(file) {
                Ok(bytecode) => {
                    let name = self.path_to_module_name(file);
                    let is_package = file.file_name().map_or(false, |n| n == "__init__.py");

                    registry.insert(BytecodeEntry {
                        name: name.clone(),
                        source_path: file.clone(),
                        bytecode,
                        is_package,
                    });

                    success_count += 1;
                }
                Err(e) => {
                    // Graceful fallback: log warning, continue
                    eprintln!("[loader] WARN: Failed to compile {}: {}", file.display(), e);
                }
            }
        }

        eprintln!(
            "[loader] Compiled {} of {} files",
            success_count,
            files.len()
        );
        success_count
    }
}

// =============================================================================
// Global Registry Access
// =============================================================================

/// Initialize the global registry (called once at startup)
pub fn init_registry(project_root: PathBuf) -> &'static ModuleRegistry {
    REGISTRY.get_or_init(|| ModuleRegistry::new(project_root))
}

/// Get reference to global registry
pub fn get_registry() -> Option<&'static ModuleRegistry> {
    REGISTRY.get()
}

// =============================================================================
// FFI: Functions exposed to Python via tach_rust module
// =============================================================================

/// Get bytecode for a module from the registry (Request Model)
///
/// Called by Python harness: `tach_rust.get_module("foo.bar")`
/// Returns bytecode bytes if found, None otherwise.
#[pyfunction]
pub fn get_module(name: &str) -> Option<Vec<u8>> {
    REGISTRY.get().and_then(|r| r.get_bytecode(name))
}

/// Get source path for a module from the registry
///
/// Called by Python harness to set __file__ attribute.
#[pyfunction]
pub fn get_module_path(name: &str) -> Option<String> {
    REGISTRY
        .get()
        .and_then(|r| r.get_source_path(name))
        .map(|p| p.to_string_lossy().to_string())
}

/// Check if a module is a package (has __init__.py)
#[pyfunction]
pub fn is_module_package(name: &str) -> Option<bool> {
    REGISTRY.get().and_then(|r| r.is_package(name))
}

/// Load bytecode into Python's sys.modules
///
/// # Safety
/// This function uses raw C-API calls. The bytecode MUST:
/// - Be at least 0 bytes (header already stripped)
/// - Be valid marshalled code object
///
/// # Ownership
/// - `PyMarshal_ReadObjectFromString` returns a new reference (we own it)
/// - `PyImport_ExecCodeModuleObject` does NOT steal the code object reference
/// - We must call `Py_DECREF` on the code object after use
#[pyfunction]
pub fn load_module(
    py: Python<'_>,
    name: &str,
    source_path: &str,
    bytecode: &[u8],
) -> PyResult<bool> {
    // Safety check: bytecode should not be empty
    if bytecode.is_empty() {
        return Err(pyo3::exceptions::PyValueError::new_err("Bytecode is empty"));
    }

    unsafe {
        // 1. Deserialize bytecode to code object
        let code_obj = ffi::PyMarshal_ReadObjectFromString(
            bytecode.as_ptr() as *const i8,
            bytecode.len() as isize,
        );

        if code_obj.is_null() {
            // Fetch and return the Python exception
            return Err(PyErr::fetch(py));
        }

        // 2. Create Python strings for module name and path
        let name_cstr = std::ffi::CString::new(name)
            .map_err(|_| pyo3::exceptions::PyValueError::new_err("Invalid module name"))?;
        let path_cstr = std::ffi::CString::new(source_path)
            .map_err(|_| pyo3::exceptions::PyValueError::new_err("Invalid source path"))?;

        let name_obj = ffi::PyUnicode_FromString(name_cstr.as_ptr());
        if name_obj.is_null() {
            ffi::Py_DECREF(code_obj);
            return Err(PyErr::fetch(py));
        }

        let path_obj = ffi::PyUnicode_FromString(path_cstr.as_ptr());
        if path_obj.is_null() {
            ffi::Py_DECREF(code_obj);
            ffi::Py_DECREF(name_obj);
            return Err(PyErr::fetch(py));
        }

        // 3. Execute code object, creating module in sys.modules
        let module = ffi::PyImport_ExecCodeModuleObject(
            name_obj,
            code_obj,
            path_obj,
            std::ptr::null_mut(), // cpathname (optional)
        );

        // Clean up references we created
        ffi::Py_DECREF(code_obj);
        ffi::Py_DECREF(name_obj);
        ffi::Py_DECREF(path_obj);

        if module.is_null() {
            return Err(PyErr::fetch(py));
        }

        // 4. Patch namespace attributes
        patch_module_namespace(py, module, name, source_path)?;

        // Module is now in sys.modules, we don't need to hold a reference
        ffi::Py_DECREF(module);

        Ok(true)
    }
}

/// Patch module namespace with __file__, __package__, __path__
///
/// # Safety
/// `module` must be a valid, non-null PyObject pointer.
unsafe fn patch_module_namespace(
    py: Python<'_>,
    module: *mut ffi::PyObject,
    name: &str,
    source_path: &str,
) -> PyResult<()> {
    // __file__: Source file path
    let file_cstr = std::ffi::CString::new("__file__").unwrap();
    let file_val = ffi::PyUnicode_FromString(std::ffi::CString::new(source_path).unwrap().as_ptr());
    if !file_val.is_null() {
        ffi::PyObject_SetAttrString(module, file_cstr.as_ptr(), file_val);
        ffi::Py_DECREF(file_val);
    }

    // __package__: Parent package name
    let package_name = name.rsplit_once('.').map(|(p, _)| p).unwrap_or("");
    let pkg_cstr = std::ffi::CString::new("__package__").unwrap();
    let pkg_val = ffi::PyUnicode_FromString(std::ffi::CString::new(package_name).unwrap().as_ptr());
    if !pkg_val.is_null() {
        ffi::PyObject_SetAttrString(module, pkg_cstr.as_ptr(), pkg_val);
        ffi::Py_DECREF(pkg_val);
    }

    // __path__: Required for packages (directories)
    // Check if this is a package by looking at the registry or source path
    let is_package = source_path.ends_with("__init__.py")
        || REGISTRY
            .get()
            .and_then(|r| r.is_package(name))
            .unwrap_or(false);

    if is_package {
        // __path__ should be a list containing the package directory
        let parent_dir = Path::new(source_path)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();

        let path_list = PyList::new(py, &[parent_dir])?;
        let path_cstr = std::ffi::CString::new("__path__").unwrap();
        ffi::PyObject_SetAttrString(module, path_cstr.as_ptr(), path_list.as_ptr());
    }

    Ok(())
}

// =============================================================================
// Unit Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    /// Test path to module name conversion
    #[test]
    fn test_path_to_module_name() {
        let temp = TempDir::new().unwrap();
        let compiler = BytecodeCompiler::new(temp.path()).unwrap();

        // Simple module
        let path = temp.path().join("foo.py");
        assert_eq!(compiler.path_to_module_name(&path), "foo");

        // Nested module
        let path = temp.path().join("foo").join("bar.py");
        assert_eq!(compiler.path_to_module_name(&path), "foo.bar");

        // Package __init__.py
        let path = temp.path().join("foo").join("__init__.py");
        assert_eq!(compiler.path_to_module_name(&path), "foo");
    }

    /// Test cache path generation
    #[test]
    fn test_cache_path() {
        let temp = TempDir::new().unwrap();
        let compiler = BytecodeCompiler::new(temp.path()).unwrap();

        let source = temp.path().join("foo").join("bar.py");
        let cache = compiler.cache_path(&source);

        assert!(cache.to_string_lossy().contains(".tach"));
        assert!(cache.to_string_lossy().ends_with(".pyc"));
    }

    /// Test compilation of a simple module
    #[test]
    fn test_compile_simple_module() {
        let temp = TempDir::new().unwrap();

        // Create a simple Python file
        let source = temp.path().join("simple.py");
        let mut file = fs::File::create(&source).unwrap();
        writeln!(file, "def hello(): return 'world'").unwrap();

        // Compile it
        let compiler = BytecodeCompiler::new(temp.path()).unwrap();
        let bytecode = compiler.compile(&source);

        assert!(bytecode.is_ok(), "Compilation should succeed");
        let bytecode = bytecode.unwrap();
        assert!(!bytecode.is_empty(), "Bytecode should not be empty");

        // Verify header was stripped (bytecode should NOT start with magic)
        // The marshalled code object starts with TYPE_CODE ('c' = 0x63 or 'C' = 0x43)
        // depending on Python version
        assert!(
            bytecode[0] == 0x63 || bytecode[0] == 0xe3,
            "First byte should be TYPE_CODE marker, got 0x{:02x}",
            bytecode[0]
        );
    }

    /// Test cache staleness detection
    #[test]
    fn test_cache_staleness() {
        let temp = TempDir::new().unwrap();
        let compiler = BytecodeCompiler::new(temp.path()).unwrap();

        let source = temp.path().join("test.py");
        let cache = compiler.cache_path(&source);

        // Source exists, cache doesn't → stale
        fs::write(&source, "x = 1").unwrap();
        assert!(compiler.is_cache_stale(&source, &cache));

        // Create cache (via compilation)
        let _ = compiler.compile(&source);
        assert!(!compiler.is_cache_stale(&source, &cache));

        // Touch source to make it newer
        std::thread::sleep(std::time::Duration::from_millis(10));
        fs::write(&source, "x = 2").unwrap();
        assert!(compiler.is_cache_stale(&source, &cache));
    }

    /// Test registry operations
    #[test]
    fn test_registry_operations() {
        let temp = TempDir::new().unwrap();
        let registry = ModuleRegistry::new(temp.path().to_path_buf());

        // Insert entry
        registry.insert(BytecodeEntry {
            name: "foo.bar".to_string(),
            source_path: temp.path().join("foo/bar.py"),
            bytecode: vec![1, 2, 3],
            is_package: false,
        });

        // Retrieve
        assert!(registry.get_bytecode("foo.bar").is_some());
        assert!(registry.get_bytecode("nonexistent").is_none());
        assert_eq!(registry.len(), 1);
    }

    /// Test batch compilation
    #[test]
    fn test_batch_compilation() {
        let temp = TempDir::new().unwrap();

        // Create multiple Python files
        let files: Vec<PathBuf> = (0..3)
            .map(|i| {
                let path = temp.path().join(format!("mod{}.py", i));
                fs::write(&path, format!("x = {}", i)).unwrap();
                path
            })
            .collect();

        let compiler = BytecodeCompiler::new(temp.path()).unwrap();
        let registry = ModuleRegistry::new(temp.path().to_path_buf());

        let count = compiler.compile_batch(&files, &registry);

        assert_eq!(count, 3);
        assert_eq!(registry.len(), 3);
        assert!(registry.get_bytecode("mod0").is_some());
        assert!(registry.get_bytecode("mod1").is_some());
        assert!(registry.get_bytecode("mod2").is_some());
    }

    // =========================================================================
    // Extended Coverage Tests
    // =========================================================================

    /// Test magic number validation with valid cache
    #[test]
    fn test_magic_validation_valid_cache() {
        let temp = TempDir::new().unwrap();
        let source = temp.path().join("test_magic.py");
        fs::write(&source, "x = 1").unwrap();

        let compiler = BytecodeCompiler::new(temp.path()).unwrap();

        // Compile to create cache with correct magic
        let _ = compiler.compile(&source).unwrap();

        // Validate magic - should be true (matches current Python)
        let cache = compiler.cache_path(&source);
        let result = compiler.validate_magic(&cache);

        assert!(result.is_ok());
        assert!(result.unwrap(), "Magic should validate for fresh cache");
    }

    /// Test read_and_strip_header produces valid bytecode
    #[test]
    fn test_header_stripping_directly() {
        let temp = TempDir::new().unwrap();
        let source = temp.path().join("strip_test.py");
        fs::write(&source, "def f(): return 42").unwrap();

        let compiler = BytecodeCompiler::new(temp.path()).unwrap();

        // Compile first
        let bytecode = compiler.compile(&source).unwrap();

        // Bytecode should start with TYPE_CODE marker (0xe3 or 0x63)
        assert!(
            bytecode[0] == 0xe3 || bytecode[0] == 0x63,
            "Header should be stripped, first byte should be TYPE_CODE"
        );
    }

    /// Test find_python_cached returns consistent path
    #[test]
    fn test_find_python_cached_consistency() {
        // Call twice to verify caching works
        let path1 = BytecodeCompiler::find_python_cached().unwrap();
        let path2 = BytecodeCompiler::find_python_cached().unwrap();

        assert_eq!(path1, path2, "Cached Python path should be consistent");
        assert!(path1.exists(), "Python path should exist");
    }

    /// Test get_python_magic_cached returns 4 bytes
    #[test]
    fn test_magic_cached_is_4_bytes() {
        let python = BytecodeCompiler::find_python_cached().unwrap();
        let magic = BytecodeCompiler::get_python_magic_cached(&python).unwrap();

        assert_eq!(magic.len(), 4, "Magic number should be 4 bytes");
        // Magic number should not be all zeros
        assert!(
            magic.iter().any(|&b| b != 0),
            "Magic should not be all zeros"
        );
    }

    /// Test registry is_package for non-existent module
    #[test]
    fn test_registry_is_package_nonexistent() {
        let temp = TempDir::new().unwrap();
        let registry = ModuleRegistry::new(temp.path().to_path_buf());

        assert!(registry.is_package("nonexistent").is_none());
    }

    /// Test registry get_source_path for non-existent module
    #[test]
    fn test_registry_get_source_path_nonexistent() {
        let temp = TempDir::new().unwrap();
        let registry = ModuleRegistry::new(temp.path().to_path_buf());

        assert!(registry.get_source_path("nonexistent").is_none());
    }

    /// Test compile with missing source file
    #[test]
    fn test_compile_missing_source() {
        let temp = TempDir::new().unwrap();
        let compiler = BytecodeCompiler::new(temp.path()).unwrap();

        let nonexistent = temp.path().join("does_not_exist.py");
        let result = compiler.compile(&nonexistent);

        assert!(result.is_err(), "Compile should fail for missing source");
    }

    /// Test cache_path for various path structures
    #[test]
    fn test_cache_path_various_structures() {
        let temp = TempDir::new().unwrap();
        let compiler = BytecodeCompiler::new(temp.path()).unwrap();

        // Simple file
        let simple = temp.path().join("simple.py");
        let cache1 = compiler.cache_path(&simple);
        assert!(cache1.to_string_lossy().ends_with(".pyc"));

        // Nested file
        let nested = temp.path().join("a").join("b").join("c.py");
        let cache2 = compiler.cache_path(&nested);
        assert!(cache2.to_string_lossy().ends_with(".pyc"));
        assert!(cache2.to_string_lossy().contains("_")); // Separators replaced
    }

    /// Test path_to_module_name for various paths
    #[test]
    fn test_path_to_module_name_edge_cases() {
        let temp = TempDir::new().unwrap();
        let compiler = BytecodeCompiler::new(temp.path()).unwrap();

        // Single underscore prefix
        let underscore = temp.path().join("_private.py");
        assert!(compiler
            .path_to_module_name(&underscore)
            .contains("_private"));

        // Double underscore prefix
        let dunder = temp.path().join("__dunder__.py");
        assert!(compiler.path_to_module_name(&dunder).contains("__dunder__"));

        // Deeply nested __init__.py
        let deep_init = temp
            .path()
            .join("a")
            .join("b")
            .join("c")
            .join("__init__.py");
        let name = compiler.path_to_module_name(&deep_init);
        assert_eq!(name, "a.b.c");
    }

    /// Test batch compilation with empty file list
    #[test]
    fn test_batch_compile_empty() {
        let temp = TempDir::new().unwrap();
        let compiler = BytecodeCompiler::new(temp.path()).unwrap();
        let registry = ModuleRegistry::new(temp.path().to_path_buf());

        let count = compiler.compile_batch(&[], &registry);

        assert_eq!(count, 0);
        assert!(registry.is_empty());
    }

    /// Test is_cache_stale with both newer and older cache
    #[test]
    fn test_cache_stale_timing() {
        let temp = TempDir::new().unwrap();
        let compiler = BytecodeCompiler::new(temp.path()).unwrap();

        let source = temp.path().join("timing_test.py");
        fs::write(&source, "x = 1").unwrap();

        // Compile to create cache
        let _ = compiler.compile(&source).unwrap();
        let cache = compiler.cache_path(&source);

        // Cache should not be stale immediately
        assert!(!compiler.is_cache_stale(&source, &cache));

        // Wait and modify source
        std::thread::sleep(std::time::Duration::from_millis(50));
        fs::write(&source, "x = 2").unwrap();

        // Cache should now be stale
        assert!(compiler.is_cache_stale(&source, &cache));
    }
}
