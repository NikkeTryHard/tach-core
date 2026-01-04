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
import logging
import os
import socket
import sys
import warnings
from dataclasses import dataclass, field, asdict
from typing import Any, Dict, List, Optional, Set, Tuple, Type, Union
from enum import Enum

# =============================================================================
# Vital Types Registry
# =============================================================================
#
# The "Fidelity Gap" Hazard:
#   Certain types CANNOT be safely degraded via repr() serialization.
#   If a fixture returns a socket or DB connection, the repr() string is
#   useless for actual I/O operations in the worker.
#
# Solution: The Vital Types Registry
#   1. Identify types that require FD handover (sockets, file handles, DB connections)
#   2. Emit CRITICAL warnings when these types are degraded
#   3. Use SCM_RIGHTS to pass actual file descriptors to workers
#
# The Orchestrator's Mandate:
#   "A degraded socket is a lie. A CRITICAL warning is the truth."
# =============================================================================

# Logger for CRITICAL warnings
_logger = logging.getLogger("tach.effects")


class VitalTypeCategory(str, Enum):
    """Categories of vital types that cannot be safely degraded."""

    FILE_DESCRIPTOR = "fd"  # File handles, sockets, pipes
    DATABASE = "db"  # Database connections, cursors
    NETWORK = "network"  # Network sockets, HTTP clients
    LOCK = "lock"  # Threading locks, semaphores
    RESOURCE = "resource"  # Generic resources requiring cleanup


@dataclass
class VitalTypeInfo:
    """Information about a vital type."""

    type_pattern: str  # Module path pattern (e.g., "socket.socket")
    category: VitalTypeCategory
    reason: str  # Why this type is vital
    requires_fd_handover: bool  # Whether SCM_RIGHTS can help


# The Vital Types Registry
# These types MUST NOT be degraded without CRITICAL warning
_VITAL_TYPES_REGISTRY: List[VitalTypeInfo] = [
    # File Descriptors
    VitalTypeInfo(
        type_pattern="socket.socket",
        category=VitalTypeCategory.FILE_DESCRIPTOR,
        reason="Socket FD is process-local; repr() cannot recreate connection",
        requires_fd_handover=True,
    ),
    VitalTypeInfo(
        type_pattern="io.FileIO",
        category=VitalTypeCategory.FILE_DESCRIPTOR,
        reason="File handle is process-local; repr() cannot recreate position",
        requires_fd_handover=True,
    ),
    VitalTypeInfo(
        type_pattern="io.BufferedReader",
        category=VitalTypeCategory.FILE_DESCRIPTOR,
        reason="Buffered file handle has internal state",
        requires_fd_handover=True,
    ),
    VitalTypeInfo(
        type_pattern="io.BufferedWriter",
        category=VitalTypeCategory.FILE_DESCRIPTOR,
        reason="Buffered file handle has internal state",
        requires_fd_handover=True,
    ),
    VitalTypeInfo(
        type_pattern="io.TextIOWrapper",
        category=VitalTypeCategory.FILE_DESCRIPTOR,
        reason="Text file wrapper has encoding state",
        requires_fd_handover=True,
    ),
    # Database Connections (common patterns)
    VitalTypeInfo(
        type_pattern="sqlite3.Connection",
        category=VitalTypeCategory.DATABASE,
        reason="SQLite connection is process-local; uses OS file lock",
        requires_fd_handover=False,  # FD handover won't help, need reconnect
    ),
    VitalTypeInfo(
        type_pattern="psycopg2.extensions.connection",
        category=VitalTypeCategory.DATABASE,
        reason="PostgreSQL connection has active TCP socket",
        requires_fd_handover=True,
    ),
    VitalTypeInfo(
        type_pattern="pymysql.connections.Connection",
        category=VitalTypeCategory.DATABASE,
        reason="MySQL connection has active TCP socket",
        requires_fd_handover=True,
    ),
    # Network Clients
    VitalTypeInfo(
        type_pattern="urllib3.poolmanager.PoolManager",
        category=VitalTypeCategory.NETWORK,
        reason="Connection pool maintains live sockets",
        requires_fd_handover=False,
    ),
    VitalTypeInfo(
        type_pattern="requests.Session",
        category=VitalTypeCategory.NETWORK,
        reason="Session has connection pool with live sockets",
        requires_fd_handover=False,
    ),
    VitalTypeInfo(
        type_pattern="httpx.Client",
        category=VitalTypeCategory.NETWORK,
        reason="HTTP client maintains connection pool",
        requires_fd_handover=False,
    ),
    # Threading Primitives
    VitalTypeInfo(
        type_pattern="threading.Lock",
        category=VitalTypeCategory.LOCK,
        reason="Lock state is thread-local; cannot transfer between processes",
        requires_fd_handover=False,
    ),
    VitalTypeInfo(
        type_pattern="threading.RLock",
        category=VitalTypeCategory.LOCK,
        reason="Reentrant lock has owner thread ID",
        requires_fd_handover=False,
    ),
    VitalTypeInfo(
        type_pattern="threading.Semaphore",
        category=VitalTypeCategory.LOCK,
        reason="Semaphore counter is process-local",
        requires_fd_handover=False,
    ),
    VitalTypeInfo(
        type_pattern="threading.Event",
        category=VitalTypeCategory.LOCK,
        reason="Event flag is process-local",
        requires_fd_handover=False,
    ),
    VitalTypeInfo(
        type_pattern="multiprocessing.synchronize.Lock",
        category=VitalTypeCategory.LOCK,
        reason="Multiprocessing lock uses shared memory",
        requires_fd_handover=False,
    ),
]


