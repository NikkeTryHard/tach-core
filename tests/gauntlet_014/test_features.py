"""
Gauntlet 0.1.4 - Feature Detection Tests

Tests for detecting available features based on Python version and system.

Features:
- sys.monitoring (PEP 669) - Python 3.12+
- Free-threaded mode - Python 3.13+ experimental
- Coverage backends (sys.settrace vs sys.monitoring)
- Async/await features
"""

import os
import sys


class TestCoverageBackend:
    """Tests for coverage backend detection."""

    def test_settrace_available(self):
        """sys.settrace should always be available."""
        assert hasattr(sys, "settrace")
        assert callable(sys.settrace)

    def test_gettrace_available(self):
        """sys.gettrace should always be available."""
        assert hasattr(sys, "gettrace")
        assert callable(sys.gettrace)

    def test_monitoring_available_when_expected(self):
        """sys.monitoring should be available in Python 3.12+."""
        version = sys.version_info
        if version.minor >= 12:
            assert hasattr(sys, "monitoring"), "Python 3.12+ should have sys.monitoring"
            monitoring = sys.monitoring
            assert hasattr(monitoring, "events")
            assert hasattr(monitoring, "register_callback")
        else:
            assert not hasattr(sys, "monitoring")

    def test_preferred_coverage_backend(self):
        """Determine preferred coverage backend for current Python."""
        version = sys.version_info
        if version.minor >= 12:
            preferred = "sys.monitoring"
            assert hasattr(sys, "monitoring")
        else:
            preferred = "sys.settrace"
            assert hasattr(sys, "settrace")
        assert preferred in ["sys.monitoring", "sys.settrace"]


class TestAsyncFeatures:
    """Tests for async/await feature availability."""

    def test_asyncio_available(self):
        """asyncio module should be available."""
        import asyncio

        assert hasattr(asyncio, "run")
        assert hasattr(asyncio, "create_task")

    def test_async_context_managers(self):
        """async context managers should work."""
        import asyncio

        class AsyncCM:
            async def __aenter__(self):
                return self

            async def __aexit__(self, *args):
                pass

        async def test():
            async with AsyncCM() as cm:
                assert cm is not None

        asyncio.run(test())

    def test_async_generators(self):
        """async generators should work."""
        import asyncio

        async def async_gen():
            for i in range(3):
                yield i

        async def test():
            results = []
            async for item in async_gen():
                results.append(item)
            assert results == [0, 1, 2]

        asyncio.run(test())


class TestTypeHintFeatures:
    """Tests for type hint feature availability."""

    def test_generic_types_3_9_plus(self):
        """Generic types (list[int]) should work in Python 3.9+."""
        version = sys.version_info
        if version.minor >= 9:
            # Can use built-in generic types
            hint = list[int]
            assert hint is not None

    def test_union_types_3_10_plus(self):
        """Union types (int | str) should work in Python 3.10+."""
        version = sys.version_info
        if version.minor >= 10:
            # Can use | for union types
            hint = int | str
            assert hint is not None

    def test_self_type_3_11_plus(self):
        """Self type should be available in Python 3.11+."""
        version = sys.version_info
        if version.minor >= 11:
            from typing import Self

            assert Self is not None


class TestSystemFeatures:
    """Tests for system-level feature detection."""

    def test_fork_available_on_unix(self):
        """os.fork should be available on Unix systems."""
        if sys.platform != "win32":
            assert hasattr(os, "fork")
            assert callable(os.fork)

    def test_shared_memory_available(self):
        """multiprocessing.shared_memory should be available."""
        from multiprocessing import shared_memory

        assert hasattr(shared_memory, "SharedMemory")

    def test_resource_limits_available(self):
        """resource module should be available on Unix."""
        if sys.platform != "win32":
            import resource

            assert hasattr(resource, "getrlimit")
            assert hasattr(resource, "setrlimit")

    def test_signal_handling_available(self):
        """signal module should be available."""
        import signal

        assert hasattr(signal, "signal")
        assert hasattr(signal, "SIGTERM")


class TestTachSpecificFeatures:
    """Tests for features specific to Tach requirements."""

    def test_ctypes_available(self):
        """ctypes should be available for FFI."""
        import ctypes

        assert hasattr(ctypes, "CDLL")
        assert hasattr(ctypes, "c_void_p")

    def test_mmap_available(self):
        """mmap should be available for memory mapping."""
        import mmap

        assert hasattr(mmap, "mmap")
        assert hasattr(mmap, "ACCESS_READ")

    def test_struct_available(self):
        """struct should be available for binary packing."""
        import struct

        assert hasattr(struct, "pack")
        assert hasattr(struct, "unpack")

    def test_tempfile_available(self):
        """tempfile should be available for test isolation."""
        import tempfile

        assert hasattr(tempfile, "TemporaryDirectory")
        assert hasattr(tempfile, "mkdtemp")
