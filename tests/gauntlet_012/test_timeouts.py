# test_timeouts.py - Tests for timeout marker parsing and handling
# Validates @pytest.mark.timeout(N) marker is correctly parsed by tach discovery

import time

import pytest


class TestTimeoutMarkerParsing:
    """Tests that verify @pytest.mark.timeout markers are correctly parsed.

    These tests are designed to be discovered by tach's AST scanner.
    The scanner should extract timeout_secs from the decorator.
    """

    def test_a_simple_test_without_timeout(self):
        """Test without any timeout marker - should use global timeout."""
        # This test has no timeout marker, so it uses the default/global timeout
        assert True

    def test_b_fast_test_passes_within_timeout(self):
        """A fast test that should pass."""
        result = 1 + 1
        assert result == 2

    @pytest.mark.timeout(60)
    def test_c_with_explicit_timeout(self):
        """Verify timeout marker is parsed and used."""
        assert True

    @pytest.mark.timeout(seconds=120)
    def test_d_with_keyword_timeout(self):
        """Verify keyword-style timeout marker is parsed."""
        assert True

    @pytest.mark.timeout(0)
    def test_e_with_zero_timeout(self):
        """Verify timeout=0 means no timeout (pytest-timeout convention)."""
        assert True


class TestTimeoutBehavior:
    """Tests for timeout behavior during execution."""

    def test_a_quick_operation(self):
        """Quick operation that finishes well before any timeout."""
        data = [i * 2 for i in range(100)]
        assert len(data) == 100

    def test_b_multiple_assertions(self):
        """Test with multiple assertions that all pass quickly."""
        assert 1 == 1
        assert "hello" == "hello"
        assert [1, 2, 3] == [1, 2, 3]
        assert {"a": 1} == {"a": 1}


class TestTimeoutEdgeCases:
    """Edge cases for timeout handling."""

    def test_a_very_fast_test(self):
        """Test that completes almost instantly."""
        pass

    def test_b_test_with_small_computation(self):
        """Test with a small computation."""
        total = sum(range(1000))
        assert total == 499500

    def test_c_test_with_string_operations(self):
        """Test with string operations that are fast."""
        s = "hello" * 100
        assert len(s) == 500
        assert s.count("hello") == 100


class TestTimeoutMarkerVariations:
    """Tests to verify different timeout marker syntaxes.

    Note: These tests themselves don't actually test timeout behavior,
    but their presence helps verify the scanner correctly parses
    different decorator formats. The actual parsing is tested in
    Rust unit tests (scanner.rs).
    """

    def test_a_no_decorator(self):
        """Test without any decorator - baseline."""
        assert True

    def test_b_other_decorators(self):
        """Test that can have other decorators without issues."""
        # In real usage, this might have @pytest.mark.slow or similar
        assert True


class TestTimeoutIntegration:
    """Integration tests for timeout functionality."""

    def test_a_test_that_respects_timeout(self):
        """Test that demonstrates we can set up timeout scenarios."""
        # This test is quick and should pass
        start = time.time()
        # Do something trivial
        _ = [x**2 for x in range(50)]
        elapsed = time.time() - start
        # Should complete in less than 1 second
        assert elapsed < 1.0

    def test_b_cpu_bound_quick(self):
        """CPU-bound test that finishes quickly."""
        result = 0
        for i in range(1000):
            result += i
        assert result == 499500

    def test_c_io_simulation_quick(self):
        """Simulates quick I/O-like operations."""
        # Small sleep to simulate minimal I/O
        time.sleep(0.01)  # 10ms
        assert True
