# Landlock Filesystem Isolation

> **Parent Document**: [isolation-overview.md](isolation-overview.md)
>
> **Purpose**: Detailed documentation of Tach's Landlock filesystem sandboxing implementation.

---

## Overview

Landlock is a Linux Security Module (LSM) that provides unprivileged filesystem sandboxing. Tach uses Landlock to restrict which paths workers can read/write.

---

## ABI Version Strategy

| ABI Version | Kernel | Features                                        | Tach Usage         |
| ----------- | ------ | ----------------------------------------------- | ------------------ |
| V1          | 5.13+  | Basic filesystem access                         | **Primary target** |
| V2          | 5.19+  | `LANDLOCK_ACCESS_FS_REFER` (rename across dirs) | Future             |
| V3          | 6.2+   | `LANDLOCK_ACCESS_FS_TRUNCATE`                   | Future             |
| V4          | 6.7+   | Network restrictions (TCP bind/connect)         | Future             |
| V5          | 6.10+  | `LANDLOCK_ACCESS_FS_IOCTL_DEV`                  | Future             |
| V6          | 6.12+  | Scope controls (IPC restrictions)               | Future             |

**Design Decision**: Tach uses ABI V1 for maximum kernel compatibility (5.13+). Higher ABIs add features but reduce portability.

> **Verified**: Kernel version mappings confirmed via [Linux Kernel Landlock Documentation](https://docs.kernel.org/userspace-api/landlock.html) and [rust-landlock crate documentation](https://docs.rs/landlock/).

---

## Filesystem Access Rules

```mermaid
graph TD
    subgraph "READ-ONLY Access"
        RO1[/usr - System libraries]
        RO2[/lib, /lib64 - Libraries]
        RO3[/bin - Binaries]
        RO4[/etc - Configuration]
        RO5[/dev - Device nodes]
        RO6[/proc - Process info]
        RO7[/sys - Hardware info]
        RO8[/opt - CI environments]
    end

    subgraph "SAFE WRITE Access"
        SW1[project_root - With restrictions]
    end

    subgraph "FULL Access"
        FW1[/tmp - Overlay mount]
        FW2[/run/tach/worker_N - Scratch space]
    end

    subgraph "DENIED"
        D1[Everything else]
    end
```

---

## Safe Write Access Definition

Project root gets "safe write" access that excludes dangerous device/socket creation:

```
Safe write = ReadFile | WriteFile | ReadDir | RemoveDir |
             RemoveFile | MakeDir | MakeReg | MakeSym | Execute
```

**Excluded operations** (for security):

- `MAKE_CHAR` - Character device creation
- `MAKE_BLOCK` - Block device creation
- `MAKE_FIFO` - Named pipe creation
- `MAKE_SOCK` - Unix socket creation

**Security Rationale**: Prevents device node creation escape attacks where a malicious test could create `/dev/sda` inside project_root and access the host disk.

---

## Path Canonicalization

All paths are canonicalized before adding Landlock rules:

- Prevents symlink-based escapes
- Ensures absolute paths
- TOCTOU-safe: uses `PathFd::new()` directly instead of `path.exists()` check

**Key Functions**:

- `apply_landlock` - Main Landlock application
- `add_path_rule` - Add rule for required path (fails if missing)
- `add_path_rule_if_exists` - Add rule for optional path (TOCTOU-safe)

---

## Enforcement Status

```rust
pub enum SandboxStatus {
    FullyEnforced,      // All requested restrictions active
    PartiallyEnforced,  // Some features unavailable (older kernel)
    NotEnforced,        // Landlock not available (< 5.13)
}
```

---

## Graceful Degradation

Landlock is designed to fail gracefully:

- If kernel doesn't support Landlock (< 5.13): log warning, continue
- If specific ABI features unavailable: use best-effort with available features
- The test runner must remain functional on older kernels

```rust
// Graceful degradation pattern
match apply_landlock(project_root, worker_id) {
    Ok(SandboxStatus::FullyEnforced) => { /* Ideal */ }
    Ok(SandboxStatus::NotEnforced) => {
        eprintln!("[worker] WARNING: Landlock not enforced");
    }
    Err(e) => {
        eprintln!("[worker] WARNING: Landlock failed: {}", e);
    }
}
```

---

## External References

- [Linux Kernel Landlock Documentation](https://docs.kernel.org/userspace-api/landlock.html) - Official kernel documentation
- [landlock(7) man page](https://man7.org/linux/man-pages/man7/landlock.7.html) - User-space API reference
- [rust-landlock crate](https://docs.rs/landlock/) - Rust bindings used by Tach

---

_Last Updated: 2026-01-17_
