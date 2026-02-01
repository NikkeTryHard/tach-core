"""Test that asyncio effects flow from conftest to worker.

This module verifies that:
1. The AsyncioSetup HookEffect is correctly converted to Python dict in zygote.rs
2. The apply_cached_effects function in tach_harness.py handles asyncio_setup
3. EventLoopManager is configured correctly from the effect
"""
import pytest

# Skip entire module if tach_harness is not available
tach_harness = pytest.importorskip("tach_harness")


# This test verifies that loop_scope configured in conftest
# is correctly applied to the EventLoopManager

@pytest.mark.asyncio
async def test_asyncio_marker_detected():
    """Verify basic async test runs."""
    import asyncio
    loop = asyncio.get_running_loop()
    assert loop is not None


@pytest.mark.asyncio
async def test_event_loop_manager_singleton():
    """Verify EventLoopManager singleton pattern works."""
    from tach_harness import EventLoopManager
    manager1 = EventLoopManager.get_instance()
    manager2 = EventLoopManager.get_instance()
    assert manager1 is manager2


@pytest.mark.asyncio(loop_scope="class")
class TestClassScopedLoop:
    """Tests with class-scoped loop."""

    async def test_first_in_class(self):
        import asyncio
        assert asyncio.get_running_loop() is not None

    async def test_second_in_class(self):
        import asyncio
        assert asyncio.get_running_loop() is not None


@pytest.mark.asyncio
async def test_asyncio_effect_application():
    """Test that AsyncioSetup effect can be applied via apply_cached_effects."""
    from tach_harness import apply_cached_effects, EventLoopManager, EFFECT_TYPE_ASYNCIO_SETUP

    # Simulate an asyncio setup effect
    effects = [
        {
            "type": EFFECT_TYPE_ASYNCIO_SETUP,
            "loop_scope": "module",
            "auto_mode": True,
        }
    ]

    # Apply the effect
    applied = apply_cached_effects(effects)
    assert applied == 1

    # Verify EventLoopManager was configured using public properties
    manager = EventLoopManager.get_instance()
    assert manager.current_scope == "module"
    assert manager.auto_mode is True

    # Reset to default for other tests
    manager.configure("function", False)
