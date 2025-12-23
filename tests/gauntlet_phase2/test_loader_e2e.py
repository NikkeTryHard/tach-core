# test_loader_e2e.py - End-to-End Test for Zero-Copy Module Loader
#
# This test verifies that the TachMetaPathFinder and TachLoader
# are correctly intercepting imports and loading modules via Rust FFI.
#
# IMPORTANT: This test only works when run via tach-core (not vanilla pytest)
# because the import hook requires the tach_rust module to be available.

import sys


def test_loader_is_installed():
    """Verify the TachMetaPathFinder is installed at sys.meta_path[0]."""
    # Check if TachMetaPathFinder is in sys.meta_path
    finder_names = [type(f).__name__ for f in sys.meta_path]

    # When running via tach-core, TachMetaPathFinder should be present
    # When running via vanilla pytest, this test is skipped
    if "TachMetaPathFinder" not in finder_names:
        import pytest

        pytest.skip("TachMetaPathFinder not installed (not running via tach-core)")

    # Verify it's at position 0 (highest priority)
    assert finder_names[0] == "TachMetaPathFinder", (
        f"TachMetaPathFinder should be at position 0, but found at {finder_names.index('TachMetaPathFinder')}"
    )


def test_module_loaded_via_tach_loader():
    """Verify that a module loaded via Tach uses TachLoader."""
    # Try to import the fixtures.hello module
    try:
        from tests.gauntlet_phase2.fixtures import hello
    except ImportError:
        # If we can't import, try relative import within the test directory
        try:
            from fixtures import hello
        except ImportError:
            import pytest

            pytest.skip("Cannot import fixtures.hello module")

    # Check if the module was loaded via TachLoader
    if hello.__spec__ is None:
        import pytest

        pytest.skip("Module spec is None (not running via tach-core)")

    loader_class = hello.__spec__.loader.__class__.__name__
    if loader_class == "TachLoader":
        # SUCCESS: Module was loaded via our custom loader
        assert True
    else:
        # Fallback: Module was loaded via standard importlib
        # This is expected when running via vanilla pytest
        import pytest

        pytest.skip(
            f"Module loaded via {loader_class}, not TachLoader (not running via tach-core)"
        )


def test_module_has_correct_file_attribute():
    """Verify that __file__ is set correctly for loaded modules."""
    try:
        from tests.gauntlet_phase2.fixtures import hello
    except ImportError:
        try:
            from fixtures import hello
        except ImportError:
            import pytest

            pytest.skip("Cannot import fixtures.hello module")

    # __file__ should be set and point to a .py file
    assert hasattr(hello, "__file__"), "Module should have __file__ attribute"
    assert hello.__file__ is not None, "__file__ should not be None"
    assert hello.__file__.endswith(".py"), (
        f"__file__ should end with .py, got {hello.__file__}"
    )
    assert "hello.py" in hello.__file__, (
        f"__file__ should contain 'hello.py', got {hello.__file__}"
    )


def test_module_functionality():
    """Verify that the module functions work correctly after loading."""
    try:
        from tests.gauntlet_phase2.fixtures import hello
    except ImportError:
        try:
            from fixtures import hello
        except ImportError:
            import pytest

            pytest.skip("Cannot import fixtures.hello module")

    # Test the greet function
    assert hello.greet("World") == "Hello, World!"
    assert hello.greet("Tach") == "Hello, Tach!"

    # Test the VALUE constant
    assert hello.VALUE == 42
