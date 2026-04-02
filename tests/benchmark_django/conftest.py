import os
import sys

import pytest

TESTS_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def pytest_configure(config):
    if TESTS_DIR not in sys.path:
        sys.path.insert(0, TESTS_DIR)

    os.environ["DJANGO_SETTINGS_MODULE"] = "django_project.settings"
    os.environ["PYTHONPATH"] = TESTS_DIR + os.pathsep + os.environ.get("PYTHONPATH", "")

    try:
        import django

        django.setup()
    except ImportError:
        pytest.skip("Django not installed", allow_module_level=True)
    except Exception as e:
        pytest.skip(f"Django setup failed: {e}", allow_module_level=True)


@pytest.fixture(scope="session")
def django_db_setup(django_test_environment, django_db_blocker):
    from django.db import connections
    from django.core.management import call_command

    with django_db_blocker.unblock():
        connections["default"].creation.create_test_db(verbosity=0, autoclobber=True)
        call_command("migrate", "--run-syncdb", verbosity=0)

    yield

    with django_db_blocker.unblock():
        for conn in connections.all():
            conn.close()


def pytest_collection_modifyitems(items):
    for item in items:
        if "benchmark_django" in str(item.fspath):
            item.add_marker(pytest.mark.django_db(transaction=True))
