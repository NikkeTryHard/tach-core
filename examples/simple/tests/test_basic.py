"""Basic test patterns for tach-core.

This module demonstrates fundamental assertion patterns and test structure.
These are the simplest possible tests to verify tach-core is working correctly.
"""


def test_pass():
    """A test that always passes."""
    assert True


def test_arithmetic():
    """Test basic arithmetic operations."""
    assert 1 + 1 == 2
    assert 10 - 3 == 7
    assert 4 * 5 == 20
    assert 15 // 3 == 5


def test_string_operations():
    """Test string manipulation."""
    text = "hello world"
    assert text.upper() == "HELLO WORLD"
    assert text.split() == ["hello", "world"]
    assert text.replace("world", "tach") == "hello tach"


def test_list_operations():
    """Test list manipulation."""
    items = [1, 2, 3]
    items.append(4)
    assert items == [1, 2, 3, 4]
    assert len(items) == 4
    assert sum(items) == 10


def test_dict_operations():
    """Test dictionary manipulation."""
    data = {"name": "tach", "version": "0.1.0"}
    assert data["name"] == "tach"
    assert "version" in data
    data["author"] = "test"
    assert len(data) == 3


def test_comparison():
    """Test comparison operators."""
    assert 5 > 3
    assert 3 < 5
    assert 5 >= 5
    assert 3 <= 3
    assert 5 != 3


def test_none_checks():
    """Test None handling."""
    value = None
    assert value is None

    value = "something"
    assert value is not None


def test_exception_handling():
    """Test that exceptions are raised correctly."""
    try:
        result = 1 / 0
        assert False, "Should have raised ZeroDivisionError"
    except ZeroDivisionError:
        pass  # Expected behavior

    # Alternative with pytest.raises would require pytest import
    # This example shows pure Python exception testing
