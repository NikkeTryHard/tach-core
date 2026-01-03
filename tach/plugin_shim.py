"""
Shadow Plugin Shim: Hook Interception for pytest Plugin Compatibility

This module implements the "Recording Phase" of the Shadow Plugin pattern.
It wraps pytest hooks to capture plugin effects without executing them in workers.

Architecture:
    Parent Process (Supervisor):
        1. Load EffectRecorder as a pytest plugin
        2. EffectRecorder wraps pytest_runtest_setup/teardown
        3. Capture environment deltas, marker additions, etc.
        4. Store EffectPacks in EffectCache

    Worker Process:
        1. Receive EffectPack via IPC
        2. EffectApplicator.apply() replays effects
        3. Run test in isolated environment

The "Dirty Record" Protection:
    After each test's effects are recorded, we restore the parent's environment
    to its pre-test state. This prevents Test A's pollution from affecting
    Test B's recording.

Usage:
    # In pytest configuration (conftest.py or plugin):
    from tach.plugin_shim import EffectRecorder

    def pytest_configure(config):
        recorder = EffectRecorder()
        config.pluginmanager.register(recorder, "tach_effect_recorder")

    # After test setup, retrieve effects:
    effect_pack = recorder.get_effects("tests/test_foo.py::test_bar")
"""

from __future__ import annotations

import os
import time
from typing import Any, Dict, List, Optional

# Import from effects module
from .effects import (
    EffectPack,
    EnvironmentEffect,
    MarkerEffect,
    EnvironmentSnapshot,
    EffectCache,
    compute_env_delta,
)


class EffectRecorder:
    """
    pytest plugin that records effects during test setup.

    This is the "Recording Phase" of the Shadow Plugin Shim.
    Register this as a pytest plugin to capture what other plugins do.

    Attributes:
        cache: EffectCache storing recorded effect packs
        _current_test: Currently recording test_id
        _env_snapshot: Environment snapshot for delta computation
        _active: Whether recording is active
    """

    def __init__(self):
        self.cache = EffectCache()
        self._current_test: Optional[str] = None
        self._env_snapshot: Optional[EnvironmentSnapshot] = None
        self._active = True
        self._plugin_order: List[str] = []

    def activate(self) -> None:
        """Enable effect recording."""
        self._active = True

    def deactivate(self) -> None:
        """Disable effect recording (e.g., in worker processes)."""
        self._active = False

    def get_effects(self, test_id: str) -> Optional[EffectPack]:
        """Retrieve recorded effects for a test."""
        return self.cache.get(test_id)

    def clear(self) -> None:
        """Clear all recorded effects."""
        self.cache.clear()

    # =========================================================================
    # pytest Hook Implementations
    # =========================================================================

    def pytest_runtest_setup(self, item: Any) -> None:
        """
        Called before each test's setup phase.

        This is a hookwrapper that:
        1. Captures environment state BEFORE other plugins run
        2. Yields to let other plugins execute
        3. Captures the delta AFTER plugins have run
        4. Stores the EffectPack
        5. Restores environment for the next test (Dirty Record protection)
        """
        if not self._active:
            return

        # Get test identifier
        self._current_test = item.nodeid
        self._plugin_order = []

        # Capture environment BEFORE other plugins
        self._env_snapshot = EnvironmentSnapshot.capture()

    def pytest_runtest_setup_post(self, item: Any) -> None:
        """
        Called after all pytest_runtest_setup hooks have completed.

        This is where we capture the delta and create the EffectPack.
        Note: This requires using hookwrapper or a custom hook caller.
        """
        if not self._active or self._env_snapshot is None:
            return

        # Compute environment delta
        env_effects = self._env_snapshot.compute_delta()

        # Create EffectPack
        pack = EffectPack(
            test_id=self._current_test or "unknown",
            effects=list(env_effects),  # type: ignore
            plugin_order=self._plugin_order.copy(),
            timestamp=time.time(),
        )

        # TODO: Capture marker effects from item.iter_markers()
        # This requires comparing markers before/after plugin hooks

        # Store in cache
        self.cache.store(pack)

        # Dirty Record Protection: Restore environment
        self._env_snapshot.restore()

        # Clean up
        self._current_test = None
        self._env_snapshot = None

    def pytest_runtest_teardown(self, item: Any) -> None:
        """
        Called during test teardown.

        Currently a no-op, but could be extended to capture teardown effects.
        """
        pass


class EffectApplicator:
    """
    Applies effects in worker processes.

    This is the "Replay Phase" of the Shadow Plugin Shim.
    Workers use this to apply effects without calling original plugins.

    Usage:
        applicator = EffectApplicator()
        pack = EffectPack.from_json(received_json)
        applicator.apply(pack)
        # Now environment matches what plugins would have done
    """

    def __init__(self):
        self._applied_count = 0

    def apply(self, pack: EffectPack, item: Optional[Any] = None) -> int:
        """
        Apply all effects from an EffectPack.

        Args:
            pack: The EffectPack to apply
            item: pytest.Item for marker effects (optional)

        Returns:
            Number of effects applied
        """
        self._applied_count = pack.apply_all(item)
        return self._applied_count

    def reset_environment(self, pack: EffectPack) -> None:
        """
        Reset environment by undoing effects.

        This is used between tests in the same worker.
        """
        for effect in pack.effects:
            if isinstance(effect, EnvironmentEffect):
                # Undo: delete if was set, restore if was unset
                if effect.action == "set":
                    os.environ.pop(effect.key, None)
                # Note: We can't restore unset vars without knowing original value

    @property
    def applied_count(self) -> int:
        """Number of effects applied in the last apply() call."""
        return self._applied_count


# =============================================================================
# hookwrapper implementation for full hook interception
# =============================================================================


def create_recording_plugin() -> Dict[str, Any]:
    """
    Create a pytest plugin dict with hookwrapper for effect recording.

    This is an alternative to the class-based EffectRecorder that uses
    pytest's hookimpl decorator for proper hook wrapping.

    Returns:
        Plugin dict suitable for config.pluginmanager.register()
    """
    import pytest

    recorder_state = {
        "cache": EffectCache(),
        "env_snapshot": None,
        "current_test": None,
        "active": True,
    }

    @pytest.hookimpl(hookwrapper=True)
    def pytest_runtest_setup(item):
        """Wrapped hook that captures effects before and after other plugins."""
        if not recorder_state["active"]:
            yield
            return

        # BEFORE: Capture initial state
        recorder_state["current_test"] = item.nodeid
        recorder_state["env_snapshot"] = EnvironmentSnapshot.capture()

        # Let other plugins run
        yield

        # AFTER: Capture delta and store
        if recorder_state["env_snapshot"] is not None:
            env_effects = recorder_state["env_snapshot"].compute_delta()

            pack = EffectPack(
                test_id=item.nodeid,
                effects=list(env_effects),  # type: ignore
                timestamp=time.time(),
            )

            recorder_state["cache"].store(pack)

            # Dirty Record Protection
            recorder_state["env_snapshot"].restore()

            # Clean up
            recorder_state["env_snapshot"] = None
            recorder_state["current_test"] = None

    return {
        "pytest_runtest_setup": pytest_runtest_setup,
        "_recorder_state": recorder_state,
    }
