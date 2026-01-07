# test_skip_xfail.py - Tests for pytest.fail, skip, xfail, importorskip compatibility
# Tests the assertion helpers in tach_harness.py

import sys
import os

# Add src to path for importing tach_harness
sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "..", "src"))

from tach_harness import (
    SkipException,
    XFailException,
    fail,
    skip,
    xfail,
    importorskip,
    _parse_version,
)


class TestFailFunction:
    """Test the fail() function."""

    def test_a_fail_raises_assertion_error(self):
        """Test that fail() raises AssertionError."""
        try:
            fail()
            assert False, "fail() should have raised AssertionError"
        except AssertionError as e:
            assert str(e) == "Test failed"

    def test_b_fail_with_reason(self):
        """Test fail() with a custom reason."""
        try:
            fail("Custom failure reason")
            assert False, "fail() should have raised AssertionError"
        except AssertionError as e:
            assert str(e) == "Custom failure reason"

    def test_c_fail_with_empty_reason(self):
        """Test fail() with empty string reason."""
        try:
            fail("")
            assert False, "fail() should have raised AssertionError"
        except AssertionError as e:
            # Empty reason defaults to "Test failed"
            assert str(e) == "Test failed"


class TestSkipFunction:
    """Test the skip() function."""

    def test_a_skip_raises_skip_exception(self):
        """Test that skip() raises SkipException."""
        try:
            skip()
            assert False, "skip() should have raised SkipException"
        except SkipException as e:
            assert e.reason == ""

    def test_b_skip_with_reason(self):
        """Test skip() with a reason."""
        try:
            skip("Need special hardware")
            assert False, "skip() should have raised SkipException"
        except SkipException as e:
            assert e.reason == "Need special hardware"

    def test_c_skip_exception_str(self):
        """Test SkipException string representation."""
        exc = SkipException("Test reason")
        assert str(exc) == "Test reason"


class TestXFailFunction:
    """Test the xfail() function."""

    def test_a_xfail_raises_xfail_exception(self):
        """Test that xfail() raises XFailException."""
        try:
            xfail()
            assert False, "xfail() should have raised XFailException"
        except XFailException as e:
            assert e.reason == ""

    def test_b_xfail_with_reason(self):
        """Test xfail() with a reason."""
        try:
            xfail("Bug #123 not fixed")
            assert False, "xfail() should have raised XFailException"
        except XFailException as e:
            assert e.reason == "Bug #123 not fixed"

    def test_c_xfail_exception_str(self):
        """Test XFailException string representation."""
        exc = XFailException("Known failure")
        assert str(exc) == "Known failure"


class TestParseVersion:
    """Test the _parse_version helper function."""

    def test_a_simple_version(self):
        """Test parsing simple version like 1.2.3."""
        result = _parse_version("1.2.3")
        assert result == (1, 2, 3)

    def test_b_two_part_version(self):
        """Test parsing two part version like 1.2."""
        result = _parse_version("1.2")
        assert result == (1, 2)

    def test_c_single_part_version(self):
        """Test parsing single part version like 1."""
        result = _parse_version("1")
        assert result == (1,)

    def test_d_version_with_alpha(self):
        """Test parsing version with alpha suffix like 1.2.3a1."""
        result = _parse_version("1.2.3a1")
        assert result[0] == 1
        assert result[1] == 2
        assert result[2] == 3
        assert "a1" in result

    def test_e_version_with_dev(self):
        """Test parsing version with dev suffix like 1.2.3.dev0."""
        result = _parse_version("1.2.3.dev0")
        assert result[0] == 1
        assert result[1] == 2
        assert result[2] == 3

    def test_f_version_comparison(self):
        """Test that parsed versions compare correctly."""
        assert _parse_version("1.2.3") < _parse_version("1.2.4")
        assert _parse_version("1.2.3") < _parse_version("1.3.0")
        assert _parse_version("1.2.3") < _parse_version("2.0.0")
        assert _parse_version("1.2.3") == _parse_version("1.2.3")
        assert _parse_version("1.10.0") > _parse_version("1.9.0")

    def test_g_version_with_whitespace(self):
        """Test that whitespace is stripped."""
        result = _parse_version("  1.2.3  ")
        assert result == (1, 2, 3)


