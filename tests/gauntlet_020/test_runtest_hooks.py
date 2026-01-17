# test_runtest_hooks.py - Tests for pytest_runtest_* hook support
#
# These tests verify that the runtest hooks correctly:
# - Execute setup hooks before tests
# - Execute teardown hooks after tests
# - Capture side effects from hooks
# - Handle errors gracefully

import os
import sys
import tempfile


def test_runtest_setup_is_called():
    """pytest_runtest_setup hook should be invoked before test."""
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
            f.write(
                """
import os
def pytest_runtest_setup(item):
    os.environ["SETUP_HOOK_CALLED"] = "yes"
"""
            )

        os.environ.pop("SETUP_HOOK_CALLED", None)

        result = harness.call_hook_impl(
            conftest_path=conftest_path,
            hook_name="pytest_runtest_setup",
            args={"item": None},
        )

        assert result["error"] is None, f"Unexpected error: {result['error']}"
        effects = [e for e in result["effects"] if e["type"] == "SetEnv"]
        assert any(e["key"] == "SETUP_HOOK_CALLED" for e in effects)


def test_runtest_setup_captures_sys_path_changes():
    """pytest_runtest_setup should capture sys.path modifications."""
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
        test_path = "/runtest_setup/custom/path"
        with open(conftest_path, "w") as f:
            f.write(
                f"""
import sys
def pytest_runtest_setup(item):
    if "{test_path}" not in sys.path:
        sys.path.insert(0, "{test_path}")
"""
            )

        # Clean up any existing path
        if test_path in sys.path:
            sys.path.remove(test_path)

        result = harness.call_hook_impl(
            conftest_path=conftest_path,
            hook_name="pytest_runtest_setup",
            args={"item": None},
        )

        assert result["error"] is None
        path_effects = [e for e in result["effects"] if e["type"] == "ModifySysPath"]
        assert any(e["path"] == test_path for e in path_effects)

        # Clean up
        if test_path in sys.path:
            sys.path.remove(test_path)


def test_runtest_setup_missing_hook():
    """pytest_runtest_setup returns empty result if hook not found."""
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
            f.write("# Empty conftest\n")

        result = harness.call_hook_impl(
            conftest_path=conftest_path,
            hook_name="pytest_runtest_setup",
            args={"item": None},
        )

        # Should not be an error - just no result
        assert result["error"] is None
        assert result["return_value"] is None


def test_runtest_teardown_is_called():
    """pytest_runtest_teardown hook should be invoked after test."""
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
            f.write(
                """
import os
def pytest_runtest_teardown(item):
    os.environ["TEARDOWN_HOOK_CALLED"] = "yes"
"""
            )

        os.environ.pop("TEARDOWN_HOOK_CALLED", None)

        result = harness.call_hook_impl(
            conftest_path=conftest_path,
            hook_name="pytest_runtest_teardown",
            args={"item": None},
        )

        assert result["error"] is None
        effects = [e for e in result["effects"] if e["type"] == "SetEnv"]
        assert any(e["key"] == "TEARDOWN_HOOK_CALLED" for e in effects)


def test_runtest_teardown_captures_effects_on_error():
    """pytest_runtest_teardown should still capture effects even if it raises."""
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
            f.write(
                """
import os
def pytest_runtest_teardown(item):
    os.environ["PARTIAL_CLEANUP"] = "done"
    raise RuntimeError("Teardown failed")
"""
            )

        os.environ.pop("PARTIAL_CLEANUP", None)

        result = harness.call_hook_impl(
            conftest_path=conftest_path,
            hook_name="pytest_runtest_teardown",
            args={"item": None},
        )

        # Should have error
        assert result["error"] is not None
        assert "Teardown failed" in result["error"]

        # Effects should still be captured even on error
        effects = [e for e in result["effects"] if e["type"] == "SetEnv"]
        assert any(e["key"] == "PARTIAL_CLEANUP" for e in effects)


def test_runtest_teardown_with_nextitem():
    """pytest_runtest_teardown handles nextitem parameter."""
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
            f.write(
                """
import os
def pytest_runtest_teardown(item, nextitem):
    if nextitem is None:
        os.environ["LAST_TEST_TEARDOWN"] = "yes"
    else:
        os.environ["HAS_NEXT_TEST"] = "yes"
"""
            )

        os.environ.pop("LAST_TEST_TEARDOWN", None)
        os.environ.pop("HAS_NEXT_TEST", None)

        # Test with nextitem=None (last test)
        result = harness.call_hook_impl(
            conftest_path=conftest_path,
            hook_name="pytest_runtest_teardown",
            args={"item": None, "nextitem": None},
        )

        assert result["error"] is None
        effects = [e for e in result["effects"] if e["type"] == "SetEnv"]
        assert any(e["key"] == "LAST_TEST_TEARDOWN" for e in effects)