def _get_type_fqn(value: Any) -> str:
    """Get fully qualified name of a type (e.g., 'socket.socket')."""
    t = type(value)
    module = t.__module__
    if module == "builtins":
        return t.__qualname__
    return f"{module}.{t.__qualname__}"


def _check_vital_type(value: Any) -> Optional[VitalTypeInfo]:
    """
    Check if a value is a vital type that cannot be safely degraded.

    Returns VitalTypeInfo if vital, None otherwise.
    """
    fqn = _get_type_fqn(value)

    # Check against registry
    for vital_info in _VITAL_TYPES_REGISTRY:
        if fqn == vital_info.type_pattern or fqn.endswith(f".{vital_info.type_pattern}"):
            return vital_info

    # Heuristic checks for types not explicitly registered
    # Check for file descriptor attribute (socket, file, pipe)
    if hasattr(value, "fileno") and callable(value.fileno):
        try:
            fd = value.fileno()
            if isinstance(fd, int) and fd >= 0:
                return VitalTypeInfo(
                    type_pattern=fqn,
                    category=VitalTypeCategory.FILE_DESCRIPTOR,
                    reason=f"Object has fileno()={fd}; FD is process-local",
                    requires_fd_handover=True,
                )
        except Exception:
            pass  # fileno() may raise if closed

    # Check for close() method (resource that needs cleanup)
    if hasattr(value, "close") and callable(value.close):
        # Exclude common false positives
        if not isinstance(value, (str, bytes, list, dict, tuple)):
            return VitalTypeInfo(
                type_pattern=fqn,
                category=VitalTypeCategory.RESOURCE,
                reason="Object has close() method; may hold external resources",
                requires_fd_handover=False,
            )

    return None


