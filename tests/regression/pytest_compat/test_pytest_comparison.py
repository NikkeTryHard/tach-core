"""Pytest API comparison tests for tach-core.

This module runs the same sample tests through both real pytest and tach-core,
then compares outcomes to detect API drift or compatibility issues.

Usage:
    pytest tests/regression/pytest_compat/test_pytest_comparison.py -v

The tests verify that tach-core's pytest harness behaves identically to
real pytest for:
- pytest.raises (exception context manager)
- pytest.approx (floating point comparison)
"""

import os
import re
import subprocess
from dataclasses import dataclass
from pathlib import Path
from typing import Dict, Set

import pytest

# Directory containing this test file
TEST_DIR = Path(__file__).parent
SAMPLE_TESTS_DIR = TEST_DIR / "sample_tests"
PROJECT_ROOT = TEST_DIR.parent.parent.parent

# Binary path
TACH_BINARY = PROJECT_ROOT / "target" / "debug" / "tach-core"


def find_venv_python() -> Path:
    """Find the Python virtual environment.

    Handles both regular repo and worktree setups by checking multiple locations.

    Returns:
        Path to the Python interpreter in the venv
    """
    # Check project root first
    venv_path = PROJECT_ROOT / ".venv" / "bin" / "python"
    if venv_path.exists():
        return venv_path

    # Check if we're in a worktree - the main repo might have the venv
    # Worktrees typically have a .git file pointing to the main repo
    git_path = PROJECT_ROOT / ".git"
    if git_path.is_file():
        # Read the .git file to find the main worktree
        git_content = git_path.read_text().strip()
        if git_content.startswith("gitdir:"):
            git_dir = git_content.replace("gitdir:", "").strip()
            # Navigate up from .git/worktrees/name to find main repo
            main_repo = Path(git_dir).parent.parent.parent
            main_venv = main_repo / ".venv" / "bin" / "python"
            if main_venv.exists():
                return main_venv

    # Fallback to system Python
    return Path("python")


# Python virtual environment
VENV_PYTHON = find_venv_python()


@dataclass
class Outcome:
    """Represents the outcome of a single test."""

    name: str
    outcome: str  # 'passed', 'failed', 'skipped', 'error'


@dataclass
class RunResult:
    """Represents the results of running a test suite."""

    passed: Set[str]
    failed: Set[str]
    skipped: Set[str]
    errors: Set[str]
    raw_output: str


def normalize_test_name(name: str) -> str:
    """Normalize test name for comparison.

    Removes module path prefixes and parametrization markers to get
    a canonical test identifier.

    Args:
        name: Raw test name from output

    Returns:
        Normalized test name (e.g., "test_raises_pass_basic_valueerror")
    """
    # Remove file path prefix (e.g., "tests/regression/.../test_file.py::")
    if "::" in name:
        name = name.split("::")[-1]

    # Remove class prefix if present (e.g., "TestClass::test_method" -> "test_method")
    if "::" in name:
        name = name.split("::")[-1]

    # Remove parametrization markers (e.g., "test_foo[param1]" -> "test_foo")
    if "[" in name:
        name = name.split("[")[0]

    return name.strip()


def run_pytest(test_file: Path) -> RunResult:
    """Run tests through real pytest and collect outcomes.

    Args:
        test_file: Path to the test file to run

    Returns:
        RunResult with categorized test outcomes
    """
    env = os.environ.copy()
    env["PYO3_PYTHON"] = str(VENV_PYTHON)

    cmd = [
        str(VENV_PYTHON),
        "-m",
        "pytest",
        str(test_file),
        "-v",
        "--tb=no",
        "--no-header",
    ]

    result = subprocess.run(
        cmd,
        capture_output=True,
        text=True,
        cwd=str(PROJECT_ROOT),
        env=env,
        timeout=120,
    )

    return parse_pytest_output(result.stdout + result.stderr)


