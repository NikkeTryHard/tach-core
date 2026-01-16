# test_hook_caller.py - Tests for call_hook_impl() in tach_harness.py
#
# These tests verify that the Python hook caller correctly:
# - Loads conftest modules dynamically
# - Calls hook functions with proper argument filtering
# - Captures return values and serializes them
# - Tracks environment and sys.path side effects
# - Handles errors gracefully

import os
import sys
import tempfile


def test_call_hook_impl_basic():
    """Test that call_hook_impl can load and call a simple hook."""
    # Import the harness module
    harness_path = os.path.join(
        os.path.dirname(__file__), "..", "..", "src", "tach_harness.py"
    )
    harness_path = os.path.abspath(harness_path)

    # Load harness module
    import importlib.util

    spec = importlib.util.spec_from_file_location("tach_harness", harness_path)
    harness = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(harness)

    # Get path to this test's conftest.py
    conftest_path = os.path.join(os.path.dirname(__file__), "conftest.py")

    # Call a custom hook that returns a value
    result = harness.call_hook_impl(conftest_path, "custom_hook_with_return", {})

    assert result["error"] is None, f"Unexpected error: {result['error']}"
    assert result["return_value"] is not None
    # Return value should be JSON-serialized
    import json

    parsed = json.loads(result["return_value"])
    assert parsed["status"] == "success"
    assert parsed["count"] == 42


def test_call_hook_impl_with_env_effect():
    """Test that call_hook_impl captures environment variable changes."""
    harness_path = os.path.join(
        os.path.dirname(__file__), "..", "..", "src", "tach_harness.py"
    )
    harness_path = os.path.abspath(harness_path)

    import importlib.util

    spec = importlib.util.spec_from_file_location("tach_harness", harness_path)
    harness = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(harness)

    conftest_path = os.path.join(os.path.dirname(__file__), "conftest.py")

    # Clean up any existing env var
    if "CUSTOM_HOOK_VAR" in os.environ:
        del os.environ["CUSTOM_HOOK_VAR"]

    result = harness.call_hook_impl(conftest_path, "custom_hook_with_env_effect", {})

    assert result["error"] is None
    assert result["return_value"] == "effect_applied"

    # Check that the effect was captured
    env_effects = [e for e in result["effects"] if e["type"] == "SetEnv"]
    assert len(env_effects) >= 1

    # Find the specific effect we're looking for
    custom_var_effect = next(
        (e for e in env_effects if e.get("key") == "CUSTOM_HOOK_VAR"), None
    )
    assert custom_var_effect is not None
    assert custom_var_effect["value"] == "custom_value"


def test_call_hook_impl_with_sys_path_effect():
    """Test that call_hook_impl captures sys.path modifications."""
    harness_path = os.path.join(
        os.path.dirname(__file__), "..", "..", "src", "tach_harness.py"
    )
    harness_path = os.path.abspath(harness_path)

    import importlib.util

    spec = importlib.util.spec_from_file_location("tach_harness", harness_path)
    harness = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(harness)

    conftest_path = os.path.join(os.path.dirname(__file__), "conftest.py")

    # Clean up any existing path
    test_path = "/gauntlet_020/custom/path"
    if test_path in sys.path:
        sys.path.remove(test_path)

    result = harness.call_hook_impl(
        conftest_path, "custom_hook_with_sys_path_effect", {}
    )

    assert result["error"] is None
    assert result["return_value"] == "path_added"

    # Check that the effect was captured
    path_effects = [e for e in result["effects"] if e["type"] == "ModifySysPath"]
    assert len(path_effects) >= 1

    # Find the specific effect
    custom_path_effect = next(
        (e for e in path_effects if e.get("path") == test_path), None
    )
    assert custom_path_effect is not None
    assert custom_path_effect["action"] == "prepend"

    # Clean up
    if test_path in sys.path:
        sys.path.remove(test_path)


def test_call_hook_impl_missing_hook():
    """Test that missing hooks return empty result without error."""
    harness_path = os.path.join(
        os.path.dirname(__file__), "..", "..", "src", "tach_harness.py"
    )
    harness_path = os.path.abspath(harness_path)

    import importlib.util

    spec = importlib.util.spec_from_file_location("tach_harness", harness_path)
    harness = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(harness)

    conftest_path = os.path.join(os.path.dirname(__file__), "conftest.py")

    # Call a hook that doesn't exist
    result = harness.call_hook_impl(conftest_path, "nonexistent_hook", {})

    # Should not be an error - just no result
    assert result["error"] is None
    assert result["return_value"] is None


