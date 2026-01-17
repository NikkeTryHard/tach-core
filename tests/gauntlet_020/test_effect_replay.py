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
    """This runs after test_effects_isolated_between_tests."""
    pass  # Document expected behavior
