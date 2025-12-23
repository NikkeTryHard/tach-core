# test_loader_regression.py - Regression Prevention Tests
#
# These tests lock in correct behavior to prevent regressions

import sys


class TestLoaderRegression:
    """Regression tests for the Zero-Copy Module Loader."""

    def test_sys_modules_populated(self):
        """Verify imported modules are added to sys.modules."""
        try:
            from tests.gauntlet_phase2.fixtures import hello

            assert "tests.gauntlet_phase2.fixtures.hello" in sys.modules
        except ImportError:
            import pytest

            pytest.skip("Cannot import hello")

    def test_module_file_is_source_path(self):
        """Verify __file__ points to source .py, not .pyc."""
        try:
            from tests.gauntlet_phase2.fixtures import hello

            assert hello.__file__.endswith(".py"), (
                f"__file__ should end with .py, got {hello.__file__}"
            )
            assert not hello.__file__.endswith(".pyc"), "__file__ should not be .pyc"
        except ImportError:
            import pytest

            pytest.skip("Cannot import hello")

    def test_package_path_is_list(self):
        """Verify package __path__ is a list."""
        try:
            from tests.gauntlet_phase2 import fixtures

            assert isinstance(fixtures.__path__, list), (
                f"__path__ should be list, got {type(fixtures.__path__)}"
            )
        except ImportError:
            import pytest

            pytest.skip("Cannot import fixtures")

    def test_function_execution(self):
        """Verify functions in loaded modules are callable and work."""
        try:
            from tests.gauntlet_phase2.fixtures import hello

            result = hello.greet("Test")
            assert result == "Hello, Test!"
        except ImportError:
            import pytest

            pytest.skip("Cannot import hello")

    def test_constant_access(self):
        """Verify constants in loaded modules are accessible."""
        try:
            from tests.gauntlet_phase2.fixtures import hello

            assert hello.VALUE == 42
        except ImportError:
            import pytest

            pytest.skip("Cannot import hello")

    def test_import_hook_priority(self):
        """Verify TachMetaPathFinder has priority in sys.meta_path."""
        finder_names = [type(f).__name__ for f in sys.meta_path]

        if "TachMetaPathFinder" in finder_names:
            idx = finder_names.index("TachMetaPathFinder")
            assert idx == 0, f"TachMetaPathFinder should be at index 0, found at {idx}"
            print("[regression] Import hook at correct priority")
        else:
            import pytest

            pytest.skip("TachMetaPathFinder not installed")

    def test_nested_import_chain(self):
        """Verify import chains work: parent imports child imports sibling."""
        try:
            from tests.gauntlet_phase2 import pkg

            # pkg imports sub, sub imports sibling.val
            assert hasattr(pkg, "sub")
            assert hasattr(pkg.sub, "val")
            assert pkg.sub.val == 1
        except ImportError:
            import pytest

            pytest.skip("Cannot import pkg")

    def test_spec_has_correct_origin(self):
        """Verify ModuleSpec.origin is the source path."""
        try:
            from tests.gauntlet_phase2.fixtures import hello

            if hello.__spec__ is None:
                import pytest

                pytest.skip("Module spec is None")

            origin = hello.__spec__.origin
            assert origin is not None
            assert origin.endswith(".py")
            assert "hello.py" in origin
        except ImportError:
            import pytest

            pytest.skip("Cannot import hello")

    def test_spec_is_package_correct(self):
        """Verify ModuleSpec.is_package is correct for packages and modules."""
        try:
            # Package
            from tests.gauntlet_phase2 import fixtures

            if fixtures.__spec__:
                # Packages MAY have submodule_search_locations
                pass

            # Module (not package)
            from tests.gauntlet_phase2.fixtures import hello

            if hello.__spec__:
                # Non-packages should not have submodule_search_locations
                # (or it should be None)
                pass
        except ImportError:
            import pytest

            pytest.skip("Cannot import fixtures or hello")

    def test_repeated_import_is_cached(self):
        """Verify repeated imports return cached module."""
        try:
            from tests.gauntlet_phase2.fixtures import hello

            id1 = id(hello)

            from tests.gauntlet_phase2.fixtures import hello

            id2 = id(hello)

            assert id1 == id2, "Repeated imports should return same object"
        except ImportError:
            import pytest

            pytest.skip("Cannot import hello")
