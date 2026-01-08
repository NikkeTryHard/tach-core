"""
Task 0.1.2-A: pytest.raises() Compatibility Tests

Tests that verify pytest.raises() works correctly in tach.
These tests use the standard pytest.raises API and should pass
both when run through pytest directly and through tach.
"""

import pytest

# =============================================================================
# Basic Exception Capture
# =============================================================================


def test_a_raises_basic_exception():
    """pytest.raises catches expected exception."""
    with pytest.raises(ValueError):
        raise ValueError("test error")


def test_b_raises_zerodivision():
    """pytest.raises catches ZeroDivisionError."""
    with pytest.raises(ZeroDivisionError):
        _ = 1 / 0


def test_c_raises_keyerror():
    """pytest.raises catches KeyError."""
    with pytest.raises(KeyError):
        d = {}
        _ = d["missing_key"]


def test_d_raises_typeerror():
    """pytest.raises catches TypeError."""
    with pytest.raises(TypeError):
        len(42)


def test_e_raises_attributeerror():
    """pytest.raises catches AttributeError."""
    with pytest.raises(AttributeError):
        None.missing_attribute


# =============================================================================
# Exception Message Matching
# =============================================================================


def test_f_raises_with_match_pattern():
    """pytest.raises with match validates exception message."""
    with pytest.raises(ValueError, match="invalid"):
        raise ValueError("invalid literal for int()")


def test_g_raises_with_regex_match():
    """pytest.raises match supports regex patterns."""
    with pytest.raises(ValueError, match=r"expected \d+"):
        raise ValueError("expected 42 items")


def test_h_raises_with_case_insensitive_match():
    """pytest.raises match can use regex flags."""
    with pytest.raises(ValueError, match=r"(?i)ERROR"):
        raise ValueError("An error occurred")


def test_i_raises_match_partial():
    """pytest.raises match only needs to match part of the message."""
    with pytest.raises(ValueError, match="part"):
        raise ValueError("This is a partial match test")


# =============================================================================
# Exception Info Access
# =============================================================================


def test_j_raises_excinfo_type():
    """pytest.raises excinfo provides exception type."""
    with pytest.raises(ValueError) as excinfo:
        raise ValueError("test")
    assert excinfo.value is not None
    assert excinfo.type == ValueError


def test_k_raises_excinfo_value():
    """pytest.raises excinfo provides exception value."""
    with pytest.raises(ValueError) as excinfo:
        raise ValueError("specific message")
    assert str(excinfo.value) == "specific message"


def test_l_raises_excinfo_match_method():
    """pytest.raises excinfo.match() validates message."""
    with pytest.raises(ValueError) as excinfo:
        raise ValueError("error code 123")
    assert excinfo.match(r"code \d+")


# =============================================================================
# Exception Tuples
# =============================================================================


def test_m_raises_exception_tuple():
    """pytest.raises accepts tuple of exception types."""
    with pytest.raises((ValueError, TypeError)):
        raise ValueError("test")


def test_n_raises_exception_tuple_second_type():
    """pytest.raises with tuple catches second type."""
    with pytest.raises((ValueError, TypeError)):
        raise TypeError("test")


def test_o_raises_exception_tuple_with_match():
    """pytest.raises with tuple and match pattern."""
    with pytest.raises((ValueError, TypeError), match="error"):
        raise ValueError("error occurred")


# =============================================================================
# Exception Inheritance
# =============================================================================


class CustomError(Exception):
    """Custom exception for testing."""

    pass


class SpecificCustomError(CustomError):
    """Subclass of CustomError for inheritance testing."""

    pass


def test_p_raises_custom_exception():
    """pytest.raises catches custom exceptions."""
    with pytest.raises(CustomError):
        raise CustomError("custom error")


def test_q_raises_catches_subclass():
    """pytest.raises catches subclass of expected exception."""
    with pytest.raises(CustomError):
        raise SpecificCustomError("subclass error")


def test_r_raises_catches_exception_base():
    """pytest.raises(Exception) catches any exception."""
    with pytest.raises(Exception):
        raise ValueError("any error")


# =============================================================================
# Edge Cases
# =============================================================================


def test_s_raises_empty_message():
    """pytest.raises handles exceptions with empty messages."""
    with pytest.raises(ValueError):
        raise ValueError()


def test_t_raises_multiline_message():
    """pytest.raises handles exceptions with multiline messages."""
    with pytest.raises(ValueError, match="line1"):
        raise ValueError("line1\nline2\nline3")


def test_u_raises_unicode_message():
    """pytest.raises handles unicode exception messages."""
    with pytest.raises(ValueError, match="unicode"):
        raise ValueError("unicode test message")


# =============================================================================
# Nested Context Managers
# =============================================================================


def test_v_raises_nested():
    """Nested pytest.raises calls work correctly."""
    with pytest.raises(ValueError):
        with pytest.raises(TypeError):
            raise TypeError("inner")
        raise ValueError("outer")


def test_w_raises_in_loop():
    """pytest.raises works correctly in loops."""
    errors = ["error1", "error2", "error3"]
    for error in errors:
        with pytest.raises(ValueError, match=error):
            raise ValueError(error)


# =============================================================================
# Callable Form (if supported)
# =============================================================================


def raise_value_error():
    """Helper function that raises ValueError."""
    raise ValueError("from function")


def raise_with_args(msg):
    """Helper function that raises with given message."""
    raise ValueError(msg)


def test_x_raises_function_call():
    """pytest.raises works with function calls."""
    with pytest.raises(ValueError):
        raise_value_error()


def test_y_raises_via_expression():
    """pytest.raises works with inline expressions that raise."""
    with pytest.raises(ValueError):
        # A simple expression that raises ValueError
        int("not_a_number")


# =============================================================================
# Class Method Exceptions
# =============================================================================


class TestClassWithRaises:
    """Test class demonstrating pytest.raises in methods."""

    def test_z_method_raises(self):
        """pytest.raises works in test methods."""
        with pytest.raises(ValueError):
            raise ValueError("method test")

    def test_za_method_with_self_reference(self):
        """pytest.raises works with instance methods."""
        with pytest.raises(AttributeError):
            self.nonexistent_method()  # type: ignore
