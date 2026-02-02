"""Integration tests for Django fixture injection (Issue #39)."""
import pytest
import sys
import os

sys.path.insert(0, os.path.join(os.path.dirname(__file__), '..', '..', 'src'))

from tach_harness import _is_django_available

pytestmark = pytest.mark.skipif(
    not _is_django_available(),
    reason="Django not configured"
)


@pytest.mark.django_db
def test_all_fixtures_registered():
    """Verify all Django fixtures are registered in the fixture registry."""
    from tach_harness import _FIXTURE_REGISTRY

    expected_fixtures = [
        "db", "client", "rf",
        "admin_client", "admin_user",
        "django_user_model", "django_username_field",
        "settings", "transactional_db", "live_server"
    ]

    for fixture_name in expected_fixtures:
        assert fixture_name in _FIXTURE_REGISTRY, f"Missing fixture: {fixture_name}"


def test_fixture_init_cleanup_pairs():
    """Each fixture should have both init and cleanup functions."""
    from tach_harness import _FIXTURE_REGISTRY

    for name, (init_fn, cleanup_fn) in _FIXTURE_REGISTRY.items():
        assert callable(init_fn), f"{name} init is not callable"
        assert callable(cleanup_fn), f"{name} cleanup is not callable"
        assert init_fn.__name__.startswith("_init_"), f"{name} init naming convention"
        assert cleanup_fn.__name__.startswith("_cleanup_"), f"{name} cleanup naming convention"


@pytest.mark.django_db
def test_multiple_fixtures_work_together():
    """Test that multiple fixtures can be initialized and cleaned up."""
    from tach_harness import (
        _init_client_fixture, _cleanup_client_fixture,
        _init_rf_fixture, _cleanup_rf_fixture,
        _init_settings_fixture, _cleanup_settings_fixture,
        _DJANGO_FIXTURES
    )

    # Initialize multiple fixtures
    client = _init_client_fixture()
    rf = _init_rf_fixture()
    settings = _init_settings_fixture()

    try:
        from django.test import Client, RequestFactory

        assert isinstance(client, Client)
        assert isinstance(rf, RequestFactory)
        assert "client" in _DJANGO_FIXTURES
        assert "rf" in _DJANGO_FIXTURES
        assert "settings" in _DJANGO_FIXTURES
    finally:
        _cleanup_settings_fixture()
        _cleanup_rf_fixture()
        _cleanup_client_fixture()

    # Verify cleanup
    assert "client" not in _DJANGO_FIXTURES
    assert "rf" not in _DJANGO_FIXTURES
    assert "settings" not in _DJANGO_FIXTURES
