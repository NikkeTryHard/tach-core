"""Test that fixture teardown failures promote status to ERROR.

When a test passes but its fixture teardown fails, the final status
should be STATUS_ERROR (4), not STATUS_PASSED.
"""
import pytest


@pytest.fixture
def failing_teardown_fixture():
    """Fixture that yields successfully but fails during teardown."""
    yield "value"
    raise RuntimeError("Teardown failure!")


@pytest.fixture
def nested_failing_teardown(failing_teardown_fixture):
    """Nested fixture to test teardown propagation."""
    yield f"nested_{failing_teardown_fixture}"


def test_with_failing_teardown(failing_teardown_fixture):
    """Test passes but fixture teardown fails - should result in ERROR status."""
    assert failing_teardown_fixture == "value"


def test_with_nested_failing_teardown(nested_failing_teardown):
    """Test with nested fixture where inner teardown fails."""
    assert nested_failing_teardown == "nested_value"


@pytest.fixture
def multi_stage_teardown():
    """Fixture with multiple teardown steps, first one fails."""
    resource = {"allocated": True, "cleaned": False}
    yield resource
    # First teardown step fails
    raise ValueError("Multi-stage teardown failure at step 1")
    # This would never run
    resource["cleaned"] = True


def test_multi_stage_teardown_failure(multi_stage_teardown):
    """Test that passes with multi-stage teardown failure."""
    assert multi_stage_teardown["allocated"] is True
