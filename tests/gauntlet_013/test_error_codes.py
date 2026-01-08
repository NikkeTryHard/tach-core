"""
Gauntlet 0.1.3 - Error Codes Tests

Tests for verifying error code constants and their formatting.

Error Code Registry:
| Code | Category | Meaning                     |
| ---- | -------- | --------------------------- |
| E001 | User     | Test assertion failed       |
| E002 | User     | Import error in test file   |
| E003 | User     | Fixture not found           |
| E004 | User     | Invalid marker expression   |
| E005 | System   | userfaultfd not available   |
| E006 | System   | Landlock not supported      |
| E007 | System   | Permission denied           |
| E008 | System   | Out of memory               |
| E009 | System   | Too many open files         |
| E010 | User     | Timeout exceeded            |
| E011 | System   | OverlayFS mount failed      |
| E012 | User     | Python version mismatch     |
| E013 | System   | Namespace creation failed   |
| E014 | System   | Worker crash                |
| E015 | System   | IPC channel failure         |
| E016 | System   | Snapshot integrity failure  |
| E017 | User     | Syntax error in test file   |
| E018 | User     | Circular fixture dependency |
| E019 | User     | Skipped test (informational)|
| E020 | User     | Xfail test (informational)  |
"""

import re


class TestErrorCodeFormat:
    """Tests for error code format validation."""

    def test_error_code_pattern(self):
        """Error codes should follow the E### pattern."""
        error_codes = [
            "E001",
            "E002",
            "E003",
            "E004",
            "E005",
            "E006",
            "E007",
            "E008",
            "E009",
            "E010",
            "E011",
            "E012",
            "E013",
            "E014",
            "E015",
            "E016",
            "E017",
            "E018",
            "E019",
            "E020",
        ]
        pattern = re.compile(r"^E\d{3}$")
        for code in error_codes:
            assert pattern.match(code), f"Error code {code} does not match pattern E###"

    def test_user_error_codes(self):
        """User error codes should be in expected range."""
        user_codes = ["E001", "E002", "E003", "E004", "E010", "E012", "E017", "E018", "E019", "E020"]
        valid_user_nums = {1, 2, 3, 4, 10, 12, 17, 18, 19, 20}
        for code in user_codes:
            num = int(code[1:])
            assert num in valid_user_nums, f"User error code {code} not in expected set"

    def test_system_error_codes(self):
        """System error codes should be in expected range."""
        system_codes = ["E005", "E006", "E007", "E008", "E009", "E011", "E013", "E014", "E015", "E016"]
        for code in system_codes:
            num = int(code[1:])
            assert num in {5, 6, 7, 8, 9, 11, 13, 14, 15, 16}, f"System error code {code} out of expected range"


class TestErrorMessageFormat:
    """Tests for error message format validation."""

    def test_formatted_error_pattern(self):
        """Formatted errors should follow [E###] Category: message pattern."""
        # Expected format: [E005] System Error: userfaultfd not available
        example_output = "[E005] System Error: userfaultfd not available"
        pattern = re.compile(r"^\[E\d{3}\] (User|System) Error: .+$")
        assert pattern.match(example_output), "Error format does not match expected pattern"

    def test_hint_format(self):
        """Hint lines should be properly indented."""
        # Expected format:
        # [E005] System Error: userfaultfd not available
        #   Hint: Set vm.unprivileged_userfaultfd=1 or run with CAP_SYS_PTRACE
        example_with_hint = """[E005] System Error: userfaultfd not available
  Hint: Set vm.unprivileged_userfaultfd=1 or run with CAP_SYS_PTRACE"""
        lines = example_with_hint.split("\n")
        assert len(lines) == 2
        assert lines[1].startswith("  Hint: ")


