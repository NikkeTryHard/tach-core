# tests/gauntlet_sqlalchemy/test_session_rollback.py
"""Test SQLAlchemy session rollback handling within isolation."""
import pytest

try:
    from sqlalchemy import create_engine, text, MetaData, Table, Column, Integer, String
    from sqlalchemy.orm import sessionmaker
    SQLALCHEMY_AVAILABLE = True
except ImportError:
    SQLALCHEMY_AVAILABLE = False

pytestmark = pytest.mark.skipif(
    not SQLALCHEMY_AVAILABLE,
    reason="SQLAlchemy not installed"
)


@pytest.fixture(scope='module')
def engine():
    """Create test engine with schema."""
    engine = create_engine('sqlite:///:memory:')
    metadata = MetaData()
    Table('users', metadata,
        Column('id', Integer, primary_key=True),
        Column('name', String(50)),
    )
    metadata.create_all(engine)
    yield engine
    engine.dispose()


def test_session_rollback_preserves_isolation(engine):
    """Test that session.rollback() within a test works correctly."""
    from tach_harness import (
        _apply_sqlalchemy_isolation,
        _cleanup_sqlalchemy_isolation,
    )

    Session = sessionmaker(bind=engine)
    context = _apply_sqlalchemy_isolation(engine, Session)
    session = context['session']

    # Insert and rollback within test
    session.execute(text("INSERT INTO users (id, name) VALUES (1, 'Alice')"))
    session.rollback()

    # Data should not be visible after rollback
    result = session.execute(text("SELECT COUNT(*) FROM users"))
    assert result.scalar() == 0

    # Insert new data
    session.execute(text("INSERT INTO users (id, name) VALUES (2, 'Bob')"))
    session.commit()

    result = session.execute(text("SELECT COUNT(*) FROM users"))
    assert result.scalar() == 1

    _cleanup_sqlalchemy_isolation(context)

    # Everything should be gone
    with engine.connect() as conn:
        result = conn.execute(text("SELECT COUNT(*) FROM users"))
        assert result.scalar() == 0


def test_exception_rollback_handling(engine):
    """Test that exceptions trigger proper rollback."""
    from tach_harness import (
        _apply_sqlalchemy_isolation,
        _cleanup_sqlalchemy_isolation,
    )

    Session = sessionmaker(bind=engine)
    context = _apply_sqlalchemy_isolation(engine, Session)
    session = context['session']

    try:
        session.execute(text("INSERT INTO users (id, name) VALUES (1, 'Alice')"))
        session.commit()
        raise ValueError("Test error")
    except ValueError:
        pass
    finally:
        _cleanup_sqlalchemy_isolation(context)

    # Data should still be rolled back
    with engine.connect() as conn:
        result = conn.execute(text("SELECT COUNT(*) FROM users"))
        assert result.scalar() == 0
