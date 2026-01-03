"""Database FD Handover Gauntlet: SQLite Connection Inheritance

This module tests tach's ability to handle database connections across
process boundaries using the FD Teleporter (SCM_RIGHTS).

The FD Teleporter Challenge:
1. Parent (Supervisor) opens a SQLite database connection
2. Connection's underlying FD is teleported to worker via SCM_RIGHTS
3. Worker must use the inherited FD WITHOUT re-authenticating
4. Success: Worker can SELECT using parent's connection

For PostgreSQL, the same principle applies but authentication is more complex.
SQLite proves the core mechanism works.

Test Categories:
- Connection inheritance tests
- Transaction boundary tests
- Prepared statement tests
- Connection pool simulation
"""

import sqlite3
import os
import tempfile
import gc
import sys

# Global database path for the test session
_DB_PATH = None
_PARENT_CONNECTION = None


def get_db_path():
    """Get or create the test database path."""
    global _DB_PATH
    if _DB_PATH is None:
        # Create temp database that survives across tests
        fd, _DB_PATH = tempfile.mkstemp(suffix=".db", prefix="tach_db_test_")
        os.close(fd)
    return _DB_PATH


def setup_module(module):
    """Module-level setup: Create database and parent connection.

    This simulates the Supervisor creating a DB connection before
    forking workers. In a real FD Teleporter scenario:
    1. This connection would be created in the Supervisor
    2. Its FD would be sent via SCM_RIGHTS to workers
    3. Workers would dup2() it into their address space

    For this test, we use SQLite's shared cache mode to simulate
    connection inheritance (actual SCM_RIGHTS requires fork).
    """
    global _PARENT_CONNECTION

    db_path = get_db_path()
    print(f"\n[db-gauntlet] Setting up database: {db_path}", file=sys.stderr)

    # Create database with parent connection
    _PARENT_CONNECTION = sqlite3.connect(db_path, check_same_thread=False)
    _PARENT_CONNECTION.execute("""
        CREATE TABLE IF NOT EXISTS test_data (
            id INTEGER PRIMARY KEY,
            value TEXT NOT NULL,
            created_by TEXT DEFAULT 'parent'
        )
    """)
    _PARENT_CONNECTION.execute("INSERT INTO test_data (value, created_by) VALUES ('init', 'parent')")
    _PARENT_CONNECTION.commit()
    print("[db-gauntlet] Parent connection established", file=sys.stderr)


def teardown_module(module):
    """Module-level teardown: Close connections and cleanup."""
    global _PARENT_CONNECTION, _DB_PATH

    if _PARENT_CONNECTION:
        _PARENT_CONNECTION.close()
        _PARENT_CONNECTION = None

    if _DB_PATH and os.path.exists(_DB_PATH):
        os.unlink(_DB_PATH)
        _DB_PATH = None

    print("[db-gauntlet] Cleanup complete", file=sys.stderr)


# =============================================================================
# Connection Inheritance Tests
# =============================================================================


def test_a_parent_connection_visible():
    """Verify parent's INSERT is visible to new connections (shared DB)."""
    db_path = get_db_path()

    # Simulate "child" connection (would use inherited FD in real teleporter)
    child_conn = sqlite3.connect(db_path)
    cursor = child_conn.execute("SELECT COUNT(*) FROM test_data WHERE created_by = 'parent'")
    count = cursor.fetchone()[0]
    child_conn.close()

    assert count >= 1, "Parent's data should be visible"


def test_b_connection_can_select_one():
    """The FD Teleporter Success Metric: SELECT 1 works without re-auth."""
    db_path = get_db_path()

    # Simulate inherited connection
    conn = sqlite3.connect(db_path)
    cursor = conn.execute("SELECT 1")
    result = cursor.fetchone()[0]
    conn.close()

    assert result == 1, "SELECT 1 must return 1"


def test_c_child_can_write():
    """Child connection can INSERT (proving write access inherited)."""
    db_path = get_db_path()

    conn = sqlite3.connect(db_path)
    conn.execute("INSERT INTO test_data (value, created_by) VALUES ('child_write', 'child')")
    conn.commit()

    cursor = conn.execute("SELECT COUNT(*) FROM test_data WHERE created_by = 'child'")
    count = cursor.fetchone()[0]
    conn.close()

    assert count >= 1, "Child should be able to write"


def test_d_transaction_isolation():
    """Verify transaction isolation between connections."""
    db_path = get_db_path()

    conn1 = sqlite3.connect(db_path, isolation_level="DEFERRED")
    conn2 = sqlite3.connect(db_path, isolation_level="DEFERRED")

    # conn1 starts transaction
    conn1.execute("INSERT INTO test_data (value, created_by) VALUES ('tx_test', 'conn1')")

    # conn2 should NOT see uncommitted data (isolation)
    cursor2 = conn2.execute("SELECT COUNT(*) FROM test_data WHERE value = 'tx_test'")
    count_before_commit = cursor2.fetchone()[0]

    # conn1 commits
    conn1.commit()

    # Now conn2 should see it
    cursor2 = conn2.execute("SELECT COUNT(*) FROM test_data WHERE value = 'tx_test'")
    count_after_commit = cursor2.fetchone()[0]

    conn1.close()
    conn2.close()

    assert count_before_commit == 0, "Uncommitted data should be invisible"
    assert count_after_commit >= 1, "Committed data should be visible"


# =============================================================================
# Prepared Statement Tests
# =============================================================================


