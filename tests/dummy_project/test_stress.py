"""Stress test: prints 10MB of data to verify anti-deadlock."""


def test_big_output():
    """This test prints 10MB of data - must not hang the runner."""
    # Print 10MB of data (10 lines of 1MB each)
    for i in range(10):
        print("X" * (1024 * 1024))  # 1MB per line
    assert True
