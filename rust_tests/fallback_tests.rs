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

#[test]
fn test_node_id_path_targeting() {
    if !pytest_available() {
        return;
    }
    let dir = project_root().join("tests/gauntlet_phase1");
    if !dir.exists() {
        return;
    }
    let test_files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("test_"))
        .collect();
    if test_files.is_empty() {
        return;
    }
    let first_file = test_files[0].file_name();
    let target = format!("tests/gauntlet_phase1/{}", first_file.to_string_lossy());
    let output = Command::new(tach_binary())
        .args(["--no-isolation", "--no-fallback", "-n", "1"])
        .arg(&target)
        .current_dir(project_root())
        .output()
        .expect("failed to run tach");

    assert!(
        output.status.success() || output.status.code() == Some(1),
        "Node ID path should find tests, got exit code {:?}",
        output.status.code()
    );
}
