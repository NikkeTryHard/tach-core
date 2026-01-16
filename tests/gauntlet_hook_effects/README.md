# gauntlet_hook_effects

Test suite for hook effect recording and replay functionality.

## Purpose

This test directory validates the hook effect system that enables Tach to:

1. Record effects from pytest hooks (pytest_configure, pytest_sessionstart, etc.)
2. Transmit effects via IPC from Zygote to Supervisor
3. Replay effects in worker processes for consistent test execution

## Test Files

| File                       | Description                                                                     |
| -------------------------- | ------------------------------------------------------------------------------- |
| `conftest.py`              | Shared fixtures for hook effect testing                                         |
| `test_hook_effects.py`     | Core hook effect recording and replay tests                                     |
| `test_effect_functions.py` | Tests for individual effect types (SetEnv, ModifySysPath, RegisterMarker, etc.) |

## Running Tests

```bash
# Run all hook effect tests
pytest tests/gauntlet_hook_effects/ -v

# Run specific test file
pytest tests/gauntlet_hook_effects/test_hook_effects.py -v

# Run with tach-core binary
./target/release/tach-core test tests/gauntlet_hook_effects/
```

## Features Covered

- **Effect Recording**: Capturing side effects from hook execution
- **Effect Serialization**: Bincode serialization for IPC transmission
- **Effect Replay**: Applying recorded effects in worker processes
- **HookEffect Variants**:
  - `SetEnv` - Environment variable modifications
  - `ModifySysPath` - Python sys.path changes (Prepend/Append/Remove)
  - `RegisterMarker` - Custom pytest marker registration
  - `ModifyItems` - Test collection modifications
  - `NoEffect` - Hooks with no observable side effects
