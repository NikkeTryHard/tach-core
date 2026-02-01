"""Tests for async-aware timeout and cancellation."""
import asyncio
import sys
from pathlib import Path

# Add src to path for importing tach_harness
_src_path = str(Path(__file__).parent.parent.parent / "src")
if _src_path not in sys.path:
    sys.path.insert(0, _src_path)

from tach_harness import run_with_timeout


def test_async_timeout_cancels_task():
    """Timeout should cancel the running coroutine gracefully."""
    async def slow_coro():
        await asyncio.sleep(10)
        return "should not reach"

    loop = asyncio.new_event_loop()
    try:
        result, timed_out = run_with_timeout(loop, slow_coro(), timeout=0.1)
        assert timed_out is True
        assert result is None
    finally:
        loop.close()


def test_async_timeout_success():
    """Fast coroutine should complete within timeout."""
    async def fast_coro():
        await asyncio.sleep(0.01)
        return "completed"

    loop = asyncio.new_event_loop()
    try:
        result, timed_out = run_with_timeout(loop, fast_coro(), timeout=1.0)
        assert timed_out is False
        assert result == "completed"
    finally:
        loop.close()


def test_async_timeout_none_means_no_timeout():
    """No timeout should run without time limit."""
    async def quick_coro():
        return "done"

    loop = asyncio.new_event_loop()
    try:
        result, timed_out = run_with_timeout(loop, quick_coro(), timeout=None)
        assert timed_out is False
        assert result == "done"
    finally:
        loop.close()


def test_async_timeout_with_cleanup():
    """Cancelled tasks should have cleanup opportunity."""
    cleanup_called = []

    async def coro_with_cleanup():
        try:
            await asyncio.sleep(10)
        except asyncio.CancelledError:
            cleanup_called.append(True)
            raise

    loop = asyncio.new_event_loop()
    try:
        run_with_timeout(loop, coro_with_cleanup(), timeout=0.1)
        assert len(cleanup_called) == 1
    finally:
        loop.close()


def test_async_timeout_coro_returns_none():
    """Coroutine returning None should not be treated as timeout."""
    async def returns_none():
        return None

    loop = asyncio.new_event_loop()
    try:
        result, timed_out = run_with_timeout(loop, returns_none(), timeout=1.0)
        assert timed_out is False
        assert result is None
    finally:
        loop.close()
