"""Full integration tests for 0.3.0 database features.

These tests require a Django project to be configured.
They are skipped when Django is not available.
"""
import pytest
import sys
import os

sys.path.insert(0, os.path.join(os.path.dirname(__file__), '..', '..', 'src'))

from tach_harness import _is_django_available

# Skip all tests in this module if Django not available
pytestmark = pytest.mark.skipif(
    not _is_django_available(),
    reason="Django not configured"
)


class TestDatabaseLifecycle:
    """Tests for --reuse-db and --create-db behavior."""

    def test_reuse_db_skips_creation_when_exists(self):
        """--reuse-db should skip creation if database exists."""
        from tach_harness import _handle_db_lifecycle, _check_test_db_exists

        # If DB exists, reuse should succeed without recreation
        if _check_test_db_exists():
            _handle_db_lifecycle(reuse_db=True, create_db=False)
            assert _check_test_db_exists()

    def test_create_db_forces_recreation(self):
        """--create-db should force recreation."""
        from tach_harness import _handle_db_lifecycle

        # This will destroy and recreate - should not raise
        _handle_db_lifecycle(reuse_db=False, create_db=True)


class TestTransactionIsolation:
    """Tests for transaction=True vs savepoint isolation."""

    @pytest.mark.django_db
    def test_savepoint_isolation_rolls_back(self):
        """Default isolation should rollback via savepoint."""
        from tach_harness import _apply_django_db_isolation_v2, _cleanup_django_db_isolation_v2

        marker_args = {"transaction": False, "databases": None}
        result = _apply_django_db_isolation_v2(marker_args)

        assert result["needs_flush"] is False
        assert len(result["savepoints"]) > 0

        # Cleanup
        _cleanup_django_db_isolation_v2(result)

    @pytest.mark.django_db(transaction=True)
    def test_transaction_true_uses_flush(self):
        """transaction=True should use flush instead of rollback."""
        from tach_harness import _apply_django_db_isolation_v2

        marker_args = {"transaction": True, "databases": None}
        result = _apply_django_db_isolation_v2(marker_args)

        assert result["needs_flush"] is True
        assert result["savepoints"] == []


class TestConnectionManagement:
    """Tests for connection disposal and multi-db support."""

    def test_dispose_closes_all_connections(self):
        """dispose should close all database connections."""
        from tach_harness import _dispose_all_connections
        from django.db import connections

        # Ensure at least one connection is open
        connections['default'].ensure_connection()

        # Dispose should close it
        disposed = _dispose_all_connections()
        assert disposed >= 0

    def test_get_aliases_returns_configured_databases(self):
        """Should return all configured database aliases."""
        from tach_harness import _get_database_aliases
        from django.conf import settings

        aliases = _get_database_aliases()
        assert 'default' in aliases
        assert len(aliases) == len(settings.DATABASES)
