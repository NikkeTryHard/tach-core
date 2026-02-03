"""Tests for Zygote plugin configuration."""

from pathlib import Path

# Compute paths relative to this file
_tests_unit_dir = Path(__file__).parent
_tests_dir = _tests_unit_dir.parent
_project_root = _tests_dir.parent
_src_dir = _project_root / "src"
_harness_path = _src_dir / "tach_harness.py"


def test_zygote_does_not_disable_asyncio():
    """Zygote should not disable pytest-asyncio plugin.

    The Zygote worker must keep asyncio/trio plugins enabled during collection
    so that test node IDs match what the supervisor discovers. Disabling these
    plugins causes parameterized async tests to have different IDs.
    """
    content = _harness_path.read_text()

    # Check that no:asyncio and no:trio are NOT in the content
    # (unless they're commented out)
    lines = content.splitlines()
    for line in lines:
        stripped = line.strip()
        # Skip comments
        if stripped.startswith("#"):
            continue
        assert "no:asyncio" not in line, "asyncio plugin should not be disabled"
        assert "no:trio" not in line, "trio plugin should not be disabled"


def test_zygote_still_disables_other_plugins():
    """Zygote should still disable non-essential plugins like terminal, cov, xdist."""
    content = _harness_path.read_text()

    # These plugins should still be disabled in the args
    required_disabled = [
        "no:terminal",
        "no:cacheprovider",
        "no:cov",
        "no:xdist",
        "no:sugar",
        "no:django",
    ]

    for plugin in required_disabled:
        assert plugin in content, f"{plugin} should be disabled but isn't found in harness"
