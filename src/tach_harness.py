# tach_harness.py - Embedded Python Harness for Tach
# This module is loaded directly into the worker process to execute tests.
# DO NOT MODIFY: This file is embedded via include_str! in zygote.rs
#
# Dual-Path Execution Support
# - Safe tests: Workers can reset memory and loop (Hypervisor Mode)
# - Toxic tests: Workers exit after execution (Isolation Mode)

import sys
import time
import traceback
import asyncio
import inspect
import socket
import os
import pdb
import re
import math
import logging
import warnings as warnings_module


_TACH_QUIET = os.environ.get("TACH_QUIET") == "1"


def _tach_log(msg: bytes) -> None:
    if not _TACH_QUIET:
        os.write(2, msg)


try:
    import pytest
    import _pytest.runner
    import _pytest.main
    import _pytest.config
except ImportError as _e:
    sys.stderr.write(
        f"[tach:harness] FATAL: pytest is not installed ({_e})\n"
        "  Install it: pip install pytest\n"
        "  Or: uv add --dev pytest\n"
    )
    sys.exit(4)
from contextlib import contextmanager
from typing import Any, Optional, Set, Type, Tuple, Union

# Module logger for cleanup and error reporting
_logger = logging.getLogger("tach.harness")

# Status codes (must match protocol.rs)
STATUS_PASS = 0
STATUS_FAIL = 1
STATUS_SKIP = 2
STATUS_CRASH = 3
STATUS_HARNESS_ERROR = 4

# Effect type constants (must match HookEffect variants in hooks.rs)
EFFECT_TYPE_SET_ENV = "SetEnv"
EFFECT_TYPE_DELETE_ENV = "DeleteEnv"
EFFECT_TYPE_ADD_SYS_PATH = "AddSysPath"
EFFECT_TYPE_REMOVE_SYS_PATH = "RemoveSysPath"
EFFECT_TYPE_REGISTER_MARKER = "RegisterMarker"
EFFECT_TYPE_DJANGO_DB_SETUP = "DjangoDbSetup"
EFFECT_TYPE_MODIFY_SYS_PATH = "ModifySysPath"
EFFECT_TYPE_ASYNCIO_SETUP = "AsyncioSetup"


# =============================================================================
# EVENT LOOP MANAGEMENT (pytest-asyncio support)
# =============================================================================


class EventLoopManager:
    """Manages event loop lifecycle based on pytest-asyncio loop_scope.

    Supports function, class, module, and session scopes.
    """

    _instance: Optional["EventLoopManager"] = None

    def __init__(self):
        self._loops: dict[str, asyncio.AbstractEventLoop] = {}
        self._current_scope: str = "function"
        self._auto_mode: bool = False
        self._policy: Optional[asyncio.AbstractEventLoopPolicy] = None
        # Scope transition tracking (Issue #43)
        self._previous_module: Optional[str] = None
        self._previous_class: Optional[str] = None

    @classmethod
    def get_instance(cls) -> "EventLoopManager":
        if cls._instance is None:
            cls._instance = cls()
        return cls._instance

    @classmethod
    def reset(cls) -> None:
        """Reset the singleton (for testing)."""
        if cls._instance is not None:
            cls._instance.close_all()
            cls._instance = None

    @classmethod
    def get_current_loop(cls) -> Optional[asyncio.AbstractEventLoop]:
        """Get the most recently created valid loop, or None."""
        instance = cls.get_instance()
        for key in reversed(list(instance._loops.keys())):
            loop = instance._loops[key]
            if loop and not loop.is_closed():
                return loop
        return None

    def configure(self, loop_scope: str = "function", auto_mode: bool = False) -> None:
        """Configure loop scope and auto mode."""
        self._current_scope = loop_scope
        self._auto_mode = auto_mode

    def set_policy(self, policy: asyncio.AbstractEventLoopPolicy) -> None:
        """Set custom event loop policy for creating new loops."""
        self._policy = policy

    def get_loop(self, scope_key: str) -> asyncio.AbstractEventLoop:
        """Get or create event loop for the given scope key."""
        if scope_key not in self._loops:
            if self._policy is not None:
                loop = self._policy.new_event_loop()
            else:
                loop = asyncio.new_event_loop()
            asyncio.set_event_loop(loop)
            self._loops[scope_key] = loop
        return self._loops[scope_key]

    def get_scope_key(self, item: Any) -> str:
        """Determine scope key based on current scope and test item."""
        if self._current_scope == "session":
            return "session"
        elif self._current_scope == "module":
            fspath = getattr(item, "fspath", None)
            return f"module:{fspath}" if fspath else f"module:unknown:{id(item)}"
        elif self._current_scope == "class":
            cls = getattr(item, "cls", None)
            if cls:
                return f"class:{cls.__module__}.{cls.__name__}"
            nodeid = getattr(item, "nodeid", None)
            return f"function:{nodeid}" if nodeid else f"function:unknown:{id(item)}"
        else:  # function scope (default)
            nodeid = getattr(item, "nodeid", None)
            return f"function:{nodeid}" if nodeid else f"function:unknown:{id(item)}"

    def close_scope(self, scope_key: str) -> None:
        """Close and remove the event loop for a scope."""
        if scope_key in self._loops:
            loop = self._loops.pop(scope_key)
            if not loop.is_closed():
                try:
                    # Cancel pending tasks
                    pending = asyncio.all_tasks(loop)
                    for task in pending:
                        task.cancel()
                    # Run until all tasks are cancelled
                    if pending:
                        loop.run_until_complete(
                            asyncio.gather(*pending, return_exceptions=True)
                        )
                except Exception as e:
                    _logger.debug("Cleanup error in close_scope: %s", e)
                finally:
                    loop.close()

    def close_all(self) -> None:
        """Close all managed event loops."""
        for scope_key in list(self._loops.keys()):
            self.close_scope(scope_key)

    def on_scope_transition(
        self, current_module: Optional[str], current_class: Optional[str]
    ) -> None:
        """Handle scope transitions and cleanup old scopes.

        Called before each test to detect and handle scope boundaries.
        Closes event loops when transitioning out of a class or module.
        """
        # Module transition: close previous module's loop if scope is module
        if self._current_scope == "module":
            if (
                self._previous_module is not None
                and self._previous_module != current_module
            ):
                old_key = f"module:{self._previous_module}"
                self.close_scope(old_key)

        # Class transition: close previous class's loop if scope is class
        if self._current_scope == "class":
            if (
                self._previous_class is not None
                and self._previous_class != current_class
            ):
                old_key = f"class:{self._previous_class}"
                self.close_scope(old_key)

        # Update tracking
        self._previous_module = current_module
        self._previous_class = current_class

    @property
    def auto_mode(self) -> bool:
        return self._auto_mode

    @property
    def current_scope(self) -> str:
        return self._current_scope

    def should_run_async(self, is_coro: bool, has_marker: bool) -> bool:
        """Determine if a test should be run as async.

        Args:
            is_coro: Whether the test function is a coroutine
            has_marker: Whether the test has @pytest.mark.asyncio

        Returns:
            True if test should be executed with event loop
        """
        if not is_coro:
            return False
        # With auto_mode, all coroutines run as async
        if self._auto_mode:
            return True
        # Without auto_mode, need explicit marker
        return has_marker


def detect_uvloop() -> bool:
    """Detect if uvloop is available."""
    try:
        import uvloop  # noqa: F401

        return True
    except ImportError:
        return False


def get_uvloop_policy() -> Optional[asyncio.AbstractEventLoopPolicy]:
    """Get uvloop policy if available, None otherwise."""
    try:
        import uvloop

        return uvloop.EventLoopPolicy()
    except ImportError:
        return None


class AsyncFixtureWrapper:
    """Wrapper that tracks async fixtures for proper teardown by scope.

    When pytest resolves an async fixture, we intercept it, consume the
    coroutine/generator, store the value, and return that to the test.
    The generator is stored for teardown based on its scope.
    """

    _generators_by_scope: dict[str, dict[str, Any]] = {
        "session": {},
        "module": {},
        "class": {},
        "function": {},
    }
    _consumed_by_scope: dict[str, set[str]] = {
        "session": set(),
        "module": set(),
        "class": set(),
        "function": set(),
    }
    _values_by_scope: dict[str, dict[str, Any]] = {
        "session": {},
        "module": {},
        "class": {},
        "function": {},
    }
    _loop: Optional[asyncio.AbstractEventLoop] = None
    _teardown_errors: list[Exception] = []
    _current_module: Optional[str] = None
    _current_class: Optional[str] = None

    @classmethod
    def set_loop(cls, loop: asyncio.AbstractEventLoop) -> None:
        cls._loop = loop

    @classmethod
    def get_loop(cls) -> asyncio.AbstractEventLoop:
        if cls._loop is not None and not cls._loop.is_closed():
            return cls._loop
        try:
            loop = EventLoopManager.get_current_loop()
            if loop:
                cls._loop = loop
                return cls._loop
        except Exception:
            pass
        cls._loop = asyncio.new_event_loop()
        asyncio.set_event_loop(cls._loop)
        return cls._loop

    @classmethod
    def on_test_start(
        cls, module_path: Optional[str], class_name: Optional[str]
    ) -> None:
        if cls._current_module is not None and cls._current_module != module_path:
            cls.teardown_module_scope()
        if cls._current_class is not None and cls._current_class != class_name:
            cls.teardown_class_scope()
        cls._current_module = module_path
        cls._current_class = class_name

    @classmethod
    def consume_async_fixture(
        cls, fixture_name: str, fixture_value: Any, scope: str = "function"
    ) -> Any:
        if scope not in cls._consumed_by_scope:
            scope = "function"
        if fixture_name in cls._consumed_by_scope[scope]:
            return cls._values_by_scope[scope].get(fixture_name, fixture_value)

        loop = cls.get_loop()
        try:
            if inspect.isasyncgen(fixture_value):

                async def consume_gen():
                    return await fixture_value.__anext__()

                result = loop.run_until_complete(consume_gen())
                cls._generators_by_scope[scope][fixture_name] = fixture_value
                cls._consumed_by_scope[scope].add(fixture_name)
                cls._values_by_scope[scope][fixture_name] = result
                return result

            if asyncio.iscoroutine(fixture_value):
                result = loop.run_until_complete(fixture_value)
                cls._consumed_by_scope[scope].add(fixture_name)
                cls._values_by_scope[scope][fixture_name] = result
                return result

            return fixture_value
        except Exception as e:
            _logger.error("Async fixture '%s' failed: %s", fixture_name, e)
            raise

    @classmethod
    def _teardown_scope(cls, scope: str) -> None:
        generators = cls._generators_by_scope.get(scope, {})
        if generators:
            loop = cls.get_loop()
            for name, gen in list(generators.items()):
                try:

                    async def cleanup():
                        await gen.aclose()

                    loop.run_until_complete(cleanup())
                except Exception as e:
                    cls._teardown_errors.append(
                        RuntimeError(f"Fixture '{name}' ({scope}) teardown: {e}")
                    )
        cls._generators_by_scope[scope].clear()
        cls._consumed_by_scope[scope].clear()
        cls._values_by_scope[scope].clear()

    @classmethod
    def teardown_function_scope(cls) -> None:
        cls._teardown_scope("function")

    @classmethod
    def teardown_class_scope(cls) -> None:
        cls._teardown_scope("class")

    @classmethod
    def teardown_module_scope(cls) -> None:
        cls._teardown_scope("class")
        cls._teardown_scope("module")
        cls._current_class = None

    @classmethod
    def teardown_session_scope(cls) -> None:
        cls._teardown_scope("session")

    @classmethod
    def teardown_all(cls) -> None:
        cls.teardown_function_scope()
        cls.teardown_class_scope()
        cls.teardown_module_scope()
        cls.teardown_session_scope()

    @classmethod
    def reset(cls) -> None:
        cls.teardown_all()
        cls._current_module = None
        cls._current_class = None
        if cls._loop is not None:
            if not cls._loop.is_running() and not cls._loop.is_closed():
                try:
                    cls._loop.close()
                except Exception:
                    pass
        cls._loop = None

    @classmethod
    def get_teardown_errors(cls) -> list[Exception]:
        errors = cls._teardown_errors.copy()
        cls._teardown_errors.clear()
        return errors


class TachFixturePlugin:
    """Pytest plugin that intercepts async fixtures at resolution time."""

    @staticmethod
    @pytest.hookimpl(hookwrapper=True, tryfirst=True)
    def pytest_fixture_setup(fixturedef, request):
        outcome = yield
        try:
            result = outcome.get_result()
        except Exception:
            return
        is_async = inspect.isasyncgen(result) or asyncio.iscoroutine(result)
        if is_async:
            scope = getattr(fixturedef, "scope", "function")
            try:
                consumed_value = AsyncFixtureWrapper.consume_async_fixture(
                    fixturedef.argname, result, scope
                )
                outcome.force_result(consumed_value)
            except Exception as e:
                _logger.error(
                    "Failed to consume async fixture '%s': %s", fixturedef.argname, e
                )
                raise


_TACH_FIXTURE_PLUGIN: Optional[TachFixturePlugin] = None


def _configure_asyncio_from_pyproject(root_dir: str) -> None:
    """Parse asyncio_mode from pyproject.toml and configure EventLoopManager."""
    try:
        import tomllib
    except ImportError:
        try:
            import tomli as tomllib  # type: ignore
        except ImportError:
            return

    from pathlib import Path

    search_path = Path(root_dir).resolve()
    pyproject_path = None
    for _ in range(5):
        candidate = search_path / "pyproject.toml"
        if candidate.exists():
            pyproject_path = candidate
            break
        if search_path.parent == search_path:
            break
        search_path = search_path.parent

    if pyproject_path is None:
        return

    try:
        with open(pyproject_path, "rb") as f:
            data = tomllib.load(f)
        pytest_opts = data.get("tool", {}).get("pytest", {}).get("ini_options", {})
        asyncio_mode = pytest_opts.get("asyncio_mode", "strict")
        loop_scope = pytest_opts.get("asyncio_default_fixture_loop_scope", "function")
        auto_mode = asyncio_mode == "auto"
        if auto_mode or loop_scope != "function":
            EventLoopManager.get_instance().configure(
                loop_scope=loop_scope, auto_mode=auto_mode
            )
            _tach_log(
                f"[tach:harness] Asyncio config: mode={asyncio_mode}, loop_scope={loop_scope}\n".encode(),
            )
    except Exception as e:
        _logger.warning("Failed to parse asyncio config: %s", e)


# Sentinel value to distinguish timeout from legitimate None return
_TIMEOUT_SENTINEL = object()


def run_with_timeout(
    loop: asyncio.AbstractEventLoop,
    coro,
    timeout: Optional[float] = None,
) -> tuple[Any, bool]:
    """Run a coroutine with optional timeout and proper cancellation.

    Args:
        loop: Event loop to run on
        coro: Coroutine to execute
        timeout: Timeout in seconds, None for no timeout

    Returns:
        Tuple of (result_or_none, timed_out)
    """
    if timeout is None:
        result = loop.run_until_complete(coro)
        return result, False

    async def with_timeout():
        try:
            return await asyncio.wait_for(coro, timeout=timeout)
        except asyncio.TimeoutError:
            return _TIMEOUT_SENTINEL

    try:
        result = loop.run_until_complete(with_timeout())
        if result is _TIMEOUT_SENTINEL:
            return None, True
        return result, False
    except asyncio.CancelledError:
        return None, True


