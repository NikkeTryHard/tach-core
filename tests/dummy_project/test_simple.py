"""Simple test functions."""


def test_simple():
    assert True


def test_with_fixture(my_fixture):
    assert my_fixture == 1


def test_multiple_deps(db, client, cache):
    """Test with multiple fixture dependencies."""
    pass
