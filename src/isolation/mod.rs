//! Process isolation modules
//!
//! This module handles test isolation:
//! - `namespace`: Linux namespace setup and OverlayFS
//! - `sandbox`: Landlock and Seccomp sandboxing (Iron Dome)
//! - `snapshot`: userfaultfd memory snapshots
//! - `calibration`: TLS self-calibration for mimalloc offset discovery

pub mod calibration;
pub mod namespace;
pub mod sandbox;
pub mod snapshot;

// Re-export main functions from namespace for backward compatibility
pub use namespace::setup_filesystem;

// Re-export calibration for Zygote warm-up
pub use calibration::TlsCalibration;
