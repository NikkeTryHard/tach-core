"""Tests for event loop scope cleanup (Issue #43)."""
import asyncio
import pytest


class TestScopeCleanup:
    """Test that event loops are properly cleaned up at scope boundaries."""

    def test_class_scope_tracking(self):
        """Verify EventLoopManager tracks previous scope for transitions."""
        import sys
        sys.path.insert(0, 'src')
        from tach_harness import EventLoopManager

        mgr = EventLoopManager()
        mgr.configure(loop_scope="class")

        assert hasattr(mgr, '_previous_module')
        assert hasattr(mgr, '_previous_class')

        mgr.close_all()

    def test_module_transition_closes_old_loop(self):
        """Verify module transition closes previous module's loop."""
        import sys
        sys.path.insert(0, 'src')
        from tach_harness import EventLoopManager

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
        import sys
        sys.path.insert(0, 'src')
        from tach_harness import EventLoopManager

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
        import sys
        sys.path.insert(0, 'src')
        from tach_harness import EventLoopManager

        mgr = EventLoopManager.get_instance()
        mgr.configure(loop_scope="module")
        mgr.on_scope_transition("/path/a.py", None)

        assert mgr._previous_module == "/path/a.py"

        EventLoopManager.reset()

        # Get new instance
        mgr2 = EventLoopManager.get_instance()
        assert mgr2._previous_module is None

        EventLoopManager.reset()
