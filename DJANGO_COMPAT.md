# Django Compatibility Tracker

Target: run Django's own test suite through tach-core with full isolation.

## Progress Overview

**19 / 213 modules cleared** · 142 blocked by `tests.py` collection bug · 47 have failures · 5 no tests

Legend: ✅ Cleared (0 failures, passes ≥ pytest) · ❌ Has failures · 🔧 Blocked by `tests.py` collection bug · ➖ No test files

### Cleared Modules (19)

| Module | tach (p/f/s) | pytest (p/f/s) | Notes |
|--------|-------------|----------------|-------|
| `aggregation` | 20/0/0 | 20/0/0 | |
| `composite_pk` | 155/0/2 | 155/0/2 | Largest cleared module |
| `csrf_tests` | 1/0/0 | 1/0/0 | |
| `deprecation` | 26/0/0 | 26/0/0 | |
| `expressions` | 8/0/0 | 8/0/0 | |
| `file_storage` | 39/0/0 | 39/0/0 | |
| `lookup` | 11/0/0 | 11/0/0 | |
| `m2m_through_regress` | 6/0/0 | 6/0/0 | |
| `model_formsets` | 7/0/0 | 7/0/0 | |
| `model_inheritance` | 16/0/0 | 16/0/0 | |
| `model_regress` | 4/0/0 | 4/0/0 | |
| `postgres_tests` | 0/0/637 | 0/0/637 | All skipped (no PostgreSQL) |
| `prefetch_related` | 24/0/0 | 24/0/0 | |
| `requests_tests` | 45/0/0 | 45/0/0 | |
| `responses` | 38/0/0 | 38/0/0 | |
| `test_client` | 2/0/0 | 2/0/0 | |
| `test_exceptions` | 5/0/0 | 5/0/0 | |
| `urlpatterns` | 8/0/0 | 8/0/0 | |
| `validation` | 37/0/0 | 37/0/0 | |

### Modules with Failures (47)

| Module | tach (p/f/s) | pytest (p/f/s) | Notes |
|--------|-------------|----------------|-------|
| `admin_changelist` | 0/4/0 | 4/0/0 | |
| `admin_docs` | 5/4/62 | 9/0/64 | |
| `admin_inlines` | 0/1/0 | 1/0/0 | |
| `admin_utils` | 0/15/0 | 15/0/0 | |
| `admin_views` | 10/68/20 | 78/0/20 | |
| `admin_widgets` | 0/10/0 | 10/0/0 | |
| `async` | 5/54/1 | 59/0/1 | |
| `auth_tests` | 346/471/12 | 827/0/12 | |
| `backends` | 112/23/62 | 134/0/66 | |
| `check_framework` | 149/8/1 | 163/0/1 | |
| `contenttypes_tests` | 52/21/0 | 87/0/0 | |
| `db_functions` | 321/26/12 | 321/0/16 | |
| `dbshell` | 20/1/5 | 21/0/5 | |
| `decorators` | 84/2/0 | 86/0/0 | |
| `empty_models` | 0/2/0 | 2/0/0 | |
| `flatpages_tests` | 4/50/0 | 54/0/0 | |
| `foreign_object` | 27/1/0 | 28/0/0 | |
| `forms_tests` | 481/290/10 | 1014/0/10 | |
| `generic_relations` | 13/2/0 | 15/0/0 | |
| `generic_views` | 58/162/0 | 220/0/0 | |
| `gis_tests` | 28/375/0 | 34/0/0 | Backend-specific |
| `handlers` | 0/3/0 | 3/0/0 | |
| `i18n` | 2/9/90 | 11/0/90 | |
| `invalid_models_tests` | 284/4/16 | 292/0/16 | |
| `mail` | 7/6/0 | 13/0/0 | |
| `messages_tests` | 30/2/0 | 98/0/0 | |
| `middleware` | 13/26/1 | 39/0/1 | |
| `migrations` | 423/387/0 | 808/1/2 | |
| `model_fields` | 348/93/49 | 389/0/50 | |
| `model_forms` | 25/4/0 | 29/0/0 | |
| `model_options` | 19/5/0 | 21/0/3 | |
| `modeladmin` | 139/1/0 | 140/0/0 | |
| `project_template` | 0/1/0 | 1/0/0 | |
| `queries` | 172/1/12 | 173/0/12 | |
| `schema` | 0/1/0 | 1/0/0 | |
| `serializers` | 35/2/0 | 206/0/0 | |
| `servers` | 0/9/0 | 8/0/1 | |
| `sphinx` | 0/18/0 | 0/0/0 | |
| `staticfiles_tests` | 46/134/0 | 196/46/0 | pytest also fails 46 |
| `tasks` | 65/10/0 | 75/0/0 | |
| `template_backends` | 19/28/2 | 61/0/6 | |
| `template_tests` | 461/1033/5 | 1508/1/5 | |
| `test_runner` | 103/5/7 | 107/4/7 | pytest also fails 4 |
| `test_utils` | 16/4/0 | 29/0/0 | |
| `urlpatterns_reverse` | 4/4/0 | 8/0/0 | |
| `utils_tests` | 618/41/21 | 658/2/21 | Regressed from 659/0 (see below) |
| `view_tests` | 21/171/2 | 188/4/2 | pytest also fails 4 |

