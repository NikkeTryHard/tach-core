"""Performance regression tests for tach-core.

These tests measure execution time and memory usage to detect performance
regressions between releases. Baselines are stored in JSON files and
compared against current measurements.

Usage:
    pytest tests/regression/perf/test_perf_regression.py

    # Skip in noisy CI environments:
    SKIP_PERF_TESTS=1 pytest tests/regression/perf/

    # Update baselines after intentional changes:
    UPDATE_PERF_BASELINE=1 pytest tests/regression/perf/

Thresholds:
    - Timing: Fail if >10% slower than baseline
    - Memory: Fail if >20% more memory than baseline
"""

import json
import os
import re
import resource
import subprocess
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Dict, Optional, Tuple

import pytest

# Directory containing this test file
TEST_DIR = Path(__file__).parent
BASELINES_DIR = TEST_DIR / "baselines"
PROJECT_ROOT = TEST_DIR.parent.parent.parent

# Binary path (check release first for CI, then debug for local dev)
_RELEASE_BINARY = PROJECT_ROOT / "target" / "release" / "tach-core"
_DEBUG_BINARY = PROJECT_ROOT / "target" / "debug" / "tach-core"
TACH_BINARY = _RELEASE_BINARY if _RELEASE_BINARY.exists() else _DEBUG_BINARY

# Check for skip/update modes
SKIP_PERF_TESTS = os.environ.get("SKIP_PERF_TESTS", "").lower() in ("1", "true", "yes")
UPDATE_BASELINE = os.environ.get("UPDATE_PERF_BASELINE", "").lower() in (
    "1",
    "true",
    "yes",
)

# Thresholds
TIMING_THRESHOLD = 0.10  # 10% slower triggers failure
MEMORY_THRESHOLD = 0.20  # 20% more memory triggers failure

# Minimum values to avoid false positives on very fast tests
MIN_BASELINE_TIMING_MS = 100  # Don't compare timing for tests < 100ms
MIN_BASELINE_MEMORY_KB = 1024  # Don't compare memory for tests < 1MB


@dataclass
class PerfMeasurement:
    """Performance measurement result."""

    timing_ms: float
    memory_kb: float
    test_count: int
    return_code: int


def get_peak_memory_kb() -> float:
    """Get peak RSS memory usage in KB.

    Returns:
        Peak RSS in kilobytes
    """
    usage = resource.getrusage(resource.RUSAGE_CHILDREN)
    return usage.ru_maxrss  # In KB on Linux


def run_tach_with_perf(test_dir: str, no_isolation: bool = True) -> PerfMeasurement:
    """Run tach-core and measure performance.

    Args:
        test_dir: Test directory relative to project root
        no_isolation: Disable sandbox (default True for CI compatibility)

    Returns:
        PerfMeasurement with timing and memory data
    """
    cmd = [str(TACH_BINARY)]

    if no_isolation:
        cmd.append("--no-isolation")

    cmd.append(str(PROJECT_ROOT / test_dir))

    env = os.environ.copy()
    env["PYO3_PYTHON"] = str(PROJECT_ROOT / ".venv" / "bin" / "python")

    python_lib_dir = subprocess.run(
        [
            str(PROJECT_ROOT / ".venv" / "bin" / "python3"),
            "-c",
            "import sysconfig; print(sysconfig.get_config_var('LIBDIR') or '')",
        ],
        capture_output=True,
        text=True,
    ).stdout.strip()
    if python_lib_dir:
        env["LD_LIBRARY_PATH"] = (
            python_lib_dir + os.pathsep + env.get("LD_LIBRARY_PATH", "")
        )

    env["PYTHONPATH"] = (
        str(PROJECT_ROOT / "tests") + os.pathsep + env.get("PYTHONPATH", "")
    )
    env.setdefault("DJANGO_SETTINGS_MODULE", "django_project.settings")

    # Reset child resource tracking
    _ = resource.getrusage(resource.RUSAGE_CHILDREN)

    start_time = time.perf_counter()

    result = subprocess.run(
        cmd,
        capture_output=True,
        text=True,
        cwd=str(PROJECT_ROOT),
        env=env,
        timeout=600,  # 10 minute timeout
    )

    end_time = time.perf_counter()
    elapsed_ms = (end_time - start_time) * 1000

    # Get memory after child completes
    peak_memory_kb = get_peak_memory_kb()

    # Parse test count from output (best effort)
    test_count = 0
    output = result.stdout + result.stderr
    # Look for patterns like "Ran 5 tests" or "5 passed"

    match = re.search(r"(\d+)\s+(?:tests?|passed|failed|error)", output, re.I)
    if match:
        test_count = int(match.group(1))

    return PerfMeasurement(
        timing_ms=elapsed_ms,
        memory_kb=peak_memory_kb,
        test_count=test_count,
        return_code=result.returncode,
    )


