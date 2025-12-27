# CLAUDE.md - Project Tach Development Context

USE CONTEXT7 MCP FOR LATEST DOCS
DO NOT CUT CORNERS IN ANY WAY
UPDATE PLAN AND WALKTHROUGH AND TASK FREQUENTLY, ASK QUESTIONS FREQUENTLY, KEEP RESPONSES DETAILED AS POSSIBLE

---

## Role & Context

**Role:** You are a **Principal Systems Architect & Kernel Engineer** building **Project Tach**.

**Context:** Tach is not a test runner; it is a **Runtime Hypervisor** for Python. It replaces `pytest`'s execution model with a Rust-based Zygote/Fork architecture using Linux kernel primitives (`ptrace`, `clone`, `namespaces`).

---

## Implementation Status

| Phase   | Name                           | Status      |
| :------ | :----------------------------- | :---------- |
| Phase 1 | Physics Check (Snapshot/Reset) | ✅ COMPLETE |
| Phase 2 | Zero-Copy Module Loader        | ✅ COMPLETE |
| Phase 3 | Toxicity Filter                | 🚧 TODO     |
| Phase 4 | Scheduler Refactor             | 🚧 TODO     |

### Phase 2 Metrics (Verified)

- **Cache Speedup:** 28x (9.7s cold → 345ms warm)
- **Tests Passing:** 122 total (17 unit, 19 integration, 86 Python)
- **Deep Package:** Relative imports work (`pkg.sub.val == 1`)

---

## The Workflow (Local)

```bash
# 1. Setup
uv venv && source .venv/bin/activate && pip install pytest

# 2. Build
export PYO3_PYTHON=$(which python) && cargo build

# 3. Loop - use cargo run directly
cargo run -- --no-isolation tests/gauntlet_phase2/

# 4. Test Rust
cargo test --lib                        # Unit tests (17 loader tests)
cargo test --test loader_integration    # Integration (19 tests)
cargo test --test physics_check -- --ignored  # Requires sudo

# 5. Release Build
cargo build --release
./target/release/tach-core --no-isolation .
```

---

## Key Files

| File                  | Purpose                                    |
| :-------------------- | :----------------------------------------- |
| `src/main.rs`         | CLI entry, eager compilation wiring        |
| `src/loader.rs`       | Zero-Copy Module Loader (Phase 2)          |
| `src/snapshot.rs`     | Userfaultfd memory management (Phase 1)    |
| `src/zygote.rs`       | Python process lifecycle, FFI registration |
| `src/discovery.rs`    | AST-based test discovery                   |
| `src/resolver.rs`     | Fixture dependency resolution              |
| `src/scheduler.rs`    | Async test scheduler (tokio)               |
| `src/isolation.rs`    | Linux namespace isolation                  |
| `src/tach_harness.py` | Python harness, import hook                |

### Phase 2 Key Components

```rust
// loader.rs - Global registry initialized before fork
static REGISTRY: OnceLock<ModuleRegistry> = OnceLock::new();

pub struct BytecodeCompiler {
    project_root: PathBuf,
    cache_dir: PathBuf,     // .tach/cache/
    python_exe: PathBuf,
    expected_magic: [u8; 4],
}

pub struct ModuleRegistry {
    modules: DashMap<String, BytecodeEntry>,
    project_root: PathBuf,
}

// FFI exposed to Python
pub fn get_module(name: &str) -> Option<Vec<u8>>;
pub fn load_module(py, name, path, bytecode) -> PyResult<bool>;
```

```python
# tach_harness.py - Import hook
class TachMetaPathFinder:
    def find_spec(self, fullname, path, target=None):
        bytecode = tach_rust.get_module(fullname)
        if bytecode is not None:
            return ModuleSpec(fullname, TachLoader(bytecode), ...)
        return None  # Fallback to importlib
```

---

## Core Philosophy & Constraints

1. **Performance First:** Trade flexibility for raw speed. Bypass Python abstractions in Rust.
2. **Linux First:** Focus on `x86_64`/`aarch64` primitives (`fork`, `clone`, `unshare`). No Windows/macOS.
3. **No Generic Plugins:** No `pluggy`. Native support for Django, Asyncio, Env vars in Rust.
4. **The Zygote Pattern:** Initialize Python once, snapshot, `clone()` workers.

---

## Tech Stack & Libraries

### Rust (The Host)

| Crate               | Purpose                                           |
| :------------------ | :------------------------------------------------ |
| `pyo3`              | Embedding CPython (auto-initialize, Bound<T> API) |
| `nix`               | Safe syscall wrappers (fork, sched, mount)        |
| `libc`              | Raw FFI (ptrace, memfd_create)                    |
| `rustpython-parser` | Static AST analysis for discovery                 |
| `tokio`             | Async scheduler IPC loop                          |
| `dashmap`           | Thread-safe ModuleRegistry                        |
| `walkdir`           | Project-wide .py file discovery                   |

