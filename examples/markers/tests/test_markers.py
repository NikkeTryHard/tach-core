"""Custom marker patterns for tach-core.

This module demonstrates pytest markers and how to filter tests with -m flag.
"""

import pytest


# Define custom markers in conftest.py or pytest.ini
# For this example, we use the markers directly


@pytest.mark.slow
def test_slow_operation():
    """A test marked as slow.

    Run with: tach-core -m "slow" examples/markers/tests/
    Skip with: tach-core -m "not slow" examples/markers/tests/
    """
    # Simulate slow operation
    result = sum(range(100000))
    assert result == 4999950000


@pytest.mark.fast
def test_fast_operation():
    """A test marked as fast.

    Run with: tach-core -m "fast" examples/markers/tests/
    """
    assert 1 + 1 == 2


@pytest.mark.integration
def test_integration_example():
    """A test marked as integration test.

    Run with: tach-core -m "integration" examples/markers/tests/
    Skip with: tach-core -m "not integration" examples/markers/tests/
    """
    # Simulate integration test
    components = ["database", "api", "cache"]
    connected = all(c for c in components)
    assert connected


@pytest.mark.unit
def test_unit_example():
    """A test marked as unit test.

    Run with: tach-core -m "unit" examples/markers/tests/
    """

    def add(a, b):
        return a + b

    assert add(2, 3) == 5


@pytest.mark.smoke
def test_smoke_basic():
    """A smoke test - basic sanity check.

    Smoke tests verify that critical functionality works.
    Run with: tach-core -m "smoke" examples/markers/tests/
    """
    assert True


@pytest.mark.smoke
@pytest.mark.fast
def test_smoke_and_fast():
    """A test with multiple markers.

    Run with: tach-core -m "smoke" examples/markers/tests/
    Or: tach-core -m "fast" examples/markers/tests/
    Or: tach-core -m "smoke and fast" examples/markers/tests/
    """
    assert 1 < 2


@pytest.mark.network
def test_network_dependent():
    """A test that requires network access.

    Skip when no network: tach-core -m "not network" examples/markers/tests/
    """
    # In real tests, this might make HTTP requests
    # Here we just simulate
    response = {"status": 200}
    assert response["status"] == 200


@pytest.mark.database
def test_database_dependent():
    """A test that requires database access.

    Skip when no database: tach-core -m "not database" examples/markers/tests/
    """
    # Simulate database operation
    records = [{"id": 1}, {"id": 2}]
    assert len(records) == 2


@pytest.mark.skip(reason="Demonstrating skip marker")
def test_skipped():
    """A test that is always skipped.

    Use @pytest.mark.skip to unconditionally skip tests.
    """
    assert False  # This never runs


@pytest.mark.skipif(condition=True, reason="Demonstrating conditional skip")
def test_conditional_skip():
    """A test that is conditionally skipped.

    Use @pytest.mark.skipif for conditional skipping based on
    platform, version, or other runtime conditions.
    """
    assert False  # This never runs when condition is True


@pytest.mark.xfail(reason="Demonstrating expected failure")
def test_expected_failure():
    """A test that is expected to fail.

    Use @pytest.mark.xfail when a test is known to fail but
    you want to track it without failing the test suite.
    """
    assert False  # Expected to fail


@pytest.mark.xfail(reason="Bug fixed, test should now pass", strict=True)
def test_xfail_strict():
    """A strict xfail test.

    With strict=True, if the test passes, it will be reported
    as a failure (useful when fixing bugs).
    """
    assert True  # This passes, but strict=True makes it "XPASS"


# Multiple markers combined
@pytest.mark.slow
@pytest.mark.integration
@pytest.mark.database
def test_slow_integration_with_db():
    """A test with multiple markers for complex filtering.

    Run only slow integration tests: tach-core -m "slow and integration"
    Run database tests that are fast: tach-core -m "database and fast"
    Skip slow or database tests: tach-core -m "not (slow or database)"
    """
    # Simulate slow database integration
    data = list(range(1000))
    assert len(data) == 1000


def test_unmarked():
    """A test without any markers.

    This test runs unless excluded by marker expressions.
    """
    assert "unmarked" == "unmarked"
