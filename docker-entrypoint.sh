#!/bin/bash
# tach-core Docker entrypoint
# Sets up Python venv with test dependencies on first run

set -e

VENV_DIR="/workspace/.venv"
MARKER="$VENV_DIR/.tach-deps-installed"

# Create venv if it doesn't exist
if [ ! -d "$VENV_DIR" ]; then
    echo "[tach:docker] Creating Python venv..."
    python3.12 -m venv "$VENV_DIR"
fi

# Install test deps if not already done
if [ ! -f "$MARKER" ]; then
    echo "[tach:docker] Installing test dependencies..."
    "$VENV_DIR/bin/pip" install --upgrade pip -q
    "$VENV_DIR/bin/pip" install -e ".[test]" -q
    touch "$MARKER"
    echo "[tach:docker] Dependencies installed."
fi

# Execute the command
exec "$@"