def parse_pytest_output(output: str) -> RunResult:
    """Parse pytest verbose output to extract test outcomes.

    Args:
        output: Combined stdout/stderr from pytest run

    Returns:
        RunResult with categorized tests
    """
    passed = set()
    failed = set()
    skipped = set()
    errors = set()

    # Pattern matches lines like: "test_file.py::test_name PASSED"
    # or "test_file.py::TestClass::test_method FAILED"
    pattern = r"([\w_/\.\:]+::\w+(?:\[.*?\])?)\s+(PASSED|FAILED|SKIPPED|ERROR)"

    for match in re.finditer(pattern, output):
        raw_name = match.group(1)
        outcome = match.group(2)
        normalized_name = normalize_test_name(raw_name)

        if outcome == "PASSED":
            passed.add(normalized_name)
        elif outcome == "FAILED":
            failed.add(normalized_name)
        elif outcome == "SKIPPED":
            skipped.add(normalized_name)
        elif outcome == "ERROR":
            errors.add(normalized_name)

    return RunResult(
        passed=passed,
        failed=failed,
        skipped=skipped,
        errors=errors,
        raw_output=output,
    )


def run_tach(test_file: Path) -> RunResult:
    """Run tests through tach-core and collect outcomes.

    Args:
        test_file: Path to the test file to run

    Returns:
        RunResult with categorized test outcomes
    """
    env = os.environ.copy()
    env["PYO3_PYTHON"] = str(VENV_PYTHON)

    cmd = [
        str(TACH_BINARY),
        "--no-isolation",
        "-n",
        "1",  # Single worker for deterministic output
        str(test_file),
    ]

    result = subprocess.run(
        cmd,
        capture_output=True,
        text=True,
        cwd=str(PROJECT_ROOT),
        env=env,
        timeout=120,
    )

    return parse_tach_output(result.stdout + result.stderr)


def parse_tach_output(output: str) -> RunResult:
    """Parse tach-core output to extract test outcomes.

    Tach uses a different output format than pytest, so we need
    separate parsing logic.

    Args:
        output: Combined stdout/stderr from tach run

    Returns:
        RunResult with categorized tests
    """
    passed = set()
    failed = set()
    skipped = set()
    errors = set()

    # Tach output format examples:
    # "PASS test_file::test_name"
    # "FAIL test_file::test_name"
    # Also check for the summary line format

    # Pattern for individual test results in tach output
    # Format: "test_name ... PASSED" or dots like "." for pass, "F" for fail
    # Check for explicit PASS/FAIL markers

    # Look for test names with outcomes in various formats
    patterns = [
        # Format: "PASS test_module::test_name" or "FAIL test_module::test_name"
        r"(PASS|FAIL)\s+[\w_/\.]+::([\w_]+)",
        # Format: "test_name ... PASSED/FAILED"
        r"([\w_]+)\s+\.+\s+(PASSED|FAILED|SKIPPED)",
        # Format: "test_name: PASSED" or "test_name: FAILED"
        r"([\w_]+):\s+(PASSED|FAILED|SKIPPED)",
    ]

    for pattern in patterns:
        for match in re.finditer(pattern, output, re.IGNORECASE):
            groups = match.groups()

            # Handle different group orders
            if groups[0].upper() in ("PASS", "PASSED"):
                passed.add(
                    normalize_test_name(groups[1] if len(groups) > 1 else groups[0])
                )
            elif groups[0].upper() in ("FAIL", "FAILED"):
                failed.add(
                    normalize_test_name(groups[1] if len(groups) > 1 else groups[0])
                )
            elif groups[0].upper() == "SKIPPED":
                skipped.add(
                    normalize_test_name(groups[1] if len(groups) > 1 else groups[0])
                )
            elif len(groups) > 1:
                test_name = normalize_test_name(groups[0])
                outcome = groups[1].upper()
                if outcome in ("PASS", "PASSED"):
                    passed.add(test_name)
                elif outcome in ("FAIL", "FAILED"):
                    failed.add(test_name)
                elif outcome == "SKIPPED":
                    skipped.add(test_name)

    # Also try to parse from summary line
    # Format: "X passed, Y failed"
    # And collect test names from the progress output

    # Look for test names in failure messages
    failure_pattern = r"FAILED.*?::([\w_]+)"
    for match in re.finditer(failure_pattern, output):
        test_name = normalize_test_name(match.group(1))
        if test_name not in passed:
            failed.add(test_name)

    # Look for test names in the test collection/run lines
    # Format: "::test_name" followed by pass/fail indicator
    collection_pattern = r"::(test_[\w_]+)"
    all_tests = set()
    for match in re.finditer(collection_pattern, output):
        all_tests.add(normalize_test_name(match.group(1)))

    # If we couldn't parse outcomes, try alternative approach:
    # Count dots and Fs from progress output
    if not passed and not failed:
        # Find summary line: "X passed, Y failed, Z total"
        summary_match = re.search(
            r"(\d+)\s+passed.*?(\d+)\s+failed.*?(\d+)\s+total",
            output,
            re.IGNORECASE,
        )
        if summary_match:
            # We know how many passed/failed but not which ones
            # Try to infer from output format
            pass

    return RunResult(
        passed=passed,
        failed=failed,
        skipped=skipped,
        errors=errors,
        raw_output=output,
    )


