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
    mold \
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
    # Network namespace support
    iproute2 \
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

# Install sccache and nextest for fast builds and test execution
RUN cargo install sccache --locked \
    && cargo install cargo-nextest --locked

# Create workspace directory
WORKDIR /workspace

# Copy entrypoint script
COPY docker-entrypoint.sh /usr/local/bin/
RUN chmod +x /usr/local/bin/docker-entrypoint.sh

# Environment variables for tach-core
ENV PYO3_PYTHON=/workspace/.venv/bin/python
ENV CARGO_TARGET_DIR=/workspace/target

# Entrypoint ensures deps are installed on first run
ENTRYPOINT ["/usr/local/bin/docker-entrypoint.sh"]

# Default command
CMD ["bash"]
