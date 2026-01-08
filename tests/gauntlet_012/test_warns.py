"""
Task 0.1.2-A: pytest.warns() Compatibility Tests

Tests that verify pytest.warns() works correctly in tach.
These tests use the standard pytest.warns API and should pass
both when run through pytest directly and through tach.
"""

import warnings

import pytest

# =============================================================================
# Basic Warning Capture
# =============================================================================


def test_a_warns_deprecation():
    """pytest.warns catches DeprecationWarning."""
    with pytest.warns(DeprecationWarning):
        warnings.warn("deprecated function", DeprecationWarning)


def test_b_warns_user_warning():
    """pytest.warns catches UserWarning."""
    with pytest.warns(UserWarning):
        warnings.warn("user warning message", UserWarning)


def test_c_warns_future_warning():
    """pytest.warns catches FutureWarning."""
    with pytest.warns(FutureWarning):
        warnings.warn("will change in future", FutureWarning)


def test_d_warns_runtime_warning():
    """pytest.warns catches RuntimeWarning."""
    with pytest.warns(RuntimeWarning):
        warnings.warn("runtime issue", RuntimeWarning)


def test_e_warns_pending_deprecation():
    """pytest.warns catches PendingDeprecationWarning."""
    with pytest.warns(PendingDeprecationWarning):
        warnings.warn("will be deprecated", PendingDeprecationWarning)


# =============================================================================
# Warning Message Matching
# =============================================================================


def test_f_warns_with_match_pattern():
    """pytest.warns with match validates warning message."""
    with pytest.warns(UserWarning, match="specific"):
        warnings.warn("specific warning message", UserWarning)


def test_g_warns_with_regex_match():
    """pytest.warns match supports regex patterns."""
    with pytest.warns(UserWarning, match=r"version \d+\.\d+"):
        warnings.warn("deprecated in version 1.5", UserWarning)


def test_h_warns_match_partial():
    """pytest.warns match only needs to match part of the message."""
    with pytest.warns(UserWarning, match="partial"):
        warnings.warn("This is a partial match warning", UserWarning)


def test_i_warns_match_case_sensitive():
    """pytest.warns match is case sensitive by default."""
    with pytest.warns(UserWarning, match="Warning"):
        warnings.warn("Warning: something happened", UserWarning)


# =============================================================================
# Warning Inheritance
# =============================================================================


class CustomWarning(UserWarning):
    """Custom warning for testing."""

    pass


class SpecificCustomWarning(CustomWarning):
    """Subclass of CustomWarning for inheritance testing."""

    pass


def test_j_warns_custom_warning():
    """pytest.warns catches custom warnings."""
    with pytest.warns(CustomWarning):
        warnings.warn("custom warning", CustomWarning)


def test_k_warns_catches_subclass():
    """pytest.warns catches subclass of expected warning."""
    with pytest.warns(CustomWarning):
        warnings.warn("subclass warning", SpecificCustomWarning)


def test_l_warns_catches_warning_base():
    """pytest.warns(Warning) catches any warning."""
    with pytest.warns(Warning):
        warnings.warn("any warning", UserWarning)


# =============================================================================
# Multiple Warnings
# =============================================================================


def test_m_warns_multiple_same_type():
    """pytest.warns handles multiple warnings of same type."""
    with pytest.warns(UserWarning):
        warnings.warn("first warning", UserWarning)
        warnings.warn("second warning", UserWarning)


def test_n_warns_multiple_different_types():
    """pytest.warns captures expected type among multiple."""
    with pytest.warns(DeprecationWarning):
        warnings.warn("user warning", UserWarning)
        warnings.warn("deprecation warning", DeprecationWarning)


def test_o_warns_match_among_multiple():
    """pytest.warns match finds pattern among multiple warnings."""
    with pytest.warns(UserWarning, match="target"):
        warnings.warn("other warning", UserWarning)
        warnings.warn("target warning", UserWarning)


# =============================================================================
# Edge Cases
# =============================================================================


def test_p_warns_empty_message():
    """pytest.warns handles warnings with empty messages."""
    with pytest.warns(UserWarning):
        warnings.warn("", UserWarning)


def test_q_warns_multiline_message():
    """pytest.warns handles warnings with multiline messages."""
    with pytest.warns(UserWarning, match="line1"):
        warnings.warn("line1\nline2\nline3", UserWarning)


def test_r_warns_unicode_message():
    """pytest.warns handles unicode warning messages."""
    with pytest.warns(UserWarning, match="unicode"):
        warnings.warn("unicode warning message", UserWarning)


# =============================================================================
# Nested Context Managers
# =============================================================================


def test_s_warns_nested():
    """Nested pytest.warns calls work correctly."""
    with pytest.warns(UserWarning):
        with pytest.warns(DeprecationWarning):
            warnings.warn("inner", DeprecationWarning)
        warnings.warn("outer", UserWarning)


def test_t_warns_in_loop():
    """pytest.warns works correctly in loops."""
    messages = ["warning1", "warning2", "warning3"]
    for msg in messages:
        with pytest.warns(UserWarning, match=msg):
            warnings.warn(msg, UserWarning)


# =============================================================================
# Combined with pytest.raises
# =============================================================================


def test_u_warns_then_raises():
    """pytest.warns followed by pytest.raises works correctly."""
    with pytest.warns(UserWarning):
        warnings.warn("warning before error", UserWarning)

    with pytest.raises(ValueError):
        raise ValueError("error after warning")


def test_v_warns_with_code_that_succeeds():
    """pytest.warns works when code completes successfully."""
    with pytest.warns(UserWarning):
        warnings.warn("warning in successful code", UserWarning)
        result = 1 + 1
    assert result == 2


# =============================================================================
# Class Method Warnings
# =============================================================================


class TestClassWithWarns:
    """Test class demonstrating pytest.warns in methods."""

    def test_w_method_warns(self):
        """pytest.warns works in test methods."""
        with pytest.warns(UserWarning):
            warnings.warn("method warning", UserWarning)

    def test_x_method_warns_with_match(self):
        """pytest.warns with match works in test methods."""
        with pytest.warns(DeprecationWarning, match="deprecated"):
            warnings.warn("deprecated method", DeprecationWarning)


# =============================================================================
# Function That Warns
# =============================================================================


def deprecated_function():
    """Function that issues a deprecation warning."""
    warnings.warn("deprecated_function is deprecated", DeprecationWarning)
    return 42


def test_y_warns_function_call():
    """pytest.warns captures warnings from function calls."""
    with pytest.warns(DeprecationWarning, match="deprecated_function"):
        result = deprecated_function()
    assert result == 42


def warn_with_message(msg):
    """Helper function that warns with given message."""
    warnings.warn(msg, UserWarning)


def test_z_warns_with_parameterized_message():
    """pytest.warns works with parameterized warning messages."""
    with pytest.warns(UserWarning, match="test123"):
        warn_with_message("test123 warning")
