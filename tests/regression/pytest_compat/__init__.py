"""Pytest API compatibility comparison tests for tach-core.

This package contains tests that compare tach-core's pytest harness behavior
against real pytest to detect API drift and compatibility issues.

Usage:
    pytest tests/regression/pytest_compat/

The comparison tests run the same sample tests through both:
1. Real pytest (subprocess call to pytest)
2. tach-core (subprocess call to tach-core binary)

And verify that test outcomes (passed/failed/skipped) match.
"""
