"""Tests for loop scope functionality."""
import pytest
import asyncio

_loop_ids: dict[str, int] = {}


@pytest.mark.asyncio(loop_scope="class")
class TestClassScopedLoop:
    """All tests in this class should share the same event loop."""

    async def test_first(self):
        loop = asyncio.get_running_loop()
        _loop_ids["class_first"] = id(loop)
        assert loop is not None

    async def test_second(self):
        loop = asyncio.get_running_loop()
        _loop_ids["class_second"] = id(loop)
        # Should be same loop as first test
        assert _loop_ids.get("class_first") == id(loop)


@pytest.mark.asyncio
class TestFunctionScopedLoop:
    """Each test gets its own event loop."""

    async def test_first(self):
        loop = asyncio.get_running_loop()
        _loop_ids["func_first"] = id(loop)
        assert loop is not None

    async def test_second(self):
        loop = asyncio.get_running_loop()
        # Should be different loop than first test
        assert _loop_ids.get("func_first") != id(loop)


@pytest.mark.asyncio
async def test_default_scope():
    """Test without explicit scope uses function scope."""
    loop = asyncio.get_running_loop()
    assert loop is not None
