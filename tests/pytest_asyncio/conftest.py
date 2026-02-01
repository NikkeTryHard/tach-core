"""Fixtures for pytest-asyncio tests."""
import pytest
import asyncio


@pytest.fixture
def sync_fixture():
    """Simple sync fixture."""
    return "sync_value"


@pytest.fixture
async def async_fixture():
    """Simple async fixture."""
    await asyncio.sleep(0.001)
    return "async_value"


@pytest.fixture
async def async_generator_fixture():
    """Async generator fixture with setup/teardown."""
    resource = {"status": "initialized"}
    yield resource
    resource["status"] = "cleaned"


@pytest.fixture
async def async_fixture_with_dep(sync_fixture):
    """Async fixture depending on sync fixture."""
    await asyncio.sleep(0.001)
    return f"async_{sync_fixture}"