def load_baselines(filepath: Path) -> Dict:
    """Load baseline measurements from JSON file.

    Args:
        filepath: Path to baseline JSON file

    Returns:
        Dictionary of baselines, empty if file doesn't exist
    """
    if filepath.exists():
        return json.loads(filepath.read_text(encoding="utf-8"))
    return {}


def save_baselines(filepath: Path, data: Dict) -> None:
    """Save baseline measurements to JSON file.

    Args:
        filepath: Path to baseline JSON file
        data: Dictionary of baselines
    """
    filepath.parent.mkdir(parents=True, exist_ok=True)
    filepath.write_text(
        json.dumps(data, indent=2, sort_keys=True),
        encoding="utf-8",
    )


def check_regression(
    name: str,
    current: float,
    baseline: float,
    threshold: float,
    metric: str,
    min_value: float = 0,
) -> Tuple[bool, str]:
    """Check if current measurement is a regression.

    Args:
        name: Test suite name
        current: Current measurement value
        baseline: Baseline measurement value
        threshold: Allowed percentage increase (e.g., 0.10 = 10%)
        metric: Name of the metric for error message
        min_value: Minimum baseline value to compare

    Returns:
        Tuple of (is_ok, message)
    """
    if baseline < min_value:
        return (
            True,
            f"{name}: Baseline {metric} {baseline:.2f} below minimum {min_value}, skipping comparison",
        )

    if current <= baseline:
        pct_change = ((baseline - current) / baseline) * 100
        return (
            True,
            f"{name}: {metric} improved by {pct_change:.1f}% ({current:.2f} vs baseline {baseline:.2f})",
        )

    pct_increase = ((current - baseline) / baseline) * 100
    allowed_pct = threshold * 100

    if pct_increase > allowed_pct:
        return False, (
            f"{name}: {metric} regression detected!\n  Current:  {current:.2f}\n  Baseline: {baseline:.2f}\n  Increase: {pct_increase:.1f}% (threshold: {allowed_pct:.0f}%)"
        )

    return (
        True,
        f"{name}: {metric} within threshold ({pct_increase:.1f}% < {allowed_pct:.0f}%)",
    )


# =============================================================================
# Test Suites Configuration
# =============================================================================

# Test suites to measure
TEST_SUITES = [
    ("gauntlet", "tests/gauntlet"),
    ("gauntlet_db", "tests/gauntlet_db"),
    ("gauntlet_numpy", "tests/gauntlet_numpy"),
    ("gauntlet_coverage", "tests/gauntlet_coverage"),
    ("benchmark_django", "tests/benchmark_django"),
]


# =============================================================================
# Test Classes
# =============================================================================


@pytest.fixture(autouse=True)
def skip_if_disabled():
    """Skip all perf tests if SKIP_PERF_TESTS=1."""
    if SKIP_PERF_TESTS:
        pytest.skip("Performance tests disabled via SKIP_PERF_TESTS=1")


class TestTimingRegression:
    """Timing regression tests for tach-core."""

    BASELINES_FILE = BASELINES_DIR / "timing.json"

    @classmethod
    def setup_class(cls):
        """Verify tach-core binary exists before running tests."""
        if not TACH_BINARY.exists():
            raise RuntimeError(
                f"tach-core binary not found at {TACH_BINARY}\nBuild with: cargo build"
            )
        cls.baselines = load_baselines(cls.BASELINES_FILE)
        cls.new_measurements = {}

    @classmethod
    def teardown_class(cls):
        """Save updated baselines if in update mode."""
        if UPDATE_BASELINE and cls.new_measurements:
            # Merge with existing baselines
            updated = {**cls.baselines, **cls.new_measurements}
            save_baselines(cls.BASELINES_FILE, updated)
            print(f"\n[perf] Updated timing baselines: {cls.BASELINES_FILE}")

    @pytest.mark.parametrize("name,test_dir", TEST_SUITES)
    def test_timing_regression(self, name: str, test_dir: str):
        """Test timing regression for a test suite.

        Args:
            name: Suite name for baseline lookup
            test_dir: Test directory relative to project root
        """
        # Run measurement
        measurement = run_tach_with_perf(test_dir)

        # Store for potential baseline update
        self.new_measurements[name] = {
            "timing_ms": measurement.timing_ms,
            "test_count": measurement.test_count,
            "recorded_at": time.strftime("%Y-%m-%dT%H:%M:%SZ"),
        }

        if UPDATE_BASELINE:
            print(f"[perf] Recorded timing for {name}: {measurement.timing_ms:.2f}ms")
            return

        # Check against baseline
        if name not in self.baselines:
            pytest.skip(
                f"No timing baseline for {name}. Run with UPDATE_PERF_BASELINE=1"
            )

        baseline = self.baselines[name]["timing_ms"]
        is_ok, message = check_regression(
            name,
            measurement.timing_ms,
            baseline,
            TIMING_THRESHOLD,
            "timing (ms)",
            MIN_BASELINE_TIMING_MS,
        )

        print(f"[perf] {message}")
        assert is_ok, message


