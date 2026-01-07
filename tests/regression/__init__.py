"""Regression test infrastructure for tach-core.

This package contains:
- golden/: Golden output snapshot tests
- perf/: Performance regression tests

Usage:
    # Run all regression tests
    pytest tests/regression/

    # Update golden files after intentional changes
    UPDATE_GOLDEN=1 pytest tests/regression/golden/

    # Update performance baselines
    UPDATE_PERF_BASELINE=1 pytest tests/regression/perf/

    # Skip performance tests in noisy CI
    SKIP_PERF_TESTS=1 pytest tests/regression/
"""
