# WSL2 Setup Guide for tach-core

This guide documents WSL2-specific limitations and workarounds for running tach-core.

## Quick Diagnosis

Run this to check your system's feature availability:

```bash
# Check userfaultfd
cat /proc/sys/vm/unprivileged_userfaultfd
# 0 = disabled (needs fix), 1 = enabled

# Check Landlock
cat /sys/kernel/security/landlock/abi_version
# Should show version number, "No such file" = not loaded

# Check kernel config
zcat /proc/config.gz | grep -E "USERFAULTFD|LANDLOCK|SECCOMP"
```

Or run the built-in diagnostics:

```bash
./target/debug/tach-core self-test
```

## Feature Status in WSL2

| Feature     | Purpose            | Typical WSL2 Status                  | Impact if Missing                  |
| ----------- | ------------------ | ------------------------------------ | ---------------------------------- |
| userfaultfd | Memory snapshots   | Compiled in, but disabled by default | Falls back to fork-server (slower) |
| Landlock    | Filesystem sandbox | Compiled in, but LSM not loaded      | No filesystem isolation            |
| Seccomp     | Syscall filtering  | Works                                | N/A                                |
| Namespaces  | Process isolation  | Works                                | N/A                                |
| OverlayFS   | Test isolation     | Works on ext4, issues on /mnt/c/     | Use native Linux paths             |

## Workarounds

### 1. Enable userfaultfd (Highest Priority)

userfaultfd enables memory snapshots for sub-millisecond test isolation reset.

#### Option A: Temporary (until WSL restart)

```bash
sudo sysctl -w vm.unprivileged_userfaultfd=1
```

#### Option B: Persistent via .wslconfig

Create or edit `C:\Users\<YourUsername>\.wslconfig` on Windows:

```ini
[wsl2]
kernelCommandLine = sysctl.vm.unprivileged_userfaultfd=1
```

Then restart WSL from PowerShell:

```powershell
wsl --shutdown
```

#### Option C: Startup script

Add to `~/.bashrc` or create `/etc/profile.d/tach.sh`:

```bash
if [ -f /proc/sys/vm/unprivileged_userfaultfd ]; then
    current=$(cat /proc/sys/vm/unprivileged_userfaultfd)
    if [ "$current" = "0" ]; then
        sudo sysctl -w vm.unprivileged_userfaultfd=1 >/dev/null 2>&1
    fi
fi
```

### 2. Enable Landlock LSM

Landlock provides filesystem sandboxing. Microsoft's WSL2 kernel has it compiled in but doesn't load it by default.

#### Option A: Add LSM to kernel command line

Edit `C:\Users\<YourUsername>\.wslconfig`:

```ini
[wsl2]
kernelCommandLine = lsm=landlock,lockdown,yama,integrity,apparmor,bpf sysctl.vm.unprivileged_userfaultfd=1
```

Restart WSL:

```powershell
wsl --shutdown
```

#### Option B: Build custom WSL2 kernel

For full control, build a custom kernel:

```bash
# Clone Microsoft's kernel source
git clone --depth 1 https://github.com/microsoft/WSL2-Linux-Kernel.git
cd WSL2-Linux-Kernel

# Use Microsoft's config as base
cp Microsoft/config-wsl .config

# Enable Landlock in menuconfig
make menuconfig
# Navigate to: Security options -> Landlock support
# Ensure it's set to [*] (built-in) and in LSM stack

# Build
make -j$(nproc) bzImage

# Copy to Windows-accessible location
cp arch/x86/boot/bzImage /mnt/c/Users/<YourUsername>/wsl-kernel
```

Then edit `.wslconfig`:

```ini
[wsl2]
kernel=C:\\Users\\<YourUsername>\\wsl-kernel\\bzImage
kernelCommandLine = lsm=landlock,lockdown,yama,integrity,apparmor,bpf sysctl.vm.unprivileged_userfaultfd=1
```

### 3. Filesystem Considerations

#### Use Native ext4 Paths

WSL2 performance is much better on the native ext4 filesystem:

```bash
# Good - native ext4
/home/username/dev/project

# Bad - Windows filesystem via 9P (slow, OverlayFS issues)
/mnt/c/Users/username/projects
```

