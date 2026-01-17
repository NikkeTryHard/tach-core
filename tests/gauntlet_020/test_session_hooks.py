# test_session_hooks.py - Tests for session-level hooks
#
# These tests verify that session-level hooks work correctly:
# - pytest_sessionfinish is called at end of session
# - Exit status is passed correctly to hooks

import os
import sys
import tempfile


def test_sessionfinish_is_called():
    """pytest_sessionfinish hook should be invoked at end of session."""
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
def pytest_sessionfinish(session, exitstatus):
    os.environ["SESSION_FINISHED"] = str(exitstatus)
"""
            )

        os.environ.pop("SESSION_FINISHED", None)

        result = harness.call_hook_impl(
            conftest_path=conftest_path,
            hook_name="pytest_sessionfinish",
            args={"session": None, "exitstatus": 0},
        )

        assert result["error"] is None, f"Unexpected error: {result['error']}"
        effects = [e for e in result["effects"] if e["type"] == "SetEnv"]
        assert any(e["key"] == "SESSION_FINISHED" for e in effects)


def test_sessionfinish_with_failure_exitstatus():
    """pytest_sessionfinish receives correct exitstatus on failure."""
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
def pytest_sessionfinish(session, exitstatus):
    os.environ["EXIT_STATUS"] = str(exitstatus)
"""
            )

        os.environ.pop("EXIT_STATUS", None)

        result = harness.call_hook_impl(
            conftest_path=conftest_path,
            hook_name="pytest_sessionfinish",
            args={"session": None, "exitstatus": 1},
        )

        assert result["error"] is None
        effects = [e for e in result["effects"] if e["type"] == "SetEnv"]
        exit_effect = next((e for e in effects if e["key"] == "EXIT_STATUS"), None)
        assert exit_effect is not None
        assert exit_effect["value"] == "1"


def test_sessionfinish_missing_hook():
    """pytest_sessionfinish returns empty result if hook not found."""
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
            hook_name="pytest_sessionfinish",
            args={"session": None, "exitstatus": 0},
        )

        # Should not be an error - just no result
        assert result["error"] is None
        assert result["return_value"] is None


def test_sessionfinish_with_return_value():
    """pytest_sessionfinish can return a value."""
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
def pytest_sessionfinish(session, exitstatus):
    return {"summary": "Session completed", "exitstatus": exitstatus}
"""
            )

        result = harness.call_hook_impl(
            conftest_path=conftest_path,
            hook_name="pytest_sessionfinish",
            args={"session": None, "exitstatus": 0},
        )

        assert result["error"] is None
        assert result["return_value"] is not None
