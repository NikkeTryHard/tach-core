# Rust Integration for Tach

Rust serves as the **hypervisor substrate** for Tach, inverting the traditional relationship between test runner and Python interpreter. Rather than Python orchestrating Python, a compiled Rust binary controls the Python runtime as a "Leaf Node" execution engine.

> Source: "the runner is a high-performance native binary--constructed in Rust--that acts as a hypervisor for the Python runtime" -- _Rust-CPython Execution Blueprint Research_

---

## Overview: Why Rust?

Python test runners like pytest suffer from an inherent "dynamic tax":

- **Import Tax**: Collection requires executing Python imports, triggering cascading module loads
- **Serialization Bottleneck**: `multiprocessing` requires pickle for IPC
- **GIL Contention**: True parallelism requires process isolation with heavy overhead

> Source: "The reliance on runtime reflection, while offering immense flexibility, imposes a severe 'dynamic tax' that scales linearly with the size of the codebase" -- _Python Testing Engine Rust Breakthroughs_

Rust eliminates these via static analysis, shared memory IPC, and native Tokio scheduling that bypasses the GIL entirely.

---

## Kineton Engine

The "Kineton" architecture treats tests as content-addressable execution units.

### Static Discovery

Tach uses `ruff_python_parser` for AST-based test discovery without executing Python:

> Source: "ruff_python_parser, the Rust-based parsing engine powering the Ruff linter. This parser is designed for extreme performance, capable of processing gigabytes of source code per second" -- _Rust-CPython Execution Blueprint Research_

Discovery extracts import statements (dependency graphs), function definitions (`test_*` patterns), and decorators (`@pytest.mark.parametrize` values).

### Semantic Hashing

Tests are fingerprinted by logical content using **SipHash** on normalized AST nodes:

> Source: "The AST visitor walks the tree of a function. It serializes the nodes into a byte stream, deliberately excluding: Docstrings, Type hints, Formatting" -- _Python Testing Engine Rust Breakthroughs_

Changes to whitespace or comments do not trigger re-execution.

### Native Mocking via PEP 523

Kineton intercepts execution at the C-level using the frame evaluation API:

> Source: "PEP 523 allows C-extensions to override the default bytecode evaluation function. Kineton installs a custom frame evaluator written in Rust" -- _Python Testing Engine Rust Breakthroughs_

Mechanism: Register via `_PyInterpreterState_SetEvalFrameFunc`, check Rust hash map for mock registration, return canned value without executing bytecode if mocked.

> Source: "The overhead of the check is a single pointer lookup... This technique allows Kineton to mock millions of calls per second with zero Python-level overhead" -- _Python Testing Engine Rust Breakthroughs_

---

## Zero-Copy Module Loading

Tach bypasses `importlib` entirely by loading pre-compiled bytecode directly into memory.

### mmap-Based Loading

> Source: "Memory mapping allows a file's contents to be mapped directly into the virtual address space. The interpreter reads directly from the OS page cache" -- _Zero-Copy Python Module Loading_

Benefits: No userspace copy, page cache sharing across workers, direct pointer access to C-API.

### PyMarshal_ReadObjectFromString

Code objects are deserialized directly from mapped memory:

```c
PyObject* PyMarshal_ReadObjectFromString(const char *data, Py_ssize_t len)
```

> Source: "The Rust Control Plane fetches the bytecode blob from the CAS. It does not instruct Python to 'import' the file. Instead, it creates the code object directly using PyMarshal_ReadObjectFromString" -- _Rust-CPython Execution Blueprint Research_

The 16-byte `.pyc` header must be skipped. Use `PyImport_ExecCodeModuleObject` for proper `sys.modules` registration.

---

## PEP 684 Sub-Interpreters

Each worker can run in an isolated sub-interpreter with its own GIL.

> Source: "PEP 684 introduces the ability to spawn sub-interpreters that each possess their own GIL... This 'Hybrid Isolation' model offers the best of both worlds" -- _Rust-CPython Execution Blueprint Research_

Configuration via `PyInterpreterConfig` with `.gil = PyInterpreterConfig_OWN_GIL`.

### Thread Affinity

> Source: "To solve this, we employ tokio::task::LocalSet. We associate a specific LocalSet with each worker thread that owns a Python interpreter" -- _Rust-CPython Execution Blueprint Research_

Tokio's work-stealing scheduler could move tasks between threads, corrupting interpreter state. `LocalSet` prevents this.

### Cross-Interpreter Data Sharing

