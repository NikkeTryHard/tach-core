"""Tests for verifying parallel execution and crash handling."""

import time


def test_sleep_1():
    """Sleep for 1 second - used to verify parallel execution."""
    time.sleep(1.0)
    assert True


def test_sleep_2():
    """Sleep for 1 second - used to verify parallel execution."""
    time.sleep(1.0)
    assert True


def test_sleep_3():
    """Sleep for 1 second - used to verify parallel execution."""
    time.sleep(1.0)
    assert True


def test_sleep_4():
    """Sleep for 1 second - used to verify parallel execution."""
    time.sleep(1.0)
    assert True
