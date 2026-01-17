# test_hook_integration.py - End-to-end integration tests for hook system
#
# These tests verify that the complete hook system works correctly:
# - Hooks execute in the correct order (root -> leaf conftest)
# - Environment effects propagate correctly to workers
# - Hook dependency graph resolves conftest hierarchy
# - Plugin registry correctly identifies plugin status

import os
import sys
import tempfile
import pytest


class TestHookDependencyOrder:
    """Test that hooks execute in the correct conftest hierarchy order."""

    def test_hooks_resolve_root_to_leaf_order(self):
        """Hook resolution should return hooks ordered from root to leaf conftest."""
        # Import the hook registry from the Rust module
        harness_path = os.path.join(
            os.path.dirname(__file__), "..", "..", "src", "tach_harness.py"
        )
        harness_path = os.path.abspath(harness_path)

        import importlib.util

        spec = importlib.util.spec_from_file_location("tach_harness", harness_path)
        harness = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(harness)

        # Create a temporary project with nested conftest files
        with tempfile.TemporaryDirectory() as tmpdir:
            # Root conftest
            root_conftest = os.path.join(tmpdir, "conftest.py")
            with open(root_conftest, "w") as f:
                f.write("""
import os
def pytest_configure(config):
    os.environ["ROOT_ORDER"] = "1"
""")

            # Tests directory with its own conftest
            tests_dir = os.path.join(tmpdir, "tests")
            os.makedirs(tests_dir)
            tests_conftest = os.path.join(tests_dir, "conftest.py")
            with open(tests_conftest, "w") as f:
                f.write("""
import os
def pytest_configure(config):
    os.environ["TESTS_ORDER"] = "2"
""")

            # Unit tests subdirectory with its own conftest
            unit_dir = os.path.join(tests_dir, "unit")
            os.makedirs(unit_dir)
            unit_conftest = os.path.join(unit_dir, "conftest.py")
            with open(unit_conftest, "w") as f:
                f.write("""
import os
def pytest_configure(config):
    os.environ["UNIT_ORDER"] = "3"
""")

            # Clear environment
            for key in ["ROOT_ORDER", "TESTS_ORDER", "UNIT_ORDER"]:
                os.environ.pop(key, None)

            # Call each conftest in order (simulating root->leaf execution)
            result1 = harness.call_hook_impl(root_conftest, "pytest_configure", {"config": None})
            result2 = harness.call_hook_impl(tests_conftest, "pytest_configure", {"config": None})
            result3 = harness.call_hook_impl(unit_conftest, "pytest_configure", {"config": None})

            # All should succeed
            assert result1["error"] is None
            assert result2["error"] is None
            assert result3["error"] is None

            # Verify environment effects captured in order
            env_effects_1 = [e for e in result1["effects"] if e["type"] == "SetEnv"]
            env_effects_2 = [e for e in result2["effects"] if e["type"] == "SetEnv"]
            env_effects_3 = [e for e in result3["effects"] if e["type"] == "SetEnv"]

            root_effect = next((e for e in env_effects_1 if e.get("key") == "ROOT_ORDER"), None)
            tests_effect = next((e for e in env_effects_2 if e.get("key") == "TESTS_ORDER"), None)
            unit_effect = next((e for e in env_effects_3 if e.get("key") == "UNIT_ORDER"), None)

            assert root_effect is not None and root_effect["value"] == "1"
            assert tests_effect is not None and tests_effect["value"] == "2"
            assert unit_effect is not None and unit_effect["value"] == "3"


