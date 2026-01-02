//! Toxicity analysis submodules
//!
//! This module analyzes Python code for patterns that require
//! process isolation (toxic tests):
//! - `blocklists`: Known toxic module patterns
//! - `visitor`: AST visitor for toxicity detection

pub mod blocklists;
pub mod visitor;

// Re-export main types
pub use super::analysis::*;
