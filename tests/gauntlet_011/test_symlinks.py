"""
Task 0.1.1-C: Bug Fix Tests - Symlinked Test Directories

Tests that test directories that are symlinks work correctly with tach-core.

This test creates a temporary symlink to a test directory and verifies
that tests in the symlinked location are properly discovered and run.
"""

import os
import subprocess
import sys
import tempfile

import pytest

# Path to tach-core binary (check release first for CI, then debug for local dev)
_PROJECT_ROOT = os.path.dirname(os.path.dirname(os.path.dirname(__file__)))
_RELEASE_BINARY = os.path.join(_PROJECT_ROOT, "target", "release", "tach-core")
_DEBUG_BINARY = os.path.join(_PROJECT_ROOT, "target", "debug", "tach-core")
TACH_BINARY = _RELEASE_BINARY if os.path.exists(_RELEASE_BINARY) else _DEBUG_BINARY


def run_tach(*args, check=True, cwd=None):
    """Run tach-core with given arguments."""
    env = os.environ.copy()
    env["PYO3_PYTHON"] = sys.executable
    cmd = [TACH_BINARY] + list(args)
    result = subprocess.run(
        cmd, capture_output=True, text=True, env=env, check=False, cwd=cwd
    )
    return result.stdout, result.stderr, result.returncode


class TestSymlinkDiscovery:
    """Tests for symlinked directory discovery."""

    def test_a_discover_through_symlink(self, tmp_path):
        """Test that tests in symlinked directories are discovered."""
        # Create a real test directory
        real_dir = tmp_path / "real_tests"
        real_dir.mkdir()

        # Create a simple test file
        test_file = real_dir / "test_example.py"
        test_file.write_text(
            '''
def test_from_symlink():
    """Test discovered through symlink."""
    assert True
'''
        )

        # Create a symlink to the directory
        symlink_dir = tmp_path / "symlink_tests"
        symlink_dir.symlink_to(real_dir)

        # Run tach list from the symlinked directory
        stdout, stderr, code = run_tach("list", cwd=str(symlink_dir))
        output = stdout + stderr

        # Should find the test
        assert code == 0, f"tach list should succeed, got: {output}"
        assert (
            "test_from_symlink" in output
        ), f"Should find test through symlink: {output}"

    def test_b_discover_symlinked_file(self, tmp_path):
        """Test that symlinked test files are discovered."""
        # Create a real test file
        real_file = tmp_path / "real_test.py"
        real_file.write_text(
            '''
def test_symlinked_file():
    """Test in a symlinked file."""
    assert True
'''
        )

        # Create a test directory with a symlink to the file
        test_dir = tmp_path / "tests"
        test_dir.mkdir()
        symlink_file = test_dir / "test_symlinked.py"
        symlink_file.symlink_to(real_file)

        # Run tach list from the test directory
        stdout, stderr, code = run_tach("list", cwd=str(test_dir))
        output = stdout + stderr

        # Should find the test
        assert code == 0, f"tach list should succeed: {output}"
        assert (
            "test_symlinked_file" in output
        ), f"Should find symlinked test file: {output}"

    def test_c_nested_symlinks(self, tmp_path):
        """Test discovery with nested directory structures containing symlinks."""
        # Create nested real directories
        base = tmp_path / "base"
        base.mkdir()
        nested = base / "nested"
        nested.mkdir()

        # Create test file in nested dir
        test_file = nested / "test_nested.py"
        test_file.write_text(
            '''
def test_in_nested():
    """Test in nested directory."""
    assert True
'''
        )

        # Create symlink at top level pointing to base
        symlink = tmp_path / "link_to_base"
        symlink.symlink_to(base)

        # Run tach list from symlink
        stdout, stderr, code = run_tach("list", cwd=str(symlink))
        output = stdout + stderr

        assert code == 0, f"Should succeed: {output}"
        assert (
            "test_in_nested" in output
        ), f"Should find test in nested symlinked dir: {output}"


class TestSymlinkEdgeCases:
    """Edge cases for symlink handling."""

    def test_d_broken_symlink_handled(self, tmp_path):
        """Test that broken symlinks don't crash discovery."""
        test_dir = tmp_path / "tests"
        test_dir.mkdir()

        # Create a broken symlink
        broken = test_dir / "test_broken.py"
        broken.symlink_to(tmp_path / "nonexistent.py")

        # Create a valid test file
        valid = test_dir / "test_valid.py"
        valid.write_text(
            """
def test_valid():
    assert True
"""
        )

        # Run tach list - should not crash
        stdout, stderr, code = run_tach("list", cwd=str(test_dir))
        output = stdout + stderr

        # Should succeed and find the valid test
        assert code == 0, f"Should handle broken symlinks gracefully: {output}"
        assert "test_valid" in output, f"Should find valid test: {output}"

    def test_e_circular_symlink_handled(self, tmp_path):
        """Test that circular symlinks don't cause infinite loops."""
        test_dir = tmp_path / "tests"
        test_dir.mkdir()

        # Create a circular symlink (directory links to itself)
        circular = test_dir / "circular"
        circular.symlink_to(test_dir)

        # Create a valid test
        valid = test_dir / "test_circular.py"
        valid.write_text(
            """
def test_with_circular_link():
    assert True
"""
        )

        # Run tach list - should complete without hanging
        stdout, stderr, code = run_tach("list", cwd=str(test_dir))
        output = stdout + stderr

        # Should succeed (may warn but shouldn't crash or hang)
        assert (
            "test_with_circular_link" in output or code == 0
        ), f"Should handle circular symlinks: {output}"
