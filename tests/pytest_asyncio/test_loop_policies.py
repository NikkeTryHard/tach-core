"""Tests for custom event loop policies."""
import asyncio
import sys
from pathlib import Path

# Add src to path for importing tach_harness
_src_path = str(Path(__file__).parent.parent.parent / "src")
if _src_path not in sys.path:
    sys.path.insert(0, _src_path)

from tach_harness import EventLoopManager, detect_uvloop, get_uvloop_policy


def test_custom_policy_configuration():
    """Test that EventLoopManager respects custom policies."""

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
    has_uvloop = detect_uvloop()
    assert isinstance(has_uvloop, bool)


def test_get_uvloop_policy():
    """Test get_uvloop_policy returns policy or None."""
    policy = get_uvloop_policy()
    # Should return either a policy object or None
    assert policy is None or isinstance(policy, asyncio.AbstractEventLoopPolicy)


def test_policy_preserved_across_scopes():
    """Test that policy is used for all new loops."""

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