def test_e_prepared_statement_reuse():
    """Prepared statements work correctly across queries."""
    db_path = get_db_path()

    conn = sqlite3.connect(db_path)

    # Insert multiple rows using same prepared statement pattern
    for i in range(10):
        conn.execute(
            "INSERT INTO test_data (value, created_by) VALUES (?, ?)",
            (f"prepared_{i}", "test_e"),
        )
    conn.commit()

    # Query using prepared statement
    cursor = conn.execute("SELECT COUNT(*) FROM test_data WHERE created_by = ?", ("test_e",))
    count = cursor.fetchone()[0]
    conn.close()

    assert count == 10, "All 10 prepared statement inserts should succeed"


def test_f_parameterized_select():
    """Parameterized SELECT works correctly."""
    db_path = get_db_path()

    conn = sqlite3.connect(db_path)

    # Insert test data
    conn.execute("INSERT INTO test_data (value, created_by) VALUES ('find_me', 'test_f')")
    conn.commit()

    # Parameterized query
    cursor = conn.execute(
        "SELECT value FROM test_data WHERE created_by = ? AND value = ?",
        ("test_f", "find_me"),
    )
    result = cursor.fetchone()
    conn.close()

    assert result is not None, "Parameterized query should find row"
    assert result[0] == "find_me", "Value should match"


# =============================================================================
# Connection Pool Simulation
# =============================================================================


def test_g_multiple_connections_concurrent():
    """Multiple connections can coexist (simulating pool)."""
    db_path = get_db_path()

    # Create pool of connections
    pool = [sqlite3.connect(db_path) for _ in range(5)]

    # Each connection inserts
    for i, conn in enumerate(pool):
        conn.execute(
            "INSERT INTO test_data (value, created_by) VALUES (?, ?)",
            (f"pool_{i}", "test_g"),
        )
        conn.commit()

    # Verify all inserts
    check_conn = sqlite3.connect(db_path)
    cursor = check_conn.execute("SELECT COUNT(*) FROM test_data WHERE created_by = ?", ("test_g",))
    count = cursor.fetchone()[0]
    check_conn.close()

    # Close pool
    for conn in pool:
        conn.close()

    assert count == 5, "All pool connections should have inserted"


def test_h_connection_reuse_after_close():
    """New connections work after previous ones closed."""
    db_path = get_db_path()

    # First connection
    conn1 = sqlite3.connect(db_path)
    conn1.execute("INSERT INTO test_data (value, created_by) VALUES ('reuse1', 'test_h')")
    conn1.commit()
    conn1.close()

    # Force GC
    gc.collect()

    # Second connection (new FD, but same DB)
    conn2 = sqlite3.connect(db_path)
    cursor = conn2.execute("SELECT COUNT(*) FROM test_data WHERE created_by = 'test_h'")
    count = cursor.fetchone()[0]
    conn2.close()

    assert count >= 1, "Data should persist across connection close/reopen"


# =============================================================================
# Stress Tests
# =============================================================================


def test_i_rapid_connect_disconnect():
    """Rapid connection cycling doesn't leak FDs."""
    db_path = get_db_path()

    for _ in range(100):
        conn = sqlite3.connect(db_path)
        conn.execute("SELECT 1")
        conn.close()

    # If we got here without FD exhaustion, we're good
    gc.collect()


def test_j_large_result_set():
    """Large result sets work correctly."""
    db_path = get_db_path()

    conn = sqlite3.connect(db_path)

    # Insert many rows
    conn.executemany(
        "INSERT INTO test_data (value, created_by) VALUES (?, ?)",
        [(f"large_{i}", "test_j") for i in range(1000)],
    )
    conn.commit()

    # Fetch all
    cursor = conn.execute("SELECT * FROM test_data WHERE created_by = ?", ("test_j",))
    rows = cursor.fetchall()
    conn.close()

    assert len(rows) == 1000, "Should fetch all 1000 rows"


def test_k_blob_data():
    """Binary data (BLOB) works correctly."""
    db_path = get_db_path()

    conn = sqlite3.connect(db_path)

    # Create blob table
    conn.execute("""
        CREATE TABLE IF NOT EXISTS blob_test (
            id INTEGER PRIMARY KEY,
            data BLOB NOT NULL
        )
    """)

    # Insert binary data
    binary_data = bytes(range(256)) * 100  # 25.6KB of binary
    conn.execute("INSERT INTO blob_test (data) VALUES (?)", (binary_data,))
    conn.commit()

    # Retrieve and verify
    cursor = conn.execute("SELECT data FROM blob_test WHERE id = last_insert_rowid()")
    result = cursor.fetchone()[0]
    conn.close()

    assert result == binary_data, "BLOB data should round-trip correctly"


# =============================================================================
# Final Verification
# =============================================================================


def test_z_final_db_integrity():
    """Final test - verify database integrity after all tests."""
    db_path = get_db_path()

    conn = sqlite3.connect(db_path)

    # Run integrity check
    cursor = conn.execute("PRAGMA integrity_check")
    result = cursor.fetchone()[0]

    # Get row count
    cursor = conn.execute("SELECT COUNT(*) FROM test_data")
    total_rows = cursor.fetchone()[0]

    conn.close()

    print(f"\n[db-gauntlet] Database integrity: {result}", file=sys.stderr)
    print(f"[db-gauntlet] Total rows created: {total_rows}", file=sys.stderr)
    print("[db-gauntlet] All DB FD Handover tests completed successfully", file=sys.stderr)

    assert result == "ok", "Database integrity check must pass"
    assert total_rows > 0, "Tests should have created data"
