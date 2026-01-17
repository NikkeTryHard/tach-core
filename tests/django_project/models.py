"""Django models for gauntlet tests.

Simple models used to verify database isolation in tests.
"""
from django.db import models


class TestModel(models.Model):
    """Simple model for testing database isolation.

    Each test creates records with unique names. If isolation is working,
    records from one test should not be visible in other tests.
    """

    name = models.CharField(max_length=255, unique=True)
    value = models.IntegerField(default=0)
    created_at = models.DateTimeField(auto_now_add=True)

    class Meta:
        app_label = "django_project"

    def __str__(self):
        return f"TestModel({self.name}, {self.value})"


class TestUser(models.Model):
    """User model for testing user-related functionality."""

    name = models.CharField(max_length=255)
    email = models.EmailField(blank=True)

    class Meta:
        app_label = "django_project"

    def __str__(self):
        return f"TestUser({self.name})"
