"""Test D: Segfault - Worker crashes violently.

Supervisor must handle this gracefully without panicking.
Other parallel tests must continue.
"""

import os
import signal


def test_segfault():
    """Kill self with SIGSEGV."""
    os.kill(os.getpid(), signal.SIGSEGV)
    # Never reached
    assert False


def test_sigkill():
    """Kill self with SIGKILL."""
    os.kill(os.getpid(), signal.SIGKILL)
    # Never reached
    assert False


def test_normal_after_crash():
    """A normal test that should still pass after crashes."""
    assert 1 + 1 == 2