def is_loop_running() -> bool:
    """Check if an event loop is currently running."""
    try:
        loop = asyncio.get_running_loop()
        return loop.is_running()
    except RuntimeError:
        return False


@contextmanager
def ensure_no_running_loop():
    """Context manager that ensures no event loop is set.

    Used to allow asyncio.run() inside sync tests by temporarily
    clearing the current event loop.
    """
    # Save current event loop if any
    try:
        old_loop = asyncio.get_event_loop_policy().get_event_loop()
        had_loop = True
    except RuntimeError:
        old_loop = None
        had_loop = False

    # Clear the current event loop
    asyncio.set_event_loop(None)

    try:
        yield
    finally:
        # Restore previous state
        if had_loop and old_loop is not None:
            try:
                asyncio.set_event_loop(old_loop)
            except RuntimeError as e:
                _logger.debug("Loop restoration skipped (may be closed): %s", e)


def cleanup_pending_tasks(loop: asyncio.AbstractEventLoop) -> int:
    """Cancel and await all pending tasks in the loop.

    This ensures proper cleanup of tasks created with asyncio.gather(),
    asyncio.create_task(), or TaskGroup (Python 3.11+).

    Args:
        loop: Event loop to clean up

    Returns:
        Number of tasks that were cancelled
    """
    try:
        pending = asyncio.all_tasks(loop)
    except RuntimeError:
        return 0

    if not pending:
        return 0

    for task in pending:
        task.cancel()

    # Give tasks a chance to handle cancellation
    async def wait_cancelled():
        await asyncio.gather(*pending, return_exceptions=True)

    try:
        loop.run_until_complete(wait_cancelled())
    except Exception as e:
        _logger.debug("Exception during cleanup: %s", e)

    return len(pending)


def run_async_fixture(
    fixture_func: Any,
    fixture_values: dict[str, Any],
    loop: asyncio.AbstractEventLoop,
) -> tuple[Any, Any]:
    """Execute an async fixture and return its value.

    NOTE: This is scaffolding for future async fixture execution.
    Currently, fixtures are resolved by pytest before reaching the harness.
    This helper will be integrated when Tach implements native fixture resolution.

    Returns:
        Tuple of (value, generator_or_none)
    """
    if inspect.isasyncgenfunction(fixture_func):
        # Async generator fixture (async yield pattern)
        async def run_gen():
            gen = fixture_func(**fixture_values)
            value = await gen.__anext__()
            return value, gen

        return loop.run_until_complete(run_gen())
    else:
        # Simple async fixture
        coro = fixture_func(**fixture_values)
        return loop.run_until_complete(coro), None


def teardown_async_fixture(
    gen: Any,
    loop: asyncio.AbstractEventLoop,
) -> None:
    """Teardown an async generator fixture.

    NOTE: This is scaffolding for future async fixture execution.
    Currently, fixtures are resolved by pytest before reaching the harness.
    This helper will be integrated when Tach implements native fixture resolution.
    """
    if gen is not None:

        async def cleanup():
            try:
                await gen.__anext__()
            except StopAsyncIteration:
                pass

        try:
            loop.run_until_complete(cleanup())
        except Exception as e:
            _logger.debug("Cleanup error in async fixture teardown: %s", e)


def parse_asyncio_marker(item: Any) -> tuple[str, bool]:
    """Parse @pytest.mark.asyncio marker from test item.

    Returns:
        Tuple of (loop_scope, has_marker)
    """
    loop_scope = "function"  # default
    has_marker = False

    if hasattr(item, "iter_markers"):
        for marker in item.iter_markers("asyncio"):
            has_marker = True
            # Extract loop_scope from marker kwargs
            if marker.kwargs:
                loop_scope = marker.kwargs.get("loop_scope", "function")
            break

    return loop_scope, has_marker


# =============================================================================
# THREAD LEAK DETECTION (Task 3: 0.1.2)
# =============================================================================

# Grace period to wait for threads to terminate (milliseconds)
_THREAD_GRACE_PERIOD_MS = 500

# Global flag set when thread leak is detected in current test
_thread_leak_detected = False


def _has_allow_threads_marker(item) -> bool:
    """Check if test item has @pytest.mark.allow_threads marker.

    Args:
        item: A pytest test item

    Returns:
        True if the test has the allow_threads marker
    """
    try:
        # Check for the marker on the item
        markers = getattr(item, "iter_markers", None)
        if markers:
            for marker in markers():
                if marker.name == "allow_threads":
                    return True
        # Also check own_markers attribute
        own_markers = getattr(item, "own_markers", [])
        for marker in own_markers:
            if marker.name == "allow_threads":
                return True
    except Exception as e:
        _logger.debug("Exception checking allow_threads marker: %s", e)
    return False


def _detect_thread_leak(initial_count: int, allow_threads: bool) -> bool:
    """Detect if test spawned threads that outlive the test.

    This function:
    1. Compares current thread count to initial count
    2. If threads increased and allow_threads is False:
       - Waits up to _THREAD_GRACE_PERIOD_MS for threads to terminate
       - If still running, returns True (leak detected)
    3. Logs a warning if threads leaked

    Args:
        initial_count: Thread count before test execution
        allow_threads: Whether @pytest.mark.allow_threads is set

    Returns:
        True if thread leak detected (worker should be marked toxic)
    """
    import threading
    import time

    current_count = threading.active_count()

    if current_count <= initial_count:
        return False  # No new threads

    if allow_threads:
        # User explicitly allowed thread leaks for this test
        print(
            f"[tach:harness] INFO: Test spawned {current_count - initial_count} additional threads (allowed by @pytest.mark.allow_threads)",
            file=sys.stderr,
        )
        return False

    # Threads increased - wait for grace period
    leaked_threads = current_count - initial_count
    print(
        f"[tach:harness] WARN: Test spawned {leaked_threads} additional thread(s), waiting {_THREAD_GRACE_PERIOD_MS}ms for them to terminate...",
        file=sys.stderr,
    )

    # Wait in small increments, checking thread count
    grace_end = time.perf_counter() + (_THREAD_GRACE_PERIOD_MS / 1000.0)
    while time.perf_counter() < grace_end:
        time.sleep(0.050)  # 50ms intervals
        current_count = threading.active_count()
        if current_count <= initial_count:
            print(
                "[tach:harness] INFO: Threads terminated within grace period",
                file=sys.stderr,
            )
            return False

    # Grace period expired, threads still running
    leaked_threads = threading.active_count() - initial_count
    print(
        f"[tach:harness] WARN: {leaked_threads} thread(s) still running after grace period. Worker marked toxic (cannot be reused).",
        file=sys.stderr,
    )
    return True


# =============================================================================
# PYTEST COMPATIBILITY: Exception and Warning Context Managers
# =============================================================================


class ExceptionInfo:
    """Wrapper for exception information (similar to pytest.ExceptionInfo).

    This class provides a compatible interface with pytest's ExceptionInfo,
    allowing tests to access exception details after using the raises context manager.

    Attributes:
        type: The exception class that was raised.
        value: The exception instance that was raised.
        tb: The traceback object.
        traceback: Alias for tb (for compatibility).
    """

    def __init__(
        self,
        type_: Type[BaseException],
        value: BaseException,
        tb,
    ):
        self.type = type_
        self.value = value
        self.tb = tb
        self.traceback = tb

    def match(self, pattern: str) -> bool:
        """Check if exception message matches a regex pattern.

        Args:
            pattern: Regular expression pattern to match against the exception message.

        Returns:
            True if the pattern matches, False otherwise.
        """
        return bool(re.search(pattern, str(self.value)))

    def __repr__(self) -> str:
        return f"<ExceptionInfo {self.type.__name__}('{self.value}')>"


class raises:
    """Context manager for expected exceptions (compatible with pytest.raises).

    This context manager verifies that a specific exception is raised within
    the context block. If the expected exception is not raised, or if a
    different exception is raised, the test fails.

    Example:
        with raises(ValueError, match="invalid"):
            int("not_a_number")

    Attributes:
        expected_exception: The exception type(s) expected to be raised.
        match: Optional regex pattern that must match the exception message.
        excinfo: ExceptionInfo object populated after successful exception capture.
    """

    def __init__(
        self,
        expected_exception: Union[Type[BaseException], Tuple[Type[BaseException], ...]],
        *,
        match: Optional[str] = None,
    ):
        """Initialize the raises context manager.

        Args:
            expected_exception: Exception type or tuple of types to expect.
            match: Optional regex pattern to match against the exception message.
        """
        self.expected_exception = expected_exception
        self.match = match
        self.excinfo: Optional[ExceptionInfo] = None

    def __enter__(self) -> "raises":
        """Enter the context manager.

        Returns:
            Self, allowing access to excinfo after the context exits.
        """
        return self

    def __exit__(self, exc_type, exc_val, exc_tb) -> bool:
        """Exit the context manager and validate the exception.

        Args:
            exc_type: The exception type if one was raised, None otherwise.
            exc_val: The exception instance if one was raised, None otherwise.
            exc_tb: The traceback if an exception was raised, None otherwise.

        Returns:
            True if the exception was expected and matched (suppresses it).
            False if a different exception was raised (re-raises it).

        Raises:
            AssertionError: If no exception was raised, or if the match pattern
                doesn't match the exception message.
        """
        if exc_type is None:
            # Format expected exception for error message
            if isinstance(self.expected_exception, tuple):
                expected_names = ", ".join(e.__name__ for e in self.expected_exception)
                raise AssertionError(f"DID NOT RAISE <{expected_names}>")
            raise AssertionError(f"DID NOT RAISE {self.expected_exception}")

        # Check if the raised exception is an instance of expected type(s)
        if not issubclass(exc_type, self.expected_exception):
            # Re-raise the unexpected exception
            return False

        # If match pattern is provided, validate it
        if self.match and not re.search(self.match, str(exc_val)):
            raise AssertionError(f"Pattern '{self.match}' not found in '{exc_val}'")

        # Capture exception info for later inspection
        self.excinfo = ExceptionInfo(exc_type, exc_val, exc_tb)

        # Suppress the expected exception
        return True


class warns:
    """Context manager for expected warnings (compatible with pytest.warns).

    This context manager verifies that a specific warning is raised within
    the context block. If the expected warning is not raised, the test fails.

    Example:
        with warns(DeprecationWarning, match="deprecated"):
            deprecated_function()

    Attributes:
        expected_warning: The warning type expected to be raised.
        match: Optional regex pattern that must match the warning message.
    """

    def __init__(
        self,
        expected_warning: Type[Warning],
        *,
        match: Optional[str] = None,
    ):
        """Initialize the warns context manager.

        Args:
            expected_warning: Warning type to expect.
            match: Optional regex pattern to match against the warning message.
        """
        self.expected_warning = expected_warning
        self.match = match
        self._catch = None
        self._warnings = None

    def __enter__(self) -> "warns":
        """Enter the context manager and start capturing warnings.

        Returns:
            Self, allowing access to captured warnings after the context exits.
        """
        self._catch = warnings_module.catch_warnings(record=True)
        self._warnings = self._catch.__enter__()
        warnings_module.simplefilter("always")
        return self

    def __exit__(self, exc_type, exc_val, exc_tb) -> bool:
        """Exit the context manager and validate the warnings.

        Args:
            exc_type: The exception type if one was raised, None otherwise.
            exc_val: The exception instance if one was raised, None otherwise.
            exc_tb: The traceback if an exception was raised, None otherwise.

        Returns:
            False (never suppresses exceptions).

        Raises:
            AssertionError: If the expected warning was not raised, or if the
                match pattern doesn't match any warning message.
        """
        self._catch.__exit__(exc_type, exc_val, exc_tb)

        # Filter warnings to find matching ones
        matching = [
            w for w in self._warnings if issubclass(w.category, self.expected_warning)
        ]

        if not matching:
            raise AssertionError(f"DID NOT WARN {self.expected_warning}")

        # If match pattern is provided, validate it
        if self.match:
            if not any(re.search(self.match, str(w.message)) for w in matching):
                raise AssertionError(f"Pattern '{self.match}' not found in warnings")

        return False  # Never suppress exceptions


# =============================================================================
# PYTEST COMPATIBILITY: Exception Classes for Skip/XFail
# =============================================================================


class SkipException(Exception):
    """Exception raised to skip a test.

    This exception is caught by the test harness and converted to a skip result.
    Compatible with pytest.skip() behavior.

    Attributes:
        reason: The reason for skipping the test.
    """

    def __init__(self, reason: str = ""):
        self.reason = reason
        super().__init__(reason)


class XFailException(Exception):
    """Exception raised to mark a test as expected to fail.

    This exception is caught by the test harness and treated as an expected failure.
    Compatible with pytest.xfail() behavior.

    Attributes:
        reason: The reason for expecting the test to fail.
    """

    def __init__(self, reason: str = ""):
        self.reason = reason
        super().__init__(reason)


# =============================================================================
# PYTEST COMPATIBILITY: Approximate Floating Point Comparison
# =============================================================================


class approx:
    """Approximate floating point comparison (compatible with pytest.approx).

    This class enables comparisons between floating point numbers with tolerance
    for small differences due to floating point representation errors.

    The comparison uses both relative and absolute tolerances:
    - A value is considered equal if it's within the greater of:
      - rel * abs(expected) (relative tolerance)
      - abs (absolute tolerance)

    Example:
        assert 0.1 + 0.2 == approx(0.3)
        assert 100 == approx(105, rel=0.1)  # 10% tolerance
        assert 1.0 == approx(1.005, abs=0.01)

    Attributes:
        expected: The expected value (float, list, or tuple).
        rel: Relative tolerance (default 1e-6).
        abs: Absolute tolerance (default 1e-12).
    """

    def __init__(
        self,
        expected,
        rel: Optional[float] = None,
        abs: Optional[float] = None,
    ):
        """Initialize the approx comparison.

        Args:
            expected: The expected value (float, list, or tuple).
            rel: Relative tolerance. Defaults to 1e-6.
            abs: Absolute tolerance. Defaults to 1e-12.
        """
        self.expected = expected
        self.rel = rel if rel is not None else 1e-6
        self.abs = abs if abs is not None else 1e-12

    def __eq__(self, actual) -> bool:
        """Compare actual value against expected with tolerance.

        Args:
            actual: The actual value to compare.

        Returns:
            True if values are approximately equal, False otherwise.
        """
        if isinstance(self.expected, (list, tuple)):
            if not isinstance(actual, (list, tuple)):
                return False
            if len(self.expected) != len(actual):
                return False
            return all(
                approx(e, self.rel, self.abs) == a
                for e, a in zip(self.expected, actual)
            )

        # Single value comparison
        # Handle special float values first
        try:
            # NaN is never equal to anything (including itself) - IEEE 754 semantics
            if math.isnan(self.expected) or math.isnan(actual):
                return False
            # Infinity requires exact comparison (inf == inf, -inf == -inf, inf != -inf)
            if math.isinf(self.expected) or math.isinf(actual):
                return self.expected == actual
        except TypeError:
            # Non-numeric types that don't support isnan/isinf - continue to normal comparison
            pass

        try:
            # Use built-in abs function (not self.abs)
            expected_abs = (
                __builtins__["abs"](self.expected)
                if isinstance(__builtins__, dict)
                else abs(self.expected)
            )
            diff = (
                __builtins__["abs"](self.expected - actual)
                if isinstance(__builtins__, dict)
                else abs(self.expected - actual)
            )
        except (TypeError, KeyError):
            # Fallback for edge cases
            import builtins

            expected_abs = builtins.abs(self.expected)
            diff = builtins.abs(self.expected - actual)

        tolerance = max(self.rel * expected_abs, self.abs)
        return diff <= tolerance

    def __repr__(self) -> str:
        """Return string representation of the approx object."""
        return f"approx({self.expected} +/- {self.rel * 100}%)"


