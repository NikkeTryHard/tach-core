# Plugin Compatibility Matrix

> Tach 0.2.5 Plugin Stabilization Documentation

This document tracks pytest plugin compatibility with Tach's hypervisor-accelerated test execution model.

## Support Tiers

| Tier | Description |
|------|-------------|
| **Full** | Plugin works without modifications |
| **Partial** | Plugin works with known limitations |
| **Superseded** | Tach provides native equivalent functionality |
| **Incompatible** | Plugin conflicts with Tach's execution model |
| **Unknown** | Not yet tested; may or may not work |

## Plugin Compatibility Matrix

### Fully Supported Plugins

| Plugin | Version | Isolation Mode | Description |
|--------|---------|----------------|-------------|
| pytest-django | >= 4.0 | All | Full support: session fixtures, DB isolation, setup_test_environment |
| pytest-asyncio | >= 0.21 | All | Full support: event loop policy, auto mode, async fixtures |
| pytest-mock | >= 3.0 | All | Mocking fixtures work normally |
| pytest-env | >= 0.8 | All | Environment variables captured via effect recording |
| pytest-randomly | >= 3.0 | All | Test randomization works normally |
| pytest-trio | >= 0.7 | All | Trio event loop support via plugin integration |

> **Note:** Framework plugins (django, asyncio, trio) now load natively instead of
> being disabled. Their session-level setup runs in the zygote, and conflicting
> per-test hooks are neutralized. See PR #104 for details.

### Partially Supported Plugins

| Plugin | Version | Isolation Mode | Description | Limitations |
|--------|---------|----------------|-------------|-------------|
| pytest-timeout | >= 2.0 | All | Timeout support | Use Tach's native `--timeout` flag for better integration |

### Superseded Plugins

These plugins provide functionality that Tach handles natively. Using them is unnecessary and may cause conflicts.

| Plugin | Tach Equivalent | Notes |
|--------|-----------------|-------|
| pytest-xdist | `tach -n <workers>` | Tach's native worker pool provides parallelism |
| pytest-forked | Tach zygote model | Tach uses fork/zygote execution by default |
| pytest-parallel | `tach -n <workers>` | Tach's native parallelism is faster |
| pytest-cov | `tach --coverage` | Tach uses PEP 669 for low-overhead coverage |

### Incompatible Plugins

| Plugin | Reason | Workaround |
|--------|--------|------------|
| pytest-sugar | Terminal manipulation conflicts with Tach's progress display | Use Tach's native progress output |

## Marker Support

Tach recognizes and handles the following pytest markers:

| Marker | Plugin | Support Level | Notes |
|--------|--------|---------------|-------|
| `@pytest.mark.django_db` | pytest-django | Full | Database transactions handled per-test |
| `@pytest.mark.urls` | pytest-django | Full | URL configuration respected |
| `@pytest.mark.asyncio` | pytest-asyncio | Full | Async tests detected and run with event loop |
| `@pytest.mark.timeout` | pytest-timeout | Partial | Prefer `--timeout` CLI flag |

## External Project Testing

Status of validation against real-world projects using these plugins:

| Project | Plugins Used | Status | Notes |
|---------|--------------|--------|-------|
| Django REST Framework | pytest-django | Pending | - |
| FastAPI | pytest-asyncio | Pending | - |
| Requests | pytest-mock | Pending | - |
| HTTPx | pytest-asyncio, pytest-mock | Pending | - |

## Known Limitations

### Isolation Mode Considerations

1. **Fork mode** (default): All supported plugins work correctly
2. **Spawn mode**: Plugin state must be serializable
3. **Thread mode**: Plugins using thread-local state may have issues

### Plugin Detection

Tach automatically detects installed plugins via:
- `pytest --co` collection output
- `conftest.py` plugin registrations
- Package metadata inspection

### Priority and Ordering

Plugin hook execution order can be configured via the `PluginRegistry::set_priority()` API when custom ordering is required.

## Reporting Issues

If you encounter plugin compatibility issues:

1. Check if the plugin is in the Unknown tier
2. Run with `TACH_DEBUG=1` for detailed hook tracing
3. File an issue with plugin name, version, and reproduction steps

## Version History

| Version | Changes |
|---------|---------|
| 0.2.5 | Initial plugin compatibility matrix |
