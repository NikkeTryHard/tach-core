"""Shared fixtures for the simple example.

conftest.py files contain fixtures shared across test modules in the same
directory and subdirectories. Fixtures defined here are automatically
available to all test files.
"""

import pytest


@pytest.fixture
def shared_config():
    """A shared configuration fixture available to all tests.

    This fixture is defined in conftest.py, making it available to:
    - test_basic.py
    - test_fixtures.py
    - Any other test file in this directory
    """
    return {
        "environment": "test",
        "debug": True,
        "max_retries": 3,
    }


@pytest.fixture
def database_connection():
    """Simulates a database connection with setup/teardown.

    In real applications, this would connect to a test database.
    """
    # Setup - create mock connection
    connection = {
        "connected": True,
        "database": "test_db",
        "queries": [],
    }

    yield connection

    # Teardown - close connection
    connection["connected"] = False
    connection["queries"].clear()
