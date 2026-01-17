# test_collection_hooks.py - Tests for pytest_collection_modifyitems hook support
#
# These tests verify that the collection_modifyitems hook correctly:
# - Allows test reordering
# - Allows test deselection (removal)
# - Handles edge cases gracefully

import os
import sys
import tempfile


def test_collection_modifyitems_reorders_tests():
    """pytest_collection_modifyitems can reorder test collection."""
    harness_path = os.path.join(
        os.path.dirname(__file__), "..", "..", "src", "tach_harness.py"
    )
    harness_path = os.path.abspath(harness_path)

    import importlib.util

    spec = importlib.util.spec_from_file_location("tach_harness", harness_path)
    harness = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(harness)

    with tempfile.TemporaryDirectory() as tmpdir:
        conftest_path = os.path.join(tmpdir, "conftest.py")
        with open(conftest_path, "w") as f:
            f.write(
                """
def pytest_collection_modifyitems(items):
    # Reverse the order
    items.reverse()
"""
            )

        # Create mock items
        mock_items = ["test_a", "test_b", "test_c"]

        result = harness.call_collection_modifyitems(
            conftest_path=conftest_path,
            items=mock_items,
        )

        assert result["error"] is None, f"Unexpected error: {result['error']}"
        assert result["reordered"] is True
        assert result["new_order"] == ["test_c", "test_b", "test_a"]


def test_collection_modifyitems_deselects_tests():
    """pytest_collection_modifyitems can remove tests from collection."""
    harness_path = os.path.join(
        os.path.dirname(__file__), "..", "..", "src", "tach_harness.py"
    )
    harness_path = os.path.abspath(harness_path)

    import importlib.util

    spec = importlib.util.spec_from_file_location("tach_harness", harness_path)
    harness = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(harness)

    with tempfile.TemporaryDirectory() as tmpdir:
        conftest_path = os.path.join(tmpdir, "conftest.py")
        with open(conftest_path, "w") as f:
            f.write(
                """
def pytest_collection_modifyitems(items):
    # Remove items containing 'skip'
    items[:] = [item for item in items if 'skip' not in item]
"""
            )

        mock_items = ["test_a", "test_skip_this", "test_b"]

        result = harness.call_collection_modifyitems(
            conftest_path=conftest_path,
            items=mock_items,
        )

        assert result["error"] is None
        assert result["removed"] == ["test_skip_this"]
        assert result["new_order"] == ["test_a", "test_b"]


def test_collection_modifyitems_no_changes():
    """pytest_collection_modifyitems with no modifications."""
    harness_path = os.path.join(
        os.path.dirname(__file__), "..", "..", "src", "tach_harness.py"
    )
    harness_path = os.path.abspath(harness_path)

    import importlib.util

    spec = importlib.util.spec_from_file_location("tach_harness", harness_path)
    harness = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(harness)

    with tempfile.TemporaryDirectory() as tmpdir:
        conftest_path = os.path.join(tmpdir, "conftest.py")
        with open(conftest_path, "w") as f:
            f.write(
                """
def pytest_collection_modifyitems(items):
    pass  # No changes
"""
            )

        mock_items = ["test_a", "test_b"]

        result = harness.call_collection_modifyitems(
            conftest_path=conftest_path,
            items=mock_items,
        )

        assert result["error"] is None
        assert result["reordered"] is False
        assert result["removed"] == []
        assert result["new_order"] == ["test_a", "test_b"]


def test_collection_modifyitems_missing_hook():
    """pytest_collection_modifyitems returns original items if hook not found."""
    harness_path = os.path.join(
        os.path.dirname(__file__), "..", "..", "src", "tach_harness.py"
    )
    harness_path = os.path.abspath(harness_path)

    import importlib.util

    spec = importlib.util.spec_from_file_location("tach_harness", harness_path)
    harness = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(harness)

    with tempfile.TemporaryDirectory() as tmpdir:
        conftest_path = os.path.join(tmpdir, "conftest.py")
        with open(conftest_path, "w") as f:
            f.write("# Empty conftest - no hooks\n")

        mock_items = ["test_a", "test_b"]

        result = harness.call_collection_modifyitems(
            conftest_path=conftest_path,
            items=mock_items,
        )

        # Should return original items without error
        assert result["error"] is None
        assert result["new_order"] == ["test_a", "test_b"]
        assert result["reordered"] is False
        assert result["removed"] == []


def test_collection_modifyitems_with_session_config_params():
    """pytest_collection_modifyitems handles session and config parameters."""
    harness_path = os.path.join(
        os.path.dirname(__file__), "..", "..", "src", "tach_harness.py"
    )
    harness_path = os.path.abspath(harness_path)

    import importlib.util

    spec = importlib.util.spec_from_file_location("tach_harness", harness_path)
    harness = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(harness)

    with tempfile.TemporaryDirectory() as tmpdir:
        conftest_path = os.path.join(tmpdir, "conftest.py")
        with open(conftest_path, "w") as f:
            f.write(
                """
def pytest_collection_modifyitems(session, config, items):
    # Hook with all parameters - should still work
    items.reverse()
"""
            )

        mock_items = ["test_a", "test_b"]

        result = harness.call_collection_modifyitems(
            conftest_path=conftest_path,
            items=mock_items,
        )

        assert result["error"] is None
        assert result["new_order"] == ["test_b", "test_a"]
        assert result["reordered"] is True
