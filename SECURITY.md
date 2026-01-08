# Security Policy

## Supported Versions

| Version | Supported          |
| ------- | ------------------ |
| 0.1.x   | :white_check_mark: |

## Reporting a Vulnerability

If you discover a security vulnerability in Tach, please report it responsibly:

1. **Do NOT** open a public GitHub issue
2. Email security concerns to the maintainers (see repository owner contact)
3. Include:
   - Description of the vulnerability
   - Steps to reproduce
   - Potential impact
   - Suggested fix (if any)

## Security Model

Tach implements multiple security layers:

### Iron Dome Sandbox

- **Landlock:** Filesystem access control (Linux 5.13+)
- **Seccomp:** System call filtering
- See [docs/development.md](docs/development.md#security-hardening) for details

### Environment Isolation

- Sensitive environment variables are blocked by default
- See `LD_PRELOAD`, `LD_LIBRARY_PATH`, `PYTHONPATH` denylist

### Test Isolation

- Tests run in isolated Linux namespaces
- Network isolation prevents cross-test interference

## Responsible Disclosure

We aim to respond to security reports within 48 hours and provide a fix timeline within 7 days for confirmed vulnerabilities.
