"""Integration tests for the plugin compatibility test runner.

These tests verify that the plugin compatibility test infrastructure
works correctly, including plugin mapping, argument parsing, and
test execution.
"""
import subprocess
import sys
from pathlib import Path

import pytest

# Import from the script to test its components
sys.path.insert(0, str(Path(__file__).parent.parent.parent / "scripts"))

from test_plugin_compat import (
    PLUGIN_ALIASES,
    PLUGIN_TEST_MAP,
    PluginTestResult,
    PluginTestSuite,
    find_project_root,
    resolve_plugin_name,
)


class TestPluginMapping:
    """Tests for plugin name mapping and resolution."""

    def test_all_plugins_have_directories(self, project_root: Path) -> None:
        """Verify all mapped plugin test directories exist."""
        for plugin, info in PLUGIN_TEST_MAP.items():
            test_dir = project_root / info["directory"]
            assert test_dir.exists(), (
                f"Test directory for {plugin} not found: {test_dir}"
            )

    def test_all_plugins_have_required_fields(self) -> None:
        """Verify all plugin entries have required fields."""
        required_fields = {"directory", "description", "package"}
        for plugin, info in PLUGIN_TEST_MAP.items():
            missing = required_fields - set(info.keys())
            assert not missing, (
                f"Plugin {plugin} missing fields: {missing}"
            )

    def test_alias_resolution(self) -> None:
        """Test that plugin aliases resolve correctly."""
        assert resolve_plugin_name("django") == "pytest-django"
        assert resolve_plugin_name("asyncio") == "pytest-asyncio"
        assert resolve_plugin_name("mock") == "pytest-mock"
        assert resolve_plugin_name("env") == "pytest-env"
        assert resolve_plugin_name("timeout") == "pytest-timeout"

    def test_canonical_name_resolution(self) -> None:
        """Test that canonical plugin names resolve to themselves."""
        for plugin in PLUGIN_TEST_MAP:
            assert resolve_plugin_name(plugin) == plugin

    def test_unknown_plugin_returns_none(self) -> None:
        """Test that unknown plugins return None."""
        assert resolve_plugin_name("unknown-plugin") is None
        assert resolve_plugin_name("") is None

    def test_all_aliases_map_to_valid_plugins(self) -> None:
        """Verify all aliases map to plugins in PLUGIN_TEST_MAP."""
        for alias, plugin in PLUGIN_ALIASES.items():
            assert plugin in PLUGIN_TEST_MAP, (
                f"Alias '{alias}' maps to unknown plugin '{plugin}'"
            )


class TestPluginTestResult:
    """Tests for the TestResult dataclass."""

    def test_default_values(self) -> None:
        """Test that TestResult has correct defaults."""
        result = PluginTestResult(
            plugin="test-plugin",
            directory="tests/test",
            passed=True,
        )
        assert result.tests_run == 0
        assert result.tests_passed == 0
        assert result.tests_failed == 0
        assert result.tests_skipped == 0
        assert result.duration_seconds == 0.0
        assert result.error_message == ""

    def test_failed_result(self) -> None:
        """Test creating a failed result with error message."""
        result = PluginTestResult(
            plugin="test-plugin",
            directory="tests/test",
            passed=False,
            error_message="Test failed",
        )
        assert not result.passed
        assert result.error_message == "Test failed"


