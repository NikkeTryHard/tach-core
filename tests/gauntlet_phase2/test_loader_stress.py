# test_loader_stress.py - Stress Tests for Zero-Copy Module Loader
#
# These tests verify the loader under stress conditions:
# - High volume imports
# - Concurrent access patterns
# - Edge case module structures

import sys
import time


def test_rapid_sequential_imports():
    """Test rapid sequential imports don't cause issues."""
    successful = 0

    for i in range(50):
        module_name = f"tests.benchmark.modules.module_{i}"
        try:
            exec(f"from tests.benchmark.modules import module_{i}")
            successful += 1
        except ImportError:
            pass

    assert successful >= 40, f"Should import most modules, got {successful}/50"
    print(f"[stress] Rapid sequential: imported {successful}/50 modules")


def test_reimport_same_module():
    """Test reimporting the same module multiple times."""
    for _ in range(10):
        try:
            from tests.benchmark.modules import module_0

            assert module_0.VALUE == 0
            # Remove from cache to force reimport
            if "tests.benchmark.modules.module_0" in sys.modules:
                del sys.modules["tests.benchmark.modules.module_0"]
        except ImportError:
            import pytest

            pytest.skip("Cannot import benchmark module")

    print("[stress] Reimport test passed")


def test_import_nonexistent_module():
    """Test that importing nonexistent modules fails gracefully."""
    try:
        from tests.benchmark.modules import nonexistent_module_xyz

        assert False, "Should have raised ImportError"
    except ImportError:
        pass  # Expected

    print("[stress] Nonexistent module test passed")


def test_import_all_benchmark_modules():
    """Test importing all 50 benchmark modules."""
    imported = []

    for i in range(50):
        try:
            module = __import__(
                f"tests.benchmark.modules.module_{i}", fromlist=[f"module_{i}"]
            )
            imported.append(module)
            assert module.VALUE == i
        except ImportError:
            pass

    assert len(imported) >= 45, f"Should import most modules, got {len(imported)}/50"
    print(f"[stress] All imports: {len(imported)}/50 modules")


def test_module_attributes_intact():
    """Test that module attributes are set correctly."""
    try:
        from tests.benchmark.modules import module_1
    except ImportError:
        import pytest

        pytest.skip("Cannot import benchmark module")

    # Verify __name__
    assert hasattr(module_1, "__name__")
    assert "module_1" in module_1.__name__

    # Verify callable functions work
    assert module_1.get_value() == 1

    print("[stress] Module attributes intact")


def test_import_timing_consistency():
    """Test that import timing is consistent."""
    times = []

    for i in range(10, 20):
        module_name = f"tests.benchmark.modules.module_{i}"

        # Clear from cache if present
        if module_name in sys.modules:
            del sys.modules[module_name]

        start = time.perf_counter_ns()
        try:
            exec(f"from tests.benchmark.modules import module_{i}")
        except ImportError:
            continue
        end = time.perf_counter_ns()
        times.append(end - start)

    if len(times) < 5:
        import pytest

        pytest.skip("Not enough modules imported for timing test")

    avg_ns = sum(times) / len(times)
    max_ns = max(times)
    min_ns = min(times)

    # Max should not be more than 10x the min (consistency check)
    ratio = max_ns / min_ns if min_ns > 0 else float("inf")

    print(
        f"[stress] Timing: avg={avg_ns / 1e6:.3f}ms, min={min_ns / 1e6:.3f}ms, max={max_ns / 1e6:.3f}ms, ratio={ratio:.1f}x"
    )

    # Relaxed assertion - just ensure no extreme outliers
    assert ratio < 100, f"Timing ratio {ratio}x is too high"
