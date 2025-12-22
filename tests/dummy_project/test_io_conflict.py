"""Test that proves filesystem isolation works.

These tests write to the SAME file path. Without isolation, one would
overwrite the other causing a race condition. With OverlayFS isolation,
each worker sees its own private /tmp.
"""

import time

SHARED_FILE = "/tmp/tach_conflict_test.txt"


def test_write_race_1():
    """First worker writes 'worker_1' and expects to read it back."""
    with open(SHARED_FILE, "w") as f:
        f.write("worker_1")

    # Sleep to let other workers clobber the file if isolation fails
    time.sleep(0.5)

    with open(SHARED_FILE, "r") as f:
        content = f.read()
        assert content == "worker_1", f"Expected 'worker_1', got '{content}'"


def test_write_race_2():
    """Second worker writes 'worker_2' and expects to read it back."""
    with open(SHARED_FILE, "w") as f:
        f.write("worker_2")

    time.sleep(0.5)

    with open(SHARED_FILE, "r") as f:
        content = f.read()
        assert content == "worker_2", f"Expected 'worker_2', got '{content}'"