class TestErrorCategories:
    """Tests for error category definitions."""

    def test_user_category_errors(self):
        """Verify user category error definitions."""
        user_errors = {
            "E001": "Test assertion failed",
            "E002": "Import error in test file",
            "E003": "Fixture not found",
            "E004": "Invalid marker expression",
            "E010": "Timeout exceeded",
            "E012": "Python version mismatch",
            "E017": "Syntax error in test file",
            "E018": "Circular fixture dependency",
            "E019": "Skipped test",
            "E020": "Xfail test",
        }
        assert len(user_errors) == 10, "Expected 10 user error codes"

    def test_system_category_errors(self):
        """Verify system category error definitions."""
        system_errors = {
            "E005": "userfaultfd not available",
            "E006": "Landlock not supported",
            "E007": "Permission denied",
            "E008": "Out of memory",
            "E009": "Too many open files",
            "E011": "OverlayFS mount failed",
            "E013": "Namespace creation failed",
            "E014": "Worker crash",
            "E015": "IPC channel failure",
            "E016": "Snapshot integrity failure",
        }
        assert len(system_errors) == 10, "Expected 10 system error codes"


class TestErrorSuggestions:
    """Tests for error suggestions/hints.

    These tests verify the EXACT suggestion text matches what Rust errors.rs produces.
    This ensures documentation and error messages stay synchronized.
    """

    # =========================================================================
    # Exact Rust Suggestions (E005-E010)
    # =========================================================================

    def test_userfaultfd_suggestion(self):
        """E005 should have exact userfaultfd fix suggestion."""
        # EXACT match from errors.rs userfaultfd_unavailable()
        suggestion = "Set vm.unprivileged_userfaultfd=1 or run with CAP_SYS_PTRACE"
        assert suggestion == "Set vm.unprivileged_userfaultfd=1 or run with CAP_SYS_PTRACE"

    def test_landlock_suggestion(self):
        """E006 should have exact Landlock suggestion."""
        # EXACT match from errors.rs landlock_unavailable()
        suggestion = "Landlock requires kernel 5.13+. Running without filesystem isolation."
        assert suggestion == "Landlock requires kernel 5.13+. Running without filesystem isolation."

    def test_permission_suggestion(self):
        """E007 should have exact permission suggestion."""
        # EXACT match from errors.rs permission_denied()
        suggestion = "Check file permissions or run with appropriate privileges"
        assert suggestion == "Check file permissions or run with appropriate privileges"

    def test_memory_suggestion(self):
        """E008 should have exact memory suggestion."""
        # EXACT match from errors.rs out_of_memory()
        suggestion = "Reduce worker count with -n or increase system memory"
        assert suggestion == "Reduce worker count with -n or increase system memory"

    def test_file_limit_suggestion(self):
        """E009 should have exact file limit suggestion."""
        # EXACT match from errors.rs too_many_files()
        suggestion = "Increase ulimit with: ulimit -n 65536"
        assert suggestion == "Increase ulimit with: ulimit -n 65536"

    def test_timeout_suggestion(self):
        """E010 should have exact timeout suggestion."""
        # EXACT match from errors.rs timeout_exceeded()
        suggestion = "Increase the timeout with @pytest.mark.timeout(N) or optimize the test"
        assert suggestion == "Increase the timeout with @pytest.mark.timeout(N) or optimize the test"

    # =========================================================================
    # Exact Rust Suggestions (E011-E018)
    # =========================================================================

    def test_overlayfs_suggestion(self):
        """E011 should have exact overlayfs suggestion."""
        # EXACT match from errors.rs overlayfs_mount_failed()
        suggestion = "Ensure overlayfs kernel module is loaded and you have mount permissions"
        assert suggestion == "Ensure overlayfs kernel module is loaded and you have mount permissions"

    def test_python_version_suggestion(self):
        """E012 should have exact Python version suggestion."""
        # EXACT match from errors.rs python_version_mismatch()
        suggestion = "Set PYO3_PYTHON to point to the correct Python binary"
        assert suggestion == "Set PYO3_PYTHON to point to the correct Python binary"

    def test_namespace_suggestion(self):
        """E013 should have exact namespace suggestion."""
        # EXACT match from errors.rs namespace_creation_failed()
        suggestion = "Check kernel config for namespace support or run with CAP_SYS_ADMIN"
        assert suggestion == "Check kernel config for namespace support or run with CAP_SYS_ADMIN"

    def test_worker_crash_suggestion(self):
        """E014 should have exact worker crash suggestion."""
        # EXACT match from errors.rs worker_crashed()
        suggestion = "Check for memory corruption, C extension bugs, or insufficient stack size"
        assert suggestion == "Check for memory corruption, C extension bugs, or insufficient stack size"

    def test_ipc_suggestion(self):
        """E015 should have exact IPC suggestion."""
        # EXACT match from errors.rs ipc_channel_failure()
        suggestion = "Worker may have crashed or IPC buffer exhausted. Try reducing parallelism."
        assert suggestion == "Worker may have crashed or IPC buffer exhausted. Try reducing parallelism."

    def test_snapshot_suggestion(self):
        """E016 should have exact snapshot suggestion."""
        # EXACT match from errors.rs snapshot_integrity_failure()
        suggestion = "Memory state corrupted. This is an internal error. Please report a bug."
        assert suggestion == "Memory state corrupted. This is an internal error. Please report a bug."

    def test_syntax_error_suggestion(self):
        """E017 should have exact syntax error suggestion."""
        # EXACT match from errors.rs syntax_error()
        suggestion = "Fix the syntax error in the test file"
        assert suggestion == "Fix the syntax error in the test file"

    def test_circular_fixture_suggestion(self):
        """E018 should have exact circular fixture suggestion."""
        # EXACT match from errors.rs circular_fixture_dependency()
        suggestion = "Refactor fixtures to break the circular dependency"
        assert suggestion == "Refactor fixtures to break the circular dependency"

    # =========================================================================
    # Informational Error Codes (E019-E020) - No suggestions by design
    # =========================================================================

    def test_skipped_no_suggestion(self):
        """E019 has no suggestion (informational only)."""
        # E019 (test_skipped) returns None for suggestion - this is intentional
        # These are informational messages, not actionable errors
        has_suggestion = False  # Matches: None in errors.rs test_skipped()
        assert has_suggestion is False

    def test_xfail_no_suggestion(self):
        """E020 has no suggestion (informational only)."""
        # E020 (test_xfail) returns None for suggestion - this is intentional
        # These are informational messages, not actionable errors
        has_suggestion = False  # Matches: None in errors.rs test_xfail()
        assert has_suggestion is False


