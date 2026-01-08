"""pytest-django fixtures for the Tach example project.

This module provides reusable fixtures for database tests.
"""

import pytest

from myapp.models import User, Task


@pytest.fixture
def sample_user(db):
    """Create a sample user for testing.

    Args:
        db: pytest-django fixture that enables database access.

    Returns:
        User: A persisted User instance.
    """
    return User.objects.create(
        username="testuser",
        email="testuser@example.com",
        is_active=True,
    )


@pytest.fixture
def inactive_user(db):
    """Create an inactive user for testing.

    Args:
        db: pytest-django fixture that enables database access.

    Returns:
        User: A persisted inactive User instance.
    """
    return User.objects.create(
        username="inactive",
        email="inactive@example.com",
        is_active=False,
    )


@pytest.fixture
def sample_task(db, sample_user):
    """Create a sample task for testing.

    Args:
        db: pytest-django fixture that enables database access.
        sample_user: User fixture for the task owner.

    Returns:
        Task: A persisted Task instance.
    """
    return Task.objects.create(
        title="Sample Task",
        description="A sample task for testing",
        owner=sample_user,
        status=Task.Status.PENDING,
    )


@pytest.fixture
def multiple_tasks(db, sample_user):
    """Create multiple tasks with different statuses.

    Args:
        db: pytest-django fixture that enables database access.
        sample_user: User fixture for the task owner.

    Returns:
        list[Task]: A list of Task instances with varied statuses.
    """
    tasks = []
    for i, status in enumerate(Task.Status.choices):
        task = Task.objects.create(
            title=f"Task {i + 1}",
            description=f"Description for task {i + 1}",
            owner=sample_user,
            status=status[0],
        )
        tasks.append(task)
    return tasks
