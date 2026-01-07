# Parametrized Example

This example demonstrates `@pytest.mark.parametrize` patterns with tach-core.

## What This Example Shows

- **test_params.py**: Various parametrization techniques
  - Single parameter parametrization
  - Multiple parameters per test
  - Different data types (int, float, str, list, dict, tuple, bool)
  - String and list operations with parametrization
  - Named parameter sets with `pytest.param(id=...)`
  - Boolean condition testing
  - Nested parametrization (cartesian product)
  - Expected exceptions with parametrization
  - Dictionary inputs as parameters
  - Class instance parameters

## Running the Example

From the tach-core root directory:

```bash
# Run all parametrized tests
./target/debug/tach-core examples/parametrized/tests/

# Run with verbose output to see all parameter combinations
./target/debug/tach-core -v examples/parametrized/tests/

# Run specific test by keyword
./target/debug/tach-core -k "addition" examples/parametrized/tests/

# Run tests with specific parameter IDs
./target/debug/tach-core -k "five" examples/parametrized/tests/
```

## Notes

- Parametrized tests generate multiple test cases from a single test function
- Each parameter combination runs as a separate test
- Nested `@pytest.mark.parametrize` decorators create cartesian products
- Use `pytest.param(id=...)` for readable test names in output

## Expected Output

All tests should pass. With verbose output, you will see each parameter
combination as a separate test run.
