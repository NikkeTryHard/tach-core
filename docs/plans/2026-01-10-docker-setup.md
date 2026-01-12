# Docker Development Environment Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Create a full-featured Docker development environment for tach-core with all kernel features (userfaultfd, Landlock, Seccomp) working without compromise.

**Architecture:** Hybrid Docker setup that works as both a standalone `docker compose` environment and a VS Code Dev Container. Uses `--privileged` mode to access WSL2 kernel features. Persistent volumes for cargo cache to enable fast rebuilds.

**Tech Stack:** Docker, Docker Compose, VS Code Dev Containers, Ubuntu 24.04, Rust 1.88+, Python 3.12

**Design Doc:** `docs/plans/2026-01-10-docker-setup-design.md`

---

## Task 1: Create Dockerfile

**Files:**

- Create: `Dockerfile`

**Step 1: Write the Dockerfile**

Create `/home/louiskaneko/dev/tach-core/Dockerfile`:

```dockerfile
# tach-core Development Container
# Provides full kernel feature support for userfaultfd, Landlock, and Seccomp
#
# Usage:
#   docker build -t tach-dev .
#   docker run -it --privileged -v $(pwd):/workspace tach-dev

FROM ubuntu:24.04

# Prevent interactive prompts during package installation
ENV DEBIAN_FRONTEND=noninteractive

# System packages: build tools, Python, debugging utilities
RUN apt-get update && apt-get install -y \
    # Build essentials
    build-essential \
    clang \
    libclang-dev \
    pkg-config \
    libssl-dev \
    cmake \
    # Python 3.12
    python3.12 \
    python3.12-venv \
    python3.12-dev \
    python3-pip \
    # Debug tools
    gdb \
    strace \
    linux-tools-generic \
    htop \
    procps \
    # Utilities
    git \
    curl \
    jq \
    ripgrep \
    fd-find \
    less \
    vim \
    # Clean up
    && rm -rf /var/lib/apt/lists/*

# Install Rust via rustup
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y \
    && . /root/.cargo/env \
    && rustup default stable \
    && rustup update

# Add cargo to PATH for all shells
ENV PATH="/root/.cargo/bin:${PATH}"

# Create workspace directory
WORKDIR /workspace

# Environment variables for tach-core
ENV PYO3_PYTHON=/workspace/.venv/bin/python
ENV CARGO_TARGET_DIR=/workspace/target

# Default command
CMD ["bash"]
```

**Step 2: Verify Dockerfile syntax**

Run:

```bash
docker build --check . 2>&1 || docker build -t tach-dev-test . --progress=plain 2>&1 | head -20
```

Expected: No syntax errors (build may start or succeed)

**Step 3: Commit**

```bash
git add Dockerfile
git commit -m "feat: add Dockerfile for development environment

Includes:
- Ubuntu 24.04 base with Python 3.12
- Rust toolchain via rustup
- Debug tools (gdb, strace, perf, htop)
- Search utilities (ripgrep, fd-find)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Task 2: Create docker-compose.yml

**Files:**

- Create: `docker-compose.yml`

**Step 1: Write docker-compose.yml**

Create `/home/louiskaneko/dev/tach-core/docker-compose.yml`:

```yaml
# tach-core Development Environment
#
# Usage:
#   docker compose up -d      # Start container in background
#   docker compose exec dev bash  # Enter container
#   docker compose down       # Stop container
#
# First run will build the image and set up volumes.
# Subsequent runs reuse cached volumes for fast startup.

services:
  dev:
    build:
      context: .
      dockerfile: Dockerfile
    image: tach-dev:latest
    container_name: tach-dev

    # Required for userfaultfd, Landlock, Seccomp, and namespace features
    privileged: true

    volumes:
      # Source code (bind mount)
      - .:/workspace

      # Persistent cargo caches (named volumes)
      - cargo-registry:/root/.cargo/registry
      - cargo-git:/root/.cargo/git

    working_dir: /workspace

    # Keep container running for interactive use
    stdin_open: true
    tty: true

    environment:
      - PYO3_PYTHON=/workspace/.venv/bin/python
      - CARGO_TARGET_DIR=/workspace/target
      - RUST_BACKTRACE=1

    # Use host network for simpler networking (optional)
    # network_mode: host

# Named volumes persist cargo cache across container restarts
volumes:
  cargo-registry:
  cargo-git:
```

**Step 2: Verify docker-compose.yml syntax**

Run:

```bash
docker compose config --quiet && echo "docker-compose.yml is valid"
```

Expected: `docker-compose.yml is valid`

**Step 3: Commit**

```bash
git add docker-compose.yml
git commit -m "feat: add docker-compose.yml for easy container management

Features:
- Privileged mode for kernel feature access
- Persistent cargo cache volumes
- Bind mount for source code
- Environment variables for PyO3

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Task 3: Create .devcontainer/devcontainer.json

**Files:**

- Create: `.devcontainer/devcontainer.json`

**Step 1: Create .devcontainer directory**

Run:

