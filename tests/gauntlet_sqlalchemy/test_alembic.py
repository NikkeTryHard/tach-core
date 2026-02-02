"""Test Alembic integration."""


def test_detect_alembic_exists():
    """Verify _detect_alembic function exists."""
    from tach_harness import _detect_alembic
    assert callable(_detect_alembic)


def test_detect_alembic_returns_bool():
    """Verify function returns boolean."""
    from tach_harness import _detect_alembic
    result = _detect_alembic()
    assert isinstance(result, bool)


def test_get_alembic_config_exists():
    """Verify _get_alembic_config function exists."""
    from tach_harness import _get_alembic_config
    assert callable(_get_alembic_config)


def test_verify_alembic_head_exists():
    """Verify _verify_alembic_head function exists."""
    from tach_harness import _verify_alembic_head
    assert callable(_verify_alembic_head)
