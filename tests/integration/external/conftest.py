"""Fixtures for external project integration tests.

This module provides fixtures for cloning and testing against
external open-source projects to verify tach-core compatibility.
"""
import os
import subprocess
from pathlib import Path
from typing import Callable, List, Optional

import pytest


@pytest.fixture(scope="session")
def external_projects_dir(tmp_path_factory: pytest.TempPathFactory) -> Path:
    """Create a session-scoped temporary directory for external projects.

    This directory persists for the entire test session, allowing
    multiple tests to share cloned repositories.

    Returns:
        Path to the temporary directory for external projects.
    """
    return tmp_path_factory.mktemp("external_projects")


@pytest.fixture(scope="session")
def clone_repo(external_projects_dir: Path) -> Callable[[str, Optional[str]], Path]:
    """Provide a function to clone git repositories.

    Returns a callable that clones a repository to the external projects
    directory and returns the path to the cloned repo.

    Usage:
        project_dir = clone_repo("https://github.com/org/repo.git", "v1.0.0")
    """

    def _clone_repo(repo_url: str, ref: Optional[str] = None) -> Path:
        """Clone a git repository to the external projects directory.

        Args:
            repo_url: The git URL to clone from.
            ref: Optional git ref (branch, tag, commit) to checkout.

        Returns:
            Path to the cloned repository directory.

        Raises:
            subprocess.CalledProcessError: If git clone fails.
        """
        # Extract repo name from URL
        repo_name = repo_url.rstrip("/").split("/")[-1]
        if repo_name.endswith(".git"):
            repo_name = repo_name[:-4]

        # Add ref to directory name if specified
        if ref:
            safe_ref = ref.replace("/", "_").replace("\\", "_")
            dir_name = f"{repo_name}_{safe_ref}"
        else:
            dir_name = repo_name

        project_dir = external_projects_dir / dir_name

        # Skip if already cloned
        if project_dir.exists():
            return project_dir

        # Clone with depth=1 for speed
        clone_cmd = ["git", "clone", "--depth", "1"]
        if ref:
            clone_cmd.extend(["--branch", ref])
        clone_cmd.extend([repo_url, str(project_dir)])

        subprocess.run(
            clone_cmd,
            check=True,
            capture_output=True,
            text=True,
            timeout=300,  # 5 minute timeout for large repos
        )

        return project_dir

    return _clone_repo


@pytest.fixture(scope="session")
def run_tach_on_project(tach_binary: Path) -> Callable[..., subprocess.CompletedProcess]:
    """Provide a function to run tach on an external project.

    Returns a callable that executes tach in the context of an
    external project directory.

    Usage:
        result = run_tach_on_project(project_dir, "discover", "tests")
        assert result.returncode == 0
    """

    def _run_tach_on_project(
        project_dir: Path,
        *args: str,
        timeout: int = 120,
        env: Optional[dict] = None,
    ) -> subprocess.CompletedProcess:
        """Execute tach with the given arguments in a project directory.

        Args:
            project_dir: The project directory to run tach in.
            *args: Command-line arguments to pass to tach.
            timeout: Maximum execution time in seconds.
            env: Additional environment variables to set.

        Returns:
            subprocess.CompletedProcess with stdout, stderr, and returncode.
        """
        cmd = [str(tach_binary)] + list(args)
        run_env = os.environ.copy()
        if env:
            run_env.update(env)

        return subprocess.run(
            cmd,
            cwd=project_dir,
            capture_output=True,
            text=True,
            timeout=timeout,
            env=run_env,
        )

    return _run_tach_on_project


@pytest.fixture(scope="session")
def tach_binary(project_root: Path) -> Path:
    """Find and return the path to the compiled tach binary.

    This is a re-export from the parent conftest for convenience.
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
def project_root() -> Path:
    """Find the project root by looking for Cargo.toml."""
    current = Path(__file__).resolve()
    for parent in [current] + list(current.parents):
        if (parent / "Cargo.toml").exists():
            return parent
    raise RuntimeError("Could not find project root (no Cargo.toml found)")
