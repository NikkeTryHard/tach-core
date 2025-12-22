"""Test E: Watch Mode Stress - Rapid file changes.

Tests that the watcher can handle rapid file modifications
without crashing or missing events.
"""

import os
import time


def test_rapid_modification_marker():
    """Create a marker file that watch mode can detect.

    This test creates and modifies a file rapidly to stress test
    the debounce logic. The actual detection happens externally.
    """
    marker_file = "/tmp/tach_watch_stress.txt"

    # Create and modify rapidly
    for i in range(10):
        with open(marker_file, "w") as f:
            f.write(f"modification {i}\n")
        time.sleep(0.01)  # 10ms between writes

    # Verify file exists
    assert os.path.exists(marker_file)

    # Cleanup
    os.remove(marker_file)


def test_large_file_creation():
    """Create a large Python file to stress test discovery.

    Verifies that discovery can handle files with many functions.
    """
    # We're not actually creating files here - just testing
    # that the test harness can handle this pattern
    functions = []
    for i in range(100):
        functions.append(f"def helper_{i}(): pass")

    # Simulate parsing effort
    source = "\n".join(functions)
    assert len(source) > 1000


def test_deep_import_chain():
    """Simulate a deep import chain.

    Tests that the worker doesn't hang on complex imports.
    """
    # This is a simple test that passes
    # The real stress comes from running many of these in parallel
    result = 0
    for i in range(1000):
        result += i

    assert result == 499500  # Sum of 0..999


def test_memory_allocation_burst():
    """Allocate and deallocate memory rapidly.

    Tests fork() CoW behavior under memory pressure.
    """
    allocations = []

    # Allocate 10MB in chunks
    for _ in range(100):
        chunk = "X" * 100000  # 100KB per chunk
        allocations.append(chunk)

    # Force reference to prevent optimization
    total_len = sum(len(a) for a in allocations)
    assert total_len == 10_000_000

    # Let Python GC handle cleanup
    allocations.clear()
    assert True


def test_exception_flood():
    """Raise and catch many exceptions.

    Tests that exception handling doesn't slow down the worker.
    """
    caught = 0

    for i in range(1000):
        try:
            if i % 2 == 0:
                raise ValueError(f"Error {i}")
            else:
                raise TypeError(f"Type {i}")
        except (ValueError, TypeError):
            caught += 1

    assert caught == 1000