class TestMemoryRegression:
    """Memory usage regression tests for tach-core."""

    BASELINES_FILE = BASELINES_DIR / "memory.json"

    @classmethod
    def setup_class(cls):
        """Verify tach-core binary exists before running tests."""
        if not TACH_BINARY.exists():
            raise RuntimeError(
                f"tach-core binary not found at {TACH_BINARY}\nBuild with: cargo build"
            )
        cls.baselines = load_baselines(cls.BASELINES_FILE)
        cls.new_measurements = {}

    @classmethod
    def teardown_class(cls):
        """Save updated baselines if in update mode."""
        if UPDATE_BASELINE and cls.new_measurements:
            # Merge with existing baselines
            updated = {**cls.baselines, **cls.new_measurements}
            save_baselines(cls.BASELINES_FILE, updated)
            print(f"\n[perf] Updated memory baselines: {cls.BASELINES_FILE}")

    @pytest.mark.parametrize("name,test_dir", TEST_SUITES)
    def test_memory_regression(self, name: str, test_dir: str):
        """Test memory regression for a test suite.

        Args:
            name: Suite name for baseline lookup
            test_dir: Test directory relative to project root
        """
        # Run measurement
        measurement = run_tach_with_perf(test_dir)

        # Store for potential baseline update
        self.new_measurements[name] = {
            "memory_kb": measurement.memory_kb,
            "test_count": measurement.test_count,
            "recorded_at": time.strftime("%Y-%m-%dT%H:%M:%SZ"),
        }

        if UPDATE_BASELINE:
            print(
                f"[perf] Recorded memory for {name}: {measurement.memory_kb:.2f}KB ({measurement.memory_kb / 1024:.2f}MB)"
            )
            return

        # Check against baseline
        if name not in self.baselines:
            pytest.skip(
                f"No memory baseline for {name}. Run with UPDATE_PERF_BASELINE=1"
            )

        baseline = self.baselines[name]["memory_kb"]
        is_ok, message = check_regression(
            name,
            measurement.memory_kb,
            baseline,
            MEMORY_THRESHOLD,
            "memory (KB)",
            MIN_BASELINE_MEMORY_KB,
        )

        print(f"[perf] {message}")
        assert is_ok, message


class TestCombinedPerf:
    """Combined performance summary test."""

    def test_summary_report(self):
        """Generate a summary report of all performance measurements."""
        if SKIP_PERF_TESTS:
            pytest.skip("Performance tests disabled via SKIP_PERF_TESTS=1")

        print("\n" + "=" * 60)
        print("PERFORMANCE SUMMARY")
        print("=" * 60)

        timing_baselines = load_baselines(BASELINES_DIR / "timing.json")
        memory_baselines = load_baselines(BASELINES_DIR / "memory.json")

        for name, test_dir in TEST_SUITES:
            print(f"\n{name}:")

            if name in timing_baselines:
                timing = timing_baselines[name]
                print(f"  Timing: {timing['timing_ms']:.2f}ms (baseline)")

            if name in memory_baselines:
                memory = memory_baselines[name]
                mem_mb = memory["memory_kb"] / 1024
                print(
                    f"  Memory: {memory['memory_kb']:.2f}KB ({mem_mb:.2f}MB) (baseline)"
                )

            if name not in timing_baselines and name not in memory_baselines:
                print("  (no baselines recorded)")

        print("\\n" + "=" * 60)
        print("Run with UPDATE_PERF_BASELINE=1 to update baselines")
        print("=" * 60)


# =============================================================================
# pytest-xdist comparison
# =============================================================================

XDIST_WORKERS = int(os.environ.get("BENCH_WORKERS", "4"))

