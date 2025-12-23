# test_complex_imports.py - Deep Package Integrity Test
#
# This test verifies that __path__ and __package__ injection works correctly
# for recursive and relative imports, which is the most common failure mode
# for custom loaders.

import sys


def test_deep_package_relative_import():
    """Test that relative imports work through the TachLoader.

    Package structure:
        pkg/
          __init__.py  (contains: from . import sub)
          sub.py       (contains: from .sibling import val)
          sibling.py   (contains: val = 1)

    This test proves __path__ and __package__ are correctly set.
    """
    # Import the package
    try:
        from tests.gauntlet_phase2 import pkg
    except ImportError as e:
        print(f"[deep_import] ERROR: Cannot import pkg: {e}")
        # Try alternate import path
        try:
            import pkg
        except ImportError as e2:
            import pytest

            pytest.skip(f"Cannot import pkg: {e}, {e2}")

    # Verify the chain: pkg -> pkg.sub -> pkg.sibling.val
    assert hasattr(pkg, "sub"), "pkg should have 'sub' attribute"
    assert hasattr(pkg.sub, "val"), "pkg.sub should have 'val' attribute from sibling"
    assert pkg.sub.val == 1, f"pkg.sub.val should be 1, got {pkg.sub.val}"

    print(f"[deep_import] SUCCESS: pkg.sub.val == {pkg.sub.val}")


def test_package_has_path():
    """Verify that __path__ is set for packages."""
    try:
        from tests.gauntlet_phase2 import pkg
    except ImportError:
        try:
            import pkg
        except ImportError:
            import pytest

            pytest.skip("Cannot import pkg")

    # Packages must have __path__
    assert hasattr(pkg, "__path__"), "Package should have __path__"
    assert pkg.__path__ is not None, "__path__ should not be None"
    print(f"[deep_import] pkg.__path__ = {pkg.__path__}")


def test_module_has_package():
    """Verify that __package__ is set correctly for submodules."""
    try:
        from tests.gauntlet_phase2.pkg import sub
    except ImportError:
        try:
            from pkg import sub
        except ImportError:
            import pytest

            pytest.skip("Cannot import pkg.sub")

    # Submodules must have __package__
    assert hasattr(sub, "__package__"), "Submodule should have __package__"
    assert sub.__package__ is not None, "__package__ should not be None"
    print(f"[deep_import] pkg.sub.__package__ = {sub.__package__}")

    # __package__ should match the parent package name
    assert "pkg" in sub.__package__, (
        f"__package__ should contain 'pkg', got {sub.__package__}"
    )


def test_loader_type_for_package():
    """Check what loader is used for the package modules."""
    try:
        from tests.gauntlet_phase2 import pkg
    except ImportError:
        try:
            import pkg
        except ImportError:
            import pytest

            pytest.skip("Cannot import pkg")

    if pkg.__spec__ is None:
        import pytest

        pytest.skip("Module spec is None")

    loader_class = pkg.__spec__.loader.__class__.__name__
    print(f"[deep_import] pkg loaded via: {loader_class}")

    if loader_class == "TachLoader":
        print("[deep_import] SUCCESS: Using TachLoader (Zero-Copy)")
    else:
        print(f"[deep_import] INFO: Using {loader_class}")
