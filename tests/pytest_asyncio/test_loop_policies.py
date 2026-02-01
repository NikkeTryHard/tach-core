"""Tests for custom event loop policies."""
import asyncio
import sys


def test_custom_policy_configuration():
    """Test that EventLoopManager respects custom policies."""
    # Import from the harness module in the worktree
    sys.path.insert(0, "/home/nikketryhard/dev/tach-core/.worktrees/pytest-asyncio-support/src")
    from tach_harness import EventLoopManager

    class CustomPolicy(asyncio.DefaultEventLoopPolicy):
        def new_event_loop(self):
            loop = super().new_event_loop()
            loop._custom_marker = True
            return loop

    EventLoopManager.reset()
    manager = EventLoopManager.get_instance()
    manager.set_policy(CustomPolicy())

    loop = manager.get_loop("test_scope")
    assert hasattr(loop, "_custom_marker")
    assert loop._custom_marker is True

    EventLoopManager.reset()


def test_uvloop_detection():
    """Test uvloop detection returns bool."""
    sys.path.insert(0, "/home/nikketryhard/dev/tach-core/.worktrees/pytest-asyncio-support/src")
    from tach_harness import detect_uvloop

    has_uvloop = detect_uvloop()
    assert isinstance(has_uvloop, bool)


def test_policy_preserved_across_scopes():
    """Test that policy is used for all new loops."""
    sys.path.insert(0, "/home/nikketryhard/dev/tach-core/.worktrees/pytest-asyncio-support/src")
    from tach_harness import EventLoopManager

    class MarkedPolicy(asyncio.DefaultEventLoopPolicy):
        def new_event_loop(self):
            loop = super().new_event_loop()
            loop._policy_marker = "marked"
            return loop

    EventLoopManager.reset()
    manager = EventLoopManager.get_instance()
    manager.set_policy(MarkedPolicy())

    loop1 = manager.get_loop("scope1")
    loop2 = manager.get_loop("scope2")

    assert loop1._policy_marker == "marked"
    assert loop2._policy_marker == "marked"

    EventLoopManager.reset()
