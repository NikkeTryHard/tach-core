"""Test file with a local fixture that overrides global."""

import pytest


@pytest.fixture
def db():
    """Local db fixture that shadows global."""
    return "local_db_connection"


def test_with_local_db(db):
    """Should use local db, not conftest.py db."""
    assert db == "local_db_connection"
