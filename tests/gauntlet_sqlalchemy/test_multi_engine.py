"""Test multi-engine support."""
import inspect


def test_apply_sqlalchemy_isolation_multi_exists():
    """Verify multi-engine isolation function exists."""
    from tach_harness import _apply_sqlalchemy_isolation_multi
    assert callable(_apply_sqlalchemy_isolation_multi)


def test_cleanup_sqlalchemy_isolation_multi_exists():
    """Verify multi-engine cleanup function exists."""
    from tach_harness import _cleanup_sqlalchemy_isolation_multi
    assert callable(_cleanup_sqlalchemy_isolation_multi)


def test_apply_multi_signature():
    """Verify function signature."""
    from tach_harness import _apply_sqlalchemy_isolation_multi
    sig = inspect.signature(_apply_sqlalchemy_isolation_multi)
    params = list(sig.parameters.keys())
    assert 'engines' in params
