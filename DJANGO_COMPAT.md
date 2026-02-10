# Django Compatibility Tracker

Target: run Django's own test suite through tach-core with full isolation.

## Test Setup

```bash
# Clone Django tests (already at /tmp/django-tests in Docker)
git clone --depth 1 https://github.com/django/django.git /tmp/django-tests
cd /tmp/django-tests

# Django's test runner does dynamic INSTALLED_APPS injection.
# Raw `pytest` or `tach-core` needs runtests.py bootstrap first:
python3 -c "
import os, sys, tempfile
os.environ['TMPDIR'] = tempfile.mkdtemp()
os.environ['DJANGO_SETTINGS_MODULE'] = 'test_sqlite'
sys.path.insert(0, '.')
from runtests import setup_run_tests
setup_run_tests(0, None, None, test_labels=['utils_tests'])
"

# Then run either:
/workspace/target/release/tach-core utils_tests/
python3 -m pytest -x -q utils_tests/
```

## Discovery Status

| Scope | tach-core | pytest | Match |
|-------|-----------|--------|-------|
| `utils_tests/` | 682 | 682 | ✅ |
| Full suite (`.`) | 9776 | TBD | ❓ |

## Issues Fixed

### PR #87 — Suffix + inheritance-based test class matching
- Classes ending in `*Test`, `*Tests`, `*TestCase` now detected
- `has_testcase_base()` checks AST bases for `*TestCase` lineage

### PR #88 — Django test database initialization before fork
- `_setup_django_test_db()` called in zygote before snapshot
- Workers inherit test DB state instead of hitting production DB

### PR #89 — Call _setup_django_test_db via module ref
- Fixed import issue where harness module ref was stale after fork

### PR #90 — TransactionTestCase isolation via toxic queue routing
- `transaction=True` tests marked toxic → run in isolated workers
- Stale DB connections closed before transaction tests

### PR #91 — Propagate class-level markers to test methods
- `@pytest.mark.django_db` on class now applies to all methods

### PR #92 — Auto-detect overlayfs and fallback for Docker
- Nested overlay-on-overlay (Docker) detected → skip mount
- Self-test diagnostic for overlay filesystem added

### PR #93 — TUI responsiveness
- Socket timeout 5s → 100ms
- Steady tick enabled for progress bar

### PR #94 — Transitive inheritance + inherited methods + bare test
- Two-phase fixed-point algorithm in `inheritance.rs`
- `ClassInfo` struct tracks all classes for cross-file resolution
- Bare `test` method name supported (not just `test_*`)

### PR #95 — Match pytest collection rules
- `is_test_class()` prefix-only (`Test*`), matching `python_classes` default
- Suffix patterns (`*Test`, `*Tests`) no longer collect mixin classes
- AST detection: `__test__ = False`, `__init__`, `__new__`, `@abstractmethod`, `abc.ABC`
- Exclusion flags propagated through inheritance resolution
- `tach list` now prints summary: `Discovered X tests in Y files`

## Known Gaps / Next Steps

### Execution gaps (not yet tested at scale)
- [ ] Full Django suite execution (9776 tests) — not yet attempted
- [ ] Multi-database tests (`databases=["default", "other"]`)
- [ ] GIS tests (need PostGIS, skip with sqlite)
- [ ] Tests requiring specific backends (postgres, mysql)

### Discovery gaps (none for utils_tests)
- `TestFinder` in `test_module_loading.py` — has `__init__`, correctly skipped by both tach and pytest

### Django setup quirk
Django's `runtests.py` dynamically adds test modules to `INSTALLED_APPS`.
Without this bootstrap, some files fail to import (`AppRegistryNotReady`).
This is Django's design, not a tach bug — pytest hits the same issue.

## Test Counts Reference

```
887 unit tests (cargo nextest run --lib)
4 pre-existing integration test failures (unrelated to Django work)
```