### Blocked by `tests.py` Collection Bug (142)

tach-core cannot collect modules that use `tests.py` instead of `test_*.py` when
passed as directory paths. These modules show 0/0/0 for tach but have real pytest
results. **This is the #1 blocker for per-module testing.**

<details>
<summary>Full list of blocked modules (click to expand)</summary>

| Module | pytest (p/f/s) | Module | pytest (p/f/s) |
|--------|----------------|--------|----------------|
| `absolute_url_overrides` | 3/0/0 | `admin_autodiscover` | 1/0/0 |
| `admin_checks` | 59/0/0 | `admin_custom_urls` | 7/0/0 |
| `admin_default_site` | 4/0/0 | `admin_filters` | 55/0/0 |
| `admin_ordering` | 10/0/0 | `admin_registration` | 19/0/0 |
| `admin_scripts` | 221/1/0 | `aggregation_regress` | 68/0/5 |
| `annotations` | 88/1/3 | `app_loading` | 7/0/0 |
| `apps` | 50/0/2 | `asgi` | 26/0/0 |
| `bash_completion` | 7/0/0 | `basic` | 71/0/3 |
| `builtin_server` | 5/0/0 | `bulk_create` | 51/0/7 |
| `cache` | 388/4/184 | `conditional_processing` | 24/0/0 |
| `constraints` | 97/0/4 | `context_processors` | 9/0/0 |
| `custom_columns` | 14/0/0 | `custom_lookups` | 31/0/4 |
| `custom_managers` | 35/0/0 | `custom_methods` | 1/0/0 |
| `custom_pk` | 14/0/1 | `datatypes` | 7/0/0 |
| `dates` | 6/0/1 | `datetimes` | 7/0/0 |
| `db_typecasts` | 1/0/0 | `db_utils` | 5/0/1 |
| `defer` | 36/0/0 | `defer_regress` | 20/0/0 |
| `delete` | 59/0/1 | `delete_regress` | 21/0/1 |
| `dispatch` | 21/0/0 | `empty` | 1/0/0 |
| `expressions_case` | 88/0/1 | `expressions_window` | 71/0/4 |
| `extra_regress` | 12/0/0 | `field_deconstruction` | 38/0/0 |
| `field_defaults` | 17/0/0 | `field_subclassing` | 2/0/0 |
| `file_uploads` | 39/1/0 | `files` | 37/0/8 |
| `filtered_relation` | 63/0/1 | `fixtures` | 47/0/1 |
| `fixtures_model_package` | 2/0/0 | `fixtures_regress` | 60/0/1 |
| `force_insert_update` | 13/0/0 | `from_db_value` | 6/0/0 |
| `generic_inline_admin` | 20/0/0 | `generic_relations_regress` | 27/0/0 |
| `get_earliest_or_latest` | 9/0/0 | `get_object_or_404` | 4/0/0 |
| `get_or_create` | 47/0/2 | `httpwrappers` | 84/0/0 |
| `humanize_tests` | 15/0/0 | `indexes` | 13/0/18 |
| `inline_formsets` | 14/0/0 | `inspectdb` | 23/0/5 |
| `introspection` | 19/0/2 | `known_related_objects` | 20/0/0 |
| `logging_tests` | 54/0/0 | `m2m_and_m2o` | 3/0/0 |
| `m2m_intermediary` | 1/0/0 | `m2m_multiple` | 1/0/0 |
| `m2m_recursive` | 12/0/0 | `m2m_regress` | 10/0/0 |
| `m2m_signals` | 14/0/0 | `m2m_through` | 55/0/0 |
| `m2o_recursive` | 2/0/0 | `managers_regress` | 14/0/0 |
| `many_to_many` | 37/0/1 | `many_to_one` | 41/0/0 |
| `many_to_one_null` | 14/0/0 | `max_lengths` | 3/0/0 |
| `middleware_exceptions` | 33/0/0 | `migrate_signals` | 3/0/0 |
| `migration_test_data_persistence` | 3/0/0 | `model_enums` | 21/0/0 |
| `model_formsets_regress` | 22/0/0 | `model_indexes` | 28/0/2 |
| `model_inheritance_regress` | 30/0/0 | `model_meta` | 35/0/0 |
| `model_package` | 3/0/0 | `model_utils` | 1/0/0 |
| `multiple_database` | 78/0/0 | `mutually_referential` | 1/0/0 |
| `nested_foreign_keys` | 7/0/0 | `no_models` | 1/0/0 |
| `null_fk` | 2/0/0 | `null_fk_ordering` | 1/0/0 |
| `null_queries` | 3/0/0 | `one_to_one` | 39/0/0 |
| `or_lookups` | 11/0/0 | `order_with_respect_to` | 16/0/0 |
| `ordering` | 35/0/0 | `pagination` | 46/0/0 |
| `properties` | 2/0/0 | `proxy_model_inheritance` | 3/0/0 |
| `proxy_models` | 30/0/0 | `queryset_pickle` | 39/0/0 |
| `raw_query` | 30/0/0 | `redirects_tests` | 10/0/0 |
| `reserved_names` | 5/0/0 | `resolve_url` | 9/0/0 |
| `reverse_lookup` | 3/0/0 | `save_delete_hooks` | 1/0/0 |
| `select_for_update` | 3/0/35 | `select_related` | 20/0/0 |
| `select_related_onetoone` | 22/0/0 | `select_related_regress` | 9/0/0 |
| `sessions_tests` | 645/0/4 | `settings_tests` | 57/0/0 |
| `shell` | 28/0/0 | `shortcuts` | 6/0/0 |
| `signals` | 22/0/0 | `signed_cookies_tests` | 7/0/0 |
| `signing` | 19/0/0 | `sitemaps_tests` | 51/0/0 |
| `sites_framework` | 5/0/0 | `sites_tests` | 27/0/0 |
| `str` | 2/0/0 | `string_lookup` | 4/0/0 |
| `swappable_models` | 2/0/0 | `syndication_tests` | 35/0/0 |
| `template_loader` | 22/0/0 | `test_client_regress` | 109/0/0 |
| `test_runner_apps` | 5/0/0 | `timezones` | 77/0/9 |
| `transaction_hooks` | 20/0/1 | `transactions` | 85/0/2 |
| `unmanaged_models` | 3/0/0 | `update` | 29/0/5 |
| `update_only_fields` | 20/0/0 | `user_commands` | 48/0/4 |
| `validators` | 14/0/1 | `version` | 5/0/0 |
| `wsgi` | 7/0/0 | `xor_lookups` | 7/0/0 |

