//! Process isolation modules
//!
//! This module handles test isolation:
//! - `namespace`: Linux namespace setup and OverlayFS
//! - `sandbox`: Landlock and Seccomp sandboxing (Iron Dome)
//! - `snapshot`: userfaultfd memory snapshots

pub mod namespace;
pub mod sandbox;
pub mod snapshot;

// Re-export main functions from namespace for backward compatibility
pub use namespace::setup_filesystem;
