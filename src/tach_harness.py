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
import warnings as warnings_module
import _pytest.runner
import _pytest.main
import _pytest.config
from contextlib import contextmanager
from typing import Any, Optional, Set, Type, Tuple, Union

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
EFFECT_TYPE_SQLALCHEMY_DB_SETUP = "SqlAlchemyDbSetup"


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
            fspath = getattr(item, 'fspath', None)
            return f"module:{fspath}" if fspath else f"module:unknown:{id(item)}"
        elif self._current_scope == "class":
            cls = getattr(item, "cls", None)
            if cls:
                return f"class:{cls.__module__}.{cls.__name__}"
            nodeid = getattr(item, 'nodeid', None)
            return f"function:{nodeid}" if nodeid else f"function:unknown:{id(item)}"
        else:  # function scope (default)
            nodeid = getattr(item, 'nodeid', None)
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
                        loop.run_until_complete(asyncio.gather(*pending, return_exceptions=True))
                except Exception as e:
                    # Log but don't fail - cleanup is best-effort (Issue #45)
                    print(f"[tach:harness] DEBUG: Loop cleanup error for {scope_key}: {e}", file=sys.stderr)
                finally:
                    loop.close()

    def close_all(self) -> None:
        """Close all managed event loops."""
        for scope_key in list(self._loops.keys()):
            self.close_scope(scope_key)

    def on_scope_transition(
        self,
        current_module: Optional[str],
        current_class: Optional[str]
    ) -> None:
        """Handle scope transitions and cleanup old scopes.

        Called before each test to detect and handle scope boundaries.
        Closes event loops when transitioning out of a class or module.
        """
        # Module transition: close previous module's loop if scope is module
        if self._current_scope == "module":
            if self._previous_module is not None and self._previous_module != current_module:
                old_key = f"module:{self._previous_module}"
                self.close_scope(old_key)

        # Class transition: close previous class's loop if scope is class
        if self._current_scope == "class":
            if self._previous_class is not None and self._previous_class != current_class:
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
            except RuntimeError:
                pass  # Loop may have been closed


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
        # Log but don't fail - cleanup is best-effort (Issue #45)
        print(f"[tach:harness] DEBUG: Task cleanup error: {e}", file=sys.stderr)

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
        except Exception:
            pass  # Ignore cleanup errors


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
    except Exception:
        pass
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
            print("[tach:harness] INFO: Threads terminated within grace period", file=sys.stderr)
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
        matching = [w for w in self._warnings if issubclass(w.category, self.expected_warning)]

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
            return all(approx(e, self.rel, self.abs) == a for e, a in zip(self.expected, actual))

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
            expected_abs = __builtins__["abs"](self.expected) if isinstance(__builtins__, dict) else abs(self.expected)
            diff = __builtins__["abs"](self.expected - actual) if isinstance(__builtins__, dict) else abs(self.expected - actual)
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
        print("[tach] WARNING: breakpoint() called but no debug socket.", file=sys.stderr)
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
            except Exception:
                pass
        if sock is not None:
            try:
                sock.close()
            except Exception:
                pass


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
        ssl_lib_path = ctypes.util.find_library('ssl')
        if ssl_lib_path:
            ssl_lib = ctypes.CDLL(ssl_lib_path)
            # Note: hasattr on CDLL may not work reliably; try/except is the real safeguard
            if hasattr(ssl_lib, 'RAND_add'):
                ssl_lib.RAND_add.argtypes = [ctypes.c_char_p, ctypes.c_int, ctypes.c_double]
                entropy_bytes = os.urandom(32)
                ssl_lib.RAND_add(entropy_bytes, 32, 32.0)
    except Exception:
        pass  # Best effort - OpenSSL may not be loaded

    # CRITICAL: Reset logging module locks after fork
    # The logging module uses RLocks that become corrupted after fork()
    # because the lock state is shared but the threads are not
    try:
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

    sys.meta_path[:] = [f for f in sys.meta_path if not isinstance(f, TachMetaPathFinder)]
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
    print(f"[tach:harness] Captured {len(_INITIAL_MODULES)} baseline modules", file=sys.stderr)

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


