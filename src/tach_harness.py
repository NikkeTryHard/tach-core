# tach_harness.py - Embedded Python Harness for Tach
# This module is loaded directly into the worker process to execute tests.
# DO NOT MODIFY: This file is embedded via include_str! in zygote.rs

import sys
import time
import traceback
import asyncio
import inspect
import _pytest.runner
import _pytest.main
import _pytest.config

# Status codes (must match protocol.rs)
STATUS_PASS = 0
STATUS_FAIL = 1
STATUS_SKIP = 2
STATUS_CRASH = 3
STATUS_HARNESS_ERROR = 4


def inject_entropy():
    """Re-seed RNGs to break the Clone Curse.

    Workers inherit Zygote's PRNG state via fork(). Without re-seeding,
    all workers generate identical random sequences, causing "hidden flaky tests"
    that pass locally but fail in production.

    We re-seed using high-resolution time to ensure each worker gets unique
    random sequences.
    """
    import random

    # Use nanosecond timestamp for high entropy
    seed = time.time_ns() % (2**32)
    random.seed(seed)

    # Optional: Re-seed numpy if present
    if "numpy" in sys.modules:
        try:
            sys.modules["numpy"].random.seed(seed)
        except Exception:
            pass

    # Optional: Re-seed torch if present
    if "torch" in sys.modules:
        try:
            sys.modules["torch"].manual_seed(seed)
        except Exception:
            pass


