"""Tests demonstrating nested conftest fixture inheritance.

These tests show how fixtures from different conftest.py levels
can be used together in test functions.
"""


def test_root_fixture_available(root_fixture):
    """Test that root-level fixtures are available in nested tests."""
    assert root_fixture["level"] == "root"
    assert root_fixture["value"] == "from_root_conftest"


def test_outer_fixture_available(outer_fixture):
    """Test that outer-level fixtures are available in inner tests."""
    assert outer_fixture["level"] == "outer"
    assert outer_fixture["value"] == "from_outer_conftest"


def test_inner_fixture_available(inner_fixture):
    """Test that inner-level fixtures are available."""
    assert inner_fixture["level"] == "inner"
    assert inner_fixture["value"] == "from_inner_conftest"


def test_all_levels_combined(full_hierarchy):
    """Test using a fixture that combines all three levels."""
    assert full_hierarchy["hierarchy_complete"] is True

    # Check root level
    assert full_hierarchy["root"]["level"] == "root"

    # Check outer level
    assert full_hierarchy["outer"]["level"] == "outer"

    # Check inner level
    assert full_hierarchy["inner"]["level"] == "inner"


def test_fixture_override(shared_config):
    """Test that the inner conftest overrides the root shared_config.

    The root conftest.py defines shared_config, but the inner conftest.py
    overrides it. Tests in this directory should get the overridden version.
    """
    assert shared_config["environment"] == "inner_test"
    assert shared_config["overridden"] is True


def test_combined_fixtures(combined_fixture):
    """Test using a fixture from outer that depends on root fixtures."""
    assert combined_fixture["combined"] is True
    assert combined_fixture["root"]["level"] == "root"
    assert combined_fixture["outer"]["level"] == "outer"


def test_multiple_fixtures(root_fixture, outer_fixture, inner_fixture):
    """Test using fixtures from all levels directly."""
    levels = [
        root_fixture["level"],
        outer_fixture["level"],
        inner_fixture["level"],
    ]
    assert levels == ["root", "outer", "inner"]


def test_resource_fixtures(outer_resource, inner_resource):
    """Test using resource fixtures from different levels."""
    assert outer_resource["name"] == "outer_resource"
    assert outer_resource["data"] == [1, 2, 3]

    assert inner_resource["name"] == "inner_resource"
    assert inner_resource["data"] == ["a", "b", "c"]


def test_connection_pool(connection_pool):
    """Test using the root-level connection pool fixture."""
    assert connection_pool["size"] == 10
    assert connection_pool["active"] == 0

    # Simulate using a connection
    connection_pool["connections"].append("conn1")
    connection_pool["active"] = 1

    assert len(connection_pool["connections"]) == 1
