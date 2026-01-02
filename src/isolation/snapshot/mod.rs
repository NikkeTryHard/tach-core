//! Snapshot submodules
//!
//! This module implements userfaultfd-based memory snapshots:
//! - `scm_rights`: File descriptor passing via SCM_RIGHTS
//! - `maps`: Memory region parsing from /proc/self/maps
//! - `elf`: ELF segment detection for libpython

pub mod elf;
pub mod maps;
pub mod scm_rights;

// Re-export main types
pub use super::snapshot::*;
