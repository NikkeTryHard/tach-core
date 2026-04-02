# 24-Hour Sprint: tach-core v0.9.0 -> 1.0.0 and Beyond

You are working on tach-core, a hypervisor-accelerated Python test runner written in Rust (v0.9.0, ~47K LOC). It uses userfaultfd memory snapshots for <50us test reset, a zygote/fork server architecture, Landlock+Seccomp sandboxing, and a custom bincode IPC protocol. 1,083 unit tests pass. 27 releases shipped. Django 6.0.3 compatibility proven (8513 passed, 87 failed).

The goal of this sprint is to complete the ENTIRE remaining roadmap -- every unchecked box from v0.9.0 through 1.0.0 production release, then lay the groundwork for post-1.0 features. This is not a "get to the next version" sprint. This is "finish the project."

You have full access to the codebase, all tools, and unlimited agent delegation. Work in parallel waves. Do not stop until every item is verified.

---

## What's Already Done (Do Not Repeat)

- Phase 1 (0.1.x Foundation): COMPLETE -- AST discovery, error codes E001-E020, diagnostics, PyO3 0.27, Rust 2024
- Phase 2 (0.2.x Plugins): COMPLETE -- pytest-django, pytest-asyncio, pytest-mock/env/timeout, Landlock V4-V6, plugin stabilization
- Phase 3 partial: 0.3.0 (Django DB savepoints) + 0.3.1 (SQLAlchemy session management) done
- Phase 4 partial: 0.4.0-0.4.2 done (session/module/class scoped fixture lifecycle with skip_reset)
- Phase 5 partial: 0.5.0 (enhanced tracebacks) + 0.5.1 (debug mode) + 0.5.6 (PEP 669 coverage) done
- Phase 6 mostly done: 0.6.0-0.6.4 done (pyproject.toml, ENV_DENYLIST, toxicity config, scheduler persist)
- Phase 7 partial: 0.7.0 (history store) + 0.7.4 (TLS calibration) + 0.7.5 (adaptive scheduling) done
- Phase 8 done: 0.8.0-0.8.6 done (GitHub Actions, JUnit XML, bench subcommand, ~68 CLI flags)
- Phase 9 partial: 0.9.0 (SIGCHLD crash detection) + 0.9.2 (CleanupGuard) + 0.9.5 (OverlayFS cleanup) + 0.9.7 (protocol versioning) done
- Test sharding (--shard) shipped early in 0.9.0

---

## WAVE 1: Foundation & CI (Hours 0-2)

Before touching any features, lay the infrastructure that validates everything else.

### 1.1 -- Production CI Pipeline

Create `.github/workflows/ci.yml`. This project has NO real CI -- only `fuzz.yml` (weekly) and `labeler.yml` exist. For a v0.9.0 project with 1,083 tests, this is unacceptable.

Matrix build: stable + nightly Rust, x86_64-unknown-linux-gnu. Steps: cargo fmt --check, cargo clippy -- -D warnings, cargo nextest run --lib, cargo nextest run (full suite), cargo bench --no-run (compile check only). Add sccache + cargo registry caching. Add a job that builds the release binary and runs `./target/release/tach-core self-test` inside Docker.

### 1.2 -- Honest Benchmarks

`benches/hot_paths.rs` benchmarks JSON parsing as a proxy -- but tach uses bincode, not JSON. Rewrite to benchmark actual `TestPayload`/`TestResult` bincode roundtrips, protocol frame encode/decode with the real 8-byte header (magic + version + length), scheduler dispatch decisions for the dual-queue (safe vs toxic), and fixture resolution graph traversal. These are the REAL hot paths.

`benches/plugin_overhead.rs` exists but should also benchmark the UFFD page fault handler latency (simulated with `madvise(MADV_DONTNEED)` + `mmap` access timing). Capture the baseline number that Wave 3 will improve.

---

## WAVE 2: Database Integration Completion (Hours 2-5) -- Phase 3 Remaining

### 2.1 -- Connection Management with FD Teleportation (Roadmap 0.3.2)

The plugin_bridge (`src/execution/plugin_bridge.rs`) already implements SCM_RIGHTS for FD handover. Wire it up for database connections:

