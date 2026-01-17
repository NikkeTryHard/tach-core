"""Minimal Django settings for gauntlet tests.

This is a minimal Django configuration using SQLite file-based database
for testing with transaction isolation.
"""
import os

# Build paths inside the project
BASE_DIR = os.path.dirname(os.path.abspath(__file__))

# SECURITY WARNING: keep the secret key used in production secret!
SECRET_KEY = "test-secret-key-not-for-production"

# SECURITY WARNING: don't run with debug turned on in production!
DEBUG = True

ALLOWED_HOSTS = ["*"]

# Application definition
INSTALLED_APPS = [
    "django.contrib.contenttypes",
    "django.contrib.auth",
    "django_project",  # Our test app with models
]

# Database - use file-based SQLite for persistence across connections
DATABASES = {
    "default": {
        "ENGINE": "django.db.backends.sqlite3",
        "NAME": os.path.join(BASE_DIR, "test_db.sqlite3"),
        "OPTIONS": {
            "timeout": 20,
        },
        "ATOMIC_REQUESTS": False,
    }
}

# Internationalization
USE_TZ = True
TIME_ZONE = "UTC"

# Default primary key field type
DEFAULT_AUTO_FIELD = "django.db.models.BigAutoField"
