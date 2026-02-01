"""Tests for TaskGroup and gather cleanup."""
import asyncio
import sys
from pathlib import Path

# Add src to path for importing tach_harness
_src_path = str(Path(__file__).parent.parent.parent / "src")
if _src_path not in sys.path:
    sys.path.insert(0, _src_path)

from tach_harness import cleanup_pending_tasks


def test_cleanup_pending_tasks_basic():
    """cleanup_pending_tasks should cancel all running tasks."""
    loop = asyncio.new_event_loop()
    cancelled = []

    async def long_running():
        try:
            await asyncio.sleep(100)
        except asyncio.CancelledError:
            cancelled.append(True)
            raise

    async def start_tasks():
        t1 = asyncio.create_task(long_running())
        t2 = asyncio.create_task(long_running())
        await asyncio.sleep(0.01)  # Let tasks start
        return t1, t2

    loop.run_until_complete(start_tasks())

    # Cleanup should cancel both tasks
    count = cleanup_pending_tasks(loop)
    assert count == 2
    assert len(cancelled) == 2

    loop.close()


def test_cleanup_pending_tasks_empty():
    """cleanup_pending_tasks should handle empty task list."""
    loop = asyncio.new_event_loop()

    count = cleanup_pending_tasks(loop)
    assert count == 0

    loop.close()


def test_cleanup_pending_tasks_gather_pattern():
    """cleanup_pending_tasks should work with gather pattern."""
    loop = asyncio.new_event_loop()
    cleanup_order = []

    async def worker(name: str):
        try:
            await asyncio.sleep(100)
        except asyncio.CancelledError:
            cleanup_order.append(name)
            raise

    async def start_gathered():
        # Create tasks but don't await the gather
        asyncio.create_task(worker("a"))
        asyncio.create_task(worker("b"))
        asyncio.create_task(worker("c"))
        await asyncio.sleep(0.01)

    loop.run_until_complete(start_gathered())

    count = cleanup_pending_tasks(loop)
    assert count == 3
    assert set(cleanup_order) == {"a", "b", "c"}

    loop.close()


def test_cleanup_with_exception_in_task():
    """cleanup_pending_tasks should handle tasks that raise on cancel."""
    loop = asyncio.new_event_loop()

    async def failing_task():
        try:
            await asyncio.sleep(100)
        except asyncio.CancelledError:
            raise ValueError("cleanup error")

    async def start_task():
        asyncio.create_task(failing_task())
        await asyncio.sleep(0.01)

    loop.run_until_complete(start_task())

    # Should not raise, even if task raises during cancel
    count = cleanup_pending_tasks(loop)
    assert count == 1

    loop.close()
