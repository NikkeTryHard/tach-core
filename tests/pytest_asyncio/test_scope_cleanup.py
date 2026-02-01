"""Tests for event loop scope cleanup (Issue #43)."""
import asyncio
import sys

import pytest

# Add src to path once at module level
sys.path.insert(0, 'src')
from tach_harness import EventLoopManager


@pytest.fixture(autouse=True)
def reset_event_loop_manager():
    """Reset EventLoopManager singleton before and after each test."""
    EventLoopManager.reset()
    yield
    EventLoopManager.reset()


class TestScopeCleanup:
    """Test that event loops are properly cleaned up at scope boundaries."""

    def test_class_scope_tracking(self):
        """Verify EventLoopManager tracks previous scope for transitions."""
        mgr = EventLoopManager()
        mgr.configure(loop_scope="class")

        assert hasattr(mgr, '_previous_module')
        assert hasattr(mgr, '_previous_class')

        mgr.close_all()

    def test_module_transition_closes_old_loop(self):
        """Verify module transition closes previous module's loop."""
        mgr = EventLoopManager()
        mgr.configure(loop_scope="module")

        # First transition sets up module_a as "previous"
        mgr.on_scope_transition(
            current_module="/path/to/module_a.py",
            current_class=None
        )

        loop_a = mgr.get_loop("module:/path/to/module_a.py")
        assert not loop_a.is_closed()

        # Second transition to module_b should close module_a's loop
        mgr.on_scope_transition(
            current_module="/path/to/module_b.py",
            current_class=None
        )

        assert loop_a.is_closed()
        mgr.close_all()

    def test_class_transition_closes_old_loop(self):
        """Verify class transition closes previous class's loop."""
        mgr = EventLoopManager()
        mgr.configure(loop_scope="class")

        # First transition sets up ClassA as "previous"
        mgr.on_scope_transition(
            current_module="/path/to/module.py",
            current_class="test_module.ClassA"
        )

        loop_a = mgr.get_loop("class:test_module.ClassA")
        assert not loop_a.is_closed()

        # Second transition to ClassB should close ClassA's loop
        mgr.on_scope_transition(
            current_module="/path/to/module.py",
            current_class="test_module.ClassB"
        )

        assert loop_a.is_closed()
        mgr.close_all()

    def test_reset_clears_tracking(self):
        """Verify reset clears scope tracking state."""
        mgr = EventLoopManager.get_instance()
        mgr.configure(loop_scope="module")
        mgr.on_scope_transition("/path/a.py", None)

        assert mgr._previous_module == "/path/a.py"

        EventLoopManager.reset()

        # Get new instance
        mgr2 = EventLoopManager.get_instance()
        assert mgr2._previous_module is None

    def test_session_scope_not_closed_on_module_transition(self):
        """Verify session-scoped loops are NOT closed when modules change."""
        mgr = EventLoopManager()
        mgr.configure(loop_scope="session")

        session_loop = mgr.get_loop("session")
        assert not session_loop.is_closed()

        # Module transition should NOT close session loop
        mgr.on_scope_transition(
            current_module="/path/to/module_a.py",
            current_class=None
        )
        mgr.on_scope_transition(
            current_module="/path/to/module_b.py",
            current_class=None
        )

        assert not session_loop.is_closed()
        mgr.close_all()


class TestResetWorkerState:
    """Test that reset_worker_state cleans up event loops."""

    def test_reset_worker_state_cleans_event_loops(self, monkeypatch):
        """Verify reset_worker_state invokes EventLoopManager.reset()."""
        import tach_harness

        # Create a session-scoped loop
        mgr = EventLoopManager.get_instance()
        mgr.configure(loop_scope="session")
        loop = mgr.get_loop("session")
        assert not loop.is_closed()

        # Mock tach_rust to avoid import error
        class MockTachRust:
            @staticmethod
            def reset_memory():
                pass

        monkeypatch.setitem(sys.modules, 'tach_rust', MockTachRust)

        result = tach_harness.reset_worker_state()

        # After reset, the session loop should be closed
        assert loop.is_closed()
        assert result is True
