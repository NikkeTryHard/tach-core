"""Nested conftest for inheritance testing."""
import os


def pytest_configure(config):
    """Leaf hook - should run AFTER root hook."""
    current = os.environ.get("TACH_020_ROOT_HOOK", "")
    os.environ["TACH_020_LEAF_HOOK"] = f"{current}+leaf"
