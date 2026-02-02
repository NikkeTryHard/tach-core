"""SQLAlchemy test configuration for gauntlet tests.

This conftest.py sets up the Python path so tach_harness can be imported.
"""
import os
import sys


def pytest_configure(config):
    """Configure Python path before test collection."""
    # Add the src directory to path so tach_harness can be found
    project_root = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
    src_dir = os.path.join(project_root, "src")
    if src_dir not in sys.path:
        sys.path.insert(0, src_dir)
