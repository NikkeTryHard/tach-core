# Conftest Nested Inheritance Example

This example demonstrates how pytest's conftest.py files work with
nested directories and fixture inheritance in tach-core.

## What This Example Shows

The directory structure demonstrates three levels of conftest.py:

```
tests/
  conftest.py           # Root level - fixtures available everywhere
  outer/
    conftest.py         # Outer level - fixtures available here and below
    inner/
      conftest.py       # Inner level - fixtures available only here
      test_nested.py    # Tests using fixtures from all levels
```

### Fixture Inheritance

- **Root fixtures** (tests/conftest.py):
  - `root_fixture` - Available to all tests
  - `shared_config` - Shared configuration (can be overridden)
  - `connection_pool` - Simulated shared resource

- **Outer fixtures** (tests/outer/conftest.py):
  - `outer_fixture` - Available to outer and inner tests
  - `outer_resource` - Resource specific to outer level
  - `combined_fixture` - Combines root and outer fixtures

- **Inner fixtures** (tests/outer/inner/conftest.py):
  - `inner_fixture` - Available only to inner tests
  - `inner_resource` - Resource specific to inner level
  - `full_hierarchy` - Combines fixtures from all three levels
  - `shared_config` - Overrides the root version

## Running the Example

From the tach-core root directory:

```bash
# Run all nested conftest tests
./target/debug/tach-core examples/conftest/tests/

# Run with verbose output
./target/debug/tach-core -v examples/conftest/tests/

# Run specific test
./target/debug/tach-core -k "hierarchy" examples/conftest/tests/
```

## Key Concepts Demonstrated

1. **Fixture Inheritance**: Inner tests can use fixtures from all parent conftest.py files
2. **Fixture Override**: Inner conftest.py can override parent fixtures (see `shared_config`)
3. **Fixture Composition**: Fixtures can depend on fixtures from parent conftest.py files
4. **Scope Isolation**: Inner fixtures are not visible to outer tests

## Expected Output

All tests should pass. The tests verify that:

- Fixtures from all levels are accessible
- Fixture overriding works correctly
- Fixture composition across levels works
- Resource cleanup (yield fixtures) works properly
