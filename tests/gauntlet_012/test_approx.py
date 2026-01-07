# test_approx.py - Tests for pytest.approx compatibility
# Tests the approx class for floating point comparison in tach_harness.py

import sys
import os

# Add src to path for importing tach_harness
sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "..", "src"))

from tach_harness import approx


class TestApproxBasicFloats:
    """Test approx with basic floating point values."""

    def test_a_basic_float_equality(self):
        """Test basic float comparison with approx."""
        # Classic floating point issue: 0.1 + 0.2 != 0.3
        assert 0.1 + 0.2 == approx(0.3)

    def test_b_exact_float_equality(self):
        """Test exact float values."""
        assert 1.0 == approx(1.0)
        assert 0.0 == approx(0.0)
        assert -1.5 == approx(-1.5)

    def test_c_float_within_default_tolerance(self):
        """Test values within default relative tolerance (1e-6)."""
        # 1e-6 relative tolerance means 0.0001% difference is OK
        assert 1.0 == approx(1.0000001)
        assert 1000000.0 == approx(1000001.0)

    def test_d_float_outside_tolerance_fails(self):
        """Test that values outside tolerance fail comparison."""
        # 10% difference should fail with default tolerance
        result = 1.0 == approx(1.1)
        assert result is False

    def test_e_negative_floats(self):
        """Test approx with negative floating point values."""
        assert -0.1 - 0.2 == approx(-0.3)
        assert -1.0 == approx(-1.0000001)


class TestApproxCustomTolerance:
    """Test approx with custom relative and absolute tolerances."""

    def test_a_custom_relative_tolerance(self):
        """Test custom relative tolerance."""
        # 10% relative tolerance
        assert 100 == approx(105, rel=0.1)
        assert 100 == approx(95, rel=0.1)

        # 1% relative tolerance
        assert 100 == approx(101, rel=0.01)
        result = 100 == approx(102, rel=0.01)
        assert result is False

    def test_b_custom_absolute_tolerance(self):
        """Test custom absolute tolerance."""
        # 0.01 absolute tolerance
        assert 1.0 == approx(1.005, abs=0.01)
        assert 1.0 == approx(0.995, abs=0.01)

        # Should fail outside tolerance
        result = 1.0 == approx(1.02, abs=0.01)
        assert result is False

    def test_c_combined_tolerances(self):
        """Test both relative and absolute tolerance together."""
        # Uses the greater of the two tolerances
        # With rel=0.01 (1%) and abs=1.0, for expected=100:
        # rel tolerance = 0.01 * 100 = 1.0
        # abs tolerance = 1.0
        # max(1.0, 1.0) = 1.0
        assert 99.5 == approx(100, rel=0.01, abs=1.0)
        assert 100.5 == approx(100, rel=0.01, abs=1.0)

    def test_d_small_values_use_absolute(self):
        """Test that small values benefit from absolute tolerance."""
        # For very small expected values, relative tolerance becomes tiny
        # Absolute tolerance provides a floor
        assert 0.0 == approx(1e-13, abs=1e-12)
        assert 1e-10 == approx(1.1e-10, abs=1e-11)


class TestApproxLists:
    """Test approx with lists of floating point values."""

    def test_a_list_of_floats(self):
        """Test comparison of lists of floats."""
        computed = [0.1 + 0.1, 0.2 + 0.2, 0.3 + 0.3]
        expected = [0.2, 0.4, 0.6]
        assert computed == approx(expected)

    def test_b_list_with_custom_tolerance(self):
        """Test list comparison with custom tolerance."""
        actual = [100, 200, 300]
        expected = [105, 195, 310]
        assert actual == approx(expected, rel=0.1)

    def test_c_list_mismatch_fails(self):
        """Test that list with one bad element fails."""
        actual = [1.0, 2.0, 3.0]
        expected = [1.0, 2.0, 4.0]  # Last element differs too much
        result = actual == approx(expected)
        assert result is False

    def test_d_empty_lists(self):
        """Test empty list comparison."""
        assert [] == approx([])

    def test_e_single_element_list(self):
        """Test single element list."""
        assert [0.1 + 0.2] == approx([0.3])

    def test_f_list_length_mismatch(self):
        """Test that different length lists fail."""
        result = [1.0, 2.0] == approx([1.0, 2.0, 3.0])
        assert result is False


