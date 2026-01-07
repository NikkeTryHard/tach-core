"""Pytest marker configuration.

This conftest.py registers custom markers to avoid warnings.
"""

import pytest


def pytest_configure(config):
    """Register custom markers."""
    config.addinivalue_line("markers", "slow: marks tests as slow")
    config.addinivalue_line("markers", "fast: marks tests as fast")
    config.addinivalue_line("markers", "integration: marks integration tests")
    config.addinivalue_line("markers", "unit: marks unit tests")
    config.addinivalue_line("markers", "smoke: marks smoke tests")
    config.addinivalue_line("markers", "network: marks tests requiring network")
    config.addinivalue_line("markers", "database: marks tests requiring database")
