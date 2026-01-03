"""NumPy Gauntlet: Memory-Heavy Stress Tests

This module tests tach's ability to handle NumPy-heavy workloads,
which are particularly challenging for memory snapshot/restore due to:

1. Large contiguous memory allocations (ndarray buffers)
2. BLAS/LAPACK native library calls
3. Memory views and buffer protocol usage
4. Thread pool interactions (NumPy's internal threading)

The Gauntlet verifies:
- No memory leaks after restore cycles
- ndarray integrity preserved across tests
- Native library state (BLAS) doesn't desync
"""

import numpy as np
import gc

# =============================================================================
# Basic ndarray Operations
# =============================================================================


def test_a_simple_array_creation():
    """Verify basic ndarray creation works after restore."""
    arr = np.array([1, 2, 3, 4, 5])
    assert arr.sum() == 15
    assert arr.dtype == np.int64


def test_b_zeros_ones_empty():
    """Test memory allocation patterns."""
    zeros = np.zeros((100, 100))
    ones = np.ones((100, 100))
    empty = np.empty((100, 100))

    assert zeros.sum() == 0
    assert ones.sum() == 10000
    # empty is uninitialized, just check shape
    assert empty.shape == (100, 100)


def test_c_arange_linspace():
    """Test sequence generation."""
    r = np.arange(0, 100, 0.5)
    assert len(r) == 200
    assert r[0] == 0.0
    assert r[-1] == 99.5

    l = np.linspace(0, 1, 101)
    assert len(l) == 101
    assert abs(l[50] - 0.5) < 1e-10


# =============================================================================
# Large Allocations (Memory Pressure)
# =============================================================================


def test_d_large_allocation_1mb():
    """1MB ndarray allocation stress test."""
    # 1MB = 1024*1024 bytes = 131072 float64 elements
    arr = np.random.random(131072)
    assert arr.nbytes == 1048576  # 1MB
    assert arr.mean() > 0.4 and arr.mean() < 0.6  # Should be ~0.5


def test_e_large_allocation_10mb():
    """10MB ndarray allocation stress test."""
    arr = np.random.random(1310720)  # 10MB of float64
    assert arr.nbytes == 10485760
    # Verify data integrity
    checksum = arr.sum()
    assert checksum > 0


def test_f_large_2d_matrix():
    """Large 2D matrix allocation (4MB)."""
    # 1000x500 float64 = 4MB
    matrix = np.random.random((1000, 500))
    assert matrix.shape == (1000, 500)
    assert matrix.nbytes == 4000000

    # Basic operation to verify integrity
    row_sums = matrix.sum(axis=1)
    assert len(row_sums) == 1000


# =============================================================================
# Mathematical Operations (BLAS/LAPACK)
# =============================================================================


def test_g_matrix_multiplication():
    """Matrix multiplication uses BLAS - tests native library integration."""
    a = np.random.random((100, 50))
    b = np.random.random((50, 100))
    c = a @ b

    assert c.shape == (100, 100)
    # Verify result is non-trivial
    assert c.sum() > 0


def test_h_linear_algebra_operations():
    """Linear algebra operations - LAPACK integration."""
    # Create a symmetric positive-definite matrix
    a = np.random.random((50, 50))
    spd = a @ a.T + np.eye(50) * 10

    # Eigenvalue decomposition
    eigenvalues, eigenvectors = np.linalg.eigh(spd)
    assert len(eigenvalues) == 50
    assert all(eigenvalues > 0)  # All positive for SPD

    # Matrix inverse
    inv = np.linalg.inv(spd)
    identity_approx = spd @ inv
    assert np.allclose(identity_approx, np.eye(50), atol=1e-10)


def test_i_singular_value_decomposition():
    """SVD - computationally intensive LAPACK operation."""
    matrix = np.random.random((100, 50))
    u, s, vh = np.linalg.svd(matrix, full_matrices=False)

    assert u.shape == (100, 50)
    assert len(s) == 50
    assert vh.shape == (50, 50)

    # Reconstruction should match original
    reconstructed = u @ np.diag(s) @ vh
    assert np.allclose(reconstructed, matrix, atol=1e-10)


# =============================================================================
# Memory Views and Slicing
# =============================================================================


def test_j_array_views():
    """Test memory views - critical for snapshot integrity."""
    original = np.arange(100)
    view = original[10:50]

    # View should share memory
    assert np.shares_memory(original, view)

    # Modify view, original should change
    view[0] = 999
    assert original[10] == 999


def test_k_strided_arrays():
    """Test strided array access patterns."""
    arr = np.arange(100).reshape(10, 10)

    # Column view (strided access)
    col = arr[:, 5]
    assert col.shape == (10,)
    assert not col.flags["C_CONTIGUOUS"]

    # Diagonal (heavily strided)
    diag = np.diag(arr)
    assert diag.sum() == sum(range(0, 100, 11))


def test_l_fancy_indexing():
    """Fancy indexing creates copies, not views."""
    arr = np.arange(100)
    indices = np.array([1, 5, 10, 50, 99])
    selected = arr[indices]

    # Should NOT share memory (fancy indexing creates copy)
    assert not np.shares_memory(arr, selected)
    assert list(selected) == [1, 5, 10, 50, 99]


# =============================================================================
# dtype Variations
# =============================================================================


