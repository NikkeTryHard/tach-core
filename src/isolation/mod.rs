//! Process isolation modules
//!
//! This module handles test isolation:
//! - `namespace`: Linux namespace setup
//! - `sandbox`: Landlock and Seccomp sandboxing
//! - `snapshot`: userfaultfd memory snapshots

pub mod namespace;
pub mod sandbox;
pub mod snapshot;