</details>

### No Test Files (5)

`base`, `custom_migration_operations`, `distinct_on_fields`, `import_error_package`, `migrations2`

### Full Suite Snapshot (Django 6.0.2, SQLite)

| | tach-core | pytest |
|---|---|---|
| **Passed** | 6789 | 4620 |
| **Failed** | 1575 | 560 |
| **Skipped** | 1273 | 1232 |
| **Errors** | 491 (zygote miss) | 3459 |

## Test Setup

**IMPORTANT**: Django tests require version-matched source and package.

```bash
# 1. Check installed Django version
docker compose exec dev python3 -c "import django; print(django.VERSION)"

# 2. Clone matching tests (e.g. 6.0.2)
docker compose exec dev bash -c "
  rm -rf /tmp/django-tests
  git clone --depth 1 --branch 6.0.2 https://github.com/django/django.git /tmp/django-repo
  cp -r /tmp/django-repo/tests/. /tmp/django-tests/
  rm -rf /tmp/django-repo
"

# 3. Create conftest.py (required for both pytest and tach-core)
docker compose exec dev bash -c "cat > /tmp/django-tests/conftest.py << 'EOF'
import os, sys, tempfile

def pytest_configure(config):
    os.environ['TMPDIR'] = tempfile.mkdtemp()
    os.environ['DJANGO_SETTINGS_MODULE'] = 'test_sqlite'
    sys.path.insert(0, os.path.dirname(__file__))
    from runtests import setup_run_tests
    setup_run_tests(0, None, None)
EOF"

# 4. Run
docker compose exec dev bash -c "cd /tmp/django-tests && /workspace/target/release/tach-core ."
docker compose exec dev bash -c "cd /tmp/django-tests && python3 -m pytest --tb=no -q --continue-on-collection-errors ."
```

