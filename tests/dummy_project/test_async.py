"""Async test functions."""

import pytest


async def test_async_basic():
    assert True


async def test_async_with_fixture(event_loop, db):
    """Async test with dependencies."""
    pass


class TestAsyncClass:
    async def test_async_method(self, client):
        pass
