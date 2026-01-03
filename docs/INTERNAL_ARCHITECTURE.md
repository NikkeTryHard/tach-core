# Internal Architecture: The Physics of Restoration

> **Status**: Phase 2.1 - Deep State Integration Complete
> **Author**: Project Tach Development Team
> **Purpose**: Define restoration invariants and document allocator-specific state locations

---

## Executive Summary

This document bridges the **EPERM Doctrine** (security enforcement) with the **Physics of Restoration** (memory snapshot/restore). It defines the invariants that must hold for successful test isolation and documents the critical memory locations that must be synchronized during restoration.

---

## The Restoration Triad

Successful memory restoration requires the synchronization of three interdependent memory regions:

```mermaid
graph TB
    subgraph "THE RESTORATION TRIAD"
        TCB[Thread Control Block<br/>fs_base register]
        BSS[BSS Segment<br/>.data/.bss in libpython]
        HEAP[Heap Segment<br/>PyObject allocations]
    end

    TCB -->|"mi_heap_t pointer"| HEAP
    BSS -->|"PyFloat_FreeList head"| HEAP
    HEAP -->|"next pointers"| HEAP

    subgraph "FAILURE MODES"
        F1[TCB stale → use-after-free]
        F2[BSS stale → double-free]
        F3[HEAP stale → dangling pointers]
    end

    TCB -.->|"If not restored"| F1
    BSS -.->|"If not restored"| F2
    HEAP -.->|"If not restored"| F3
```

---

## Restoration Invariants

### Invariant 1: Bit-Perfect Alignment

A successful restore is **NOT** just "no crash." It is a **bit-perfect** alignment of:

| Component | Location                      | Validation                           |
| --------- | ----------------------------- | ------------------------------------ |
| **TCB**   | `fs_base` register            | `self_ptr == fs_base`                |
| **BSS**   | libpython .data/.bss segments | `sha256(restored) == sha256(golden)` |
| **Heap**  | Anonymous mappings + `[heap]` | `sha256(restored) == sha256(golden)` |

### Invariant 2: Pointer Consistency

All pointers from BSS → Heap must point to valid, restored objects:

```
BEFORE RESTORE:
  BSS: PyFloat_FreeList → 0x7f1234560000 (heap object A)
  Heap: Object A at 0x7f1234560000, next → Object B

AFTER RESTORE (CORRECT):
  BSS: PyFloat_FreeList → 0x7f1234560000 (SAME address)
  Heap: Object A at 0x7f1234560000 (RESTORED content)

AFTER RESTORE (FAILURE):
  BSS: PyFloat_FreeList → 0x7f1234560000 (old address)
  Heap: 0x7f1234560000 is ZEROED (MADV_DONTNEED zapped it)
  RESULT: Next float allocation follows NULL/garbage pointer → SIGSEGV
```

### Invariant 3: TLS Synchronization

Thread Local Storage must be restored alongside Heap when using allocators that cache state in TLS (mimalloc in Python 3.13+):

```
TCB at fs_base:
  +0x0ad8: mi_heap_t* → points into anonymous heap region
  +0x0ae0: mi_tld_t* → thread-local data
  ...

If Heap is restored but TLS is not:
  mi_heap_t* points to RESTORED memory
  BUT mi_heap_t->pages still references STALE page list
  RESULT: Allocator returns memory that was "freed" in snapshot
```

### Invariant 4: GC Stability

Post-restoration, the garbage collector must be able to traverse all objects without fault:

```python
# Verification: Run 100 times without SIGSEGV
for _ in range(100):
    gc.collect()
```

If any of the following occur, restoration has FAILED:

- `SIGSEGV` (invalid pointer dereference)
- `SIGBUS` (unaligned access)
- Python exception from gc internals
- Memory leak detected by gc

---

## The mimalloc Offset Registry

Python 3.13 uses mimalloc as its memory allocator. mimalloc stores thread-local state at fixed offsets from `fs_base`.

### Discovered Offsets (Python 3.13, x86_64, glibc)

