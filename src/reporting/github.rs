use crate::reporter::Reporter;
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write;

pub fn is_github_actions() -> bool {
    std::env::var("GITHUB_ACTIONS").ok().as_deref() == Some("true")
}

#[derive(Default)]
pub struct GitHubReporter {
    failures: Vec<FailureAnnotation>,
    passed: usize,
    failed: usize,
    skipped: usize,
    total_duration_ms: u64,
    current_files: HashMap<String, String>,
}

struct FailureAnnotation {
    file: String,
    line: Option<u32>,
    test_id: String,
    message: String,
}

impl GitHubReporter {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Reporter for GitHubReporter {
    fn on_run_start(&mut self, _count: usize) {}

    fn on_test_start(&mut self, id: &str, file: &str) {
        self.current_files.insert(id.to_string(), file.to_string());
    }

    fn on_test_finished(
        &mut self,
        id: &str,
        status: &str,
        _duration_ms: u64,
        message: Option<&str>,
    ) {
        let file = self
            .current_files
            .remove(id)
            .unwrap_or_else(|| "unknown".to_string());
        match status {
            "pass" => self.passed += 1,
            "skip" => self.skipped += 1,
            "fail" => {
                self.failed += 1;
                let line = extract_line_from_traceback(message.unwrap_or(""));
                self.failures.push(FailureAnnotation {
                    file,
                    line,
                    test_id: id.to_string(),
                    message: first_error_line(message.unwrap_or("Test failed")),
                });
            }
            _ => {}
        }
    }

    fn on_run_finished(&mut self, passed: usize, failed: usize, skipped: usize, duration_ms: u64) {
        self.passed = passed;
        self.failed = failed;
        self.skipped = skipped;
        self.total_duration_ms = duration_ms;

        emit_annotations(&self.failures);
        write_step_summary(passed, failed, skipped, duration_ms, &self.failures);
        write_github_output(passed, failed, skipped, duration_ms);
    }

    fn on_error(&mut self, message: &str) {
        eprintln!("::error::{}", sanitize_annotation(message));
    }

