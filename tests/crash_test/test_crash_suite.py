"""Crash handling test suite."""

def test_simple_before():
    """First test - should pass."""
    assert True

def test_crash():
    """This will crash via abort()."""
    pass  # Mock worker handles this

def test_simple_after():
    """Third test - should pass after crash."""
    assert True
