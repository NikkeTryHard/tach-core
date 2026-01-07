# Fork Safety in Tach

> **Source Papers**: See [Fork Safety of Python C-Extensions](../papers/Fork%20Safety%20of%20Python%20C-Extensions.txt) and [Rust Static Analysis for Toxic Python Modules](../papers/Rust%20Static%20Analysis%20for%20Toxic%20Python%20Modules.txt) for complete analysis.

---

## Overview: The Fork-Safety Paradox

The Unix `fork()` system call was designed for single-threaded processes. When applied to multi-threaded Python applications with C-extensions, it creates a fundamental incompatibility that threatens process stability.

> "The fundamental assumptions of fork()---specifically regarding memory isolation and state duplication---are incompatible with the complex internal threading pools, global state mutexes, and hardware contexts managed by modern C libraries."
> Source: Fork Safety of Python C-Extensions

**The Paradox**: Libraries most valuable to pre-load (NumPy, TensorFlow, database drivers) are precisely those most likely to corrupt state after fork. This directly impacts Tach's Zygote architecture.

**Python 3.12+ Response**: A `DeprecationWarning` is now issued when `os.fork()` is called in a multithreaded process. Python 3.14 will likely change the default multiprocessing start method from `fork` to `spawn`.

---

## The Orphaned Lock Problem

When `fork()` duplicates a multi-threaded process, only the calling thread survives in the child. All other threads vanish without cleanup.

> "If a background thread holds a mutex or lock at the precise nanosecond fork() is invoked, that lock is copied into the child process's memory in a 'locked' state. However, the thread that 'owns' the lock does not exist in the child process."
> Source: Fork Safety of Python C-Extensions

**Consequences**:

- Child waits indefinitely for a non-existent thread to release the lock
- No exception thrown, no traceback generated
- Silent deadlock freezes the child immediately

**POSIX Requirement**: After `fork()` in a multithreaded program, the child may only execute **async-signal-safe** functions until it calls `exec()`. The Python interpreter, `malloc`, and `printf` are NOT async-signal-safe.

**Common Victim**: The `logging` module. If a background thread is writing to a log file during `fork()`, the logging lock is inherited in "acquired" state, deadlocking the first log call in the child.

---

## Toxic Module Detection

Tach uses static AST analysis to classify modules as "safe" or "toxic" before execution. This prevents fork-unsafe modules from corrupting Zygote children.

### Toxicity Categories

| Category          | Pattern                             | Consequence                                          |
| ----------------- | ----------------------------------- | ---------------------------------------------------- |
| **Threading**     | `threading.Thread().start()`        | Thread structures copied but no kernel thread exists |
| **Locking**       | `threading.Lock()` at module level  | Mutex may be inherited in locked state               |
| **IPC**           | `multiprocessing.Pool()`            | Pipes/semaphores corrupted across fork               |
| **Randomness**    | `random.seed()`, `ssl.SSLContext()` | PRNG state duplicated, identical "random" values     |
| **I/O Resources** | `socket.socket()`, `open()`         | FD duplication causes interleaved writes             |

### Static Analysis Approach

> "Identify 'toxic' or 'fork-unsafe' Python modules through static analysis of import graphs."
> Source: Rust Static Analysis for Toxic Python Modules

**Key heuristics**:

1. **Blocklist Imports**: Flag `multiprocessing`, `socket`, `ctypes`, `grpc`, `tkinter`
2. **Top-Level Calls**: Detect `Thread().start()`, `Lock()`, `Pool()` at module scope
3. **Global Assignments**: Flag `MY_LOCK = threading.Lock()` patterns
4. **Scope Analysis**: Only flag code at `scope_depth == 0` (executed on import)
5. **Main Guard**: Skip code inside `if __name__ == "__main__":` blocks

### Transitive Toxicity

> "Toxicity is contagious. If Module A imports Module B, and Module B opens a database connection, then importing Module A effectively opens a database connection."
> Source: Python Monorepo Zygote Tree Design

Tach builds a dependency graph and propagates toxicity status. A module is toxic if:

- It is locally toxic, OR
- It imports a toxic module

---

## C-Extension Risks

The most severe fork-safety violations occur in C-extensions that manage their own threading and resources.

### NumPy / BLAS

> "If the child process attempts a linear algebra operation, the BLAS library checks its internal state, sees an 'initialized' pool, and attempts to dispatch work to the threads. Since the threads do not exist, the dispatch mechanism deadlocks."
> Source: Fork Safety of Python C-Extensions

**Failure Mode**: `np.linalg.inv(A)` in child hangs if parent triggered BLAS initialization.

### TensorFlow / PyTorch

> "TensorFlow is explicitly not fork-safe. The primary point of failure is the interaction with the GPU via CUDA. The CUDA runtime API does not support fork()."
> Source: Fork Safety of Python C-Extensions

**Failure Mode**: GPU memory mapping invalid in child, Eigen thread pool becomes zombie.

> "PyTorch documentation defines the 'Poison Fork' as a scenario where the accelerator runtime (CUDA or OpenMP) is initialized before the fork."
> Source: Fork Safety of Python C-Extensions

### gRPC

> "Historically, gRPC was completely unsafe to fork. The background threads managed by grpc-core would die upon fork, leaving the completion queue in a zombie state."
> Source: Fork Safety of Python C-Extensions

**Mitigation**: `GRPC_ENABLE_FORK_SUPPORT=1` enables `pthread_atfork` handlers, but only works with `epoll1` polling and requires no active RPCs.

### Database Drivers (Psycopg2, Redis)

