"""Tests for @pytest.mark.django_db marker argument support.

These tests verify that the marker argument parsing and database isolation
work correctly for various django_db marker configurations.
"""
import pytest


# Skip all tests if Django is not available
django = pytest.importorskip("django")


@pytest.mark.django_db
def test_default_transaction_rollback():
    """Default: transaction=False (implicit), should rollback.

    This test creates a record and verifies it exists. Due to SAVEPOINT
    isolation, this record will be rolled back after the test completes,
    ensuring no pollution for other tests.
    """
    from django_project.models import TestModel

    # Create a unique record
    TestModel.objects.create(name="DefaultRollbackTest", value=100)

    # Verify it exists within this test
    assert TestModel.objects.filter(name="DefaultRollbackTest").exists()
    count = TestModel.objects.filter(name="DefaultRollbackTest").count()
    assert count == 1, f"Expected 1 record, got {count}"


@pytest.mark.django_db(transaction=False)
def test_explicit_transaction_false():
    """Explicit transaction=False, should rollback.

    Same as default behavior - uses SAVEPOINT for isolation.
    """
    from django_project.models import TestModel

    TestModel.objects.create(name="ExplicitFalseTest", value=200)

    assert TestModel.objects.filter(name="ExplicitFalseTest").exists()
    count = TestModel.objects.filter(name="ExplicitFalseTest").count()
    assert count == 1


@pytest.mark.django_db
def test_isolation_from_previous_tests():
    """Verify this test doesn't see records from previous tests.

    If isolation is working correctly, records created by other tests
    should have been rolled back and not visible here.
    """
    from django_project.models import TestModel

    # These names were used in previous tests - they should NOT exist
    # if SAVEPOINT isolation is working correctly
    assert not TestModel.objects.filter(name="DefaultRollbackTest").exists(), \
        "Record from test_default_transaction_rollback should have been rolled back"
    assert not TestModel.objects.filter(name="ExplicitFalseTest").exists(), \
        "Record from test_explicit_transaction_false should have been rolled back"


@pytest.mark.django_db
def test_multiple_records_isolated():
    """Create multiple records and verify they're isolated."""
    from django_project.models import TestModel

    # Create several records
    for i in range(5):
        TestModel.objects.create(name=f"MultiRecord_{i}", value=i)

    # All should exist within this test
    count = TestModel.objects.filter(name__startswith="MultiRecord_").count()
    assert count == 5, f"Expected 5 records, got {count}"


@pytest.mark.django_db
def test_no_multi_records_pollution():
    """Verify previous test's records don't pollute this test."""
    from django_project.models import TestModel

    count = TestModel.objects.filter(name__startswith="MultiRecord_").count()
    assert count == 0, f"Expected 0 records (isolation), got {count}"


# Note: transaction=True tests require table truncation and are marked
# as toxic. Deferred to 0.3.0 per the plan.