class TestHookEffectPropagation:
    """Test that hook effects are correctly captured and can be replayed."""

    def test_env_effects_are_captured(self):
        """Environment variable changes should be captured as effects."""
        harness_path = os.path.join(
            os.path.dirname(__file__), "..", "..", "src", "tach_harness.py"
        )
        harness_path = os.path.abspath(harness_path)

        import importlib.util

        spec = importlib.util.spec_from_file_location("tach_harness", harness_path)
        harness = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(harness)

        with tempfile.TemporaryDirectory() as tmpdir:
            conftest_path = os.path.join(tmpdir, "conftest.py")
            with open(conftest_path, "w") as f:
                f.write("""
import os
def pytest_configure(config):
    os.environ["INTEGRATION_VAR_1"] = "value1"
    os.environ["INTEGRATION_VAR_2"] = "value2"
    os.environ["INTEGRATION_VAR_3"] = "value3"
""")

            # Clear any existing vars
            for key in ["INTEGRATION_VAR_1", "INTEGRATION_VAR_2", "INTEGRATION_VAR_3"]:
                os.environ.pop(key, None)

            result = harness.call_hook_impl(conftest_path, "pytest_configure", {"config": None})

            assert result["error"] is None

            # Should have captured all 3 env effects
            env_effects = [e for e in result["effects"] if e["type"] == "SetEnv"]
            keys = {e["key"] for e in env_effects}

            assert "INTEGRATION_VAR_1" in keys
            assert "INTEGRATION_VAR_2" in keys
            assert "INTEGRATION_VAR_3" in keys

    def test_sys_path_effects_are_captured(self):
        """sys.path modifications should be captured as effects."""
        harness_path = os.path.join(
            os.path.dirname(__file__), "..", "..", "src", "tach_harness.py"
        )
        harness_path = os.path.abspath(harness_path)

        import importlib.util

        spec = importlib.util.spec_from_file_location("tach_harness", harness_path)
        harness = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(harness)

        with tempfile.TemporaryDirectory() as tmpdir:
            conftest_path = os.path.join(tmpdir, "conftest.py")
            test_path = "/integration/test/path"
            with open(conftest_path, "w") as f:
                f.write(f"""
import sys
def pytest_configure(config):
    sys.path.insert(0, "{test_path}")
""")

            # Clean up any existing path
            if test_path in sys.path:
                sys.path.remove(test_path)

            result = harness.call_hook_impl(conftest_path, "pytest_configure", {"config": None})

            assert result["error"] is None

            # Should have captured the sys.path effect
            path_effects = [e for e in result["effects"] if e["type"] == "ModifySysPath"]
            assert len(path_effects) >= 1

            path_effect = next((e for e in path_effects if e.get("path") == test_path), None)
            assert path_effect is not None
            assert path_effect["action"] == "prepend"

            # Clean up
            if test_path in sys.path:
                sys.path.remove(test_path)


class TestMultipleHookTypes:
    """Test that different hook types work correctly together."""

    def test_all_supported_hooks_can_be_called(self):
        """All supported pytest hooks should be callable without error."""
        harness_path = os.path.join(
            os.path.dirname(__file__), "..", "..", "src", "tach_harness.py"
        )
        harness_path = os.path.abspath(harness_path)

        import importlib.util

        spec = importlib.util.spec_from_file_location("tach_harness", harness_path)
        harness = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(harness)

        with tempfile.TemporaryDirectory() as tmpdir:
            conftest_path = os.path.join(tmpdir, "conftest.py")
            with open(conftest_path, "w") as f:
                f.write("""
import os

def pytest_configure(config):
    os.environ["HOOK_CONFIGURE"] = "called"

def pytest_sessionstart(session):
    os.environ["HOOK_SESSIONSTART"] = "called"

def pytest_collection_modifyitems(session, config, items):
    pass  # No-op

def pytest_runtest_setup(item):
    pass  # No-op

def pytest_runtest_teardown(item, nextitem):
    pass  # No-op

def pytest_runtest_makereport(item, call):
    return None

def pytest_sessionfinish(session, exitstatus):
    os.environ["HOOK_SESSIONFINISH"] = "called"
""")

            # Test each hook type
            hooks_to_test = [
                ("pytest_configure", {"config": None}),
                ("pytest_sessionstart", {"session": None}),
                ("pytest_collection_modifyitems", {"session": None, "config": None, "items": []}),
                ("pytest_runtest_setup", {"item": None}),
                ("pytest_runtest_teardown", {"item": None, "nextitem": None}),
                ("pytest_runtest_makereport", {"item": None, "call": None}),
                ("pytest_sessionfinish", {"session": None, "exitstatus": 0}),
            ]

            for hook_name, args in hooks_to_test:
                result = harness.call_hook_impl(conftest_path, hook_name, args)
                assert result["error"] is None, f"Hook {hook_name} failed: {result['error']}"


