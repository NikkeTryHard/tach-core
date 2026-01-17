"""Test autouse fixture detection and execution."""
import pytest


def test_autouse_ran_at_least_once(autouse_count_tracker):
    """Verify autouse fixture executed."""
    assert autouse_count_tracker["count"] >= 1


def test_autouse_increments_each_test(autouse_count_tracker):
    """Verify autouse runs for each test."""
    assert autouse_count_tracker["count"] >= 2
