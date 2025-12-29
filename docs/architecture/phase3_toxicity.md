# Phase 3: Toxicity Filter Architecture

> **Status:** Implementation Complete (3.1, 3.2), Integration In Progress (3.3)
> **Goal:** Identify modules unsafe for snapshot/reset and tag tests accordingly
> **Prerequisite:** Phase 2 (Zero-Copy Loader) Complete

---

## Table of Contents

1. [Problem Statement](#1-problem-statement)
2. [Solution Overview](#2-solution-overview)
3. [Implementation Status](#3-implementation-status)
4. [Module: analysis.rs (Phase 3.1)](#4-module-analysisrs-phase-31)
5. [Module: graph.rs (Phase 3.2)](#5-module-graphrs-phase-32)
6. [Integration (Phase 3.3)](#6-integration-phase-33)
7. [Testing Strategy](#7-testing-strategy)
8. [File Changes Summary](#8-file-changes-summary)

---

## 1. Problem Statement

### The Fork Safety Problem

Certain Python operations create resources that cannot be safely snapshot/reset:

| Resource Type | Example | Problem |
|:--------------|:--------|:--------|
| **OS Threads** | `threading.Thread` | Threads persist across snapshot, cause deadlocks |
| **Subprocesses** | `multiprocessing.Process` | Child processes have shared state |
| **File Descriptors** | `socket.socket` | FDs inherit incorrectly after reset |
| **Native Code** | `ctypes.CDLL` | C libraries may hold locks |
| **Background Threads** | `grpc.insecure_channel` | Connection threads persist |

### The Consequence

If a test uses any of these "toxic" patterns and we attempt snapshot/reset:
- Deadlocks from locked mutexes
- Orphaned threads consuming resources
- Socket connections in invalid state
- Segfaults from native code state corruption

### The Solution

**Toxicity Filter**: Statically analyze Python code to identify toxic patterns, then route:
- **Safe tests** -> Snapshot/Reset path (fast, reuse worker)
- **Toxic tests** -> Fork/Kill path (slower, fresh worker each time)

---

## 2. Solution Overview

```
┌─────────────────────────────────────────────────────────────┐
│                    PHASE 3 ARCHITECTURE                     │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  ┌─────────────┐    ┌─────────────┐    ┌─────────────┐     │
│  │ analysis.rs │───>│  graph.rs   │───>│ Integration │     │
│  │  (3.1)      │    │   (3.2)     │    │   (3.3)     │     │
│  └─────────────┘    └─────────────┘    └─────────────┘     │
│        │                  │                  │              │
│        v                  v                  v              │
│  ┌───────────┐      ┌───────────┐      ┌───────────┐       │
│  │ Local     │      │ Transitive│      │ Tag Tests │       │
│  │ Toxicity  │      │ Propagate │      │ is_toxic  │       │
│  └───────────┘      └───────────┘      └───────────┘       │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

**Phase 3 Scope:** Detection + Classification + Tagging
**Phase 4 Scope:** Execution (using toxicity to decide reset vs kill)

---

## 3. Implementation Status

| Sub-Stage | Status | Description |
|:----------|:-------|:------------|
| 3.1 Local Scanner | **COMPLETE** | `src/analysis.rs` - 45 tests |
| 3.2 Dependency Graph | **COMPLETE** | `src/graph.rs` - 20 tests |
| 3.3 Integration | **IN PROGRESS** | Wire into discovery/resolver |

---

## 4. Module: analysis.rs (Phase 3.1)

### 4.1 Purpose

Static analysis of a single Python file to detect toxic patterns.

### 4.2 Key Design Decisions

| Decision | Choice | Rationale |
|:---------|:-------|:----------|
| Star imports from toxic modules | **Toxic** | Aggressive stance - can't know what's imported |
| Dynamic imports | **Toxic** | `importlib.import_module`, `__import__`, `exec` |
| Imports inside functions | **Detected** | Tach worker recycling model - threads persist |
| Parse errors | **Toxic** | Conservative - can't analyze, assume worst |
| TYPE_CHECKING blocks | **Skipped** | Type hints only, never executed at runtime |

### 4.3 Blocklists

```rust
/// Toxic standard library modules
const TOXIC_STD_LIB: &[&str] = &[
    "threading",
    "_thread",
    "multiprocessing",
    "socket",
    "ctypes",
    "signal",
    "concurrent.futures",
];

/// Toxic external packages
const TOXIC_EXTERNAL_MODULES: &[&str] = &[
    "grpc",
    "pandas",     // OpenMP threads
    "tensorflow", // CUDA context
    "torch",      // CUDA context
    "cv2",        // OpenCV threads
    "gevent",     // Greenlets
    "cffi",
];
```

### 4.4 Data Structures

```rust
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
```

### 4.5 Public API

```rust
/// Analyze a single Python source file for toxicity
pub fn analyze_file(source: &str, path: &Path) -> ToxicityReport
```

### 4.6 Test Coverage (49 tests)

- Basic imports (threading, multiprocessing, socket, ctypes, signal, concurrent.futures)
- From-imports (`from threading import Thread`)
- Star imports (`from threading import *`)
- Aliased imports (`import threading as t`)
- Dynamic imports (importlib, `__import__`, exec)
- External packages (grpc, pandas, tensorflow, torch, cv2, gevent, cffi)
- Safe modules (os, json, pathlib, collections)
- Function body detection (imports inside functions, nested functions, class methods)
- Control flow (if, try/except, with, for, while)
- Edge cases (empty file, comments, docstrings, parse errors)
- **TYPE_CHECKING skip** (`if TYPE_CHECKING:`, `if typing.TYPE_CHECKING:`)

---

## 5. Module: graph.rs (Phase 3.2)

### 5.1 Purpose

Build a dependency graph of Python modules and propagate toxicity transitively.

### 5.2 Key Design Decisions

| Decision | Choice | Rationale |
|:---------|:-------|:----------|
| Graph library | `petgraph` | Mature, well-tested, on crates.io |
| Edge direction | A -> B means "A imports B" | Natural for propagation |
| Cycle handling | Fixed-point iteration | Simple, handles all cycle patterns |
| External imports | Ignore (not in graph) | Already caught by 3.1 local analysis |

### 5.3 Data Structures

```rust
/// Node data in the toxicity graph
#[derive(Debug, Clone)]
pub struct ModuleNode {
    /// Dotted module name (e.g., "app.utils")
    pub name: String,
    /// File path
    pub path: PathBuf,
    /// Whether this module is toxic
    pub is_toxic: bool,
    /// Reasons for toxicity
    pub reasons: Vec<String>,
}

/// The Toxicity Dependency Graph
pub struct ToxicityGraph {
    /// The directed graph: Edge A -> B means "A imports B"
    graph: DiGraph<ModuleNode, ()>,
    /// Map dotted module name -> NodeIndex
    name_to_node: HashMap<String, NodeIndex>,
    /// Map file path -> NodeIndex
    path_to_node: HashMap<PathBuf, NodeIndex>,
}
```

### 5.4 Public API

```rust
/// Build a toxicity graph from a list of Python file paths
pub fn build(paths: &[PathBuf], project_root: &Path) -> Self

/// Check if a module (by path) is toxic
pub fn is_toxic(&self, path: &Path) -> bool

/// Check if a module (by name) is toxic
pub fn is_toxic_by_name(&self, name: &str) -> bool

/// Get the toxicity report for a module
pub fn get_report(&self, path: &Path) -> Option<(bool, Vec<String>)>

/// Get all toxic modules
pub fn toxic_modules(&self) -> Vec<&ModuleNode>

/// Get all safe modules
pub fn safe_modules(&self) -> Vec<&ModuleNode>
```

### 5.5 Propagation Algorithm

```rust
/// Fixed-point iteration for toxicity propagation
fn propagate(&mut self) {
    loop {
        let mut changed = false;

        for (from_idx, to_idx) in edges {
            // If B is toxic and A imports B, then A becomes toxic
            if graph[to_idx].is_toxic && !graph[from_idx].is_toxic {
                graph[from_idx].is_toxic = true;
                graph[from_idx].reasons.push(
                    format!("Imports toxic module '{}'", graph[to_idx].name)
                );
                changed = true;
            }
        }

        if !changed { break; }
    }
}
```

### 5.6 Module Name Resolution

```rust
/// Convert file path to dotted module name
/// "app/utils.py" -> "app.utils"
/// "app/utils/__init__.py" -> "app.utils"
pub fn path_to_module_name(path: &Path, project_root: &Path) -> String
```

### 5.7 Test Coverage (20 tests)

- Path to module name conversion (simple, nested, __init__.py, deep)
- Single module (safe and toxic)
- Propagation: 1-hop, 2-hop, circular (2 nodes), circular (3 nodes)
- Safe chains (no propagation)
- Partial toxicity (mixed imports)
- Nested module imports (`app.utils`)
- From-import resolution
- Query APIs (`is_toxic_by_name`, `toxic_modules`)
- Edge cases (external imports, unresolved imports, self-import)

---

## 6. Integration (Phase 3.3)

### 6.1 Overview

Wire `ToxicityGraph` into the test discovery and resolution pipeline.

### 6.2 Changes Required

#### 6.2.1 Add `is_toxic` to RunnableTest

**File:** `src/resolver.rs`

```rust
pub struct RunnableTest {
    pub file_path: PathBuf,
    pub test_name: String,
    pub is_async: bool,
    pub fixtures: Vec<ResolvedFixture>,
    // NEW
    pub is_toxic: bool,
}
```

#### 6.2.2 Add `is_toxic` to TestModule

**File:** `src/discovery.rs`

```rust
pub struct TestModule {
    pub path: PathBuf,
    pub tests: Vec<TestCase>,
    pub fixtures: Vec<FixtureDefinition>,
    // NEW
    pub is_toxic: bool,
}
```

#### 6.2.3 Integration Function

**File:** `src/lib.rs` or new `src/pipeline.rs`

```rust
/// Discover tests with toxicity analysis
pub fn discover_with_toxicity(root: &Path) -> Result<(DiscoveryResult, ToxicityGraph)> {
    // 1. Run standard discovery
    let discovery = discovery::discover(root)?;

    // 2. Collect all file paths
    let paths: Vec<PathBuf> = discovery.modules
        .iter()
        .map(|m| m.path.clone())
        .collect();

    // 3. Build toxicity graph
    let graph = ToxicityGraph::build(&paths, root);

    Ok((discovery, graph))
}
```

#### 6.2.4 Wiring in main.rs

```rust
// In execute_session():
let (discovery_result, toxicity_graph) = discover_with_toxicity(&cwd)?;

// After resolution:
for test in &mut runnable_tests {
    test.is_toxic = toxicity_graph.is_toxic(&test.file_path);
}
```

---

## 7. Testing Strategy

### 7.1 Unit Tests

- **analysis.rs**: 45 tests for local pattern detection
- **graph.rs**: 20 tests for graph construction and propagation

### 7.2 Integration Tests

**File:** `rust_tests/toxicity_integration.rs`

```rust
#[test]
fn test_discover_with_toxicity_marks_toxic_tests() {
    let tmp = TempDir::new().unwrap();

    // Create toxic test file
    create_file(tmp.path(), "test_toxic.py", "import threading\ndef test_foo(): pass");

    // Create safe test file
    create_file(tmp.path(), "test_safe.py", "import os\ndef test_bar(): pass");

    // Run discovery with toxicity
    let (discovery, graph) = discover_with_toxicity(tmp.path()).unwrap();

    // Verify
    assert!(graph.is_toxic_by_name("test_toxic"));
    assert!(!graph.is_toxic_by_name("test_safe"));
}
```

### 7.3 Python Gauntlet Tests

**File:** `tests/gauntlet_phase3/`

- `test_toxic_patterns.py` - Tests using threading, multiprocessing, socket
- `test_safe_patterns.py` - Tests using only safe modules
- `toxic_helper.py` - Helper module with toxic patterns
- `test_transitive.py` - Tests importing toxic helper

---

## 8. File Changes Summary

| File | Status | Description |
|:-----|:-------|:------------|
| `src/analysis.rs` | **COMPLETE** | Local toxicity scanner (45 tests) |
| `src/graph.rs` | **COMPLETE** | Dependency graph with propagation (20 tests) |
| `src/lib.rs` | **MODIFIED** | Added `pub mod analysis;` and `pub mod graph;` |
| `Cargo.toml` | **MODIFIED** | Added `petgraph = "0.6"` |
| `src/resolver.rs` | **PENDING** | Add `is_toxic` to RunnableTest |
| `src/discovery.rs` | **PENDING** | Add `is_toxic` to TestModule |
| `src/main.rs` | **PENDING** | Wire toxicity into pipeline |
| `rust_tests/toxicity_integration.rs` | **PENDING** | Integration tests |
| `docs/architecture/phase3_toxicity.md` | **UPDATED** | This document |

---

## Appendix: Performance

Target: < 50ms for 1000 files

The implementation achieves this through:
1. Single-pass AST traversal per file
2. HashMap-based module resolution (O(1) lookup)
3. Fixed-point propagation (typically 2-3 iterations)
4. No file I/O during propagation (all data in memory)

---

## Appendix: Future Enhancements

### Fixture Toxicity Propagation

If a fixture is toxic, all tests using that fixture should be toxic.

**Status:** Deferred to Phase 3.3 or Phase 4.