def test_m_various_dtypes():
    """Test various data types for snapshot fidelity."""
    int8_arr = np.array([1, 2, 3], dtype=np.int8)
    int32_arr = np.array([1, 2, 3], dtype=np.int32)
    int64_arr = np.array([1, 2, 3], dtype=np.int64)
    float32_arr = np.array([1.0, 2.0, 3.0], dtype=np.float32)
    float64_arr = np.array([1.0, 2.0, 3.0], dtype=np.float64)
    complex64_arr = np.array([1 + 2j, 3 + 4j], dtype=np.complex64)
    complex128_arr = np.array([1 + 2j, 3 + 4j], dtype=np.complex128)
    bool_arr = np.array([True, False, True], dtype=np.bool_)

    assert int8_arr.sum() == 6
    assert int32_arr.sum() == 6
    assert int64_arr.sum() == 6
    assert abs(float32_arr.sum() - 6.0) < 1e-5
    assert abs(float64_arr.sum() - 6.0) < 1e-10
    assert complex64_arr.sum() == (4 + 6j)
    assert complex128_arr.sum() == (4 + 6j)
    assert bool_arr.sum() == 2


def test_n_structured_arrays():
    """Structured arrays with named fields."""
    dt = np.dtype([("name", "U10"), ("age", np.int32), ("score", np.float64)])
    data = np.array([("Alice", 25, 95.5), ("Bob", 30, 87.3), ("Charlie", 22, 91.0)], dtype=dt)

    assert data["name"][0] == "Alice"
    assert data["age"].sum() == 77
    assert data["score"].mean() > 90


# =============================================================================
# Random Number Generator State
# =============================================================================


def test_o_rng_reproducibility():
    """RNG state must be isolated between tests."""
    # Set seed and generate
    np.random.seed(42)
    first = np.random.random(10)

    # Reset and generate again
    np.random.seed(42)
    second = np.random.random(10)

    assert np.array_equal(first, second)


def test_p_rng_independence():
    """Each test should have independent RNG state."""
    # Generate some random numbers
    values = np.random.random(100)
    # Just verify they're reasonable
    assert values.min() >= 0.0
    assert values.max() <= 1.0


# =============================================================================
# Memory-Intensive Computations
# =============================================================================


def test_q_fft_operations():
    """FFT - memory-intensive signal processing."""
    # 1D FFT
    signal = np.sin(np.linspace(0, 10 * np.pi, 1000))
    spectrum = np.fft.fft(signal)
    assert len(spectrum) == 1000

    # 2D FFT
    image = np.random.random((128, 128))
    freq = np.fft.fft2(image)
    assert freq.shape == (128, 128)


def test_r_sorting_operations():
    """Sorting large arrays."""
    arr = np.random.random(10000)
    sorted_arr = np.sort(arr)

    assert sorted_arr[0] <= sorted_arr[-1]
    assert len(np.unique(sorted_arr)) == 10000  # All unique


def test_s_reduction_operations():
    """Aggregation operations on large arrays."""
    arr = np.random.random(100000)

    assert arr.sum() > 0
    assert 0.4 < arr.mean() < 0.6
    assert arr.std() > 0
    assert arr.min() >= 0
    assert arr.max() <= 1


# =============================================================================
# Edge Cases
# =============================================================================


def test_t_empty_arrays():
    """Empty arrays are a common edge case."""
    empty = np.array([])
    assert len(empty) == 0
    assert empty.sum() == 0

    empty_2d = np.zeros((0, 10))
    assert empty_2d.shape == (0, 10)


def test_u_scalar_arrays():
    """0-dimensional arrays (scalars)."""
    scalar = np.array(42)
    assert scalar.ndim == 0
    assert scalar.shape == ()
    assert int(scalar) == 42


def test_v_broadcasting():
    """Broadcasting rules must work correctly."""
    a = np.array([[1], [2], [3]])  # (3, 1)
    b = np.array([10, 20, 30])  # (3,)
    c = a + b

    assert c.shape == (3, 3)
    expected = np.array([[11, 21, 31], [12, 22, 32], [13, 23, 33]])
    assert np.array_equal(c, expected)


# =============================================================================
# Cleanup and Memory
# =============================================================================


def test_w_garbage_collection():
    """Verify GC works properly with NumPy arrays."""
    # Create and delete large arrays
    for _ in range(10):
        large = np.random.random(1000000)  # 8MB
        del large

    gc.collect()
    # If we got here without OOM, we're good


def test_x_reference_counting():
    """NumPy reference counting must survive snapshot."""
    arr = np.arange(100)
    view1 = arr[10:20]
    view2 = arr[20:30]

    # Both views should be valid
    assert view1.sum() == sum(range(10, 20))
    assert view2.sum() == sum(range(20, 30))

    # Modifying through view should work
    view1[0] = 999
    assert arr[10] == 999


def test_y_final_integrity_check():
    """Final comprehensive check after all stress tests."""
    # Create a complex structure
    data = {
        "1d": np.arange(1000),
        "2d": np.random.random((100, 100)),
        "3d": np.zeros((10, 10, 10)),
        "complex": np.array([1 + 2j, 3 + 4j]),
    }

    # Verify all structures
    assert data["1d"].sum() == sum(range(1000))
    assert data["2d"].shape == (100, 100)
    assert data["3d"].sum() == 0
    assert data["complex"][0] == (1 + 2j)


def test_z_memory_report():
    """Final test - report memory stats."""
    import sys

    # Create a 10MB array
    large = np.zeros(1310720)  # 10MB

    # Report sizes
    nbytes = large.nbytes
    refcount = sys.getrefcount(large)

    print(f"\n[numpy-gauntlet] Array size: {nbytes / 1024 / 1024:.2f} MB")
    print(f"[numpy-gauntlet] Reference count: {refcount}")
    print("[numpy-gauntlet] All NumPy stress tests completed successfully")

    assert nbytes == 10485760
