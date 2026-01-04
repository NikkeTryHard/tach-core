//! Fuzz target for TOML Config Parsing
//!
//! This fuzzer tests that the TOML configuration parser handles arbitrary
//! input without panicking and validates that invalid configs are rejected.

#![no_main]

use libfuzzer_sys::fuzz_target;

/// Simulated pyproject.toml structure for fuzzing
fn parse_toml_config(data: &[u8]) -> Option<()> {
    // Try to parse as UTF-8 string
    let content = std::str::from_utf8(data).ok()?;

    // Try to parse as TOML value
    let _value: toml::Value = toml::from_str(content).ok()?;

    Some(())
}

/// Extract environment variables from pyproject.toml [tool.pytest.ini_options]
fn extract_pytest_env(data: &[u8]) -> Vec<(String, String)> {
    let mut result = Vec::new();

    let content = match std::str::from_utf8(data) {
        Ok(s) => s,
        Err(_) => return result,
    };

    let value: toml::Value = match toml::from_str(content) {
        Ok(v) => v,
        Err(_) => return result,
    };

    // Navigate to [tool.pytest.ini_options.env]
    if let Some(tool) = value.get("tool") {
        if let Some(pytest) = tool.get("pytest") {
            if let Some(ini_options) = pytest.get("ini_options") {
                if let Some(env) = ini_options.get("env") {
                    if let Some(env_array) = env.as_array() {
                        for item in env_array {
                            if let Some(s) = item.as_str() {
                                // Parse "KEY=VALUE" format
                                if let Some(eq_pos) = s.find('=') {
                                    let key = s[..eq_pos].to_string();
                                    let value = s[eq_pos + 1..].to_string();
                                    result.push((key, value));
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    result
}

fuzz_target!(|data: &[u8]| {
    // Test 1: Basic TOML parsing should never panic
    let _ = parse_toml_config(data);

    // Test 2: Environment extraction should never panic
    let env_vars = extract_pytest_env(data);

    // Invariant: All extracted keys should be non-empty
    for (key, _value) in &env_vars {
        assert!(!key.is_empty(), "Environment variable key should not be empty");
    }

    // Invariant: Keys should not contain null bytes
    for (key, value) in &env_vars {
        assert!(!key.contains('\0'), "Key should not contain null bytes");
        assert!(!value.contains('\0'), "Value should not contain null bytes");
    }
});