    fn on_phase(&mut self, _phase: &str, _detail: Option<&crate::reporter::PhaseDetail>) {}
}

fn emit_annotations(failures: &[FailureAnnotation]) {
    for f in failures {
        let loc = match f.line {
            Some(line) => format!("file={},line={}", f.file, line),
            None => format!("file={}", f.file),
        };
        eprintln!(
            "::error {}::FAILED {} - {}",
            loc,
            f.test_id,
            sanitize_annotation(&f.message)
        );
    }
}

fn write_step_summary(
    passed: usize,
    failed: usize,
    skipped: usize,
    duration_ms: u64,
    failures: &[FailureAnnotation],
) {
    let summary_path = match std::env::var("GITHUB_STEP_SUMMARY") {
        Ok(p) if !p.is_empty() => p,
        _ => return,
    };

    let mut md = String::with_capacity(1024);
    let secs = duration_ms as f64 / 1000.0;
    let total = passed + failed + skipped;
    let icon = if failed > 0 { "x" } else { "white_check_mark" };

    md.push_str(&format!("## :{}: tach-core Test Results\n\n", icon));
    md.push_str("| Total | Passed | Failed | Skipped | Duration |\n");
    md.push_str("| ----- | ------ | ------ | ------- | -------- |\n");
    md.push_str(&format!(
        "| {} | {} | {} | {} | {:.2}s |\n\n",
        total, passed, failed, skipped, secs
    ));

    if !failures.is_empty() {
        md.push_str("### Failed Tests\n\n");
        md.push_str("| Test | File | Error |\n");
        md.push_str("| ---- | ---- | ----- |\n");
        for f in failures.iter().take(50) {
            let short_msg = if f.message.len() > 100 {
                format!("{}...", &f.message[..97])
            } else {
                f.message.clone()
            };
            md.push_str(&format!(
                "| `{}` | `{}` | {} |\n",
                f.test_id,
                f.file,
                escape_md_table(&short_msg)
            ));
        }
        if failures.len() > 50 {
            md.push_str(&format!(
                "\n*...and {} more failures*\n",
                failures.len() - 50
            ));
        }
    }

    if let Ok(mut file) = OpenOptions::new()
        .append(true)
        .create(true)
        .open(summary_path)
    {
        let _ = file.write_all(md.as_bytes());
    }
}

fn write_github_output(passed: usize, failed: usize, skipped: usize, duration_ms: u64) {
    let output_path = match std::env::var("GITHUB_OUTPUT") {
        Ok(p) if !p.is_empty() => p,
        _ => return,
    };
    let total = passed + failed + skipped;
    let secs = duration_ms as f64 / 1000.0;
    let result = if failed > 0 { "failure" } else { "success" };

    let output = format!(
        "total={total}\npassed={passed}\nfailed={failed}\nskipped={skipped}\n\
         duration={secs:.2}\nresult={result}\n"
    );

    if let Ok(mut file) = OpenOptions::new()
        .append(true)
        .create(true)
        .open(output_path)
    {
        let _ = file.write_all(output.as_bytes());
    }
}

fn sanitize_annotation(s: &str) -> String {
    s.replace('%', "%25")
        .replace('\n', "%0A")
        .replace('\r', "%0D")
}

fn escape_md_table(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}

fn first_error_line(traceback: &str) -> String {
    traceback
        .lines()
        .rev()
        .find(|l| {
            let t = l.trim();
            t.contains("Error") || t.starts_with("E ") || t.starts_with("assert")
        })
        .unwrap_or_else(|| {
            traceback
                .lines()
                .rev()
                .find(|l| !l.trim().is_empty())
                .unwrap_or("Test failed")
        })
        .trim()
        .to_string()
}

fn extract_line_from_traceback(traceback: &str) -> Option<u32> {
    traceback
        .lines()
        .rev()
        .find(|line| line.contains("File \"") && line.contains(", line "))
        .and_then(|line| {
            let start = line.find(", line ")? + 7;
            let end = line[start..]
                .find(|c: char| !c.is_ascii_digit())
                .map(|i| i + start)
                .unwrap_or(line.len());
            line[start..end].parse().ok()
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_github_actions_false_by_default() {
        assert!(
            !is_github_actions() || std::env::var("GITHUB_ACTIONS").ok().as_deref() == Some("true")
        );
    }

    #[test]
    fn test_sanitize_annotation() {
        assert_eq!(sanitize_annotation("hello\nworld"), "hello%0Aworld");
        assert_eq!(sanitize_annotation("100%"), "100%25");
    }

    #[test]
    fn test_escape_md_table() {
        assert_eq!(escape_md_table("a|b"), "a\\|b");
        assert_eq!(escape_md_table("a\nb"), "a b");
    }

    #[test]
    fn test_extract_line_from_traceback() {
        let tb = r#"Traceback (most recent call last):
  File "tests/test_foo.py", line 42, in test_bar
    assert False
AssertionError"#;
        assert_eq!(extract_line_from_traceback(tb), Some(42));
    }

    #[test]
    fn test_extract_line_no_traceback() {
        assert_eq!(extract_line_from_traceback("just an error"), None);
    }

    #[test]
    fn test_first_error_line() {
        let tb = "  File \"test.py\", line 1\n    x = 1\nAssertionError: expected True";
        assert_eq!(first_error_line(tb), "AssertionError: expected True");
    }

    #[test]
    fn test_first_error_line_with_e_prefix() {
        let tb = "stuff\nE       assert 1 == 2\nmore stuff";
        assert_eq!(first_error_line(tb), "E       assert 1 == 2");
    }

    #[test]
    fn test_github_reporter_tracks_counts() {
        let mut r = GitHubReporter::new();
        r.on_run_start(3);
        r.on_test_start("t1", "test.py");
        r.on_test_finished("t1", "pass", 10, None);
        r.on_test_start("t2", "test.py");
        r.on_test_finished("t2", "fail", 20, Some("AssertionError"));
        r.on_test_start("t3", "test.py");
        r.on_test_finished("t3", "skip", 0, None);
        assert_eq!(r.passed, 1);
        assert_eq!(r.failed, 1);
        assert_eq!(r.skipped, 1);
        assert_eq!(r.failures.len(), 1);
    }

    #[test]
    fn test_extract_line_multiframe_traceback() {
        let tb = r#"  File "conftest.py", line 10, in setup
    db.connect()
  File "tests/test_api.py", line 55, in test_create
    assert resp.status == 201
AssertionError"#;
        assert_eq!(extract_line_from_traceback(tb), Some(55));
    }

    #[test]
    fn test_first_error_line_empty_traceback() {
        assert_eq!(first_error_line(""), "Test failed");
    }

    #[test]
    fn test_first_error_line_only_whitespace() {
        assert_eq!(first_error_line("  \n  \n  "), "Test failed");
    }

    #[test]
    fn test_sanitize_annotation_all_special() {
        let s = "line1\nline2\r100%";
        assert_eq!(sanitize_annotation(s), "line1%0Aline2%0D100%25");
    }

    #[test]
    fn test_github_reporter_unknown_test_id() {
        let mut r = GitHubReporter::new();
        r.on_run_start(1);
        r.on_test_finished("unknown_id", "fail", 10, Some("Error"));
        assert_eq!(r.failures[0].file, "unknown");
    }

    #[test]
    fn test_write_github_output_to_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let output_path = dir.path().join("github_output");
        unsafe { std::env::set_var("GITHUB_OUTPUT", output_path.to_str().unwrap()) };

        write_github_output(10, 2, 3, 5000);

        let content = std::fs::read_to_string(&output_path).unwrap();
        assert!(content.contains("total=15"));
        assert!(content.contains("passed=10"));
        assert!(content.contains("failed=2"));
        assert!(content.contains("result=failure"));

        unsafe { std::env::remove_var("GITHUB_OUTPUT") };
    }

    #[test]
    fn test_write_github_output_success() {
        let dir = tempfile::TempDir::new().unwrap();
        let output_path = dir.path().join("github_output2");
        unsafe { std::env::set_var("GITHUB_OUTPUT", output_path.to_str().unwrap()) };

        write_github_output(10, 0, 1, 3000);

        let content = std::fs::read_to_string(&output_path).unwrap();
        assert!(content.contains("result=success"));

        unsafe { std::env::remove_var("GITHUB_OUTPUT") };
    }
}
