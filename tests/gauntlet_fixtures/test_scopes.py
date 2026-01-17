"""Test fixture scope behavior with Tach isolation."""
import pytest

# Track module-scoped fixture state within the test module
_fixture_instances = []

@pytest.fixture(scope="module")
def module_scoped_tracker():
    """Module-scoped fixture that tracks its lifecycle."""
    instance = {"id": len(_fixture_instances), "created": True}
    _fixture_instances.append(instance)
    yield instance
    instance["cleaned_up"] = True

def test_module_scope_first(module_scoped_resource, module_scoped_tracker):
    """First test using module-scoped fixture."""
    assert module_scoped_resource["created"] is True
    assert module_scoped_tracker["created"] is True
    # Store the instance id for comparison
    module_scoped_tracker["seen_by_first"] = True

def test_module_scope_second(module_scoped_resource, module_scoped_tracker):
    """Second test - should reuse same fixture instance."""
    assert module_scoped_resource["created"] is True
    # Should be the same instance as first test
    assert module_scoped_tracker.get("seen_by_first") is True
    # Cleanup should not have happened yet
    assert module_scoped_tracker.get("cleaned_up") is None
