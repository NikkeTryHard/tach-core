# Changelog

> **Future Roadmap:** See [docs/research/roadmap.md](docs/research/roadmap.md) for planned versions 0.1.x through 1.0.0+.

---

All notable changes to Project Tach will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4.1] - 2026-03-04

### Added

- **Toxicity Config Overrides** (0.6.2): `[tool.tach.toxicity]` section with
  `force_safe` and `force_toxic` module lists
- **pytest-cov Auto-Detection**: Disable pytest-cov and suggest `tach --coverage`
- **pytest-xdist Auto-Detection**: Disable xdist and suggest `tach -n auto`
- **`--retries` CLI Flag**: Plumbing for flaky test detection (interface only)
- **Cache Module**: Extracted duration/lastfailed cache into `src/cache.rs` with 6 tests

### Changed

- Refactored main.rs to use extracted cache module

## [0.4.0] - 2026-03-04

### Added

- **Full pyproject.toml Configuration** (0.6.0): 12 new `[tool.tach]` keys
  (`keyword`, `markers`, `exitfirst`, `maxfail`, `force_toxic`, `no_fallback`,
  `no_isolation`, `traceback`, `durations`, `memory`, `no_ignore`, `reuse_db`)
- **MergedConfig Single Source of Truth**: CLI and file config merge into one
  struct; CLI always wins over pyproject.toml values
- **GitHub Actions Integration** (0.8.0): Auto-detect `GITHUB_ACTIONS` env,
  emit `::error` annotations with file/line, write markdown summary to
  `$GITHUB_STEP_SUMMARY`
- **`--showlocals` / `-l` Flag** (0.5.0): Show local variables in tracebacks,
  forwarded to Python harness via `TACH_SHOWLOCALS` env var
- **Stale Run Directory Cleanup** (0.9.5): Remove `/tmp/tach_run_*` dirs older
  than 1 hour on startup to prevent disk leaks from crashed sessions
- **Pytest Fallback Retry**: Automatically retry failed tests with vanilla pytest
  to distinguish tach-specific failures from real test failures (`--no-fallback` to disable)
- **Framework Plugin Support**: Let pytest-django, pytest-asyncio, pytest-trio run their
  session-level setup instead of disabling them
- **Session Fixture Execution**: Execute session-scoped autouse fixtures in the zygote
  before fork so workers inherit the setup via CoW

### Fixed

- ALLOWED_HOSTS 400 errors via session fixture execution
- assertLogs failures by preserving logging handlers across fork
- Unicode errors by setting UTF-8 locale at supervisor startup
- Missing pytest options (verbose, tbstyle) when terminal plugin disabled
- Duplicate `load_env_from_pyproject` call removed from inner execute_tests

### Changed

- Django test suite: 1575 failures -> 87 raw / 51 with fallback (93% improvement)

---

## [0.2.4] - 2026-02-01

### Added

