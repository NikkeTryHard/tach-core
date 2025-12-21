"""Test that crashes the worker - for crash handling verification."""

import ctypes


def test_crash():
    """Call abort() to crash the worker process."""
    # This will cause a SIGABRT
    ctypes.pythonapi.Py_FatalError(b"Intentional crash for testing")