# =============================================================================
# PYTEST COMPATIBILITY: fail, skip, xfail, importorskip
# =============================================================================


def fail(reason: str = "") -> None:
    """Explicitly fail the test with an optional reason.

    This function immediately fails the current test by raising an AssertionError.
    Compatible with pytest.fail() behavior.

    Args:
        reason: The reason for the failure. Defaults to "Test failed".

    Raises:
        AssertionError: Always raised to fail the test.

    Example:
        if not some_condition:
            fail("Condition was not met")
    """
    raise AssertionError(reason or "Test failed")


def skip(reason: str = "") -> None:
    """Skip the current test with an optional reason.

    This function immediately skips the current test by raising a SkipException.
    Compatible with pytest.skip() behavior.

    Args:
        reason: The reason for skipping the test.

    Raises:
        SkipException: Always raised to skip the test.

    Example:
        if sys.platform == "win32":
            skip("This test only runs on Linux")
    """
    raise SkipException(reason)


def xfail(reason: str = "") -> None:
    """Mark the current test as expected to fail.

    This function immediately marks the current test as an expected failure
    by raising an XFailException. Compatible with pytest.xfail() behavior.

    Args:
        reason: The reason for expecting the test to fail.

    Raises:
        XFailException: Always raised to mark the test as expected to fail.

    Example:
        if bug_not_fixed():
            xfail("Bug #123 not yet fixed")
    """
    raise XFailException(reason)


def _parse_version(version_str: str) -> Tuple:
    """Parse a version string into a comparable tuple.

    Handles common version formats like "1.2.3", "1.2.3a1", "1.2.3.dev0".
    Non-numeric parts are sorted after numeric parts.

    Args:
        version_str: Version string to parse.

    Returns:
        Tuple of version components for comparison.

    Example:
        _parse_version("1.2.3") -> (1, 2, 3)
        _parse_version("1.2.3a1") -> (1, 2, 3, 'a1')
    """
    # Split on dots
    parts = version_str.strip().split(".")
    result = []

    for part in parts:
        # Try to extract numeric prefix
        match = re.match(r"^(\d+)(.*)?$", part)
        if match:
            result.append(int(match.group(1)))
            if match.group(2):
                result.append(match.group(2))
        else:
            result.append(part)

    return tuple(result)


def importorskip(modname: str, minversion: Optional[str] = None):
    """Import and return a module, or skip the test if not available.

    This function attempts to import the specified module. If the import fails
    or the module version is below the required minimum, the test is skipped.
    Compatible with pytest.importorskip() behavior.

    Args:
        modname: The name of the module to import.
        minversion: Optional minimum version string. If provided, the module's
            __version__ attribute is checked against this value.

    Returns:
        The imported module if successful.

    Raises:
        SkipException: If the module cannot be imported or version is too low.

    Example:
        numpy = importorskip("numpy")
        pandas = importorskip("pandas", minversion="1.0.0")
    """
    import importlib

    try:
        mod = importlib.import_module(modname)
    except ImportError:
        skip(f"{modname} not available")
        # The skip() call raises, but we add return for type checkers
        return None  # pragma: no cover

    if minversion is not None:
        version = getattr(mod, "__version__", "0.0.0")
        if _parse_version(version) < _parse_version(minversion):
            skip(f"{modname} >= {minversion} required (found {version})")

    return mod


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

    sock = None
    sock_file = None
    try:
        sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        sock.connect(_debug_socket_path)
        sock_file = sock.makefile("rw", buffering=1, encoding="utf-8")
        debugger = TachPdb(sock_file)
        frame = sys._getframe(1)
        debugger.set_trace(frame)
    except Exception as e:
        print(f"[tach] ERROR: Failed to start debug session: {e}", file=sys.stderr)
    finally:
        # Always clean up socket resources
        if sock_file is not None:
            try:
                sock_file.close()
            except Exception as e:
                _logger.debug("Socket file close error: %s", e)
        if sock is not None:
            try:
                sock.close()
            except Exception as e:
                _logger.debug("Socket close error: %s", e)


sys.breakpointhook = tach_breakpointhook


def inject_entropy():
    """Re-seed RNGs and reset fork-unsafe state to break the Clone Curse."""
    import random
    import logging
    import threading

    seed = time.time_ns() % (2**32)
    random.seed(seed)

    # Reseed OpenSSL DRBG to prevent identical SSL nonces across forked workers
    try:
        import ctypes
        import ctypes.util

        ssl_lib_path = ctypes.util.find_library("ssl")
        if ssl_lib_path:
            ssl_lib = ctypes.CDLL(ssl_lib_path)
            # Note: hasattr on CDLL may not work reliably; try/except is the real safeguard
            if hasattr(ssl_lib, "RAND_add"):
                ssl_lib.RAND_add.argtypes = [
                    ctypes.c_char_p,
                    ctypes.c_int,
                    ctypes.c_double,
                ]
                entropy_bytes = os.urandom(32)
                ssl_lib.RAND_add(entropy_bytes, 32, 32.0)
    except Exception as e:
        _logger.debug("OpenSSL workaround skipped: %s", e)

    # CRITICAL: Reset logging module locks after fork
    # The logging module uses RLocks that become corrupted after fork()
    # because the lock state is shared but the threads are not.
    # We ONLY reset locks, NOT handlers or loggerDict -- those contain
    # valid configuration from the zygote that tests depend on
    # (e.g. Django's logging config, assertLogs handlers).
    try:
        logging._lock = threading.RLock()

        if hasattr(logging.Logger, "manager") and logging.Logger.manager:
            logging.Logger.manager._lock = threading.RLock()

        # Recreate handler locks (handlers have .lock, loggers do not)
        for handler in logging.root.handlers:
            if hasattr(handler, "lock"):
                handler.lock = threading.RLock()

        if hasattr(logging.Logger, "manager") and logging.Logger.manager:
            for _name, logger_ref in logging.Logger.manager.loggerDict.items():
                if isinstance(logger_ref, logging.Logger):
                    for handler in logger_ref.handlers:
                        if hasattr(handler, "lock"):
                            handler.lock = threading.RLock()
    except Exception as e:
        _logger.debug("Logging lock reset error: %s", e)

    if "numpy" in sys.modules:
        try:
            sys.modules["numpy"].random.seed(seed)
        except Exception as e:
            _logger.debug("Numpy seed error: %s", e)

    if "torch" in sys.modules:
        try:
            sys.modules["torch"].manual_seed(seed)
        except Exception as e:
            _logger.debug("Torch seed error: %s", e)


# =============================================================================
# ZERO-COPY LOADER: sys.meta_path Import Hook
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

# =============================================================================
# PHASE 5.3: HOT RELOADING - Module Cleanup Infrastructure
# =============================================================================

# Baseline snapshot of sys.modules captured after Zygote initialization
# This is the "golden state" - modules loaded BEFORE any test imports
_INITIAL_MODULES: Optional[Set[str]] = None

# Modules to NEVER remove (critical for runtime stability)
# Uses tuple for efficient startswith() prefix matching
_PROTECTED_PREFIXES = (
    "sys",
    "builtins",
    "__main__",
    "_thread",
    "threading",
    "importlib",
    "_frozen_importlib",
    "_imp",
    "tach_rust",
    "tach_harness",
    "_pytest",
    "pytest",
    "pluggy",
    "py",
    "django",
    "encodings",
    "codecs",
    "io",
    "_io",
    "os",
    "posix",
    "errno",
    "stat",
    "_stat",
    "abc",
    "typing",
    "types",
    "functools",
    "collections",
    "warnings",
    "weakref",
    "contextlib",
    "logging",
    "_logging",
)


def post_fork_init() -> bool:
    """Initialize worker after fork - called ONCE at start of worker lifecycle.

    This function:
    1. Performs post-fork hygiene (RNG reseed, logging reset)
    2. Installs the Tach import hook for zero-copy module loading
    3. Captures baseline sys.modules for Hot Reloading
    4. Initiates snapshot handshake with Supervisor if TACH_SUPERVISOR_SOCK is set
    5. Freezes (SIGSTOP) for Supervisor to capture golden snapshot

    Returns True if snapshot mode is enabled, False otherwise.
    """
    global _CAN_RECYCLE, _INITIAL_MODULES

    # 1. Post-fork hygiene
    inject_entropy()

    # 2. Install import hook for zero-copy module loading
    # This must be done BEFORE snapshot to be part of the golden state
    install_tach_import_hook()

    # 3. Capture baseline sys.modules for hot reloading
    # This snapshot defines what modules are "framework" vs "test-imported"
    _INITIAL_MODULES = set(sys.modules.keys())
    print(
        f"[tach:harness] Captured {len(_INITIAL_MODULES)} baseline modules",
        file=sys.stderr,
    )

    # 4. Check if snapshot mode is enabled
    import os

    supervisor_sock = os.environ.get("TACH_SUPERVISOR_SOCK")
    if not supervisor_sock:
        # No snapshot mode - standard fork-server behavior
        return False

    # 5. Initialize snapshot mode via Rust FFI
    try:
        import tach_rust

        _CAN_RECYCLE = tach_rust.init_snapshot_mode(supervisor_sock)
        return _CAN_RECYCLE
    except ImportError:
        print("[tach:harness] WARN: tach_rust module not available", file=sys.stderr)
        return False
    except Exception as e:
        print(f"[tach:harness] WARN: Snapshot init failed: {e}", file=sys.stderr)
        return False


def can_recycle() -> bool:
    """Returns True if this worker can be recycled via userfaultfd reset."""
    return _CAN_RECYCLE


# =============================================================================
# ENHANCED FAILURE INTROSPECTION (Tasks 2.1, 2.2, 2.6)
# Captures local variables, source context, and formats assertions better
# =============================================================================

# Maximum length for individual values before truncation
_MAX_VALUE_LENGTH = 200
# Number of context lines to show before/after failing line
_CONTEXT_LINES = 2


def _truncate_value(value: str, max_length: int = _MAX_VALUE_LENGTH) -> str:
    """Truncate a value representation intelligently.

    For dicts, lists, and long strings, truncate with "..." and show length.

    Args:
        value: String representation of the value.
        max_length: Maximum length before truncation.

    Returns:
        Truncated string with length indicator if truncated.
    """
    if len(value) <= max_length:
        return value

    # Calculate the length indicator
    length_info = f" (len={len(value)})"

    # Leave room for "..." and length info
    truncate_at = max_length - 3 - len(length_info)
    if truncate_at < 10:
        truncate_at = 10

    return value[:truncate_at] + "..." + length_info


def _format_local_value(name: str, value) -> str:
    """Format a local variable for display.

    Args:
        name: Variable name.
        value: Variable value.

    Returns:
        Formatted string like "name = repr(value)"
    """
    try:
        repr_value = repr(value)
        return f"    {name} = {_truncate_value(repr_value)}"
    except Exception:
        return f"    {name} = <repr failed>"


def _get_source_context(
    filename: str, lineno: int, context_lines: int = _CONTEXT_LINES
) -> Optional[str]:
    """Get source code context around a specific line.

    Args:
        filename: Path to the source file.
        lineno: Line number (1-indexed).
        context_lines: Number of lines before and after to include.

    Returns:
        Formatted source context with line numbers, or None if unavailable.
    """
    import linecache

    lines = []
    start = max(1, lineno - context_lines)
    end = lineno + context_lines + 1

    for i in range(start, end):
        line = linecache.getline(filename, i)
        if not line:
            continue

        # Mark the failing line with an arrow
        prefix = ">>> " if i == lineno else "    "
        lines.append(f"{prefix}{i:4d} | {line.rstrip()}")

    if lines:
        return "\n".join(lines)
    return None


def _extract_locals_from_traceback(tb) -> Optional[dict]:
    """Extract local variables from the deepest frame in a traceback.

    Args:
        tb: Traceback object.

    Returns:
        Dict of local variables, or None if extraction fails.
    """
    if tb is None:
        return None

    # Walk to the deepest frame
    while tb.tb_next is not None:
        tb = tb.tb_next

    frame = tb.tb_frame
    if frame is None:
        return None

    # Filter out dunder variables and modules
    locals_dict = {}
    for name, value in frame.f_locals.items():
        # Skip dunder variables
        if name.startswith("__") and name.endswith("__"):
            continue
        # Skip module imports
        if isinstance(value, type(sys)):
            continue
        # Skip functions and classes (keep instances)
        if callable(value) and not hasattr(value, "__dict__"):
            continue
        locals_dict[name] = value

    return locals_dict if locals_dict else None


def _get_failing_location(tb) -> Tuple[Optional[str], Optional[int]]:
    """Get the filename and line number of the failing assertion.

    Args:
        tb: Traceback object.

    Returns:
        Tuple of (filename, lineno) or (None, None) if extraction fails.
    """
    if tb is None:
        return None, None

    # Walk to the deepest frame
    while tb.tb_next is not None:
        tb = tb.tb_next

    return tb.tb_frame.f_code.co_filename, tb.tb_lineno


def _format_enhanced_failure(
    exc_type: Type[BaseException],
    exc_value: BaseException,
    exc_tb,
    original_longrepr: str,
) -> str:
    """Format an enhanced failure message with locals and source context.

    Args:
        exc_type: Exception type.
        exc_value: Exception value.
        exc_tb: Traceback object.
        original_longrepr: Original pytest longrepr string.

    Returns:
        Enhanced failure message with locals and source context.
    """
    parts = []

    # Get failing location
    filename, lineno = _get_failing_location(exc_tb)

    # Add source context if available
    if filename and lineno:
        source_context = _get_source_context(filename, lineno)
        if source_context:
            parts.append("")
            parts.append("Source context:")
            parts.append(source_context)

    # Add local variables
    locals_dict = _extract_locals_from_traceback(exc_tb)
    if locals_dict:
        parts.append("")
        parts.append("Local variables:")
        for name, value in sorted(locals_dict.items()):
            parts.append(_format_local_value(name, value))

    # Add the original traceback
    if parts:
        parts.append("")
        parts.append("Traceback:")

    parts.append(original_longrepr)

    return "\n".join(parts)


# =============================================================================
# PLUGIN DETECTION (v0.2.0)
# Detects installed pytest plugins and warns about unsupported ones
# =============================================================================

# Plugins that are known to work with Tach (or are explicitly disabled)
_SUPPORTED_PLUGINS: set = {
    "pytest",  # Core pytest
    "pytest-timeout",  # We handle timeouts ourselves
    "pytest-xdist",  # We disable this, but it's known
    "pytest-cov",  # We disable this, but it's known
    "pytest-sugar",  # We disable this, but it's known
    "pytest-asyncio",  # We disable this, but it's known
    "pytest-trio",  # We disable this, but it's known
    "pytest-django",  # We disable this, but markers are detected
    "pytest-mock",  # Works with Tach
    "pytest-env",  # Environment variables - effects are replayed
    "pytest-randomly",  # Test ordering - we handle in supervisor
    "pytest-order",  # Test ordering - we handle in supervisor
    "pytest-lazy-fixture",  # Fixture handling
    "pytest-factoryboy",  # Fixtures
    "pytest-freezegun",  # Time mocking
    "pytest-httpx",  # HTTP mocking
    "pytest-responses",  # HTTP mocking
    "pytest-vcr",  # HTTP recording
    "pytest-benchmark",  # Benchmarking (may have issues with fork)
}

