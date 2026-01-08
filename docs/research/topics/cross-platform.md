# Cross-Platform Process Cloning

> **Status:** Future work (CHANGELOG 0.8.x+)
> **Deep Dive:** [Cross-Platform Process Cloning Research](../papers-very-verbose/Cross-Platform%20Process%20Cloning%20Research.txt)

---

## Overview

Linux `fork()` provides Copy-on-Write (CoW) semantics enabling sub-10ms worker spawns. Neither macOS nor Windows natively supports this paradigm.

> Source: "The Darwin kernel (XNU) and the Windows NT kernel utilize fundamentally different process creation paradigms that were not designed with the optimization of runtime cloning as a primary objective." [Paper, Section 1]

**Core Challenge:** Replicate Linux's Zygote pattern performance (<10ms startup) on non-Linux platforms without kernel-level CoW support.

---

## macOS (Darwin)

### Key Primitives

| Primitive                                     | Purpose                                         |
| --------------------------------------------- | ----------------------------------------------- |
| `mach_vm_remap`                               | Map memory from another task with CoW semantics |
| `posix_spawn` + `POSIX_SPAWN_START_SUSPENDED` | Create BSD process in suspended state           |
| `task_for_pid()`                              | Acquire Mach task port for memory surgery       |
| `thread_get_state` / `thread_set_state`       | Transfer register state between processes       |

### Recommended Strategy: Suspended Spawn + Remap

1. `posix_spawn` with `POSIX_SPAWN_START_SUSPENDED` - creates valid PID
2. `task_for_pid()` to get task port (requires entitlement)
3. `mach_vm_remap` with `VM_FLAGS_OVERWRITE` and `copy=TRUE` for CoW
4. `thread_set_state` to transfer register context
5. `task_resume` to start execution

> Source: "This hybrid approach leverages the BSD subsystem for process lifecycle management while utilizing Mach primitives for high-performance memory cloning." [Paper, Section 2.2.1]

### Why Not `task_create`?

Creates a "bare" Mach task with no BSD identity (no PID, no file descriptors). Python would crash on any POSIX syscall.

> Source: "A Python interpreter running inside a raw Mach task would immediately crash upon attempting any POSIX system call." [Paper, Section 2.2]

---

## Windows (NT)

### Key Primitives

| Primitive                                | Purpose                                         |
| ---------------------------------------- | ----------------------------------------------- |
| `NtCreateProcessEx`                      | Legacy POSIX fork (creates zombie - no threads) |
| `RtlCloneUserProcess`                    | Modern fork with thread cloning                 |
| `NtCreateSection` / `NtMapViewOfSection` | Section Objects for shared memory               |
| `PAGE_WRITECOPY`                         | Manual CoW via memory protection                |
| Job Objects                              | Lifecycle management (kill-on-close)            |

### The Lock Inheritance Problem

`RtlCloneUserProcess` clones only the calling thread. Mutexes held by other threads remain locked in the child, causing deadlocks.

> Source: "This leads to immediate deadlocks if the child attempts to allocate memory or call generic Win32 APIs. This is the classic 'fork-safety' problem." [Paper, Section 4.2]

### Recommended Strategy: Section Objects + Manual CoW

1. Zygote creates `NtCreateSection` backed by paging file for Python heap
2. Workers spawn via standard `CreateProcess` (clean Win32 process)
3. Workers map section with `PAGE_WRITECOPY` protection
4. OS handles CoW at page level automatically

> Source: "This architecture avoids the CPU overhead of parsing and loading Python modules in the worker, as the data structures are already present in the mapped memory." [Paper, Section 5.1]

### Job Objects for Cleanup

```rust
// Essential flags for worker lifecycle
JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE  // Kill all workers if supervisor dies
```

> Source: "The lifecycle of the workers is cryptographically tied to the Job handle." [Paper, Section 5.2]

---

## Micro-VMs: Not Viable for <10ms

### Latency Analysis

| Framework                                | Boot Latency | Notes                           |
| ---------------------------------------- | ------------ | ------------------------------- |
| Virtualization.framework                 | 150-300ms    | virtio overhead, Swift bridging |
| Hypervisor.framework (Firecracker-style) | ~100-125ms   | Context switch overhead         |
| **Target**                               | **<10ms**    | Required for test isolation     |

> Source: "The analysis conclusively indicates that neither framework can currently achieve <10ms startup times for a fresh VM boot sequence." [Paper, Section 3.3]

**Verdict:** Userspace cloning via `mach_vm_remap` remains the only viable path on macOS.

---

## Security Considerations

### macOS Entitlements

| Requirement                         | Impact                                |
| ----------------------------------- | ------------------------------------- |
| `com.apple.security.get-task-allow` | Required for `task_for_pid()`         |
| System Integrity Protection (SIP)   | May block task port acquisition       |
| Hardened Runtime                    | Strips entitlements in release builds |

> Source: "This operation requires the com.apple.security.get-task-allow entitlement. While standard in debug builds, this entitlement is stripped in release distributions." [Paper, Section 2.2.1]

### Windows Considerations

- `NtCreateProcessEx` / `RtlCloneUserProcess` are undocumented APIs
- May trigger EDR/security software alerts
- Section Objects require careful handle inheritance

---

## Implementation in Tach

**Target Version:** 0.8.x+ (future roadmap)

| Phase | Platform | Key Work                                                           |
| ----- | -------- | ------------------------------------------------------------------ |
| 1     | macOS    | `mach_vm_remap` FFI, suspended spawn, `mach_vm_region` enumeration |
| 2     | Windows  | Section Objects, Job Objects, ConPTY integration                   |

**Dependencies:** `portable-pty` crate, custom allocator hooking for Section Object backing

---

## Key References

> Source: [Apple vm_remap Documentation](https://developer.apple.com/documentation/kernel/1585336-vm_remap)

> Source: [Hunt and Hackett - Process Cloning on Windows](https://github.com/huntandhackett/process-cloning)

> Source: [Chrome PartitionAlloc Design](https://chromium.googlesource.com/chromium/src/+/master/base/allocator/partition_allocator/PartitionAlloc.md)

> Source: [portable_pty crate](https://docs.rs/portable-pty) | [Microsoft ConPTY](https://learn.microsoft.com/en-us/windows/console/creating-a-pseudoconsole-session)

## Summary

| Platform | Viable Approach                    | Expected Latency       |
| -------- | ---------------------------------- | ---------------------- |
| Linux    | Native `fork()` / `clone()`        | <10ms                  |
| macOS    | `posix_spawn` + `mach_vm_remap`    | ~10-20ms (theoretical) |
| Windows  | Section Objects + `PAGE_WRITECOPY` | ~20-50ms (theoretical) |

Micro-VMs are not viable for Tach's latency requirements. Userspace cloning primitives are the only path to approximate Linux `fork()` performance on non-Linux platforms.