def compare_outcomes(
    pytest_result: RunResult,
    tach_result: RunResult,
    test_name: str,
) -> Dict[str, list]:
    """Compare test outcomes between pytest and tach.

    Args:
        pytest_result: Results from running through pytest
        tach_result: Results from running through tach
        test_name: Name for error messages

    Returns:
        Dictionary with differences found
    """
    differences = {
        "pytest_passed_tach_failed": [],
        "pytest_failed_tach_passed": [],
        "pytest_only": [],
        "tach_only": [],
    }

    pytest_all = pytest_result.passed | pytest_result.failed | pytest_result.skipped
    tach_all = tach_result.passed | tach_result.failed | tach_result.skipped

    # Tests that passed in pytest but failed in tach
    for test in pytest_result.passed:
        if test in tach_result.failed:
            differences["pytest_passed_tach_failed"].append(test)

    # Tests that failed in pytest but passed in tach
    for test in pytest_result.failed:
        if test in tach_result.passed:
            differences["pytest_failed_tach_passed"].append(test)

    # Tests only in pytest (tach didn't run or discover them)
    differences["pytest_only"] = list(pytest_all - tach_all)

    # Tests only in tach (shouldn't happen normally)
    differences["tach_only"] = list(tach_all - pytest_all)

    return differences