class TestApproxTuples:
    """Test approx with tuples of floating point values."""

    def test_a_tuple_of_floats(self):
        """Test comparison of tuples of floats."""
        computed = (0.1 + 0.1, 0.2 + 0.2)
        expected = (0.2, 0.4)
        assert computed == approx(expected)

    def test_b_tuple_with_custom_tolerance(self):
        """Test tuple comparison with custom tolerance."""
        actual = (100, 200)
        expected = (105, 195)
        assert actual == approx(expected, rel=0.1)

    def test_c_tuple_mismatch_fails(self):
        """Test that tuple with bad element fails."""
        actual = (1.0, 2.0)
        expected = (1.0, 3.0)
        result = actual == approx(expected)
        assert result is False


class TestApproxMixedTypes:
    """Test approx behavior with mixed types."""

    def test_a_list_vs_tuple(self):
        """Test that list can compare to tuple via approx."""
        # When expected is a list, actual can be tuple (both are sequences)
        assert (0.1 + 0.2, 0.2 + 0.3) == approx([0.3, 0.5])

    def test_b_tuple_vs_list(self):
        """Test that tuple can compare to list via approx."""
        assert [0.1 + 0.2, 0.2 + 0.3] == approx((0.3, 0.5))

    def test_c_non_sequence_vs_sequence_fails(self):
        """Test that comparing scalar to sequence fails."""
        result = 1.0 == approx([1.0])
        assert result is False


class TestApproxRepr:
    """Test the string representation of approx."""

    def test_a_repr_format(self):
        """Test that repr shows expected value and tolerance."""
        a = approx(1.5)
        repr_str = repr(a)
        assert "1.5" in repr_str
        assert "approx" in repr_str

    def test_b_repr_with_list(self):
        """Test repr with list expected."""
        a = approx([1.0, 2.0, 3.0])
        repr_str = repr(a)
        assert "approx" in repr_str

    def test_c_repr_with_custom_tolerance(self):
        """Test repr with custom relative tolerance."""
        a = approx(100, rel=0.1)
        repr_str = repr(a)
        assert "10" in repr_str  # 0.1 * 100 = 10%


class TestApproxEdgeCases:
    """Test edge cases for approx."""

    def test_a_integer_values(self):
        """Test that approx works with integers."""
        assert 10 == approx(10)
        assert 10 == approx(10.0)

    def test_b_zero_values(self):
        """Test comparison involving zero."""
        # Zero with zero
        assert 0.0 == approx(0.0)
        # Small value near zero (uses absolute tolerance)
        assert 0.0 == approx(1e-13, abs=1e-12)

    def test_c_large_values(self):
        """Test with large floating point values."""
        assert 1e10 == approx(1e10 + 1)
        assert 1e10 == approx(1e10 * 1.0000001)

    def test_d_very_small_tolerance(self):
        """Test with very tight tolerance."""
        # This should pass - exact same value
        assert 1.0 == approx(1.0, rel=1e-15, abs=1e-15)

    def test_e_scientific_notation(self):
        """Test with scientific notation."""
        assert 1.23e-5 == approx(1.23e-5)
        assert 1.23e10 == approx(1.23e10)

    def test_f_infinity_values(self):
        """Test approx with infinity values."""
        # Positive infinity should equal positive infinity
        assert float("inf") == approx(float("inf"))
        # Negative infinity should equal negative infinity
        assert float("-inf") == approx(float("-inf"))
        # Positive infinity should not equal negative infinity
        result = float("inf") == approx(float("-inf"))
        assert result is False
        # Negative infinity should not equal positive infinity
        result = float("-inf") == approx(float("inf"))
        assert result is False
        # Infinity should not equal finite values
        result = float("inf") == approx(1e308)
        assert result is False
        result = 1e308 == approx(float("inf"))
        assert result is False

    def test_g_nan_values(self):
        """Test approx with NaN values."""
        # NaN should never equal NaN (IEEE 754 semantics)
        result = float("nan") == approx(float("nan"))
        assert result is False
        # NaN should not equal any finite value
        result = float("nan") == approx(0.0)
        assert result is False
        result = 0.0 == approx(float("nan"))
        assert result is False
        result = float("nan") == approx(1.0)
        assert result is False