| Offset from fs_base | Structure    | Description                         |
| ------------------- | ------------ | ----------------------------------- |
| `+0x0ad8`           | `mi_heap_t*` | **Primary heap pointer** (CRITICAL) |
| `+0x0ae0`           | `mi_tld_t*`  | Thread-local data                   |
| `+0x0af8`           | Unknown      | Secondary heap reference            |
| `+0x0b00`           | Unknown      | Page list pointer                   |
| `+0x0b20`           | Unknown      | Segment metadata                    |
| `+0x0b40`           | Unknown      | Segment base                        |
| `+0x0b60`           | Unknown      | Free list cache                     |
| `+0x0b80`           | Unknown      | Free list cache                     |

### Version Compatibility Matrix

| Python Version | Allocator | TLS Offsets               | Status     |
| -------------- | --------- | ------------------------- | ---------- |
| 3.11.x         | pymalloc  | N/A (no TLS caching)      | Safe       |
| 3.12.x         | pymalloc  | N/A (no TLS caching)      | Safe       |
| 3.13.x         | mimalloc  | `fs_base+0xad8` (primary) | **HAZARD** |
| 3.14.x         | TBD       | TBD                       | Unknown    |

### Detection Method

The mimalloc heartbeats are detected by:

1. Reading `fs_base` via `arch_prctl(ARCH_GET_FS)`
2. Parsing `/proc/self/maps` to identify TLS region
3. Scanning TLS for pointers that target `[heap]` or anonymous regions
4. Recording offsets where valid heap pointers are found

See `experiments/tls_python_poc.rs` for the implementation.

---

## The Split-Brain Hazard

### Definition

The **Split-Brain Hazard** occurs when BSS and Heap are restored independently, leaving cross-segment pointers in an inconsistent state.

```mermaid
sequenceDiagram
    participant BSS as BSS (.data)
    participant Heap as Heap
    participant GC as gc.collect()

    Note over BSS,Heap: GOLDEN SNAPSHOT
    BSS->>Heap: FreeList head → Object A
    Heap->>Heap: Object A.next → Object B

    Note over BSS,Heap: TEST EXECUTION (DIRTY)
    BSS->>Heap: FreeList head → Object C (new)
    Heap->>Heap: Object C.next → Object D (new)

    Note over BSS,Heap: RESTORE (INCORRECT)
    BSS->>BSS: Restored to → Object A
    Heap->>Heap: NOT restored (still Object C, D)

    GC->>BSS: Read FreeList head
    BSS-->>GC: Returns Object A address
    GC->>Heap: Access Object A
    Heap-->>GC: SIGSEGV (Object A doesn't exist)
```

### Mitigation

The Split-Brain Hazard is mitigated by:

1. **Atomic Restoration**: Both BSS and Heap are invalidated in a single `madvise(MADV_DONTNEED)` pass
2. **userfaultfd Handling**: All page faults are resolved from the golden snapshot
3. **Validation**: Post-restore GC stress test (100 iterations)

---

## The Free List Architecture

### PyFloat_FreeList (Example)

Python caches freed `PyFloatObject` instances in a singly-linked free list:

```c
// In Objects/floatobject.c
static PyFloatObject *free_list = NULL;  // BSS segment
static int numfree = 0;                   // BSS segment

// When a float is freed:
void float_dealloc(PyFloatObject *op) {
    op->ob_type = (PyTypeObject *)free_list;  // Heap modification
    free_list = op;                            // BSS modification
    numfree++;
}

// When a float is allocated:
PyFloatObject *float_alloc(void) {
    if (free_list != NULL) {
        PyFloatObject *op = free_list;              // Read from BSS
        free_list = (PyFloatObject *)op->ob_type;   // BSS modification
        numfree--;
        return op;  // Return from Heap
    }
    return PyObject_Malloc(sizeof(PyFloatObject));
}
```

### Restoration Requirement

For correct restoration:

| Segment | Must Contain                                         |
| ------- | ---------------------------------------------------- |
| BSS     | `free_list` pointer from golden snapshot             |
| Heap    | The exact `PyFloatObject` that `free_list` points to |

If BSS is restored but Heap is not:

