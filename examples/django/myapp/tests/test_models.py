"""Model tests for the Tach Django example.

These tests demonstrate database testing patterns with Tach.
All tests use @pytest.mark.django_db to enable database access.
"""

import pytest

from myapp.models import User, Task


# =============================================================================
# User Model Tests
# =============================================================================


@pytest.mark.django_db
class TestUserModel:
    """Tests for the User model."""

    def test_create_user(self):
        """User can be created with valid data."""
        user = User.objects.create(
            username="newuser",
            email="newuser@example.com",
        )
        assert user.id is not None
        assert user.username == "newuser"
        assert user.email == "newuser@example.com"
        assert user.is_active is True

    def test_user_string_representation(self, sample_user):
        """User string representation returns username."""
        assert str(sample_user) == "testuser"

    def test_deactivate_user(self, sample_user):
        """User.deactivate() sets is_active to False."""
        assert sample_user.is_active is True
        sample_user.deactivate()
        sample_user.refresh_from_db()
        assert sample_user.is_active is False

    def test_user_unique_username(self, sample_user):
        """Username must be unique."""
        with pytest.raises(Exception):
            User.objects.create(
                username="testuser",
                email="different@example.com",
            )

    def test_user_unique_email(self, sample_user):
        """Email must be unique."""
        with pytest.raises(Exception):
            User.objects.create(
                username="different",
                email="testuser@example.com",
            )


# =============================================================================
# Task Model Tests
# =============================================================================


@pytest.mark.django_db
class TestTaskModel:
    """Tests for the Task model."""

    def test_create_task(self, sample_user):
        """Task can be created with valid data."""
        task = Task.objects.create(
            title="New Task",
            description="Task description",
            owner=sample_user,
        )
        assert task.id is not None
        assert task.title == "New Task"
        assert task.status == Task.Status.PENDING

    def test_task_string_representation(self, sample_task):
        """Task string representation returns title."""
        assert str(sample_task) == "Sample Task"

    def test_task_default_status(self, sample_user):
        """New tasks default to PENDING status."""
        task = Task.objects.create(
            title="Default Status Task",
            owner=sample_user,
        )
        assert task.status == Task.Status.PENDING

    def test_mark_task_completed(self, sample_task):
        """Task.mark_completed() updates status."""
        assert sample_task.status == Task.Status.PENDING
        sample_task.mark_completed()
        sample_task.refresh_from_db()
        assert sample_task.status == Task.Status.COMPLETED

    def test_mark_task_in_progress(self, sample_task):
        """Task.mark_in_progress() updates status."""
        assert sample_task.status == Task.Status.PENDING
        sample_task.mark_in_progress()
        sample_task.refresh_from_db()
        assert sample_task.status == Task.Status.IN_PROGRESS

    def test_task_owner_relationship(self, sample_task, sample_user):
        """Task has correct owner relationship."""
        assert sample_task.owner == sample_user
        assert sample_task in sample_user.tasks.all()

    def test_cascade_delete(self, sample_task, sample_user):
        """Deleting user cascades to tasks."""
        task_id = sample_task.id
        sample_user.delete()
        assert not Task.objects.filter(id=task_id).exists()


# =============================================================================
# Query Tests
# =============================================================================


@pytest.mark.django_db
class TestQueries:
    """Tests for database queries and filters."""

    def test_filter_active_users(self, sample_user, inactive_user):
        """Can filter users by is_active status."""
        active_users = User.objects.filter(is_active=True)
        inactive_users = User.objects.filter(is_active=False)

        assert sample_user in active_users
        assert inactive_user in inactive_users
        assert sample_user not in inactive_users

    def test_filter_tasks_by_status(self, multiple_tasks):
        """Can filter tasks by status."""
        pending = Task.objects.filter(status=Task.Status.PENDING)
        completed = Task.objects.filter(status=Task.Status.COMPLETED)

        assert pending.count() >= 1
        assert completed.count() >= 1

    def test_user_task_count(self, sample_user, multiple_tasks):
        """Can count tasks for a user."""
        task_count = sample_user.tasks.count()
        assert task_count == len(multiple_tasks)

    def test_ordering_by_created_at(self, sample_user):
        """Tasks are ordered by created_at descending."""
        Task.objects.create(title="First", owner=sample_user)
        Task.objects.create(title="Second", owner=sample_user)
        Task.objects.create(title="Third", owner=sample_user)

        tasks = list(sample_user.tasks.all())
        assert tasks[0].title == "Third"
        assert tasks[-1].title == "First"


# =============================================================================
# Transaction Tests
# =============================================================================


@pytest.mark.django_db(transaction=True)
class TestTransactions:
    """Tests demonstrating transaction behavior."""

    def test_atomic_operations(self, sample_user):
        """Database operations are atomic within a test."""
        initial_count = Task.objects.count()

        Task.objects.create(title="Task A", owner=sample_user)
        Task.objects.create(title="Task B", owner=sample_user)

        assert Task.objects.count() == initial_count + 2

    def test_isolation_between_tests(self):
        """Each test starts with a clean database state."""
        # This test runs after others but should see no users
        # (except any created by migrations)
        user_count = User.objects.count()
        # Create a user in this test
        User.objects.create(username="isolated", email="isolated@example.com")
        assert User.objects.count() == user_count + 1
