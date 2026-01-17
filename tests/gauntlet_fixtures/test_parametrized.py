"""Test parametrized fixtures with Tach."""
import pytest

@pytest.fixture(params=[1, 2, 3])
def number_fixture(request):
    """Parametrized fixture providing multiple values."""
    return request.param

def test_parametrized_fixture(number_fixture):
    """This test runs 3 times with different values."""
    assert number_fixture in [1, 2, 3]
    assert isinstance(number_fixture, int)

@pytest.fixture(params=["a", "b"])
def letter_fixture(request):
    return request.param

def test_multiple_params(number_fixture, letter_fixture):
    """Cartesian product: 3 numbers x 2 letters = 6 runs."""
    assert number_fixture in [1, 2, 3]
    assert letter_fixture in ["a", "b"]