# Plugins that are known to NOT work with Tach
_UNSUPPORTED_PLUGINS: dict = {
    "pytest-parallel": "Uses multiprocessing, conflicts with Tach workers",
    "pytest-forked": "Fork-based isolation conflicts with Tach's fork model",
    "pytest-testmon": "Requires file watching, not compatible with snapshot model",
    "pytest-picked": "Git-based selection conflicts with static discovery",
    "pytest-split": "Test splitting conflicts with Tach's scheduler",
}


def detect_installed_plugins() -> dict:
    """Detect installed pytest plugins using importlib.metadata.

    Returns:
        Dict with keys:
        - 'installed': List of installed plugin names
        - 'supported': List of supported plugins
        - 'unsupported': Dict of unsupported plugins with reasons
        - 'unknown': List of unknown plugins (may or may not work)
    """
    try:
        from importlib.metadata import distributions
    except ImportError:
        # Python < 3.8 fallback
        try:
            from importlib_metadata import distributions
        except ImportError:
            return {
                "installed": [],
                "supported": [],
                "unsupported": {},
                "unknown": [],
                "error": "importlib.metadata not available",
            }

    installed = []
    supported = []
    unsupported = {}
    unknown = []

    for dist in distributions():
        name = dist.metadata.get("Name", "").lower()
        # Check if it's a pytest plugin (entry point group pytest11)
        try:
            eps = dist.entry_points
            is_pytest_plugin = any(ep.group == "pytest11" for ep in eps)
        except Exception:
            is_pytest_plugin = name.startswith("pytest-") or name.startswith("pytest_")

        if is_pytest_plugin or name.startswith("pytest-") or name.startswith("pytest_"):
            installed.append(name)

            if name in _SUPPORTED_PLUGINS:
                supported.append(name)
            elif name in _UNSUPPORTED_PLUGINS:
                unsupported[name] = _UNSUPPORTED_PLUGINS[name]
            else:
                unknown.append(name)

    return {
        "installed": sorted(installed),
        "supported": sorted(supported),
        "unsupported": unsupported,
        "unknown": sorted(unknown),
    }


def log_plugin_warnings() -> None:
    """Log warnings for unsupported or unknown plugins.

    Called during Zygote initialization to warn users about potential issues.
    """
    result = detect_installed_plugins()

    if result.get("error"):
        _tach_log(f"[tach:plugins] Warning: {result['error']}\n".encode())
        return

    # Log unsupported plugins as warnings
    for plugin, reason in result.get("unsupported", {}).items():
        _tach_log(
            f"[tach:plugins] WARNING: Plugin '{plugin}' is not supported: {reason}\n".encode(),
        )

    # Log unknown plugins as info (they might work)
    unknown = result.get("unknown", [])
    if unknown:
        _tach_log(
            f"[tach:plugins] INFO: Unknown plugins detected (may or may not work): {', '.join(unknown)}\n".encode(),
        )

    # Log summary
    installed_count = len(result.get("installed", []))
    supported_count = len(result.get("supported", []))
    if installed_count > 0:
        _tach_log(
            f"[tach:plugins] Detected {installed_count} pytest plugins ({supported_count} supported)\n".encode(),
        )


# =============================================================================
# HOOK EFFECT RECORDING (v0.2.0)
# Captures env and sys.path changes from session-level hooks (pytest_configure)
# These effects are cached and replayed in workers before test execution
# =============================================================================

# Global storage for recorded session-level hook effects
# Format: list of dicts with 'type' key ('SetEnv' or 'ModifySysPath')
_SESSION_HOOK_EFFECTS: list = []


def _capture_sys_path_snapshot() -> list:
    """Capture current sys.path as a list (copy)."""
    return list(sys.path)