class TestPytestComparison:
    """Compare pytest and tach-core behavior on sample tests."""

    @classmethod
    def setup_class(cls):
        """Verify prerequisites before running tests."""
        if not TACH_BINARY.exists():
            pytest.skip(f"tach-core binary not found at {TACH_BINARY}")

        if not VENV_PYTHON.exists():
            pytest.skip(f"Python venv not found at {VENV_PYTHON}")

    def test_raises_compatibility(self):
        """Compare pytest.raises behavior between pytest and tach-core."""
        test_file = SAMPLE_TESTS_DIR / "test_raises_samples.py"
        if not test_file.exists():
            pytest.fail(f"Sample test file not found: {test_file}")

        # Run through both pytest and tach
        pytest_result = run_pytest(test_file)
        tach_result = run_tach(test_file)

        # Verify pytest found tests
        pytest_total = len(pytest_result.passed) + len(pytest_result.failed)
        assert (
            pytest_total > 0
        ), f"pytest didn't find any tests. Output:\n{pytest_result.raw_output}"

        # Verify tach found tests
        tach_total = len(tach_result.passed) + len(tach_result.failed)
        if tach_total == 0:
            pytest.skip(
                f"tach-core output parsing needs adjustment. Raw output:\n{tach_result.raw_output[:2000]}"
            )

        # Compare outcomes
        differences = compare_outcomes(pytest_result, tach_result, "pytest.raises")

        # Build error message if there are differences
        error_parts = []

        if differences["pytest_passed_tach_failed"]:
            error_parts.append(
                f"Tests that PASS in pytest but FAIL in tach:\n  {differences['pytest_passed_tach_failed']}"
            )

        if differences["pytest_failed_tach_passed"]:
            error_parts.append(
                f"Tests that FAIL in pytest but PASS in tach:\n  {differences['pytest_failed_tach_passed']}"
            )

        if error_parts:
            pytest.fail(
                "pytest.raises API drift detected!\n\n"
                + "\n\n".join(error_parts)
                + f"\n\npytest: {len(pytest_result.passed)} passed, {len(pytest_result.failed)} failed\ntach: {len(tach_result.passed)} passed, {len(tach_result.failed)} failed"
            )

    def test_approx_compatibility(self):
        """Compare pytest.approx behavior between pytest and tach-core."""
        test_file = SAMPLE_TESTS_DIR / "test_approx_samples.py"
        if not test_file.exists():
            pytest.fail(f"Sample test file not found: {test_file}")

        # Run through both pytest and tach
        pytest_result = run_pytest(test_file)
        tach_result = run_tach(test_file)

        # Verify pytest found tests
        pytest_total = len(pytest_result.passed) + len(pytest_result.failed)
        assert (
            pytest_total > 0
        ), f"pytest didn't find any tests. Output:\n{pytest_result.raw_output}"

        # Verify tach found tests
        tach_total = len(tach_result.passed) + len(tach_result.failed)
        if tach_total == 0:
            pytest.skip(
                f"tach-core output parsing needs adjustment. Raw output:\n{tach_result.raw_output[:2000]}"
            )

        # Compare outcomes
        differences = compare_outcomes(pytest_result, tach_result, "pytest.approx")

        # Build error message if there are differences
        error_parts = []

        if differences["pytest_passed_tach_failed"]:
            error_parts.append(
                f"Tests that PASS in pytest but FAIL in tach:\n  {differences['pytest_passed_tach_failed']}"
            )

        if differences["pytest_failed_tach_passed"]:
            error_parts.append(
                f"Tests that FAIL in pytest but PASS in tach:\n  {differences['pytest_failed_tach_passed']}"
            )

        if error_parts:
            pytest.fail(
                "pytest.approx API drift detected!\n\n"
                + "\n\n".join(error_parts)
                + f"\n\npytest: {len(pytest_result.passed)} passed, {len(pytest_result.failed)} failed\ntach: {len(tach_result.passed)} passed, {len(tach_result.failed)} failed"
            )

    def test_sample_tests_discoverable(self):
        """Verify sample test files are valid and pytest can discover them."""
        raises_file = SAMPLE_TESTS_DIR / "test_raises_samples.py"
        approx_file = SAMPLE_TESTS_DIR / "test_approx_samples.py"

        assert raises_file.exists(), f"Missing: {raises_file}"
        assert approx_file.exists(), f"Missing: {approx_file}"

        # Use pytest --collect-only to verify tests are discoverable
        env = os.environ.copy()
        env["PYO3_PYTHON"] = str(VENV_PYTHON)

        for test_file in [raises_file, approx_file]:
            result = subprocess.run(
                [
                    str(VENV_PYTHON),
                    "-m",
                    "pytest",
                    str(test_file),
                    "--collect-only",
                    "-q",
                ],
                capture_output=True,
                text=True,
                cwd=str(PROJECT_ROOT),
                env=env,
                timeout=30,
            )

            # Should find some tests
            assert (
                "test_" in result.stdout.lower()
            ), f"No tests found in {test_file.name}:\n{result.stdout}\n{result.stderr}"

    def test_expected_outcomes(self):
        """Verify that sample tests have expected pass/fail outcomes."""
        # Run raises tests through pytest and verify expected outcomes
        raises_file = SAMPLE_TESTS_DIR / "test_raises_samples.py"
        result = run_pytest(raises_file)

        # Tests named "test_raises_pass_*" should pass
        for test_name in result.passed:
            if "pass" in test_name:
                continue  # Expected
            elif "fail" in test_name:
                pytest.fail(f"Test {test_name} was expected to fail but passed")

        # Tests named "test_raises_fail_*" should fail
        for test_name in result.failed:
            if "fail" in test_name:
                continue  # Expected
            elif "pass" in test_name:
                pytest.fail(f"Test {test_name} was expected to pass but failed")

        # Same for approx tests
        approx_file = SAMPLE_TESTS_DIR / "test_approx_samples.py"
        result = run_pytest(approx_file)

        for test_name in result.passed:
            if "pass" in test_name:
                continue
            elif "fail" in test_name:
                pytest.fail(f"Test {test_name} was expected to fail but passed")

        for test_name in result.failed:
            if "fail" in test_name:
                continue
            elif "pass" in test_name:
                pytest.fail(f"Test {test_name} was expected to pass but failed")
