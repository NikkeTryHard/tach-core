"""Unit tests for hook effect recording functions in tach_harness.

These tests verify the Python-side effect recording infrastructure.
"""

import os
import sys


class TestEffectDelta:
    """Test effect delta computation functions."""

    def test_a_compute_env_delta_new_var(self):
        """Test that new environment variables are detected."""
        # Import the harness functions
        sys.path.insert(0, "/home/louiskaneko/dev/tach-core/.worktrees/v0.2.0-hooks/src")
        from tach_harness import _compute_env_delta

        before = {"EXISTING": "value"}
        after = {"EXISTING": "value", "NEW_VAR": "new_value"}

        effects = _compute_env_delta(before, after)

        assert len(effects) == 1
        assert effects[0]["type"] == "SetEnv"
        assert effects[0]["key"] == "NEW_VAR"
        assert effects[0]["value"] == "new_value"

    def test_b_compute_env_delta_changed_var(self):
        """Test that changed environment variables are detected."""
        sys.path.insert(0, "/home/louiskaneko/dev/tach-core/.worktrees/v0.2.0-hooks/src")
        from tach_harness import _compute_env_delta

        before = {"EXISTING": "old_value"}
        after = {"EXISTING": "new_value"}

        effects = _compute_env_delta(before, after)

        assert len(effects) == 1
        assert effects[0]["type"] == "SetEnv"
        assert effects[0]["key"] == "EXISTING"
        assert effects[0]["value"] == "new_value"

    def test_c_compute_env_delta_no_change(self):
        """Test that unchanged environment produces no effects."""
        sys.path.insert(0, "/home/louiskaneko/dev/tach-core/.worktrees/v0.2.0-hooks/src")
        from tach_harness import _compute_env_delta

        before = {"EXISTING": "value"}
        after = {"EXISTING": "value"}

        effects = _compute_env_delta(before, after)

        assert len(effects) == 0

    def test_d_compute_sys_path_delta_added(self):
        """Test that new sys.path entries are detected."""
        sys.path.insert(0, "/home/louiskaneko/dev/tach-core/.worktrees/v0.2.0-hooks/src")
        from tach_harness import _compute_sys_path_delta

        before = ["/existing/path"]
        after = ["/existing/path", "/new/path"]

        effects = _compute_sys_path_delta(before, after)

        assert len(effects) == 1
        assert effects[0]["type"] == "ModifySysPath"
        assert effects[0]["path"] == "/new/path"
        assert effects[0]["action"] in ("append", "prepend")

    def test_e_compute_sys_path_delta_prepended(self):
        """Test that prepended sys.path entries are detected as prepend."""
        sys.path.insert(0, "/home/louiskaneko/dev/tach-core/.worktrees/v0.2.0-hooks/src")
        from tach_harness import _compute_sys_path_delta

        before = ["/existing/path"]
        after = ["/new/path", "/existing/path"]  # New path at index 0

        effects = _compute_sys_path_delta(before, after)

        assert len(effects) == 1
        assert effects[0]["type"] == "ModifySysPath"
        assert effects[0]["path"] == "/new/path"
        assert effects[0]["action"] == "prepend"


class TestApplyCachedEffects:
    """Test effect application functions."""

    def test_a_apply_env_effect(self):
        """Test applying an environment effect."""
        sys.path.insert(0, "/home/louiskaneko/dev/tach-core/.worktrees/v0.2.0-hooks/src")
        from tach_harness import apply_cached_effects

        # Clean up before test
        os.environ.pop("TEST_APPLY_EFFECT_VAR", None)

        effects = [
            {"type": "SetEnv", "key": "TEST_APPLY_EFFECT_VAR", "value": "test_value"}
        ]

        applied = apply_cached_effects(effects)

        assert applied == 1
        assert os.environ.get("TEST_APPLY_EFFECT_VAR") == "test_value"

        # Clean up
        os.environ.pop("TEST_APPLY_EFFECT_VAR", None)

    def test_b_apply_sys_path_effect_append(self):
        """Test applying a sys.path append effect."""
        sys.path.insert(0, "/home/louiskaneko/dev/tach-core/.worktrees/v0.2.0-hooks/src")
        from tach_harness import apply_cached_effects

        test_path = "/tmp/test_apply_effect_path_append"
        # Clean up before test
        if test_path in sys.path:
            sys.path.remove(test_path)

        effects = [
            {"type": "ModifySysPath", "action": "append", "path": test_path}
        ]

        applied = apply_cached_effects(effects)

        assert applied == 1
        assert test_path in sys.path

        # Clean up
        sys.path.remove(test_path)

    def test_c_apply_sys_path_effect_prepend(self):
        """Test applying a sys.path prepend effect."""
        sys.path.insert(0, "/home/louiskaneko/dev/tach-core/.worktrees/v0.2.0-hooks/src")
        from tach_harness import apply_cached_effects

        test_path = "/tmp/test_apply_effect_path_prepend"
        # Clean up before test
        if test_path in sys.path:
            sys.path.remove(test_path)

        effects = [
            {"type": "ModifySysPath", "action": "prepend", "path": test_path}
        ]

        applied = apply_cached_effects(effects)

        assert applied == 1
        assert test_path in sys.path
        assert sys.path[0] == test_path  # Should be at the front

        # Clean up
        sys.path.remove(test_path)

    def test_d_apply_multiple_effects(self):
        """Test applying multiple effects at once."""
        sys.path.insert(0, "/home/louiskaneko/dev/tach-core/.worktrees/v0.2.0-hooks/src")
        from tach_harness import apply_cached_effects

        # Clean up before test
        os.environ.pop("TEST_MULTI_1", None)
        os.environ.pop("TEST_MULTI_2", None)

        effects = [
            {"type": "SetEnv", "key": "TEST_MULTI_1", "value": "value1"},
            {"type": "SetEnv", "key": "TEST_MULTI_2", "value": "value2"},
        ]

        applied = apply_cached_effects(effects)

        assert applied == 2
        assert os.environ.get("TEST_MULTI_1") == "value1"
        assert os.environ.get("TEST_MULTI_2") == "value2"

        # Clean up
        os.environ.pop("TEST_MULTI_1", None)
        os.environ.pop("TEST_MULTI_2", None)
