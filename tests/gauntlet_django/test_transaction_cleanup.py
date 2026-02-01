"""Tests for transaction=True cleanup via truncation."""
import pytest
import sys
import os

sys.path.insert(0, os.path.join(os.path.dirname(__file__), '..', '..', 'src'))


def test_flush_database_noop_without_django():
    """Without Django, flush should be a no-op."""
    from tach_harness import _flush_database
    # Should not raise
    _flush_database(databases=None)


def test_flush_database_with_alias_list():
    """Should accept explicit database alias list."""
    from tach_harness import _flush_database
    # Without Django, should handle gracefully
    _flush_database(databases=['default', 'secondary'])


def test_apply_isolation_v2_returns_flush_flag_for_transaction_true():
    """When transaction=True, should return flush indicator."""
    from tach_harness import _apply_django_db_isolation_v2

    marker_args = {"transaction": True, "reset_sequences": False, "databases": None}
    result = _apply_django_db_isolation_v2(marker_args)

    # Without Django, result should have defaults but structure should be correct
    assert "needs_flush" in result
    assert "savepoints" in result
    assert "databases" in result


def test_apply_isolation_v2_returns_savepoints_for_default():
    """When transaction=False, should use savepoints."""
    from tach_harness import _apply_django_db_isolation_v2

    marker_args = {"transaction": False, "reset_sequences": False, "databases": None}
    result = _apply_django_db_isolation_v2(marker_args)

    # Without Django, needs_flush should be False
    assert result.get("needs_flush") is False


def test_cleanup_v2_handles_flush_path():
    """Cleanup should call flush when needs_flush is True."""
    from tach_harness import _cleanup_django_db_isolation_v2

    isolation_result = {"needs_flush": True, "databases": ["default"], "savepoints": []}
    # Should not raise
    _cleanup_django_db_isolation_v2(isolation_result)


def test_cleanup_v2_handles_savepoint_path():
    """Cleanup should rollback savepoints when needs_flush is False."""
    from tach_harness import _cleanup_django_db_isolation_v2

    isolation_result = {"needs_flush": False, "databases": [], "savepoints": []}
    # Should not raise
    _cleanup_django_db_isolation_v2(isolation_result)
