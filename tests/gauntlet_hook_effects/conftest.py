"""Conftest with pytest_configure hook that modifies environment.

This tests the hook effect recording and replay mechanism:
1. pytest_configure is called during session initialization
2. Environment changes are recorded as effects
3. Workers replay these effects before running tests
"""

import os
import sys


def pytest_configure(config):
    """Session-level hook that modifies environment variables.

    These changes should be recorded by tach and replayed in workers.
    """
    # Set environment variables that tests will check
    os.environ["TACH_HOOK_TEST_VAR"] = "configured_value"
    os.environ["TACH_HOOK_TEST_NUMBER"] = "42"

    # Also modify sys.path (another effect type)
    # Note: Adding a test-specific path to verify sys.path effect recording
    test_path = "/tmp/tach_hook_effects_test_path"
    if test_path not in sys.path:
        sys.path.append(test_path)
