"""Test basic mocker fixture functionality."""
import pytest


class Calculator:
    """Simple class for mocking tests."""

    def add(self, a: int, b: int) -> int:
        return a + b

    def multiply(self, a: int, b: int) -> int:
        return a * b


def fetch_data():
    """Function that would normally do I/O."""
    raise RuntimeError("Should not be called in tests")


class TestMockerPatch:
    """Test mocker.patch functionality."""

    def test_patch_function(self, mocker):
        """Test patching a module-level function."""
        mock_fetch = mocker.patch(
            "tests.gauntlet_mock.test_mocker_basic.fetch_data",
            return_value={"data": "mocked"},
        )

        result = fetch_data()

        assert result == {"data": "mocked"}
        mock_fetch.assert_called_once()

    def test_patch_object_method(self, mocker):
        """Test patching an object method."""
        calc = Calculator()
        mocker.patch.object(calc, "add", return_value=42)

        result = calc.add(1, 2)

        assert result == 42

    def test_patch_dict(self, mocker):
        """Test patching a dictionary."""
        config = {"debug": False, "timeout": 30}
        mocker.patch.dict(config, {"debug": True})

        assert config["debug"] is True
        assert config["timeout"] == 30


class TestMockerSpy:
    """Test mocker.spy functionality."""

    def test_spy_tracks_calls(self, mocker):
        """Test that spy tracks calls while preserving behavior."""
        calc = Calculator()
        spy = mocker.spy(calc, "add")

        result = calc.add(2, 3)

        assert result == 5  # Original behavior preserved
        spy.assert_called_once_with(2, 3)

    def test_spy_call_count(self, mocker):
        """Test spy call counting."""
        calc = Calculator()
        spy = mocker.spy(calc, "multiply")

        calc.multiply(2, 3)
        calc.multiply(4, 5)

        assert spy.call_count == 2


class TestMockerStub:
    """Test mocker.stub functionality."""

    def test_stub_creation(self, mocker):
        """Test creating a stub."""
        stub = mocker.stub(name="my_stub")
        stub.return_value = "stubbed"

        result = stub()

        assert result == "stubbed"
        stub.assert_called_once()


class TestMockerCleanup:
    """Test that mocks are cleaned up between tests."""

    def test_first_no_mock(self):
        """First test: verify fetch_data raises."""
        with pytest.raises(RuntimeError, match="Should not be called"):
            fetch_data()

    def test_second_with_mock(self, mocker):
        """Second test: mock fetch_data."""
        mocker.patch(
            "tests.gauntlet_mock.test_mocker_basic.fetch_data",
            return_value="mocked",
        )
        assert fetch_data() == "mocked"

    def test_third_no_mock(self):
        """Third test: verify mock was cleaned up."""
        with pytest.raises(RuntimeError, match="Should not be called"):
            fetch_data()
