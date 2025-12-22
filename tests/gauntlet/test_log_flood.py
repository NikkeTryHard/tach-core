"""Test C: Log Flood - Stress test memfd ring buffer.

Prints 10MB to stdout and 10MB to stderr.
Must not deadlock or crash the supervisor.
"""

import sys


def test_log_flood():
    """Flood stdout and stderr with 20MB total."""
    # 10MB of stdout
    chunk = "X" * 10000  # 10KB per line
    for _ in range(1000):  # 1000 x 10KB = 10MB
        print(chunk)

    # 10MB of stderr
    for _ in range(1000):
        print(chunk, file=sys.stderr)

    # If we get here without deadlock, we pass
    assert True
