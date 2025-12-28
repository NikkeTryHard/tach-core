#!/bin/bash
# Safe test runner with CGROUP ISOLATION to prevent OOM from killing session
#
# PROBLEM: OOM killer targets your display manager, logging you out.
# SOLUTION: Isolate cargo in its own cgroup. If it exceeds memory, only cargo dies.

set -e

# Memory limits (adjust based on your system)
# Your system: 7.7GB RAM, ~4GB in use, ~3.5GB available
# Safe limit: 2.5GB for cargo (leaves 5GB for system + session)
MEMORY_MAX="2500M"
SWAP_MAX="1G"

echo "=== Cgroup-Isolated Test Runner ==="
echo "Memory limit: $MEMORY_MAX (swap: $SWAP_MAX)"
echo ""

# Show memory before
echo "Memory before:"
free -h | grep -E "^Mem|^Swap"
echo ""

# Build the command
CARGO_CMD="cargo test $@ -- --test-threads=1"
echo "Running: $CARGO_CMD"
echo ""

# Run with systemd-run for cgroup isolation
# --user: Run in user scope (no sudo needed)
# --scope: Run as a scope unit (transient)
# -p MemoryMax: Hard memory limit
# -p MemorySwapMax: Swap limit
# --description: For identification in systemctl
systemd-run --user --scope \
    -p MemoryMax="$MEMORY_MAX" \
    -p MemorySwapMax="$SWAP_MAX" \
    --description="cargo-test-isolated" \
    $CARGO_CMD

EXIT_CODE=$?

echo ""
echo "Memory after:"
free -h | grep -E "^Mem|^Swap"

if [ $EXIT_CODE -eq 0 ]; then
    echo ""
    echo "=== Tests completed successfully ==="
else
    echo ""
    echo "=== Tests failed with exit code: $EXIT_CODE ==="
    if [ $EXIT_CODE -eq 137 ]; then
        echo "Process was killed (likely OOM within cgroup - but your session is safe!)"
    fi
fi

exit $EXIT_CODE
