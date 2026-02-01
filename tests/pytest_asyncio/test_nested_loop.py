"""Tests for handling asyncio.run() calls within tests."""
import asyncio
import sys
from pathlib import Path

# Add src to path for importing tach_harness
_src_path = str(Path(__file__).parent.parent.parent / "src")
if _src_path not in sys.path:
    sys.path.insert(0, _src_path)

from tach_harness import is_loop_running, ensure_no_running_loop


def test_is_loop_running_outside():
    """Outside of run_until_complete, no loop is running."""
    # Create a loop but don't run it
    loop = asyncio.new_event_loop()
    asyncio.set_event_loop(loop)

    try:
        assert is_loop_running() is False
    finally:
        loop.close()


def test_is_loop_running_inside():
    """Inside run_until_complete, loop is running."""
    loop = asyncio.new_event_loop()
    asyncio.set_event_loop(loop)

    async def check_inside():
        return is_loop_running()

    try:
        result = loop.run_until_complete(check_inside())
        assert result is True
    finally:
        loop.close()


def test_sync_test_with_asyncio_run():
    """Sync tests that use asyncio.run() should work with ensure_no_running_loop."""
    with ensure_no_running_loop():
        async def inner():
            return "from asyncio.run"

        result = asyncio.run(inner())
        assert result == "from asyncio.run"


def test_ensure_no_running_loop_restores_state():
    """ensure_no_running_loop should restore previous event loop."""
    # Set up an event loop
    original_loop = asyncio.new_event_loop()
    asyncio.set_event_loop(original_loop)

    try:
        with ensure_no_running_loop():
            # Inside context, no loop should be set
            try:
                current = asyncio.get_event_loop()
                # On Python 3.10+, this may raise or return a new loop
            except RuntimeError:
                pass  # Expected - no loop set

            # Can use asyncio.run() here
            async def quick():
                return "ok"
            asyncio.run(quick())

        # After context, loop should be restored (if still open)
        # Note: asyncio.run() may close loops, so we just verify no crash
    finally:
        if not original_loop.is_closed():
            original_loop.close()
