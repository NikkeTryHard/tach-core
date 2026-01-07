"""Fixture patterns for tach-core.

This module demonstrates various fixture patterns including:
- Function-scoped fixtures
- Yield fixtures with cleanup
- Fixtures with return values
- Fixtures using other fixtures
"""

import pytest


@pytest.fixture
def sample_list():
    """A simple fixture that returns a list.

    Function-scoped by default, created fresh for each test.
    """
    return [1, 2, 3, 4, 5]


@pytest.fixture
def sample_dict():
    """A fixture that returns a dictionary."""
    return {
        "name": "test",
        "value": 42,
        "active": True,
    }


@pytest.fixture
def temp_data():
    """A yield fixture demonstrating setup and teardown.

    Code before yield is setup, code after is teardown.
    """
    # Setup
    data = {"initialized": True, "items": []}

    yield data

    # Teardown - runs after the test completes
    data.clear()


@pytest.fixture
def configured_object(sample_dict):
    """A fixture that depends on another fixture.

    Demonstrates fixture composition and dependency injection.
    """
    return {
        "config": sample_dict,
        "status": "ready",
    }


def test_using_list_fixture(sample_list):
    """Test using a simple list fixture."""
    assert len(sample_list) == 5
    assert sum(sample_list) == 15
    sample_list.append(6)
    assert len(sample_list) == 6


def test_using_dict_fixture(sample_dict):
    """Test using a dictionary fixture."""
    assert sample_dict["name"] == "test"
    assert sample_dict["value"] == 42
    assert sample_dict["active"] is True


def test_yield_fixture(temp_data):
    """Test using a yield fixture with setup/teardown."""
    assert temp_data["initialized"] is True
    temp_data["items"].append("item1")
    assert len(temp_data["items"]) == 1


def test_composed_fixture(configured_object):
    """Test using a fixture that depends on another fixture."""
    assert configured_object["status"] == "ready"
    assert configured_object["config"]["name"] == "test"


def test_multiple_fixtures(sample_list, sample_dict):
    """Test using multiple fixtures in the same test."""
    assert len(sample_list) == 5
    assert sample_dict["value"] == 42

    # Combine fixture data
    sample_dict["list_sum"] = sum(sample_list)
    assert sample_dict["list_sum"] == 15
