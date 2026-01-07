"""
Inner conftest for nested conftest inheritance testing.

This conftest.py is in a subdirectory and tests that use fixtures
from here should also be able to access fixtures from the parent conftest.py.
"""

import pytest


@pytest.fixture
def inner_fixture():
    """Fixture defined in inner conftest.py."""
    return "inner"


@pytest.fixture
def inherited_fixture(gauntlet_011_fixture):
    """Fixture that depends on parent conftest fixture."""
    return f"inherited from {gauntlet_011_fixture}"
