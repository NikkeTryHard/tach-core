#!/usr/bin/env python3
"""Plugin compatibility test runner for tach-core.

This script tests plugin compatibility by running gauntlet tests for each
supported pytest plugin. It provides options for testing specific plugins,
verbose output, and JSON reporting.

Usage:
    python scripts/test_plugin_compat.py                    # Test all plugins
    python scripts/test_plugin_compat.py --plugin django    # Test specific plugin
    python scripts/test_plugin_compat.py --verbose          # Verbose output
    python scripts/test_plugin_compat.py --json             # JSON output
"""
import argparse
import json
import os
import subprocess
import sys
from dataclasses import dataclass, field
from pathlib import Path
from typing import Dict, List, Optional


# Plugin to test directory mapping
PLUGIN_TEST_MAP: Dict[str, Dict[str, str]] = {
    "pytest-django": {
        "directory": "tests/gauntlet_django",
        "description": "Django integration testing plugin",
        "package": "pytest-django",
    },
    "pytest-asyncio": {
        "directory": "tests/pytest_asyncio",
        "description": "Async/await test support",
        "package": "pytest-asyncio",
    },
    "pytest-mock": {
        "directory": "tests/gauntlet_mock",
        "description": "Thin mocker wrapper for unittest.mock",
        "package": "pytest-mock",
    },
    "pytest-env": {
        "directory": "tests/env_test",
        "description": "Environment variable configuration",
        "package": "pytest-env",
    },
    "pytest-timeout": {
        "directory": "tests/gauntlet_012",
        "description": "Test timeout enforcement",
        "package": "pytest-timeout",
    },
}

# Short aliases for convenience
PLUGIN_ALIASES: Dict[str, str] = {
    "django": "pytest-django",
    "asyncio": "pytest-asyncio",
    "mock": "pytest-mock",
    "env": "pytest-env",
    "timeout": "pytest-timeout",
}


@dataclass
class PluginTestResult:
    """Result of a plugin compatibility test run."""

    plugin: str
    directory: str
    passed: bool
    tests_run: int = 0
    tests_passed: int = 0
    tests_failed: int = 0
    tests_skipped: int = 0
    duration_seconds: float = 0.0
    error_message: str = ""
    stdout: str = ""
    stderr: str = ""


@dataclass
class PluginTestSuite:
    """Collection of test results for a complete run."""

    results: List[PluginTestResult] = field(default_factory=list)
    total_passed: int = 0
    total_failed: int = 0

    def add_result(self, result: PluginTestResult) -> None:
        """Add a test result and update totals."""
        self.results.append(result)
        if result.passed:
            self.total_passed += 1
        else:
            self.total_failed += 1

    def to_dict(self) -> dict:
        """Convert to dictionary for JSON output."""
        return {
            "summary": {
                "total_plugins": len(self.results),
                "passed": self.total_passed,
                "failed": self.total_failed,
            },
            "results": [
                {
                    "plugin": r.plugin,
                    "directory": r.directory,
                    "passed": r.passed,
                    "tests_run": r.tests_run,
                    "tests_passed": r.tests_passed,
                    "tests_failed": r.tests_failed,
                    "tests_skipped": r.tests_skipped,
                    "duration_seconds": r.duration_seconds,
                    "error_message": r.error_message,
                }
                for r in self.results
            ],
        }


def find_project_root() -> Path:
    """Find the project root by looking for Cargo.toml."""
    current = Path(__file__).resolve()
    for parent in [current] + list(current.parents):
        if (parent / "Cargo.toml").exists():
            return parent
    raise RuntimeError("Could not find project root (no Cargo.toml found)")


def find_tach_binary(project_root: Path) -> Optional[Path]:
    """Find the tach binary."""
    # Check release build first
    release_binary = project_root / "target" / "release" / "tach"
    if release_binary.exists() and os.access(release_binary, os.X_OK):
        return release_binary

    # Check debug build
    debug_binary = project_root / "target" / "debug" / "tach"
    if debug_binary.exists() and os.access(debug_binary, os.X_OK):
        return debug_binary

    # Check PATH
    try:
        result = subprocess.run(
            ["which", "tach"],
            capture_output=True,
            text=True,
            check=True,
        )
        return Path(result.stdout.strip())
    except subprocess.CalledProcessError:
        return None


def resolve_plugin_name(name: str) -> Optional[str]:
    """Resolve a plugin name or alias to the canonical name."""
    if name in PLUGIN_TEST_MAP:
        return name
    if name in PLUGIN_ALIASES:
        return PLUGIN_ALIASES[name]
    return None


