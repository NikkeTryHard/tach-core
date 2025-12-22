"""Environment Variable Tests

Verify that environment variables from pyproject.toml are correctly
loaded and available to test workers.
"""

import os


def test_env_var_from_pyproject():
    """Verify TACH_TEST_VAR from pyproject.toml is available."""
    value = os.environ.get("TACH_TEST_VAR")
    assert value == "hello_from_pyproject", (
        f"Expected 'hello_from_pyproject', got '{value}'"
    )


def test_debug_mode_from_pyproject():
    """Verify TACH_DEBUG_MODE from pyproject.toml is available."""
    value = os.environ.get("TACH_DEBUG_MODE")
    assert value == "true", f"Expected 'true', got '{value}'"
