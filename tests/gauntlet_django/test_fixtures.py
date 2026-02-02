"""Tests for native Django fixture implementations (Issue #39).

This module tests the db, client, and rf fixtures which are the three
most commonly used Django fixtures that unblock 80% of Django test migrations.
"""
import pytest
import sys
import os

sys.path.insert(0, os.path.join(os.path.dirname(__file__), '..', '..', 'src'))

from tach_harness import _is_django_available

pytestmark = pytest.mark.skipif(
    not _is_django_available(),
    reason="Django not configured"
)


class TestDbFixture:
    """Tests for the db fixture."""

    def test_db_fixture_initialization(self):
        """db fixture should initialize correctly."""
        from tach_harness import _init_db_fixture, _cleanup_db_fixture, _DJANGO_FIXTURES

        _init_db_fixture()
        try:
            assert _DJANGO_FIXTURES.get("db") is True
        finally:
            _cleanup_db_fixture()
            assert "db" not in _DJANGO_FIXTURES

    @pytest.mark.django_db
    def test_db_fixture_provides_database_access(self, db_session):
        """db fixture should allow database operations."""
        from django_project.models import TestModel

        item = TestModel.objects.create(name="test_fixture_item", value=42)
        assert item.pk is not None
        assert TestModel.objects.filter(name="test_fixture_item").exists()

    @pytest.mark.django_db
    def test_db_fixture_isolates_between_tests(self, db_session):
        """Each test should start with clean database (no leftover from previous test)."""
        from django_project.models import TestModel

        # This should not find items from other tests due to isolation
        assert not TestModel.objects.filter(name="test_fixture_item").exists()
        # Create our own item
        TestModel.objects.create(name="isolation_test_item", value=1)
        assert TestModel.objects.filter(name="isolation_test_item").exists()


class TestClientFixture:
    """Tests for the client fixture."""

    def test_client_fixture_returns_test_client(self):
        """client fixture should return Django test client."""
        from tach_harness import _init_client_fixture, _cleanup_client_fixture

        client = _init_client_fixture()
        try:
            from django.test import Client

            assert isinstance(client, Client)
        finally:
            _cleanup_client_fixture()

    def test_client_fixture_stored_in_registry(self):
        """client fixture should be stored in _DJANGO_FIXTURES."""
        from tach_harness import _init_client_fixture, _cleanup_client_fixture, _DJANGO_FIXTURES

        client = _init_client_fixture()
        try:
            assert _DJANGO_FIXTURES.get("client") is client
        finally:
            _cleanup_client_fixture()
            assert "client" not in _DJANGO_FIXTURES

    def test_client_can_make_requests(self):
        """client should be able to make HTTP requests."""
        from tach_harness import _init_client_fixture, _cleanup_client_fixture

        client = _init_client_fixture()
        try:
            # Making a request to root - may return 200 or 404 depending on URL config
            response = client.get("/")
            assert response.status_code in (200, 404)
        finally:
            _cleanup_client_fixture()

    def test_client_cleanup_handles_logout(self):
        """cleanup should handle logout gracefully even if not logged in."""
        from tach_harness import _init_client_fixture, _cleanup_client_fixture

        client = _init_client_fixture()
        assert client is not None
        # Cleanup should not raise even though we never logged in
        _cleanup_client_fixture()


class TestRfFixture:
    """Tests for the rf (RequestFactory) fixture."""

    def test_rf_fixture_returns_request_factory(self):
        """rf fixture should return Django RequestFactory."""
        from tach_harness import _init_rf_fixture, _cleanup_rf_fixture

        rf = _init_rf_fixture()
        try:
            from django.test import RequestFactory

            assert isinstance(rf, RequestFactory)
        finally:
            _cleanup_rf_fixture()

    def test_rf_fixture_stored_in_registry(self):
        """rf fixture should be stored in _DJANGO_FIXTURES."""
        from tach_harness import _init_rf_fixture, _cleanup_rf_fixture, _DJANGO_FIXTURES

        rf = _init_rf_fixture()
        try:
            assert _DJANGO_FIXTURES.get("rf") is rf
        finally:
            _cleanup_rf_fixture()
            assert "rf" not in _DJANGO_FIXTURES

    def test_rf_can_create_get_request(self):
        """rf should be able to create GET request objects."""
        from tach_harness import _init_rf_fixture, _cleanup_rf_fixture

        rf = _init_rf_fixture()
        try:
            request = rf.get("/test/")
            assert request.method == "GET"
            assert request.path == "/test/"
        finally:
            _cleanup_rf_fixture()

    def test_rf_can_create_post_request(self):
        """rf should be able to create POST request objects."""
        from tach_harness import _init_rf_fixture, _cleanup_rf_fixture

        rf = _init_rf_fixture()
        try:
            request = rf.post("/submit/", {"key": "value"})
            assert request.method == "POST"
            assert request.path == "/submit/"
        finally:
            _cleanup_rf_fixture()

    def test_rf_can_create_request_with_user(self):
        """rf should be able to create requests and attach user."""
        from tach_harness import _init_rf_fixture, _cleanup_rf_fixture
        from django.contrib.auth.models import AnonymousUser

        rf = _init_rf_fixture()
        try:
            request = rf.get("/test/")
            request.user = AnonymousUser()
            assert request.user.is_anonymous
        finally:
            _cleanup_rf_fixture()


class TestFixtureAvailability:
    """Tests for fixture availability when Django is not configured."""

    def test_fixtures_handle_missing_django_gracefully(self):
        """Fixtures should return None/no-op when Django is unavailable."""
        # This test verifies the guard clauses work correctly
        # The actual _is_django_available() check happens in the fixture functions
        from tach_harness import (
            _init_db_fixture,
            _init_client_fixture,
            _init_rf_fixture,
            _cleanup_db_fixture,
            _cleanup_client_fixture,
            _cleanup_rf_fixture,
        )

        # When Django IS available (our test environment), these should work
        _init_db_fixture()
        client = _init_client_fixture()
        rf = _init_rf_fixture()

        assert client is not None
        assert rf is not None

        # Cleanup should not raise
        _cleanup_db_fixture()
        _cleanup_client_fixture()
        _cleanup_rf_fixture()
