//! Test result reporting modules
//!
//! This module handles test output and reporting:
//! - `reporter`: Base reporter trait and implementations
//! - `junit`: JUnit XML output
//! - `logcapture`: stdout/stderr capture
//! - `logredirect`: Redirect diagnostic stderr logs to a file
//! - `debugger`: Interactive debugging support (TTY proxy)
//! - `coverage`: Code coverage collection

pub mod coverage;
pub mod debugger;
pub mod github;
pub mod junit;
pub mod logcapture;
pub mod logredirect;
pub mod ratatui_reporter;
pub mod reporter;
