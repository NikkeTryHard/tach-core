"""Test B: Port Storm - Multiple workers binding same port.

Proves network namespace isolation handles rapid creation/destruction.
Since Tach uses static discovery, we generate explicit test functions.
"""

import socket
import time


def _bind_port_8080():
    """Helper to bind port 8080."""
    sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    sock.bind(("127.0.0.1", 8080))
    sock.listen(1)
    time.sleep(0.1)
    sock.close()


# Generate 20 test functions (enough to stress test with 8+ workers)
def test_port_00():
    _bind_port_8080()


def test_port_01():
    _bind_port_8080()


def test_port_02():
    _bind_port_8080()


def test_port_03():
    _bind_port_8080()


def test_port_04():
    _bind_port_8080()


def test_port_05():
    _bind_port_8080()


def test_port_06():
    _bind_port_8080()


def test_port_07():
    _bind_port_8080()


def test_port_08():
    _bind_port_8080()


def test_port_09():
    _bind_port_8080()


def test_port_10():
    _bind_port_8080()


def test_port_11():
    _bind_port_8080()


def test_port_12():
    _bind_port_8080()


def test_port_13():
    _bind_port_8080()


def test_port_14():
    _bind_port_8080()


def test_port_15():
    _bind_port_8080()


def test_port_16():
    _bind_port_8080()


def test_port_17():
    _bind_port_8080()


def test_port_18():
    _bind_port_8080()


def test_port_19():
    _bind_port_8080()
