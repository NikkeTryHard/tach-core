"""Test SQLAlchemy isolation cleanup function."""
import inspect


def test_cleanup_sqlalchemy_isolation_exists():
    """Verify _cleanup_sqlalchemy_isolation function exists."""
    from tach_harness import _cleanup_sqlalchemy_isolation

    assert callable(_cleanup_sqlalchemy_isolation)


def test_cleanup_sqlalchemy_isolation_signature():
    """Verify function accepts isolation_context dict."""
    from tach_harness import _cleanup_sqlalchemy_isolation

    sig = inspect.signature(_cleanup_sqlalchemy_isolation)
    params = list(sig.parameters.keys())
    assert 'isolation_context' in params
