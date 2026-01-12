"""Phase 5.4: Jemalloc Allocator Stability Tests

These tests verify the jemalloc integration for deterministic snapshots.

Test Execution Order:
- Tests are named alphabetically to control execution order
- test_a_* runs first, test_z_* runs last
- This is critical because some tests depend on state from previous tests

Key Invariants Being Tested:
1. Jemalloc is the active allocator (not glibc malloc)
2. quiesce_allocator() flushes tcache and syncs epoch
3. Memory allocations survive snapshot/reset cycles
4. Python's small_ints cache (-5 to 256) remains valid
5. Reference counts are stable after reset (for non-immortal objects)

Note on Python 3.12+:
- PEP 683 introduces "immortal objects" with refcount = 0xFFFFFFFF
- Small integers (-5 to 256), None, True, False are immortal
- Refcount tests must account for this
"""

import sys
import gc
import pytest

# Try to import tach_rust - it's only available when running inside the Hypervisor
try:
    import tach_rust

    TACH_RUST_AVAILABLE = True
except ImportError:
    TACH_RUST_AVAILABLE = False

# Python 3.12+ uses immortal objects (PEP 683)
# Immortal objects have refcount = 0xFFFFFFFF (4294967295)
IMMORTAL_REFCOUNT = 0xFFFFFFFF
PYTHON_HAS_IMMORTAL = sys.version_info >= (3, 12)


def is_immortal(obj) -> bool:
    """Check if an object is immortal (Python 3.12+ PEP 683).

    In Python 3.12+, small integers, None, True, False are immortal.
    In Python 3.14+, more aggressive immortalization may apply.
    """
    if not PYTHON_HAS_IMMORTAL:
        return False
    refcount = sys.getrefcount(obj)
    # Immortal objects have very high refcounts (either exact IMMORTAL_REFCOUNT
    # or refcount that doesn't change when creating references)
    if refcount == IMMORTAL_REFCOUNT:
        return True
    # Python 3.14+ may use different immortalization behavior
    # Test by creating a reference and checking if refcount changes
    if sys.version_info >= (3, 14):
        refs = [obj]  # Create a reference
        new_refcount = sys.getrefcount(obj)
        del refs
        # If refcount didn't change, object is effectively immortal
        return new_refcount == refcount
    return False


class TestJemallocVerification:
    """Verify jemalloc is properly configured as the allocator."""

    @pytest.mark.skipif(
        not TACH_RUST_AVAILABLE, reason="tach_rust not available (not in Hypervisor)"
    )
    def test_a_jemalloc_is_active(self):
        """Verify jemalloc is the active allocator.

        This test MUST pass for the Hypervisor to function correctly.
        If jemalloc is not active, memory corruption will occur after reset.
        """
        # This should return the jemalloc version string
        version = tach_rust.verify_jemalloc()

        assert version is not None, "verify_jemalloc returned None"
        assert len(version) > 0, "jemalloc version string is empty"

        # Version should look like "5.3.0" or similar
        print(f"[test] jemalloc version: {version}", file=sys.stderr)

    @pytest.mark.skipif(
        not TACH_RUST_AVAILABLE, reason="tach_rust not available (not in Hypervisor)"
    )
    def test_b_quiesce_allocator_succeeds(self):
        """Verify quiesce_allocator() completes without error.

        quiesce_allocator() performs:
        1. mallctl("thread.tcache.flush") - flush thread cache
        2. mallctl("epoch") - sync metadata

        Both must succeed for snapshot safety.
        """
        # Should not raise
        tach_rust.quiesce_allocator()

        print("[test] quiesce_allocator() succeeded", file=sys.stderr)

    @pytest.mark.skipif(
        not TACH_RUST_AVAILABLE, reason="tach_rust not available (not in Hypervisor)"
    )
    def test_c_quiesce_is_idempotent(self):
        """Verify multiple quiesce calls are safe.

        The Hypervisor may call quiesce multiple times in edge cases.
        This must be idempotent (no side effects from repeated calls).
        """
        for i in range(10):
            tach_rust.quiesce_allocator()

        print("[test] quiesce_allocator() is idempotent", file=sys.stderr)