def _compute_sys_path_delta(before: list, after: list) -> list:
    """Compute sys.path changes between two snapshots.

    Returns list of SysPathEffect-compatible dicts.
    """
    effects = []

    # Find paths added (in after but not in before)
    # Check position to determine if prepended or appended
    for path in after:
        if path not in before:
            # Determine action based on position
            idx = after.index(path)
            if idx == 0 or (idx < len(before) // 2):
                action = "prepend"
            else:
                action = "append"
            effects.append(
                {
                    "type": EFFECT_TYPE_MODIFY_SYS_PATH,
                    "action": action,
                    "path": path,
                }
            )

    # Find paths removed (in before but not in after)
    for path in before:
        if path not in after:
            effects.append(
                {
                    "type": EFFECT_TYPE_MODIFY_SYS_PATH,
                    "action": "remove",
                    "path": path,
                }
            )

    return effects


def _compute_env_delta(before: dict, after: dict) -> list:
    """Compute environment variable changes between two snapshots.

    Returns list of SetEnv effect dicts compatible with HookEffect.
    """
    effects = []

    # Find added or changed variables
    for key, value in after.items():
        if key not in before:
            effects.append(
                {
                    "type": EFFECT_TYPE_SET_ENV,
                    "key": key,
                    "value": value,
                }
            )
        elif before[key] != value:
            effects.append(
                {
                    "type": EFFECT_TYPE_SET_ENV,
                    "key": key,
                    "value": value,
                }
            )

    # Note: We don't track unset env vars for session-level hooks
    # as pytest_configure typically only adds env vars

    return effects


def _get_recorded_session_effects() -> list:
    """Internal: Get pre-recorded session effects (deprecated, use get_session_hook_effects).

    Note: This function exists for backwards compatibility. The actual recording
    happens in init_session() which captures env and sys.path changes during
    pytest configuration. Use get_session_hook_effects() instead.

    Returns:
        List of effect dicts with 'type' key being 'SetEnv' or 'ModifySysPath'
    """
    return _SESSION_HOOK_EFFECTS


def get_session_hook_effects() -> list:
    """Get recorded session-level hook effects for transmission to workers.

    This is the primary API for retrieving effects recorded during init_session().
    The Zygote calls this after init_session() completes to get effects for
    transmission to the Supervisor.

    Returns:
        List of effect dicts with 'type' key being 'SetEnv' or 'ModifySysPath'
    """
    return _SESSION_HOOK_EFFECTS


def get_collected_tests():
    """Return the authoritative test list from pytest's collection.

    Called by the Zygote after init_session() to send collected test metadata
    back to the Rust Supervisor. The Supervisor uses this as the source of truth
    for which tests exist, replacing Rust-only AST discovery.

    Returns a list of dicts, each containing:
      - node_id: Full pytest node ID (str)
      - file_path: File path relative to project root (str)
      - markers: List of marker names (list[str])
      - is_async: Whether the test is async (bool)
    """
    result = []
    for node_id, item in _ITEMS_MAP.items():
        fspath = str(getattr(item, "fspath", ""))
        try:
            file_path = os.path.relpath(fspath, os.getcwd())
        except ValueError:
            file_path = fspath

        markers = [m.name for m in getattr(item, "own_markers", [])]

        obj = getattr(item, "obj", None)
        func = getattr(obj, "__func__", obj) if obj else None
        is_async = inspect.iscoroutinefunction(func) if func else False

        result.append(
            {
                "node_id": node_id,
                "file_path": file_path,
                "markers": markers,
                "is_async": is_async,
            }
        )

    return result


def _load_hook_function(
    hook_module_path: str,
    hook_function_name: str,
    module_name: str,
) -> tuple[object | None, str | None]:
    """Load a hook function from a conftest.py file.

    This helper function handles the common module loading logic used by
    call_hook_impl and call_collection_modifyitems.

    Args:
        hook_module_path: Path to the conftest.py containing the hook
        hook_function_name: Name of the function to load
        module_name: Name to use for the module in sys.modules

    Returns:
        Tuple of (hook_func, error_message)
        - hook_func: The callable hook function, or None if loading failed
        - error_message: Error description if loading failed, or None on success
    """
    import importlib.util

    # Load the hook module
    spec = importlib.util.spec_from_file_location(module_name, hook_module_path)
    if spec is None or spec.loader is None:
        return None, f"Could not load module spec from {hook_module_path}"

    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module

    try:
        spec.loader.exec_module(module)
    except Exception as e:
        return None, f"Failed to execute module {hook_module_path}: {e}"

    # Get the hook function
    hook_func = getattr(module, hook_function_name, None)
    if hook_func is None:
        return None, f"Function {hook_function_name} not found in {hook_module_path}"

    if not callable(hook_func):
        return None, f"{hook_function_name} is not callable"

    return hook_func, None


def call_hook_impl(
    hook_name: str,
    hook_module_path: str,
    hook_function_name: str,
    hook_args: dict,
) -> dict:
    """Execute a pytest hook implementation and capture its effects.

    This function loads and executes a hook implementation from a conftest.py file,
    capturing any environment variable or sys.path changes made by the hook.

    The function uses try/finally to ensure effects are ALWAYS captured, even if
    the hook raises an exception or returns early on error paths.

    Args:
        hook_name: Name of the hook (e.g., "pytest_configure")
        hook_module_path: Path to the conftest.py containing the hook
        hook_function_name: Name of the function to call
        hook_args: Arguments to pass to the hook function

    Returns:
        Dict with keys:
        - 'success': bool - Whether the hook executed successfully
        - 'effects': list - List of effect dicts (SetEnv, ModifySysPath)
        - 'error': str or None - Error message if hook failed
        - 'result': Any - Return value from hook (usually None for pytest hooks)
    """
    # Capture state BEFORE hook execution
    env_before = dict(os.environ)
    sys_path_before = _capture_sys_path_snapshot()

    result = {
        "success": False,
        "effects": [],
        "error": None,
        "result": None,
    }

    try:
        # Load the hook function using shared helper
        hook_func, error = _load_hook_function(
            hook_module_path,
            hook_function_name,
            f"conftest_{hook_name}",
        )
        if error:
            result["error"] = error
            return result

        # Execute the hook
        try:
            hook_result = hook_func(**hook_args)
            result["result"] = hook_result
            result["success"] = True
        except Exception as e:
            result["error"] = f"Hook {hook_name} raised exception: {e}"
            # Note: We still capture effects even on exception
            # because the hook may have made changes before failing

    finally:
        # ALWAYS capture effects, even on error paths
        # This ensures we don't lose track of state changes
        env_after = dict(os.environ)
        sys_path_after = _capture_sys_path_snapshot()

        env_effects = _compute_env_delta(env_before, env_after)
        sys_path_effects = _compute_sys_path_delta(sys_path_before, sys_path_after)
        result["effects"] = env_effects + sys_path_effects

    return result


def call_collection_modifyitems(
    hook_module_path: str,
    config: object,
    items: list,
) -> dict:
    """Execute pytest_collection_modifyitems hook and capture its effects.

    This function loads and executes pytest_collection_modifyitems from a conftest.py,
    capturing environment and sys.path changes. The hook may modify the items list
    in-place (filtering, reordering).

    Uses try/finally to ensure effects are ALWAYS captured, even on errors.

    Args:
        hook_module_path: Path to the conftest.py containing the hook
        config: Pytest config object to pass to the hook
        items: List of pytest items (may be modified in-place by hook)

    Returns:
        Dict with keys:
        - 'success': bool - Whether the hook executed successfully
        - 'effects': list - List of effect dicts (SetEnv, ModifySysPath)
        - 'error': str or None - Error message if hook failed
        - 'items_before': int - Number of items before hook execution
        - 'items_after': int - Number of items after hook execution
        - 'removed': list - Node IDs of items that were removed
        - 'reordered': bool - Whether items were reordered
    """
    # Capture state BEFORE hook execution
    env_before = dict(os.environ)
    sys_path_before = _capture_sys_path_snapshot()

    # Capture items state before
    items_before_count = len(items)
    items_before_ids = [getattr(item, "nodeid", str(item)) for item in items]

    result = {
        "success": False,
        "effects": [],
        "error": None,
        "items_before": items_before_count,
        "items_after": items_before_count,
        "removed": [],
        "reordered": False,
    }

    try:
        # Load the hook function using shared helper
        hook_func, error = _load_hook_function(
            hook_module_path,
            "pytest_collection_modifyitems",
            "conftest_collection_modifyitems",
        )
        if error:
            result["error"] = error
            return result

        # Execute the hook (it modifies items in-place)
        try:
            hook_func(config=config, items=items)
            result["success"] = True
        except Exception as e:
            result["error"] = f"pytest_collection_modifyitems raised exception: {e}"
            # Note: We still capture effects even on exception

        # Capture items state after
        items_after_count = len(items)
        items_after_ids = [getattr(item, "nodeid", str(item)) for item in items]

        result["items_after"] = items_after_count

        # Determine removed items
        removed = [nid for nid in items_before_ids if nid not in items_after_ids]
        result["removed"] = removed

        # Determine if reordered (same items but different order)
        if (
            items_before_count == items_after_count
            and items_before_ids != items_after_ids
        ):
            result["reordered"] = True

    finally:
        # ALWAYS capture effects, even on error paths
        env_after = dict(os.environ)
        sys_path_after = _capture_sys_path_snapshot()

        env_effects = _compute_env_delta(env_before, env_after)
        sys_path_effects = _compute_sys_path_delta(sys_path_before, sys_path_after)
        result["effects"] = env_effects + sys_path_effects

    return result


def apply_cached_effects(effects: list) -> int:
    """Apply cached effects to the worker process.

    Args:
        effects: List of effect dicts from TestPayload.cached_effects

    Returns:
        Number of effects applied
    """
    provided = len(effects)
    applied = 0

    for effect in effects:
        effect_type = effect.get("type")

        if effect_type == EFFECT_TYPE_SET_ENV:
            key = effect.get("key")
            value = effect.get("value")
            if key and value is not None:
                os.environ[key] = value
                applied += 1

        elif effect_type == EFFECT_TYPE_MODIFY_SYS_PATH:
            action = effect.get("action", "append")
            path = effect.get("path")
            if path:
                if action == "prepend":
                    if path not in sys.path:
                        sys.path.insert(0, path)
                        applied += 1
                elif action == "append":
                    if path not in sys.path:
                        sys.path.append(path)
                        applied += 1
                elif action == "remove":
                    if path in sys.path:
                        sys.path.remove(path)
                        applied += 1

        elif effect_type == EFFECT_TYPE_ASYNCIO_SETUP:
            # Configure EventLoopManager from cached effect
            loop_scope = effect.get("loop_scope", "function")
            auto_mode = effect.get("auto_mode", False)
            EventLoopManager.get_instance().configure(loop_scope, auto_mode)
            applied += 1

    # Debug logging: show count of effects provided vs applied
    if provided > 0:
        if applied > 0:
            print(
                f"[tach:harness] Applied {applied}/{provided} cached hook effects",
                file=sys.stderr,
            )
        else:
            # Warning: effects were provided but none were applied
            print(
                f"[tach:harness] WARNING: {provided} effects provided but 0 applied (possible mismatch)",
                file=sys.stderr,
            )

    return applied


# =============================================================================
# ZYGOTE COLLECTION PATTERN
# Pytest session is initialized ONCE in Zygote, workers inherit via fork CoW
# =============================================================================

_SESSION = None
_ITEMS_MAP = {}  # nodeid -> pytest Item
_PARAM_FUZZY_INDEX = {}  # "file::test_name" -> [pytest Item, ...]
_DJANGO_OLD_CONFIG = None  # Stores setup_databases() return for teardown


def _fuzzy_parametrize_lookup(rust_node_id: str) -> Any:
    """Fuzzy lookup for parametrized tests when Rust-generated IDs don't match pytest's.

    Rust's AST parser can't evaluate runtime expressions like json.dumps(),
    OSError(), Exception(), etc. This causes its generated parameter IDs to
    differ from pytest's. This function matches using multiple strategies:

    1. Suffix matching: Rust params are a suffix of pytest params
    2. Shared suffix matching: the last N parts that Rust resolved match
    3. Index extraction: Rust fallback IDs contain the value index ({param}{N})
       which maps to the Nth pytest candidate in order

    This is generalizable — works for any repo where Rust discovery generates
    different parametrize IDs than pytest collection.
    """
    if "[" not in rust_node_id:
        return None

    base, rust_params = rust_node_id.rsplit("[", 1)
    rust_params = rust_params.rstrip("]")

    candidates = _PARAM_FUZZY_INDEX.get(base, [])
    if not candidates:
        return None

    rust_parts = rust_params.split("-")

    # Strategy 1: Suffix matching — Rust params are a contiguous suffix of pytest params
    for item in candidates:
        pytest_params = item.nodeid.rsplit("[", 1)[-1].rstrip("]")
        pytest_parts = pytest_params.split("-")
        if len(rust_parts) <= len(pytest_parts):
            if pytest_parts[-len(rust_parts) :] == rust_parts:
                return item

    # Strategy 2: Shared suffix — find candidates where the last N resolved parts match
    # This handles cases where Rust resolves SOME params (e.g. True, None) but not others
    if len(rust_parts) > 1:
        for item in candidates:
            pytest_params = item.nodeid.rsplit("[", 1)[-1].rstrip("]")
            pytest_parts = pytest_params.split("-")
            # Count matching parts from the end
            match_count = 0
            for r, p in zip(reversed(rust_parts), reversed(pytest_parts)):
                if r == p:
                    match_count += 1
                else:
                    break
            # If at least half the parts match from the end, it's likely the same test
            if match_count > 0 and match_count >= len(rust_parts) // 2:
                # Verify this is unique — only return if exactly one candidate matches
                matches = []
                for c in candidates:
                    cp = c.nodeid.rsplit("[", 1)[-1].rstrip("]").split("-")
                    mc = 0
                    for r, p in zip(reversed(rust_parts), reversed(cp)):
                        if r == p:
                            mc += 1
                        else:
                            break
                    if mc == match_count:
                        matches.append(c)
                if len(matches) == 1:
                    return matches[0]

    # Strategy 3: Index extraction — Rust fallback IDs use {param_name}{index} pattern
    # Extract the index from fallback parts and map to the Nth candidate
    import re

    for part in rust_parts:
        m = re.match(r"^[a-zA-Z_][a-zA-Z0-9_]*?(\d+)$", part)
        if m:
            idx = int(m.group(1))
            if 0 <= idx < len(candidates):
                return candidates[idx]
            break

    return None


def _override_db_session_fixtures(session) -> None:
    """Replace DB-related session fixtures with no-ops.

    Tach manages test database creation via _setup_django_test_db() in the
    zygote. Framework plugins (like pytest-django) may also register session
    fixtures that call setup_databases(). If both run, the DB gets created
    twice, causing errors.

    We detect DB fixtures by name patterns and replace their func with a
    no-op generator that yields immediately. This is general -- it doesn't
    hardcode specific plugin names, just detects DB-related fixture names.
    """
    DB_PATTERNS = {"db_setup", "database_setup"}

    try:
        fm = session._fixturemanager
        overridden = []

        for argname, fixdef_list in fm._arg2fixturedefs.items():
            is_db = any(pat in argname for pat in DB_PATTERNS)
            if not is_db:
                continue

            for fixdef in fixdef_list:
                if getattr(fixdef, "scope", None) == "session":
                    # Replace with a no-op generator
                    original_func = fixdef.func

                    def _noop_fixture(**kwargs):
                        yield None

                    fixdef.func = _noop_fixture
                    overridden.append(argname)

        if overridden:
            _tach_log(
                f"[tach:harness] Overrode {len(overridden)} DB session fixtures "
                f"with no-ops: {', '.join(overridden)} "
                f"(tach manages DB lifecycle)\n".encode(),
            )
    except Exception as e:
        _tach_log(
            f"[tach:harness] WARN: DB fixture override failed: {e}\n".encode(),
        )


class _MinimalFixtureRequest:
    """Minimal request object for session-scoped fixture execution.

    Session fixtures that take a `request` parameter typically only need
    config, session, and getini/getoption access. This provides enough
    to satisfy most framework plugins without requiring a real test item.
    """

    def __init__(self, config, session, resolved=None):
        self.config = config
        self.session = session
        self.node = session
        self.fspath = None
        self.scope = "session"
        self._finalizers = []
        self._resolved = resolved or {}

    def addfinalizer(self, finalizer):
        self._finalizers.append(finalizer)

    def getfixturevalue(self, argname):
        if argname in self._resolved:
            return self._resolved[argname]
        raise NotImplementedError(
            f"getfixturevalue('{argname}') not available in zygote context"
        )


def _trigger_session_fixtures(cfg, session) -> None:
    """Force-execute session-scoped autouse fixtures in the Zygote.

    Framework plugins register session-scoped autouse fixtures that perform
    critical one-time setup. These fixtures normally run lazily when
    the first test requests them, but tach forks workers before any test
    runs, so workers would miss the setup.

    Strategy: find all session-scoped autouse FixtureDefs and execute their
    underlying functions directly, resolving dependencies in topological order.

    DB-related fixtures (those that call setup_databases) are SKIPPED here
    because tach has its own _setup_django_test_db() called from the Rust
    zygote code. We detect DB fixtures by checking if they depend on a
    "db_blocker" or "db_setup" named fixture.
    """
    if not session.items:
        return

    try:
        fm = session._fixturemanager

        # Collect ALL session-scoped fixture definitions (both autouse and not)
        # We need non-autouse ones too for dependency resolution.
        all_session_fixdefs = {}
        autouse_names = []
        for argname, fixdef_list in fm._arg2fixturedefs.items():
            for fixdef in fixdef_list:
                if getattr(fixdef, "scope", None) == "session":
                    all_session_fixdefs[argname] = fixdef
                    if getattr(fixdef, "_autouse", False):
                        autouse_names.append(argname)

        if not autouse_names:
            return

        _tach_log(
            f"[tach:harness] Found {len(autouse_names)} session-scoped "
            f"autouse fixtures: {', '.join(autouse_names)}\n".encode(),
        )

        # Skip DB-related fixtures -- tach handles DB setup separately.
        # Detection: a fixture is DB-related if its name or any dependency
        # contains "db_setup", "db_blocker", or "database".
        DB_FIXTURE_MARKERS = {"db_setup", "db_blocker", "database", "django_db_setup"}

        def _is_db_fixture(name):
            for marker in DB_FIXTURE_MARKERS:
                if marker in name:
                    return True
            fixdef = all_session_fixdefs.get(name)
            if fixdef:
                for dep in getattr(fixdef, "argnames", []):
                    for marker in DB_FIXTURE_MARKERS:
                        if marker in dep:
                            return True
            return False

        executed = {}
        _generators = []
        _in_progress = set()  # Cycle detection

        def _exec(name):
            if name in executed:
                return executed[name]
            if name in _in_progress:
                return None  # Break circular dependency
            _in_progress.add(name)

            fixdef = all_session_fixdefs.get(name)
            if fixdef is None:
                _in_progress.discard(name)
                return None

            if _is_db_fixture(name):
                _tach_log(
                    f"[tach:harness] Skipping DB fixture: {name}\n".encode(),
                )
                executed[name] = None
                _in_progress.discard(name)
                return None

            for dep in getattr(fixdef, "argnames", []):
                if dep in all_session_fixdefs and dep not in executed:
                    _exec(dep)

            # Build kwargs
            kwargs = {}
            for dep in getattr(fixdef, "argnames", []):
                if dep in executed:
                    kwargs[dep] = executed[dep]
                elif dep == "request":
                    kwargs[dep] = _MinimalFixtureRequest(cfg, session, executed)

            try:
                result = fixdef.func(**kwargs)
                if hasattr(result, "__next__"):
                    val = next(result)
                    _generators.append((name, result))
                    executed[name] = val
                else:
                    executed[name] = result
                _tach_log(
                    f"[tach:harness] Executed session fixture: {name}\n".encode(),
                )
            except Exception as e:
                _tach_log(
                    f"[tach:harness] WARN: Session fixture '{name}' "
                    f"failed: {e}\n".encode(),
                )
                executed[name] = None
            finally:
                _in_progress.discard(name)

        for name in autouse_names:
            _exec(name)

        # CRITICAL: Replace executed fixtures with no-ops so workers don't
        # re-execute them. Session fixtures run in the zygote and workers
        # inherit the state via fork. If pytest tries to run them again in
        # workers, they'll fail (e.g. setup_test_environment raises
        # "already called"). By replacing the func, the fixture becomes a
        # cache-hit that immediately returns None.
        for name, result in executed.items():
            fixdef = all_session_fixdefs.get(name)
            if fixdef is not None:
                # Store the result in the fixture's cache so pytest sees it
                # as already resolved.
                # Format: (value, cache_key, exc_info_or_None)
                # cache_key=None matches session-scoped non-parametrized fixtures
                fixdef.cached_result = (result, None, None)
                overridden_count = 0
                overridden_count += 1
        if executed:
            _tach_log(
                f"[tach:harness] Cached {len(executed)} session fixture results "
                f"for worker inheritance\n".encode(),
            )

        if _generators:
            import atexit

            def _teardown():
                for name, gen in reversed(_generators):
                    try:
                        next(gen, None)
                    except (StopIteration, Exception):
                        pass

            atexit.register(_teardown)

    except Exception as e:
        _tach_log(
            f"[tach:harness] WARN: Session fixture trigger failed: {e}\n".encode(),
        )


def _neutralize_plugin_conflicts(cfg) -> None:
    """Undo harmful side-effects from framework plugins after configure.

    Framework plugins (pytest-django, pytest-asyncio, etc.) do two things:
    1. Session-level setup in pytest_configure — we WANT this.
    2. Per-test hooks and patches — some conflict with tach's execution model.

    This function neutralizes the conflicts while preserving the setup.
    It is deliberately general: it inspects what plugins DID rather than
    hardcoding knowledge of specific plugins. If a plugin monkey-patched
    something harmful, we detect and undo it.
    """
    pm = cfg.pluginmanager

    # --- pytest-django DB blocker ---
    # pytest-django patches BaseDatabaseWrapper.ensure_connection to block
    # all DB access by default. Tach manages DB isolation itself via
    # savepoints in _apply_django_db_isolation(). We need to unblock so
    # workers can access the DB freely.
    #
    # Detection: check if ensure_connection was replaced with a function
    # that raises RuntimeError("Database access not allowed").
    try:
        from django.db.backends.base.base import BaseDatabaseWrapper

        ec = BaseDatabaseWrapper.ensure_connection
        # pytest-django's blocker is a static method that raises RuntimeError
        if callable(ec) and getattr(ec, "__name__", "") == "_blocking_wrapper":
            # Find the blocker and unblock it
            _unblock_django_db(cfg)
        elif callable(ec):
            # Check if it's a wrapper by trying to detect the RuntimeError
            import types

            if isinstance(ec, types.FunctionType):
                # Inspect source or co_consts for the blocking message
                consts = getattr(ec, "__code__", None)
                if consts and any(
                    "Database access not allowed" in str(c)
                    for c in getattr(consts, "co_consts", ())
                ):
                    _unblock_django_db(cfg)
    except ImportError:
        pass  # Django not installed, nothing to do

    # --- pytest-asyncio mode override ---
    # When pytest-asyncio is loaded in "auto" mode, it wraps all async test
    # functions. Tach has its own EventLoopManager that handles this.
    # We don't disable the plugin (we need its configure-time setup like
    # event loop policy), but we prevent it from wrapping tests.
    try:
        asyncio_plugin = pm.get_plugin("asyncio")
        if asyncio_plugin is not None:
            # Unregister asyncio's per-test hooks but keep configure hooks
            _selective_unregister_hooks(
                pm,
                asyncio_plugin,
                keep={"pytest_configure", "pytest_addoption"},
            )
            _tach_log(
                b"[tach:harness] pytest-asyncio: kept configure, removed per-test hooks\n",
            )
    except Exception:
        pass

    # --- pytest-trio ---
    try:
        trio_plugin = pm.get_plugin("trio")
        if trio_plugin is not None:
            _selective_unregister_hooks(
                pm,
                trio_plugin,
                keep={"pytest_configure", "pytest_addoption"},
            )
    except Exception:
        pass

    # --- pytest-cov ---
    # Detect pytest-cov and warn that tach has native PEP 669 coverage.
    # Disable pytest-cov to avoid double-counting and overhead.
    try:
        cov_plugin = pm.get_plugin("pytest_cov")
        if cov_plugin is not None:
            if not _TACH_QUIET:
                _tach_log(
                    b"[tach:harness] pytest-cov detected; use tach --coverage instead for zero-overhead PEP 669 coverage\n",
                )
            pm.unregister(cov_plugin)
    except Exception:
        pass

    # --- pytest-xdist ---
    # Detect pytest-xdist and disable it; tach has native parallelism.
    try:
        xdist_plugin = pm.get_plugin("xdist")
        if xdist_plugin is not None:
            if not _TACH_QUIET:
                _tach_log(
                    b"[tach:harness] pytest-xdist detected; tach -n auto provides native parallelism\n",
                )
            pm.unregister(xdist_plugin)
    except Exception:
        pass


def _unblock_django_db(cfg) -> None:
    """Restore the original BaseDatabaseWrapper.ensure_connection.

    pytest-django's DjangoDbBlocker.block() replaces ensure_connection
    with a wrapper that raises RuntimeError. We restore the original
    so tach workers can access the DB freely (tach manages isolation).
    """
    try:
        # Import the stash key directly from pytest-django's plugin module
        from pytest_django.plugin import blocking_manager_key

        blocker = cfg.stash[blocking_manager_key]
        blocker.unblock()
        _tach_log(
            b"[tach:harness] DB blocker neutralized (tach manages isolation)\n",
        )
    except (ImportError, KeyError):
        # pytest-django not installed or blocker not registered — try fallback
        try:
            from django.db.backends.base.base import BaseDatabaseWrapper

            # The blocker replaces ensure_connection with a static _blocking_wrapper.
            # Restore from the first non-blocking version in the MRO.
            for klass in BaseDatabaseWrapper.__mro__:
                if "ensure_connection" in klass.__dict__:
                    original = klass.__dict__["ensure_connection"]
                    if not (
                        callable(original)
                        and getattr(original, "__name__", "") == "_blocking_wrapper"
                    ):
                        BaseDatabaseWrapper.ensure_connection = original
                        _tach_log(
                            b"[tach:harness] DB blocker neutralized via MRO fallback\n",
                        )
                        break
        except ImportError:
            pass
    except Exception as e:
        _tach_log(
            f"[tach:harness] WARN: Failed to unblock Django DB: {e}\n".encode(),
        )


def _selective_unregister_hooks(pm, plugin, keep: set[str]) -> None:
    """Unregister a plugin's hooks EXCEPT those in the keep set.

    This allows framework plugins to do their configure-time setup
    while preventing their per-test hooks from interfering with tach.
    """
    try:
        # Get all hook callers this plugin participates in
        plugin_name = pm.get_name(plugin) or str(plugin)
        # We can't selectively unregister individual hooks easily,
        # so we unregister the plugin entirely and re-register a
        # stripped-down version that only has the kept hooks.
        import types

        # Collect the hook implementations we want to keep
        kept_attrs = {}
        for attr_name in keep:
            impl = getattr(plugin, attr_name, None)
            if impl is not None:
                kept_attrs[attr_name] = impl

        # Unregister the full plugin
        pm.unregister(plugin)

        # Re-register a lightweight wrapper with only kept hooks
        if kept_attrs:
            wrapper = types.SimpleNamespace(**kept_attrs)
            pm.register(wrapper, name=f"{plugin_name}_tach_stripped")
    except Exception as e:
        _tach_log(
            f"[tach:harness] WARN: selective unregister failed for {plugin}: {e}\n".encode(),
        )


def init_session(root_dir: str):
    """Initialize pytest session in Zygote BEFORE forking workers.

    This pays the "Pytest Tax" (config parsing, plugin loading, test collection)
    exactly ONCE. Workers inherit the session via Copy-on-Write fork semantics.

    Hook Effect Recording (v0.2.0):
    We capture env and sys.path before and after pytest configuration to record
    effects from session-level hooks (pytest_configure). These effects are cached
    and replayed in workers before test execution.

    Plugin Detection (v0.2.0):
    We detect installed pytest plugins and warn about unsupported ones.
    """
    global _SESSION, _ITEMS_MAP, _SESSION_HOOK_EFFECTS, _PARAM_FUZZY_INDEX

    _tach_log(f"[tach:harness] init_session: {root_dir}\n".encode())

    # PLUGIN DETECTION (v0.2.0): Warn about unsupported plugins
    log_plugin_warnings()

    # HOOK EFFECT RECORDING: Capture state BEFORE pytest configuration
    env_before = dict(os.environ)
    sys_path_before = _capture_sys_path_snapshot()

    # Disable only INFRASTRUCTURE plugins that tach fully replaces.
    # Framework integration plugins (django, asyncio, trio, etc.) are LEFT
    # ENABLED so their pytest_configure hooks run — these do critical session
    # setup (e.g. pytest-django's setup_test_environment adds "testserver" to
    # ALLOWED_HOSTS, installs instrumented template renderer, etc.).
    #
    # Disabled:
    #   terminal    — tach has its own reporter
    #   cacheprovider — pytest result caching not needed
    #   cov         — tach has native PEP 669 coverage
    #   xdist       — tach IS the parallelizer
    #   sugar       — output formatting conflicts
    #
    # NOT disabled (framework plugins that do session-level setup):
    #   django      — setup_test_environment, DB blocker, test ordering
    #   asyncio     — event loop policy, auto mode detection
    #   trio        — trio event loop setup
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
    ]

    keyword = os.environ.get("TACH_KEYWORD", "")
    if keyword:
        args.extend(["-k", keyword])

    markers = os.environ.get("TACH_MARKERS", "")
    if markers:
        args.extend(["-m", markers])

    for key, val in os.environ.items():
        if key.startswith("TACH_DISABLE_PLUGIN_") and val == "1":
            plugin_name = key[len("TACH_DISABLE_PLUGIN_") :].lower().replace("_", "-")
            args.extend(["-p", f"no:{plugin_name}"])

    import_mode = os.environ.get("TACH_IMPORT_MODE")
    if import_mode:
        args.extend(["--import-mode", import_mode])

    confcutdir = os.environ.get("TACH_CONFCUTDIR")
    if confcutdir:
        args.extend(["--confcutdir", confcutdir])

    override_ini = os.environ.get("TACH_OVERRIDE_INI", "")
    if override_ini:
        for ini_val in override_ini.split("\x1f"):
            if ini_val:
                args.extend(["-o", ini_val])

    if os.environ.get("TACH_STRICT_MARKERS") == "1":
        args.append("--strict-markers")
    if os.environ.get("TACH_RUNXFAIL") == "1":
        args.append("--runxfail")

    deselect = os.environ.get("TACH_DESELECT", "")
    if deselect:
        for node_id in deselect.split("\x1f"):
            if node_id:
                args.extend(["--deselect", node_id])

    ignore_paths = os.environ.get("TACH_IGNORE", "")
    if ignore_paths:
        for path in ignore_paths.split("\x1f"):
            if path:
                args.extend(["--ignore", path])

    ignore_globs = os.environ.get("TACH_IGNORE_GLOB", "")
    if ignore_globs:
        for glob in ignore_globs.split("\x1f"):
            if glob:
                args.extend(["--ignore-glob", glob])

    if os.environ.get("TACH_PDB") == "1":
        args.append("--pdb")
    if os.environ.get("TACH_WERROR") == "1":
        args.extend(["-W", "error"])
    if os.environ.get("TACH_NEW_FIRST") == "1":
        args.append("--nf")

    log_file = os.environ.get("TACH_LOG_FILE")
    if log_file:
        args.extend(["--log-file", log_file])

    html_report = os.environ.get("TACH_HTML")
    if html_report:
        args.extend(["--html", html_report])

    basetemp = os.environ.get("TACH_BASETEMP")
    if basetemp:
        args.extend(["--basetemp", basetemp])

    randomly_seed = os.environ.get("TACH_RANDOMLY_SEED")
    if randomly_seed:
        args.extend(["-p", "randomly", "--randomly-seed", randomly_seed])

    if os.environ.get("TACH_FORKED") == "1":
        args.append("--forked")

    if os.environ.get("TACH_SETUP_PLAN") == "1":
        args.append("--setup-plan")
    if os.environ.get("TACH_SETUP_SHOW") == "1":
        args.append("--setup-show")
    if os.environ.get("TACH_SETUP_ONLY") == "1":
        args.append("--setup-only")

    if os.environ.get("TACH_DOCTEST_MODULES") == "1":
        args.append("--doctest-modules")

    extra_args = os.environ.get("TACH_PYTEST_ARGS", "")
    if extra_args:
        args.extend(extra_args.split("\x1f"))

    if os.environ.get("TACH_PYARGS") == "1":
        args.append("--pyargs")

    if os.environ.get("TACH_NO_CAPTURE") == "1":
        args.extend(["-s"])

    cfg = _pytest.config._prepareconfig(args)
    cfg._do_configure()

    # Ensure options that framework plugins expect exist on the namespace.
    # Disabling `no:terminal` removes the `verbose` option that plugins
    # (e.g. pytest-django's django_db_setup) read via config.option.verbose.
    # We inject missing attributes with sensible defaults so plugins don't crash.
    _show_locals = os.environ.get("TACH_SHOWLOCALS") == "1"
    _option_defaults = {"verbose": 0, "tbstyle": "auto", "showlocals": _show_locals}
    for attr, default in _option_defaults.items():
        if not hasattr(cfg.option, attr):
            setattr(cfg.option, attr, default)

    # POST-CONFIGURE FIXUPS: Undo harmful side-effects from framework plugins
    # while keeping their beneficial session-level setup.
    #
    # This is the general pattern: let plugins run their configure phase
    # (which does critical env setup), then neutralize hooks/patches that
    # conflict with tach's execution model.
    _neutralize_plugin_conflicts(cfg)

    # Patch FixtureDef.execute to consume async fixtures at resolution time.
    # This MUST be in init_session() so workers inherit the patch via fork.
    from _pytest.fixtures import FixtureDef

    if not getattr(FixtureDef.execute, "_tach_patched", False):
        _original_execute = FixtureDef.execute

        def _patched_execute(self, request):
            result = _original_execute(self, request)
            if inspect.isasyncgen(result) or asyncio.iscoroutine(result):
                scope = getattr(self, "scope", "function")
                result = AsyncFixtureWrapper.consume_async_fixture(
                    self.argname, result, scope
                )
            return result

        _patched_execute._tach_patched = True
        FixtureDef.execute = _patched_execute

    # Register TachFixturePlugin to intercept async fixtures (indirect deps)
    global _TACH_FIXTURE_PLUGIN
    if not cfg.pluginmanager.has_plugin("tach_fixture_plugin"):
        _TACH_FIXTURE_PLUGIN = TachFixturePlugin()
        cfg.pluginmanager.register(_TACH_FIXTURE_PLUGIN, "tach_fixture_plugin")

    # HOOK EFFECT RECORDING: Capture state AFTER pytest_configure
    # At this point, pytest_configure hooks have run via cfg._do_configure()
    env_after = dict(os.environ)
    sys_path_after = _capture_sys_path_snapshot()

    # Compute deltas and store as session hook effects
    env_effects = _compute_env_delta(env_before, env_after)
    sys_path_effects = _compute_sys_path_delta(sys_path_before, sys_path_after)
    _SESSION_HOOK_EFFECTS = env_effects + sys_path_effects

    if _SESSION_HOOK_EFFECTS:
        _tach_log(
            f"[tach:harness] Recorded {len(_SESSION_HOOK_EFFECTS)} session hook effects "
            f"({len(env_effects)} env, {len(sys_path_effects)} sys.path)\n".encode(),
        )

    _SESSION = _pytest.main.Session.from_config(cfg)
    cfg.hook.pytest_sessionstart(session=_SESSION)

    # Parse asyncio_mode from pyproject.toml and configure EventLoopManager
    _configure_asyncio_from_pyproject(root_dir)

    _SESSION.perform_collect()

    def _nodeid_suffix(nodeid: str) -> str:
        parts = nodeid.split("::")
        return "::".join(parts[1:]) if len(parts) > 1 else nodeid

    # --lf filter: select only tests whose IDs match the lastfailed cache
    lf_file = os.environ.get("TACH_LF_FILE", "")
    if lf_file and os.path.isfile(lf_file) and _SESSION.items:
        lf_ids = set(open(lf_file).read().splitlines())
        original_count = len(_SESSION.items)
        _SESSION.items = [
            item for item in _SESSION.items if _nodeid_suffix(item.nodeid) in lf_ids
        ]
        _tach_log(
            f"[tach:harness] --lf filter: {len(_SESSION.items)}/{original_count} tests\n".encode(),
        )

    # --ff reorder: move lastfailed tests to front of collection
    ff_file = os.environ.get("TACH_FF_FILE", "")
    if ff_file and os.path.isfile(ff_file) and _SESSION.items:
        ff_ids = set(open(ff_file).read().splitlines())
        failed = [i for i in _SESSION.items if _nodeid_suffix(i.nodeid) in ff_ids]
        rest = [i for i in _SESSION.items if _nodeid_suffix(i.nodeid) not in ff_ids]
        _SESSION.items = failed + rest
        _tach_log(
            f"[tach:harness] --ff reorder: {len(failed)} failed-first, {len(rest)} rest\n".encode(),
        )

    # TRIGGER SESSION-SCOPED AUTOUSE FIXTURES in the Zygote so workers
    # inherit their effects via fork CoW.  Framework plugins register
    # session-scoped autouse fixtures that do critical setup:
    #   - pytest-django: django_test_environment (setup_test_environment)
    #   - pytest-django: django_db_setup (creates test database)
    # These fixtures normally only run when the first test requests them.
    # We force them here so the setup happens once in the parent process.
    _trigger_session_fixtures(cfg, _SESSION)

    # Override DB-related session fixtures with no-ops.
    # Tach creates the test DB via _setup_django_test_db() (called from Rust).
    # If a framework plugin (e.g. pytest-django) registers a django_db_setup
    # fixture that also calls setup_databases(), workers would re-create the DB.
    # We replace those fixtures' func with a no-op so they yield immediately.
    _override_db_session_fixtures(_SESSION)

    for item in _SESSION.items:
        _ITEMS_MAP[item.nodeid] = item

    # Node ID alignment: Rust discovery builds node IDs relative to the project
    # root (CWD), but pytest builds them relative to rootdir.  When rootdir is a
    # subdirectory (e.g. "test-aistudio/"), the paths diverge:
    #   Rust sends:  "test-aistudio/tests/file.py::test_foo"
    #   Pytest has:  "tests/file.py::test_foo"
    # Fix: store a second key per item using the project-root-relative path so
    # the O(1) lookup in run_test() succeeds regardless of which form is used.
    project_root = os.path.abspath(os.getcwd())
    rootdir_abs = os.path.abspath(str(cfg.rootdir))
    if project_root != rootdir_abs:
        for item in _SESSION.items:
            abs_path = str(item.fspath)
            rel_to_project = os.path.relpath(abs_path, project_root)
            # item.nodeid = "tests/file.py::Class::method[param]"
            # Split at first "::" to replace only the file path portion
            parts = item.nodeid.split("::", 1)
            if len(parts) == 2:
                full_key = rel_to_project + "::" + parts[1]
                if full_key not in _ITEMS_MAP:
                    _ITEMS_MAP[full_key] = item

    # Build fuzzy parametrize index: "file::test_base" -> [items]
    # This enables fuzzy matching when Rust-generated parametrize IDs differ
    # from pytest's (e.g. runtime expressions like json.dumps(), OSError()).
    global _PARAM_FUZZY_INDEX
    _PARAM_FUZZY_INDEX = {}
    for item in _SESSION.items:
        if "[" in item.nodeid:
            base = item.nodeid.rsplit("[", 1)[0]
            _PARAM_FUZZY_INDEX.setdefault(base, []).append(item)
    # Also index with project-root-relative keys for dual-key alignment
    if project_root != rootdir_abs:
        for item in _SESSION.items:
            if "[" in item.nodeid:
                abs_path = str(item.fspath)
                rel_to_project = os.path.relpath(abs_path, project_root)
                parts = item.nodeid.split("::", 1)
                if len(parts) == 2:
                    full_base = (rel_to_project + "::" + parts[1]).rsplit("[", 1)[0]
                    if full_base not in _PARAM_FUZZY_INDEX:
                        _PARAM_FUZZY_INDEX[full_base] = _PARAM_FUZZY_INDEX.get(
                            item.nodeid.rsplit("[", 1)[0], []
                        )

    fuzzy_count = sum(len(v) for v in _PARAM_FUZZY_INDEX.values())
    _tach_log(
        f"[tach:harness] Pre-collected {len(_ITEMS_MAP)} tests ({fuzzy_count} fuzzy parametrize entries)\n".encode(),
    )