```bash
mkdir -p /home/louiskaneko/dev/tach-core/.devcontainer
```

**Step 2: Write devcontainer.json**

Create `/home/louiskaneko/dev/tach-core/.devcontainer/devcontainer.json`:

```json
{
  "name": "tach-core Dev",
  "dockerComposeFile": "../docker-compose.yml",
  "service": "dev",
  "workspaceFolder": "/workspace",

  "customizations": {
    "vscode": {
      "extensions": ["rust-lang.rust-analyzer", "ms-python.python", "ms-python.vscode-pylance", "tamasfe.even-better-toml", "vadimcn.vscode-lldb", "usernamehw.errorlens", "eamodio.gitlens"],
      "settings": {
        "terminal.integrated.defaultProfile.linux": "bash",
        "python.defaultInterpreterPath": "/workspace/.venv/bin/python",
        "rust-analyzer.cargo.buildScripts.enable": true,
        "rust-analyzer.procMacro.enable": true,
        "editor.formatOnSave": true,
        "[rust]": {
          "editor.defaultFormatter": "rust-lang.rust-analyzer"
        },
        "[python]": {
          "editor.defaultFormatter": "ms-python.python"
        }
      }
    }
  },

  "postCreateCommand": ".devcontainer/post-create.sh",

  "remoteUser": "root"
}
```

**Step 3: Verify JSON syntax**

Run:

```bash
python3 -c "import json; json.load(open('.devcontainer/devcontainer.json'))" && echo "JSON valid"
```

Expected: `JSON valid`

**Step 4: Commit**

```bash
git add .devcontainer/devcontainer.json
git commit -m "feat: add VS Code devcontainer.json

Configures:
- rust-analyzer, Python, LLDB extensions
- Format on save for Rust and Python
- Uses docker-compose.yml for container config

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Task 4: Create .devcontainer/post-create.sh

**Files:**

- Create: `.devcontainer/post-create.sh`

**Step 1: Write post-create.sh**

Create `/home/louiskaneko/dev/tach-core/.devcontainer/post-create.sh`:

```bash
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
```

**Step 2: Make script executable**

Run:

```bash
chmod +x /home/louiskaneko/dev/tach-core/.devcontainer/post-create.sh
```

**Step 3: Commit**

```bash
git add .devcontainer/post-create.sh
git commit -m "feat: add post-create.sh setup script

Sets up:
- Python venv with pytest
- Initial cargo build
- Runs self-test to verify kernel features

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Task 5: Update docs/wsl2-setup.md with Docker section

**Files:**

- Modify: `docs/wsl2-setup.md` (replace section "4. Docker Alternative")

**Step 1: Read current Docker section location**

The current Docker section starts at line 149 with "### 4. Docker Alternative" and ends before "### 5. Accept Graceful Degradation" at line 165.

**Step 2: Update the Docker section**

Replace lines 149-163 in `/home/louiskaneko/dev/tach-core/docs/wsl2-setup.md` with:

````markdown
### 4. Docker Development Environment

For a fully-configured development environment, use the included Docker setup:

#### Quick Start (Docker Compose)

```bash
# Build and start container
docker compose up -d

# Enter container
docker compose exec dev bash

# Inside container - first time setup
source .venv/bin/activate  # Created by post-create.sh or manually
python3.12 -m venv .venv && source .venv/bin/activate && pip install pytest

# Build and verify
cargo build --release
./target/release/tach-core self-test
```
````

#### VS Code Dev Container

1. Install the "Dev Containers" extension in VS Code
2. Open the tach-core folder
3. Click "Reopen in Container" when prompted (or Ctrl+Shift+P → "Dev Containers: Reopen in Container")
4. Wait for the container to build and setup to complete
5. Open a terminal - all tools are ready

#### What's Included

| Tool        | Purpose                  |
| ----------- | ------------------------ |
| Rust 1.88+  | Build tach-core          |
| Python 3.12 | Run tests, PyO3 bindings |
| gdb         | Debug Rust/Python        |
| strace      | Trace syscalls           |
| perf        | Performance profiling    |
| ripgrep, fd | Fast searching           |

#### Kernel Features in Docker

The container runs with `--privileged` mode, which grants access to all kernel features:

```bash
# Inside container, all features work:
./target/release/tach-core self-test
# [PASS] userfaultfd: Enabled
# [PASS] Landlock: ABI v4 supported
# [PASS] Seccomp: BPF filters available
```

Note: The Docker container inherits kernel features from WSL2. If your WSL2 kernel doesn't have a feature enabled (e.g., Landlock LSM not loaded), Docker cannot provide it. See sections above for WSL2 kernel configuration.

#### Persistent Cargo Cache

Docker volumes preserve cargo's package cache between container restarts:

```bash
# First build downloads all crates
cargo build  # ~2 minutes

# Container restart
docker compose down && docker compose up -d

# Subsequent builds use cached crates
cargo build  # ~10 seconds (incremental)
```

````

**Step 3: Commit**

