"""
Test file that should be blocked by .ignore unless --no-ignore is used.

This file exists to verify that:
1. Without --no-ignore, this test is NOT discovered
2. With --no-ignore, this test IS discovered and can run
"""


def test_blocked_by_ignore():
    """A simple test that should only run when --no-ignore is used."""
    assert True, "This test runs when --no-ignore bypasses .ignore"


def test_another_blocked():
    """Another test to verify multiple tests are discovered."""
    result = 1 + 1
    assert result == 2
