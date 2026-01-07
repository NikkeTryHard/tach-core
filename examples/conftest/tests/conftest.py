"""Root conftest.py for the conftest example.

This is the outermost conftest.py file. Fixtures defined here are
available to ALL test files in this directory and all subdirectories.
"""

import pytest


@pytest.fixture
def root_fixture():
    """A fixture defined at the root level.

    This fixture is available to:
    - Tests in tests/outer/
    - Tests in tests/outer/inner/
    - Any other subdirectory
    """
    return {"level": "root", "value": "from_root_conftest"}


@pytest.fixture
def shared_config():
    """Configuration shared across all test levels.

    Inner conftest.py files can override this fixture if needed.
    """
    return {
        "environment": "test",
        "debug": True,
        "nested_example": True,
    }


@pytest.fixture
def connection_pool():
    """Simulates a connection pool shared by all tests.

    This demonstrates a resource that should be shared across
    the entire test hierarchy.
    """
    pool = {
        "size": 10,
        "active": 0,
        "connections": [],
    }

    yield pool

    # Cleanup
    pool["connections"].clear()
