"""Test yield fixture teardown behavior with Tach isolation."""
import pytest
import os
import tempfile

@pytest.fixture
def temp_file_fixture():
    """Yield fixture that creates and cleans up a temp file."""
    fd, path = tempfile.mkstemp(prefix="tach_test_")
    os.write(fd, b"test content")
    os.close(fd)
    yield path
    # Teardown: remove the file
    if os.path.exists(path):
        os.unlink(path)

def test_yield_fixture_setup(temp_file_fixture):
    """Verify yield fixture setup runs correctly."""
    assert os.path.exists(temp_file_fixture)
    with open(temp_file_fixture) as f:
        assert f.read() == "test content"

def test_yield_fixture_independent(temp_file_fixture):
    """Each test gets its own fixture instance."""
    assert os.path.exists(temp_file_fixture)
    # Write something different
    with open(temp_file_fixture, "w") as f:
        f.write("modified")