def test_call_hook_impl_with_args():
    """Test that call_hook_impl passes arguments to hooks correctly."""
    harness_path = os.path.join(
        os.path.dirname(__file__), "..", "..", "src", "tach_harness.py"
    )
    harness_path = os.path.abspath(harness_path)

    import importlib.util

    spec = importlib.util.spec_from_file_location("tach_harness", harness_path)
    harness = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(harness)

    conftest_path = os.path.join(os.path.dirname(__file__), "conftest.py")

    # Call hook with arguments
    result = harness.call_hook_impl(
        conftest_path, "hook_with_args", {"name": "test_name", "value": 99}
    )

    assert result["error"] is None
    assert result["return_value"] == "name=test_name, value=99"


def test_call_hook_impl_filters_unknown_args():
    """Test that call_hook_impl filters out arguments not in function signature."""
    harness_path = os.path.join(
        os.path.dirname(__file__), "..", "..", "src", "tach_harness.py"
    )
    harness_path = os.path.abspath(harness_path)

    import importlib.util

    spec = importlib.util.spec_from_file_location("tach_harness", harness_path)
    harness = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(harness)

    conftest_path = os.path.join(os.path.dirname(__file__), "conftest.py")

    # Call hook with extra arguments that should be filtered
    result = harness.call_hook_impl(
        conftest_path,
        "hook_with_args",
        {"name": "filtered_test", "value": 50, "unknown_arg": "should_be_ignored"},
    )

    assert result["error"] is None
    assert result["return_value"] == "name=filtered_test, value=50"


def test_call_hook_impl_error_handling():
    """Test that call_hook_impl captures hook execution errors."""
    harness_path = os.path.join(
        os.path.dirname(__file__), "..", "..", "src", "tach_harness.py"
    )
    harness_path = os.path.abspath(harness_path)

    import importlib.util

    spec = importlib.util.spec_from_file_location("tach_harness", harness_path)
    harness = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(harness)

    conftest_path = os.path.join(os.path.dirname(__file__), "conftest.py")

    # Call a hook that raises an exception
    result = harness.call_hook_impl(conftest_path, "hook_with_error", {})

    assert result["error"] is not None
    assert "Intentional test error" in result["error"]
    assert result["return_value"] is None


def test_call_hook_impl_invalid_path():
    """Test that call_hook_impl handles invalid conftest paths."""
    harness_path = os.path.join(
        os.path.dirname(__file__), "..", "..", "src", "tach_harness.py"
    )
    harness_path = os.path.abspath(harness_path)

    import importlib.util

    spec = importlib.util.spec_from_file_location("tach_harness", harness_path)
    harness = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(harness)

    # Call with non-existent path
    result = harness.call_hook_impl("/nonexistent/conftest.py", "some_hook", {})

    assert result["error"] is not None
    # Either loading or execution should fail


def test_call_hook_impl_pytest_configure():
    """Test calling the actual pytest_configure hook."""
    harness_path = os.path.join(
        os.path.dirname(__file__), "..", "..", "src", "tach_harness.py"
    )
    harness_path = os.path.abspath(harness_path)

    import importlib.util

    spec = importlib.util.spec_from_file_location("tach_harness", harness_path)
    harness = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(harness)

    conftest_path = os.path.join(os.path.dirname(__file__), "conftest.py")

    # Clean up env vars first
    for key in ["GAUNTLET_020_CONFIGURED", "HOOK_TEST_VALUE"]:
        if key in os.environ:
            del os.environ[key]

    # Call pytest_configure - it sets env vars but returns None
    result = harness.call_hook_impl(conftest_path, "pytest_configure", {"config": None})

    assert result["error"] is None
    # pytest_configure returns None
    assert result["return_value"] is None

    # But it should have captured the env effects
    env_effects = [e for e in result["effects"] if e["type"] == "SetEnv"]
    assert len(env_effects) >= 2

    # Verify the env vars were actually set
    configured_effect = next(
        (e for e in env_effects if e.get("key") == "GAUNTLET_020_CONFIGURED"), None
    )
    assert configured_effect is not None
    assert configured_effect["value"] == "true"
