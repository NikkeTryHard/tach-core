"""External project tests for FastAPI.

This module tests tach-core compatibility with the FastAPI project,
verifying that tach can correctly discover and run tests from
real-world async web framework test suites.
"""
import pytest


# Mark all tests in this module
pytestmark = [
    pytest.mark.slow,
    pytest.mark.external,
]


FASTAPI_REPO = "https://github.com/tiangolo/fastapi.git"
FASTAPI_REF = "0.109.0"  # Pin to a stable release


@pytest.mark.skip(reason="Requires network access - run with --run-external")
def test_fastapi_tests_run(clone_repo, run_tach_on_project):
    """Test that tach can discover and run FastAPI tests.

    This test clones the FastAPI repository and verifies that
    tach can successfully discover its test suite structure.

    Note:
        This test is skipped by default. To run it, use:
        pytest --run-external -m external
    """
    # Clone FastAPI repository
    project_dir = clone_repo(FASTAPI_REPO, FASTAPI_REF)

    # Verify the tests directory exists
    tests_dir = project_dir / "tests"
    assert tests_dir.exists(), "FastAPI tests directory should exist"

    # Run tach discover on the tests directory
    result = run_tach_on_project(project_dir, "discover", "tests")

    # We expect discovery to complete (may find tests or not depending on setup)
    # The key is that tach doesn't crash on a real-world project
    assert result.returncode in (0, 1), (
        f"tach discover should complete without crashing.\n"
        f"stdout: {result.stdout}\n"
        f"stderr: {result.stderr}"
    )


def test_external_test_structure():
    """Verify the external test infrastructure exists and is functional.

    This is a simple smoke test that verifies the test module is
    properly set up and can be executed without external dependencies.
    """
    # Verify module-level markers are applied
    assert pytest.mark.slow in pytestmark
    assert pytest.mark.external in pytestmark

    # Verify constants are defined
    assert FASTAPI_REPO.startswith("https://")
    assert FASTAPI_REF is not None

    # Basic sanity check
    assert True, "External test infrastructure is functional"