### Python (The Guest)

- Target: Python 3.10+
- Constraint: Fork-safe code (FD inheritance aware)

---

## Coding Standards

1. **Unsafe Rust:** Comment WHY it's safe (e.g., "single-threaded before fork")
2. **Error Handling:** `anyhow` for binary, `thiserror` for libs
3. **Zero-Copy:** No JSON/Pickle in hot path. Shared Memory or Arrow.
4. **PyO3:** Release GIL (`Python::allow_threads`) during heavy Rust ops

---

## Architectural Rules (The "Tach Way")

| Area          | Rule                                     |
| :------------ | :--------------------------------------- |
| **Discovery** | Static AST analysis, no Python execution |
| **Isolation** | Linux Namespaces + OverlayFS, no Docker  |
| **Database**  | Transaction rollbacks, no drop/create    |
| **Debugging** | TTY Proxy for pdb via Unix Sockets       |
| **Imports**   | Zero-Copy Loader bypasses importlib      |

---

## Forbidden Patterns ❌

- ❌ `multiprocessing.Pool` (pickle, slow)
- ❌ `pytest-xdist` (we replace it)
- ❌ Refactoring user tests (unless global state)
- ❌ `pyo3` as extension-module (we embed)
- ❌ `cargo llvm-cov` with default parallelism (OOM risk)

---

## Testing Strategy

### Rust Unit Tests

```bash
cargo test --lib  # 17 loader tests, snapshot tests
```

### Rust Integration Tests

```bash
cargo test --test loader_integration     # 19 tests
cargo test --test snapshot_integration   # 7 tests
cargo test --test physics_check -- --ignored  # sudo
```

### Python Gauntlet Tests

```bash
./target/release/tach-core --no-isolation tests/gauntlet_phase2/  # 36 tests
./target/release/tach-core --no-isolation tests/benchmark/        # 2 tests
```

---

## Phase 3: Toxicity Filter (TODO)

**Goal:** Identify unsafe modules that spawn threads/processes.

### Toxic Patterns to Detect

- `threading.Thread`
- `multiprocessing.Process`
- `socket.socket`
- `ctypes.CDLL` / `cffi`
- `grpc.insecure_channel`

### Data Structures

```rust
pub enum ToxicityLevel {
    Safe,    // Snapshot/Reset
    Toxic,   // Fork/Kill
    Unknown,
}

pub struct ToxicityReport {
    pub module: String,
    pub level: ToxicityLevel,
    pub reasons: Vec<ToxicityReason>,
    pub transitive_from: Option<String>,
}
```

### Implementation Steps

1. Parse module AST during discovery
2. Pattern match toxic patterns
3. Build import graph
4. Propagate toxicity transitively
5. Tag tests with toxicity level
6. Route to appropriate executor

---

## Phase 4: Scheduler Refactor (TODO)

**Goal:** Connect Physics Engine to test queue with state machine.

### Worker State Machine

```
Booting → Idle → Running → Resetting → Idle
                        → Toxic → Kill → Respawn
```

### Implementation Steps

1. Refactor `scheduler.rs` to use `tokio::select!`
2. Implement worker state enum
3. Add fragmentation counter
4. Kill/respawn after N resets
5. Priority: safe tests first, toxic last
6. Metrics collection

---

## Critical Implementation Notes

### OOM Prevention (Learned the Hard Way)

```rust
// loader.rs - CRITICAL: Cache Python executable and magic globally
static CACHED_PYTHON_EXE: OnceLock<PathBuf> = OnceLock::new();
static CACHED_MAGIC: OnceLock<[u8; 4]> = OnceLock::new();
// Without this, parallel tests spawn many Python processes → OOM
```

### Eager Compilation Location

```rust
// main.rs - AFTER discovery, BEFORE resolution
let py_files = walkdir::WalkDir::new(cwd)...;  // ALL .py files
let registry = loader::init_registry(cwd.clone());
compiler.compile_batch(&py_files, registry);
// Workers inherit registry via CoW after fork
```

### Import Hook Priority

```python
# tach_harness.py - Must be at index 0
sys.meta_path.insert(0, TachMetaPathFinder())
```

---

## Commit Convention

```
feat: implement Phase X feature
fix: resolve issue with Y
docs: update README with Z
test: add gauntlet tests for W
refactor: restructure V for clarity
```

---

## Quick Reference

| Command                                       | Purpose                                       |
| :-------------------------------------------- | :-------------------------------------------- |
| `cargo build`                                 | Dev build                                     |
| `cargo build --release`                       | Release build                                 |
| `cargo test --lib`                            | Unit tests                                    |
| `cargo test --test loader_integration`        | Loader integration                            |
| `./target/release/tach-core --no-isolation .` | Run tests without isolation                   |
| `./target/release/tach-core .`                | Run with full isolation (needs CAP_SYS_ADMIN) |
