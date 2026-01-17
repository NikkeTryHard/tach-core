"""Parallel isolation tests for Django database transactions.

These tests verify that when running with multiple workers (tach -n 4),
each test sees isolated database state despite running in parallel.

Each test creates a unique record and asserts count == 1. If isolation
is working, all tests should pass even when running concurrently.
"""
import pytest
import uuid


# Skip all tests if Django is not available
django = pytest.importorskip("django")


def _create_unique_record(prefix: str) -> str:
    """Create a record with a unique name and return the name."""
    from django_project.models import TestModel

    unique_name = f"{prefix}_{uuid.uuid4().hex[:8]}"
    TestModel.objects.create(name=unique_name, value=42)
    return unique_name


@pytest.mark.django_db
def test_parallel_worker_1():
    """Worker 1: Create unique record, verify isolation."""
    from django_project.models import TestModel

    name = _create_unique_record("worker1")

    # Should see exactly 1 record with our unique name
    count = TestModel.objects.filter(name=name).count()
    assert count == 1, f"Worker 1: Expected 1 record, got {count}"

    # Total count should be low (just our record, maybe some from this test)
    total = TestModel.objects.count()
    assert total <= 10, f"Worker 1: Too many records ({total}), possible pollution"


@pytest.mark.django_db
def test_parallel_worker_2():
    """Worker 2: Create unique record, verify isolation."""
    from django_project.models import TestModel

    name = _create_unique_record("worker2")

    count = TestModel.objects.filter(name=name).count()
    assert count == 1, f"Worker 2: Expected 1 record, got {count}"

    total = TestModel.objects.count()
    assert total <= 10, f"Worker 2: Too many records ({total}), possible pollution"


@pytest.mark.django_db
def test_parallel_worker_3():
    """Worker 3: Create unique record, verify isolation."""
    from django_project.models import TestModel

    name = _create_unique_record("worker3")

    count = TestModel.objects.filter(name=name).count()
    assert count == 1, f"Worker 3: Expected 1 record, got {count}"

    total = TestModel.objects.count()
    assert total <= 10, f"Worker 3: Too many records ({total}), possible pollution"


@pytest.mark.django_db
def test_parallel_worker_4():
    """Worker 4: Create unique record, verify isolation."""
    from django_project.models import TestModel

    name = _create_unique_record("worker4")

    count = TestModel.objects.filter(name=name).count()
    assert count == 1, f"Worker 4: Expected 1 record, got {count}"

    total = TestModel.objects.count()
    assert total <= 10, f"Worker 4: Too many records ({total}), possible pollution"


@pytest.mark.django_db
def test_parallel_worker_5():
    """Worker 5: Create unique record, verify isolation."""
    from django_project.models import TestModel

    name = _create_unique_record("worker5")

    count = TestModel.objects.filter(name=name).count()
    assert count == 1, f"Worker 5: Expected 1 record, got {count}"

    total = TestModel.objects.count()
    assert total <= 10, f"Worker 5: Too many records ({total}), possible pollution"


@pytest.mark.django_db
def test_parallel_worker_6():
    """Worker 6: Create unique record, verify isolation."""
    from django_project.models import TestModel

    name = _create_unique_record("worker6")

    count = TestModel.objects.filter(name=name).count()
    assert count == 1, f"Worker 6: Expected 1 record, got {count}"

    total = TestModel.objects.count()
    assert total <= 10, f"Worker 6: Too many records ({total}), possible pollution"


@pytest.mark.django_db
def test_parallel_worker_7():
    """Worker 7: Create unique record, verify isolation."""
    from django_project.models import TestModel

    name = _create_unique_record("worker7")

    count = TestModel.objects.filter(name=name).count()
    assert count == 1, f"Worker 7: Expected 1 record, got {count}"

    total = TestModel.objects.count()
    assert total <= 10, f"Worker 7: Too many records ({total}), possible pollution"


@pytest.mark.django_db
def test_parallel_worker_8():
    """Worker 8: Create unique record, verify isolation."""
    from django_project.models import TestModel

    name = _create_unique_record("worker8")

    count = TestModel.objects.filter(name=name).count()
    assert count == 1, f"Worker 8: Expected 1 record, got {count}"

    total = TestModel.objects.count()
    assert total <= 10, f"Worker 8: Too many records ({total}), possible pollution"
