"""
Conftest for gauntlet_011 tests.

Provides fixtures for nested conftest inheritance testing.
"""

import pytest


@pytest.fixture
def gauntlet_011_fixture():
    """Fixture defined in parent conftest.py for nested tests."""
    return "gauntlet_011_root"
