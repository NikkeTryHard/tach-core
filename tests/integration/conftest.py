"""Shared fixtures for integration tests.

This module provides common fixtures used across all integration tests
for tach-core, including binary location and execution utilities.
"""
import os
import subprocess
import sys
from pathlib import Path
from typing import Callable, List, Optional

import pytest


def _find_project_root() -> Path:
    """Find the project root by looking for Cargo.toml."""
    current = Path(__file__).resolve()
    for parent in [current] + list(current.parents):
        if (parent / "Cargo.toml").exists():
            return parent
    raise RuntimeError("Could not find project root (no Cargo.toml found)")


@pytest.fixture(scope="session")
def project_root() -> Path:
    """Return the project root directory.

    The project root is determined by finding the nearest parent
    directory containing Cargo.toml.
    """
    return _find_project_root()


@pytest.fixture(scope="session")
def tach_binary(project_root: Path) -> Path:
    """Find and return the path to the compiled tach binary.

    Searches for the binary in the following order:
    1. Release build (target/release/tach)
    2. Debug build (target/debug/tach)
    3. PATH lookup (for installed versions)

    Raises:
        pytest.skip: If no tach binary can be found.
    """
    # Check release build first
    release_binary = project_root / "target" / "release" / "tach"
    if release_binary.exists() and os.access(release_binary, os.X_OK):
        return release_binary

    # Check debug build
    debug_binary = project_root / "target" / "debug" / "tach"
    if debug_binary.exists() and os.access(debug_binary, os.X_OK):
        return debug_binary

    # Check if tach is in PATH
    try:
        result = subprocess.run(
            ["which", "tach"],
            capture_output=True,
            text=True,
            check=True,
        )
        return Path(result.stdout.strip())
    except subprocess.CalledProcessError:
        pass

    pytest.skip(
        "tach binary not found. Build with 'cargo build --release' first."
    )


@pytest.fixture(scope="session")
def run_tach(tach_binary: Path, project_root: Path) -> Callable:
    """Provide a function to run tach with arguments.

    Returns a callable that executes tach with the given arguments
    and returns a CompletedProcess object.

    Usage:
        result = run_tach(["discover", "tests/gauntlet_mock"])
        assert result.returncode == 0
    """

    def _run_tach(
        args: List[str],
        cwd: Optional[Path] = None,
        timeout: int = 60,
        env: Optional[dict] = None,
    ) -> subprocess.CompletedProcess:
        """Execute tach with the given arguments.

        Args:
            args: Command-line arguments to pass to tach.
            cwd: Working directory for the command. Defaults to project root.
            timeout: Maximum execution time in seconds.
            env: Additional environment variables to set.

        Returns:
            subprocess.CompletedProcess with stdout, stderr, and returncode.
        """
        cmd = [str(tach_binary)] + args
        run_env = os.environ.copy()
        if env:
            run_env.update(env)

        return subprocess.run(
            cmd,
            cwd=cwd or project_root,
            capture_output=True,
            text=True,
            timeout=timeout,
            env=run_env,
        )

    return _run_tach


@pytest.fixture(scope="session")
def python_executable() -> str:
    """Return the path to the current Python executable."""
    return sys.executable


@pytest.fixture(scope="session")
def tests_dir(project_root: Path) -> Path:
    """Return the tests directory path."""
    return project_root / "tests"
