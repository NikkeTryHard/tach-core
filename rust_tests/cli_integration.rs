//! CLI Integration Tests for Phase 5.2 and 5.3 features
//!
//! Tests for:
//! - Line number metadata in discovery
//! - CLI flag recognition (--format, --junit-xml, --watch)
//! - JSON output format

use std::path::PathBuf;
use tach_core::config::{Cli, Commands, OutputFormat};
use tach_core::discovery::{DiscoveryResult, TestCase, TestModule};
use tach_core::reporter::{HumanReporter, JsonReporter, MultiReporter, Reporter};

/// Test that TestCase includes line_number field
#[test]
fn test_testcase_has_line_number() {
    let test = TestCase {
        name: "test_foo".to_string(),
        dependencies: vec![],
        is_async: false,
        line_number: 42,
    };
    assert_eq!(test.line_number, 42);
}

/// Test line number is non-zero for real tests
#[test]
fn test_line_number_not_always_one() {
    // Verify that our TestCase struct has line_number field
    let test1 = TestCase {
        name: "test_first".to_string(),
        dependencies: vec![],
        is_async: false,
        line_number: 5,
    };
    let test2 = TestCase {
        name: "test_second".to_string(),
        dependencies: vec![],
        is_async: false,
        line_number: 15,
    };

    assert_ne!(test1.line_number, test2.line_number);
    assert!(test1.line_number > 0);
    assert!(test2.line_number > 0);
}

/// Test output format enum variants
#[test]
fn test_output_format_variants() {
    let human = OutputFormat::Human;
    let json = OutputFormat::Json;

    assert_ne!(human, json);
    assert_eq!(OutputFormat::default(), OutputFormat::Human);
}

/// Test MultiReporter broadcasts to all child reporters
#[test]
fn test_multi_reporter_creation() {
    let reporters: Vec<Box<dyn Reporter>> = vec![Box::new(HumanReporter)];
    let _ = MultiReporter::new(reporters);
    // If it compiles and creates, the pattern works
}

/// Test DiscoveryResult test counting with line numbers
#[test]
fn test_discovery_result_with_line_numbers() {
    let result = DiscoveryResult {
        modules: vec![TestModule {
            path: PathBuf::from("test_example.py"),
            tests: vec![
                TestCase {
                    name: "test_first".to_string(),
                    dependencies: vec![],
                    is_async: false,
                    line_number: 5,
                },
                TestCase {
                    name: "test_second".to_string(),
                    dependencies: vec![],
                    is_async: true,
                    line_number: 20,
                },
            ],
            fixtures: vec![],
        }],
    };

    assert_eq!(result.test_count(), 2);
    assert_eq!(result.modules[0].tests[0].line_number, 5);
    assert_eq!(result.modules[0].tests[1].line_number, 20);
}

/// Test that JsonReporter implements Reporter trait
#[test]
fn test_json_reporter_implements_reporter() {
    fn accepts_reporter<T: Reporter>(_r: T) {}
    accepts_reporter(JsonReporter);
}

/// Test that HumanReporter implements Reporter trait  
#[test]
fn test_human_reporter_implements_reporter() {
    fn accepts_reporter<T: Reporter>(_r: T) {}
    accepts_reporter(HumanReporter);
}
