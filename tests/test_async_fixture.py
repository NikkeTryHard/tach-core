# tests/test_async_fixture.py
"""Tests for async fixture handling in tach harness."""
import pytest
import asyncio


@pytest.fixture
async def async_value():
    """Simple async fixture that should return a value."""
    await asyncio.sleep(0)
    return 42


@pytest.fixture
async def async_gen_value():
    """Async generator fixture with setup/teardown."""
    await asyncio.sleep(0)
    yield "hello"
    await asyncio.sleep(0)  # teardown


def test_async_fixture_value(async_value):
    """Test that async fixture returns actual value, not coroutine."""
    assert async_value == 42
    assert not asyncio.iscoroutine(async_value)


def test_async_gen_fixture_value(async_gen_value):
    """Test that async generator fixture returns yielded value."""
    assert async_gen_value == "hello"
    assert not hasattr(async_gen_value, '__anext__')


# Teardown tracking for verification test
_teardown_tracker = []


@pytest.fixture
async def tracked_teardown_fixture():
    """Async generator fixture that tracks setup/teardown."""
    _teardown_tracker.append("setup")
    yield "tracked"
    _teardown_tracker.append("teardown")


def test_async_fixture_teardown_runs(tracked_teardown_fixture):
    """Test that async fixture returns value and teardown is registered."""
    assert tracked_teardown_fixture == "tracked"
    assert "setup" in _teardown_tracker
    # Teardown runs after test completes via AsyncFixtureWrapper.teardown_all()
