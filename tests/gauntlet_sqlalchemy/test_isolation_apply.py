"""Test SQLAlchemy isolation apply function."""
import inspect


def test_apply_sqlalchemy_isolation_exists():
    """Verify _apply_sqlalchemy_isolation function exists."""
    from tach_harness import _apply_sqlalchemy_isolation

    assert callable(_apply_sqlalchemy_isolation)


def test_apply_sqlalchemy_isolation_signature():
    """Verify function accepts engine and session_factory."""
    from tach_harness import _apply_sqlalchemy_isolation

    sig = inspect.signature(_apply_sqlalchemy_isolation)
    params = list(sig.parameters.keys())
    assert 'engine' in params
    assert 'session_factory' in params
