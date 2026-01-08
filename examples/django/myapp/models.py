"""Django models for the Tach example project.

This module defines simple User and Task models to demonstrate
database testing with Tach.
"""

from django.db import models


class User(models.Model):
    """A simple user model for testing."""

    username = models.CharField(max_length=150, unique=True)
    email = models.EmailField(unique=True)
    created_at = models.DateTimeField(auto_now_add=True)
    is_active = models.BooleanField(default=True)

    class Meta:
        app_label = "myapp"

    def __str__(self) -> str:
        return self.username

    def deactivate(self) -> None:
        """Deactivate the user account."""
        self.is_active = False
        self.save()


class Task(models.Model):
    """A task model with a foreign key to User."""

    class Status(models.TextChoices):
        PENDING = "pending", "Pending"
        IN_PROGRESS = "in_progress", "In Progress"
        COMPLETED = "completed", "Completed"

    title = models.CharField(max_length=200)
    description = models.TextField(blank=True)
    owner = models.ForeignKey(User, on_delete=models.CASCADE, related_name="tasks")
    status = models.CharField(max_length=20, choices=Status.choices, default=Status.PENDING)
    created_at = models.DateTimeField(auto_now_add=True)
    updated_at = models.DateTimeField(auto_now=True)

    class Meta:
        app_label = "myapp"
        ordering = ["-created_at"]

    def __str__(self) -> str:
        return self.title

    def mark_completed(self) -> None:
        """Mark the task as completed."""
        self.status = self.Status.COMPLETED
        self.save()

    def mark_in_progress(self) -> None:
        """Mark the task as in progress."""
        self.status = self.Status.IN_PROGRESS
        self.save()