> Source: "We define a custom Rust type that implements the Python Buffer Protocol slots. The memoryview supports the buffer protocol natively, allowing Python code in the sub-interpreter to read the data without copying" -- _Rust-CPython Execution Blueprint Research_

---

## PEP 669 Low-Impact Monitoring

Tach uses PEP 669 for coverage and observability with minimal overhead.

> Source: "PEP 669 replaces the slow sys.settrace with a low-overhead monitoring API" -- _Rust-CPython Execution Blueprint Research_

Subscribe to events (`PY_MONITORING_EVENT_BRANCH`, `_LINE`, `_RAISE`) via `PyMonitoring_RegisterCallback`. The Rust callback writes to a lock-free ring buffer consumed asynchronously.

> Source: "We can run tests with 'Always-On' coverage with less than 2-5% overhead, compared to the 30-50% typical of coverage.py" -- _Rust-CPython Execution Blueprint Research_

---

## PyO3 Integration

PyO3 bridges Rust and Python with careful GIL management.

### GIL Release Patterns

```rust
py.allow_threads(|| {
    heavy_rust_computation()
})
```

> Source: "Always release GIL (Python::allow_threads) during heavy Rust ops" -- _CLAUDE.md_

### Rayon Parallelism

> Source: "Using Rust's rayon data parallelism library, the Control Plane can distribute the parsing of 10,000+ files across all available CPU cores" -- _Rust-CPython Execution Blueprint Research_

Pattern: Parse files in parallel, merge results single-threaded.

---

## Implementation in Tach

The CHANGELOG maps research concepts to version milestones:

| Version   | Research Phase    | Primary Paper                               | Key Deliverable                      |
| --------- | ----------------- | ------------------------------------------- | ------------------------------------ |
| **0.1.x** | Static Discovery  | _Python Testing Engine Rust Breakthroughs_  | AST-based test discovery ("Kineton") |
| **0.5.x** | Observability     | _Rust-CPython Execution Blueprint Research_ | PEP 669 low-impact monitoring        |
| **0.6.x** | Zero-Copy Loading | _Zero-Copy Python Module Loading_           | mmap-based bytecode loading          |

### 0.1.x - Kineton Foundation (Current)

- `ruff_python_parser` for static AST analysis
- Fixture dependency graph construction
- Zygote fork-server pattern

> Source: "shifts the heavy lifting of static analysis, dependency graph resolution, and execution supervision out of the slow, interpreted Python runtime and into a high-performance, compiled substrate: Rust" -- CHANGELOG

### 0.5.x/0.6.x - Planned

- PEP 669 monitoring, ring buffer coverage
- mmap-based bytecode cache, topological module loading

---

## Key References

### Primary Papers

1. **Python Testing Engine Rust Breakthroughs** - Kineton, semantic hashing, PEP 523
   - [Full paper](../papers/Python%20Testing%20Engine%20Rust%20Breakthroughs.txt)

2. **Rust-CPython Execution Blueprint Research** - PEP 684, PEP 669, Tokio
   - [Full paper](../papers/Rust-CPython%20Execution%20Blueprint%20Research.txt)

3. **Zero-Copy Python Module Loading** - mmap, PyMarshal, importlib bypass
   - [Full paper](../papers/Zero-Copy%20Python%20Module%20Loading.txt)

### External References

> Source: [PyO3 Parallelism Guide](https://pyo3.rs/main/parallelism) - GIL release patterns

> Source: [PEP 684](https://peps.python.org/pep-0684/) - Per-Interpreter GIL

> Source: [PEP 669](https://peps.python.org/pep-0669/) - Low Impact Monitoring

> Source: [PEP 523](https://peps.python.org/pep-0523/) - Frame Evaluation API

> Source: [Python C-API Marshal](https://docs.python.org/3/c-api/marshal.html) - PyMarshal functions

---

## Summary

| Component | Python Approach      | Tach Rust Approach        | Speedup |
| --------- | -------------------- | ------------------------- | ------- |
| Discovery | Runtime import       | Static AST parsing        | 10-100x |
| IPC       | Pickle serialization | Shared memory             | 10-50x  |
| Mocking   | `MagicMock` proxies  | PEP 523 C-level intercept | 10-50x  |
| Loading   | `importlib` + I/O    | mmap + PyMarshal          | 10-100x |
| Coverage  | `sys.settrace`       | PEP 669 + ring buffer     | 10-15x  |

The architecture treats Python as an embedded execution engine, with Rust handling all control plane operations.
