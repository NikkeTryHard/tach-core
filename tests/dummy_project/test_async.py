"""Async test functions for Tach native async support.

These tests verify that Tach can run async def tests WITHOUT:
- @pytest.mark.asyncio decorators
- pytest-asyncio plugin

Tach handles the event loop natively (Batteries-Included philosophy).
"""


async def test_async_basic():
    """Pure async test with no dependencies."""
    assert True


async def test_async_with_fixture(db):
    """Async test with sync fixture dependency.

    Note: We only support sync fixtures. Async fixtures are out of scope
    for Task 3.1 and require complex loop scope management.
    """
    # db is a sync fixture from conftest.py
    assert db == "database_connection"


class TestAsyncClass:
    async def test_async_method(self, client):
        """Async method test with fixture dependency chain (client -> db)."""
        assert client == "client_using_database_connection"
