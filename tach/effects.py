"""
Effect Capture and Replay for Shadow Plugin Shim

This module implements the "Effect Pack" pattern for pytest plugin compatibility.
Instead of workers calling plugins directly (which would break the Iron Dome),
the parent process captures plugin effects and workers replay them.

Architecture:
    1. RECORDING (Parent): EffectRecorder captures what plugins DO
    2. TRANSFER (IPC): EffectPack serialized to JSON, sent to worker
    3. REPLAY (Worker): EffectApplicator applies effects without calling plugins

Effect Types:
    - EnvironmentEffect: Set/unset environment variables (P0)
    - MarkerEffect: Add/modify pytest markers (P0)
    - MonkeypatchEffect: Replace module/object attributes (P1)
    - FixtureEffect: Inject fixture values (P2)

The "Dirty Record" Hazard:
    The parent process must reset its environment between recordings to prevent
    pollution from Test A affecting Test B's recording.
"""

from __future__ import annotations

import json
import os
from dataclasses import dataclass, field, asdict
from typing import Any, Dict, List, Optional, Union
from enum import Enum


class EffectType(str, Enum):
    """Types of effects that can be captured and replayed."""

    ENVIRONMENT = "env"
    MARKER = "marker"
    MONKEYPATCH = "monkeypatch"
    FIXTURE = "fixture"
    CONFIG = "config"


@dataclass
class EnvironmentEffect:
    """
    Environment variable mutation.

    Captures changes to os.environ made by plugins during pytest_runtest_setup.

    Attributes:
        key: The environment variable name
        value: The new value (None means unset/delete)
        action: 'set' for new/changed, 'unset' for deleted
    """

    key: str
    value: Optional[str]
    action: str = "set"  # 'set' or 'unset'

    @property
    def effect_type(self) -> EffectType:
        return EffectType.ENVIRONMENT

    def apply(self) -> None:
        """Apply this effect to the current process environment."""
        if self.action == "set" and self.value is not None:
            os.environ[self.key] = self.value
        elif self.action == "unset" or self.value is None:
            os.environ.pop(self.key, None)

    def to_dict(self) -> Dict[str, Any]:
        return {
            "type": self.effect_type.value,
            "key": self.key,
            "value": self.value,
            "action": self.action,
        }

    @classmethod
    def from_dict(cls, data: Dict[str, Any]) -> "EnvironmentEffect":
        return cls(
            key=data["key"],
            value=data.get("value"),
            action=data.get("action", "set"),
        )


# =============================================================================
# Degraded Serialization Helper
# =============================================================================


def _try_serialize(value: Any) -> tuple[Any, Optional[str]]:
    """
    Attempt to serialize a value to JSON-compatible format.

    If serialization fails, returns (repr(value), reason) for degraded mode.

    The Marker Serialization Paradox:
    - Simple markers: @pytest.mark.unit -> serializable
    - Complex markers: @pytest.mark.parametrize("x", [MyObject()]) -> NOT serializable

    We handle this by:
    1. Trying JSON serialization
    2. If it fails, capturing repr() and flagging as degraded
    3. Never crashing the Zygote (the Orchestrator's mandate)

    Args:
        value: Any Python value to serialize

    Returns:
        (serialized_value, degraded_reason) tuple
        - If serializable: (value, None)
        - If not: (repr(value), "type_name: error_msg")
    """
    # Fast path: primitives are always serializable
    if value is None or isinstance(value, (bool, int, float, str)):
        return value, None

    # Lists and tuples: recursively check elements
    if isinstance(value, (list, tuple)):
        result = []
        degraded_reasons = []
        for i, item in enumerate(value):
            serialized, reason = _try_serialize(item)
            result.append(serialized)
            if reason:
                degraded_reasons.append(f"[{i}]: {reason}")
        if degraded_reasons:
            return result, "; ".join(degraded_reasons)
        return result if isinstance(value, list) else tuple(result), None

    # Dicts: recursively check values
    if isinstance(value, dict):
        result = {}
        degraded_reasons = []
        for k, v in value.items():
            # Keys must be strings for JSON
            if not isinstance(k, str):
                k_str = str(k)
                degraded_reasons.append(f"key {repr(k)} converted to str")
            else:
                k_str = k
            serialized, reason = _try_serialize(v)
            result[k_str] = serialized
            if reason:
                degraded_reasons.append(f"{k_str}: {reason}")
        if degraded_reasons:
            return result, "; ".join(degraded_reasons)
        return result, None

    # Try JSON serialization for other types
    try:
        json.dumps(value)
        return value, None
    except (TypeError, ValueError) as e:
        # Degraded mode: capture repr()
        type_name = type(value).__name__

        # Special handling for common non-serializable types
        if hasattr(value, "__next__"):  # Generator
            reason = f"{type_name}: generator object (consumed on iteration)"
        elif callable(value):
            reason = f"{type_name}: callable object"
        elif hasattr(value, "__dict__"):
            reason = f"{type_name}: custom object"
        else:
            reason = f"{type_name}: {str(e)[:50]}"

        # Capture repr, truncating if too long
        try:
            repr_str = repr(value)
            if len(repr_str) > 200:
                repr_str = repr_str[:197] + "..."
        except Exception:
            repr_str = f"<{type_name} (repr failed)>"

        return repr_str, reason


