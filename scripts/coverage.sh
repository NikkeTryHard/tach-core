#!/bin/bash
# Regression Prevention Coverage Script
# Runs cargo-llvm-cov and enforces 90% threshold

set -euo pipefail

THRESHOLD="${COVERAGE_THRESHOLD:-90}"
OUTPUT_DIR="${OUTPUT_DIR:-coverage}"

echo "========================================"
echo "Regression Prevention Coverage Check"
echo "========================================"
echo "Threshold: ${THRESHOLD}%"
echo ""

# Ensure output directory exists
mkdir -p "$OUTPUT_DIR"

# Check if cargo-llvm-cov is installed
if ! command -v cargo-llvm-cov &> /dev/null; then
    echo "Installing cargo-llvm-cov..."
    cargo install cargo-llvm-cov
fi

# Ensure llvm-tools are installed
rustup component add llvm-tools-preview 2>/dev/null || true

# Run coverage with all features
echo "Running tests with coverage..."
cargo llvm-cov --all-features --workspace \
    --lcov --output-path "$OUTPUT_DIR/lcov.info" \
    --html --output-dir "$OUTPUT_DIR/html" \
    2>&1 | tee "$OUTPUT_DIR/coverage.log"

# Extract coverage percentage
echo ""
echo "Extracting coverage summary..."
SUMMARY=$(cargo llvm-cov --all-features --workspace --summary-only 2>/dev/null || echo "")

if [ -z "$SUMMARY" ]; then
    echo "WARNING: Could not extract coverage summary"
    COVERAGE="0"
else
    # Extract the line coverage percentage
    COVERAGE=$(echo "$SUMMARY" | grep -oP 'lines:\s+\K[\d.]+(?=%)' || echo "0")
fi

echo ""
echo "========================================"
echo "COVERAGE RESULTS"
echo "========================================"
echo "Line Coverage: ${COVERAGE}%"
echo "Threshold:     ${THRESHOLD}%"
echo ""

# Compare coverage to threshold
if command -v bc &> /dev/null; then
    PASS=$(echo "$COVERAGE >= $THRESHOLD" | bc -l)
else
    # Fallback: integer comparison
    COVERAGE_INT=${COVERAGE%.*}
    THRESHOLD_INT=${THRESHOLD%.*}
    if [ "${COVERAGE_INT:-0}" -ge "${THRESHOLD_INT:-90}" ]; then
        PASS=1
    else
        PASS=0
    fi
fi

if [ "$PASS" -eq 1 ]; then
    echo "PASS: Coverage ${COVERAGE}% meets threshold ${THRESHOLD}%"
    echo "========================================"
    exit 0
else
    echo "FAIL: Coverage ${COVERAGE}% is below threshold ${THRESHOLD}%"
    echo "========================================"
    exit 1
fi
