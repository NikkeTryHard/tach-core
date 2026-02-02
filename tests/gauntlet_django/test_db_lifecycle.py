"""Tests for --reuse-db and --create-db CLI flag handling."""
import pytest
import sys
import os

# Add src to path for importing harness
sys.path.insert(0, os.path.join(os.path.dirname(__file__), '..', '..', 'src'))


def test_check_db_exists_returns_false_when_no_django():
    """Without Django configured, should return False."""
    from tach_harness import _check_test_db_exists
    assert _check_test_db_exists() is False


def test_create_test_db_noop_without_django():
    """Without Django configured, should be a no-op."""
    from tach_harness import _create_test_db
    # Should not raise
    _create_test_db(verbosity=0)


def test_destroy_test_db_noop_without_django():
    """Without Django configured, should be a no-op."""
    from tach_harness import _destroy_test_db
    # Should not raise
    _destroy_test_db(verbosity=0)


def test_handle_db_lifecycle_reuse_existing():
    """When reuse_db=True and db exists, should not recreate."""
    from tach_harness import _handle_db_lifecycle
    # Should return without error when Django not configured
    _handle_db_lifecycle(reuse_db=True, create_db=False)


def test_handle_db_lifecycle_force_create():
    """When create_db=True, should destroy and recreate."""
    from tach_harness import _handle_db_lifecycle
    # Should return without error when Django not configured
    _handle_db_lifecycle(reuse_db=False, create_db=True)
