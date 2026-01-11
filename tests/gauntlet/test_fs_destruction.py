"""Test A: Destructive Worker - Prove Iron Dome protects host.

With Iron Dome:
- / is mounted Read-Only
- /tmp is writable overlay
- Project root is writable overlay
- Symlink escapes are blocked
- Proc filesystem is protected
"""

import os


def test_fs_destruction():
    """Attempt hostile operations - should be blocked by Iron Dome."""
    errors = []

    # 1. Try to write to /etc/passwd - MUST FAIL (RO filesystem)
    try:
        with open("/etc/passwd", "a") as f:
            f.write("\n# TACH_TEST_MARKER\n")
        errors.append("ERROR: /etc/passwd was writable!")
    except OSError as e:
        # Expected: "Read-only file system" or "Permission denied"
        print(f"GOOD: /etc/passwd protected: {e}")

    # 2. Write to /tmp - MUST SUCCEED (overlay)
    try:
        with open("/tmp/survivor.txt", "w") as f:
            f.write("I survived the gauntlet!")
        print("GOOD: /tmp is writable")
    except OSError as e:
        errors.append(f"ERROR: /tmp not writable: {e}")

    # 3. Write to CWD (project root) - MUST SUCCEED (overlay)
    try:
        with open("test_output.txt", "w") as f:
            f.write("CWD is writable!")
        print("GOOD: CWD is writable")
        os.remove("test_output.txt")
    except OSError as e:
        errors.append(f"ERROR: CWD not writable: {e}")

    # Report all errors
    if errors:
        for err in errors:
            print(err)
        assert False, "\n".join(errors)

    assert True


def test_symlink_escape_prevention():
    """Symlinks should not allow escaping sandbox.

    Target a path NOT in Landlock allow-list.
    /root is not allowed, /etc is (for Python compatibility).
    """
    import pytest
    import tempfile

    # Target a path NOT in Landlock allow-list
    # /root is not allowed, /etc is (for Python compatibility)
    target = "/root/.bashrc"

    # Skip if target doesn't exist (non-root environments)
    if not os.path.exists(target):
        pytest.skip("Target path doesn't exist")

    # Generate unique symlink path without creating unnecessary file
    symlink_path = tempfile.mktemp(suffix="_symlink")

    try:
        os.symlink(target, symlink_path)

        # This should fail - sandbox should block reading outside allowed paths
        with pytest.raises((PermissionError, OSError)):
            with open(symlink_path, 'r') as f:
                f.read()
    finally:
        if os.path.islink(symlink_path):
            os.unlink(symlink_path)


def test_proc_self_protection():
    """Verify /proc/self is accessible but not abusable."""
    # Reading own pid should work
    pid = os.getpid()
    print(f"Own PID: {pid}")
    assert pid > 0

    # Reading /proc/self/cmdline should work
    try:
        with open("/proc/self/cmdline", "rb") as f:
            cmdline = f.read()
        print(f"Cmdline: {cmdline[:50]}...")
        assert len(cmdline) > 0
    except OSError as e:
        print(f"Could not read cmdline: {e}")

    # Writing to /proc should fail
    try:
        with open("/proc/sys/kernel/hostname", "w") as f:
            f.write("hacked")
        assert False, "ERROR: Was able to write to /proc!"
    except OSError as e:
        print(f"GOOD: /proc write protected: {e}")


def test_etc_readonly():
    """Verify /etc is read-only."""
    test_file = "/etc/tach_test_marker.txt"
    try:
        with open(test_file, "w") as f:
            f.write("test")
        os.remove(test_file)
        assert False, "ERROR: /etc was writable!"
    except OSError as e:
        print(f"GOOD: /etc protected: {e}")


def test_usr_readonly():
    """Verify /usr is read-only."""
    test_file = "/usr/tach_test_marker.txt"
    try:
        with open(test_file, "w") as f:
            f.write("test")
        os.remove(test_file)
        assert False, "ERROR: /usr was writable!"
    except OSError as e:
        print(f"GOOD: /usr protected: {e}")
