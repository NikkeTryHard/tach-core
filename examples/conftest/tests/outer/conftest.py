"""Outer conftest.py in the nested hierarchy.

Fixtures here are available to:
- test_outer.py (if it existed)
- tests/outer/inner/ and all its contents
"""

import pytest


@pytest.fixture
def outer_fixture():
    """A fixture defined at the outer level.

    This fixture is available to:
    - Tests in this directory
    - Tests in inner/ subdirectory
    """
    return {"level": "outer", "value": "from_outer_conftest"}


@pytest.fixture
def outer_resource():
    """A resource specific to the outer level.

    The inner conftest can define its own version if needed.
    """
    return {
        "name": "outer_resource",
        "data": [1, 2, 3],
    }


@pytest.fixture
def combined_fixture(root_fixture, outer_fixture):
    """A fixture that combines fixtures from different levels.

    This demonstrates how fixtures can depend on fixtures from
    parent conftest.py files.
    """
    return {
        "root": root_fixture,
        "outer": outer_fixture,
        "combined": True,
    }
