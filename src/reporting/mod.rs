//! Test result reporting modules
//!
//! This module handles test output and reporting:
//! - `reporter`: Base reporter trait and implementations
//! - `junit`: JUnit XML output
//! - `logcapture`: stdout/stderr capture
//! - `debugger`: Interactive debugging support
//! - `coverage`: Code coverage collection

pub mod coverage;
pub mod debugger;
pub mod junit;
pub mod logcapture;
pub mod reporter;
