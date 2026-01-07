"""Async test patterns for tach-core.

This module demonstrates testing async functions with pytest.
Tach-core has built-in asyncio loop management for coroutine tests.
"""

import asyncio

import pytest


async def async_add(a: int, b: int) -> int:
    """An async function that adds two numbers."""
    await asyncio.sleep(0.001)  # Simulate async operation
    return a + b


async def async_fetch_data(key: str) -> dict:
    """Simulates an async data fetch operation."""
    await asyncio.sleep(0.001)
    return {"key": key, "value": f"data_for_{key}"}


async def async_process_items(items: list) -> list:
    """Process items asynchronously."""
    results = []
    for item in items:
        await asyncio.sleep(0.001)
        results.append(item * 2)
    return results


@pytest.mark.asyncio
async def test_simple_async():
    """Test a simple async function."""
    result = await async_add(2, 3)
    assert result == 5


@pytest.mark.asyncio
async def test_async_fetch():
    """Test async data fetching."""
    data = await async_fetch_data("test_key")
    assert data["key"] == "test_key"
    assert data["value"] == "data_for_test_key"


@pytest.mark.asyncio
async def test_async_processing():
    """Test async processing of multiple items."""
    items = [1, 2, 3, 4, 5]
    results = await async_process_items(items)
    assert results == [2, 4, 6, 8, 10]


@pytest.mark.asyncio
async def test_multiple_awaits():
    """Test function with multiple await calls."""
    result1 = await async_add(1, 2)
    result2 = await async_add(3, 4)
    result3 = await async_add(result1, result2)
    assert result3 == 10  # (1+2) + (3+4) = 10


@pytest.mark.asyncio
async def test_async_exception():
    """Test that async exceptions are handled correctly."""

    async def failing_async():
        await asyncio.sleep(0.001)
        raise ValueError("Expected error")

    with pytest.raises(ValueError, match="Expected error"):
        await failing_async()


@pytest.mark.asyncio
async def test_async_timeout():
    """Test async operation with timeout."""

    async def slow_operation():
        await asyncio.sleep(0.01)
        return "completed"

    result = await asyncio.wait_for(slow_operation(), timeout=1.0)
    assert result == "completed"
