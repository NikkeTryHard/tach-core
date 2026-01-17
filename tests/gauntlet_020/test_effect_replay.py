"""Test that hook effects are replayed correctly in workers."""
import os
import sys


def test_env_effect_persists():
    """Environment changes from pytest_configure should persist."""
    assert "TACH_020_ROOT_HOOK" in os.environ
    assert os.environ["TACH_020_ROOT_HOOK"] == "executed"


def test_sys_path_effect_persists():
    """sys.path changes from pytest_configure should persist."""
    assert "/tmp/tach_020_test_path" in sys.path


def test_effects_isolated_between_tests():
    """Verify effects don't leak between tests incorrectly."""
    os.environ["TACH_020_LOCAL_TEST"] = "local_value"
    assert os.environ["TACH_020_LOCAL_TEST"] == "local_value"


def test_previous_test_env_not_leaked():
    """This runs after test_effects_isolated_between_tests.

    Verifies that environment variables set by previous tests do not leak
    into subsequent tests. TACH_020_LOCAL_TEST was set in the previous test
    but should not be present here due to test isolation.

    Note: This test only provides meaningful verification when run under Tach,
    which provides process isolation between tests. When run with vanilla pytest,
    we skip this test since pytest runs all tests in the same process where
    environment variables naturally persist.
    """
    # Skip when not running under Tach - vanilla pytest doesn't isolate tests
    if "TACH_WORKER_ID" not in os.environ:
        import pytest
        pytest.skip("Environment isolation only works under Tach workers")

    assert "TACH_020_LOCAL_TEST" not in os.environ, (
        "Environment variable from previous test leaked - isolation failure"
    )
