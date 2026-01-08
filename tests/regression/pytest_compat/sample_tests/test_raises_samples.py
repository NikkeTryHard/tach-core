"""Sample tests for pytest.raises API compatibility.

These tests exercise various pytest.raises patterns and are designed
to be run through both real pytest and tach-core for comparison.

Test naming convention:
- test_raises_pass_* : Tests that should PASS
- test_raises_fail_* : Tests that should FAIL (expected exception not raised)
"""

import pytest

# =============================================================================
# Tests that should PASS (exception is correctly raised and caught)
# =============================================================================


def test_raises_pass_basic_valueerror():
    """Basic ValueError should be caught."""
    with pytest.raises(ValueError):
        raise ValueError("expected error")


def test_raises_pass_zerodivision():
    """ZeroDivisionError should be caught."""
    with pytest.raises(ZeroDivisionError):
        _ = 1 / 0


def test_raises_pass_keyerror():
    """KeyError should be caught."""
    with pytest.raises(KeyError):
        d = {}
        _ = d["missing"]


def test_raises_pass_with_match():
    """pytest.raises with match pattern should work."""
    with pytest.raises(ValueError, match="invalid"):
        raise ValueError("invalid input")


def test_raises_pass_regex_match():
    """pytest.raises match supports regex."""
    with pytest.raises(ValueError, match=r"code \d+"):
        raise ValueError("error code 123")


def test_raises_pass_exception_tuple():
    """pytest.raises accepts tuple of exception types."""
    with pytest.raises((ValueError, TypeError)):
        raise ValueError("one of these")


def test_raises_pass_excinfo_type():
    """excinfo.type should match raised exception."""
    with pytest.raises(ValueError) as excinfo:
        raise ValueError("test")
    assert excinfo.type == ValueError


def test_raises_pass_excinfo_value():
    """excinfo.value should contain exception message."""
    with pytest.raises(ValueError) as excinfo:
        raise ValueError("specific message")
    assert str(excinfo.value) == "specific message"


def test_raises_pass_subclass():
    """pytest.raises catches subclasses of expected exception."""

    class CustomError(ValueError):
        pass

    with pytest.raises(ValueError):
        raise CustomError("subclass")


def test_raises_pass_empty_message():
    """Exception with empty message should be caught."""
    with pytest.raises(ValueError):
        raise ValueError()


# =============================================================================
# Tests that should FAIL (exception not raised when expected)
# =============================================================================


def test_raises_fail_no_exception():
    """This should FAIL - no exception raised when expected."""
    with pytest.raises(ValueError):
        pass  # No exception raised!


def test_raises_fail_wrong_exception():
    """This should FAIL - wrong exception type raised."""
    with pytest.raises(TypeError):
        raise ValueError("wrong type")


def test_raises_fail_match_mismatch():
    """This should FAIL - message doesn't match pattern."""
    with pytest.raises(ValueError, match="expected"):
        raise ValueError("actual")
