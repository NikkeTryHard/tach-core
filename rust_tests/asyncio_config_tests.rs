use std::fs;
use tach_core::discovery::parse_asyncio_config;
use tempfile::TempDir;

#[test]
fn test_parse_asyncio_mode_auto_from_pyproject() {
    let temp = TempDir::new().unwrap();
    let pyproject = temp.path().join("pyproject.toml");

    fs::write(
        &pyproject,
        r#"
[tool.pytest.ini_options]
asyncio_mode = "auto"
"#,
    )
    .unwrap();

    let config = parse_asyncio_config(temp.path()).unwrap();
    assert_eq!(config.asyncio_mode, "auto");
    assert!(config.auto_mode);
}

#[test]
fn test_parse_asyncio_mode_strict_from_pyproject() {
    let temp = TempDir::new().unwrap();
    let pyproject = temp.path().join("pyproject.toml");

    fs::write(
        &pyproject,
        r#"
[tool.pytest.ini_options]
asyncio_mode = "strict"
"#,
    )
    .unwrap();

    let config = parse_asyncio_config(temp.path()).unwrap();
    assert_eq!(config.asyncio_mode, "strict");
    assert!(!config.auto_mode);
}

#[test]
fn test_parse_asyncio_mode_missing_defaults_to_strict() {
    let temp = TempDir::new().unwrap();
    let pyproject = temp.path().join("pyproject.toml");

    fs::write(
        &pyproject,
        r#"
[tool.pytest.ini_options]
addopts = "-v"
"#,
    )
    .unwrap();

    let config = parse_asyncio_config(temp.path()).unwrap();
    assert_eq!(config.asyncio_mode, "strict");
    assert!(!config.auto_mode);
}

#[test]
fn test_parse_asyncio_mode_no_pyproject_defaults() {
    let temp = TempDir::new().unwrap();
    // No pyproject.toml created

    let config = parse_asyncio_config(temp.path()).unwrap();
    assert_eq!(config.asyncio_mode, "strict");
    assert!(!config.auto_mode);
    assert_eq!(config.loop_scope, "function");
}

#[test]
fn test_parse_asyncio_loop_scope() {
    let temp = TempDir::new().unwrap();
    let pyproject = temp.path().join("pyproject.toml");

    fs::write(
        &pyproject,
        r#"
[tool.pytest.ini_options]
asyncio_mode = "auto"
asyncio_default_fixture_loop_scope = "session"
"#,
    )
    .unwrap();

    let config = parse_asyncio_config(temp.path()).unwrap();
    assert_eq!(config.asyncio_mode, "auto");
    assert!(config.auto_mode);
    assert_eq!(config.loop_scope, "session");
}

#[test]
fn test_parse_asyncio_no_tool_section() {
    let temp = TempDir::new().unwrap();
    let pyproject = temp.path().join("pyproject.toml");

    fs::write(
        &pyproject,
        r#"
[project]
name = "my-project"
version = "0.1.0"
"#,
    )
    .unwrap();

    let config = parse_asyncio_config(temp.path()).unwrap();
    assert_eq!(config.asyncio_mode, "strict");
    assert!(!config.auto_mode);
}
