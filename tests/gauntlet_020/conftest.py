# conftest.py - Test fixtures for gauntlet_020 (Hook Caller tests)
#
# This conftest demonstrates hook implementations that can be called
# by the call_hook_impl() function in tach_harness.py

import os
import sys


def pytest_configure(config):
    """Session-level hook that sets environment variables and modifies sys.path."""
    os.environ["GAUNTLET_020_CONFIGURED"] = "true"
    os.environ["HOOK_TEST_VALUE"] = "from_pytest_configure"


def pytest_collection_modifyitems(session, config, items):
    """Collection hook that can reorder or filter tests."""
    # This hook doesn't modify items, just returns a value for testing
    return ["item1", "item2", "item3"]


def pytest_runtest_setup(item):
    """Per-test setup hook."""
    # Side-effect only hook - no return value
    pass


def custom_hook_with_return():
    """Custom hook that returns a value (for testing non-pytest hooks)."""
    return {"status": "success", "count": 42}


def custom_hook_with_env_effect():
    """Custom hook that modifies environment."""
    os.environ["CUSTOM_HOOK_VAR"] = "custom_value"
    return "effect_applied"


def custom_hook_with_sys_path_effect(test_path: str = "/tach_test_hook_caller_unique_path_12345"):
    """Hook that modifies sys.path for testing effect capture."""
    if test_path not in sys.path:
        sys.path.insert(0, test_path)
    return "path_added"


def hook_with_error():
    """Hook that raises an exception (for error handling tests)."""
    raise ValueError("Intentional test error")


def hook_with_args(name: str, value: int = 10):
    """Hook that accepts arguments."""
    return f"name={name}, value={value}"