class TestImportorskip:
    """Test the importorskip() function."""

    def test_a_import_existing_module(self):
        """Test importing an existing module."""
        # 'os' is always available
        mod = importorskip("os")
        assert mod is not None
        assert hasattr(mod, "path")

    def test_b_import_nonexistent_module_skips(self):
        """Test that importing nonexistent module raises SkipException."""
        try:
            importorskip("this_module_definitely_does_not_exist_12345")
            assert False, "Should have raised SkipException"
        except SkipException as e:
            assert "not available" in e.reason

    def test_c_import_with_version_check(self):
        """Test importing module with version check that passes."""
        # sys module always exists but doesn't have __version__
        # Let's use a module we know has __version__
        try:
            # Try to import a module with __version__
            # If pytest is installed, it has __version__
            mod = importorskip("pytest", minversion="1.0.0")
            assert mod is not None
        except SkipException:
            # pytest might not be installed in minimal environments
            pass

    def test_d_import_with_version_too_low(self):
        """Test that version too low raises SkipException."""
        # Create a mock scenario by trying to import os with an impossibly high version
        # os doesn't have __version__, so it defaults to "0.0.0"
        try:
            importorskip("os", minversion="999.0.0")
            assert False, "Should have raised SkipException"
        except SkipException as e:
            assert ">=" in e.reason

    def test_e_import_returns_module(self):
        """Test that importorskip returns the actual module."""
        mod = importorskip("json")
        import json

        assert mod is json

    def test_f_import_submodule(self):
        """Test importing a submodule."""
        mod = importorskip("os.path")
        import os.path

        assert mod is os.path


class TestExceptionClasses:
    """Test the exception classes directly."""

    def test_a_skip_exception_inheritance(self):
        """Test SkipException inherits from Exception."""
        exc = SkipException("reason")
        assert isinstance(exc, Exception)

    def test_b_xfail_exception_inheritance(self):
        """Test XFailException inherits from Exception."""
        exc = XFailException("reason")
        assert isinstance(exc, Exception)

    def test_c_skip_exception_reason_attribute(self):
        """Test SkipException has reason attribute."""
        exc = SkipException("my reason")
        assert exc.reason == "my reason"

    def test_d_xfail_exception_reason_attribute(self):
        """Test XFailException has reason attribute."""
        exc = XFailException("my reason")
        assert exc.reason == "my reason"

    def test_e_exception_default_reason(self):
        """Test exceptions with default empty reason."""
        skip_exc = SkipException()
        xfail_exc = XFailException()
        assert skip_exc.reason == ""
        assert xfail_exc.reason == ""


class TestIntegrationScenarios:
    """Test realistic usage scenarios."""

    def test_a_conditional_skip(self):
        """Test conditional skipping based on platform."""
        # Simulate a condition check
        condition = sys.platform == "nonexistent_platform"
        if condition:
            skip("This test only runs on real platforms")
        # If we get here, the test passes
        assert True

    def test_b_skip_on_missing_optional_dependency(self):
        """Test skipping when optional dependency is missing."""
        try:
            importorskip("nonexistent_optional_package")
            # This line should not be reached
            assert False
        except SkipException:
            # Expected - the module doesn't exist
            pass

    def test_c_xfail_on_known_bug(self):
        """Test xfail for known bugs."""
        bug_is_fixed = True  # Simulate bug fix check

        if not bug_is_fixed:
            xfail("Bug #999 is not yet fixed")

        # Bug is fixed, test should pass
        assert 1 + 1 == 2
