use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::time::Instant;

use crate::cache::read_lastfailed_cache_from;
use crate::scheduler::SchedulerStats;

pub fn pytest_fallback_retry(
    stats: &SchedulerStats,
    cwd: &Path,
    is_json: bool,
    target_path: &str,
) -> usize {
    let failed_ids = &stats.failed_test_ids;
    if failed_ids.is_empty() {
        return 0;
    }

    let fallback_start = Instant::now();
    if !is_json {
        eprintln!(
            "\n[tach:fallback] Retrying {} failed test(s) with pytest...",
            failed_ids.len()
        );
    }

    // Write a Python script that runs pytest with exact test name filtering.
    // This avoids -k's substring matching (test_foo matches test_foobar)
    // and OS arg length limits on large failure sets.
    let cache_dir = cwd.join(".tach_cache");
    if let Err(e) = std::fs::create_dir_all(&cache_dir)
        && !is_json
    {
        eprintln!("[tach:fallback] Cannot create cache dir: {}, using /tmp", e);
    }
    let fallback_dir = if cache_dir.exists() {
        &cache_dir
    } else {
        Path::new("/tmp")
    };
    let retry_file = fallback_dir.join("_fallback_retry.txt");
    let runner_file = fallback_dir.join("_fallback_runner.py");
    {
        let mut f = match std::fs::File::create(&retry_file) {
            Ok(f) => f,
            Err(e) => {
                if !is_json {
                    eprintln!("[tach:fallback] Failed to create retry file: {}", e);
                }
                return stats.failed;
            }
        };
        for id in failed_ids {
            let _ = writeln!(f, "{}", id);
        }
    }

    let results_file = fallback_dir.join("_fallback_results.txt");
    let runner_code = format!(
        r#"import sys, pathlib, pytest
_IDS = set(pathlib.Path({retry_path:?}).read_text().splitlines())
_RESULTS = pathlib.Path({results_path:?})
class _TachFilter:
    def pytest_collection_modifyitems(self, items):
        items[:] = [i for i in items if _suffix(i.nodeid) in _IDS]
    def pytest_runtest_logreport(self, report):
        if report.when == "call" and report.failed:
            with open(_RESULTS, "a") as f:
                f.write(_suffix(report.nodeid) + "\n")
def _suffix(nodeid):
    parts = nodeid.split("::")
    return "::".join(parts[1:]) if len(parts) > 1 else nodeid
_RESULTS.unlink(missing_ok=True)
sys.exit(pytest.main(["--tb=no", "-q", "--no-header",
    "--continue-on-collection-errors", {target:?}], plugins=[_TachFilter()]))
"#,
        retry_path = retry_file.display(),
        results_path = results_file.display(),
        target = if target_path.contains("::") {
            target_path.split("::").next().unwrap_or(".")
        } else {
            target_path
        },
    );
    if let Err(e) = std::fs::write(&runner_file, &runner_code) {
        if !is_json {
            eprintln!("[tach:fallback] Failed to write runner: {}", e);
        }
        return stats.failed;
    }

    let output = Command::new("python3")
        .arg(&runner_file)
        .current_dir(cwd)
        .output();

    let _ = std::fs::remove_file(&retry_file);
    let _ = std::fs::remove_file(&runner_file);

    match output {
        Ok(result) => {
            if result.status.code() == Some(127) {
                if !is_json {
                    eprintln!("[tach:fallback] pytest not found, reporting raw tach results");
                }
                return stats.failed;
            }

            let stdout = String::from_utf8_lossy(&result.stdout);
            let pytest_failed = parse_pytest_summary_failed(&stdout);

            let real_failure_ids = read_lastfailed_cache_from(&results_file);
            let real_failures = if real_failure_ids.is_empty() {
                pytest_failed
            } else {
                real_failure_ids.len()
            };
            let tach_specific = failed_ids.len().saturating_sub(real_failures);

            if !is_json {
                if tach_specific > 0 {
                    eprintln!(
                        "[tach:fallback] {} test(s) passed in pytest (tach-specific failures)",
                        tach_specific
                    );
                }
                if real_failures > 0 {
                    eprintln!(
                        "[tach:fallback] {} test(s) failed in both tach and pytest (real failures)",
                        real_failures
                    );
                }
                let elapsed = fallback_start.elapsed();
                let total_effective_pass = stats.passed + tach_specific;
                eprintln!(
                    "[tach:fallback] Fallback completed in {:.1}s",
                    elapsed.as_secs_f64()
                );
                eprintln!(
                    "[tach:fallback] Final: {} passed, {} failed, {} skipped",
                    total_effective_pass, real_failures, stats.skipped
                );
            }

            real_failures
        }
        Err(e) => {
            if !is_json {
                eprintln!("[tach:fallback] Failed to run pytest: {}", e);
            }
            stats.failed
        }
    }
}

pub fn parse_pytest_summary_failed(output: &str) -> usize {
    for line in output.lines().rev() {
        let line = line.trim().trim_matches('=').trim();
        if line.contains(" in ") && (line.contains("passed") || line.contains("failed")) {
            for part in line.split(',') {
                let words: Vec<&str> = part.split_whitespace().collect();
                if words.len() >= 2
                    && words[1].starts_with("failed")
                    && let Ok(n) = words[0].parse::<usize>()
                {
                    return n;
                }
            }
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_pytest_summary_all_passed() {
        let output = "======= 42 passed in 1.23s =======";
        assert_eq!(parse_pytest_summary_failed(output), 0);
    }

    #[test]
    fn test_parse_pytest_summary_mixed() {
        let output = "======= 3 failed, 10 passed in 2.5s =======";
        assert_eq!(parse_pytest_summary_failed(output), 3);
    }

    #[test]
    fn test_parse_pytest_summary_with_errors() {
        let output = "======= 1 failed, 2 error, 5 passed in 0.8s =======";
        assert_eq!(parse_pytest_summary_failed(output), 1);
    }

    #[test]
    fn test_parse_pytest_summary_only_failed() {
        let output = "======= 7 failed in 3.1s =======";
        assert_eq!(parse_pytest_summary_failed(output), 7);
    }

    #[test]
    fn test_parse_pytest_summary_empty() {
        assert_eq!(parse_pytest_summary_failed(""), 0);
    }

    #[test]
    fn test_parse_pytest_summary_no_summary_line() {
        let output = "collecting... 10 items\nPASSED test_foo.py\n";
        assert_eq!(parse_pytest_summary_failed(output), 0);
    }

    #[test]
    fn test_parse_pytest_summary_with_warnings() {
        let output = "======= 1 failed, 9 passed, 3 warnings in 1.5s =======";
        assert_eq!(parse_pytest_summary_failed(output), 1);
    }

    #[test]
    fn test_parse_pytest_summary_multiline() {
        let output = "FAILED test_a.py::test_1\nFAILED test_b.py::test_2\n\
                       ======= 2 failed, 8 passed, 1 skipped in 4.2s =======";
        assert_eq!(parse_pytest_summary_failed(output), 2);
    }
}