XDIST_SUITES = [
    ("benchmark_django", "tests/benchmark_django"),
]


def run_pytest_with_perf(test_dir: str, workers: int = 0) -> PerfMeasurement:
    """Run pytest (optionally with xdist) and measure wall time + memory."""
    python = str(PROJECT_ROOT / ".venv" / "bin" / "python3")
    cmd = [python, "-m", "pytest", str(PROJECT_ROOT / test_dir), "-q", "--tb=no"]

    if workers > 0:
        cmd += ["-n", str(workers)]
    else:
        cmd += ["-p", "no:xdist"]

    env = os.environ.copy()
    env["PYTHONPATH"] = (
        str(PROJECT_ROOT / "tests") + os.pathsep + env.get("PYTHONPATH", "")
    )
    env["DJANGO_SETTINGS_MODULE"] = "django_project.settings"

    _ = resource.getrusage(resource.RUSAGE_CHILDREN)

    start_time = time.perf_counter()

    result = subprocess.run(
        cmd,
        capture_output=True,
        text=True,
        cwd=str(PROJECT_ROOT),
        env=env,
        timeout=600,
    )

    end_time = time.perf_counter()
    elapsed_ms = (end_time - start_time) * 1000
    peak_memory_kb = get_peak_memory_kb()

    test_count = 0
    output = result.stdout + result.stderr
    match = re.search(r"(\d+)\s+(?:tests?|passed|failed|error)", output, re.I)
    if match:
        test_count = int(match.group(1))

    return PerfMeasurement(
        timing_ms=elapsed_ms,
        memory_kb=peak_memory_kb,
        test_count=test_count,
        return_code=result.returncode,
    )


class TestXdistComparison:
    """Compare tach-core against pytest-xdist on Django benchmarks."""

    BASELINES_FILE = BASELINES_DIR / "xdist_comparison.json"

    @classmethod
    def setup_class(cls):
        if not TACH_BINARY.exists():
            raise RuntimeError(
                f"tach-core binary not found at {TACH_BINARY}\nBuild with: cargo build"
            )
        cls.baselines = load_baselines(cls.BASELINES_FILE)
        cls.new_measurements = {}

    @classmethod
    def teardown_class(cls):
        if UPDATE_BASELINE and cls.new_measurements:
            updated = {**cls.baselines, **cls.new_measurements}
            save_baselines(cls.BASELINES_FILE, updated)
            print(f"\n[perf] Updated xdist comparison baselines: {cls.BASELINES_FILE}")

    @pytest.mark.parametrize("name,test_dir", XDIST_SUITES)
    def test_tach_vs_xdist(self, name: str, test_dir: str):
        tach_result = run_tach_with_perf(test_dir)
        pytest_serial = run_pytest_with_perf(test_dir, workers=0)
        pytest_xdist = run_pytest_with_perf(test_dir, workers=XDIST_WORKERS)

        speedup_vs_serial = (
            pytest_serial.timing_ms / tach_result.timing_ms
            if tach_result.timing_ms > 0
            else 0
        )
        speedup_vs_xdist = (
            pytest_xdist.timing_ms / tach_result.timing_ms
            if tach_result.timing_ms > 0
            else 0
        )

        self.new_measurements[name] = {
            "tach_ms": tach_result.timing_ms,
            "pytest_serial_ms": pytest_serial.timing_ms,
            "pytest_xdist_ms": pytest_xdist.timing_ms,
            "xdist_workers": XDIST_WORKERS,
            "speedup_vs_serial": round(speedup_vs_serial, 2),
            "speedup_vs_xdist": round(speedup_vs_xdist, 2),
            "tach_tests": tach_result.test_count,
            "pytest_tests": pytest_serial.test_count,
            "recorded_at": time.strftime("%Y-%m-%dT%H:%M:%SZ"),
        }

        print(f"\n[perf] {name} comparison:")
        print(
            f"  pytest serial:     {pytest_serial.timing_ms:.0f}ms ({pytest_serial.test_count} tests)"
        )
        print(
            f"  pytest-xdist({XDIST_WORKERS}w): {pytest_xdist.timing_ms:.0f}ms ({pytest_xdist.test_count} tests)"
        )
        print(
            f"  tach-core:         {tach_result.timing_ms:.0f}ms ({tach_result.test_count} tests)"
        )
        print(f"  speedup vs serial: {speedup_vs_serial:.2f}x")
        print(f"  speedup vs xdist:  {speedup_vs_xdist:.2f}x")
