"""Verification test: Assert failure should report FAIL with traceback."""


def test_assert_failure():
    """Should report FAIL with traceback showing '1 == 2'."""
    assert 1 == 2