def _parse_django_db_marker(
    marker_info: list[dict[str, Any]] | None,
) -> dict[str, Any] | None:
    """Parse @pytest.mark.django_db marker arguments.

    Looks for a marker named 'django_db' in the marker_info list and
    extracts its arguments for database isolation configuration.

    Args:
        marker_info: List of marker dicts with 'name' and 'args' keys.
            Each dict should have 'name' (str) and optional 'args' (dict).

    Returns:
        Dict with keys 'transaction', 'reset_sequences', 'databases' if
        django_db marker is present. Returns None if no django_db marker found.
    """
    if not marker_info:
        return None

    for marker in marker_info:
        if isinstance(marker, dict) and marker.get("name") == "django_db":
            args = marker.get("args", {})
            return {
                "transaction": args.get("transaction", False),
                "reset_sequences": args.get("reset_sequences", False),
                "databases": args.get("databases", None),  # None means all databases
            }

    return None


def _is_django_available() -> bool:
    """Check if Django is available and configured in the current process.

    This function checks both that Django is imported into sys.modules and
    that Django settings have been configured. Both conditions must be true
    for Django database operations to work.

    Returns:
        True if Django is imported and settings are configured, False otherwise.
    """
    if "django" not in sys.modules:
        return False
    try:
        from django.conf import settings

        return settings.configured
    except ImportError:
        return False