```bash
git add docs/wsl2-setup.md
git commit -m "docs: expand Docker section with full dev environment guide

Replaces basic Docker example with:
- Docker Compose quick start
- VS Code Dev Container instructions
- Tool inventory table
- Kernel feature explanation
- Persistent cache documentation

Co-Authored-By: Claude <noreply@anthropic.com>"
````

---

## Task 6: Build and Test Docker Environment

**Files:**

- None (verification only)

**Step 1: Build the Docker image**

Run:

```bash
docker compose build --progress=plain 2>&1 | tail -30
```

Expected: Build completes successfully with "Successfully built" or "Successfully tagged"

**Step 2: Start the container**

Run:

```bash
docker compose up -d && docker compose ps
```

Expected: Container "tach-dev" is running

**Step 3: Verify kernel features inside container**

Run:

```bash
docker compose exec dev bash -c "
source /root/.cargo/env
python3.12 -m venv .venv
source .venv/bin/activate
pip install pytest -q
cargo build --release 2>&1 | tail -5
./target/release/tach-core self-test
"
```

Expected: All 8 self-test checks pass:

- `[PASS] Kernel Version`
- `[PASS] userfaultfd`
- `[PASS] Landlock`
- `[PASS] Seccomp`
- `[PASS] Jemalloc`
- `[PASS] ptrace`
- `[PASS] Python`
- `[PASS] Physics Heartbeat`

**Step 4: Verify debug tools**

Run:

```bash
docker compose exec dev bash -c "which gdb strace htop rg fd"
```

Expected: All paths printed (e.g., `/usr/bin/gdb`, `/usr/bin/strace`, etc.)

**Step 5: Verify cargo cache persistence**

Run:

```bash
docker compose down
docker compose up -d
docker compose exec dev bash -c "ls /root/.cargo/registry/cache/ 2>/dev/null | head -5 || echo 'Cache exists'"
```

Expected: Cache directory exists or lists cached crates

**Step 6: Stop container (cleanup)**

Run:

```bash
docker compose down
```

---

## Task 7: Add .dockerignore

**Files:**

- Create: `.dockerignore`

**Step 1: Write .dockerignore**

Create `/home/louiskaneko/dev/tach-core/.dockerignore`:

```
# Build artifacts (rebuilt in container)
target/
*.so
*.rlib

# Python
.venv/
__pycache__/
*.pyc
*.pyo
.pytest_cache/
*.egg-info/

# IDE
.idea/
.vscode/
*.swp
*.swo

# Git
.git/
.gitignore

# Docker (avoid recursive context)
.docker/

# Documentation (not needed for build)
docs/
*.md
!README.md

# Tests (optional - include if running tests in container)
# tests/

# Misc
.env
.envrc
*.log
coverage.lcov
```

**Step 2: Commit**

```bash
git add .dockerignore
git commit -m "feat: add .dockerignore to optimize build context

Excludes:
- Build artifacts (target/, .venv/)
- IDE files
- Git history
- Documentation

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Task 8: Final Verification and Documentation Commit

**Files:**

- Modify: None (verification only)

**Step 1: Run full test suite in Docker**

Run:

```bash
docker compose up -d
docker compose exec dev bash -c "
source .venv/bin/activate 2>/dev/null || (python3.12 -m venv .venv && source .venv/bin/activate && pip install pytest -q)
source .venv/bin/activate
cargo test --lib 2>&1 | tail -10
"
```

Expected: All Rust unit tests pass

**Step 2: Run Python gauntlet tests**

Run:

```bash
docker compose exec dev bash -c "
source .venv/bin/activate
pytest tests/gauntlet/ -v --tb=short 2>&1 | tail -20
"
```

Expected: Gauntlet tests pass (or skip gracefully if binary not in expected location)

**Step 3: Cleanup**

Run:

```bash
docker compose down
```

**Step 4: Create final commit with all files**

If any files were missed:

```bash
git status
git add -A
git commit -m "feat: complete Docker development environment setup

Files added:
- Dockerfile (Ubuntu 24.04, Rust, Python 3.12, debug tools)
- docker-compose.yml (privileged mode, persistent volumes)
- .devcontainer/devcontainer.json (VS Code integration)
- .devcontainer/post-create.sh (automatic setup)
- .dockerignore (optimized build context)

Verified:
- All kernel features work (userfaultfd, Landlock, Seccomp)
- Cargo cache persists across restarts
- Debug tools available (gdb, strace, perf)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Verification Checklist

After completing all tasks, verify:

- [ ] `docker compose build` succeeds
- [ ] `docker compose up -d` starts container
- [ ] `tach-core self-test` passes all 8 checks inside container
- [ ] `cargo test --lib` passes inside container
- [ ] `gdb`, `strace`, `perf` commands are available
- [ ] Cargo cache persists after `docker compose down && docker compose up`
- [ ] `.devcontainer/devcontainer.json` has valid JSON syntax

---

## Rollback

If issues occur, remove Docker files:

```bash
git checkout HEAD -- Dockerfile docker-compose.yml .devcontainer/ .dockerignore
docker compose down --volumes  # Remove containers and volumes
```
