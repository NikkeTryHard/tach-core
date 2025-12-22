"""Django Isolation Proof Tests

These tests prove that transaction rollback isolation works correctly.
Each test creates a unique record and asserts the total count is 1.

If isolation FAILS, one test will see records from another test
and the count assertion will fail.

The tests are designed to run in parallel - if they pass,
it proves Django DB isolation is working.
"""

from tests.django_project.models import TestUser


def test_create_alice():
    """Create Alice. Should see exactly 1 record."""
    TestUser.objects.create(name="Alice")
    count = TestUser.objects.count()
    assert count == 1, f"Expected 1 user, got {count}. Isolation failure!"


def test_create_bob():
    """Create Bob. Should see exactly 1 record (not Alice)."""
    TestUser.objects.create(name="Bob")
    count = TestUser.objects.count()
    assert count == 1, f"Expected 1 user, got {count}. Isolation failure!"


def test_create_charlie():
    """Create Charlie. Should see exactly 1 record (not Alice or Bob)."""
    TestUser.objects.create(name="Charlie")
    count = TestUser.objects.count()
    assert count == 1, f"Expected 1 user, got {count}. Isolation failure!"
