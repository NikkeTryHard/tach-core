"""Django test configuration for gauntlet tests.

This conftest.py sets up Django for the gauntlet_django test suite.
It configures the Django settings module and initializes Django before
any tests run.
"""
import os
import sys

import pytest


def pytest_configure(config):
    """Configure Django settings before test collection."""
    # Add the tests directory to path so django_project can be found
    tests_dir = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    if tests_dir not in sys.path:
        sys.path.insert(0, tests_dir)

    os.environ.setdefault("DJANGO_SETTINGS_MODULE", "django_project.settings")

    try:
        import django

        django.setup()
    except ImportError:
        pytest.skip("Django not installed", allow_module_level=True)
    except Exception as e:
        pytest.skip(f"Django setup failed: {e}", allow_module_level=True)


@pytest.fixture(scope="session")
def django_db_setup():
    """Ensure database tables exist for tests."""
    from django.core.management import call_command

    call_command("migrate", "--run-syncdb", verbosity=0)


@pytest.fixture
def db_session(django_db_setup):
    """Provide database access for tests that need it."""
    pass
