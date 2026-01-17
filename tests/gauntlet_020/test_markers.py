"""Test marker detection and filtering."""
import subprocess
import sys

import pytest


@pytest.mark.slow
def test_marked_as_slow():
    """This test has a custom marker."""
    assert True


@pytest.mark.integration
def test_marked_as_integration():
    """Another custom marker."""
    assert True


@pytest.mark.slow
@pytest.mark.integration
def test_multiple_markers():
    """Test with multiple markers."""
    assert True


def test_no_markers():
    """Test without markers."""
    assert True


def test_marker_filtering_with_subprocess():
    """Verify marker filtering works correctly via subprocess.

    This test runs pytest with -m slow and verifies that:
    - 2 tests pass (test_marked_as_slow, test_multiple_markers)
    - 2 tests are deselected (test_marked_as_integration, test_no_markers)

    Note: This test itself is not marked as slow, so it would be deselected
    when running with -m slow, which is why we use subprocess.
    """
    result = subprocess.run(
        [
            sys.executable,
            "-m",
            "pytest",
            __file__,
            "-m",
            "slow",
            "-v",
            "--tb=short",
        ],
        capture_output=True,
        text=True,
        timeout=30,
    )

    # Check stdout for expected results
    stdout = result.stdout

    # Should have 2 passed tests (test_marked_as_slow and test_multiple_markers)
    assert "2 passed" in stdout, f"Expected 2 passed tests, got: {stdout}"

    # Should have deselected tests (the non-slow ones)
    assert "deselected" in stdout, f"Expected deselected tests, got: {stdout}"
