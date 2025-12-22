"""Minimal Django settings for Tach DB isolation testing."""

SECRET_KEY = "tach-test-secret-key-not-for-production"

INSTALLED_APPS = [
    "django.contrib.contenttypes",
    "tests.django_project",
]

# Use file-based SQLite to avoid SQLite fork issues
# :memory: databases don't survive across fork() properly
DATABASES = {
    "default": {
        "ENGINE": "django.db.backends.sqlite3",
        "NAME": "/tmp/tach_django_test.db",
    }
}

DEFAULT_AUTO_FIELD = "django.db.models.BigAutoField"

# Disable migrations for faster test setup
MIGRATION_MODULES = {"django_project": None}
