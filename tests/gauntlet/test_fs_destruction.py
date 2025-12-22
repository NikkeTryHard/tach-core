"""Test A: Destructive Worker - Prove Iron Dome protects host.

With Iron Dome:
- / is mounted Read-Only
- /tmp is writable overlay
- Project root is writable overlay
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
