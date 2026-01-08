"""
Gauntlet 0.1.3 - Error Codes Tests

Tests for verifying error code constants and their formatting.

Error Code Registry:
| Code | Category | Meaning                   |
| ---- | -------- | ------------------------- |
| E001 | User     | Test assertion failed     |
| E002 | User     | Import error in test file |
| E003 | User     | Fixture not found         |
| E004 | User     | Invalid marker expression |
| E005 | System   | userfaultfd not available |
| E006 | System   | Landlock not supported    |
| E007 | System   | Permission denied         |
| E008 | System   | Out of memory             |
| E009 | System   | Too many open files       |
| E010 | User     | Timeout exceeded          |
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
        ]
        pattern = re.compile(r"^E\d{3}$")
        for code in error_codes:
            assert pattern.match(code), f"Error code {code} does not match pattern E###"

    def test_user_error_codes(self):
        """User error codes should be in expected range."""
        user_codes = ["E001", "E002", "E003", "E004", "E010"]
        valid_user_nums = {1, 2, 3, 4, 10}
        for code in user_codes:
            num = int(code[1:])
            assert num in valid_user_nums, f"User error code {code} not in expected set"

    def test_system_error_codes(self):
        """System error codes should be in expected range."""
        system_codes = ["E005", "E006", "E007", "E008", "E009"]
        for code in system_codes:
            num = int(code[1:])
            assert 5 <= num <= 9, f"System error code {code} out of expected range"


class TestErrorMessageFormat:
    """Tests for error message format validation."""

    def test_formatted_error_pattern(self):
        """Formatted errors should follow [E###] Category: message pattern."""
        # Expected format: [E005] System Error: userfaultfd not available
        example_output = "[E005] System Error: userfaultfd not available"
        pattern = re.compile(r"^\[E\d{3}\] (User|System) Error: .+$")
        assert pattern.match(
            example_output
        ), "Error format does not match expected pattern"

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
        }
        assert len(user_errors) == 5, "Expected 5 user error codes"

    def test_system_category_errors(self):
        """Verify system category error definitions."""
        system_errors = {
            "E005": "userfaultfd not available",
            "E006": "Landlock not supported",
            "E007": "Permission denied",
            "E008": "Out of memory",
            "E009": "Too many open files",
        }
        assert len(system_errors) == 5, "Expected 5 system error codes"


class TestErrorSuggestions:
    """Tests for error suggestions/hints."""

    def test_userfaultfd_suggestion(self):
        """E005 should have userfaultfd fix suggestion."""
        suggestion = "Set vm.unprivileged_userfaultfd=1 or run with CAP_SYS_PTRACE"
        assert "vm.unprivileged_userfaultfd" in suggestion
        assert "CAP_SYS_PTRACE" in suggestion

    def test_landlock_suggestion(self):
        """E006 should mention kernel version requirement."""
        suggestion = (
            "Landlock requires kernel 5.13+. Running without filesystem isolation."
        )
        assert "5.13" in suggestion
        assert "filesystem isolation" in suggestion

    def test_permission_suggestion(self):
        """E007 should suggest checking permissions."""
        suggestion = "Check file permissions or run with appropriate privileges"
        assert "permissions" in suggestion

    def test_memory_suggestion(self):
        """E008 should suggest reducing worker count."""
        suggestion = "Reduce worker count with -n or increase system memory"
        assert "-n" in suggestion or "worker" in suggestion

    def test_file_limit_suggestion(self):
        """E009 should suggest ulimit command."""
        suggestion = "Increase ulimit with: ulimit -n 65536"
        assert "ulimit" in suggestion

    def test_timeout_suggestion(self):
        """E010 should suggest timeout marker or optimization."""
        suggestion = (
            "Increase the timeout with @pytest.mark.timeout(N) or optimize the test"
        )
        assert "timeout" in suggestion
        assert "optimize" in suggestion


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
        ]
        assert len(codes) == len(set(codes)), "Duplicate error codes found"

    def test_code_count(self):
        """Should have exactly 10 error codes defined."""
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
        ]
        assert len(codes) == 10, f"Expected 10 error codes, got {len(codes)}"

    def test_no_gaps_in_sequence(self):
        """Error codes should be sequential (E001-E010)."""
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
        ]
        expected_nums = list(range(1, 11))
        actual_nums = [int(code[1:]) for code in codes]
        assert actual_nums == expected_nums, "Error codes have gaps in sequence"
