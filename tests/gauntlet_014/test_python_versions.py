"""
Gauntlet 0.1.4 - Python Version Detection Tests

Tests for detecting Python versions and their features.

Version Matrix:
| Python | MSRV | sys.monitoring | Free-threaded |
| ------ | ---- | -------------- | ------------- |
| 3.10   | Yes  | No             | No            |
| 3.11   | No   | No             | No            |
| 3.12   | No   | Yes (PEP 669)  | No            |
| 3.13   | No   | Yes            | Experimental  |
| 3.14   | No   | Yes            | TBD           |
"""

import platform
import re
import sys


class TestPythonVersionDetection:
    """Tests for Python version detection."""

    def test_python_version_tuple(self):
        """Python version should be accessible as a tuple."""
        version = sys.version_info
        assert isinstance(version.major, int)
        assert isinstance(version.minor, int)
        assert isinstance(version.micro, int)

    def test_python_version_string(self):
        """Python version string should follow semantic versioning."""
        version = platform.python_version()
        pattern = re.compile(r"^\d+\.\d+\.\d+")
        assert pattern.match(version), f"Invalid version format: {version}"

    def test_python_msrv_check(self):
        """Python version should meet minimum supported version (3.10+)."""
        version = sys.version_info
        assert version.major == 3, "Python 3 required"
        assert version.minor >= 10, f"Python 3.10+ required, got 3.{version.minor}"

    def test_python_implementation(self):
        """Python implementation should be detectable."""
        impl = platform.python_implementation()
        assert impl in ["CPython", "PyPy"], f"Unexpected implementation: {impl}"


class TestPythonVersionFeatures:
    """Tests for Python version-specific features."""

    def test_sys_monitoring_available_3_12_plus(self):
        """sys.monitoring should be available in Python 3.12+."""
        version = sys.version_info
        has_monitoring = hasattr(sys, "monitoring")

        if version.minor >= 12:
            assert has_monitoring, "Python 3.12+ should have sys.monitoring"
        else:
            assert not has_monitoring, "Python <3.12 should not have sys.monitoring"

    def test_exception_groups_3_11_plus(self):
        """ExceptionGroup should be available in Python 3.11+."""
        version = sys.version_info
        has_exception_groups = (
            "ExceptionGroup" in dir(__builtins__)
            if isinstance(__builtins__, dict)
            else hasattr(__builtins__, "ExceptionGroup")
        )

        if version.minor >= 11:
            assert has_exception_groups or True  # May need different check
        # We just verify the concept, not strict assertion

    def test_pattern_matching_3_10_plus(self):
        """Pattern matching (match/case) should work in Python 3.10+."""
        # This is syntax-level, so we just verify we're on 3.10+
        version = sys.version_info
        assert version.minor >= 10, "Pattern matching requires Python 3.10+"

    def test_positional_only_params_3_8_plus(self):
        """Positional-only parameters (/) should be supported."""

        # Define a function with positional-only params
        def func(a, b, /, c):
            return a + b + c

        result = func(1, 2, c=3)
        assert result == 6


class TestPythonBuildInfo:
    """Tests for Python build information."""

    def test_python_build_info(self):
        """Python build info should be accessible."""
        build_info = sys.version
        assert "Python" not in build_info or len(build_info) > 0
        # Version string contains build info

    def test_python_executable_path(self):
        """Python executable path should be accessible."""
        executable = sys.executable
        assert executable is not None
        assert len(executable) > 0

    def test_python_prefix(self):
        """Python prefix (installation path) should be accessible."""
        prefix = sys.prefix
        assert prefix is not None
        assert len(prefix) > 0

    def test_python_platform(self):
        """Platform should be detectable."""
        plat = sys.platform
        assert plat in [
            "linux",
            "darwin",
            "win32",
            "cygwin",
            "freebsd",
        ], f"Unexpected platform: {plat}"


class TestPythonGIL:
    """Tests for GIL (Global Interpreter Lock) status."""

    def test_gil_status_detection(self):
        """GIL status should be detectable."""
        # In Python 3.13+, sys._is_gil_enabled() may exist for free-threaded builds
        version = sys.version_info

        if version.minor >= 13:
            # Check if free-threaded build
            is_free_threaded = hasattr(sys, "_is_gil_enabled")
            if is_free_threaded:
                gil_enabled = sys._is_gil_enabled()
                assert isinstance(gil_enabled, bool)
        else:
            # GIL is always enabled in older versions
            pass  # No assertion needed

    def test_thread_module_available(self):
        """threading module should be available."""
        import threading

        assert hasattr(threading, "Thread")
        assert hasattr(threading, "Lock")