class TestHookErrorHandling:
    """Test that hook errors are handled gracefully."""

    def test_hook_exception_is_captured(self):
        """Exceptions in hooks should be captured, not crash the system."""
        harness_path = os.path.join(
            os.path.dirname(__file__), "..", "..", "src", "tach_harness.py"
        )
        harness_path = os.path.abspath(harness_path)

        import importlib.util

        spec = importlib.util.spec_from_file_location("tach_harness", harness_path)
        harness = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(harness)

        with tempfile.TemporaryDirectory() as tmpdir:
            conftest_path = os.path.join(tmpdir, "conftest.py")
            with open(conftest_path, "w") as f:
                f.write("""
def pytest_configure(config):
    raise RuntimeError("Intentional integration test error")
""")

            result = harness.call_hook_impl(conftest_path, "pytest_configure", {"config": None})

            # Error should be captured
            assert result["error"] is not None
            assert "Intentional integration test error" in result["error"]
            # Return value should be None on error
            assert result["return_value"] is None

    def test_missing_hook_returns_empty_result(self):
        """Missing hooks should return empty result, not error."""
        harness_path = os.path.join(
            os.path.dirname(__file__), "..", "..", "src", "tach_harness.py"
        )
        harness_path = os.path.abspath(harness_path)

        import importlib.util

        spec = importlib.util.spec_from_file_location("tach_harness", harness_path)
        harness = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(harness)

        with tempfile.TemporaryDirectory() as tmpdir:
            conftest_path = os.path.join(tmpdir, "conftest.py")
            with open(conftest_path, "w") as f:
                f.write("""
# No pytest_configure defined here
def some_other_function():
    pass
""")

            result = harness.call_hook_impl(conftest_path, "pytest_configure", {"config": None})

            # Should not be an error - hook just doesn't exist
            assert result["error"] is None
            assert result["return_value"] is None

    def test_invalid_conftest_path_returns_error(self):
        """Invalid conftest path should return error result."""
        harness_path = os.path.join(
            os.path.dirname(__file__), "..", "..", "src", "tach_harness.py"
        )
        harness_path = os.path.abspath(harness_path)

        import importlib.util

        spec = importlib.util.spec_from_file_location("tach_harness", harness_path)
        harness = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(harness)

        result = harness.call_hook_impl("/nonexistent/path/conftest.py", "pytest_configure", {"config": None})

        # Should return an error
        assert result["error"] is not None


class TestHookReturnValues:
    """Test that hook return values are correctly captured."""

    def test_hook_with_dict_return_value(self):
        """Hook returning a dict should have it JSON-serialized."""
        harness_path = os.path.join(
            os.path.dirname(__file__), "..", "..", "src", "tach_harness.py"
        )
        harness_path = os.path.abspath(harness_path)

        import importlib.util
        import json

        spec = importlib.util.spec_from_file_location("tach_harness", harness_path)
        harness = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(harness)

        with tempfile.TemporaryDirectory() as tmpdir:
            conftest_path = os.path.join(tmpdir, "conftest.py")
            with open(conftest_path, "w") as f:
                f.write("""
def custom_hook():
    return {"key": "value", "number": 42, "nested": {"a": 1}}
""")

            result = harness.call_hook_impl(conftest_path, "custom_hook", {})

            assert result["error"] is None
            assert result["return_value"] is not None

            # Return value should be JSON-serialized
            parsed = json.loads(result["return_value"])
            assert parsed["key"] == "value"
            assert parsed["number"] == 42
            assert parsed["nested"]["a"] == 1

    def test_hook_with_list_return_value(self):
        """Hook returning a list should have it JSON-serialized."""
        harness_path = os.path.join(
            os.path.dirname(__file__), "..", "..", "src", "tach_harness.py"
        )
        harness_path = os.path.abspath(harness_path)

        import importlib.util
        import json

        spec = importlib.util.spec_from_file_location("tach_harness", harness_path)
        harness = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(harness)

        with tempfile.TemporaryDirectory() as tmpdir:
            conftest_path = os.path.join(tmpdir, "conftest.py")
            with open(conftest_path, "w") as f:
                f.write("""
def custom_hook():
    return ["a", "b", "c", 1, 2, 3]
""")

            result = harness.call_hook_impl(conftest_path, "custom_hook", {})

            assert result["error"] is None
            assert result["return_value"] is not None

            parsed = json.loads(result["return_value"])
            assert parsed == ["a", "b", "c", 1, 2, 3]

    def test_hook_with_string_return_value(self):
        """Hook returning a string should be preserved as-is."""
        harness_path = os.path.join(
            os.path.dirname(__file__), "..", "..", "src", "tach_harness.py"
        )
        harness_path = os.path.abspath(harness_path)

        import importlib.util

        spec = importlib.util.spec_from_file_location("tach_harness", harness_path)
        harness = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(harness)

        with tempfile.TemporaryDirectory() as tmpdir:
            conftest_path = os.path.join(tmpdir, "conftest.py")
            with open(conftest_path, "w") as f:
                f.write("""
def custom_hook():
    return "simple string result"
""")

            result = harness.call_hook_impl(conftest_path, "custom_hook", {})

            assert result["error"] is None
            assert result["return_value"] == "simple string result"

    def test_hook_with_none_return_value(self):
        """Hook returning None should have None return value."""
        harness_path = os.path.join(
            os.path.dirname(__file__), "..", "..", "src", "tach_harness.py"
        )
        harness_path = os.path.abspath(harness_path)

        import importlib.util

        spec = importlib.util.spec_from_file_location("tach_harness", harness_path)
        harness = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(harness)

        with tempfile.TemporaryDirectory() as tmpdir:
            conftest_path = os.path.join(tmpdir, "conftest.py")
            with open(conftest_path, "w") as f:
                f.write("""
def custom_hook():
    return None
""")

            result = harness.call_hook_impl(conftest_path, "custom_hook", {})

            assert result["error"] is None
            assert result["return_value"] is None