- `free_list` points to golden address (e.g., `0x7f1234560000`)
- But that address now contains post-test data
- Next `float_alloc()` returns corrupted object

---

## Validation Strategy

### The Memory Invariant Test

Located at: `rust_tests/memory_invariant.rs`

```mermaid
flowchart TB
    subgraph Phase1["WARMUP"]
        W1[Initialize Python]
        W2[Allocate 1000 floats]
        W3[Delete floats → populate FreeList]
    end

    subgraph Phase2["SNAPSHOT"]
        S1[SIGSTOP]
        S2[Supervisor captures golden]
        S3[SIGCONT]
    end

    subgraph Phase3["DIRTY"]
        D1[Allocate 500 more floats]
        D2[Mutate heap]
        D3[BSS and Heap now diverged]
    end

    subgraph Phase4["RESTORE"]
        R1[madvise MADV_DONTNEED]
        R2[Access triggers UFFD]
        R3[Supervisor restores golden]
    end

    subgraph Phase5["VERIFY"]
        V1[Run gc.collect 100x]
        V2[Allocate 100 floats]
        V3[Access all floats]
        V4[No SIGSEGV = PASS]
    end

    Phase1 --> Phase2 --> Phase3 --> Phase4 --> Phase5
```

### Success Criteria

| Metric                      | Target            | Validation Method                |
| --------------------------- | ----------------- | -------------------------------- |
| **Bit-Perfect Restoration** | sha256 match      | Compare memory ranges            |
| **GC Stability**            | 100x gc.collect() | No SIGSEGV                       |
| **Float Allocation**        | Success           | Allocate 100 floats post-restore |
| **Latency**                 | <500μs for 1GB    | Benchmark restoration time       |

---

## Security Integration

### From EPERM Doctrine to Physics

The EPERM Doctrine (documented in `docs/security/sandbox-enforcement.md`) ensures that workers cannot escape their sandbox. The Physics of Restoration ensures that workers cannot corrupt each other through stale memory state.

```mermaid
graph LR
    subgraph Security["EPERM DOCTRINE"]
        S1[Seccomp blocks syscalls]
        S2[Landlock blocks filesystem]
        S3[PID namespace isolates processes]
    end

    subgraph Physics["RESTORATION PHYSICS"]
        P1[userfaultfd captures faults]
        P2[Golden snapshot provides source of truth]
        P3[TLS restoration prevents allocator desync]
    end

    S1 --> P1
    S2 --> P2
    S3 --> P3

    subgraph Result["IRON DOME"]
        R1[Workers cannot escape]
        R2[Workers cannot corrupt]
        R3[Workers are perfectly recyclable]
    end

    Security --> Result
    Physics --> Result
```

---

## Future Work

### Phase 2.2: TLS Restoration Implementation

1. **Capture TLS** during golden snapshot alongside BSS/Heap
2. **Restore TLS** via `ptrace(PTRACE_ARCH_PRCTL, ARCH_SET_FS)` or direct memory write
3. **Validate** mimalloc state after restoration

### Phase 2.3: Multi-Version Support

1. **Detect Python version** at runtime
2. **Load appropriate offset registry** for that version
3. **Skip TLS restoration** for pre-3.13 (pymalloc doesn't use TLS)

### Phase 3: Performance Optimization

1. **Lazy TLS capture**: Only snapshot TLS offsets that contain heap pointers
2. **COW optimization**: Use `userfaultfd(UFFD_FEATURE_MINOR_HUGETLBFS)` for huge pages
3. **Batched restoration**: Group page faults for reduced syscall overhead

---

## References

- `docs/security/sandbox-enforcement.md` - EPERM Doctrine
- `docs/ci/self-hosted-runner.md` - CI infrastructure requirements
- `experiments/tls_python_poc.rs` - mimalloc TLS detection
- `rust_tests/memory_invariant.rs` - BSS/Heap validation test
- `src/isolation/snapshot.rs` - Snapshot manager implementation

---

_"The Iron Dome is only as strong as its weakest pointer."_

_Project Tach Internal Architecture Standard_
