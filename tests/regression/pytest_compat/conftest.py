"""Pytest configuration for pytest_compat tests.

Uses collect_ignore_glob to exclude sample_tests from direct collection.
Sample tests are intentionally designed to pass/fail and are run
via subprocess from the comparison tests.
"""

# Ignore sample_tests directory when collecting tests from this directory
collect_ignore_glob = ["sample_tests/*"]
