# Changelog

> **Future Roadmap:** See [docs/research/roadmap.md](docs/research/roadmap.md) for planned versions 0.1.x through 1.0.0+.

---

All notable changes to Project Tach will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.9.0] - 2026-03-08

### Added

- **Phase 4.0: Scope-Aware Fixture Lifecycle** - The biggest feature for pytest compatibility
  - Module-scoped and class-scoped fixtures now persist across tests in the same scope
  - Workers skip memory reset between tests sharing module/class fixtures
  - Scheduler groups scoped tests and dispatches them sequentially to the same worker
  - Python harness passes `nextitem` to `runtestprotocol()` for proper fixture teardown timing
  - `skip_reset` and `next_node_id` fields added to TestPayload protocol
  - `has_scoped_fixtures()`, `max_fixture_scope()`, `class_name()` helpers on RunnableTest
- **Phase 7.5: Adaptive Scheduling** - Smarter test ordering using historical data
  - Scheduler uses multi-run historical average durations as fallback
  - Longest-first scheduling for optimal parallel worker utilization
  - History loaded from test history store (`.tach_cache/history.json`)
- **`--shard INDEX/TOTAL`**: Test sharding for CI parallelism
  - Deterministic hash-based test distribution across N parallel CI jobs
  - Each shard gets a unique, balanced subset of tests
  - Compatible with GitHub Actions matrix, CircleCI parallelism, etc.
- 17 new tests (1067 total, up from 1050)

## [0.8.6] - 2026-03-05

### Added

- **`--live-log`**: Enable live log output at DEBUG level
- Created GitHub release notes for all 27 missing versions (v0.4.0-v0.8.6)

## [0.8.5] - 2026-03-05

### Added

- 2 new verification tests (--sw-skip, --tb=auto)
- 1043 tests total (95 new this session)

## [0.8.4] - 2026-03-05

### Added

- **`--sw-skip`**: Stepwise with skip first failure on resume
- **`--tb-short`**: Convenience flag for short tracebacks
- Updated roadmap status to v0.8.3
- ~70 CLI flags for complete pytest compatibility

## [0.8.3] - 2026-03-05

### Added

- Comprehensive pytest CLI compat integration test (all ~20 flags combined)
- 1041 tests total (93 new this session)

## [0.8.2] - 2026-03-05

### Added

- **`--tb-short`**: Convenience flag for short tracebacks
- 1 new test (--tb-short merges to TracebackStyle::Short)
- 1040 tests total (92 new this session)

## [0.8.1] - 2026-03-05

### Added

- **`tach bench`**: Discovery performance benchmark (files, tests, rate)
- **Git Hash in Version**: `tach version` now shows commit hash
- **`--log-cli-level LEVEL`**: Live logging output level control
- 11 subcommands total

## [0.8.0] - 2026-03-05

### Added

- **`--log-cli-level LEVEL`**: Control live logging output level
- Complete pytest CLI drop-in replacement with ~65 flags
- 1039 tests (91 new this session), 10 subcommands
- Session milestone: v0.3.1 -> v0.8.0 (31 releases)

## [0.7.8] - 2026-03-05

### Added

- 2 new tests (--durations-min, --html)
- 1038 tests total (90 new this session)

## [0.7.7] - 2026-03-05

### Added

- --Werror flag test verification
- 1036 tests total (88 new this session)

## [0.7.6] - 2026-03-05

### Added

- **`--Werror`**: Treat warnings as errors (passes `-W error` to pytest)

## [0.7.5] - 2026-03-05

### Added

- 2 new edge case tests for pytest summary parser (large numbers, xfail/xpass)
- 1035 tests total (87 new this session)

## [0.7.4] - 2026-03-05

### Added

- **`--html PATH`**: Generate HTML test report (pytest-html compat)
- **`--durations-min SECS`**: Filter durations display by minimum threshold

## [0.7.3] - 2026-03-05

### Added

- 3 new verification tests for --forked, --nf, --setup-plan/show/only
- 1033 tests total (85 new this session)

## [0.7.2] - 2026-03-05

### Added

- **`--forked`**: pytest-forked compatibility flag
- **`--tb=auto`**: Auto-detect traceback style
- **`--nf`**: Run new tests first (pytest-cache compat)

## [0.7.1] - 2026-03-05

### Added

- **`tach check`**: Validate Python, pytest, config, and test discovery
- **`tach stats`**: Show project statistics (tests, async, parametrized, fixtures)
- 10 subcommands total (init, config, markers, fixtures, clean, stats, check, list, version, completions)

## [0.7.0] - 2026-03-05

### Added

- **`--tb=auto`**: Auto-detect traceback style based on context
- **`--nf`**: Run newly added tests first (pytest-cache compat)
- Complete pytest CLI compatibility (~52 flags supported)

## [0.6.6] - 2026-03-05

### Added

- **`--nf` (New First)**: Run newly added tests before existing ones

## [0.6.5] - 2026-03-05

### Added

- **`--setup-plan`**: Show fixture setup plan without executing
- **`--setup-show`**: Show fixture setup/teardown during execution
- **`--setup-only`**: Run only fixture setup, skip tests

## [0.6.4] - 2026-03-05

### Added

- **Quiet Collect Mode**: `--co -q` suppresses test count footer for piping
- **`--pyargs`**: Import test modules by Python package path
- **`--doctest-modules`**: Collect and run doctests from Python modules

## [0.6.3] - 2026-03-05

### Added

- **`--pyargs`**: Import test modules by Python path instead of file path
- **`--randomly-seed SEED`**: Test order randomization (pytest-randomly compat)
- 2 new CLI flag tests (--pyargs, --doctest-modules)

## [0.6.2] - 2026-03-05

### Added

