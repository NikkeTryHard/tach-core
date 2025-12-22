# tach_harness.py - Embedded Python Harness for Tach
# This module is loaded directly into the worker process to execute tests.
# DO NOT MODIFY: This file is embedded via include_str! in zygote.rs

import sys
import time
import traceback
import asyncio
import inspect
import socket
import pdb
import _pytest.runner
import _pytest.main
import _pytest.config

# Status codes (must match protocol.rs)
STATUS_PASS = 0
STATUS_FAIL = 1
STATUS_SKIP = 2
STATUS_CRASH = 3
STATUS_HARNESS_ERROR = 4

# =============================================================================
# TTY Proxy: Interactive Debugging Support
# =============================================================================

_debug_socket_path = None


def set_debug_socket_path(path: str):
    """Called by worker initialization to set the debug socket path."""
    global _debug_socket_path
    _debug_socket_path = path


class TachPdb(pdb.Pdb):
    """PDB subclass that uses a Unix socket for I/O."""

    def __init__(self, sock_file):
        super().__init__(stdin=sock_file, stdout=sock_file)
        self.use_rawinput = False


def tach_breakpointhook(*args, **kwargs):
    """Custom breakpoint hook that tunnels to supervisor."""
    global _debug_socket_path

    if not _debug_socket_path:
        print(
            "[tach] WARNING: breakpoint() called but no debug socket.", file=sys.stderr
        )
        return

    try:
        sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        sock.connect(_debug_socket_path)
        sock_file = sock.makefile("rw", buffering=1, encoding="utf-8")
        debugger = TachPdb(sock_file)
        frame = sys._getframe(1)
        debugger.set_trace(frame)
        try:
            sock_file.close()
            sock.close()
        except Exception:
            pass
    except Exception as e:
        print(f"[tach] ERROR: Failed to start debug session: {e}", file=sys.stderr)


sys.breakpointhook = tach_breakpointhook


def inject_entropy():
    """Re-seed RNGs and reset fork-unsafe state to break the Clone Curse."""
    import random
    import logging
    import threading

    seed = time.time_ns() % (2**32)
    random.seed(seed)

    # CRITICAL: Reset logging module locks after fork
    # The logging module uses RLocks that become corrupted after fork()
    # because the lock state is shared but the threads are not
    try:
        import logging
        import threading

        # Recreate ALL module-level locks
        logging._lock = threading.RLock()

        # The Manager's lock is the main culprit
        if hasattr(logging.Logger, "manager") and logging.Logger.manager:
            logging.Logger.manager._lock = threading.RLock()

        # Recreate locks for root logger and all handlers
        logging.root.handlers = []  # Clear handlers to avoid lock issues

        # Reset the logger dict to force fresh loggers
        if hasattr(logging.Logger, "manager") and logging.Logger.manager:
            logging.Logger.manager.loggerDict = {}
    except Exception:
        pass  # Best effort

    if "numpy" in sys.modules:
        try:
            sys.modules["numpy"].random.seed(seed)
        except Exception:
            pass

    if "torch" in sys.modules:
        try:
            sys.modules["torch"].manual_seed(seed)
        except Exception:
            pass


# =============================================================================
# ZYGOTE COLLECTION PATTERN
# Pytest session is initialized ONCE in Zygote, workers inherit via fork CoW
# =============================================================================

_SESSION = None
_ITEMS_MAP = {}  # nodeid -> pytest Item


def init_session(root_dir: str):
    """Initialize pytest session in Zygote BEFORE forking workers.

    This pays the "Pytest Tax" (config parsing, plugin loading, test collection)
    exactly ONCE. Workers inherit the session via Copy-on-Write fork semantics.
    """
    global _SESSION, _ITEMS_MAP
    import os

    os.write(2, f"[harness] init_session: {root_dir}\n".encode())

    args = [
        root_dir,
        "-s",
        "-o",
        "addopts=",
        "-p",
        "no:terminal",
        "-p",
        "no:cacheprovider",
        "-p",
        "no:cov",
        "-p",
        "no:xdist",
        "-p",
        "no:sugar",
        "-p",
        "no:asyncio",
        "-p",
        "no:trio",
        "-p",
        "no:django",
    ]

    cfg = _pytest.config._prepareconfig(args)
    cfg._do_configure()

    _SESSION = _pytest.main.Session.from_config(cfg)
    cfg.hook.pytest_sessionstart(session=_SESSION)

    _SESSION.perform_collect()

    for item in _SESSION.items:
        _ITEMS_MAP[item.nodeid] = item

    os.write(2, f"[harness] Pre-collected {len(_ITEMS_MAP)} tests\n".encode())


