"""
Thread Leak Detection Tests (0.1.2)

These tests verify that tach correctly detects and handles tests that spawn
threads which outlive the test execution.

Task 3 from CHANGELOG 0.1.2:
- Detect when test spawns non-daemon threads
- Warn user about thread leak
- Worker marked toxic if threads don't terminate within grace period
- @pytest.mark.allow_threads bypasses check
"""

import threading
import time
import pytest


# =============================================================================
# Test Helper: Thread that outlives test
# =============================================================================


def spawn_lingering_thread(duration_seconds: float = 2.0):
    """Spawn a thread that sleeps for longer than the test runs."""

    def worker():
        time.sleep(duration_seconds)

    thread = threading.Thread(target=worker, daemon=False)
    thread.start()
    return thread


# =============================================================================
# Thread Leak Detection Tests
# =============================================================================


class TestThreadLeakDetection:
    """Tests for thread leak detection functionality."""

    def test_a_clean_test_no_threads(self):
        """A test that doesn't spawn threads should pass cleanly."""
        # This test should pass without any thread warnings
        result = 1 + 1
        assert result == 2

    def test_b_daemon_thread_is_ok(self):
        """Daemon threads should not trigger leak detection."""

        def worker():
            time.sleep(0.5)

        thread = threading.Thread(target=worker, daemon=True)
        thread.start()
        # Don't wait - daemon threads are fine to leave running
        assert thread.is_alive()

    def test_c_joined_thread_is_ok(self):
        """Threads that are properly joined should not trigger leak detection."""

        def worker():
            time.sleep(0.01)

        thread = threading.Thread(target=worker, daemon=False)
        thread.start()
        thread.join()  # Properly joined
        assert not thread.is_alive()


class TestThreadLeakWarning:
    """Tests that verify warnings are emitted for thread leaks."""

    def test_a_lingering_thread_triggers_warning(self, capsys):
        """A test that spawns a non-daemon thread should trigger a warning.

        The harness should detect that thread count increased and log a warning.
        This test intentionally leaks a thread to verify detection works.

        Note: The warning is logged by the harness, not captured by capsys.
        We verify the thread was spawned and is still running.
        """
        initial_count = threading.active_count()
        thread = spawn_lingering_thread(duration_seconds=5.0)

        # Thread should be running
        assert thread.is_alive()
        final_count = threading.active_count()

        # More threads now than before
        assert final_count > initial_count

        # We don't join - this is an intentional leak for testing
        # The harness should detect and warn about this


class TestAllowThreadsMarker:
    """Tests for @pytest.mark.allow_threads marker."""

    @pytest.mark.allow_threads
    def test_a_allow_threads_bypasses_detection(self):
        """With @pytest.mark.allow_threads, lingering threads are allowed.

        This marker tells the harness: "I know what I'm doing, don't warn
        about thread leaks for this test."
        """
        thread = spawn_lingering_thread(duration_seconds=5.0)
        assert thread.is_alive()
        # No warning should be emitted for this test


# =============================================================================
# Thread Count Verification (Rust unit test compatibility)
# =============================================================================


def test_thread_count_api():
    """Verify threading.active_count() works as expected for our detection."""
    initial = threading.active_count()
    assert initial >= 1  # At least main thread

    def worker():
        time.sleep(0.1)

    # Spawn a thread
    t = threading.Thread(target=worker)
    t.start()

    # Count increased
    assert threading.active_count() > initial

    # Wait for thread to finish
    t.join()

    # Count should be back to initial (or close to it)
    # Note: Some background threads may exist, so we just check it decreased
    assert threading.active_count() <= initial + 1