def run_test(file_path: str, node_id: str) -> tuple:
    """
    Execute a single pytest test item.

    Args:
        file_path: Path to the test file (relative to project root)
        node_id: The FULL node identifier (e.g., "tests/test_foo.py::test_bar")
                 This must match pytest's nodeid exactly.

    Returns:
        (status_code, duration_seconds, message)
    """
    # Inject entropy FIRST to break the Clone Curse
    inject_entropy()

    start = time.perf_counter()

    try:
        # 1. Initialize pytest config with minimal plugins
        # -s: disable capture (we use memfd), -p no:terminal: disable terminal reporter
        # --collect-only would skip execution, we don't use it here
        # Disable plugins: terminal (we use memfd), cacheprovider, and async plugins
        # CRITICAL: no:asyncio and no:trio ensure WE own async execution, not plugins
        # CRITICAL: no:django ensures WE own DB isolation, not pytest-django
        args = [
            file_path,
            "-s",
            "-p",
            "no:terminal",
            "-p",
            "no:cacheprovider",
            "-p",
            "no:asyncio",  # Disable pytest-asyncio to own async execution
            "-p",
            "no:trio",  # Disable pytest-trio to prevent conflicts
            "-p",
            "no:django",  # Disable pytest-django to own DB isolation
        ]

        # Use _prepareconfig which properly initializes all default plugins
        # This is the same function pytest.main() uses internally
        cfg = _pytest.config._prepareconfig(args)

        # Critical: run _do_configure to set up all stash keys and hooks
        # This matches what wrap_session does in pytest.main()
        cfg._do_configure()

        # 2. Create Session and perform surgical collection
        session = _pytest.main.Session.from_config(cfg)
        cfg.hook.pytest_sessionstart(session=session)

        # Collect only this file
        session.perform_collect([file_path])

        # 3. Find the specific test item by EXACT node_id match
        # This avoids ambiguity (e.g., test_bar vs test_foo_bar)
        target_item = None
        for item in session.items:
            if item.nodeid == node_id:
                target_item = item
                break

        if not target_item:
            duration = time.perf_counter() - start
            # Provide helpful debug info
            collected_ids = [item.nodeid for item in session.items]
            return (
                STATUS_HARNESS_ERROR,
                duration,
                f"Test not found: {node_id}\nCollected: {collected_ids}",
            )

        # 4. Native Async Support: Wrap coroutine functions with event loop
        # We detect if the test function is a coroutine and wrap it ourselves
        # This implements the "Batteries-Included" philosophy - no pytest-asyncio needed
        original_obj = target_item.obj

        # Handle both regular functions and bound methods
        # Bound methods wrap the underlying function, so we need to check __func__
        func_to_check = original_obj
        if hasattr(original_obj, "__func__"):
            # Bound method - get the underlying function for coroutine check
            func_to_check = original_obj.__func__

        if inspect.iscoroutinefunction(func_to_check):
            # Create a sync wrapper that runs the coroutine in a fresh event loop
            # Fresh loop per test ensures isolation - no state leakage between tests
            def make_sync_wrapper(async_fn):
                def sync_wrapper(*args, **kwargs):
                    # Create a fresh event loop for this test (isolation)
                    loop = asyncio.new_event_loop()
                    asyncio.set_event_loop(loop)
                    try:
                        # Run the coroutine to completion
                        return loop.run_until_complete(async_fn(*args, **kwargs))
                    finally:
                        # Cleanup: close loop to prevent resource leaks
                        loop.close()
                        asyncio.set_event_loop(None)

                return sync_wrapper

            # Replace the test function with our sync wrapper
            target_item.obj = make_sync_wrapper(original_obj)

        # 4.5. Django Transaction Isolation: Wrap test in atomic block with rollback
        # This ensures DB changes are rolled back after each test (isolation!)
        django_atomics = []
        if "django" in sys.modules:
            try:
                from django.conf import settings

                # Only attempt DB operations if Django is properly configured
                if settings.configured:
                    from django.db import connections, transaction

                    # CRITICAL: Close connections inherited from Zygote and reopen fresh
                    # SQLite connections don't survive fork() properly - they get corrupted
                    # Closing all connections forces Django to create new ones for this worker
                    try:
                        connections.close_all()
                    except Exception:
                        pass  # Connection might not exist yet

                    # Enter atomic block for ALL database connections
                    for alias in connections:
                        try:
                            atomic = transaction.atomic(using=alias)
                            atomic.__enter__()
                            django_atomics.append((alias, atomic))
                        except Exception:
                            pass  # Connection might not be available
            except ImportError:
                pass

        try:
            # 5. Execute test (setup -> call -> teardown)
            reports = _pytest.runner.runtestprotocol(
                target_item, nextitem=None, log=False
            )
        finally:
            # Rollback all Django transactions (cleanup regardless of test result)
            if django_atomics:
                from django.db import transaction

                for alias, atomic in reversed(django_atomics):
                    try:
                        transaction.set_rollback(True, using=alias)
                        atomic.__exit__(None, None, None)
                    except Exception:
                        pass  # Best effort cleanup

        duration = time.perf_counter() - start

        # 6. Analyze reports
        failed_report = None
        skipped_report = None

        for report in reports:
            if report.failed:
                failed_report = report
            elif report.skipped:
                skipped_report = report

        if failed_report:
            # Extract traceback
            longrepr = failed_report.longrepr
            if longrepr:
                msg = str(longrepr)
            else:
                msg = "Test failed (no traceback)"
            return (STATUS_FAIL, duration, msg)

        if skipped_report:
            skip_reason = ""
            if skipped_report.longrepr:
                skip_reason = str(skipped_report.longrepr)
            return (STATUS_SKIP, duration, f"Skipped: {skip_reason}")

        return (STATUS_PASS, duration, "")

    except SystemExit as e:
        # pytest may call sys.exit() on certain errors
        duration = time.perf_counter() - start
        return (STATUS_HARNESS_ERROR, duration, f"SystemExit: {e.code}")

    except Exception as e:
        duration = time.perf_counter() - start
        tb = traceback.format_exc()
        return (STATUS_HARNESS_ERROR, duration, f"Harness Error: {e}\n{tb}")

    finally:
        # CRITICAL: Flush buffers to memfd
        sys.stdout.flush()
        sys.stderr.flush()
