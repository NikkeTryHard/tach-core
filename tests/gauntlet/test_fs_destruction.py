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
    """Try to escape via symlink to read protected files."""
    import tempfile

    # Create symlink pointing to /etc/shadow
    symlink_path = "/tmp/escape_symlink"
    try:
        if os.path.exists(symlink_path):
            os.remove(symlink_path)
        os.symlink("/etc/shadow", symlink_path)
        print("Created symlink to /etc/shadow")
    except OSError as e:
        print(f"Could not create symlink: {e}")
        return  # Skip test if symlink creation fails

    # Try to read via symlink - should fail with permission denied
    try:
        with open(symlink_path, "r") as f:
            content = f.read()
        # If we get here, we read /etc/shadow - BAD!
        print("ERROR: Read /etc/shadow via symlink escape!")
        assert False, "Symlink escape succeeded - security vulnerability!"
    except PermissionError:
        print("GOOD: /etc/shadow protected via symlink")
    except OSError as e:
        print(f"GOOD: Symlink read blocked: {e}")
    finally:
        try:
            os.remove(symlink_path)
        except OSError:
            pass


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
