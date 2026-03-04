use std::path::PathBuf;
use std::process::Command;

fn tach_binary() -> PathBuf {
    let mut path = std::env::current_exe().unwrap();
    path.pop();
    path.pop();
    path.push("tach-core");
    path
}

fn project_root() -> PathBuf {
    let mut path = std::env::current_exe().unwrap();
    path.pop();
    path.pop();
    path.pop();
    path.pop();
    path
}

fn pytest_available() -> bool {
    Command::new("python3")
        .args(["-c", "import pytest"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[test]
fn test_no_fallback_flag_reports_raw_failures() {
    if !pytest_available() {
        return;
    }
    let dir = project_root().join("tests/regression/pytest_compat/sample_tests");
    if !dir.exists() {
        return;
    }
    let output = Command::new(tach_binary())
        .args(["--no-isolation", "--no-fallback", "-n", "1"])
        .arg(&dir)
        .current_dir(project_root())
        .output()
        .expect("failed to run tach");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("[tach:fallback]"),
        "--no-fallback should suppress fallback output"
    );
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn test_exit_code_5_no_tests() {
    let output = Command::new(tach_binary())
        .args(["--no-isolation", "--no-fallback", "-n", "1"])
        .arg(project_root().join("docs"))
        .current_dir(project_root())
        .output()
        .expect("failed to run tach");

    assert_eq!(
        output.status.code(),
        Some(5),
        "No tests collected should return exit code 5 (pytest compat)"
    );
}
