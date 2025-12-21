"""Test with circular fixture dependencies - should fail resolution."""

import pytest


@pytest.fixture
def fixture_a(fixture_b):
    return f"a_uses_{fixture_b}"


@pytest.fixture
def fixture_b(fixture_a):
    return f"b_uses_{fixture_a}"


def test_with_cycle(fixture_a):
    """This test should fail resolution due to cyclic dependency."""
    pass
