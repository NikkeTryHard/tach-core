# Contributing to Tach

Thank you for your interest in contributing to Tach.

## Getting Started

1. Fork the repository
2. Clone your fork: `git clone https://github.com/YOUR_USERNAME/tach-core.git`
3. Set up development environment:
   ```bash
   python -m venv .venv && source .venv/bin/activate && pip install pytest
   export PYO3_PYTHON=$(which python)
   cargo build
   ```

## Development Workflow

See [docs/development.md](docs/development.md) for detailed development guide.

### Quick Commands

```bash
cargo test --lib              # Unit tests
cargo test --test '*'         # Integration tests
pytest tests/gauntlet/ -v     # Python gauntlet tests
```

### Before Submitting

1. Run `cargo fmt --check && cargo clippy -- -D warnings`
2. Run `cargo test --lib && cargo test --test '*'`
3. Update documentation if adding features

## Commit Messages

Use [Conventional Commits](https://www.conventionalcommits.org/):

- `feat:` - New feature
- `fix:` - Bug fix
- `docs:` - Documentation only
- `test:` - Tests only
- `refactor:` - Code restructure
- `chore:` - Maintenance

## Pull Requests

1. Create a feature branch from `master`
2. Write tests for new functionality
3. Ensure all tests pass
4. Submit PR with clear description

## Code of Conduct

Be respectful and constructive in all interactions.

## Questions?

Open an issue for questions about contributing.
