"""Test SQLAlchemy detection functions."""


def test_detect_sqlalchemy_exists():
    """Verify _detect_sqlalchemy function exists."""
    from tach_harness import _detect_sqlalchemy
    assert callable(_detect_sqlalchemy)


def test_detect_sqlalchemy_returns_bool():
    """Verify function returns boolean."""
    from tach_harness import _detect_sqlalchemy
    result = _detect_sqlalchemy()
    assert isinstance(result, bool)


def test_get_sqlalchemy_version_exists():
    """Verify _get_sqlalchemy_version function exists."""
    from tach_harness import _get_sqlalchemy_version
    assert callable(_get_sqlalchemy_version)
