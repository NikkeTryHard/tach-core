"""
Task 0.1.1-C: Bug Fix Tests - Nested Conftest Inheritance

Tests that fixtures in parent conftest.py files are visible to tests
in child directories.

Directory structure:
    tests/gauntlet_011/
        conftest.py (defines outer_fixture)
        nested_conftest/
            conftest.py (defines inner_fixture)
            test_conftest_nested.py (this file, should see both fixtures)
"""

import pytest


# This file tests nested conftest fixture inheritance
# The fixtures used here are defined in the conftest.py files


def test_a_uses_outer_fixture(gauntlet_011_fixture):
    """Test that we can use fixture from parent conftest.py."""
    assert gauntlet_011_fixture == "gauntlet_011_root"


def test_b_uses_inner_fixture(inner_fixture):
    """Test that we can use fixture from local conftest.py."""
    assert inner_fixture == "inner"


def test_c_uses_both_fixtures(gauntlet_011_fixture, inner_fixture):
    """Test that we can use fixtures from both parent and local conftest.py."""
    assert gauntlet_011_fixture == "gauntlet_011_root"
    assert inner_fixture == "inner"


def test_d_uses_nested_inheritance(inherited_fixture):
    """Test that inner fixture can use outer fixture as dependency."""
    # inherited_fixture depends on gauntlet_011_fixture
    assert inherited_fixture == "inherited from gauntlet_011_root"


class TestNestedConftestInClass:
    """Test class using nested conftest fixtures."""

    def test_e_class_uses_outer_fixture(self, gauntlet_011_fixture):
        """Test class method can use parent conftest fixture."""
        assert gauntlet_011_fixture == "gauntlet_011_root"

    def test_f_class_uses_inner_fixture(self, inner_fixture):
        """Test class method can use local conftest fixture."""
        assert inner_fixture == "inner"

    def test_g_class_uses_both(self, gauntlet_011_fixture, inner_fixture):
        """Test class method can use both fixtures."""
        assert gauntlet_011_fixture == "gauntlet_011_root"
        assert inner_fixture == "inner"
