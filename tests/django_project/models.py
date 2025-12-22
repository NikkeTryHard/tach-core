"""Models for Django isolation testing."""

from django.db import models


class TestUser(models.Model):
    """Simple model to test transaction rollback isolation."""

    name = models.CharField(max_length=100)

    class Meta:
        app_label = "django_project"

    def __str__(self):
        return self.name
