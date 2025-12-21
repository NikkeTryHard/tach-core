# tach-core

`tach-core` is a high-performance Python test runner written in Rust. It is designed to execute pytest-style tests with significant speed improvements by leveraging static analysis and a zygote process model.

## Key Features

- **Static Discovery**: Parses Python files (`.py`) to discover tests and fixtures without importing them. This uses `rustpython-parser` for speed and safety.
- **Fixture Resolution**: Resolves pytest-style fixtures, including scoping and dependency injection. It detects missing fixtures and cyclic dependencies before execution.
- **Zygote Execution Model**: Uses a "Zygote" process to pre-initialize the Python interpreter. Workers are forked from the Zygote, eliminating the startup overhead for each test file.
- **Parallel Execution**: Runs tests in parallel using a scheduler that manages worker processes.
- **IPC**: Uses Unix domain sockets for fast communication between the supervisor and the worker processes.

## Architecture

1.  **Supervisor**: The main Rust process. It scans files, resolves the test plan, and manages the Zygote.
2.  **Zygote**: A Python process that initializes the environment and listens for commands.
3.  **Workers**: Forked from the Zygote to execute specific tests in isolation.

## Usage

To run the tests in the current directory:

```bash
cargo run
```

Or build the binary:

```bash
cargo build --release
./target/release/tach-core
```

## Requirements

- Rust (latest stable)
- Python 3.x
- `pytest` (must be installed in the Python environment)
