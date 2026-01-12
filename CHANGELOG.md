# Changelog

> **Future Roadmap:** See [docs/research/roadmap.md](docs/research/roadmap.md) for planned versions 0.1.x through 1.0.0+.

---

All notable changes to Project Tach will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.0](https://github.com/NikkeTryHard/tach-core/compare/v0.1.4...v0.2.0) (2026-01-12)


### Features

* add --no-ignore flag to bypass .ignore/.gitignore in discovery ([913b443](https://github.com/NikkeTryHard/tach-core/commit/913b44312e815ff171795f50b51afef9bd46df3b))
* add .dockerignore to optimize build context ([196248e](https://github.com/NikkeTryHard/tach-core/commit/196248e344d99f5f5b21c5ab69644d5ca37cc09b))
* add docker-compose.yml for easy container management ([a44bab1](https://github.com/NikkeTryHard/tach-core/commit/a44bab1f1f048f28ab79370a0903b5cc0261b1a0))
* add Dockerfile for development environment ([52ac11b](https://github.com/NikkeTryHard/tach-core/commit/52ac11b7c145b5b8d5fd2855aaf98717f9f25d96))
* add post-create.sh setup script ([9c21f81](https://github.com/NikkeTryHard/tach-core/commit/9c21f81596eb288483a0ea99b83424d607e5bdbf))
* add VS Code devcontainer.json ([0475562](https://github.com/NikkeTryHard/tach-core/commit/0475562604f4e92f198420f011f4f98602f01ff7))
* detect and warn about .ignore patterns blocking Python discovery ([884e774](https://github.com/NikkeTryHard/tach-core/commit/884e7741dac2f7f44a63c18831c3aa527519cfb0))
* integrate research insights for pytest parity ([1067f51](https://github.com/NikkeTryHard/tach-core/commit/1067f5125e48fc23f346896dd3d0aded2eb63b1a))
* warn about blocking patterns even when some tests found ([7425447](https://github.com/NikkeTryHard/tach-core/commit/7425447b01dc72b13b040839a81565515752fc1e))


### Bug Fixes

* add iproute2 to Docker image for network namespace support ([9c3e1b1](https://github.com/NikkeTryHard/tach-core/commit/9c3e1b16033ad7cf22b4c5ce384b3a69bce7a8a7))
* add llvm-cov target directory to binary path detection ([4d70214](https://github.com/NikkeTryHard/tach-core/commit/4d7021499ea510cd4880772b8f0012a85e7604ce))
* address code review issues ([5a6e4a1](https://github.com/NikkeTryHard/tach-core/commit/5a6e4a124bacc3c45829723d5601b3a6e08c1f60))
* address minor code review issues ([b97e335](https://github.com/NikkeTryHard/tach-core/commit/b97e3354c9191ce296b6db7f97dbc0d29d5fcfa3))
* adjust coverage threshold and improve test stability ([a3992b6](https://github.com/NikkeTryHard/tach-core/commit/a3992b64766f77c0877fee967b1a3ccb262b8cf1))
* allow write access to project_root in Landlock for OverlayFS ([e8bb534](https://github.com/NikkeTryHard/tach-core/commit/e8bb534bfc390abaeefb8223032fc445ecebf147))
* **build:** correct rust-version MSRV to 1.88 and fix clippy warnings ([1740388](https://github.com/NikkeTryHard/tach-core/commit/17403881b2ce2fcfcc11735dfff5a69678c768dc))
* **ci:** move crash signal tests to tests/crash_test/ ([e651bea](https://github.com/NikkeTryHard/tach-core/commit/e651bea1815e29ed916058dcf73925041128ac89))
* **ci:** split coverage lcov and html generation ([cb35716](https://github.com/NikkeTryHard/tach-core/commit/cb35716bccf66a1d42d24ec82acbc1a5bf4a567c))
* handle Python 3.14 immortalization behavior in refcount test ([f7e40e0](https://github.com/NikkeTryHard/tach-core/commit/f7e40e03de1a8aed014bad695af6dfc69fa97c45))
* make golden tests opt-in via GOLDEN_TESTS=1 ([c022a5d](https://github.com/NikkeTryHard/tach-core/commit/c022a5dae546624c7882893abf5ad33ab7ab0310))
* prioritize release binary in Rust tests and add missing __init__.py ([9895696](https://github.com/NikkeTryHard/tach-core/commit/989569697cee2c311004cf7416c546f493451f81))
* prioritize release binary in test files for CI compatibility ([9ef6649](https://github.com/NikkeTryHard/tach-core/commit/9ef66498bddccf0afde0fe3bc33154cf09c2b906))
* resolve remaining CI failures ([10e4a84](https://github.com/NikkeTryHard/tach-core/commit/10e4a84ab307b43fa3e078b4b970033c5958edb5))
* skip tach-specific tests and improve coverage parsing ([b45c29c](https://github.com/NikkeTryHard/tach-core/commit/b45c29c864f4b790c8e368ec2f79dd72e0bfc3a9))
* update symlink escape test to target path outside Landlock allow-list ([ecddee7](https://github.com/NikkeTryHard/tach-core/commit/ecddee7e56034b5408c4ab11dd93658d428f3c4a))
* use correct immortal refcount for Python 3.14 ([8e4beb0](https://github.com/NikkeTryHard/tach-core/commit/8e4beb074848da37cb382edcf57b83f966d7d876))

## [Unreleased]

See [docs/research/roadmap.md](docs/research/roadmap.md) for the complete development roadmap including:

- 0.1.x - Foundation (current, complete)
- 0.2.x - Plugin Compatibility
- 0.3.x - Database Integration
- 0.4.x - Fixture Lifecycle
- 0.5.x - Developer Experience
- 0.6.x - Configuration
- 0.7.x - Performance
- 0.8.x - CI/CD Integration
- 0.9.x - Stability
- 1.0.0 - Production Ready

---

## [0.1.0] - 2026-01-04

### Initial Alpha Release

This is the first public release of Tach - a Hypervisor-Accelerated Python Test Runner.

> **Note**: This is an alpha release. APIs may change. Not recommended for production use.

### Highlights

- **Restoration Physics Engine**: Bit-perfect snapshot/restore of Python interpreter state
- **Zero-Copy Test Execution**: Workers inherit pre-initialized Python via fork
- **Iron Dome Sandbox**: Landlock + Seccomp hardening with graceful kernel degradation
- **pytest-Compatible CLI**: Drop-in command-line interface

### Core Features

#### Test Discovery

- Static AST-based test discovery (no Python execution during discovery)
- Fixture resolution with topological dependency ordering
- Parametrized test expansion with proper deduplication
- Class-scoped and module-scoped fixture support

#### Execution Engine

- Zygote fork-server pattern for instant worker spawning
- userfaultfd memory snapshots for sub-millisecond restore
- Toxicity classification for safe vs unsafe tests
- Parallel worker pool with automatic CPU detection

#### Memory Restoration

- TLS (Thread-Local Storage) capture and restore
- BSS/Heap split-brain prevention
- Stack restoration via ptrace
- Self-calibrating mimalloc offset discovery (Python 3.13+)

#### Sandbox (Iron Dome)

- Landlock filesystem restrictions (ABI V1+)
- Seccomp syscall filtering (blacklist approach)
- Safe workers: Full Iron Dome (Landlock + Seccomp)
- Toxic workers: Landlock only (subprocess support)

#### Coverage

- Zero-overhead bytecode instrumentation
- Ring buffer collection via memfd
- LCOV/JSON output formats

#### Reporting

- Progress bar for interactive terminals
- Dots reporter for CI environments
- JUnit XML output for CI integration
- Time Saved metric showing initialization overhead reduction

### CLI Interface

```
tach [OPTIONS] [PATH] [COMMAND]

Commands:
  test       Run tests (default)
  list       List discovered tests
  self-test  Run system diagnostics
  version    Show version information

Options:
  -n <WORKERS>     Number of parallel workers (default: auto)
  -x, --exitfirst  Exit on first failure
  --maxfail <N>    Exit after N failures
  -v, -vv          Increase verbosity
  -q               Quiet mode
  -k <EXPR>        Filter tests by keyword expression
  -m <MARKERS>     Filter tests by markers
  --coverage       Enable coverage collection
  --no-isolation   Disable worker isolation
  --junit-xml <F>  Write JUnit XML report
  --json           Output results as JSON
  --watch          Watch mode for file changes
```

### System Requirements

- Linux 5.13+ (for Landlock ABI V1)
- x86_64 or aarch64 architecture
- Python 3.10+ with libpython (3.12+ for coverage)
- userfaultfd privileges (CAP_SYS_PTRACE or `vm.unprivileged_userfaultfd=1`)

Run `tach self-test` to verify system compatibility.

### Known Limitations

- No pytest plugin support yet (planned for 0.2.0)
- No database transaction rollback (planned for 0.3.0)
- Session-scoped fixtures not fully cached (planned for 0.4.0)
- Linux only (no Windows/macOS support)

---

## Development History

> The following documents internal development milestones.
> These were not public releases.

### Internal Milestones

| Milestone   | Focus Area      | Key Deliverables                           |
| ----------- | --------------- | ------------------------------------------ |
| Discovery   | Test Discovery  | AST-based scanning, fixture resolution     |
| Zygote      | Process Model   | Fork-server pattern, worker pool           |
| Snapshot    | Memory          | userfaultfd snapshots, MADV_DONTNEED       |
| Workers     | Execution       | Scheduler, IPC protocol, result collection |
| Coverage    | Instrumentation | PEP 669, ring buffers, memfd               |
| Iron Dome   | Security        | Landlock, Seccomp, graceful degradation    |
| Hot Reload  | Isolation       | sys.modules cleanup, import reset          |
| Allocator   | Memory          | jemalloc, tcache flush                     |
| Restoration | TLS             | fs_base capture, mimalloc calibration      |
| CLI         | Interface       | pytest-compatible arguments                |

### Key Technical Learnings

- **Clone syscall**: Never block in Seccomp - Python threading requires it
- **Landlock ABI**: Use V1 minimum for kernel 5.13+ compatibility
- **Path canonicalization**: Always canonicalize before adding Landlock rules
- **Seccomp blacklist**: Safer than allowlist - don't break unknown syscalls
- **Graceful degradation**: Log warnings, never crash on unsupported kernels
- **Toxic workers**: Need subprocess support, so bypass Seccomp

---

## Version History

> v1.0.0 was prematurely tagged and has been retracted. The first official release is v0.1.0.
>
> **Note:** Versions 0.1.1-0.1.4 were developed in parallel during the Foundation phase.
> v0.1.4 is the first tagged release after v0.1.0.

[Unreleased]: https://github.com/NikkeTryHard/tach-core/compare/v0.1.4...HEAD
[0.1.4]: https://github.com/NikkeTryHard/tach-core/compare/v0.1.0...v0.1.4
[0.1.0]: https://github.com/NikkeTryHard/tach-core/releases/tag/v0.1.0
