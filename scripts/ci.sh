#!/usr/bin/env bash
#
# CI script for plugin stabilization checks
# Runs all tests and validations for the plugin system
#

set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color
BOLD='\033[1m'

# Counters
PASSED=0
FAILED=0
SKIPPED=0

# Track failed steps
FAILED_STEPS=()

# Print section header
section() {
    echo ""
    echo -e "${BLUE}${BOLD}=== $1 ===${NC}"
    echo ""
}

# Print success
success() {
    echo -e "${GREEN}[PASS]${NC} $1"
    ((PASSED++))
}

# Print failure
failure() {
    echo -e "${RED}[FAIL]${NC} $1"
    ((FAILED++))
    FAILED_STEPS+=("$1")
}

# Print skip
skip() {
    echo -e "${YELLOW}[SKIP]${NC} $1"
    ((SKIPPED++))
}

# Print info
info() {
    echo -e "${CYAN}[INFO]${NC} $1"
}

# Run a command and track result
run_check() {
    local name="$1"
    shift
    local cmd="$*"

    info "Running: $cmd"
    if eval "$cmd"; then
        success "$name"
        return 0
    else
        failure "$name"
        return 1
    fi
}

# Main CI pipeline
main() {
    local start_time
    start_time=$(date +%s)

    echo -e "${BOLD}Plugin Stabilization CI Pipeline${NC}"
    echo "Started at: $(date)"
    echo ""

    # Build (release)
    section "Build (Release)"
    run_check "Release build" "cargo build --release" || true

    # Unit tests
    section "Unit Tests"
    run_check "Unit tests" "cargo nextest run --lib" || true

    # Hook tests
    section "Hook Tests"
    run_check "Hook tests" "cargo nextest run -E 'test(hook)'" || true

    # Plugin registry tests
    section "Plugin Registry Tests"
    run_check "Plugin registry tests" "cargo nextest run -E 'test(plugin)'" || true

    # Cache tests
    section "Cache Tests"
    run_check "Cache tests" "cargo nextest run -E 'test(cache)'" || true

    # Gauntlet tests (if binary exists)
    section "Gauntlet Tests"
    if [[ -x "target/release/tach" ]]; then
        run_check "Gauntlet tests" "cargo nextest run --test gauntlet" || true
    else
        skip "Gauntlet tests (release binary not found)"
    fi

    # Benchmark compilation check
    section "Benchmark Compilation"
    run_check "Benchmark compilation" "cargo build --benches" || true

    # Clippy
    section "Clippy Lints"
    run_check "Clippy" "cargo clippy -- -D warnings" || true

    # Format check
    section "Format Check"
    run_check "Format" "cargo fmt --check" || true

    # Summary
    local end_time
    end_time=$(date +%s)
    local duration=$((end_time - start_time))

    section "Summary"
    echo -e "Duration: ${BOLD}${duration}s${NC}"
    echo ""
    echo -e "${GREEN}Passed:${NC}  $PASSED"
    echo -e "${RED}Failed:${NC}  $FAILED"
    echo -e "${YELLOW}Skipped:${NC} $SKIPPED"
    echo ""

    if [[ $FAILED -gt 0 ]]; then
        echo -e "${RED}${BOLD}CI FAILED${NC}"
        echo ""
        echo "Failed steps:"
        for step in "${FAILED_STEPS[@]}"; do
            echo -e "  ${RED}-${NC} $step"
        done
        exit 1
    else
        echo -e "${GREEN}${BOLD}CI PASSED${NC}"
        exit 0
    fi
}

main "$@"
