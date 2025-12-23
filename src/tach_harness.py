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
# ZERO-COPY LOADER: sys.meta_path Import Hook (Phase 2)
# =============================================================================

import importlib.abc
import importlib.machinery
import importlib.util

# Flag to track if the import hook is installed
_TACH_IMPORT_HOOK_INSTALLED = False


class TachLoader(importlib.abc.Loader):
    """Custom loader that uses Rust FFI to load bytecode directly.

    This loader bypasses importlib's file reading and uses pre-compiled,
    header-stripped bytecode from the Rust ModuleRegistry.
    """

    def __init__(self, name: str, bytecode: bytes, source_path: str, is_package: bool):
        self.name = name
        self.bytecode = bytecode
        self.source_path = source_path
        self.is_package = is_package

    def create_module(self, spec):
        """Let the default machinery create the module object."""
        return None  # Use default semantics

    def exec_module(self, module):
        """Execute the module using Rust FFI.

        Calls tach_rust.load_module which uses PyMarshal_ReadObjectFromString
        and PyImport_ExecCodeModuleObject to inject the bytecode directly.
        """
        try:
            import tach_rust

            success = tach_rust.load_module(self.name, self.source_path, self.bytecode)
            if not success:
                raise ImportError(f"tach_rust.load_module failed for {self.name}")
        except Exception as e:
            # Log error and re-raise - let Python handle it
            print(f"[tach] ERROR: Failed to load {self.name}: {e}", file=sys.stderr)
            raise


class TachMetaPathFinder(importlib.abc.MetaPathFinder):
    """Meta path finder that intercepts imports and routes to Rust loader.

    Installed at sys.meta_path[0] to have first priority.
    If the module is in the Rust registry, we return a TachLoader.
    Otherwise, we return None to let standard importlib handle it.
    """

    def find_spec(self, fullname, path, target=None):
        """Find module spec for the given module name.

        Args:
            fullname: Fully qualified module name (e.g., "foo.bar")
            path: Parent package's __path__ (for submodules)
            target: Optional target module (used for reloading)

        Returns:
            ModuleSpec if module is in Rust registry, None otherwise.
        """
        try:
            import tach_rust
        except ImportError:
            return None  # tach_rust not available, fall back to standard import

        # Check if module is in registry
        bytecode = tach_rust.get_module(fullname)
        if bytecode is None:
            # Not in registry - check if it's a namespace package (directory without __init__.py)
            # For now, let standard importlib handle it
            return None

        # Get source path for __file__ attribute
        source_path = tach_rust.get_module_path(fullname) or ""

        # Check if it's a package
        is_package = tach_rust.is_module_package(fullname) or False

        # Determine submodule search locations for packages
        submodule_search_locations = None
        if is_package and source_path:
            import os

            parent_dir = os.path.dirname(source_path)
            submodule_search_locations = [parent_dir]

        # Create loader
        loader = TachLoader(fullname, bytecode, source_path, is_package)

        # Create and return ModuleSpec
        spec = importlib.machinery.ModuleSpec(
            name=fullname,
            loader=loader,
            origin=source_path,
            is_package=is_package,
        )
        if submodule_search_locations:
            spec.submodule_search_locations = submodule_search_locations

        return spec


def install_tach_import_hook():
    """Install the Tach import hook at sys.meta_path[0].

    This gives Tach first priority for module resolution.
    Standard importlib remains as fallback for modules not in registry.
    """
    global _TACH_IMPORT_HOOK_INSTALLED

    if _TACH_IMPORT_HOOK_INSTALLED:
        return  # Already installed

    # Check if tach_rust module is available
    try:
        import tach_rust

        # Verify the loader functions exist
        if not hasattr(tach_rust, "get_module"):
            print(
                "[tach] WARN: get_module not available, skipping import hook",
                file=sys.stderr,
            )
            return
    except ImportError:
        print(
            "[tach] WARN: tach_rust not available, skipping import hook",
            file=sys.stderr,
        )
        return

    # Install at position 0 for highest priority
    finder = TachMetaPathFinder()
    sys.meta_path.insert(0, finder)
    _TACH_IMPORT_HOOK_INSTALLED = True
    print("[tach] Import hook installed at sys.meta_path[0]", file=sys.stderr)


def uninstall_tach_import_hook():
    """Remove the Tach import hook from sys.meta_path."""
    global _TACH_IMPORT_HOOK_INSTALLED

    sys.meta_path[:] = [
        f for f in sys.meta_path if not isinstance(f, TachMetaPathFinder)
    ]
    _TACH_IMPORT_HOOK_INSTALLED = False


# =============================================================================
# POST-FORK INITIALIZATION: Snapshot Mode Handshake
# =============================================================================

# Global flag tracking whether this worker can be recycled via userfaultfd
_CAN_RECYCLE = False


def post_fork_init() -> bool:
    """Initialize worker after fork - called ONCE at start of worker lifecycle.

    This function:
    1. Performs post-fork hygiene (RNG reseed, logging reset)
    2. Installs the Tach import hook for zero-copy module loading
    3. Initiates snapshot handshake with Supervisor if TACH_SUPERVISOR_SOCK is set
    4. Freezes (SIGSTOP) for Supervisor to capture golden snapshot

    Returns True if snapshot mode is enabled, False otherwise.
    """
    global _CAN_RECYCLE

    # 1. Post-fork hygiene
    inject_entropy()

    # 2. Install import hook for zero-copy module loading (Phase 2)
    # This must be done BEFORE snapshot to be part of the golden state
    install_tach_import_hook()

    # 3. Check if snapshot mode is enabled
    import os

    supervisor_sock = os.environ.get("TACH_SUPERVISOR_SOCK")
    if not supervisor_sock:
        # No snapshot mode - standard fork-server behavior
        return False

    # 4. Initialize snapshot mode via Rust FFI
    try:
        import tach_rust

        _CAN_RECYCLE = tach_rust.init_snapshot_mode(supervisor_sock)
        return _CAN_RECYCLE
    except ImportError:
        print("[harness] WARN: tach_rust module not available", file=sys.stderr)
        return False
    except Exception as e:
        print(f"[harness] WARN: Snapshot init failed: {e}", file=sys.stderr)
        return False


def can_recycle() -> bool:
    """Returns True if this worker can be recycled via userfaultfd reset."""
    return _CAN_RECYCLE


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
