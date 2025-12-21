import pytest


@pytest.fixture
def my_fixture():
    return 1


def test_example(my_fixture):
    assert my_fixture == 1


def test_another():
    assert True