- **`--doctest-modules`**: Collect and run doctests from Python modules
- **`-s` Flag**: Disable output capture (pytest `-s` compat)
- **`--pdb` Flag**: Drop into pdb debugger on failure

## [0.6.1] - 2026-03-05

### Added

- **`--randomly-seed SEED`**: Test order randomization (pytest-randomly compat)
- 3 new CLI flag tests (--randomly-seed, --ignore, --ignore-glob)

## [0.6.0] - 2026-03-05

### Added

- **`--ignore PATH`**: Exclude specific files/directories from test discovery
- **`--ignore-glob GLOB`**: Exclude by glob pattern from test discovery
- Both support multiple values and are wired to pytest harness

## [0.5.9] - 2026-03-05

### Added

- **Enhanced `tach clean`**: Also removes `__pycache__` directories recursively
- **CLI Help Examples**: Added examples for all new commands in `--help` output

## [0.5.8] - 2026-03-04

### Added

- **`-s` Flag**: Disable output capture (pytest `-s` / `--capture=no` compat)
- **`--pdb` Flag**: Drop into pdb debugger on test failure

## [0.5.7] - 2026-03-04

### Added

- **`tach fixtures` Command**: List all discovered fixtures with scope and source file

## [0.5.6] - 2026-03-04

### Added

- **`tach clean` Command**: Remove .tach_cache directory and all cached data
- **`--count` Flag**: Quick test count without running (supports `--format json`)
- **`tach config` Improvements**: Shows configfile path, plugins, database sections
- 2 new CLI tests (--count, combined flags integration)

## [0.5.5] - 2026-03-04

### Added

- **`tach markers` Command**: List all discovered pytest markers from test files
- **Enriched JSON Discovery**: `--co` JSON output now includes fixtures, class_name, timeout
- **`tach config` Improvements**: Shows plugins and database sections, JSON output support
- 2 new JSON serialization tests

## [0.5.4] - 2026-03-04

### Added

- **`--deselect NODE_ID`**: Exclude specific tests by node ID (multiple allowed)
- 11 new verification tests for CLI flags

### Fixed

- **JSON Config Output**: Use serde_json instead of hand-crafted format strings
  to properly escape paths with quotes/backslashes
- **Dependency Detection**: `has_dependency()` now matches exact package names
  instead of substring (prevents `django-debug-toolbar` matching `django`)

## [0.5.3] - 2026-03-04

### Added

- **JSON Config Output**: `tach --format json config` for IDE integration
- **Smart `tach init`**: Auto-detect Django/Flask projects and configure defaults
- **`--runxfail`**: Run xfail tests as normal (pytest compat)
- **`--cache-show`**: Display lastfailed and duration cache contents
- **`--log-file PATH`**: Save test log output to file
- **`--timeout-method signal|thread`**: Timeout implementation method
- **`--color auto|yes|no`**: Control ANSI output with NO_COLOR/FORCE_COLOR
- **`--strict-markers`** and **`--assert`**: Additional pytest-compat flags
- 9 new CLI flag verification tests, config command improvements

### Changed

- `tach config` now shows disabled plugins and database settings sections

## [0.5.2] - 2026-03-04

### Added

- **Scheduler Persistence** (0.6.4): `--resume` flag to continue interrupted runs
- Saves completed test IDs to `.tach_cache/interrupted` on SIGINT
- Clears interrupted cache on successful completion
- 3 new cache tests for interrupted state

### Changed

- Updated roadmap status: 14 items now marked done across phases 5-9

## [0.5.1] - 2026-03-04

### Added

- **`tach config` Command**: Show effective merged configuration for debugging
- **`--cache-show`**: Display lastfailed and duration cache contents
- **`--runxfail`**: Run xfail tests as normal (pytest compat)
- **`--log-file PATH`**: Save test log output to file
- **`--timeout-method signal|thread`**: Timeout implementation method
- **Pytest Summary Parser**: Extracted and tested (8 tests)

### Changed

- Hit **1000 tests** milestone (up from 948 at session start)
- Refactored fallback module with extracted, tested parser function

## [0.5.0] - 2026-03-04

### Added

- **`tach init` Command**: Generate starter `[tool.tach]` config in pyproject.toml
- **`--color auto|yes|no`**: Control ANSI color output (sets NO_COLOR/FORCE_COLOR)
- **`--strict-markers`**: Raise error on unregistered markers
- **`--assert plain|rewrite`**: Control assertion rewriting mode
- **`--basetemp DIR`**: Set pytest temporary directory
- **GITHUB_OUTPUT Integration**: Write structured test results for downstream steps
- **Input Validation**: --import-mode and --assert use clap value_parser for clean errors

## [0.4.3] - 2026-03-04

### Added

- **Session Header Banner**: pytest-style header showing version and platform
- **`--confcutdir DIR`**: Stop conftest.py discovery at directory boundary
- **`-o`/`--override-ini`**: Override ini-file options (pytest -o compat)
- **Graceful pytest Import Error**: Actionable error message when pytest missing

### Changed

- 5 new edge case tests for GitHub reporter (multiframe, empty, sanitization)

## [0.4.2] - 2026-03-04

### Added

- **`--rootdir DIR`**: Override root directory for test discovery (pytest compat)
- **`--import-mode MODE`**: Python import mode (prepend/append/importlib)
- **`--sw` (Stepwise)**: Stop on first failure, resume from that test next run
- **`--no-header`**: Suppress platform/version banner
- **`--co` alias**: Shorthand for `--collect-only`
- **`-p no:PLUGIN`**: Pytest-compatible plugin disable syntax
- 5 verification tests for new CLI features

### Changed

- Extracted `pytest_fallback_retry` into `src/fallback.rs` (main.rs -200 lines)
- main.rs: 1900 -> 1703 lines after cache + fallback extraction

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
