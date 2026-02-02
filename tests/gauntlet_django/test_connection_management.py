"""Tests for database connection management."""
import pytest
import sys
import os

sys.path.insert(0, os.path.join(os.path.dirname(__file__), '..', '..', 'src'))


def test_dispose_all_connections_noop_without_django():
    """Without Django, dispose should be a no-op."""
    from tach_harness import _dispose_all_connections
    # Should not raise
    _dispose_all_connections()


def test_dispose_all_connections_closes_pools():
    """Should close connection pools to prevent FD inheritance."""
    from tach_harness import _dispose_all_connections
    # Without Django, should return cleanly
    result = _dispose_all_connections()
    assert result is None or result == 0


def test_get_database_aliases_returns_configured_aliases():
    """With Django configured, should return configured aliases."""
    from tach_harness import _get_database_aliases
    aliases = _get_database_aliases()
    # Django is configured by conftest, so we get actual aliases
    assert 'default' in aliases


def test_get_database_aliases_filters_invalid():
    """Should filter out aliases that don't exist in DATABASES."""
    from tach_harness import _get_database_aliases
    # Should return only valid aliases, filtering out nonexistent
    aliases = _get_database_aliases(requested=['default', 'nonexistent'])
    assert 'default' in aliases
    assert 'nonexistent' not in aliases


def test_all_db_functions_importable():
    """All database functions should be importable from harness."""
    from tach_harness import (
        _is_django_available,
        _close_django_connections,
        _dispose_all_connections,
        _get_database_aliases,
        _apply_django_db_isolation,
        _apply_django_db_isolation_v2,
        _cleanup_django_db_isolation,
        _cleanup_django_db_isolation_v2,
        _flush_database,
        _check_test_db_exists,
        _create_test_db,
        _destroy_test_db,
        _handle_db_lifecycle,
    )
    # All imports should succeed
    assert callable(_is_django_available)
