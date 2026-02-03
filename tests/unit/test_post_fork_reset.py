"""Tests for post-fork event loop reset."""
import sys
from pathlib import Path

# Add src to path for imports
sys.path.insert(0, str(Path(__file__).parent.parent.parent / "src"))


def test_post_fork_init_resets_event_loop_manager():
    """post_fork_init should reset EventLoopManager to clear stale Zygote state."""
    from tach_harness import EventLoopManager, post_fork_init
    import asyncio

    # Simulate Zygote state: create a loop in the manager
    manager = EventLoopManager.get_instance()
    loop = manager.get_loop("session")
    assert loop is not None
    assert "session" in manager._loops

    # Simulate fork by calling post_fork_init
    # This should reset the manager, clearing stale loops
    post_fork_init()

    # After reset, the manager should have no loops
    fresh_manager = EventLoopManager.get_instance()
    assert len(fresh_manager._loops) == 0, "post_fork_init should reset EventLoopManager"


def test_post_fork_init_clears_async_fixture_wrapper():
    """post_fork_init should also reset AsyncFixtureWrapper state."""
    from tach_harness import AsyncFixtureWrapper, post_fork_init
    import asyncio

    # Simulate stale fixture state from Zygote
    AsyncFixtureWrapper._consumed_by_scope["function"].add("fake_fixture")
    assert "fake_fixture" in AsyncFixtureWrapper._consumed_by_scope["function"]

    # post_fork_init should clear this
    post_fork_init()

    assert "fake_fixture" not in AsyncFixtureWrapper._consumed_by_scope["function"], \
        "post_fork_init should reset AsyncFixtureWrapper state"
