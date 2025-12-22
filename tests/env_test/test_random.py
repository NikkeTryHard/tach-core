"""Randomness Divergence Tests

Verify that entropy injection works correctly - each worker should get
unique random sequences, not identical ones inherited from the Zygote.

Run these tests in parallel and check that RANDOM_1 and RANDOM_2
print DIFFERENT values.
"""

import random


def test_random_1():
    """Log a random number to prove entropy injection works."""
    val = random.randint(0, 1_000_000)
    print(f"RANDOM_1: {val}")
    assert True  # Always passes, we check output manually


def test_random_2():
    """Log a random number - should differ from test_random_1."""
    val = random.randint(0, 1_000_000)
    print(f"RANDOM_2: {val}")
    assert True


def test_random_3():
    """Third random test for additional verification."""
    val = random.randint(0, 1_000_000)
    print(f"RANDOM_3: {val}")
    assert True