class TestPluginTestSuite:
    """Tests for the TestSuite dataclass."""

    def test_empty_suite(self) -> None:
        """Test that empty suite has correct defaults."""
        suite = PluginTestSuite()
        assert len(suite.results) == 0
        assert suite.total_passed == 0
        assert suite.total_failed == 0

    def test_add_passing_result(self) -> None:
        """Test adding a passing result updates totals."""
        suite = PluginTestSuite()
        result = PluginTestResult(
            plugin="test",
            directory="tests/test",
            passed=True,
        )
        suite.add_result(result)
        assert suite.total_passed == 1
        assert suite.total_failed == 0
        assert len(suite.results) == 1

    def test_add_failing_result(self) -> None:
        """Test adding a failing result updates totals."""
        suite = PluginTestSuite()
        result = PluginTestResult(
            plugin="test",
            directory="tests/test",
            passed=False,
        )
        suite.add_result(result)
        assert suite.total_passed == 0
        assert suite.total_failed == 1

    def test_to_dict(self) -> None:
        """Test JSON serialization."""
        suite = PluginTestSuite()
        suite.add_result(PluginTestResult(
            plugin="test-plugin",
            directory="tests/test",
            passed=True,
            tests_run=5,
            tests_passed=5,
        ))

        data = suite.to_dict()
        assert "summary" in data
        assert "results" in data
        assert data["summary"]["total_plugins"] == 1
        assert data["summary"]["passed"] == 1
        assert data["results"][0]["plugin"] == "test-plugin"


class TestProjectRoot:
    """Tests for project root discovery."""

    def test_find_project_root(self, project_root: Path) -> None:
        """Test that find_project_root returns valid path."""
        found_root = find_project_root()
        assert found_root == project_root
        assert (found_root / "Cargo.toml").exists()


class TestScriptExecution:
    """Tests for running the script as a subprocess."""

    def test_list_flag(self, project_root: Path) -> None:
        """Test --list flag shows available plugins."""
        script = project_root / "scripts" / "test_plugin_compat.py"
        result = subprocess.run(
            [sys.executable, str(script), "--list"],
            capture_output=True,
            text=True,
            cwd=project_root,
        )
        assert result.returncode == 0
        assert "pytest-django" in result.stdout
        assert "pytest-asyncio" in result.stdout
        assert "pytest-mock" in result.stdout

    def test_help_flag(self, project_root: Path) -> None:
        """Test --help flag shows usage."""
        script = project_root / "scripts" / "test_plugin_compat.py"
        result = subprocess.run(
            [sys.executable, str(script), "--help"],
            capture_output=True,
            text=True,
            cwd=project_root,
        )
        assert result.returncode == 0
        assert "plugin" in result.stdout.lower()
        assert "verbose" in result.stdout.lower()
        assert "json" in result.stdout.lower()

    def test_unknown_plugin_fails(self, project_root: Path) -> None:
        """Test that unknown plugin returns error."""
        script = project_root / "scripts" / "test_plugin_compat.py"
        result = subprocess.run(
            [sys.executable, str(script), "--plugin", "unknown-plugin"],
            capture_output=True,
            text=True,
            cwd=project_root,
        )
        assert result.returncode != 0
        assert "unknown" in result.stderr.lower()


class TestFixtures:
    """Tests for the conftest fixtures."""

    def test_project_root_fixture(self, project_root: Path) -> None:
        """Test project_root fixture returns valid path."""
        assert project_root.exists()
        assert (project_root / "Cargo.toml").exists()
        assert (project_root / "tests").is_dir()

    def test_tests_dir_fixture(self, tests_dir: Path) -> None:
        """Test tests_dir fixture returns valid path."""
        assert tests_dir.exists()
        assert tests_dir.is_dir()
        assert (tests_dir / "integration").is_dir()

    def test_python_executable_fixture(self, python_executable: str) -> None:
        """Test python_executable fixture returns valid path."""
        assert Path(python_executable).exists()
        assert "python" in python_executable.lower()

    @pytest.mark.skipif(
        not (Path(__file__).parent.parent.parent / "target").exists(),
        reason="No target directory - binary not built",
    )
    def test_tach_binary_fixture(self, tach_binary: Path) -> None:
        """Test tach_binary fixture returns valid path when built."""
        assert tach_binary.exists()
        assert tach_binary.name == "tach"

    @pytest.mark.skipif(
        not (Path(__file__).parent.parent.parent / "target").exists(),
        reason="No target directory - binary not built",
    )
    def test_run_tach_fixture(self, run_tach) -> None:
        """Test run_tach fixture works when binary is available."""
        result = run_tach(["--version"])
        assert result.returncode == 0
        assert "tach" in result.stdout.lower() or result.returncode == 0
