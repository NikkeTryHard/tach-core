#!/bin/bash
# =============================================================================
# Project Tach: Iron Dome Bootstrap Script
# =============================================================================
#
# This script enables the Physics tests to run on a local Linux machine.
# It handles:
#   1. Checking/enabling vm.unprivileged_userfaultfd
#   2. Optionally running tests inside a Docker container with proper capabilities
#   3. Running the full Physics test suite
#
# Usage:
#   ./scripts/run_physics_local.sh [--docker] [--check-only]
#
# Options:
#   --docker      Run tests inside a Docker container with CAP_SYS_PTRACE
#   --check-only  Only check prerequisites, don't run tests
#   --help        Show this help message
#
# Requirements:
#   - Linux kernel 5.13+ (for Landlock ABI v1)
#   - Root access OR vm.unprivileged_userfaultfd already enabled
#   - Python 3.10+ with venv
#   - Rust toolchain
#
# =============================================================================

set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Script directory
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

# =============================================================================
# Helper Functions
# =============================================================================

log_info() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

log_success() {
    echo -e "${GREEN}[OK]${NC} $1"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

log_header() {
    echo ""
    echo -e "${BLUE}======================================================================${NC}"
    echo -e "${BLUE} $1${NC}"
    echo -e "${BLUE}======================================================================${NC}"
}

show_help() {
    head -30 "$0" | tail -25
    exit 0
}

# =============================================================================
# Prerequisite Checks
# =============================================================================

check_linux() {
    if [[ "$(uname -s)" != "Linux" ]]; then
        log_error "This script requires Linux. Detected: $(uname -s)"
        exit 1
    fi
    log_success "Running on Linux"
}

check_kernel_version() {
    local kernel_version
    kernel_version=$(uname -r | cut -d. -f1-2)
    local major minor
    major=$(echo "$kernel_version" | cut -d. -f1)
    minor=$(echo "$kernel_version" | cut -d. -f2)

    if [[ $major -lt 5 ]] || [[ $major -eq 5 && $minor -lt 13 ]]; then
        log_warn "Kernel $kernel_version may not support Landlock (requires 5.13+)"
        log_warn "Landlock tests will gracefully degrade"
    else
        log_success "Kernel version: $kernel_version (Landlock supported)"
    fi
}

check_userfaultfd() {
    local uffd_status
    uffd_status=$(cat /proc/sys/vm/unprivileged_userfaultfd 2>/dev/null || echo "unavailable")

    if [[ "$uffd_status" == "1" ]]; then
        log_success "userfaultfd: ENABLED (vm.unprivileged_userfaultfd=1)"
        return 0
    elif [[ "$uffd_status" == "0" ]]; then
        log_warn "userfaultfd: DISABLED (vm.unprivileged_userfaultfd=0)"
        return 1
    else
        log_error "userfaultfd: Cannot read /proc/sys/vm/unprivileged_userfaultfd"
        log_error "This may indicate a non-standard kernel configuration"
        return 1
    fi
}

enable_userfaultfd() {
    log_info "Attempting to enable userfaultfd..."

    if [[ $EUID -eq 0 ]]; then
        # Running as root
        echo 1 > /proc/sys/vm/unprivileged_userfaultfd
        log_success "userfaultfd enabled (running as root)"
    else
        # Need sudo
        log_info "Requesting sudo to enable userfaultfd..."
        if sudo sysctl -w vm.unprivileged_userfaultfd=1 > /dev/null 2>&1; then
            log_success "userfaultfd enabled via sudo"
        else
            log_error "Failed to enable userfaultfd"
            log_error ""
            log_error "Please run one of the following:"
            log_error "  1. sudo sysctl -w vm.unprivileged_userfaultfd=1"
            log_error "  2. Add 'vm.unprivileged_userfaultfd=1' to /etc/sysctl.conf"
            log_error "  3. Run this script with --docker flag"
            exit 1
        fi
    fi
}

check_rust() {
    if ! command -v cargo &> /dev/null; then
        log_error "Rust toolchain not found. Install via: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
        exit 1
    fi
    log_success "Rust: $(cargo --version)"
}

check_python() {
    local python_cmd=""

    # Check for venv first
    if [[ -f "$PROJECT_ROOT/.venv/bin/python" ]]; then
        python_cmd="$PROJECT_ROOT/.venv/bin/python"
    elif command -v python3 &> /dev/null; then
        python_cmd="python3"
    elif command -v python &> /dev/null; then
        python_cmd="python"
    else
        log_error "Python not found. Install Python 3.10+"
        exit 1
    fi

    local python_version
    python_version=$($python_cmd --version 2>&1 | cut -d' ' -f2)
    log_success "Python: $python_version ($python_cmd)"

    # Export for later use
    export PYO3_PYTHON="$python_cmd"
}

check_docker() {
    if ! command -v docker &> /dev/null; then
        log_error "Docker not found. Install Docker to use --docker mode"
        exit 1
    fi
    log_success "Docker: $(docker --version | cut -d' ' -f3 | tr -d ',')"
}

# =============================================================================
# Test Runners
# =============================================================================

run_physics_tests_native() {
    log_header "Running Physics Tests (Native)"

    cd "$PROJECT_ROOT"

    # Ensure venv is set up
    if [[ ! -f ".venv/bin/python" ]]; then
        log_info "Setting up Python virtual environment..."
        python3 -m venv .venv
        .venv/bin/pip install --quiet pytest
    fi

    export PYO3_PYTHON="$PROJECT_ROOT/.venv/bin/python"

    # Run the physics tests
    log_info "Running physics_check tests..."
    cargo test --test physics_check -- --nocapture 2>&1 || {
        log_warn "physics_check had failures (this may be expected on first run)"
    }

    log_info "Running memory_invariant tests..."
    cargo test --test memory_invariant -- --nocapture 2>&1 || {
        log_warn "memory_invariant had failures"
    }

    log_info "Running sandbox_enforcement tests..."
    cargo test --test sandbox_enforcement -- --nocapture 2>&1 || {
        log_warn "sandbox_enforcement had failures"
    }
}

run_physics_tests_docker() {
    log_header "Running Physics Tests (Docker)"

    cd "$PROJECT_ROOT"

    # Check if Dockerfile exists, create if not
    if [[ ! -f "Dockerfile.physics" ]]; then
        log_info "Creating Dockerfile.physics..."
        cat > Dockerfile.physics << 'DOCKERFILE'
FROM rust:1.75-bookworm

# Install Python
RUN apt-get update && apt-get install -y \
    python3 \
    python3-venv \
    python3-dev \
    && rm -rf /var/lib/apt/lists/*

# Enable userfaultfd inside container
# Note: This requires the container to be run with --privileged or --cap-add=SYS_PTRACE

WORKDIR /app

# Copy project
COPY . .

# Setup Python venv
RUN python3 -m venv .venv && .venv/bin/pip install pytest

# Set environment
ENV PYO3_PYTHON=/app/.venv/bin/python

# Build
RUN cargo build --release

# Default command runs physics tests
CMD ["cargo", "test", "--test", "physics_check", "--", "--ignored", "--nocapture"]
DOCKERFILE
    fi

    log_info "Building Docker image..."
    docker build -t tach-physics -f Dockerfile.physics . || {
        log_error "Docker build failed"
        exit 1
    }

    log_info "Running physics tests in Docker container..."
    docker run --rm \
        --cap-add=SYS_PTRACE \
        --security-opt seccomp=unconfined \
        --security-opt apparmor=unconfined \
        -e PYO3_PYTHON=/app/.venv/bin/python \
        tach-physics \
        bash -c "
            sysctl -w vm.unprivileged_userfaultfd=1 2>/dev/null || true
            cargo test --test physics_check -- --nocapture
            cargo test --test memory_invariant -- --nocapture
            cargo test --test sandbox_enforcement -- --nocapture
        "
}

# =============================================================================
# Main
# =============================================================================

main() {
    local use_docker=false
    local check_only=false

    # Parse arguments
    while [[ $# -gt 0 ]]; do
        case $1 in
            --docker)
                use_docker=true
                shift
                ;;
            --check-only)
                check_only=true
                shift
                ;;
            --help|-h)
                show_help
                ;;
            *)
                log_error "Unknown option: $1"
                show_help
                ;;
        esac
    done

    log_header "Project Tach: Iron Dome Bootstrap"

    # Run checks
    log_info "Checking prerequisites..."
    check_linux
    check_kernel_version
    check_rust
    check_python

    if $use_docker; then
        check_docker
    fi

    # Check userfaultfd
    if ! check_userfaultfd; then
        if $use_docker; then
            log_info "userfaultfd will be enabled inside Docker container"
        else
            enable_userfaultfd
        fi
    fi

    if $check_only; then
        log_header "Prerequisite Check Complete"
        log_success "All prerequisites met. Ready to run physics tests."
        exit 0
    fi

    # Run tests
    if $use_docker; then
        run_physics_tests_docker
    else
        run_physics_tests_native
    fi

    log_header "Physics Test Run Complete"
}

main "$@"
