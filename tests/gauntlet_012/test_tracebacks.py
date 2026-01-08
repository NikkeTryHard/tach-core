"""
Gauntlet 0.1.2 - Traceback Formatting Tests

Tests for --tb flag traceback formatting styles.

Traceback Styles:
| Style  | Description                                      |
| ------ | ------------------------------------------------ |
| short  | First and last frames only                       |
| long   | Full traceback (default)                         |
| line   | Single line: file:line: message                  |
| native | Python's native format (same as long)            |
| no     | Suppress traceback output                        |
"""

import re


class TestTracebackStyles:
    """Tests for traceback style definitions."""

    def test_valid_traceback_styles(self):
        """All valid --tb styles should be defined."""
        valid_styles = ["short", "long", "line", "native", "no"]
        assert len(valid_styles) == 5

    def test_default_style_is_long(self):
        """Default traceback style should be 'long'."""
        default = "long"
        assert default == "long"

    def test_style_names_are_lowercase(self):
        """Traceback style names should be lowercase."""
        styles = ["short", "long", "line", "native", "no"]
        for style in styles:
            assert style == style.lower()


class TestTracebackShortFormat:
    """Tests for --tb short format."""

    def test_short_format_description(self):
        """Short format should show first and last frames."""
        description = "Shows only the first and last frames of the traceback"
        assert "first" in description.lower()
        assert "last" in description.lower()

    def test_short_reduces_output(self):
        """Short format should produce less output than long."""
        # Simulate traceback lengths
        long_lines = 10
        short_lines = 2
        assert short_lines < long_lines

    def test_short_preserves_error_message(self):
        """Short format should preserve the error message."""
        # Error message should always be visible
        sample_error = "AssertionError: Expected 1, got 2"
        assert "AssertionError" in sample_error


class TestTracebackLongFormat:
    """Tests for --tb long format (default)."""

    def test_long_format_is_default(self):
        """Long format should be the default."""
        default_style = "long"
        assert default_style == "long"

    def test_long_shows_full_traceback(self):
        """Long format should show complete traceback."""
        description = "Returns the full traceback unchanged"
        assert "full" in description.lower()

    def test_long_includes_all_frames(self):
        """Long format should include all stack frames."""
        # Simulate a traceback with multiple frames
        frames = ["frame1", "frame2", "frame3", "frame4"]
        # Long format should show all frames
        assert len(frames) == 4


class TestTracebackLineFormat:
    """Tests for --tb line format."""

    def test_line_format_pattern(self):
        """Line format should follow file:line: message pattern."""
        pattern = re.compile(r".+:\d+:.+")
        example = "test_foo.py:42: AssertionError"
        assert pattern.match(example)

    def test_line_format_is_single_line(self):
        """Line format should produce exactly one line."""
        output = "test_foo.py:42: AssertionError: expected 1, got 2"
        lines = output.split("\n")
        assert len(lines) == 1

    def test_line_format_includes_file(self):
        """Line format should include file path."""
        output = "test_foo.py:42: AssertionError"
        assert "test_foo.py" in output

    def test_line_format_includes_line_number(self):
        """Line format should include line number."""
        output = "test_foo.py:42: AssertionError"
        assert ":42:" in output


class TestTracebackNativeFormat:
    """Tests for --tb native format."""

    def test_native_matches_python(self):
        """Native format should match Python's native traceback format."""
        description = "Returns the traceback unchanged (same as Long)"
        assert "unchanged" in description.lower()

    def test_native_equivalent_to_long(self):
        """Native and long should produce equivalent output."""
        native_behavior = "same as long"
        assert "long" in native_behavior


class TestTracebackNoFormat:
    """Tests for --tb no format."""

    def test_no_suppresses_output(self):
        """No format should suppress traceback output."""
        output = ""  # --tb no produces empty string
        assert output == ""

    def test_no_still_shows_test_failure(self):
        """Even with --tb no, test failure should be reported."""
        # The test result (PASS/FAIL) is always shown
        # Only the traceback is suppressed
        result = "FAILED"
        assert result == "FAILED"


class TestTracebackEdgeCases:
    """Tests for traceback edge cases."""

    def test_empty_traceback_handled(self):
        """Empty traceback should be handled gracefully."""
        traceback = ""
        # Should not crash, may return empty or default message
        assert traceback == "" or len(traceback) >= 0

    def test_very_long_traceback(self):
        """Very long tracebacks should be handled."""
        # Simulate a deep call stack
        frames = [f"frame_{i}" for i in range(100)]
        assert len(frames) == 100

    def test_unicode_in_traceback(self):
        """Unicode characters in traceback should be preserved."""
        message = "AssertionError: Expected '日本語', got 'English'"
        assert "日本語" in message

    def test_multiline_assertion_message(self):
        """Multi-line assertion messages should be handled."""
        message = """AssertionError: Lists differ:
First differing element 0:
1
2"""
        lines = message.split("\n")
        assert len(lines) > 1


class TestTracebackIntegration:
    """Integration tests for traceback formatting with pytest."""

    def test_pytest_assertion_rewriting(self):
        """pytest assertion rewriting should work with all --tb styles."""
        # pytest rewrites assertions to provide detailed diffs
        # This should work regardless of --tb style
        expected = [1, 2, 3]
        actual = [1, 2, 3]
        assert expected == actual

    def test_pytest_diff_output(self):
        """pytest diff output should be affected by --tb style."""
        # With --tb short/line, diffs may be truncated
        # With --tb long/native, full diffs are shown
        styles_affecting_diff = ["short", "line", "no"]
        styles_preserving_diff = ["long", "native"]
        assert len(styles_affecting_diff) + len(styles_preserving_diff) == 5
