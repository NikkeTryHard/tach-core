#!/bin/bash
# Setup git hooks for tach-core development
# This script creates symlinks from .git/hooks to scripts/

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
HOOKS_DIR="$REPO_ROOT/.git/hooks"

echo "[setup-hooks] Installing git hooks..."

# Create hooks directory if it doesn't exist
mkdir -p "$HOOKS_DIR"

# Install pre-commit hook
if [ -f "$SCRIPT_DIR/pre-commit" ]; then
    ln -sf "../../scripts/pre-commit" "$HOOKS_DIR/pre-commit"
    chmod +x "$HOOKS_DIR/pre-commit"
    echo "[setup-hooks] Installed pre-commit hook"
fi

echo "[setup-hooks] Done! Git hooks are now active."
echo ""
echo "The pre-commit hook will automatically:"
echo "  1. Format Rust code with cargo fmt"
echo "  2. Stage any formatted files"
echo "  3. Run clippy (warnings as errors)"
echo "  4. Run unit tests"
echo ""
echo "To bypass hooks in emergencies: git commit --no-verify"
