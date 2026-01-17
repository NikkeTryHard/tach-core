"""Test that hooks execute in root-to-leaf order."""
import os
import sys


def test_root_hook_executed_first():
    """Verify root pytest_configure ran before leaf."""
    assert os.environ.get("TACH_020_ROOT_HOOK") == "executed"
    assert os.environ.get("TACH_020_LEAF_HOOK") == "executed+leaf"


def test_sys_path_modified():
    """Verify sys.path was modified by root hook."""
    assert "/tmp/tach_020_test_path" in sys.path
