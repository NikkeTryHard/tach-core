"""Integration tests for SQLAlchemy isolation with real engine."""
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
def test_engine():
    """Create in-memory SQLite engine with test table."""
    engine = create_engine('sqlite:///:memory:')
    metadata = MetaData()
    Table('users', metadata,
        Column('id', Integer, primary_key=True),
        Column('name', String(50)),
    )
    metadata.create_all(engine)
    yield engine
    engine.dispose()


def test_isolation_rollback_works(test_engine):
    """Test that changes are rolled back after cleanup."""
    from tach_harness import (
        _apply_sqlalchemy_isolation,
        _cleanup_sqlalchemy_isolation,
    )

    Session = sessionmaker(bind=test_engine)
    context = _apply_sqlalchemy_isolation(test_engine, Session)
    session = context['session']

    # Insert data
    session.execute(text("INSERT INTO users (id, name) VALUES (1, 'Alice')"))
    session.commit()

    # Verify visible in session
    result = session.execute(text("SELECT COUNT(*) FROM users"))
    assert result.scalar() == 1

    # Cleanup
    _cleanup_sqlalchemy_isolation(context)

    # Verify rolled back
    with test_engine.connect() as conn:
        result = conn.execute(text("SELECT COUNT(*) FROM users"))
        assert result.scalar() == 0


def test_multiple_commits_rollback(test_engine):
    """Test that multiple commits within savepoint all rollback."""
    from tach_harness import (
        _apply_sqlalchemy_isolation,
        _cleanup_sqlalchemy_isolation,
    )

    Session = sessionmaker(bind=test_engine)
    context = _apply_sqlalchemy_isolation(test_engine, Session)
    session = context['session']

    # Multiple inserts and commits
    session.execute(text("INSERT INTO users (id, name) VALUES (1, 'Alice')"))
    session.commit()
    session.execute(text("INSERT INTO users (id, name) VALUES (2, 'Bob')"))
    session.commit()

    result = session.execute(text("SELECT COUNT(*) FROM users"))
    assert result.scalar() == 2

    _cleanup_sqlalchemy_isolation(context)

    # All should be rolled back
    with test_engine.connect() as conn:
        result = conn.execute(text("SELECT COUNT(*) FROM users"))
        assert result.scalar() == 0


def test_dispose_clears_registry():
    """Test that engine registry is cleared after disposal."""
    from tach_harness import (
        _register_sqlalchemy_engine,
        _dispose_sqlalchemy_engines,
        _sqlalchemy_engines,
    )

    engine = create_engine('sqlite:///:memory:')
    _register_sqlalchemy_engine(engine)

    assert len(_sqlalchemy_engines) > 0

    _dispose_sqlalchemy_engines()

    assert len(_sqlalchemy_engines) == 0
    engine.dispose()