def _close_django_connections() -> None:
    """Close all Django database connections before fork.

    This should be called in the Zygote before forking workers to ensure
    each worker gets fresh database connections. Closing connections in
    the parent prevents file descriptor sharing issues and ensures each
    worker establishes its own database connections.

    Note:
        This function fails silently if Django is not available or if
        closing connections fails, as it's called during fork preparation.
    """
    if not _is_django_available():
        return
    try:
        from django.db import connections
        from django.db import DatabaseError

        connections.close_all()
    except DatabaseError as e:
        print(
            f"[tach:harness] WARN: Database error closing connections: {e}",
            file=sys.stderr,
        )
    except Exception as e:
        _logger.debug("Django connection close error: %s", e)


def _setup_django_test_db() -> None:
    """Create Django test database in Zygote before forking workers.

    Calls django.test.utils.setup_databases() to create a test DB with
    all migrations applied. Workers inherit this via fork.

    Skips if a session-scoped fixture (e.g. pytest-django's django_db_setup)
    already called setup_databases(). Detection: check if the default DB
    connection's settings already point at a test database.

    Reads TACH_REUSE_DB / TACH_CREATE_DB env vars for keepdb behavior.
    Registers atexit handler for teardown.
    """
    global _DJANGO_OLD_CONFIG

    if not _is_django_available():
        return

    # Skip if a framework plugin's session fixture already set up the DB.
    # pytest-django's django_db_setup fixture calls setup_databases() which
    # modifies connection.settings_dict['NAME'] to point to a test DB.
    # We detect this by checking if the DB name already contains 'test_'.
    try:
        from django.db import connections

        default_name = connections["default"].settings_dict.get("NAME", "")
        if isinstance(default_name, str) and "test_" in str(default_name):
            print(
                "[tach:harness] Django test DB already configured by session fixture, skipping",
                file=sys.stderr,
            )
            _close_django_connections()
            return
    except Exception:
        pass

    reuse_db = os.environ.get("TACH_REUSE_DB", "") == "1"
    create_db = os.environ.get("TACH_CREATE_DB", "") == "1"

    # --create-db overrides --reuse-db (matches pytest-django semantics)
    keepdb = reuse_db and not create_db

    try:
        from django.test.utils import setup_databases

        print(
            f"[tach:harness] Setting up Django test database (keepdb={keepdb})",
            file=sys.stderr,
        )

        _DJANGO_OLD_CONFIG = setup_databases(
            verbosity=1,
            interactive=False,
            keepdb=keepdb,
        )

        print("[tach:harness] Django test database ready", file=sys.stderr)

        # Close connections BEFORE fork — workers must get fresh FDs
        _close_django_connections()

        # Register teardown — Zygote has no clean Python shutdown path
        import atexit

        atexit.register(_teardown_django_test_db)

    except Exception as e:
        print(
            f"[tach:harness] ERROR: Django test DB setup failed: {e}",
            file=sys.stderr,
        )
        raise


def _teardown_django_test_db() -> None:
    """Tear down Django test database via atexit.

    Skips DROP DATABASE when keepdb=True (--reuse-db preserves DB for next run).
    """
    global _DJANGO_OLD_CONFIG

    if _DJANGO_OLD_CONFIG is None:
        return

    reuse_db = os.environ.get("TACH_REUSE_DB", "") == "1"
    create_db = os.environ.get("TACH_CREATE_DB", "") == "1"
    keepdb = reuse_db and not create_db

    try:
        from django.test.utils import teardown_databases

        teardown_databases(
            _DJANGO_OLD_CONFIG,
            verbosity=1,
            keepdb=keepdb,
        )
        print("[tach:harness] Django test database torn down", file=sys.stderr)
    except Exception as e:
        print(
            f"[tach:harness] WARN: Django test DB teardown failed: {e}",
            file=sys.stderr,
        )
    finally:
        _DJANGO_OLD_CONFIG = None


def _apply_django_db_isolation(
    marker_args: dict[str, Any] | None,
) -> list[tuple[str, str]]:
    """Apply database isolation based on marker args.

    Uses SAVEPOINT for transaction isolation when transaction=False (default).
    When transaction=True, no isolation is applied (test manages its own transactions).

    This function creates savepoints on all requested databases, allowing
    test changes to be rolled back after test completion.

    Args:
        marker_args: Parsed django_db marker arguments from _parse_django_db_marker,
            or None for default behavior (isolate all databases with savepoints).

    Returns:
        List of (alias, savepoint_id) tuples for cleanup via _cleanup_django_db_isolation.
        Returns empty list if Django is not available or transaction=True.
    """
    if not _is_django_available():
        return []

    try:
        from django.conf import settings

        if not settings.configured:
            print(
                "[tach:harness] WARN: Django settings not configured, skipping DB isolation",
                file=sys.stderr,
            )
            return []
    except ImportError:
        return []

    from django.db import connections, transaction, DatabaseError

    # Close stale connections first.
    # IMPORTANT: This is required for fork-based isolation. After fork(),
    # database connections inherited from the zygote process are stale and
    # MUST be closed before creating new ones. This is NOT "connection pool
    # thrashing" - it's required for correctness in forked child processes.
    # See: Django docs on database connections in multi-process environments.
    try:
        connections.close_all()
    except DatabaseError as e:
        print(
            f"[tach:harness] WARN: Database error closing connections: {e}",
            file=sys.stderr,
        )
    except Exception as e:
        print(
            f"[tach:harness] WARN: Failed to close Django connections: {e}",
            file=sys.stderr,
        )

    # If no marker_args, apply default isolation to all databases
    if marker_args is None:
        marker_args = {
            "transaction": False,
            "reset_sequences": False,
            "databases": None,
        }

    # If transaction=True, the test manages its own transactions and may
    # commit directly to the database. Savepoint-based isolation won't work
    # because commits escape the savepoint. Instead, these tests are marked
    # toxic so the worker exits after running — the Zygote's clean DB
    # snapshot is restored on the next fork.
    if marker_args.get("transaction", False):
        _close_django_connections()
        return []

    # Determine which databases to isolate
    databases = marker_args.get("databases")
    if databases is None:
        databases = list(connections)

    # Validate database aliases exist
    valid_databases = []
    for alias in databases:
        if alias in connections:
            valid_databases.append(alias)
        else:
            print(
                f"[tach:harness] WARN: Unknown database alias '{alias}', skipping",
                file=sys.stderr,
            )

    # Create savepoints for each database
    savepoints = []
    for alias in valid_databases:
        try:
            # Ensure connection is usable
            conn = connections[alias]
            conn.ensure_connection()

            # Create savepoint for isolation
            sid = transaction.savepoint(using=alias)
            savepoints.append((alias, sid))
        except DatabaseError as e:
            # Database-specific error during savepoint creation
            print(
                f"[tach:harness] WARN: Database error creating savepoint for '{alias}': {e}",
                file=sys.stderr,
            )
            print(
                f"[tach:harness] INFO: Rolling back {len(savepoints)} previously created savepoints",
                file=sys.stderr,
            )
            for prev_alias, prev_sid in reversed(savepoints):
                try:
                    transaction.savepoint_rollback(prev_sid, using=prev_alias)
                except DatabaseError as rollback_error:
                    print(
                        f"[tach:harness] WARN: Database error rolling back savepoint for '{prev_alias}': {rollback_error}",
                        file=sys.stderr,
                    )
                except Exception as rollback_error:
                    print(
                        f"[tach:harness] WARN: Failed to rollback savepoint for '{prev_alias}': {rollback_error}",
                        file=sys.stderr,
                    )
            return []  # Return empty - no isolation applied
        except Exception as e:
            # Unexpected error - still roll back and fail gracefully
            print(
                f"[tach:harness] WARN: Failed to create savepoint for '{alias}': {e}",
                file=sys.stderr,
            )
            print(
                f"[tach:harness] INFO: Rolling back {len(savepoints)} previously created savepoints",
                file=sys.stderr,
            )
            for prev_alias, prev_sid in reversed(savepoints):
                try:
                    transaction.savepoint_rollback(prev_sid, using=prev_alias)
                except Exception as rollback_error:
                    print(
                        f"[tach:harness] WARN: Failed to rollback savepoint for '{prev_alias}': {rollback_error}",
                        file=sys.stderr,
                    )
            return []  # Return empty - no isolation applied

    return savepoints


def _cleanup_django_db_isolation(savepoints: list[tuple[str, str]]) -> None:
    """Rollback savepoints after test to restore database state.

    This function rolls back all savepoints created by _apply_django_db_isolation,
    restoring the database to its pre-test state. Savepoints are rolled back in
    reverse order to handle any dependencies between databases.

    Args:
        savepoints: List of (alias, savepoint_id) tuples from _apply_django_db_isolation.
            Each tuple contains the database alias and the savepoint identifier.
    """
    if not savepoints:
        return

    from django.db import transaction, DatabaseError

    for alias, sid in reversed(savepoints):
        try:
            transaction.savepoint_rollback(sid, using=alias)
        except DatabaseError as e:
            print(
                f"[tach:harness] WARN: Database error rolling back savepoint for '{alias}': {e}",
                file=sys.stderr,
            )
        except Exception as e:
            print(
                f"[tach:harness] WARN: Failed to rollback savepoint for '{alias}': {e}",
                file=sys.stderr,
            )


def run_test(
    file_path: str,
    node_id: str,
    cached_effects: list[dict[str, Any]] | None = None,
    marker_info: list[dict[str, Any]] | None = None,
) -> tuple[int, float, str, bool]:
    """
    Execute a single pytest test item using pre-collected session.

    FAST PATH: Item lookup is O(1) from _ITEMS_MAP.
    No pytest config, no collection, just run the test.

    Hook Effect Replay (v0.2.0):
    Before running the test, we apply cached effects from session-level hooks.
    This ensures that environment variables and sys.path modifications from
    pytest_configure are restored after memory reset.

    Django Database Isolation (v0.2.1):
    Parses marker_info to extract @pytest.mark.django_db arguments and applies
    appropriate database isolation using SAVEPOINT/ROLLBACK.

    Args:
        file_path: Path to the test file
        node_id: Pytest node ID (e.g., "tests/test_foo.py::test_bar")
        cached_effects: Optional list of effects to apply before test (from TestPayload)
        marker_info: Optional list of marker info dicts with 'name' and 'args' keys

    Returns:
        Tuple of (status, duration, message, thread_leaked)
        - status: Test result status code
        - duration: Execution time in seconds
        - message: Error/skip message if any
        - thread_leaked: True if test spawned threads that didn't terminate
    """
    global _SESSION, _ITEMS_MAP, _thread_leak_detected

    # Reset thread leak flag for this test
    _thread_leak_detected = False

    # CRITICAL: Reset logging lock FIRST before anything else
    # fork() corrupts the logging module's RLock, causing segfaults
    import logging
    import threading

    logging._lock = threading.RLock()

    # Ensure stdout/stderr use UTF-8 encoding after fork/redirect
    # Workers redirect stdout to memfd which may default to ASCII encoding,
    # causing UnicodeEncodeError in libraries like colorama
    for stream_name in ("stdout", "stderr"):
        stream = getattr(sys, stream_name, None)
        if stream is not None and hasattr(stream, "reconfigure"):
            try:
                stream.reconfigure(encoding="utf-8", errors="replace")
            except Exception:
                pass

    # Set PYTHONIOENCODING for any subprocesses spawned by tests.
    # Without this, subprocesses inherit the memfd's ASCII encoding,
    # causing UnicodeDecodeError when reading non-ASCII output
    # (e.g. Django's makemessages command with UTF-8 .po files).
    os.environ.setdefault("PYTHONIOENCODING", "utf-8")

    # Ensure filesystem encoding is UTF-8, not ASCII.
    # After fork+redirect to memfd, Python may detect ASCII as the
    # filesystem encoding. This breaks tests that write non-ASCII
    # filenames (e.g. staticfiles ⊗.txt). Setting LANG/LC_ALL
    # ensures consistent UTF-8 handling for all file operations.
    for env_var in ("LANG", "LC_ALL", "LC_CTYPE"):
        if (
            not os.environ.get(env_var)
            or "ascii" in os.environ.get(env_var, "").lower()
        ):
            os.environ[env_var] = "C.UTF-8"

    # HOOK EFFECT REPLAY (v0.2.0):
    # Apply cached effects before running the test
    # If cached_effects is provided (from TestPayload), use those
    # Otherwise, use session hook effects from Python globals (inherited from Zygote)
    if cached_effects:
        apply_cached_effects(cached_effects)
    else:
        # Apply session-level hook effects from Python globals
        session_effects = get_session_hook_effects()
        if session_effects:
            apply_cached_effects(session_effects)

    inject_entropy()
    start = time.perf_counter()

    # Record initial thread count BEFORE test execution
    initial_thread_count = threading.active_count()

    try:
        # O(1) lookup from pre-collected items
        target_item = _ITEMS_MAP.get(node_id)

        if not target_item:
            # Fuzzy fallback for parametrize ID mismatches.
            # Rust AST can't evaluate runtime expressions (json.dumps, OSError, etc.)
            # so its generated IDs may differ from pytest's. Try matching by:
            # 1. Same file + test name (before '[')
            # 2. Rust params are a suffix of pytest params (extra params were dropped)
            target_item = _fuzzy_parametrize_lookup(node_id)

        if not target_item:
            duration = time.perf_counter() - start
            return (
                STATUS_HARNESS_ERROR,
                duration,
                f"Test not found in Zygote session: {node_id}\nAvailable: {len(_ITEMS_MAP)} items",
                False,  # No thread leak detection for failed lookup
            )

        # Check for @pytest.mark.allow_threads marker
        allow_threads = _has_allow_threads_marker(target_item)

        # Native Async Support
        original_obj = target_item.obj
        func_to_check = original_obj
        if hasattr(original_obj, "__func__"):
            func_to_check = original_obj.__func__

        is_async_test = False
        if inspect.iscoroutinefunction(func_to_check):
            is_async_test = True
            # Use EventLoopManager for scoped loop management
            loop_manager = EventLoopManager.get_instance()

            # Parse asyncio marker for loop_scope configuration
            loop_scope, _has_asyncio_marker = parse_asyncio_marker(target_item)
            loop_manager.configure(
                loop_scope=loop_scope, auto_mode=loop_manager.auto_mode
            )

            # Scope transition handling (Issue #43)
            fspath = str(getattr(target_item, "fspath", ""))
            item_cls = getattr(target_item, "cls", None)
            class_name = (
                f"{item_cls.__module__}.{item_cls.__name__}" if item_cls else None
            )
            loop_manager.on_scope_transition(fspath, class_name)

            scope_key = loop_manager.get_scope_key(target_item)

            def make_sync_wrapper(async_fn, mgr, key):
                def sync_wrapper(*args, **kwargs):
                    loop = mgr.get_loop(key)
                    # Set as current event loop for this thread
                    asyncio.set_event_loop(loop)
                    AsyncFixtureWrapper.set_loop(loop)

                    # Consume async fixtures in kwargs before calling test
                    for name, val in list(kwargs.items()):
                        if inspect.isasyncgen(val) or asyncio.iscoroutine(val):
                            kwargs[name] = AsyncFixtureWrapper.consume_async_fixture(
                                name, val, "function"
                            )

                    try:
                        return loop.run_until_complete(async_fn(*args, **kwargs))
                    finally:
                        # Only close function-scoped loops immediately
                        if mgr.current_scope == "function":
                            mgr.close_scope(key)
                            asyncio.set_event_loop(None)

                return sync_wrapper

            target_item.obj = make_sync_wrapper(original_obj, loop_manager, scope_key)

        # Django Database Isolation (v0.2.1)
        # Parse marker_info to get django_db settings and apply SAVEPOINT isolation
        django_db_args = _parse_django_db_marker(marker_info)
        django_savepoints = _apply_django_db_isolation(django_db_args)

        # Set up scoped event loop for async fixture consumption BEFORE runtestprotocol
        if is_async_test:
            fixture_loop = loop_manager.get_loop(scope_key)
        else:
            fixture_scope_key = f"function:{target_item.nodeid}"
            loop_manager = EventLoopManager.get_instance()
            fixture_loop = loop_manager.get_loop(fixture_scope_key)

        asyncio.set_event_loop(fixture_loop)
        AsyncFixtureWrapper.set_loop(fixture_loop)

        # Scope transition handling for async fixtures
        fspath_for_fixture = str(getattr(target_item, "fspath", ""))
        item_cls_for_fixture = getattr(target_item, "cls", None)
        class_name_for_fixture = (
            f"{item_cls_for_fixture.__module__}.{item_cls_for_fixture.__name__}"
            if item_cls_for_fixture
            else None
        )
        AsyncFixtureWrapper.on_test_start(fspath_for_fixture, class_name_for_fixture)

        # Invalidate stale async fixture caches inherited from Zygote parent
        from _pytest.fixtures import FixtureDef

        fixture_info = getattr(target_item, "_fixtureinfo", None)
        if fixture_info and hasattr(fixture_info, "name2fixturedefs"):
            for fixturedefs in fixture_info.name2fixturedefs.values():
                for fixturedef in fixturedefs:
                    cached = getattr(fixturedef, "cached_result", None)
                    if (
                        cached is not None
                        and isinstance(cached, tuple)
                        and len(cached) > 0
                    ):
                        val = cached[0]
                        if asyncio.iscoroutine(val) or inspect.isasyncgen(val):
                            scope = getattr(fixturedef, "scope", "function")
                            consumed = AsyncFixtureWrapper.consume_async_fixture(
                                fixturedef.argname, val, scope
                            )
                            fixturedef.cached_result = (
                                (consumed, cached[1], cached[2])
                                if len(cached) >= 3
                                else (consumed,)
                            )

        try:
            reports = _pytest.runner.runtestprotocol(
                target_item, nextitem=None, log=False
            )
        finally:
            AsyncFixtureWrapper.teardown_function_scope()
            # Rollback savepoints to restore database state
            _cleanup_django_db_isolation(django_savepoints)

        duration = time.perf_counter() - start

        # Thread leak detection: check if test spawned threads that outlived execution
        _thread_leak_detected = _detect_thread_leak(initial_thread_count, allow_threads)

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

            # Try to enhance the failure message with locals and source context
            # pytest stores exception info in the report when available
            try:
                exc_info = getattr(failed_report, "excinfo", None)
                if exc_info is not None:
                    # exc_info is a pytest.ExceptionInfo object
                    exc_type = exc_info.type
                    exc_value = exc_info.value
                    exc_tb = exc_info.tb
                    msg = _format_enhanced_failure(exc_type, exc_value, exc_tb, msg)
            except Exception as enhance_err:
                # If enhancement fails, use the original message
                # Debug logging for troubleshooting enhancement failures
                print(
                    f"[tach:harness] DEBUG: Enhanced failure formatting failed: {enhance_err}",
                    file=sys.stderr,
                )

            return (STATUS_FAIL, duration, msg, _thread_leak_detected)

        if skipped_report:
            skip_reason = (
                str(skipped_report.longrepr) if skipped_report.longrepr else ""
            )
            return (
                STATUS_SKIP,
                duration,
                f"Skipped: {skip_reason}",
                _thread_leak_detected,
            )

        return (STATUS_PASS, duration, "", _thread_leak_detected)

    except SystemExit as e:
        duration = time.perf_counter() - start
        return (STATUS_HARNESS_ERROR, duration, f"SystemExit: {e.code}", False)

    except Exception as e:
        duration = time.perf_counter() - start
        tb = traceback.format_exc()
        return (STATUS_HARNESS_ERROR, duration, f"Harness Error: {e}\n{tb}", False)

    finally:
        sys.stdout.flush()
        sys.stderr.flush()


