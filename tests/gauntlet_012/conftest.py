# conftest.py - Configuration for gauntlet_012 tests
import pytest


def pytest_configure(config):
    """Register custom markers to avoid warnings."""
    config.addinivalue_line(
        "markers",
        "timeout(seconds): Set test timeout in seconds (parsed by tach discovery)"
    )
