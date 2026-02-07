# Changelog

> **Future Roadmap:** See [docs/research/roadmap.md](docs/research/roadmap.md) for planned versions 0.1.x through 1.0.0+.

---

All notable changes to Project Tach will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.4.0](https://github.com/NikkeTryHard/tach-core/compare/v0.3.1...v0.4.0) (2026-02-07)


### Features

* **discovery:** expand parametrized tests to match pytest collection ([39f52d4](https://github.com/NikkeTryHard/tach-core/commit/39f52d41941844e84c05a4de2e86f723443f5b62))
* **discovery:** parse asyncio_mode from pyproject.toml ([08b6e36](https://github.com/NikkeTryHard/tach-core/commit/08b6e36265a137c4cc2f50b569bb647de0aad365))
* **discovery:** resolve import aliases in parametrize IDs ([0bd87f4](https://github.com/NikkeTryHard/tach-core/commit/0bd87f48993020e3685c5be8a89a03850edcc086))
* **harness:** add AsyncFixtureWrapper for async fixture handling ([2afd3f3](https://github.com/NikkeTryHard/tach-core/commit/2afd3f3ae2879e5f3999214f109aa63921ab89a7))
* **harness:** add scope-aware fixture tracking to AsyncFixtureWrapper ([2a44a0a](https://github.com/NikkeTryHard/tach-core/commit/2a44a0ae234911331c6fd51be7281d464386fb2b))
* **harness:** integrate AsyncFixtureWrapper with EventLoopManager ([afb9e72](https://github.com/NikkeTryHard/tach-core/commit/afb9e7200a7dcff5a14af1c75533c51f132b6949))
* **reporter:** add vitest-style TachReporter with file-grouped output ([d348579](https://github.com/NikkeTryHard/tach-core/commit/d3485794d3875bc94ddece3731ec8979e5636034))
* **reporter:** make TachReporter the default interactive reporter ([b80a78b](https://github.com/NikkeTryHard/tach-core/commit/b80a78b0bc6037ff917fbdfbb4832441c05c6f87))
* **reporter:** vitest-style TachReporter with file-grouped output ([3d9e28f](https://github.com/NikkeTryHard/tach-core/commit/3d9e28fe7e042cef2ab012f3b022cf548728d3d1))
* **signals:** add shutdown watchdog for forced exit on hang ([8049c02](https://github.com/NikkeTryHard/tach-core/commit/8049c028479454bc2cc49e0b8ead913fe8d08567))
* **supervisor:** wire up asyncio config parsing to session effects ([3c239c3](https://github.com/NikkeTryHard/tach-core/commit/3c239c3e57682251d0c27652a5af108305deb8a6))


### Bug Fixes

* **bincode:** serialize MarkerInfo.args as JSON string for bincode compatibility ([84bd96b](https://github.com/NikkeTryHard/tach-core/commit/84bd96b1335de77e2f1ce425e8c30cc9a5d0b3fd))
* **ci:** add integration tests job to CI workflow ([808a550](https://github.com/NikkeTryHard/tach-core/commit/808a550b01b00092552d6f1f93e1e29ae5996a22)), closes [#41](https://github.com/NikkeTryHard/tach-core/issues/41)
* correct skip counting, unicode nodeids, and fixture event loop ([633e3aa](https://github.com/NikkeTryHard/tach-core/commit/633e3aac759e31682b6a8a7d133cd64e5c017646))
* correct skip counting, unicode nodeids, and fixture event loop ([4cc092c](https://github.com/NikkeTryHard/tach-core/commit/4cc092c20f0472ecadd139f376c2ca0299ca903a))
* **discovery:** add allowlist to reduce false positive toxicity ([66e8d8d](https://github.com/NikkeTryHard/tach-core/commit/66e8d8d134e13efd99a6fb3f984af1461f4ff304))
* **discovery:** add Bytes constant support to expr_to_pytest_id ([6387d1b](https://github.com/NikkeTryHard/tach-core/commit/6387d1b81689f4d4eec9a1b2a85fb142c4ee7bf1))
* **discovery:** add Bytes constant support to expr_to_pytest_id ([680a076](https://github.com/NikkeTryHard/tach-core/commit/680a076abd764de3b71142cfe7512d2847b45669))
* **discovery:** use from_utf8_lossy for non-ASCII file handling ([894597b](https://github.com/NikkeTryHard/tach-core/commit/894597b6c41e49baaf97a9af3f393ea1f30856a9))
* **docker:** add mold linker for Rust builds ([860ef6f](https://github.com/NikkeTryHard/tach-core/commit/860ef6f23ada88eefbe1dc28694ccc8b7a7b8774))
* **harness:** add AsyncFixtureWrapper for async fixture handling ([2fb3b34](https://github.com/NikkeTryHard/tach-core/commit/2fb3b3446ff6c051476c61e5eca522d2f52b93a6))
* **harness:** add plugin registration idempotency check ([852dcc2](https://github.com/NikkeTryHard/tach-core/commit/852dcc228ec1b83d01d5ba5e032db0e40de70587))
* **harness:** address audit issues in async scope handling ([f4ae67a](https://github.com/NikkeTryHard/tach-core/commit/f4ae67a3dafdc56b13c964de0e3198906218b07a))
* **harness:** address audit issues in AsyncFixtureWrapper ([4272dad](https://github.com/NikkeTryHard/tach-core/commit/4272dad5db59370238315f58752e30a2b73c9c5b))
* **harness:** align status codes with Rust protocol ([dd1fc10](https://github.com/NikkeTryHard/tach-core/commit/dd1fc10d825bca43c2c44fa65fe789c516752ca4))
* **harness:** call AsyncFixtureWrapper.get_teardown_errors not EventLoopManager ([56eb813](https://github.com/NikkeTryHard/tach-core/commit/56eb8131e64bc08376c0c644037a343799b22eea))
* **harness:** check teardown errors and upgrade status accordingly ([1a0293f](https://github.com/NikkeTryHard/tach-core/commit/1a0293fd0b28bc8608fb0bf718fba1950df04af5))
* **harness:** construct nodeid relative to pytest rootdir ([f41d4d1](https://github.com/NikkeTryHard/tach-core/commit/f41d4d1e1ca66e77f4b9841292cb30d6af9cf262))
* **harness:** construct nodeid relative to pytest rootdir ([a3a7462](https://github.com/NikkeTryHard/tach-core/commit/a3a7462c204c26e049f1f8fbba46ea426c685efc))
* **harness:** consume async fixtures from Zygote cache before test execution ([b4cef54](https://github.com/NikkeTryHard/tach-core/commit/b4cef54d07419cd2c46f09c0a467ee1dcc285317))
* **harness:** consume async fixtures from Zygote cache before test execution ([8ffa818](https://github.com/NikkeTryHard/tach-core/commit/8ffa818815dfb3c87160ecf5b0b49d4fe88d6990))
* **harness:** correctly parse nodeid path component ([2516df3](https://github.com/NikkeTryHard/tach-core/commit/2516df34600c4bdfdf42c116d3e86cb7e5146ba9))
* **harness:** fix pytest import for TachFixturePlugin ([11efbcf](https://github.com/NikkeTryHard/tach-core/commit/11efbcffc8a990656e8475e6f15d396c0bf12679))
* **harness:** prioritize STATUS_ERROR for teardown failures ([905159f](https://github.com/NikkeTryHard/tach-core/commit/905159fe3c00a3356e126398b1b6456d4b6fe743))
* **harness:** reset EventLoopManager and AsyncFixtureWrapper in post_fork_init ([f3a1da6](https://github.com/NikkeTryHard/tach-core/commit/f3a1da628d920a3d46200250da72938086b6b4a4))
* **harness:** restore async fixtures, fuzzy parametrize matching, and node ID alignment ([#72](https://github.com/NikkeTryHard/tach-core/issues/72)) ([904b178](https://github.com/NikkeTryHard/tach-core/commit/904b17839f7eba2a33fe709268dfeb124ce55ad9))
* **harness:** scope-aware async fixture handling ([b295253](https://github.com/NikkeTryHard/tach-core/commit/b2952535e01e49fd4ca3cece044fbe5caac7499d))
* **harness:** set scoped event loop before fixture resolution ([f9a240c](https://github.com/NikkeTryHard/tach-core/commit/f9a240c592471a5a741db6e7c6103251ee5bb7a6))
* **harness:** set scoped loop before runtestprotocol for fixtures ([64cffbb](https://github.com/NikkeTryHard/tach-core/commit/64cffbb9c596ee3530925131f3bb7addf5928104))
* **harness:** stop disabling asyncio/trio plugins in Zygote collection ([1092424](https://github.com/NikkeTryHard/tach-core/commit/10924249f916c0c50b23d04a6f62887b3f8858f1))
* **harness:** use aclose() for async fixture teardown ([f434f22](https://github.com/NikkeTryHard/tach-core/commit/f434f22f13905c664591628d6cb9e153f593ee69))
* issues [#41](https://github.com/NikkeTryHard/tach-core/issues/41), [#42](https://github.com/NikkeTryHard/tach-core/issues/42), [#44](https://github.com/NikkeTryHard/tach-core/issues/44), [#45](https://github.com/NikkeTryHard/tach-core/issues/45) - CI, logging, tests ([f721ab8](https://github.com/NikkeTryHard/tach-core/commit/f721ab89df84bd9c471dbf0e90722fa7ce578cd8))
* **junit:** properly strip OSC escape sequences from error messages ([c5bbdb1](https://github.com/NikkeTryHard/tach-core/commit/c5bbdb145628e5bac9f7a11be4365996c2da2fb1))
* prevent unkillable hang on Ctrl+C ([144827f](https://github.com/NikkeTryHard/tach-core/commit/144827fcfd2c4ade360bf827178f3e1b69a9eaa0))
* **pyproject:** remove deprecated license classifier (PEP 639) ([4091eef](https://github.com/NikkeTryHard/tach-core/commit/4091eefb81c40e7e857e9c4b909ad3565984a636))
* reduce tach test failures with UTF-8 encoding, fuzzy matching, and toxicity allowlist ([8b329fb](https://github.com/NikkeTryHard/tach-core/commit/8b329fb6f04585b214c31297c6cbbf55abcf7785))
* **reporter:** count crash/timeout/error as failures in all reporters ([faee803](https://github.com/NikkeTryHard/tach-core/commit/faee8036a31fec36fb23d8b770f0b913b44950a0))
* **reporter:** distinguish skipped files and show skip counts in per-file output ([f99a7c8](https://github.com/NikkeTryHard/tach-core/commit/f99a7c832aa109331f21f1b83e47aa2a1c2999cc))
* **reporter:** remove inaccurate 'Saved' initialization overhead metric ([3ab27cb](https://github.com/NikkeTryHard/tach-core/commit/3ab27cbf850eaa82dd36e18fee1f42ef00f4784f))
* resolve test count discrepancy between aistudioproxy and tach-core ([b9d6e22](https://github.com/NikkeTryHard/tach-core/commit/b9d6e228763b5182950fb24a8646fc68dcb2d162))
* **scheduler:** add crash detection to capacity-wait loop ([59db68d](https://github.com/NikkeTryHard/tach-core/commit/59db68d3e7419f125fc6e0608c04874ac0c0cabd))
* **scheduler:** add crash detection to capacity-wait loop ([707fadf](https://github.com/NikkeTryHard/tach-core/commit/707fadf0a7d4c418e9f564658742cf7267228354))
* **scheduler:** add timeout to cmd_socket to prevent indefinite hang ([b67f862](https://github.com/NikkeTryHard/tach-core/commit/b67f862f1bc0e982e6e44330a609fd832b4e982f))
* **scheduler:** address all audit issues from end-of-worktree review ([fbc3050](https://github.com/NikkeTryHard/tach-core/commit/fbc30504a7ac8e2a4ad640383cd4849f96dce2f1))
* **scheduler:** count skipped tests separately from failures ([#61](https://github.com/NikkeTryHard/tach-core/issues/61)) ([8d2cd3d](https://github.com/NikkeTryHard/tach-core/commit/8d2cd3d536297921632b5b1b853ef23ce902b07a))
* **signals:** prevent watchdog false positive with SHUTDOWN_COMPLETE flag ([9173bc3](https://github.com/NikkeTryHard/tach-core/commit/9173bc3e0419e76f495cd55b017f7c7a740cdcd6))
* **toxicity:** canonicalize paths in is_toxic() lookup ([0fd85c7](https://github.com/NikkeTryHard/tach-core/commit/0fd85c7d89da230aa260bec60eb92830fc5151ca))

## [Unreleased]

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
