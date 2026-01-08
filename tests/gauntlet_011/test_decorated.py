"""
Task 0.1.1-C: Bug Fix Tests - Decorated Function Discovery

Tests that decorated test functions are properly discovered by tach-core.
Decorated tests with @pytest.mark.* or custom decorators should be found.
"""

import pytest

# =============================================================================
# Basic pytest.mark decorators
# =============================================================================


@pytest.mark.slow
def test_a_slow_marked_test():
    """Test with @pytest.mark.slow should be discovered."""
    assert True


@pytest.mark.skip(reason="Testing skip decorator detection")
def test_b_skip_marked_test():
    """Test with @pytest.mark.skip should be discovered."""
    assert True


@pytest.mark.skipif(False, reason="Testing skipif decorator detection")
def test_c_skipif_marked_test():
    """Test with @pytest.mark.skipif should be discovered."""
    assert True


@pytest.mark.xfail(reason="Testing xfail decorator detection")
def test_d_xfail_marked_test():
    """Test with @pytest.mark.xfail should be discovered."""
    assert True


# =============================================================================
# Custom decorators
# =============================================================================


def custom_decorator(func):
    """A simple custom decorator for testing."""

    def wrapper(*args, **kwargs):
        return func(*args, **kwargs)

    return wrapper


def custom_decorator_with_args(arg1, arg2=None):
    """A custom decorator with arguments."""

    def decorator(func):
        def wrapper(*args, **kwargs):
            return func(*args, **kwargs)

        return wrapper

    return decorator


@custom_decorator
def test_e_custom_decorator():
    """Test with custom decorator should be discovered."""
    assert True


@custom_decorator_with_args("arg1", arg2="arg2")
def test_f_custom_decorator_with_args():
    """Test with custom decorator that takes args should be discovered."""
    assert True


# =============================================================================
# Decorator chains
# =============================================================================


@pytest.mark.slow
@custom_decorator
def test_g_multiple_decorators():
    """Test with multiple decorators should be discovered."""
    assert True


@pytest.mark.slow
@pytest.mark.timeout(30)
@custom_decorator
def test_h_decorator_chain():
    """Test with long decorator chain should be discovered."""
    assert True


# =============================================================================
# Class with decorated methods
# =============================================================================


class TestDecoratedMethods:
    """Test class with decorated methods."""

    @pytest.mark.slow
    def test_i_decorated_method(self):
        """Decorated method in class should be discovered."""
        assert True

    @custom_decorator
    def test_j_custom_decorated_method(self):
        """Method with custom decorator should be discovered."""
        assert True

    @pytest.mark.parametrize("x", [1, 2, 3])
    def test_k_parametrized_method(self, x):
        """Parametrized method should be discovered."""
        assert x in [1, 2, 3]


# =============================================================================
# Async decorated tests
# =============================================================================


@pytest.mark.skip(reason="Requires pytest-asyncio which may not be installed")
@pytest.mark.asyncio
async def test_l_async_decorated():
    """Async test with marker should be discovered."""
    import asyncio

    await asyncio.sleep(0)
    assert True
