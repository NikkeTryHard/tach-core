"""Test SQLAlchemy protocol constants are defined."""


def test_sqlalchemy_effect_type_defined():
    """Verify EFFECT_TYPE_SQLALCHEMY_DB_SETUP constant exists."""
    from tach_harness import EFFECT_TYPE_SQLALCHEMY_DB_SETUP

    assert EFFECT_TYPE_SQLALCHEMY_DB_SETUP == "SqlAlchemyDbSetup"