def _get_source_context(filename: str, lineno: int, context_lines: int = _CONTEXT_LINES) -> Optional[str]:
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
            is_pytest_plugin = any(
                ep.group == "pytest11" for ep in eps
            )
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
        os.write(2, f"[tach:plugins] Warning: {result['error']}\n".encode())
        return

    # Log unsupported plugins as warnings
    for plugin, reason in result.get("unsupported", {}).items():
        os.write(
            2,
            f"[tach:plugins] WARNING: Plugin '{plugin}' is not supported: {reason}\n".encode(),
        )

    # Log unknown plugins as info (they might work)
    unknown = result.get("unknown", [])
    if unknown:
        os.write(
            2,
            f"[tach:plugins] INFO: Unknown plugins detected (may or may not work): {', '.join(unknown)}\n".encode(),
        )

    # Log summary
    installed_count = len(result.get("installed", []))
    supported_count = len(result.get("supported", []))
    if installed_count > 0:
        os.write(
            2,
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
            effects.append({
                "type": EFFECT_TYPE_MODIFY_SYS_PATH,
                "action": action,
                "path": path,
            })

    # Find paths removed (in before but not in after)
    for path in before:
        if path not in after:
            effects.append({
                "type": EFFECT_TYPE_MODIFY_SYS_PATH,
                "action": "remove",
                "path": path,
            })

    return effects


def _compute_env_delta(before: dict, after: dict) -> list:
    """Compute environment variable changes between two snapshots.

    Returns list of SetEnv effect dicts compatible with HookEffect.
    """
    effects = []

    # Find added or changed variables
    for key, value in after.items():
        if key not in before:
            effects.append({
                "type": EFFECT_TYPE_SET_ENV,
                "key": key,
                "value": value,
            })
        elif before[key] != value:
            effects.append({
                "type": EFFECT_TYPE_SET_ENV,
                "key": key,
                "value": value,
            })

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
        if items_before_count == items_after_count and items_before_ids != items_after_ids:
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
            print(f"[tach:harness] Applied {applied}/{provided} cached hook effects", file=sys.stderr)
        else:
            # Warning: effects were provided but none were applied
            print(f"[tach:harness] WARNING: {provided} effects provided but 0 applied (possible mismatch)", file=sys.stderr)

    return applied


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

    Hook Effect Recording (v0.2.0):
    We capture env and sys.path before and after pytest configuration to record
    effects from session-level hooks (pytest_configure). These effects are cached
    and replayed in workers before test execution.

    Plugin Detection (v0.2.0):
    We detect installed pytest plugins and warn about unsupported ones.
    """
    global _SESSION, _ITEMS_MAP, _SESSION_HOOK_EFFECTS

    os.write(2, f"[tach:harness] init_session: {root_dir}\n".encode())

    # PLUGIN DETECTION (v0.2.0): Warn about unsupported plugins
    log_plugin_warnings()

    # HOOK EFFECT RECORDING: Capture state BEFORE pytest configuration
    env_before = dict(os.environ)
    sys_path_before = _capture_sys_path_snapshot()

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

    # HOOK EFFECT RECORDING: Capture state AFTER pytest_configure
    # At this point, pytest_configure hooks have run via cfg._do_configure()
    env_after = dict(os.environ)
    sys_path_after = _capture_sys_path_snapshot()

    # Compute deltas and store as session hook effects
    env_effects = _compute_env_delta(env_before, env_after)
    sys_path_effects = _compute_sys_path_delta(sys_path_before, sys_path_after)
    _SESSION_HOOK_EFFECTS = env_effects + sys_path_effects

    if _SESSION_HOOK_EFFECTS:
        os.write(
            2,
            f"[tach:harness] Recorded {len(_SESSION_HOOK_EFFECTS)} session hook effects "
            f"({len(env_effects)} env, {len(sys_path_effects)} sys.path)\n".encode(),
        )

    _SESSION = _pytest.main.Session.from_config(cfg)
    cfg.hook.pytest_sessionstart(session=_SESSION)

    _SESSION.perform_collect()

    for item in _SESSION.items:
        _ITEMS_MAP[item.nodeid] = item

    os.write(2, f"[tach:harness] Pre-collected {len(_ITEMS_MAP)} tests\n".encode())


def _parse_django_db_marker(marker_info: list[dict[str, Any]] | None) -> dict[str, Any] | None:
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
        print(f"[tach:harness] WARN: Database error closing connections: {e}", file=sys.stderr)
    except Exception:
        pass


def _dispose_all_connections() -> int:
    """Dispose all database connections before fork.

    This is more aggressive than close_all() - it ensures connection pools
    are completely reset so forked workers don't inherit stale FDs.

    Called by Zygote before forking workers to ensure:
    1. No file descriptors are shared between parent and child
    2. SSL connections are cleanly terminated (prevents MAC errors)
    3. Connection pools are empty so workers create fresh connections

    Returns:
        Number of connections disposed, or 0 if Django not available.
    """
    if not _is_django_available():
        return 0

    disposed = 0
    try:
        from django.db import connections

        for alias in connections:
            try:
                conn = connections[alias]
                # Close the connection if open
                if conn.connection is not None:
                    conn.close()
                    disposed += 1
                # Also close any pooled connections
                if hasattr(conn, 'close_if_unusable_or_obsolete'):
                    conn.close_if_unusable_or_obsolete()
            except Exception as e:
                print(f"[tach:harness] DEBUG: Error disposing connection '{alias}': {e}", file=sys.stderr)

        # Final close_all to ensure everything is cleaned up
        connections.close_all()

    except Exception as e:
        print(f"[tach:harness] WARN: Failed to dispose connections: {e}", file=sys.stderr)

    return disposed


# =============================================================================
# SQLALCHEMY ENGINE MANAGEMENT (fork-safe disposal)
# =============================================================================


def _detect_sqlalchemy() -> bool:
    """Detect if SQLAlchemy is installed and importable.

    Returns:
        True if SQLAlchemy is available, False otherwise.
    """
    try:
        import sqlalchemy
        return True
    except ImportError:
        return False


def _get_sqlalchemy_version() -> tuple[int, int, int] | None:
    """Get the installed SQLAlchemy version.

    Returns:
        Tuple of (major, minor, patch) or None if not installed.
    """
    try:
        from sqlalchemy import __version__
        parts = __version__.split('.')
        return (int(parts[0]), int(parts[1]), int(parts[2].split('+')[0].split('rc')[0]))
    except (ImportError, IndexError, ValueError):
        return None


def _handle_sqlalchemy_marker(marker_args: dict[str, Any] | None) -> dict[str, Any]:
    """Parse and validate SQLAlchemy marker arguments.

    Supports:
        @pytest.mark.sqlalchemy - Use all configured databases
        @pytest.mark.sqlalchemy(databases=['default']) - Specific databases

    Args:
        marker_args: Dict of marker keyword arguments.

    Returns:
        Normalized configuration dict with 'databases' key.
    """
    if marker_args is None:
        marker_args = {}

    config = {
        'databases': marker_args.get('databases', None),
        'use_savepoint': marker_args.get('use_savepoint', True),
    }

    return config


# SQLAlchemy engine registry for fork-safe disposal
_sqlalchemy_engines: list[Any] = []


def _register_sqlalchemy_engine(engine: Any) -> None:
    """Register a SQLAlchemy engine for fork-safe disposal."""
    global _sqlalchemy_engines
    if engine not in _sqlalchemy_engines:
        _sqlalchemy_engines.append(engine)


def _dispose_sqlalchemy_engines() -> list[str]:
    """Dispose all registered SQLAlchemy engines after fork.

    Calls engine.dispose(close=False) to clear pool without
    sending close to server.
    """
    global _sqlalchemy_engines
    disposed = []

    for engine in _sqlalchemy_engines:
        try:
            url = str(getattr(engine, 'url', 'unknown'))
            engine.dispose(close=False)
            disposed.append(url)
        except Exception as e:
            print(f"[tach:harness] WARN: Failed to dispose engine: {e}", file=sys.stderr)

    _sqlalchemy_engines.clear()
    return disposed


def _apply_sqlalchemy_isolation(
    engine: Any,
    session_factory: Any | None = None,
    *,
    use_savepoint: bool = True,
) -> dict[str, Any]:
    """Apply SQLAlchemy transaction isolation for a test."""
    try:
        from sqlalchemy import __version__ as sa_version
        sa_major = int(sa_version.split('.')[0])
    except ImportError:
        raise RuntimeError("SQLAlchemy is not installed")

    # Dispose engine to clear any inherited connections from fork
    engine.dispose(close=False)

    # Create a new connection and start the outer transaction
    connection = engine.connect()
    transaction = connection.begin()

    result = {
        'connection': connection,
        'transaction': transaction,
        'session': None,
        'engine': engine,
    }

    if session_factory is not None:
        if sa_major >= 2 and use_savepoint:
            session = session_factory(
                bind=connection,
                join_transaction_mode="create_savepoint",
            )
        else:
            session = session_factory(bind=connection)
            session.begin_nested()

        result['session'] = session

    return result


def _cleanup_sqlalchemy_isolation(isolation_context: dict[str, Any]) -> None:
    """Clean up SQLAlchemy transaction isolation after a test."""
    session = isolation_context.get('session')
    transaction = isolation_context.get('transaction')
    connection = isolation_context.get('connection')

    if session is not None:
        try:
            session.rollback()
            session.close()
        except Exception as e:
            print(f"[tach:harness] WARN: Error closing SQLAlchemy session: {e}", file=sys.stderr)

    if transaction is not None:
        try:
            transaction.rollback()
        except Exception as e:
            print(f"[tach:harness] WARN: Error rolling back SQLAlchemy transaction: {e}", file=sys.stderr)

    if connection is not None:
        try:
            connection.close()
        except Exception as e:
            print(f"[tach:harness] WARN: Error closing SQLAlchemy connection: {e}", file=sys.stderr)


async def _apply_sqlalchemy_isolation_async(
    engine: Any,
    session_factory: Any | None = None,
    *,
    use_savepoint: bool = True,
) -> dict[str, Any]:
    """Apply async SQLAlchemy transaction isolation for a test.

    Creates an async connection, starts a transaction, and optionally
    creates an AsyncSession bound to that connection.

    Args:
        engine: SQLAlchemy AsyncEngine instance.
        session_factory: Optional async_sessionmaker.
        use_savepoint: If True, use join_transaction_mode="create_savepoint".

    Returns:
        Dict with 'connection', 'transaction', and optionally 'session'.
    """
    try:
        from sqlalchemy import __version__ as sa_version
        sa_major = int(sa_version.split('.')[0])
    except ImportError:
        raise RuntimeError("SQLAlchemy is not installed")

    # Dispose engine to clear inherited connections
    await engine.dispose(close=False)

    # Create async connection and start transaction
    connection = await engine.connect()
    transaction = await connection.begin()

    result = {
        'connection': connection,
        'transaction': transaction,
        'session': None,
        'engine': engine,
    }

    if session_factory is not None:
        if sa_major >= 2 and use_savepoint:
            session = session_factory(
                bind=connection,
                join_transaction_mode="create_savepoint",
            )
        else:
            session = session_factory(bind=connection)
            await session.begin_nested()

        result['session'] = session

    return result


async def _cleanup_sqlalchemy_isolation_async(isolation_context: dict[str, Any]) -> None:
    """Clean up async SQLAlchemy transaction isolation after a test.

    Rolls back the outer transaction and closes the connection.

    Args:
        isolation_context: Dict returned by _apply_sqlalchemy_isolation_async.
    """
    session = isolation_context.get('session')
    transaction = isolation_context.get('transaction')
    connection = isolation_context.get('connection')

    if session is not None:
        try:
            await session.rollback()
            await session.close()
        except Exception as e:
            print(f"[tach:harness] WARN: Error closing async SQLAlchemy session: {e}", file=sys.stderr)

    if transaction is not None:
        try:
            await transaction.rollback()
        except Exception as e:
            print(f"[tach:harness] WARN: Error rolling back async SQLAlchemy transaction: {e}", file=sys.stderr)

    if connection is not None:
        try:
            await connection.close()
        except Exception as e:
            print(f"[tach:harness] WARN: Error closing async SQLAlchemy connection: {e}", file=sys.stderr)


def _apply_sqlalchemy_isolation_scoped(
    engine: Any,
    scoped_session_instance: Any,
) -> dict[str, Any]:
    """Apply SQLAlchemy isolation for scoped_session patterns.

    For applications using scoped_session (like Flask-SQLAlchemy),
    this function reconfigures the scoped session to use a new
    connection with transaction isolation.

    Args:
        engine: SQLAlchemy Engine instance.
        scoped_session_instance: The scoped_session registry.

    Returns:
        Isolation context dict for cleanup.
    """
    # Dispose and get fresh connection
    engine.dispose(close=False)
    connection = engine.connect()
    transaction = connection.begin()

    # Remove any existing session from the registry
    scoped_session_instance.remove()

    # Reconfigure to use our isolated connection
    scoped_session_instance.configure(bind=connection)

    # Get the actual session and start nested transaction
    session = scoped_session_instance()
    session.begin_nested()

    return {
        'connection': connection,
        'transaction': transaction,
        'session': session,
        'scoped_session': scoped_session_instance,
        'engine': engine,
    }


def _cleanup_sqlalchemy_isolation_scoped(isolation_context: dict[str, Any]) -> None:
    """Clean up scoped_session isolation.

    Args:
        isolation_context: Dict from _apply_sqlalchemy_isolation_scoped.
    """
    scoped_session = isolation_context.get('scoped_session')
    transaction = isolation_context.get('transaction')
    connection = isolation_context.get('connection')
    engine = isolation_context.get('engine')

    # Remove the scoped session
    if scoped_session is not None:
        try:
            scoped_session.remove()
        except Exception as e:
            print(f"[tach:harness] WARN: Error removing scoped session: {e}", file=sys.stderr)

    # Rollback transaction
    if transaction is not None:
        try:
            transaction.rollback()
        except Exception as e:
            print(f"[tach:harness] WARN: Error rolling back: {e}", file=sys.stderr)

    # Close connection
    if connection is not None:
        try:
            connection.close()
        except Exception as e:
            print(f"[tach:harness] WARN: Error closing connection: {e}", file=sys.stderr)

    # Reconfigure scoped session to use engine directly
    if scoped_session is not None and engine is not None:
        try:
            scoped_session.configure(bind=engine)
        except Exception as e:
            print(f"[tach:harness] WARN: Error reconfiguring scoped session: {e}", file=sys.stderr)


def _apply_sqlalchemy_isolation_multi(
    engines: dict[str, Any],
    session_factories: dict[str, Any] | None = None,
) -> dict[str, dict[str, Any]]:
    """Apply SQLAlchemy isolation for multiple engines.

    For applications with multiple databases (e.g., read replicas,
    sharded databases), this applies isolation to each engine.

    Args:
        engines: Dict mapping names to Engine instances.
        session_factories: Optional dict mapping names to session factories.

    Returns:
        Dict mapping names to isolation contexts.
    """
    contexts = {}

    if session_factories is None:
        session_factories = {}

    for name, engine in engines.items():
        factory = session_factories.get(name)
        contexts[name] = _apply_sqlalchemy_isolation(
            engine,
            factory,
            use_savepoint=True,
        )

    return contexts


def _cleanup_sqlalchemy_isolation_multi(contexts: dict[str, dict[str, Any]]) -> None:
    """Clean up isolation for multiple engines.

    Cleans up in reverse order to handle any cross-database dependencies.

    Args:
        contexts: Dict of isolation contexts from _apply_sqlalchemy_isolation_multi.
    """
    # Cleanup in reverse order
    for name in reversed(list(contexts.keys())):
        context = contexts[name]
        try:
            _cleanup_sqlalchemy_isolation(context)
        except Exception as e:
            print(f"[tach:harness] WARN: Error cleaning up engine '{name}': {e}", file=sys.stderr)


# =============================================================================
# ALEMBIC INTEGRATION (migration detection and verification)
# =============================================================================


def _detect_alembic() -> bool:
    """Detect if Alembic is installed.

    Returns:
        True if Alembic is available, False otherwise.
    """
    try:
        import alembic
        return True
    except ImportError:
        return False


def _get_alembic_config(config_path: str | None = None) -> Any | None:
    """Get Alembic configuration.

    Args:
        config_path: Path to alembic.ini. If None, searches common locations.

    Returns:
        Alembic Config object or None if not found.
    """
    if not _detect_alembic():
        return None

    from pathlib import Path

    # Search paths
    search_paths = []
    if config_path:
        search_paths.append(Path(config_path))
    else:
        cwd = Path.cwd()
        search_paths.extend([
            cwd / 'alembic.ini',
            cwd / 'migrations' / 'alembic.ini',
            cwd / 'src' / 'alembic.ini',
        ])

    for path in search_paths:
        if path.exists():
            try:
                from alembic.config import Config
                return Config(str(path))
            except Exception as e:
                print(f"[tach:harness] WARN: Failed to load alembic config from {path}: {e}", file=sys.stderr)

    return None


def _verify_alembic_head(engine: Any, config: Any = None) -> tuple[bool, str]:
    """Verify database is at Alembic head revision.

    Args:
        engine: SQLAlchemy Engine.
        config: Optional Alembic Config.

    Returns:
        Tuple of (is_at_head, current_revision).
    """
    if config is None:
        config = _get_alembic_config()

    if config is None:
        return True, "no_alembic"

    try:
        from alembic.runtime.migration import MigrationContext
        from alembic.script import ScriptDirectory

        with engine.connect() as conn:
            context = MigrationContext.configure(conn)
            current = context.get_current_revision()

        # Get head revision
        script = ScriptDirectory.from_config(config)
        head = script.get_current_head()

        return current == head, current or "none"
    except Exception as e:
        print(f"[tach:harness] WARN: Error checking Alembic head: {e}", file=sys.stderr)
        return True, "error"


def _get_database_aliases(requested: list[str] | None = None) -> list[str]:
    """Get valid database aliases for iteration.

    Filters requested aliases against actually configured databases.

    Args:
        requested: Specific aliases to use, or None for all configured.

    Returns:
        List of valid database aliases.
    """
    if not _is_django_available():
        return []

    try:
        from django.db import connections

        if requested is None:
            return list(connections)

        # Filter to only valid aliases
        valid = []
        for alias in requested:
            if alias in connections:
                valid.append(alias)
            else:
                print(f"[tach:harness] DEBUG: Skipping unknown database alias '{alias}'", file=sys.stderr)
        return valid

    except Exception:
        return []


def _check_test_db_exists() -> bool:
    """Check if the test database already exists.

    Used by --reuse-db to skip database creation when possible.
    Checks the actual test database name from Django's TEST settings.

    Returns:
        True if test database exists and is accessible, False otherwise.
    """
    if not _is_django_available():
        return False

    try:
        from django.db import connection

        # Get the test database name from settings
        test_db_name = connection.creation._get_test_db_name()

        # Try to connect to check if it exists
        # We use a temporary connection to avoid side effects
        from django.db import connections
        conn = connections['default']

        # Save original database name
        original_name = conn.settings_dict['NAME']

        try:
            # Temporarily point to test database
            conn.settings_dict['NAME'] = test_db_name
            conn.close()
            conn.ensure_connection()
            return True
        except Exception:
            return False
        finally:
            # Restore original database name
            conn.settings_dict['NAME'] = original_name
            conn.close()

    except Exception:
        return False


def _create_test_db(verbosity: int = 0) -> None:
    """Create the test database and run migrations.

    Uses Django's proper test database creation utilities to ensure
    the test database is created correctly with proper isolation.

    Args:
        verbosity: Output verbosity level (0=quiet, 1=normal, 2=verbose)
    """
    if not _is_django_available():
        return

    try:
        from django.db import connection
        from django.test.utils import setup_test_environment

        # Setup test environment first
        try:
            setup_test_environment()
        except Exception:
            pass  # May already be set up

        # Use Django's test database creation
        # keepdb=True to avoid destroying if it exists (--reuse-db behavior)
        connection.creation.create_test_db(verbosity=verbosity, keepdb=True)

    except Exception as e:
        print(f"[tach:harness] WARN: Failed to create test database: {e}", file=sys.stderr)


def _destroy_test_db(verbosity: int = 0) -> None:
    """Destroy the test database.

    Uses Django's proper test database destruction utilities.

    Args:
        verbosity: Output verbosity level (0=quiet, 1=normal, 2=verbose)
    """
    if not _is_django_available():
        return

    try:
        from django.db import connection, connections
        from django.test.utils import teardown_test_environment

        # Close all connections first
        connections.close_all()

        # Destroy the test database using Django's utilities
        # This actually drops the database
        connection.creation.destroy_test_db(verbosity=verbosity)

        # Teardown test environment
        try:
            teardown_test_environment()
        except Exception:
            pass  # May not be set up

    except Exception as e:
        print(f"[tach:harness] WARN: Failed to destroy test database: {e}", file=sys.stderr)


def _handle_db_lifecycle(reuse_db: bool = False, create_db: bool = False) -> None:
    """Handle database lifecycle based on CLI flags.

    Implements --reuse-db and --create-db behavior:
    - --reuse-db: Skip creation if database exists
    - --create-db: Force recreation even if --reuse-db is set

    Args:
        reuse_db: Whether to reuse existing database
        create_db: Whether to force database recreation
    """
    if not _is_django_available():
        return

    # --create-db takes precedence: destroy and recreate
    if create_db:
        print("[tach:harness] INFO: --create-db set, forcing database recreation", file=sys.stderr)
        _destroy_test_db(verbosity=1)
        _create_test_db(verbosity=1)
        return

    # --reuse-db: check if database exists
    if reuse_db:
        if _check_test_db_exists():
            print("[tach:harness] INFO: --reuse-db set, reusing existing database", file=sys.stderr)
            return
        else:
            print("[tach:harness] INFO: --reuse-db set but database doesn't exist, creating", file=sys.stderr)
            _create_test_db(verbosity=1)
            return

    # Default: always create fresh database
    _create_test_db(verbosity=0)


def _apply_django_db_isolation(marker_args: dict[str, Any] | None) -> list[tuple[str, str]]:
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
            print("[tach:harness] WARN: Django settings not configured, skipping DB isolation", file=sys.stderr)
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
        print(f"[tach:harness] WARN: Database error closing connections: {e}", file=sys.stderr)
    except Exception as e:
        print(f"[tach:harness] WARN: Failed to close Django connections: {e}", file=sys.stderr)

    # If no marker_args, apply default isolation to all databases
    if marker_args is None:
        marker_args = {"transaction": False, "reset_sequences": False, "databases": None}

    # If transaction=True, skip isolation (test manages its own transactions)
    if marker_args.get("transaction", False):
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
            print(f"[tach:harness] WARN: Unknown database alias '{alias}', skipping", file=sys.stderr)

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
            print(f"[tach:harness] WARN: Database error creating savepoint for '{alias}': {e}", file=sys.stderr)
            print(f"[tach:harness] INFO: Rolling back {len(savepoints)} previously created savepoints", file=sys.stderr)
            for prev_alias, prev_sid in reversed(savepoints):
                try:
                    transaction.savepoint_rollback(prev_sid, using=prev_alias)
                except DatabaseError as rollback_error:
                    print(f"[tach:harness] WARN: Database error rolling back savepoint for '{prev_alias}': {rollback_error}", file=sys.stderr)
                except Exception as rollback_error:
                    print(f"[tach:harness] WARN: Failed to rollback savepoint for '{prev_alias}': {rollback_error}", file=sys.stderr)
            return []  # Return empty - no isolation applied
        except Exception as e:
            # Unexpected error - still roll back and fail gracefully
            print(f"[tach:harness] WARN: Failed to create savepoint for '{alias}': {e}", file=sys.stderr)
            print(f"[tach:harness] INFO: Rolling back {len(savepoints)} previously created savepoints", file=sys.stderr)
            for prev_alias, prev_sid in reversed(savepoints):
                try:
                    transaction.savepoint_rollback(prev_sid, using=prev_alias)
                except Exception as rollback_error:
                    print(f"[tach:harness] WARN: Failed to rollback savepoint for '{prev_alias}': {rollback_error}", file=sys.stderr)
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
            print(f"[tach:harness] WARN: Database error rolling back savepoint for '{alias}': {e}", file=sys.stderr)
        except Exception as e:
            print(f"[tach:harness] WARN: Failed to rollback savepoint for '{alias}': {e}", file=sys.stderr)


def _flush_database(databases: list[str] | None = None) -> None:
    """Flush (truncate) database tables after a transaction=True test.

    This is the cleanup mechanism for tests that use real transactions.
    Unlike savepoint rollback, this physically deletes data from tables.

    Args:
        databases: List of database aliases to flush, or None for all.
    """
    if not _is_django_available():
        return

    try:
        from django.core.management import call_command
        from django.db import connections

        # Determine which databases to flush
        if databases is None:
            databases = list(connections)

        for alias in databases:
            if alias not in connections:
                print(f"[tach:harness] WARN: Unknown database '{alias}' for flush", file=sys.stderr)
                continue

            try:
                # Use Django's flush command which truncates all tables
                call_command('flush', '--no-input', database=alias, verbosity=0)
            except Exception as e:
                print(f"[tach:harness] WARN: Failed to flush database '{alias}': {e}", file=sys.stderr)

    except Exception as e:
        print(f"[tach:harness] WARN: Database flush failed: {e}", file=sys.stderr)


def _apply_django_db_isolation_v2(marker_args: dict[str, Any] | None) -> dict[str, Any]:
    """Apply database isolation based on marker args (v2 with transaction=True support).

    Enhanced version that returns structured result including flush indicator.

    Args:
        marker_args: Parsed django_db marker arguments.

    Returns:
        Dict with keys:
        - savepoints: List of (alias, sid) tuples for rollback
        - needs_flush: True if transaction=True (needs flush after test)
        - databases: List of database aliases involved
    """
    result = {
        "savepoints": [],
        "needs_flush": False,
        "databases": [],
    }

    if not _is_django_available():
        return result

    # If no marker_args, apply default isolation
    if marker_args is None:
        marker_args = {"transaction": False, "reset_sequences": False, "databases": None}

    # Determine databases
    from django.db import connections
    databases = marker_args.get("databases")
    if databases is None:
        databases = list(connections)
    result["databases"] = databases

    # If transaction=True, mark for flush instead of savepoint
    if marker_args.get("transaction", False):
        result["needs_flush"] = True
        return result

    # Otherwise, use savepoint isolation (existing logic)
    result["savepoints"] = _apply_django_db_isolation(marker_args)
    return result


def _cleanup_django_db_isolation_v2(isolation_result: dict[str, Any]) -> None:
    """Cleanup database isolation based on result from _apply_django_db_isolation_v2.

    Handles both savepoint rollback and flush cleanup.

    Args:
        isolation_result: Result dict from _apply_django_db_isolation_v2.
    """
    if isolation_result.get("needs_flush"):
        _flush_database(databases=isolation_result.get("databases"))
    else:
        _cleanup_django_db_isolation(isolation_result.get("savepoints", []))


# =============================================================================
# Django URL and Template Markers (v0.2.4 - Issue #35)
# =============================================================================


def _parse_urls_marker(marker_info: list[dict[str, Any]] | None) -> str | None:
    """Extract URL module path from @pytest.mark.urls marker.

    The urls marker accepts a single positional argument specifying the
    ROOT_URLCONF module to use for the test.

    Example:
        @pytest.mark.urls('myapp.test_urls')

    Args:
        marker_info: List of marker dictionaries from discovery.

    Returns:
        The URL module path string, or None if marker not present.
    """
    if not marker_info:
        return None

    for marker in marker_info:
        if isinstance(marker, dict) and marker.get("name") == "urls":
            args = marker.get("args", {})
            # Positional arg is stored with key "0"
            return args.get("0")

    return None


def _apply_urls_override(urlconf: str | None) -> str | None:
    """Override ROOT_URLCONF for a test and clear URL resolver cache.

    Args:
        urlconf: The URL module path to use, or None to skip override.

    Returns:
        The original ROOT_URLCONF value for restoration, or None if not applied.
    """
    if urlconf is None or not _is_django_available():
        return None

    try:
        from django.conf import settings
        from django.urls import clear_url_caches

        original = getattr(settings, "ROOT_URLCONF", None)
        settings.ROOT_URLCONF = urlconf
        clear_url_caches()
        return original
    except Exception as e:
        print(f"[tach:harness] WARN: Failed to apply urls override: {e}", file=sys.stderr)
        return None


def _cleanup_urls_override(original_urlconf: str | None) -> None:
    """Restore original ROOT_URLCONF after test.

    Args:
        original_urlconf: The original ROOT_URLCONF value to restore,
            or None if no override was applied.
    """
    if original_urlconf is None:
        return

    try:
        from django.conf import settings
        from django.urls import clear_url_caches

        settings.ROOT_URLCONF = original_urlconf
        clear_url_caches()
    except Exception as e:
        print(f"[tach:harness] WARN: Failed to restore urls: {e}", file=sys.stderr)


def _parse_ignore_template_errors_marker(marker_info: list[dict[str, Any]] | None) -> bool:
    """Check if @pytest.mark.ignore_template_errors marker is present.

    Args:
        marker_info: List of marker dictionaries from discovery.

    Returns:
        True if the marker is present, False otherwise.
    """
    if not marker_info:
        return False

    for marker in marker_info:
        if isinstance(marker, dict) and marker.get("name") == "ignore_template_errors":
            return True

    return False


def _apply_ignore_template_errors(ignore: bool) -> dict[str, Any] | None:
    """Disable template debug mode to suppress template errors.

    When ignore_template_errors is set, we disable DEBUG and template
    debugging to prevent TemplateSyntaxError from being raised.

    Args:
        ignore: Whether to ignore template errors.

    Returns:
        Dictionary of original settings for restoration, or None if not applied.
    """
    if not ignore or not _is_django_available():
        return None

    try:
        from django.conf import settings

        originals: dict[str, Any] = {}

        # Save and modify DEBUG
        originals["DEBUG"] = getattr(settings, "DEBUG", False)

        # Save and modify TEMPLATES debug settings
        if hasattr(settings, "TEMPLATES"):
            originals["TEMPLATES"] = []
            for i, template_config in enumerate(settings.TEMPLATES):
                if isinstance(template_config, dict):
                    orig_debug = template_config.get("OPTIONS", {}).get("debug")
                    originals["TEMPLATES"].append(orig_debug)
                    # Set debug to False to suppress template errors
                    if "OPTIONS" not in template_config:
                        template_config["OPTIONS"] = {}
                    template_config["OPTIONS"]["debug"] = False

        return originals
    except Exception as e:
        print(f"[tach:harness] WARN: Failed to apply ignore_template_errors: {e}", file=sys.stderr)
        return None


def _cleanup_ignore_template_errors(originals: dict[str, Any] | None) -> None:
    """Restore original template settings after test.

    Args:
        originals: Dictionary of original settings from _apply_ignore_template_errors,
            or None if no override was applied.
    """
    if originals is None:
        return

    try:
        from django.conf import settings

        # Restore TEMPLATES debug settings
        if "TEMPLATES" in originals and hasattr(settings, "TEMPLATES"):
            for i, orig_debug in enumerate(originals["TEMPLATES"]):
                if i < len(settings.TEMPLATES):
                    template_config = settings.TEMPLATES[i]
                    if isinstance(template_config, dict) and "OPTIONS" in template_config:
                        if orig_debug is None:
                            template_config["OPTIONS"].pop("debug", None)
                        else:
                            template_config["OPTIONS"]["debug"] = orig_debug
    except Exception as e:
        print(f"[tach:harness] WARN: Failed to restore template settings: {e}", file=sys.stderr)


# =============================================================================
# DJANGO FIXTURES (Issue #39)
# =============================================================================

_DJANGO_FIXTURES: dict[str, Any] = {}


def _init_db_fixture() -> None:
    """Initialize the db fixture.

    The db fixture provides database access for tests. It signals that
    the test requires database access and should have proper isolation.

    This is a marker fixture - it doesn't return a value but enables
    database operations within the test.
    """
    if not _is_django_available():
        return
    _DJANGO_FIXTURES["db"] = True


def _cleanup_db_fixture() -> None:
    """Cleanup the db fixture.

    Removes the db marker from the fixture registry.
    Actual database cleanup (rollback/flush) is handled by isolation functions.
    """
    _DJANGO_FIXTURES.pop("db", None)


def _init_client_fixture() -> Any:
    """Initialize the client fixture.

    The client fixture provides a Django test client for making HTTP requests
    to views without starting a real server.

    Returns:
        Django Test Client instance, or None if Django is not available.
    """
    if not _is_django_available():
        return None
    try:
        from django.test import Client

        client = Client()
        _DJANGO_FIXTURES["client"] = client
        return client
    except ImportError:
        return None


def _cleanup_client_fixture() -> None:
    """Cleanup the client fixture.

    Logs out any authenticated session and removes the client from registry.
    """
    client = _DJANGO_FIXTURES.pop("client", None)
    if client is not None:
        try:
            client.logout()
        except Exception:
            pass


def _init_rf_fixture() -> Any:
    """Initialize the rf (RequestFactory) fixture.

    The rf fixture provides a Django RequestFactory for creating mock
    request objects without going through the URL routing.

    Returns:
        Django RequestFactory instance, or None if Django is not available.
    """
    if not _is_django_available():
        return None
    try:
        from django.test import RequestFactory

        rf = RequestFactory()
        _DJANGO_FIXTURES["rf"] = rf
        return rf
    except ImportError:
        return None


def _cleanup_rf_fixture() -> None:
    """Cleanup the rf fixture.

    Removes the RequestFactory from the fixture registry.
    """
    _DJANGO_FIXTURES.pop("rf", None)


def _init_django_user_model_fixture() -> Any:
    """Initialize the django_user_model fixture.

    Returns the Django User model class (or custom user model).
    """
    if not _is_django_available():
        return None
    try:
        from django.contrib.auth import get_user_model

        User = get_user_model()
        _DJANGO_FIXTURES["django_user_model"] = User
        return User
    except ImportError:
        return None


def _cleanup_django_user_model_fixture() -> None:
    """Cleanup the django_user_model fixture."""
    _DJANGO_FIXTURES.pop("django_user_model", None)


def _init_django_username_field_fixture() -> Any:
    """Initialize the django_username_field fixture.

    Returns the name of the username field on the User model.
    """
    if not _is_django_available():
        return None
    try:
        from django.contrib.auth import get_user_model

        User = get_user_model()
        field_name = User.USERNAME_FIELD
        _DJANGO_FIXTURES["django_username_field"] = field_name
        return field_name
    except (ImportError, AttributeError):
        return "username"  # Default fallback


def _cleanup_django_username_field_fixture() -> None:
    """Cleanup the django_username_field fixture."""
    _DJANGO_FIXTURES.pop("django_username_field", None)


def _init_admin_user_fixture() -> Any:
    """Initialize the admin_user fixture.

    Creates and returns a superuser with known credentials.
    Username: admin, Password: password
    """
    if not _is_django_available():
        return None

    try:
        from django.contrib.auth import get_user_model

        User = get_user_model()

        username_field = User.USERNAME_FIELD

        # Create superuser with known credentials
        user_data = {
            username_field: "admin",
            "email": "admin@example.com",
            "password": "password",
        }

        # Check if user already exists
        try:
            admin_user = User.objects.get(**{username_field: "admin"})
        except User.DoesNotExist:
            admin_user = User.objects.create_superuser(**user_data)

        _DJANGO_FIXTURES["admin_user"] = admin_user
        return admin_user
    except ImportError:
        return None


def _cleanup_admin_user_fixture() -> None:
    """Cleanup the admin_user fixture."""
    _DJANGO_FIXTURES.pop("admin_user", None)


def _init_admin_client_fixture() -> Any:
    """Initialize the admin_client fixture.

    Returns a Django test client logged in as admin.
    """
    if not _is_django_available():
        return None

    try:
        from django.test import Client

        # Get or create admin user
        admin_user = _DJANGO_FIXTURES.get("admin_user")
        if admin_user is None:
            admin_user = _init_admin_user_fixture()

        # Create client and force login
        client = Client()
        client.force_login(admin_user)

        _DJANGO_FIXTURES["admin_client"] = client
        return client
    except ImportError:
        return None


def _cleanup_admin_client_fixture() -> None:
    """Cleanup the admin_client fixture."""
    client = _DJANGO_FIXTURES.pop("admin_client", None)
    if client is not None:
        try:
            client.logout()
        except Exception:
            pass


class SettingsWrapper:
    """Wrapper for Django settings that tracks and restores overrides.

    Uses Django's override_settings internally to ensure proper signal
    emission and cache invalidation.
    """

    def __init__(self):
        self._overrides: list[Any] = []
        self._original_values: dict[str, Any] = {}

    def __getattr__(self, name: str) -> Any:
        from django.conf import settings

        return getattr(settings, name)

    def __setattr__(self, name: str, value: Any) -> None:
        if name.startswith("_"):
            super().__setattr__(name, value)
            return

        from django.conf import settings
        from django.test.utils import override_settings

        # Store original if not already stored
        if name not in self._original_values:
            self._original_values[name] = getattr(settings, name, None)

        # Create and enable override
        override = override_settings(**{name: value})
        override.enable()
        self._overrides.append(override)

    def finalize(self) -> None:
        """Restore all overridden settings in reverse order."""
        for override in reversed(self._overrides):
            try:
                override.disable()
            except Exception:
                pass
        self._overrides.clear()
        self._original_values.clear()


def _init_settings_fixture() -> Any:
    """Initialize the settings fixture.

    Returns a SettingsWrapper for overriding Django settings.
    """
    if not _is_django_available():
        return None

    wrapper = SettingsWrapper()
    _DJANGO_FIXTURES["settings"] = wrapper
    return wrapper


def _cleanup_settings_fixture() -> None:
    """Cleanup the settings fixture."""
    wrapper = _DJANGO_FIXTURES.pop("settings", None)
    if wrapper is not None:
        wrapper.finalize()


def _init_transactional_db_fixture() -> Any:
    """Initialize the transactional_db fixture.

    Provides database access without savepoint wrapping.
    Uses flush for cleanup instead of rollback.
    """
    if not _is_django_available():
        return None

    # Mark that we need flush cleanup, not savepoint rollback
    _DJANGO_FIXTURES["transactional_db"] = True
    _DJANGO_FIXTURES["_needs_flush"] = True
    return True


def _cleanup_transactional_db_fixture() -> None:
    """Cleanup the transactional_db fixture."""
    needs_flush = _DJANGO_FIXTURES.pop("_needs_flush", False)
    _DJANGO_FIXTURES.pop("transactional_db", None)

    if needs_flush and _is_django_available():
        try:
            from django.core.management import call_command
            call_command("flush", "--no-input", verbosity=0)
        except Exception as e:
            print(f"[tach:harness] WARN: Failed to flush database: {e}", file=sys.stderr)


class LiveServer:
    """Wrapper for Django's live server test case functionality."""

    def __init__(self, host: str = "localhost", port: int = 0):
        self._host = host
        self._port = port
        self._server_thread = None
        self._url: Optional[str] = None

    def start(self) -> None:
        """Start the live server."""
        if not _is_django_available():
            return

        try:
            from django.test.testcases import LiveServerThread

            # Find an available port
            if self._port == 0:
                with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
                    s.bind(('', 0))
                    self._port = s.getsockname()[1]

            # Start server thread
            self._server_thread = LiveServerThread(self._host, [self._port])
            self._server_thread.daemon = True
            self._server_thread.start()
            self._server_thread.is_ready.wait()

            if self._server_thread.error:
                raise self._server_thread.error

            self._url = f"http://{self._host}:{self._port}"
        except ImportError as e:
            print(f"[tach:harness] WARN: LiveServerThread not available: {e}", file=sys.stderr)
            self._url = f"http://{self._host}:8000"  # Fallback

    def stop(self) -> None:
        """Stop the live server."""
        if self._server_thread is not None:
            try:
                self._server_thread.terminate()
                self._server_thread.join(timeout=5)
            except Exception:
                pass
            self._server_thread = None

    @property
    def url(self) -> str:
        """Return the live server URL."""
        return self._url or f"http://{self._host}:{self._port}"

    def __str__(self) -> str:
        return self.url


def _init_live_server_fixture() -> Any:
    """Initialize the live_server fixture."""
    if not _is_django_available():
        return None

    server = LiveServer()
    server.start()
    _DJANGO_FIXTURES["live_server"] = server
    return server


def _cleanup_live_server_fixture() -> None:
    """Cleanup the live_server fixture."""
    server = _DJANGO_FIXTURES.pop("live_server", None)
    if server is not None:
        server.stop()


# Fixture registry mapping names to init/cleanup functions
_FIXTURE_REGISTRY: dict[str, tuple[Any, Any]] = {
    "db": (_init_db_fixture, _cleanup_db_fixture),
    "client": (_init_client_fixture, _cleanup_client_fixture),
    "rf": (_init_rf_fixture, _cleanup_rf_fixture),
    "django_user_model": (_init_django_user_model_fixture, _cleanup_django_user_model_fixture),
    "django_username_field": (_init_django_username_field_fixture, _cleanup_django_username_field_fixture),
    "admin_user": (_init_admin_user_fixture, _cleanup_admin_user_fixture),
    "admin_client": (_init_admin_client_fixture, _cleanup_admin_client_fixture),
    "settings": (_init_settings_fixture, _cleanup_settings_fixture),
    "transactional_db": (_init_transactional_db_fixture, _cleanup_transactional_db_fixture),
    "live_server": (_init_live_server_fixture, _cleanup_live_server_fixture),
}


def _setup_django_fixtures(fixture_names: list[str]) -> dict[str, Any]:
    """Initialize Django fixtures requested by a test.

    Takes a list of fixture names the test requests, calls the appropriate
    init functions, and returns a dict of fixture name -> value.

    Args:
        fixture_names: List of fixture names (e.g., ["db", "client"])

    Returns:
        Dict mapping fixture name to its initialized value
    """
    fixture_values: dict[str, Any] = {}

    for name in fixture_names:
        if name in _FIXTURE_REGISTRY:
            init_fn, _ = _FIXTURE_REGISTRY[name]
            value = init_fn()
            fixture_values[name] = value

    return fixture_values


def _teardown_django_fixtures() -> None:
    """Cleanup all active Django fixtures.

    Calls all cleanup functions in reverse order of the fixture registry
    and clears the _DJANGO_FIXTURES registry.
    """
    # Get cleanup functions in reverse order for proper teardown
    cleanup_order = list(reversed(_FIXTURE_REGISTRY.keys()))

    for name in cleanup_order:
        if name in _DJANGO_FIXTURES:
            _, cleanup_fn = _FIXTURE_REGISTRY[name]
            try:
                cleanup_fn()
            except Exception:
                # Swallow cleanup errors to ensure all fixtures are cleaned
                pass

    # Ensure registry is cleared even if cleanup functions missed something
    _DJANGO_FIXTURES.clear()


def _get_fixture_names_from_item(item: Any) -> list[str]:
    """Extract Django fixture names from a pytest test item.

    Inspects the test function signature to find parameters that match
    known Django fixture names.

    Args:
        item: Pytest test item

    Returns:
        List of fixture names the test requires
    """
    fixture_names: list[str] = []

    try:
        # Get the actual test function
        func = item.obj
        if hasattr(func, "__wrapped__"):
            func = func.__wrapped__

        # Inspect function signature for fixture parameters
        import inspect
        sig = inspect.signature(func)
        for param_name in sig.parameters:
            if param_name in _FIXTURE_REGISTRY:
                fixture_names.append(param_name)
    except Exception:
        # If inspection fails, return empty list
        pass

    return fixture_names


def _construct_nodeid(file_path: str, test_name: str) -> str:
    """
    Construct a pytest nodeid relative to the session's rootdir.

    This ensures the nodeid matches _ITEMS_MAP keys regardless of:
    - Where tach was invoked from (parent dir, project root, etc.)
    - Nested pyproject.toml/pytest.ini files affecting rootdir

    Args:
        file_path: Absolute or relative path to the test file
        test_name: Test identifier (e.g., "TestClass::test_method" or "test_func")

    Returns:
        Nodeid string matching pytest's format (e.g., "tests/foo.py::test_bar")
    """
    global _SESSION

    if _SESSION is None:
        # Fallback if session not initialized
        return f"{file_path}::{test_name}"

    # Get pytest's rootdir from the session config
    rootdir = _SESSION.config.rootdir

    # Convert file_path to absolute, then make relative to rootdir
    abs_path = os.path.abspath(file_path)
    try:
        rel_path = os.path.relpath(abs_path, start=str(rootdir))
    except ValueError:
        # On Windows, relpath fails if paths are on different drives
        rel_path = file_path

    # Normalize path separators to forward slashes (pytest convention)
    rel_path = rel_path.replace(os.sep, "/")

    # Debug logging if enabled
    if os.environ.get("TACH_DEBUG_NODEID"):
        os.write(2, f"[tach:harness] nodeid: {file_path} + {test_name} -> {rel_path}::{test_name}\n".encode())

    return f"{rel_path}::{test_name}"


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
        # Construct nodeid relative to pytest's rootdir for reliable lookup
        # The node_id parameter contains test_name (e.g., "TestClass::test_method")
        # We combine it with file_path to build the correct nodeid
        constructed_nodeid = _construct_nodeid(file_path, node_id)

        # O(1) lookup from pre-collected items
        target_item = _ITEMS_MAP.get(constructed_nodeid)

        # Fallback: try the original node_id in case it's already correct
        if not target_item:
            target_item = _ITEMS_MAP.get(node_id)

        if not target_item:
            duration = time.perf_counter() - start
            # Include both attempted nodeids in error for debugging
            return (
                STATUS_HARNESS_ERROR,
                duration,
                f"Test not found in Zygote session.\n"
                f"  Constructed: {constructed_nodeid}\n"
                f"  Original: {node_id}\n"
                f"  Available: {len(_ITEMS_MAP)} items\n"
                f"  Sample keys: {list(_ITEMS_MAP.keys())[:3]}",
                False,  # No thread leak detection for failed lookup
            )

        # Check for @pytest.mark.allow_threads marker
        allow_threads = _has_allow_threads_marker(target_item)

        # Native Async Support
        original_obj = target_item.obj
        func_to_check = original_obj
        if hasattr(original_obj, "__func__"):
            func_to_check = original_obj.__func__

        if inspect.iscoroutinefunction(func_to_check):
            # Use EventLoopManager for scoped loop management
            loop_manager = EventLoopManager.get_instance()

            # Parse asyncio marker for loop_scope configuration
            loop_scope, _has_asyncio_marker = parse_asyncio_marker(target_item)
            loop_manager.configure(loop_scope=loop_scope)

            # Scope transition handling (Issue #43)
            fspath = str(getattr(target_item, 'fspath', ''))
            item_cls = getattr(target_item, 'cls', None)
            class_name = f"{item_cls.__module__}.{item_cls.__name__}" if item_cls else None
            loop_manager.on_scope_transition(fspath, class_name)

            scope_key = loop_manager.get_scope_key(target_item)

            def make_sync_wrapper(async_fn, mgr, key):
                def sync_wrapper(*args, **kwargs):
                    loop = mgr.get_loop(key)
                    # Set as current event loop for this thread
                    asyncio.set_event_loop(loop)
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

        # Django Fixtures (v0.3.0 - Issue #39)
        # Setup fixtures after db isolation is applied
        fixture_names = _get_fixture_names_from_item(target_item)
        fixture_values = _setup_django_fixtures(fixture_names)

        # Django URL and Template Markers (v0.2.4 - Issue #35)
        urlconf = _parse_urls_marker(marker_info)
        original_urlconf = _apply_urls_override(urlconf)

        ignore_templates = _parse_ignore_template_errors_marker(marker_info)
        template_originals = _apply_ignore_template_errors(ignore_templates)

        try:
            reports = _pytest.runner.runtestprotocol(target_item, nextitem=None, log=False)
        finally:
            # Cleanup in reverse order of application
            _cleanup_ignore_template_errors(template_originals)
            _cleanup_urls_override(original_urlconf)
            # Teardown Django fixtures before db isolation cleanup
            _teardown_django_fixtures()
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
                print(f"[tach:harness] DEBUG: Enhanced failure formatting failed: {enhance_err}", file=sys.stderr)

            return (STATUS_FAIL, duration, msg, _thread_leak_detected)

        if skipped_report:
            skip_reason = str(skipped_report.longrepr) if skipped_report.longrepr else ""
            return (STATUS_SKIP, duration, f"Skipped: {skip_reason}", _thread_leak_detected)

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
            print(f"[tach:harness] WARN: Failed to remove {mod_name}: {e}", file=sys.stderr)

    if removed_count > 0:
        print(f"[tach:harness] Cleaned up {removed_count} test modules", file=sys.stderr)

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


def worker_loop_iteration(file_path: str, node_id: str, is_toxic: bool, cached_effects: list = None, marker_info: list = None) -> tuple:
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
    status, duration, message, thread_leaked = run_test(file_path, node_id, cached_effects, marker_info)

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

    except Exception:
        # Never let coverage errors crash the test
        pass

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

    except Exception:
        # Never let coverage errors crash the test
        pass

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
        sys.monitoring.register_callback(_coverage_tool_id, sys.monitoring.events.LINE, _coverage_line_callback)

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
        sys.monitoring.register_callback(_coverage_tool_id, sys.monitoring.events.PY_START, None)
        sys.monitoring.register_callback(_coverage_tool_id, sys.monitoring.events.LINE, None)

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
            "coverage_overflow": tach_rust.get_coverage_overflow() if _coverage_enabled else 0,
            "mapping_overflow": tach_rust.get_mapping_overflow() if _coverage_enabled else 0,
        }
    except Exception:
        return {"enabled": False, "coverage_overflow": 0, "mapping_overflow": 0}