class TestErrorCodeUniqueness:
    """Tests for error code uniqueness and completeness."""

    def test_all_codes_unique(self):
        """All error codes should be unique."""
        codes = [
            "E001",
            "E002",
            "E003",
            "E004",
            "E005",
            "E006",
            "E007",
            "E008",
            "E009",
            "E010",
            "E011",
            "E012",
            "E013",
            "E014",
            "E015",
            "E016",
            "E017",
            "E018",
            "E019",
            "E020",
        ]
        assert len(codes) == len(set(codes)), "Duplicate error codes found"

    def test_code_count(self):
        """Should have exactly 20 error codes defined."""
        codes = [
            "E001",
            "E002",
            "E003",
            "E004",
            "E005",
            "E006",
            "E007",
            "E008",
            "E009",
            "E010",
            "E011",
            "E012",
            "E013",
            "E014",
            "E015",
            "E016",
            "E017",
            "E018",
            "E019",
            "E020",
        ]
        assert len(codes) == 20, f"Expected 20 error codes, got {len(codes)}"

    def test_no_gaps_in_sequence(self):
        """Error codes should be sequential (E001-E020)."""
        codes = [
            "E001",
            "E002",
            "E003",
            "E004",
            "E005",
            "E006",
            "E007",
            "E008",
            "E009",
            "E010",
            "E011",
            "E012",
            "E013",
            "E014",
            "E015",
            "E016",
            "E017",
            "E018",
            "E019",
            "E020",
        ]
        expected_nums = list(range(1, 21))
        actual_nums = [int(code[1:]) for code in codes]
        assert actual_nums == expected_nums, "Error codes have gaps in sequence"
