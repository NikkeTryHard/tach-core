"""Asyncio.gather patterns for tach-core.

This module demonstrates concurrent async execution patterns using
asyncio.gather, asyncio.create_task, and related patterns.
"""

import asyncio

import pytest


async def fetch_url(url: str) -> dict:
    """Simulates fetching a URL asynchronously."""
    await asyncio.sleep(0.001)  # Simulate network delay
    return {"url": url, "status": 200, "data": f"content_from_{url}"}


async def process_item(item: str) -> str:
    """Process a single item asynchronously."""
    await asyncio.sleep(0.001)
    return item.upper()


async def compute_value(n: int) -> int:
    """Compute a value asynchronously."""
    await asyncio.sleep(0.001)
    return n * n


async def async_wrapper(n: int) -> int:
    """Helper that returns value after async sleep."""
    await asyncio.sleep(0.001)
    return n


@pytest.mark.asyncio
async def test_gather_basic():
    """Test basic asyncio.gather usage."""
    results = await asyncio.gather(
        async_wrapper(1),
        async_wrapper(2),
        async_wrapper(3),
    )
    assert results == [1, 2, 3]


@pytest.mark.asyncio
async def test_gather_multiple_urls():
    """Test gathering results from multiple URL fetches."""
    urls = ["http://example.com", "http://test.com", "http://api.com"]

    results = await asyncio.gather(*[fetch_url(url) for url in urls])

    assert len(results) == 3
    for i, result in enumerate(results):
        assert result["url"] == urls[i]
        assert result["status"] == 200


@pytest.mark.asyncio
async def test_gather_with_processing():
    """Test gathering processed items."""
    items = ["apple", "banana", "cherry"]

    results = await asyncio.gather(*[process_item(item) for item in items])

    assert results == ["APPLE", "BANANA", "CHERRY"]


@pytest.mark.asyncio
async def test_gather_with_computations():
    """Test gathering computed values."""
    numbers = [1, 2, 3, 4, 5]

    squares = await asyncio.gather(*[compute_value(n) for n in numbers])

    assert squares == [1, 4, 9, 16, 25]


@pytest.mark.asyncio
async def test_create_task_basic():
    """Test asyncio.create_task for concurrent execution."""
    task1 = asyncio.create_task(compute_value(5))
    task2 = asyncio.create_task(compute_value(10))

    result1 = await task1
    result2 = await task2

    assert result1 == 25
    assert result2 == 100


@pytest.mark.asyncio
async def test_gather_return_exceptions():
    """Test gather with return_exceptions=True."""

    async def success():
        await asyncio.sleep(0.001)
        return "ok"

    async def failure():
        await asyncio.sleep(0.001)
        raise ValueError("failed")

    results = await asyncio.gather(
        success(),
        failure(),
        success(),
        return_exceptions=True,
    )

    assert results[0] == "ok"
    assert isinstance(results[1], ValueError)
    assert results[2] == "ok"


@pytest.mark.asyncio
async def test_as_completed():
    """Test asyncio.as_completed for processing results as they arrive."""

    async def delayed_value(n: int, delay: float) -> int:
        await asyncio.sleep(delay)
        return n

    tasks = [
        delayed_value(3, 0.003),
        delayed_value(1, 0.001),
        delayed_value(2, 0.002),
    ]

    results = []
    for coro in asyncio.as_completed(tasks):
        result = await coro
        results.append(result)

    # Results arrive in order of completion (shortest delay first)
    assert sorted(results) == [1, 2, 3]


@pytest.mark.asyncio
async def test_wait_for_with_timeout():
    """Test asyncio.wait_for with timeout."""

    async def quick_operation():
        await asyncio.sleep(0.001)
        return "done"

    result = await asyncio.wait_for(quick_operation(), timeout=1.0)
    assert result == "done"


@pytest.mark.asyncio
async def test_semaphore_limited_concurrency():
    """Test limiting concurrency with asyncio.Semaphore."""
    semaphore = asyncio.Semaphore(2)  # Max 2 concurrent operations
    active_count = 0
    max_active = 0

    async def limited_operation(n: int) -> int:
        nonlocal active_count, max_active
        async with semaphore:
            active_count += 1
            max_active = max(max_active, active_count)
            await asyncio.sleep(0.001)
            active_count -= 1
            return n

    results = await asyncio.gather(*[limited_operation(i) for i in range(5)])

    assert results == [0, 1, 2, 3, 4]
    assert max_active <= 2  # Never more than 2 concurrent
