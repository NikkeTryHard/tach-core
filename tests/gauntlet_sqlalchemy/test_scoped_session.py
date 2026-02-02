"""Test scoped_session support."""
import inspect


def test_apply_isolation_scoped_exists():
    """Test _apply_sqlalchemy_isolation_scoped function exists."""
    from tach_harness import _apply_sqlalchemy_isolation_scoped
    assert callable(_apply_sqlalchemy_isolation_scoped)


def test_cleanup_isolation_scoped_exists():
    """Test _cleanup_sqlalchemy_isolation_scoped function exists."""
    from tach_harness import _cleanup_sqlalchemy_isolation_scoped
    assert callable(_cleanup_sqlalchemy_isolation_scoped)


def test_apply_scoped_signature():
    """Verify function signature."""
    from tach_harness import _apply_sqlalchemy_isolation_scoped
    sig = inspect.signature(_apply_sqlalchemy_isolation_scoped)
    params = list(sig.parameters.keys())
    assert 'engine' in params
    assert 'scoped_session_instance' in params
