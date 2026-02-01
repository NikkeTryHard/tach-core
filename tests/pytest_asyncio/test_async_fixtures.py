"""Tests for async fixture support."""
import pytest
import asyncio


@pytest.mark.asyncio
class TestAsyncFixtures:
    """Tests for async fixture support."""

    async def test_simple_async_fixture(self, async_fixture):
        """Test that async fixtures are awaited correctly."""
        assert async_fixture == "async_value"

    async def test_async_generator_fixture(self, async_generator_fixture):
        """Test async generator fixtures with teardown."""
        assert async_generator_fixture["status"] == "initialized"

    async def test_mixed_fixtures(self, sync_fixture, async_fixture):
        """Test mixing sync and async fixtures."""
        assert sync_fixture == "sync_value"
        assert async_fixture == "async_value"

    async def test_async_fixture_chain(self, async_fixture_with_dep):
        """Test async fixture depending on sync fixture."""
        assert async_fixture_with_dep == "async_sync_value"
