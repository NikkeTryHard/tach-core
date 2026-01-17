"""Shared fixtures for fixture edge case testing."""
import pytest
import tempfile
import os

_module_fixture_state = {"initialized": False, "cleanup_count": 0}

@pytest.fixture(scope="module")
def module_scoped_resource():
    """Module-scoped fixture to test scope handling."""
    _module_fixture_state["initialized"] = True
    yield {"created": True}
    _module_fixture_state["cleanup_count"] += 1

def get_module_state():
    return _module_fixture_state.copy()
