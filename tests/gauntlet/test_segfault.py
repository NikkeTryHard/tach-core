"""Test D: Crash Signals - Worker crashes violently with various signals.

Supervisor must handle all of these gracefully without panicking.
Other parallel tests must continue.
"""

import os
import signal


def test_segfault():
    """Kill self with SIGSEGV (segmentation fault)."""
    os.kill(os.getpid(), signal.SIGSEGV)
    # Never reached
    assert False


def test_sigkill():
    """Kill self with SIGKILL (immediate termination)."""
    os.kill(os.getpid(), signal.SIGKILL)
    # Never reached
    assert False


def test_sigabrt():
    """Kill self with SIGABRT (abort signal)."""
    os.kill(os.getpid(), signal.SIGABRT)
    # Never reached
    assert False


def test_sigbus():
    """Kill self with SIGBUS (bus error - invalid memory access)."""
    os.kill(os.getpid(), signal.SIGBUS)
    # Never reached
    assert False


def test_sigfpe():
    """Kill self with SIGFPE (floating point exception)."""
    os.kill(os.getpid(), signal.SIGFPE)
    # Never reached
    assert False


def test_sigterm():
    """Kill self with SIGTERM (graceful termination request)."""
    os.kill(os.getpid(), signal.SIGTERM)
    # Never reached
    assert False


def test_normal_after_crash():
    """A normal test that should still pass after crashes."""
    assert 1 + 1 == 2


def test_normal_after_crash_2():
    """Second normal test to verify stability."""
    assert 2 * 3 == 6