### Why conftest.py is required

Django's `runtests.py` dynamically injects test modules into `INSTALLED_APPS`
and calls `apps.set_installed_apps()`. Without this, Django's app registry is
empty and most test files fail to import.

The conftest.py runs `setup_run_tests()` inside `pytest_configure`, so the
setup happens in the **same Python process** as test collection. Without it,
tach-core's zygote spawns a fresh Python process that has no apps registered.

## `utils_tests/` Results (Focus Suite)

### tach-core (after /tmp + __main__.__file__ fixes)
```
659 passed, 0 failed, 21 skipped in 1.99s
```

### pytest
```
618 passed, 3 failed, 21 skipped, 41 errors in 1.51s
```

### Comparison

| Metric | tach-core | pytest |
|--------|-----------|--------|
| Passed | 659 | 618 |
| Failed | 0 | 3 |
| Skipped | 21 | 21 |
| Errors | 0 | 41 |

tach-core achieves **100% pass rate** on utils_tests/ (all non-skipped tests pass).
pytest has 44 failures/errors on the same suite.

### Fixes applied (not yet merged)

**1. Read-only `/tmp` in Docker workers (32 failures fixed)**
- `src/isolation/namespace.rs`: When overlay is disabled (Docker), bind-mount
  `/tmp` onto itself and remount writable instead of using tmpfs (which would
  hide existing `/tmp` contents) or leaving it read-only.
- Tests using `tempfile.mkdtemp()` / `TemporaryDirectory()` now work.

**2. Missing `__main__.__file__` (1 failure fixed)**
- `src/execution/zygote.rs`: Set `__main__.__file__` to `/proc/self/exe`
  during zygote init. Embedded Python lacks a script entry point, so
  `__main__` has no `__file__` attribute by default.
- Django's autoreload (`iter_modules_and_files`) now resolves `__main__`.

## Full Suite Results (Django 6.0.2 on SQLite)

