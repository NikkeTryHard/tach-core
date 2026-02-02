# SQLAlchemy Gauntlet Tests

Integration tests for SQLAlchemy support in tach-core.

## Requirements

- SQLAlchemy 2.0+ (recommended) or 1.4+
- Optional: Alembic for migration tests
- Optional: asyncpg/aiosqlite for async tests

## Test Categories

| File | Purpose |
|------|---------|
| `test_constants.py` | Protocol constant definitions |
| `test_engine_disposal.py` | Fork-safe engine disposal |
| `test_isolation_apply.py` | Isolation setup function |
| `test_isolation_cleanup.py` | Isolation cleanup function |
| `test_async_isolation.py` | Async session support |
| `test_integration.py` | End-to-end savepoint tests |
| `test_session_rollback.py` | Session.rollback() handling |
| `test_scoped_session.py` | scoped_session patterns |
| `test_multi_engine.py` | Multi-database support |
| `test_alembic.py` | Alembic integration |
| `test_detection.py` | SQLAlchemy detection |
| `test_marker.py` | pytest.mark.sqlalchemy |

## Running Tests

```bash
# All SQLAlchemy tests
pytest tests/gauntlet_sqlalchemy/ -v

# Skip if SQLAlchemy not installed (tests use skipif)
pytest tests/gauntlet_sqlalchemy/ -v
```

## Key Patterns

### Savepoint Isolation

Tests are wrapped in transactions with savepoints:
1. `_apply_sqlalchemy_isolation()` starts outer transaction
2. Session uses `join_transaction_mode="create_savepoint"` (SA 2.0)
3. `session.commit()` only commits savepoint
4. `_cleanup_sqlalchemy_isolation()` rolls back everything

### Fork Safety

After fork, call `engine.dispose(close=False)` to:
- Clear connection pool in child
- NOT send close to server (parent still using)
- Prevent "decryption failed" SSL errors

### Async Support

Use `_apply_sqlalchemy_isolation_async()` for AsyncEngine/AsyncSession.