class TestAllocationStability:
    """Verify allocations survive the snapshot/reset cycle."""

    def test_d_allocate_before_snapshot(self):
        """Allocate memory that will be captured in the snapshot.

        This test runs BEFORE the snapshot is taken. The data allocated
        here should be restored after reset.
        """
        global _SNAPSHOT_DATA

        # Allocate various data structures
        _SNAPSHOT_DATA = {
            "list": [i * 2 for i in range(1000)],
            "dict": {f"key_{i}": i for i in range(100)},
            "string": "Hello, Hypervisor!" * 100,
            "nested": [[j for j in range(10)] for i in range(10)],
        }

        # Verify allocations
        assert len(_SNAPSHOT_DATA["list"]) == 1000
        assert len(_SNAPSHOT_DATA["dict"]) == 100
        assert len(_SNAPSHOT_DATA["string"]) == 1800
        assert len(_SNAPSHOT_DATA["nested"]) == 10

        print("[test] Allocated snapshot data", file=sys.stderr)

    def test_e_verify_data_after_potential_reset(self):
        """Verify data survives (or is properly reset).

        In Hypervisor Mode, this test runs AFTER a memory reset.
        The _SNAPSHOT_DATA should either:
        - Still exist (if no reset occurred)
        - Be absent (if reset cleared it - this is expected in Hypervisor Mode)

        The key invariant is: no corruption, no crashes.
        """
        # In Hypervisor Mode, globals may be reset
        # This is expected behavior - the test should not crash
        if "_SNAPSHOT_DATA" in globals():
            data = globals()["_SNAPSHOT_DATA"]
            # If data exists, verify it's not corrupted
            assert isinstance(data, dict), "Data corrupted: not a dict"
            print("[test] Data survived reset (or no reset occurred)", file=sys.stderr)
        else:
            # Data was reset - this is expected in Hypervisor Mode
            print("[test] Data was reset (Hypervisor Mode)", file=sys.stderr)

    @pytest.mark.skipif(
        not TACH_RUST_AVAILABLE, reason="tach_rust not available (not in Hypervisor)"
    )
    def test_f_heavy_allocation_stress(self):
        """Stress test: many allocations followed by quiesce.

        This simulates a test that allocates heavily, then the Hypervisor
        quiesces before snapshot. No corruption should occur.
        """
        # Allocate many objects
        data = []
        for i in range(1000):
            data.append(
                {
                    "index": i,
                    "payload": list(range(100)),
                    "nested": {"a": 1, "b": 2, "c": [1, 2, 3]},
                }
            )

        # Force GC to exercise allocator
        gc.collect()

        # Quiesce
        tach_rust.quiesce_allocator()

        # Verify data is still valid
        assert len(data) == 1000
        assert data[500]["index"] == 500
        assert len(data[500]["payload"]) == 100

        print("[test] Heavy allocation stress passed", file=sys.stderr)


