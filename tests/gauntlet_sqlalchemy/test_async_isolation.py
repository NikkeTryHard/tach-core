"""Test async SQLAlchemy isolation functions."""
import pytest
import asyncio


def test_apply_sqlalchemy_isolation_async_exists():
    """Verify _apply_sqlalchemy_isolation_async function exists."""
    from tach_harness import _apply_sqlalchemy_isolation_async
    assert callable(_apply_sqlalchemy_isolation_async)


def test_cleanup_sqlalchemy_isolation_async_exists():
    """Verify _cleanup_sqlalchemy_isolation_async function exists."""
    from tach_harness import _cleanup_sqlalchemy_isolation_async
    assert callable(_cleanup_sqlalchemy_isolation_async)


def test_apply_async_is_coroutine():
    """Verify apply function is async."""
    from tach_harness import _apply_sqlalchemy_isolation_async
    assert asyncio.iscoroutinefunction(_apply_sqlalchemy_isolation_async)


def test_cleanup_async_is_coroutine():
    """Verify cleanup function is async."""
    from tach_harness import _cleanup_sqlalchemy_isolation_async
    assert asyncio.iscoroutinefunction(_cleanup_sqlalchemy_isolation_async)
