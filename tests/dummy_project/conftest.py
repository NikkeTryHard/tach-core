"""Fixtures for testing."""

import pytest


@pytest.fixture
def my_fixture():
    return 1


@pytest.fixture(scope="module")
def db():
    return "database_connection"


@pytest.fixture(scope="session")
def cache():
    return {}


@pytest.fixture
def client(db):
    """Client depends on db fixture."""
    return f"client_using_{db}"
