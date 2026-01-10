#!/bin/bash
# Post-create setup for tach-core dev container
# Runs once after the container is created

set -e

echo "=== tach-core Dev Container Setup ==="

# Create Python virtual environment if it doesn't exist
if [ ! -d ".venv" ]; then
    echo "Creating Python virtual environment..."
    python3.12 -m venv .venv
fi

# Activate and install Python dependencies
echo "Installing Python dependencies..."
source .venv/bin/activate
pip install --upgrade pip
pip install pytest

# Ensure cargo is in PATH
source /root/.cargo/env

# Build tach-core (release for faster test runs)
echo "Building tach-core..."
cargo build --release

# Run self-test to verify kernel features
echo ""
echo "=== Verifying kernel features ==="
./target/release/tach-core self-test

echo ""
echo "=== Setup complete! ==="
echo "Run 'source .venv/bin/activate' to activate Python environment"
echo "Run 'cargo build' to rebuild"
echo "Run 'pytest tests/gauntlet/' to run tests"
