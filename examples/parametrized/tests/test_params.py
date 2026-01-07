"""Parametrization patterns for tach-core.

This module demonstrates @pytest.mark.parametrize usage with various
data types and patterns.
"""

import pytest


# Simple parametrization with single parameter
@pytest.mark.parametrize("value", [1, 2, 3, 4, 5])
def test_single_param(value):
    """Test with single parameter values."""
    assert value > 0
    assert value <= 5


# Parametrization with multiple parameters
@pytest.mark.parametrize(
    "a,b,expected",
    [
        (1, 1, 2),
        (2, 3, 5),
        (10, 20, 30),
        (0, 0, 0),
        (-1, 1, 0),
    ],
)
def test_addition(a, b, expected):
    """Test addition with multiple parameter combinations."""
    assert a + b == expected


# Parametrization with different data types
@pytest.mark.parametrize(
    "input_val,expected_type",
    [
        (42, int),
        (3.14, float),
        ("hello", str),
        ([1, 2, 3], list),
        ({"key": "value"}, dict),
        ((1, 2), tuple),
        (True, bool),
    ],
)
def test_type_checking(input_val, expected_type):
    """Test various Python data types."""
    assert isinstance(input_val, expected_type)


# Parametrization with string operations
@pytest.mark.parametrize(
    "text,operation,expected",
    [
        ("hello", "upper", "HELLO"),
        ("HELLO", "lower", "hello"),
        ("hello world", "title", "Hello World"),
        ("  hello  ", "strip", "hello"),
        ("hello", "capitalize", "Hello"),
    ],
)
def test_string_operations(text, operation, expected):
    """Test string operations with parametrization."""
    method = getattr(text, operation)
    assert method() == expected


# Parametrization with list operations
@pytest.mark.parametrize(
    "items,expected_len,expected_sum",
    [
        ([1, 2, 3], 3, 6),
        ([10, 20, 30, 40], 4, 100),
        ([], 0, 0),
        ([5], 1, 5),
        ([-1, 0, 1], 3, 0),
    ],
)
def test_list_operations(items, expected_len, expected_sum):
    """Test list operations with various inputs."""
    assert len(items) == expected_len
    assert sum(items) == expected_sum


# Parametrization with IDs for better test output
@pytest.mark.parametrize(
    "n,expected",
    [
        pytest.param(0, 1, id="zero"),
        pytest.param(1, 1, id="one"),
        pytest.param(2, 2, id="two"),
        pytest.param(5, 120, id="five"),
        pytest.param(10, 3628800, id="ten"),
    ],
)
def test_factorial_with_ids(n, expected):
    """Test factorial with named parameter sets."""

    def factorial(n):
        if n <= 1:
            return 1
        return n * factorial(n - 1)

    assert factorial(n) == expected


# Parametrization with boolean conditions
@pytest.mark.parametrize(
    "value,is_positive,is_even",
    [
        (4, True, True),
        (3, True, False),
        (-2, False, True),
        (-3, False, False),
        (0, False, True),
    ],
)
def test_number_properties(value, is_positive, is_even):
    """Test number properties with boolean expectations."""
    assert (value > 0) == is_positive
    assert (value % 2 == 0) == is_even


# Nested parametrization (cartesian product)
@pytest.mark.parametrize("x", [1, 2, 3])
@pytest.mark.parametrize("y", [10, 20])
def test_cartesian_product(x, y):
    """Test all combinations of x and y (6 total tests)."""
    result = x * y
    assert result in [10, 20, 20, 40, 30, 60]
    assert result == x * y


# Parametrization with expected exceptions
@pytest.mark.parametrize(
    "a,b,raises_error",
    [
        (10, 2, False),
        (10, 0, True),
        (0, 5, False),
        (100, 0, True),
    ],
)
def test_division_with_errors(a, b, raises_error):
    """Test division with expected error handling."""
    if raises_error:
        with pytest.raises(ZeroDivisionError):
            _ = a / b
    else:
        result = a / b
        assert result == a / b


# Parametrization with dictionary inputs
@pytest.mark.parametrize(
    "config",
    [
        {"name": "test1", "enabled": True, "priority": 1},
        {"name": "test2", "enabled": False, "priority": 2},
        {"name": "test3", "enabled": True, "priority": 3},
    ],
)
def test_config_objects(config):
    """Test with dictionary configuration objects."""
    assert "name" in config
    assert "enabled" in config
    assert "priority" in config
    assert isinstance(config["name"], str)
    assert isinstance(config["enabled"], bool)
    assert isinstance(config["priority"], int)


# Parametrization with class instances
class Point:
    """A simple point class for testing."""

    def __init__(self, x: int, y: int):
        self.x = x
        self.y = y

    def distance_from_origin(self) -> float:
        return (self.x**2 + self.y**2) ** 0.5


@pytest.mark.parametrize(
    "point,expected_distance",
    [
        (Point(0, 0), 0.0),
        (Point(3, 4), 5.0),
        (Point(1, 1), 2**0.5),
        (Point(5, 12), 13.0),
    ],
)
def test_point_distance(point, expected_distance):
    """Test with class instance parameters."""
    assert abs(point.distance_from_origin() - expected_distance) < 0.0001
