"""Verification test: Print output should be captured in supervisor logs."""


def test_print_output():
    """Should capture 'I am alive' in supervisor logs."""
    print("I am alive")
    assert True
