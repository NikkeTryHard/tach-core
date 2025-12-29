"""Phase 5.3: Hot Reloading Verification Tests

These tests verify that sys.modules is properly cleaned between test executions
when workers are reused in Hypervisor Mode.

Test Strategy:
1. test_module_pollution_marker_set: Imports a unique module and sets a marker
2. test_module_pollution_marker_absent: Runs AFTER the first test on the SAME worker
   - If cleanup works: marker module is gone, test passes
   - If cleanup fails: marker module persists, test fails

The tests are named alphabetically to ensure execution order.
"""

import sys


# Unique marker module name that should NEVER be in baseline
_POLLUTION_MARKER = "_tach_phase5_pollution_marker_12345"


def test_a_module_pollution_marker_set():
    """First test: Set a pollution marker in sys.modules.

    This test imports a fake module into sys.modules.
    If hot reloading works, this module should be removed before the next test.
    """
    # Verify marker is not already present (sanity check)
    if _POLLUTION_MARKER in sys.modules:
        # This could happen if cleanup failed on a previous run
        del sys.modules[_POLLUTION_MARKER]

    # Create a fake module object and inject it
    class FakeModule:
        __name__ = _POLLUTION_MARKER
        __file__ = "/fake/path/to/pollution_marker.py"
        pollution_value = "I_WAS_HERE"

    sys.modules[_POLLUTION_MARKER] = FakeModule()

    # Verify it's now present
    assert _POLLUTION_MARKER in sys.modules, "Marker should be in sys.modules"
    assert sys.modules[_POLLUTION_MARKER].pollution_value == "I_WAS_HERE"

    print(f"[test] Set pollution marker: {_POLLUTION_MARKER}")


def test_b_module_pollution_marker_absent():
    """Second test: Verify the pollution marker was cleaned up.

    This test runs AFTER test_a on the same worker (if worker reuse is working).
    If hot reloading works correctly, the marker should have been removed.
    """
    if _POLLUTION_MARKER in sys.modules:
        # Cleanup failed - the marker persisted
        marker = sys.modules[_POLLUTION_MARKER]
        pollution_value = getattr(marker, 'pollution_value', 'UNKNOWN')

        # Clean up for next run
        del sys.modules[_POLLUTION_MARKER]

        raise AssertionError(
            f"Hot reloading FAILED: Pollution marker '{_POLLUTION_MARKER}' "
            f"was NOT cleaned up between tests. Value: {pollution_value}"
        )

    print(f"[test] Pollution marker absent - hot reloading works!")


def test_c_import_fresh_module():
    """Third test: Verify we can import a module fresh after cleanup.

    This test imports a standard library module that might have been
    imported by a previous test. If cleanup works, it should be re-imported.
    """
    # Import a module that's likely not in baseline
    import uuid

    # Generate a UUID to prove the module is functional
    test_uuid = uuid.uuid4()
    assert len(str(test_uuid)) == 36, "UUID should be 36 characters"

    print(f"[test] Fresh import works: generated UUID {test_uuid}")


def test_d_verify_protected_modules_intact():
    """Fourth test: Verify protected modules are NOT removed.

    Critical modules like sys, builtins, pytest should never be removed.
    """
    # These should ALWAYS be present
    protected = ["sys", "builtins", "_pytest", "pytest"]

    for mod_name in protected:
        assert mod_name in sys.modules, f"Protected module {mod_name} should be in sys.modules"

    # Verify sys is functional
    assert hasattr(sys, 'modules'), "sys.modules should exist"
    assert hasattr(sys, 'path'), "sys.path should exist"

    print(f"[test] Protected modules intact: {protected}")