# =============================================================================
# PHASE 4: WORKER LOOP INFRASTRUCTURE
# Prepared for Sub-Stage 4.3 (Persistent Workers)
# =============================================================================


def reset_worker_state() -> bool:
    """Reset worker memory state for next test execution.

    This is the Python-side of the Hypervisor Mode reset.
    Called after a safe test completes to prepare for the next test.

    Returns True if reset succeeded, False otherwise.
    """
    try:
        # Reset event loop manager to cleanup any lingering loops (Issue #43)
        EventLoopManager.reset()

        # Reset async fixture wrapper state
        try:
            AsyncFixtureWrapper.reset()
        except Exception as e:
            _logger.debug("AsyncFixtureWrapper reset failed: %s", e)

        import tach_rust

        tach_rust.reset_memory()
        return True
    except ImportError:
        print("[tach:harness] WARN: tach_rust not available for reset", file=sys.stderr)
        return False
    except Exception as e:
        print(f"[tach:harness] WARN: reset_memory failed: {e}", file=sys.stderr)
        return False


def cleanup_test_modules() -> int:
    """Remove test-imported modules from sys.modules.

    Hot Reloading Support

    This function:
    1. Identifies modules imported AFTER Zygote initialization
    2. Removes them from sys.modules (forcing re-import on next test)
    3. Protects critical modules from removal

    Called by Rust reset_and_signal_ready() BEFORE memory reset.

    Returns: Number of modules removed
    """
    global _INITIAL_MODULES

    if _INITIAL_MODULES is None:
        # Baseline not captured - skip cleanup
        return 0

    # Calculate delta: modules loaded AFTER baseline
    current_modules = set(sys.modules.keys())
    test_modules = current_modules - _INITIAL_MODULES

    # Filter out protected modules using efficient prefix matching
    to_remove = []
    for mod_name in test_modules:
        # Skip modules with protected prefixes
        if mod_name.startswith(_PROTECTED_PREFIXES):
            continue
        # Also skip submodules of protected prefixes
        for prefix in _PROTECTED_PREFIXES:
            if mod_name.startswith(prefix + "."):
                break
        else:
            to_remove.append(mod_name)

    # Remove in reverse order (children before parents)
    # This prevents import errors when parent modules reference children
    to_remove.sort(key=lambda x: x.count("."), reverse=True)

    removed_count = 0
    for mod_name in to_remove:
        try:
            del sys.modules[mod_name]
            removed_count += 1
        except KeyError:
            pass  # Already removed
        except Exception as e:
            # Log but don't crash - dirty worker is better than dead worker
            print(
                f"[tach:harness] WARN: Failed to remove {mod_name}: {e}",
                file=sys.stderr,
            )

    if removed_count > 0:
        print(
            f"[tach:harness] Cleaned up {removed_count} test modules", file=sys.stderr
        )

    return removed_count


def should_worker_exit(is_toxic: bool, thread_leaked: bool = False) -> bool:
    """Determine if worker should exit after test execution.

    Dual-Path Decision:
    - Toxic tests: Always exit (Isolation Mode)
    - Thread leak: Always exit (threads can't be cleaned up)
    - Safe tests: Can continue if reset succeeds (Hypervisor Mode)

    Args:
        is_toxic: Whether the test was marked as toxic
        thread_leaked: Whether the test spawned threads that didn't terminate

    Returns:
        True if worker should exit, False if it can continue
    """
    if is_toxic:
        # TOXIC PATH: Always exit
        # OS cleans up threads, file descriptors, network connections, etc.
        return True
    if thread_leaked:
        # THREAD LEAK: Worker is now contaminated, must exit
        # Threads can persist and affect subsequent tests
        return True
    else:
        # SAFE PATH: Can continue if reset succeeds
        # Sub-Stage 4.3 will use this to keep workers alive
        return False


# =============================================================================
# PERSISTENT WORKER LOOP
# This function is called by zygote.rs when Hypervisor Mode is enabled
# =============================================================================


def worker_loop_iteration(
    file_path: str,
    node_id: str,
    is_toxic: bool,
    cached_effects: list = None,
    marker_info: list = None,
) -> tuple:
    """Execute one iteration of the worker loop.

    This is the main entry point for persistent workers.
    It combines test execution with the dual-path decision.

    Args:
        file_path: Path to the test file
        node_id: Full pytest node ID
        is_toxic: Whether this test is toxic
        cached_effects: Optional list of effects to apply before test
        marker_info: Optional list of marker info for Django isolation

    Returns:
        Tuple of (status, duration, message, should_exit)
        - status: Test result status code
        - duration: Execution time in seconds
        - message: Error message if any
        - should_exit: Whether worker should exit after this test
    """
    # 1. Execute the test (now returns 4 values including thread_leaked)
    status, duration, message, thread_leaked = run_test(
        file_path, node_id, cached_effects, marker_info
    )

    # 2. Determine if worker should exit (consider thread leaks)
    exit_after = should_worker_exit(is_toxic, thread_leaked)

    # 3. If continuing (safe test), reset memory
    if not exit_after:
        if not reset_worker_state():
            # Reset failed - must exit to be safe
            exit_after = True
            print("[tach:harness] Reset failed, forcing exit", file=sys.stderr)

    return (status, duration, message, exit_after)


# =============================================================================
# PHASE 5.1: ZERO-OVERHEAD COVERAGE (PEP 669)
# =============================================================================
#
# This section implements coverage collection using Python 3.12+'s sys.monitoring
# API (PEP 669). This is dramatically faster than sys.settrace because:
#
# 1. Callbacks are per-code-object, not per-frame
# 2. Events can be selectively enabled/disabled
# 3. The VM can optimize out disabled events
#
# Architecture:
# - Python LINE events call tach_rust.record_line(code_id, lineno)
# - Rust writes to a shared memory ring buffer (memfd_create)
# - Supervisor's aggregator thread drains the buffer and maps code_id to files
# =============================================================================

# Coverage state
_coverage_enabled = False
_coverage_tool_id = None

# Check if PEP 669 is available (Python 3.12+)
_HAS_MONITORING = hasattr(sys, "monitoring")


def _coverage_py_start_callback(code, instruction_offset):
    """PEP 669 PY_START event callback.

    Called on function entry. Registers code_id -> filename mapping.
    This is the REGISTRATION PATH - called once per function execution.

    The Rust side uses a thread-local HashSet to ensure each code object
    is only registered once, so repeated calls for the same code object
    are O(1) no-ops.

    Args:
        code: The code object being entered
        instruction_offset: Always 0 for PY_START

    Returns:
        sys.monitoring.DISABLE to disable PY_START for this code object
        after first registration (optimization).
    """
    try:
        import tach_rust

        # Register code_id -> filename mapping
        # Rust handles deduplication via thread-local SEEN_CODES set
        code_id = id(code)
        filename = code.co_filename

        tach_rust.record_py_start(code_id, filename)

    except Exception as e:
        # Never let coverage errors crash the test
        _logger.debug("Coverage PY_START error (non-fatal): %s", e)

    # Disable PY_START for this code object after registration
    # This is an optimization - we only need to register once
    return sys.monitoring.DISABLE


def _coverage_line_callback(code, instruction_offset):
    """PEP 669 LINE event callback.

    Called for every line executed when coverage is enabled.
    This is the HOT PATH - must be as fast as possible.

    Args:
        code: The code object being executed
        instruction_offset: Bytecode offset within the code object

    Returns:
        None to continue monitoring, or sys.monitoring.DISABLE to stop
        monitoring this code object.
    """
    try:
        import tach_rust

        # Get code object ID (memory address) and line number
        # id(code) is the memory address of the code object
        code_id = id(code)

        # Map instruction offset to line number
        # This is fast because co_lines() is a generator that caches results
        lineno = code.co_firstlineno  # Default to first line
        for start, end, line in code.co_lines():
            if start <= instruction_offset < end:
                if line is not None:
                    lineno = line
                break

        # Record to ring buffer (releases GIL internally)
        tach_rust.record_line(code_id, lineno)

    except Exception as e:
        # Never let coverage errors crash the test
        _logger.debug("Coverage LINE error (non-fatal): %s", e)

    return None  # Continue monitoring


def enable_coverage():
    """Enable PEP 669 coverage collection.

    Must be called BEFORE tests run to capture coverage data.
    Safe to call multiple times (idempotent).

    Returns:
        True if coverage was enabled, False if not available
    """
    global _coverage_enabled, _coverage_tool_id

    if _coverage_enabled:
        return True

    if not _HAS_MONITORING:
        print(
            "[coverage] WARNING: sys.monitoring not available (requires Python 3.12+)",
            file=sys.stderr,
        )
        return False

    try:
        import tach_rust

        if not tach_rust.is_coverage_enabled():
            print(
                "[coverage] WARNING: Coverage ring buffer not initialized by Supervisor",
                file=sys.stderr,
            )
            return False

        # Use COVERAGE_ID (1) as our tool ID
        _coverage_tool_id = sys.monitoring.COVERAGE_ID

        # Register our tool
        sys.monitoring.use_tool_id(_coverage_tool_id, "tach_coverage")

        # Register PY_START callback (code_id -> filename registration)
        # This is called on function entry to register code objects
        sys.monitoring.register_callback(
            _coverage_tool_id,
            sys.monitoring.events.PY_START,
            _coverage_py_start_callback,
        )

        # Register LINE callback (coverage recording)
        sys.monitoring.register_callback(
            _coverage_tool_id, sys.monitoring.events.LINE, _coverage_line_callback
        )

        # Enable both PY_START and LINE events globally
        sys.monitoring.set_events(
            _coverage_tool_id,
            sys.monitoring.events.PY_START | sys.monitoring.events.LINE,
        )

        _coverage_enabled = True
        print("[coverage] PEP 669 coverage enabled", file=sys.stderr)
        return True

    except Exception as e:
        print(f"[coverage] Failed to enable coverage: {e}", file=sys.stderr)
        return False


def disable_coverage():
    """Disable PEP 669 coverage collection.

    Safe to call even if coverage was never enabled.
    """
    global _coverage_enabled, _coverage_tool_id

    if not _coverage_enabled or not _HAS_MONITORING:
        return

    try:
        # Disable events
        sys.monitoring.set_events(_coverage_tool_id, 0)

        # Unregister callbacks
        sys.monitoring.register_callback(
            _coverage_tool_id, sys.monitoring.events.PY_START, None
        )
        sys.monitoring.register_callback(
            _coverage_tool_id, sys.monitoring.events.LINE, None
        )

        # Free tool ID
        sys.monitoring.free_tool_id(_coverage_tool_id)

        _coverage_enabled = False
        _coverage_tool_id = None
        print("[coverage] PEP 669 coverage disabled", file=sys.stderr)

    except Exception as e:
        print(f"[coverage] Error disabling coverage: {e}", file=sys.stderr)


def get_coverage_stats() -> dict:
    """Get coverage collection statistics.

    Returns:
        Dict with 'enabled', 'coverage_overflow', 'mapping_overflow' keys
    """
    try:
        import tach_rust

        return {
            "enabled": _coverage_enabled,
            "coverage_overflow": tach_rust.get_coverage_overflow()
            if _coverage_enabled
            else 0,
            "mapping_overflow": tach_rust.get_mapping_overflow()
            if _coverage_enabled
            else 0,
        }
    except Exception as e:
        _logger.debug("Coverage stats error: %s", e)
        return {"enabled": False, "coverage_overflow": 0, "mapping_overflow": 0}
