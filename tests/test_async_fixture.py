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