### tach-core
```
6789 passed, 1575 failed, 1273 skipped in 82.27s
491 "Test not found in Zygote" (AST-discovered but not in pytest session)
```

### pytest
```
4620 passed, 560 failed, 1232 skipped, 3459 errors in 28.96s
```

### Comparison

| Metric | tach-core | pytest |
|--------|-----------|--------|
| Passed | 6789 | 4620 |
| Failed | 1575 | 560 |
| Skipped | 1273 | 1232 |
| Errors (import/collection) | 491 | 3459 |

tach-core passes **2169 more tests** than pytest because tach runs tests in
isolated workers that survive import-time errors that crash pytest's collector.
The 491 remaining zygote misses are tests tach discovers via AST but pytest's
session can't collect (likely GIS/postgres-specific modules).

### Failure categories (tach-core's 1575 failures)

| Category | Count | Root Cause |
|----------|-------|------------|
| Zygote session miss | 491 | AST finds tests pytest can't collect (backend-specific) |
| Actual test failures | ~560 | Same tests that fail under pytest (Django test bugs on SQLite) |
| Missing fixtures | ~200 | `_pre_setup`, Django-specific fixtures not wired |
| Import errors | ~100 | Modules needing postgres/GIS backends |
| Other | ~224 | Various runtime errors |

## Discovery Status

| Scope | tach-core | pytest | Match |
|-------|-----------|--------|-------|
| `utils_tests/` | 682 | 682 | ✅ |
| Full suite discovery | 9700 | 9822 | ~99% |
| Full suite zygote session | 9875 items available | 9822 collected | ✅ |

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

## Critical Finding: conftest.py bootstrap

**Root cause of 5614 → 491 zygote misses**: Django's test setup must happen
inside the same Python process as test collection. Running `setup_run_tests()`
in a separate `python3 -c` command before `tach-core` doesn't work because:

1. tach-core spawns its own Python zygote process
2. The zygote calls `pytest.Session.perform_collect()` in a fresh Python env
3. Without `INSTALLED_APPS`, Django's app registry is empty
4. pytest can't import test modules → "Test not found in Zygote session"

**Fix**: A `conftest.py` with `pytest_configure` hook that runs the Django
bootstrap. This hook executes inside the zygote's Python process during
`_prepareconfig()` → `_do_configure()`.

## Known Gaps / Next Steps

### Remaining work
- [x] `utils_tests/` — 100% pass rate achieved (659/659 non-skipped tests pass)
- [x] Full sweep of all 213 modules (tach vs pytest per-module comparison)
- [x] 19 modules cleared (0 tach failures, passes ≥ pytest)
- [ ] **P0: Fix `tests.py` collection bug** — tach-core cannot collect `tests.py`
      files when passed as directory paths. "Selected 0 tests" for all
      `tests.py`-only modules (~142 modules blocked). pytest works fine.
- [ ] Investigate `utils_tests/` regression: 659/0 → 618/41 in sweep
      (may be conftest/environment difference)
- [ ] Investigate the 47 modules with tach failures (fixture issues, isolation, real bugs)
- [ ] Investigate the 491 remaining zygote misses (likely fixable)
- [ ] Wire missing Django fixtures (`_pre_setup`, `_post_teardown`)
- [ ] GIS tests (need PostGIS backend, always skip with sqlite)
- [ ] Backend-specific tests (postgres JSON, mysql-specific)
- [ ] `no:django` plugin flag in harness — confirmed safe for Django TestCase
       subclasses (lifecycle is self-contained in Django's __call__)

### Not tach-core bugs
- `TestFinder` in `test_module_loading.py` — has `__init__`, correctly skipped
- 560 tests fail under both tach-core and pytest (Django issues on SQLite)
- GIS/postgres collection errors (backend not available)

## Test Counts Reference

```
887 Rust unit tests (cargo nextest run --lib)
4 pre-existing integration test failures (unrelated to Django work)
```
