"""Verification test: Fixture from conftest.py should work."""


def test_fixture_works(my_fixture):
    """Should PASS, proving fixture inheritance from Zygote to Worker works."""
    assert my_fixture == 1
