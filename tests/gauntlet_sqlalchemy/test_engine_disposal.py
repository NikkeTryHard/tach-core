"""Test SQLAlchemy engine disposal after fork."""


def test_dispose_sqlalchemy_engines_exists():
    """Verify _dispose_sqlalchemy_engines function exists."""
    from tach_harness import _dispose_sqlalchemy_engines

    assert callable(_dispose_sqlalchemy_engines)


def test_dispose_sqlalchemy_engines_handles_no_engines():
    """Verify graceful handling when no engines registered."""
    from tach_harness import _dispose_sqlalchemy_engines

    result = _dispose_sqlalchemy_engines()
    assert isinstance(result, list)


def test_register_sqlalchemy_engine_exists():
    """Verify _register_sqlalchemy_engine function exists."""
    from tach_harness import _register_sqlalchemy_engine

    assert callable(_register_sqlalchemy_engine)
