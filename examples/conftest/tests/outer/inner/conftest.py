"""Inner conftest.py in the nested hierarchy.

Fixtures here are available only to tests in this directory.
This conftest can:
- Define new fixtures
- Override fixtures from parent conftest.py files
- Use fixtures from parent conftest.py files
"""

import pytest


@pytest.fixture
def inner_fixture():
    """A fixture defined at the inner level.

    This fixture is only available to tests in this directory.
    """
    return {"level": "inner", "value": "from_inner_conftest"}


@pytest.fixture
def inner_resource():
    """A resource specific to the inner level."""
    return {
        "name": "inner_resource",
        "data": ["a", "b", "c"],
    }


@pytest.fixture
def full_hierarchy(root_fixture, outer_fixture, inner_fixture):
    """A fixture that demonstrates the full conftest hierarchy.

    This fixture uses fixtures from all three levels:
    - root_fixture from tests/conftest.py
    - outer_fixture from tests/outer/conftest.py
    - inner_fixture from tests/outer/inner/conftest.py
    """
    return {
        "root": root_fixture,
        "outer": outer_fixture,
        "inner": inner_fixture,
        "hierarchy_complete": True,
    }


# Example of overriding a parent fixture
@pytest.fixture
def shared_config():
    """Override the root shared_config fixture.

    This demonstrates that inner conftest.py files can override
    fixtures defined in parent conftest.py files.

    Tests in this directory will get this version instead of
    the one defined in the root conftest.py.
    """
    return {
        "environment": "inner_test",
        "debug": True,
        "nested_example": True,
        "overridden": True,  # New field showing this is the overridden version
    }