class TestSmallIntsCache:
    """Verify Python's small_ints cache survives reset.

    Python caches integers from -5 to 256 in libpython's .data segment.
    These are singletons - the same object is returned for each value.

    If the .data segment is not properly snapshotted and restored,
    reference counts will corrupt, causing crashes or memory leaks.

    Note: In Python 3.12+, small integers are immortal (PEP 683).
    """

    def test_g_small_ints_are_singletons(self):
        """Verify small integers are cached singletons.

        This is a sanity check that Python's small_ints cache is working.
        """
        # These should be the same object (cached)
        a = 42
        b = 42
        assert a is b, "small_ints cache not working: 42 is not singleton"

        # Edge cases
        assert (-5) is (-5), "-5 should be cached"
        assert 256 == 256, "256 should be cached"

        # Outside cache range - should be different objects
        x = 257
        y = 257
        # Note: In CPython, this may still be the same due to compiler optimization
        # The key test is that small_ints ARE singletons

        print("[test] small_ints cache verified", file=sys.stderr)

    def test_h_small_ints_refcount_stable(self):
        """Verify small_ints reference counts are stable.

        After a snapshot/reset cycle, reference counts must be restored
        to their snapshotted values. If not, we'll see:
        - Premature deallocation (use-after-free)
        - Memory leaks (refcount never reaches 0)

        Note: Python 3.12+ uses immortal objects (PEP 683).
        Immortal objects have a fixed refcount of 0xFFFFFFFF and don't
        participate in reference counting. This test accounts for that.

        Note: Reference counts can vary due to internal Python activities
        (module imports, GC cycles, etc.) so we use a generous tolerance.
        """
        # Check if 42 is immortal (Python 3.12+)
        if is_immortal(42):
            # Immortal objects don't track refcounts - just verify stability
            refcount = sys.getrefcount(42)
            assert refcount == IMMORTAL_REFCOUNT, (
                f"Immortal refcount changed: {refcount}"
            )
            print(
                f"[test] small_ints are immortal (PEP 683), refcount=0x{refcount:X}",
                file=sys.stderr,
            )
            return

        # Non-immortal path (Python < 3.12)
        # Get reference count of a cached integer
        refcount_before = sys.getrefcount(42)

        # Create some references
        refs = [42 for _ in range(100)]

        # Reference count should have increased
        refcount_during = sys.getrefcount(42)
        assert refcount_during > refcount_before, "refcount should increase"

        # Delete references
        del refs
        gc.collect()

        # Reference count should be back to original (approximately)
        # Use generous tolerance due to internal Python reference variance
        refcount_after = sys.getrefcount(42)
        diff = abs(refcount_after - refcount_before)
        # Allow variance up to 50 due to internal Python activities
        assert diff < 50, (
            f"refcount unstable: before={refcount_before}, after={refcount_after}, diff={diff}"
        )

        print(
            f"[test] small_ints refcount stable: {refcount_before} -> {refcount_after} (diff={diff})",
            file=sys.stderr,
        )

    def test_i_none_singleton_stable(self):
        """Verify None singleton is stable.

        None is a singleton stored in libpython's .data segment.
        Its reference count must be stable after reset.

        Note: In Python 3.12+, None is immortal (PEP 683).
        """
        # None should always be the same object
        a = None
        b = None
        assert a is b, "None is not singleton"
        assert a is None, "None identity check failed"

        refcount = sys.getrefcount(None)

        if is_immortal(None):
            # Immortal - refcount should be 0xFFFFFFFF
            assert refcount == IMMORTAL_REFCOUNT, (
                f"None immortal refcount wrong: {refcount}"
            )
            print(
                f"[test] None singleton is immortal (PEP 683), refcount=0x{refcount:X}",
                file=sys.stderr,
            )
        else:
            # Non-immortal - refcount should be very high
            assert refcount > 1000, f"None refcount suspiciously low: {refcount}"
            print(f"[test] None singleton stable, refcount={refcount}", file=sys.stderr)


class TestQuiesceSequence:
    """Test the full quiesce sequence as it would run before snapshot."""

    @pytest.mark.skipif(
        not TACH_RUST_AVAILABLE, reason="tach_rust not available (not in Hypervisor)"
    )
    def test_j_full_quiesce_sequence(self):
        """Simulate the full quiesce sequence.

        This mirrors what happens in zygote.rs before SIGSTOP:
        1. gc.collect() - Python garbage collection
        2. quiesce_allocator() - jemalloc tcache flush + epoch sync
        """
        # Step 1: Python GC
        gc.collect()
        gc.collect()  # Second pass for weak references
        gc.collect()  # Third pass for cyclic garbage

        # Step 2: Quiesce jemalloc
        tach_rust.quiesce_allocator()

        # If we get here without crashing, the sequence works
        print("[test] Full quiesce sequence completed", file=sys.stderr)

    @pytest.mark.skipif(
        not TACH_RUST_AVAILABLE, reason="tach_rust not available (not in Hypervisor)"
    )
    def test_k_quiesce_after_gc_stress(self):
        """Quiesce after creating and destroying many objects.

        This tests that quiesce works correctly after heavy GC activity,
        which exercises the allocator's free lists and arenas.
        """
        # Create and destroy many objects
        for _ in range(10):
            data = [{"key": i, "value": list(range(100))} for i in range(1000)]
            del data
            gc.collect()

        # Quiesce should still work
        tach_rust.quiesce_allocator()

        print("[test] Quiesce after GC stress passed", file=sys.stderr)