def _emit_vital_type_warning(
    value: Any,
    vital_info: VitalTypeInfo,
    context: str,
) -> None:
    """
    Emit CRITICAL warning when a vital type is degraded.

    This is the Orchestrator's mandate: "A degraded socket is a lie."
    """
    fqn = _get_type_fqn(value)

    # Build warning message
    msg = f"[CRITICAL] Vital type degraded: {fqn}\n  Context: {context}\n  Category: {vital_info.category.value}\n  Reason: {vital_info.reason}\n"

    if vital_info.requires_fd_handover:
        msg += "  Solution: Use FD Teleporter (SCM_RIGHTS) to pass file descriptor\n  See: docs/architecture/internal-architecture.md#scm-rights-handover\n"
    else:
        msg += "  Solution: Recreate resource in worker process\n  This type cannot be transferred; fixture must be re-evaluated\n"

    # Log at CRITICAL level
    _logger.critical(msg)

    # Also emit Python warning for test visibility
    warnings.warn(
        f"Vital type {fqn} was degraded during effect serialization. Test may behave incorrectly. Reason: {vital_info.reason}",
        RuntimeWarning,
        stacklevel=4,
    )


class EffectType(str, Enum):
    """Types of effects that can be captured and replayed."""

    ENVIRONMENT = "env"
    MARKER = "marker"
    MONKEYPATCH = "monkeypatch"
    FIXTURE = "fixture"
    CONFIG = "config"
    FILE_DESCRIPTOR = "fd"  # New: For SCM_RIGHTS handover


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


def _try_serialize(value: Any, context: str = "unknown") -> tuple[Any, Optional[str], bool]:
    """
    Attempt to serialize a value to JSON-compatible format.

    If serialization fails, returns (repr(value), reason, is_vital) for degraded mode.
    If the value is a VITAL TYPE, emits a CRITICAL warning.

    The Marker Serialization Paradox:
    - Simple markers: @pytest.mark.unit -> serializable
    - Complex markers: @pytest.mark.parametrize("x", [MyObject()]) -> NOT serializable

    We handle this by:
    1. Trying JSON serialization
    2. If it fails, check if it's a Vital Type (emit CRITICAL warning)
    3. Capturing repr() and flagging as degraded
    4. Never crashing the Zygote (the Orchestrator's mandate)

    Args:
        value: Any Python value to serialize
        context: Description of where this value came from (for CRITICAL warnings)

    Returns:
        (serialized_value, degraded_reason, is_vital_type) tuple
        - If serializable: (value, None, False)
        - If not: (repr(value), "type_name: error_msg", is_vital)
    """
    # Fast path: primitives are always serializable
    if value is None or isinstance(value, (bool, int, float, str)):
        return value, None, False

    # Lists and tuples: recursively check elements
    if isinstance(value, (list, tuple)):
        result = []
        degraded_reasons = []
        has_vital = False
        for i, item in enumerate(value):
            serialized, reason, is_vital = _try_serialize(item, f"{context}[{i}]")
            result.append(serialized)
            if reason:
                degraded_reasons.append(f"[{i}]: {reason}")
            if is_vital:
                has_vital = True
        if degraded_reasons:
            return result, "; ".join(degraded_reasons), has_vital
        return result if isinstance(value, list) else tuple(result), None, False

    # Dicts: recursively check values
    if isinstance(value, dict):
        result = {}
        degraded_reasons = []
        has_vital = False
        for k, v in value.items():
            # Keys must be strings for JSON
            if not isinstance(k, str):
                k_str = str(k)
                degraded_reasons.append(f"key {repr(k)} converted to str")
            else:
                k_str = k
            serialized, reason, is_vital = _try_serialize(v, f"{context}.{k_str}")
            result[k_str] = serialized
            if reason:
                degraded_reasons.append(f"{k_str}: {reason}")
            if is_vital:
                has_vital = True
        if degraded_reasons:
            return result, "; ".join(degraded_reasons), has_vital
        return result, None, False

    # Try JSON serialization for other types
    try:
        json.dumps(value)
        return value, None, False
    except (TypeError, ValueError) as e:
        # Check if this is a Vital Type (requires CRITICAL warning)
        vital_info = _check_vital_type(value)
        is_vital = vital_info is not None

        if is_vital:
            # Emit CRITICAL warning for vital types
            _emit_vital_type_warning(value, vital_info, context)

        # Degraded mode: capture repr()
        type_name = type(value).__name__

        # Special handling for common non-serializable types
        if hasattr(value, "__next__"):  # Generator
            reason = f"{type_name}: generator object (consumed on iteration)"
        elif callable(value):
            reason = f"{type_name}: callable object"
        elif is_vital:
            reason = f"{type_name}: VITAL TYPE - {vital_info.reason}"
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

        return repr_str, reason, is_vital