If your project is on `/mnt/c/`, consider moving it:

```bash
mv /mnt/c/Users/username/project ~/dev/
```

### 4. Docker Alternative

Run tach-core inside a Docker container with elevated privileges:

```bash
docker run -it --privileged \
  -v $(pwd):/workspace \
  -w /workspace \
  ubuntu:24.04 bash

# Inside container, kernel features work normally
apt update && apt install -y python3 python3-pip
pip install pytest
# ... build and run tach-core
```

### 5. Accept Graceful Degradation

tach-core is designed to handle missing features gracefully:

- **Without userfaultfd**: Uses fork-server pattern (no snapshots, slower but works)
- **Without Landlock**: Logs warning, continues without filesystem sandbox
- **Without Seccomp**: Only affects safe workers (toxic workers bypass it anyway)

Use `--no-isolation` flag to explicitly disable sandboxing:

```bash
./target/debug/tach-core --no-isolation tests/
```

## Complete .wslconfig Template

Create `C:\Users\<YourUsername>\.wslconfig`:

```ini
[wsl2]
# Enable userfaultfd for memory snapshots
# Enable Landlock LSM for filesystem sandboxing
kernelCommandLine = lsm=landlock,lockdown,yama,integrity,apparmor,bpf sysctl.vm.unprivileged_userfaultfd=1

# Optional: Limit memory/CPU if needed
# memory=8GB
# processors=4

# Optional: Custom kernel path (if you built one)
# kernel=C:\\Users\\<YourUsername>\\wsl-kernel\\bzImage
```

After creating/editing, restart WSL:

```powershell
wsl --shutdown
```

## Verification Script

Save as `~/verify-tach-wsl2.sh`:

```bash
#!/bin/bash
echo "=== tach-core WSL2 Feature Check ==="
echo ""

# userfaultfd
uffd=$(cat /proc/sys/vm/unprivileged_userfaultfd 2>/dev/null)
if [ "$uffd" = "1" ]; then
    echo "[OK] userfaultfd: enabled"
else
    echo "[!!] userfaultfd: DISABLED (run: sudo sysctl -w vm.unprivileged_userfaultfd=1)"
fi

# Landlock
ll_ver=$(cat /sys/kernel/security/landlock/abi_version 2>/dev/null)
if [ -n "$ll_ver" ]; then
    echo "[OK] Landlock: ABI v$ll_ver"
else
    echo "[!!] Landlock: NOT LOADED (add lsm= to .wslconfig kernelCommandLine)"
fi

# Seccomp
if grep -q "CONFIG_SECCOMP=y" /proc/config.gz 2>/dev/null; then
    echo "[OK] Seccomp: enabled"
else
    echo "[??] Seccomp: unknown"
fi

# Filesystem
if [[ "$(pwd)" == /mnt/* ]]; then
    echo "[!!] Filesystem: Windows path (slow) - consider moving to ~/dev/"
else
    echo "[OK] Filesystem: native ext4"
fi

echo ""
echo "Run './target/debug/tach-core self-test' for full diagnostics"
```

## Troubleshooting

### "EPERM on userfaultfd"

userfaultfd is disabled. Fix:

```bash
sudo sysctl -w vm.unprivileged_userfaultfd=1
```

### "Landlock not available"

LSM not loaded. Add to `.wslconfig` kernelCommandLine or accept degraded mode.

### Tests hang or timeout

Possible causes:

- Project on `/mnt/c/` (slow 9P filesystem)
- Insufficient memory allocated to WSL2
- Docker Desktop consuming resources

### Build fails with PyO3 errors

Ensure Python is accessible:

```bash
export PYO3_PYTHON=$(which python3)
cargo build
```

## References

- [Microsoft WSL2 Kernel Source](https://github.com/microsoft/WSL2-Linux-Kernel)
- [WSL Configuration Options](https://learn.microsoft.com/en-us/windows/wsl/wsl-config)
- [Landlock Documentation](https://docs.kernel.org/userspace-api/landlock.html)
- [userfaultfd Documentation](https://www.kernel.org/doc/html/latest/admin-guide/mm/userfaultfd.html)
