"""
Task 0.1.1-B: CLI Improvements Tests

Tests for the new CLI flags introduced in 0.1.1:
- --dry-run: Discover tests and show what would run without executing
- --collect-only: Alias for 'list' command (pytest compatibility)
- --version --verbose: Show extended version info with capabilities
- --help improvements: Examples section in help output
"""

import os
import subprocess
import sys

import pytest

# Get the path to the tach-core binary (check release first for CI, then debug for local dev)
_PROJECT_ROOT = os.path.dirname(os.path.dirname(os.path.dirname(__file__)))
_RELEASE_BINARY = os.path.join(_PROJECT_ROOT, "target", "release", "tach-core")
_DEBUG_BINARY = os.path.join(_PROJECT_ROOT, "target", "debug", "tach-core")
TACH_BINARY = _RELEASE_BINARY if os.path.exists(_RELEASE_BINARY) else _DEBUG_BINARY

# Set PYO3_PYTHON for version detection
TACH_ENV = os.environ.copy()
TACH_ENV["PYO3_PYTHON"] = sys.executable


def run_tach(*args, check=True, capture_stderr=True):
    """Run tach-core with given arguments and return output."""
    cmd = [TACH_BINARY] + list(args)
    result = subprocess.run(
        cmd,
        capture_output=True,
        text=True,
        env=TACH_ENV,
        check=False,
    )
    if check and result.returncode != 0:
        raise subprocess.CalledProcessError(
            result.returncode, cmd, result.stdout, result.stderr
        )
    # Combine stdout and stderr for easier testing
    output = result.stderr if capture_stderr else result.stdout
    return output, result.returncode


class TestDryRunFlag:
    """Tests for --dry-run flag."""

    def test_a_dry_run_discovers_tests(self):
        """Verify --dry-run discovers tests without running them."""
        output, code = run_tach("--dry-run", "tests/gauntlet_011")
        print(output, file=sys.stderr)

        assert code == 0, f"--dry-run should exit with 0, got {code}"
        assert "DRY RUN SUMMARY" in output, "Should show dry run summary"
        assert "Would run" in output, "Should show tests that would run"
        assert "No tests were executed" in output, "Should indicate no tests ran"

    def test_b_dry_run_shows_test_count(self):
        """Verify --dry-run shows test count summary."""
        output, code = run_tach("--dry-run", "tests/gauntlet_011")
        print(output, file=sys.stderr)

        assert "Total tests:" in output, "Should show total test count"
        assert "Safe tests:" in output, "Should show safe test count"
        assert "Toxic tests:" in output, "Should show toxic test count"

    def test_c_dry_run_with_path_filter(self):
        """Verify --dry-run respects path filters."""
        output, code = run_tach("--dry-run", "tests/gauntlet_011")
        print(output, file=sys.stderr)

        # Should only show tests from gauntlet_011
        assert "gauntlet_011" in output, "Should show tests from filtered path"


class TestCollectOnlyFlag:
    """Tests for --collect-only flag (pytest compatibility)."""

    def test_d_collect_only_lists_tests(self):
        """Verify --collect-only lists discovered tests."""
        output, code = run_tach("--collect-only")
        print(output, file=sys.stderr)

        assert code == 0, f"--collect-only should exit with 0, got {code}"
        # Should list tests in format file::test_name
        assert "::" in output, "Should list tests in pytest format"

    def test_e_collect_only_matches_list_command(self):
        """Verify --collect-only produces same output as 'list' command."""
        collect_output, collect_code = run_tach("--collect-only")
        list_output, list_code = run_tach("list")

        assert collect_code == list_code, "Should have same exit code"
        # The test list should be the same (ignoring header messages)
        collect_tests = [l for l in collect_output.split("\n") if "::" in l]
        list_tests = [l for l in list_output.split("\n") if "::" in l]
        assert collect_tests == list_tests, "Should list same tests"


class TestVerboseVersionFlag:
    """Tests for verbose version output."""

    def test_f_version_shows_basic_info(self):
        """Verify 'version' command shows basic info."""
        output, code = run_tach("version")
        print(output, file=sys.stderr)

        assert code == 0, f"version should exit with 0, got {code}"
        assert "tach" in output.lower(), "Should show tach name"
        assert "Allocator:" in output, "Should show allocator info"
        assert "Kernel:" in output, "Should show kernel version"

    def test_g_verbose_version_shows_capabilities(self):
        """Verify '-v version' shows capabilities."""
        output, code = run_tach("-v", "version")
        print(output, file=sys.stderr)

        assert code == 0, f"-v version should exit with 0, got {code}"
        assert "Capabilities:" in output, "Should show capabilities section"
        assert "userfaultfd:" in output, "Should show userfaultfd status"
        assert "Landlock:" in output, "Should show Landlock ABI"
        assert "Seccomp:" in output, "Should show Seccomp status"


class TestHelpExamples:
    """Tests for --help improvements."""

    def test_h_help_shows_examples(self):
        """Verify --help shows EXAMPLES section."""
        output, code = run_tach("--help", capture_stderr=False)
        print(output, file=sys.stderr)

        assert code == 0, f"--help should exit with 0, got {code}"
        assert "EXAMPLES:" in output, "Should have EXAMPLES section"

    def test_i_help_shows_dry_run(self):
        """Verify --help documents --dry-run flag."""
        output, code = run_tach("--help", capture_stderr=False)
        print(output, file=sys.stderr)

        assert "--dry-run" in output, "Should document --dry-run flag"

    def test_j_help_shows_collect_only(self):
        """Verify --help documents --collect-only flag."""
        output, code = run_tach("--help", capture_stderr=False)
        print(output, file=sys.stderr)

        assert "--collect-only" in output, "Should document --collect-only flag"

    def test_k_help_shows_usage_examples(self):
        """Verify --help shows common usage patterns."""
        output, code = run_tach("--help", capture_stderr=False)
        print(output, file=sys.stderr)

        # Check for common example patterns
        assert "tach tests/" in output, "Should show directory example"
        assert 'tach -k "' in output, "Should show keyword filter example"
        assert "tach -m " in output, "Should show marker filter example"


class TestFlagCombinations:
    """Tests for flag combinations."""

    def test_l_dry_run_with_json_format(self):
        """Verify --dry-run works with --format json."""
        output, code = run_tach(
            "--dry-run", "--format", "json", "tests/gauntlet_011", capture_stderr=False
        )
        # JSON goes to stdout
        print(output, file=sys.stderr)

        assert code == 0, f"--dry-run --format json should exit with 0, got {code}"
        assert '"dry_run": true' in output, "Should indicate dry run in JSON"
        assert '"test_count":' in output, "Should show test count in JSON"