> "libpq connections are stateful and tied to a socket. Forking duplicates the socket. If the child uses the inherited connection, it injects data into the parent's TCP stream."
> Source: Fork Safety of Python C-Extensions

**SSL Complication**: Encryption context cannot be shared. Results in "SSL error: decryption failed or bad record mac".

---

## Mitigation Strategies

### 1. Use `spawn` Instead of `fork`

```python
import multiprocessing
multiprocessing.set_start_method('spawn')
```

> "The industry-wide migration away from fork toward spawn and forkserver models, a shift formally recognized by the Python Steering Council's deprecation of fork-with-threads in Python 3.12."
> Source: Fork Safety of Python C-Extensions

### 2. Dispose Pattern for Database Connections

```python
# In the child process:
engine.dispose(close=False)  # Discards pool struct without closing parent sockets
```

> "Ensure that any connection pool created in the parent is explicitly discarded (not closed, which kills the parent's socket) in the child process immediately after startup."
> Source: Fork Safety of Python C-Extensions

### 3. Environment Variables for Thread Control

```bash
export OMP_NUM_THREADS=1
export OPENBLAS_NUM_THREADS=1
export MKL_NUM_THREADS=1
```

This prevents BLAS/OpenMP from creating thread pools that corrupt on fork.

### 4. Lazy Loading Pattern

> "The report recommends that 'Toxic' modules are not necessarily banned but must be refactored to use Lazy Loading."
> Source: Rust Static Analysis for Toxic Python Modules

**Toxic Pattern**:

```python
# db.py - WRONG
import redis
CLIENT = redis.Redis()  # Connection created at import
```

**Safe Pattern**:

```python
# db.py - CORRECT
import redis
_CLIENT = None

def get_client():
    global _CLIENT
    if _CLIENT is None:
        _CLIENT = redis.Redis()  # Connection created at first use
    return _CLIENT
```

---

## Implementation in Tach

Tach addresses fork-safety through multiple mechanisms mapped to the development roadmap.

### Toxicity Classification (Current)

Tach classifies tests and their dependencies as safe or toxic at discovery time:

- **Safe Workers**: Full Iron Dome (Landlock + Seccomp), can reuse workers
- **Toxic Workers**: Landlock only (skip Seccomp for subprocess support), must exit after test

> "The result is a binary classification for every module in the monorepo: Safe or Toxic."
> Source: Rust Static Analysis for Toxic Python Modules

### Database Integration (0.3.x)

The 0.3.x series specifically addresses database fork-safety:

> "Injecting SAVEPOINT and ROLLBACK TO SAVEPOINT to make DB tests I/O-free."
> Source: Rust-Python Test Isolation Blueprint

Key features:

- Transaction wrapping with automatic rollback
- Connection pool disposal in child processes
- FD handover via SCM_RIGHTS for connection preservation

See [CHANGELOG.md](../../../CHANGELOG.md) section 0.3.x for complete database roadmap.

### Hierarchical Zygotes (0.4.x)

The 0.4.x series implements hierarchical zygote trees that respect toxicity boundaries:

> "The root node contains universally shared modules (e.g., os, sys). Child nodes branch off to specialize (e.g., a 'Data Science Zygote' adds numpy)."
> Source: Python Monorepo Zygote Tree Design

Toxic modules are excluded from zygote pre-loading. Tests requiring toxic dependencies fork from appropriate safe ancestors.

---

## Quick Reference: Fork-Safety Status

| Library      | Status        | Failure Mode      | Mitigation                   |
| ------------ | ------------- | ----------------- | ---------------------------- |
| NumPy        | Unsafe        | BLAS deadlock     | `OPENBLAS_NUM_THREADS=1`     |
| Pandas       | Unsafe        | Inherits NumPy    | spawn                        |
| TensorFlow   | Unsafe        | CUDA/Eigen zombie | spawn (mandatory)            |
| PyTorch      | Unsafe        | OpenMP/CUDA       | spawn (mandatory)            |
| gRPC         | Conditional   | Completion queue  | `GRPC_ENABLE_FORK_SUPPORT=1` |
| Psycopg2     | Unsafe        | Socket/SSL        | `engine.dispose()`           |
| Redis-py     | Unsafe        | Pool duplication  | Reset pool in child          |
| Cryptography | Safe (Modern) | Historic PRNG     | Update OpenSSL > 1.1.1d      |
| orjson       | Safe          | Stateless Rust    | N/A                          |

---

## Key References

> "The fork() system call was designed in an era of single-threaded programming."
> Source: Rust Static Analysis for Toxic Python Modules, Section 1.3

> "Code at the top level of a module (indentation zero) executes on import. Code inside a function body executes only when called."
> Source: Rust Static Analysis for Toxic Python Modules, Section 4.1

> "The child process inherits a corrupted state. The background thread is dead, but the memory structures indicating it is running remain."
> Source: Rust Static Analysis for Toxic Python Modules, Section 1.3

> "The 'Copy-on-Write' Fallacy: Python utilizes reference counting for memory management. Even reading a Python object requires incrementing its reference count, which is a write operation."
> Source: Fork Safety of Python C-Extensions, Section 2.3

> "Rust, utilizing the rayon data parallelism library, can saturate all CPU cores to parse and analyze thousands of files per second."
> Source: Rust Static Analysis for Toxic Python Modules, Section 3.1

---

## See Also

- [Toxicity Architecture](../../architecture/toxicity.md) - Tach's toxicity classification system
- [Zygote Architecture](../../architecture/zygote.md) - Fork-server implementation details
- [CHANGELOG 0.3.x](../../../CHANGELOG.md) - Database integration roadmap
