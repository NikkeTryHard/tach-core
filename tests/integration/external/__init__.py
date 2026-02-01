"""External project integration tests.

This module contains tests that verify tach-core works correctly with
real-world external projects like FastAPI, Django, Flask, etc.

These tests are designed to:
- Clone external repositories into temporary directories
- Run tach against their test suites
- Verify compatibility and correctness

Tests in this module are marked with:
- @pytest.mark.slow: Tests take significant time to run
- @pytest.mark.external: Tests require network access

By default, these tests are skipped unless explicitly enabled.
"""