@dataclass
class MarkerEffect:
    """
    Pytest marker addition/modification.

    Captures markers added to test items by plugins (e.g., pytest-django adds
    the django_db marker dynamically).

    Attributes:
        name: Marker name (e.g., 'django_db', 'asyncio')
        args: Positional arguments to the marker
        kwargs: Keyword arguments to the marker
        degraded: If True, args/kwargs contain repr() strings, not actual values
        degraded_reason: Why serialization was degraded (e.g., "generator object")
    """

    name: str
    args: tuple = field(default_factory=tuple)
    kwargs: Dict[str, Any] = field(default_factory=dict)
    degraded: bool = False
    degraded_reason: Optional[str] = None

    @property
    def effect_type(self) -> EffectType:
        return EffectType.MARKER

    def apply(self, item: Any) -> None:
        """
        Apply this marker to a pytest item.

        Args:
            item: The pytest.Item to add the marker to

        Note:
            If this marker was degraded, we still apply it but with the
            repr() strings. This may not perfectly replicate the original
            behavior but maintains marker presence for filtering.
        """
        import pytest

        if self.degraded:
            # Degraded mode: Apply marker with name only (args/kwargs are repr strings)
            # This preserves marker presence for test selection but not data
            marker = pytest.mark.__getattr__(self.name)
        else:
            marker = pytest.mark.__getattr__(self.name)(*self.args, **self.kwargs)
        item.add_marker(marker)

    def to_dict(self) -> Dict[str, Any]:
        return {
            "type": self.effect_type.value,
            "name": self.name,
            "args": list(self.args),
            "kwargs": self.kwargs,
            "degraded": self.degraded,
            "degraded_reason": self.degraded_reason,
        }

    @classmethod
    def from_dict(cls, data: Dict[str, Any]) -> "MarkerEffect":
        return cls(
            name=data["name"],
            args=tuple(data.get("args", [])),
            kwargs=data.get("kwargs", {}),
            degraded=data.get("degraded", False),
            degraded_reason=data.get("degraded_reason"),
        )

    @classmethod
    def from_pytest_marker(cls, marker: Any) -> "MarkerEffect":
        """
        Create a MarkerEffect from a pytest Mark object.

        This implements "Degraded Serialization" - if an argument cannot be
        JSON-serialized, we capture its repr() and flag the effect as degraded.

        Args:
            marker: A pytest.Mark object (from item.iter_markers())

        Returns:
            MarkerEffect with args/kwargs, possibly in degraded mode
        """
        name = marker.name
        args = []
        kwargs = {}
        degraded = False
        degraded_reasons = []

        # Try to serialize args
        for i, arg in enumerate(marker.args):
            serialized, reason = _try_serialize(arg)
            if reason:
                degraded = True
                degraded_reasons.append(f"arg[{i}]: {reason}")
            args.append(serialized)

        # Try to serialize kwargs
        for key, value in marker.kwargs.items():
            serialized, reason = _try_serialize(value)
            if reason:
                degraded = True
                degraded_reasons.append(f"kwarg[{key}]: {reason}")
            kwargs[key] = serialized

        return cls(
            name=name,
            args=tuple(args),
            kwargs=kwargs,
            degraded=degraded,
            degraded_reason="; ".join(degraded_reasons) if degraded_reasons else None,
        )


# Type alias for any effect
Effect = Union[EnvironmentEffect, MarkerEffect]


