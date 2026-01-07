# Async Example

This example demonstrates async test patterns with tach-core.

## What This Example Shows

- **test_async.py**: Basic async test patterns
  - `async def` test functions
  - Awaiting coroutines
  - Testing async exceptions
  - Async timeout handling

- **test_gather.py**: Concurrent async patterns
  - `asyncio.gather` for parallel execution
  - `asyncio.create_task` for task management
  - `asyncio.as_completed` for processing results as they arrive
  - `asyncio.Semaphore` for limiting concurrency
  - Error handling with `return_exceptions=True`

## Requirements

These tests require pytest-asyncio for the `@pytest.mark.asyncio` marker:

```bash
pip install pytest-asyncio
```

## Running the Example

From the tach-core root directory:

```bash
# Run all async tests
./target/debug/tach-core examples/async/tests/

# Run with verbose output
./target/debug/tach-core -v examples/async/tests/

# Run specific test
./target/debug/tach-core -k "gather" examples/async/tests/
```

## Notes

- Tach-core has built-in asyncio loop management for coroutine tests
- Each async test runs in its own event loop
- Async fixtures are also supported (not shown in this example)

## Expected Output

All tests should pass. The tests use short delays (0.001s) to keep execution fast.
