#!/bin/bash
# Docker Environment Enforcement Script
# Blocks execution outside of the Docker dev container
#
# Usage: source scripts/require-docker.sh
#        Or call directly: ./scripts/require-docker.sh

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

check_docker_environment() {
    # Check for bypass environment variable
    if [[ -n "${SKIP_DOCKER_CHECK:-}" ]]; then
        echo -e "${YELLOW}[tach] Docker check bypassed via SKIP_DOCKER_CHECK${NC}"
        return 0
    fi

    # Method 1: Check for /.dockerenv file (most reliable)
    if [[ -f /.dockerenv ]]; then
        echo -e "${GREEN}[tach] Running inside Docker container${NC}"
        return 0
    fi

    # Method 2: Check cgroup for docker/container references
    if grep -qE '(docker|containerd|lxc)' /proc/1/cgroup 2>/dev/null; then
        echo -e "${GREEN}[tach] Running inside container (cgroup detected)${NC}"
        return 0
    fi

    # Method 3: Check for container environment variable
    if [[ -n "${container:-}" ]]; then
        echo -e "${GREEN}[tach] Running inside container (env detected)${NC}"
        return 0
    fi

    # Method 4: Check hostname matches our container name
    if [[ "$(hostname)" == "tach-dev" ]]; then
        echo -e "${GREEN}[tach] Running inside tach-dev container${NC}"
        return 0
    fi

    # Not in Docker - check if WSL2
    if grep -qi microsoft /proc/version 2>/dev/null; then
        echo -e "${RED}========================================${NC}"
        echo -e "${RED}ERROR: WSL2 DEVELOPMENT IS NOT ALLOWED${NC}"
        echo -e "${RED}========================================${NC}"
        echo ""
        echo -e "${YELLOW}WSL2 causes kernel instability with userfaultfd and jemalloc.${NC}"
        echo -e "${YELLOW}You MUST develop inside the Docker container.${NC}"
        echo ""
        echo "RECOMMENDED - Use Docker:"
        echo ""
        echo -e "  ${GREEN}docker compose up -d${NC}"
        echo -e "  ${GREEN}docker compose exec dev bash${NC}"
        echo ""
        echo "BYPASS (NOT RECOMMENDED):"
        echo ""
        echo -e "  ${GREEN}TACH_ALLOW_NATIVE_BUILD=1 cargo build --release${NC}"
        echo -e "  ${GREEN}SKIP_DOCKER_CHECK=1 git commit -m \"message\"${NC}"
        echo ""
        return 1
    fi

    # Native Linux - also block (for consistency)
    echo -e "${RED}========================================${NC}"
    echo -e "${RED}ERROR: NATIVE DEVELOPMENT IS NOT ALLOWED${NC}"
    echo -e "${RED}========================================${NC}"
    echo ""
    echo -e "${YELLOW}Development must happen inside the Docker container${NC}"
    echo -e "${YELLOW}for consistent environment and kernel features.${NC}"
    echo ""
    echo "RECOMMENDED - Use Docker:"
    echo ""
    echo -e "  ${GREEN}docker compose up -d${NC}"
    echo -e "  ${GREEN}docker compose exec dev bash${NC}"
    echo ""
    echo "BYPASS (NOT RECOMMENDED):"
    echo ""
    echo -e "  ${GREEN}SKIP_DOCKER_CHECK=1 git commit -m \"message\"${NC}"
    echo ""
    return 1
}

# Run check if script is executed directly (not sourced)
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    check_docker_environment
    exit $?
fi
