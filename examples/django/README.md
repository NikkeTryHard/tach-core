# Django Example with Tach

This example demonstrates how to use Tach with Django database tests. Tach provides significant performance improvements for Django test suites through memory snapshots and connection pool preservation.

---

## Overview

This minimal Django project showcases:

- **Model tests** - Testing Django ORM models with the database
- **Database fixtures** - Using pytest fixtures for test data
- **pytest-django integration** - The `@pytest.mark.django_db` marker

---

## Prerequisites

| Requirement | Version |
| :---------- | :------ |
| Python      | 3.10+   |
| Django      | 4.2+    |
| pytest      | 7.0+    |
| Tach        | 0.1.0+  |

---

## Setup

```bash
# Create virtual environment
python -m venv .venv
source .venv/bin/activate

# Install dependencies
pip install django pytest pytest-django

# Run migrations
python manage.py migrate

# Verify Django setup
python manage.py check
```

---

## Running Tests

### With pytest (traditional)

```bash
pytest myapp/tests/ -v
```

### With Tach (fast)

```bash
# From project root
tach-core examples/django/

# Or from this directory
tach-core .

# With parallel workers
tach-core -n 4 .
```

---

## Project Structure

```
examples/django/
  README.md           # This file
  pyproject.toml      # Project configuration
  manage.py           # Django management script
  myapp/
    __init__.py
    models.py         # User and Task models
    settings.py       # Django settings (SQLite)
    tests/
      __init__.py
      conftest.py     # pytest-django fixtures
      test_models.py  # Model tests
```

---

## Key Concepts

### Database Marker

All database tests must use the `@pytest.mark.django_db` marker:

```python
import pytest
from myapp.models import User

@pytest.mark.django_db
def test_user_creation():
    user = User.objects.create(username="testuser", email="test@example.com")
    assert user.id is not None
```

### Fixtures

Use fixtures to create reusable test data:

```python
@pytest.fixture
def sample_user(db):
    return User.objects.create(username="sample", email="sample@example.com")

@pytest.mark.django_db
def test_with_fixture(sample_user):
    assert sample_user.username == "sample"
```

### Transaction Rollback

Tach automatically handles transaction rollbacks after each test, preserving database state isolation without the overhead of creating new connections.

---

## Performance Comparison

| Metric           | pytest (standard) | Tach             |
| :--------------- | :---------------- | :--------------- |
| Connection setup | Per test          | Once (preserved) |
| State reset      | Drop/recreate     | Transaction      |
| Typical speedup  | 1x                | 10-50x           |

---

## Troubleshooting

### Database not found

Ensure migrations have been run:

```bash
python manage.py migrate
```

### Permission denied

Tach requires Linux kernel 5.13+ for Landlock. Check your kernel:

```bash
uname -r
```

### pytest-django not found

Install the package:

```bash
pip install pytest-django
```

---

## Related Documentation

- [Tach Configuration](../../docs/configuration.md)
- [Tach Quickstart](../../docs/quickstart.md)
- [pytest-django documentation](https://pytest-django.readthedocs.io/)
