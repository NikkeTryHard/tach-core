#!/bin/bash
# Mutation Testing Script for tach-core
# Uses cargo-mutants to verify test effectiveness
#
# Run nightly or before releases: ./scripts/mutation_test.sh
# This verifies that tests actually catch code changes (mutations)

set -euo pipefail

echo "========================================"
echo "Mutation Testing - tach-core"
echo "========================================"
echo ""
echo "WARNING: This may take a long time (30min+ for subset, hours for full)"
echo ""

# Configuration
TIMEOUT="${MUTATION_TIMEOUT:-300}"  # 5 minutes per mutation
JOBS="${MUTATION_JOBS:-4}"
OUTPUT_DIR="${OUTPUT_DIR:-mutants.out}"

# Critical modules to test (subset for faster feedback)
CRITICAL_MODULES=(
    "src/core/config.rs"
    "src/core/protocol.rs"
    "src/discovery/scanner.rs"
    "src/discovery/resolver.rs"
)

# Check if cargo-mutants is installed
if ! command -v cargo-mutants &> /dev/null; then
    echo "Installing cargo-mutants..."
    cargo install cargo-mutants
    echo ""
fi

# Parse arguments
MODE="${1:-subset}"

case "$MODE" in
    subset)
        echo "Mode: SUBSET (critical modules only)"
        echo "Modules:"
        for mod in "${CRITICAL_MODULES[@]}"; do
            echo "  - $mod"
        done
        echo ""

        # Build file arguments
        FILE_ARGS=""
        for mod in "${CRITICAL_MODULES[@]}"; do
            if [ -f "$mod" ]; then
                FILE_ARGS="$FILE_ARGS --file $mod"
            else
                echo "WARNING: $mod not found, skipping"
            fi
        done

        if [ -z "$FILE_ARGS" ]; then
            echo "ERROR: No valid files to test"
            exit 1
        fi

        echo "Running mutation tests..."
        cargo mutants \
            --package tach-core \
            $FILE_ARGS \
            --timeout "$TIMEOUT" \
            --jobs "$JOBS" \
            --output "$OUTPUT_DIR" \
            || true  # Don't fail script on surviving mutants
        ;;

    full)
        echo "Mode: FULL (all source files)"
        echo ""
        echo "Running mutation tests on entire codebase..."
        cargo mutants \
            --package tach-core \
            --timeout "$TIMEOUT" \
            --jobs "$JOBS" \
            --output "$OUTPUT_DIR" \
            || true
        ;;

    quick)
        echo "Mode: QUICK (config.rs only, for CI smoke test)"
        echo ""
        cargo mutants \
            --package tach-core \
            --file "src/core/config.rs" \
            --timeout 120 \
            --jobs 2 \
            --output "$OUTPUT_DIR" \
            || true
        ;;

    *)
        echo "Usage: $0 [subset|full|quick]"
        echo ""
        echo "Modes:"
        echo "  subset  - Test critical modules only (default, ~30min)"
        echo "  full    - Test entire codebase (hours)"
        echo "  quick   - CI smoke test on config.rs only (~5min)"
        exit 1
        ;;
esac

echo ""
echo "========================================"
echo "Mutation Testing Complete"
echo "========================================"
echo ""
echo "Results saved to: $OUTPUT_DIR/"
echo ""

# Parse and display summary if results exist
if [ -f "$OUTPUT_DIR/mutants.json" ]; then
    echo "Summary:"

    # Count outcomes using grep (works without jq)
    TOTAL=$(grep -c '"outcome"' "$OUTPUT_DIR/mutants.json" 2>/dev/null || echo "0")
    CAUGHT=$(grep -c '"Caught"' "$OUTPUT_DIR/mutants.json" 2>/dev/null || echo "0")
    MISSED=$(grep -c '"Missed"' "$OUTPUT_DIR/mutants.json" 2>/dev/null || echo "0")
    TIMEOUT_COUNT=$(grep -c '"Timeout"' "$OUTPUT_DIR/mutants.json" 2>/dev/null || echo "0")

    echo "  Total mutations:  $TOTAL"
    echo "  Caught (good):    $CAUGHT"
    echo "  Missed (bad):     $MISSED"
    echo "  Timeouts:         $TIMEOUT_COUNT"
    echo ""

    if [ "$MISSED" -gt 0 ]; then
        echo "WARNING: $MISSED mutations survived - tests may not be thorough enough"
        echo "Check $OUTPUT_DIR/mutants.json for details on surviving mutants"
    else
        echo "All mutations were caught by tests!"
    fi
fi

echo ""
echo "For detailed HTML report, run:"
echo "  cargo mutants --open"