def run_test(file_path: str, node_id: str) -> tuple:
    """
    Execute a single pytest test item using pre-collected session.

    FAST PATH: Item lookup is O(1) from _ITEMS_MAP.
    No pytest config, no collection, just run the test.
    """
    global _SESSION, _ITEMS_MAP

    # CRITICAL: Reset logging lock FIRST before anything else
    # fork() corrupts the logging module's RLock, causing segfaults
    import logging
    import threading

    logging._lock = threading.RLock()

    inject_entropy()
    start = time.perf_counter()

    try:
        # O(1) lookup from pre-collected items
        target_item = _ITEMS_MAP.get(node_id)

        if not target_item:
            duration = time.perf_counter() - start
            return (
                STATUS_HARNESS_ERROR,
                duration,
                f"Test not found in Zygote session: {node_id}\nAvailable: {len(_ITEMS_MAP)} items",
            )

        # Native Async Support
        original_obj = target_item.obj
        func_to_check = original_obj
        if hasattr(original_obj, "__func__"):
            func_to_check = original_obj.__func__

        if inspect.iscoroutinefunction(func_to_check):

            def make_sync_wrapper(async_fn):
                def sync_wrapper(*args, **kwargs):
                    loop = asyncio.new_event_loop()
                    asyncio.set_event_loop(loop)
                    try:
                        return loop.run_until_complete(async_fn(*args, **kwargs))
                    finally:
                        loop.close()
                        asyncio.set_event_loop(None)

                return sync_wrapper

            target_item.obj = make_sync_wrapper(original_obj)

        # Django Transaction Isolation
        django_atomics = []
        if "django" in sys.modules:
            try:
                from django.conf import settings

                if settings.configured:
                    from django.db import connections, transaction

                    try:
                        connections.close_all()
                    except Exception:
                        pass
                    for alias in connections:
                        try:
                            atomic = transaction.atomic(using=alias)
                            atomic.__enter__()
                            django_atomics.append((alias, atomic))
                        except Exception:
                            pass
            except ImportError:
                pass

        try:
            reports = _pytest.runner.runtestprotocol(
                target_item, nextitem=None, log=False
            )
        finally:
            if django_atomics:
                from django.db import transaction

                for alias, atomic in reversed(django_atomics):
                    try:
                        transaction.set_rollback(True, using=alias)
                        atomic.__exit__(None, None, None)
                    except Exception:
                        pass

        duration = time.perf_counter() - start

        failed_report = None
        skipped_report = None

        for report in reports:
            if report.failed:
                failed_report = report
            elif report.skipped:
                skipped_report = report

        if failed_report:
            longrepr = failed_report.longrepr
            msg = str(longrepr) if longrepr else "Test failed (no traceback)"
            return (STATUS_FAIL, duration, msg)

        if skipped_report:
            skip_reason = (
                str(skipped_report.longrepr) if skipped_report.longrepr else ""
            )
            return (STATUS_SKIP, duration, f"Skipped: {skip_reason}")

        return (STATUS_PASS, duration, "")

    except SystemExit as e:
        duration = time.perf_counter() - start
        return (STATUS_HARNESS_ERROR, duration, f"SystemExit: {e.code}")

    except Exception as e:
        duration = time.perf_counter() - start
        tb = traceback.format_exc()
        return (STATUS_HARNESS_ERROR, duration, f"Harness Error: {e}\n{tb}")

    finally:
        sys.stdout.flush()
        sys.stderr.flush()
