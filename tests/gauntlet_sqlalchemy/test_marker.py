"""Test SQLAlchemy marker handling."""


def test_handle_sqlalchemy_marker_exists():
    """Verify _handle_sqlalchemy_marker function exists."""
    from tach_harness import _handle_sqlalchemy_marker
    assert callable(_handle_sqlalchemy_marker)


def test_handle_sqlalchemy_marker_parses_args():
    """Verify marker args are parsed correctly."""
    from tach_harness import _handle_sqlalchemy_marker

    marker_args = {'databases': ['default', 'replica']}
    result = _handle_sqlalchemy_marker(marker_args)

    assert result is not None
    assert 'databases' in result
    assert result['databases'] == ['default', 'replica']


def test_handle_sqlalchemy_marker_defaults():
    """Verify defaults when no args provided."""
    from tach_harness import _handle_sqlalchemy_marker

    result = _handle_sqlalchemy_marker(None)

    assert result['databases'] is None
    assert result['use_savepoint'] is True
