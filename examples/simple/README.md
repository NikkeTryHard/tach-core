# Simple Example

This example demonstrates basic test patterns with tach-core.

## What This Example Shows

- **test_basic.py**: Fundamental assertion patterns
  - Pass/fail tests
  - Arithmetic operations
  - String and list manipulation
  - Exception handling

- **test_fixtures.py**: Fixture patterns
  - Simple fixtures with return values
  - Yield fixtures with cleanup
  - Fixture composition (fixtures using other fixtures)
  - Multiple fixtures in one test

- **conftest.py**: Shared fixtures
  - Fixtures available to all test modules
  - Setup/teardown patterns

## Running the Example

From the tach-core root directory:

```bash
# Run all tests in this example
./target/debug/tach-core examples/simple/tests/

# Run with verbose output
./target/debug/tach-core -v examples/simple/tests/

# Run specific file
./target/debug/tach-core examples/simple/tests/test_basic.py

# Run specific test by keyword
./target/debug/tach-core -k "arithmetic" examples/simple/tests/
```

## Expected Output

All tests should pass. This example contains no intentionally failing tests.
