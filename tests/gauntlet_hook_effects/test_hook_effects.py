"""Tests for hook effect recording and replay mechanism (v0.2.0).

These tests verify that:
1. pytest_configure hook effects (env vars) are recorded during session init
2. Workers can access the recorded effects
3. Effect replay works correctly in worker processes
"""

import os
import sys


class TestHookEffects:
    """Test hook effect recording and replay."""

    def test_a_env_var_from_configure_hook(self):
        """Test that environment variable set in pytest_configure is available.

        The conftest.py::pytest_configure hook sets TACH_HOOK_TEST_VAR.
        This test verifies that the effect is available in the worker.
        """
        # Environment variable should be set from pytest_configure
        value = os.environ.get("TACH_HOOK_TEST_VAR")
        assert value == "configured_value", (
            f"Expected 'configured_value', got '{value}'. "
            "pytest_configure hook effect was not applied."
        )

    def test_b_another_env_var_from_hook(self):
        """Test another environment variable from pytest_configure."""
        value = os.environ.get("TACH_HOOK_TEST_NUMBER")
        assert value == "42", (
            f"Expected '42', got '{value}'. "
            "pytest_configure hook effect was not applied."
        )

    def test_c_sys_path_effect(self):
        """Test that sys.path modification from pytest_configure is present.

        The conftest.py::pytest_configure hook appends a test path.
        This test verifies the sys.path effect was recorded and replayed.
        """
        test_path = "/tmp/tach_hook_effects_test_path"
        assert test_path in sys.path, (
            f"Expected '{test_path}' in sys.path. "
            "pytest_configure hook effect for sys.path was not applied."
        )

    def test_d_effects_persist_across_tests(self):
        """Test that effects persist across multiple test executions.

        Even if memory is reset between tests, the effects should be replayed.
        """
        # Both environment and sys.path effects should still be present
        assert os.environ.get("TACH_HOOK_TEST_VAR") == "configured_value"
        assert "/tmp/tach_hook_effects_test_path" in sys.path
