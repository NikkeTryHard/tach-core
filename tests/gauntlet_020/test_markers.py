"""Test marker detection and filtering."""
import pytest


@pytest.mark.slow
def test_marked_as_slow():
    """This test has a custom marker."""
    assert True


@pytest.mark.integration
def test_marked_as_integration():
    """Another custom marker."""
    assert True


@pytest.mark.slow
@pytest.mark.integration
def test_multiple_markers():
    """Test with multiple markers."""
    assert True


def test_no_markers():
    """Test without markers."""
    assert True
