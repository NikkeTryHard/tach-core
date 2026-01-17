"""Root conftest for v0.2.0 hook testing."""
import os
import sys

import pytest

_autouse_counter = {"count": 0}


def pytest_configure(config):
    """Session-level hook that modifies environment."""
    os.environ["TACH_020_ROOT_HOOK"] = "executed"
    sys.path.insert(0, "/tmp/tach_020_test_path")

    # Register custom markers to avoid pytest warnings
    config.addinivalue_line(
        "markers", "slow: marks tests as slow (deselect with '-m \"not slow\"')"
    )
    config.addinivalue_line(
        "markers", "integration: marks tests as integration tests"
    )


@pytest.fixture(autouse=True)
def track_autouse_execution():
    """Autouse fixture that runs for every test."""
    _autouse_counter["count"] += 1
    yield


@pytest.fixture
def autouse_count_tracker():
    """Fixture to expose autouse counter to tests."""
    return _autouse_counter


def get_autouse_count():
    """Helper to check autouse execution count."""
    return _autouse_counter["count"]
