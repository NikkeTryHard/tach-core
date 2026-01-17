"""
Conftest for gauntlet_015 tests.

Provides fixtures for --no-ignore flag testing.
"""

import os
import pytest


@pytest.fixture
def blocked_project_path():
    """Return the path to the blocked_project fixture directory."""
    return os.path.join(os.path.dirname(__file__), "fixtures", "blocked_project")