# Backwards compatibility wrapper (for existing code using 2-tuple return)
def _try_serialize_compat(value: Any) -> tuple[Any, Optional[str]]:
    """Backwards-compatible wrapper for _try_serialize."""
    serialized, reason, _ = _try_serialize(value, "unknown")
    return serialized, reason


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
        has_vital_types: If True, at least one arg was a Vital Type (CRITICAL)
    """

    name: str
    args: tuple = field(default_factory=tuple)
    kwargs: Dict[str, Any] = field(default_factory=dict)
    degraded: bool = False
    degraded_reason: Optional[str] = None
    has_vital_types: bool = False  # New: Track if CRITICAL types were degraded

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
            "has_vital_types": self.has_vital_types,
        }

    @classmethod
    def from_dict(cls, data: Dict[str, Any]) -> "MarkerEffect":
        return cls(
            name=data["name"],
            args=tuple(data.get("args", [])),
            kwargs=data.get("kwargs", {}),
            degraded=data.get("degraded", False),
            degraded_reason=data.get("degraded_reason"),
            has_vital_types=data.get("has_vital_types", False),
        )

    @classmethod
    def from_pytest_marker(cls, marker: Any) -> "MarkerEffect":
        """
        Create a MarkerEffect from a pytest Mark object.

        This implements "Degraded Serialization" - if an argument cannot be
        JSON-serialized, we capture its repr() and flag the effect as degraded.

        For VITAL TYPES (sockets, DB connections, file handles), emits CRITICAL
        warnings via the Vital Types Registry.

        Args:
            marker: A pytest.Mark object (from item.iter_markers())

        Returns:
            MarkerEffect with args/kwargs, possibly in degraded mode
        """
        name = marker.name
        args = []
        kwargs = {}
        degraded = False
        has_vital = False
        degraded_reasons = []

        # Try to serialize args (with Vital Type detection)
        for i, arg in enumerate(marker.args):
            serialized, reason, is_vital = _try_serialize(arg, f"marker[{name}].args[{i}]")
            if reason:
                degraded = True
                degraded_reasons.append(f"arg[{i}]: {reason}")
            if is_vital:
                has_vital = True
            args.append(serialized)

        # Try to serialize kwargs (with Vital Type detection)
        for key, value in marker.kwargs.items():
            serialized, reason, is_vital = _try_serialize(value, f"marker[{name}].kwargs[{key}]")
            if reason:
                degraded = True
                degraded_reasons.append(f"kwarg[{key}]: {reason}")
            if is_vital:
                has_vital = True
            kwargs[key] = serialized

        return cls(
            name=name,
            args=tuple(args),
            kwargs=kwargs,
            degraded=degraded,
            degraded_reason="; ".join(degraded_reasons) if degraded_reasons else None,
            has_vital_types=has_vital,
        )


# Type alias for any effect
Effect = Union[EnvironmentEffect, MarkerEffect]


# =============================================================================
# File Descriptor Effect (SCM_RIGHTS Teleporter)
# =============================================================================
#
# The "FD Teleporter" Pattern:
#   On Linux, a File Descriptor is just an index into the kernel's fd table.
#   When we fork a worker, the child inherits the parent's fd table.
#   But for Vital Types (sockets, DB connections), we need to pass NEW fds
#   from the parent to an existing worker - this requires SCM_RIGHTS.
#
# This effect type captures file descriptors that need special handling:
#   1. Parent captures the FD number and fixture name
#   2. During effect transfer, FD is sent via SCM_RIGHTS over Unix socket
#   3. Worker receives FD and maps it to the appropriate fixture
#
# The Orchestrator's Wisdom:
#   "On Linux, a File Descriptor is just an index. In a Zygote, it is a
#    Tether to the Past."
# =============================================================================


@dataclass
class FileDescriptorEffect:
    """
    File Descriptor handover via SCM_RIGHTS.

    Used when a fixture returns a socket, file handle, or other FD-backed
    resource that cannot be serialized via repr().

    Attributes:
        name: Fixture or resource name (for debugging)
        fd: The file descriptor number (in parent's fd table)
        fd_type: Type of FD (socket, file, pipe, etc.)
        metadata: Additional metadata (e.g., socket address, file path)
        received_fd: After SCM_RIGHTS transfer, the worker's FD number
    """

    name: str
    fd: int
    fd_type: str = "unknown"  # socket, file, pipe, etc.
    metadata: Dict[str, Any] = field(default_factory=dict)
    received_fd: Optional[int] = None  # Set after SCM_RIGHTS transfer

    @property
    def effect_type(self) -> EffectType:
        return EffectType.FILE_DESCRIPTOR

    def to_dict(self) -> Dict[str, Any]:
        """Serialize to dict (FD number is placeholder, actual transfer via SCM_RIGHTS)."""
        return {
            "type": self.effect_type.value,
            "name": self.name,
            "fd": self.fd,
            "fd_type": self.fd_type,
            "metadata": self.metadata,
        }

    @classmethod
    def from_dict(cls, data: Dict[str, Any]) -> "FileDescriptorEffect":
        return cls(
            name=data["name"],
            fd=data["fd"],
            fd_type=data.get("fd_type", "unknown"),
            metadata=data.get("metadata", {}),
        )

    @classmethod
    def from_value(cls, value: Any, name: str) -> Optional["FileDescriptorEffect"]:
        """
        Create a FileDescriptorEffect from a value with a fileno() method.

        Returns None if the value doesn't have a valid FD.
        """
        if not hasattr(value, "fileno") or not callable(value.fileno):
            return None

        try:
            fd = value.fileno()
            if not isinstance(fd, int) or fd < 0:
                return None
        except Exception:
            return None

        # Determine FD type
        type_name = type(value).__name__
        if "socket" in type_name.lower():
            fd_type = "socket"
        elif "file" in type_name.lower() or "io" in type_name.lower():
            fd_type = "file"
        elif "pipe" in type_name.lower():
            fd_type = "pipe"
        else:
            fd_type = "unknown"

        # Gather metadata
        metadata: Dict[str, Any] = {"type": _get_type_fqn(value)}

        # Socket-specific metadata
        if fd_type == "socket" and hasattr(value, "getsockname"):
            try:
                metadata["local_addr"] = str(value.getsockname())
            except Exception:
                pass
            try:
                metadata["remote_addr"] = str(value.getpeername())
            except Exception:
                pass

        # File-specific metadata
        if hasattr(value, "name"):
            try:
                metadata["path"] = str(value.name)
            except Exception:
                pass
        if hasattr(value, "mode"):
            try:
                metadata["mode"] = str(value.mode)
            except Exception:
                pass

        return cls(name=name, fd=fd, fd_type=fd_type, metadata=metadata)

    def apply(self) -> int:
        """
        Apply this effect by returning the received FD.

        The caller is responsible for using the FD appropriately
        (e.g., wrapping in a socket object, file object, etc.)

        Returns:
            The file descriptor number (received_fd if set, otherwise fd)
        """
        if self.received_fd is not None:
            return self.received_fd
        return self.fd


# Update the Effect type alias to include FileDescriptorEffect
Effect = Union[EnvironmentEffect, MarkerEffect, FileDescriptorEffect]


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
            elif effect_type == EffectType.FILE_DESCRIPTOR.value:
                effects.append(FileDescriptorEffect.from_dict(effect_data))
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
