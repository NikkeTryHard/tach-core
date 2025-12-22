"""Test that proves network isolation works.

These tests bind to the SAME port. Without isolation, the second would
fail with 'Address already in use'. With network namespace isolation,
each worker has its own localhost.
"""

import socket
import time


def test_server_1():
    """First worker binds to port 8080."""
    sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)

    # This would fail with "Address in use" if isolation doesn't work
    sock.bind(("127.0.0.1", 8080))
    sock.listen(1)

    # Keep the port bound to overlap with other workers
    time.sleep(0.5)

    sock.close()
    assert True


def test_server_2():
    """Second worker also binds to port 8080."""
    sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)

    sock.bind(("127.0.0.1", 8080))
    sock.listen(1)

    time.sleep(0.5)

    sock.close()
    assert True
