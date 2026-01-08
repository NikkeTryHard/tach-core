"""Sample tests for pytest.approx API compatibility.

These tests exercise various pytest.approx patterns and are designed
to be run through both real pytest and tach-core for comparison.

Test naming convention:
- test_approx_pass_* : Tests that should PASS
- test_approx_fail_* : Tests that should FAIL (values not approximately equal)
"""

import pytest

# =============================================================================
# Tests that should PASS (values are approximately equal)
# =============================================================================


def test_approx_pass_basic_float():
    """Basic floating point comparison."""
    # Classic floating point issue: 0.1 + 0.2 != 0.3 exactly
    assert 0.1 + 0.2 == pytest.approx(0.3)


def test_approx_pass_exact_value():
    """Exact values should match."""
    assert 1.0 == pytest.approx(1.0)
    assert 0.0 == pytest.approx(0.0)


def test_approx_pass_within_default_tolerance():
    """Values within default tolerance (1e-6 relative)."""
    assert 1.0 == pytest.approx(1.0000001)


def test_approx_pass_negative_floats():
    """Negative floating point values."""
    assert -0.1 - 0.2 == pytest.approx(-0.3)


def test_approx_pass_custom_rel_tolerance():
    """Custom relative tolerance."""
    assert 100 == pytest.approx(105, rel=0.1)  # 10% tolerance


def test_approx_pass_custom_abs_tolerance():
    """Custom absolute tolerance."""
    assert 1.0 == pytest.approx(1.005, abs=0.01)


def test_approx_pass_list_of_floats():
    """List of floating point values."""
    computed = [0.1 + 0.1, 0.2 + 0.2, 0.3 + 0.3]
    expected = [0.2, 0.4, 0.6]
    assert computed == pytest.approx(expected)


def test_approx_pass_tuple_of_floats():
    """Tuple of floating point values."""
    computed = (0.1 + 0.2, 0.2 + 0.3)
    expected = (0.3, 0.5)
    assert computed == pytest.approx(expected)


def test_approx_pass_integers():
    """Integers should work with approx."""
    assert 10 == pytest.approx(10)
    assert 10 == pytest.approx(10.0)


def test_approx_pass_zero_with_abs_tolerance():
    """Zero comparison with absolute tolerance."""
    assert 0.0 == pytest.approx(1e-13, abs=1e-12)


def test_approx_pass_large_values():
    """Large floating point values."""
    assert 1e10 == pytest.approx(1e10 + 1)


def test_approx_pass_scientific_notation():
    """Scientific notation values."""
    assert 1.23e-5 == pytest.approx(1.23e-5)
    assert 1.23e10 == pytest.approx(1.23e10)


def test_approx_pass_infinity():
    """Infinity should equal infinity."""
    assert float("inf") == pytest.approx(float("inf"))
    assert float("-inf") == pytest.approx(float("-inf"))


# =============================================================================
# Tests that should FAIL (values not approximately equal)
# =============================================================================


def test_approx_fail_outside_tolerance():
    """This should FAIL - 10% difference with default tolerance."""
    assert 1.0 == pytest.approx(1.1)


def test_approx_fail_wrong_sign():
    """This should FAIL - opposite signs."""
    assert 1.0 == pytest.approx(-1.0)


def test_approx_fail_list_mismatch():
    """This should FAIL - one element differs too much."""
    actual = [1.0, 2.0, 3.0]
    expected = [1.0, 2.0, 4.0]
    assert actual == pytest.approx(expected)


def test_approx_fail_list_length_mismatch():
    """This should FAIL - different list lengths."""
    assert [1.0, 2.0] == pytest.approx([1.0, 2.0, 3.0])


def test_approx_fail_inf_vs_finite():
    """This should FAIL - infinity vs finite value."""
    assert float("inf") == pytest.approx(1e308)


def test_approx_fail_nan_comparison():
    """This should FAIL - NaN never equals NaN."""
    assert float("nan") == pytest.approx(float("nan"))