- Capture database connection file descriptors from the zygote process
- Pass FDs to worker processes via Unix domain sockets using SCM_RIGHTS
- Reconstruct connection objects from FDs in the Python harness
- Handle SSL connections specially (SSL state can't survive fork -- need to renegotiate)
- Connection pool size limits, health checks, automatic reconnection on failure
- Support connection aging (close connections older than N seconds)

The research paper says: "Ensure that any connection pool created in the parent is explicitly discarded in the child process immediately after startup" -- implement exactly this.

### 2.2 -- Additional Database Support (Roadmap 0.3.3)

PostgreSQL: native savepoints, advisory lock cleanup, LISTEN/NOTIFY cleanup, temp table handling, pg_dump/pg_restore for fixture loading.

MySQL/MariaDB: savepoints, MySQL-specific locking patterns, charset handling, MySQL 8.0+ feature support.

SQLite: in-memory database optimization (fastest path), file-based database snapshotting via filesystem copy, WAL mode handling, shared cache mode support.

MongoDB (experimental): PyMongo session hooks, transaction support (requires replica set), collection cleanup for non-transactional mode, Motor (async) support.

Redis (experimental): transaction support, pub/sub cleanup, cluster support, connection pool management.

gRPC fork safety: auto-detect gRPC usage, set `GRPC_ENABLE_FORK_SUPPORT=1`, verify `epoll1` polling engine, warn if active RPCs detected before fork.

---

## WAVE 3: Memory Engine Upgrades (Hours 5-10) -- Phase 7 Remaining

This is the hardest wave. The snapshot engine is the soul of tach.

### 3.1 -- Snapshot Compression (Roadmap 0.7.1)

Golden pages consume `N_pages * 4KB` resident memory. A Django app with 100MB+ imports = 25K+ pages = 100MB just for the golden copy. Implement LZ4 compression of golden pages using `lz4_flex` crate (pure Rust, no C dependency). Compress on capture, decompress lazily on `UFFDIO_COPY`. Measure the tradeoff: LZ4 decompresses at ~4 GB/s, so a 4KB page decompresses in ~1us. If golden pages compress 3:1, we save 66% memory at ~1us latency cost per page fault. That's worth it.

### 3.2 -- UFFD Write-Protect Dirty Page Tracking (Roadmap 0.7.2)

Currently tach does a full `madvise(MADV_DONTNEED)` on ALL registered memory regions and restores every page on access. Most tests only dirty a handful of pages out of thousands. Implement `UFFDIO_WRITEPROTECT`:

1. After capturing golden pages, mark all regions write-protected via `UFFDIO_WRITEPROTECT`
2. When a test writes to a protected page, UFFD generates a write-protect fault
3. Record the faulting address in a per-worker dirty set (use a `HashSet<usize>` keyed by page-aligned address)
4. Remove write protection on that page so the write completes
5. On reset, only `madvise(MADV_DONTNEED)` the dirty pages, then re-protect everything

The userfaultfd crate (0.9) supports `WriteProtect` mode via `Uffd::write_protect()`. Wire through `src/isolation/snapshot.rs`.

Target: 5-10x reduction in reset overhead for typical tests that touch <5% of pages.

### 3.3 -- Vectorized Restore (Roadmap 0.7.3)

After 3.2, batch dirty page restoration. Instead of sequential `UFFDIO_COPY` calls (one syscall per page), use `UFFDIO_COPY` with `UFFDIO_COPY_MODE_DONTWAKE` for all pages except the last, then issue a single `UFFDIO_WAKE` to resume the worker. This reduces syscall count from N to N+1 (where the +1 is the wake). For a test that dirtied 50 pages, this cuts 50 context switches to ~1.

Alternative: investigate `process_vm_writev` for cross-process memory writes without page fault handling.

### 3.4 -- UFFD Event Tracking (Roadmap 0.7.8 + 0.7.9)

Track `UFFD_EVENT_FORK` -- when a worker internally calls `fork()` (e.g., `subprocess.run()`), the child inherits the UFFD registrations. The supervisor needs to handle this: either track the child's UFFD events too, or mark the test as toxic and switch to fork/kill mode.

Track `UFFD_EVENT_REMAP` -- when a worker calls `mremap()`, the memory mapping changes. The supervisor must update its golden page registry to reflect the new mapping.

### 3.5 -- Lazy Module Loading (Roadmap 0.7.6)

Don't import all modules at zygote initialization time. Instead, load test modules on-demand when a worker receives a test for that module. The zero-copy loader (`src/discovery/loader.rs`) already supports this architecture -- it has a global registry and `load_module()` FFI. Wire it into the execution path: worker receives `TestPayload` -> checks if module is loaded -> if not, calls `load_module()` -> runs test.

This reduces zygote startup time and memory for projects with many test files where each test run only executes a subset.

### 3.6 -- Advanced Snapshots Research (Roadmap 0.7.7)

Evaluate AFL-Snapshot-LKM for kernel-level snapshots targeting 1-5us reset time. This hooks `do_wp_page` and `page_add_new_anon_rmap` at kernel level. Document: feasibility for tach, GPL licensing implications (tach is MIT -- kernel module must be separate optional download), kernel version compatibility matrix, benchmark data vs userfaultfd approach. This is RESEARCH -- prototype and measure, don't ship.

---

## WAVE 4: Fixture Completeness (Hours 10-13) -- Phase 4 Remaining

### 4.1 -- Autouse Fixture Injection (Roadmap 0.4.3)

The scanner (`src/discovery/scanner.rs`) already detects `autouse=True` in fixture decorators. The resolver (`src/discovery/resolver.rs`) does NOT auto-inject them. Fix the pipeline:

1. When building `RunnableTest` in the resolver, scan all visible fixtures: same file, parent conftest.py chain (conftest inheritance is already implemented in `src/discovery/inheritance.rs`)
2. Filter for `autouse=True` fixtures
3. Inject them into the test's fixture dependency set
4. Respect scope ordering: session > module > class > function
5. Handle autouse fixtures that depend on non-autouse fixtures (transitive deps)
6. Handle autouse in nested conftest.py directories (more specific conftest overrides parent)

Write integration tests in `rust_tests/` with conftest.py autouse fixtures at multiple directory levels. Add Python gauntlet tests in `tests/`.

### 4.2 -- Parametrized Fixtures (Roadmap 0.4.4)

`@pytest.fixture(params=[1, 2, 3])` should expand tests. The plan doc exists at `docs/plans/2026-02-02-fix-parametrize-discovery.md` -- partially implemented for test-level parametrize but NOT for fixture params.

1. AST scanner: extract `params=` keyword argument from `@pytest.fixture()` decorators
2. Store params in the `FixtureInfo` struct alongside name, scope, autouse
3. Resolver: when a test depends on a parametrized fixture, generate N `RunnableTest` variants -- `test_foo[param0]`, `test_foo[param1]`, etc.
4. Handle fixture param IDs (`ids=` kwarg in the decorator)
5. Support indirect parametrization (`@pytest.mark.parametrize("fixture_name", [...], indirect=True)`)
6. Support fixture param marks (`@pytest.fixture(params=[pytest.param(1, marks=pytest.mark.slow)])`)
7. Wire through protocol: `TestPayload` needs to carry the fixture param value
8. Wire through harness: `tach_harness.py` needs to inject the param value into the fixture function

### 4.3 -- Fixture Finalization Order

Build a proper fixture dependency graph (leveraging the existing hook graph infrastructure in `src/hooks/graph.rs`). Teardown in reverse dependency order. Handle `yield` fixtures correctly (everything after `yield` is teardown). Handle generator fixtures. Detect and error on circular dependencies.

### 4.4 -- Zygote Warmup (Roadmap 0.4.5)

Add `[tool.tach.warmup]` config section:

```toml
[tool.tach.warmup]
imports = ["django", "numpy", "pandas"]  # Pre-import in zygote
```

The zygote process imports these modules before forking workers. Workers inherit the warm import cache via CoW. This eliminates the import tax for commonly-used heavy libraries.

### 4.5 -- Zygote Pool (Roadmap 0.4.6)

Implement per-scope zygote pools using the DAAC (Dependency-Aware Agglomerative Clustering) algorithm from the Forklift paper:

1. Analyze test import graphs to cluster tests by shared dependencies
2. Create specialized zygotes for each cluster (e.g., a "Django zygote" that pre-imports Django, a "data science zygote" that pre-imports numpy/pandas)
3. Route tests to the appropriate zygote based on their dependency cluster
4. Lazy creation: zygotes spawn on first use, not all upfront
5. Eviction under memory pressure

### 4.6 -- Fixture Visualization

Add `--fixtures` flag: list all available fixtures with scope, autouse status, and definition location. Add `--fixture-graph` flag: export fixture dependency graph as Mermaid or DOT format. Show which fixtures are used by which tests.

---

## WAVE 5: Developer Experience (Hours 13-17) -- Phase 5 Remaining

### 5.1 -- Interactive Debugging (Roadmap 0.5.2)

The debugger infrastructure exists at `src/reporting/debugger.rs` with PTY proxy. Wire end-to-end:

1. `--pdb` flag: drop into pdb on first test failure
2. `--pdb-first` flag: drop into pdb on first failure only, then continue
3. Detect `breakpoint()` calls in test code (the harness already hooks `sys.breakpointhook`)
4. When a worker hits a breakpoint: pause execution, send `MSG_DEBUG_REQUEST` to supervisor
5. Supervisor pauses the scheduler (no new test dispatches), connects the worker's PTY to the user's terminal using raw mode
6. User interacts with pdb/ipdb/pudb
7. On continue/quit: supervisor disconnects PTY, resumes scheduler, worker continues/exits
8. Support `pytest.set_trace()` equivalent
9. Support post-mortem debugging (capture exception state, allow inspection after crash)
10. Document VS Code launch configurations for attaching to tach workers
11. Support DAP (Debug Adapter Protocol) for IDE integration

### 5.2 -- Watch Mode Enhancements (Roadmap 0.5.3 + 0.5.4)

Current watch mode (`src/execution/watch.rs`) re-runs everything on file change.

1. Targeted re-discovery: when a file changes, use the toxicity graph to identify which tests are affected. Only re-scan changed files and their dependents.
2. `.tachignore` support: gitignore-style file exclusion for watch mode. Default ignore: `__pycache__`, `.git`, `.venv`, `node_modules`, `.mypy_cache`, `.pytest_cache`
3. `--watch-delay` flag: debounce rapid saves (default 300ms)
4. "Waiting for changes..." display with the ratatui reporter
5. Show which files changed and which tests will re-run
6. Support `--watch-filter` to only watch specific directories

### 5.3 -- Log Capture (Roadmap 0.5.5)

Capture stdout/stderr per test (the infrastructure exists in `src/reporting/logcapture.rs`). Show captured output only for failing tests. Support `--capture=no` to disable capture. Support `--capture=sys` vs `--capture=fd` modes. Parse structured log output (JSON logs) and display formatted.

### 5.4 -- Coverage Optimization (Roadmap 0.5.4)

Implement SlipCover-style de-instrumentation for PEP 669 coverage:

1. After a line is covered once, use `sys.monitoring.DISABLE` return value to turn off the event for that line
2. De-instrument branches after both paths are covered
3. Hot-path detection: if a file is fully covered, disable all monitoring for it
4. Incremental coverage mode: only instrument files changed since last run (using mtime/hash from the cache)
5. Target: <5% overhead for typical test suites (benchmark against coverage.py and SlipCover)

### 5.5 -- Assertion Introspection

When `assert a == b` fails, pytest shows a rich diff with sub-expression values. Tach currently shows raw AssertionError. Implement assertion rewriting in the harness:

1. Intercept `assert` statements via AST transformation (pytest uses `_pytest.assertion.rewrite`)
2. Capture LHS and RHS values for comparison operators
3. Show sub-expression values for complex assertions (`assert len(x) == 3`)
4. Generate diffs for string, dict, and list comparisons (unified diff format)
5. Color-code additions/deletions
6. Include introspection data in `TestResult.message` field in the protocol

### 5.6 -- Output Customization (Roadmap 0.5.3)

- `--color=auto/always/never`
- `--no-header` for minimal output
- `--quiet` for summary only
- Verbose levels: `-v`, `-vv`, `-vvv`
- Progress styles: bar, dots, verbose, none (`--no-progress` for CI)
- Show ETA for test completion based on historical durations
- Show test rate (tests/second)

---

## WAVE 6: Configuration Completion (Hours 17-18) -- Phase 6 Remaining

### 6.1 -- Config Profiles (Roadmap 0.6.5)

```toml
[tool.tach.profiles.ci]
workers = "auto"
timeout = 120
exitfirst = true
coverage = true
format = "junit"

[tool.tach.profiles.dev]
workers = 4
timeout = 30
coverage = false
format = "progress"
```

Switch via `--profile ci` or `--profile dev`. Support profile inheritance (`extends = "default"`). Auto-detect CI environment (check `CI`, `GITHUB_ACTIONS`, `GITLAB_CI`, `JENKINS_URL` env vars) and apply CI profile automatically.

### 6.2 -- Plugin Config (Roadmap 0.6.3)

Allow users to configure plugin behavior:

```toml
[tool.tach.plugins]
disabled = ["pytest-cov"]  # Disable specific plugins
priority = { "pytest-django" = 100, "pytest-asyncio" = 90 }
```

Detect pytest-cov and warn about tach's native coverage (`--coverage`). Detect pytest-xdist and warn about tach's native parallelism. Support `-n` flag as alias for `--workers` for xdist muscle memory.

---

## WAVE 7: CI/CD & Reporting (Hours 18-20) -- Phase 8 Remaining

### 7.1 -- Coverage Formats (Roadmap 0.8.3)

The PEP 669 ring buffer coverage engine (`src/reporting/coverage.rs`) produces LCOV.

Add Cobertura XML: line-rate, branch-rate, package/class/method breakdowns. This is the standard for GitLab CI, Azure DevOps, SonarQube.

Add HTML report: self-contained single HTML file with embedded CSS/JS, per-file line-by-line coverage with syntax highlighting. Use `syntect` for highlighting. Zero-dependency coverage viewer.

Add JSON format: machine-readable coverage data for custom tooling.

Add SonarQube format: for SonarQube/SonarCloud integration.

Add coverage diff mode: show coverage for only new/changed lines (`--coverage-diff`). Integrates with `git diff` to identify changed lines.

Add coverage thresholds: `--coverage-fail-under 80` fails the run if total coverage is below threshold.

### 7.2 -- Other CI Platforms (Roadmap 0.8.2)

Create CI templates:
- `.gitlab-ci.yml` template with JUnit and coverage integration
- `Jenkinsfile` pipeline library
- Azure DevOps YAML template
- CircleCI orb definition
- Buildkite plugin

### 7.3 -- Flaky Test Detection

The cache (`src/cache.rs`) tracks test history with durations and pass/fail.

1. When a test flips between pass/fail across runs, flag it as flaky
2. Store flaky history: first_seen, last_seen, flip_count, total_runs
3. `--flaky-retries N` flag: automatically re-run flaky tests up to N times before failing
4. Add `flaky` field to `TestResult` in the protocol
5. Show flaky stats in the summary reporter (e.g., "3 tests are flaky (retried successfully)")
6. Output flaky test report in JUnit XML (using the `flakyFailure` extension)
7. `--flaky-report` flag: generate a standalone flaky test report

### 7.4 -- Test Impact Analysis (--affected mode)

The killer feature. Build on the PEP 669 coverage data:

1. After each test run, persist a mapping: `source_file -> set(test_ids_that_exercised_it)` in the SQLite cache
2. When `--affected` is passed: run `git diff --name-only` (or accept explicit file list), look up which tests cover those files, run ONLY those tests
3. Support `--affected --commit-range HEAD~5..HEAD` for CI (only run tests affected by the PR's changes)
4. Cache the dependency mapping with file content hashes for invalidation
5. Provide `--affected-fallback=all` (run all tests if cache miss) or `--affected-fallback=none` (run nothing)
6. Show which source changes triggered which tests

This is what snob does as its core feature -- tach should have it built-in, powered by its superior coverage engine.

### 7.5 -- Sub-Interpreter Architecture (Roadmap 0.8.4-0.8.6)

Design and prototype sub-interpreter workers using PEP 684 (per-interpreter GIL):

1. Architecture design doc: how sub-interpreters fit alongside the zygote model (hybrid approach -- sub-interpreters for safe tests, fork for toxic)
2. Prototype: create sub-interpreter pool via `Py_NewInterpreterFromConfig` with `PyInterpreterConfig_OWN_GIL`
3. Channel-based communication between interpreters (no direct object sharing -- use `interpreters.Queue` or shared memory)
4. Benchmark against fork-based workers
5. Document C-extension compatibility requirements (many C extensions don't support sub-interpreters yet)
6. PEP 734 integration (Python 3.14+): use `concurrent.interpreters` when available
7. Module state re-initialization on reset (the "sub-interpreter reset" problem)

---

## WAVE 8: Stability & Hardening (Hours 20-22) -- Phase 9 Remaining

### 8.1 -- Signal Routing (Roadmap 0.9.1)

Complete signal handling:
- SIGINT (Ctrl+C): graceful shutdown with progress summary (partially done in `src/core/signals.rs`)
- SIGTERM: clean exit with partial results
- SIGHUP: reload configuration without restart
- SIGQUIT: dump all worker stack traces (Python tracebacks) to stderr
- SIGUSR1: status dump (worker count, queue depth, tests completed, ETA)
- Forward signals to workers, timeout on worker shutdown, force kill unresponsive workers after 5s

### 8.2 -- UFFD FD Limits (Roadmap 0.9.3)

Track per-worker UFFD file descriptor usage. Each worker gets one UFFD fd from the supervisor. Monitor total FD count against the system limit (`/proc/sys/fs/file-max`). Warn when approaching 80% of the limit. Reduce worker count if FD exhaustion is imminent.

### 8.3 -- Snapshot Memory Budget (Roadmap 0.9.4)

Golden pages have a memory cost. Implement a budget system:

1. Track total golden page memory: `num_pages * 4KB` (or compressed size after Wave 3.1)
2. `--snapshot-budget 512M` flag: maximum memory for golden pages
3. When budget is exceeded: fall back to fork/kill mode for additional workers
4. Report snapshot memory usage in `--debug` output
5. Optimize: share identical golden pages across workers using content-addressed deduplication

### 8.4 -- Seccomp Limits (Roadmap 0.9.6)

The Seccomp BPF filter (`src/isolation/sandbox.rs`) blocks 22 syscalls. BPF programs have an instruction count limit (4096 for classic, larger for extended). Validate that the tach filter stays within limits. Add a diagnostic check in `tach self-test`. If the filter is too complex, split into multiple filters or use extended BPF.

### 8.5 -- Stress Testing at Scale (Roadmap 0.9.8)

Create a stress test harness:

1. Generate 10,000+ synthetic Python test files with realistic patterns:
   - Parametrized tests with 5-20 params each
   - Class-based tests with setup/teardown
   - Async tests with asyncio fixtures
   - Conftest.py hierarchies 5 levels deep
   - Tests that import heavy modules (mock the import time)
   - Tests that write to disk, create temp files, use subprocess
   - Tests that intentionally fail, skip, xfail
2. Run tach against the generated suite
3. Measure and validate:
   - No scheduler starvation (all tests complete)
   - No FD exhaustion (FDs are cleaned up)
   - No OOM from golden page accumulation (budget system works)
   - No worker pool deadlocks (all workers eventually become ready)
   - No protocol desync (all messages parse correctly)
   - Wall-clock time scales linearly with test count (not quadratically)
4. Run under high parallelism (64+ workers) to stress the SIGCHLD handler and worker pool
5. Run with --coverage enabled to stress the ring buffer under load

---

## WAVE 9: Beta & Release Candidates (Hours 22-23) -- Phase 10

### 9.1 -- Beta 1 (0.10.0)

- Feature freeze: no new features after this point
- API stability review: review all public CLI flags, config keys, and protocol messages for naming consistency
- Complete documentation sweep: update all `docs/architecture/*.md` files to reflect changes from this sprint
- Update `docs/research/roadmap.md`: mark all completed items with checkboxes
- Migration guide draft: document how to migrate from pytest to tach (differences, limitations, workarounds)
- Public beta announcement draft: write release notes for GitHub

### 9.2 -- Beta 1 Fixes (0.10.1)

- Run the full Django 6.0.3 test suite again. Target: <20 failures (down from 87)
- Triage each failure: is it a tach bug or a Django-internal issue?
- Fix all tach-caused failures
- Performance regression testing: compare benchmarks from Wave 1.2 against current numbers
- Security audit: review all `unsafe` blocks in the codebase, verify Landlock/Seccomp coverage

### 9.3 -- Beta 2 (0.11.0)

- Address beta 1 feedback (simulated: re-run all tests, fix any regressions)
- Final API changes based on dogfooding
- Documentation updates for any API changes
- Performance optimization: profile with flamegraphs, optimize the top 3 hotspots

### 9.4 -- Release Candidates (0.11.1 + 0.11.2)

- RC1: final bug fixes, release notes, upgrade path testing
- RC2: critical fixes only, final documentation, package verification
- Protocol fuzz hardening: run the fuzz targets in `fuzz/` for 30+ minutes against protocol parser, discovery scanner, and config parser. Fix any crashes found. A protocol parser crash is a security vulnerability since workers are sandboxed but the supervisor isn't.

---

## WAVE 10: 1.0.0 Production Release (Hour 23)

### 10.1 -- Release Checklist

- [ ] All 1,083+ unit tests pass (should be significantly more by now)
- [ ] All integration tests pass
- [ ] Django 6.0.3: <20 test failures
- [ ] Stress test: 10K+ tests complete successfully
- [ ] Benchmarks published: reset latency, throughput, memory usage
- [ ] Complete user documentation at `docs/`
- [ ] Migration guide from pytest
- [ ] API stability commitment (SemVer)
- [ ] Long-term support policy documented
- [ ] Security best practices documented
- [ ] CHANGELOG.md updated with all changes from v0.9.0 through v1.0.0
- [ ] Version bumped in Cargo.toml
- [ ] Git tag v1.0.0
- [ ] Release notes on GitHub

### 10.2 -- 1.0.0 Deliverables

The 1.0.0 release represents:
- Full pytest compatibility for 95%+ of real-world test suites
- Sub-50us memory reset (sub-10us with write-protect dirty tracking)
- Complete fixture lifecycle (session/module/class/function + autouse + parametrized)
- Database integration (Django, SQLAlchemy, PostgreSQL, MySQL, SQLite)
- Production CI/CD integration (GitHub, GitLab, Jenkins, Azure, CircleCI)
- Coverage in 5 formats (LCOV, Cobertura, HTML, JSON, SonarQube)
- Test Impact Analysis (--affected)
- Flaky test detection with auto-retry
- Interactive debugging (pdb/ipdb/pudb)
- Watch mode with targeted re-runs
- Sub-interpreter experimental support
- Proven stability at 10K+ tests

---

## WAVE 11: Post-1.0 Groundwork (Hour 24) -- Future Phases

Use the remaining time to lay foundations for the post-1.0 roadmap. These are stubs and architecture designs, not full implementations.

### 11.1 -- Remote/Distributed Execution Architecture (Post-1.0: 0.12.x)

Design doc for Maelstrom-style distributed execution:
- Broker/worker architecture: central broker distributes tests to remote workers
- Workers are tach instances on different machines
- Tests packaged as OCI-like container images for reproducibility
- Cross-node result aggregation back to the local supervisor
- Network protocol design: extend the existing bincode IPC protocol for TCP/TLS transport
- Worker discovery: mDNS for local network, explicit config for CI farms

### 11.2 -- pytest Plugin Mode Architecture (Future Vision)

Design for `pytest-tach` plugin that lets users adopt incrementally:
```bash
pip install pytest-tach
pytest --tach .  # Uses tach engine under the hood
```
- Plugin registers as a pytest plugin via entry points
- Overrides pytest's execution backend while keeping pytest's collection
- Transparent to existing pytest configs, markers, and plugins
- Graceful degradation on non-Linux platforms (fall back to normal pytest execution)

### 11.3 -- Cross-Platform Research Stubs (Future Vision)

macOS: `mach_vm_remap` for CoW memory cloning. Document API surface, security implications (SIP restrictions), estimated performance (~80% of Linux userfaultfd).

Windows: NT Section Objects for shared memory. Document API (`NtCreateSection`, `NtMapViewOfSection`), security model, WSL2 Hyper-V backend as fallback.

Graceful degradation: on unsupported platforms, fall back to process-per-test mode. Still faster than pytest because of the Rust supervisor, AST discovery, and zero-copy loading.

### 11.4 -- Advanced Testing Modes (Post-1.0: 0.14.x-0.20.x)

Write architecture stubs for:

- **Visual Testing (0.14.x)**: Playwright snapshot integration, screenshot comparison
- **AI-Powered (0.15.x)**: ML-based flaky detection (feature extraction from test history), AI test generation from code coverage gaps, predictive scheduling (ML-based test ordering for fastest feedback)
- **Mutation Testing (0.16.x)**: AST-based code mutation, parallel mutant execution using the snapshot engine (each mutant = one snapshot cycle), mutation score reporting
- **Property-Based Testing (0.17.x)**: Hypothesis integration, snapshot between property iterations for massive speedup
- **Contract Testing (0.18.x)**: OpenAPI/GraphQL schema validation, API contract verification
- **Benchmarking (0.19.x)**: `@pytest.mark.benchmark` marker, statistical analysis (warmup, outliers, significance), regression detection across runs
- **Observability (0.20.x)**: OpenTelemetry spans for test execution, Prometheus metrics endpoint, Grafana dashboard templates

### 11.5 -- `tach init` Command

First-run experience:
1. Detect Python project type: Django (`manage.py`, `DJANGO_SETTINGS_MODULE`), Flask (`app.py`, `flask`), FastAPI (`main.py`, `fastapi`), plain
2. Generate `[tool.tach]` section in `pyproject.toml` with sane defaults per project type
3. Run `tach self-test` to verify kernel features
4. Show capability matrix: what works (userfaultfd, Landlock, Seccomp) and what doesn't
5. Run `tach --dry-run .` to show discovered tests
6. Show "Ready to go!" message with the command to run

---

## Success Criteria

At the end of 24 hours, tach-core should have:

**Infrastructure:**
- [ ] Real CI pipeline catching regressions on every push
- [ ] Honest benchmarks measuring actual hot paths (bincode, UFFD, scheduler)
- [ ] Proven stability at 10,000+ tests

**Core Engine:**
- [ ] LZ4-compressed golden pages (66% memory savings)
- [ ] Write-protect dirty page tracking (5-10x faster reset)
- [ ] Vectorized page restore (batch syscalls)
- [ ] UFFD_EVENT_FORK/REMAP tracking
- [ ] Lazy module loading
- [ ] Adaptive worker pool scaling with memory pressure handling
- [ ] Snapshot memory budget system

**Compatibility:**
- [ ] Complete fixture support: autouse + parametrized + finalization order + visualization
- [ ] Zygote warmup and pooling (DAAC clustering)
- [ ] Database support: PostgreSQL, MySQL, SQLite, MongoDB, Redis + FD teleportation
- [ ] gRPC fork safety
- [ ] <20 Django 6.0.3 test failures (down from 87)

**Developer Experience:**
- [ ] Interactive pdb/ipdb/pudb debugging end-to-end
- [ ] Watch mode with targeted re-runs and .tachignore
- [ ] Assertion introspection with rich diffs
- [ ] Log capture per test
- [ ] Coverage de-instrumentation (<5% overhead)
- [ ] `tach init` first-run experience
- [ ] Output customization (color, verbosity, progress styles)

**Reporting & CI:**
- [ ] Coverage in 5 formats: LCOV, Cobertura, HTML, JSON, SonarQube
- [ ] Coverage diff mode and thresholds
- [ ] Flaky test detection with auto-retry
- [ ] Test Impact Analysis (--affected)
- [ ] CI templates for GitHub, GitLab, Jenkins, Azure, CircleCI
- [ ] Sub-interpreter experimental support

**Stability:**
- [ ] Complete signal routing (SIGINT, SIGTERM, SIGHUP, SIGQUIT, SIGUSR1)
- [ ] UFFD FD limit tracking
- [ ] Seccomp BPF instruction count validation
- [ ] Protocol fuzz hardening (30+ min fuzzing, zero crashes)
- [ ] Config profiles with CI auto-detection

**Release:**
- [ ] v0.10.0 Beta 1 tagged
- [ ] v0.11.0 Beta 2 tagged
- [ ] v0.11.1 RC1 + v0.11.2 RC2 tagged
- [ ] v1.0.0 Production tagged and released
- [ ] Complete documentation and migration guide
- [ ] API stability commitment (SemVer)

**Post-1.0 Groundwork:**
- [ ] Distributed execution architecture design
- [ ] pytest plugin mode architecture design
- [ ] Cross-platform research stubs (macOS, Windows)
- [ ] Architecture stubs for 7 future testing modes

**This sprint takes tach from v0.9.0 to 1.0.0 production release with post-1.0 foundations laid. Every unchecked box in the roadmap gets checked. Every gap gets filled. The project ships.**