- **Landlock V4-V6 Network Isolation** (Issue #20): Fine-grained TCP network restrictions
  - `detect_landlock_abi()` probes kernel for Landlock ABI versions 1-6
  - `supports_landlock_network()` checks for ABI V4+ (kernel 6.7+)
  - `apply_landlock_network()` restricts TCP bind/connect operations
  - `NetworkIsolationStatus` enum for reporting isolation level (LandlockV4, Namespace, SeccompOnly, None)
  - `apply_iron_dome_with_network()` combined sandbox entry point
  - Graceful fallback to Seccomp on kernels < 6.7

- **Network Configuration** (`[tool.tach.network]`):
  - `allow_localhost` - permit loopback connections (default: true)
  - `allow_connect` - whitelist of host:port targets
  - `allow_bind_ports` - allowed TCP bind ports (0 = ephemeral)

### Fixed

- **AsyncioSetup effects dropped in IPC conversion** - Added match arm for AsyncioSetup
  in `convert_py_effects_to_rust`, fixing silent effect drops during worker initialization (#46)
- **Event loop resource leak for class/module/session scopes** - Added scope transition
  tracking to EventLoopManager and cleanup in reset_worker_state (#43)
- CI now runs integration tests in addition to unit tests ([#41](https://github.com/NikkeTryHard/tach-core/issues/41))
- Tests properly skip instead of silently passing when tach_harness unavailable ([#42](https://github.com/NikkeTryHard/tach-core/issues/42))
- Tests use public properties instead of private attributes ([#44](https://github.com/NikkeTryHard/tach-core/issues/44))
- Cleanup errors are now logged instead of silently swallowed ([#45](https://github.com/NikkeTryHard/tach-core/issues/45))

---

## [0.2.2] - 2026-01-31

### Added

- **pytest-asyncio Support**: Full async test and fixture support

---

## [0.2.1] - 2026-01-30

### Added

- **pytest-django Support (Core Infrastructure)**: Foundation for Django test support
  - Static parsing of `@pytest.mark.django_db` markers with argument extraction
  - `MarkerInfo` structure for IPC propagation of marker arguments
  - `DjangoDbSetup` HookEffect variant for database configuration
  - SAVEPOINT-based transaction isolation in Python harness
  - `_apply_django_db_isolation()` wraps tests in atomic savepoints
  - `_cleanup_django_db_isolation()` ensures rollback on success or failure
  - pytest-django registered as "Supported" plugin in registry
  - Integration tests in `tests/gauntlet_django/` (marker isolation, parallel isolation, savepoint cleanup)

### Not Yet Implemented

The following features are tracked in GitHub issues and deferred to 0.3.x:

- `transaction=True` argument (#40)
- `reset_sequences=True` argument (#36)
- Multi-database `databases=[...]` support (#38)
- Django fixtures: `client`, `rf`, `admin_client`, `live_server` (#39)
- `--reuse-db` and `--create-db` CLI flags (#37)
- `@pytest.mark.urls` and `@pytest.mark.ignore_template_errors` markers (#35)

---

## [0.2.0] - 2026-01-17

### Added

- **Hook Interception Framework**: Complete pytest plugin compatibility system
  - Hook discovery in conftest.py files with toxicity integration
  - Conftest inheritance resolution (root-to-leaf hook ordering)
  - Effect recording for pytest_configure (env vars, sys.path modifications)
  - Effect replay in workers before test execution
  - IPC protocol extension with hooks, cached_effects, markers fields in TestPayload
  - Plugin detection and warning system using importlib.metadata
  - **HookResult type and aggregation strategies** (FirstResult, AllResults, NoReturn)
  - **HookCaller with PyO3 bridge** for Rust-side hook orchestration
  - **HookDependencyGraph** for conftest hierarchy ordering (root→leaf)
  - **PluginRegistry** with plugin status tracking (Supported, Partial, Superseded, Incompatible)
  - **Plugin configuration** via pyproject.toml (disabled plugins, priority ordering)
  - **call_hook_impl()** Python function for loading conftest and calling hooks
  - **pytest_collection_modifyitems** hook support (reordering, deselection)
  - **pytest_runtest_setup/teardown** hook support with effect capture
  - **pytest_runtest_makereport** hook support for result reporting
  - **pytest_sessionfinish** hook support for session cleanup
- **Hook Registry**: Foundation for pytest plugin compatibility
  - HookSpec, Hook, HookEffect types with Serde derives for IPC serialization
  - HookRegistry for tracking discovered hooks
  - builtin_hook_specs() for 10 known pytest hooks
  - file_has_toxic_hooks() for toxicity graph integration
  - resolve_hooks_for_path() for conftest inheritance
  - get_session_effects() for session-level hook effects
- **Hook Detection**: Discover pytest hooks in conftest.py files (only conftest.py, not test files)
- **Marker Detection**: Extract pytest markers from test decorators
  - Markers included in `tach list --json` output
  - Markers propagated to workers via TestPayload
  - Excludes decorator-only markers (parametrize, usefixtures, filterwarnings)
- **Plugin Detection**: Detect installed pytest plugins at startup
  - Warn about unsupported plugins (pytest-parallel, pytest-forked, etc.)
  - Log info about unknown plugins that may or may not work
  - Supported plugins list includes pytest-mock, pytest-env, pytest-randomly, etc.
- **Effect Recording**: Capture side effects from session-level hooks
  - Environment variable changes (SetEnv effect)
  - sys.path modifications (ModifySysPath effect with prepend/append/remove)
  - Effects transmitted from Zygote to Supervisor to Workers
- **Autouse Fixture Detection**: Parse autouse=True from @pytest.fixture

### Changed

- TestModule now includes hooks field
- TestCase now includes markers field
- FixtureDefinition now includes autouse field
- RunnableTest now includes markers field for worker propagation
- TestPayload now includes hooks, cached_effects, and markers fields
- JsonTestInfo now includes markers field for JSON discovery output
- ToxicityGraph::build() now accepts HookRegistry parameter for hook-based toxicity

See [docs/research/roadmap.md](docs/research/roadmap.md) for the complete development roadmap including:

- 0.1.x - Foundation (v0.1.5 released, complete)
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

## [0.1.5] - 2026-01-14

### Tooling Integration Research

This release completes the 0.1.x Foundation phase with tooling ecosystem documentation, container compatibility research, and developer experience improvements.

### Added

- **Docker Development Environment**: Full containerized dev setup with Dockerfile, docker-compose.yml, VS Code devcontainer.json, and post-create.sh script
- **`--no-ignore` CLI Flag**: Bypass `.ignore`/`.gitignore` files during test discovery
- **`.ignore` Pattern Warnings**: Detect and warn when `.ignore` patterns block Python file discovery
- **Research Documentation**: Container compatibility matrix, tooling conflicts analysis, and test discovery edge case catalogue

### Changed

- **CI Coverage Threshold**: Enforced 90% coverage as hard failure
- **Golden Tests**: Now opt-in via `GOLDEN_TESTS=1` environment variable
- **MSRV**: Updated minimum supported Rust version to 1.88

### Fixed

- **Python 3.14 Support**: Handle immortalization behavior in refcount tests
- **Landlock Security**: Restrict project_root access, remove excessive /run access, fix symlink escape paths
- **CI Stability**: Prioritize release binary in tests, fix coverage parsing, add missing test directories

### Security

- Restricted Landlock access for project_root (write access only where needed for OverlayFS)
- Removed excessive /run filesystem access from Landlock rules
- Added mknod blocking test for Landlock enforcement

### Documentation

- Consolidated research topics into single archive
- Merged errors.md into troubleshooting.md, wsl2-setup.md into quickstart.md
- Added container compatibility research with empirical testing
- Removed volatile data (test counts, line numbers) from documentation

---

## [0.1.4] - 2026-01-07

### Dependency Updates

Completed the Foundation phase with dependency updates and Python compatibility testing.

See [docs/research/roadmap.md](docs/research/roadmap.md) for details on 0.1.4 deliverables.

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

[Unreleased]: https://github.com/NikkeTryHard/tach-core/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/NikkeTryHard/tach-core/compare/v0.1.5...v0.2.0
[0.1.5]: https://github.com/NikkeTryHard/tach-core/compare/v0.1.4...v0.1.5
[0.1.4]: https://github.com/NikkeTryHard/tach-core/compare/v0.1.0...v0.1.4
[0.1.0]: https://github.com/NikkeTryHard/tach-core/releases/tag/v0.1.0
