"""Tests for discovery reconciliation between Rust AST and pytest collection."""


def test_run_test_falls_back_to_fuzzy_match():
    """When exact nodeid not found, try fuzzy matching by test name."""
    import sys
    from pathlib import Path

    src_dir = Path(__file__).parent.parent.parent / "src"
    sys.path.insert(0, str(src_dir))

    from tach_harness import _fuzzy_match_test

    # Simulate _ITEMS_MAP with a slightly different path
    items_map = {
        "tests/foo/test_bar.py::test_example[param1]": "mock_item_1",
        "tests/foo/test_bar.py::test_example[param2]": "mock_item_2",
    }

    # Requested nodeid has different path prefix
    requested = "test_bar.py::test_example[param1]"

    result = _fuzzy_match_test(requested, items_map)
    assert result is not None, "Should find fuzzy match"
    assert "param1" in result, f"Should match param1 variant: {result}"


def test_fuzzy_match_without_params():
    """Fuzzy matching works for tests without parameters."""
    import sys
    from pathlib import Path

    src_dir = Path(__file__).parent.parent.parent / "src"
    sys.path.insert(0, str(src_dir))

    from tach_harness import _fuzzy_match_test

    items_map = {
        "tests/integration/test_api.py::test_create_user": "mock_item",
        "tests/integration/test_api.py::test_delete_user": "mock_item_2",
    }

    # Match by function name only
    requested = "test_api.py::test_create_user"
    result = _fuzzy_match_test(requested, items_map)
    assert result is not None
    assert "test_create_user" in result


def test_fuzzy_match_returns_none_when_no_match():
    """Fuzzy matching returns None when no match found."""
    import sys
    from pathlib import Path

    src_dir = Path(__file__).parent.parent.parent / "src"
    sys.path.insert(0, str(src_dir))

    from tach_harness import _fuzzy_match_test

    items_map = {
        "tests/foo/test_bar.py::test_example": "mock_item",
    }

    requested = "test_nonexistent.py::test_something_else"
    result = _fuzzy_match_test(requested, items_map)
    assert result is None


def test_fuzzy_match_prefers_same_file_basename():
    """When multiple tests match, prefer the one with same file basename."""
    import sys
    from pathlib import Path

    src_dir = Path(__file__).parent.parent.parent / "src"
    sys.path.insert(0, str(src_dir))

    from tach_harness import _fuzzy_match_test

    items_map = {
        "tests/module_a/test_foo.py::test_example": "item_a",
        "tests/module_b/test_bar.py::test_example": "item_b",
    }

    # Should prefer test_foo.py match
    result = _fuzzy_match_test("different/path/test_foo.py::test_example", items_map)
    assert result == "tests/module_a/test_foo.py::test_example"
