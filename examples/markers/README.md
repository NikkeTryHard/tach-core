# Markers Example

This example demonstrates pytest markers and `-m` filtering with tach-core.

## What This Example Shows

- **test_markers.py**: Various marker patterns
  - Custom markers (`@pytest.mark.slow`, `@pytest.mark.fast`, etc.)
  - Built-in markers (`@pytest.mark.skip`, `@pytest.mark.skipif`, `@pytest.mark.xfail`)
  - Multiple markers on a single test
  - Marker combinations for filtering

- **conftest.py**: Marker registration
  - Registering custom markers to avoid warnings

## Available Custom Markers

| Marker        | Description                     |
| ------------- | ------------------------------- |
| `slow`        | Tests that take a long time     |
| `fast`        | Quick tests                     |
| `integration` | Integration tests               |
| `unit`        | Unit tests                      |
| `smoke`       | Basic sanity checks             |
| `network`     | Tests requiring network access  |
| `database`    | Tests requiring database access |

## Running the Example

From the tach-core root directory:

```bash
# Run all tests
./target/debug/tach-core examples/markers/tests/

# Run only fast tests
./target/debug/tach-core -m "fast" examples/markers/tests/

# Run only slow tests
./target/debug/tach-core -m "slow" examples/markers/tests/

# Skip slow tests
./target/debug/tach-core -m "not slow" examples/markers/tests/

# Run smoke tests
./target/debug/tach-core -m "smoke" examples/markers/tests/

# Complex expressions
./target/debug/tach-core -m "slow and integration" examples/markers/tests/
./target/debug/tach-core -m "not (slow or database)" examples/markers/tests/
./target/debug/tach-core -m "smoke and fast" examples/markers/tests/
```

## Marker Expression Syntax

| Expression                    | Meaning                       |
| ----------------------------- | ----------------------------- |
| `-m "slow"`                   | Run tests marked with slow    |
| `-m "not slow"`               | Skip tests marked with slow   |
| `-m "slow and integration"`   | Run tests with both markers   |
| `-m "slow or fast"`           | Run tests with either marker  |
| `-m "not (slow or database)"` | Skip tests with either marker |

## Notes

- Tests without markers run by default
- Use `conftest.py` to register markers and avoid warnings
- Multiple markers can be combined with `and`, `or`, `not`, and parentheses
- Built-in markers like `skip`, `skipif`, and `xfail` work as expected

## Expected Output

Most tests should pass. Some tests are explicitly:

- **Skipped**: `test_skipped`, `test_conditional_skip`
- **Expected to fail**: `test_expected_failure`
- **Expected to pass but marked xfail**: `test_xfail_strict` (XPASS)