@dataclass
class EffectPack:
    """
    Container for all effects captured during a test's setup phase.

    This is the unit of transfer between parent (recorder) and worker (applicator).
    Serialized to JSON for IPC transport.

    Attributes:
        test_id: Unique test identifier (e.g., 'tests/test_foo.py::test_bar')
        effects: List of effects to apply
        plugin_order: Order in which plugins were invoked (for debugging)
        timestamp: Unix timestamp of when effects were captured
    """

    test_id: str
    effects: List[Effect] = field(default_factory=list)
    plugin_order: List[str] = field(default_factory=list)
    timestamp: float = field(default_factory=lambda: 0.0)

    def add(self, effect: Effect) -> None:
        """Add an effect to the pack."""
        self.effects.append(effect)

    def apply_all(self, item: Optional[Any] = None) -> int:
        """
        Apply all effects in order.

        Args:
            item: pytest.Item for marker effects (optional)

        Returns:
            Number of effects applied
        """
        applied = 0
        for effect in self.effects:
            if isinstance(effect, EnvironmentEffect):
                effect.apply()
                applied += 1
            elif isinstance(effect, MarkerEffect) and item is not None:
                effect.apply(item)
                applied += 1
        return applied

    def to_json(self) -> str:
        """Serialize to JSON for IPC transport."""
        return json.dumps(
            {
                "test_id": self.test_id,
                "effects": [e.to_dict() for e in self.effects],
                "plugin_order": self.plugin_order,
                "timestamp": self.timestamp,
            }
        )

    @classmethod
    def from_json(cls, data: str) -> "EffectPack":
        """Deserialize from JSON."""
        parsed = json.loads(data)

        effects = []
        for effect_data in parsed.get("effects", []):
            effect_type = effect_data.get("type")
            if effect_type == EffectType.ENVIRONMENT.value:
                effects.append(EnvironmentEffect.from_dict(effect_data))
            elif effect_type == EffectType.MARKER.value:
                effects.append(MarkerEffect.from_dict(effect_data))
            # Future: MonkeypatchEffect, FixtureEffect

        return cls(
            test_id=parsed["test_id"],
            effects=effects,
            plugin_order=parsed.get("plugin_order", []),
            timestamp=parsed.get("timestamp", 0.0),
        )

    def __len__(self) -> int:
        return len(self.effects)


def compute_env_delta(initial: Dict[str, str], final: Dict[str, str]) -> List[EnvironmentEffect]:
    """
    Compute environment variable changes between two snapshots.

    Args:
        initial: Environment before plugins ran
        final: Environment after plugins ran

    Returns:
        List of EnvironmentEffects representing the delta
    """
    effects = []

    # Find added or changed variables
    for key, value in final.items():
        if key not in initial:
            effects.append(EnvironmentEffect(key=key, value=value, action="set"))
        elif initial[key] != value:
            effects.append(EnvironmentEffect(key=key, value=value, action="set"))

    # Find deleted variables
    for key in initial:
        if key not in final:
            effects.append(EnvironmentEffect(key=key, value=None, action="unset"))

    return effects


class EnvironmentSnapshot:
    """
    Captures and restores environment state for the "Dirty Record" protection.

    Usage:
        snapshot = EnvironmentSnapshot.capture()
        # ... run plugin hooks ...
        delta = snapshot.compute_delta()
        snapshot.restore()  # Reset for next test
    """

    def __init__(self, env: Dict[str, str]):
        self._snapshot = env.copy()

    @classmethod
    def capture(cls) -> "EnvironmentSnapshot":
        """Capture current environment state."""
        return cls(dict(os.environ))

    def compute_delta(self) -> List[EnvironmentEffect]:
        """Compute changes since snapshot was captured."""
        return compute_env_delta(self._snapshot, dict(os.environ))

    def restore(self) -> None:
        """
        Restore environment to snapshotted state.

        This is the "Soft Reset" required to prevent the "Dirty Record" hazard.
        """
        current = set(os.environ.keys())
        snapshot = set(self._snapshot.keys())

        # Remove keys that were added
        for key in current - snapshot:
            del os.environ[key]

        # Restore original values (including re-adding deleted keys)
        for key, value in self._snapshot.items():
            os.environ[key] = value


# =============================================================================
# Effect Cache (Parent-Side Storage)
# =============================================================================


class EffectCache:
    """
    Cache for effect packs, keyed by test_id.

    The parent process populates this during the Recording phase.
    Workers request effect packs via IPC before running tests.
    """

    def __init__(self):
        self._cache: Dict[str, EffectPack] = {}

    def store(self, pack: EffectPack) -> None:
        """Store an effect pack for later retrieval."""
        self._cache[pack.test_id] = pack

    def get(self, test_id: str) -> Optional[EffectPack]:
        """Retrieve an effect pack by test_id."""
        return self._cache.get(test_id)

    def remove(self, test_id: str) -> None:
        """Remove an effect pack (after worker has retrieved it)."""
        self._cache.pop(test_id, None)

    def clear(self) -> None:
        """Clear all cached effect packs."""
        self._cache.clear()

    def __len__(self) -> int:
        return len(self._cache)
