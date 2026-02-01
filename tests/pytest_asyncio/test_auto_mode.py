"""Tests for automatic async test detection mode."""
import asyncio
import sys
from pathlib import Path

# Add src to path for importing tach_harness
_src_path = str(Path(__file__).parent.parent.parent / "src")
if _src_path not in sys.path:
    sys.path.insert(0, _src_path)

from tach_harness import EventLoopManager, apply_cached_effects, EFFECT_TYPE_ASYNCIO_SETUP


def test_auto_mode_enables_async_detection():
    """When auto_mode=True, async tests run without explicit marker."""
    EventLoopManager.reset()
    manager = EventLoopManager.get_instance()
    manager.configure(loop_scope="function", auto_mode=True)

    assert manager.auto_mode is True
    assert manager.should_run_async(is_coro=True, has_marker=False) is True

    EventLoopManager.reset()


def test_auto_mode_disabled_requires_marker():
    """When auto_mode=False, async tests need explicit marker."""
    EventLoopManager.reset()
    manager = EventLoopManager.get_instance()
    manager.configure(loop_scope="function", auto_mode=False)

    assert manager.auto_mode is False
    # Without marker and without auto_mode, should not auto-run as async
    assert manager.should_run_async(is_coro=True, has_marker=False) is False
    # With marker, should run as async
    assert manager.should_run_async(is_coro=True, has_marker=True) is True

    EventLoopManager.reset()


def test_sync_function_never_async():
    """Sync functions should never run as async."""
    EventLoopManager.reset()
    manager = EventLoopManager.get_instance()
    manager.configure(loop_scope="function", auto_mode=True)

    # Even with auto_mode, sync functions don't run as async
    assert manager.should_run_async(is_coro=False, has_marker=False) is False
    assert manager.should_run_async(is_coro=False, has_marker=True) is False

    EventLoopManager.reset()


def test_auto_mode_from_effect():
    """Auto mode should be configurable via AsyncioSetup effect."""
    EventLoopManager.reset()

    effects = [
        {"type": EFFECT_TYPE_ASYNCIO_SETUP, "loop_scope": "module", "auto_mode": True}
    ]

    applied = apply_cached_effects(effects)
    assert applied >= 1

    manager = EventLoopManager.get_instance()
    assert manager.auto_mode is True
    assert manager.current_scope == "module"

    EventLoopManager.reset()
