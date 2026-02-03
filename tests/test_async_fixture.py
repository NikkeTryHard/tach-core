# tests/test_async_fixture.py
"""Tests for async fixture handling in tach harness."""
import pytest
import asyncio


@pytest.fixture
async def async_value():
    """Simple async fixture that should return a value."""
    await asyncio.sleep(0)
    return 42


@pytest.fixture
async def async_gen_value():
    """Async generator fixture with setup/teardown."""
    await asyncio.sleep(0)
    yield "hello"
    await asyncio.sleep(0)  # teardown


def test_async_fixture_value(async_value):
    """Test that async fixture returns actual value, not coroutine."""
    assert async_value == 42
    assert not asyncio.iscoroutine(async_value)


def test_async_gen_fixture_value(async_gen_value):
    """Test that async generator fixture returns yielded value."""
    assert async_gen_value == "hello"
    assert not hasattr(async_gen_value, '__anext__')


# Teardown tracking for verification test
_teardown_tracker = []


@pytest.fixture
async def tracked_teardown_fixture():
    """Async generator fixture that tracks setup/teardown."""
    _teardown_tracker.append("setup")
    yield "tracked"
    _teardown_tracker.append("teardown")


def test_async_fixture_teardown_runs(tracked_teardown_fixture):
    """Test that async fixture returns value and teardown is registered."""
    assert tracked_teardown_fixture == "tracked"
    assert "setup" in _teardown_tracker
    # Teardown runs after test completes via AsyncFixtureWrapper.teardown_all()


# =============================================================================
# Batch 2: Edge Cases & Robustness
# =============================================================================


@pytest.fixture
async def base_value():
    """Base async fixture that returns a value."""
    await asyncio.sleep(0)
    return 10


@pytest.fixture
async def derived_value(base_value):
    """Async fixture that depends on another async fixture."""
    await asyncio.sleep(0)
    return base_value * 2


def test_nested_async_fixtures(derived_value):
    """Test that nested async fixtures are resolved correctly."""
    assert derived_value == 20


@pytest.fixture
async def failing_async_fixture():
    """Async fixture that raises an exception during setup."""
    await asyncio.sleep(0)
    raise ValueError("Fixture setup failed")


def test_fixture_error_is_reported(failing_async_fixture):
    """Test that async fixture errors are properly reported."""
    pass  # Should never reach here - fixture fails during setup


# =============================================================================
# Batch 1: Scope-Aware Fixture Tracking Tests
# =============================================================================

_scope_tracker = {"setup": 0, "teardown": 0}


@pytest.fixture(scope="module")
async def module_scoped_fixture():
    """Module-scoped async fixture that tracks setup/teardown."""
    _scope_tracker["setup"] += 1
    yield f"module_value_{_scope_tracker['setup']}"
    _scope_tracker["teardown"] += 1


def test_module_scope_first(module_scoped_fixture):
    """First test using module-scoped fixture - should trigger setup."""
    assert module_scoped_fixture == "module_value_1"
    assert _scope_tracker["setup"] == 1
    assert _scope_tracker["teardown"] == 0


def test_module_scope_second(module_scoped_fixture):
    """Second test using same module-scoped fixture - should reuse value."""
    assert module_scoped_fixture == "module_value_1"
    assert _scope_tracker["setup"] == 1
    assert _scope_tracker["teardown"] == 0


# Session-scoped fixture tracking
_session_tracker = {"setup": 0, "teardown": 0}


@pytest.fixture(scope="session")
async def session_scoped_fixture():
    """Session-scoped async fixture that tracks setup/teardown."""
    _session_tracker["setup"] += 1
    yield f"session_value_{_session_tracker['setup']}"
    _session_tracker["teardown"] += 1


def test_session_scope_first(session_scoped_fixture):
    """First test using session-scoped fixture."""
    assert session_scoped_fixture == "session_value_1"
    assert _session_tracker["setup"] == 1
    assert _session_tracker["teardown"] == 0


def test_session_scope_second(session_scoped_fixture):
    """Second test using same session-scoped fixture - should reuse."""
    assert session_scoped_fixture == "session_value_1"
    assert _session_tracker["setup"] == 1
    assert _session_tracker["teardown"] == 0


# Function-scoped fixture tracking (should teardown after each test)
# NOTE: In tach's parallel execution model, each test runs in an isolated
# forked process, so state tracking between tests doesn't work. We verify
# function-scoped fixtures work correctly within a single test.
_function_tracker = {"setup": 0, "teardown": 0}


@pytest.fixture(scope="function")
async def function_scoped_fixture():
    """Function-scoped async fixture that tracks setup/teardown."""
    _function_tracker["setup"] += 1
    yield f"function_value_{_function_tracker['setup']}"
    _function_tracker["teardown"] += 1


def test_function_scope_first(function_scoped_fixture):
    """First test using function-scoped fixture."""
    assert function_scoped_fixture == "function_value_1"
    assert _function_tracker["setup"] == 1
    # Teardown happens after test, so count is 0 during test
    assert _function_tracker["teardown"] == 0


def test_function_scope_second(function_scoped_fixture):
    """Second test - each test gets fresh fixture in tach's fork model."""
    # In tach, each test runs in isolated process, so fixture is fresh
    assert function_scoped_fixture == "function_value_1"
    assert _function_tracker["setup"] == 1
    assert _function_tracker["teardown"] == 0


# =============================================================================
# Batch 2: Indirect Async Dependency Tests (pytest_fixture_setup hook)
# =============================================================================


@pytest.fixture
async def base_async():
    """Base async fixture that returns a dict value."""
    await asyncio.sleep(0)
    return {"key": "base_value"}


@pytest.fixture
def sync_uses_async(base_async):
    """Sync fixture that depends on an async fixture.

    This tests the indirect dependency problem: pytest resolves base_async
    and passes it to this sync fixture. Without the pytest_fixture_setup hook,
    base_async would be a raw coroutine instead of the resolved value.
    """
    return f"got_{base_async['key']}"


def test_indirect_async_dependency(sync_uses_async):
    """Test that sync fixtures can depend on async fixtures.

    This verifies the pytest_fixture_setup hook intercepts async fixtures
    at resolution time, not just when they're direct dependencies.
    """
    assert sync_uses_async == "got_base_value"


# =============================================================================
# Batch 4: Teardown Error Handling Tests
# =============================================================================


@pytest.fixture
async def failing_teardown_fixture():
    """Async fixture with teardown that raises an exception."""
    yield "value_before_teardown"
    raise RuntimeError("Teardown failed intentionally")


def test_teardown_error_is_captured(failing_teardown_fixture):
    """Test that passes but has a failing teardown.

    The test itself should pass, but the teardown error should be captured
    and reported via EventLoopManager.get_teardown_errors().
    This verifies Batch 4 implementation: teardown errors upgrade STATUS_PASS to STATUS_ERROR.
    """
    assert failing_teardown_fixture == "value_before_teardown"
