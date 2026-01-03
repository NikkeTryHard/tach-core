"""
Tach: The Runtime Hypervisor for Python Tests

This package provides the Python-side components for Tach's test isolation:
- effects: Effect capture and replay for pytest plugin compatibility
- plugin_shim: Hook interception for the Shadow Plugin pattern

Key Types:
- VitalTypeCategory: Categories of vital types (FD, DB, NETWORK, LOCK, RESOURCE)
- VitalTypeInfo: Information about a vital type
- EffectType: Types of effects (ENVIRONMENT, MARKER, FILE_DESCRIPTOR, etc.)
- EnvironmentEffect: Environment variable mutation
- MarkerEffect: Pytest marker addition/modification
- FileDescriptorEffect: FD handover via SCM_RIGHTS
- EffectPack: Container for all effects during test setup
"""

__version__ = "0.8.5-alpha"

# Export key types for external use
from .effects import (
    VitalTypeCategory,
    VitalTypeInfo,
    EffectType,
    EnvironmentEffect,
    MarkerEffect,
    FileDescriptorEffect,
    EffectPack,
    EffectCache,
    EnvironmentSnapshot,
)

from .plugin_shim import (
    EffectRecorder,
    EffectApplicator,
    create_recording_plugin,
)

__all__ = [
    # Version
    "__version__",
    # Vital Types
    "VitalTypeCategory",
    "VitalTypeInfo",
    # Effect Types
    "EffectType",
    "EnvironmentEffect",
    "MarkerEffect",
    "FileDescriptorEffect",
    "EffectPack",
    "EffectCache",
    "EnvironmentSnapshot",
    # Plugin Shim
    "EffectRecorder",
    "EffectApplicator",
    "create_recording_plugin",
]
