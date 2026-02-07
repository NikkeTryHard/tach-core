"""Regression test for subdirectory rootdir node ID alignment (issue #67).

When tach runs from the project root but the test suite is in a subdirectory
with its own pyproject.toml, pytest sets rootdir to that subdirectory. This
causes pytest node IDs to differ from Rust-discovered node IDs:

  Rust:   tests/regression/subdir_rootdir/tests/test_basic.py::test_simple
  Pytest: tests/test_basic.py::test_simple

The dual-key fix in init_session() stores both forms in _ITEMS_MAP so the
O(1) lookup in run_test() succeeds regardless.
"""


def test_simple():
    """Simplest possible test — validates node ID lookup works."""
    assert 1 + 1 == 2


def test_string():
    """Another basic test to confirm multiple tests in a file work."""
    assert "hello".upper() == "HELLO"


class TestClass:
    """Verify class-based tests also get correct dual-key node IDs."""

    def test_method(self):
        assert [1, 2, 3][-1] == 3

    def test_another_method(self):
        assert isinstance({}, dict)