def run_plugin_test(
    plugin_name: str,
    project_root: Path,
    tach_binary: Path,
    verbose: bool = False,
) -> PluginTestResult:
    """Run tests for a specific plugin."""
    plugin_info = PLUGIN_TEST_MAP[plugin_name]
    test_dir = project_root / plugin_info["directory"]

    result = PluginTestResult(
        plugin=plugin_name,
        directory=plugin_info["directory"],
        passed=False,
    )

    if not test_dir.exists():
        result.error_message = f"Test directory not found: {test_dir}"
        return result

    # Run tach on the test directory
    import time

    start_time = time.time()

    try:
        cmd = [str(tach_binary), str(test_dir)]
        if verbose:
            print(f"  Running: {' '.join(cmd)}")

        proc = subprocess.run(
            cmd,
            cwd=project_root,
            capture_output=True,
            text=True,
            timeout=120,
        )

        result.duration_seconds = time.time() - start_time
        result.stdout = proc.stdout
        result.stderr = proc.stderr

        # Parse output for test counts
        # Look for patterns like "X passed", "X failed", etc.
        import re

        for line in proc.stdout.split("\n") + proc.stderr.split("\n"):
            if "passed" in line.lower():
                match = re.search(r"(\d+)\s+passed", line, re.IGNORECASE)
                if match:
                    result.tests_passed = int(match.group(1))
            if "failed" in line.lower():
                match = re.search(r"(\d+)\s+failed", line, re.IGNORECASE)
                if match:
                    result.tests_failed = int(match.group(1))
            if "skipped" in line.lower():
                match = re.search(r"(\d+)\s+skipped", line, re.IGNORECASE)
                if match:
                    result.tests_skipped = int(match.group(1))

        result.tests_run = (
            result.tests_passed + result.tests_failed + result.tests_skipped
        )

        # Consider success if return code is 0 or only skips
        result.passed = proc.returncode == 0 or (
            result.tests_failed == 0 and result.tests_run > 0
        )

        if not result.passed and proc.returncode != 0:
            result.error_message = f"Exit code: {proc.returncode}"

    except subprocess.TimeoutExpired:
        result.error_message = "Test timed out after 120 seconds"
        result.duration_seconds = 120.0
    except Exception as e:
        result.error_message = str(e)

    return result


def print_result(result: PluginTestResult, verbose: bool = False) -> None:
    """Print a test result in human-readable format."""
    status = "PASS" if result.passed else "FAIL"
    status_symbol = "[+]" if result.passed else "[-]"

    print(f"{status_symbol} {result.plugin}: {status}")

    if verbose or not result.passed:
        print(f"    Directory: {result.directory}")
        print(f"    Duration: {result.duration_seconds:.2f}s")
        if result.tests_run > 0:
            print(
                f"    Tests: {result.tests_passed} passed, "
                f"{result.tests_failed} failed, {result.tests_skipped} skipped"
            )
        if result.error_message:
            print(f"    Error: {result.error_message}")
        if verbose and result.stderr:
            print(f"    Stderr: {result.stderr[:500]}")


def main() -> int:
    """Main entry point."""
    parser = argparse.ArgumentParser(
        description="Test plugin compatibility for tach-core",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""
Supported plugins:
  pytest-django (alias: django)   - Django integration testing
  pytest-asyncio (alias: asyncio) - Async/await test support
  pytest-mock (alias: mock)       - Mocker wrapper
  pytest-env (alias: env)         - Environment variables
  pytest-timeout (alias: timeout) - Test timeouts
        """,
    )
    parser.add_argument(
        "--plugin",
        "-p",
        help="Test a specific plugin (name or alias)",
    )
    parser.add_argument(
        "--verbose",
        "-v",
        action="store_true",
        help="Enable verbose output",
    )
    parser.add_argument(
        "--json",
        action="store_true",
        help="Output results as JSON",
    )
    parser.add_argument(
        "--list",
        "-l",
        action="store_true",
        help="List available plugins",
    )

    args = parser.parse_args()

    # Handle --list
    if args.list:
        print("Available plugins:")
        for plugin, info in PLUGIN_TEST_MAP.items():
            aliases = [k for k, v in PLUGIN_ALIASES.items() if v == plugin]
            alias_str = f" (alias: {', '.join(aliases)})" if aliases else ""
            print(f"  {plugin}{alias_str}")
            print(f"    {info['description']}")
            print(f"    Test dir: {info['directory']}")
        return 0

    # Validate plugin name early (before checking binary)
    if args.plugin:
        resolved = resolve_plugin_name(args.plugin)
        if not resolved:
            print(f"Error: Unknown plugin '{args.plugin}'", file=sys.stderr)
            print("Use --list to see available plugins", file=sys.stderr)
            return 1
        plugins_to_test = [resolved]
    else:
        plugins_to_test = list(PLUGIN_TEST_MAP.keys())

    # Find project root and binary
    try:
        project_root = find_project_root()
    except RuntimeError as e:
        print(f"Error: {e}", file=sys.stderr)
        return 1

    tach_binary = find_tach_binary(project_root)
    if not tach_binary:
        print(
            "Error: tach binary not found. Build with 'cargo build --release'",
            file=sys.stderr,
        )
        return 1

    if args.verbose and not args.json:
        print(f"Project root: {project_root}")
        print(f"Using binary: {tach_binary}")
        print()

    # Run tests
    suite = TestSuite()

    if not args.json:
        print(f"Testing {len(plugins_to_test)} plugin(s)...")
        print()

    for plugin in plugins_to_test:
        if not args.json and args.verbose:
            print(f"Testing {plugin}...")

        result = run_plugin_test(
            plugin,
            project_root,
            tach_binary,
            verbose=args.verbose,
        )
        suite.add_result(result)

        if not args.json:
            print_result(result, verbose=args.verbose)

    # Output results
    if args.json:
        print(json.dumps(suite.to_dict(), indent=2))
    else:
        print()
        print(
            f"Summary: {suite.total_passed}/{len(suite.results)} plugins passed"
        )

    return 0 if suite.total_failed == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
