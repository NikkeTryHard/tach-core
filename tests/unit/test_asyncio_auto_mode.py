"""Tests for asyncio auto_mode handling in harness."""
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent.parent.parent / "src"))


def test_apply_asyncio_setup_effect_auto_mode():
    """ASYNCIO_SETUP effect with auto_mode=True should configure EventLoopManager."""
    from tach_harness import apply_cached_effects, EventLoopManager, EFFECT_TYPE_ASYNCIO_SETUP

    # Reset manager state
    EventLoopManager.reset()

    effects = [
        {
            "type": EFFECT_TYPE_ASYNCIO_SETUP,
            "loop_scope": "function",
            "auto_mode": True,
        }
    ]

    applied = apply_cached_effects(effects)

    assert applied == 1, "Should apply 1 effect"

    manager = EventLoopManager.get_instance()
    assert manager.auto_mode is True, "auto_mode should be True"
    assert manager.current_scope == "function", "loop_scope should be function"


def test_apply_asyncio_setup_effect_strict_mode():
    """ASYNCIO_SETUP effect with auto_mode=False should not enable auto detection."""
    from tach_harness import apply_cached_effects, EventLoopManager, EFFECT_TYPE_ASYNCIO_SETUP

    # Reset manager state
    EventLoopManager.reset()

    effects = [
        {
            "type": EFFECT_TYPE_ASYNCIO_SETUP,
            "loop_scope": "module",
            "auto_mode": False,
        }
    ]

    applied = apply_cached_effects(effects)

    assert applied == 1, "Should apply 1 effect"

    manager = EventLoopManager.get_instance()
    assert manager.auto_mode is False, "auto_mode should be False"
    assert manager.current_scope == "module", "loop_scope should be module"


def test_should_run_async_respects_auto_mode():
    """With auto_mode=True, coroutines should run as async even without marker."""
    from tach_harness import EventLoopManager

    EventLoopManager.reset()
    manager = EventLoopManager.get_instance()
    manager.configure(loop_scope="function", auto_mode=True)

    # Coroutine without marker should run as async in auto mode
    assert manager.should_run_async(is_coro=True, has_marker=False) is True

    # Non-coroutine should not run as async
    assert manager.should_run_async(is_coro=False, has_marker=False) is False


def test_should_run_async_strict_mode_requires_marker():
    """With auto_mode=False (strict), coroutines need marker to run as async."""
    from tach_harness import EventLoopManager

    EventLoopManager.reset()
    manager = EventLoopManager.get_instance()
    manager.configure(loop_scope="function", auto_mode=False)

    # Coroutine without marker should NOT run as async in strict mode
    assert manager.should_run_async(is_coro=True, has_marker=False) is False

    # Coroutine WITH marker should run as async
    assert manager.should_run_async(is_coro=True, has_marker=True) is True
