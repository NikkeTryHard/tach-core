# tach-core

Hypervisor-accelerated Python test runner in Rust.

## Build & Test

```bash
# Build (uses mold + sccache automatically)
cargo build

# Tests - use nextest, not cargo test
cargo nextest run --lib        # Unit tests only (fast)
cargo nextest run              # All tests
cargo nextest run -E 'test(async)'  # Filter by name

# Linting
cargo fmt && cargo clippy -- -D warnings
```

## Speed Stack

| Tool | Purpose |
|------|---------|
| **mold** | 10x faster linker |
| **sccache** | Compilation caching |
| **nextest** | Parallel test execution |

Config lives in `.cargo/config.toml` and `.config/nextest.toml`.

## Project Structure

- `src/` - Rust core (discovery, execution, hooks, protocol)
- `src/tach_harness.py` - Python test harness
- `tests/` - Python gauntlet tests
- `rust_tests/` - Rust integration tests

## Roadmap

See `docs/research/roadmap.md` for implementation phases.
